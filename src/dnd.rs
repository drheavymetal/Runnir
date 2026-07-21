//! Drag-and-drop of files, on Wayland.
//!
//! winit 0.30 raises `WindowEvent::DroppedFile` on X11, macOS and Windows, but its
//! Wayland backend implements no drag-and-drop at all — and Wayland is where runnir
//! actually runs (Hyprland, GNOME). So this module speaks `wl_data_device` itself.
//!
//! It attaches to the connection winit already opened, found through the raw
//! display handle, and opens its own event queue on it. That is legal and cheap: a
//! Wayland event queue only receives events for the proxies created from it, so our
//! registry, seat and data device are entirely ours and winit never sees them.
//! Binding a second `wl_seat` does not disturb the first — seats are shared state
//! on the compositor, not a client-side claim.
//!
//! It runs on its own thread, blocking in `blocking_dispatch`, and reports drops to
//! the UI thread through the same `EventLoopProxy` the PTY and AI workers use.

use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wayland_client::protocol::wl_data_device::{Event as DeviceEvent, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_registry::{Event as RegistryEvent, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, backend::Backend};

use crate::UserEvent;

/// The MIME type file managers use for a dragged selection: one URI per line,
/// CRLF-separated, `#` comments allowed (RFC 2483).
const URI_LIST: &str = "text/uri-list";

/// Where the pointer was, in surface-logical coordinates, at the last motion. The
/// drop event itself carries no position, so the last motion is the drop point.
///
/// Packed into one atomic (two f32 bits) so the reader never sees an x from one
/// motion paired with a y from the next.
#[derive(Default)]
struct DropPoint(AtomicU64);

impl DropPoint {
    fn set(&self, x: f64, y: f64) {
        let packed = (((x as f32).to_bits() as u64) << 32) | (y as f32).to_bits() as u64;
        self.0.store(packed, Ordering::Relaxed);
    }

    fn get(&self) -> (f64, f64) {
        let packed = self.0.load(Ordering::Relaxed);
        let x = f32::from_bits((packed >> 32) as u32);
        let y = f32::from_bits(packed as u32);
        (x as f64, y as f64)
    }
}

struct State {
    /// Our window's surface, as a raw pointer, so an enter on some other surface
    /// (a tooltip, another window of ours) is ignored.
    surface: *mut std::ffi::c_void,
    seat: Option<WlSeat>,
    manager: Option<WlDataDeviceManager>,
    device: Option<WlDataDevice>,
    /// The offer currently hovering our surface, and whether it advertised a type
    /// we can read. `None` between drags.
    offer: Option<WlDataOffer>,
    over_us: bool,
    point: Arc<DropPoint>,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
}

// The surface pointer is only ever compared, never dereferenced.
unsafe impl Send for State {}

/// Starts the drag-and-drop listener. Returns without doing anything on a display
/// that is not Wayland; the winit path covers X11.
///
/// `display` and `surface` are the raw `wl_display` and `wl_surface` pointers from
/// winit's handles. Safety: they must outlive the process's use of the terminal,
/// which holds for winit's window (dropped only at exit).
pub fn start(
    display: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
) {
    let display = display as usize;
    let surface = surface as usize;
    std::thread::Builder::new()
        .name("runnir-dnd".into())
        .spawn(move || run(display as *mut _, surface as *mut _, proxy))
        .ok();
}

fn run(
    display: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
) {
    // SAFETY: the pointer comes from winit's live display handle, and the backend
    // takes a borrowed reference — it does not close a display it did not open.
    let backend = unsafe { Backend::from_foreign_display(display.cast()) };
    let conn = Connection::from_backend(backend);
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = State {
        surface,
        seat: None,
        manager: None,
        device: None,
        offer: None,
        over_us: false,
        point: Arc::new(DropPoint::default()),
        proxy,
    };

    // Two roundtrips: the first delivers the globals, the second whatever they
    // advertise, after which the data device can be created.
    for _ in 0..2 {
        if queue.roundtrip(&mut state).is_err() {
            return;
        }
        state.ensure_device(&qh);
    }
    if state.device.is_none() {
        // No seat or no data-device manager: nothing to listen to. Not an error —
        // a headless or unusual compositor is allowed to lack them.
        return;
    }

    while queue.blocking_dispatch(&mut state).is_ok() {}
}

impl State {
    fn ensure_device(&mut self, qh: &QueueHandle<Self>) {
        if self.device.is_some() {
            return;
        }
        if let (Some(seat), Some(manager)) = (&self.seat, &self.manager) {
            self.device = Some(manager.get_data_device(seat, qh, ()));
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: RegistryEvent,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let RegistryEvent::Global { name, interface, version } = event else { return };
        match interface.as_str() {
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind(name, version.min(5), qh, ()));
            }
            "wl_data_device_manager" if state.manager.is_none() => {
                state.manager = Some(registry.bind(name, version.min(3), qh, ()));
            }
            _ => {}
        }
        state.ensure_device(qh);
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: DeviceEvent,
        _: &(),
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            DeviceEvent::Enter { serial, surface, x, y, id } => {
                // Only our own window. `id` is None for a drag with no data.
                state.over_us = surface.id().as_ptr() as *mut std::ffi::c_void == state.surface;
                state.offer = id;
                state.point.set(x, y);
                if let Some(offer) = &state.offer {
                    if state.over_us {
                        // Accepting is what turns the source's cursor into "this
                        // will work"; a compositor may refuse the drop otherwise.
                        offer.accept(serial, Some(URI_LIST.into()));
                    } else {
                        offer.accept(serial, None);
                    }
                }
            }
            DeviceEvent::Motion { x, y, .. } => state.point.set(x, y),
            DeviceEvent::Leave => {
                if let Some(offer) = state.offer.take() {
                    offer.destroy();
                }
                state.over_us = false;
            }
            DeviceEvent::Drop => {
                let Some(offer) = state.offer.take() else { return };
                if !state.over_us {
                    offer.destroy();
                    return;
                }
                let paths = read_uri_list(&offer, conn);
                // `finish` tells the source the drag is done so it can clean up;
                // it is only valid on a v3 manager, and only after a real drop.
                if offer.version() >= 3 {
                    offer.finish();
                }
                offer.destroy();
                if !paths.is_empty() {
                    let (x, y) = state.point.get();
                    let _ = state.proxy.send_event(UserEvent::FilesDropped(paths, x, y));
                }
            }
            _ => {}
        }
    }

    // `wl_data_device.data_offer` creates a new object, and wayland-client makes
    // the client say which type and user-data it gets. Without this the default
    // implementation panics — inside a C callback, so it aborts the process rather
    // than unwinding. It fired the instant the compositor announced the current
    // clipboard selection, which is why the window opened and died.
    wayland_client::event_created_child!(State, WlDataDevice, [
        wayland_client::protocol::wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

/// Reads the offer's `text/uri-list` payload and turns it into local paths.
///
/// The compositor hands the data over a pipe we supply: send the write end, flush
/// so the source sees the request, drop our copy of it (otherwise the read never
/// sees EOF, because *we* still hold a writer), then read to the end.
fn read_uri_list(offer: &WlDataOffer, conn: &Connection) -> Vec<PathBuf> {
    let Ok((reader, writer)) = pipe() else { return Vec::new() };
    offer.receive(URI_LIST.into(), writer.as_fd());
    if conn.flush().is_err() {
        return Vec::new();
    }
    drop(writer);

    let mut buf = Vec::new();
    let mut file = std::fs::File::from(reader);
    use std::io::Read;
    if file.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    parse_uri_list(&String::from_utf8_lossy(&buf))
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid two-element array for the duration of the call.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both descriptors are freshly created and owned by us.
    unsafe {
        use std::os::fd::FromRawFd;
        Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])))
    }
}

