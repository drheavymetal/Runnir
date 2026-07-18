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
    if norm.contains(":> /dev/sd") || norm.contains("> /dev/sd") {
        return Some("redirect into a raw disk device");
    }
    if norm.contains("chmod -r 777 /") || norm.contains("chmod 777 -r /") {
        return Some("recursive chmod 777 on a root path");
    }
    None
}

/// The last stage of a pipeline / command list, so leading `sudo`, earlier pipe
/// stages and a shell prompt do not mask (or fabricate) a hazard in the tail.
fn last_command(line: &str) -> String {
    line.rsplit(['|', ';'])
        .next()
        .unwrap_or(line)
        .trim()
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
    norm.starts_with("git ")
        && norm.contains("push")
        && (norm.contains("--force") || norm.contains("-f") || norm.contains("+"))
        && !norm.contains("--force-with-lease")
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
        assert!(danger("git push origin main").is_none());
        assert!(danger("git push --force-with-lease").is_none());
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
}
