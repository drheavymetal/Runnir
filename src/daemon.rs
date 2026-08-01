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

/// The per-user runtime directory, or nothing.
///
/// Falling back to `/tmp` was the whole trust boundary quietly disappearing: it is
/// world-writable, so another user could pre-create the socket, our bind would fail,
/// and the WINDOW would connect to theirs — sending them every command and drawing
/// whatever they sent back, including a share URL the person is invited to hand out.
/// With no runtime directory there is nowhere safe to put this, so there is no player.
fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

/// The socket every window looks for and at most one daemon owns.
///
/// One per user, not one per window: the whole point is that they meet at the same
/// place. `XDG_RUNTIME_DIR` is already 0700 per-user, which is the same trust boundary
/// the control socket relies on.
pub fn socket_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("runnir-player.sock"))
}

/// Takes the single-daemon lock, or returns nothing if somebody else holds it.
///
/// `flock` and not a pidfile: the kernel releases it however the process dies, so there
/// is no stale lock to reason about and no pid to race against being reused.
fn take_the_lock(path: &std::path::Path) -> Option<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    // SAFETY: a plain flock on a file descriptor we own.
    let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    taken.then_some(file)
}

/// How long a window waits for a daemon it has just started.
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the daemon looks at its own state to decide whether anything changed.
const POLL: std::time::Duration = std::time::Duration::from_millis(80);

/// How long a client connection may go without a byte before one is sent to check that
/// it is still there.
const KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(10);

// ---- the daemon ------------------------------------------------------------

