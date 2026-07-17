use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::grid::Grid;

// Concurrency invariants (the lock hierarchy of a pane):
//
// - `grid` (Mutex<Grid>) is taken by the reader thread while parsing and by the
//   main thread for input/render/session snapshots. It is the only lock either
//   side holds while doing real work.
// - Writing to the child NEVER takes a lock and NEVER blocks: bytes are queued to
//   a dedicated writer thread through an mpsc channel. This matters twice over:
//   1. A blocking `write_all` on the main thread (paste into a child that is not
//      reading stdin) would freeze the whole UI until the child drained it.
//   2. The reader thread answers graphics queries while holding `grid`. With a
//      shared writer *mutex* (the old design) this deadlocked: main blocks in
//      `write_all` holding the writer lock → reader blocks on the writer lock and
//      stops draining the child's output → the child blocks writing output and so
//      never reads its input → main's write never completes. A cycle with no
//      timeout. A channel send cannot participate in any such cycle.

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
    /// Queue to the dedicated writer thread. Cloned into the reader thread so it
    /// can answer graphics support queries (kitty `a=q`) without taking any lock.
    /// Sends never block, so no caller can wedge on a full PTY input buffer.
    writer_tx: std::sync::mpsc::Sender<Vec<u8>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
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
        let mut writer = pair.master.take_writer()?;

        // All writes to the child go through this thread. It exits when every
        // sender is gone (the Pty dropped and the reader thread ended), which
        // then drops the writer and closes the master's write side.
        let (writer_tx, writer_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            while let Ok(bytes) = writer_rx.recv() {
                if writer.write_all(&bytes).is_err() {
                    break; // Child gone; drop the rest.
                }
                let _ = writer.flush();
            }
        });

        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = alive.clone();
        let thread_writer = writer_tx.clone();
        let thread = std::thread::spawn(move || {
            let mut parser = vte::Parser::new();
            let mut decoder = crate::graphics::Decoder::default();
            // Bytes of an image APC split across reads, prepended to the next chunk.
            let mut carry: Vec<u8> = Vec::new();
            let mut buf = [0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF: the child closed its end.
                    Ok(n) => {
                        // vte discards APC, so pull kitty graphics out of the stream
                        // first and feed only the VT bytes to the parser.
                        let chunk = if carry.is_empty() {
                            buf[..n].to_vec()
                        } else {
                            let mut c = std::mem::take(&mut carry);
                            c.extend_from_slice(&buf[..n]);
                            c
                        };
                        let (vt, cmds, rem) = crate::graphics::split(&chunk);
                        // Bound the carry: an unterminated APC (no ST) would grow it
                        // without limit as a program streams bytes. Past the cap,
                        // give up on the sequence and flush it to vte rather than
                        // buffering forever.
                        const MAX_CARRY: usize = 16 * 1024 * 1024;
                        carry = if rem.len() > MAX_CARRY {
                            parser.advance(&mut *grid.lock().unwrap(), &rem);
                            Vec::new()
                        } else {
                            rem
                        };
                        let replies = {
                            let mut g = grid.lock().unwrap();
                            parser.advance(&mut *g, &vt);
                            for cmd in cmds {
                                match decoder.feed(cmd) {
                                    crate::graphics::Event::Show(img) => g.place_image(img),
                                    crate::graphics::Event::Delete { all, id } => {
                                        g.clear_images(all, id)
                                    }
                                    crate::graphics::Event::Query(id) => {
                                        // Answer "OK" so icat & co. know inline
                                        // images work and proceed to send them.
                                        // A channel send: safe under the grid
                                        // lock, can never block or deadlock.
                                        let _ = thread_writer
                                            .send(crate::graphics::respond(id));
                                    }
                                    crate::graphics::Event::None => {}
                                }
                            }
                            // Terminal query replies (DA1/DA2/DSR) the parser queued.
                            g.take_responses()
                        };
                        for reply in replies {
                            let _ = thread_writer.send(reply);
                        }
                        on_output();
                    }
                    // A signal can interrupt the read; that is not the child
                    // exiting, so retry rather than declaring it dead.
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            thread_alive.store(false, Ordering::Release);
            on_output();
        });

        Ok(Self {
            master: pair.master,
            writer_tx,
            child,
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

    /// Queues bytes for the child. Never blocks — the writer thread does the
    /// actual (possibly blocking) write, so a child that stops reading its input
    /// can never freeze the UI or the output-reader thread.
    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer_tx.send(bytes.to_vec());
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

impl Drop for Pty {
    fn drop(&mut self) {
        // Kill and reap the child. Nothing else ever waits on it, so without the
        // `wait` every closed pane (and every shell that exited on its own) would
        // leave a zombie in the process table for the life of the terminal. The
        // kill makes the wait prompt when the pane is closed while the child is
        // still running; on an already-exited child it is a no-op.
        let _ = self.child.kill();
        let _ = self.child.wait();
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
            // Options that take a value: skip the value too, or the tunnel spec of
            // `-L 8080:host:80` would be misread as the host. This is every
            // value-taking ssh flag, so an unlisted one cannot swallow the host.
            if matches!(
                arg.as_str(),
                "-b" | "-c" | "-D" | "-E" | "-e" | "-F" | "-I" | "-i" | "-J" | "-L" | "-l"
                    | "-m" | "-O" | "-o" | "-p" | "-Q" | "-R" | "-S" | "-W" | "-w"
            ) {
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
    fn ssh_host_skips_tunnel_specs() {
        // Regression: value-taking flags like -L/-R/-D used to leak their value as
        // the host.
        assert_eq!(fg(&["ssh", "-L", "8080:localhost:80", "box"]).ssh_host().as_deref(), Some("box"));
        assert_eq!(fg(&["ssh", "-D", "1080", "proxy"]).ssh_host().as_deref(), Some("proxy"));
        assert_eq!(
            fg(&["ssh", "-R", "9000:localhost:9000", "-L", "80:x:80", "user@h"]).ssh_host().as_deref(),
            Some("h")
        );
    }

    #[test]
    fn ssh_host_is_none_for_other_commands() {
        assert_eq!(fg(&["vim", "file"]).ssh_host(), None);
        assert_eq!(fg(&["ssh"]).ssh_host(), None, "no host given");
    }

    #[test]
    fn write_never_blocks_even_when_the_child_ignores_stdin() {
        // Regression: writes used to run synchronously under a shared mutex on
        // the caller's thread. A paste larger than the kernel PTY input buffer
        // into a child that does not read stdin blocked the UI thread — and,
        // because the reader thread answers graphics queries through the same
        // writer while holding the grid lock, could deadlock the whole process
        // (main holds writer waiting on the child; reader waits on writer and
        // stops draining; child blocks on output and never reads input).
        let grid = Arc::new(Mutex::new(Grid::new(20, 5)));
        let spawn = Spawn { command: Some(vec!["sleep".into(), "30".into()]), cwd: None };
        let mut pty = Pty::spawn(grid, &spawn, || {}).expect("spawn");
        let big = vec![b'x'; 1 << 20]; // far beyond any PTY input buffer
        let start = std::time::Instant::now();
        pty.write(&big);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "write must queue asynchronously, never block on the child"
        );
        // Dropping the pty kills and reaps the sleeping child.
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_dropped_pty_reaps_its_child() {
        // Regression: the child was never killed nor waited on, so every closed
        // pane left a zombie in the process table for the life of the terminal.
        let grid = Arc::new(Mutex::new(Grid::new(20, 5)));
        let spawn = Spawn { command: Some(vec!["true".into()]), cwd: None };
        let mut pty = Pty::spawn(grid, &spawn, || {}).expect("spawn");
        let pid = pty.child.process_id().expect("child pid") as i32;
        pty.wait(); // the child exits on its own; the reader sees EOF
        drop(pty); // must reap
        // If the pid still exists it must not be a zombie. (The pid could have
        // been recycled by an unrelated live process; only state Z is the bug.)
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            assert!(!stat.contains(") Z"), "child left as a zombie: {stat}");
        }
    }
}
