mod actions;
mod ai;
mod boxdraw;
mod catchup;
mod clipboard;
mod config;
mod control;
mod daemon;
mod dnd;
mod docs;
mod docker;
mod explorer;
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
mod mpris;
mod overlay;
mod pane;
mod platform;
mod player;
mod project_session;
mod pty;
mod render;
mod selection;
mod share;
mod session;
mod settings;
mod shell_integration;
mod tab;
mod themes;
mod tidal;
mod verbs;
mod warroom;
mod watch;
mod whisper;
mod zsa;

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

/// Whether an armed leader layer has outlived its step. Both halves have to be
/// there: nothing armed is nothing to lapse, and no timeout means the layer waits
/// as long as the user does.
pub fn leader_lapsed(armed: Option<Instant>, timeout: Option<Duration>) -> bool {
    matches!((armed, timeout), (Some(since), Some(limit)) if since.elapsed() >= limit)
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
    /// An answer from a TIDAL worker, tagged with the request sequence.
    Tidal(u64, Result<TidalAnswer, String>),
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
    /// One directory the explorer sidebar asked for: the tab it belongs to, the
    /// tree generation that asked, the directory, and its entries. Off the UI
    /// thread because `read_dir` of a huge or networked directory drops frames.
    Explorer(usize, u64, PathBuf, Vec<explorer::Entry>),
    /// A file the explorer asked to view, read on a worker: text, decoded image art,
    /// or why it could not be shown.
    FileRead(PathBuf, explorer::ViewRead),
    /// A delete confirm whose label had to be counted first (a tree walk), for the
    /// tab that asked.
    ExplorerConfirm(usize, String),
    /// One path's properties, read on a worker.
    ExplorerProps(Result<explorer::Props, String>),
    /// Data or a command result for the docker panel, tagged with the request
    /// sequence so a snapshot of one host never paints under another.
    Docker(u64, docker::PanelMsg),
    /// What git says about the explorer's tree — a badge per changed file and the
    /// ignored set — for the tab and tree generation that asked. Two `git`
    /// processes, so never on the UI thread.
    ExplorerGit(usize, u64, PathBuf, explorer::GitMarks),
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
        // `runnir --zsa-map <revision> [layer]` — what the ZSA keyboard would light
        // for the leader's top level, as a table. Answers the only question that
        // cannot be checked by reading code ("is LED 19 really the g key on THIS
        // board?") without touching the keyboard, and it is how the layout reader was
        // verified against a board measured by hand.
        Some("--zsa-map") => {
            let Some(revision) = args.get(2) else {
                eprintln!(
                    "usage: runnir --zsa-map <revision> [layer]\n  \
                     revision: the half after the slash in the firmware version\n  \
                     (kontroll status -> 'Firmware version: L4g4A/Jad5YO' -> Jad5YO)"
                );
                return;
            };
            let layer: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            return zsa_map(revision, layer);
        }
        // `runnir --zsa-paint <revision> [seconds]` — paint the leader's top level on
        // the real board, hold it, put it back. The whole of step 3 end to end,
        // without the terminal being involved, so the keyboard half can be looked at
        // on its own before anything is wired to a keystroke.
        Some("--zsa-paint") => {
            let Some(revision) = args.get(2) else {
                return eprintln!("usage: runnir --zsa-paint <revision> [seconds]");
            };
            let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);
            return zsa_paint(revision, secs);
        }
        // `runnir --tidal-login` — sign in to TIDAL by the device flow. Prints the code
        // to approve and waits. Separate from the terminal for the same reason
        // `--zsa-paint` is: it is the half that needs a human and a service to both be
        // there, and it must be checkable without a window in the way.
        // `runnir --tidal-login --import` reads a session out of pasted text on stdin.
        // The flows above are gated by what the client id is registered FOR; a refresh
        // token is not, so this is the way in when both of them are refused.
        Some("--tidal-login") if args.get(2).map(String::as_str) == Some("--import") => {
            return tidal_import();
        }
        // `--app` asks for the sign-in that lands on TIDAL's own page instead of on a
        // local listener, for when a client id will not accept a loopback redirect.
        // The code then has to be pasted back, which is worse and is why it is not the
        // default — but a sign-in that works beats a nicer one that does not.
        Some("--tidal-login") if args.get(2).map(String::as_str) == Some("--app") => {
            let (_, creds) = match tidal_creds() {
                Ok(v) => v,
                Err(e) => return eprintln!("runnir: {e}"),
            };
            return tidal_login_paste(&creds);
        }
        Some("--tidal-login") => return tidal_login(args.get(2).map(String::as_str)),
        // `runnir --tidal-play <track-id|search words>` — fetch, decode and play one
        // track, then report the signal path it came out on. This is how the audio
        // chain gets verified: whether the DAC really took the stream untouched is not
        // something a test can answer.
        Some("--tidal-play") => {
            let what = args[2..].join(" ");
            if what.is_empty() {
                return eprintln!("usage: runnir --tidal-play <track-id|search words>");
            }
            return tidal_play(&what);
        }
        // `runnir --tidal-devices` — what the output chain would try, in order, for the
        // current config. Answers "why is it not bit-perfect" without playing anything.
        Some("--tidal-devices") => return tidal_devices(),
        // The player process itself. Started by a window that finds no daemon running,
        // never by a person — which is why it is not in --help.
        Some("--player-daemon") => {
            let (cfg, creds) = match tidal_creds() {
                Ok(v) => v,
                Err(e) => return eprintln!("runnir: {e}"),
            };
            return daemon::main(cfg, creds);
        }
        // `runnir --tidal-browse <words>` — exercises the whole catalogue layer in one
        // go: the four search types, the user's playlists and favourites, an album's
        // tracks, an artist's top tracks, and whether a track has timed lyrics. The
        // unit tests cover the parsing; only this covers the SHAPE the service really
        // answers with, which is the half that changes without warning.
        Some("--tidal-browse") => {
            let what = args[2..].join(" ");
            if what.is_empty() {
                return eprintln!("usage: runnir --tidal-browse <words>");
            }
            return tidal_browse(&what);
        }
        // `runnir --tidal-info <track-id|search words>` — what TIDAL would serve for one
        // track at each quality tier: which manifest shape, which codec, what depth and
        // rate. Makes no sound, which is the point: the questions "is this really
        // hi-res" and "which manifest does this tier use" should not require playing a
        // song out loud to answer.
        Some("--tidal-info") => {
            let what = args[2..].join(" ");
            if what.is_empty() {
                return eprintln!("usage: runnir --tidal-info <track-id|search words>");
            }
            return tidal_info(&what);
        }
        // `runnir --tidal-decode [--play] <file…>` — run local files through the same
        // decoder and output chain a TIDAL stream takes. Several files are read as one
        // stream, which is what a DASH init segment plus its media segments are.
        //
        // Without `--play` nothing is opened and nothing is heard: it measures the
        // decode alone. That separation is the point — it tells "this stream does not
        // decode" apart from "this device will not take it", which are otherwise the
        // same silence.
        Some("--tidal-decode") => {
            let mut files: Vec<&str> = args[2..].iter().map(String::as_str).collect();
            let play = files.first() == Some(&"--play");
            if play {
                files.remove(0);
            }
            if files.is_empty() {
                return eprintln!("usage: runnir --tidal-decode [--play] <file…>");
            }
            return tidal_decode(&files, play);
        }
        Some("--version" | "-v") => return println!("runnir {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h") => return print_help(),
        Some("--demo") => {
            let path = args.get(2).map(String::as_str).unwrap_or("/tmp/runnir-demo.png");
            // A third argument names the leader level to draw the which-key panel
            // for: "" is the root, "t" the tabs group, and so on. Without it the
            // scene is the plain multi-pane one with the palette open.
            return match args.get(3).map(String::as_str) {
                // `git[:state]` draws the git panel over a real repository (the cwd)
                // instead of the leader layer, so its three columns can be looked at
                // without driving a window.
                Some(s) if s.starts_with("git") => {
                    git_scene(path, s.strip_prefix("git:").unwrap_or(""))
                }
                // `tidal` draws the music panel with a made-up library, so the colours
                // and the layout can be LOOKED at rather than reasoned about. Made up
                // on purpose: it needs no account, no network and no sound, and it can
                // show all three quality tiers at once, which a real library rarely
                // does — nearly everything on TIDAL is plain lossless.
                Some(s) if s.starts_with("tidal") => {
                    tidal_scene(path, s.strip_prefix("tidal:").unwrap_or(""))
                }
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
         launch, new-tab, close-tab, set-colors. Example: runnir @ send-text --text 'ls\\n'\n  \
         Driving runnir itself: key, click, drag, wheel, action — e.g.\n  \
         runnir @ action --id git_panel, runnir @ key --chord enter,\n  \
         runnir @ drag --col 40 --row 6 --to-col 60. They answer with what is\n  \
         on screen, so a script can check what it just did.\n\n\
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

/// Renders the git panel over the repository in the current directory, exactly as
/// the app draws it — same layout, same hit geometry, same data.
///
/// `state` picks what to show: `""` the log, `commit` a commit open with its file
/// column, `zoom` one file full width, `leader` the panel's menu, `leader f` one of
/// its groups. Used to look at the panel without driving a window, which is the
/// only way to check a three-column layout in a test run.
/// The music panel, drawn over a plain terminal, with a library invented for it.
fn tidal_scene(path_out: &str, state: &str) {
    use crate::render::Rect;
    let track = |title: &str, artist: &str, secs: u32, quality: &str| tidal::Track {
        id: title.len() as u64,
        title: title.into(),
        artist: artist.into(),
        album: String::new(),
        duration_secs: secs,
        quality: quality.into(),
    };
    let queue = vec![
        track("My Home Is In The Delta", "Muddy Waters", 240, "HI_RES_LOSSLESS"),
        track("Dreams", "Fleetwood Mac", 257, "HI_RES_LOSSLESS"),
        track("Ghost of Perdition", "Opeth", 629, "LOSSLESS"),
    ];
    let snapshot = player::Snapshot {
        queue: queue.clone(),
        index: 0,
        playing: true,
        position_secs: 87.0,
        signal: player::SignalPath {
            device: "hw:2,0".into(),
            rung: Some(player::Rung::BitPerfect),
            decoded_bits: 24,
            decoded_rate: 192_000,
            quality: "HI_RES_LOSSLESS".into(),
            ..Default::default()
        },
        // A frame of the wave: one value per column, so the bars stand where they are.
        wave: (0..player::WAVE_LEN)
            .map(|i| {
                let x = i as f32 / player::WAVE_LEN as f32 * std::f32::consts::TAU;
                0.45 + 0.4 * (x * 2.0).sin() * (x * 0.7).cos()
            })
            .collect(),
        ..Default::default()
    };

    let mut panel = overlay::TidalPanel::new(snapshot);
    panel.source = overlay::Source::Search;
    panel.query = "muddy".into();
    panel.editing = false;
    panel.crumb = None;
    panel.rows = vec![
        overlay::TidalRow::Heading("TRACKS".into()),
        overlay::TidalRow::Track(track("My Home Is In The Delta", "Muddy Waters", 240, "HI_RES_LOSSLESS")),
        overlay::TidalRow::Track(track("Mannish Boy", "Muddy Waters", 172, "LOSSLESS")),
        overlay::TidalRow::Track(track("Rollin' Stone", "Muddy Waters", 189, "LOSSLESS")),
        overlay::TidalRow::Track(track("Got My Mojo Working", "Muddy Waters", 168, "HIGH")),
        overlay::TidalRow::Heading("ALBUMS".into()),
        overlay::TidalRow::Album(tidal::Album {
            id: 1,
            title: "Folk Singer".into(),
            artist: "Muddy Waters".into(),
            tracks: 9,
            year: Some(1964),
            quality: "HI_RES_LOSSLESS".into(),
        }),
        overlay::TidalRow::Album(tidal::Album {
            id: 2,
            title: "The Best Of Muddy Waters".into(),
            artist: "Muddy Waters".into(),
            tracks: 12,
            year: Some(1958),
            quality: "LOSSLESS".into(),
        }),
        overlay::TidalRow::Heading("ARTISTS".into()),
        overlay::TidalRow::Artist(tidal::Artist { id: 3, name: "Muddy Waters".into() }),
        overlay::TidalRow::Heading("PLAYLISTS".into()),
        overlay::TidalRow::Playlist(tidal::Playlist {
            uuid: "x".into(),
            title: "Blues Essentials".into(),
            tracks: 40,
            owner: "TIDAL".into(),
            mine: false,
        }),
    ];
    panel.cursor = 1;
    if state == "queue" {
        panel.source = overlay::Source::Queue;
        panel.rows = queue.into_iter().map(overlay::TidalRow::Track).collect();
        panel.cursor = 0;
    }

    const WIDTH: u32 = 1400;
    const HEIGHT: u32 = 820;
    let overlay = Overlay::Tidal(panel);
    render::offscreen_scene(path_out, WIDTH, HEIGHT, 16.0, |r| {
        let (cw, ch) = r.cell_size();
        let cols = (WIDTH as f32 / cw) as usize;
        let rows = (HEIGHT as f32 / ch) as usize;
        let pen = Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), ..Pen::default() };
        let mut g = Grid::new(cols, rows);
        g.write_str(0, 0, "~/projects/runnir ❯ ", pen);
        let panes = vec![(g, Rect { x: 0.0, y: 0.0, w: WIDTH as f32, h: HEIGHT as f32 }, None, true)];

        let panels = overlay.render(cols, rows, &config::Theme::default());
        let specs: Vec<(Grid, Rect)> = panels
            .into_iter()
            .map(|p| {
                let rect = Rect {
                    x: p.col as f32 * cw,
                    y: p.row as f32 * ch,
                    w: p.grid.cols() as f32 * cw,
                    h: p.grid.rows() as f32 * ch,
                };
                (p.grid, rect)
            })
            .collect();
        (panes, Some(specs))
    });
}

