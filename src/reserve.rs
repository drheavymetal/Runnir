//! Asking PipeWire for the sound card, politely, before taking it.
//!
//! A card that PipeWire is holding cannot be opened exclusively, and PipeWire holds
//! every card it manages — even an idle one with nothing playing through it. So the
//! bit-perfect path never got a device on a normal desktop: every exclusive attempt
//! came back `EBUSY` and the music went out through the mixer instead.
//!
//! `pactl suspend-sink` does NOT help. It stops the sink but keeps the file
//! descriptor, which is exactly as busy as before.
//!
//! What does work is the Device Reservation protocol: a D-Bus name per card,
//! `org.freedesktop.ReserveDevice1.Audio<N>`, owned by WirePlumber. Taking that name
//! from it — with `ReplaceExisting`, which it allows — makes it close the card and stay
//! off it. Dropping the name gives it back. It is the mechanism JACK has used for this
//! for years, and PipeWire implements the other side of it precisely so that an
//! application wanting the hardware to itself can say so rather than fight for it.
//!
//! Nothing here is required for playback: without a reservation the chain still works,
//! it just lands a rung lower and says so.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

/// Set when a signal has arrived and the card has to be given back before exiting.
///
/// Read by whatever is feeding a device, which is the only thing that can close it.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// How many ALSA devices are open right now. The signal thread waits on this: it is the
/// difference between exiting with the card still held and exiting after it is free.
static OPEN_DEVICES: AtomicUsize = AtomicUsize::new(0);

/// Which signal arrived, so a command that stopped because of one can exit saying so.
static SIGNAL: AtomicI32 = AtomicI32::new(0);

/// The signal that stopped this process, if one did.
///
/// Needed because the fix works TOO well: the playback loop now notices the signal and
/// returns normally, so the process exits 0 and a caller cannot tell an interrupted run
/// from a finished one. Before the fix it died of the signal and reported 130.
pub fn signalled() -> Option<i32> {
    match SIGNAL.load(Ordering::Relaxed) {
        0 => None,
        n => Some(n),
    }
}

/// True once a signal has asked this process to stop. A playback loop that keeps going
/// after this is a playback loop that will be killed mid-buffer, taking the card with it.
pub fn shutting_down() -> bool {
    SHUTDOWN.load(Ordering::Relaxed)
}

pub fn device_opened() {
    OPEN_DEVICES.fetch_add(1, Ordering::SeqCst);
}

pub fn device_closed() {
    OPEN_DEVICES.fetch_sub(1, Ordering::SeqCst);
}

/// Catches the signals that would otherwise kill this process with a card still in it.
///
/// **Why this exists.** A signalled process runs no destructors. The PCM handle and the
/// D-Bus name are then released by the kernel at the same instant — and PipeWire, seeing
/// the name freed, goes to take a card back that ALSA has not finished letting go of.
/// It fails, and leaves the card at `Active Profile: off`: gone from the desktop's sound
/// settings, with the profile it was using no longer even offered, until wireplumber is
/// restarted. Measured, both ways, on a Focusrite Scarlett 2i2:
///
///     clean exit (through the drops)   Active Profile: HiFi   1 sink
///     SIGTERM                          Active Profile: off    0 sinks
///
/// The handler itself only writes a byte to a pipe, which is one of the few things that
/// is safe inside one. A thread reads that byte and does the work: ask the playback loop
/// to stop, wait for the device to actually close, wait out ALSA's own release, and only
/// then exit — which is what finally drops the name.
///
/// Installed lazily, from `take`, so that a runnir which never reserves a card never
/// changes how it handles signals. The mask is untouched for the same reason: it is
/// inherited across fork and exec, and a blocked SIGTERM would quietly break the
/// `PR_SET_PDEATHSIG` that stops a daemon's children outliving it.
#[cfg(unix)]
fn install_guard_once() {
    use std::os::fd::RawFd;
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    static mut WRITE_FD: RawFd = -1;

    extern "C" fn on_signal(sig: libc::c_int) {
        // Async-signal-safe: one `write` of one byte, and nothing else. No allocation,
        // no locks, no formatting.
        let byte = sig as u8;
        unsafe {
            let fd = std::ptr::addr_of!(WRITE_FD).read_volatile();
            if fd >= 0 {
                libc::write(fd, std::ptr::addr_of!(byte).cast(), 1);
            }
        }
    }

    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a two-element array, which is what pipe2 writes into.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        eprintln!("runnir: could not arm the device-release guard; a kill will leave the card off");
        return;
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);
    // SAFETY: single-threaded initialisation, before any handler can run.
    unsafe { std::ptr::addr_of_mut!(WRITE_FD).write_volatile(write_fd) };

    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `action` is zeroed and then filled as sigaction expects.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }

    std::thread::Builder::new()
        .name("runnir-release".to_string())
        .spawn(move || {
            let mut byte = 0u8;
            // SAFETY: reading one byte into a byte.
            let n = unsafe { libc::read(read_fd, std::ptr::addr_of_mut!(byte).cast(), 1) };
            if n != 1 {
                return;
            }
            SIGNAL.store(byte as i32, Ordering::SeqCst);
            SHUTDOWN.store(true, Ordering::SeqCst);

            // Give the loop feeding the device a chance to close it. Two seconds is far
            // more than a loop that checks between packets needs, and a loop that is
            // wedged is not going to be helped by waiting longer.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while OPEN_DEVICES.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // ALSA frees a card an instant AFTER the handle is closed, and the whole
            // point of this thread is not to release the name inside that instant.
            std::thread::sleep(RELEASE_GRACE);
            std::process::exit(128 + byte as i32);
        })
        .ok();
}