/// Runs the player until the last window goes away. Never returns in the normal case.
pub fn main(cfg: TidalCfg, creds: tidal::Creds) {
    let Some(path) = socket_path() else {
        return eprintln!("runnir: no XDG_RUNTIME_DIR, so there is nowhere safe for the player");
    };
    // Held for the life of the process. Without it, two windows starting at the same
    // instant both found no daemon, both unlinked, and both bound — two players
    // fighting for the exclusive device, and the second one deleting the first one's
    // socket on the way past. "Connect to see if anyone is there" cannot settle that
    // race on its own, because both sides look before either has bound.
    let Some(_lock) = take_the_lock(&path.with_extension("lock")) else {
        eprintln!("runnir: another player daemon holds the lock");
        return;
    };
    // With the lock held, a socket file here belongs to a daemon that died.
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
    // Set the moment the daemon decides to go. The accept loop refuses connections
    // after that, so a window opened during the last two hundred milliseconds sees a
    // closed socket and starts a fresh daemon — rather than connecting successfully to
    // a process about to vanish and then having a transport that silently does nothing
    // for ever.
    let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // One share for the machine, held by the daemon. A second window asking to share
    // finds it already on rather than opening a second tunnel.
    let share: Arc<Mutex<Option<crate::share::Share>>> = Arc::new(Mutex::new(None));
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
        let share_for_exit = share.clone();
        let closing = closing.clone();
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
                closing.store(true, Ordering::Relaxed);
                // The link goes first: a public URL still answering after the terminal
                // is gone is exactly what nobody expects.
                if let Ok(mut held) = share_for_exit.lock() {
                    if let Some(share) = held.take() {
                        share.stop();
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
        if closing.load(Ordering::Relaxed) {
            // Already on the way out. Dropping the connection is the honest answer;
            // accepting it would hand back a working handle to a dying process.
            drop(stream);
            continue;
        }
        clients.fetch_add(1, Ordering::Relaxed);
        seen_anyone.fetch_add(1, Ordering::Relaxed);
        let jukebox = jukebox.clone();
        let clients = clients.clone();
        let share = share.clone();
        let spawned = std::thread::Builder::new()
            .name("runnir-daemon-client".into())
            .spawn({
                let clients = clients.clone();
                move || {
                    serve(stream, &jukebox, &share);
                    clients.fetch_sub(1, Ordering::Relaxed);
                }
            });
        if spawned.is_err() {
            // The count was raised before the spawn. Leaving it raised on a failure
            // means the daemon can never decide it has no clients, so it would outlive
            // every window for ever.
            clients.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Starts or stops the public link, and records the result where every window sees it.
fn set_share(jukebox: &Jukebox, share: &Mutex<Option<crate::share::Share>>, on: bool) {
    let Ok(mut held) = share.lock() else { return };
    let publish = |state: Option<crate::share::State>| {
        if let Ok(mut s) = jukebox.shared().lock() {
            s.share = state;
            s.generation += 1;
        }
    };
    if !on {
        if let Some(share) = held.take() {
            share.stop();
        }
        publish(None);
        return;
    }
    if held.is_some() {
        return; // already sharing; asking twice is not an error
    }
    // Starting blocks for as long as cloudflared takes to publish a URL — up to
    // twenty-five seconds. It runs on this client's own thread, so the player keeps
    // playing and every other window keeps being served while it happens.
    match crate::share::Share::start(jukebox.shared()) {
        Ok(share) => {
            publish(Some(share.state()));
            *held = Some(share);
        }
        Err(e) => publish(Some(crate::share::State {
            error: Some(e),
            ..Default::default()
        })),
    }
}

/// One window, for as long as it is open.
fn serve(
    stream: UnixStream,
    jukebox: &Arc<Jukebox>,
    share: &Arc<Mutex<Option<crate::share::Share>>>,
) {
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
            let mut quiet = std::time::Instant::now();
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
                    quiet = std::time::Instant::now();
                } else if quiet.elapsed() > KEEPALIVE {
                    // Nothing has changed — the player is stopped or paused — so there
                    // has been nothing to write, and a thread that only discovers a
                    // dead socket by writing to it would loop here for the life of the
                    // daemon holding a thread and a file descriptor. An empty line is
                    // ignored by the reader and costs one byte.
                    if writeln!(out).is_err() || out.flush().is_err() {
                        return;
                    }
                    quiet = std::time::Instant::now();
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
            // The share is the daemon's, not the player's: it must outlive the window
            // that asked for it, and the player has no business knowing about tunnels.
            // On its own thread: starting one waits on cloudflared for up to
            // twenty-five seconds and then on the edge for twenty more, and this loop
            // is the only thing reading THIS window's commands. Doing it here made the
            // asking window deaf for the whole time — press share then play, and play
            // happened three quarters of a minute later.
            Ok(Cmd::Share(on)) => {
                let jukebox = jukebox.clone();
                let share = share.clone();
                std::thread::spawn(move || set_share(&jukebox, &share, on));
            }
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
        let path = socket_path().ok_or("no XDG_RUNTIME_DIR, so there is nowhere safe for the player")?;
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
pub fn log_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("runnir-player.log"))
}

/// Starts a daemon from this same binary, detached from this window's lifetime.
fn spawn_daemon() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find my own binary: {e}"))?;
    // No log rather than a log in a shared directory: `File::create` follows a symlink,
    // so a pre-planted one in a world-writable place would have this truncate whatever
    // it points at, as the user.
    let log = log_path().and_then(|p| std::fs::File::create(p).ok());
    let (out, err) = match log {
        Some(f) => match f.try_clone() {
            Ok(second) => (std::process::Stdio::from(f), std::process::Stdio::from(second)),
            Err(_) => (std::process::Stdio::null(), std::process::Stdio::null()),
        },
        None => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    let child = std::process::Command::new(exe)
        .arg("--player-daemon")
        // Its output must not land in whatever pane happened to start it.
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("cannot start the player: {e}"))?;
    // Reaped on a thread of its own. Dropping the handle leaves a zombie for every
    // attempt, and a window with broken credentials retries on every keypress.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
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
        let Some(path) = socket_path() else { return };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, "runnir-player.sock");
        assert!(!name.contains(&std::process::id().to_string()));
    }

    #[test]
    fn there_is_no_player_without_a_private_directory_to_put_it_in() {
        // Falling back to /tmp was the trust boundary disappearing: world-writable, so
        // another user pre-creates the socket, our bind fails, and the WINDOW connects
        // to theirs — sending them every command and drawing what they send back.
        let saved = std::env::var_os("XDG_RUNTIME_DIR");
        // SAFETY: single-threaded test, and the variable is put back below.
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        let without = socket_path();
        let log = log_path();
        if let Some(saved) = saved {
            unsafe { std::env::set_var("XDG_RUNTIME_DIR", saved) };
        }
        assert!(without.is_none(), "no runtime dir must mean no socket, not /tmp");
        assert!(log.is_none());
    }

    #[test]
    fn only_one_daemon_can_hold_the_lock() {
        // The race this settles: two windows opened at the same instant both found no
        // daemon, both unlinked, and both bound — two players fighting for the
        // exclusive device, the second deleting the first's socket on the way past.
        let dir = std::env::temp_dir().join(format!("runnir-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("player.lock");
        let first = take_the_lock(&path).expect("the first one takes it");
        assert!(take_the_lock(&path).is_none(), "the second must be turned away");
        drop(first);
        // Released with the file, however the holder went.
        assert!(take_the_lock(&path).is_some(), "and it is free again afterwards");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_log_sits_beside_the_socket_and_not_in_dev_null() {
        // The daemon's stderr went to /dev/null at first, and the first real bug in it
        // cost an hour of guessing. It has a file now, next to the socket.
        let Some(log) = log_path() else { return };
        assert_eq!(log.file_name().unwrap(), "runnir-player.log");
        assert_eq!(log.parent(), socket_path().as_deref().and_then(|p| p.parent()));
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
