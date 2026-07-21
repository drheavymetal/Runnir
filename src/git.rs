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
    /// An operation left half-finished in this repository — a rebase, a merge, a
    /// cherry-pick. The bar has to say so: every command behaves differently in the
    /// middle of one, and the branch name alone hides it completely.
    pub operation: Option<&'static str>,
    /// Local branch names, so hint mode can tell a branch from any other word on
    /// the screen. Read from the refs, never guessed: a token is a branch only if
    /// this repository actually has a branch by that name.
    pub branches: Vec<String>,
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
        // `.exists()`, not `.is_dir()`: in a worktree or submodule `.git` is a file.
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

/// The git directory for a working tree.
///
/// Usually `<root>/.git`, but in a WORKTREE (and in a submodule) `.git` is a FILE
/// holding `gitdir: <path>`. Reading `<root>/.git/HEAD` there fails outright, which
/// is how a worktree ended up with no branch anywhere in the UI.
pub fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let path = text.strip_prefix("gitdir:")?.trim();
    let path = Path::new(path);
    Some(if path.is_absolute() { path.to_path_buf() } else { root.join(path) })
}

/// Where the refs live for this working tree. A worktree's own git dir holds its
/// HEAD and index, but `refs/heads` and `packed-refs` belong to the main one, named
/// by the `commondir` file. Branch lists must come from there or a worktree lists
/// nothing.
pub fn common_dir(git_dir: &Path) -> PathBuf {
    let Ok(text) = std::fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let p = Path::new(text.trim());
    if p.is_absolute() { p.to_path_buf() } else { git_dir.join(p) }
}

/// The current branch for `dir`, read straight out of HEAD. No git process is
/// spawned, so this is safe to call from the draw path. A detached head yields the
/// short commit id.
pub fn head_branch(dir: &Path) -> Option<String> {
    let root = repo_root(dir)?;
    let content = std::fs::read_to_string(git_dir(&root)?.join("HEAD")).ok()?;
    let content = content.trim();
    Some(match content.strip_prefix("ref: refs/heads/") {
        Some(name) => name.chars().take(24).collect(),
        // Detached: HEAD holds the raw commit. Show it the way git does.
        None => content.chars().take(8).collect(),
    })
}