fn git_scene(path_out: &str, state: &str) {
    use crate::render::Rect;
    let cwd = std::env::current_dir().unwrap_or_default();
    let Some(root) = git::repo_root(&cwd) else {
        eprintln!("runnir: --demo git needs to run inside a git repository");
        return;
    };
    let mut p = overlay::GitPanel::new(root.clone());
    p.log = git::log(&root, 60);
    p.files = git::status_files(&root);
    p.current_branch = git::head_branch(&root).unwrap_or_default();
    p.set_view(overlay::GitView::Log);

    let (want_commit, want_zoom) = (state.starts_with("commit"), state.starts_with("zoom"));
    if want_commit || want_zoom {
        // The newest commit that actually has a sha (graph-art rows do not).
        if let Some(sha) = p.log.iter().find(|c| !c.sha.is_empty()).map(|c| c.sha.clone()) {
            p.commit_files = git::commit_files(&root, &sha);
            p.enter_commit(sha.clone());
            p.commit_files = git::commit_files(&root, &sha);
            if let Some(f) = p.selected_commit_file().map(|f| f.path.clone()) {
                p.set_preview(git::show_file(&root, &sha, &f));
            }
            if want_zoom {
                p.toggle_zoom();
            }
        }
    } else if let Some(sha) = p.log.first().map(|c| c.sha.clone()) {
        p.set_preview(git::show(&root, &sha));
    }
    if let Some(rest) = state.strip_prefix("leader") {
        p.arm_leader();
        for c in rest.trim().chars() {
            p.leader_key(c);
        }
    }

    const WIDTH: u32 = 1400;
    const HEIGHT: u32 = 820;
    let overlay = Overlay::Git(p);
    render::offscreen_scene(path_out, WIDTH, HEIGHT, 16.0, |r| {
        let (cw, ch) = r.cell_size();
        let cols = (WIDTH as f32 / cw) as usize;
        let rows = (HEIGHT as f32 / ch) as usize;
        // A plain terminal behind it, so the panel is seen the way it is used:
        // over something, dimmed.
        let pen = Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), ..Pen::default() };
        let mut g = Grid::new(cols, rows);
        g.write_str(0, 0, "~/projects/runnir ❯ ", pen);
        let panes = vec![(g, Rect { x: 0.0, y: 0.0, w: WIDTH as f32, h: HEIGHT as f32 }, None, true)];

        let panels = overlay.render(cols, rows, &config::Theme::default());
        let specs: Vec<(Grid, Rect)> = panels
            .into_iter()
            .map(|p| (p.grid, Rect { x: p.col as f32 * cw, y: p.row as f32 * ch, w: 0.0, h: 0.0 }))
            .collect();
        (panes, Some(specs))
    });
}

