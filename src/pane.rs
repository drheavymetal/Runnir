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
    /// The command counter when the user last stepped away, for the catch-up.
    catch_up_seq: u64,
    /// Opened by the war room, so teardown knows what it may close.
    pub from_war_room: bool,
    /// The user has typed in this pane. A pane someone worked in is theirs, whoever
    /// opened it — teardown leaves it alone.
    pub touched: bool,
    /// The command counter already handed to the verb learner, so each command is
    /// counted once and never twice.
    verbs_seq: u64,
    /// The last watched word this pane saw, kept for the catch-up because the
    /// notification path consumes its own copy. Scoped to the current away window:
    /// see [`Pane::mark_catch_up_point`].
    last_watch_hit: Option<String>,
    last_bell: u64,
    /// Keyword to watch for in this pane's output (lowercased), or `None`. When set,
    /// a matching line raises a desktop notification (W4).
    watch: Option<String>,
    /// Next unscanned stable row for the watcher, so each line fires at most once.
    watch_stable: usize,
    /// Membership in the broadcast group. When any pane in a tab is a member,
    /// broadcast input goes only to members; otherwise it goes to every pane (D8).
    pub in_group: bool,
}

impl Pane {
    pub fn new(
        cols: usize,
        rows: usize,
        scrollback: usize,
        cell_px: (f32, f32),
        spawn: &Spawn,
        shell_integration: bool,
        wake: impl Fn() + Send + 'static,
    ) -> anyhow::Result<Self> {
        let mut grid = Grid::new(cols, rows);
        grid.set_scrollback_limit(scrollback);
        grid.set_cell_px(cell_px.0, cell_px.1);
        let grid = Arc::new(Mutex::new(grid));
        // Inject OSC 133/7 shell integration (env / rcfile tweaks) unless disabled or
        // the shell is unrecognised — apply() is a no-op in those cases.
        let mut spawn = spawn.clone();
        crate::shell_integration::apply(&mut spawn, shell_integration);
        let pty = Pty::spawn(grid.clone(), &spawn, wake)?;
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
            catch_up_seq: 0,
            verbs_seq: 0,
            from_war_room: false,
            touched: false,
            last_watch_hit: None,
            last_bell: 0,
            watch: None,
            watch_stable: 0,
            in_group: false,
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


    /// The facts the catch-up needs about this pane, gathered under one lock.
    ///
    /// `seen_seq` is the command counter as of the moment the user went away, which
    /// is what makes "changed while you were gone" answerable — a pane that has been
    /// sitting at a prompt reports the same number and gets no headline.
    pub fn catch_up_snapshot(&self, id: u64, waiting: bool) -> crate::catchup::Snapshot {
        let g = self.grid.lock().unwrap();
        let marked = g.command_seq() > 0;
        let running = g.command_running();
        crate::catchup::Snapshot {
            pane: id,
            // What happened here is the COMMAND, not the shell's window title: a row
            // reading "drheavymetal@host:~" tells you nothing about what you missed.
            // The running command's name wins while one runs; otherwise the last one
            // the shell marked; the pane title is the last resort, for panes with no
            // shell integration at all.
            title: self
                .running_since
                .as_ref()
                .map(|(_, name)| name.clone())
                .or_else(|| g.last_command_line().map(|c| c.trim().to_string()))
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| {
                    self.title_override.clone().unwrap_or_else(|| self.title.clone())
                }),
            // Anything that finished since we started watching, or is running now, is
            // news. A quiet prompt is not.
            changed: running || g.command_seq() > self.catch_up_seq,
            waiting,
            running,
            exit: g.last_exit(),
            secs: self.running_since.as_ref().map(|(t, _)| t.elapsed().as_secs()),
            watch_hit: self.last_watch_hit.clone(),
            last_line: g.last_nonblank_line(),
            marked,
        }
    }

