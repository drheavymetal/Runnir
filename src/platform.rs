//! Per-OS process/desktop bits, abstracted so the rest of the code is portable
//! across Linux and macOS. Linux reads `/proc`; macOS uses `libproc` + a small
//! `KERN_PROCARGS2` sysctl. Each function degrades to `None`/no-op on an unsupported
//! platform rather than failing to build.

use std::path::PathBuf;

/// The working directory of process `pid`, for the status bar and split inheritance.
pub fn cwd(pid: i32) -> Option<PathBuf> {
    imp::cwd(pid)
}

/// The foreground process's name and full argv, for the SSH/root/docker context tint
/// and the tab title/icon. `None` when it can't be read.
pub fn foreground(pid: i32) -> Option<(String, Vec<String>)> {
    imp::foreground(pid)
}

/// Raises a desktop notification (long-command done, keyword watch). Best-effort.
pub fn notify(body: &str) {
    imp::notify(body)
}

// ---- Linux -----------------------------------------------------------------

#[cfg(target_os = "linux")]
mod imp {
    use super::PathBuf;

    pub fn cwd(pid: i32) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub fn foreground(pid: i32) -> Option<(String, Vec<String>)> {
        let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let argv: Vec<String> = raw
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        Some((name.trim().to_string(), argv))
    }

    pub fn notify(body: &str) {
        // `--` so a body starting with '-' is not eaten as an option.
        let _ = std::process::Command::new("notify-send")
            .arg("--")
            .arg("runnir")
            .arg(body)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

// ---- macOS -----------------------------------------------------------------

#[cfg(target_os = "macos")]
mod imp {
    use super::PathBuf;

    pub fn cwd(_pid: i32) -> Option<PathBuf> {
        // libproc 0.14's pidcwd is a stub on macOS (returns Err), and replicating
        // proc_pidinfo(PROC_PIDVNODEPATHINFO) here is fragile. macOS relies on the
        // shell's OSC 7 report instead (Pane::cwd prefers it), so this is None.
        None
    }

    pub fn foreground(pid: i32) -> Option<(String, Vec<String>)> {
        use libproc::proc_pid;
        let name = proc_pid::name(pid).ok()?;
        // Full argv needs KERN_PROCARGS2; fall back to just the name if it fails, so
        // "ssh"/"sudo"/"docker" context detection still works even without the host.
        let argv = proc_argv(pid).unwrap_or_else(|| vec![name.clone()]);
        Some((name, argv))
    }

    /// Reads `argv` of `pid` via `sysctl(KERN_PROCARGS2)`. The buffer is:
    /// `[argc: i32][exec_path\0][\0 padding][argv[0]\0 argv[1]\0 ...]`.
    fn proc_argv(pid: i32) -> Option<Vec<String>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size: libc::size_t = 0;
        // First call: get the required buffer size.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buf.as_mut_ptr() as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size < 4 {
            return None;
        }
        buf.truncate(size);
        let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        // Guard the count before allocating: a bogus negative value cast to usize is
        // enormous and would abort the process on the with_capacity below.
        if argc <= 0 {
            return None;
        }
        let argc = (argc as usize).min(4096);
        // Skip argc, then the exec path and its trailing NULs.
        let mut i = 4;
        while i < buf.len() && buf[i] != 0 {
            i += 1;
        }
        while i < buf.len() && buf[i] == 0 {
            i += 1;
        }
        // Now argc NUL-terminated strings.
        let mut argv = Vec::with_capacity(argc);
        while argv.len() < argc && i < buf.len() {
            let start = i;
            while i < buf.len() && buf[i] != 0 {
                i += 1;
            }
            argv.push(String::from_utf8_lossy(&buf[start..i]).into_owned());
            i += 1;
        }
        (!argv.is_empty()).then_some(argv)
    }

    pub fn notify(body: &str) {
        // `display notification` via AppleScript — no extra binary to install.
        let script = format!(
            "display notification {} with title \"runnir\"",
            applescript_quote(body)
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    fn applescript_quote(s: &str) -> String {
        // A double-quoted AppleScript literal can't hold a raw newline; escape them.
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        format!("\"{escaped}\"")
    }
}

// ---- other (never built for now, keeps the crate portable) -----------------

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::PathBuf;
    pub fn cwd(_pid: i32) -> Option<PathBuf> {
        None
    }
    pub fn foreground(_pid: i32) -> Option<(String, Vec<String>)> {
        None
    }
    pub fn notify(_body: &str) {}
}
