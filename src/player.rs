//! Playing a TIDAL stream: fetching it, decoding it, and getting it to a DAC with as
//! little done to it as the hardware allows.
//!
//! The rule this module exists to keep: **the music always plays, and the badge never
//! lies about how.** Bit-perfect is the best case, not a requirement. When the DAC can
//! take the stream exactly as it was decoded, nothing touches the samples; when it
//! cannot, we say so and fall down the chain rather than going silent.
//!
//! Nothing here draws, and nothing here knows about the panel. It is given a
//! [`crate::tidal::StreamInfo`] and it makes sound.

use std::io::Read;

use crate::config::Tidal as TidalCfg;
use crate::tidal::{self, Media, StreamInfo};

/// How much of one URL we are willing to pull into memory. A hi-res track runs to a few
/// hundred megabytes decoded but far less on the wire; this only guards against a
/// redirect to something that is not a track at all.
const MAX_PART_BYTES: usize = 512 * 1024 * 1024;

/// The rung of the output chain the audio actually came out on. This is what the status
/// badge reports, and the whole point of naming them is that "bit-perfect" must mean
/// bit-perfect and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    /// Exclusive access, the source's own rate, the source's own depth. No conversion
    /// of any kind between the decoder and the DAC.
    BitPerfect,
    /// Exclusive access and the source's own rate, but the samples were widened into a
    /// bigger container (16 → 32, or 24 → 32). Zero-padding is a bit-exact operation,
    /// so this is still lossless; it is a separate rung because it is not the same
    /// claim.
    ExclusivePadded,
    /// Exclusive access, but the rate had to change. Now something is resampling.
    ExclusiveResampled,
    /// ALSA's own conversion layer (`plughw:`). Whatever it takes to play.
    Converted,
    /// The system mixer — PipeWire or PulseAudio. Shared with every other application,
    /// never bit-perfect, and the rung that always works.
    Shared,
}

impl Rung {
    pub fn label(self) -> &'static str {
        match self {
            Rung::BitPerfect => "BIT-PERFECT",
            Rung::ExclusivePadded => "exclusive (padded)",
            Rung::ExclusiveResampled => "exclusive (resampled)",
            Rung::Converted => "converted",
            Rung::Shared => "SHARED",
        }
    }

    /// True only for a path that changed no sample value at all. Read by the panel's
    /// badge in phase 1; kept here beside the rungs it judges.
    #[allow(dead_code)]
    pub fn is_bit_exact(self) -> bool {
        matches!(self, Rung::BitPerfect | Rung::ExclusivePadded)
    }
}

/// What the decoder produced, what the device accepted, and every difference between
/// them. The panel shows this; it is also the only honest way to answer "am I actually
/// getting hi-res?".
#[derive(Clone, Debug, Default)]
pub struct SignalPath {
    pub device: String,
    pub rung: Option<Rung>,
    pub decoded_bits: u32,
    pub decoded_rate: u32,
    // Read by the panel in phase 1. The badge summarises; the panel shows the pair
    // (what was decoded, what the device took) side by side, which is the form that
    // answers "why is this not bit-perfect".
    #[allow(dead_code)]
    pub decoded_channels: u32,
    #[allow(dead_code)]
    pub output_bits: u32,
    #[allow(dead_code)]
    pub output_rate: u32,
    /// Set when the container was widened — `Some((16, 32))` reads as "16-bit samples
    /// in a 32-bit container".
    pub padded: Option<(u32, u32)>,
    pub resampled: Option<(u32, u32)>,
    /// TIDAL's word for what it served, which is not always what was asked for.
    pub quality: String,
}

impl SignalPath {
    /// The status-bar line. Deliberately assembled here rather than in the drawing code
    /// so that what the user reads and what actually happened cannot drift apart.
    pub fn badge(&self) -> String {
        let mut s = format!(
            "{} {}/{} · {}",
            if self.quality.is_empty() { "PCM" } else { self.quality.as_str() },
            self.decoded_bits,
            fmt_khz(self.decoded_rate),
            self.device
        );
        if let Some((from, to)) = self.padded {
            s.push_str(&format!(" · {from}→{to} zero-pad"));
        }
        if let Some((from, to)) = self.resampled {
            s.push_str(&format!(" · resampled {} → {}", fmt_khz(from), fmt_khz(to)));
        }
        if let Some(rung) = self.rung {
            s.push_str(&format!(" · {}", rung.label()));
        }
        s
    }
}

