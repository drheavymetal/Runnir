//! Announcing the player on the session bus, so the desktop knows runnir is playing.
//!
//! Without this the music is invisible: the media keys go to whatever else is running,
//! the desktop's now-playing widget shows nothing, and there is no way to pause from
//! outside the terminal. A player that only its own window can control is a worse
//! player, however good the audio path is.
//!
//! MPRIS is a D-Bus interface, and D-Bus is asynchronous, but runnir has no async
//! runtime and should not grow one for this. The whole thing therefore lives on ONE
//! thread of its own that blocks on a tiny executor: nothing here ever runs on the UI
//! thread, and the rest of the program talks to it through the same `Cmd` channel and
//! `Snapshot` the panel uses.
//!
//! `media.rs` is the mirror image of this file — it READS other players over MPRIS
//! (through `playerctl`). This one publishes.

use std::sync::{Arc, Mutex};

use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Time, TrackId, Volume, zbus::fdo,
};

use crate::player::{Cmd, Snapshot};

/// How often the published state is compared with the player's. Polling rather than
/// being pushed at, because the player must not have to know that D-Bus exists — and
/// five times a second is far below what anyone can see in a widget.
const POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// The handle the D-Bus side holds: commands out, state in.
///
/// `Sender` is `Send` but not `Sync`, and the interface has to be both, hence the
/// mutex. Contention on it is nil — one thread sends, and only when a key is pressed
/// somewhere on the desktop.
struct Player {
    tx: Mutex<std::sync::mpsc::Sender<Cmd>>,
    state: Arc<Mutex<Snapshot>>,
}

