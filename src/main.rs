mod actions;
mod ai;
mod boxdraw;
mod clipboard;
mod config;
mod docs;
mod font;
mod graphics;
mod grid;
mod guardian;
mod history;
mod hints;
mod keys;
mod layout;
mod mouse;
mod overlay;
mod pane;
mod pty;
mod render;
mod selection;
mod session;
mod tab;
mod whisper;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::actions::{Action, Keymap};
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

/// A message from a background worker back to the UI thread.
pub enum UserEvent {
    Ai(ai::Reply),
    /// A PTY produced output. On Wayland, `Window::request_redraw` from another
    /// thread does not reliably interrupt `ControlFlow::Wait`; sending a user event
    /// through the proxy does. Without this, echoed input and command output appear
    /// only on the next keystroke or blink tick — the "typing feels laggy" bug.
    Redraw,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
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
            return demo_scene(args.get(2).map(String::as_str).unwrap_or("/tmp/runnir-demo.png"));
        }
        _ => {}
    }

    let quake = args.iter().any(|a| a == "--quake");
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    // Wait, not Poll: an idle terminal must not burn a core.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(event_loop.create_proxy(), quake);
    event_loop.run_app(&mut app).unwrap();
}

fn print_help() {
    println!(
        "runnir {} — a GPU terminal emulator\n\n\
         USAGE:\n  \
         runnir                     start the terminal\n  \
         runnir --write-config      write a default config file\n  \
         runnir --dump CMD          run CMD, print the resulting grid (debug)\n  \
         runnir --render OUT CMD    render CMD's output to a PNG (debug)\n\n\
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
        let keymap = Keymap::new(&config.keys);
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
        self.keymap = Keymap::new(&new.keys);
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
    broadcast: bool,
    /// Fractional scroll carry-over, so slow touchpad swipes (sub-line pixel deltas)
    /// accumulate into smooth motion instead of being truncated to zero (D9).
    scroll_accum: f32,
    /// The URL/path currently under the pointer, underlined and Ctrl-clickable (D14).
    hover_url: Option<HoverUrl>,
    /// Keyboard copy-mode state, or `None` when off (D12).
    copy_mode: Option<CopyMode>,
    /// An in-flight eased scroll: (pane id, current offset, target offset) in
    /// scrollback lines. Drives smooth glide on scroll-to-top/bottom and jumps.
    scroll_glide: Option<(u64, f32, f32)>,
    /// Cursor trail ghosts (D15): each is a cell rect and the instant it was left
    /// behind; drawn fading toward the background, pruned once faded.
    cursor_trail: Vec<(f32, f32, f32, f32, Instant)>,
    /// The focused pane id and its cursor cell rect last frame, to detect a jump for
    /// the trail — keyed to the pane so a focus/tab change is not read as a move.
    last_cursor_rect: Option<(u64, f32, f32, f32, f32)>,
    font_px: f32,
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

/// Fires a desktop notification. Silent on failure — there is nowhere useful to
/// report that the notifier itself is missing.
fn notify(body: &str) {
    let _ = std::process::Command::new("notify-send")
        .arg("runnir")
        .arg(body)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no suitable GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("runnir"),
            ..Default::default()
        }))
        .expect("failed to create device");

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
        println!("runnir: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);

        let font_px = self.config.font.size;
        let mut font =
            FontAtlas::new_with(&self.config.font.family, font_px).unwrap_or_else(|e| panic!("font: {e}"));
        font.ligatures = self.config.font.ligatures;
        let mut renderer = Renderer::new(&device, surface_config.format, font);
        renderer.set_theme(self.config.theme.clone());
        // Only apply opacity when the surface actually composites with alpha; on an
        // opaque surface it would merely darken the background, not reveal anything.
        renderer.set_opacity(if translucent { self.config.window.opacity } else { 1.0 });
        load_background(&self.config, &device, &queue, &mut renderer);

        let cell = renderer.cell_size();

        // Restore the previous session if enabled and present; otherwise one tab.
        let restored = self
            .config
            .behaviour
            .restore_session
            .then(session::Session::load)
            .flatten();
        let (tabs, active, next_seed) = match restored {
            Some(saved) => {
                session::Session::clear();
                restore_tabs(&saved, &surface_config, cell, &self.config, self.proxy.clone())
            }
            None => {
                let area = content_area(&surface_config, cell, 1, self.config.window.status_bar);
                let tab = Tab::new(area, cell, &self.config, 1, &Spawn::default(), wake_fn(self.proxy.clone()))
                    .expect("failed to spawn first pane");
                (vec![tab], 0, 1000)
            }
        };

        Gpu {
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
            broadcast: false,
            scroll_accum: 0.0,
            hover_url: None,
            copy_mode: None,
            scroll_glide: None,
            cursor_trail: Vec::new(),
            last_cursor_rect: None,
            font_px,
            applied_font: (
                self.config.font.family.clone(),
                self.config.font.size,
                self.config.font.ligatures,
            ),
            translucent,
            status_bar: self.config.window.status_bar,
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
        }
    }
}

/// The config file's last-modified time, or `None` if it does not exist yet. Used
/// by hot-reload to notice edits.
fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(Config::path()).and_then(|m| m.modified()).ok()
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
            WindowEvent::RedrawRequested => gpu.render(&self.config),
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
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. }
                if event.state == ElementState::Pressed =>
            {
                gpu.on_key(event, self.mods, &self.config, &self.keymap, event_loop);
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
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(wait),
            ));
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
        for tab in &mut self.tabs {
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
    fn periodic(&mut self, config: &Config) {
        // Bells are checked here (not only in render) so an occluded or minimized
        // window — the case where the urgency hint matters most — still raises it;
        // render early-returns when the surface is hidden. Cheap: one u64 compare
        // per pane.
        self.check_bells();

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
