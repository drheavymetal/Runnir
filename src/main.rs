mod actions;
mod ai;
mod boxdraw;
mod clipboard;
mod config;
mod control;
mod dnd;
mod docs;
mod font;
mod git;
mod graphics;
mod grid;
mod guardian;
mod history;
mod hints;
mod keys;
mod layout;
mod media;
mod mouse;
mod overlay;
mod pane;
mod platform;
mod project_session;
mod pty;
mod render;
mod selection;
mod session;
mod settings;
mod shell_integration;
mod tab;
mod themes;
mod watch;
mod whisper;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::actions::{Action, Chord, Keymap, LeaderNode};
use crate::config::Config;
use crate::grid::{Color, Grid, Pen};
use crate::font::FontAtlas;
use crate::layout::{Axis, Rect};
use crate::overlay::{Overlay, Palette, Prompt, PromptKind};
use crate::pty::Spawn;
use crate::render::{Overlay as OverlayDraw, PaneDraw, Renderer};
use crate::selection::Mode as SelMode;
use crate::tab::Tab;

/// Height of the tab bar, in cells. Shown only when more than one tab exists.
const TABBAR_ROWS: f32 = 1.0;

/// How long each leader step waits for the next key, from the config. `None` when
/// `leader_timeout = 0`: the layer then stays armed until an action, a miss or
/// Escape, the way a tmux prefix does.
pub fn leader_timeout(config: &Config) -> Option<Duration> {
    (config.leader_timeout > 0).then(|| Duration::from_secs(config.leader_timeout))
}

/// Width of the minimap strip, in pixels. Like the tab bar, this is chrome that
/// overlaps the pane, so the text grid must reserve it — see `tab::cells_in`.
pub const MINIMAP_W: f32 = 46.0;

/// A message from a background worker back to the UI thread.
pub enum UserEvent {
    Ai(ai::Reply),
    /// A PTY produced output. On Wayland, `Window::request_redraw` from another
    /// thread does not reliably interrupt `ControlFlow::Wait`; sending a user event
    /// through the proxy does. Without this, echoed input and command output appear
    /// only on the next keystroke or blink tick — the "typing feels laggy" bug.
    Redraw,
    /// A remote-control request from the socket thread, paired with a one-shot
    /// channel to send the response back. The UI thread runs it against `Gpu` and
    /// replies; the socket thread waits (bounded) on the other end. This is the only
    /// safe cross-thread path to the terminal state — same reasoning as `Redraw`.
    Control(control::ControlRequest, std::sync::mpsc::Sender<control::ControlResponse>),
    /// Files dropped onto the window under Wayland, with the surface-logical
    /// coordinates of the drop. Comes from the `dnd` thread, which is the only
    /// place Wayland drag-and-drop exists — winit has none.
    FilesDropped(Vec<std::path::PathBuf>, f64, f64),
    /// A now-playing update from a media worker: fetched metadata or a waveform frame.
    /// Delivered off the UI thread via the proxy, same wake pattern as `Ai`, so the
    /// playerctl / cava subprocess never blocks rendering.
    Media(media::MediaMsg),
    /// Repository state for a repo root, from a `git status` worker. `None` when the
    /// command failed or the directory stopped being a repository. Never computed on
    /// the UI thread: in a large repository `git status` takes seconds.
    Git(PathBuf, Option<git::RepoState>),
    /// Data or a command result for the git panel, tagged with the request sequence
    /// so a stale preview never paints over a newer one.
    GitPanel(u64, git::PanelMsg),
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // `runnir @ <cmd> [flags]` — the remote-control client; talks to a running
        // terminal over its Unix socket and never opens a window.
        Some("@") => return control::client_main(&args[2..]),
        Some("--dump") => return dump(args.get(2).map(String::as_str).unwrap_or("echo hola")),
        Some("--render") => {
            let path = args.get(2).map(String::as_str).unwrap_or("/tmp/runnir.png");
            let cmd = args.get(3).map(String::as_str).unwrap_or("echo hola");
            let delay = args.get(4).and_then(|s| s.parse().ok());
            return render::offscreen(path, cmd, 16.0, delay);
        }
        Some("--write-config") => {
            let path = Config::path();
            match Config::write_default(&path) {
                Ok(()) => println!("runnir: wrote {}", path.display()),
                Err(e) => eprintln!("runnir: could not write config: {e}"),
            }
            return;
        }
        Some("--version" | "-v") => return println!("runnir {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h") => return print_help(),
        Some("--demo") => {
            let path = args.get(2).map(String::as_str).unwrap_or("/tmp/runnir-demo.png");
            // A third argument names the leader level to draw the which-key panel
            // for: "" is the root, "t" the tabs group, and so on. Without it the
            // scene is the plain multi-pane one with the palette open.
            return match args.get(3).map(String::as_str) {
                Some(level) => leader_scene(path, level),
                None => demo_scene(path),
            };
        }
        _ => {}
    }

    let quake = args.iter().any(|a| a == "--quake");
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    // Wait, not Poll: an idle terminal must not burn a core.
    event_loop.set_control_flow(ControlFlow::Wait);
    // Start the remote-control listener before the loop spawns the first pane, so
    // that pane (and every later one) inherits RUNNIR_LISTEN in its environment.
    control::start_listener(event_loop.create_proxy());
    let mut app = App::new(event_loop.create_proxy(), quake);
    event_loop.run_app(&mut app).unwrap();
    // Clean up our own socket on a graceful exit (best effort).
    let _ = std::fs::remove_file(control::socket_path());
}

fn print_help() {
    println!(
        "runnir {} — a GPU terminal emulator\n\n\
         USAGE:\n  \
         runnir                     start the terminal\n  \
         runnir --write-config      write a default config file\n  \
         runnir --dump CMD          run CMD, print the resulting grid (debug)\n  \
         runnir --render OUT CMD    render CMD's output to a PNG (debug)\n  \
         runnir @ CMD [flags]       remote-control a running terminal\n\n\
         Remote control (like kitty @): ls, send-text, get-text, focus-tab,\n  \
         launch, new-tab, close-tab, set-colors. Example: runnir @ send-text --text 'ls\\n'\n\n\
         Press F1 inside runnir for the full key reference.",
        env!("CARGO_PKG_VERSION")
    );
}

fn dump(cmd: &str) {
    let grid = Arc::new(Mutex::new(Grid::new(80, 24)));
    let spawn = Spawn {
        command: Some(vec![
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
            "-c".into(),
            cmd.into(),
        ]),
        cwd: None,
        ..Default::default()
    };
    let mut pty = pty::Pty::spawn(grid.clone(), &spawn, || {}).expect("pty");
    pty.wait();
    let grid = grid.lock().unwrap();
    println!("{}", grid.dump());
}

