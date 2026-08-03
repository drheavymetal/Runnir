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

/// Frames per second, or **0 for automatic** — which is the default, and means
/// the fastest rate this machine can actually paint, up to [`AUTO_FPS_MAX`].
///
/// The same sentinel the tile count uses, and for the same reason: the right
/// number depends on hardware runnir can measure and a room it cannot, so it
/// answers the half it knows and leaves the other half to a person with a phone.
///
/// The history is worth keeping, because reasoning got this wrong three times.
/// The theory said half the capture rate — 30 against a 60 fps camera — on the
/// grounds that a code needs one clean look. Then a measurement said 10, on the
/// grounds that a hand-held phone needs time to settle. Then 10 turned out to be
/// an artifact of a receiver capped at ~5 codes a second: with the receiver
/// fixed, and with each capture carrying two codes in colour, 30 beat 25 and 40
/// was much worse. So the shape of the answer is not a constant at all — it is
/// "as fast as the painter can go", with the ceiling set by the receiver
/// saturating rather than by the screen.
pub const DEFAULT_FPS: u32 = 0;

/// The fastest the automatic rate will choose.
///
/// Measured against a real phone on 2026-08-03: 30 beat 25, and 40 was much
/// worse. Above this the sender is not the constraint anyway — the receiver
/// reads about 9 codes a second, and 30 frames a second in colour already puts
/// 60 on the glass. Past saturation more frames buy nothing but a chance the
/// painter falls behind, which costs.
pub const AUTO_FPS_MAX: u32 = 30;

/// The slowest the automatic rate will drop to on a machine that cannot paint.
/// Below this the stream is barely a stream, and a person is better told than
/// silently throttled — which the panel does.
pub const AUTO_FPS_MIN: u32 = 5;

/// How much of the frame interval the painting may take before the automatic
/// rate steps down.
///
/// Not 1.0: painting is not all a frame does. The texture still has to be
/// uploaded and the rest of the window drawn, and they share the interval. The
/// measurement that fixes this: two codes cost ~25 ms to paint, which fits a
/// 40 fps interval of 25 ms exactly — and 40 fps measured MUCH worse than 30.
const PAINT_HEADROOM: f64 = 1.3;

/// How many codes to show at once. 0 means one code, as big as the window
/// allows, which is what a phone at arm's length actually wants.
///
/// Tiling is the only way past the frame-rate ceiling and it looks free — a code
/// is sized by the SHORT axis, so a 16:9 window has room for a second one beside
/// it at identical pixels per module. It is not free, and a real phone said so:
/// the pixels that decide a transfer belong to the CAMERA, and framing a wider
/// window puts every code on a smaller share of the sensor. Whether the trade
/// pays depends on how big the screen is and how close the phone gets, which is
/// information runnir does not have and the person holding the phone does.
pub const DEFAULT_TILES: usize = 0;

/// Carry a second code in colour on top of the first. On, because it measured
/// **+73%** against the same file and phone in the same session: 22.5 KB/s at 30
/// fps in colour, against 13 KB/s at best without it.
///
/// That result settles the doubt it was built to settle. The worry was that a
/// second scan per capture would cancel the second code, which would be true if
/// decoding were the constraint — and the +73% says it is not: the constraint is
/// how many codes each capture YIELDS, which is exactly what colour changes. The
/// scheme, and why the base code stays an ordinary QR for every other receiver
/// in the world, is in [`paint_mosaic`].
///
/// `--color 0` turns it off, which is worth reaching for on a phone slow enough
/// that its metrics line shows captures being dropped.
pub const DEFAULT_COLOR: bool = true;

/// Ceiling on tiles, whatever the window or the setting asks for.
///
/// Each tile is a whole QR encode and paint per drawn frame, so the cost is
/// linear in tiles and paid `fps` times a second. Sixteen at 30 fps is already
/// 480 encodes a second.
const MAX_TILES: usize = 16;

/// Modules of white around the code. Four is the QR standard's quiet zone, and
/// a decoder that cannot find it will not even look at the symbol.
const QUIET_MODULES: u32 = 4;

/// How much of the window the codes may cover, per axis, in pixels.
///
/// A version-40 symbol is 177 modules wide; with the quiet zone, 185. At four
/// screen pixels per module that is 740 px, which is about the smallest that
/// reads from a hand-held phone at arm's length. Bigger is better right up to
/// the size of the window, so this ceiling only exists to keep a 4K window from
/// uploading a huge texture every frame — the whole mosaic is re-uploaded `fps`
/// times a second, so the area is a bandwidth budget, not just a size.
const MAX_BOX_PX: u32 = 2200;

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

/// How many codes go on screen, how big, and how much room that really needs.
///
/// Resolved once from the space available and then carried around, rather than
/// recomputed by whoever needs a piece of it. The pixels that get painted and the
/// cells they are placed in have to agree exactly — a texture that is not its
/// quad's size is resampled, and resampling is what a decoder cannot afford — so
/// there is one answer and everybody is handed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Codes across and down.
    pub grid: (u32, u32),
    /// Screen pixels per module. Always a whole number.
    pub scale: u32,
    /// What the grid actually covers, quiet zones included.
    pub drawn: (u32, u32),
}

impl Layout {
    pub fn tiles(&self) -> u32 {
        self.grid.0 * self.grid.1
    }
}

