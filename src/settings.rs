//! The in-terminal settings panel (Ctrl+Shift+, or the palette). Edits a working
//! copy of the `Config` live and saves it as JSON. Every editable field is one row;
//! bools toggle, numbers step, enums cycle, text/paths open an inline editor.

use crate::config::Config;

/// One editable setting, identified so the value/adjust logic stays in one place.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    FontFamily,
    FontSize,
    FontLigatures,
    Opacity,
    Padding,
    Decorations,
    StatusBar,
    Background,
    BackgroundDim,
    Minimap,
    CursorShape,
    CursorBlink,
    BlinkInterval,
    CursorTrail,
    ScrollbackLines,
    CopyOnSelect,
    WheelLines,
    ContextTint,
    NotifyAfter,
    ConfirmClose,
    RestoreSession,
    CommandGuardian,
    SmoothScroll,
    ExplorerSide,
    ExplorerWidth,
    ExplorerHidden,
    KeyboardAmbient,
    KeyboardFlashMs,
    KeyboardLeaderLights,
}

/// How a setting is edited, which drives the key handling and the value hint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Float,
    Int,
    Text,
    Enum,
}

pub struct Row {
    pub section: &'static str,
    pub label: &'static str,
    pub id: SettingId,
    pub kind: Kind,
}

macro_rules! row {
    ($section:expr, $label:expr, $id:ident, $kind:ident) => {
        Row { section: $section, label: $label, id: SettingId::$id, kind: Kind::$kind }
    };
}

/// Every editable setting, in display order (grouped by section).
pub fn rows() -> Vec<Row> {
    vec![
        row!("Font", "Family", FontFamily, Text),
        row!("Font", "Size", FontSize, Float),
        row!("Font", "Ligatures", FontLigatures, Bool),
        row!("Window", "Opacity", Opacity, Float),
        row!("Window", "Padding", Padding, Float),
        row!("Window", "Decorations", Decorations, Bool),
        row!("Window", "Status bar", StatusBar, Bool),
        row!("Window", "Background image", Background, Text),
        row!("Window", "Background dim", BackgroundDim, Float),
        row!("Window", "Minimap", Minimap, Bool),
        row!("Cursor", "Shape", CursorShape, Enum),
        row!("Cursor", "Blink", CursorBlink, Bool),
        row!("Cursor", "Blink interval (ms)", BlinkInterval, Int),
        row!("Cursor", "Trail", CursorTrail, Bool),
        row!("Scrollback", "Lines", ScrollbackLines, Int),
        row!("Behaviour", "Copy on select", CopyOnSelect, Bool),
        row!("Behaviour", "Wheel lines", WheelLines, Float),
        row!("Behaviour", "Context tint", ContextTint, Bool),
        row!("Behaviour", "Notify after (s)", NotifyAfter, Int),
        row!("Behaviour", "Confirm close", ConfirmClose, Bool),
        row!("Behaviour", "Restore last closed window", RestoreSession, Bool),
        row!("Behaviour", "Command guardian", CommandGuardian, Bool),
        row!("Behaviour", "Smooth scroll", SmoothScroll, Bool),
        row!("Explorer", "Side", ExplorerSide, Enum),
        row!("Explorer", "Width (columns)", ExplorerWidth, Int),
        row!("Explorer", "Show hidden files", ExplorerHidden, Bool),
        row!("Keyboard", "Flash the ZSA board", KeyboardAmbient, Bool),
        row!("Keyboard", "Flash length (ms)", KeyboardFlashMs, Int),
        row!("Keyboard", "Light the leader on the keys", KeyboardLeaderLights, Bool),
    ]
}

/// The current value of `id`, formatted for display.
pub fn value(cfg: &Config, id: SettingId) -> String {
    use SettingId::*;
    match id {
        FontFamily => cfg.font.family.clone(),
        FontSize => format!("{:.0}", cfg.font.size),
        FontLigatures => onoff(cfg.font.ligatures),
        Opacity => format!("{:.2}", cfg.window.opacity),
        Padding => format!("{:.0}", cfg.window.padding),
        Decorations => onoff(cfg.window.decorations),
        StatusBar => onoff(cfg.window.status_bar),
        Background => cfg.window.background.clone().unwrap_or_else(|| "(none)".into()),
        BackgroundDim => format!("{:.2}", cfg.window.background_dim),
        Minimap => onoff(cfg.window.minimap),
        CursorShape => match cfg.cursor.shape {
            crate::config::CursorShape::Block => "block".to_string(),
            crate::config::CursorShape::Beam => "beam".to_string(),
            crate::config::CursorShape::Underline => "underline".to_string(),
        },
        CursorBlink => onoff(cfg.cursor.blink),
        BlinkInterval => cfg.cursor.blink_interval.to_string(),
        CursorTrail => onoff(cfg.cursor.trail),
        ScrollbackLines => cfg.scrollback.lines.to_string(),
        CopyOnSelect => onoff(cfg.behaviour.copy_on_select),
        WheelLines => format!("{:.0}", cfg.behaviour.wheel_lines),
        ContextTint => onoff(cfg.behaviour.context_tint),
        NotifyAfter => cfg.behaviour.notify_after_secs.to_string(),
        ConfirmClose => onoff(cfg.behaviour.confirm_close),
        RestoreSession => onoff(cfg.behaviour.restore_session),
        CommandGuardian => onoff(cfg.behaviour.command_guardian),
        SmoothScroll => onoff(cfg.behaviour.smooth_scroll),
        ExplorerSide => cfg.explorer.side.clone(),
        ExplorerWidth => cfg.explorer.width.to_string(),
        ExplorerHidden => onoff(cfg.explorer.show_hidden),
        KeyboardAmbient => onoff(cfg.keyboard.ambient),
        KeyboardFlashMs => cfg.keyboard.flash_ms.to_string(),
        KeyboardLeaderLights => onoff(cfg.keyboard.leader_lights),
    }
}

