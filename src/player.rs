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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    /// Every device that was tried and would not take it, with what it said.
    ///
    /// Without this, landing on the laptop speakers instead of the DAC is indis-
    /// tinguishable from choosing them: the badge names where the audio WENT, and this
    /// is the only record of where it could not go. It is the first question anyone
    /// asks when the good device is plugged in and the sound comes out somewhere else.
    pub refused: Vec<(String, String)>,
}

impl SignalPath {
    /// The short form for the status bar: depth, rate, and the rung — the three things
    /// worth a glance. The long form is for the panel, where there is room to explain.
    pub fn short(&self) -> String {
        if self.decoded_rate == 0 {
            return String::new();
        }
        match self.rung {
            Some(rung) => format!("{}/{} {}", self.decoded_bits, fmt_khz(self.decoded_rate), rung.label()),
            None => format!("{}/{}", self.decoded_bits, fmt_khz(self.decoded_rate)),
        }
    }

    /// True when the path changed no sample value. The status bar colours by this:
    /// bit-exact reads as accent, anything else as ordinary text, so "am I getting
    /// what I pay for" is answered without reading a word.
    pub fn is_bit_exact(&self) -> bool {
        self.rung.is_some_and(|r| r.is_bit_exact())
    }

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
    /// True when playback ended because it was told to, not because the track did.
    /// The difference decides whether the queue advances.
    pub stopped: bool,
    /// ALSA underruns. Not fatal — recovered from — but they are audible, so a run that
    /// had them is not a run that worked.
    pub underruns: u32,
}

/// Fetches, decodes and plays one track, blocking until it ends.
///
/// Phase 0 shape: one call, one track, no transport. The panel will drive a longer-lived
/// version of this loop, which is why the decode and the output are already separated
/// from the fetching.
/// What the thing driving playback wants done next, asked once per decoded packet.
///
/// The audio loop never decides any of this: it knows about decoding and about ALSA,
/// and nothing about queues, keys or panels. Everything that IS a decision lives on the
/// other side of this callback — which is also why moving the queue into a separate
/// process later changes nothing here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Hold the device but stop feeding it. The loop pauses the PCM rather than
    /// blocking with a full buffer, which would underrun the moment it resumed.
    Pause,
    /// Abandon this track and return normally — the next one is wanted.
    Skip,
    /// Stop playing entirely.
    Stop,
}

/// Told to the conductor once per packet: how far in, and (once known) how it is coming
/// out. Passing the signal path back this way means the panel learns the rung at the
/// moment the device opens, not when the track ends.
pub struct Progress<'a> {
    pub frames: u64,
    pub rate: u32,
    pub signal: Option<&'a SignalPath>,
    /// How loud this packet was, 0..=1. Measured from the samples on their way to the
    /// device, so what is drawn is what is being heard and not a second guess at it.
    pub level: f32,
}

/// How many levels the wave keeps. Wide enough to fill a panel, small enough that
/// cloning the snapshot stays free.
pub const WAVE_LEN: usize = 96;

/// Loudness of one decoded buffer, 0..=1.
///
/// RMS rather than peak: peak jumps on a single sample and makes a bar chart that
/// twitches, while RMS is what "loud" means to an ear. Scaled logarithmically over 60
/// dB, because linear amplitude spends nine tenths of its range on the loudest tenth of
/// what music does and leaves quiet passages flat against the floor.
pub fn level_of(buf: &symphonia::core::audio::GenericAudioBufferRef<'_>) -> f32 {
    let frames = buf.frames();
    let channels = buf.spec().channels().count().max(1);
    if frames == 0 {
        return 0.0;
    }
    // The destination must hold frames * channels. Sizing it by frames alone panics
    // inside symphonia — "destination slice does not match number of samples" — and it
    // panics on the FIRST packet, which killed the player thread while the state went
    // on claiming to be playing. Measured once, remembered here.
    let mut samples = vec![0f32; frames * channels];
    buf.copy_to_slice_interleaved(&mut samples[..]);
    // One channel is enough for a level meter, so only every Nth sample is squared.
    let sum: f32 = samples.iter().step_by(channels).map(|s| s * s).sum();
    let rms = (sum / frames as f32).sqrt();
    db_level(rms)
}

