//! Now-playing media integration: metadata, transport control, album-art rendering
//! and a live audio waveform. All platform I/O is shelled out, so there is no heavy
//! D-Bus / CoreMedia Rust dependency to carry.
//!
//! - Linux uses `playerctl` (MPRIS) for metadata + control, and `cava` (optional)
//!   for the waveform.
//! - macOS uses `nowplaying-cli` when present, else AppleScript via `osascript`
//!   against Music / Spotify. Album art on macOS is skipped (best-effort).
//!
//! Every backend call is guarded so a missing tool (or no active player) degrades to
//! "no player" rather than an error, and every subprocess runs off the UI thread.

use std::path::PathBuf;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

/// Playback state, parsed from the backend's status string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

impl Status {
    /// Maps a backend status word ("Playing" / "Paused" / anything else) to a state.
    /// Unknown or empty is treated as stopped.
    fn parse(s: &str) -> Status {
        match s.trim().to_ascii_lowercase().as_str() {
            "playing" => Status::Playing,
            "paused" => Status::Paused,
            _ => Status::Stopped,
        }
    }
}

/// A snapshot of what a media player is currently playing, backend-independent.
#[derive(Clone, Debug)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A local cover-art file, when one is known and readable. Remote (http) art is
    /// skipped in v1 to keep the UI path off the network.
    pub art: Option<PathBuf>,
    pub status: Status,
}

impl NowPlaying {
    /// True when there is no track metadata worth showing. Used to treat an empty
    /// player (a live MPRIS bus with nothing loaded) as "no player".
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.artist.is_empty() && self.album.is_empty()
    }
}

/// A message a media worker delivers back to the UI thread via the event-loop proxy,
/// same wake pattern as the AI worker.
pub enum MediaMsg {
    /// Result of a metadata fetch. `None` means there is no active player.
    NowPlaying(Option<NowPlaying>),
    /// One waveform frame: a single amplitude byte (0..=255) per bar.
    Waveform(Vec<u8>),
}

// ---- metadata parsing (pure, unit-tested) ---------------------------------

/// Parses the newline-separated playerctl metadata format into a [`NowPlaying`].
///
/// The format string used is
/// `{{title}}\n{{artist}}\n{{album}}\n{{mpris:artUrl}}\n{{status}}`, but any field
/// can be blank and trailing fields can be missing entirely (an empty tag prints
/// nothing), so every line is read positionally and defaulted to empty.
pub fn parse_playerctl(out: &str) -> NowPlaying {
    let mut lines = out.split('\n');
    let title = lines.next().unwrap_or("").trim().to_string();
    let artist = lines.next().unwrap_or("").trim().to_string();
    let album = lines.next().unwrap_or("").trim().to_string();
    let art_url = lines.next().unwrap_or("").trim().to_string();
    let status = lines.next().unwrap_or("").trim();
    NowPlaying {
        title,
        artist,
        album,
        art: art_from_url(&art_url),
        status: Status::parse(status),
    }
}

/// Turns an MPRIS `mpris:artUrl` into a local path, but only when it is a readable
/// `file://` URL. Remote (`http` / `https`) art is deliberately not downloaded in v1
/// (no network on the UI path); anything else is ignored.
fn art_from_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // file:///path has an empty host; file://host/path carries one. In both cases the
    // filesystem path is what starts at the first slash.
    let path = match rest.find('/') {
        Some(0) => rest.to_string(),
        Some(i) => rest[i..].to_string(),
        None => return None,
    };
    let p = PathBuf::from(percent_decode(&path));
    p.exists().then_some(p)
}

/// Minimal percent-decoding for file URLs (spaces arrive as `%20`, etc.). An invalid
/// escape is left verbatim rather than dropped.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---- half-block album art (pure, unit-tested) -----------------------------