impl Player {
    fn send(&self, cmd: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(cmd);
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Starts the announcement. Returns immediately; everything happens on its own thread.
///
/// A failure to reach the bus is not fatal and not even worth a message on screen: a
/// machine with no session bus still plays music, it just does not tell anyone.
pub fn publish(tx: std::sync::mpsc::Sender<Cmd>, state: Arc<Mutex<Snapshot>>) {
    std::thread::Builder::new()
        .name("runnir-mpris".into())
        .spawn(move || {
            futures_lite::future::block_on(async move {
                let player = Player { tx: Mutex::new(tx), state: state.clone() };
                let server = match Server::new("runnir", player).await {
                    Ok(s) => s,
                    Err(e) => {
                        log_once(&format!("mpris: not announcing ({e})"));
                        return;
                    }
                };
                announce_changes(server, state).await;
            });
        })
        .ok();
}

/// Watches the player and tells the bus when something a widget draws has changed.
///
/// Only on a real change: `PropertiesChanged` five times a second forever would keep
/// every listener on the bus awake for nothing. The generation counter is what makes
/// "nothing happened" cheap to detect.
async fn announce_changes(server: Server<Player>, state: Arc<Mutex<Snapshot>>) {
    let mut last_generation = u64::MAX;
    let mut last_status = PlaybackStatus::Stopped;
    let mut last_track: Option<u64> = None;
    loop {
        let snapshot = state.lock().map(|s| s.clone()).unwrap_or_default();
        if snapshot.generation != last_generation {
            last_generation = snapshot.generation;
            let status = status_of(&snapshot);
            let track = snapshot.now_playing().map(|t| t.id);

            let mut changed: Vec<Property> = Vec::new();
            if status != last_status {
                last_status = status;
                changed.push(Property::PlaybackStatus(status));
                // What can be done changes with the state, and a widget greys its
                // buttons by these rather than by guessing from the status.
                changed.push(Property::CanPause(status == PlaybackStatus::Playing));
                changed.push(Property::CanPlay(true));
            }
            if track != last_track {
                last_track = track;
                changed.push(Property::Metadata(metadata_of(&snapshot)));
                changed.push(Property::CanGoNext(can_go_next(&snapshot)));
                changed.push(Property::CanGoPrevious(snapshot.index > 0));
            }
            if !changed.is_empty() {
                let _ = server.properties_changed(changed).await;
            }
        }
        // A plain sleep on this thread: it owns nothing else, and a timer that needs a
        // reactor would mean bringing in the runtime this file exists to avoid.
        std::thread::sleep(POLL);
    }
}

fn status_of(s: &Snapshot) -> PlaybackStatus {
    if !s.playing {
        PlaybackStatus::Stopped
    } else if s.paused {
        PlaybackStatus::Paused
    } else {
        PlaybackStatus::Playing
    }
}

fn can_go_next(s: &Snapshot) -> bool {
    s.index + 1 < s.queue.len()
}

fn metadata_of(s: &Snapshot) -> Metadata {
    let mut meta = Metadata::new();
    let Some(track) = s.now_playing() else { return meta };
    meta.set_title(Some(&track.title));
    if !track.artist.is_empty() {
        meta.set_artist(Some([&track.artist]));
    }
    if !track.album.is_empty() {
        meta.set_album(Some(&track.album));
    }
    meta.set_length(Some(Time::from_secs(track.duration_secs as i64)));
    // A track id is required to be a valid D-Bus object path, so the TIDAL id is put
    // in a path of ours rather than used raw.
    if let Ok(id) = TrackId::try_from(format!("/com/runnir/tidal/{}", track.id)) {
        meta.set_trackid(Some(id));
    }
    meta
}

/// Reports a bus problem once, to stderr. There is no toast from this thread, and a
/// terminal that prints the same line every two hundred milliseconds is worse than one
/// that says nothing.
fn log_once(msg: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SAID: AtomicBool = AtomicBool::new(false);
    if !SAID.swap(true, Ordering::Relaxed) {
        eprintln!("runnir: {msg}");
    }
}

impl RootInterface for Player {
    async fn raise(&self) -> fdo::Result<()> {
        Ok(())
    }

    /// Refused. "Quit" from a desktop widget would close the whole terminal, which is
    /// never what someone dismissing a music player meant.
    async fn quit(&self) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("runnir is a terminal, not a music player".into()))
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _: bool) -> mpris_server::zbus::Result<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    /// The window cannot be raised from here: runnir has no D-Bus side that could ask
    /// the compositor, and claiming otherwise leaves a widget button that does nothing.
    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok("runnir".into())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok("runnir".into())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl PlayerInterface for Player {
    async fn next(&self) -> fdo::Result<()> {
        self.send(Cmd::Next);
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.send(Cmd::Prev);
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        // The player has one toggle, so pausing something already paused would start
        // it: the state has to be checked rather than assumed.
        if !self.snapshot().paused {
            self.send(Cmd::Toggle);
        }
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.send(Cmd::Toggle);
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.send(Cmd::Stop);
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        let snapshot = self.snapshot();
        if !snapshot.playing || snapshot.paused {
            self.send(Cmd::Toggle);
        }
        Ok(())
    }

    /// Seeking is not built yet, and saying so is better than accepting the call and
    /// doing nothing — a progress bar that does not move after a drag looks broken.
    async fn seek(&self, _: Time) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("seeking is not implemented yet".into()))
    }

    async fn set_position(&self, _: TrackId, _: Time) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("seeking is not implemented yet".into()))
    }

    async fn open_uri(&self, _: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("runnir does not open URIs".into()))
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(status_of(&self.snapshot()))
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(LoopStatus::None)
    }

    async fn set_loop_status(&self, _: LoopStatus) -> mpris_server::zbus::Result<()> {
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _: PlaybackRate) -> mpris_server::zbus::Result<()> {
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_shuffle(&self, _: bool) -> mpris_server::zbus::Result<()> {
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(metadata_of(&self.snapshot()))
    }

    /// Always unity, and read-only. In bit-perfect mode there IS no volume to change —
    /// that is the whole promise — and a slider that silently does nothing is worse
    /// than one that is obviously fixed.
    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(1.0)
    }

    async fn set_volume(&self, _: Volume) -> mpris_server::zbus::Result<()> {
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_micros((self.snapshot().position_secs * 1_000_000.0) as i64))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(can_go_next(&self.snapshot()))
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        // Previous always does something once there is a queue: at the first track it
        // restarts it rather than refusing.
        Ok(!self.snapshot().queue.is_empty())
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(!self.snapshot().queue.is_empty())
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(self.snapshot().playing)
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tidal::Track;

    fn snapshot(playing: bool, paused: bool, queue: usize, index: usize) -> Snapshot {
        Snapshot {
            queue: (0..queue)
                .map(|i| Track {
                    id: i as u64,
                    title: format!("track {i}"),
                    artist: "Opeth".into(),
                    album: "Blackwater Park".into(),
                    duration_secs: 601,
                    quality: "LOSSLESS".into(),
                })
                .collect(),
            index,
            playing,
            paused,
            ..Default::default()
        }
    }

    #[test]
    fn the_three_states_map_to_the_three_mpris_ones() {
        assert_eq!(status_of(&snapshot(true, false, 1, 0)), PlaybackStatus::Playing);
        assert_eq!(status_of(&snapshot(true, true, 1, 0)), PlaybackStatus::Paused);
        // Stopped, not paused: a widget draws a play button for one and a resume for
        // the other, and they are not the same offer.
        assert_eq!(status_of(&snapshot(false, false, 1, 0)), PlaybackStatus::Stopped);
    }

    #[test]
    fn next_is_only_offered_when_there_is_a_next() {
        assert!(can_go_next(&snapshot(true, false, 3, 0)));
        assert!(!can_go_next(&snapshot(true, false, 3, 2)));
        assert!(!can_go_next(&snapshot(false, false, 0, 0)));
    }

    #[test]
    fn the_metadata_carries_what_a_widget_draws() {
        let meta = metadata_of(&snapshot(true, false, 2, 1));
        assert_eq!(meta.title(), Some("track 1"));
        assert_eq!(meta.album(), Some("Blackwater Park"));
        assert_eq!(meta.length(), Some(Time::from_secs(601)));
        // The track id has to be a valid object path — a bare number is not one, and a
        // widget that cannot parse it drops the whole metadata.
        assert_eq!(meta.trackid().map(|t| t.to_string()), Some("/com/runnir/tidal/1".into()));
    }

    #[test]
    fn nothing_playing_still_answers_with_empty_metadata_rather_than_failing() {
        let meta = metadata_of(&Snapshot::default());
        assert_eq!(meta.title(), None);
    }
}
