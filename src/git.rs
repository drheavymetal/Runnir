//! Repository state for the status bar: branch, dirt, and how far the branch has
//! drifted from its upstream.
//!
//! Two very different costs live here, and they are kept apart on purpose.
//!
//! [`head_branch`] reads `.git/HEAD` and nothing else. It is a single file read, so
//! the draw path can call it every frame, and it answers instantly in a repository
//! of any size.
//!
//! [`read_state`] shells out to git, which can take seconds in a large repository
//! and must never run on the UI thread. It is spawned onto a worker and reported
//! back through `UserEvent::Git` — the same pattern the AI, media and control
//! workers use.
//!
//! `--no-optional-locks` is not optional. A plain `git status` refreshes the index
//! and takes `index.lock` to do it, which means a background status poll can fail —
//! or make *the user's own git command in that very pane* fail — with "another git
//! process seems to be running". A status bar must never be able to do that.

use std::path::{Path, PathBuf};

/// What the status bar shows about the repository a pane is sitting in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoState {
    /// Branch name, or the short commit when the head is detached.
    pub branch: String,
    pub detached: bool,
    /// Files with worktree changes that are not staged, plus untracked files: what
    /// you would lose to a `git checkout -- .`.
    pub dirty: usize,
    /// Files with staged changes.
    pub staged: usize,
    /// Unmerged paths — a conflict in progress.
    pub conflicts: usize,
    /// Commits the branch is ahead of / behind its upstream. Both zero when there
    /// is no upstream, which [`RepoState::has_upstream`] distinguishes.
    pub ahead: usize,
    pub behind: usize,
    pub upstream: bool,
}

impl RepoState {
    pub fn has_upstream(&self) -> bool {
        self.upstream
    }

    /// Nothing to report beyond the branch itself: a clean tree, level with upstream.
    pub fn is_clean(&self) -> bool {
        self.dirty == 0 && self.staged == 0 && self.conflicts == 0 && self.ahead == 0 && self.behind == 0
    }
}

/// The repository root containing `dir`, by walking up looking for `.git`. `None`
/// outside a repository. Used to key the cache, so two panes in the same repo share
/// one entry and one git process instead of one each.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

/// The current branch for `dir`, read straight out of `.git/HEAD`. No git process is
/// spawned, so this is safe to call from the draw path. A detached head yields the
/// short commit id.
pub fn head_branch(dir: &Path) -> Option<String> {
    let root = repo_root(dir)?;
    let content = std::fs::read_to_string(root.join(".git/HEAD")).ok()?;
    let content = content.trim();
    Some(match content.strip_prefix("ref: refs/heads/") {
        Some(name) => name.chars().take(24).collect(),
        // Detached: HEAD holds the raw commit. Show it the way git does.
        None => content.chars().take(8).collect(),
    })
}

/// Runs `git status` in `dir` and parses it. Blocking — call this on a worker.
pub fn read_state(dir: &Path) -> Option<RepoState> {
    let out = std::process::Command::new("git")
        .arg("--no-optional-locks")
        .args(["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])
        .current_dir(dir)
        // Belt and braces: the flag covers this git, the variable covers any git it
        // invokes on its own behalf.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_porcelain_v2(&String::from_utf8_lossy(&out.stdout)))
}

/// Parses `git status --porcelain=v2 --branch`. Kept pure and separate from the
/// process call so the format handling is unit-testable without a repository.
///
/// Header lines start with `#`; entries are `1`/`2` (changed, renamed), `u`
/// (unmerged) and `?` (untracked). On a `1`/`2` line the two-character field is
/// staged-status then worktree-status, `.` meaning unchanged — so the same file can
/// count as both staged and dirty, which is exactly what it is.
pub fn parse_porcelain_v2(text: &str) -> RepoState {
    let mut s = RepoState::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("#") => match it.next() {
                Some("branch.head") => {
                    if let Some(name) = it.next() {
                        s.detached = name == "(detached)";
                        if !s.detached {
                            s.branch = name.chars().take(24).collect();
                        }
                    }
                }
                Some("branch.oid") => {
                    // Only used when detached, where branch.head has no name to give.
                    if let Some(oid) = it.next() {
                        if s.branch.is_empty() {
                            s.branch = oid.chars().take(8).collect();
                        }
                    }
                }
                Some("branch.upstream") => s.upstream = true,
                Some("branch.ab") => {
                    // `+1 -2`, signs always present.
                    if let (Some(a), Some(b)) = (it.next(), it.next()) {
                        s.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
                        s.behind = b.trim_start_matches('-').parse().unwrap_or(0);
                    }
                }
                _ => {}
            },
            Some("1") | Some("2") => {
                if let Some(xy) = it.next() {
                    let mut c = xy.chars();
                    if c.next().is_some_and(|x| x != '.') {
                        s.staged += 1;
                    }
                    if c.next().is_some_and(|y| y != '.') {
                        s.dirty += 1;
                    }
                }
            }
            Some("u") => s.conflicts += 1,
            Some("?") => s.dirty += 1,
            _ => {}
        }
    }
    // A detached head that somehow reported neither name nor oid still needs a label.
    if s.branch.is_empty() {
        s.branch = "HEAD".into();
    }
    s
}