/// Local branch names, from `.git/refs/heads` plus `.git/packed-refs`. No git
/// process: refs are files, and a repository that has just been cloned keeps most
/// of them packed, which is why both sources are read.
///
/// Capped, because a repository with thousands of branches would otherwise make
/// every hint-mode scan walk them all.
pub fn local_branches(root: &Path) -> Vec<String> {
    const CAP: usize = 512;
    let mut out = Vec::new();
    let Some(common) = git_dir(root).map(|g| common_dir(&g)) else { return out };
    let heads = common.join("refs/heads");
    walk_refs(&heads, &heads, &mut out, CAP);
    if let Ok(packed) = std::fs::read_to_string(common.join("packed-refs")) {
        for line in packed.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some(name) = line.split_whitespace().nth(1).and_then(|r| r.strip_prefix("refs/heads/")) {
                if out.len() >= CAP {
                    break;
                }
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Recurses `refs/heads`, since a branch called `feature/x` is a directory and a
/// file, not a file with a slash in its name.
fn walk_refs(base: &Path, dir: &Path, out: &mut Vec<String>, cap: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.len() >= cap {
            return;
        }
        let path = e.path();
        if path.is_dir() {
            walk_refs(base, &path, out, cap);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
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
    let mut state = parse_porcelain_v2(&String::from_utf8_lossy(&out.stdout));
    // Collected on the same worker: it is file I/O, and the caller is already off
    // the UI thread here.
    if let Some(root) = repo_root(dir) {
        state.branches = local_branches(&root);
        state.operation = git_dir(&root).and_then(|g| in_progress(&g));
    }
    Some(state)
}

/// The operation this repository is in the middle of, by the marker files git
/// leaves in its git dir. Named the way git names them in its own messages.
pub fn in_progress(git_dir: &Path) -> Option<&'static str> {
    let has = |p: &str| git_dir.join(p).exists();
    if has("rebase-merge") || has("rebase-apply") {
        Some("REBASE")
    } else if has("MERGE_HEAD") {
        Some("MERGE")
    } else if has("CHERRY_PICK_HEAD") {
        Some("CHERRY-PICK")
    } else if has("REVERT_HEAD") {
        Some("REVERT")
    } else if has("BISECT_LOG") {
        Some("BISECT")
    } else {
        None
    }
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

// ---- data for the git panel ------------------------------------------------

/// One line of history, as the panel lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    /// Relative date, the way git writes it: "3 hours ago".
    pub when: String,
    /// Ref decorations on this commit: `HEAD -> main, origin/main`.
    pub refs: String,
}

/// One path in `git status`, split the way the panel shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    /// Index (staged) status letter, `.` when unchanged, `?` when untracked.
    pub index: char,
    /// Worktree status letter, `.` when unchanged.
    pub worktree: char,
}

impl FileEntry {
    pub fn is_staged(&self) -> bool {
        self.index != '.' && self.index != '?'
    }
    pub fn is_unstaged(&self) -> bool {
        self.worktree != '.' || self.index == '?'
    }
    pub fn untracked(&self) -> bool {
        self.index == '?'
    }
}

/// Field separator for the log format. A unit separator cannot appear in a subject,
/// which splitting on a printable character could not promise.
const SEP: char = '\u{1f}';

/// Reads history. Blocking — worker only.
pub fn log(root: &Path, limit: usize) -> Vec<Commit> {
    log_filtered(root, limit, "")
}

/// History, optionally narrowed to commits whose message matches `grep`. Empty
/// filter means the plain log — the panel passes whatever was typed, and an empty
/// prompt is how you clear it.
pub fn log_filtered(root: &Path, limit: usize, grep: &str) -> Vec<Commit> {
    let fmt = format!("--pretty=format:%h{SEP}%s{SEP}%an{SEP}%ar{SEP}%D");
    let mut cmd = std::process::Command::new("git");
    cmd.arg("--no-optional-locks")
        .args(["log", "--no-color"])
        .arg(format!("-{limit}"))
        .arg(fmt);
    if !grep.trim().is_empty() {
        cmd.arg("--regexp-ignore-case").arg(format!("--grep={}", grep.trim()));
    }
    let out = cmd
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => parse_log(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut f = l.split(SEP);
            Commit {
                sha: f.next().unwrap_or_default().to_string(),
                subject: f.next().unwrap_or_default().to_string(),
                author: f.next().unwrap_or_default().to_string(),
                when: f.next().unwrap_or_default().to_string(),
                refs: f.next().unwrap_or_default().to_string(),
            }
        })
        .collect()
}

/// The files `git status` reports, in a stable order: staged first, then the rest.
pub fn status_files(root: &Path) -> Vec<FileEntry> {
    let out = std::process::Command::new("git")
        .arg("--no-optional-locks")
        .args(["status", "--porcelain=v2", "--untracked-files=normal"])
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => parse_status_files(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Parses the entry lines of `--porcelain=v2`. Pure, so the format is testable.
///
/// A rename (`2`) carries two paths separated by a tab; the new one is what the
/// panel acts on. Untracked (`?`) has no XY field at all.
pub fn parse_status_files(text: &str) -> Vec<FileEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some(tag @ ("1" | "2")) => {
                let Some(xy) = it.next() else { continue };
                let mut c = xy.chars();
                let index = c.next().unwrap_or('.');
                let worktree = c.next().unwrap_or('.');
                // The header is a fixed number of space-separated fields and the
                // path is everything after it — split by COUNT, not by whitespace,
                // because a path may contain spaces, and a rename appends the old
                // path after a TAB (which `split_whitespace` would eat).
                let header = if tag == "1" { 8 } else { 9 };
                let path = line.splitn(header + 1, ' ').nth(header).unwrap_or_default();
                let path = path.split('\t').next().unwrap_or(path).to_string();
                if !path.is_empty() {
                    out.push(FileEntry { path, index, worktree });
                }
            }
            Some("u") => {
                // `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`.
                let path = line.splitn(11, ' ').nth(10).unwrap_or_default().to_string();
                if !path.is_empty() {
                    out.push(FileEntry { path, index: 'U', worktree: 'U' });
                }
            }
            Some("?") => {
                let path: String = line[1..].trim().to_string();
                if !path.is_empty() {
                    out.push(FileEntry { path, index: '?', worktree: '?' });
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| b.is_staged().cmp(&a.is_staged()).then(a.path.cmp(&b.path)));
    out
}

/// The files one commit touched, with git's status letter for each. This is what
/// makes a commit readable: a 40-file diff in one scrolling pane is a wall, and the
/// question is almost always "what did it do to THIS file".
pub fn commit_files(root: &Path, sha: &str) -> Vec<FileEntry> {
    let text = read_text(
        root,
        &["show", "--no-color", "--name-status", "--format=", "--find-renames", sha],
    );
    parse_name_status(&text)
}

/// The same list for a stash entry, which is a commit like any other.
pub fn parse_name_status(text: &str) -> Vec<FileEntry> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let status = parts.next()?.chars().next()?;
            // A rename is `R100 <old> <new>`; the new path is the one to show.
            let first = parts.next()?;
            let path = parts.next().unwrap_or(first);
            Some(FileEntry { path: path.to_string(), index: status, worktree: '.' })
        })
        .collect()
}

