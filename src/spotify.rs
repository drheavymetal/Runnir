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

/// Refreshed this long before it actually expires. A token that dies mid-request is a
/// failure a user sees; a minute of unused life is not.
const REFRESH_MARGIN: u64 = 60;

pub const NOT_SIGNED_IN: &str = "not signed in — run: runnir --spotify-login";

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
    pub fn path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("runnir").join("spotify-session.json"))
    }

    pub fn load() -> Option<Session> {
        let text = std::fs::read_to_string(Self::path()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Owner-only, with the mode set before the tokens are written rather than after.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no data directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::tidal::write_private(&path, json.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    #[allow(dead_code)]
    pub fn forget() -> Result<(), String> {
        let Some(path) = Self::path() else { return Ok(()) };
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
pub fn redirect_uri(cfg: &Cfg) -> String {
    format!("http://127.0.0.1:{}/login", cfg.callback_port)
}

fn client(cfg: &Cfg) -> Result<librespot_oauth::OAuthClient, String> {
    librespot_oauth::OAuthClientBuilder::new(&cfg.client_id, &redirect_uri(cfg), SCOPES.to_vec())
        .open_in_browser()
        .build()
        .map_err(|e| format!("could not start the Spotify sign-in: {e}"))
}

/// Signs in: opens the browser, listens on the loopback port, exchanges the code.
///
/// Blocking on purpose. `librespot-oauth` offers both forms and the synchronous one
/// needs no runtime, which keeps the one tokio runtime this program builds inside the
/// player daemon where it belongs.
pub fn login(cfg: &Cfg) -> Result<Session, String> {
    let token = client(cfg)?
        .get_access_token()
        .map_err(|e| format!("Spotify sign-in failed: {e}"))?;
    let session = session_from(token);
    session.save()?;
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
pub fn current() -> Result<Session, String> {
    let session = Session::load().ok_or(NOT_SIGNED_IN)?;
    if !session.expired() {
        return Ok(session);
    }
    // Poisoning is not a reason to stop refreshing: the guarded data is `()`.
    let _guard = REFRESH.lock().unwrap_or_else(|e| e.into_inner());
    // Re-read under the lock: waiting for someone else's refresh and then refreshing
    // again from the copy read before the wait is the race this exists to lose.
    let session = Session::load().ok_or(NOT_SIGNED_IN)?;
    if !session.expired() {
        return Ok(session);
    }
    let cfg = crate::config::Config::load().spotify;
    let token = client(&cfg)?
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
    if let Err(e) = next.save() {
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
        ChainSink { output, device: None, signal, rate, channels }
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
            sink.signal.lossy = true;
            if let Ok(mut s) = self.signal.lock() {
                *s = sink.signal.clone();
            }
            self.device = Some(sink);
        }
        Ok(self.device.as_mut().expect("just opened"))
    }
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
    let session = current()?;

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

    /// Loopback, by IP. Spotify refuses `localhost` for a redirect URI, and the failure
    /// arrives as a rejected sign-in rather than as a message about the hostname.
    #[test]
    fn the_redirect_is_a_loopback_ip() {
        let cfg = Cfg::default();
        assert_eq!(redirect_uri(&cfg), "http://127.0.0.1:8898/login");
        // Not arbitrary: it is the redirect registered for the default client id, so a
        // different number needs a client id of your own to go with it.
        assert_eq!(cfg.callback_port, 8898);
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
