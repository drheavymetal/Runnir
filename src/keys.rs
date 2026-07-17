use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Terminal modes that change what bytes a key produces.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyMode {
    /// DECCKM. Set by full-screen apps so arrows send SS3 instead of CSI.
    pub app_cursor: bool,
}

/// Translates a key press into the bytes a PTY expects, or `None` if the key
/// produces nothing.
pub fn encode(event: &KeyEvent, mods: ModifiersState, mode: KeyMode) -> Option<Vec<u8>> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    let shift = mods.shift_key();

    let bytes = match &event.logical_key {
        Key::Named(named) => named_key(*named, mods, mode)?,
        Key::Character(s) => {
            if ctrl {
                let c = s.chars().next()?.to_ascii_uppercase();
                // Ctrl+Shift+letter is a terminal shortcut namespace, not a control
                // code. If such a chord reaches here it was unbound; sending the C0
                // code anyway would leak e.g. Ctrl+Shift+Z -> 0x1A (SIGTSTP) to the
                // program. Only bare Ctrl+letter produces a control code.
                if shift && c.is_ascii_alphabetic() {
                    return None;
                }
                // Ctrl+key collapses to a C0 control code: @ABC.. -> 0x00,0x01,..
                match c {
                    '@'..='_' => vec![c as u8 - 0x40],
                    'a'..='z' => vec![c as u8 - 0x60],
                    '?' => vec![0x7f],
                    ' ' => vec![0x00],
                    _ => return None,
                }
            } else {
                s.as_bytes().to_vec()
            }
        }
        _ => {
            // Dead keys and IME output arrive here as committed text.
            let text = event.text.as_ref()?;
            text.as_bytes().to_vec()
        }
    };

    // Alt is the classic ESC prefix (xterm's metaSendsEscape).
    if alt && !bytes.is_empty() && !matches!(event.logical_key, Key::Named(_)) {
        let mut out = vec![0x1b];
        out.extend_from_slice(&bytes);
        return Some(out);
    }

    let _ = shift;
    Some(bytes)
}

fn named_key(key: NamedKey, mods: ModifiersState, mode: KeyMode) -> Option<Vec<u8>> {
    // Encodes the xterm modifier parameter: 1 + bit flags.
    let modifier = 1 + (mods.shift_key() as u8)
        + ((mods.alt_key() as u8) << 1)
        + ((mods.control_key() as u8) << 2);

    let b = |s: &str| Some(s.as_bytes().to_vec());

    // With any modifier held, cursor keys must use the CSI form with a parameter
    // even in application mode.
    let cursor = |final_byte: char| -> Option<Vec<u8>> {
        if modifier > 1 {
            Some(format!("\x1b[1;{modifier}{final_byte}").into_bytes())
        } else if mode.app_cursor {
            Some(format!("\x1bO{final_byte}").into_bytes())
        } else {
            Some(format!("\x1b[{final_byte}").into_bytes())
        }
    };

    // CSI n ~ keys (Home/End/Insert/Delete/PgUp/PgDn/F5+).
    let tilde = |n: u8| -> Option<Vec<u8>> {
        if modifier > 1 {
            Some(format!("\x1b[{n};{modifier}~").into_bytes())
        } else {
            Some(format!("\x1b[{n}~").into_bytes())
        }
    };

    match key {
        NamedKey::Enter => b("\r"),
        NamedKey::Backspace => {
            // Plain Backspace: DEL, not BS. Every Unix terminal has done this since
            // the VT220, and erase=^? in termios depends on it. Ctrl+Backspace and
            // Alt+Backspace both send ESC-DEL, which readline/fish/zsh treat as
            // backward-kill-word — "delete the whole word".
            if mods.control_key() || mods.alt_key() {
                b("\x1b\x7f")
            } else {
                b("\x7f")
            }
        }
        NamedKey::Tab => {
            if mods.shift_key() {
                b("\x1b[Z")
            } else {
                b("\t")
            }
        }
        NamedKey::Escape => b("\x1b"),
        NamedKey::Space => b(" "),

        NamedKey::ArrowUp => cursor('A'),
        NamedKey::ArrowDown => cursor('B'),
        NamedKey::ArrowRight => cursor('C'),
        NamedKey::ArrowLeft => cursor('D'),
        NamedKey::Home => cursor('H'),
        NamedKey::End => cursor('F'),

        NamedKey::Insert => tilde(2),
        NamedKey::Delete => tilde(3),
        NamedKey::PageUp => tilde(5),
        NamedKey::PageDown => tilde(6),

        NamedKey::F1 => b("\x1bOP"),
        NamedKey::F2 => b("\x1bOQ"),
        NamedKey::F3 => b("\x1bOR"),
        NamedKey::F4 => b("\x1bOS"),
        NamedKey::F5 => tilde(15),
        NamedKey::F6 => tilde(17),
        NamedKey::F7 => tilde(18),
        NamedKey::F8 => tilde(19),
        NamedKey::F9 => tilde(20),
        NamedKey::F10 => tilde(21),
        NamedKey::F11 => tilde(23),
        NamedKey::F12 => tilde(24),

        _ => None,
    }
}
