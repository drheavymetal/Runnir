use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::grid::Grid;

/// How to start a pane's process.
#[derive(Clone, Debug, Default)]
pub struct Spawn {
    /// A program + args to run instead of the login shell. `None` runs `$SHELL`.
    pub command: Option<Vec<String>>,
    /// Directory to start in. `None` inherits the parent's.
    pub cwd: Option<std::path::PathBuf>,
}

pub struct Pty {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Option<std::thread::JoinHandle<()>>,
    /// Cleared by the reader thread when the child's output ends, i.e. it exited.
    alive: Arc<AtomicBool>,
}

impl Pty {
    /// Spawns a process on a new PTY and starts a thread parsing its output into
    /// `grid`. `on_output` fires after each chunk, so the caller can wake its loop.
    pub fn spawn(
        grid: Arc<Mutex<Grid>>,
        spawn: &Spawn,
        on_output: impl Fn() + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (cols, rows) = {
            let g = grid.lock().unwrap();
            (g.cols() as u16, g.rows() as u16)
        };

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let mut builder = match &spawn.command {
            Some(cmd) if !cmd.is_empty() => {
                let mut b = CommandBuilder::new(&cmd[0]);
                b.args(&cmd[1..]);
                b
            }
            _ => {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                CommandBuilder::new(shell)
            }
        };
        if let Some(cwd) = &spawn.cwd {
            builder.cwd(cwd);
        }
        // Claimed before the renderer can honour it, but every shell prompt and TUI
        // branches on this, so lying here is cheaper than being treated as dumb.
        // NOTE: never env_clear() — ssh reads ~/.ssh/config and the 1Password agent
        // through the inherited environment.
        builder.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(builder)?;
        // The child holds its own slave fd. Ours must go, or the reader never sees
        // EOF when the child exits and the thread hangs forever.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = alive.clone();
        let thread = std::thread::spawn(move || {
            let mut parser = vte::Parser::new();
            let mut buf = [0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // Parse the whole chunk under one lock. Locking per byte
                        // would serialise the reader against the renderer.
                        parser.advance(&mut *grid.lock().unwrap(), &buf[..n]);
                        on_output();
                    }
                }
            }
            thread_alive.store(false, Ordering::Release);
            on_output();
        });

        Ok(Self {
            master: pair.master,
            writer,
            _child: child,
            reader: Some(thread),
            alive,
        })
    }

    /// Whether the child process is still running.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// Blocks until the child exits and its output is fully parsed.
    pub fn wait(&mut self) {
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// The process group leader's pid, used to read its cwd from `/proc`.
    pub fn pid(&self) -> Option<i32> {
        self.master.process_group_leader()
    }

    /// The command name and full command line of the foreground process, read from
    /// the process group leader. This drives context tinting (ssh/sudo/docker) with
    /// no cooperation from the remote end. Linux-only; elsewhere returns `None`.
    pub fn foreground(&self) -> Option<Foreground> {
        let pid = self.master.process_group_leader()?;
        foreground_of(pid)
    }
}

/// The foreground process of a pane, as far as context detection cares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foreground {
    pub name: String,
    pub argv: Vec<String>,
}

impl Foreground {
    /// The remote host of an ssh command, if this is one. Parses the first
    /// non-flag, non-option argument as `[user@]host`.
    pub fn ssh_host(&self) -> Option<String> {
        if self.name != "ssh" {
            return None;
        }
        let mut args = self.argv.iter().skip(1);
        while let Some(arg) = args.next() {
            // Options that take a value: skip the value too.
            if matches!(arg.as_str(), "-i" | "-p" | "-l" | "-o" | "-F" | "-J" | "-b" | "-c") {
                args.next();
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            let host = arg.rsplit('@').next().unwrap_or(arg);
            return Some(host.to_string());
        }
        None
    }
}

#[cfg(target_os = "linux")]
fn foreground_of(pid: i32) -> Option<Foreground> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let argv: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Some(Foreground { name: name.trim().to_string(), argv })
}

#[cfg(not(target_os = "linux"))]
fn foreground_of(_pid: i32) -> Option<Foreground> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(argv: &[&str]) -> Foreground {
        Foreground {
            name: argv[0].rsplit('/').next().unwrap().to_string(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn ssh_host_ignores_flags_and_strips_user() {
        assert_eq!(fg(&["ssh", "drheavymetal@192.168.1.3"]).ssh_host().as_deref(), Some("192.168.1.3"));
        assert_eq!(fg(&["ssh", "-p", "22", "host"]).ssh_host().as_deref(), Some("host"));
        assert_eq!(
            fg(&["ssh", "-i", "key", "-o", "X=y", "user@box"]).ssh_host().as_deref(),
            Some("box")
        );
    }

    #[test]
    fn ssh_host_is_none_for_other_commands() {
        assert_eq!(fg(&["vim", "file"]).ssh_host(), None);
        assert_eq!(fg(&["ssh"]).ssh_host(), None, "no host given");
    }
}
