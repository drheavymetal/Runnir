//! Hint mode: find every URL, path and git hash on the visible screen so the user
//! can jump to one by typing a short label, no mouse involved.
//!
//! Extraction is line-based and deliberately simple — a full URL grammar is not
//! worth it when the payoff is "open the thing I can see".

use crate::grid::Grid;
use crate::overlay::{Hint, HintKind, hint_labels};

/// What the pane knows about its repository, so a scan can recognise git objects on
/// screen. Empty outside a repository, which turns every git-specific rule off.
///
/// Branch names are DATA, not a pattern: `main`, `dev` and `wip` are ordinary words
/// that appear all over normal output, so a token counts as a branch only when this
/// repository has a branch by exactly that name. The list is snapshotted from the
/// status worker's cache, so a scan does no I/O — it runs on every mouse move for
/// the hover underline.
#[derive(Default)]
pub struct Context<'a> {
    pub branches: &'a [String],
}

/// Finds targets on the currently visible rows of `grid`. The reported column is a
/// real grid column (accounting for wide/spacer cells), so a hint label or a hover
/// underline lands on the target even on a row with CJK or emoji.
pub fn find(grid: &Grid, ctx: &Context) -> Vec<Hint> {
    let mut raw: Vec<(usize, usize, String, HintKind)> = Vec::new();
    for row in 0..grid.rows() {
        let abs = grid.abs_row(row);
        let (line, col_map) = row_text(grid, abs);
        let mut row_hits: Vec<(usize, usize, String, HintKind)> = Vec::new();
        scan_line(abs, &line, ctx, &mut row_hits);
        // scan_line records a char index into the spacer-stripped line; translate it
        // back to the grid column via the map so wide chars before it are accounted.
        for (a, char_col, text, kind) in row_hits {
            let grid_col = col_map.get(char_col).copied().unwrap_or(char_col);
            raw.push((a, grid_col, text, kind));
        }
    }

    let labels = hint_labels(raw.len());
    raw.into_iter()
        .zip(labels)
        .map(|((abs_row, col, text, kind), label)| Hint { label, abs_row, col, text, kind })
        .collect()
}

/// The row's text (spacers stripped) plus, for each char, its real grid column.
fn row_text(grid: &Grid, abs: usize) -> (String, Vec<usize>) {
    let mut s = String::new();
    let mut cols = Vec::new();
    for c in 0..grid.cols() {
        let cell = grid.abs_cell(abs, c);
        if cell.is_spacer() {
            continue;
        }
        s.push(cell.ch);
        cols.push(c);
    }
    (s, cols)
}

/// Extracts targets from one line, recording each one's starting column.
fn scan_line(abs: usize, line: &str, ctx: &Context, out: &mut Vec<(usize, usize, String, HintKind)>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // A run of characters that could belong to a URL / path / hash.
        if is_token(chars[i]) {
            let start = i;
            while i < chars.len() && is_token(chars[i]) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let token = token.trim_end_matches(['.', ',', ':', ')', ']', '}', '\'', '"']);
            if let Some(kind) = classify(token, ctx) {
                out.push((abs, start, token.to_string(), kind));
            }
        } else {
            i += 1;
        }
    }
}

fn is_token(c: char) -> bool {
    c.is_ascii_graphic() && !c.is_whitespace() && !"<>\"'`|()[]{}".contains(c)
}

fn classify(token: &str, ctx: &Context) -> Option<HintKind> {
    if token.len() < 3 {
        return None;
    }
    // Checked before the hash rule: a branch can legally be named `deadbeef`, and
    // the branch is the more useful target when both fit.
    if ctx.branches.iter().any(|b| b == token) {
        return Some(HintKind::Branch);
    }
    if token.starts_with("http://")
        || token.starts_with("https://")
        || token.starts_with("git@")
        || token.starts_with("ssh://")
        || token.starts_with("ftp://")
    {
        return Some(HintKind::Url);
    }
    // A git-style hex hash: 7-40 hex digits, nothing else.
    if (7..=40).contains(&token.len()) && token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(HintKind::Hash);
    }
    // A path: has a separator and starts plausibly. Not just any dotted word.
    if (token.starts_with('/') || token.starts_with("./") || token.starts_with("~/"))
        && token.contains('/')
    {
        return Some(HintKind::Path);
    }
    // A repo-relative path, which is how git names files: `src/main.rs` in a status,
    // a diff header or a build error. No filesystem check — this runs on every mouse
    // move for the hover underline, and a stat storm per motion is not worth the
    // certainty. The shape has to carry it: a separator, a file extension on the last
    // segment, and an ordinary first character, which rules out dates (21/07/2026),
    // ratios (+2/-1) and flags.
    if is_relative_path(token) {
        return Some(HintKind::Path);
    }
    None
}