fn fmt_khz(rate: u32) -> String {
    if rate % 1000 == 0 {
        format!("{} kHz", rate / 1000)
    } else {
        format!("{:.1} kHz", rate as f64 / 1000.0)
    }
}

/// One device to try, and how hard we are allowed to bend for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub device: String,
    /// Refuse anything but the source's own rate and depth.
    pub exact: bool,
    /// Refuse a rate change (a wider container is still allowed).
    pub same_rate: bool,
}

/// The order in which devices are tried, before any hardware is touched.
///
/// Split out from the opening so it can be tested without a sound card, because the
/// ORDER is the feature: a chain that reaches `default` too eagerly silently costs the
/// quality the whole panel exists to deliver, and one that never reaches it leaves the
/// laptop mute.
pub fn plan(pref: &str, bit_perfect: bool, hw_devices: &[String]) -> Vec<Attempt> {
    // Asking for `default` is a decision to hand the audio to the system mixer, and
    // there is nothing better to fall back to from there. It must NOT then go on to
    // grab an exclusive device: someone who chose the mixer wants the card left alone.
    if pref == "default" {
        return vec![Attempt { device: "default".into(), exact: false, same_rate: false }];
    }
    let mut out = Vec::new();
    // An explicit device is a decision, not a hint: it is tried first, at every rung,
    // before anything else is considered.
    let named = !matches!(pref, "auto" | "" | "default");
    let heads: Vec<String> =
        if named { vec![pref.to_string()] } else { hw_devices.to_vec() };

    if bit_perfect {
        for d in &heads {
            out.push(Attempt { device: d.clone(), exact: true, same_rate: true });
        }
        // Second pass: same devices, but a wider container is acceptable. Kept as a
        // separate pass so a second card cannot take an exact match away from the first
        // one just by being listed earlier.
        for d in &heads {
            out.push(Attempt { device: d.clone(), exact: false, same_rate: true });
        }
    }
    // Exclusive, resampling allowed.
    for d in &heads {
        out.push(Attempt { device: d.clone(), exact: false, same_rate: false });
    }
    // ALSA's conversion layer for the preferred card, then the system mixer. `default`
    // is last and unconditional: it is the rung that cannot fail, so the chain always
    // ends somewhere that makes sound.
    if let Some(first) = heads.first() {
        if let Some(rest) = first.strip_prefix("hw:") {
            out.push(Attempt {
                device: format!("plughw:{rest}"),
                exact: false,
                same_rate: false,
            });
        }
    }
    out.push(Attempt { device: "default".into(), exact: false, same_rate: false });
    out
}

/// Reads a track that arrives as an ordered list of URLs.
///
/// A DASH stream is an initialisation segment plus N media segments that mean nothing
/// apart; a BTS stream is one file. Both are "read these URLs in order", so both are
/// this. Parts are fetched as they are needed rather than up front — a hi-res track is
/// hundreds of megabytes and the first sample should not wait for the last byte.
struct PartsReader {
    parts: Vec<Part>,
    next: usize,
    current: std::io::Cursor<Vec<u8>>,
}

/// One piece of a track. A local file is not something TIDAL ever sends: it exists so
/// the decoder and the whole output chain can be exercised with a known file, on a
/// machine with no subscription and no network. The audio path is the half of this
/// feature that a unit test cannot reach, so it needs a way in that does not depend on
/// a service being up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Url(String),
    File(std::path::PathBuf),
}

impl PartsReader {
    fn new(parts: Vec<Part>) -> Self {
        Self { parts, next: 0, current: std::io::Cursor::new(Vec::new()) }
    }
}

impl Read for PartsReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = self.current.read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            if self.next >= self.parts.len() {
                return Ok(0); // end of track
            }
            let part = self.parts[self.next].clone();
            self.next += 1;
            let bytes = match part {
                Part::Url(url) => tidal::fetch(&url, MAX_PART_BYTES)
                    .map_err(|e| std::io::Error::other(format!("fetch part: {e}")))?,
                Part::File(path) => std::fs::read(path)?,
            };
            self.current = std::io::Cursor::new(bytes);
        }
    }
}

