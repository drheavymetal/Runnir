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
    // Dead keys and IME output arrive only as committed `text` on the event, so that
    // case stays here; everything a scripted key can produce goes through `encode_key`.
    if matches!(event.logical_key, Key::Unidentified(_) | Key::Dead(_)) {
        let text = event.text.as_ref()?;
        return Some(text.as_bytes().to_vec());
    }
    encode_key(&event.logical_key, mods, mode)
}

/// The same encoding from a bare `Key`, for callers that do not have a winit
/// `KeyEvent` — a scripted keypress cannot construct one (`platform_specific` is
/// private), and without this a scripted key could drive overlays but never type.
pub fn encode_key(key: &Key, mods: ModifiersState, mode: KeyMode) -> Option<Vec<u8>> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    let shift = mods.shift_key();

    let bytes = match key {
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
        // Anything else (dead keys, IME) only carries text on the full event, which
        // `encode` handles before delegating here.
        _ => return None,
    };

    // Alt is the classic ESC prefix (xterm's metaSendsEscape).
    if alt && !bytes.is_empty() && !matches!(key, Key::Named(_)) {
        let mut out = vec![0x1b];
        out.extend_from_slice(&bytes);
        return Some(out);
    }

    let _ = shift;
    Some(bytes)
}

// --- Kitty keyboard protocol (CSI u) -------------------------------------------
//
// Progressive-enhancement flag bits, as pushed by the app via `CSI > flags u`:
pub const KITTY_DISAMBIGUATE: u8 = 0b0_0001; // bit 0: disambiguate escape codes
pub const KITTY_REPORT_EVENTS: u8 = 0b0_0010; // bit 1: report press/repeat/release
pub const KITTY_REPORT_ALTERNATE: u8 = 0b0_0100; // bit 2: report alternate keys
pub const KITTY_REPORT_ALL: u8 = 0b0_1000; // bit 3: report all keys as escape codes
pub const KITTY_REPORT_TEXT: u8 = 0b1_0000; // bit 4: report associated text

/// How a key is terminated in the CSI-u form. `Unicode`/`Tilde` always carry their
/// number; `Legacy` keys (arrows, Home/End, F1-F4) default their number to 1 and
/// omit it when there is no modifier or event to report.
enum KittyKey {
    Unicode(u32),
    Tilde(u32),
    Legacy(char),
}

/// Encodes a key event under the kitty keyboard protocol. `flags` is the active
/// (top-of-stack) enhancement flag set — guaranteed non-zero by the caller, which
/// falls back to [`encode`] when it is 0. `released` is true for a key-up event.
///
/// Implemented enhancement bits:
/// - bit 0 (disambiguate) and bit 3 (report all keys as escape codes): fully.
/// - bit 1 (report event types): press/repeat/release, driven by winit ElementState
///   and `KeyEvent::repeat` (the caller only forwards releases when this bit is set).
/// - bit 2 (report alternate keys): the shifted alternate for letters.
/// - bit 4 (report associated text): the produced codepoint for printable keys.
pub fn encode_kitty(
    event: &KeyEvent,
    mods: ModifiersState,
    flags: u8,
    released: bool,
) -> Option<Vec<u8>> {
    encode_kitty_inner(&event.logical_key, event.text.as_deref(), event.repeat, mods, flags, released)
}

/// The kitty encoding from a bare `Key`, for the scripted path: a keypress arriving
/// down the control socket has no winit `KeyEvent` behind it, and no text or repeat
/// flag to carry. Without this, a scripted key sent the legacy bytes to a pane that
/// had asked for CSI-u — different bytes from the hand it stands in for.
pub fn encode_kitty_key(key: &Key, mods: ModifiersState, flags: u8) -> Option<Vec<u8>> {
    encode_kitty_inner(key, None, false, mods, flags, false)
}