impl Default for Layout {
    /// One code, one pixel per module. Only ever used by a panel that has no
    /// transfer to ask, which draws an error instead of a code.
    fn default() -> Self {
        Self { grid: (1, 1), scale: 1, drawn: (0, 0) }
    }
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
    /// Codes put on the glass, not screen refreshes: with a mosaic one drawn
    /// frame carries several, and it is the codes a receiver counts.
    pub frames_sent: u64,
    pub paused: bool,
    /// Tiles asked for, 0 for automatic. Kept rather than resolved once because
    /// the answer depends on the window, which resizes.
    pub tiles_req: usize,
    /// Whether every tile carries a second frame in its colour. See
    /// [`paint_mosaic`] for the scheme and [`Self::layers`] for what it costs.
    pub color: bool,
    packed: PackedFile,
    encoder: LtEncoder,
    seq: u32,
    last_advance: Instant,
    /// The grid the last drawn frame used. What [`Self::advance`] steps the
    /// sequence by, so every tile on screen carries a different frame.
    grid: (u32, u32),
    /// What one code cost to produce last time, dominated by the QR encode.
    /// Reported rather than acted on: see [`Self::painter_behind`].
    per_tile: Option<Duration>,
    cached: Option<Raster>,
    cached_for: (u32, u32, u32, u32, u32),
}