fn onoff(b: bool) -> String {
    if b { "on".into() } else { "off".into() }
}

/// Steps a numeric/bool/enum setting by `dir` (-1 or +1). Text settings are edited
/// inline instead and ignore this.
pub fn adjust(cfg: &mut Config, id: SettingId, dir: i32) {
    use SettingId::*;
    let up = dir > 0;
    match id {
        FontSize => cfg.font.size = (cfg.font.size + dir as f32).clamp(6.0, 72.0),
        FontLigatures => cfg.font.ligatures = up,
        Opacity => cfg.window.opacity = (cfg.window.opacity + dir as f32 * 0.05).clamp(0.1, 1.0),
        Padding => cfg.window.padding = (cfg.window.padding + dir as f32 * 2.0).clamp(0.0, 40.0),
        Decorations => cfg.window.decorations = up,
        StatusBar => cfg.window.status_bar = up,
        BackgroundDim => {
            cfg.window.background_dim = (cfg.window.background_dim + dir as f32 * 0.05).clamp(0.0, 1.0)
        }
        Minimap => cfg.window.minimap = up,
        CursorShape => {
            use crate::config::CursorShape as Cs;
            let order = [Cs::Block, Cs::Beam, Cs::Underline];
            let i = order.iter().position(|&s| s == cfg.cursor.shape).unwrap_or(0);
            let n = order.len() as i32;
            cfg.cursor.shape = order[(((i as i32 + dir) % n + n) % n) as usize];
        }
        CursorBlink => cfg.cursor.blink = up,
        BlinkInterval => {
            cfg.cursor.blink_interval =
                (cfg.cursor.blink_interval as i64 + dir as i64 * 50).clamp(50, 5000) as u64
        }
        CursorTrail => cfg.cursor.trail = up,
        ScrollbackLines => {
            cfg.scrollback.lines =
                (cfg.scrollback.lines as i64 + dir as i64 * 1000).clamp(100, 1_000_000) as usize
        }
        CopyOnSelect => cfg.behaviour.copy_on_select = up,
        WheelLines => {
            cfg.behaviour.wheel_lines = (cfg.behaviour.wheel_lines + dir as f32).clamp(1.0, 20.0)
        }
        ContextTint => cfg.behaviour.context_tint = up,
        NotifyAfter => {
            cfg.behaviour.notify_after_secs =
                (cfg.behaviour.notify_after_secs as i64 + dir as i64 * 5).clamp(0, 600) as u64
        }
        ConfirmClose => cfg.behaviour.confirm_close = up,
        RestoreSession => cfg.behaviour.restore_session = up,
        CommandGuardian => cfg.behaviour.command_guardian = up,
        SmoothScroll => cfg.behaviour.smooth_scroll = up,
        // The sidebar is stored in columns, so it steps in columns. Clamped to the
        // same floor the sidebar itself enforces, or the panel could set a width
        // the draw path silently refuses.
        ExplorerSide => {
            let side = crate::explorer::Side::parse(&cfg.explorer.side).unwrap_or_default();
            cfg.explorer.side = side.flip().label().to_string();
        }
        ExplorerWidth => {
            let w = cfg.explorer.width as i32 + dir * 2;
            cfg.explorer.width = w.clamp(crate::explorer::MIN_WIDTH as i32, 120) as usize;
        }
        ExplorerHidden => cfg.explorer.show_hidden = up,
        KeyboardAmbient => cfg.keyboard.ambient = up,
        KeyboardLeaderLights => cfg.keyboard.leader_lights = up,
        KeyboardFlashMs => {
            let ms = cfg.keyboard.flash_ms as i32 + dir * 200;
            cfg.keyboard.flash_ms = ms.clamp(200, 10_000) as u32;
        }
        FontFamily | Background => {} // text; edited inline
    }
}

/// Applies an inline-edited text value to a text setting.
pub fn set_text(cfg: &mut Config, id: SettingId, text: String) {
    match id {
        SettingId::FontFamily => {
            if !text.trim().is_empty() {
                cfg.font.family = text.trim().to_string();
            }
        }
        SettingId::Background => {
            let t = text.trim();
            cfg.window.background = if t.is_empty() { None } else { Some(t.to_string()) };
        }
        _ => {}
    }
}