/// One file's diff inside one commit.
///
/// `--format=` drops the commit header: the message is already on screen when you
/// pick the commit, and repeating it above every file would push the diff — the
/// thing you drilled in for — off the top.
pub fn show_file(root: &Path, sha: &str, path: &str) -> String {
    read_text(root, &["show", "--no-color", "--format=", "--patch", sha, "--", path])
}

/// A commit's full diff, for the panel's preview pane.
pub fn show(root: &Path, sha: &str) -> String {
    read_text(root, &["show", "--no-color", "--stat", "--patch", sha])
}

/// One file's diff — staged or unstaged, which are different diffs of the same path.
pub fn diff_file(root: &Path, path: &str, staged: bool, untracked: bool) -> String {
    if untracked {
        // An untracked file has no diff; show it, which is what you want to see
        // before staging it.
        return std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|e| format!("cannot read {path}: {e}"));
    }
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--staged");
    }
    args.push("--");
    args.push(path);
    read_text(root, &args)
}

fn read_text(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(e) => format!("git: {e}"),
    }
}

/// How long a panel command may take before it is killed. Generous enough for a
/// push over a slow link, short enough that a hung one does not leave the panel
/// saying "working…" for the rest of the session.
pub const RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Runs a git command that CHANGES the repository, returning its combined output.
///
/// The panel binds only operations git can undo: staging, committing, fetching,
/// pushing, switching branch, stashing. Nothing that discards uncommitted work is
/// reachable from a key here — those live at the prompt, where the guardian sees
/// them and asks.
///
/// Two things make this safe to run behind a UI:
/// - **No prompting.** `GIT_TERMINAL_PROMPT=0` and ssh in batch mode mean a command
///   that needs a password FAILS instead of blocking on a terminal that is not
///   there. [`needs_terminal`] recognises that failure so the caller can rerun it
///   in a real pane.
/// - **A deadline.** A remote that never answers, or a filesystem that hangs, would
///   otherwise pin the panel in its busy state forever.
pub fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let child = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        // Batch mode turns "ask for the passphrase" into an immediate, recognisable
        // failure. Without it ssh opens /dev/tty behind our back and hangs there.
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = match rx.recv_timeout(RUN_TIMEOUT) {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("git: {e}")),
        Err(_) => {
            kill(pid);
            return Err(format!(
                "git {} timed out after {}s — killed",
                args.first().map(String::as_str).unwrap_or(""),
                RUN_TIMEOUT.as_secs()
            ));
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() { Ok(text) } else { Err(text) }
}