impl Transfer {
    pub fn start(
        name: &str,
        media_type: &str,
        bytes: &[u8],
        frame_bytes: usize,
        fps: u32,
        tiles: usize,
        color: bool,
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
            // 0 survives on purpose: it is the automatic rate, not a rate of
            // zero. Everything reads [`Self::effective_fps`] rather than this.
            fps: fps.min(120),
            version,
            started: now,
            frames_sent: 0,
            paused: false,
            tiles_req: tiles.min(MAX_TILES),
            color,
            packed,
            encoder,
            seq: 0,
            last_advance: now,
            grid: (1, 1),
            per_tile: None,
            cached: None,
            cached_for: (0, 0, 0, 0, 0),
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

    /// Codes on screen at once, as they were last drawn.
    pub fn grid(&self) -> (u32, u32) {
        self.grid
    }

    /// The rate this stream actually runs at.
    ///
    /// An explicit `fps` is obeyed, even when the painter cannot keep up — the
    /// panel says so instead, exactly as it does for an explicit tile count. The
    /// person who typed a number is the one who can judge whether their screen
    /// and their phone make it worth having.
    ///
    /// Automatic (`fps = 0`) asks for [`AUTO_FPS_MAX`] and steps down to what
    /// this machine measured itself painting. Until the first frame has been
    /// painted there is nothing to measure and it asks for the ceiling, which is
    /// the right guess: the cost is known one frame later and costs one frame.
    pub fn effective_fps(&self) -> u32 {
        if self.fps > 0 {
            return self.fps;
        }
        let Some(per_code) = self.per_tile else { return AUTO_FPS_MAX };
        let frame = per_code.as_secs_f64() * f64::from(self.codes_per_frame()) * PAINT_HEADROOM;
        if frame <= 0.0 {
            return AUTO_FPS_MAX;
        }
        ((1.0 / frame).floor() as u32).clamp(AUTO_FPS_MIN, AUTO_FPS_MAX)
    }

    /// Whether the rate is being chosen rather than obeyed.
    pub fn fps_is_automatic(&self) -> bool {
        self.fps == 0
    }

    /// Codes carried by one tile: two in colour mode, one otherwise.
    ///
    /// A tile is one square of screen either way. What colour buys is a second
    /// code in the SAME square, so everything counted in codes — the pass
    /// length, the sequence step, the paint budget — multiplies by this and
    /// nothing counted in screen area does.
    pub fn layers(&self) -> u32 {
        if self.color { 2 } else { 1 }
    }

    /// Codes put on the glass by one drawn frame.
    pub fn codes_per_frame(&self) -> u32 {
        self.grid.0 * self.grid.1 * self.layers()
    }

    /// Whether drawing a frame now costs more than the gap between frames.
    ///
    /// Past that line the stream does not merely stop speeding up, it SLOWS
    /// DOWN: more codes asked for, fewer codes on the glass. The panel says so
    /// rather than quietly overriding the setting, because a tile count is only
    /// ever set by hand and the person who set it is the one who can judge
    /// whether their screen and their phone make it worth having.
    pub fn painter_behind(&self) -> bool {
        let Some(per_code) = self.per_tile else { return false };
        per_code * self.codes_per_frame() > self.interval()
    }

    /// Frames a receiver needs before it can rebuild the file, and therefore the
    /// shortest time this can possibly take. Reported rather than the file size
    /// because "17 seconds" is the answer to "how long do I hold my phone here".
    ///
    /// A mosaic divides it: the codes go out `fps` times a second, several at a
    /// time, and a camera that resolves them reads them all from one capture.
    /// Colour divides it again, for the same reason and with the same caveat —
    /// this is the floor if every code sent is read, which no camera manages.
    pub fn shortest_pass(&self) -> Duration {
        let needed = (self.encoder.k as f64 * 1.15).ceil();
        let per_second = f64::from(self.effective_fps()) * f64::from(self.codes_per_frame());
        Duration::from_secs_f64(needed / per_second)
    }

    /// How many complete passes have gone by. A receiver that starts late still
    /// finishes — the fountain has no beginning to miss — so this is progress
    /// information for the person holding the phone, not a completion state.
    pub fn passes(&self) -> f64 {
        let needed = (self.encoder.k as f64 * 1.15).ceil();
        self.frames_sent as f64 / needed
    }

    /// How many codes fit in `avail_w` by `avail_h` pixels, and how big.
    ///
    /// The automatic answer never sacrifices sharpness: it takes the scale ONE
    /// code would get — set by the short axis, because a code is square and a
    /// window is not — and then fits as many codes as the long axis has room for
    /// at that same scale. On a 16:9 window that is a second code in space that
    /// was white margin, at identical pixels per module. It is the one place
    /// where throughput is genuinely free.
    ///
    /// An explicit tile count is allowed to shrink the modules, because someone
    /// standing close with a good camera can spend sharpness on count. The grid
    /// for a given count is whichever arrangement leaves the modules biggest.
    pub fn layout_for(&self, avail_w: u32, avail_h: u32) -> Layout {
        let total = symbol_modules(self.version);
        let avail_w = avail_w.min(MAX_BOX_PX);
        let avail_h = avail_h.min(MAX_BOX_PX);
        let fit = |gx: u32, gy: u32| -> u32 {
            std::cmp::min(avail_w / (gx * total), avail_h / (gy * total))
        };
        let (grid, scale) = match self.tiles_req {
            0 => {
                // ONE code, as big as the window allows. Measured against a real
                // phone: a mosaic is a loss on a laptop screen, and the reason is
                // optical rather than arithmetic.
                //
                // Filling spare width with a second code looked free — same
                // pixels per module, twice the goodput. But the pixels that
                // decide the transfer are the CAMERA's, not the screen's. Framing
                // a 1920px window instead of a 950px one puts each code on half
                // the sensor width it had, so the modules resolve worse and more
                // captures decode as nothing. Whether that trade pays depends on
                // the physical size of the screen and how close the phone is —
                // neither of which runnir can see, which is exactly why this is
                // not a decision it should be making on its own.
                //
                // `[transfer] tiles = N` is for someone who has that information:
                // a big monitor, or a phone held close.
                let scale = std::cmp::max(1, fit(1, 1));
                ((1, 1), scale)
            }
            n => {
                let n = n.clamp(1, MAX_TILES) as u32;
                let mut best = ((1, 1), 0);
                for gx in 1..=n {
                    let gy = n.div_ceil(gx);
                    let scale = fit(gx, gy);
                    // Ties go to the arrangement already found, which is the one
                    // with fewer columns — a taller stack on a wide window would
                    // waste the axis that had the room.
                    if scale > best.1 {
                        best = ((gx, gy), scale);
                    }
                }
                if best.1 == 0 {
                    // Nothing fits at a whole pixel per module. One code, as
                    // small as it has to be, beats an empty panel.
                    ((1, 1), std::cmp::max(1, fit(1, 1)))
                } else {
                    best
                }
            }
        };
        Layout { grid, scale, drawn: (grid.0 * total * scale, grid.1 * total * scale) }
    }

    /// Move to the next frame if its time has come. `true` when the picture
    /// changed and the window needs repainting.
    ///
    /// `layout` is the grid about to be drawn: the sequence steps by a whole
    /// mosaic, so no two codes on the glass at the same time carry the same
    /// frame. A resize between this and the paint can repeat or skip a few
    /// sequence numbers, which a fountain does not care about — a repeated frame
    /// is a frame the receiver already had, and there is nothing to skip past.
    pub fn advance(&mut self, now: Instant, layout: &Layout) -> bool {
        self.grid = layout.grid;
        if self.paused {
            return false;
        }
        let interval = self.interval();
        if self.frames_sent > 0 && now.duration_since(self.last_advance) < interval {
            return false;
        }
        // Deliberately not `last_advance += interval`: catching up on missed
        // frames would show two symbols within one screen refresh, and the second
        // one is one the camera never had a chance to see.
        self.last_advance = now;
        // Codes, not tiles: in colour mode one tile carries two frames, and both
        // of them have to be sequence numbers this stream has not used yet.
        let codes = layout.tiles() * self.layers();
        if self.frames_sent > 0 {
            self.seq = self.seq.wrapping_add(codes);
        }
        self.frames_sent += u64::from(codes);
        self.cached = None;
        true
    }

    /// The current frame, painted to fill a `box_w` by `box_h` field.
    ///
    /// Cached on the frame and the size because a window repaints far more often than
    /// the stream advances, and a fresh texture serial on every repaint would
    /// re-upload the same picture — at these sizes, tens of megabytes a second
    /// of nothing.
    pub fn raster(&mut self, layout: &Layout, box_w: u32, box_h: u32) -> &Raster {
        let box_w = box_w.max(64);
        let box_h = box_h.max(64);
        let key = (self.seq, box_w, box_h, layout.grid.0, layout.grid.1);
        if self.cached.is_none() || self.cached_for != key {
            // The first `tiles` frames are the ones a plain decoder sees; the
            // rest, in colour mode, ride in the blue channel of the same tiles.
            let codes = layout.tiles() * self.layers();
            let frames: Vec<Vec<u8>> = (0..codes)
                .map(|i| {
                    self.encoder.frame(
                        self.seq.wrapping_add(i),
                        self.packed.container.len(),
                        self.packed.payload_fnv,
                    )
                })
                .collect();
            let started = Instant::now();
            let raster = paint_mosaic(&frames, self.version, layout, box_w, box_h, self.color);
            self.per_tile = Some(started.elapsed() / codes.max(1));
            self.cached = Some(raster);
            self.cached_for = key;
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
        Duration::from_secs_f64(1.0 / f64::from(self.effective_fps()))
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
///
/// # Colour
///
/// With `color`, each tile carries TWO codes at once: `frames[i]` as usual, and
/// `frames[tiles + i]` in the blue channel of the same modules. Four colours,
/// chosen so that neither reader has to know about the other:
///
/// ```text
///   base dark, extra dark    black   (0,0,0)        luma 0    blue 0
///   base dark, extra light   blue    (0,0,255)      luma ~29  blue 255
///   base light, extra dark   yellow  (255,255,0)    luma ~226 blue 0
///   base light, extra light  white   (255,255,255)  luma 255  blue 255
/// ```
///
/// Read as brightness — which is what an ordinary QR decoder does, and what a
/// camera's luma plane carries at full resolution — black and blue are dark and
/// yellow and white are light, so the BASE code is a completely ordinary QR
/// symbol. That is not a nicety: decimen's own receivers, and any QR app at all,
/// keep reading a colour stream and simply get half of it. Nothing was added to
/// the wire format, and there is no flag anywhere saying colour is in use.
///
/// The blue channel is the extra code, on its own, with the same modules and the
/// same quiet zone — white surroundings are blue-light, so its quiet zone comes
/// free from the base code's.
///
/// What this costs, and why it is off by default: the receiver has to scan a
/// second image per capture, so it roughly doubles decode time. Measured against
/// a phone on 2026-08-03, codes read per second were constant at 5-7 no matter
/// what the sender did, which says the receiver was the bottleneck — and doubling
/// both sides of a bottleneck is a wash. Colour pays exactly when it is NOT: when
/// the phone reports few dropped captures, when the pool has spare workers, or
/// when the optics are marginal enough that many captures fail anyway (a failed
/// extra costs nothing the fountain notices). The camera also subsamples chroma
/// to a quarter resolution, which is precisely the module-scale detail the blue
/// plane has to carry, so the extra layer is expected to fail more often than the
/// base one. It failing is harmless; that is the whole reason it is a second
/// fountain frame rather than half of each frame's bits.
fn paint_mosaic(
    frames: &[Vec<u8>],
    version: i16,
    layout: &Layout,
    box_w: u32,
    box_h: u32,
    color: bool,
) -> Raster {
    let total = symbol_modules(version);
    let side = total * layout.scale;
    let w = std::cmp::max(box_w, layout.drawn.0);
    let h = std::cmp::max(box_h, layout.drawn.1);
    // The whole mosaic is centred as a block, so the gaps between codes are the
    // quiet zones themselves and nothing has to be spaced by eye.
    let left = (w - layout.drawn.0) / 2;
    let top = (h - layout.drawn.1) / 2;

    // White, not the theme's background: the quiet zone and the light modules are
    // half of the contrast a camera measures, and a themed panel would be reading
    // a code printed on grey.
    let mut rgba = vec![0xffu8; (w * h * 4) as usize];
    let tiles = layout.tiles() as usize;
    let encode = |bytes: &[u8]| {
        encode_byte_mode(bytes, version)
            .expect("the version was pinned by encoding a frame-sized probe the same way")
    };
    for (tile, bytes) in frames.iter().take(tiles).enumerate() {
        let code = encode(bytes);
        let width = code.width() as u32;
        let base = code.to_colors();
        // The second layer is optional per tile rather than per image only so a
        // short `frames` slice cannot index out of bounds; in practice the caller
        // always sends either `tiles` or `2 * tiles` of them.
        let extra = color
            .then(|| frames.get(tiles + tile))
            .flatten()
            .map(|bytes| encode(bytes).to_colors());
        let tile = tile as u32;
        let origin_x = left + (tile % layout.grid.0) * side + QUIET_MODULES * layout.scale;
        let origin_y = top + (tile / layout.grid.0) * side + QUIET_MODULES * layout.scale;
        for my in 0..width {
            for mx in 0..width {
                let at = (my * width + mx) as usize;
                let base_dark = base[at] == qrcode::Color::Dark;
                let extra_dark = extra.as_ref().is_some_and(|e| e[at] == qrcode::Color::Dark);
                // The background is already white, so a module that is light in
                // both layers is nothing to do. Skipping it also keeps the
                // monochrome path exactly as cheap as it was.
                if !base_dark && !extra_dark {
                    continue;
                }
                // Red and green carry the base code, blue carries the extra one.
                // Every pixel is one of the four corners of that pair, which is
                // why there is no blending anywhere and no intermediate value a
                // camera would have to resolve.
                let rg = if base_dark { 0 } else { 0xff };
                // With no second layer the blue channel follows the base code, so
                // a dark module is black and not blue. Colour has to be something
                // that was asked for, never a tint that appears because a channel
                // happened to be free.
                let b = match &extra {
                    Some(_) => {
                        if extra_dark {
                            0
                        } else {
                            0xff
                        }
                    }
                    None => rg,
                };
                let x0 = origin_x + mx * layout.scale;
                let y0 = origin_y + my * layout.scale;
                for y in y0..y0 + layout.scale {
                    let row = (y * w) as usize * 4;
                    for x in x0..x0 + layout.scale {
                        let i = row + x as usize * 4;
                        rgba[i] = rg;
                        rgba[i + 1] = rg;
                        rgba[i + 2] = b;
                    }
                }
            }
        }
    }
    Raster { rgba: Arc::new(rgba), w, h, serial: crate::grid::next_image_serial() }
}

/// Modules across one drawn code, quiet zone included. A version-`v` symbol is
/// `4v + 17` modules; version 40 with its quiet zone is 185.
fn symbol_modules(version: i16) -> u32 {
    (4 * version.max(1) as u32) + 17 + 2 * QUIET_MODULES
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
        let t = Transfer::start("a.jpg", "image/jpeg", &sample(), DEFAULT_FRAME_BYTES, 24, 1, false).unwrap();
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
        let err = match Transfer::start("a.jpg", "image/jpeg", &big, 100, 24, 1, false) {
            Err(e) => e,
            Ok(_) => panic!("a payload that outruns the block numbering must be refused"),
        };
        assert!(err.contains("bytes per frame"), "the error must name the fix: {err}");
    }

    #[test]
    fn a_frame_no_larger_than_its_header_is_refused() {
        let err = match Transfer::start("a.bin", "text/plain", b"hello", 20, 24, 1, false) {
            Err(e) => e,
            Ok(_) => panic!("a frame with no room for a payload must be refused"),
        };
        assert!(err.contains("header"), "{err}");
    }

    #[test]
    fn frames_advance_on_the_clock_and_never_skip_ahead() {
        let mut t =
            Transfer::start("a.bin", "application/octet-stream", &sample(), DEFAULT_FRAME_BYTES, 20, 1, false)
                .unwrap();
        let one = t.layout_for(800, 800);
        let t0 = Instant::now();
        assert!(t.advance(t0, &one), "the first frame shows immediately");
        assert_eq!(t.frames_sent, 1);
        // Too soon: the camera has not had a chance to read what is up there.
        assert!(!t.advance(t0 + Duration::from_millis(10), &one));
        assert_eq!(t.frames_sent, 1);
        assert!(t.advance(t0 + Duration::from_millis(60), &one));
        assert_eq!(t.frames_sent, 2);

        // A long stall must not then flush a burst of frames through in one
        // repaint: every one of those but the last would be unreadable.
        let after = t0 + Duration::from_secs(5);
        assert!(t.advance(after, &one));
        assert!(!t.advance(after, &one));
        assert_eq!(t.frames_sent, 3);
    }

    #[test]
    fn a_paused_transfer_holds_its_frame() {
        let mut t = Transfer::start("a.bin", "text/plain", &sample(), DEFAULT_FRAME_BYTES, 20, 1, false).unwrap();
        let one = t.layout_for(800, 800);
        let t0 = Instant::now();
        t.advance(t0, &one);
        t.paused = true;
        assert!(!t.advance(t0 + Duration::from_secs(1), &one));
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
        let mut t = Transfer::start("a.txt", "text/plain", &incompressible(90_000), DEFAULT_FRAME_BYTES, 24, 1, false)
            .unwrap();
        let lay = t.layout_for(200, 200);
        let mut now = Instant::now();
        for _ in 0..20 {
            t.advance(now, &lay);
            now += Duration::from_millis(50);
            t.raster(&lay, 200, 200);
        }
    }

    #[test]
    fn every_frame_is_a_real_decodable_qr_of_the_pinned_version() {
        let mut t =
            Transfer::start("a.bin", "application/octet-stream", &sample(), DEFAULT_FRAME_BYTES, 24, 1, false)
                .unwrap();
        let lay = t.layout_for(800, 800);
        let mut seen = Vec::new();
        for i in 0..5 {
            t.advance(Instant::now() + Duration::from_millis(i * 100), &lay);
            let r = t.raster(&lay, 800, 800);
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
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24, 1, false).unwrap();
        let lay = t.layout_for(900, 900);
        t.advance(Instant::now(), &lay);
        let r = t.raster(&lay, 900, 900);
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
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24, 1, false).unwrap();
        let lay = t.layout_for(800, 800);
        t.advance(Instant::now(), &lay);
        let r = t.raster(&lay, 800, 800);
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
        let mut t = Transfer::start("a.bin", "text/plain", b"hello there", 2953, 24, 1, false).unwrap();
        let lay = t.layout_for(1, 1);
        t.advance(Instant::now(), &lay);
        let r = t.raster(&lay, 1, 1);
        assert!(r.w >= 185, "the box grows to hold one pixel per module: {}", r.w);
    }

    #[test]
    fn two_transfers_of_the_same_file_get_different_sessions() {
        // Or a receiver watching the first would feed the second's frames into the
        // decoder it already had.
        let mut a = Transfer::start("a.bin", "text/plain", &sample(), 2953, 24, 1, false).unwrap();
        let mut b = Transfer::start("a.bin", "text/plain", &sample(), 2953, 24, 1, false).unwrap();
        let lay = a.layout_for(400, 400);
        a.advance(Instant::now(), &lay);
        b.advance(Instant::now(), &lay);
        assert_ne!(a.raster(&lay, 400, 400).rgba, b.raster(&lay, 400, 400).rgba);
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

        // A mosaic is the interesting case: several codes in ONE image, which is
        // what a camera really sees on a wide window. `RUNNIR_PAINTED_TILES=4`
        // paints four per frame, and the harness has to read them all.
        let tiles: usize = std::env::var("RUNNIR_PAINTED_TILES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        // `RUNNIR_PAINTED_COLOR=1` paints the two-layer colour scheme, which the
        // harness then has to read TWICE — once as brightness for the base code
        // and once off the blue channel for the extra one.
        let color = std::env::var("RUNNIR_PAINTED_COLOR").is_ok_and(|v| v != "0" && !v.is_empty());

        let bytes = incompressible(60_000);
        let mut t = Transfer::start(
            "painted.bin",
            "image/jpeg",
            &bytes,
            DEFAULT_FRAME_BYTES,
            24,
            tiles,
            color,
        )
        .unwrap();
        // Three pixels per module: below that a decoder starts failing on its own
        // account, and this test is not about how small a code can get.
        let unit = (185 * 3) as u32;
        let lay = t.layout_for(unit * tiles.max(1) as u32, unit);
        let (box_w, box_h) = lay.drawn;
        // Enough to rebuild the file with room to spare, so a failure means the
        // decoder could not READ a frame rather than that it ran out of them.
        // Counted in DRAWN images, each of which now carries `tiles` frames, or
        // twice that in colour.
        let per_image = (lay.tiles() * t.layers()) as usize;
        let count = ((t.blocks() as f64 * 2.0).ceil() as usize + 8).div_ceil(per_image);

        let index = format!(
            "{{\"frames\":{count},\"px\":{box_w},\"w\":{box_w},\"h\":{box_h},\
             \"tiles\":{},\"color\":{color},\"grid\":\"{}x{}\",\"blocks\":{},\"version\":{},\
             \"name\":\"painted.bin\",\"size\":{},\"code\":\"{}\"}}\n",
            lay.tiles(),
            lay.grid.0,
            lay.grid.1,
            t.blocks(),
            t.version,
            bytes.len(),
            t.verification_code()
        );
        let mut now = Instant::now();
        for i in 0..count {
            t.advance(now, &lay);
            now += Duration::from_millis(100);
            let r = t.raster(&lay, box_w, box_h);
            std::fs::write(format!("{dir}/frame-{i:04}.rgba"), r.rgba.as_slice()).unwrap();
        }
        std::fs::write(format!("{dir}/index.json"), index).unwrap();
        eprintln!(
            "painted {count} images of {box_w}x{box_h} ({}x{} codes each) into {dir}",
            lay.grid.0, lay.grid.1
        );
    }

    /// What a mosaic costs, measured rather than assumed.
    ///
    /// Every tile is a whole version-40 encode plus its paint, and the bill
    /// arrives `fps` times a second. This is the number that decides whether a
    /// grid is worth offering at all — if painting a frame takes longer than the
    /// interval between frames, the stream slows down instead of speeding up.
    ///
    /// Measured on an Iris Xe laptop, release build: 12.8 ms for one code, and
    /// it scales linearly — 2 codes 26 ms, 4 codes 53 ms. Split further, 8 ms of
    /// that is the QR ENCODE and 0.4 ms is the painting; the rest is the
    /// fountain's XOR. So the ceiling on a mosaic is the encoder, not the
    /// camera, and it lands at two codes on a 30 fps budget of 33 ms.
    ///
    /// Ignored because it is a measurement, not an assertion:
    /// `cargo test --release -- --ignored --nocapture cost_of_a_mosaic`
    #[test]
    #[ignore = "a measurement, not an assertion; run it explicitly"]
    fn cost_of_a_mosaic() {
        let bytes = incompressible(300_000);
        // Colour is measured beside the mosaic because it is the same bill in a
        // different currency: two encodes for one tile rather than two tiles.
        // Whether they cost the same is the question the number answers.
        for (tiles, color) in [(1usize, false), (2, false), (4, false), (8, false), (1, true), (2, true)] {
            let mut t = Transfer::start(
                "m.bin",
                "image/jpeg",
                &bytes,
                DEFAULT_FRAME_BYTES,
                30,
                tiles,
                color,
            )
            .unwrap();
            let unit = 185 * 4;
            let lay = t.layout_for(unit * tiles as u32, unit * tiles as u32);
            let mut now = Instant::now();
            let start = Instant::now();
            const ROUNDS: u32 = 10;
            for _ in 0..ROUNDS {
                t.advance(now, &lay);
                now += Duration::from_millis(100);
                t.raster(&lay, lay.drawn.0, lay.drawn.1);
            }
            let each = start.elapsed() / ROUNDS;
            let budget = Duration::from_secs_f64(1.0 / 30.0);
            eprintln!(
                "  automatic rate here: {} fps",
                {
                    let mut auto = Transfer::start(
                        "m.bin",
                        "image/jpeg",
                        &bytes,
                        DEFAULT_FRAME_BYTES,
                        0,
                        tiles,
                        color,
                    )
                    .unwrap();
                    auto.advance(Instant::now(), &lay);
                    auto.raster(&lay, lay.drawn.0, lay.drawn.1);
                    auto.effective_fps()
                }
            );
            eprintln!(
                "{}x{}{} = {} codes at x{}px: {:?} per drawn frame ({:.0}% of a 30 fps budget)",
                lay.grid.0,
                lay.grid.1,
                if color { " colour" } else { "" },
                lay.tiles() * t.layers(),
                lay.scale,
                each,
                each.as_secs_f64() / budget.as_secs_f64() * 100.0
            );
        }
    }

    #[test]
    fn every_tile_of_a_mosaic_carries_a_different_frame() {
        // Two tiles showing the same sequence number would be the same picture
        // twice: it would look like a mosaic and double nothing. The sequence
        // steps by a whole mosaic for exactly this reason.
        let mut t =
            Transfer::start("a.bin", "image/jpeg", &incompressible(80_000), DEFAULT_FRAME_BYTES, 30, 4, false)
                .unwrap();
        let lay = t.layout_for(185 * 4 * 4, 185 * 4);
        assert_eq!(lay.tiles(), 4, "four codes should fit across that box: {:?}", lay.grid);

        let now = Instant::now();
        t.advance(now, &lay);
        assert_eq!(t.frames_sent, 4, "a drawn frame puts four codes on the glass");
        t.advance(now + Duration::from_millis(100), &lay);
        assert_eq!(t.frames_sent, 8);
        // Four codes a frame at 30 fps is 120 a second, so the pass is four times
        // shorter than the same file through a single code.
        let one = Transfer::start("a.bin", "image/jpeg", &incompressible(80_000), DEFAULT_FRAME_BYTES, 30, 1, false)
            .unwrap();
        assert!(
            t.shortest_pass().as_secs_f64() < one.shortest_pass().as_secs_f64() / 3.5,
            "four tiles should cut the pass by about four: {:?} vs {:?}",
            t.shortest_pass(),
            one.shortest_pass()
        );
    }

    /// The colour scheme, checked where it matters: in the pixels.
    ///
    /// `paint_mosaic` is called directly rather than through two `Transfer`s,
    /// because each transfer draws its own session id and would encode different
    /// bytes — and the whole claim here is about two paintings of the SAME frame.
    fn painted(frames: &[Vec<u8>], color: bool) -> (Raster, u32) {
        let version = pin_version(DEFAULT_FRAME_BYTES).unwrap();
        let scale = 3;
        let side = symbol_modules(version) * scale;
        let layout = Layout { grid: (1, 1), scale, drawn: (side, side) };
        (paint_mosaic(frames, version, &layout, side, side, color), side)
    }

    /// Rec. 601 brightness, which is what a decoder reduces colour to and what a
    /// camera's luma plane carries at full resolution.
    fn luma(px: &[u8]) -> u32 {
        (299 * px[0] as u32 + 587 * px[1] as u32 + 114 * px[2] as u32) / 1000
    }

    #[test]
    fn a_colour_frame_is_still_an_ordinary_qr_code_in_black_and_white() {
        // The claim that makes colour safe to ship: a decoder that knows nothing
        // about it — decimen's receivers, any QR app — reads the base code and
        // simply never sees the second one. So the DARK-BY-BRIGHTNESS pixels of a
        // colour painting have to be exactly the black pixels of the plain one.
        let base = vec![0xa5u8; DEFAULT_FRAME_BYTES];
        let extra = vec![0x5au8; DEFAULT_FRAME_BYTES];
        let (mono, side) = painted(std::slice::from_ref(&base), false);
        let (colour, _) = painted(&[base.clone(), extra.clone()], true);
        let (extra_alone, _) = painted(std::slice::from_ref(&extra), false);
        assert_eq!(mono.rgba.len(), colour.rgba.len());

        let mut colours = std::collections::BTreeSet::new();
        for i in (0..(side * side * 4) as usize).step_by(4) {
            let c = &colour.rgba[i..i + 4];
            colours.insert((c[0], c[1], c[2]));
            assert_eq!(
                luma(c) < 128,
                mono.rgba[i] == 0,
                "brightness at pixel {} disagrees with the plain painting",
                i / 4
            );
            // And the blue channel on its own is the extra code, module for
            // module, quiet zone included — the white surround is blue-light, so
            // the second code inherits the first one's quiet zone for free.
            assert_eq!(
                c[2] == 0,
                extra_alone.rgba[i] == 0,
                "the blue channel at pixel {} is not the second code",
                i / 4
            );
        }
        // Four corners of a two-bit choice and nothing in between: no blending,
        // no intermediate value for a camera to resolve.
        assert_eq!(
            colours,
            [(0, 0, 0), (0, 0, 0xff), (0xff, 0xff, 0), (0xff, 0xff, 0xff)].into_iter().collect(),
        );
    }

    #[test]
    fn without_colour_nothing_is_tinted() {
        // The blue channel follows the base code when there is no second layer.
        // Left free it would paint every dark module blue — a stream that still
        // decodes, on a screen that looks broken.
        let (mono, side) = painted(&[vec![0xa5u8; DEFAULT_FRAME_BYTES]], false);
        for i in (0..(side * side * 4) as usize).step_by(4) {
            assert_eq!(mono.rgba[i], mono.rgba[i + 2], "pixel {} is tinted", i / 4);
            assert_eq!(mono.rgba[i], mono.rgba[i + 1], "pixel {} is tinted", i / 4);
        }
    }

    #[test]
    fn colour_sends_two_frames_per_tile_and_halves_the_pass() {
        let file = incompressible(80_000);
        let mut plain =
            Transfer::start("a.bin", "image/jpeg", &file, DEFAULT_FRAME_BYTES, 30, 1, false).unwrap();
        let mut colour =
            Transfer::start("a.bin", "image/jpeg", &file, DEFAULT_FRAME_BYTES, 30, 1, true).unwrap();
        let lay = plain.layout_for(185 * 4, 185 * 4);
        assert_eq!(lay.tiles(), 1);

        let now = Instant::now();
        plain.advance(now, &lay);
        colour.advance(now, &lay);
        assert_eq!(plain.frames_sent, 1);
        assert_eq!(colour.frames_sent, 2, "one tile, two codes");
        // The sequence has to step by both, or the next drawn frame repeats one
        // the receiver already has and the second layer buys nothing.
        colour.advance(now + Duration::from_millis(100), &lay);
        assert_eq!(colour.frames_sent, 4);
        assert!(
            (colour.shortest_pass().as_secs_f64() * 2.0 - plain.shortest_pass().as_secs_f64()).abs()
                < 1e-6,
            "colour should halve the shortest pass: {:?} vs {:?}",
            colour.shortest_pass(),
            plain.shortest_pass()
        );
    }

    #[test]
    fn the_automatic_rate_steps_down_to_what_the_machine_paints_and_an_explicit_one_does_not() {
        let file = incompressible(80_000);
        let mut auto =
            Transfer::start("a.bin", "image/jpeg", &file, DEFAULT_FRAME_BYTES, 0, 1, true).unwrap();
        assert!(auto.fps_is_automatic());
        // Nothing painted yet, nothing measured: it asks for the ceiling, which
        // is the right guess to be wrong by one frame about.
        assert_eq!(auto.effective_fps(), AUTO_FPS_MAX);

        let lay = auto.layout_for(185 * 4, 185 * 4);
        auto.advance(Instant::now(), &lay);
        auto.raster(&lay, lay.drawn.0, lay.drawn.1);
        let chosen = auto.effective_fps();
        assert!(
            (AUTO_FPS_MIN..=AUTO_FPS_MAX).contains(&chosen),
            "the automatic rate stays inside its bounds: {chosen}"
        );
        // Above the floor, whatever it chose has to be a rate it can paint. AT
        // the floor it may not be, and that is deliberate: a machine too slow for
        // five frames a second is told rather than throttled to a standstill.
        // Asserting the floor is always paintable would be asserting something
        // about the machine — and a debug build paints an order of magnitude
        // slower than the release one this ships as.
        assert!(
            chosen == AUTO_FPS_MIN || !auto.painter_behind(),
            "the automatic rate chose {chosen}, which it cannot paint"
        );

        // More codes per frame is more painting, so the rate it settles on can
        // only go one way. This holds whatever the machine — which is the point
        // of checking the direction rather than the number.
        let mut mono =
            Transfer::start("a.bin", "image/jpeg", &file, DEFAULT_FRAME_BYTES, 0, 1, false).unwrap();
        mono.advance(Instant::now(), &lay);
        mono.raster(&lay, lay.drawn.0, lay.drawn.1);
        assert!(
            auto.effective_fps() <= mono.effective_fps(),
            "colour paints two codes a frame, so it cannot choose a faster rate than mono: {} vs {}",
            auto.effective_fps(),
            mono.effective_fps()
        );

        // An explicit rate is obeyed, even an absurd one: the panel warns rather
        // than overriding a number a person typed.
        let mut forced =
            Transfer::start("a.bin", "image/jpeg", &file, DEFAULT_FRAME_BYTES, 120, 8, true).unwrap();
        assert!(!forced.fps_is_automatic());
        assert_eq!(forced.effective_fps(), 120);
        let lay = forced.layout_for(185 * 8, 185 * 8);
        forced.advance(Instant::now(), &lay);
        forced.raster(&lay, lay.drawn.0, lay.drawn.1);
        assert_eq!(forced.effective_fps(), 120, "an explicit rate is never stepped down");
        assert!(forced.painter_behind(), "sixteen codes at 120 fps cannot possibly be painted");
    }

    #[test]
    fn an_explicit_tile_count_may_shrink_the_modules_but_the_automatic_one_never_does() {
        let auto =
            Transfer::start("a.bin", "image/jpeg", &incompressible(80_000), DEFAULT_FRAME_BYTES, 30, 0, false)
                .unwrap();
        let forced =
            Transfer::start("a.bin", "image/jpeg", &incompressible(80_000), DEFAULT_FRAME_BYTES, 30, 4, false)
                .unwrap();
        // A square box with room for exactly one code at four pixels a module.
        let (w, h) = (185 * 4, 185 * 4);
        let a = auto.layout_for(w, h);
        let f = forced.layout_for(w, h);
        assert_eq!(a.tiles(), 1, "there is no room for a second code at full size");
        assert_eq!(a.scale, 4);
        assert_eq!(f.tiles(), 4, "an explicit count is allowed to spend sharpness");
        assert_eq!(f.scale, 2, "and it spends it by halving the modules");
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