/// The status-bar text for a repository: branch first, then only the counts that are
/// non-zero. A clean repo shows its branch and nothing else, so the bar stays quiet
/// until there is something to say.
pub fn status_text(s: &RepoState) -> String {
    let mut out = String::new();
    if s.detached {
        out.push_str("detached ");
    }
    out.push_str(&s.branch);
    if s.behind > 0 {
        out.push_str(&format!(" \u{2193}{}", s.behind));
    }
    if s.ahead > 0 {
        out.push_str(&format!(" \u{2191}{}", s.ahead));
    }
    if s.staged > 0 {
        out.push_str(&format!(" +{}", s.staged));
    }
    if s.dirty > 0 {
        out.push_str(&format!(" \u{25cf}{}", s.dirty));
    }
    if s.conflicts > 0 {
        out.push_str(&format!(" !{}", s.conflicts));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# branch.oid 8f6876d1c0ffee00
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 .M N... 100644 100644 100644 aaa bbb src/grid.rs
1 M. N... 100644 100644 100644 ccc ddd src/main.rs
1 MM N... 100644 100644 100644 eee fff src/render.rs
? notes.txt
? scratch/
";

    #[test]
    fn parses_branch_and_divergence() {
        let s = parse_porcelain_v2(SAMPLE);
        assert_eq!(s.branch, "main");
        assert!(!s.detached);
        assert!(s.has_upstream());
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
    }

    #[test]
    fn a_file_can_be_both_staged_and_dirty() {
        // src/render.rs is MM: staged edits AND further unstaged ones. Counting it
        // once would hide half of what is about to be lost or committed.
        let s = parse_porcelain_v2(SAMPLE);
        assert_eq!(s.staged, 2, "M. and MM are staged");
        assert_eq!(s.dirty, 4, ".M, MM and the two untracked entries");
        assert_eq!(s.conflicts, 0);
    }

    #[test]
    fn counts_conflicts_separately() {
        let s = parse_porcelain_v2("# branch.head main\nu UU N... 1 2 3 4 aa bb cc src/x.rs\n");
        assert_eq!(s.conflicts, 1);
        // A conflict is not "dirt" — it needs resolving, not committing, and the
        // status bar says so with its own marker.
        assert_eq!(s.dirty, 0);
        assert_eq!(s.staged, 0);
    }

    #[test]
    fn a_detached_head_reports_the_commit() {
        let s = parse_porcelain_v2("# branch.oid 1234567890abcdef\n# branch.head (detached)\n");
        assert!(s.detached);
        assert_eq!(s.branch, "12345678");
        assert!(!s.has_upstream(), "a detached head has no upstream to be ahead of");
    }

    #[test]
    fn a_clean_repo_says_only_its_branch() {
        let s = parse_porcelain_v2("# branch.oid aaa\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n");
        assert!(s.is_clean());
        assert_eq!(status_text(&s), "main");
    }

    #[test]
    fn status_text_shows_only_what_is_non_zero() {
        let s = parse_porcelain_v2(SAMPLE);
        assert_eq!(status_text(&s), "main \u{2193}1 \u{2191}2 +2 \u{25cf}4");
    }

    #[test]
    fn empty_output_is_not_a_panic() {
        let s = parse_porcelain_v2("");
        assert_eq!(s.branch, "HEAD");
        assert!(s.is_clean());
    }
}