/// Builds a static multi-pane scene with an overlay and renders it, so the layout,
/// tinting, focus dimming and overlay path can be verified without a live window.
fn demo_scene(path: &str) {
    use crate::render::Rect;
    render::offscreen_scene(path, 1000, 600, 16.0, |r| {
        let (cw, ch) = r.cell_size();
        let cells = |rect: Rect| {
            ((rect.w / cw).floor().max(1.0) as usize, (rect.h / ch).floor().max(1.0) as usize)
        };
        let bar_h = ch;
        let full = Rect { x: 0.0, y: bar_h, w: 1000.0, h: 600.0 - bar_h };
        // Left pane full-height; right column split into two.
        let left = Rect { x: 0.0, y: full.y, w: 496.0, h: full.h };
        let rt = Rect { x: 504.0, y: full.y, w: 496.0, h: (full.h - 8.0) / 2.0 };
        let rb = Rect { x: 504.0, y: rt.y + rt.h + 8.0, w: 496.0, h: rt.h };

        let pen = Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), ..Pen::default() };
        let accent = Pen { fg: Color::Rgb(0x0d, 0xbc, 0x79), ..Pen::default() };

        let (lc, lr) = cells(left);
        let mut g_left = Grid::new(lc, lr);
        g_left.write_str(0, 0, "~/projects/runnir ❯ cargo build", accent);
        g_left.write_str(1, 0, "   Compiling runnir v0.1.0", pen);
        g_left.write_str(2, 0, "    Finished in 2.41s", pen);
        g_left.write_str(3, 0, "~/projects/runnir ❯ █", accent);

        let (rc, rr) = cells(rt);
        let mut g_rt = Grid::new(rc, rr);
        g_rt.write_str(0, 0, "drheavymetal@192.168.1.3 ❯ docker ps", pen);
        g_rt.write_str(1, 0, "CONTAINER   IMAGE      STATUS", pen);
        g_rt.write_str(2, 0, "a1b2c3d4    hermes     Up 3 days", pen);

        let (rbc, rbr) = cells(rb);
        let mut g_rb = Grid::new(rbc, rbr);
        g_rb.write_str(0, 0, "❯ ssh cloudmax → building...", pen);
        g_rb.write_str(1, 0, "  #1 [internal] load build definition", pen);

        // Tab bar chrome.
        let mut bar = Grid::new((1000.0 / cw) as usize, 1);
        bar.fill(Pen { bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() });
        bar.write_str(0, 1, " 1 runnir ", Pen {
            fg: Color::Rgb(0x0d, 0x0d, 0x0f),
            bg: Color::Rgb(0x4c, 0x9f, 0xd4),
            ..Pen::default()
        });
        bar.write_str(0, 12, " 2 servers ", Pen { fg: Color::Rgb(0x9a, 0x9d, 0xa4), bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() });

        let panes = vec![
            (bar, Rect { x: 0.0, y: 0.0, w: 1000.0, h: bar_h }, None, true),
            (g_left, left, None, true),
            (g_rt, rt, Some((40, 60, 90)), false), // ssh-tinted, unfocused
            (g_rb, rb, Some((30, 45, 70)), false),
        ];

        // A command palette overlay.
        let cols = (1000.0 / cw) as usize;
        let rows = (600.0 / ch) as usize;
        let palette = Palette::new(&actions::default_hints());
        let overlay = Overlay::Palette(palette);
        let panels = overlay.render(cols, rows, &config::Theme::default());
        let overlay_specs: Vec<(Grid, Rect)> = panels
            .into_iter()
            .map(|p| {
                let rect =
                    Rect { x: p.col as f32 * cw, y: p.row as f32 * ch, w: 0.0, h: 0.0 };
                (p.grid, rect)
            })
            .collect();

        (panes, Some(overlay_specs))
    });
}

/// Renders the leader layer as the app draws it: a working terminal, the LEADER
/// chip in the status bar and the which-key panel for `level`.
///
/// `level` is the leader path already pressed — `""` for the root, `"t"` for the
/// tabs group. The entries come from `Keymap`, not from a hand-written list, so a
/// screenshot cannot claim a binding the terminal does not have.
fn leader_scene(path_out: &str, level: &str) {
    use crate::render::Rect;
    let keymap =
        actions::Keymap::new(&std::collections::HashMap::new(), &Config::default().leader);
    let steps: Vec<actions::Chord> =
        level.split_whitespace().filter_map(actions::Chord::parse).collect();
    let entries = keymap.leader_entries(&steps);
    if entries.is_empty() {
        eprintln!("runnir: no leader entries for {level:?}");
        return;
    }
    let labels: Vec<String> = steps.iter().map(|c| c.label()).collect();

    // The panel's height is data-dependent (the root level is far taller than a
    // group), so size the canvas to it: measure the cell, lay the panel out, then
    // add the tab bar, six rows of terminal and the status bar.
    const WIDTH: f32 = 1000.0;
    const TERM_ROWS: f32 = 6.0;
    let (cw, ch) = {
        let f = font::FontAtlas::new(16.0).expect("font");
        (f.cell_w, f.cell_h)
    };
    let cols = (WIDTH / cw) as usize;
    let panel_rows = whichkey_grid(&entries, &labels, cols).rows() as f32;
    let height = ((panel_rows + TERM_ROWS + 2.0) * ch).ceil() as u32;

    render::offscreen_scene(path_out, WIDTH as u32, height, 16.0, |r| {
        let (cw, ch) = r.cell_size();
        let cols = (WIDTH / cw) as usize;
        let bar_h = ch;
        let height = height as f32;

        let panel = whichkey_grid(&entries, &labels, cols);
        let panel_h = panel.rows() as f32 * ch;
        // Terminal area is what the chrome leaves: tab bar on top, which-key panel
        // and status bar at the bottom.
        let term = Rect { x: 0.0, y: bar_h, w: WIDTH, h: height - bar_h - panel_h - ch };

        let pen = Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), ..Pen::default() };
        let accent = Pen { fg: Color::Rgb(0x0d, 0xbc, 0x79), ..Pen::default() };
        let mut g = Grid::new((term.w / cw) as usize, (term.h / ch).max(1.0) as usize);
        g.write_str(0, 0, "~/projects/runnir ❯ cargo test", accent);
        g.write_str(1, 0, "   Compiling runnir v0.1.0", pen);
        g.write_str(2, 0, "    Finished in 2.41s", pen);
        g.write_str(3, 0, "  running 148 tests ... ok", pen);
        g.write_str(4, 0, "~/projects/runnir ❯ █", accent);

        // Tab bar.
        let mut bar = Grid::new(cols, 1);
        bar.fill(Pen { bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() });
        bar.write_str(0, 1, " 1 runnir ", Pen {
            fg: Color::Rgb(0x0d, 0x0d, 0x0f),
            bg: Color::Rgb(0x4c, 0x9f, 0xd4),
            ..Pen::default()
        });
        bar.write_str(0, 12, " 2 servers ", Pen { fg: Color::Rgb(0x9a, 0x9d, 0xa4), bg: Color::Rgb(0x15, 0x16, 0x1a), ..Pen::default() });

        // Status bar with the armed LEADER chip, same shape as `build_status`.
        let sbg = Color::Rgb(0x12, 0x13, 0x17);
        let a = config::Theme::default().accent;
        let mut status = Grid::new(cols, 1);
        status.fill(Pen { bg: sbg, ..Pen::default() });
        status.write_str(0, 1, " LEADER ", Pen {
            fg: Color::Rgb(0x12, 0x13, 0x17),
            bg: Color::Rgb(a.0, a.1, a.2),
            flags: crate::grid::Flags::BOLD,
            ..Pen::default()
        });
        status.write_str(0, 10, "~/projects/runnir", Pen { fg: Color::Rgb(0x8a, 0x8d, 0x94), bg: sbg, ..Pen::default() });
        status.write_str(0, 29, "\u{e0a0} main", Pen { fg: Color::Rgb(a.0, a.1, a.2), bg: sbg, ..Pen::default() });

        let panes = vec![
            (bar, Rect { x: 0.0, y: 0.0, w: WIDTH, h: bar_h }, None, true),
            (g, term, None, true),
            (panel, Rect { x: 0.0, y: height - ch - panel_h, w: WIDTH, h: panel_h }, None, true),
            (status, Rect { x: 0.0, y: height - ch, w: WIDTH, h: ch }, None, true),
        ];
        (panes, None)
    });
}