impl std::io::Seek for PartsReader {
    /// Never seekable. Symphonia asks and then works forward-only, which is what a
    /// stream is. Scrubbing within a track is a later phase and will need range
    /// requests, not this.
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "stream is not seekable"))
    }
}

impl symphonia::core::io::MediaSource for PartsReader {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

/// The parts of a stream, in the order they must be read.
pub fn parts_of(info: &StreamInfo) -> Result<Vec<Part>, String> {
    match info.media()? {
        Media::Direct(urls) => Ok(urls.iter().cloned().map(Part::Url).collect()),
        Media::Dash { init, segments } => {
            let mut parts = Vec::with_capacity(segments.len() + 1);
            // The initialisation segment carries the codec setup; without it first the
            // media segments are undecodable bytes.
            if let Some(init) = init {
                parts.push(Part::Url(init.clone()));
            }
            parts.extend(segments.iter().cloned().map(Part::Url));
            Ok(parts)
        }
    }
}

/// A file-extension hint for the demuxer, from what TIDAL said it was sending. Symphonia
/// can probe without it, but a hint saves it guessing between FLAC-in-MP4 and raw FLAC,
/// which look different only a few bytes in.
pub fn hint_for(info: &StreamInfo) -> &'static str {
    if info.mime.contains("dash") {
        "mp4"
    } else if info.codec.contains("flac") || info.codec.is_empty() {
        "flac"
    } else {
        "m4a"
    }
}

/// What one track's playback did. Returned by the blocking play so the spike (and later
/// the panel) can report something checkable rather than "it seemed fine".
#[derive(Clone, Debug, Default)]
pub struct Played {
    pub frames: u64,
    pub signal: SignalPath,
    /// ALSA underruns. Not fatal — recovered from — but they are audible, so a run that
    /// had them is not a run that worked.
    pub underruns: u32,
}

/// Fetches, decodes and plays one track, blocking until it ends.
///
/// Phase 0 shape: one call, one track, no transport. The panel will drive a longer-lived
/// version of this loop, which is why the decode and the output are already separated
/// from the fetching.
pub fn play(info: &StreamInfo, cfg: &TidalCfg) -> Result<Played, String> {
    play_parts(parts_of(info)?, hint_for(info), &info.quality, cfg, true)
}

/// The whole audio path, from a list of parts to the DAC.
///
/// `output` off decodes and measures without opening a device — which is how the
/// decoder gets checked on a machine where making noise would be rude, and how a
/// stream that fails can be told apart from a device that refuses.
pub fn play_parts(
    parts: Vec<Part>,
    hint_ext: &str,
    quality: &str,
    cfg: &TidalCfg,
    output: bool,
) -> Result<Played, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    if parts.is_empty() {
        return Err("stream has no parts to play".into());
    }
    let source = Box::new(PartsReader::new(parts));
    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    hint.with_extension(hint_ext);

    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("cannot read this stream ({hint_ext}): {e}"))?;

    let track = format
        .first_track(TrackType::Audio)
        .ok_or("no audio track in stream")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("audio track has no codec parameters")?
        .clone();

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("no decoder for this codec: {e}"))?;

    let mut sink: Option<Sink> = None;
    let mut opened_yet: Option<Want> = None;
    let mut played = Played::default();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break, // end of stream
            Err(e) => {
                // A truncated last packet is how a stream that ended mid-segment looks;
                // anything else is a real failure and must not be swallowed.
                if is_end_of_stream(&e) {
                    break;
                }
                return Err(format!("read error: {e}"));
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| format!("decode error: {e}"))?;
        let spec = decoded.spec().clone();
        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }

        // The device is opened on the FIRST decoded packet, not before: only now is the
        // real rate, channel count and sample width known, and opening a DAC for a
        // format we then discover is different is how you get a click before track one.
        if opened_yet.is_none() {
            let want = Want {
                rate: spec.rate(),
                channels: spec.channels().count() as u32,
                bits: sample_bits(&decoded),
            };
            played.signal.decoded_bits = want.bits;
            played.signal.decoded_rate = want.rate;
            played.signal.decoded_channels = want.channels;
            played.signal.quality = quality.to_string();
            if output {
                let device = Sink::open(cfg, &want, quality)?;
                played.signal = device.signal.clone();
                sink = Some(device);
            } else {
                played.signal.device = "none (decode only)".into();
            }
            opened_yet = Some(want);
        }
        if let Some(sink) = sink.as_mut() {
            played.underruns += sink.write(&decoded)?;
        }
        played.frames += frames as u64;
    }

    if let Some(sink) = sink.as_mut() {
        sink.drain();
    }
    if played.frames == 0 {
        return Err("stream decoded to no audio at all".into());
    }
    Ok(played)
}

