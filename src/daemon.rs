//! The player as its own process, so the music belongs to the session and not to a
//! window.
//!
//! Every runnir window is a VIEW: it sends commands and reads a snapshot, and the sound
//! is made somewhere else. That buys three things at once. Closing the window that
//! started a song does not stop it. Two windows show the same thing rather than each
//! showing its own idea of it. And exactly one process holds the ALSA device, so the
//! exclusive path stops being a race between windows.
//!
//! The last window closing takes the daemon with it — Pedro's rule, and the right one:
//! a music player that outlives its UI is a process you find later in `ps` and wonder
//! about. There is no "keep playing in the background" mode and that is deliberate.
//!
//! ## Shape
//!
//! One Unix socket, one line of JSON per message, in `$XDG_RUNTIME_DIR`. A client
//! connection is held open for its whole life: commands go up it, and every change to
//! the player state comes back down it. That the connection IS the subscription is what
//! makes the shutdown rule free — when the last one closes, there is nobody left to
//! play for.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::Tidal as TidalCfg;
use crate::player::{Cmd, Jukebox, Snapshot};
use crate::tidal;

/// The socket every window looks for and at most one daemon owns.
///
/// One per user, not one per window: the whole point is that they meet at the same
/// place. `XDG_RUNTIME_DIR` is already 0700 per-user, which is the same trust boundary
/// the control socket relies on.
pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("runnir-player.sock")
}

/// How long a window waits for a daemon it has just started.
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the daemon looks at its own state to decide whether anything changed.
const POLL: std::time::Duration = std::time::Duration::from_millis(80);

// ---- the daemon ------------------------------------------------------------

/// Runs the player until the last window goes away. Never returns in the normal case.
pub fn main(cfg: TidalCfg, creds: tidal::Creds) {
    let path = socket_path();
    // A socket file left by a daemon that died is not a daemon. Binding fails on an
    // existing path whether or not anything is listening, so the difference has to be
    // established by CONNECTING, not by looking.
    if UnixStream::connect(&path).is_ok() {
        eprintln!("runnir: a player daemon is already running");
        return;
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => return eprintln!("runnir: cannot listen on {}: {e}", path.display()),
    };
    // Owner only. The runtime directory is already 0700, so this is defence in depth
    // rather than the boundary itself — but a socket that accepts commands and hands
    // back what somebody is listening to should not be readable by anything that
    // happens to get inside that directory. The control socket does the same.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    // Nothing wakes a UI here: the daemon has none. Clients are told about changes by
    // the writer loop on their own connection.
    let jukebox = Arc::new(Jukebox::start(cfg, creds, Box::new(|| {})));
    let clients = Arc::new(AtomicUsize::new(0));
    // Set once a window has actually connected, so a daemon started a moment too early
    // does not decide it has been abandoned before anyone arrives.
    let seen_anyone = Arc::new(AtomicUsize::new(0));

    {
        // The shutdown watcher. It owns the decision so that no single disconnecting
        // client has to know whether it was the last.
        let clients = clients.clone();
        let seen_anyone = seen_anyone.clone();
        let jukebox = jukebox.clone();
        let path = path.clone();
        std::thread::Builder::new()
            .name("runnir-daemon-life".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                loop {
                    std::thread::sleep(POLL);
                    let alive = clients.load(Ordering::Relaxed);
                    let arrived = seen_anyone.load(Ordering::Relaxed) > 0;
                    // Nobody yet, and nobody ever: a daemon started by a window that
                    // then failed to start must not sit here forever.
                    if !arrived && started.elapsed() > START_TIMEOUT {
                        break;
                    }
                    if arrived && alive == 0 {
                        break;
                    }
                }
                jukebox.send(Cmd::Quit);
                // Give the player a moment to close the device rather than having it
                // yanked: a card released cleanly is one the next application does not
                // have to recover.
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = std::fs::remove_file(&path);
                std::process::exit(0);
            })
            .ok();
    }

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        clients.fetch_add(1, Ordering::Relaxed);
        seen_anyone.fetch_add(1, Ordering::Relaxed);
        let jukebox = jukebox.clone();
        let clients = clients.clone();
        std::thread::Builder::new()
            .name("runnir-daemon-client".into())
            .spawn(move || {
                serve(stream, &jukebox);
                clients.fetch_sub(1, Ordering::Relaxed);
            })
            .ok();
    }
}