    /// Hands over a command that just finished, once, with its exit code.
    ///
    /// Separate counter from `take_completion` on purpose: two consumers sharing one
    /// "have I seen this" marker means whichever polls first eats the other's event.
    pub fn take_finished_command(&mut self) -> Option<(String, i32)> {
        let g = self.grid.lock().unwrap();
        let seq = g.command_seq();
        if seq <= self.verbs_seq || g.command_running() {
            return None;
        }
        drop(g);
        self.verbs_seq = seq;
        let g = self.grid.lock().unwrap();
        Some((g.last_command_line()?, g.last_exit()?))
    }

    /// Marks the point the user stepped away from, so the next catch-up can tell
    /// what moved. Called when the away clock starts, not when the panel opens —
    /// otherwise every pane looks unchanged.
    ///
    /// The remembered watch hit goes with it: it is a fact about the absence that
    /// just ended, measured from the same instant `changed` is. Kept past here it
    /// would headline the pane "watch" for the rest of the session, and a watch
    /// outranks an exit code — so the command that failed afterwards would never be
    /// the line you read.
    pub fn mark_catch_up_point(&mut self) {
        self.catch_up_seq = self.grid.lock().unwrap().command_seq();
        self.last_watch_hit = None;
    }

    pub fn alive(&self) -> bool {
        self.pty.alive()
    }

    /// Drains OSC 52 clipboard-write requests the child made. The caller sets each
    /// on the system clipboard — the grid can't reach it from the reader thread.
    pub fn take_clipboard_writes(&mut self) -> Vec<String> {
        self.grid.lock().unwrap().take_clipboard_writes()
    }

    /// Drains OSC 9 / 99 / 777 desktop-notification requests the child made.
    pub fn take_notifications(&mut self) -> Vec<String> {
        self.grid.lock().unwrap().take_notifications()
    }

    /// Whether the grid changed since the last render (used for the tab activity
    /// badge: a background tab's grid stays dirty until it is shown again).
    pub fn is_dirty(&self) -> bool {
        self.grid.lock().unwrap().dirty
    }

    /// Exit code of this pane's most recent finished command, for the fail badge.
    pub fn last_exit(&self) -> Option<i32> {
        self.grid.lock().unwrap().last_exit()
    }

    /// How many commands have finished in this pane (OSC 133 D). The git status
    /// refresh keys off it: a repository can only change because something ran.
    pub fn command_seq(&self) -> u64 {
        self.grid.lock().unwrap().command_seq()
    }

    pub fn pty_pid(&self) -> Option<i32> {
        self.pty.pid()
    }