/// One character cell of half-block cover art. The upper half is drawn as the glyph
/// `▀` in `top`'s colour; the lower half shows through as the cell background in
/// `bottom`'s colour. So a single cell encodes two vertically-stacked pixels, which
/// squares up the usually 1:2 terminal cell aspect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HalfCell {
    pub top: (u8, u8, u8),
    pub bottom: (u8, u8, u8),
}

/// Renders RGBA cover-art pixels into a `cols` x `rows` grid of half-block cells.
/// Each cell packs two vertical pixels, so the source is sampled into `cols` x
/// `2*rows` points by nearest-neighbour centre sampling. Pure and deterministic — the
/// unit tests pin its dimensions and colours. Returns an empty vec if the target or
/// source is degenerate, or the buffer is too small for the stated dimensions.
pub fn halfblock_art(rgba: &[u8], width: u32, height: u32, cols: usize, rows: usize) -> Vec<Vec<HalfCell>> {
    if cols == 0 || rows == 0 || width == 0 || height == 0 {
        return Vec::new();
    }
    let (wz, hz) = (width as usize, height as usize);
    if rgba.len() < wz * hz * 4 {
        return Vec::new();
    }
    let sample_rows = rows * 2;
    let stride = wz * 4;
    let at = |px: usize, py: usize| -> (u8, u8, u8) {
        let px = px.min(wz - 1);
        let py = py.min(hz - 1);
        let i = py * stride + px * 4;
        (rgba[i], rgba[i + 1], rgba[i + 2])
    };
    // Nearest-neighbour centre sampling: source index for target step `t` of `n`.
    let map = |t: usize, n: usize, size: u32| ((t as u64 * 2 + 1) * size as u64 / (n as u64 * 2)) as usize;
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut line = Vec::with_capacity(cols);
        for c in 0..cols {
            let sx = map(c, cols, width);
            let top = at(sx, map(r * 2, sample_rows, height));
            let bottom = at(sx, map(r * 2 + 1, sample_rows, height));
            line.push(HalfCell { top, bottom });
        }
        out.push(line);
    }
    out
}

// ---- waveform bar glyphs (pure, unit-tested) ------------------------------