fn is_relative_path(token: &str) -> bool {
    if !token.contains('/') || token.contains("://") || token.ends_with('/') {
        return false;
    }
    if !token.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    // `src/main.rs:412` and `src/main.rs:412:7` are how compilers, grep -n and git
    // grep name a place, and they are the ones worth jumping to. The suffix is
    // stripped for the shape test and put back by `split_line` when acting.
    let token = strip_line_suffix(token).0;
    let Some(last) = token.rsplit('/').next() else { return false };
    // An extension of 1-6 characters, all letters or digits: .rs, .toml, .py, .json.
    match last.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=6).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Splits a `path:line[:col]` target into the path and the line number. Returns the
/// token unchanged when there is no numeric suffix.
fn strip_line_suffix(token: &str) -> (&str, Option<u32>) {
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    // Try the two-suffix form first (`:line:col`), else the one-suffix form.
    if let Some((head, col)) = token.rsplit_once(':') {
        if numeric(col) {
            if let Some((path, line)) = head.rsplit_once(':') {
                if numeric(line) {
                    return (path, line.parse().ok());
                }
            }
            return (head, col.parse().ok());
        }
    }
    (token, None)
}

/// What the caller should do with a chosen hint.
pub enum HintAct {
    /// Put this on the clipboard (routed through the app's copy path, so a hint copy
    /// lands in the clipboard history like any other).
    Copy(String),
    /// Already handled here — the browser was launched.
    Done,
    /// Run this argv in a split. Every command produced here only READS: `git show`,
    /// `git log`, an editor on an existing file. Nothing under a hint key may change
    /// the repository, because a hint is a one-keystroke action on a target the user
    /// picked by sight, and a mis-typed label must never be able to move a branch.
    Split(Vec<String>),
}

/// What to do once a hint is chosen.
///
/// The plain action is the safe one: copy, except a URL, which opens. `alt` (the
/// label typed in UPPER CASE) is "show me this instead": a hash opens the commit, a
/// branch its log, a path its file, and a URL falls back to copying — the one case
/// where opening is already the default, so the alternate is the other thing you
/// might want.
#[must_use]
pub fn act(text: &str, kind: HintKind, alt: bool) -> HintAct {
    match (kind, alt) {
        (HintKind::Url, false) => {
            open_in_browser(text);
            HintAct::Done
        }
        (HintKind::Hash, true) => {
            HintAct::Split(vec!["git".into(), "show".into(), "--stat".into(), "--patch".into(), text.into()])
        }
        (HintKind::Branch, true) => HintAct::Split(vec![
            "git".into(),
            "log".into(),
            "--oneline".into(),
            "--graph".into(),
            "--decorate".into(),
            "-40".into(),
            text.into(),
        ]),
        (HintKind::Path, true) => {
            // `+N` is how vi, vim, neovim, nano and emacs all take a line number.
            // An editor that does not (VS Code wants `-g file:line`) simply opens
            // the file at the top, which is the harmless failure.
            let (path, line) = strip_line_suffix(text);
            match line {
                Some(n) => HintAct::Split(vec![editor(), format!("+{n}"), path.into()]),
                None => HintAct::Split(vec![editor(), path.into()]),
            }
        }
        _ => HintAct::Copy(text.to_string()),
    }
}

/// The user's editor, same precedence the scrollback dump uses.
fn editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