/// True for the "the stream just ended" flavours of error, which are normal.
fn is_end_of_stream(e: &symphonia::core::errors::Error) -> bool {
    match e {
        symphonia::core::errors::Error::IoError(io) => {
            io.kind() == std::io::ErrorKind::UnexpectedEof
        }
        symphonia::core::errors::Error::ResetRequired => false,
        _ => false,
    }
}

/// The bit depth the decoder actually produced, which is what bit-perfect is measured
/// against — not what the metadata claimed.
fn sample_bits(buf: &symphonia::core::audio::GenericAudioBufferRef<'_>) -> u32 {
    use symphonia::core::audio::GenericAudioBufferRef as B;
    match buf {
        B::U8(_) | B::S8(_) => 8,
        B::U16(_) | B::S16(_) => 16,
        B::U24(_) | B::S24(_) => 24,
        B::U32(_) | B::S32(_) => 32,
        B::F32(_) => 32,
        B::F64(_) => 64,
    }
}

/// What the decoder wants the device to accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub rate: u32,
    pub channels: u32,
    pub bits: u32,
}

#[cfg(target_os = "linux")]
pub use linux::Sink;

#[cfg(not(target_os = "linux"))]
pub use other::Sink;

/// The real hardware devices this machine has, for the chain and for reporting it.
/// Empty off Linux, where there is no exclusive path to report on.
pub fn hw_devices_public() -> Vec<Device> {
    #[cfg(target_os = "linux")]
    {
        devices_under(std::path::Path::new("/proc/asound"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Attempt, Rung, SignalPath, Want, plan};
    use crate::config::Tidal as TidalCfg;
    use alsa::pcm::{Access, Format, HwParams, PCM, State};
    use alsa::{Direction, ValueOr};

    pub struct Sink {
        pcm: PCM,
        format: Format,
        channels: u32,
        pub signal: SignalPath,
    }

    impl Sink {
        /// Walks the chain until something opens, and records which rung that was.
        pub fn open(cfg: &TidalCfg, want: &Want, quality: &str) -> Result<Sink, String> {
            let devices = hw_devices();
            let attempts = plan(&cfg.output, cfg.bit_perfect, &devices);
            let mut last_error = String::from("no audio device to try");
            for attempt in &attempts {
                match try_open(attempt, want) {
                    Ok(mut sink) => {
                        sink.signal.quality = quality.to_string();
                        return Ok(sink);
                    }
                    // A busy device is the single most common failure here — PipeWire is
                    // holding the card — and it is not an error, it is the next rung.
                    Err(e) => last_error = format!("{}: {e}", attempt.device),
                }
            }
            Err(format!("no usable audio output ({last_error})"))
        }

        /// Writes one decoded buffer. Returns the number of underruns recovered from,
        /// because a run that stuttered is not a run that worked.
        pub fn write(
            &mut self,
            decoded: &symphonia::core::audio::GenericAudioBufferRef<'_>,
        ) -> Result<u32, String> {
            let frames = decoded.frames();
            let samples = frames * self.channels as usize;
            let mut underruns = 0;
            match self.format {
                Format::S16LE => {
                    let mut buf = vec![0i16; samples];
                    decoded.copy_to_slice_interleaved(&mut buf[..]);
                    underruns += self.write_i16(&buf)?;
                }
                _ => {
                    let mut buf = vec![0i32; samples];
                    decoded.copy_to_slice_interleaved(&mut buf[..]);
                    underruns += self.write_i32(&buf)?;
                }
            }
            Ok(underruns)
        }

        fn write_i16(&mut self, buf: &[i16]) -> Result<u32, String> {
            let io = self.pcm.io_i16().map_err(|e| e.to_string())?;
            write_all(&self.pcm, buf, self.channels as usize, |b| io.writei(b))
        }

        fn write_i32(&mut self, buf: &[i32]) -> Result<u32, String> {
            let io = self.pcm.io_i32().map_err(|e| e.to_string())?;
            write_all(&self.pcm, buf, self.channels as usize, |b| io.writei(b))
        }

        /// Lets the device finish what it has been given rather than cutting the last
        /// fraction of a second off every track.
        pub fn drain(&mut self) {
            let _ = self.pcm.drain();
        }
    }

    /// Writes until the whole buffer is gone, recovering from underruns.
    ///
    /// A short write is normal — the device took what fitted — so the loop advances by
    /// what was actually accepted. An underrun is recovered rather than fatal, but it is
    /// counted: silently swallowing them turns "the audio stutters" into a bug with no
    /// evidence.
    fn write_all<S: Copy>(
        pcm: &PCM,
        buf: &[S],
        channels: usize,
        mut writei: impl FnMut(&[S]) -> Result<usize, alsa::Error>,
    ) -> Result<u32, String> {
        let mut offset = 0usize;
        let mut underruns = 0u32;
        while offset < buf.len() {
            match writei(&buf[offset..]) {
                Ok(0) => break,
                Ok(frames) => offset += frames * channels,
                Err(e) => {
                    // EPIPE is an underrun: the device ran dry while we were away.
                    if e.errno() == libc::EPIPE {
                        underruns += 1;
                        pcm.try_recover(e, true).map_err(|e| e.to_string())?;
                        continue;
                    }
                    return Err(e.to_string());
                }
            }
        }
        if pcm.state() == State::Prepared {
            pcm.start().map_err(|e| e.to_string())?;
        }
        Ok(underruns)
    }

    /// Opens one candidate, or explains why it could not be used.
    fn try_open(attempt: &Attempt, want: &Want) -> Result<Sink, String> {
        let pcm = PCM::new(&attempt.device, Direction::Playback, false)
            .map_err(|e| e.to_string())?;
        let (format, rate) = {
            let hwp = HwParams::any(&pcm).map_err(|e| e.to_string())?;
            hwp.set_access(Access::RWInterleaved).map_err(|e| e.to_string())?;
            hwp.set_channels(want.channels).map_err(|e| e.to_string())?;

            // Rate first: a rate change is the one conversion that is never bit-exact,
            // so it decides the rung before the format does.
            let rate = if hwp.set_rate(want.rate, ValueOr::Nearest).is_ok() {
                hwp.get_rate().map_err(|e| e.to_string())?
            } else {
                return Err("device refused the rate".into());
            };
            if rate != want.rate && attempt.same_rate {
                return Err(format!("device wants {rate} Hz, not {}", want.rate));
            }

            let format = choose_format(&hwp, want.bits, attempt.exact)?;
            hwp.set_format(format).map_err(|e| e.to_string())?;
            // A buffer long enough that a scheduling hiccup does not empty it, short
            // enough that pause and track change do not feel late.
            let _ = hwp.set_buffer_time_near(250_000, ValueOr::Nearest);
            let _ = hwp.set_period_time_near(50_000, ValueOr::Nearest);
            pcm.hw_params(&hwp).map_err(|e| e.to_string())?;
            (format, rate)
        };

        let out_bits = format_bits(format);
        let padded = (out_bits > want.bits).then_some((want.bits, out_bits));
        let resampled = (rate != want.rate).then_some((want.rate, rate));
        let shared = attempt.device == "default" || attempt.device.starts_with("plug");
        let rung = if shared && attempt.device == "default" {
            Rung::Shared
        } else if shared {
            Rung::Converted
        } else if resampled.is_some() {
            Rung::ExclusiveResampled
        } else if padded.is_some() {
            Rung::ExclusivePadded
        } else {
            Rung::BitPerfect
        };

        Ok(Sink {
            format,
            channels: want.channels,
            signal: SignalPath {
                device: attempt.device.clone(),
                rung: Some(rung),
                decoded_bits: want.bits,
                decoded_rate: want.rate,
                decoded_channels: want.channels,
                output_bits: out_bits,
                output_rate: rate,
                padded,
                resampled,
                quality: String::new(),
            },
            pcm,
        })
    }

    /// Picks the sample format: the source's own width when the device takes it, else
    /// the narrowest container that still holds every bit.
    ///
    /// Never narrower than the source. Truncating 24-bit samples into a 16-bit device
    /// would still play, and would quietly throw away the eight bits the whole feature
    /// is about.
    fn choose_format(hwp: &HwParams, bits: u32, exact: bool) -> Result<Format, String> {
        let exact_fmt = match bits {
            16 => Format::S16LE,
            24 => Format::S243LE, // packed 24 — ALSA's S24LE is the 32-bit container
            _ => Format::S32LE,
        };
        if hwp.test_format(exact_fmt).is_ok() {
            return Ok(exact_fmt);
        }
        if exact {
            return Err(format!("device does not take {bits}-bit samples"));
        }
        // Widening only. ALSA's S24LE is 24 bits inside a 32-bit word, which is why it
        // sits between the packed form and true 32-bit.
        for candidate in [Format::S24LE, Format::S32LE] {
            if format_bits(candidate) >= bits && hwp.test_format(candidate).is_ok() {
                return Ok(candidate);
            }
        }
        Err(format!("device takes no format wide enough for {bits}-bit samples"))
    }

    /// Bits of real audio in a format, NOT the container size: ALSA's `S24LE` carries 24
    /// bits in a 32-bit word, and calling that 32 would report a promotion that never
    /// happened.
    fn format_bits(f: Format) -> u32 {
        match f {
            Format::S16LE => 16,
            Format::S243LE | Format::S24LE => 24,
            _ => 32,
        }
    }

    /// The devices `auto` is allowed to choose between, best first.
    pub(super) fn hw_devices() -> Vec<String> {
        super::auto_candidates(&super::hw_devices_public())
    }
}

/// Filters the machine's devices down to the ones `auto` may pick.
///
/// HDMI is excluded. It would accept 48 kHz without complaint and could therefore WIN
/// the bit-perfect rung for a 48 kHz track — sending the music to a monitor that may
/// have no speakers at all, while headphones sit in the jack. Silence that looks like
/// success is the worst failure this chain can produce, so a display output is only
/// ever reachable by naming it in the config.
pub fn auto_candidates(devices: &[Device]) -> Vec<String> {
    let usable: Vec<String> =
        devices.iter().filter(|d| !d.is_display).map(|d| d.name.clone()).collect();
    // Unless there is nothing else at all: a machine whose only output is HDMI should
    // still play, and there the monitor IS the speakers.
    if usable.is_empty() {
        return devices.iter().map(|d| d.name.clone()).collect();
    }
    usable
}

/// One real playback device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The ALSA name to open: `hw:1,0`.
    pub name: String,
    /// What to show a person: `PCH — ALC256 Analog`.
    pub label: String,
    /// Lower sorts first. See [`devices_under`] for what the ranks mean.
    pub rank: u8,
    /// A monitor or television output. Never chosen by `auto` — see
    /// [`auto_candidates`].
    pub is_display: bool,
}

/// Reads the machine's playback devices out of `/proc/asound`.
///
/// ALSA's own hint iterator does NOT list `hw:` names — on this machine it returns
/// `default`, `sysdefault:CARD=…`, `front:…`, `hdmi:…` and nothing openable
/// exclusively. Those are all conversion or routing aliases. `/proc/asound` is where
/// the real cards are, so that is what is read: `cards` for the card list, then each
/// card's `pcmNp` directories for its playback devices.
///
/// The ORDER encodes an assumption worth stating: a USB card is almost always a DAC
/// somebody just plugged in on purpose, so it goes first; HDMI goes last, because an
/// idle monitor accepting 48 kHz would otherwise silently win "auto" over the DAC.
/// Everything else — the built-in analogue output, the headphone jack — sits between.
///
/// Takes the root as an argument so the parsing can be tested against a captured
/// directory instead of whatever hardware the test machine happens to have.
pub fn devices_under(root: &std::path::Path) -> Vec<Device> {
    let Ok(cards) = std::fs::read_to_string(root.join("cards")) else {
        return Vec::new();
    };
    let mut out: Vec<Device> = Vec::new();
    for line in cards.lines() {
        // " 1 [PCH            ]: HDA-Intel - HDA Intel PCH"
        let trimmed = line.trim_start();
        let Some((index, rest)) = trimmed.split_once(' ') else { continue };
        let Ok(card): Result<u32, _> = index.parse() else { continue };
        let card_id = rest
            .split_once('[')
            .and_then(|(_, r)| r.split_once(']'))
            .map(|(id, _)| id.trim().to_string())
            .unwrap_or_else(|| card.to_string());
        let is_usb = rest.contains("USB-Audio") || rest.contains("USB Audio");

        let card_dir = root.join(format!("card{card}"));
        let Ok(entries) = std::fs::read_dir(&card_dir) else { continue };
        let mut devices: Vec<u32> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // `pcm0p` is playback, `pcm0c` is capture. Only playback can be output.
                let digits = name.strip_prefix("pcm")?.strip_suffix('p')?;
                digits.parse::<u32>().ok()
            })
            .collect();
        devices.sort_unstable();

        for device in devices {
            let info = std::fs::read_to_string(card_dir.join(format!("pcm{device}p/info")))
                .unwrap_or_default();
            let pcm_name = info
                .lines()
                .find_map(|l| l.strip_prefix("name: "))
                .unwrap_or("")
                .trim()
                .to_string();
            let is_hdmi = pcm_name.contains("HDMI") || pcm_name.contains("DisplayPort");
            let rank = if is_usb {
                0
            } else if is_hdmi {
                2
            } else {
                1
            };
            let label = if pcm_name.is_empty() {
                card_id.clone()
            } else {
                format!("{card_id} — {pcm_name}")
            };
            out.push(Device {
                name: format!("hw:{card},{device}"),
                label,
                rank,
                is_display: is_hdmi,
            });
        }
    }
    // Stable sort: within a rank the card and device order from /proc is kept, which is
    // the order the kernel enumerated them in and the order aplay -l shows.
    out.sort_by_key(|d| d.rank);
    out
}