/// The credentials from the config, or a message explaining what is missing. Every
/// TIDAL entry point starts here, because "the panel does not exist without
/// credentials" has to be one decision made in one place.
fn tidal_creds() -> Result<(config::Tidal, tidal::Creds), String> {
    let cfg = Config::load().tidal;
    if !cfg.configured() {
        return Err(format!(
            "no TIDAL credentials.\n  \
             Put them in {} under [tidal]:\n    \
             client_id = \"...\"\n    \
             client_secret = \"...\"   # or set {} in the environment",
            Config::path().display(),
            cfg.client_secret_env
        ));
    }
    let creds = tidal::Creds {
        client_id: cfg.client_id.clone(),
        client_secret: cfg.client_secret(),
    };
    Ok((cfg, creds))
}

/// Signs in.
///
/// Two flows, chosen by what the credentials are registered for rather than by a flag.
/// The device flow is nicer — a code, no browser — but TIDAL only allows it for clients
/// registered as limited-input devices, and a web player client id is refused with
/// `sub_status 1002`. Rather than make the user know which kind they pasted, the device
/// flow is tried first and the refusal switches to PKCE automatically.
///
/// PKCE needs the code the browser was redirected with, so it runs in two commands:
/// this one prints the URL, and the same command with the pasted URL finishes it.
fn tidal_login(pasted: Option<&str>) {
    let (_, creds) = match tidal_creds() {
        Ok(v) => v,
        Err(e) => return eprintln!("runnir: {e}"),
    };

    // Second half of a PKCE sign-in: the browser has been and this is what it showed.
    if let Some(pasted) = pasted {
        let Some(pkce) = tidal::Pkce::load() else {
            return eprintln!("runnir: no sign-in is in progress — run: runnir --tidal-login");
        };
        let Some(code) = tidal::code_from_redirect(pasted) else {
            return eprintln!(
                "runnir: no grant code in that URL.\n  \
                 Paste the whole address the browser ended on, the one with ?code=… in it."
            );
        };
        match tidal::finish_pkce(&creds, &pkce, &code) {
            Ok(session) => {
                tidal::Pkce::clear();
                if let Err(e) = session.save() {
                    return eprintln!("runnir: signed in but could not save the session: {e}");
                }
                println!("  Signed in ({}).", session.country_code);
            }
            Err(e) => eprintln!("runnir: could not complete sign-in: {e}"),
        }
        return;
    }

    let auth = match tidal::start_device_auth(&creds) {
        Ok(a) => a,
        Err(e) if tidal::is_not_a_device_client(&e) => return tidal_login_pkce(&creds),
        Err(e) => return eprintln!("runnir: could not start sign-in: {e}"),
    };
    println!("\n  Open {}\n  and enter this code:\n", auth.verification_uri);
    println!("      {}\n", auth.user_code);
    println!("  Waiting (up to {}s)…", auth.expires_in);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(auth.expires_in);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(auth.interval));
        match tidal::poll_device_token(&creds, &auth.device_code) {
            Ok(tidal::Poll::Pending) => continue,
            Ok(tidal::Poll::Granted(session)) => {
                if let Err(e) = session.save() {
                    return eprintln!("runnir: signed in but could not save the session: {e}");
                }
                let where_ = tidal::Session::path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                println!("  Signed in ({}). Session saved to {where_}", session.country_code);
                return;
            }
            Err(e) => return eprintln!("runnir: sign-in failed: {e}"),
        }
    }
    eprintln!("runnir: the code expired before it was approved");
}

