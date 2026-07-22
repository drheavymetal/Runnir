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

/// The user's editor as an argv, for every "open this file in an editor" key.
///
/// `$VISUAL` wins over `$EDITOR` — the long-standing convention, `$VISUAL` being the
/// full-screen one — and either may carry arguments (`"code -w"` is a legal `$EDITOR`),
/// so the value is split rather than treated as a bare program name. What is set is
/// trusted: it may name a shell function or an alias that only the user's shell can
/// resolve, and second-guessing it would override an explicit choice.
///
/// With neither set we probe `$PATH` instead of assuming `vi`. A terminal launched
/// from the compositor inherits the compositor's environment, which usually carries
/// no editor at all, and on a distro without `vi` installed that assumption sends a
/// command the user's own shell then rejects. `None` means nothing was found, so the
/// caller can say so rather than run something that cannot work.
pub fn editor_argv() -> Option<Vec<String>> {
    if let Some(argv) = env_editor() {
        return Some(argv);
    }
    // Newest-first, then the traditional names. `nano` outranks `vi` because a user
    // who set nothing is likelier to want the editor that says how to quit.
    ["nvim", "vim", "nano", "vi", "emacs"]
        .into_iter()
        .find(|c| on_path(c))
        .map(|c| vec![c.to_string()])
}

/// `$VISUAL`, else `$EDITOR`, split into an argv. A variable set to the empty string
/// (or to blanks) is not a setting — it is an unset variable that survived an export.
fn env_editor() -> Option<Vec<String>> {
    ["VISUAL", "EDITOR"].into_iter().find_map(|var| {
        let value = std::env::var(var).ok()?;
        let argv: Vec<String> = value.split_whitespace().map(str::to_string).collect();
        (!argv.is_empty()).then_some(argv)
    })
}

/// Whether `name` resolves to an executable file on `$PATH`. A name containing a
/// separator is a path already and is checked where it stands.
fn on_path(name: &str) -> bool {
    if name.contains('/') {
        return is_executable(std::path::Path::new(name));
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name)))
}

fn is_executable(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The env vars are process-global, so these tests never touch them: they cover
    /// the PATH probe, which is the part that decides what an unconfigured machine
    /// gets. `sh` is on PATH everywhere this builds.
    #[test]
    fn on_path_finds_a_real_program() {
        assert!(on_path("sh"));
        assert!(!on_path("runnir-definitely-not-a-program"));
    }

    #[test]
    fn on_path_rejects_a_directory() {
        // A dir named like a program must not count as one — `metadata` succeeds on
        // it and the executable bit is set, so only the is_file check saves us.
        assert!(!on_path("/tmp"));
        assert!(!is_executable(std::path::Path::new("/")));
    }

    #[test]
    fn an_absolute_name_is_checked_where_it_stands() {
        assert!(on_path("/bin/sh") || on_path("/usr/bin/sh"));
        assert!(!on_path("/nonexistent/sh"));
    }

    /// Whatever the machine has, the fallback is never a bare name that is not
    /// installed — that was the `vi` bug: a command the user's shell then rejects.
    #[test]
    fn the_fallback_is_something_that_exists() {
        if let Some(argv) = editor_argv() {
            assert!(!argv.is_empty());
            if std::env::var_os("VISUAL").is_none() && std::env::var_os("EDITOR").is_none() {
                assert!(on_path(&argv[0]), "picked {:?}, which is not on PATH", argv[0]);
            }
        }
    }
}
