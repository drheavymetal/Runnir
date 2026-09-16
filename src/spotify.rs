//! Spotify: sign-in, session, and the seam where librespot's samples meet runnir's
//! output chain.
//!
//! The shape of this file is decided by one fact that no amount of design can route
//! around: **no endpoint in Spotify's Web API returns audio**. The Web API is metadata
//! and remote control; the only first-party way to hear a track outside their apps is
//! the Web Playback SDK, which is a browser with Widevine. So playing something means
//! being a client, and `librespot` is that client reimplemented.
//!
//! What follows from that, and what the badge has to stop promising: every Connect
//! endpoint — every third-party streamer, librespot included — is served Ogg Vorbis
//! 320. FLAC goes only to Spotify's own apps. There is no DRM barrier in the way and
//! librespot's decoder is ready for the day it changes; the backend simply does not
//! hand FLAC to outside devices. So bit-perfect is unreachable here and *exclusive,
//! not resampled* is the honest ceiling.

use crate::config::Spotify as Cfg;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Scopes. `streaming` is the one that makes playback possible at all; the rest are the
/// catalogue the panel will want in phase 1, asked for now so that a session saved today
/// does not have to be thrown away when the panel arrives.
pub const SCOPES: &[&str] = &[
    "streaming",
    "user-read-private",
    "user-read-email",
    "user-library-read",
    "user-top-read",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
];

/// Which of the two sign-ins is being asked for.
///
/// There are two because of a rate limit, not a permission: playback needs the desktop
/// client id (the one known to open a session), and the catalogue needs an id that is
/// not shared with every librespot install on earth. When no catalogue id is configured
/// the two collapse into one, which is exactly what happens if the user's own id turns
/// out to work for playback as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Audio,
    Api,
}

impl Which {
    fn client_id(self, cfg: &Cfg) -> String {
        match self {
            Which::Audio => cfg.client_id.clone(),
            Which::Api if cfg.api_client_id.is_empty() => cfg.client_id.clone(),
            Which::Api => cfg.api_client_id.clone(),
        }
    }

    fn port(self, cfg: &Cfg) -> u16 {
        match self {
            Which::Audio => cfg.callback_port,
            Which::Api if cfg.api_client_id.is_empty() => cfg.callback_port,
            Which::Api => cfg.api_callback_port,
        }
    }

    /// Which file holds it. Two ids mean two sessions; one id means one, and sharing the
    /// file is what makes "put your own id in `client_id`" a complete answer rather than
    /// a second sign-in for the same account.
    fn file(self, cfg: &Cfg) -> &'static str {
        match self {
            Which::Audio => "spotify-session.json",
            Which::Api if cfg.api_client_id.is_empty() => "spotify-session.json",
            Which::Api => "spotify-api-session.json",
        }
    }

    fn what(self) -> &'static str {
        match self {
            Which::Audio => "playback",
            Which::Api => "the catalogue",
        }
    }
}

/// Refreshed this long before it actually expires. A token that dies mid-request is a
/// failure a user sees; a minute of unused life is not.
const REFRESH_MARGIN: u64 = 60;

pub const NOT_SIGNED_IN: &str = "not signed in — run: runnir --spotify-login";

/// Names the sign-in that is missing, which is not always the same command: with a
/// catalogue id of its own there are two, and being told to run the wrong one is worse
/// than being told nothing.
fn not_signed_in(which: Which, cfg: &Cfg) -> String {
    match which {
        Which::Api if !cfg.api_client_id.is_empty() => {
            "not signed in for the catalogue — run: runnir --spotify-login --api".to_string()
        }
        _ => NOT_SIGNED_IN.to_string(),
    }
}

pub const SESSION_REVOKED: &str =
    "the Spotify sign-in was revoked — run: runnir --spotify-login";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A signed-in session, as it lives on disk.
///
/// `expires_at` is unix seconds, not the `Instant` librespot hands back: an `Instant` is
/// measured from an arbitrary point in this process's life and means nothing at all once
/// it has been written to a file and read by a different process an hour later.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

impl Session {
    fn expired(&self) -> bool {
        now() + REFRESH_MARGIN >= self.expires_at
    }