/// Core of [`encode_kitty`], split out so it can be unit-tested without a winit
/// `KeyEvent` (whose platform field is not publicly constructible).
fn encode_kitty_inner(
    logical_key: &Key,
    key_text: Option<&str>,
    repeat: bool,
    mods: ModifiersState,
    flags: u8,
    released: bool,
) -> Option<Vec<u8>> {
    let shift = mods.shift_key();
    let alt = mods.alt_key();
    let ctrl = mods.control_key();
    let sup = mods.super_key();

    // Release and repeat only exist when the app asked for event types; otherwise a
    // release produces nothing and a repeat is indistinguishable from a fresh press.
    let report_events = flags & KITTY_REPORT_EVENTS != 0;
    if released && !report_events {
        return None;
    }
    let event_type: u8 = if released {
        3
    } else if repeat && report_events {
        2
    } else {
        1
    };

    // xterm-style modifier parameter: 1 + bitmask.
    let mod_bits = (shift as u8) | ((alt as u8) << 1) | ((ctrl as u8) << 2) | ((sup as u8) << 3);
    let mods_val = 1 + mod_bits;
    let force_csi = flags & KITTY_REPORT_ALL != 0;
    // Ctrl/Alt/Super turn a key into an escape code (plain Shift does not), but only
    // once the app has opted into disambiguation or into reporting all keys — that
    // is what makes e.g. Ctrl+I distinct from Tab instead of collapsing to a C0 byte.
    let escape_mods =
        (ctrl || alt || sup) && (flags & (KITTY_DISAMBIGUATE | KITTY_REPORT_ALL) != 0);

    // Resolve the key into a CSI-u shape, plus (for text keys) the base codepoint,
    // any shifted alternate, and the plain text it would otherwise produce.
    let mut alternate: Option<u32> = None;
    let mut assoc_text: Option<char> = None;
    let mut plain: Option<Vec<u8>> = None;

    let kk: KittyKey = match logical_key {
        Key::Named(named) => match named {
            NamedKey::Enter => KittyKey::Unicode(13),
            NamedKey::Tab => KittyKey::Unicode(9),
            NamedKey::Escape => KittyKey::Unicode(27),
            NamedKey::Backspace => KittyKey::Unicode(127),
            NamedKey::Space => {
                // Space is a text-producing key: like a letter it keeps its plain
                // text unless a forcing modifier or report-all sends it to CSI form.
                if !ctrl && !sup {
                    assoc_text = Some(' ');
                    plain = Some(b" ".to_vec());
                }
                KittyKey::Unicode(32)
            }
            NamedKey::ArrowUp => KittyKey::Legacy('A'),
            NamedKey::ArrowDown => KittyKey::Legacy('B'),
            NamedKey::ArrowRight => KittyKey::Legacy('C'),
            NamedKey::ArrowLeft => KittyKey::Legacy('D'),
            NamedKey::Home => KittyKey::Legacy('H'),
            NamedKey::End => KittyKey::Legacy('F'),
            NamedKey::F1 => KittyKey::Legacy('P'),
            NamedKey::F2 => KittyKey::Legacy('Q'),
            NamedKey::F3 => KittyKey::Legacy('R'),
            NamedKey::F4 => KittyKey::Legacy('S'),
            NamedKey::Insert => KittyKey::Tilde(2),
            NamedKey::Delete => KittyKey::Tilde(3),
            NamedKey::PageUp => KittyKey::Tilde(5),
            NamedKey::PageDown => KittyKey::Tilde(6),
            NamedKey::F5 => KittyKey::Tilde(15),
            NamedKey::F6 => KittyKey::Tilde(17),
            NamedKey::F7 => KittyKey::Tilde(18),
            NamedKey::F8 => KittyKey::Tilde(19),
            NamedKey::F9 => KittyKey::Tilde(20),
            NamedKey::F10 => KittyKey::Tilde(21),
            NamedKey::F11 => KittyKey::Tilde(23),
            NamedKey::F12 => KittyKey::Tilde(24),
            // xterm's ~-scheme carries on to F20. Past that there is no agreed
            // legacy encoding, so F21-F24 stay bindable in runnir and send nothing
            // to the child rather than inventing bytes a program would misread.
            NamedKey::F13 => KittyKey::Tilde(25),
            NamedKey::F14 => KittyKey::Tilde(26),
            NamedKey::F15 => KittyKey::Tilde(28),
            NamedKey::F16 => KittyKey::Tilde(29),
            NamedKey::F17 => KittyKey::Tilde(31),
            NamedKey::F18 => KittyKey::Tilde(32),
            NamedKey::F19 => KittyKey::Tilde(33),
            NamedKey::F20 => KittyKey::Tilde(34),
            _ => return None,
        },
        Key::Character(s) => {
            let ch = s.chars().next()?;
            // Base layout key: kitty reports the lowercase codepoint, with the
            // shifted form as the alternate.
            let base = ch.to_ascii_lowercase();
            if shift && flags & KITTY_REPORT_ALTERNATE != 0 && base != ch {
                alternate = Some(ch as u32);
            }
            // Text the key would type, for both the plain fallback and bit 4.
            if !ctrl && !sup {
                assoc_text = Some(ch);
                plain = Some(s.as_bytes().to_vec());
            }
            KittyKey::Unicode(base as u32)
        }
        _ => {
            // Dead keys / IME commit text: pass it straight through.
            let text = key_text?;
            if released {
                return None;
            }
            return Some(text.as_bytes().to_vec());
        }
    };

    // A printable key with no escape-forcing modifier, when the app has not asked
    // for all keys as escape codes, is delivered as its plain text — exactly as in
    // legacy mode. (Only presses/repeats produce text.)
    if let Some(bytes) = plain {
        if !escape_mods && !force_csi && event_type != 3 {
            return Some(bytes);
        }
    }

    // Legacy-compatible keys (Enter/Tab/Backspace) stay as their classic single
    // byte when unmodified and not forced to CSI form. Esc is NOT among them: a
    // lone 0x1b is exactly the ambiguity bit 0 removes, so once the app opts into
    // disambiguation Esc must be reported as CSI 27 u.
    if !force_csi && mods_val == 1 && event_type == 1 {
        match logical_key {
            Key::Named(NamedKey::Enter) => return Some(b"\r".to_vec()),
            Key::Named(NamedKey::Tab) => return Some(b"\t".to_vec()),
            Key::Named(NamedKey::Escape) if flags & KITTY_DISAMBIGUATE == 0 => {
                return Some(b"\x1b".to_vec());
            }
            Key::Named(NamedKey::Backspace) => return Some(b"\x7f".to_vec()),
            _ => {}
        }
    }

    let with_text = assoc_text.filter(|_| flags & KITTY_REPORT_TEXT != 0 && event_type != 3);
    Some(build_kitty_csi(kk, mods_val, event_type, alternate, with_text))
}