/// Adopts a session pasted on stdin.
fn tidal_import() {
    let (cfg, mut creds) = match tidal_creds() {
        Ok(v) => v,
        Err(e) => return eprintln!("runnir: {e}"),
    };
    let mut text = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
        return eprintln!("runnir: could not read stdin: {e}");
    }
    let Some(imported) = tidal::parse_import(&text) else {
        return eprintln!(
            "runnir: no refresh_token in that text.\n  \
             Paste the session from a signed-in TIDAL web player — it must contain a\n  \
             refresh_token, and ideally the client_id it was issued to."
        );
    };
    // A refresh token belongs to the client id it was issued to. Using the configured
    // one instead would be refused, so the pasted id wins when there is one.
    if let Some(id) = imported.client_id.clone() {
        if id != creds.client_id {
            println!("  using the client id from the pasted session ({id})");
            creds.client_id = id;
        }
    }
    let _ = cfg;
    match tidal::adopt(&creds, &imported) {
        Ok(session) => {
            if let Err(e) = session.save() {
                return eprintln!("runnir: signed in but could not save the session: {e}");
            }
            println!("  Signed in ({}).", session.country_code);
            if imported.client_id.as_deref() != Some(creds.client_id.as_str()) {
                return;
            }
            println!("  Put that client_id in [tidal] so refreshes keep working.");
        }
        Err(e) => eprintln!("runnir: that session was refused: {e}"),
    }
}

/// The browser sign-in: open a link, wait for the redirect to come back here.
///
/// The loopback redirect is what makes it a real callback — the browser returns to a
/// listener runnir is holding open, so nothing has to be copied out of an address bar.
/// If TIDAL refuses that redirect for this client id, the flow falls back to its own
/// app redirect and the code does have to be pasted; that path is kept working rather
/// than removed, because which one a client id allows is TIDAL's decision, not ours.
fn tidal_login_pkce(creds: &tidal::Creds) {
    let port = Config::load().tidal.callback_port;
    let redirect = tidal::loopback_redirect(port);
    let pkce = match tidal::start_pkce(creds, &redirect) {
        Ok(p) => p,
        Err(e) => return eprintln!("runnir: could not start sign-in: {e}"),
    };
    // Saved before the browser opens: if this process is interrupted, the paste-back
    // form of the same sign-in still works.
    if let Err(e) = pkce.save() {
        return eprintln!("runnir: could not remember the sign-in: {e}");
    }

    println!("\n  Opening TIDAL in your browser. Log in there and this will finish itself.\n");
    println!("  {}\n", pkce.authorize_url);
    if let Err(e) = open_in_browser(&pkce.authorize_url) {
        println!("  (could not open a browser here: {e} — the link above still works)\n");
    }
    println!("  Waiting for the callback on {redirect} …");

    match tidal::wait_for_callback(port, std::time::Duration::from_secs(300)) {
        Ok(code) => match tidal::finish_pkce(creds, &pkce, &code) {
            Ok(session) => {
                tidal::Pkce::clear();
                match session.save() {
                    Ok(()) => println!("  Signed in ({}).", session.country_code),
                    Err(e) => eprintln!("runnir: signed in but could not save it: {e}"),
                }
            }
            Err(e) => eprintln!("runnir: could not complete sign-in: {e}"),
        },
        Err(e) => eprintln!(
            "runnir: {e}\n  \
             If TIDAL showed an error instead of coming back, this client id may not\n  \
             accept a loopback redirect. Paste the address it ended on:\n     \
             runnir --tidal-login '<address>'"
        ),
    }
}

/// The sign-in that comes back through a page of TIDAL's own, so the grant code has to
/// be copied out of the address bar and handed over in a second command.
fn tidal_login_paste(creds: &tidal::Creds) {
    let pkce = match tidal::start_pkce(creds, tidal::APP_REDIRECT) {
        Ok(p) => p,
        Err(e) => return eprintln!("runnir: could not start sign-in: {e}"),
    };
    if let Err(e) = pkce.save() {
        return eprintln!("runnir: could not remember the sign-in: {e}");
    }
    println!("\n  1. Log in here:\n\n  {}\n", pkce.authorize_url);
    let _ = open_in_browser(&pkce.authorize_url);
    println!(
        "  2. The browser lands on a tidal.com page that looks empty or broken. That is\n     \
         expected — the grant code is in its ADDRESS BAR.\n\n  \
         3. Copy that whole address and run:\n\n     \
         runnir --tidal-login '<the address you copied>'\n"
    );
}

/// Opens a URL in whatever the desktop considers a browser.
fn open_in_browser(url: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// A signed-in session and the track `what` names, for the commands that need both.
///
/// A bare number is a track id; anything else is something to search for, because
/// typing a track id is not how anyone finds music.
fn tidal_find(what: &str) -> Result<(config::Tidal, tidal::Session, tidal::Track), String> {
    let (cfg, creds) = tidal_creds()?;
    let session = tidal::Session::load()
        .ok_or_else(|| "not signed in — run: runnir --tidal-login".to_string())?;
    let session = tidal::ensure_fresh(&creds, &session)
        .map_err(|e| format!("could not refresh the session: {e}"))?;
    let track = match what.parse::<u64>() {
        Ok(id) => tidal::track(&session, id)?,
        Err(_) => tidal::search_tracks(&session, what, 1)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("nothing found for {what:?}"))?,
    };
    Ok((cfg, session, track))
}

/// Walks the catalogue once and prints what came back.
fn tidal_browse(what: &str) {
    let (_, creds) = match tidal_creds() {
        Ok(v) => v,
        Err(e) => return eprintln!("runnir: {e}"),
    };
    let Some(session) = tidal::Session::load() else {
        return eprintln!("runnir: not signed in — run: runnir --tidal-login");
    };
    let session = match tidal::ensure_fresh(&creds, &session) {
        Ok(s) => s,
        Err(e) => return eprintln!("runnir: {e}"),
    };

    let found = match tidal::search(&session, what, 5) {
        Ok(f) => f,
        Err(e) => return eprintln!("runnir: search failed: {e}"),
    };
    println!(
        "  search {what:?} -> {} tracks, {} albums, {} artists, {} playlists",
        found.tracks.len(),
        found.albums.len(),
        found.artists.len(),
        found.playlists.len()
    );
    for t in found.tracks.iter().take(3) {
        println!("    track    {:<14} {} — {}", t.quality, t.artist, t.title);
    }
    for a in found.albums.iter().take(3) {
        println!(
            "    album    {:<14} {} — {} ({}, {} tracks)",
            a.quality,
            a.artist,
            a.title,
            a.year.map(|y| y.to_string()).unwrap_or_else(|| "?".into()),
            a.tracks
        );
    }
    for a in found.artists.iter().take(3) {
        println!("    artist   {:<14} {}", "", a.name);
    }
    for p in found.playlists.iter().take(3) {
        println!(
            "    playlist {:<14} {} by {} ({} tracks)",
            if p.mine { "mine" } else { "" },
            p.title,
            p.owner,
            p.tracks
        );
    }

    if let Some(album) = found.albums.first() {
        match tidal::album_tracks(&session, album.id) {
            Ok(tracks) => println!("\n  album {:?}: {} tracks", album.title, tracks.len()),
            Err(e) => println!("\n  album tracks failed: {e}"),
        }
    }
    if let Some(artist) = found.artists.first() {
        match tidal::artist_top_tracks(&session, artist.id) {
            Ok(tracks) => println!("  artist {:?}: {} top tracks", artist.name, tracks.len()),
            Err(e) => println!("  artist top tracks failed: {e}"),
        }
    }
    match tidal::my_playlists(&session) {
        Ok(mine) => {
            println!("  my playlists: {}", mine.len());
            for p in mine.iter().take(3) {
                println!("    {} ({} tracks)", p.title, p.tracks);
            }
        }
        Err(e) => println!("  my playlists failed: {e}"),
    }
    match tidal::favourite_tracks(&session) {
        Ok(tracks) => println!("  favourite tracks: {}", tracks.len()),
        Err(e) => println!("  favourites failed: {e}"),
    }
    if let Some(track) = found.tracks.first() {
        match tidal::lyrics(&session, track.id) {
            Ok(l) if l.timed.is_empty() && l.plain.is_empty() => {
                println!("  lyrics for {:?}: none", track.title)
            }
            Ok(l) => println!(
                "  lyrics for {:?}: {} timed lines, {} chars plain",
                track.title,
                l.timed.len(),
                l.plain.len()
            ),
            Err(e) => println!("  lyrics for {:?}: {e}", track.title),
        }
    }
}

