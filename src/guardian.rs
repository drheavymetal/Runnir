//! Dangerous-command detection for the command guardian (W1).
//!
//! When the guardian is on, the terminal reads the command line about to run at the
//! moment Enter is pressed and checks it against a list of well-known destructive
//! patterns. A match does not block the command — it turns a reflex keystroke into a
//! deliberate confirmation, which is exactly where these mistakes happen.
//!
//! The scan is intentionally conservative: it looks for shapes that are almost never
//! typed on purpose in a hurry (recursive force-remove of an absolute path, writing a
//! raw device, reformatting, dropping a table, force-pushing, a fork bomb). False
//! positives are cheap (one keypress to confirm); a missed `rm -rf /` is not.

/// The reason a command line is considered dangerous, or `None` if it looks safe.
/// The string is shown to the user, so it names the specific hazard.
pub fn danger(line: &str) -> Option<&'static str> {
    // Work on the last logical command: a prompt prefix or earlier pipeline stages
    // do not change what the tail is about to do, and scanning the tail keeps the
    // prompt's own text (which may contain a path like ~/rm-backups) out of it.
    // The fork bomb is riddled with ';' and '|', so it must be checked against the
    // whole line before the pipeline split below chops it apart.
    if fork_bomb(line) {
        return Some("fork bomb — will spawn processes until the machine locks up");
    }

    let cmd = last_command(line);
    let lc = cmd.to_lowercase();
    let norm = normalize_ws(&lc);
    // Case matters for exactly one git rule: `-d` refuses to delete an unmerged
    // branch, `-D` does it anyway. Lowercasing would erase the difference between
    // the safe form and the destructive one, so that rule reads the original.
    let raw = normalize_ws(&cmd);
    if rm_rf_root(&norm) {
        return Some("recursive force-remove of a root/home path");
    }
    if dd_to_device(&norm) {
        return Some("dd writing to a raw device — can destroy a disk");
    }
    if norm.contains("mkfs") {
        return Some("mkfs — reformats a filesystem, erasing it");
    }
    if norm.contains("drop table") || norm.contains("drop database") {
        return Some("SQL DROP — deletes a table/database irreversibly");
    }
    if norm.contains("truncate table") {
        return Some("SQL TRUNCATE — empties a table irreversibly");
    }
    if git_force_push(&norm) {
        return Some("git force-push — can overwrite remote history");
    }
    if let Some(reason) = git_destroys_work(&norm, &raw) {
        return Some(reason);
    }
    if norm.contains(":> /dev/sd") || norm.contains("> /dev/sd") {
        return Some("redirect into a raw disk device");
    }
    if norm.contains("chmod -r 777 /") || norm.contains("chmod 777 -r /") {
        return Some("recursive chmod 777 on a root path");
    }
    None
}

/// The last stage of a command list / pipeline, so leading `sudo`, earlier stages
/// and a shell prompt do not mask (or fabricate) a hazard in the tail. Splits on
/// `;`, `|` and `&&` and takes the last NON-EMPTY segment, so `rm -rf /;` (trailing
/// separator) and `cd x && rm -rf /` (a dangerous tail after a harmless head) are
/// both caught.
fn last_command(line: &str) -> String {
    line.split("&&")
        .flat_map(|s| s.split([';', '|']))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("")
        .trim_start_matches("sudo ")
        .trim()
        .to_string()
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn rm_rf_root(norm: &str) -> bool {
    // Only when the command itself is rm — not when "rm" merely appears as an
    // argument (e.g. `echo rm -rf / is bad`).
    if !norm.starts_with("rm ") {
        return false;
    }
    // Flags may be combined (-rf, -fr) or split (-r -f); require both recursive
    // and force, then an absolute or home path as a target.
    let recursive = norm.contains("-rf")
        || norm.contains("-fr")
        || (norm.contains("-r") && norm.contains("-f"))
        || norm.contains("--recursive");
    if !recursive {
        return false;
    }
    // A dangerous target: /, /*, a top-level system dir, ~, $HOME.
    norm.split_whitespace().any(|tok| {
        matches!(tok, "/" | "/*" | "~" | "~/" | "~/*" | "$home" | "$home/" | "$home/*")
            || tok == "/*"
            || is_system_root_path(tok)
    })
}

fn is_system_root_path(tok: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/opt", "/proc", "/root",
        "/run", "/sbin", "/srv", "/sys", "/usr", "/var", "/home",
    ];
    ROOTS.iter().any(|r| tok == *r || tok == format!("{r}/*").as_str() || tok == format!("{r}/").as_str())
}

