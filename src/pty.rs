use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::grid::Grid;

pub struct Pty {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Pty {
    /// Spawns `cmd` (or the user's `$SHELL`) on a new PTY and starts a thread that
    /// parses its output into `grid`. `on_output` fires after each chunk lands, so
    /// the caller can wake its event loop.
    pub fn spawn(
        grid: Arc<Mutex<Grid>>,
        cmd: Option<&str>,
        on_output: impl Fn() + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (cols, rows) = {
            let g = grid.lock().unwrap();
            (g.cols() as u16, g.rows() as u16)
        };

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut builder = CommandBuilder::new(&shell);
        if let Some(cmd) = cmd {
            builder.arg("-c");
            builder.arg(cmd);
        }
        // Claimed before the renderer can honour it, but every shell prompt and TUI
        // branches on this, so lying here is cheaper than being treated as dumb.
        builder.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(builder)?;
        // The child holds its own slave fd. Ours must go, or the reader never sees
        // EOF when the child exits and the thread hangs forever.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

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
        });

        Ok(Self { master: pair.master, writer, _child: child, reader: Some(thread) })
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
}