/// Kills a git that ran past its deadline. The reader thread ends when the pipes
/// close, so nothing is leaked by walking away from it.
fn kill(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// A cheap fingerprint of the repository's own state: the modification times of
/// the index and of HEAD.
///
/// The status bar refreshes when a command finishes in the pane, which misses every
/// change made from somewhere else — an editor writing a file, a git run in another
/// pane, a rebase in a second window. Two `stat` calls per tick is a price worth
/// paying to notice those; walking the working tree would not be.
pub fn state_stamp(root: &Path) -> u64 {
    let Some(dir) = git_dir(root) else { return 0 };
    let mtime = |p: PathBuf| -> u64 {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    // The index covers staging and checkouts; HEAD covers commits, switches and
    // every step of a rebase. A working-tree edit shows up through the index only
    // once it is staged, which is why the counters can still lag a bare edit — the
    // status bar says "dirty", and dirty it stays.
    mtime(dir.join("index")) ^ mtime(dir.join("HEAD")).rotate_left(1)
}

/// Whether this failure means "there was nobody to ask for a credential".
///
/// A push to a repo behind HTTPS, an ssh key with a passphrase, an unknown host
/// key, a 2FA prompt: none of them can be answered from a background process with
/// no terminal. The answer is not to prompt inside the panel — it is to run the
/// same command again in a real pane, where ssh and git already know how to ask.
pub fn needs_terminal(err: &str) -> bool {
    const SIGNS: &[&str] = &[
        "terminal prompts disabled",
        "could not read username",
        "could not read password",
        "authentication failed",
        "permission denied (publickey",
        "host key verification failed",
        "no such identity",
        "enter passphrase",
        "askpass",
        "connection closed by remote host",
    ];
    let low = err.to_lowercase();
    SIGNS.iter().any(|s| low.contains(s))
}

/// The status-bar text for a repository: branch first, then only the counts that are
/// non-zero. A clean repo shows its branch and nothing else, so the bar stays quiet
/// until there is something to say.
pub fn status_text(s: &RepoState) -> String {
    let mut out = String::new();
    // First, because it changes what every other number means: 2 commits "ahead"
    // in the middle of a rebase is not the same fact as 2 commits ahead on a
    // finished branch.
    if let Some(op) = s.operation {
        out.push_str(op);
        out.push(' ');
    }
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

/// The line ranges of each hunk in a parsed diff: from its `@@` header to the row
/// before the next one.
pub fn hunk_ranges(rows: &[DiffRow]) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == DiffKind::Meta && r.text.starts_with("@@"))
        .map(|(i, _)| i)
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| (start, starts.get(n + 1).copied().unwrap_or(rows.len())))
        .collect()
}

/// Rebuilds a one-hunk patch that `git apply` will take.
///
/// The panel strips the `+`/`-` column for display, so it is put back here from the
/// row's kind. The file header has to come along — a hunk on its own applies to
/// nothing — and it is taken from the metadata rows above the hunk, which is where
/// `diff --git` and the `---`/`+++` pair live.
pub fn patch_for_hunk(rows: &[DiffRow], range: (usize, usize)) -> Option<String> {
    let (start, end) = range;
    if start >= rows.len() {
        return None;
    }
    let mut out = String::new();
    for row in &rows[..start] {
        let t = &row.text;
        if t.starts_with("diff --git")
            || t.starts_with("index ")
            || t.starts_with("--- ")
            || t.starts_with("+++ ")
            || t.starts_with("old mode")
            || t.starts_with("new mode")
            || t.starts_with("new file mode")
            || t.starts_with("deleted file mode")
            || t.starts_with("rename from")
            || t.starts_with("rename to")
        {
            out.push_str(t);
            out.push('\n');
        }
    }
    if !out.contains("--- ") || !out.contains("+++ ") {
        return None; // No file header: nothing to apply this against.
    }
    for row in &rows[start..end.min(rows.len())] {
        match row.kind {
            DiffKind::Added => out.push('+'),
            DiffKind::Removed => out.push('-'),
            DiffKind::Context => out.push(' '),
            DiffKind::Meta => {}
        }
        out.push_str(&row.text);
        out.push('\n');
    }
    Some(out)
}