    /// The child's current working directory, for session persistence and for a
    /// split to inherit.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        // Prefer the shell's own OSC 7 report (portable, works on macOS); fall back
        // to the OS process query (Linux /proc) when the shell doesn't emit it.
        if let Some(dir) = self.grid.lock().unwrap().cwd() {
            return Some(dir);
        }
        crate::platform::cwd(self.pty.pid()?)
    }

    /// The last `n` non-blank lines the pane printed, prompt excluded. See
    /// [`Grid::recent_output`].
    pub fn recent_output(&self, n: usize) -> Vec<String> {
        self.grid.lock().unwrap().recent_output(n)
    }

    pub fn scrollback_text(&self) -> Vec<String> {
        self.grid.lock().unwrap().scrollback_text()
    }

    /// Everything readable about this pane's history, for a summary: the scrollback,
    /// plus the primary screen parked behind a full-screen app. A pane that has been
    /// in vim or Claude Code since early on has almost nothing in its scrollback,
    /// which is why asking to summarise it used to answer "nothing to summarise".
    pub fn history_text(&self) -> Vec<String> {
        let g = self.grid.lock().unwrap();
        let mut lines = g.parked_text();
        lines.extend(g.scrollback_text());
        lines
    }

    /// Whether a full-screen app is up, so a summary can say what it is looking at
    /// instead of silently summarising something else.
    pub fn in_full_screen_app(&self) -> bool {
        self.grid.lock().unwrap().alt_screen()
    }

    /// Whether a bell (BEL) has arrived since the last call. Drives the visual
    /// flash and window-urgency hint, once per bell.
    pub fn take_bell(&mut self) -> bool {
        let c = self.grid.lock().unwrap().bell_count;
        if c != self.last_bell {
            self.last_bell = c;
            true
        } else {
            false
        }
    }

    /// Sets (or clears, with an empty string) the keyword this pane watches for.
    /// Scanning starts from the current bottom, so pre-existing scrollback does not
    /// fire a flood of stale matches.
    pub fn set_watch(&mut self, keyword: String) {
        let kw = keyword.trim();
        // Whatever the old watch saw stops being news the moment the watch changes:
        // reporting "saw ERROR" for a word nobody is watching any more is a headline
        // about a pane the user has already moved on from.
        self.last_watch_hit = None;
        if kw.is_empty() {
            self.watch = None;
        } else {
            self.watch = Some(kw.to_lowercase());
            self.watch_stable = self.grid.lock().unwrap().watch_mark();
        }
    }

    /// Whether this pane is currently watching for a keyword.
    pub fn watching(&self) -> Option<&str> {
        self.watch.as_deref()
    }

    /// Returns a description of the first new line matching the watched keyword since
    /// the last call, or `None`. Scans only rows produced since the previous check,
    /// so each line notifies at most once.
    pub fn take_watch_hit(&mut self) -> Option<String> {
        let kw = self.watch.clone()?;
        let (text, end) = {
            let g = self.grid.lock().unwrap();
            // While a full-screen app is up the primary screen and scrollback are
            // frozen, and the watch mark is derived from the alt cursor (which moves
            // up and down as you edit). Scanning it would notify on the file's own
            // text and, because the mark can regress, re-notify every poll. Skip
            // entirely and leave watch_stable untouched, so the pre-app mark is
            // exactly right when the app exits.
            if g.alt_screen() {
                return None;
            }
            g.text_since_stable(self.watch_stable)
        };
        self.watch_stable = end;
        let hit = text
            .lines()
            .find(|l| l.to_lowercase().contains(&kw))
            .map(|l| format!("{}: {}", self.title, l.trim()));
        // Keep a copy for the catch-up. This method is a TAKING one — the
        // notification path consumes the hit — so without this the catch-up could
        // never report a watch that already fired while you were away, which is
        // exactly the case it exists for.
        if hit.is_some() {
            self.last_watch_hit = Some(kw.clone());
        }
        hit
    }

    /// Toggles folding of every finished command's output (W2): folds all if none is
    /// folded, else clears all folds.
    pub fn toggle_fold_all(&mut self) {
        let mut g = self.grid.lock().unwrap();
        if g.has_folds() {
            g.unfold_all();
        } else {
            g.fold_all();
        }
    }

    /// Toggles the fold covering the command at a given absolute (local) row.
    pub fn toggle_fold_at(&mut self, local_row: usize) {
        self.grid.lock().unwrap().toggle_fold_at(local_row);
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.pty.write(bytes);
    }

    /// Bytes a PERSON put in this pane — typed, pasted, dropped on the window, chosen
    /// from a picker, or broadcast into it from another pane.
    ///
    /// However they arrived, they make the pane theirs. A deploy pasted with its
    /// trailing newline is running exactly as much as one that was typed key by key,
    /// and counting only keystrokes is how the war room decides that pane was never
    /// used and kills the deploy taking it down.
    pub fn write_from_user(&mut self, bytes: &[u8]) {
        self.touched = true;
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

    /// Adopts a new cell size after a font zoom or a display-scale change. The grid
    /// keeps it to size inline images in cells.
    pub fn set_cell_px(&mut self, cell: (f32, f32)) {
        self.grid.lock().unwrap().set_cell_px(cell.0, cell.1);
    }

    pub fn scroll(&mut self, lines: isize) -> bool {
        self.grid.lock().unwrap().scroll_display(lines)
    }

    pub fn app_cursor(&self) -> bool {
        self.grid.lock().unwrap().app_cursor
    }

    /// Active kitty keyboard protocol flags (0 = legacy). The input layer switches
    /// key encoding on this.
    pub fn keyboard_flags(&self) -> u8 {
        self.grid.lock().unwrap().keyboard_flags()
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

    /// Text to pipe into an external command: the whole scrollback when `whole`,
    /// otherwise just the last command's OSC 133 output block (`None` if unmarked).
    pub fn pipe_text(&self, whole: bool) -> Option<String> {
        self.grid.lock().unwrap().pipe_text(whole)
    }

    /// The command line of the last finished command (OSC 133 C mark). `None` if
    /// there is no mark to anchor on.
    pub fn last_command_line(&self) -> Option<String> {
        self.grid.lock().unwrap().last_command_line()
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

    /// A pane that will sit still for the length of the test, with a real PTY behind
    /// it: the watch reads the grid, and the grid belongs to a pane.
    fn quiet_pane() -> Pane {
        let spawn = Spawn {
            command: Some(vec!["sleep".into(), "30".into()]),
            cwd: None,
            ..Default::default()
        };
        Pane::new(20, 6, 100, (8.0, 16.0), &spawn, false, || {}).expect("spawn")
    }

    /// Output as a command would produce it: the echoed command line, then what it
    /// printed. The watch starts scanning below the row the cursor was on, so a
    /// keyword typed into the first row would not be a hit — nor should it be.
    fn print(pane: &Pane, text: &str) {
        vte::Parser::new().advance(&mut *pane.grid.lock().unwrap(), text.as_bytes());
    }

    /// The war room closes the panes it opened "and only those the user never typed
    /// in", and a paste is not typing. A deploy pasted into a room pane with its
    /// trailing newline is running; if the pane still counts as untouched, `leader w q`
    /// takes the tab down and `Pty::drop` kills the deploy mid-flight.
    #[test]
    fn a_pasted_command_claims_the_pane_as_much_as_a_typed_one() {
        let mut pane = quiet_pane();
        assert!(!pane.touched, "a freshly opened pane is nobody's yet");

        // No keystroke anywhere: this is the clipboard arriving whole.
        pane.write_from_user(b"\x1b[200~./deploy.sh prod\n\x1b[201~");

        assert!(pane.touched, "the pane a command was pasted into is the user's");
    }

    /// A watch hit is news about the stretch of absence it fired in. Kept past that,
    /// a single old hit headlines the pane "watch" forever — and because a watch
    /// outranks an exit code, the command that failed afterwards is never the line
    /// the user reads.
    #[test]
    fn a_watch_hit_does_not_outlive_the_absence_it_fired_in() {
        let mut pane = quiet_pane();
        pane.set_watch("error".into());
        print(&pane, "$ deploy\r\nbuild failed: ERROR 3\r\n");

        assert!(pane.take_watch_hit().is_some(), "the watched word was printed");
        assert_eq!(
            pane.catch_up_snapshot(1, false).watch_hit.as_deref(),
            Some("error"),
            "the catch-up exists to report exactly this"
        );

        // Stepping away again opens a new window; what the last one saw is not in it.
        pane.mark_catch_up_point();
        assert_eq!(pane.catch_up_snapshot(1, false).watch_hit, None);
    }

    /// A word nobody is watching any more cannot be a headline about this pane.
    #[test]
    fn a_watch_taken_off_takes_what_it_saw_with_it() {
        let mut pane = quiet_pane();
        pane.set_watch("error".into());
        print(&pane, "$ deploy\r\nbuild failed: ERROR 3\r\n");
        assert!(pane.take_watch_hit().is_some());

        pane.set_watch(String::new());
        assert_eq!(pane.catch_up_snapshot(1, false).watch_hit, None);
        assert!(pane.watching().is_none());
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
