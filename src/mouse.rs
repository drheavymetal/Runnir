//! Encoding mouse events for programs that request mouse tracking.
//!
//! When a full-screen app (vim, tmux, htop) turns on mouse mode, clicks and drags
//! must reach it as escape sequences rather than driving runnir's own selection.
//! Holding Shift always overrides this, so text selection stays available even
//! inside such an app — the same convention every terminal uses.

use crate::grid::MouseMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    Move,
}

/// Encodes a mouse event as the bytes the program expects, or `None` if this event
/// should not be forwarded in the current mode.
///
/// `col`/`row` are zero-based cell coordinates. SGR encoding (DECSET 1006) is used
/// when the app asked for it — it has no 223-column limit and reports releases
/// distinctly, so it is what modern apps want; the legacy X10 form is the
/// fallback.
pub fn encode(
    mode: MouseMode,
    sgr: bool,
    button: Button,
    kind: Kind,
    col: usize,
    row: usize,
) -> Option<Vec<u8>> {
    if mode == MouseMode::Off {
        return None;
    }
    // Motion is only reported in drag/motion modes, and drag mode only while a
    // button is down (the caller passes Move only then).
    if kind == Kind::Move && mode == MouseMode::Click {
        return None;
    }

    // The low button bits, plus the motion flag (32) for a move.
    let base = match button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
        Button::WheelUp => 64,
        Button::WheelDown => 65,
    };
    let cb = base + if kind == Kind::Move { 32 } else { 0 };

    if sgr {
        // CSI < b ; x ; y (M press | m release), 1-based coordinates.
        let final_char = if kind == Kind::Release { 'm' } else { 'M' };
        Some(format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, final_char).into_bytes())
    } else {
        // Legacy X10: CSI M then three bytes, each offset by 32. Coordinates above
        // 223 cannot be encoded and are clamped, which is the historical behaviour.
        let b = if kind == Kind::Release { 3 } else { cb } as u32;
        let enc = |v: usize| (32 + 1 + v.min(222)) as u8;
        Some(vec![0x1b, b'[', b'M', 32 + b as u8, enc(col), enc(row)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_press_and_release_differ() {
        let press = encode(MouseMode::Click, true, Button::Left, Kind::Press, 4, 2).unwrap();
        assert_eq!(press, b"\x1b[<0;5;3M");
        let release = encode(MouseMode::Click, true, Button::Left, Kind::Release, 4, 2).unwrap();
        assert_eq!(release, b"\x1b[<0;5;3m", "release ends with lowercase m");
    }

    #[test]
    fn wheel_encodes_as_buttons_64_65() {
        let up = encode(MouseMode::Click, true, Button::WheelUp, Kind::Press, 0, 0).unwrap();
        assert_eq!(up, b"\x1b[<64;1;1M");
        let down = encode(MouseMode::Click, true, Button::WheelDown, Kind::Press, 0, 0).unwrap();
        assert_eq!(down, b"\x1b[<65;1;1M");
    }

    #[test]
    fn motion_is_suppressed_in_click_mode() {
        assert!(encode(MouseMode::Click, true, Button::Left, Kind::Move, 1, 1).is_none());
        assert!(encode(MouseMode::Drag, true, Button::Left, Kind::Move, 1, 1).is_some());
    }

    #[test]
    fn off_mode_forwards_nothing() {
        assert!(encode(MouseMode::Off, true, Button::Left, Kind::Press, 0, 0).is_none());
    }

    #[test]
    fn legacy_encoding_offsets_by_33() {
        // X10: coordinates are value + 1 + 32.
        let e = encode(MouseMode::Click, false, Button::Left, Kind::Press, 0, 0).unwrap();
        assert_eq!(e, vec![0x1b, b'[', b'M', 32, 33, 33]);
    }
}