/// What TIDAL would serve for one track, at each tier. Plays nothing.
fn tidal_info(what: &str) {
    let (_, session, track) = match tidal_find(what) {
        Ok(v) => v,
        Err(e) => return eprintln!("runnir: {e}"),
    };
    println!("  {} — {} [{}]  id {}", track.artist, track.title, track.album, track.id);
    println!("  the tier TIDAL lists for it: {}\n", track.quality);

    for quality in [
        config::Quality::HiResLossless,
        config::Quality::Lossless,
        config::Quality::High,
    ] {
        print!("  {:<18} ", quality.as_api());
        match tidal::stream_info(&session, track.id, quality.as_api()) {
            Ok(info) => {
                let shape = match info.media.as_ref() {
                    Some(tidal::Media::Direct(urls)) => format!("BTS, {} url(s)", urls.len()),
                    Some(tidal::Media::Dash { init, segments }) => format!(
                        "DASH, {} segment(s){}",
                        segments.len(),
                        if init.is_some() { " + init" } else { "" }
                    ),
                    None => "no media".to_string(),
                };
                println!(
                    "served {:<16} {:<22} {} bit / {} Hz  codec {}",
                    info.quality,
                    shape,
                    info.bit_depth.map(|b| b.to_string()).unwrap_or_else(|| "?".into()),
                    info.sample_rate.map(|r| r.to_string()).unwrap_or_else(|| "?".into()),
                    if info.codec.is_empty() { "?" } else { info.codec.as_str() },
                );
            }
            Err(e) => println!("refused: {e}"),
        }
    }
}

/// Plays one track end to end and reports the path the audio actually took.
fn tidal_play(what: &str) {
    let (cfg, _session, track) = match tidal_find(what) {
        Ok(v) => v,
        Err(e) => return eprintln!("runnir: {e}"),
    };
    let session = match tidal::Session::load() {
        Some(s) => s,
        None => return eprintln!("runnir: not signed in"),
    };
    println!(
        "  {} — {} [{}]  ({}, {}:{:02})",
        track.artist,
        track.title,
        track.album,
        track.quality,
        track.duration_secs / 60,
        track.duration_secs % 60
    );

    let info = match tidal::stream_info(&session, track.id, cfg.quality.as_api()) {
        Ok(i) => i,
        Err(e) => return eprintln!("runnir: no stream for this track: {e}"),
    };
    println!(
        "  manifest {} · codec {} · {} bit / {} Hz",
        info.mime,
        if info.codec.is_empty() { "?" } else { info.codec.as_str() },
        info.bit_depth.map(|b| b.to_string()).unwrap_or_else(|| "?".into()),
        info.sample_rate.map(|r| r.to_string()).unwrap_or_else(|| "?".into()),
    );

    // Reported the moment the device opens rather than when the track ends: "which
    // rung did it land on" is the question being asked, and waiting four minutes for
    // the answer makes the command useless for the thing it exists to check.
    let mut announced = false;
    let played = player::play_parts(
        match player::parts_of(&info) {
            Ok(p) => p,
            Err(e) => return eprintln!("runnir: {e}"),
        },
        player::hint_for(&info),
        &info.quality,
        &cfg,
        true,
        &mut None,
        &mut |progress| {
            if !announced {
                if let Some(signal) = progress.signal {
                    if signal.rung.is_some() {
                        announced = true;
                        for (device, why) in &signal.refused {
                            println!("  skipped {device}: {why}");
                        }
                        println!("  {}", signal.badge());
                    }
                }
            }
            player::Flow::Continue
        },
    );

    match played {
        Ok(played) => {
            println!(
                "  {} frames ({}:{:02}){}",
                played.frames,
                played.frames / played.signal.decoded_rate.max(1) as u64 / 60,
                played.frames / played.signal.decoded_rate.max(1) as u64 % 60,
                if played.underruns > 0 {
                    format!(", {} underruns", played.underruns)
                } else {
                    String::new()
                }
            );
        }
        Err(e) => eprintln!("runnir: playback failed: {e}"),
    }
}

/// Runs local files through the decoder, and optionally through the output chain.
fn tidal_decode(files: &[&str], play: bool) {
    let cfg = Config::load().tidal;
    let parts: Vec<player::Part> =
        files.iter().map(|f| player::Part::File(std::path::PathBuf::from(f))).collect();
    // The first file names the container for the whole stream: an init segment and its
    // media segments are one MP4, not several files that each stand alone.
    let ext = std::path::Path::new(files[0])
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("flac");
    let ext = if matches!(ext, "m4s" | "m4a" | "mp4") { "mp4" } else { ext };

    match player::play_parts(parts, ext, "", &cfg, play, &mut None, &mut |_| player::Flow::Continue) {
        Ok(played) => {
            let seconds = played.frames as f64 / played.signal.decoded_rate.max(1) as f64;
            println!("  {}", played.signal.badge());
            println!("  {} frames ({seconds:.2} s)", played.frames);
            if played.underruns > 0 {
                println!("  {} underruns", played.underruns);
            }
        }
        Err(e) => eprintln!("runnir: {e}"),
    }
}

/// Prints the output chain for the current config, without playing anything.
fn tidal_devices() {
    let cfg = Config::load().tidal;
    let devices = player::hw_devices_public();
    if devices.is_empty() {
        println!("  no playback devices found under /proc/asound");
    }
    for d in &devices {
        println!(
            "  found {:<10} {}{}",
            d.name,
            d.label,
            if d.is_display { "   (display — only reachable by name)" } else { "" }
        );
    }
    let names = player::auto_candidates(&devices);
    println!("\n  chain for output = {:?}, bit_perfect = {}:", cfg.output, cfg.bit_perfect);
    for (i, attempt) in player::plan(&cfg.output, cfg.bit_perfect, &names).iter().enumerate() {
        println!(
            "   {}. {:<14} {}",
            i + 1,
            attempt.device,
            match (attempt.exact, attempt.same_rate) {
                (true, _) => "exact rate and depth (bit-perfect)",
                (false, true) => "same rate, wider container allowed",
                (false, false) => "whatever it takes",
            }
        );
    }
}

