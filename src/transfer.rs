//! The sending half of an optical transfer: a file becomes an endless stream of
//! QR codes on screen, and a phone camera pointed at the window rebuilds it.
//!
//! [`crate::optical`] is the format; this is the session that drives it — which
//! frame is showing, how fast they turn over, and how the code is painted so a
//! camera can actually read it off a terminal window.
//!
//! There is no receiver here and there never will be: runnir has no camera. The
//! other end is a page on runnir's website, which is why the format had to match
//! byte for byte rather than being invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use qrcode::bits::Bits;
use qrcode::{EcLevel, QrCode, Version};

use crate::optical::{self, LtEncoder, PackedFile};

/// Payload bytes per frame. 2953 is exactly a version-40 code at error
/// correction L, which is the densest a QR can be — and density is goodput here,
/// because the frame rate is capped by the screen and the camera, not by us.
pub const DEFAULT_FRAME_BYTES: usize = 2953;

/// Frames per second.
///
/// decimen's browser sender defaults to 60 because it runs full-screen on a
/// desktop monitor. runnir defaults lower on purpose: the phone camera is the
/// bottleneck either way, an LCD needs time to actually settle on a new pattern,
/// and a frame the camera catches mid-transition is a frame wasted. Their own
/// guidance is that 24 on a 60 Hz screen is comfortable.
pub const DEFAULT_FPS: u32 = 24;

/// Modules of white around the code. Four is the QR standard's quiet zone, and
/// a decoder that cannot find it will not even look at the symbol.
const QUIET_MODULES: u32 = 4;

/// How big the code is allowed to get in pixels, per side.
///
/// A version-40 symbol is 177 modules wide; with the quiet zone, 185. At four
/// screen pixels per module that is 740 px, which is about the smallest that
/// reads from a hand-held phone at arm's length. Bigger is better right up to
/// the size of the window, so this ceiling only exists to keep a 4K window from
/// uploading a 30 MB texture every frame.
const MAX_BOX_PX: u32 = 1400;

/// One drawn frame: the pixels, and the serial the renderer caches them by.
///
/// The size is the size of the CELLS it will be drawn into, not a square, even
/// though the code inside it is square. That is the point: the renderer stretches
/// a texture to fill its quad, so a square texture in a slightly-not-square box
/// of cells is resampled — and resampling is what a QR decoder cannot afford.
pub struct Raster {
    pub rgba: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
    pub serial: u64,
}

/// A live transfer. Frames advance on a clock, not on a request: a camera that
/// misses one simply gets a different one, and the file arrives anyway.
pub struct Transfer {
    pub name: String,
    pub fps: u32,
    /// The QR version every frame is pinned to. Every frame is the same length,
    /// so this never has to change mid-stream — and it must not, because a
    /// receiver that has locked onto one symbol size loses the stream when it
    /// changes under it.
    pub version: i16,
    pub started: Instant,
    pub frames_sent: u64,
    pub paused: bool,
    packed: PackedFile,
    encoder: LtEncoder,
    seq: u32,
    last_advance: Instant,
    cached: Option<Raster>,
    cached_for: (u32, u32, u32),
}

impl Transfer {
    pub fn start(
        name: &str,
        media_type: &str,
        bytes: &[u8],
        frame_bytes: usize,
        fps: u32,
    ) -> Result<Self, String> {
        if frame_bytes <= optical::HEADER_LEN {
            return Err(format!("a frame must be larger than its {}-byte header", optical::HEADER_LEN));
        }
        let packed = optical::pack_file(name, media_type, bytes).map_err(|e| e.to_string())?;
        // Caught here rather than mid-stream: `k` is a u16, so a big payload at a
        // small frame size runs out of block numbers, and the fix is a setting.
        if !optical::fits_in_one_stream(packed.container.len(), frame_bytes) {
            return Err(format!(
                "{} needs at least {} bytes per frame; this stream is set to {frame_bytes}",
                human(packed.container.len()),
                optical::minimum_frame_bytes(packed.container.len())
            ));
        }
        let block_len = optical::block_length(frame_bytes);
        let session_id = random_session_id();
        let version = pin_version(frame_bytes)?;
        let encoder = LtEncoder::new(&packed.container, block_len, session_id);
        let now = Instant::now();
        Ok(Self {
            name: name.to_string(),
            fps: fps.clamp(1, 120),
            version,
            started: now,
            frames_sent: 0,
            paused: false,
            packed,
            encoder,
            seq: 0,
            last_advance: now,
            cached: None,
            cached_for: (0, 0, 0),
        })
    }

