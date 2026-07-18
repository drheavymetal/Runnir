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
