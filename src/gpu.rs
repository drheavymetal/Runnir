//! Which GPU the compositor renders on, so we can render on the same one.
//!
//! On a hybrid laptop the two GPUs are not interchangeable. A client that renders
//! on one and hands the buffer to a compositor running on the other needs a
//! cross-GPU dmabuf import, and that import is only as good as the two drivers'
//! agreement about tiling modifiers. When it breaks, the window goes black with no
//! error anywhere in the client: every frame is drawn, acquired and presented, and
//! the compositor samples nothing out of it. (Seen on an Alder Lake + RTX 4060
//! laptop whose Hyprland renders on the NVIDIA card: every Vulkan client that
//! picked the Intel GPU turned black, `vkcube` included.)
//!
//! The compositor names the GPU it wants clients on in `zwp_linux_dmabuf_v1`'s
//! default feedback, as `main_device`. This asks it, on winit's own connection
//! (see `dnd` for why a second event queue there is safe), and maps the `dev_t`
//! back through sysfs to the PCI ids `wgpu::AdapterInfo` reports, which is all we
//! need to recognise the same card among the adapters.

/// A GPU as `wgpu::AdapterInfo` identifies one: its PCI vendor and device ids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pci {
    pub vendor: u32,
    pub device: u32,
}

impl Pci {
    /// The discrete card on a hybrid laptop is the NVIDIA one, and its ICD is the
    /// one we otherwise hide from the Vulkan loader.
    pub fn is_nvidia(&self) -> bool {
        self.vendor == 0x10de
    }
}

/// Nothing to ask on a platform with no Wayland compositor to ask.
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn compositor_device(_window: &winit::window::Window) -> Option<Pci> {
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
pub use wayland::compositor_device;

#[cfg(all(unix, not(target_os = "macos")))]
mod wayland {
    use wayland_client::protocol::wl_registry::{Event as RegistryEvent, WlRegistry};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, backend::Backend};
    use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_feedback_v1::{
        Event as FeedbackEvent, ZwpLinuxDmabufFeedbackV1,
    };
    use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::{
        Event as DmabufEvent, ZwpLinuxDmabufV1,
    };

    /// `get_default_feedback` needs version 4 of the protocol; older compositors
    /// simply do not answer the question, and we fall back to the default adapter.
    const FEEDBACK_VERSION: u32 = 4;

    #[derive(Default)]
    struct State {
        dmabuf: Option<ZwpLinuxDmabufV1>,
        /// `dev_t` of the render node the compositor wants clients to use.
        main_device: Option<u64>,
        done: bool,
    }

    /// The compositor's render GPU, or `None` when it does not say (not Wayland, no
    /// `zwp_linux_dmabuf_v1` v4, or a device we cannot resolve in sysfs).
    pub fn compositor_device(window: &winit::window::Window) -> Option<super::Pci> {
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: the pointer comes from winit's live display handle, and the backend
        // takes a borrowed reference — it does not close a display it did not open.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        let _registry = conn.display().get_registry(&qh, ());

        let mut state = State::default();
        // First roundtrip: the globals, so we know whether the compositor has dmabuf
        // feedback at all.
        queue.roundtrip(&mut state).ok()?;
        let dmabuf = state.dmabuf.clone()?;

        let feedback = dmabuf.get_default_feedback(&qh, ());
        // The compositor sends the whole feedback block at once, so one roundtrip is
        // normally enough; a second covers a compositor that splits it across a flush.
        // Roundtrips, not `blocking_dispatch`: a compositor that answers nothing must
        // cost us a wait we know ends, not the whole startup.
        queue.roundtrip(&mut state).ok()?;
        if !state.done {
            queue.roundtrip(&mut state).ok()?;
        }
        feedback.destroy();
        dmabuf.destroy();

        pci_of(state.main_device?)
    }

    /// Resolves a DRM node's `dev_t` to the PCI ids of the card behind it.
    fn pci_of(dev: u64) -> Option<super::Pci> {
        let (major, minor) = (libc::major(dev), libc::minor(dev));
        let device = std::path::PathBuf::from(format!("/sys/dev/char/{major}:{minor}/device"));
        let id = |file: &str| -> Option<u32> {
            let raw = std::fs::read_to_string(device.join(file)).ok()?;
            u32::from_str_radix(raw.trim().trim_start_matches("0x"), 16).ok()
        };
        Some(super::Pci { vendor: id("vendor")?, device: id("device")? })
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
            if interface == ZwpLinuxDmabufV1::interface().name && version >= FEEDBACK_VERSION {
                state.dmabuf = Some(registry.bind(name, FEEDBACK_VERSION, qh, ()));
            }
        }
    }

    impl Dispatch<ZwpLinuxDmabufV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ZwpLinuxDmabufV1,
            _: DmabufEvent,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // Version 4 sends the old format/modifier events to nobody; the feedback
            // object carries everything we asked for.
        }
    }

    impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for State {
        fn event(
            state: &mut Self,
            _: &ZwpLinuxDmabufFeedbackV1,
            event: FeedbackEvent,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                // A `dev_t`, native-endian, in an array the protocol sizes for us.
                FeedbackEvent::MainDevice { device } => {
                    if let Ok(bytes) = <[u8; 8]>::try_from(device.as_slice()) {
                        state.main_device = Some(u64::from_ne_bytes(bytes));
                    }
                }
                FeedbackEvent::Done => state.done = true,
                _ => {}
            }
        }
    }
}