/// The dB curve, split out because it is the part with a decision in it and the part a
/// test can reach without a decoded buffer.
fn db_level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
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
    conductor: &mut dyn FnMut(Progress<'_>) -> Flow,
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

    // The depth the STREAM was encoded at, which is not the same as the width of the
    // buffer the decoder hands back: symphonia decodes 16-bit FLAC into an i32 buffer,
    // and believing the buffer would report every lossless track as 32-bit — turning
    // the one number this whole feature exists to be honest about into a lie.
    let declared_bits = params.bits_per_sample;

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
                bits: source_bits(declared_bits, sample_bits(&decoded)),
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

        // Asked AFTER the write, so the answer is about audio that has already been
        // handed over: a pause here stops the next packet, not the one being heard.
        let level = level_of(&decoded);
        let mut flow = conductor(Progress {
            frames: played.frames,
            rate: played.signal.decoded_rate,
            signal: Some(&played.signal),
            level,
        });
        while flow == Flow::Pause {
            if let Some(sink) = sink.as_mut() {
                sink.set_paused(true);
            }
            std::thread::sleep(PAUSE_POLL);
            // A paused player is silent, and a wave that keeps its last shape while
            // nothing plays looks frozen rather than paused.
            flow = conductor(Progress {
                frames: played.frames,
                rate: played.signal.decoded_rate,
                signal: Some(&played.signal),
                level: 0.0,
            });
        }
        if let Some(sink) = sink.as_mut() {
            sink.set_paused(false);
        }
        match flow {
            Flow::Continue | Flow::Pause => {}
            // Skip drains nothing: the point of skipping is not to hear the rest.
            Flow::Skip => return Ok(played),
            Flow::Stop => {
                played.stopped = true;
                return Ok(played);
            }
        }
    }

    if let Some(sink) = sink.as_mut() {
        sink.drain();
    }
    if played.frames == 0 {
        return Err("stream decoded to no audio at all".into());
    }
    Ok(played)
}

/// How often a paused track asks whether it may go on. Short enough that pressing play
/// feels immediate, long enough that a paused player costs nothing.
const PAUSE_POLL: std::time::Duration = std::time::Duration::from_millis(40);

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

/// The source's real bit depth.
///
/// `declared` comes from the codec parameters and is the truth when it is there. The
/// buffer width is only a fallback, and a poor one: it says how wide the decoder's
/// container is, not how many bits carry audio. Widths outside what audio actually uses
/// are ignored rather than trusted.
fn source_bits(declared: Option<u32>, buffer_bits: u32) -> u32 {
    match declared {
        Some(bits) if (8..=32).contains(&bits) => bits,
        _ => buffer_bits,
    }
}