/// Prints which LED each key of the leader's top level sits under, for `revision`
/// of the flashed layout with `layer` active. Read-only, and it never touches the
/// keyboard: the layout comes out of Keymapp's database.
fn zsa_map(revision: &str, layer: usize) {
    let Some(db) = zsa::default_db() else {
        return eprintln!("runnir: no config directory to find keymapp's database in");
    };
    let Some(layout) = zsa::read_layout(&db, revision) else {
        return eprintln!(
            "runnir: could not read revision {revision} from {}\n  \
             (is keymapp installed, and is sqlite3 on PATH?)",
            db.display()
        );
    };
    let keymap = actions::Keymap::new(&std::collections::HashMap::new(), &Config::default().leader);
    let keys = keymap.leader_level_keys(&[]);
    let spellings: Vec<&str> = keys.iter().map(|(s, _)| s.as_str()).collect();
    let leds: std::collections::HashMap<&str, u8> =
        layout.leds_for(spellings, layer).into_iter().collect();

    println!("revision {revision}, layer {layer}, {} layers in the layout", layout.layers());
    let (mut lit, mut dark) = (0, 0);
    for (spelling, is_group) in &keys {
        let kind = if *is_group { "group" } else { "leaf " };
        match leds.get(spelling.as_str()) {
            Some(led) => {
                lit += 1;
                println!("  {spelling:<9} {kind} LED {led}");
            }
            None => {
                dark += 1;
                println!("  {spelling:<9} {kind} -- not on this board");
            }
        }
    }
    println!("{lit} keys would light, {dark} have no key on this layout");
}