#[cfg(not(target_os = "linux"))]
mod other {
    use super::{SignalPath, Want};
    use crate::config::Tidal as TidalCfg;

    /// No output backend outside Linux yet. Browsing and the catalogue work; playback
    /// says so plainly instead of failing somewhere deeper with a stranger message.
    pub struct Sink {
        pub signal: SignalPath,
    }

    impl Sink {
        pub fn open(_: &TidalCfg, _: &Want, _: &str) -> Result<Sink, String> {
            Err("audio output on this platform is not implemented (ALSA is Linux-only)".into())
        }
        pub fn write(
            &mut self,
            _: &symphonia::core::audio::GenericAudioBufferRef<'_>,
        ) -> Result<u32, String> {
            Err("no audio output".into())
        }
        pub fn drain(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<String> {
        vec!["hw:0,0".to_string(), "hw:2,0".to_string()]
    }

    #[test]
    fn a_usb_dac_outranks_the_built_in_card_and_hdmi_comes_last() {
        let root = std::env::temp_dir().join("runnir-asound-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("cards"),
            " 0 [NVidia         ]: HDA-Intel - HDA NVidia\n             \x20                     HDA NVidia at 0x60080000 irq 17\n             1 [PCH            ]: HDA-Intel - HDA Intel PCH\n             \x20                     HDA Intel PCH at 0x622f280000 irq 216\n             2 [R3             ]: USB-Audio - HiBy R3\n",
        )
        .unwrap();
        for (card, dev, name) in
            [(0u32, 3u32, "HDMI 0"), (1, 0, "ALC256 Analog"), (2, 0, "HiBy R3")]
        {
            let dir = root.join(format!("card{card}/pcm{dev}p"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("info"), format!("card: {card}\nname: {name}\n")).unwrap();
            // A capture device on the same card must not be offered as an output.
            let cap = root.join(format!("card{card}/pcm{dev}c"));
            std::fs::create_dir_all(&cap).unwrap();
        }
        let found = devices_under(&root);
        assert_eq!(
            found.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["hw:2,0", "hw:1,0", "hw:0,3"],
            "USB first, HDMI last: an idle monitor must not win auto over the DAC"
        );
        assert_eq!(found[0].label, "R3 — HiBy R3");
        // auto never reaches for the monitor, even though it is a real device.
        assert_eq!(auto_candidates(&found), ["hw:2,0", "hw:1,0"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_machine_with_only_hdmi_still_plays_through_it() {
        let only_hdmi = vec![Device {
            name: "hw:0,3".into(),
            label: "NVidia — HDMI 0".into(),
            rank: 2,
            is_display: true,
        }];
        assert_eq!(auto_candidates(&only_hdmi), ["hw:0,3"]);
    }

    #[test]
    fn no_proc_asound_is_no_devices_rather_than_a_panic() {
        assert!(devices_under(std::path::Path::new("/nonexistent/asound")).is_empty());
    }

    #[test]
    fn the_chain_tries_every_card_exactly_before_bending_for_any_of_them() {
        let plan = plan("auto", true, &devices());
        // Both cards get an exact attempt before either gets a padded one — otherwise
        // the first card listed could take a lossy path while the second would have
        // played the stream untouched.
        assert_eq!(plan[0], Attempt { device: "hw:0,0".into(), exact: true, same_rate: true });
        assert_eq!(plan[1], Attempt { device: "hw:2,0".into(), exact: true, same_rate: true });
        assert!(!plan[2].exact);
    }

    #[test]
    fn the_chain_always_ends_somewhere_that_makes_sound() {
        for pref in ["auto", "hw:2,0"] {
            let plan = plan(pref, true, &devices());
            assert_eq!(
                plan.last().map(|a| a.device.as_str()).unwrap_or(""),
                "default",
                "chain for {pref:?} must be able to fall back"
            );
        }
    }

    #[test]
    fn a_named_device_is_a_decision_and_no_other_card_is_tried() {
        let plan = plan("hw:2,0", true, &devices());
        assert!(plan.iter().all(|a| a.device != "hw:0,0"));
        // …but plughw and default still follow it, because "this DAC" must not mean
        // "silence when this DAC is busy".
        assert!(plan.iter().any(|a| a.device == "plughw:2,0"));
        assert!(plan.iter().any(|a| a.device == "default"));
    }

    #[test]
    fn asking_for_the_mixer_leaves_the_hardware_alone() {
        // Choosing `default` is choosing to share the card. Trying an exclusive open
        // first would take the DAC away from whatever else is using it, which is the
        // opposite of what was asked for.
        let plan = plan("default", true, &devices());
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].device, "default");
    }

    #[test]
    fn without_bit_perfect_no_exact_attempt_is_made() {
        let plan = plan("auto", false, &devices());
        assert!(plan.iter().all(|a| !a.exact));
        assert!(plan.iter().all(|a| !a.same_rate));
    }

    #[test]
    fn the_badge_says_what_happened_to_the_samples() {
        let signal = SignalPath {
            device: "hw:2,0".into(),
            rung: Some(Rung::ExclusivePadded),
            decoded_bits: 24,
            decoded_rate: 96000,
            output_bits: 32,
            output_rate: 96000,
            padded: Some((24, 32)),
            quality: "HI_RES_LOSSLESS".into(),
            ..Default::default()
        };
        let badge = signal.badge();
        assert!(badge.contains("24/96 kHz"), "{badge}");
        assert!(badge.contains("24→32 zero-pad"), "{badge}");
        assert!(badge.contains("exclusive (padded)"), "{badge}");
        // Padding is bit-exact; resampling is not, and the two must never read alike.
        assert!(Rung::ExclusivePadded.is_bit_exact());
        assert!(!Rung::ExclusiveResampled.is_bit_exact());
        assert!(!Rung::Shared.is_bit_exact());
    }

    #[test]
    fn odd_sample_rates_keep_their_decimal() {
        assert_eq!(fmt_khz(44100), "44.1 kHz");
        assert_eq!(fmt_khz(192000), "192 kHz");
    }

    #[test]
    fn a_dash_stream_reads_its_init_segment_first() {
        let info = StreamInfo {
            media: Some(Media::Dash {
                init: Some("https://x/init.mp4".into()),
                segments: vec!["https://x/1.mp4".into(), "https://x/2.mp4".into()],
            }),
            mime: "application/dash+xml".into(),
            ..Default::default()
        };
        let parts = parts_of(&info).unwrap();
        assert_eq!(parts[0], Part::Url("https://x/init.mp4".into()));
        assert_eq!(parts.len(), 3);
        assert_eq!(hint_for(&info), "mp4");
    }
}