/// The width of the buffer the decoder handed back. See [`source_bits`] for why this is
/// the fallback and not the answer.
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
    use symphonia::core::audio::sample::i24;

    pub struct Sink {
        pcm: PCM,
        format: Format,
        channels: u32,
        /// Whether this device implements `snd_pcm_pause`. Asked once, at open, because
        /// the answer decides which of two quite different pauses is possible.
        can_pause: bool,
        paused: bool,
        pub signal: SignalPath,
    }

    impl Sink {
        /// Walks the chain until something opens, and records which rung that was.
        pub fn open(cfg: &TidalCfg, want: &Want, quality: &str) -> Result<Sink, String> {
            let devices = hw_devices();
            let attempts = plan(&cfg.output, cfg.bit_perfect, &devices);
            let mut refused: Vec<(String, String)> = Vec::new();
            for attempt in &attempts {
                match try_open(attempt, want) {
                    Ok(mut sink) => {
                        sink.signal.quality = quality.to_string();
                        sink.signal.refused = refused;
                        return Ok(sink);
                    }
                    // A busy device is the commonest failure here — PipeWire holding the
                    // card — and it is not an error, it is the next rung. Kept rather
                    // than overwritten: the refusals ARE the explanation for the rung
                    // that eventually worked.
                    Err(e) => refused.push((attempt.device.clone(), e)),
                }
            }
            let why = refused
                .iter()
                .map(|(d, e)| format!("{d}: {e}"))
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!("no usable audio output ({why})"))
        }

        /// Writes one decoded buffer. Returns the number of underruns recovered from,
        /// because a run that stuttered is not a run that worked.
        pub fn write(
            &mut self,
            decoded: &symphonia::core::audio::GenericAudioBufferRef<'_>,
        ) -> Result<u32, String> {
            let frames = decoded.frames();
            let samples = frames * self.channels as usize;
            match self.format {
                Format::S16LE => {
                    let mut buf = vec![0i16; samples];
                    decoded.copy_to_slice_interleaved(&mut buf[..]);
                    self.write_i16(&buf)
                }
                // PACKED 24: three bytes per sample, and there is no typed ALSA IO for
                // it — `io_i32` on this device returns "operation not supported", which
                // is what sent the first hi-res track to the laptop speakers instead of
                // the DAC. The bytes are laid out by hand and written as bytes.
                Format::S243LE => {
                    let mut buf = vec![i24(0); samples];
                    decoded.copy_to_slice_interleaved(&mut buf[..]);
                    let mut bytes = Vec::with_capacity(samples * 3);
                    for sample in &buf {
                        let v = sample.0 as u32;
                        bytes.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
                    }
                    self.write_packed24(&bytes)
                }
                // S24LE is 24 bits inside a 32-bit word, and S32LE is a full word: both
                // are four bytes per sample and take the i32 path.
                _ => {
                    let mut buf = vec![0i32; samples];
                    decoded.copy_to_slice_interleaved(&mut buf[..]);
                    self.write_i32(&buf)
                }
            }
        }

        fn write_i16(&mut self, buf: &[i16]) -> Result<u32, String> {
            let io = self.pcm.io_i16().map_err(|e| e.to_string())?;
            write_all(&self.pcm, buf, self.channels as usize, |b| io.writei(b))
        }

        fn write_i32(&mut self, buf: &[i32]) -> Result<u32, String> {
            let io = self.pcm.io_i32().map_err(|e| e.to_string())?;
            write_all(&self.pcm, buf, self.channels as usize, |b| io.writei(b))
        }

        fn write_packed24(&mut self, bytes: &[u8]) -> Result<u32, String> {
            let io = self.pcm.io_bytes();
            // Three bytes per sample times the channels: what ALSA counts as one frame,
            // and what a short write has to be advanced by.
            write_all(&self.pcm, bytes, self.channels as usize * 3, |b| io.writei(b))
        }

        /// Lets the device finish what it has been given rather than cutting the last
        /// fraction of a second off every track.
        pub fn drain(&mut self) {
            let _ = self.pcm.drain();
        }

        /// Stops and restarts the stream under a pause.
        ///
        /// `snd_pcm_pause` is the clean way and keeps the buffer, but plenty of
        /// hardware does not implement it. The fallback drops what is buffered and
        /// prepares again, which costs the fraction of a second already in the buffer —
        /// audible as a tiny clip, and better than a pause that does not pause.
        pub fn set_paused(&mut self, paused: bool) {
            if self.paused == paused {
                return;
            }
            self.paused = paused;
            if self.can_pause {
                let _ = self.pcm.pause(paused);
            } else if paused {
                let _ = self.pcm.drop();
            } else {
                let _ = self.pcm.prepare();
            }
        }
    }

    /// Writes until the whole buffer is gone, recovering from underruns.
    ///
    /// A short write is normal — the device took what fitted — so the loop advances by
    /// what was actually accepted. An underrun is recovered rather than fatal, but it is
    /// counted: silently swallowing them turns "the audio stutters" into a bug with no
    /// evidence.
    /// `per_frame` is how many ELEMENTS of `buf` make up one frame — two `i32`s for
    /// stereo, but six bytes when the samples are packed 24-bit. Getting it wrong does
    /// not fail, it advances by the wrong amount and plays noise.
    fn write_all<S: Copy>(
        pcm: &PCM,
        buf: &[S],
        per_frame: usize,
        mut writei: impl FnMut(&[S]) -> Result<usize, alsa::Error>,
    ) -> Result<u32, String> {
        let mut offset = 0usize;
        let mut underruns = 0u32;
        while offset < buf.len() {
            match writei(&buf[offset..]) {
                Ok(0) => break,
                Ok(frames) => offset += frames * per_frame,
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
        let (format, rate, can_pause) = {
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
            let can_pause = hwp.can_pause();
            pcm.hw_params(&hwp).map_err(|e| e.to_string())?;
            (format, rate, can_pause)
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
            can_pause,
            paused: false,
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
                refused: Vec::new(),
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
        pub fn set_paused(&mut self, _: bool) {}
    }
}

// ---- the jukebox ----------------------------------------------------------

/// What the player can be told to do.
///
/// Deliberately small and deliberately serialisable in shape: when the player moves
/// into its own process, this enum becomes the wire protocol and the panel does not
/// change. Nothing here carries a callback, a handle or a lifetime for that reason.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Cmd {
    /// Replace the queue and start at `at`.
    Play { tracks: Vec<tidal::Track>, at: usize },
    Enqueue(tidal::Track),
    /// Play if paused, pause if playing.
    Toggle,
    Next,
    /// Back to the start of this track, or to the previous one when barely started —
    /// the behaviour every music player has, and the reason is that a mis-pressed
    /// "previous" should be cheap to undo.
    Prev,
    Stop,
    /// The config changed: the output device or bit-perfect setting may be different.
    /// Takes effect on the next track, since the current one is already on a device.
    Reconfigure(Box<TidalCfg>),
    Quit,
}

/// Everything a panel (or a status bar, or another window) needs to draw the player.
/// One struct, cloned under a lock, so a reader can never see half an update.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub queue: Vec<tidal::Track>,
    pub index: usize,
    pub playing: bool,
    pub paused: bool,
    pub position_secs: f64,
    pub signal: SignalPath,
    /// The last thing that went wrong, kept until something goes right. A track that
    /// will not play must say so somewhere the person can see it.
    pub error: Option<String>,
    /// Bumped on every change, so a drawing pass can tell "nothing happened" from
    /// "happened to look the same".
    pub generation: u64,
    /// The last [`WAVE_LEN`] loudness readings, oldest first — a scrolling picture of
    /// what has just been heard. Drawn by the panel; empty when nothing is playing.
    pub wave: Vec<f32>,
}

