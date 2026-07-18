//! Reads the user's shell history for the "insert from history" picker (D3).
//!
//! Three formats are handled: fish (`~/.local/share/fish/fish_history`, a small YAML
//! subset), zsh extended history (`: <time>:<dur>;<cmd>`) and plain bash. The picker
//! only ever inserts a chosen line at the prompt — it never runs it — so a best-effort
//! parse is fine: a mangled entry is a slightly odd suggestion, not a hazard.

use std::collections::HashSet;
use std::path::PathBuf;

/// Recent shell history, most-recent-first and de-duplicated. Empty if no history
/// file is found or readable.
pub fn recent(limit: usize) -> Vec<String> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return Vec::new(),
    };
    let shell = std::env::var("SHELL").unwrap_or_default();

    // Prefer the current shell's history, but fall back to whatever exists.
    let candidates: [(&str, PathBuf); 3] = [
        ("fish", home.join(".local/share/fish/fish_history")),
        ("zsh", home.join(".zsh_history")),
        ("bash", home.join(".bash_history")),
    ];
    let ordered = order_by_shell(&shell, candidates);

    for (kind, path) in ordered {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        // zsh "metafies" bytes >= 0x80 (0x83 marker, next byte XOR 0x20), which makes
        // the file invalid UTF-8; un-metafy before a lossy decode so non-ASCII
        // history is not silently dropped.
        let raw = if kind == "zsh" {
            String::from_utf8_lossy(&unmetafy(&bytes)).into_owned()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let mut cmds = match kind {
            "fish" => parse_fish(&raw),
            "zsh" => parse_zsh(&raw),
            _ => parse_bash(&raw),
        };
        if cmds.is_empty() {
            continue;
        }
        // History files are oldest-first; the picker wants newest first.
        cmds.reverse();
        return dedup_keep_order(cmds, limit);
    }
    Vec::new()
}

fn order_by_shell(shell: &str, mut c: [(&'static str, PathBuf); 3]) -> Vec<(&'static str, PathBuf)> {
    // Put the running shell's file first without dropping the others.
    let key = if shell.contains("fish") {
        0
    } else if shell.contains("zsh") {
        1
    } else {
        2
    };
    c.rotate_left(key);
    c.into_iter().collect()
}

fn parse_fish(raw: &str) -> Vec<String> {
    // Entries look like:  "- cmd: git status\n  when: 1700000000".
    raw.lines()
        .filter_map(|l| l.trim_start().strip_prefix("- cmd: "))
        .map(unescape_fish)
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn unescape_fish(s: &str) -> String {
    // fish escapes backslashes and newlines in the YAML value. A single left-to-right
    // pass is required: sequential `replace` calls mis-decode `\\n` (escaped
    // backslash + n) into a backslash + real newline.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_zsh(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|l| {
            // Extended format: ": 1700000000:0;the command". Strip the metadata.
            match l.strip_prefix(": ") {
                Some(rest) => rest.splitn(2, ';').nth(1).unwrap_or(rest).to_string(),
                None => l.to_string(),
            }
        })
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// Reverses zsh's metafication: a 0x83 byte marks the next byte as XOR-0x20.
fn unmetafy(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut it = bytes.iter().copied();
    while let Some(b) = it.next() {
        if b == 0x83 {
            if let Some(n) = it.next() {
                out.push(n ^ 0x20);
            }
        } else {
            out.push(b);
        }
    }
    out
}

fn parse_bash(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn dedup_keep_order(cmds: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in cmds {
        if seen.insert(c.clone()) {
            out.push(c);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fish_entries() {
        let raw = "- cmd: git status\n  when: 1\n- cmd: cargo build\n  when: 2\n";
        assert_eq!(parse_fish(raw), vec!["git status", "cargo build"]);
    }

    #[test]
    fn fish_unescape_is_single_pass() {
        // A real newline in a multiline command: fish stores it as backslash-n.
        assert_eq!(unescape_fish("line1\\nline2"), "line1\nline2");
        // An escaped backslash followed by n must NOT become a newline.
        assert_eq!(unescape_fish("printf '\\\\n'"), "printf '\\n'");
    }

    #[test]
    fn unmetafy_restores_high_bytes() {
        // 0x83 marks the next byte as XOR 0x20: 0x83,0xA9 -> 0xA9^0x20 = 0x89.
        assert_eq!(unmetafy(&[b'a', 0x83, 0xA9, b'b']), vec![b'a', 0x89, b'b']);
        // A plain byte stream is unchanged.
        assert_eq!(unmetafy(b"plain"), b"plain".to_vec());
    }

    #[test]
    fn parses_zsh_extended_and_plain() {
        assert_eq!(parse_zsh(": 1700:0;ls -la"), vec!["ls -la"]);
        assert_eq!(parse_zsh("plain cmd"), vec!["plain cmd"]);
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let v = vec!["a".into(), "b".into(), "a".into(), "c".into()];
        assert_eq!(dedup_keep_order(v, 10), vec!["a", "b", "c"]);
    }

    #[test]
    fn respects_the_limit() {
        let v = (0..100).map(|i| i.to_string()).collect();
        assert_eq!(dedup_keep_order(v, 5).len(), 5);
    }
}