/// One window, for as long as it is open.
fn serve(stream: UnixStream, jukebox: &Jukebox) {
    let Ok(write_half) = stream.try_clone() else { return };

    // The writer: pushes a snapshot whenever it differs from the last one sent. Polled
    // rather than pushed at, so the player stays ignorant of who is watching — the same
    // reason it takes a `wake` closure rather than knowing about a window.
    let state = jukebox.snapshot();
    let sender = std::thread::spawn({
        let jukebox_state = jukebox.shared();
        move || {
            let mut out = write_half;
            let mut last = u64::MAX;
            let _ = state;
            loop {
                let snapshot = jukebox_state.lock().map(|s| s.clone()).unwrap_or_default();
                if snapshot.generation != last {
                    last = snapshot.generation;
                    let Ok(line) = serde_json::to_string(&snapshot) else { continue };
                    // A write failing is how a closed window announces itself; there is
                    // no other signal and none is needed.
                    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                        return;
                    }
                }
                std::thread::sleep(POLL);
            }
        }
    });

    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Cmd>(&line) {
            Ok(cmd) => jukebox.send(cmd),
            // A command this daemon does not understand comes from a newer window than
            // itself. Ignored rather than fatal: the answer is to restart the daemon,
            // not to take the music down mid-song.
            Err(e) => eprintln!("runnir: daemon ignored a command it could not read: {e}"),
        }
    }
    // The reader ended, so the window is gone. The writer thread will notice on its
    // next write; nothing here waits for it.
    drop(sender);
}

// ---- the client ------------------------------------------------------------

/// A window's end of the daemon: the same surface [`Jukebox`] offers, over a socket.
///
/// Deliberately identical, so the panel, the status bar and the close guard cannot tell
/// which they are talking to. That was the point of building the player behind a
/// channel and a snapshot in the first place.
pub struct Remote {
    stream: Mutex<UnixStream>,
    state: Arc<Mutex<Snapshot>>,
}

impl Remote {
    /// Connects, starting a daemon first if there is none.
    ///
    /// The race of two windows starting at once is settled by the daemon itself: the
    /// second one to bind finds the socket answering and exits, and both windows then
    /// connect to the first.
    pub fn connect(
        cfg: &TidalCfg,
        wake: Box<dyn Fn() + Send>,
    ) -> Result<Remote, String> {
        let path = socket_path();
        let stream = match UnixStream::connect(&path) {
            Ok(s) => s,
            Err(_) => {
                spawn_daemon()?;
                wait_for_socket(&path)?
            }
        };
        let _ = cfg;
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let shared = state.clone();
        std::thread::Builder::new()
            .name("runnir-player-client".into())
            .spawn(move || {
                for line in BufReader::new(reader).lines() {
                    let Ok(line) = line else { break };
                    let Ok(snapshot) = serde_json::from_str::<Snapshot>(&line) else { continue };
                    if let Ok(mut s) = shared.lock() {
                        *s = snapshot;
                    }
                    wake();
                }
                // The daemon went away. The window keeps its last snapshot rather than
                // blanking: what was playing a second ago is closer to the truth than
                // an empty panel, and the next command will report the real failure.
            })
            .ok();
        Ok(Remote { stream: Mutex::new(stream), state })
    }

