//! A single terminal pane: its grid, its child process, and its selection.
//!
//! A pane is the unit the layout tree arranges and the renderer draws. It owns an
//! `Arc<Mutex<Grid>>` because the PTY reader thread writes the grid while the main
//! thread reads it.

use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::grid::Grid;
use crate::pty::{Foreground, Pty, Spawn};
use crate::selection::{Mode as SelMode, Point, Selection};

/// The "world" a pane is in, inferred from its foreground process. Drives the
/// background tint so a remote or root shell is unmistakable.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Context {
    #[default]
    Local,
    Ssh(String),
    Root,
    Docker,
}

pub struct Pane {
    pub grid: Arc<Mutex<Grid>>,
    pub pty: Pty,
    pub selection: Option<Selection>,
    pub selecting: bool,
    pub context: Context,
    /// Title the pane shows: OSC-set title, else the foreground process name.
    pub title: String,
    /// Set by the user; overrides the automatic title.
    pub title_override: Option<String>,
    /// When the currently-running command started, and its name, for completion
    /// notifications. Tracked via OSC 133 marks when available, else the foreground.
    running_since: Option<(std::time::Instant, String)>,
    last_command_seq: u64,
}

impl Pane {
    pub fn new(
        cols: usize,
        rows: usize,
        scrollback: usize,
        cell_px: (f32, f32),
        spawn: &Spawn,
        wake: impl Fn() + Send + 'static,
    ) -> anyhow::Result<Self> {
        let mut grid = Grid::new(cols, rows);
        grid.set_scrollback_limit(scrollback);
        grid.set_cell_px(cell_px.0, cell_px.1);
        let grid = Arc::new(Mutex::new(grid));
        let pty = Pty::spawn(grid.clone(), spawn, wake)?;
        Ok(Self {
            grid,
            pty,
            selection: None,
            selecting: false,
            context: Context::Local,
            title: "shell".into(),
            title_override: None,
            running_since: None,
            last_command_seq: 0,
        })
    }

    /// Returns a message if a command that ran at least `threshold` seconds just
    /// finished. Uses the OSC 133 command counter so it fires once per command, and
    /// only for commands long enough to be worth interrupting for.
    pub fn take_completion(&mut self, threshold: u64) -> Option<String> {
        let (seq, running) = {
            let g = self.grid.lock().unwrap();
            (g.command_seq(), g.command_running())
        };
        let mut done = None;
        // Check completion FIRST. Doing this after "start tracking" would let a C
        // (new command) and a D (previous finished) landing in the same poll
        // consume the just-set timer against the old command, restarting the new
        // command's clock a poll late and undercounting it.
        if seq > self.last_command_seq {
            self.last_command_seq = seq;
            if let Some((started, name)) = self.running_since.take() {
                let secs = started.elapsed().as_secs();
                if secs >= threshold {
                    done = Some(format!("{name} finished after {secs}s"));
                }
            }
        }
        // Then start tracking a newly-running command.
        if running && self.running_since.is_none() {
            self.running_since = Some((std::time::Instant::now(), self.title.clone()));
        } else if !running {
            self.running_since = None;
        }
        done
    }

    pub fn alive(&self) -> bool {
        self.pty.alive()
    }

    pub fn pty_pid(&self) -> Option<i32> {
        self.pty.pid()
    }