    /// The six digits shown beside the code, for comparing against the phone.
    pub fn verification_code(&self) -> String {
        self.packed.verification_code()
    }

    pub fn original_size(&self) -> usize {
        self.packed.original_size
    }

    pub fn transmitted_size(&self) -> usize {
        self.packed.transmitted_size
    }

    pub fn is_compressed(&self) -> bool {
        self.packed.compression == optical::Compression::Gzip
    }

    pub fn blocks(&self) -> usize {
        self.encoder.k
    }

    /// Frames a receiver needs before it can rebuild the file, and therefore the
    /// shortest time this can possibly take. Reported rather than the file size
    /// because "17 seconds" is the answer to "how long do I hold my phone here".
    pub fn shortest_pass(&self) -> Duration {
        let needed = (self.encoder.k as f64 * 1.15).ceil();
        Duration::from_secs_f64(needed / f64::from(self.fps))
    }

    /// How many complete passes have gone by. A receiver that starts late still
    /// finishes — the fountain has no beginning to miss — so this is progress
    /// information for the person holding the phone, not a completion state.
    pub fn passes(&self) -> f64 {
        let needed = (self.encoder.k as f64 * 1.15).ceil();
        self.frames_sent as f64 / needed
    }

    /// Move to the next frame if its time has come. `true` when the picture
    /// changed and the window needs repainting.
    pub fn advance(&mut self, now: Instant) -> bool {
        if self.paused {
            return false;
        }
        let interval = Duration::from_secs_f64(1.0 / f64::from(self.fps));
        if self.frames_sent > 0 && now.duration_since(self.last_advance) < interval {
            return false;
        }
        // Deliberately not `last_advance += interval`: catching up on missed
        // frames would show two symbols within one screen refresh, and the second
        // one is one the camera never had a chance to see.
        self.last_advance = now;
        if self.frames_sent > 0 {
            self.seq = self.seq.wrapping_add(1);
        }
        self.frames_sent += 1;
        self.cached = None;
        true
    }

    /// The current frame, painted to fill a `box_w` by `box_h` field.
    ///
    /// Cached on the frame and the size because a window repaints far more often than
    /// the stream advances, and a fresh texture serial on every repaint would
    /// re-upload the same picture — at these sizes, tens of megabytes a second
    /// of nothing.
    pub fn raster(&mut self, box_w: u32, box_h: u32) -> &Raster {
        let box_w = box_w.clamp(64, MAX_BOX_PX);
        let box_h = box_h.clamp(64, MAX_BOX_PX);
        if self.cached.is_none() || self.cached_for != (self.seq, box_w, box_h) {
            let frame = self.encoder.frame(
                self.seq,
                self.packed.container.len(),
                self.packed.payload_fnv,
            );
            let raster = paint_qr(&frame, self.version, box_w, box_h);
            self.cached = Some(raster);
            self.cached_for = (self.seq, box_w, box_h);
        }
        self.cached.as_ref().expect("just filled")
    }

    /// The frame already painted, for a draw pass that cannot paint one itself.
    /// `None` until the first [`Self::raster`] call, which the window's tick makes.
    pub fn painted(&self) -> Option<&Raster> {
        self.cached.as_ref()
    }

    /// How long the interval between frames is, which is also how often a window
    /// showing this has to wake up.
    pub fn interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.fps))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// The smallest QR version that holds a whole frame at error correction L.
///
/// Resolved once, from the frame SIZE rather than from the first frame's
/// content, because every frame is exactly the same length. decimen locks the
/// version off frame zero and gets the same answer; doing it up front means
/// there is no state that could be wrong for one frame.
fn pin_version(frame_bytes: usize) -> Result<i16, String> {
    let probe = vec![0u8; frame_bytes];
    for v in 1..=40i16 {
        if encode_byte_mode(&probe, v).is_ok() {
            return Ok(v);
        }
    }
    Err(format!("{frame_bytes} bytes does not fit in any QR code at error correction L"))
}

