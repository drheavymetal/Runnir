//! Clipboard access.
//!
//! On Wayland, `arboard` opens its own Wayland connection separate from winit's,
//! which frequently fails to serve or read the selection — the copy/paste "does
//! nothing" symptom. So on Wayland we shell out to `wl-copy`/`wl-paste`, which
//! talk to the compositor the normal way, and keep `arboard` for X11 and as a
//! fallback.

use std::io::Write;
use std::process::{Command, Stdio};

pub struct Clipboard {
    /// True when `WAYLAND_DISPLAY` is set and `wl-copy` is on PATH.
    wayland: bool,
    arboard: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn new() -> Self {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some() && has("wl-copy");
        // Only build the arboard fallback when we might need it; on Wayland with
        // wl-clipboard present it is never touched.
        let arboard = if wayland { None } else { arboard::Clipboard::new().ok() };
        Self { wayland, arboard }
    }

    pub fn set(&mut self, text: &str) {
        if self.wayland && wl_copy(text, false) {
            return;
        }
        if let Some(cb) = self.arboard.as_mut() {
            let _ = cb.set_text(text.to_string());
        }
    }

    pub fn get(&mut self) -> Option<String> {
        if self.wayland {
            if let Some(text) = wl_paste(false) {
                return Some(text);
            }
        }
        self.arboard.as_mut().and_then(|cb| cb.get_text().ok())
    }

    /// Sets the PRIMARY selection (middle-click paste). Uses `wl-copy --primary` on
    /// Wayland and arboard's Linux primary-selection extension on X11.
    pub fn set_primary(&mut self, text: &str) {
        if self.wayland {
            let _ = wl_copy(text, true);
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(cb) = self.arboard.as_mut() {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            let _ = cb.set().clipboard(LinuxClipboardKind::Primary).text(text.to_string());
        }
    }

    /// Reads the PRIMARY selection, falling back to the regular clipboard so
    /// middle-click still pastes something useful when primary is unavailable.
    pub fn get_primary(&mut self) -> Option<String> {
        if self.wayland {
            if let Some(text) = wl_paste(true) {
                return Some(text);
            }
        }
        #[cfg(target_os = "linux")]
        if !self.wayland {
            if let Some(cb) = self.arboard.as_mut() {
                use arboard::{GetExtLinux, LinuxClipboardKind};
                if let Ok(text) = cb.get().clipboard(LinuxClipboardKind::Primary).text() {
                    return Some(text);
                }
            }
        }
        self.get()
    }
}

/// A bounded, in-memory ring of recently copied text, newest first.
///
/// Every copy runnir makes — selection copy, Ctrl+Shift+C, copy-mode yank, an
/// OSC 52 clipboard write from a program, copy-last-output, a hint copy — is pushed
/// here, so the clipboard-history picker can offer them for re-paste. A re-copy of
/// an entry already present moves it to the top instead of duplicating it, and the
/// oldest entry drops once capacity is exceeded.
///
/// Deliberately never persisted to disk: the clipboard routinely carries secrets
/// (passwords, tokens, private paths), so the history lives only for the session.
pub struct ClipHistory {
    entries: std::collections::VecDeque<String>,
    capacity: usize,
    enabled: bool,
}

impl ClipHistory {
    pub fn new(capacity: usize, enabled: bool) -> Self {
        Self { entries: std::collections::VecDeque::new(), capacity: capacity.max(1), enabled }
    }

    /// Records a copied entry at the front (dedup-to-top): a value already present is
    /// moved to the front rather than added again; the ring then drops its oldest
    /// entry while it exceeds capacity. Empty or whitespace-only text is ignored, and
    /// the whole call is a no-op when the history is disabled.
    pub fn push(&mut self, text: &str) {
        if !self.enabled || text.trim().is_empty() {
            return;
        }
        if let Some(pos) = self.entries.iter().position(|e| e == text) {
            self.entries.remove(pos);
        }
        self.entries.push_front(text.to_string());
        while self.entries.len() > self.capacity {
            self.entries.pop_back();
        }
    }

    /// The recorded entries, newest first.
    pub fn entries(&self) -> &std::collections::VecDeque<String> {
        &self.entries
    }

    /// Adopts capacity/enabled from a (re)loaded config, trimming the ring if the
    /// capacity shrank. Existing entries are kept even when disabling — only further
    /// recording stops.
    pub fn configure(&mut self, capacity: usize, enabled: bool) {
        self.capacity = capacity.max(1);
        self.enabled = enabled;
        while self.entries.len() > self.capacity {
            self.entries.pop_back();
        }
    }
}

fn has(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Pipes `text` into `wl-copy`. Returns whether it was launched; the process
/// detaches to keep serving the selection after we return.
fn wl_copy(text: &str, primary: bool) -> bool {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let Ok(mut child) = cmd
        .arg("--type")
        .arg("text/plain")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    // wl-copy forks a daemon and its foreground process exits at once. Reap that
    // process on a detached thread so it does not linger as a zombie — a selection
    // spawns two of these (clipboard + primary), so not reaping piles them up.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    true
}

fn wl_paste(primary: bool) -> Option<String> {
    let mut cmd = Command::new("wl-paste");
    if primary {
        cmd.arg("--primary");
    }
    let out = cmd
        .arg("--no-newline")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None // Empty clipboard makes wl-paste exit non-zero.
    }
}

#[cfg(test)]
mod tests {
    use super::ClipHistory;

    fn snapshot(h: &ClipHistory) -> Vec<String> {
        h.entries().iter().cloned().collect()
    }

    #[test]
    fn newest_first_and_skips_blank() {
        let mut h = ClipHistory::new(50, true);
        h.push("one");
        h.push("two");
        h.push("   "); // whitespace-only is ignored
        h.push("");
        assert_eq!(snapshot(&h), vec!["two".to_string(), "one".to_string()]);
    }

    #[test]
    fn evicts_oldest_past_capacity() {
        let mut h = ClipHistory::new(3, true);
        for s in ["a", "b", "c", "d", "e"] {
            h.push(s);
        }
        // Only the three newest survive, newest first.
        assert_eq!(snapshot(&h), vec!["e".to_string(), "d".to_string(), "c".to_string()]);
    }

    #[test]
    fn recopy_moves_to_top_without_duplicating() {
        let mut h = ClipHistory::new(50, true);
        h.push("a");
        h.push("b");
        h.push("c");
        h.push("a"); // re-copy an existing entry
        assert_eq!(
            snapshot(&h),
            vec!["a".to_string(), "c".to_string(), "b".to_string()],
            "re-copy must move to the top, not duplicate"
        );
        assert_eq!(h.entries().len(), 3);
    }

    #[test]
    fn disabled_records_nothing() {
        let mut h = ClipHistory::new(50, false);
        h.push("secret");
        assert!(h.entries().is_empty());
    }

    #[test]
    fn configure_shrinks_and_toggles() {
        let mut h = ClipHistory::new(50, true);
        for s in ["a", "b", "c", "d"] {
            h.push(s);
        }
        h.configure(2, false); // shrink capacity and disable
        assert_eq!(snapshot(&h), vec!["d".to_string(), "c".to_string()]);
        h.push("e"); // disabled → ignored
        assert_eq!(snapshot(&h), vec!["d".to_string(), "c".to_string()]);
    }
}