// ---- application -----------------------------------------------------------

struct App {
    proxy: EventLoopProxy<UserEvent>,
    gpu: Option<Gpu>,
    config: Config,
    keymap: Keymap,
    mods: ModifiersState,
    /// Quake ("dropdown") mode: a distinct app-id and no decorations so the
    /// compositor can match and toggle it as a scratchpad. The toggle itself is
    /// the compositor's job — Wayland gives no app global hotkeys — so `--quake`
    /// pairs with a Hyprland binding (see the F1 docs).
    quake: bool,
    /// Config-file mtime last seen, for hot-reload. Refreshed after each apply so a
    /// single save triggers exactly one reload.
    config_mtime: Option<std::time::SystemTime>,
    /// When the config file was last stat'd, to throttle the check to ~1 Hz.
    last_config_check: Instant,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>, quake: bool) -> Self {
        let config = Config::load();
        let keymap = Keymap::new(&config.keys, &config.leader);
        let config_mtime = config_mtime();
        Self {
            proxy,
            gpu: None,
            config,
            keymap,
            mods: ModifiersState::empty(),
            quake,
            config_mtime,
            last_config_check: Instant::now(),
        }
    }

    /// Reloads the config when its file has changed on disk, applying the new theme,
    /// opacity, font and key bindings live. Throttled to once a second so it costs a
    /// single `stat` per idle wake at most.
    fn maybe_reload_config(&mut self) {
        if self.last_config_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_config_check = Instant::now();
        let now = config_mtime();
        if now == self.config_mtime {
            return;
        }
        self.config_mtime = now;
        // Keep the running config (and custom keybindings) when the file is mid-edit
        // or has a typo, instead of snapping the live session back to defaults.
        let Some(new) = Config::try_load() else {
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.status = Some("config error — keeping previous".into());
                gpu.status_expiry = Some(Instant::now() + Duration::from_secs(3));
                gpu.window.request_redraw();
            }
            return;
        };
        self.keymap = Keymap::new(&new.keys, &new.leader);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.apply_config(&new);
            gpu.status = Some("config reloaded".into());
            gpu.status_expiry = Some(Instant::now() + Duration::from_secs(2));
            gpu.window.request_redraw();
        }
        self.config = new;
    }
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    tabs: Vec<Tab>,
    active: usize,
    next_pane_seed: u64,
    overlay: Option<Overlay>,
    /// Tabs closed this session, most recent last, so `ReopenClosed` can bring the
    /// last one back with its layout and scrollback.
    closed_tabs: Vec<session::TabState>,
    cursor_px: PhysicalPosition<f64>,
    clipboard: clipboard::Clipboard,
    /// Bounded, in-memory ring of recent copies (selection, OSC 52, yank, hint, …),
    /// offered by the Super+V picker for re-paste. Never persisted (privacy).
    clip_history: clipboard::ClipHistory,
    broadcast: bool,
    /// Fractional scroll carry-over, so slow touchpad swipes (sub-line pixel deltas)
    /// accumulate into smooth motion instead of being truncated to zero (D9).
    scroll_accum: f32,
    /// The URL/path currently under the pointer, underlined and Ctrl-clickable (D14).
    hover_url: Option<HoverUrl>,
    /// Keyboard copy-mode state, or `None` when off (D12).
    copy_mode: Option<CopyMode>,
    /// When the leader layer was armed, or last stepped into a group: keys resolve
    /// against the leader tree instead of reaching the pane. Disarmed by an action,
    /// a miss, Escape, or `LEADER_TIMEOUT` — an indefinitely armed leader would turn
    /// a keystroke typed minutes later into an action the user never asked for.
    /// Entering a group restarts the clock; the panel is up, so the user is reading.
    leader_armed: Option<Instant>,
    /// The keys pressed since the leader was armed, i.e. how deep into the tree we
    /// are. Empty at the root — the which-key panel renders whatever level it names.
    leader_path: Vec<Chord>,
    /// How long each leader step waits, from `leader_timeout` in the config.
    /// `None` means it never lapses. Cached on `Gpu` because the expiry is read
    /// from the draw path and the event loop, neither of which holds the config.
    leader_timeout: Option<Duration>,
    /// What the which-key panel draws for the level `leader_path` names: `(key,
    /// what it does, is it a group)`. Snapshotted when the layer is armed or steps
    /// into a group, because the keymap lives in `App` and the draw code only ever
    /// sees `Gpu`.
    leader_entries: Vec<(String, String, bool)>,
    /// The armed image auto-preview watch, or `None` when not watching.
    image_watch: Option<ImageWatch>,
    /// The running now-playing waveform worker, or `None`. Dropping it (on overlay
    /// close, or when a new one starts) stops the worker and kills its cava child.
    media_wave: Option<media::WaveHandle>,
    /// When the now-playing overlay last had its metadata refreshed, so a track change
    /// shows while it stays open without re-fetching on every wake. `None` when closed.
    media_last_refresh: Option<Instant>,
    /// Repository state per repo ROOT, not per pane: two panes in the same repository
    /// share one entry and one `git status`.
    git_state: std::collections::HashMap<PathBuf, git::RepoState>,
    /// Roots with a `git status` worker in flight, so a slow repository cannot
    /// accumulate one process per wake.
    git_pending: std::collections::HashSet<PathBuf>,
    /// Sequence for git-panel requests: a reply older than the current one is only
    /// allowed to update lists, never the preview.
    git_gen: u64,
    /// Last seen `git::state_stamp` per repo root, so a change made outside this
    /// pane — another pane, an editor, a second window — still refreshes the bar.
    git_stamp: std::collections::HashMap<PathBuf, u64>,
    /// Repository root per focused pane id, refreshed on the periodic tick so the
    /// tab badges can ask "is this tab's repo dirty" without touching the disk from
    /// the draw path.
    pane_repo: std::collections::HashMap<u64, PathBuf>,
    /// The pane command counter each root was last refreshed at, keyed by root. The
    /// refresh trigger is "a command finished in a pane sitting in this repo", not a
    /// timer: nothing else the user does can change the repository, and a poll would
    /// run git forever on an idle terminal.
    git_seen: std::collections::HashMap<PathBuf, u64>,
    /// An in-flight eased scroll: (pane id, current offset, target offset) in
    /// scrollback lines. Drives smooth glide on scroll-to-top/bottom and jumps.
    scroll_glide: Option<(u64, f32, f32)>,
    /// A config edited in the settings panel, waiting for `App` to adopt it (update
    /// its own `config` + keymap). Drained after each event.
    pending_config: Option<Config>,
    /// Cursor trail ghosts (D15): each is a cell rect and the instant it was left
    /// behind; drawn fading toward the background, pruned once faded.
    cursor_trail: Vec<(f32, f32, f32, f32, Instant)>,
    /// The focused pane id and its cursor cell rect last frame, to detect a jump for
    /// the trail — keyed to the pane so a focus/tab change is not read as a move.
    last_cursor_rect: Option<(u64, f32, f32, f32, f32)>,
    /// The font size in *logical* pixels — what the config asks for and what the
    /// zoom actions step. The atlas is always rasterised at `font_px * scale`, so
    /// this stays display-independent and a zoom means the same thing on every
    /// monitor.
    font_px: f32,
    /// The current display scale factor (1.0 on a normal monitor, 1.5/2.0 on HiDPI
    /// or under fractional scaling). Everything else in the renderer works in
    /// physical pixels; this is the one place logical becomes physical.
    scale: f32,
    /// The (family, size, ligatures) the config last asked for, so hot-reload can
    /// tell an actual font change from an unrelated edit — and so a color-only reload
    /// does not snap a runtime font-zoom back to the configured size.
    applied_font: (String, f32, bool),
    /// Whether the surface actually composites with alpha (PreMultiplied was
    /// selected). Off means opacity must stay 1.0 or the window merely darkens — the
    /// hot-reload path checks this before re-applying config opacity.
    translucent: bool,
    /// Show a status bar along the bottom (cwd, git branch, clock). Costs one row.
    status_bar: bool,
    /// (path, dim) of the background last loaded, so hot-reload only re-decodes on a
    /// real change (image decode is expensive).
    applied_bg: (Option<String>, f32),
    /// Cached clock string ("HH:MM"), refreshed periodically to avoid formatting time
    /// (no chrono dep) every frame.
    clock: String,
    /// When the clock was last refreshed (attempted), or `None` before the first
    /// attempt. `Option` so we never subtract from `Instant` (panics on low uptime).
    last_clock: Option<Instant>,
    ai: ai::Session,
    last_context_refresh: Instant,
    last_autosave: Instant,
    /// Process-start instant, the time base for cursor blink.
    start: Instant,
    /// Last cursor-blink phase drawn, so an idle terminal repaints only on a flip.
    last_blink_phase: u64,
    /// Last left-click time and cell, and the run length, for double/triple click.
    last_click: (Instant, selection::Point),
    click_count: u32,
    /// Button held down, for drag reporting to mouse-mode apps.
    mouse_down: Option<mouse::Button>,
    /// A divider being dragged with the mouse to resize panes.
    resizing: Option<crate::layout::DividerHit>,
    /// When set, the focused pane of the active tab fills the whole area (zoom).
    zoomed: Option<u64>,
    /// Until when a bell flash is drawn over the panes.
    bell_flash: Option<Instant>,
    /// A transient status shown as a toast (e.g. "whispering…") while a background
    /// request is in flight, so an AI action never looks like it did nothing.
    status: Option<String>,
    /// When set, the toast is a terminal message (an error) that no reply will
    /// ever clear, so it must expire on its own at this instant. Without it a
    /// synchronous `ai::ask` failure would leave the spinner turning forever.
    status_expiry: Option<Instant>,
    proxy: EventLoopProxy<UserEvent>,
}