/// The eight vertical block glyphs, shortest to tallest, for a waveform bar.
const BLOCKS: [char; 8] = ['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];

/// Maps a cava amplitude byte (0..=255) to a vertical block glyph. Zero is a blank
/// space, so silence reads as an empty row instead of a solid floor; every non-zero
/// value shows at least the shortest block.
pub fn bar_block(v: u8) -> char {
    if v == 0 {
        return ' ';
    }
    let idx = ((v as usize - 1) * BLOCKS.len()) / 255;
    BLOCKS[idx.min(BLOCKS.len() - 1)]
}

// ---- fetch (metadata) -----------------------------------------------------

/// Fetches the current now-playing snapshot by shelling out to the platform backend.
/// Blocking — call from a worker thread. `None` means no active player (or no backend
/// tool installed).
pub fn fetch() -> Option<NowPlaying> {
    fetch_impl()
}

#[cfg(target_os = "linux")]
fn fetch_impl() -> Option<NowPlaying> {
    let out = Command::new("playerctl")
        .args([
            "metadata",
            "--format",
            "{{title}}\n{{artist}}\n{{album}}\n{{mpris:artUrl}}\n{{status}}",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // no player, or playerctl not installed
    }
    let np = parse_playerctl(&String::from_utf8_lossy(&out.stdout));
    (!np.is_empty()).then_some(np)
}

#[cfg(target_os = "macos")]
fn fetch_impl() -> Option<NowPlaying> {
    // Prefer nowplaying-cli (whole-system Now Playing), else fall back to AppleScript.
    if let Some(np) = fetch_nowplaying_cli() {
        return Some(np);
    }
    fetch_applescript()
}

#[cfg(target_os = "macos")]
fn fetch_nowplaying_cli() -> Option<NowPlaying> {
    let out = Command::new("nowplaying-cli")
        .args(["get", "title", "artist", "album", "playbackRate"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut lines = s.lines();
    let clean = |v: &str| -> String {
        let v = v.trim();
        if v == "null" { String::new() } else { v.to_string() }
    };
    let title = clean(lines.next().unwrap_or(""));
    let artist = clean(lines.next().unwrap_or(""));
    let album = clean(lines.next().unwrap_or(""));
    let rate = lines.next().unwrap_or("").trim();
    if title.is_empty() && artist.is_empty() && album.is_empty() {
        return None;
    }
    // playbackRate 0 means paused; anything else is playing.
    let stopped = rate.is_empty() || rate.chars().all(|c| c == '0' || c == '.');
    let status = if stopped { Status::Paused } else { Status::Playing };
    Some(NowPlaying { title, artist, album, art: None, status })
}

#[cfg(target_os = "macos")]
fn fetch_applescript() -> Option<NowPlaying> {
    for app in ["Music", "Spotify"] {
        let script = format!(
            "tell application \"{app}\"\nif it is running then\ntry\nset t to name of current track\nset a to artist of current track\nset al to album of current track\nset st to player state as text\nreturn t & \"\\n\" & a & \"\\n\" & al & \"\\n\" & st\nend try\nend if\nend tell"
        );
        let out = match Command::new("osascript").arg("-e").arg(&script).output() {
            Ok(o) => o,
            Err(_) => return None, // osascript missing: give up entirely
        };
        if !out.status.success() {
            continue;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let mut lines = s.lines();
        let title = lines.next().unwrap_or("").trim().to_string();
        let artist = lines.next().unwrap_or("").trim().to_string();
        let album = lines.next().unwrap_or("").trim().to_string();
        let status = Status::parse(lines.next().unwrap_or(""));
        if title.is_empty() && artist.is_empty() && album.is_empty() {
            continue;
        }
        return Some(NowPlaying { title, artist, album, art: None, status });
    }
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn fetch_impl() -> Option<NowPlaying> {
    None
}

// ---- transport control (fire-and-forget) ----------------------------------

/// A transport command, backend-agnostic.
enum Ctl {
    PlayPause,
    Next,
    Prev,
    VolUp,
    VolDown,
}

pub fn play_pause() {
    control(Ctl::PlayPause);
}
pub fn next() {
    control(Ctl::Next);
}
pub fn prev() {
    control(Ctl::Prev);
}
/// Steps the player volume up (`up = true`) or down by a small fixed increment.
pub fn volume(up: bool) {
    control(if up { Ctl::VolUp } else { Ctl::VolDown });
}

/// Runs a control command on a detached worker thread so the UI never blocks on the
/// subprocess, and the child is reaped (no zombie).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_detached(mut cmd: Command) {
    use std::process::Stdio;
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    std::thread::spawn(move || {
        let _ = cmd.status();
    });
}

#[cfg(target_os = "linux")]
fn control(c: Ctl) {
    let mut cmd = Command::new("playerctl");
    match c {
        Ctl::PlayPause => {
            cmd.arg("play-pause");
        }
        Ctl::Next => {
            cmd.arg("next");
        }
        Ctl::Prev => {
            cmd.arg("previous");
        }
        Ctl::VolUp => {
            cmd.args(["volume", "0.05+"]);
        }
        Ctl::VolDown => {
            cmd.args(["volume", "0.05-"]);
        }
    }
    spawn_detached(cmd);
}

#[cfg(target_os = "macos")]
fn control(c: Ctl) {
    // Transport via nowplaying-cli; volume via AppleScript on the system output.
    match c {
        Ctl::PlayPause | Ctl::Next | Ctl::Prev => {
            let sub = match c {
                Ctl::Next => "next",
                Ctl::Prev => "previous",
                _ => "togglePlayPause",
            };
            let mut cmd = Command::new("nowplaying-cli");
            cmd.arg(sub);
            spawn_detached(cmd);
        }
        Ctl::VolUp | Ctl::VolDown => {
            let delta = if matches!(c, Ctl::VolUp) { "+ 6" } else { "- 6" };
            let script = format!("set volume output volume (output volume of (get volume settings) {delta})");
            let mut cmd = Command::new("osascript");
            cmd.arg("-e").arg(script);
            spawn_detached(cmd);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn control(_c: Ctl) {}

// ---- live waveform (Linux / cava) -----------------------------------------

/// Handle to a running waveform worker. Dropping it stops the worker and kills the
/// cava child, so closing the overlay never leaves a background process behind.
pub struct WaveHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for WaveHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Starts the live waveform on a worker thread by driving `cava` in raw stdout mode,
/// delivering one [`MediaMsg::Waveform`] frame (one byte per bar) per cava frame
/// through `proxy`. Returns `None` when cava is not installed — the overlay then just
/// shows no wave. The returned handle stops the worker when dropped.
#[cfg(target_os = "linux")]
pub fn start_waveform(bars: usize, proxy: EventLoopProxy<UserEvent>) -> Option<WaveHandle> {
    use std::io::Read;
    use std::process::Stdio;

    let bars = bars.clamp(1, 512);
    // A tiny cava config that emits raw 8-bit bytes, one per bar, on stdout. Input is
    // left to cava's autodetect (pulse / pipewire). Written 0600 in the runtime dir.
    let cfg = format!(
        "[general]\nbars = {bars}\nframerate = 30\n[output]\nmethod = raw\nraw_target = /dev/stdout\nbit_format = 8bit\nchannels = mono\n"
    );
    let cfg_path = runtime_dir().join(format!("runnir-cava-{}.conf", std::process::id()));
    if write_private(&cfg_path, cfg.as_bytes()).is_err() {
        return None;
    }
    let mut child = match Command::new("cava")
        .arg("-p")
        .arg(&cfg_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::remove_file(&cfg_path);
            return None; // cava not installed
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    std::thread::spawn(move || {
        let mut stdout = match child.stdout.take() {
            Some(s) => s,
            None => return,
        };
        let mut buf = vec![0u8; bars];
        loop {
            if worker_stop.load(Ordering::Relaxed) {
                break;
            }
            // A full frame is exactly `bars` bytes (mono). read_exact blocks until one
            // arrives; cava emits at the framerate even on silence, so the stop flag is
            // noticed within a frame.
            if stdout.read_exact(&mut buf).is_err() {
                break; // cava exited or the pipe closed
            }
            if worker_stop.load(Ordering::Relaxed) {
                break;
            }
            if proxy.send_event(UserEvent::Media(MediaMsg::Waveform(buf.clone()))).is_err() {
                break; // event loop gone
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&cfg_path);
    });
    Some(WaveHandle { stop })
}

/// The waveform is cava-based and Linux-only for now; other platforms show art +
/// metadata without a wave.
#[cfg(not(target_os = "linux"))]
pub fn start_waveform(_bars: usize, _proxy: EventLoopProxy<UserEvent>) -> Option<WaveHandle> {
    None
}

#[cfg(target_os = "linux")]
fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Writes `data` to `path` with 0600 permissions, refusing to follow a symlink
/// (`O_NOFOLLOW`), mirroring the input layer's private-temp write.
#[cfg(target_os = "linux")]
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    f.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_playerctl_output() {
        let out = "Bohemian Rhapsody\nQueen\nA Night at the Opera\n\nPlaying";
        let np = parse_playerctl(out);
        assert_eq!(np.title, "Bohemian Rhapsody");
        assert_eq!(np.artist, "Queen");
        assert_eq!(np.album, "A Night at the Opera");
        assert_eq!(np.status, Status::Playing);
        assert!(np.art.is_none(), "no art url yields no art");
        assert!(!np.is_empty());
    }

    #[test]
    fn parses_empty_and_missing_fields() {
        // Empty artist and album, and the status/art lines missing entirely.
        let np = parse_playerctl("Just a Title\n\n");
        assert_eq!(np.title, "Just a Title");
        assert_eq!(np.artist, "");
        assert_eq!(np.album, "");
        assert_eq!(np.status, Status::Stopped, "a missing status is stopped");
        assert!(np.art.is_none());

        // A wholly empty output is an empty player.
        let empty = parse_playerctl("");
        assert!(empty.is_empty());
        assert_eq!(empty.status, Status::Stopped);

        // Paused is recognised case-insensitively.
        assert_eq!(parse_playerctl("t\na\nb\n\npaused").status, Status::Paused);
    }

    #[test]
    fn art_url_only_resolves_local_existing_files() {
        // http art is never fetched.
        let np = parse_playerctl("t\na\nb\nhttps://example.com/cover.jpg\nPlaying");
        assert!(np.art.is_none(), "remote art is skipped in v1");

        // A real file:// url pointing at an existing file resolves.
        let dir = std::env::temp_dir();
        let path = dir.join(format!("runnir-media-art-test-{}.bin", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let url = format!("file://{}", path.display());
        let np = parse_playerctl(&format!("t\na\nb\n{url}\nPlaying"));
        assert_eq!(np.art.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_file(&path);

        // A file:// url to a missing file resolves to None (not a dangling path).
        let np = parse_playerctl("t\na\nb\nfile:///no/such/runnir/cover.png\nPlaying");
        assert!(np.art.is_none());
    }

    #[test]
    fn percent_decoding_handles_spaces_and_bad_escapes() {
        assert_eq!(percent_decode("/a%20b/c.png"), "/a b/c.png");
        // A malformed escape is left verbatim rather than dropped.
        assert_eq!(percent_decode("/a%2/b"), "/a%2/b");
        assert_eq!(percent_decode("/plain"), "/plain");
    }

    #[test]
    fn halfblock_art_dimensions_and_colours() {
        // A 2x2 image: top row red, bottom row blue (RGBA).
        let red = [255u8, 0, 0, 255];
        let blue = [0u8, 0, 255, 255];
        let mut rgba = Vec::new();
        rgba.extend_from_slice(&red);
        rgba.extend_from_slice(&red); // row 0
        rgba.extend_from_slice(&blue);
        rgba.extend_from_slice(&blue); // row 1

        let cells = halfblock_art(&rgba, 2, 2, 1, 1);
        assert_eq!(cells.len(), 1, "one character row");
        assert_eq!(cells[0].len(), 1, "one character column");
        // The single cell stacks the two source pixel rows: red over blue.
        assert_eq!(cells[0][0].top, (255, 0, 0));
        assert_eq!(cells[0][0].bottom, (0, 0, 255));

        // A wider target keeps the requested dimensions.
        let cells = halfblock_art(&rgba, 2, 2, 4, 3);
        assert_eq!(cells.len(), 3);
        assert!(cells.iter().all(|r| r.len() == 4));

        // Degenerate inputs return empty rather than panicking.
        assert!(halfblock_art(&rgba, 2, 2, 0, 1).is_empty());
        assert!(halfblock_art(&[], 2, 2, 1, 1).is_empty(), "too-small buffer is rejected");
    }

    #[test]
    fn bar_block_maps_amplitude_to_height() {
        assert_eq!(bar_block(0), ' ', "silence is blank");
        assert_eq!(bar_block(255), '\u{2588}', "full amplitude is the tallest block");
        assert_eq!(bar_block(1), '\u{2581}', "the faintest signal is the shortest block");
        // Monotonic non-decreasing across the range.
        let mut prev = 0u32;
        for v in 0..=255u8 {
            let h = BLOCKS.iter().position(|&c| c == bar_block(v)).map(|i| i as u32 + 1).unwrap_or(0);
            assert!(h >= prev, "bar height must not decrease as amplitude rises");
            prev = h;
        }
    }
}