fn open_in_browser(url: &str) {
    // xdg-open is the portable opener on Linux; failure is silent because there is
    // nothing useful to tell the user mid-keystroke.
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(line: &str) -> Vec<(String, HintKind)> {
        scan_with(line, &[])
    }

    /// Scans with a repository context: `branches` are the local branches the pane's
    /// repo actually has.
    fn scan_with(line: &str, branches: &[String]) -> Vec<(String, HintKind)> {
        let mut out = Vec::new();
        scan_line(0, line, &Context { branches }, &mut out);
        out.into_iter().map(|(_, _, t, k)| (t, k)).collect()
    }

    #[test]
    fn finds_urls() {
        let hits = scan("see https://go2chain.es/docs and http://x.io done");
        assert!(hits.iter().any(|(t, k)| t == "https://go2chain.es/docs" && *k == HintKind::Url));
        assert!(hits.iter().any(|(t, _)| t == "http://x.io"));
    }

    #[test]
    fn strips_trailing_punctuation() {
        let hits = scan("visit (https://x.io).");
        assert_eq!(hits[0].0, "https://x.io", "the closing ). must not be part of it");
    }

    #[test]
    fn finds_paths_but_not_plain_words() {
        let hits = scan("edit /etc/hosts and ./src/main.rs but not example.com");
        let paths: Vec<_> = hits.iter().filter(|(_, k)| *k == HintKind::Path).map(|(t, _)| t.clone()).collect();
        assert!(paths.contains(&"/etc/hosts".to_string()));
        assert!(paths.contains(&"./src/main.rs".to_string()));
        assert!(!hits.iter().any(|(t, _)| t == "example.com"), "a bare domain is not a path");
    }

    #[test]
    fn a_branch_is_recognised_by_name_not_by_shape() {
        let branches = vec!["main".to_string(), "feature/git-panel".to_string()];
        let hits = scan_with("On branch feature/git-panel, ahead of main", &branches);
        assert!(hits.iter().any(|(t, k)| t == "feature/git-panel" && *k == HintKind::Branch));
        assert!(hits.iter().any(|(t, k)| t == "main" && *k == HintKind::Branch));

        // The same words with no such branches in the repo are not targets: `main`
        // is an ordinary word, and guessing would put a hint label on every prose
        // line that happens to contain it.
        let hits = scan_with("On branch feature/git-panel, ahead of main", &[]);
        assert!(!hits.iter().any(|(_, k)| *k == HintKind::Branch));
        assert!(!hits.iter().any(|(t, _)| t == "main"));
    }

    #[test]
    fn a_branch_wins_over_a_hex_reading_of_the_same_token() {
        // `deadbeef` is a legal branch name and a legal short hash. When the repo
        // has the branch, the branch is the useful target.
        let hits = scan_with("merged deadbeef today", &["deadbeef".to_string()]);
        assert_eq!(hits[0].1, HintKind::Branch);
        let hits = scan_with("merged deadbeef today", &[]);
        assert_eq!(hits[0].1, HintKind::Hash);
    }

    #[test]
    fn finds_the_repo_relative_paths_git_prints() {
        let hits = scan("        modified:   src/app_input.rs");
        assert!(hits.iter().any(|(t, k)| t == "src/app_input.rs" && *k == HintKind::Path));
        let hits = scan("error[E0061]: docs-site/src/data/features.js:708");
        assert!(hits.iter().any(|(t, k)| t == "docs-site/src/data/features.js:708" && *k == HintKind::Path));
    }

    #[test]
    fn the_relative_path_rule_does_not_swallow_ordinary_text() {
        // Each of these has a slash and no business being a hint.
        for line in ["fecha 21/07/2026", "ratio +2/-1", "and/or", "he/him", "50/50 split"] {
            let hits = scan(line);
            assert!(
                !hits.iter().any(|(_, k)| *k == HintKind::Path),
                "{line:?} must not read as a path, got {hits:?}"
            );
        }
    }

    #[test]
    fn a_line_suffix_survives_into_the_editor_command() {
        let HintAct::Split(cmd) = act("src/main.rs:412:7", HintKind::Path, true) else {
            panic!("a path's alternate action opens it");
        };
        assert_eq!(cmd[1], "+412", "the line, not the column");
        assert_eq!(cmd[2], "src/main.rs");
        // Copying keeps the whole thing: file:line is what you paste into an issue.
        assert!(matches!(act("src/main.rs:412", HintKind::Path, false),
            HintAct::Copy(t) if t == "src/main.rs:412"));
    }

    #[test]
    fn the_alternate_action_never_changes_the_repository() {
        // A hint is one keystroke on a target picked by sight. Everything reachable
        // that way must be read-only; a mistyped label must not be able to move a
        // branch or touch the working tree.
        for (kind, text) in [
            (HintKind::Hash, "8f6876d"),
            (HintKind::Branch, "main"),
            (HintKind::Path, "src/main.rs"),
        ] {
            let HintAct::Split(cmd) = act(text, kind, true) else {
                continue; // Copy is read-only by construction.
            };
            if cmd[0] == "git" {
                assert!(
                    matches!(cmd[1].as_str(), "show" | "log"),
                    "only read-only git subcommands may hang off a hint, got {cmd:?}"
                );
            }
        }
    }

    #[test]
    fn finds_git_hashes() {
        let hits = scan("commit ba4c4f3 and 28c9785f0a1b done");
        assert!(hits.iter().any(|(t, k)| t == "ba4c4f3" && *k == HintKind::Hash));
    }

    #[test]
    fn a_short_or_non_hex_word_is_not_a_hash() {
        let hits = scan("the cat sat xyz");
        assert!(hits.is_empty(), "ordinary words must not be hinted: {hits:?}");
    }

    #[test]
    fn git_ssh_remote_is_a_url() {
        let hits = scan("origin git@github.com:drheavymetal/Runnar.git");
        assert!(hits.iter().any(|(t, k)| t.starts_with("git@") && *k == HintKind::Url));
    }
}