impl Snapshot {
    pub fn now_playing(&self) -> Option<&tidal::Track> {
        self.queue.get(self.index)
    }

    /// One line for the status bar: what is playing and what it is coming out as.
    ///
    /// `width` is the space actually available, and the parts drop in order of what
    /// can be spared — the quality goes before the artist, and the artist before the
    /// title. A bar that overflows is worse than one that says less.
    pub fn status_line(&self, width: usize) -> Option<String> {
        let track = self.now_playing()?;
        if !self.playing {
            return None;
        }
        let mark = if self.paused { "\u{2016}" } else { "\u{25b8}" };
        let quality = self.signal.short();
        let full = format!("{mark} {} \u{2014} {} \u{b7} {quality}", track.artist, track.title);
        if full.chars().count() <= width {
            return Some(full);
        }
        let no_artist = format!("{mark} {} \u{b7} {quality}", track.title);
        if no_artist.chars().count() <= width {
            return Some(no_artist);
        }
        let bare = format!("{mark} {}", track.title);
        Some(clip(&bare, width))
    }
}

/// Cuts to fit, keeping the front and marking the cut.
fn clip(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let keep = width.saturating_sub(1);
    format!("{}\u{2026}", s.chars().take(keep).collect::<String>())
}

/// The handle the rest of the program holds. Cheap to clone, safe to keep on `App`.
///
/// It owns no audio state: everything real is behind the channel and the mutex, which
/// is what lets the same handle later point at a socket instead of a thread.
pub struct Jukebox {
    tx: std::sync::mpsc::Sender<Cmd>,
    state: std::sync::Arc<std::sync::Mutex<Snapshot>>,
}