/// Stages (or, reversed, unstages) a patch by feeding it to `git apply --cached`.
///
/// `--cached` touches only the index, so a hunk staged this way leaves the working
/// tree exactly as it was — the property that makes partial staging safe to bind to
/// a key at all.
pub fn apply_patch(root: &Path, patch: &str, reverse: bool) -> Result<String, String> {
    use std::io::Write;
    let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    let mut child = std::process::Command::new("git")
        .args(&args)
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git apply: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("git apply: no stdin")?
        .write_all(patch.as_bytes())
        .map_err(|e| format!("git apply: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("git apply: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(if reverse { "hunk unstaged".into() } else { "hunk staged".into() })
    } else {
        Err(text)
    }
}

// ---- the rest of the operation set ----------------------------------------

/// Remote-tracking branches, from the refs — same two sources as the local ones,
/// and the same reason (a fresh clone keeps them packed).
pub fn remote_branches(root: &Path) -> Vec<String> {
    const CAP: usize = 512;
    let mut out = Vec::new();
    let Some(common) = git_dir(root).map(|g| common_dir(&g)) else { return out };
    let base = common.join("refs/remotes");
    walk_refs(&base, &base, &mut out, CAP);
    if let Ok(packed) = std::fs::read_to_string(common.join("packed-refs")) {
        for line in packed.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some(name) =
                line.split_whitespace().nth(1).and_then(|r| r.strip_prefix("refs/remotes/"))
            {
                if out.len() >= CAP {
                    break;
                }
                out.push(name.to_string());
            }
        }
    }
    // `origin/HEAD` is a symbolic pointer, not a branch anyone checks out.
    out.retain(|b| !b.ends_with("/HEAD"));
    out.sort();
    out.dedup();
    out
}