    /// `dirs::data_dir()/runnir/spotify-session.json`, 0600. Beside TIDAL's, for the
    /// same reason TIDAL's is there: `session.rs` already writes to `data_dir`, and a
    /// third opinion about which directory is the right one would make known debt worse.
    pub fn path(which: Which, cfg: &Cfg) -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("runnir").join(which.file(cfg)))
    }

    pub fn load(which: Which, cfg: &Cfg) -> Option<Session> {
        let text = std::fs::read_to_string(Self::path(which, cfg)?).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Owner-only, with the mode set before the tokens are written rather than after.
    pub fn save(&self, which: Which, cfg: &Cfg) -> Result<(), String> {
        let path = Self::path(which, cfg).ok_or("no data directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::tidal::write_private(&path, json.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    #[allow(dead_code)]
    pub fn forget(which: Which, cfg: &Cfg) -> Result<(), String> {
        let Some(path) = Self::path(which, cfg) else { return Ok(()) };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// The loopback redirect. Spotify accepts one — which is the whole difference from
/// TIDAL, where no first-party client would and `error 11102` refused both the app
/// redirect and the loopback one. `127.0.0.1` and not `localhost`: Spotify rejects the
/// hostname form for loopback redirects.
pub fn redirect_uri(which: Which, cfg: &Cfg) -> String {
    format!("http://127.0.0.1:{}/login", which.port(cfg))
}

fn client(which: Which, cfg: &Cfg) -> Result<librespot_oauth::OAuthClient, String> {
    let id = which.client_id(cfg);
    if id.is_empty() {
        return Err(format!("no client id configured for {}", which.what()));
    }
    librespot_oauth::OAuthClientBuilder::new(&id, &redirect_uri(which, cfg), SCOPES.to_vec())
        .open_in_browser()
        .build()
        .map_err(|e| format!("could not start the Spotify sign-in: {e}"))
}

/// Signs in: opens the browser, listens on the loopback port, exchanges the code.
///
/// Blocking on purpose. `librespot-oauth` offers both forms and the synchronous one
/// needs no runtime, which keeps the one tokio runtime this program builds inside the
/// player daemon where it belongs.
pub fn login(which: Which, cfg: &Cfg) -> Result<Session, String> {
    let token = client(which, cfg)?.get_access_token().map_err(|e| {
        let e = e.to_string();
        // The one failure worth translating: a redirect the client id does not know
        // about is refused as INVALID_CLIENT, and the message never mentions the port.
        if e.contains("INVALID_CLIENT") || e.contains("invalid_client") {
            format!(
                "Spotify refused the sign-in: the client id does not have {} registered \
                 as a redirect URI. Add it in the app's settings at developer.spotify.com, \
                 or use the desktop id, which has 127.0.0.1:8898/login.",
                redirect_uri(which, cfg)
            )
        } else {
            format!("Spotify sign-in failed: {e}")
        }
    })?;
    let session = session_from(token);
    session.save(which, cfg)?;
    Ok(session)
}

/// librespot reports expiry as an `Instant`; disk needs a wall clock. Converted by
/// measuring how far away it is now rather than by trying to translate the instant
/// itself, which cannot be done.
fn session_from(token: librespot_oauth::OAuthToken) -> Session {
    let lasts = token.expires_at.saturating_duration_since(std::time::Instant::now()).as_secs();
    Session {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now() + lasts,
    }
}

/// Serialises refreshes. Opening the panel fires several catalogue requests at once and
/// every one of them would otherwise find the same stale session and refresh it
/// separately.
static REFRESH: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The session to make the next call with: loaded, refreshed if it has aged out, saved
/// back. **Every** entry point goes through here.
///
/// This is not a precaution, it is a bug that already happened once in this program: the
/// TIDAL panel loaded its session raw while playback renewed it, so search reported an
/// expired session on the same machine, in the same second, while the music played. A
/// path that loads the session raw is correct for exactly one hour after each sign-in,
/// which is long enough to look right everywhere it is written.
pub fn current(which: Which) -> Result<Session, String> {
    let cfg = crate::config::Config::load().spotify;
    let session = Session::load(which, &cfg).ok_or(not_signed_in(which, &cfg))?;
    if !session.expired() {
        return Ok(session);
    }
    // Poisoning is not a reason to stop refreshing: the guarded data is `()`.
    let _guard = REFRESH.lock().unwrap_or_else(|e| e.into_inner());
    // Re-read under the lock: waiting for someone else's refresh and then refreshing
    // again from the copy read before the wait is the race this exists to lose.
    let session = Session::load(which, &cfg).ok_or(not_signed_in(which, &cfg))?;
    if !session.expired() {
        return Ok(session);
    }
    let token = client(which, &cfg)?
        .refresh_token(&session.refresh_token)
        .map_err(|e| {
            // "Expired" and "revoked" are different sentences, and only one of them is
            // something a person has to act on. A timeout says nothing about the session
            // and answering it with "sign in again" sends someone to redo a login that
            // was never the problem.
            let e = e.to_string();
            if e.contains("invalid_grant") { SESSION_REVOKED.to_string() } else {
                format!("could not refresh the Spotify session: {e}")
            }
        })?;
    let next = session_from(token);
    // A failure to write is reported, not fatal: the token in hand is good for the next
    // hour, and refusing to use it turns a permissions problem into "Spotify is broken".
    if let Err(e) = next.save(which, &cfg) {
        eprintln!("runnir: could not save the refreshed Spotify session: {e}");
    }
    Ok(next)
}

/// Turns whatever someone pasted into a Spotify URI.
///
/// The share button produces a URL, so that is what lands in a terminal; asking someone
/// to retype it as a URI to satisfy a command would be a small insult. A query string
/// (`?si=…`, which every share link carries) is dropped rather than fed to the parser.
pub fn uri_from(input: &str) -> String {
    let input = input.trim();
    let rest = match input.strip_prefix("https://open.spotify.com/") {
        Some(r) => r,
        None => return input.to_string(),
    };
    // Localised share links carry a country segment first: /intl-es/track/<id>.
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let (kind, id) = match (parts.next(), parts.next()) {
        (Some(k), Some(i)) if k.starts_with("intl-") => match parts.next() {
            Some(third) => (i, third),
            None => return input.to_string(),
        },
        (Some(k), Some(i)) => (k, i),
        _ => return input.to_string(),
    };
    let id = id.split(['?', '#']).next().unwrap_or(id);
    if id.is_empty() { input.to_string() } else { format!("spotify:{kind}:{id}") }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// runnir's output chain, wearing librespot's `Sink` trait.
///
/// This is the whole integration. librespot fetches, decrypts and decodes; everything
/// downstream of `write` is the chain that was built for TIDAL and is not touched —
/// `plan()`, the rungs, the PipeWire reservation, the refusal list, the device held open
/// across tracks.
///
/// The device is opened on the FIRST packet, not at construction: only then is the real
/// rate and channel count known. That is the same rule the symphonia path follows, and
/// for the same reason — opening a DAC for a shape you then discover is different is how
/// you get a click before track one.
pub struct ChainSink {
    output: crate::player::Output,
    device: Option<crate::player::Sink>,
    /// The loudness of the packet that just went to the device, one value per column.
    ///
    /// Measured here rather than guessed at elsewhere for the same reason the TIDAL path
    /// measures it in its own write loop: what is drawn should be what is being heard.
    /// RMS and not peak — a peak meter jumps on one sample and trembles.
    pub levels: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    /// Filled in as soon as the device opens, so the caller can report which rung it
    /// landed on without waiting for the track to end.
    pub signal: std::sync::Arc<std::sync::Mutex<crate::player::SignalPath>>,
    rate: u32,
    channels: u32,
}

impl ChainSink {
    pub fn new(
        output: crate::player::Output,
        rate: u32,
        channels: u32,
        signal: std::sync::Arc<std::sync::Mutex<crate::player::SignalPath>>,
    ) -> ChainSink {
        ChainSink {
            output,
            device: None,
            levels: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            signal,
            rate,
            channels,
        }
    }

    fn device_for(&mut self) -> Result<&mut crate::player::Sink, String> {
        if self.device.is_none() {
            let want = crate::player::Want {
                rate: self.rate,
                channels: self.channels,
                // Ogg Vorbis decodes to floats; there is no source depth to preserve.
                // 16 is what the format is worth and what the chain will ask the device
                // for, which on a DAC that takes 44.1/16 means nothing resamples.
                bits: 16,
            };
            // `OGG 320` and not a tier asked for: this is what Spotify serves every
            // Connect endpoint, and a badge that says more than that is the fourth
            // version of a lie this program has already told three times.
            let mut sink = crate::player::Sink::open(&self.output, &want, "OGG 320")?;
            // Set here rather than inside `open`, because the chain has no way of
            // knowing what it is carrying: the same device, the same rung and the same
            // numbers describe a FLAC from TIDAL and a Vorbis from Spotify.
            sink.signal.lossy = crate::music::Source::Spotify.is_lossy();
            if let Ok(mut s) = self.signal.lock() {
                *s = sink.signal.clone();
            }
            self.device = Some(sink);
        }
        Ok(self.device.as_mut().expect("just opened"))
    }
}

/// One value per column for a packet of interleaved samples, 0..=1.
///
/// The buffer is cut into as many slices as there are columns and each is measured on
/// its own, so the columns rise and fall in place instead of marching leftwards. A shape
/// that moves under the eye cannot be read — that was the lesson the TIDAL wave learned
/// the long way, and there is no reason to learn it twice.
fn levels_of(samples: &[f64], channels: usize) -> Vec<f32> {
    const COLUMNS: usize = 24;
    let frames = samples.len() / channels.max(1);
    if frames == 0 {
        return Vec::new();
    }
    let per = frames.div_ceil(COLUMNS);
    let mut out = Vec::with_capacity(COLUMNS);
    for chunk in samples.chunks(per * channels.max(1)) {
        let sum: f64 = chunk.iter().map(|s| s * s).sum();
        let rms = (sum / chunk.len().max(1) as f64).sqrt();
        // The same 60 dB scale the TIDAL meter uses: linear amplitude spends nine
        // tenths of its range on the top tenth of what music does.
        let db = 20.0 * rms.max(1e-6).log10();
        out.push(((db + 60.0) / 60.0).clamp(0.0, 1.0) as f32);
    }
    out
}

impl librespot_playback::audio_backend::Sink for ChainSink {
    fn start(&mut self) -> librespot_playback::audio_backend::SinkResult<()> {
        // The exact inverse of `stop`: `set_paused(false)` undoes a hardware pause on a
        // device that has one and prepares the stream on a device that does not.
        // `resume()` only ever prepares, which is the wrong half for the first kind.
        if let Some(d) = self.device.as_mut() {
            d.set_paused(false);
        }
        Ok(())
    }

    /// Pauses the device. Does NOT close it, and that distinction is the whole comment.
    ///
    /// librespot calls `stop` on every pause, not only at the end of a queue
    /// (`handle_pause` → `ensure_sink_stopped(false)`). Closing the device here would
    /// hand the card back on every pause and then have to take it again on resume — and
    /// ALSA releases a card an instant AFTER the holder lets go, so the reopen lands on
    /// the `EBUSY` of its own release and waits out the patience budget. That is exactly
    /// the gap this program already diagnosed once, between tracks, in August.
    ///
    /// Holding an exclusive device through a pause is safe because the reservation
    /// answers `RequestRelease` with yes: anything else that wants the card takes it.
    /// The device is closed when this sink is dropped, which is when playback is
    /// actually over.
    fn stop(&mut self) -> librespot_playback::audio_backend::SinkResult<()> {
        if let Some(d) = self.device.as_mut() {
            d.set_paused(true);
        }
        Ok(())
    }

    fn write(
        &mut self,
        packet: librespot_playback::decoder::AudioPacket,
        converter: &mut librespot_playback::convert::Converter,
    ) -> librespot_playback::audio_backend::SinkResult<()> {
        use librespot_playback::audio_backend::SinkError;
        use librespot_playback::decoder::AudioPacket;

        let samples = match &packet {
            AudioPacket::Samples(s) => s,
            // Only produced by the passthrough decoder, which is a feature this build
            // does not enable. Refused rather than guessed at.
            AudioPacket::Raw(_) => {
                return Err(SinkError::InvalidParams("raw packets are not decoded here".into()));
            }
        };
        if samples.is_empty() {
            return Ok(());
        }
        let width = {
            let device = self.device_for().map_err(SinkError::ConnectionRefused)?;
            device.width()
        };
        if let Ok(mut l) = self.levels.lock() {
            *l = levels_of(samples, self.channels as usize);
        }
        // The conversion is librespot's, not ours: it carries the ditherer, and
        // reimplementing a float-to-integer reduction by hand is how you get quiet
        // distortion that nobody can point at.
        let device = self.device_for().map_err(SinkError::ConnectionRefused)?;
        let wrote = match width {
            crate::player::Width::S16 => device.write_i16(&converter.f64_to_s16(samples)),
            crate::player::Width::S32 => device.write_i32(&converter.f64_to_s32(samples)),
            crate::player::Width::S24In32 => device.write_i32_s24(&converter.f64_to_s24(samples)),
            crate::player::Width::S24Packed => {
                // Built from the s24 form rather than librespot's packed `i24`, whose
                // field is private. Same number, laid out by hand — which is what the
                // symphonia path does for this format too, for the same reason: there
                // is no typed ALSA IO for packed 24.
                let mut bytes = Vec::with_capacity(samples.len() * 3);
                for v in converter.f64_to_s24(samples) {
                    let v = v as u32;
                    bytes.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
                }
                device.write_packed24(&bytes)
            }
        };
        wrote.map(|_| ()).map_err(SinkError::OnWrite)
    }
}

// ---------------------------------------------------------------------------
// Phase 0: prove the seam holds, with no UI in the way
// ---------------------------------------------------------------------------

/// What a track turned out to be, for the command line to print.
pub struct Playing {
    pub title: String,
    pub artist: String,
    pub badge: String,
}

/// Plays one track, start to finish, through the chain. Blocking.
///
/// The tokio runtime is built HERE and nowhere else in this program: librespot is async
/// all the way down, and the alternative — a runtime somewhere central — would put an
/// executor under a terminal emulator that has done without one for its whole life. In
/// the daemon this same call is what the player thread makes.
pub fn play_uri(
    cfg: &Cfg,
    uri: &str,
    limit: Option<std::time::Duration>,
    announce: &mut dyn FnMut(&Playing),
) -> Result<(), String> {
    use librespot_core::{Session as LsSession, SessionConfig, SpotifyUri, authentication::Credentials};
    use librespot_playback::config::{Bitrate, PlayerConfig};
    use librespot_playback::mixer::NoOpVolume;
    use librespot_playback::player::{Player, PlayerEvent};

    let uri = SpotifyUri::from_uri(uri).map_err(|e| format!("not a Spotify URI: {e}"))?;
    let session = current(Which::Audio)?;

    // Single threaded: this runtime drives network IO for one player, and a thread pool
    // to do it would be a pool sitting idle inside every window's child process.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the async runtime: {e}"))?;

    let signal = std::sync::Arc::new(std::sync::Mutex::new(crate::player::SignalPath::default()));
    let output: crate::player::Output = crate::player::Output {
        device: cfg.output.clone(),
        bit_perfect: cfg.bit_perfect,
        release_device: cfg.release_device,
    };

    rt.block_on(async move {
        let ls_cfg = SessionConfig { client_id: cfg.client_id.clone(), ..SessionConfig::default() };
        let ls = LsSession::new(ls_cfg, None);
        ls.connect(Credentials::with_access_token(session.access_token), false)
            .await
            .map_err(|e| format!("Spotify refused the session: {e}"))?;

        let title = track_name(&ls, &uri).await;

        let player_cfg = PlayerConfig { bitrate: Bitrate::Bitrate320, ..PlayerConfig::default() };
        let sink_signal = signal.clone();
        let player = Player::new(player_cfg, ls, Box::new(NoOpVolume), move || {
            Box::new(ChainSink::new(
                output,
                librespot_playback::SAMPLE_RATE,
                librespot_playback::NUM_CHANNELS as u32,
                sink_signal,
            ))
        });

        let mut events = player.get_player_event_channel();
        player.load(uri, true, 0);

        // A deadline exists so that a diagnostic run can end the way a queue ends,
        // through the drops, instead of being killed from outside. The difference is not
        // cosmetic: a signalled process runs no destructors, so the PCM handle and the
        // reservation are released by the kernel at the same instant — and PipeWire then
        // tries to take a card back that ALSA has not finished letting go of.
        let deadline = limit.map(|d| std::time::Instant::now() + d);
        let mut announced = false;
        loop {
            // Never waits longer than a tick without looking up, because a signal has
            // to be answered by closing the device and this loop owns it. The deadline,
            // when there is one, only shortens the tick further.
            const TICK: std::time::Duration = std::time::Duration::from_millis(100);
            let wait = match deadline {
                Some(end) => {
                    let left = end.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    left.min(TICK)
                }
                None => TICK,
            };
            let event = match tokio::time::timeout(wait, events.recv()).await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                // Nothing happened in this tick. Check why we might want to stop, and
                // otherwise go round again.
                Err(_) => {
                    if crate::reserve::shutting_down() {
                        break;
                    }
                    continue;
                }
            };
            if crate::reserve::shutting_down() {
                break;
            }
            match event {
                // Announced when the device opens rather than when the track ends:
                // "which rung did it land on" is the question, and waiting four minutes
                // for the answer makes the command useless for checking it.
                // `Playing` is emitted when playback STARTS, which is before the first
                // packet has reached the sink — and the device is opened by that first
                // packet, because only then is the real shape known. Reporting here
                // reads the signal path before anything has filled it in: `PCM 0/0 kHz`
                // and an empty device. So wait for the device, briefly.
                PlayerEvent::Playing { .. } if !announced => {
                    announced = true;
                    let mut badge = String::new();
                    for _ in 0..100 {
                        if let Ok(s) = signal.lock() {
                            if s.decoded_rate != 0 {
                                badge = s.badge();
                                break;
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    // Five seconds is long enough for a device to open and short enough
                    // that a chain which never opens one says so instead of hanging.
                    if badge.is_empty() {
                        badge = "no device opened — nothing was written to a sink".to_string();
                    }
                    announce(&Playing {
                        title: title.0.clone(),
                        artist: title.1.clone(),
                        badge,
                    });
                }
                PlayerEvent::EndOfTrack { .. } | PlayerEvent::Stopped { .. } => break,
                PlayerEvent::Unavailable { .. } => {
                    return Err("Spotify will not serve this track to this account".to_string());
                }
                _ => {}
            }
        }
        player.stop();
        Ok::<(), String>(())
    })
}

/// Title and artist, best effort. A track that plays but cannot be named is still a
/// track that plays, so a metadata failure must not become a playback failure.
async fn track_name(
    session: &librespot_core::Session,
    uri: &librespot_core::SpotifyUri,
) -> (String, String) {
    use librespot_metadata::{Metadata, Track};
    match Track::get(session, uri).await {
        Ok(t) => {
            let artists = t.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ");
            (t.name.clone(), artists)
        }
        Err(_) => (String::new(), String::new()),
    }
}

// ---------------------------------------------------------------------------
// The catalogue
// ---------------------------------------------------------------------------

const API: &str = "https://api.spotify.com/v1";
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// How many pages a listing will follow before giving up.
///
/// There IS a limit, because a `next` that never ends would hang the panel with no way
/// to tell. 100 pages of 50 is five thousand items, which is a larger library than the
/// panel can usefully draw.
const MAX_PAGES: usize = 100;

/// One track, in the shape the panel wants: a URI to play, and words to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// `spotify:track:…`. A URI and not a number, which is the one shape change the
    /// panel has to absorb coming from TIDAL.
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub seconds: u32,
    pub explicit: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Album {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub tracks: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Artist {
    pub uri: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playlist {
    pub uri: String,
    pub title: String,
    pub owner: String,
    pub tracks: u32,
}

/// What one search turned up. Four types in one request, drawn together under headings —
/// the shape the TIDAL panel already knows how to draw.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// A GET against the Web API, with the two failures that mean something specific.
fn get(session: &Session, url: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let mut req = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .header("Authorization", &format!("Bearer {}", session.access_token));
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut response = req.call().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    match status {
        200..=299 => serde_json::from_str(&body).map_err(|e| format!("unreadable answer: {e}")),
        401 => Err(SESSION_REVOKED.to_string()),
        // Not "you are not allowed": the endpoint itself is closed to this client. Spotify
        // withdrew a set of Web API endpoints from apps registered after November 2024,
        // and a personal app in development mode is exactly that. Measured on this
        // account, with every scope granted and the user's own data: `/me/playlists`
        // answers, `/playlists/{id}` answers, and `/playlists/{id}/tracks` is 403 — for a
        // playlist the user owns. So this is not something a sign-in can fix, and saying
        // "forbidden" would send someone to check permissions that are already correct.
        403 => Err(format!("Spotify does not serve {} to this client id", endpoint_name(url))),
        // The failure this whole two-id arrangement exists for. Said in full, because
        // "429" on its own sends someone looking for what they did wrong, and they did
        // nothing wrong: the quota belongs to the client id, not to them.
        429 => Err(format!(
            "Spotify is rate-limiting this client id{}. {}",
            retry.map(|r| format!(" (retry after {r}s)")).unwrap_or_default(),
            "If [spotify] api_client_id is empty, the catalogue is sharing the desktop id \
             with every librespot program there is. Register one at developer.spotify.com \
             and sign in with: runnir --spotify-login --api"
        )),
        _ => Err(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>())),
    }
}

/// The page size each endpoint will actually accept, learned at runtime.
///
/// A client id registered now has its page size capped PER ENDPOINT, the caps are not
/// documented, and they do not agree: `/me/tracks` takes 50, `/artists/{id}/albums`
/// refuses 20 and takes 10, `/search` refuses 50. Worse, the refusal is a `400 Invalid
/// limit` — a hard failure, so a fixed number that is right today becomes an empty list
/// the day Spotify moves it.
///
/// So the number is discovered: ask for what we want, halve it on `Invalid limit`, and
/// remember what worked. The alternative — using the smallest cap everywhere — turns a
/// library of 4073 saved tracks into 408 requests instead of 82.
static LIMITS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// The part of a URL that identifies the endpoint rather than the item: the ids in the
/// path are replaced, so `/albums/A/tracks` and `/albums/B/tracks` share a cap.
fn endpoint_key(url: &str) -> String {
    url.trim_start_matches(API)
        .split('?')
        .next()
        .unwrap_or("")
        .split('/')
        .map(|seg| if seg.len() >= 20 && seg.chars().all(|c| c.is_ascii_alphanumeric()) { "{id}" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

fn remembered_limit(url: &str, wanted: u32) -> u32 {
    LIMITS
        .lock()
        .ok()
        .and_then(|m| m.get(&endpoint_key(url)).copied())
        .map(|cap| cap.min(wanted))
        .unwrap_or(wanted)
}

fn remember_limit(url: &str, limit: u32) {
    if let Ok(mut m) = LIMITS.lock() {
        m.insert(endpoint_key(url), limit);
    }
}

/// A GET that keeps halving its page size until Spotify stops calling it invalid.
fn get_paged_once(
    session: &Session,
    url: &str,
    query: &[(&str, &str)],
    limit: u32,
) -> Result<(serde_json::Value, u32), String> {
    let mut limit = remembered_limit(url, limit).max(1);
    loop {
        let text = limit.to_string();
        let mut q: Vec<(&str, &str)> = query.iter().copied().filter(|(k, _)| *k != "limit").collect();
        q.push(("limit", &text));
        match get(session, url, &q) {
            Ok(v) => {
                remember_limit(url, limit);
                return Ok((v, limit));
            }
            // The only error worth retrying, and only downwards.
            Err(e) if e.contains("Invalid limit") && limit > 1 => {
                limit = (limit / 2).max(1);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Follows `next` to the end of a listing.
///
/// Reading one page and stopping is not a smaller version of this: it is a library that
/// silently ends at fifty. The TIDAL audit caught exactly that — playlists, albums and
/// favourites all stopped at the first page, so a playlist of 444 tracks played as one
/// of 100 — and it is written here from the start rather than found again.
fn paged<T>(
    session: &Session,
    url: &str,
    query: &[(&str, &str)],
    parse: impl Fn(&serde_json::Value) -> Option<T>,
) -> Result<Vec<T>, String> {
    let wanted: u32 =
        query.iter().find(|(k, _)| *k == "limit").and_then(|(_, v)| v.parse().ok()).unwrap_or(50);
    let mut out = Vec::new();
    let mut next = Some(url.to_string());
    let mut first = true;
    let mut pages = 0;
    while let Some(url) = next.take() {
        // Only the first request carries the query; `next` already has it baked in, and
        // adding it again is how a paginated request quietly restarts from the top.
        let page = if first {
            get_paged_once(session, &url, query, wanted)?.0
        } else {
            get(session, &url, &[])?
        };
        first = false;
        let items = page.get("items").and_then(|v| v.as_array());
        if let Some(items) = items {
            out.extend(items.iter().filter_map(&parse));
        }
        pages += 1;
        if pages >= MAX_PAGES {
            break;
        }
        next = page.get("next").and_then(|v| v.as_str()).map(str::to_string);
    }
    Ok(out)
}

/// Names the endpoint in the words a person would use, for the message above.
fn endpoint_name(url: &str) -> &'static str {
    if url.contains("/top-tracks") {
        "an artist's top tracks"
    } else if url.contains("/playlists/") && url.contains("/tracks") {
        "the contents of a playlist"
    } else if url.contains("/recommendations") {
        "recommendations"
    } else {
        "this part of the catalogue"
    }
}

fn text(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

fn artists_of(v: &serde_json::Value) -> String {
    v.get("artists")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().map(|x| text(x, "name")).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

pub fn parse_track(v: &serde_json::Value) -> Option<Track> {
    // A playlist's items wrap the track; a search's do not. Unwrapping here rather than
    // at each call site means a playlist entry and a search hit cannot drift apart.
    let v = v.get("track").filter(|t| !t.is_null()).unwrap_or(v);
    let uri = text(v, "uri");
    // Local files a user added to a playlist have no URI worth playing, and a row that
    // cannot be played should not be drawn as if it could.
    if uri.is_empty() || !uri.starts_with("spotify:track:") {
        return None;
    }
    Some(Track {
        uri,
        title: text(v, "name"),
        artist: artists_of(v),
        album: v.get("album").map(|a| text(a, "name")).unwrap_or_default(),
        seconds: v.get("duration_ms").and_then(|d| d.as_u64()).unwrap_or(0).div_ceil(1000) as u32,
        explicit: v.get("explicit").and_then(|e| e.as_bool()).unwrap_or(false),
    })
}

pub fn parse_album(v: &serde_json::Value) -> Option<Album> {
    let v = v.get("album").filter(|a| !a.is_null()).unwrap_or(v);
    let uri = text(v, "uri");
    if uri.is_empty() {
        return None;
    }
    Some(Album {
        uri,
        title: text(v, "name"),
        artist: artists_of(v),
        // `release_date` is "1964", "1964-04" or "1964-04-01" depending on how much the
        // label knew. The year is the part that is always there.
        year: text(v, "release_date").chars().take(4).collect(),
        tracks: v.get("total_tracks").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
    })
}

pub fn parse_artist(v: &serde_json::Value) -> Option<Artist> {
    let uri = text(v, "uri");
    if uri.is_empty() {
        return None;
    }
    Some(Artist { uri, name: text(v, "name") })
}

pub fn parse_playlist(v: &serde_json::Value) -> Option<Playlist> {
    let uri = text(v, "uri");
    if uri.is_empty() {
        return None;
    }
    Some(Playlist {
        uri,
        title: text(v, "name"),
        owner: v.get("owner").map(|o| text(o, "display_name")).unwrap_or_default(),
        tracks: v.get("tracks").and_then(|t| t.get("total")).and_then(|t| t.as_u64()).unwrap_or(0)
            as u32,
    })
}

/// Four types in ONE request, because four requests is four chances to be rate-limited
/// and three extra round trips for a panel that draws them together anyway.
pub fn search(session: &Session, query: &str, limit: u32) -> Result<Found, String> {
    let (v, _) = get_paged_once(
        session,
        &format!("{API}/search"),
        &[("q", query), ("type", "track,album,artist,playlist")],
        limit.clamp(1, 50),
    )?;
    let list = |key: &str| -> Vec<serde_json::Value> {
        v.get(key)
            .and_then(|s| s.get("items"))
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default()
    };
    Ok(Found {
        // Spotify pads a search page with nulls when a result has been removed, and a
        // null is not a row.
        tracks: list("tracks").iter().filter_map(parse_track).collect(),
        albums: list("albums").iter().filter_map(parse_album).collect(),
        artists: list("artists").iter().filter_map(parse_artist).collect(),
        playlists: list("playlists").iter().filter_map(parse_playlist).collect(),
    })
}

/// The id inside a URI: `spotify:track:ABC` → `ABC`. Refuses anything else rather than
/// building a URL out of a guess.
fn id_of(uri: &str, kind: &str) -> Result<String, String> {
    uri.strip_prefix(&format!("spotify:{kind}:"))
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_string)
        .ok_or_else(|| format!("{uri} is not a Spotify {kind} URI"))
}

pub fn album_tracks(session: &Session, uri: &str) -> Result<Vec<Track>, String> {
    let id = id_of(uri, "album")?;
    // An album's own track list omits the album, which is the one field the panel needs
    // and cannot get from here. Asked for once and pasted on, rather than left blank.
    let album = get(session, &format!("{API}/albums/{id}"), &[])?;
    let name = text(&album, "name");
    let mut tracks =
        paged(session, &format!("{API}/albums/{id}/tracks"), &[("limit", "50")], parse_track)?;
    for t in tracks.iter_mut() {
        t.album = name.clone();
    }
    Ok(tracks)
}

pub fn artist_top_tracks(session: &Session, uri: &str) -> Result<Vec<Track>, String> {
    let id = id_of(uri, "artist")?;
    let v = get(session, &format!("{API}/artists/{id}/top-tracks"), &[])?;
    Ok(v.get("tracks")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(parse_track).collect())
        .unwrap_or_default())
}

/// An artist's albums — the answer to "what else is there", and the one that is
/// actually served.
///
/// `/artists/{id}/top-tracks` is 403 for an app registered now, so a panel built on it
/// would have an empty column for every artist. Albums answer a slightly different
/// question and answer it reliably, which is the better trade.
pub fn artist_albums(session: &Session, uri: &str) -> Result<Vec<Album>, String> {
    let id = id_of(uri, "artist")?;
    paged(
        session,
        &format!("{API}/artists/{id}/albums"),
        &[("limit", "50"), ("include_groups", "album,single")],
        parse_album,
    )
}

/// The contents of a playlist.
///
/// Asks the Web API first and falls back to the client protocol, which is the opposite
/// of the order the effort suggests — the fallback is the one that actually works today.
/// The Web API refuses this to a client id registered now, measured on a playlist the
/// signed-in user created with every scope granted, so the first call is there for the
/// day that changes: it costs one request and it is how anyone will find out.
pub fn playlist_tracks(session: &Session, uri: &str) -> Result<Vec<Track>, String> {
    let id = id_of(uri, "playlist")?;
    match paged(session, &format!("{API}/playlists/{id}/tracks"), &[("limit", "50")], parse_track) {
        Ok(tracks) => Ok(tracks),
        // Only the refusal falls through. A timeout or a 500 says nothing about whether
        // this client is allowed, and answering those by opening a second session to
        // Spotify would turn a blip into a minute of reconnecting.
        Err(e) if e.contains("does not serve") => {
            let cfg = crate::config::Config::load().spotify;
            playlist_tracks_deep(&cfg, uri)
        }
        Err(e) => Err(e),
    }
}

pub fn my_playlists(session: &Session) -> Result<Vec<Playlist>, String> {
    paged(session, &format!("{API}/me/playlists"), &[("limit", "50")], parse_playlist)
}

pub fn saved_tracks(session: &Session) -> Result<Vec<Track>, String> {
    paged(session, &format!("{API}/me/tracks"), &[("limit", "50")], parse_track)
}

pub fn saved_albums(session: &Session) -> Result<Vec<Album>, String> {
    paged(session, &format!("{API}/me/albums"), &[("limit", "50")], parse_album)
}

#[cfg(test)]
mod jam_tests {
    use super::*;

    /// Recorded from the real service on 2026-09-16, trimmed to the fields that are
    /// read. Here as a golden vector for the same reason `optical.rs` has them: this is
    /// an UNDOCUMENTED endpoint, so the only thing standing between a shape change and a
    /// panel handing out a dead link is a test that knows what the shape was.
    const REAL_ANSWER: &str = r#"{
        "active_device_id": "…",
        "is_session_owner": true,
        "join_session_token": "4QLt94WQxVijhbQLvcs4mq",
        "join_session_uri": "spotify:socialsession:4QLt94WQxVijhbQLvcs4mq",
        "join_session_url": "hm://social-connect/v2/sessions/join/4QLt94WQxVijhbQLvcs4mq",
        "maxMemberCount": 32,
        "session_id": "9f63fa63dcd32177b6e6133581c1543e",
        "session_members": [
            {"display_name": "drheavymetal", "is_current_user": true},
            {"display_name": "somebody else", "is_current_user": false}
        ],
        "session_owner_id": "3a90cbb26be234047fd0d7e552117b9c"
    }"#;

    #[test]
    fn the_join_link_is_one_a_phone_can_actually_open() {
        let jam = jam_from(&serde_json::from_str(REAL_ANSWER).expect("valid JSON"));
        // The service's own `join_session_url` is an `hm://` address — Spotify's internal
        // scheme. Handing that to somebody is handing them nothing, and it LOOKS like a
        // link, which is worse than an empty footer.
        assert_eq!(
            jam.join_url,
            "https://open.spotify.com/socialsession/4QLt94WQxVijhbQLvcs4mq"
        );
        assert_eq!(jam.session_id, "9f63fa63dcd32177b6e6133581c1543e");
        assert_eq!(jam.members, 2, "the host counts");
        assert!(jam.error.is_none());
    }

    #[test]
    fn a_real_https_link_would_be_preferred_to_one_we_built() {
        let v = serde_json::json!({
            "session_id": "abc",
            "join_session_token": "tok",
            "join_session_url": "https://open.spotify.com/socialsession/tok?si=1",
        });
        assert_eq!(jam_from(&v).join_url, "https://open.spotify.com/socialsession/tok?si=1");
    }

    #[test]
    fn an_answer_with_no_session_is_not_read_as_one() {
        // What an endpoint that refused looks like. `jam_start` turns this into an error
        // rather than a Jam with an empty id, which the panel would draw as a live one.
        let jam = jam_from(&serde_json::json!({}));
        assert!(jam.session_id.is_empty());
        assert_eq!(jam.members, 0);
    }

    #[test]
    fn the_token_can_be_recovered_from_the_url_alone() {
        // Defence for the day `join_session_token` stops being sent: the token is the
        // last segment of the address either way.
        let v = serde_json::json!({
            "session_id": "abc",
            "join_session_url": "hm://social-connect/v2/sessions/join/tok123",
        });
        assert_eq!(jam_from(&v).join_url, "https://open.spotify.com/socialsession/tok123");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_url_becomes_a_uri() {
        assert_eq!(
            uri_from("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"),
            "spotify:track:4cOdK2wGLETKBW3PvgPWqT"
        );
    }

    /// Every link the share button produces has one of these, and feeding it to the
    /// parser makes an id that does not exist — which reads as "Spotify does not have
    /// this track" rather than as a parsing bug.
    #[test]
    fn the_share_tracking_parameter_is_dropped() {
        assert_eq!(
            uri_from("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=abc123"),
            "spotify:track:4cOdK2wGLETKBW3PvgPWqT"
        );
    }

    /// A phone set to Spanish shares `/intl-es/track/<id>`. Taking the first two
    /// segments would make `spotify:intl-es:track`, which parses and then finds nothing.
    #[test]
    fn a_localised_share_url_keeps_the_kind_after_the_country() {
        assert_eq!(
            uri_from("https://open.spotify.com/intl-es/track/4cOdK2wGLETKBW3PvgPWqT?si=x"),
            "spotify:track:4cOdK2wGLETKBW3PvgPWqT"
        );
    }

    #[test]
    fn a_uri_is_left_alone_and_so_is_nonsense() {
        assert_eq!(uri_from("spotify:track:abc"), "spotify:track:abc");
        assert_eq!(uri_from("hello"), "hello");
        assert_eq!(uri_from("https://open.spotify.com/track/"), "https://open.spotify.com/track/");
    }

    /// The fallback to the client protocol is triggered by MATCHING THE ERROR TEXT, which
    /// is a thread thin enough to snap without anyone noticing: reword the 403 message
    /// and playlists silently stop working, with no failing test and no compiler error.
    /// This is the test that notices.
    #[test]
    fn the_refusal_message_is_what_the_fallback_looks_for() {
        let message =
            format!("Spotify does not serve {} to this client id", endpoint_name("/playlists/P/tracks"));
        assert!(
            message.contains("does not serve"),
            "playlist_tracks falls back on this substring; keep them together"
        );
    }

    /// Loopback, by IP. Spotify refuses `localhost` for a redirect URI, and the failure
    /// arrives as a rejected sign-in rather than as a message about the hostname.
    #[test]
    fn the_redirect_is_a_loopback_ip() {
        let cfg = Cfg::default();
        assert_eq!(redirect_uri(Which::Audio, &cfg), "http://127.0.0.1:8898/login");
        // Not arbitrary: it is the redirect registered for the default client id, so a
        // different number needs a client id of your own to go with it.
        assert_eq!(cfg.callback_port, 8898);
    }

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("fixture is valid JSON")
    }

    /// A search hit and a playlist entry are the same track in two wrappers. Unwrapping
    /// in one place is what stops them drifting apart.
    #[test]
    fn a_track_parses_wrapped_and_bare() {
        let bare = json(
            r#"{"uri":"spotify:track:ABC","name":"My Home Is In The Delta",
                "artists":[{"name":"Muddy Waters"}],"album":{"name":"Folk Singer"},
                "duration_ms":241000,"explicit":false}"#,
        );
        let wrapped = json(&format!(r#"{{"added_at":"2020-01-01T00:00:00Z","track":{bare}}}"#));
        let a = parse_track(&bare).expect("bare track");
        let b = parse_track(&wrapped).expect("wrapped track");
        assert_eq!(a, b);
        assert_eq!(a.title, "My Home Is In The Delta");
        assert_eq!(a.album, "Folk Singer");
        assert_eq!(a.seconds, 241);
    }

    #[test]
    fn several_artists_are_joined_in_order() {
        let v = json(r#"{"uri":"spotify:track:ABC","name":"Sinner's Prayer",
                         "artists":[{"name":"Ray Charles"},{"name":"B.B. King"}]}"#);
        assert_eq!(parse_track(&v).unwrap().artist, "Ray Charles, B.B. King");
    }

    /// A local file a user dragged into a playlist has no playable URI. Drawing it as a
    /// row would offer something that cannot be played, and a null is what Spotify puts
    /// where a removed track used to be.
    #[test]
    fn rows_that_cannot_be_played_are_not_rows() {
        assert_eq!(parse_track(&json(r#"{"uri":"spotify:local:x:y:z","name":"a.mp3"}"#)), None);
        assert_eq!(parse_track(&json(r#"{"track":null}"#)), None);
        assert_eq!(parse_track(&json(r#"{"name":"no uri at all"}"#)), None);
    }

    /// `release_date` is as precise as the label bothered to be. The year is the part
    /// that is always there, and slicing four characters off "1964-04-01" is the whole
    /// trick — but it has to survive "1964" too.
    #[test]
    fn the_year_survives_every_shape_of_release_date() {
        for date in ["1964", "1964-04", "1964-04-01"] {
            let v = json(&format!(
                r#"{{"uri":"spotify:album:A","name":"Folk Singer","release_date":"{date}",
                     "artists":[{{"name":"Muddy Waters"}}],"total_tracks":14}}"#
            ));
            assert_eq!(parse_album(&v).unwrap().year, "1964", "for {date}");
        }
        // No date at all is empty, not "1970" and not a panic.
        let v = json(r#"{"uri":"spotify:album:A","name":"x"}"#);
        assert_eq!(parse_album(&v).unwrap().year, "");
    }

    /// Spotify stopped sending `tracks.total` to newly registered clients, so the count
    /// is missing rather than wrong. Zero must not become a panic or a wrong number.
    #[test]
    fn a_playlist_without_a_count_still_parses() {
        let v = json(r#"{"uri":"spotify:playlist:P","name":"Viking folk",
                         "owner":{"display_name":"drheavymetal"}}"#);
        let p = parse_playlist(&v).unwrap();
        assert_eq!(p.title, "Viking folk");
        assert_eq!(p.owner, "drheavymetal");
        assert_eq!(p.tracks, 0);
    }

    /// The page-size cap is per endpoint, so two albums must share one entry and two
    /// different endpoints must not.
    #[test]
    fn the_limit_cache_is_keyed_by_endpoint_not_by_item() {
        assert_eq!(
            endpoint_key("https://api.spotify.com/v1/albums/4bi0CKFKviadIaSlkakfN7/tracks"),
            endpoint_key("https://api.spotify.com/v1/albums/0CFpUxbVKTYbqpEiaXAyZT/tracks")
        );
        assert_eq!(
            endpoint_key("https://api.spotify.com/v1/albums/4bi0CKFKviadIaSlkakfN7/tracks"),
            "/albums/{id}/tracks"
        );
        assert_ne!(endpoint_key("/me/tracks"), endpoint_key("/me/albums"));
    }

    /// Building a URL out of the wrong kind of URI asks Spotify about something that
    /// does not exist, and the answer looks like "not found" rather than "wrong type".
    #[test]
    fn an_id_is_only_taken_from_the_right_kind_of_uri() {
        assert_eq!(id_of("spotify:album:4bi0CKFKviadIaSlkakfN7", "album").unwrap(), "4bi0CKFKviadIaSlkakfN7");
        assert!(id_of("spotify:track:ABC", "album").is_err());
        assert!(id_of("spotify:album:", "album").is_err());
        // Path traversal through an id is refused rather than sent.
        assert!(id_of("spotify:album:../../me", "album").is_err());
    }

    /// A refusal has to say which thing was refused, because "403" sends someone to
    /// check permissions that are already correct.
    #[test]
    fn a_refusal_names_what_was_refused() {
        assert_eq!(
            endpoint_name("https://api.spotify.com/v1/playlists/P/tracks"),
            "the contents of a playlist"
        );
        assert_eq!(endpoint_name("https://api.spotify.com/v1/artists/A/top-tracks"), "an artist's top tracks");
    }

    /// The margin is the point: a token that dies mid-request is a failure someone sees.
    #[test]
    fn a_session_is_stale_before_it_actually_expires() {
        let live = Session { access_token: "a".into(), refresh_token: "r".into(), expires_at: now() + REFRESH_MARGIN * 4 };
        assert!(!live.expired());
        let nearly = Session { access_token: "a".into(), refresh_token: "r".into(), expires_at: now() + REFRESH_MARGIN / 2 };
        assert!(nearly.expired(), "a token inside the margin must be refreshed, not used");
    }
}

// ---------------------------------------------------------------------------
// The engine: one session, one player, for as long as the daemon lives
// ---------------------------------------------------------------------------

/// A live Spotify player, owned by whoever is running the queue.
///
/// Built once and kept, for the same reason the TIDAL path keeps its ALSA device across
/// tracks: connecting costs a second or two, and paying it per track would put a gap in
/// every album. It also means librespot's own gapless handling gets to do its job, since
/// the sink is not torn down between tracks either.
///
/// The tokio runtime lives here and nowhere else in this program. One worker thread, not
/// a pool: it drives one player's network IO. It is not `current_thread`, though it was,
/// because a `current_thread` runtime only runs what is spawned on it while somebody is
/// blocked on it — and the Connect device has to keep answering Spotify between tracks
/// and while nothing is playing at all, which is most of the time.
pub struct Engine {
    rt: tokio::runtime::Runtime,
    player: std::sync::Arc<librespot_playback::player::Player>,
    events: librespot_playback::player::PlayerEventChannel,
    signal: std::sync::Arc<std::sync::Mutex<crate::player::SignalPath>>,
    levels: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    /// The Spotify Connect device, when the terminal is announcing itself as one.
    ///
    /// When it is there it is the only thing that may touch the player: loading the
    /// player behind Spirc's back leaves the phone saying nothing is playing while the
    /// terminal plays, which is the whole complaint this answers.
    connect: Option<librespot_connect::Spirc>,
    /// Kept for the endpoints that are not playback: a Jam is hosted by a DEVICE, and
    /// this session is what makes the terminal one.
    session: librespot_core::Session,
}

/// A Jam hosted by this terminal: a listening session other people can join.
///
/// Shaped like `share::State` on purpose. They are the same idea twice — a link that
/// lets other people in on what is happening here — and the panel draws them the same
/// way, so they had better report themselves the same way too.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Jam {
    /// Spotify's id for the session. Needed to end it.
    pub session_id: String,
    /// The link to hand out — the same one the official apps put behind their QR code.
    pub join_url: String,
    /// How many people are in it, the host included.
    pub members: usize,
    /// Set when starting failed, so the panel can say why rather than showing nothing.
    pub error: Option<String>,
}

/// The live session, reachable from outside the thread that is busy making sound.
///
/// A Jam is not playback. It is asked for by a window, it outlives the track, and the
/// answer takes a network round trip that must not happen between two audio packets. So
/// the session — cheap to clone, safe to use from anywhere — is left here when the engine
/// is built and taken back when it goes, and the daemon does the asking on a thread of
/// its own. Exactly the shape the share already has, one service over.
static JAM: std::sync::Mutex<Option<JamSeat>> = std::sync::Mutex::new(None);

struct JamSeat {
    session: librespot_core::Session,
    rt: tokio::runtime::Handle,
}

/// Starts or ends the Jam. BLOCKS on the network; call it on a thread of its own.
///
/// `session_id` is the one being ended and is ignored when starting. It comes from the
/// snapshot rather than from here because the daemon is what remembers, and a second
/// copy of "which Jam is open" is a second chance to end the wrong one.
pub fn jam_set(on: bool, session_id: &str) -> Result<Option<Jam>, String> {
    let (session, rt) = {
        let held = JAM.lock().map_err(|_| "the Spotify session is wedged".to_string())?;
        let seat = held
            .as_ref()
            .ok_or("Spotify is not connected yet — play something first")?;
        (seat.session.clone(), seat.rt.clone())
    };
    let engine = JamClient { session, rt };
    if on {
        engine.start().map(Some)
    } else {
        engine.stop(session_id).map(|()| None)
    }
}

/// The Jam endpoints, off the engine so they can be used while it plays.
struct JamClient {
    session: librespot_core::Session,
    rt: tokio::runtime::Handle,
}

impl JamClient {
    fn ask(&self, method: http::Method, path: &str) -> Result<serde_json::Value, String> {
        let client = self.session.spclient();
        let bytes = self
            .rt
            .block_on(async { client.request_as_json(&method, path, None, None).await })
            .map_err(|e| format!("Spotify refused the request: {e}"))?;
        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            format!(
                "Spotify answered something that is not JSON ({e}): {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    }

    fn start(&self) -> Result<Jam, String> {
        let path = format!(
            "/social-connect/v2/sessions/current_or_new?local_device_id={}",
            self.session.device_id()
        );
        let answer = self.ask(http::Method::GET, &path)?;
        let jam = jam_from(&answer);
        if jam.session_id.is_empty() {
            return Err(format!("Spotify started no Jam: {answer}"));
        }
        Ok(jam)
    }

    fn stop(&self, session_id: &str) -> Result<(), String> {
        if session_id.is_empty() {
            return Ok(());
        }
        self.ask(http::Method::DELETE, &format!("/social-connect/v2/sessions/{session_id}"))?;
        Ok(())
    }
}

/// Reads whatever a session endpoint answered into the shape the panel wants.
fn jam_from(v: &serde_json::Value) -> Jam {
    // `join_session_url` comes back as an `hm://` address — Spotify's own internal
    // scheme, which no phone can open. The token is the part that is actually the
    // invitation, so the link is built from it, and the service's own url is used only
    // if it ever starts being a real one.
    let token = v["join_session_token"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            v["join_session_url"]
                .as_str()
                .or_else(|| v["join_session_uri"].as_str())
                .and_then(|u| u.rsplit('/').next())
                .unwrap_or_default()
                .to_string()
        });
    let join_url = match v["join_session_url"].as_str() {
        Some(url) if url.starts_with("https://") => url.to_string(),
        _ => format!("https://open.spotify.com/socialsession/{token}"),
    };
    Jam {
        session_id: v["session_id"].as_str().unwrap_or_default().to_string(),
        join_url,
        members: v["session_members"].as_array().map(Vec::len).unwrap_or(0),
        error: None,
    }
}

impl Drop for Engine {
    /// Takes the Jam's seat back. The handle in it belongs to this engine's runtime, and
    /// a handle outliving its runtime is a panic waiting for whoever asks next.
    fn drop(&mut self) {
        if let Ok(mut seat) = JAM.lock() {
            *seat = None;
        }
    }
}

/// A mixer that remembers a number and touches nothing.
///
/// Spotify Connect insists on one; this output chain must not have one. The point of the
/// device path is that the samples reach the card as they left the decoder, and the only
/// way a volume slider can reach them is by multiplying them. So the number is kept, and
/// reported back so a remote slider does not snap about, and never applied: the default
/// `get_soft_volume` is librespot's no-op, which is unity gain. The device also asks
/// Spotify not to offer the slider at all — see `disable_volume` below.
#[derive(Default)]
struct FixedMixer(std::sync::atomic::AtomicU16);

impl librespot_playback::mixer::Mixer for FixedMixer {
    fn open(_: librespot_playback::mixer::MixerConfig) -> Result<Self, librespot_core::Error> {
        Ok(Self::default())
    }

    fn volume(&self) -> u16 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_volume(&self, volume: u16) {
        self.0.store(volume, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Engine {
    /// Connects. Blocking, and slow enough to be worth doing once.
    pub fn new(cfg: &Cfg) -> Result<Engine, String> {
        use librespot_core::{Session as LsSession, SessionConfig, authentication::Credentials};
        use librespot_playback::config::{Bitrate, PlayerConfig};
        use librespot_playback::mixer::NoOpVolume;
        use librespot_playback::player::Player;

        let session = current(Which::Audio)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("could not start the async runtime: {e}"))?;

        let signal = std::sync::Arc::new(std::sync::Mutex::new(crate::player::SignalPath::default()));
        let levels = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = crate::player::Output {
            device: cfg.output.clone(),
            bit_perfect: cfg.bit_perfect,
            release_device: cfg.release_device,
        };

        let (player, connect, ls) = rt.block_on(async {
            let ls_cfg =
                SessionConfig { client_id: cfg.client_id.clone(), ..SessionConfig::default() };
            let ls = LsSession::new(ls_cfg, None);
            let creds = Credentials::with_access_token(session.access_token);

            let player_cfg = PlayerConfig {
                bitrate: Bitrate::Bitrate320,
                // Without this there is no position to draw: the panel would show a
                // progress bar frozen at zero for the length of every song.
                position_update_interval: Some(std::time::Duration::from_millis(200)),
                ..PlayerConfig::default()
            };
            let (sig, lev) = (signal.clone(), levels.clone());
            // Built before the session is connected, which is librespot's own order: the
            // player works on a thread of its own and only needs the session once there
            // is a track to fetch. Connect wants to do the connecting itself.
            let player = Player::new(player_cfg, ls.clone(), Box::new(NoOpVolume), move || {
                let mut sink = ChainSink::new(
                    output,
                    librespot_playback::SAMPLE_RATE,
                    librespot_playback::NUM_CHANNELS as u32,
                    sig,
                );
                sink.levels = lev;
                Box::new(sink)
            });

            let connect = if cfg.connect_device {
                let connect_cfg = librespot_connect::ConnectConfig {
                    name: cfg.device_name.clone(),
                    // A terminal is a computer, and saying so is what puts the right icon
                    // beside the name in the phone's list of devices.
                    device_type: librespot_core::config::DeviceType::Computer,
                    // The one control this device must not offer. Everything below here is
                    // exclusive and unresampled, and a remote volume slider can only reach
                    // those samples by multiplying them — so the phone is told there is no
                    // slider, rather than given one that quietly lies about what it does.
                    disable_volume: true,
                    ..Default::default()
                };
                // `Spirc::new` connects the session itself, so this is also the sign-in.
                match librespot_connect::Spirc::new(
                    connect_cfg,
                    ls.clone(),
                    creds.clone(),
                    player.clone(),
                    std::sync::Arc::new(FixedMixer::default()),
                )
                .await
                {
                    Ok((spirc, task)) => {
                        tokio::spawn(task);
                        Some(spirc)
                    }
                    // Being visible is not worth losing playback over: if Connect will not
                    // start, the terminal still plays — just silently, as far as the rest
                    // of Spotify is concerned.
                    Err(e) => {
                        eprintln!("runnir: Spotify Connect did not start ({e}); playing anyway");
                        None
                    }
                }
            } else {
                None
            };

            if connect.is_none() {
                ls.connect(creds, false)
                    .await
                    .map_err(|e| format!("Spotify refused the session: {e}"))?;
            }
            Ok::<_, String>((player, connect, ls))
        })?;

        // The Jam's seat. Left here rather than handed out, because the thing that wants
        // it — a window asking for a Jam — has no way to reach an engine that is inside
        // the player thread's queue loop.
        if let Ok(mut seat) = JAM.lock() {
            *seat = Some(JamSeat { session: ls.clone(), rt: rt.handle().clone() });
        }

        let events = player.get_player_event_channel();
        Ok(Engine { rt, player, events, signal, levels, connect, session: ls })
    }

    /// Everything Spotify says about this terminal's session, unread. For the diagnostic.
    pub fn jam_raw(&self) -> Result<serde_json::Value, String> {
        let client = self.session.spclient();
        let path = format!(
            "/social-connect/v2/sessions/current_or_new?local_device_id={}",
            self.session.device_id()
        );
        let bytes = self
            .rt
            .block_on(async { client.request_as_json(&http::Method::GET, &path, None, None).await })
            .map_err(|e| format!("Spotify refused the request: {e}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("not JSON ({e}): {}", String::from_utf8_lossy(&bytes)))
    }

    /// Plays one track to its end, or until the conductor says otherwise.
    ///
    /// Shaped like the TIDAL path on purpose: same `Flow`, same `Progress`, same
    /// `Outcome`, so the queue above it does not have to know which provider it is
    /// running. What differs is where the numbers come from — librespot reports the
    /// position, and the sink reports how loud what it just wrote was.
    pub fn play(
        &mut self,
        uri: &str,
        conductor: &mut dyn FnMut(crate::player::Progress<'_>) -> crate::player::Flow,
    ) -> crate::player::Outcome {
        use crate::player::{Flow, Outcome};
        use librespot_core::SpotifyUri;
        use librespot_playback::player::PlayerEvent;

        let raw_uri = uri;
        let uri = match SpotifyUri::from_uri(uri) {
            Ok(u) => u,
            Err(e) => return Outcome::Failed(format!("not a Spotify URI: {e}")),
        };
        // Left over from whatever played before: a wave that keeps its last shape while
        // a new track is still resolving looks like the old track is still playing.
        if let Ok(mut l) = self.levels.lock() {
            l.clear();
        }
        if let Some(spirc) = self.connect.as_ref() {
            // Through the device, never around it. Spirc drops every command that reaches
            // it while it is only advertising, so activating is not optional — and a load
            // that goes straight to the player leaves Spotify showing an idle device
            // while the terminal plays, which is the whole point of having one.
            if let Err(e) = spirc.activate() {
                return Outcome::Failed(format!("Spotify Connect would not take over: {e}"));
            }
            // One track, not the queue. The queue above here owns what plays next, and a
            // context holding the rest of it would have Spirc advancing as well — two
            // things moving one queue, each skipping the other's track.
            let load = librespot_connect::LoadRequest::from_tracks(
                vec![raw_uri.to_string()],
                librespot_connect::LoadRequestOptions {
                    start_playing: true,
                    ..Default::default()
                },
            );
            if let Err(e) = spirc.load(load) {
                return Outcome::Failed(format!("Spotify Connect refused the track: {e}"));
            }
        } else {
            self.player.load(uri, true, 0);
        }

        let mut position_ms: u32 = 0;
        let mut paused = false;
        let events = &mut self.events;
        let (player, signal, levels) = (&self.player, &self.signal, &self.levels);
        let connect = self.connect.as_ref();

        self.rt.block_on(async move {
            loop {
                // Never blocks for long: the conductor has to be asked often enough that
                // a key press is answered promptly, and a signal has to be able to stop
                // this loop before the process is exited out from under the device.
                let tick = std::time::Duration::from_millis(100);
                let event = match tokio::time::timeout(tick, events.recv()).await {
                    Ok(Some(e)) => Some(e),
                    Ok(None) => return Outcome::Ended,
                    Err(_) => None,
                };
                match event {
                    // `PositionChanged` is the one that arrives on a timer, and the
                    // timer is `position_update_interval` above. Without it in here the
                    // only position ever seen was the zero that came with `Playing`, so
                    // the progress bar sat at the start of the song for the whole song.
                    // `PositionCorrection` is not a substitute: it fires when the clock
                    // was WRONG, which on a track that plays normally is never.
                    Some(PlayerEvent::PositionChanged { position_ms: p, .. })
                    | Some(PlayerEvent::PositionCorrection { position_ms: p, .. })
                    | Some(PlayerEvent::Playing { position_ms: p, .. })
                    | Some(PlayerEvent::Paused { position_ms: p, .. }) => position_ms = p,
                    Some(PlayerEvent::EndOfTrack { .. }) => return Outcome::Ended,
                    Some(PlayerEvent::Stopped { .. }) => return Outcome::Stopped,
                    Some(PlayerEvent::Unavailable { .. }) => {
                        return Outcome::Failed(
                            "Spotify will not serve this track to this account".into(),
                        );
                    }
                    _ => {}
                }
                if crate::reserve::shutting_down() {
                    player.stop();
                    return Outcome::Quit;
                }

                let sig = signal.lock().ok().map(|s| s.clone()).unwrap_or_default();
                let rate = if sig.decoded_rate == 0 {
                    librespot_playback::SAMPLE_RATE
                } else {
                    sig.decoded_rate
                };
                let flow = conductor(crate::player::Progress {
                    // The queue counts in frames because that is what the TIDAL path
                    // has; librespot counts in milliseconds. One multiplication keeps
                    // the difference out of everything above here.
                    frames: position_ms as u64 * rate as u64 / 1000,
                    rate,
                    signal: Some(&sig),
                    levels: if paused {
                        // Bars standing still next to a paused track read as "still
                        // playing". Flat is the honest shape.
                        Vec::new()
                    } else {
                        levels.lock().map(|l| l.clone()).unwrap_or_default()
                    },
                });
                match flow {
                    Flow::Continue => {
                        if paused {
                            paused = false;
                            match connect {
                                Some(spirc) => drop(spirc.play()),
                                None => player.play(),
                            }
                        }
                    }
                    Flow::Pause => {
                        if !paused {
                            paused = true;
                            // Pausing the player directly would leave the phone showing a
                            // song still running. Spirc sees the same player events either
                            // way, but only a command it issued itself updates its state.
                            match connect {
                                Some(spirc) => drop(spirc.pause()),
                                None => player.pause(),
                            }
                        }
                    }
                    Flow::Skip => {
                        player.stop();
                        return Outcome::Ended;
                    }
                    Flow::Stop => {
                        player.stop();
                        return Outcome::Stopped;
                    }
                }
            }
        })
    }

    /// Stops and lets go of the device, which is what makes room for the other provider.
    pub fn release(&mut self) {
        // And lets go of the Connect device as well. A terminal that goes on claiming to
        // be Spotify's active speaker while it is playing TIDAL is lying to every phone
        // on the account — and the next thing sent to it would go nowhere.
        if let Some(spirc) = self.connect.as_ref() {
            let _ = spirc.disconnect(true);
        }
        self.player.stop();
        // The sink closes on the player's own thread; give it the moment ALSA needs
        // before anything else tries to open the same card.
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}

// ---------------------------------------------------------------------------
// Playlists, through the door the Web API closed
// ---------------------------------------------------------------------------

/// A connection to Spotify kept for metadata, separate from the one the daemon plays
/// through.
///
/// **This is an amendment to the design in this file**, which said the tokio runtime
/// would live in the player daemon and nowhere else. That held until the Web API refused
/// to hand over playlist contents: the panel resolves its own lists, in its own worker
/// threads, in the window process — and the only door left to those contents speaks the
/// client protocol. So the window gets a runtime too.
///
/// Kept rather than built per call because connecting costs about a second. Two
/// connections for one account is fine — Spotify allows it, and neither of them is a
/// Connect device, so nothing appears on anyone's phone because a playlist was opened.
struct Metadata {
    rt: tokio::runtime::Runtime,
    session: librespot_core::Session,
}

static METADATA: std::sync::Mutex<Option<Metadata>> = std::sync::Mutex::new(None);

/// Fills the slot if it is empty, so the caller can unwrap it.
///
/// Takes the guard rather than locking inside, so the connection happens with the lock
/// held: two panels opening two playlists at once should make one session, not two.
fn metadata_session(
    guard: &mut Option<Metadata>,
    cfg: &Cfg,
) -> Result<(), String> {
    use librespot_core::{Session as LsSession, SessionConfig, authentication::Credentials};
    if guard.is_none() {
        let session = current(Which::Audio)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("could not start the async runtime: {e}"))?;
        let ls_cfg = SessionConfig { client_id: cfg.client_id.clone(), ..SessionConfig::default() };
        // Built INSIDE the runtime: `Session::new` registers with the reactor, and
        // constructing one outside panics with "there is no reactor running" — which is
        // a message about tokio rather than about Spotify, and sends you looking in the
        // wrong place entirely.
        let ls = rt.block_on(async move {
            let ls = LsSession::new(ls_cfg, None);
            ls.connect(Credentials::with_access_token(session.access_token), false)
                .await
                .map_err(|e| format!("Spotify refused the session: {e}"))?;
            Ok::<_, String>(ls)
        })?;
        *guard = Some(Metadata { rt, session: ls });
    }
    Ok(())
}

/// The contents of a playlist, fetched over the client protocol instead of the Web API.
///
/// **Why this exists.** `/playlists/{id}/tracks` answers 403 to a client id registered
/// now — measured on a playlist the signed-in user created, with every scope granted. It
/// is not a permission and not a sign-in, so no amount of asking differently fixes it.
/// librespot speaks the protocol the desktop client speaks, where a playlist carries its
/// own item list, and that door is not the one Spotify closed.
///
/// The cost is real and worth stating: an item carries only a URI, so every track is a
/// round trip. They go out concurrently, in flight at a time rather than all at once,
/// because a playlist of four hundred would otherwise open four hundred requests in one
/// breath.
pub fn playlist_tracks_deep(cfg: &Cfg, uri: &str) -> Result<Vec<Track>, String> {
    use librespot_core::SpotifyUri;
    use librespot_metadata::{Metadata, Playlist, Track as LsTrack};

    let uri = SpotifyUri::from_uri(uri).map_err(|e| format!("not a Spotify URI: {e}"))?;
    let mut guard = METADATA.lock().unwrap_or_else(|e| e.into_inner());
    metadata_session(&mut guard, cfg)?;
    let meta = guard.as_ref().expect("metadata_session filled it");
    let (rt, ls) = (&meta.rt, meta.session.clone());

    rt.block_on(async move {
        let playlist = Playlist::get(&ls, &uri)
            .await
            .map_err(|e| format!("could not read the playlist: {e}"))?;

        let uris: Vec<SpotifyUri> = playlist
            .contents
            .items
            .iter()
            .map(|item| item.id.clone())
            .filter(|id| matches!(id, SpotifyUri::Track { .. }))
            .collect();

        /// How many metadata requests are allowed in flight. Enough that a long playlist
        /// does not resolve one round trip at a time; few enough that opening one does
        /// not look like a flood to the other end.
        const IN_FLIGHT: usize = 16;

        let mut out: Vec<Option<Track>> = vec![None; uris.len()];
        let mut next = 0usize;
        let mut running = tokio::task::JoinSet::new();
        loop {
            while running.len() < IN_FLIGHT && next < uris.len() {
                let (ls, uri, at) = (ls.clone(), uris[next].clone(), next);
                next += 1;
                running.spawn(async move {
                    let track = LsTrack::get(&ls, &uri).await.ok();
                    (at, track)
                });
            }
            let Some(done) = running.join_next().await else { break };
            if let Ok((at, Some(t))) = done {
                out[at] = Some(Track {
                    uri: t.id.to_uri().unwrap_or_default(),
                    title: t.name,
                    artist: t.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "),
                    album: t.album.name,
                    // Milliseconds here, as everywhere in this protocol.
                    seconds: (t.duration.max(0) as u32).div_ceil(1000),
                    explicit: t.is_explicit,
                });
            }
        }
        // A track that would not resolve is dropped rather than drawn as a blank row:
        // a row with no title is a row nobody can decide about.
        Ok(out.into_iter().flatten().collect())
    })
}