impl Jukebox {
    /// Starts the player thread. `wake` is called whenever the snapshot changes, so the
    /// UI thread can be nudged to redraw — the same pattern `media.rs` uses, and the
    /// only thing here that knows a UI exists at all.
    pub fn start(
        cfg: TidalCfg,
        creds: tidal::Creds,
        wake: Box<dyn Fn() + Send>,
    ) -> Jukebox {
        let (tx, rx) = std::sync::mpsc::channel();
        let state = std::sync::Arc::new(std::sync::Mutex::new(Snapshot::default()));
        let shared = state.clone();
        std::thread::Builder::new()
            .name("runnir-player".into())
            .spawn(move || run(rx, shared, wake, cfg, creds))
            .expect("spawn player thread");
        // Tell the desktop there is a player here. Done at start rather than at first
        // track: the media keys and the desktop's widget should find runnir the moment
        // it can play, not only once it already is.
        crate::mpris::publish(tx.clone(), state.clone());
        Jukebox { tx, state }
    }

    pub fn send(&self, cmd: Cmd) {
        // A dead player thread is not worth crashing a terminal over: the panel will
        // show the last snapshot and nothing will move, which is visible enough.
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// The state itself, for the daemon's writer thread — it watches for changes rather
    /// than polling a copy, which would mean cloning a queue eighty times a second.
    pub fn shared(&self) -> std::sync::Arc<std::sync::Mutex<Snapshot>> {
        self.state.clone()
    }

    /// Whether closing the window would interrupt music.
    ///
    /// The window asks its [`crate::daemon::Remote`] rather than this, since the player
    /// is in another process; kept here because the daemon runs a Jukebox directly and
    /// the two answers must not be allowed to drift apart.
    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        self.state.lock().map(|s| s.playing && !s.paused).unwrap_or(false)
    }
}

/// The player thread.
fn run(
    rx: std::sync::mpsc::Receiver<Cmd>,
    state: std::sync::Arc<std::sync::Mutex<Snapshot>>,
    wake: Box<dyn Fn() + Send>,
    mut cfg: TidalCfg,
    creds: tidal::Creds,
) {
    let publish = |f: &dyn Fn(&mut Snapshot)| {
        if let Ok(mut s) = state.lock() {
            f(&mut s);
            s.generation += 1;
        }
        wake();
    };

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Quit => return,
            Cmd::Reconfigure(next) => cfg = *next,
            Cmd::Play { tracks, at } => {
                if tracks.is_empty() {
                    continue;
                }
                publish(&|s| {
                    s.queue = tracks.clone();
                    s.index = at.min(tracks.len() - 1);
                    s.error = None;
                });
                if play_queue(&rx, &state, &wake, &mut cfg, &creds) {
                    return; // quit arrived mid-track
                }
            }
            Cmd::Enqueue(track) => publish(&|s| s.queue.push(track.clone())),
            // Anything else while stopped is meaningless — there is nothing to pause or
            // skip — except that a Toggle with a queue means "start it again".
            Cmd::Toggle => {
                let has_queue = state.lock().map(|s| !s.queue.is_empty()).unwrap_or(false);
                if has_queue && play_queue(&rx, &state, &wake, &mut cfg, &creds) {
                    return;
                }
            }
            Cmd::Next | Cmd::Prev | Cmd::Stop => {}
        }
    }
}

/// Plays from the current index to the end of the queue. Returns true if it was told to
/// quit outright.
fn play_queue(
    rx: &std::sync::mpsc::Receiver<Cmd>,
    state: &std::sync::Arc<std::sync::Mutex<Snapshot>>,
    wake: &dyn Fn(),
    cfg: &mut TidalCfg,
    creds: &tidal::Creds,
) -> bool {
    loop {
        let (track, index) = {
            let Ok(s) = state.lock() else { return false };
            match s.queue.get(s.index) {
                Some(t) => (t.clone(), s.index),
                None => return false, // ran off the end of the queue
            }
        };

        let outcome = play_one(&track, rx, state, wake, cfg, creds);
        match outcome {
            Outcome::Quit => return true,
            Outcome::Stopped => {
                set(state, wake, |s| {
                    s.playing = false;
                    s.paused = false;
                });
                return false;
            }
            Outcome::Failed(why) => {
                // One bad track must not end the evening: it is reported and the queue
                // moves on. A queue where every track fails still stops, at the end.
                set(state, wake, |s| s.error = Some(format!("{}: {why}", track.title)));
                if !advance(state, wake, 1) {
                    return false;
                }
            }
            Outcome::Ended => {
                if !advance(state, wake, 1) {
                    set(state, wake, |s| {
                        s.playing = false;
                        s.paused = false;
                    });
                    return false;
                }
            }
            // Restart and "the queue was replaced under us" both mean: play whatever
            // the index now points at, without moving it.
            Outcome::Restart => {
                let _ = index;
            }
            Outcome::Previous => {
                // At the first track, "previous" restarts it rather than stopping: the
                // queue has nowhere further back to go.
                advance(state, wake, -1);
            }
        }
    }
}

