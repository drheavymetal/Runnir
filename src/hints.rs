//! Hint mode: find every URL, path and git hash on the visible screen so the user
//! can jump to one by typing a short label, no mouse involved.
//!
//! Extraction is line-based and deliberately simple — a full URL grammar is not
//! worth it when the payoff is "open the thing I can see".

use crate::grid::Grid;
use crate::overlay::{Hint, HintKind, hint_labels};

/// Finds targets on the currently visible rows of `grid`.
pub fn find(grid: &Grid) -> Vec<Hint> {
    let mut raw: Vec<(usize, usize, String, HintKind)> = Vec::new();
    for row in 0..grid.rows() {
        let abs = grid.abs_row(row);
        let line = row_text(grid, abs);
        scan_line(abs, &line, &mut raw);
    }

    let labels = hint_labels(raw.len());
    raw.into_iter()
        .zip(labels)
        .map(|((abs_row, col, text, kind), label)| Hint { label, abs_row, col, text, kind })
        .collect()
}

fn row_text(grid: &Grid, abs: usize) -> String {
    (0..grid.cols())
        .map(|c| grid.abs_cell(abs, c))
        .filter(|cell| !cell.is_spacer())
        .map(|cell| cell.ch)
        .collect()
}

/// Extracts targets from one line, recording each one's starting column.
fn scan_line(abs: usize, line: &str, out: &mut Vec<(usize, usize, String, HintKind)>) {
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
            if let Some(kind) = classify(token) {
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

fn classify(token: &str) -> Option<HintKind> {
    if token.len() < 3 {
        return None;
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
    None
}

/// What to do once a hint is chosen. URLs open in the browser; paths and hashes go
/// to the clipboard, which is the safe, useful default (you usually want to paste
/// a path into a command, not launch it).
pub fn act(text: &str, kind: HintKind, clipboard: &mut Option<arboard::Clipboard>) {
    match kind {
        HintKind::Url => open_in_browser(text),
        HintKind::Path | HintKind::Hash => {
            if let Some(cb) = clipboard.as_mut() {
                let _ = cb.set_text(text.to_string());
            }
        }
    }
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
        let mut out = Vec::new();
        scan_line(0, line, &mut out);
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