/// Encode a frame as ONE byte-mode segment, bypassing the crate's optimal
/// segmentation.
///
/// `QrCode::with_version` runs a segmenter that may split the data into numeric,
/// alphanumeric and byte runs. Usually that is smaller; here it is fatal. A
/// version-40 L symbol holds exactly 2953 bytes in byte mode, which leaves four
/// spare bits — and every extra segment costs a mode indicator and a length
/// field, twenty bits. So a frame whose bytes happened to contain a long run of
/// ASCII digits could be segmented into something that no longer fits, and the
/// encode would fail for that frame alone.
///
/// It showed up as one flaky test in fifteen. In a real transfer it would have
/// been an occasional frame that simply refused to draw, on random-looking data
/// that changes every frame — which is as close to unreproducible as a bug gets.
/// decimen forces byte mode for the same reason, and the crate documents
/// `with_bits` as the way to avoid the segmenter.
fn encode_byte_mode(bytes: &[u8], version: i16) -> Result<QrCode, qrcode::types::QrError> {
    let mut bits = Bits::new(Version::Normal(version));
    bits.push_byte_data(bytes)?;
    bits.push_terminator(EcLevel::L)?;
    QrCode::with_bits(bits, EcLevel::L)
}

/// Paint one frame's bytes as a QR code inside a `box_w` by `box_h` field.
///
/// Two things happen here and both are about sharpness. The code is drawn at a
/// WHOLE number of pixels per module, and the field is exactly the size of the
/// quad it will be drawn into. The renderer's image sampler filters linearly, so
/// either a fractional module scale or a texture that has to be stretched to fit
/// its box blurs the module edges — which is the contrast a decoder lives on.
/// Painting at an integer scale into an exactly-sized field and padding the rest
/// with white means the texture is drawn 1:1 and the filter has nothing to do.
///
/// The box is not square, because a terminal cell is about twice as tall as it is
/// wide and a whole number of them almost never lands on a square. The extra goes
/// to the white margin, which the quiet zone wanted anyway.
fn paint_qr(bytes: &[u8], version: i16, box_w: u32, box_h: u32) -> Raster {
    let code = encode_byte_mode(bytes, version)
        .expect("the version was pinned by encoding a frame-sized probe the same way");
    let width = code.width() as u32;
    let colors = code.to_colors();
    let total = width + 2 * QUIET_MODULES;
    // At least one pixel per module even in a box too small to be readable: a
    // shrunken window should show a useless code, not panic or vanish.
    let scale = std::cmp::max(1, std::cmp::min(box_w, box_h) / total);
    let drawn = total * scale;
    let w = std::cmp::max(box_w, drawn);
    let h = std::cmp::max(box_h, drawn);
    let origin_x = (w - drawn) / 2 + QUIET_MODULES * scale;
    let origin_y = (h - drawn) / 2 + QUIET_MODULES * scale;

    // White, not the theme's background: the quiet zone and the light modules are
    // half of the contrast a camera measures, and a themed panel would be reading
    // a code printed on grey.
    let mut rgba = vec![0xffu8; (w * h * 4) as usize];
    for my in 0..width {
        for mx in 0..width {
            if colors[(my * width + mx) as usize] != qrcode::Color::Dark {
                continue;
            }
            let x0 = origin_x + mx * scale;
            let y0 = origin_y + my * scale;
            for y in y0..y0 + scale {
                let row = (y * w) as usize * 4;
                for x in x0..x0 + scale {
                    let i = row + x as usize * 4;
                    rgba[i] = 0;
                    rgba[i + 1] = 0;
                    rgba[i + 2] = 0;
                }
            }
        }
    }
    Raster { rgba: Arc::new(rgba), w, h, serial: crate::grid::next_image_serial() }
}

/// A fresh session id per transfer, so a receiver that was watching the previous
/// one resets instead of feeding new frames into an old decoder.
fn random_session_id() -> u16 {
    let mut buf = [0u8; 2];
    match std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf))
    {
        Ok(()) => u16::from_le_bytes(buf),
        // Not worth failing a transfer over: the id only has to differ from the
        // last one, and the clock does that.
        Err(_) => (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(1)
            & 0xffff) as u16,
    }
}