enum Outcome {
    Ended,
    Stopped,
    /// Play the same track again from the top.
    Restart,
    Previous,
    Failed(String),
    Quit,
}

/// Resolves and plays one track, obeying commands as they arrive.
fn play_one(
    track: &tidal::Track,
    rx: &std::sync::mpsc::Receiver<Cmd>,
    state: &std::sync::Arc<std::sync::Mutex<Snapshot>>,
    wake: &dyn Fn(),
    cfg: &mut TidalCfg,
    creds: &tidal::Creds,
) -> Outcome {
    let Some(session) = tidal::Session::load() else {
        return Outcome::Failed("not signed in".into());
    };
    let session = match tidal::ensure_fresh(creds, &session) {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(e),
    };
    let info = match tidal::stream_info(&session, track.id, cfg.quality.as_api()) {
        Ok(i) => i,
        Err(e) => return Outcome::Failed(e),
    };
    let parts = match parts_of(&info) {
        Ok(p) => p,
        Err(e) => return Outcome::Failed(e),
    };

    set(state, wake, |s| {
        s.playing = true;
        s.paused = false;
        s.position_secs = 0.0;
        s.error = None;
        // The wave belongs to the track being heard, not to the player: carrying the
        // last one into the next song draws a shape that never happened.
        s.wave.clear();
    });

    // What the conductor decided, read after the loop returns. A closure cannot return
    // this through `Flow`, which only says what the audio loop should do next.
    let mut verdict = Outcome::Ended;
    let mut paused = false;
    let mut published_signal = false;
    let mut last_tick = u64::MAX;
    let mut wave_clock = 0u32;
    // The track plays under the config it started with. A `Reconfigure` arriving now is
    // held and applied to the NEXT track — the device for this one is already open, and
    // swapping it underneath would be a gap in the middle of a song.
    let mut pending_cfg: Option<TidalCfg> = None;
    let playing_cfg = cfg.clone();

    // Caught, because a panic here kills the player thread and leaves the state saying
    // "playing" forever: silent, with nothing on screen to explain it. A crash has to
    // become a message like any other failure — the queue then moves on, which is what
    // it does for a track that will not play for any other reason.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play_parts(
        parts,
        hint_for(&info),
        &info.quality,
        &playing_cfg,
        true,
        &mut |progress| {
            // Every command waiting right now, not just one: a burst of key presses
            // must not be answered one packet at a time.
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Cmd::Toggle => {
                        paused = !paused;
                        set(state, wake, |s| s.paused = paused);
                    }
                    Cmd::Next => {
                        verdict = Outcome::Ended;
                        return Flow::Skip;
                    }
                    Cmd::Prev => {
                        // Past the grace period "previous" means the start of THIS
                        // track; within it, the one before. Every player behaves this
                        // way because a mis-pressed previous should be cheap to undo.
                        let secs = progress.frames as f64 / progress.rate.max(1) as f64;
                        verdict = if secs > PREV_RESTARTS_AFTER {
                            Outcome::Restart
                        } else {
                            Outcome::Previous
                        };
                        return Flow::Skip;
                    }
                    Cmd::Stop => {
                        verdict = Outcome::Stopped;
                        return Flow::Stop;
                    }
                    Cmd::Quit => {
                        verdict = Outcome::Quit;
                        return Flow::Stop;
                    }
                    Cmd::Play { tracks, at } => {
                        set(state, wake, |s| {
                            s.queue = tracks.clone();
                            s.index = at.min(tracks.len().saturating_sub(1));
                        });
                        verdict = Outcome::Restart; // the new index is already right
                        return Flow::Skip;
                    }
                    Cmd::Enqueue(t) => set(state, wake, |s| s.queue.push(t.clone())),
                    Cmd::Reconfigure(next) => pending_cfg = Some(*next),
                }
            }

            let secs = progress.frames as f64 / progress.rate.max(1) as f64;
            // A packet is 20-40 ms of audio, so this runs about forty times a second.
            // Everything is RECORDED every time; what is paced is the waking, because
            // a wake is a full window repaint. The clock only needs a wake when the
            // second changes, but the wave needs to move to look alive — so it sets
            // its own, slower, rhythm.
            let tick = secs as u64;
            let mut worth_drawing = tick != last_tick;
            last_tick = tick;
            if wave_clock >= WAVE_EVERY {
                wave_clock = 0;
                worth_drawing = true;
            }
            wave_clock += 1;
            if let Ok(mut s) = state.lock() {
                s.position_secs = secs;
                if s.wave.len() >= WAVE_LEN {
                    s.wave.remove(0);
                }
                s.wave.push(progress.level);
                if !published_signal {
                    if let Some(signal) = progress.signal {
                        s.signal = signal.clone();
                        published_signal = true;
                        worth_drawing = true;
                    }
                }
                if worth_drawing {
                    s.generation += 1;
                }
            }
            if worth_drawing {
                wake();
            }

            if paused { Flow::Pause } else { Flow::Continue }
        },
    )
    ));

    if let Some(next) = pending_cfg {
        *cfg = next;
    }
    match result {
        Err(panic) => {
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "player panicked".into());
            Outcome::Failed(format!("internal error: {what}"))
        }
        Ok(Err(e)) => Outcome::Failed(e),
        Ok(Ok(_)) => verdict,
    }
}