/// Fires a desktop notification (per-OS via `platform`). Silent on failure.
fn notify(body: &str) {
    platform::notify(body);
}

/// A PTY wake closure. Sends a user event through the proxy — the reliable way to
/// interrupt `ControlFlow::Wait` from another thread on Wayland — rather than
/// calling `Window::request_redraw` directly, which can be missed there.
fn wake_fn(proxy: EventLoopProxy<UserEvent>) -> impl Fn() + Send + Clone + 'static {
    move || {
        let _ = proxy.send_event(UserEvent::Redraw);
    }
}

impl App {
    fn init_gpu(&mut self, event_loop: &ActiveEventLoop) -> Gpu {
        let t0 = Instant::now();
        let mark = |what: &str| eprintln!("[boot] {what}: {:?}", t0.elapsed());
        let mut attrs = Window::default_attributes()
            .with_title("runnir")
            .with_decorations(self.config.window.decorations && !self.quake)
            .with_inner_size(LogicalSize::new(self.config.window.width, self.config.window.height));
        // Set a Wayland app-id so compositor rules can match runnir; a distinct one
        // in quake mode so a dropdown rule targets only that instance.
        #[cfg(target_os = "linux")]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            let app_id = if self.quake { "runnir-quake" } else { "runnir" };
            attrs = attrs.with_name(app_id, app_id);
        }
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        mark("create_window");

        // Wayland drag-and-drop, which winit does not implement. Started here
        // because it needs the surface, and skipped entirely on X11/macOS, where
        // winit's own `DroppedFile` covers it.
        #[cfg(all(unix, not(target_os = "macos")))]
        start_wayland_dnd(&window, self.proxy.clone());