#[cfg(not(unix))]
fn install_guard_once() {}

/// A held reservation. The card is ours until this is dropped.
pub struct Reservation {
    card: u32,
    /// Kept alive: releasing the connection releases the name, and releasing the name
    /// is how PipeWire learns it may have the card back.
    _connection: zbus::blocking::Connection,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        // Nothing to do by hand — the connection closing drops the name — but say it
        // once so the log of a session that took a card also shows it giving it back.
        log_once(&format!("released card {} back to the session", self.card));
    }
}

/// The object the protocol expects the holder to export.
///
/// Taking the bus name is only half of it: whoever holds a card is supposed to be
/// reachable, so the previous owner can ask for it back and so anything looking at the
/// bus can see who has it and why. Without this object WirePlumber releases the card
/// when the name is taken and then never reclaims it — the device simply vanishes from
/// the desktop until the session is restarted, which is a far worse bug than the one
/// the reservation set out to fix.
struct Held {
    application_name: String,
}

#[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
impl Held {
    /// Whether we will hand the card over to somebody who wants it more.
    ///
    /// Always yes. A terminal that is playing music has no business keeping a sound
    /// card from an application the person actually reached for, and refusing would
    /// leave them with a device nothing else can open and no way to see why.
    fn request_release(&self, _priority: i32) -> bool {
        true
    }

    #[zbus(property)]
    fn priority(&self) -> i32 {
        // The value the protocol suggests for an ordinary application.
        0
    }

    #[zbus(property)]
    fn application_name(&self) -> String {
        self.application_name.clone()
    }

    #[zbus(property)]
    fn application_device_name(&self) -> String {
        "runnir".to_string()
    }
}

/// Takes the reservation for a card, or explains why not.
///
/// `Ok(None)` means there was nobody to take it from: no session bus, or a card
/// PipeWire does not manage. That is not a failure — the open can simply go ahead.
pub fn take(card: u32) -> Result<Option<Reservation>, String> {
    use zbus::fdo::RequestNameFlags;
    use zbus::names::WellKnownName;

    let name = format!("org.freedesktop.ReserveDevice1.Audio{card}");
    let Ok(well_known) = WellKnownName::try_from(name.clone()) else {
        return Err(format!("{name} is not a valid bus name"));
    };
    let connection = match zbus::blocking::Connection::session() {
        Ok(c) => c,
        // No session bus at all: nothing is holding the card through PipeWire either.
        Err(e) => {
            log_once(&format!("no session bus, not reserving devices ({e})"));
            return Ok(None);
        }
    };

    // `ReplaceExisting` is the whole point: WirePlumber owns this name and allows
    // replacement, and losing it is its signal to close the card. `AllowReplacement`
    // is the other half of the bargain — somebody else wanting the card can take it
    // from us the same way, rather than being stuck behind a terminal.
    // The object goes up BEFORE the name is requested. Between taking the name and
    // exporting the object there is a moment when we own a card and cannot be asked
    // about it, and the previous owner is looking in exactly that moment.
    let path = format!("/org/freedesktop/ReserveDevice1/Audio{card}");
    let held = Held { application_name: "runnir".to_string() };
    if let Err(e) = connection.object_server().at(path.as_str(), held) {
        return Err(format!("could not offer the reservation object: {e}"));
    }

    let flags = RequestNameFlags::ReplaceExisting
        | RequestNameFlags::AllowReplacement
        | RequestNameFlags::DoNotQueue;
    match connection.request_name_with_flags(well_known, flags.into()) {
        Ok(_) => {
            // From here on, dying without running destructors costs the user their sound
            // card until they restart wireplumber. Armed now rather than at startup, so
            // that a runnir which never reserves anything never changes how it handles
            // signals.
            install_guard_once();
            // PipeWire closes the device when it loses the name, but it does that on
            // its own thread and not instantly. Without this pause the open that
            // follows still finds the card busy, which is the bug this exists to fix.
            std::thread::sleep(RELEASE_GRACE);
            Ok(Some(Reservation { card, _connection: connection }))
        }
        Err(e) => Err(format!("could not reserve card {card}: {e}")),
    }
}

/// How long PipeWire is given to actually let go after losing the name.
/// Measured, not guessed: at four hundred milliseconds the first open still found the
/// card busy and only the second attempt succeeded.
const RELEASE_GRACE: std::time::Duration = std::time::Duration::from_millis(900);

/// The card number in an ALSA device name: `hw:2,0` is card 2.
///
/// Also accepts the `hw:CARD=NAME` spelling by refusing it — a reservation is per
/// NUMBER, and guessing which number a name refers to would be a way to release
/// somebody else's card.
pub fn card_of(device: &str) -> Option<u32> {
    let rest = device.strip_prefix("hw:")?;
    let head = rest.split(',').next()?;
    head.parse().ok()
}

fn log_once(msg: &str) {
    static SAID: AtomicBool = AtomicBool::new(false);
    if !SAID.swap(true, Ordering::Relaxed) {
        eprintln!("runnir: {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_number_comes_from_the_device_name() {
        assert_eq!(card_of("hw:2,0"), Some(2));
        assert_eq!(card_of("hw:0,3"), Some(0));
        assert_eq!(card_of("hw:11"), Some(11));
        // Not a raw hardware device: nothing to reserve, and reserving on a guess
        // would mean telling PipeWire to drop a card we never asked for.
        assert_eq!(card_of("default"), None);
        assert_eq!(card_of("plughw:2,0"), None);
        // The named spelling is refused rather than guessed at: the protocol is keyed
        // on the number, and mapping a name to one is not this function's business.
        assert_eq!(card_of("hw:CARD=R4,DEV=0"), None);
    }
}