fn dd_to_device(norm: &str) -> bool {
    norm.starts_with("dd ") && norm.contains("of=/dev/")
}

fn git_force_push(norm: &str) -> bool {
    if !norm.starts_with("git ") || !norm.split_whitespace().any(|t| t == "push") {
        return false;
    }
    if norm.contains("--force-with-lease") {
        return false;
    }
    // Match --force / -f as whole tokens (not a substring of --follow-tags or a
    // branch like feature-fix), and a leading-'+' refspec (git push origin +main).
    // The refspec must carry a letter, so a numeric RPROMPT token like "+2" that the
    // full-row scan may pick up is not mistaken for a force-push refspec.
    norm.split_whitespace().any(|t| {
        t == "--force"
            || t == "-f"
            || (t.starts_with('+') && t[1..].chars().any(|c| c.is_alphabetic()))
    })
}

/// Git commands that destroy work which is not in any commit — the only kind git
/// cannot give back. A force-push is loud and famous; these are the quiet ones, and
/// they are what people actually lose an afternoon to.
///
/// `norm` is lowercased and whitespace-normalised, `raw` the same with the original
/// case (see the `-D` note in [`danger`]). Everything here is deliberately narrow:
/// `git clean -n` is a dry run, `git checkout main` is a branch switch that git
/// refuses when it would lose edits, `git reset` without `--hard` keeps the working
/// tree. None of those are flagged.
fn git_destroys_work(norm: &str, raw: &str) -> Option<&'static str> {
    if !norm.starts_with("git ") {
        return None;
    }
    let tok: Vec<&str> = norm.split_whitespace().collect();
    let has = |t: &str| tok.iter().any(|x| *x == t);
    let sub = |name: &str| tok.get(1) == Some(&name);

    if sub("reset") && (has("--hard") || has("--merge") || has("--keep")) {
        return Some("git reset --hard — throws away every uncommitted change");
    }
    // `clean` does nothing at all without -f/--force, so requiring it excludes the
    // dry run people use to check first. Flags combine: -fd, -xdf, -ffd.
    if sub("clean")
        && tok.iter().any(|t| *t == "--force" || (t.starts_with('-') && !t.starts_with("--") && t.contains('f')))
    {
        return Some("git clean — deletes untracked files outright; they are in no commit");
    }
    // A path checkout, not a branch switch: `git checkout -- .` or `git checkout .`.
    if sub("checkout") && (has("--") || has(".")) {
        return Some("git checkout of a path — discards your edits to it");
    }
    // `restore` writes the working tree unless it is only touching the index.
    if sub("restore") && (!has("--staged") || has("--worktree")) {
        return Some("git restore — overwrites your edits with the committed version");
    }
    if sub("stash") && (has("clear") || has("drop")) {
        return Some("git stash clear/drop — deletes stashed work, which no commit holds");
    }
    // Case-sensitive on purpose: -D force-deletes, -d refuses an unmerged branch.
    if sub("branch") && raw.split_whitespace().any(|t| t == "-D") {
        return Some("git branch -D — deletes a branch even if it was never merged");
    }
    if sub("push") && (has("--delete") || has("--mirror")) {
        return Some("git push --delete/--mirror — removes branches on the remote");
    }
    // The reflog is what makes a bad reset survivable. Dropping it removes the way
    // back from every other mistake on this list.
    if (sub("reflog") && has("expire") && tok.iter().any(|t| t.starts_with("--expire=") && !t.ends_with("=never")))
        || (sub("gc") && tok.iter().any(|t| *t == "--prune=now" || *t == "--prune=all"))
    {
        return Some("dropping the reflog — the last way back from a bad reset");
    }
    None
}