/// Paints the leader's top level on the keyboard for `secs`, then restores it.
///
/// The sustain is set past the hold on purpose: if this process is killed while the
/// board is lit, Keymapp still puts it back on its own. That is the property the whole
/// feature rests on, so the probe that demonstrates it should rely on it too.
fn zsa_paint(revision: &str, secs: u64) {
    let Some(board) = zsa::Board::start() else {
        return eprintln!("runnir: no kontroll on PATH — install it with `cargo install kontroll`");
    };
    let Some(layout) = zsa::default_db().and_then(|db| zsa::read_layout(&db, revision)) else {
        return eprintln!("runnir: could not read revision {revision} (is keymapp installed?)");
    };
    let config = Config::default();
    let palette = config.theme.leader_palette();
    let keymap = actions::Keymap::new(&std::collections::HashMap::new(), &config.leader);
    let keys = keymap.leader_level_keys(&[]);

    let spellings: Vec<&str> = keys.iter().map(|(s, _)| s.as_str()).collect();
    let groups: std::collections::HashSet<&str> =
        keys.iter().filter(|(_, g)| *g).map(|(s, _)| s.as_str()).collect();
    let leds: Vec<(u8, config::Rgb)> = layout
        .leds_for(spellings, 0)
        .into_iter()
        .map(|(s, led)| (led, if groups.contains(s) { palette.group } else { palette.leaf }))
        .collect();

    println!("painting {} keys for {secs}s, then restoring", leds.len());
    let sustain = (secs as u32 + 5) * 1000;
    board.paint(leds, palette.background, sustain);
    std::thread::sleep(Duration::from_secs(secs));
    board.restore();
    // The worker owns the channel; dropping the handle queues a restore too, but the
    // process has to outlive the thread doing the work.
    std::thread::sleep(Duration::from_millis(500));
    println!("restored");
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
    let palette = crate::config::Theme::default().leader_palette();
    let panel_rows = whichkey_grid(&entries, &labels, cols, &palette).rows() as f32;
    let height = ((panel_rows + TERM_ROWS + 2.0) * ch).ceil() as u32;

    render::offscreen_scene(path_out, WIDTH as u32, height, 16.0, |r| {
        let (cw, ch) = r.cell_size();
        let cols = (WIDTH / cw) as usize;
        let bar_h = ch;
        let height = height as f32;

        let panel = whichkey_grid(&entries, &labels, cols, &palette);
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
    /// The overlay the close confirm displaced, put back when the confirm is
    /// dismissed. The question has to be asked over whatever is up — it is about
    /// the whole window — but answering "no" must leave the screen as it was, with
    /// a half-typed prompt or a mid-edit settings panel still there.
    overlay_under_confirm: Option<Overlay>,
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
    /// This window's end of the player. The player itself lives in a daemon shared by
    /// every runnir on the session, so closing a window — any window but the last —
    /// does not stop the music. Connected on first use.
    jukebox: Option<daemon::Remote>,
    /// Search request counter, so an answer that arrives after a newer query is dropped
    /// instead of drawn over it.
    tidal_seq: u64,
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
    /// A git panel column separator being dragged (0 = list/files, 1 = files/diff).
    git_drag: Option<usize>,
    /// The same for the docker panel (0 = hosts/objects, 1 = objects/detail).
    docker_drag: Option<usize>,
    /// The explorer sidebar's edge is being dragged. The panes are reflowed when it
    /// is released, not while it moves.
    explorer_resizing: bool,
    /// A `git status` for the explorer is in flight. One at a time: the tree asks
    /// for one on every wake that looks stale, and a slow repository would otherwise
    /// stack them up.
    explorer_git_pending: bool,
    /// Request generation for the docker panel, the same guard the git panel uses.
    docker_gen: u64,
    /// What the explorer's marks were read at: the root, the repository stamp and
    /// the pane's command counter. Unchanged means there is nothing to re-read —
    /// the same two triggers the status bar uses, for the same reason.
    explorer_git_at: Option<(usize, PathBuf, u64, u64)>,
    /// The file the viewer is waiting for. A read that is no longer this one is
    /// dropped rather than drawn over whatever is on screen by the time it lands.
    pending_view: Option<PathBuf>,
    /// Permission bits waiting on a recursive-chmod confirm, since the confirm
    /// replaces the panel that was holding them.
    pending_mode: Option<u32>,
    /// A docker operation waiting on its confirm, and the command line waiting on
    /// the confirm for a remote or a `compose down`. Parked here for the same
    /// reason: the prompt replaces the panel that was holding them.
    pending_docker: Option<docker::Op>,
    pending_docker_cmd: Option<Vec<String>>,
    /// The docker panel itself while a confirm is up, so either answer puts it back.
    docker_stash: Option<overlay::DockerPanel>,
    /// Whether the pointer is currently over one, so the resize cursor is set once
    /// on the way in and once on the way out rather than on every motion event.
    git_over_split: bool,
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
    /// The ZSA keyboard, when there is one and the user asked for it. `None` covers
    /// both "no such keyboard" and "not enabled", which behave identically.
    board: Option<zsa::Board>,
    /// Where the middle button went down, for the tactile pipe: a middle DRAG hands
    /// a command block to another pane, a middle CLICK still pastes the primary
    /// selection.
    middle_press: Option<PhysicalPosition<f64>>,
    /// Frame counter for the map's screensaver, so the rain and the re-readings can
    /// run at different rates off one wake-up.
    map_frame: u32,
    /// Whether the map on screen put itself there. An unasked overlay must not eat
    /// the keystroke that dismisses it.
    screensaver: bool,
    /// What this project is really worked with, learned from successful commands.
    /// Loaded once; only written when it changes.
    verbs: verbs::Verbs,
    /// When a keystroke last reached a child process. The catch-up measures "away"
    /// from here rather than from window focus, which lies in both directions.
    last_pty_key: Instant,
    /// Whether the panes still owe the catch-up a baseline. Marking has to happen at
    /// the last KEYSTROKE, not when the panel opens (by then everything that moved has
    /// already moved, and every pane looks unchanged) and not when the absence is
    /// finally noticed either — see [`catchup::Baseline`].
    baseline: catchup::Baseline,
    /// The flashed layout, read once at startup: which LED sits under which key.
    /// Only loaded when the leader lights are on, since nothing else needs it.

    proxy: EventLoopProxy<UserEvent>,
}

/// Whole-board colours for the ambient signals. Chosen by MEANING, not by theme: the
/// board is not a surface runnir owns, and a colour there has to read the same way a
/// traffic light does — the DEVLOG entry on opaque keycaps is why these are the only
/// kind of signal this hardware can carry at all.
const FLASH_WATCH: config::Rgb = config::Rgb(0xff, 0x9a, 0x00);
const FLASH_DONE: config::Rgb = config::Rgb(0x00, 0xc8, 0x50);
const FLASH_GUARDIAN: config::Rgb = config::Rgb(0xff, 0x20, 0x20);

/// Fires a desktop notification (per-OS via `platform`). Silent on failure.
fn notify(body: &str) {
    platform::notify(body);
}

/// A PTY wake closure. Sends a user event through the proxy — the reliable way to
/// interrupt `ControlFlow::Wait` from another thread on Wayland — rather than
/// calling `Window::request_redraw` directly, which can be missed there.
/// What a TIDAL worker can come back with. Two shapes rather than one: a list and a
/// set of lyrics are drawn by different halves of the panel, and collapsing them into
/// one type would mean each side checking whether the answer was meant for it.
pub enum TidalAnswer {
    Found(tidal::Found),
    /// The track the words are for, and the words. The id travels with them because
    /// the answer can arrive after the song has changed, and words for the wrong song
    /// are worse than none.
    Lyrics(u64, tidal::Lyrics),
}

/// Turns a search result into the rows a list draws, headings and all.
///
/// The order is deliberate: tracks first because they are what a search is usually
/// for, then albums, artists and playlists. Headings only appear when there is more
/// than one kind, since a single-kind list needs no label.
fn rows_of(found: &tidal::Found) -> Vec<overlay::TidalRow> {
    let kinds = [
        !found.tracks.is_empty(),
        !found.albums.is_empty(),
        !found.artists.is_empty(),
        !found.playlists.is_empty(),
    ]
    .iter()
    .filter(|x| **x)
    .count();
    let mut rows = Vec::new();
    let heading = |rows: &mut Vec<overlay::TidalRow>, text: &str| {
        if kinds > 1 {
            rows.push(overlay::TidalRow::Heading(text.to_string()));
        }
    };
    if !found.tracks.is_empty() {
        heading(&mut rows, "TRACKS");
        rows.extend(found.tracks.iter().cloned().map(overlay::TidalRow::Track));
    }
    if !found.albums.is_empty() {
        heading(&mut rows, "ALBUMS");
        rows.extend(found.albums.iter().cloned().map(overlay::TidalRow::Album));
    }
    if !found.artists.is_empty() {
        heading(&mut rows, "ARTISTS");
        rows.extend(found.artists.iter().cloned().map(overlay::TidalRow::Artist));
    }
    if !found.playlists.is_empty() {
        heading(&mut rows, "PLAYLISTS");
        rows.extend(found.playlists.iter().cloned().map(overlay::TidalRow::Playlist));
    }
    rows
}

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

        // Is another runnir already on screen? Asked once, here, and used for both
        // reading and clearing the snapshot below: the window you closed belongs to
        // the next window you open, not to one opened beside a live one.
        let another_window = control::another_instance_running();

        // Restore, in order of precedence:
        //   1. this project's saved layout (opt-in `session_restore`), keyed by the
        //      nearest git ancestor of the launch cwd — layout + cwd only. This one
        //      is a TEMPLATE you saved on purpose, so a second window in the same
        //      project gets it too;
        //   2. otherwise the previous whole-window session (`restore_session`), and
        //      only when this is the only window running;
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
                let saved = session::should_restore(
                    self.config.behaviour.restore_session,
                    another_window,
                )
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
            overlay_under_confirm: None,
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
            jukebox: None,
            tidal_seq: 0,
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
            git_drag: None,
            docker_drag: None,
            explorer_resizing: false,
            explorer_git_pending: false,
            docker_gen: 0,
            explorer_git_at: None,
            pending_view: None,
            pending_mode: None,
            pending_docker: None,
            pending_docker_cmd: None,
            docker_stash: None,
            git_over_split: false,
            zoomed: None,
            bell_flash: None,
            status: None,
            status_expiry: None,
            // Started only when asked for: the worker spawns a thread and looks for
            // kontroll, and a terminal that does neither unless told to is the point.
            board: (self.config.keyboard.ambient || self.config.keyboard.leader_lights)
                .then(zsa::Board::start)
                .flatten(),

            middle_press: None,
            map_frame: 0,
            screensaver: false,
            verbs: verbs::Verbs::load(),
            last_pty_key: Instant::now(),
            baseline: catchup::Baseline::default(),
            proxy: self.proxy.clone(),
        };
        // The flashed layout, read once. Blocking (two processes: kontroll status and
        // sqlite3), so it happens here at startup and never on a keystroke.
        if self.config.keyboard.leader_lights {
            gpu.refresh_board_layout();
        }
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        match event {
            // A wake is also how the player says something changed, so a panel that is
            // open follows the music without polling it on a timer.
            UserEvent::Redraw => {
                gpu.refresh_tidal_panel_if_open();
                gpu.window.request_redraw();
            }
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
                // The keymap and the event loop go in because `key` and `action` run
                // real actions, which is what makes the panels scriptable.
                let resp = gpu.handle_control(req, &self.config, &self.keymap, event_loop);
                let _ = reply.send(resp);
            }
            UserEvent::Media(msg) => gpu.on_media_msg(msg, &self.config),
            UserEvent::Tidal(seq, found) => gpu.on_tidal_results(seq, found),
            UserEvent::GitPanel(seq, msg) => gpu.on_git_panel_msg(seq, msg, &self.config),
            UserEvent::Docker(seq, msg) => gpu.on_docker_msg(seq, msg),
            UserEvent::Explorer(tab, seq, dir, entries) => {
                gpu.on_explorer_read(tab, seq, dir, entries)
            }
            UserEvent::FileRead(path, read) => gpu.on_file_read(path, read),
            UserEvent::ExplorerConfirm(tab, label) => gpu.on_explorer_confirm(tab, label),
            UserEvent::ExplorerProps(props) => gpu.on_explorer_props(props),
            UserEvent::ExplorerGit(tab, seq, root, marks) => {
                gpu.on_explorer_git(tab, seq, root, marks)
            }
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
            // The window manager's close button, and the compositor keybinding that
            // does the same. It is one keystroke away from everything else, so it
            // asks before it kills whatever is still running in the window.
            WindowEvent::CloseRequested => {
                if !gpu.request_close(&self.config) {
                    return;
                }
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
            //
            // Unless another window is still open: the snapshot on disk is then
            // ITS layout, kept for when it closes, and this window was never the
            // one that owned it.
            if !control::another_instance_running() {
                session::Session::clear();
            }
            event_loop.exit();
            return;
        }
        gpu.periodic(&self.config);

        // Anything the TIDAL panel is waiting for animates a spinner, and a spinner
        // needs a clock: nothing wakes the window while a stream is being resolved,
        // because no state has changed yet. Ninety milliseconds is one frame.
        if matches!(&gpu.overlay, Some(Overlay::Tidal(p)) if p.is_waiting()) {
            gpu.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(90),
            ));
            return;
        }

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

        // Nothing typed for long enough: put the map up by itself. A screensaver you
        // have to ask for is a contradiction, and runnir already knows what "away"
        // means without believing window focus.
        let after = self.config.behaviour.screensaver_after_secs;
        if after > 0
            && gpu.overlay.is_none()
            && gpu.idle_for() >= Duration::from_secs(after)
        {
            gpu.show_map_as_screensaver();
        }

        // The map is a screensaver, so it has to keep moving on an idle terminal: the
        // rain falls, the clock ticks over, and the cards are re-read so what you
        // glance at on the way past is what is happening NOW, not what was happening
        // when you opened it.
        if gpu.tick_map() {
            gpu.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(90),
            ));
            return;
        }

        // An armed leader expires on a deadline nothing else wakes us for: on an idle
        // terminal the status-bar chip would stay lit long after the layer was gone.
        // Through `end_leader` like every other exit — clearing the fields here would
        // leave the keyboard painted with a level nobody is in until the board's own
        // dead-man expires. The wake itself is folded into `extra_wake` below rather
        // than returning early, which would stall the animations.
        if leader_lapsed(gpu.leader_armed, gpu.leader_timeout) {
            gpu.end_leader(&self.config);
            gpu.window.request_redraw();
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
    /// The area the PANES get: the window minus the chrome, minus the explorer
    /// sidebar's columns when the active tab has one open.
    ///
    /// Reserving here is what keeps the sidebar out of the layout tree: everything
    /// downstream — `Tab::layout`, `reflow`, the hit tests, the divider drags — asks
    /// this one question and never learns there is a sidebar at all.
    fn active_area(&self) -> Rect {
        let full = self.window_area();
        match self.tabs.get(self.active).and_then(|t| t.explorer.as_ref()) {
            Some(e) => e.reserve(full, self.renderer.cell_size()),
            None => full,
        }
    }

    /// The whole content area, sidebar included — what the sidebar itself is placed
    /// against.
    fn window_area(&self) -> Rect {
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

        // Follow the focused shell into another REPOSITORY (not into every
        // directory) with the explorer's root. Cheap: a cwd read and a compare.
        self.explorer_sync_root();

        // ...and ask git what it says about that tree when something looks to have
        // changed. Two `stat` calls per wake, no process unless one of them moved.
        self.explorer_poll_git();

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
            // The baseline the catch-up measures from: every pane's command counter as
            // of the last keystroke. Taken here, on the first sweep after a key, rather
            // than on the key itself — this loop already walks the panes, and locking
            // every grid on every keypress would put the typing behind whatever a busy
            // child is printing.
            //
            // EVERY tab, not just the active one: the watch scan below runs over all of
            // them, so a pane left with the baseline it had two tabs ago reports work
            // that finished yesterday as news, and an old watch hit outranks a real
            // failure the moment you switch to it.
            if self.baseline.take_due() {
                for tab in &mut self.tabs {
                    for pane in tab.panes.values_mut() {
                        pane.mark_catch_up_point();
                    }
                }
            }
            // The keyboard belongs to the whole desktop: runnir holds it only while
            // it has the window focus. The lapse itself is not checked here — it has
            // its own deadline wake in `about_to_wait`, which fires on the second
            // rather than on this half-second tick.
            if self.leader_armed.is_some() && !focused {
                self.end_leader(config);
                self.window.request_redraw();
            }
            // Collected rather than flashed inline: the panes are borrowed here, and
            // two signals in one sweep should be one flash, not a fight over the board.
            let mut flashes: Vec<config::Rgb> = Vec::new();
            // Collected here, applied after the pane loop: recording borrows the
            // window-level store while the panes are already borrowed.
            let mut learned: Vec<(std::path::PathBuf, String)> = Vec::new();
            for tab in &mut self.tabs {
                for pane in tab.panes.values_mut() {
                    pane.refresh_context(config);
                    // A command that ran longer than the threshold and finished
                    // while the window is unfocused earns a desktop notification.
                    if config.behaviour.notify_after_secs > 0 && !focused {
                        if let Some(msg) = pane.take_completion(config.behaviour.notify_after_secs) {
                            notify(&msg);
                            flashes.push(FLASH_DONE);
                        }
                    }
                    // Learn this repo's verbs from commands that SUCCEEDED. Failures
                    // teach the wrong thing, and the verb is extracted (arguments
                    // dropped) before anything reaches memory, let alone disk.
                    if config.verbs.enabled {
                        if let Some((line, 0)) = pane.take_finished_command() {
                            if let Some(root) =
                                pane.cwd().and_then(|d| crate::git::repo_root(&d))
                            {
                                learned.push((root, line));
                            }
                        }
                    }
                    // Keyword watch (W4): fires whether focused or not — it is an
                    // explicit "tell me when this appears" on a monitored pane.
                    if pane.watching().is_some() {
                        if let Some(hit) = pane.take_watch_hit() {
                            notify(&hit);
                            flashes.push(FLASH_WATCH);
                        }
                    }
                }
            }
            if !learned.is_empty() {
                for (root, line) in learned {
                    self.verbs.record(&root, &line);
                }
                self.verbs.save();
            }
            // The board says ONE thing at a time: the most urgent colour of this
            // sweep wins. Ordering is by list position, so watch beats done.
            if let Some(colour) = flashes.first().copied() {
                self.flash_board(colour, config);
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