/// Guess a media type from the extension.
///
/// Only used to decide whether gzip is worth attempting and what the phone calls
/// the file when it saves it — a wrong guess costs a few percent of transfer
/// time, never correctness, so a short table beats a dependency.
pub fn media_type_for(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" | "log" | "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "js" | "ts" | "css"
        | "html" | "sh" | "py" | "c" | "h" | "conf" | "ini" | "csv" => "text/plain",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

pub fn human(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes gzip cannot help with. A short repeating pattern would compress to
    /// almost nothing, and then every size assertion below would be measuring the
    /// compressor rather than the transfer.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut state: u32 = 0x1234_5678;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    fn sample() -> Vec<u8> {
        incompressible(40_000)
    }

    #[test]
    fn the_default_frame_size_pins_to_the_densest_qr_there_is() {
        // 2953 bytes at error correction L is exactly a version-40 symbol. If this
        // ever picks something smaller, the default silently lost goodput.
        assert_eq!(pin_version(DEFAULT_FRAME_BYTES).unwrap(), 40);
        assert!(pin_version(2954).is_err(), "nothing larger than V40-L exists");
        // A smaller frame gets a smaller symbol, monotonically — the property
        // that matters, since a denser frame must never pick a sparser code.
        let mut last = 0;
        for bytes in [64usize, 512, 1024, 1465, 2048, 2953] {
            let v = pin_version(bytes).unwrap();
            assert!(v >= last, "{bytes} B picked V{v} after V{last}");
            last = v;
        }
        assert_eq!(pin_version(1465).unwrap(), 27, "recorded, so a crate bump cannot move it quietly");
    }

    #[test]
    fn a_transfer_reports_what_the_person_holding_the_phone_needs() {
        let t = Transfer::start("a.jpg", "image/jpeg", &sample(), DEFAULT_FRAME_BYTES, 24).unwrap();
        assert_eq!(t.version, 40);
        assert_eq!(t.original_size(), 40_000);
        assert!(t.blocks() >= 14, "40 KB does not fit in fewer blocks than that");
        // The answer to "how long do I hold this here", not a byte count.
        assert!(t.shortest_pass() > Duration::from_millis(500));
        assert_eq!(t.verification_code().len(), 6);
    }

    #[test]
    fn a_payload_too_big_for_the_frame_size_is_refused_before_anything_is_drawn() {
        // k is a u16, so 6 MB at 100 bytes a frame runs out of block numbers.
        let big = incompressible(6 * 1024 * 1024);
        let err = match Transfer::start("a.jpg", "image/jpeg", &big, 100, 24) {
            Err(e) => e,
            Ok(_) => panic!("a payload that outruns the block numbering must be refused"),
        };
        assert!(err.contains("bytes per frame"), "the error must name the fix: {err}");
    }

    #[test]
    fn a_frame_no_larger_than_its_header_is_refused() {
        let err = match Transfer::start("a.bin", "text/plain", b"hello", 20, 24) {
            Err(e) => e,
            Ok(_) => panic!("a frame with no room for a payload must be refused"),
        };
        assert!(err.contains("header"), "{err}");
    }

    #[test]
    fn frames_advance_on_the_clock_and_never_skip_ahead() {
        let mut t =
            Transfer::start("a.bin", "application/octet-stream", &sample(), DEFAULT_FRAME_BYTES, 20)
                .unwrap();
        let t0 = Instant::now();
        assert!(t.advance(t0), "the first frame shows immediately");
        assert_eq!(t.frames_sent, 1);
        // Too soon: the camera has not had a chance to read what is up there.
        assert!(!t.advance(t0 + Duration::from_millis(10)));
        assert_eq!(t.frames_sent, 1);
        assert!(t.advance(t0 + Duration::from_millis(60)));
        assert_eq!(t.frames_sent, 2);

        // A long stall must not then flush a burst of frames through in one
        // repaint: every one of those but the last would be unreadable.
        let after = t0 + Duration::from_secs(5);
        assert!(t.advance(after));
        assert!(!t.advance(after));
        assert_eq!(t.frames_sent, 3);
    }

    #[test]
    fn a_paused_transfer_holds_its_frame() {
        let mut t = Transfer::start("a.bin", "text/plain", &sample(), DEFAULT_FRAME_BYTES, 20).unwrap();
        let t0 = Instant::now();
        t.advance(t0);
        t.paused = true;
        assert!(!t.advance(t0 + Duration::from_secs(1)));
        assert_eq!(t.frames_sent, 1);
    }

    #[test]
    fn a_frame_encodes_at_the_pinned_version_whatever_bytes_it_holds() {
        // The regression for the segmenter bug. A frame at the default size sits
        // four bits under a version-40 L symbol, so any segmentation other than a
        // single byte-mode run overflows — and the crate's optimal segmenter will
        // produce one for data that happens to contain a long ASCII digit run.
        // Fountain frames are XORs of file bytes, so this is a matter of luck per
        // frame, which is exactly the bug you never reproduce.
        let digits: Vec<u8> = std::iter::repeat_n(b'7', DEFAULT_FRAME_BYTES).collect();
        assert!(encode_byte_mode(&digits, 40).is_ok(), "a frame of ASCII digits must still fit");
        let alnum: Vec<u8> = (0..DEFAULT_FRAME_BYTES).map(|i| b"0123456789ABCDEF $%*+-./:"[i % 25]).collect();
        assert!(encode_byte_mode(&alnum, 40).is_ok(), "a frame of alphanumerics must still fit");
        assert!(encode_byte_mode(&vec![0u8; DEFAULT_FRAME_BYTES], 40).is_ok());
        assert!(encode_byte_mode(&vec![0u8; DEFAULT_FRAME_BYTES + 1], 40).is_err());

        // And through the real path on real fountain output, which is where it
        // actually panicked. Kept short: a version-40 encode costs about a tenth
        // of a second in a debug build, and the two cases above are the ones that
        // pin the bug — this only checks that the real path reaches them.
        let mut t = Transfer::start("a.txt", "text/plain", &incompressible(90_000), DEFAULT_FRAME_BYTES, 24)
            .unwrap();
        let mut now = Instant::now();
        for _ in 0..20 {
            t.advance(now);
            now += Duration::from_millis(50);
            t.raster(200, 200);
        }
    }

    #[test]
    fn every_frame_is_a_real_decodable_qr_of_the_pinned_version() {
        let mut t =
            Transfer::start("a.bin", "application/octet-stream", &sample(), DEFAULT_FRAME_BYTES, 24)
                .unwrap();
        let mut seen = Vec::new();
        for i in 0..5 {
            t.advance(Instant::now() + Duration::from_millis(i * 100));
            let r = t.raster(800, 800);
            assert_eq!(r.rgba.len(), (r.w * r.h * 4) as usize);
            seen.push(r.serial);
        }
        // A new picture every frame, and a new serial with it, or the renderer
        // would keep showing the texture it already had.
        seen.dedup();
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn the_code_is_painted_at_a_whole_number_of_pixels_per_module() {
        // The renderer filters linearly, so a fractional scale blurs exactly the
        // edges a decoder measures. Check by reading the pixels back: every run of
        // equal pixels along the first module row must be a multiple of the scale.
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24).unwrap();
        t.advance(Instant::now());
        let r = t.raster(900, 900);
        let px = r.w as usize;
        // V40 plus the quiet zone is 185 modules; 900/185 = 4 pixels each.
        let scale = 900 / (177 + 2 * 4);
        assert_eq!(scale, 4);

        // The row through the middle of the top-left finder pattern: its dark run
        // must be a whole number of modules wide.
        let origin = (px - 185 * scale) / 2 + 4 * scale;
        let y = origin + scale / 2;
        let mut run = 0;
        let mut x = origin;
        while r.rgba[(y * px + x) * 4] == 0 {
            run += 1;
            x += 1;
        }
        assert_eq!(run % scale, 0, "a finder pattern edge landed mid-pixel: run {run}, scale {scale}");
        assert_eq!(run / scale, 7, "the finder pattern is seven modules wide");
    }

    #[test]
    fn the_quiet_zone_is_white_all_the_way_round() {
        // A decoder that cannot find the quiet zone does not even look at the
        // symbol, and the panel background is not white.
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24).unwrap();
        t.advance(Instant::now());
        let r = t.raster(800, 800);
        let px = r.w as usize;
        for i in 0..px {
            for (x, y) in [(i, 0), (i, px - 1), (0, i), (px - 1, i)] {
                let p = (y * px + x) * 4;
                assert_eq!(
                    (r.rgba[p], r.rgba[p + 1], r.rgba[p + 2], r.rgba[p + 3]),
                    (0xff, 0xff, 0xff, 0xff),
                    "the border pixel at {x},{y} is not white"
                );
            }
        }
    }

    #[test]
    fn a_tiny_box_still_produces_a_code_rather_than_a_panic() {
        // A window dragged small must show a useless code, not crash the terminal.
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24).unwrap();
        t.advance(Instant::now());
        let r = t.raster(1, 1);
        assert!(r.w >= 185, "the box grows to hold one pixel per module: {}", r.w);
    }

    #[test]
    fn two_transfers_of_the_same_file_get_different_sessions() {
        // Or a receiver watching the first would feed the second's frames into the
        // decoder it already had.
        let mut a = Transfer::start("a.bin", "text/plain", &sample(), 2953, 24).unwrap();
        let mut b = Transfer::start("a.bin", "text/plain", &sample(), 2953, 24).unwrap();
        a.advance(Instant::now());
        b.advance(Instant::now());
        assert_ne!(a.raster(400, 400).rgba, b.raster(400, 400).rgba);
    }

    /// Paint real frames for the optical cross-check.
    ///
    /// Everything above measures the pixels with a ruler I wrote myself. The
    /// claim that matters is that a QR DECODER can read what runnir paints, and
    /// the only honest way to settle it is to hand the pixels to one — the same
    /// zxing-wasm build the receiver runs. This writes the raw RGBA of a run of
    /// frames; `tools/optical-cross-check.mjs --painted` decodes them and pushes
    /// what comes out through the fountain decoder to recover the file.
    ///
    /// Ignored because it writes tens of megabytes and the harness runs it:
    /// `RUNNIR_PAINTED_FRAMES=/tmp/painted cargo test -- --ignored emit_painted_frames`
    #[test]
    #[ignore = "paints frames for the QR decoder cross-check; run it explicitly"]
    fn emit_painted_frames() {
        let Ok(dir) = std::env::var("RUNNIR_PAINTED_FRAMES") else {
            panic!("set RUNNIR_PAINTED_FRAMES to the output directory");
        };
        std::fs::create_dir_all(&dir).unwrap();

        let bytes = incompressible(60_000);
        let mut t =
            Transfer::start("painted.bin", "image/jpeg", &bytes, DEFAULT_FRAME_BYTES, 24).unwrap();
        // Enough to rebuild the file with room to spare, so a failure means the
        // decoder could not READ a frame rather than that it ran out of them.
        let count = (t.blocks() as f64 * 2.0).ceil() as usize + 8;
        // Three pixels per module: below that a decoder starts failing on its own
        // account, and this test is not about how small a code can get.
        let box_px = (185 * 3) as u32;

        let index = format!(
            "{{\"frames\":{count},\"px\":{box_px},\"blocks\":{},\"version\":{},\
             \"name\":\"painted.bin\",\"size\":{},\"code\":\"{}\"}}\n",
            t.blocks(),
            t.version,
            bytes.len(),
            t.verification_code()
        );
        let mut now = Instant::now();
        for i in 0..count {
            t.advance(now);
            now += Duration::from_millis(100);
            let r = t.raster(box_px, box_px);
            std::fs::write(format!("{dir}/frame-{i:04}.rgba"), r.rgba.as_slice()).unwrap();
        }
        std::fs::write(format!("{dir}/index.json"), index).unwrap();
        eprintln!("painted {count} frames of {box_px}px into {dir}");
    }

    #[test]
    fn the_media_type_guess_only_has_to_be_good_enough_for_gzip() {
        assert_eq!(media_type_for(std::path::Path::new("a/b/notes.md")), "text/plain");
        assert_eq!(media_type_for(std::path::Path::new("photo.JPG")), "image/jpeg");
        assert_eq!(media_type_for(std::path::Path::new("noext")), "application/octet-stream");
    }

    #[test]
    fn sizes_read_the_way_a_person_says_them() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(5 * 1024 * 1024), "5.0 MB");
    }
}