/// Assembles `CSI number[:alt] ; modifiers[:event] [; text] <final>`, omitting each
/// section when it carries only its default value.
fn build_kitty_csi(
    key: KittyKey,
    mods_val: u8,
    event_type: u8,
    alternate: Option<u32>,
    text: Option<char>,
) -> Vec<u8> {
    let (number, final_char, is_legacy) = match key {
        KittyKey::Unicode(n) => (n, 'u', false),
        KittyKey::Tilde(n) => (n, '~', false),
        KittyKey::Legacy(c) => (1, c, true),
    };

    let need_mods = mods_val != 1 || event_type != 1 || text.is_some();
    let mut s = String::from("\x1b[");

    // Key-code section. A legacy key with number 1 and nothing else to say drops the
    // number entirely (e.g. bare Up -> CSI A).
    if !(is_legacy && !need_mods && alternate.is_none()) {
        s.push_str(&number.to_string());
        if let Some(alt) = alternate {
            s.push(':');
            s.push_str(&alt.to_string());
        }
    }

    if need_mods {
        s.push(';');
        s.push_str(&mods_val.to_string());
        if event_type != 1 {
            s.push(':');
            s.push_str(&event_type.to_string());
        }
        if let Some(t) = text {
            s.push(';');
            s.push_str(&(t as u32).to_string());
        }
    }

    s.push(final_char);
    s.into_bytes()
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
        // Plain Enter is CR. Shift+Enter has no legacy encoding of its own — the
        // kitty protocol spells it CSI 13;2u, but an app only gets that if it
        // enabled the protocol first, and several (Claude Code among them) never
        // send the CSI ? u query. ESC-CR is the de-facto fallback those apps
        // already understand: it is exactly the sequence Claude Code's
        // /terminal-setup installs for Alacritty and VS Code. Apps that ignore it
        // see Alt+Enter, which is harmless.
        NamedKey::Enter if mods.shift_key() => b("\x1b\r"),
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
        NamedKey::F13 => tilde(25),
        NamedKey::F14 => tilde(26),
        NamedKey::F15 => tilde(28),
        NamedKey::F16 => tilde(29),
        NamedKey::F17 => tilde(31),
        NamedKey::F18 => tilde(32),
        NamedKey::F19 => tilde(33),
        NamedKey::F20 => tilde(34),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn m(shift: bool, ctrl: bool, alt: bool, sup: bool) -> ModifiersState {
        let mut s = ModifiersState::empty();
        if shift {
            s |= ModifiersState::SHIFT;
        }
        if ctrl {
            s |= ModifiersState::CONTROL;
        }
        if alt {
            s |= ModifiersState::ALT;
        }
        if sup {
            s |= ModifiersState::SUPER;
        }
        s
    }

    fn ch(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    /// Convenience over the inner encoder for press events.
    fn kit(key: &Key, text: Option<&str>, mods: ModifiersState, flags: u8) -> Option<Vec<u8>> {
        encode_kitty_inner(key, text, false, mods, flags, false)
    }

    const DISAMB: u8 = KITTY_DISAMBIGUATE;
    const ALL: u8 = KITTY_REPORT_ALL;

    #[test]
    fn ctrl_i_disambiguates_from_tab() {
        // Legacy: Ctrl+I and Tab both collapse to 0x09. With the disambiguate flag
        // set, Ctrl+I becomes CSI 105 ; 5 u (i=105, ctrl modifier = 1+4).
        let ctrl_i = kit(&ch("i"), None, m(false, true, false, false), DISAMB).unwrap();
        assert_eq!(ctrl_i, b"\x1b[105;5u");
        // Plain Tab with only disambiguate stays the classic byte.
        let tab = kit(&Key::Named(NamedKey::Tab), Some("\t"), m(false, false, false, false), DISAMB)
            .unwrap();
        assert_eq!(tab, b"\t");
    }

    #[test]
    fn legacy_shift_enter_sends_esc_cr() {
        let mode = KeyMode::default();
        // Unmodified Enter stays a bare CR.
        assert_eq!(named_key(NamedKey::Enter, m(false, false, false, false), mode).unwrap(), b"\r");
        // Shift+Enter gets the ESC-CR fallback so apps can tell it from Enter
        // without the kitty protocol.
        assert_eq!(named_key(NamedKey::Enter, m(true, false, false, false), mode).unwrap(), b"\x1b\r");
        // Ctrl+Enter is not the shortcut we are encoding — it keeps plain CR.
        assert_eq!(named_key(NamedKey::Enter, m(false, true, false, false), mode).unwrap(), b"\r");
    }

    #[test]
    fn plain_legacy_keys_roundtrip_in_kitty_mode() {
        // Enter/Tab/Backspace unmodified keep their legacy single byte unless
        // report-all-keys is set.
        let f = DISAMB;
        assert_eq!(kit(&Key::Named(NamedKey::Enter), Some("\r"), m(false, false, false, false), f).unwrap(), b"\r");
        assert_eq!(kit(&Key::Named(NamedKey::Backspace), Some("\x7f"), m(false, false, false, false), f).unwrap(), b"\x7f");
        // A plain letter is just its text.
        assert_eq!(kit(&ch("a"), Some("a"), m(false, false, false, false), f).unwrap(), b"a");
    }

    #[test]
    fn report_all_keys_forces_csi_u() {
        // With report-all-keys, even Enter and a plain letter become escape codes.
        assert_eq!(kit(&Key::Named(NamedKey::Enter), Some("\r"), m(false, false, false, false), ALL).unwrap(), b"\x1b[13u");
        assert_eq!(kit(&ch("a"), Some("a"), m(false, false, false, false), ALL).unwrap(), b"\x1b[97u");
    }

    #[test]
    fn modifiers_encode_as_csi_u() {
        // Shift+Enter: modifier param 1+shift(1) = 2.
        assert_eq!(
            kit(&Key::Named(NamedKey::Enter), Some("\r"), m(true, false, false, false), DISAMB).unwrap(),
            b"\x1b[13;2u"
        );
        // Alt+letter: modifier 1+alt(2) = 3, reported as CSI u under disambiguation.
        assert_eq!(kit(&ch("a"), Some("a"), m(false, false, true, false), DISAMB).unwrap(), b"\x1b[97;3u");
        // Ctrl+Alt+letter: 1+alt(2)+ctrl(4) = 7.
        assert_eq!(kit(&ch("c"), None, m(false, true, true, false), DISAMB).unwrap(), b"\x1b[99;7u");
    }

    #[test]
    fn shifted_alternate_and_associated_text() {
        // bit 2: Shift+a reports the shifted alternate (A=65) after the base (97).
        // Forced to CSI via report-all so the plain-text shortcut is bypassed.
        let f = ALL | KITTY_REPORT_ALTERNATE;
        assert_eq!(kit(&ch("A"), Some("A"), m(true, false, false, false), f).unwrap(), b"\x1b[97:65;2u");
        // bit 4: associated text is appended as a codepoint in the third field.
        let f = ALL | KITTY_REPORT_TEXT;
        assert_eq!(kit(&ch("a"), Some("a"), m(false, false, false, false), f).unwrap(), b"\x1b[97;1;97u");
    }

    #[test]
    fn functional_keys_use_legacy_finals() {
        // Unmodified arrow keeps the bare legacy form even in kitty mode.
        assert_eq!(kit(&Key::Named(NamedKey::ArrowUp), None, m(false, false, false, false), DISAMB).unwrap(), b"\x1b[A");
        // Modified arrow takes the CSI 1 ; mods <final> form.
        assert_eq!(kit(&Key::Named(NamedKey::ArrowUp), None, m(false, true, false, false), DISAMB).unwrap(), b"\x1b[1;5A");
        // Tilde keys keep their number.
        assert_eq!(kit(&Key::Named(NamedKey::Delete), None, m(false, false, false, false), ALL).unwrap(), b"\x1b[3~");
        assert_eq!(kit(&Key::Named(NamedKey::Delete), None, m(true, false, false, false), DISAMB).unwrap(), b"\x1b[3;2~");
    }

    #[test]
    fn esc_is_csi_27_u_under_disambiguate() {
        // A lone 0x1b is the ambiguity bit 0 exists to remove: with disambiguate
        // set, Esc must be reported as CSI 27 u.
        assert_eq!(
            kit(&Key::Named(NamedKey::Escape), Some("\x1b"), m(false, false, false, false), DISAMB)
                .unwrap(),
            b"\x1b[27u"
        );
        // Without bit 0 (or report-all) — e.g. only report-text pushed — the legacy
        // byte stays.
        assert_eq!(
            kit(&Key::Named(NamedKey::Escape), Some("\x1b"), m(false, false, false, false), KITTY_REPORT_TEXT)
                .unwrap(),
            b"\x1b"
        );
    }

    #[test]
    fn plain_space_stays_text_in_kitty_mode() {
        // Space is a text key: under disambiguate it must type a space, not CSI 32 u.
        assert_eq!(
            kit(&Key::Named(NamedKey::Space), Some(" "), m(false, false, false, false), DISAMB).unwrap(),
            b" "
        );
        // Shift alone never forces the escape form for text keys.
        assert_eq!(
            kit(&Key::Named(NamedKey::Space), Some(" "), m(true, false, false, false), DISAMB).unwrap(),
            b" "
        );
        // Ctrl+Space is an escape-forcing chord: CSI 32 ; 5 u.
        assert_eq!(
            kit(&Key::Named(NamedKey::Space), None, m(false, true, false, false), DISAMB).unwrap(),
            b"\x1b[32;5u"
        );
        // Report-all still forces the CSI form even for a plain space.
        assert_eq!(
            kit(&Key::Named(NamedKey::Space), Some(" "), m(false, false, false, false), ALL).unwrap(),
            b"\x1b[32u"
        );
    }

    /// A key sent down the control socket must reach the child as the same bytes the
    /// keyboard would have produced. The scripted path has no `KeyEvent`, and the
    /// text and repeat it cannot supply are not what the encoding turns on.
    #[test]
    fn a_scripted_key_reaches_the_child_as_the_typed_one_would() {
        let plain = m(false, false, false, false);
        let ctrl = m(false, true, false, false);
        for (key, text) in [
            (ch("a"), Some("a")),
            (ch("i"), Some("i")),
            (Key::Named(NamedKey::Enter), Some("\r")),
            (Key::Named(NamedKey::Escape), Some("\x1b")),
            (Key::Named(NamedKey::ArrowUp), None),
        ] {
            for mods in [plain, ctrl] {
                assert_eq!(
                    encode_kitty_key(&key, mods, DISAMB),
                    encode_kitty_inner(&key, text, false, mods, DISAMB, false),
                    "{key:?} with {mods:?}"
                );
            }
        }
        // …and that is a real difference: under the kitty protocol Ctrl+I is CSI u,
        // where the legacy encoder collapses it to the same byte as Tab.
        assert_eq!(encode_kitty_key(&ch("i"), ctrl, DISAMB).unwrap(), b"\x1b[105;5u");
        assert_eq!(encode_key(&ch("i"), ctrl, KeyMode::default()).unwrap(), b"\t");
    }

    #[test]
    fn release_only_when_event_types_requested() {
        // Without the report-events flag a release produces nothing.
        assert_eq!(encode_kitty_inner(&ch("a"), Some("a"), false, m(false, false, false, false), ALL, true), None);
        // With it, a release is event type 3: CSI 97 ; 1 : 3 u.
        let f = ALL | KITTY_REPORT_EVENTS;
        assert_eq!(
            encode_kitty_inner(&ch("a"), Some("a"), false, m(false, false, false, false), f, true).unwrap(),
            b"\x1b[97;1:3u"
        );
        // A key repeat is event type 2 on press.
        assert_eq!(
            encode_kitty_inner(&ch("a"), Some("a"), true, m(false, false, false, false), f, false).unwrap(),
            b"\x1b[97;1:2u"
        );
    }
}