        // On a hybrid laptop the Vulkan loader enumerates every ICD, and touching the
        // NVIDIA one resumes a runtime-suspended discrete GPU. That wake costs ~1.8s,
        // which is why the first launch after an idle stretch feels slow while the
        // next ones are instant. We ask for LowPower anyway, so hide the discrete ICD
        // from the loader and only put it back if that leaves us with no adapter.
        let hide_discrete = cfg!(target_os = "linux")
            && std::env::var_os("VK_LOADER_DRIVERS_DISABLE").is_none()
            && std::env::var_os("VK_LOADER_DRIVERS_SELECT").is_none();
        let native = if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::VULKAN
        };
        let try_adapter = |hidden: bool, backends: wgpu::Backends| {
            if cfg!(target_os = "linux") {
                // Read by the loader inside vkCreateInstance below. Nothing else in
                // the process reads these, so the racy-set_var hazard does not apply.
                unsafe {
                    if hidden {
                        std::env::set_var("VK_LOADER_DRIVERS_DISABLE", "nvidia_icd.json");
                    } else {
                        std::env::remove_var("VK_LOADER_DRIVERS_DISABLE");
                    }
                }
            }
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let surface = instance.create_surface(window.clone()).ok()?;
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .ok()?;
            Some((instance, surface, adapter))
        };
        // Widen the search only if the cheap path came up empty: first the native
        // backend without the discrete ICD, then with it, then everything else.
        let (_instance, surface, adapter) = try_adapter(hide_discrete, native)
            .or_else(|| hide_discrete.then(|| try_adapter(false, native)).flatten())
            .or_else(|| try_adapter(false, wgpu::Backends::all()))
            .expect("no suitable GPU adapter");
        mark("request_adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("runnir"),
            ..Default::default()
        }))
        .expect("failed to create device");
        mark("request_device");

        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("adapter does not support this surface");
        // For a translucent window, ask the surface for premultiplied-alpha
        // compositing so the compositor blends (and can blur) behind us. Fall back to
        // opaque if the platform does not offer it.
        let mut translucent = false;
        if self.config.window.opacity < 1.0 {
            let caps = surface.get_capabilities(&adapter);
            if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                surface_config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
                translucent = true;
            }
        }
        surface.configure(&device, &surface_config);
        mark("surface_configure");
        println!("runnir: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);

        let font_px = self.config.font.size;
        let scale = window.scale_factor() as f32;
        let mut font = FontAtlas::new_with(&self.config.font.family, font_px * scale)
            .unwrap_or_else(|e| panic!("font: {e}"));
        font.ligatures = self.config.font.ligatures;
        mark("font_atlas");
        let mut renderer = Renderer::new(&device, surface_config.format, font);
        mark("renderer_new");
        renderer.set_theme(self.config.theme.clone());
        // Apply opacity when the compositor can show through (translucent) OR a
        // background image is set (the image is drawn in-pass, behind the translucent
        // cells, so it shows even on an opaque surface). Otherwise 1.0, or opacity
        // would merely darken a solid background.
        let want_opacity = translucent || self.config.window.background.is_some();
        renderer.set_opacity(if want_opacity { self.config.window.opacity } else { 1.0 });
        load_background(&self.config, &device, &queue, &mut renderer);

        let cell = renderer.cell_size();

        // Restore, in order of precedence:
        //   1. this project's saved layout (opt-in `session_restore`), keyed by the
        //      nearest git ancestor of the launch cwd — layout + cwd only;
        //   2. otherwise the previous whole-window session (`restore_session`);
        //   3. otherwise a single fresh tab.
        // The project layout is turned into a `session::Session` so the same
        // `restore_tabs` / `Tab::from_session` rebuild path serves all cases.
        let project = self
            .config
            .behaviour
            .session_restore
            .then(|| std::env::current_dir().ok())
            .flatten()
            .map(|cwd| project_session::project_key(&cwd))
            .and_then(|key| project_session::ProjectSessions::load().get(&key).cloned());
        let restored = match project {
            Some(entry) => Some(entry.to_session()),
            None => {
                let saved = self
                    .config
                    .behaviour
                    .restore_session
                    .then(session::Session::load)
                    .flatten();
                if saved.is_some() {
                    // Consume the whole-window snapshot so a later crash cannot
                    // restore a stale one; the project store is left intact.
                    session::Session::clear();
                }
                saved
            }
        };
        let (tabs, active, next_seed) = match restored {
            Some(saved) => {
                restore_tabs(&saved, &surface_config, cell, &self.config, self.proxy.clone())
            }
            None => {
                let area = content_area(&surface_config, cell, 1, self.config.window.status_bar);
                let tab = Tab::new(area, cell, &self.config, 1, &Spawn::default(), wake_fn(self.proxy.clone()))
                    .expect("failed to spawn first pane");
                (vec![tab], 0, 1000)
            }
        };

        let mut gpu = Gpu {
            window,
            surface,
            device,
            queue,
            surface_config,
            renderer,
            tabs,
            active,
            next_pane_seed: next_seed,
            overlay: None,
            closed_tabs: Vec::new(),
            cursor_px: PhysicalPosition::new(0.0, 0.0),
            clipboard: clipboard::Clipboard::new(),
            clip_history: clipboard::ClipHistory::new(
                self.config.clipboard.capacity,
                self.config.clipboard.enabled,
            ),
            broadcast: false,
            scroll_accum: 0.0,
            hover_url: None,
            copy_mode: None,
            leader_armed: None,
            leader_path: Vec::new(),
            leader_entries: Vec::new(),
            leader_timeout: leader_timeout(&self.config),
            image_watch: None,
            media_wave: None,
            media_last_refresh: None,
            git_state: std::collections::HashMap::new(),
            git_pending: std::collections::HashSet::new(),
            git_seen: std::collections::HashMap::new(),
            git_gen: 0,
            git_stamp: std::collections::HashMap::new(),
            pane_repo: std::collections::HashMap::new(),
            scroll_glide: None,
            pending_config: None,
            cursor_trail: Vec::new(),
            last_cursor_rect: None,
            font_px,
            scale,
            applied_font: (
                self.config.font.family.clone(),
                self.config.font.size,
                self.config.font.ligatures,
            ),
            translucent,
            status_bar: self.config.window.status_bar,
            applied_bg: (self.config.window.background.clone(), self.config.window.background_dim),
            clock: String::new(),
            last_clock: None,
            ai: ai::Session::new(),
            last_context_refresh: Instant::now(),
            last_autosave: Instant::now(),
            start: Instant::now(),
            last_blink_phase: 0,
            // A sentinel cell no real click can match, so the first click is never
            // mistaken for the second half of a double.
            last_click: (Instant::now(), (usize::MAX, usize::MAX)),
            click_count: 0,
            mouse_down: None,
            resizing: None,
            zoomed: None,
            bell_flash: None,
            status: None,
            status_expiry: None,
            proxy: self.proxy.clone(),
        };
        // Arm the image auto-preview watch at startup when the config asks for it and
        // names a directory. A snapshot of the directory is taken now, so files
        // already there never flood the pane — only new drops fire.
        if self.config.watch.enabled {
            if let Some(dir) = self.config.watch.directory.as_deref() {
                gpu.arm_image_watch(watch::expand_tilde(dir), &self.config);
            }
        }
        gpu
    }
}

/// The config file's last-modified time, or `None` if it does not exist yet. Used
/// by hot-reload to notice edits.
fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(Config::active_path()).and_then(|m| m.modified()).ok()
}

/// Keyboard copy-mode (D12): a virtual cursor navigating a pane's scrollback with
/// vim motions, optionally extending a selection, to copy without the mouse.
struct CopyMode {
    pane: u64,
    /// Cursor in absolute grid space (row indexes scrollback ++ screen).
    cur: crate::selection::Point,
    /// Selection anchor once `v` is pressed; `None` means just navigating.
    anchor: Option<crate::selection::Point>,
    /// The grid's `dropped` count when last synced, so eviction (which shifts the
    /// scrollback++screen index space) can be rebased out and the cursor stays on the
    /// same line as new output arrives.
    dropped: usize,
}

/// An armed image auto-preview watch (the `[watch]` feature): the directory being
/// polled, the debounce state machine, the extension filter and preview width taken
/// from config at arm time, and when it was last polled (to throttle to the poll
/// interval regardless of how often the loop wakes).
struct ImageWatch {
    dir: std::path::PathBuf,
    state: watch::WatchState,
    exts: Vec<String>,
    max_width: usize,
    last_poll: Instant,
}

/// How often the watched directory is polled, in milliseconds. Slow enough to cost
/// nothing (one `read_dir` per interval), fast enough that a finished render shows
/// promptly; also the debounce granularity (a new file waits one interval).
const WATCH_POLL_MS: u64 = 700;

/// A URL/path under the pointer: which pane, where on screen (absolute row and
/// start column), how long, and the target itself for a Ctrl-click to act on.
#[derive(Clone, PartialEq)]
struct HoverUrl {
    pane: u64,
    abs_row: usize,
    col: usize,
    len: usize,
    text: String,
    kind: overlay::HintKind,
}

/// Loads the configured background image into the renderer (decodes to RGBA8). A
/// missing or unreadable path just leaves the background solid.
fn load_background(config: &Config, device: &wgpu::Device, queue: &wgpu::Queue, renderer: &mut Renderer) {
    let Some(path) = config.window.background.as_ref() else {
        renderer.set_background(device, queue, None, config.window.background_dim);
        return;
    };
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(rest))
            .unwrap_or_else(|| path.into())
    } else {
        path.into()
    };
    match image::open(&expanded) {
        Ok(img) => {
            // Clamp to the GPU's max texture size (default 8192), or a big wallpaper
            // is a validation error → wgpu's default handler panics the process.
            let max = device.limits().max_texture_dimension_2d;
            let img = if img.width() > max || img.height() > max {
                img.thumbnail(max, max)
            } else {
                img
            };
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            renderer.set_background(device, queue, Some((&rgba, w, h)), config.window.background_dim);
        }
        Err(e) => {
            eprintln!("runnir: could not load background {}: {e}", expanded.display());
            renderer.set_background(device, queue, None, config.window.background_dim);
        }
    }
}