/// How many packets pass between wakes for the wave's sake. At 20-40 ms a packet this
/// is roughly ten times a second: enough that the wave moves smoothly, few enough that
/// a playing terminal is not repainting itself forty times a second for a picture.
const WAVE_EVERY: u32 = 4;

/// Seconds after which "previous" restarts the current track instead of going back.
const PREV_RESTARTS_AFTER: f64 = 3.0;

/// Moves the queue cursor. False when there is nowhere to move to.
fn advance(
    state: &std::sync::Arc<std::sync::Mutex<Snapshot>>,
    wake: &dyn Fn(),
    by: i64,
) -> bool {
    let mut moved = false;
    if let Ok(mut s) = state.lock() {
        let next = s.index as i64 + by;
        if next >= 0 && (next as usize) < s.queue.len() {
            s.index = next as usize;
            s.generation += 1;
            moved = true;
        }
    }
    wake();
    moved
}

fn set(
    state: &std::sync::Arc<std::sync::Mutex<Snapshot>>,
    wake: &dyn Fn(),
    f: impl Fn(&mut Snapshot),
) {
    if let Ok(mut s) = state.lock() {
        f(&mut s);
        s.generation += 1;
    }
    wake();
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
    fn the_level_meter_spends_its_range_where_music_lives() {
        // Silence is the floor, and anything below -60 dB is silence for a picture.
        assert_eq!(db_level(0.0), 0.0);
        assert_eq!(db_level(0.0001), 0.0);
        // Full scale is the top.
        assert!((db_level(1.0) - 1.0).abs() < 0.001);
        // -30 dB is a normal quiet passage and must land in the MIDDLE, not squashed
        // against the floor the way a linear scale would leave it.
        let quiet = db_level(0.0316);
        assert!((0.4..0.6).contains(&quiet), "-30 dB mapped to {quiet}");
    }

    #[test]
    fn the_source_depth_wins_over_the_decoders_buffer_width() {
        // 16-bit FLAC decoded into an i32 buffer is still 16-bit audio. Reporting 32
        // would claim a depth the recording never had, and would send a 16-bit stream
        // to the DAC as if it were 32.
        assert_eq!(source_bits(Some(16), 32), 16);
        assert_eq!(source_bits(Some(24), 32), 24);
        // Nothing declared: the buffer is all there is.
        assert_eq!(source_bits(None, 32), 32);
        // A nonsense declaration is not trusted over something real.
        assert_eq!(source_bits(Some(0), 24), 24);
        assert_eq!(source_bits(Some(64), 24), 24);
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