fn fork_bomb(cmd: &str) -> bool {
    let squished: String = cmd.chars().filter(|c| !c.is_whitespace()).collect();
    squished.contains(":(){:|:&};:") || squished.contains(":(){:|:&}:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_recursive_root_remove() {
        assert!(danger("rm -rf /").is_some());
        assert!(danger("sudo rm -rf /*").is_some());
        assert!(danger("rm -fr /usr").is_some());
        assert!(danger("rm -r -f ~").is_some());
        assert!(danger("rm -rf $HOME").is_some());
    }

    #[test]
    fn ignores_safe_removes() {
        assert!(danger("rm -rf ./build").is_none());
        assert!(danger("rm -rf target").is_none());
        assert!(danger("rm file.txt").is_none());
        assert!(danger("ls -la /").is_none());
    }

    #[test]
    fn flags_disk_and_fs_destroyers() {
        assert!(danger("dd if=x.iso of=/dev/sda").is_some());
        assert!(danger("mkfs.ext4 /dev/sdb1").is_some());
        assert!(danger("echo x > /dev/sda").is_some());
    }

    #[test]
    fn flags_sql_and_git_hazards() {
        assert!(danger("psql -c 'DROP TABLE users'").is_some());
        assert!(danger("git push --force origin main").is_some());
        assert!(danger("git push -f origin main").is_some());
        assert!(danger("git push origin +main").is_some());
        assert!(danger("git push origin main").is_none());
        assert!(danger("git push --force-with-lease").is_none());
        // Whole-token match: these must NOT false-positive.
        assert!(danger("git push --follow-tags").is_none());
        assert!(danger("git push -u origin feature-fix").is_none());
        // A numeric RPROMPT token the full-row scan may append is not a +refspec.
        assert!(danger("git push origin main +2").is_none());
    }

    #[test]
    fn flags_git_commands_that_destroy_uncommitted_work() {
        assert!(danger("git reset --hard").is_some());
        assert!(danger("git reset --hard HEAD~3").is_some());
        assert!(danger("git clean -fd").is_some());
        assert!(danger("git clean -xdf").is_some());
        assert!(danger("git clean --force").is_some());
        assert!(danger("git checkout -- .").is_some());
        assert!(danger("git checkout -- src/main.rs").is_some());
        assert!(danger("git restore src/grid.rs").is_some());
        assert!(danger("git stash clear").is_some());
        assert!(danger("git stash drop").is_some());
        assert!(danger("git branch -D feature").is_some());
        assert!(danger("git push --delete origin feature").is_some());
        assert!(danger("git push --mirror").is_some());
        assert!(danger("git reflog expire --expire=now --all").is_some());
        assert!(danger("git gc --prune=now").is_some());
    }

    #[test]
    fn leaves_the_safe_git_shapes_alone() {
        // The whole point of the guardian is that it stays out of the way.
        assert!(danger("git reset HEAD~1").is_none()); // keeps the working tree
        assert!(danger("git reset --soft HEAD~1").is_none());
        assert!(danger("git clean -n").is_none()); // dry run
        assert!(danger("git checkout main").is_none()); // branch switch; git refuses if it would lose edits
        assert!(danger("git checkout -b feature").is_none());
        assert!(danger("git restore --staged src/main.rs").is_none()); // unstages only
        assert!(danger("git stash").is_none());
        assert!(danger("git stash pop").is_none());
        assert!(danger("git branch -d merged").is_none()); // lowercase -d refuses unmerged
        assert!(danger("git branch --list").is_none());
        assert!(danger("git push origin main").is_none());
        assert!(danger("git gc").is_none());
        assert!(danger("git reflog").is_none());
        assert!(danger("git reflog expire --expire=never --all").is_none());
        assert!(danger("git log --oneline").is_none());
        assert!(danger("git status").is_none());
        assert!(danger("git commit -m 'clean up'").is_none());
    }

    #[test]
    fn the_branch_rule_reads_the_original_case() {
        // -D and -d differ only in case, and only one of them can lose a branch.
        // Lowercasing the line before this check would flag both.
        assert!(danger("git branch -D wip").is_some());
        assert!(danger("git branch -d wip").is_none());
    }

    #[test]
    fn flags_fork_bomb() {
        assert!(danger(":(){ :|:& };:").is_some());
    }

    #[test]
    fn scans_only_the_tail_command() {
        // A safe pipeline stage before a dangerous one is still caught.
        assert!(danger("cat list | sudo rm -rf /").is_some());
        // A prompt-like prefix does not create a false positive.
        assert!(danger("echo rm -rf / is dangerous").is_none());
    }

    #[test]
    fn catches_trailing_separator_and_and_chains() {
        // A trailing ';' must not leave an empty tail that skips every check.
        assert!(danger("rm -rf /;").is_some());
        // A dangerous tail after a harmless head (&&) is still caught.
        assert!(danger("cd /tmp && rm -rf /").is_some());
        assert!(danger("make && sudo rm -rf /usr").is_some());
        // Only the tail is judged: a dangerous head with a harmless tail is not
        // flagged (documented limitation — the tail is what runs last).
        assert!(danger("rm -rf / && echo done").is_none());
    }

    #[test]
    fn empty_line_is_safe() {
        assert!(danger("").is_none());
        assert!(danger("   ").is_none());
    }
}