    pub fn send(&self, cmd: Cmd) {
        let Ok(line) = serde_json::to_string(&cmd) else { return };
        if let Ok(mut stream) = self.stream.lock() {
            let _ = writeln!(stream, "{line}");
            let _ = stream.flush();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn is_playing(&self) -> bool {
        self.state.lock().map(|s| s.playing && !s.paused).unwrap_or(false)
    }
}

/// Where the daemon's own complaints go.
///
/// NOT `/dev/null`, which is what it was first: a player that dies quietly in another
/// process leaves nothing to read, and the first real bug in it cost an hour of
/// guessing before this file existed. Truncated at each start so it stays small and
/// always describes the run you are actually looking at.
pub fn log_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("runnir-player.log")
}

/// Starts a daemon from this same binary, detached from this window's lifetime.
fn spawn_daemon() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find my own binary: {e}"))?;
    let log = std::fs::File::create(log_path()).ok();
    let (out, err) = match log {
        Some(f) => match f.try_clone() {
            Ok(second) => (std::process::Stdio::from(f), std::process::Stdio::from(second)),
            Err(_) => (std::process::Stdio::null(), std::process::Stdio::null()),
        },
        None => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    std::process::Command::new(exe)
        .arg("--player-daemon")
        // Its output must not land in whatever pane happened to start it.
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start the player: {e}"))
}

fn wait_for_socket(path: &std::path::Path) -> Result<UnixStream, String> {
    let deadline = std::time::Instant::now() + START_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Ok(stream) = UnixStream::connect(path) {
            return Ok(stream);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err("the player did not start".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_one_per_user_and_not_one_per_window() {
        // The control socket carries a pid because every window has its own; this one
        // must NOT, because the whole point is that they all meet at the same place.
        let path = socket_path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, "runnir-player.sock");
        assert!(!name.contains(&std::process::id().to_string()));
    }

    #[test]
    fn the_log_sits_beside_the_socket_and_not_in_dev_null() {
        // The daemon's stderr went to /dev/null at first, and the first real bug in it
        // cost an hour of guessing. It has a file now, next to the socket.
        let log = log_path();
        assert_eq!(log.file_name().unwrap(), "runnir-player.log");
        assert_eq!(log.parent(), socket_path().parent());
    }

    #[test]
    fn a_snapshot_survives_the_round_trip_as_json() {
        // The wire format IS the type, so a field added without thought would break
        // every window talking to an older daemon. This is the test that notices.
        let snapshot = Snapshot {
            queue: vec![crate::tidal::Track {
                id: 7,
                title: "Bleak".into(),
                artist: "Opeth".into(),
                album: "Blackwater Park".into(),
                duration_secs: 546,
                quality: "LOSSLESS".into(),
            }],
            index: 0,
            playing: true,
            paused: false,
            position_secs: 12.5,
            generation: 3,
            wave: vec![0.1, 0.9],
            ..Default::default()
        };
        let line = serde_json::to_string(&snapshot).unwrap();
        let back: Snapshot = serde_json::from_str(&line).unwrap();
        assert_eq!(back.queue.len(), 1);
        assert_eq!(back.queue[0].title, "Bleak");
        assert!(back.playing);
        assert_eq!(back.position_secs, 12.5);
        assert_eq!(back.wave, vec![0.1, 0.9]);
    }

    #[test]
    fn every_command_survives_the_round_trip_too() {
        for cmd in [
            Cmd::Toggle,
            Cmd::Next,
            Cmd::Prev,
            Cmd::Stop,
            Cmd::Quit,
            Cmd::Enqueue(crate::tidal::Track { id: 1, ..Default::default() }),
            Cmd::Play { tracks: vec![crate::tidal::Track::default()], at: 0 },
        ] {
            let line = serde_json::to_string(&cmd).expect("serialisable");
            let back: Cmd = serde_json::from_str(&line).expect("readable");
            assert_eq!(
                std::mem::discriminant(&cmd),
                std::mem::discriminant(&back),
                "{line} came back as a different command"
            );
        }
    }
}