fn content_area(cfg: &wgpu::SurfaceConfiguration, cell: (f32, f32), tab_count: usize, status: bool) -> Rect {
    let bar = if tab_count > 1 { TABBAR_ROWS * cell.1 } else { 0.0 };
    let status_h = if status { cell.1 } else { 0.0 };
    let h = (cfg.height as f32 - bar - status_h).max(cell.1);
    Rect { x: 0.0, y: bar, w: cfg.width as f32, h }
}

/// Rebuilds tabs from a saved session. Returns the tabs, the active index, and the
/// next free pane id (above every restored one, so new panes never collide).
fn restore_tabs(
    saved: &session::Session,
    cfg: &wgpu::SurfaceConfiguration,
    cell: (f32, f32),
    config: &Config,
    proxy: EventLoopProxy<UserEvent>,
) -> (Vec<Tab>, usize, u64) {
    let area = content_area(cfg, cell, saved.tabs.len(), config.window.status_bar);
    let mut tabs = Vec::new();
    let mut max_id = 0u64;
    for state in &saved.tabs {
        max_id = max_id.max(state.panes.keys().copied().max().unwrap_or(0));
        let p = proxy.clone();
        let wake = move |_id| -> Box<dyn Fn() + Send + 'static> {
            let p = p.clone();
            Box::new(move || {
                let _ = p.send_event(UserEvent::Redraw);
            })
        };
        match Tab::from_session(state, area, cell, config, wake) {
            Ok(tab) => tabs.push(tab),
            Err(e) => eprintln!("runnir: could not restore a tab: {e}"),
        }
    }
    if tabs.is_empty() {
        let tab = Tab::new(area, cell, config, 1, &Spawn::default(), wake_fn(proxy.clone()))
            .expect("failed to spawn fallback pane");
        return (vec![tab], 0, 1000);
    }
    let active = saved.active.min(tabs.len() - 1);
    (tabs, active, max_id.max(1000) + 1)
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            self.gpu = Some(self.init_gpu(event_loop));
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        match event {
            UserEvent::Redraw => gpu.window.request_redraw(),
            UserEvent::Ai(reply) => {
                // The request finished: clear the "thinking" toast.
                gpu.status = None;
                gpu.status_expiry = None;
                match gpu.ai.receive(reply, gpu.overlay.as_mut()) {
                    ai::Delivery::Insert(cmd) => gpu.insert_command(cmd),
                    ai::Delivery::Whisper(plan) => gpu.execute_whisper(plan, &self.config),
                    ai::Delivery::ToPanel | ai::Delivery::Nothing => {}
                }
                gpu.window.request_redraw();
            }
            UserEvent::Control(req, reply) => {
                // Run the request against the live terminal and answer the socket
                // thread. A dropped receiver (client hung up) just discards the reply.
                let resp = gpu.handle_control(req, &self.config);
                let _ = reply.send(resp);
            }
            UserEvent::Media(msg) => gpu.on_media_msg(msg, &self.config),
            UserEvent::GitPanel(seq, msg) => gpu.on_git_panel_msg(seq, msg, &self.config),
            UserEvent::Git(root, state) => {
                gpu.git_pending.remove(&root);
                match state {
                    Some(s) => {
                        gpu.git_state.insert(root, s);
                    }
                    // Not a repository any more (or git failed): forget it rather
                    // than leaving the bar showing a state that no longer exists.
                    None => {
                        gpu.git_state.remove(&root);
                    }
                }
                gpu.window.request_redraw();
            }
            // Wayland reports the drop in surface-logical coordinates; the pane hit
            // test works in physical pixels, so scale before asking where it landed.
            UserEvent::FilesDropped(paths, x, y) => {
                let scale = gpu.scale as f64;
                let at = PhysicalPosition::new(x * scale, y * scale);
                gpu.on_files_dropped(&paths, Some(at));
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                gpu.save_session(&self.config);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => gpu.resize(size.width, size.height, &self.config),
            // Dragging the window to a monitor with a different scale (or a
            // fractional-scale change under Wayland) keeps the same logical font
            // size but must re-rasterise the atlas at the new density — otherwise
            // the glyphs stay at the old monitor's pixel size and look tiny on a
            // HiDPI screen. winit sends a Resized right after this, which reflows.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                gpu.set_scale(scale_factor as f32, &self.config)
            }
            WindowEvent::RedrawRequested => gpu.render(&self.config),
            // One event per file, so a multi-file drag arrives as a run of these
            // and each path appends its own argument. winit gives no drop
            // coordinates here, hence `None` — see `on_files_dropped`.
            //
            // NOTE: winit 0.30 only raises this on X11, macOS and Windows; its
            // Wayland backend has no drag-and-drop at all. Under a Wayland session
            // the drop is picked up by `dnd`, which speaks wl_data_device directly.
            WindowEvent::DroppedFile(path) => gpu.on_files_dropped(&[path], None),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::MouseWheel { delta, .. } => gpu.on_wheel(delta, &self.config, self.mods),
            WindowEvent::CursorMoved { position, .. } => gpu.on_cursor(position, self.mods),
            WindowEvent::MouseInput { state, button, .. } => {
                gpu.on_click(state, button, self.mods, &self.config)
            }
            // `is_synthetic` presses are emitted by winit for every key already held
            // when the window *gains focus* — they exist only to sync key state, not
            // to enter text. Forwarding them to the PTY double-sends the first typed
            // character when a keystroke also brings the window into focus (the
            // "ssh" -> "sssh" bug). Only real, non-synthetic presses produce bytes.
            // Both presses and releases are forwarded: releases are needed for the
            // kitty keyboard protocol's event-type reporting (on_key drops them when
            // no pane has that flag set, so legacy input is unchanged).
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                gpu.on_key(event, self.mods, &self.config, &self.keymap, event_loop);
                // The settings panel may have edited the config: adopt it (and its
                // key bindings) so behaviour/keys take effect live, and refresh the
                // hot-reload mtime so the panel's own save doesn't trigger a reload.
                if let Some(cfg) = gpu.pending_config.take() {
                    self.keymap = Keymap::new(&cfg.keys, &cfg.leader);
                    self.config = cfg;
                    self.config_mtime = config_mtime();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.maybe_reload_config();
        let Some(gpu) = self.gpu.as_mut() else { return };
        if !gpu.reap(&self.config) {
            // Every shell exited: an intentional close. Clear the session so the
            // next launch starts fresh rather than restoring a dead layout — and
            // so this does not overwrite a good autosave with an empty state.
            session::Session::clear();
            event_loop.exit();
            return;
        }
        gpu.periodic(&self.config);

        // A pending AI request animates a spinner: wake often and repaint. An
        // error toast has an expiry (no reply will clear it); once it passes,
        // drop the toast and fall through to normal idling instead of spinning
        // the spinner forever.
        if gpu.status.is_some() {
            let expired = gpu.status_expiry.is_some_and(|e| Instant::now() >= e);
            if expired {
                gpu.status = None;
                gpu.status_expiry = None;
                gpu.window.request_redraw();
            } else {
                gpu.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(120),
                ));
                return;
            }
        }

        // An armed leader expires on a deadline nothing else wakes us for: on an idle
        // terminal the status-bar chip would stay lit long after the layer was gone.
        // Clear it here and repaint once; the wake itself is folded into `extra_wake`
        // below rather than returning early, which would stall the animations.
        if let Some(limit) = gpu.leader_timeout {
            if gpu.leader_armed.is_some_and(|t| t.elapsed() >= limit) {
                gpu.leader_armed = None;
                gpu.leader_path.clear();
                gpu.leader_entries.clear();
                gpu.window.request_redraw();
            }
        }

        // Animate a scroll glide (smooth scroll-to-top/bottom / jump-to-prompt).
        if let Some((id, cur, target)) = gpu.scroll_glide {
            let next = cur + (target - cur) * 0.3;
            let done = (target - next).abs() < 0.5;
            let pos = if done { target } else { next };
            if let Some(pane) = gpu.tabs.iter_mut().find_map(|t| t.panes.get_mut(&id)) {
                let actual = pane.grid.lock().unwrap().display_offset() as isize;
                pane.scroll(pos.round() as isize - actual);
            }
            gpu.scroll_glide = if done { None } else { Some((id, pos, target)) };
            gpu.window.request_redraw();
            if !done {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(16),
                ));
                return;
            }
        }

        // Animate the bell flash to completion: without this, an idle window (blink
        // off, or an overlay open) would freeze the flash on screen until the next
        // event. Drive redraws until it expires, then clear it and repaint once to
        // erase the last frame.
        if let Some(until) = gpu.bell_flash {
            if Instant::now() < until {
                gpu.window.request_redraw();
                event_loop
                    .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
                return;
            }
            gpu.bell_flash = None;
            gpu.window.request_redraw();
        }

        // Animate the cursor trail (D15) to completion, same as the bell flash. Prune
        // HERE, not only in render(): render early-returns when the window is
        // occluded, so without this the loop would spin at 60Hz forever while hidden.
        gpu.cursor_trail.retain(|g| g.4.elapsed().as_millis() <= 180);
        if !gpu.cursor_trail.is_empty() {
            gpu.window.request_redraw();
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
            return;
        }

        // An armed image watch needs a periodic wake to poll its directory, even on a
        // fully idle terminal (no output, no blink). This is that wake — the same
        // self-sustaining WaitUntil pattern the blink uses.
        let watch_wake = gpu.image_watch.is_some()
            .then(|| Instant::now() + Duration::from_millis(WATCH_POLL_MS));
        // The now-playing overlay needs a periodic wake too: to refresh its metadata
        // (above) and to animate the waveform even on an idle terminal.
        let media_wake = matches!(gpu.overlay, Some(Overlay::Media(_)))
            .then(|| Instant::now() + Duration::from_millis(250));
        // And the armed leader, so the chip clears itself on an otherwise idle window.
        // Nothing to wake for when the layer never lapses.
        let leader_wake = gpu.leader_timeout.and_then(|d| gpu.leader_armed.map(|t| t + d));
        // Whichever background timer is soonest is the one to wake on.
        let extra_wake = [watch_wake, media_wake, leader_wake].into_iter().flatten().min();

        // Drive cursor blink. A WaitUntil wake does not itself repaint, so redraw
        // only when the blink phase actually flips — that keeps an idle terminal
        // repainting at exactly the blink rate, not on every timer tick, and never
        // busy-loops.
        if self.config.cursor.blink && gpu.overlay.is_none() {
            let interval = self.config.cursor.blink_interval.max(50);
            let phase = gpu.start.elapsed().as_millis() as u64 / interval;
            if phase != gpu.last_blink_phase {
                gpu.last_blink_phase = phase;
                gpu.window.request_redraw();
            }
            // Wake at the next toggle boundary, not a fixed interval from now, so
            // the phase check above lands right when it changes.
            let next = (phase + 1) * interval;
            let since = gpu.start.elapsed().as_millis() as u64;
            let wait = next.saturating_sub(since).max(1);
            let mut deadline = Instant::now() + Duration::from_millis(wait);
            // Whichever comes first, blink or a background timer, is the one to wake on.
            if let Some(w) = extra_wake {
                deadline = deadline.min(w);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else if let Some(w) = extra_wake {
            event_loop.set_control_flow(ControlFlow::WaitUntil(w));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

impl Gpu {
    fn active_area(&self) -> Rect {
        content_area(&self.surface_config, self.renderer.cell_size(), self.tabs.len(), self.status_bar)
    }

    /// Detects a bell on any pane of ANY tab: a bell in a background tab still raises
    /// the window's urgency hint, but only a bell in the active tab flashes the
    /// screen (a background tab's flash would be meaningless). Draining `take_bell`
    /// on every tab also stops a stale bell from flashing later on tab switch.
    fn check_bells(&mut self) {
        let active = self.active;
        let mut active_rang = false;
        let mut any_rang = false;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            for pane in tab.panes.values_mut() {
                if pane.take_bell() {
                    any_rang = true;
                    if i == active {
                        active_rang = true;
                    }
                }
            }
        }
        if active_rang {
            self.bell_flash = Some(Instant::now() + Duration::from_millis(120));
            self.window.request_redraw();
        }
        if any_rang && !self.window.has_focus() {
            self.window
                .request_user_attention(Some(winit::window::UserAttentionType::Critical));
        }
    }

    /// Bell-flash overlay alpha for this frame (0 = none), decaying over the window.
    fn bell_alpha(&self) -> f32 {
        match self.bell_flash {
            Some(until) => {
                let remaining = until.saturating_duration_since(Instant::now()).as_millis() as f32;
                (remaining / 120.0 * 0.35).clamp(0.0, 0.35)
            }
            None => 0.0,
        }
    }

    /// Pane rectangles for the active tab, honouring zoom: a zoomed pane fills the
    /// whole area alone. Used by rendering and mouse hit-testing so both agree.
    fn visible_rects(&self, area: Rect) -> Vec<(u64, Rect)> {
        match self.zoomed {
            Some(id) if self.tabs[self.active].panes.contains_key(&id) => {
                vec![(id, self.tabs[self.active].full_rect(area))]
            }
            _ => self.tabs[self.active].layout(area),
        }
    }

    fn resize(&mut self, w: u32, h: u32, _config: &Config) {
        self.surface_config.width = w.max(1);
        self.surface_config.height = h.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        for tab in &mut self.tabs {
            tab.set_cell(cell);
            tab.reflow(area);
        }
        self.reapply_zoom();
        // Window resize moves the cursor rect without a cursor move; drop the trail
        // baseline so it does not leave a phantom ghost.
        self.last_cursor_rect = None;
        self.window.request_redraw();
    }

    /// Refreshes context tints/titles periodically, autosaves the session, and
    /// checks for long-running commands that finished while unfocused.
    /// Asks a worker for the focused pane's repository state, when it can have
    /// changed. "Can have changed" means a command finished in that pane since the
    /// last look, or the pane moved into a repository we have nothing for — a `cd`
    /// is itself a command, so both cases are covered by the OSC 133 counter.
    ///
    /// Nothing here polls. An idle terminal sitting in a repository never spawns a
    /// git process, which is the whole reason this is not on the 500ms tick.
    fn refresh_git(&mut self) {
        // The status bar and the tab badges are the consumers; with the bar off the
        // badges still want it, so this only stops when there is no tab bar either.
        if !self.status_bar && self.tabs.len() < 2 {
            return;
        }
        // Every tab's focused pane, not just the active one: a badge that only knew
        // about the tab you are looking at would be telling you what you can already
        // see. The map is what the draw path reads — it never touches the disk.
        let mut seen_roots: Vec<PathBuf> = Vec::new();
        self.pane_repo.clear();
        for tab in &self.tabs {
            let id = tab.focus;
            let Some(cwd) = tab.focused_ref().cwd() else { continue };
            let Some(root) = crate::git::repo_root(&cwd) else { continue };
            self.pane_repo.insert(id, root.clone());
            if !seen_roots.contains(&root) {
                seen_roots.push(root);
            }
        }
        // At most one git per wake, active tab's repo first. A window with eight
        // tabs in eight repositories must not answer a keystroke with eight
        // processes.
        let active_root = self.tabs.get(self.active).and_then(|t| self.pane_repo.get(&t.focus)).cloned();
        let order = active_root.into_iter().chain(seen_roots).collect::<Vec<_>>();
        for root in order {
            if self.git_pending.contains(&root) {
                continue;
            }
            let seq = self
                .tabs
                .iter()
                .find(|t| self.pane_repo.get(&t.focus) == Some(&root))
                .map(|t| t.focused_ref().command_seq())
                .unwrap_or(0);
            // Two triggers, because a repository changes in two ways: something ran
            // in that pane (the command counter), or something changed the repo from
            // outside it (the index/HEAD stamp — an editor, another pane, a rebase in
            // a second window). Neither alone is enough.
            let stamp = crate::git::state_stamp(&root);
            let fresh = self.git_seen.get(&root) == Some(&seq)
                && self.git_stamp.get(&root) == Some(&stamp)
                && self.git_state.contains_key(&root);
            if fresh {
                continue;
            }
            self.git_stamp.insert(root.clone(), stamp);
            self.git_seen.insert(root.clone(), seq);
            self.git_pending.insert(root.clone());
            let proxy = self.proxy.clone();
            // Detached: if git hangs on a network filesystem, this thread hangs, not
            // the UI, and the `git_pending` guard keeps it to one.
            std::thread::spawn(move || {
                let state = crate::git::read_state(&root);
                let _ = proxy.send_event(UserEvent::Git(root, state));
            });
            return;
        }
    }

    fn periodic(&mut self, config: &Config) {
        // Bells are checked here (not only in render) so an occluded or minimized
        // window — the case where the urgency hint matters most — still raises it;
        // render early-returns when the surface is hidden. Cheap: one u64 compare
        // per pane.
        self.check_bells();

        // Poll the image auto-preview watch (no-op unless armed). Runs on the periodic
        // wake driven from about_to_wait; never blocks (one read_dir at most).
        self.poll_image_watch(config);

        // Refresh the now-playing overlay's metadata on a slow timer while it is open,
        // so a track change shows without reopening. Non-blocking: the fetch runs on a
        // worker thread and answers via UserEvent::Media.
        if matches!(self.overlay, Some(Overlay::Media(_))) {
            let due = self
                .media_last_refresh
                .map_or(true, |t| t.elapsed() >= Duration::from_millis(1500));
            if due {
                self.media_last_refresh = Some(Instant::now());
                self.spawn_media_fetch();
            }
        } else {
            self.media_last_refresh = None;
        }

        // Drain OSC 52 clipboard writes and OSC 9/99/777 notifications from every
        // pane on every wake (a PTY produced output → we were woken), so a program's
        // copy or notification lands promptly rather than waiting on the 500ms
        // context tick below. Cheap: one lock + take of a usually-empty vec per pane.
        for tab in &mut self.tabs {
            for pane in tab.panes.values_mut() {
                for text in pane.take_clipboard_writes() {
                    // Record OSC 52 writes in the history too, then set the clipboard.
                    // Field accesses stay disjoint from the `&mut self.tabs` loop above,
                    // so this is inlined rather than routed through `set_clipboard`
                    // (which would borrow all of `self`).
                    self.clip_history.push(&text);
                    self.clipboard.set(&text);
                }
                for body in pane.take_notifications() {
                    notify(&body);
                }
            }
        }

        // Repository state for the focused pane's repo, if it moved on.
        self.refresh_git();

        // Refresh the status-bar clock roughly every 20s (formatting local time
        // without a chrono dependency; `date` handles the timezone).
        // Refresh at most every 20s, keyed on the last ATTEMPT (not success) so a
        // missing/failing `date` can't spawn a process on every event.
        let due = self.last_clock.map_or(true, |t| t.elapsed() >= Duration::from_secs(20));
        if self.status_bar && due {
            self.last_clock = Some(Instant::now());
            if let Ok(out) = std::process::Command::new("date").arg("+%H:%M").output() {
                if out.status.success() {
                    self.clock = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    self.window.request_redraw();
                }
            }
        }

        if self.last_context_refresh.elapsed() >= Duration::from_millis(500) {
            self.last_context_refresh = Instant::now();
            let focused = self.window.has_focus();
            for tab in &mut self.tabs {
                for pane in tab.panes.values_mut() {
                    pane.refresh_context(config);
                    // A command that ran longer than the threshold and finished
                    // while the window is unfocused earns a desktop notification.
                    if config.behaviour.notify_after_secs > 0 && !focused {
                        if let Some(msg) = pane.take_completion(config.behaviour.notify_after_secs) {
                            notify(&msg);
                        }
                    }
                    // Keyword watch (W4): fires whether focused or not — it is an
                    // explicit "tell me when this appears" on a monitored pane.
                    if pane.watching().is_some() {
                        if let Some(hit) = pane.take_watch_hit() {
                            notify(&hit);
                        }
                    }
                }
            }
            if self.last_autosave.elapsed() >= Duration::from_secs(30) {
                self.last_autosave = Instant::now();
                self.save_session(config);
            }
            self.window.request_redraw();
        }
    }

    /// Removes exited panes and empty tabs. Returns false when nothing is left.
    fn reap(&mut self, _config: &Config) -> bool {
        let area = self.active_area();
        let mut i = 0;
        while i < self.tabs.len() {
            if !self.tabs[i].reap_dead(area) {
                self.tabs.remove(i);
                // Removing a tab at or before `active` shifts it down. Without this
                // the focus would silently jump to the next tab.
                if self.active > i || self.active >= self.tabs.len() {
                    self.active = self.active.saturating_sub(1);
                }
            } else {
                i += 1;
            }
        }
        !self.tabs.is_empty()
    }

    fn tab(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn new_pane_id(&mut self) -> u64 {
        self.next_pane_seed += 1;
        self.next_pane_seed
    }
}

include!("app_input.rs");
include!("app_ai.rs");
include!("app_draw.rs");

/// Hands the `dnd` listener the raw Wayland handles for this window.
///
/// A no-op on an X11 display: the handles are then X11 handles, winit already
/// delivers `DroppedFile` there, and running both would type the path twice.
#[cfg(all(unix, not(target_os = "macos")))]
fn start_wayland_dnd(window: &Window, proxy: EventLoopProxy<UserEvent>) {
    use winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    let (Ok(dh), Ok(wh)) = (window.display_handle(), window.window_handle()) else { return };
    if let (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) = (dh.as_raw(), wh.as_raw())
    {
        dnd::start(d.display.as_ptr(), w.surface.as_ptr(), proxy);
    }
}