/// Tags, newest first: a tag list in creation order is far more useful than in
/// alphabetical order, where v10 sorts before v9.
pub fn tags(root: &Path) -> Vec<String> {
    read_text(root, &["tag", "--sort=-creatordate", "--format=%(refname:short) %(subject)"])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// The reflog: every position HEAD has held. This is the undo history for
/// everything the panel refuses to bind — a bad reset, a lost branch, a dropped
/// commit are all recoverable from here, so showing it is worth more than binding
/// the operations that make you need it.
pub fn reflog(root: &Path, limit: usize) -> Vec<Commit> {
    let fmt = format!("--pretty=format:%h{SEP}%gs{SEP}%an{SEP}%gd{SEP}%s");
    let out = std::process::Command::new("git")
        .arg("--no-optional-locks")
        .args(["reflog", "--no-color"])
        .arg(format!("-{limit}"))
        .arg(fmt)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => parse_log(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// The worktrees of this repository, one per line as `git worktree list` prints
/// them: path, commit, and the branch checked out there. Relevant here because
/// agent branches live in worktrees, and a terminal is how you visit one.
pub fn worktrees(root: &Path) -> Vec<String> {
    read_text(root, &["worktree", "list"])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// Submodules, as `git submodule status` prints them: a status character, the
/// commit, the path, and the branch. Listed beside the worktrees because they are
/// the same question — "what other checkouts hang off this repository" — and both
/// answers are a directory you may want a shell in.
pub fn submodules(root: &Path) -> Vec<String> {
    read_text(root, &["submodule", "status", "--recursive"])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| format!("submodule {}", l.trim()))
        .collect()
}

/// The path column of a `git worktree list` line, so opening one is a real cd.
/// A submodule row is `submodule <sha> <path> (<branch>)`, so its path is third.
pub fn worktree_path(line: &str) -> &str {
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("submodule") => {
            parts.next();
            parts.next().unwrap_or(line)
        }
        Some(first) => first,
        None => line,
    }
}

/// Whether a path is unmerged right now. Checked before binding a resolution key to
/// it, so `ours`/`theirs` cannot be aimed at a file that has no conflict.
pub fn is_conflicted(files: &[FileEntry], path: &str) -> bool {
    files.iter().any(|f| f.path == path && f.index == 'U')
}

/// The upstream branch for HEAD, or `None` when the branch has never been pushed.
/// A push then needs `-u`, which is the difference between working and a fatal.
pub fn upstream_of_head(root: &Path) -> Option<String> {
    let out = read_text(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
    let out = out.trim();
    (!out.is_empty() && !out.contains("fatal") && !out.contains("no upstream")).then(|| out.to_string())
}

/// The argv for pushing this branch: plain when it already tracks something, and
/// `-u origin HEAD` the first time, which is what makes a brand-new branch push at
/// all instead of failing with "has no upstream branch".
pub fn push_args(root: &Path) -> Vec<String> {
    match upstream_of_head(root) {
        Some(_) => vec!["push".into()],
        None => vec!["push".into(), "-u".into(), "origin".into(), "HEAD".into()],
    }
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
    fn a_worktree_line_yields_its_path() {
        let line = "/home/pedro/projects/runnir/.claude/worktrees/agent-a03  8f6876d [worktree-agent-a03]";
        assert_eq!(worktree_path(line), "/home/pedro/projects/runnir/.claude/worktrees/agent-a03");
    }

    #[test]
    fn parses_the_files_a_commit_touched() {
        let files = parse_name_status(
            "M\tsrc/git.rs\nA\tsrc/new.rs\nD\tdocs/old.md\nR100\tdocs/a.md\tdocs/b.md\n",
        );
        assert_eq!(files.len(), 4);
        assert_eq!((files[0].index, files[0].path.as_str()), ('M', "src/git.rs"));
        assert_eq!(files[1].index, 'A');
        assert_eq!(files[2].index, 'D');
        // A rename reports both paths; the new one is what the diff is against.
        assert_eq!((files[3].index, files[3].path.as_str()), ('R', "docs/b.md"));
    }

    #[test]
    fn a_submodule_row_yields_its_path_not_its_sha() {
        let line = "submodule  8f6876d1 vendor/thing (v1.2.0)";
        assert_eq!(worktree_path(line), "vendor/thing");
    }

    #[test]
    fn a_conflict_is_recognised_by_its_status_letter() {
        let files = parse_status_files(
            "1 .M N... 100644 100644 100644 aaa bbb src/ok.rs\n\
             u UU N... 100644 100644 100644 100644 aa bb cc src/bad.rs\n",
        );
        assert!(is_conflicted(&files, "src/bad.rs"));
        assert!(!is_conflicted(&files, "src/ok.rs"), "a modified file is not a conflict");
        assert!(!is_conflicted(&files, "src/absent.rs"));
    }

    #[test]
    fn rebuilds_a_patch_for_one_hunk_of_many() {
        let text = "\
diff --git a/src/x.rs b/src/x.rs
index aaa..bbb 100644
--- a/src/x.rs
+++ b/src/x.rs
@@ -1,3 +1,3 @@
 one
-two
+TWO
@@ -10,3 +10,3 @@
 ten
-eleven
+ELEVEN
";
        let rows = parse_diff(text);
        let ranges = hunk_ranges(&rows);
        assert_eq!(ranges.len(), 2);
        let patch = patch_for_hunk(&rows, ranges[1]).expect("second hunk");
        // The file header travels with it, or git apply has nothing to apply to.
        assert!(patch.starts_with("diff --git a/src/x.rs b/src/x.rs\n"));
        assert!(patch.contains("--- a/src/x.rs\n+++ b/src/x.rs\n"));
        // Only the SECOND hunk's lines, with the markers put back.
        assert!(patch.contains("@@ -10,3 +10,3 @@\n ten\n-eleven\n+ELEVEN\n"));
        assert!(!patch.contains("TWO"), "the first hunk must not leak in: {patch}");
    }

    #[test]
    fn a_diff_with_no_file_header_yields_no_patch() {
        // An untracked file's "diff" is its contents; there is nothing to apply.
        let rows = parse_diff("@@ -0,0 +1 @@\n+hello\n");
        let ranges = hunk_ranges(&rows);
        assert!(patch_for_hunk(&rows, ranges[0]).is_none());
    }

    #[test]
    fn recognises_a_failure_that_needs_a_real_terminal() {
        for err in [
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            "git@github.com: Permission denied (publickey).",
            "Host key verification failed.",
            "remote: Authentication failed for 'https://github.com/x/y.git/'",
        ] {
            assert!(needs_terminal(err), "{err:?} should ask for a terminal");
        }
        // An ordinary rejection must NOT reopen in a pane: nothing there can be
        // typed to fix it, and a split per failed push would be noise.
        for err in [
            "error: failed to push some refs to 'origin' (non-fast-forward)",
            "error: Your local changes would be overwritten by merge",
            "fatal: not a git repository",
        ] {
            assert!(!needs_terminal(err), "{err:?} is not an auth failure");
        }
    }

    #[test]
    fn an_unfinished_operation_leads_the_status_text() {
        let mut st = parse_porcelain_v2("# branch.head main\n# branch.ab +2 -0\n");
        st.operation = Some("REBASE");
        assert_eq!(status_text(&st), "REBASE main \u{2191}2");
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
    fn parses_the_log_format() {
        let sep = SEP;
        let text = format!(
            "59248cc{sep}Make hints understand git objects{sep}drheavymetal{sep}2 minutes ago{sep}HEAD -> main\n\
             f36a585{sep}Show the repository's real state{sep}drheavymetal{sep}1 hour ago{sep}\n"
        );
        let log = parse_log(&text);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].sha, "59248cc");
        assert_eq!(log[0].subject, "Make hints understand git objects");
        assert_eq!(log[0].refs, "HEAD -> main");
        // A subject can contain anything printable, including the characters one
        // would be tempted to split on. The unit separator cannot appear in it.
        assert_eq!(log[1].subject, "Show the repository's real state");
        assert_eq!(log[1].refs, "");
    }

    #[test]
    fn parses_status_entries_into_files() {
        let text = "\
1 M. N... 100644 100644 100644 aaa bbb src/main.rs
1 .M N... 100644 100644 100644 ccc ddd src/git.rs
2 R. N... 100644 100644 100644 eee fff R100 docs/NEW.md\tdocs/OLD.md
u UU N... 100644 100644 100644 100644 aa bb cc src/conflict.rs
? notes.txt
";
        let files = parse_status_files(text);
        let by = |p: &str| files.iter().find(|f| f.path == p).unwrap_or_else(|| panic!("{p} missing"));
        assert!(by("src/main.rs").is_staged());
        assert!(!by("src/main.rs").is_unstaged());
        assert!(by("src/git.rs").is_unstaged());
        assert!(by("notes.txt").untracked());
        // A rename lists both paths; the panel acts on the new one.
        assert!(by("docs/NEW.md").is_staged());
        assert_eq!(by("src/conflict.rs").index, 'U');
        // Staged first, so the commit-ready set reads as a block.
        assert!(files[0].is_staged());
    }

    #[test]
    fn a_diff_becomes_numbered_rows() {
        let text = "\
diff --git a/src/git.rs b/src/git.rs
index aaa..bbb 100644
--- a/src/git.rs
+++ b/src/git.rs
@@ -10,4 +10,5 @@ fn thing() {
 unchanged one
-old line
+new line
+extra line
 unchanged two
";
        let rows = parse_diff(text);
        let body: Vec<_> = rows.iter().filter(|r| r.kind != DiffKind::Meta).collect();
        assert_eq!(body[0].kind, DiffKind::Context);
        assert_eq!(body[0].number, Some(10), "context is numbered from the hunk header");
        // The removed line keeps the OLD file's number, the added ones the new
        // file's: that is what makes the pair readable side by side.
        assert_eq!((body[1].kind, body[1].number), (DiffKind::Removed, Some(11)));
        assert_eq!((body[2].kind, body[2].number), (DiffKind::Added, Some(11)));
        assert_eq!((body[3].kind, body[3].number), (DiffKind::Added, Some(12)));
        assert_eq!((body[4].kind, body[4].number), (DiffKind::Context, Some(13)));
        // The marker character is stripped: the panel shows the code, and says
        // added or removed with the row's background instead.
        assert_eq!(body[1].text, "old line");
        assert_eq!(body[2].text, "new line");
    }

    #[test]
    fn diff_metadata_is_not_mistaken_for_content() {
        let rows = parse_diff("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n");
        assert!(rows.iter().take(3).all(|r| r.kind == DiffKind::Meta), "{rows:?}");
        assert_eq!(rows[3].text, "a");
    }

    #[test]
    fn empty_output_is_not_a_panic() {
        let s = parse_porcelain_v2("");
        assert_eq!(s.branch, "HEAD");
        assert!(s.is_clean());
    }
}

// ---- panel workers ---------------------------------------------------------

/// A worker's answer for the git panel, delivered through `UserEvent::GitPanel`.
pub enum PanelMsg {
    Files(Vec<FileEntry>),
    Log(Vec<Commit>),
    /// Local branches, remote-tracking branches, and the branch checked out now.
    Branches(Vec<String>, Vec<String>, String),
    /// The files one commit touched, after drilling into it.
    CommitFiles(Vec<FileEntry>),
    Tags(Vec<String>),
    Reflog(Vec<Commit>),
    Worktrees(Vec<String>),
    Stashes(Vec<String>),
    Preview(String),
    /// A command finished: what it was, and its output or its error. The argv comes
    /// back too, because a failure that needs a terminal is rerun in a real pane.
    Ran(Vec<String>, Result<String, String>),
}

/// The stash list, one entry per line (`stash@{0}: WIP on main: ...`).
pub fn stashes(root: &Path) -> Vec<String> {
    read_text(root, &["stash", "list"])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

pub fn stash_show(root: &Path, name: &str) -> String {
    read_text(root, &["stash", "show", "--stat", "--patch", "--no-color", name])
}

/// A branch's recent history, for the panel's preview pane.
pub fn branch_log(root: &Path, branch: &str) -> String {
    read_text(root, &["log", "--no-color", "--oneline", "--graph", "--decorate", "-40", branch])
}

// ---- diff rendering --------------------------------------------------------

/// What a line of a diff is, once parsed. The panel draws each kind differently,
/// so the `+`/`-` column can go away: a full-width background says it better, and
/// leaves the code itself starting at a constant column where it stays readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Context,
    /// File headers, hunk headers, commit metadata, `git show --stat` output — any
    /// line that is about the diff rather than in it.
    Meta,
}

/// One line of a diff, with the line number it has in the file it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffKind,
    /// The number to show: the new file's for added and context lines, the old
    /// file's for removed ones. `None` on metadata.
    pub number: Option<u32>,
    pub text: String,
}

/// Parses a unified diff into numbered rows.
///
/// Line numbers come from the hunk headers (`@@ -a,b +c,d @@`) and are counted
/// forward, which is the whole point: a raw diff makes you count rows by hand to
/// find out which line changed.
pub fn parse_diff(text: &str) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let mut in_hunk = false;
    for line in text.lines() {
        if line.starts_with("@@") {
            (old_no, new_no) = parse_hunk_header(line).unwrap_or((old_no, new_no));
            in_hunk = true;
            rows.push(DiffRow { kind: DiffKind::Meta, number: None, text: line.to_string() });
            continue;
        }
        // Outside a hunk everything is metadata: the commit header, the --stat
        // block, `diff --git`, `index`, mode changes.
        if !in_hunk {
            rows.push(DiffRow { kind: DiffKind::Meta, number: None, text: line.to_string() });
            continue;
        }
        match line.as_bytes().first() {
            Some(b'+') if !line.starts_with("+++") => {
                rows.push(DiffRow {
                    kind: DiffKind::Added,
                    number: Some(new_no),
                    text: line[1..].to_string(),
                });
                new_no += 1;
            }
            Some(b'-') if !line.starts_with("---") => {
                rows.push(DiffRow {
                    kind: DiffKind::Removed,
                    number: Some(old_no),
                    text: line[1..].to_string(),
                });
                old_no += 1;
            }
            Some(b'\\') => {
                // "\ No newline at end of file" belongs to neither side.
                rows.push(DiffRow { kind: DiffKind::Meta, number: None, text: line.to_string() });
            }
            Some(b' ') | None => {
                rows.push(DiffRow {
                    kind: DiffKind::Context,
                    number: Some(new_no),
                    text: line.get(1..).unwrap_or("").to_string(),
                });
                old_no += 1;
                new_no += 1;
            }
            // A new `diff --git` ends the hunk we were in.
            _ => {
                in_hunk = false;
                rows.push(DiffRow { kind: DiffKind::Meta, number: None, text: line.to_string() });
            }
        }
    }
    rows
}

/// `@@ -12,7 +12,9 @@ context` -> the first old and new line numbers.
fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    let mut parts = line.split_whitespace();
    parts.next()?; // @@
    let old = parts.next()?.trim_start_matches('-');
    let new = parts.next()?.trim_start_matches('+');
    let first = |s: &str| s.split(',').next().unwrap_or("0").parse().ok();
    Some((first(old)?, first(new)?))
}