    /// The child's current working directory, for session persistence and for a
    /// split to inherit.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        let pid = self.pty.pid()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub fn scrollback_text(&self) -> Vec<String> {
        self.grid.lock().unwrap().scrollback_text()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.pty.write(bytes);
    }

    /// Snaps the view to the live output. Any keystroke should trigger this so
    /// typing while scrolled back is not silently swallowed.
    pub fn snap_to_bottom(&mut self) -> bool {
        self.grid.lock().unwrap().scroll_to_bottom()
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.lock().unwrap().resize(cols, rows);
        self.pty.resize(cols as u16, rows as u16);
    }

    pub fn scroll(&mut self, lines: isize) -> bool {
        self.grid.lock().unwrap().scroll_display(lines)
    }

    pub fn app_cursor(&self) -> bool {
        self.grid.lock().unwrap().app_cursor
    }

    pub fn bracketed_paste(&self) -> bool {
        self.grid.lock().unwrap().bracketed_paste
    }

    pub fn begin_selection(&mut self, at: Point, mode: SelMode) {
        self.selection = Some(Selection::new(at, mode));
        self.selecting = true;
    }

    pub fn update_selection(&mut self, to: Point) -> bool {
        if let Some(sel) = self.selection.as_mut() {
            sel.update(to);
            true
        } else {
            false
        }
    }

    pub fn end_selection(&mut self) {
        self.selecting = false;
    }

    pub fn clear_selection(&mut self) -> bool {
        self.selecting = false;
        self.selection.take().is_some()
    }

    pub fn selection_text(&self) -> Option<String> {
        let sel = self.selection?;
        let grid = self.grid.lock().unwrap();
        if sel.is_empty(&grid) {
            return None;
        }
        Some(sel.text(&grid))
    }

    /// Text of the output of the last command, using OSC 133 marks when the shell
    /// emits them. `None` if there is no mark to anchor on.
    pub fn last_command_output(&self) -> Option<String> {
        self.grid.lock().unwrap().last_command_output()
    }

    /// Recomputes the pane's context and title from its foreground process. Cheap
    /// enough (two `/proc` reads) to call on a timer.
    pub fn refresh_context(&mut self, config: &Config) {
        let osc_title = { self.grid.lock().unwrap().title.clone() };

        let fg = self.pty.foreground();
        let (context, proc_title) = match &fg {
            Some(f) => (classify(f), f.name.clone()),
            None => (Context::Local, "shell".into()),
        };

        self.context = if config.behaviour.context_tint { context } else { Context::Local };
        self.title = self
            .title_override
            .clone()
            .or_else(|| if osc_title.is_empty() { None } else { Some(osc_title) })
            .unwrap_or(proc_title);
    }
}

fn classify(fg: &Foreground) -> Context {
    if let Some(host) = fg.ssh_host() {
        return Context::Ssh(host);
    }
    match fg.name.as_str() {
        "sudo" | "su" | "root" | "doas" => Context::Root,
        "docker" | "podman" | "kubectl" | "distrobox" => Context::Docker,
        _ => Context::Local,
    }
}

impl Context {
    /// The tint blended over the pane background, or `None` for local. Derived from
    /// the host name so a given server is always the same shade everywhere, with no
    /// configuration.
    pub fn tint(&self) -> Option<(u8, u8, u8)> {
        match self {
            Context::Local => None,
            Context::Root => Some((70, 20, 20)),
            Context::Docker => Some((20, 35, 60)),
            Context::Ssh(host) => Some(host_colour(host)),
        }
    }

    pub fn label(&self) -> Option<String> {
        match self {
            Context::Local => None,
            Context::Root => Some("root".into()),
            Context::Docker => Some("docker".into()),
            Context::Ssh(host) => Some(format!("ssh {host}")),
        }
    }
}

/// A deterministic dim tint from a host name: same host, same colour, on every
/// machine, with nothing to configure. Hued but dark, so text stays readable.
fn host_colour(host: &str) -> (u8, u8, u8) {
    let mut hash: u64 = 1469598103934665603;
    for b in host.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    // Hue from the hash; fixed low value and saturation keep it a background tint.
    let hue = (hash % 360) as f32;
    hsv_to_rgb(hue, 0.55, 0.28)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_context_labels_the_host() {
        let ctx = Context::Ssh("192.168.1.3".into());
        assert_eq!(ctx.label().as_deref(), Some("ssh 192.168.1.3"));
        assert!(ctx.tint().is_some());
    }

    #[test]
    fn local_context_has_no_tint() {
        assert_eq!(Context::Local.tint(), None);
        assert_eq!(Context::Local.label(), None);
    }

    #[test]
    fn a_host_always_gets_the_same_colour() {
        // The whole point of deriving from the name: reproducible across machines.
        assert_eq!(host_colour("192.168.1.3"), host_colour("192.168.1.3"));
        assert_ne!(host_colour("192.168.1.3"), host_colour("192.168.1.7"));
    }

    #[test]
    fn host_tints_stay_dark_enough_to_read_on() {
        // A tint brighter than the text would drown it. Value is capped at 0.28.
        for host in ["a", "reports.cromowin.com", "192.168.1.188", "cloudmax"] {
            let (r, g, b) = host_colour(host);
            assert!(r < 130 && g < 130 && b < 130, "{host} tint too bright: {r},{g},{b}");
        }
    }

    #[test]
    fn classify_reads_the_foreground() {
        let ssh = Foreground { name: "ssh".into(), argv: vec!["ssh".into(), "box".into()] };
        assert_eq!(classify(&ssh), Context::Ssh("box".into()));
        let sudo = Foreground { name: "sudo".into(), argv: vec!["sudo".into()] };
        assert_eq!(classify(&sudo), Context::Root);
        let sh = Foreground { name: "fish".into(), argv: vec!["fish".into()] };
        assert_eq!(classify(&sh), Context::Local);
    }
}