/// Turns an RFC 2483 uri-list into the local paths it names.
///
/// Only `file://` URIs become paths: an `https://` drag from a browser is a URL,
/// not a file, and typing it as if it were one would be a lie. Percent-escapes are
/// decoded, which is what makes a dragged `my report.txt` (sent as `my%20report`)
/// arrive with its real name.
fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        // Strip the (optional, usually empty) host component: file://host/path.
        .map(|rest| match rest.find('/') {
            Some(i) => &rest[i..],
            None => rest,
        })
        .map(|p| PathBuf::from(percent_decode(p)))
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // A `%` not followed by two hex digits is a literal `%`, not an error: the
        // name stays readable instead of the path being dropped entirely.
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// The seat, manager and offer carry no state of their own for us.
wayland_client::delegate_noop!(State: ignore WlSeat);
wayland_client::delegate_noop!(State: ignore WlDataDeviceManager);
wayland_client::delegate_noop!(State: ignore WlDataOffer);

#[cfg(test)]
mod tests {
    use super::{parse_uri_list, percent_decode};
    use std::path::PathBuf;

    #[test]
    fn a_single_file_uri_becomes_a_path() {
        assert_eq!(
            parse_uri_list("file:///home/pedro/notes.md\r\n"),
            vec![PathBuf::from("/home/pedro/notes.md")]
        );
    }

    #[test]
    fn several_files_keep_their_order() {
        let list = "file:///tmp/a.txt\r\nfile:///tmp/b.txt\r\n";
        assert_eq!(
            parse_uri_list(list),
            vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
        );
    }

    #[test]
    fn escapes_are_decoded_so_the_name_is_the_real_one() {
        assert_eq!(
            parse_uri_list("file:///tmp/my%20report%20(final).txt"),
            vec![PathBuf::from("/tmp/my report (final).txt")]
        );
        // UTF-8 arrives as a run of escaped bytes, not one escape per character.
        assert_eq!(percent_decode("/tmp/informe-a%C3%B1o.txt"), "/tmp/informe-año.txt");
    }

    #[test]
    fn comments_blank_lines_and_non_file_uris_are_dropped() {
        let list = "# comment\r\n\r\nhttps://example.com/x\r\nfile:///tmp/ok\r\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/tmp/ok")]);
    }

    #[test]
    fn a_host_component_is_not_part_of_the_path() {
        assert_eq!(parse_uri_list("file://localhost/tmp/x"), vec![PathBuf::from("/tmp/x")]);
    }

    #[test]
    fn a_lone_percent_stays_literal_rather_than_eating_the_name() {
        assert_eq!(percent_decode("/tmp/100%"), "/tmp/100%");
        assert_eq!(percent_decode("/tmp/a%zz"), "/tmp/a%zz");
    }
}
