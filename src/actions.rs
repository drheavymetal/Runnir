//! Actions and key bindings.
//!
//! Every command runnir can perform is an `Action`. A `Keymap` resolves a key
//! chord to one. Keeping actions data (rather than closures) is what lets the same
//! list drive key bindings, the command palette, and the in-terminal docs from one
//! source of truth.

use std::collections::HashMap;

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::layout::{Axis, Direction};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    // Tabs
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    GoToTab(usize),
    RenameTab,
    ReopenClosed,
    MoveTabLeft,
    MoveTabRight,
    // Panes / splits
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    FocusNext,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    // Clipboard & scrollback
    Copy,
    Paste,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    JumpPrevPrompt,
    JumpNextPrompt,
    CopyLastOutput,
    SearchScrollback,
    OpenScrollbackInEditor,
    HistorySearch,
    WatchKeyword,
    LaunchLayout,
    // Font
    FontBigger,
    FontSmaller,
    FontReset,
    // Overlays
    CommandPalette,
    ShowDocs,
    ToggleAi,
    AskAiAboutError,
    AiCommand,
    AiExplain,
    SummarizeSession,
    QuickConnect,
    HintMode,
    // Launchers
    LaunchClaude,
    Whisper,
    // Misc
    ToggleBroadcast,
    ToggleBroadcastGroup,
    ToggleZoom,
    ClearSelectionOrScrollback,
    Quit,
}

impl Action {
    /// Stable identifier used in the config file and the command palette.
    pub fn id(&self) -> &'static str {
        use Action::*;
        match self {
            NewTab => "new_tab",
            CloseTab => "close_tab",
            NextTab => "next_tab",
            PrevTab => "prev_tab",
            GoToTab(_) => "go_to_tab",
            RenameTab => "rename_tab",
            ReopenClosed => "reopen_closed",
            MoveTabLeft => "move_tab_left",
            MoveTabRight => "move_tab_right",
            SplitHorizontal => "split_horizontal",
            SplitVertical => "split_vertical",
            ClosePane => "close_pane",
            FocusLeft => "focus_left",
            FocusRight => "focus_right",
            FocusUp => "focus_up",
            FocusDown => "focus_down",
            FocusNext => "focus_next",
            ResizeLeft => "resize_left",
            ResizeRight => "resize_right",
            ResizeUp => "resize_up",
            ResizeDown => "resize_down",
            Copy => "copy",
            Paste => "paste",
            ScrollUp => "scroll_up",
            ScrollDown => "scroll_down",
            ScrollPageUp => "scroll_page_up",
            ScrollPageDown => "scroll_page_down",
            ScrollToTop => "scroll_to_top",
            ScrollToBottom => "scroll_to_bottom",
            JumpPrevPrompt => "jump_prev_prompt",
            JumpNextPrompt => "jump_next_prompt",
            CopyLastOutput => "copy_last_output",
            SearchScrollback => "search",
            OpenScrollbackInEditor => "open_scrollback_in_editor",
            HistorySearch => "history_search",
            WatchKeyword => "watch_keyword",
            LaunchLayout => "launch_layout",
            FontBigger => "font_bigger",
            FontSmaller => "font_smaller",
            FontReset => "font_reset",
            CommandPalette => "command_palette",
            ShowDocs => "show_docs",
            ToggleAi => "toggle_ai",
            AskAiAboutError => "ask_ai_about_error",
            AiCommand => "ai_command",
            AiExplain => "ai_explain",
            SummarizeSession => "summarize_session",
            QuickConnect => "quick_connect",
            HintMode => "hint_mode",
            LaunchClaude => "launch_claude",
            Whisper => "whisper",
            ToggleBroadcast => "toggle_broadcast",
            ToggleBroadcastGroup => "toggle_broadcast_group",
            ToggleZoom => "toggle_zoom",
            ClearSelectionOrScrollback => "clear",
            Quit => "quit",
        }
    }

    /// Human-readable name for the palette and docs.
    pub fn title(&self) -> &'static str {
        use Action::*;
        match self {
            NewTab => "New tab",
            CloseTab => "Close tab",
            NextTab => "Next tab",
            PrevTab => "Previous tab",
            GoToTab(_) => "Go to tab N",
            RenameTab => "Rename tab",
            ReopenClosed => "Reopen closed tab",
            MoveTabLeft => "Move tab left",
            MoveTabRight => "Move tab right",
            SplitHorizontal => "Split pane left/right",
            SplitVertical => "Split pane up/down",
            ClosePane => "Close pane",
            FocusLeft => "Focus pane left",
            FocusRight => "Focus pane right",
            FocusUp => "Focus pane up",
            FocusDown => "Focus pane down",
            FocusNext => "Focus next pane",
            ResizeLeft => "Shrink pane",
            ResizeRight => "Grow pane",
            ResizeUp => "Grow pane up",
            ResizeDown => "Grow pane down",
            Copy => "Copy selection",
            Paste => "Paste",
            ScrollUp => "Scroll up",
            ScrollDown => "Scroll down",
            ScrollPageUp => "Scroll page up",
            ScrollPageDown => "Scroll page down",
            ScrollToTop => "Scroll to top",
            ScrollToBottom => "Scroll to bottom",
            JumpPrevPrompt => "Jump to previous command",
            JumpNextPrompt => "Jump to next command",
            CopyLastOutput => "Copy last command output",
            SearchScrollback => "Search scrollback",
            OpenScrollbackInEditor => "Open scrollback in $EDITOR",
            HistorySearch => "Insert from shell history",
            WatchKeyword => "Watch pane for keyword",
            LaunchLayout => "Launch layout",
            FontBigger => "Increase font size",
            FontSmaller => "Decrease font size",
            FontReset => "Reset font size",
            CommandPalette => "Command palette",
            ShowDocs => "Show documentation",
            ToggleAi => "Toggle AI assistant",
            AskAiAboutError => "Ask AI: why did this fail?",
            AiCommand => "AI: natural language to command",
            AiExplain => "AI: explain the selection",
            SummarizeSession => "AI: summarize this session",
            QuickConnect => "SSH quick connect",
            HintMode => "Hint mode (open/copy on screen)",
            LaunchClaude => "Launch Claude Code",
            Whisper => "Whisper (tell the terminal what to do)",
            ToggleBroadcast => "Toggle broadcast input",
            ToggleBroadcastGroup => "Toggle pane in broadcast group",
            ToggleZoom => "Zoom / unzoom focused pane",
            ClearSelectionOrScrollback => "Clear selection / scrollback",
            Quit => "Quit runnir",
        }
    }

    /// Parses an action from its config id. `go_to_tab_1` … `go_to_tab_9` map to
    /// `GoToTab`.
    pub fn parse(id: &str) -> Option<Action> {
        use Action::*;
        if let Some(n) = id.strip_prefix("go_to_tab_") {
            return n.parse::<usize>().ok().map(GoToTab);
        }
        Some(match id {
            "new_tab" => NewTab,
            "close_tab" => CloseTab,
            "next_tab" => NextTab,
            "prev_tab" => PrevTab,
            "rename_tab" => RenameTab,
            "reopen_closed" => ReopenClosed,
            "move_tab_left" => MoveTabLeft,
            "move_tab_right" => MoveTabRight,
            "split_horizontal" => SplitHorizontal,
            "split_vertical" => SplitVertical,
            "close_pane" => ClosePane,
            "focus_left" => FocusLeft,
            "focus_right" => FocusRight,
            "focus_up" => FocusUp,
            "focus_down" => FocusDown,
            "focus_next" => FocusNext,
            "resize_left" => ResizeLeft,
            "resize_right" => ResizeRight,
            "resize_up" => ResizeUp,
            "resize_down" => ResizeDown,
            "copy" => Copy,
            "paste" => Paste,
            "scroll_up" => ScrollUp,
            "scroll_down" => ScrollDown,
            "scroll_page_up" => ScrollPageUp,
            "scroll_page_down" => ScrollPageDown,
            "scroll_to_top" => ScrollToTop,
            "scroll_to_bottom" => ScrollToBottom,
            "jump_prev_prompt" => JumpPrevPrompt,
            "jump_next_prompt" => JumpNextPrompt,
            "copy_last_output" => CopyLastOutput,
            "search" => SearchScrollback,
            "open_scrollback_in_editor" => OpenScrollbackInEditor,
            "history_search" => HistorySearch,
            "watch_keyword" => WatchKeyword,
            "launch_layout" => LaunchLayout,
            "font_bigger" => FontBigger,
            "font_smaller" => FontSmaller,
            "font_reset" => FontReset,
            "command_palette" => CommandPalette,
            "show_docs" => ShowDocs,
            "toggle_ai" => ToggleAi,
            "ask_ai_about_error" => AskAiAboutError,
            "ai_command" => AiCommand,
            "ai_explain" => AiExplain,
            "summarize_session" => SummarizeSession,
            "quick_connect" => QuickConnect,
            "hint_mode" => HintMode,
            "launch_claude" => LaunchClaude,
            "whisper" => Whisper,
            "toggle_broadcast" => ToggleBroadcast,
            "toggle_broadcast_group" => ToggleBroadcastGroup,
            "toggle_zoom" => ToggleZoom,
            "clear" => ClearSelectionOrScrollback,
            "quit" => Quit,
            _ => return None,
        })
    }

    pub fn split_axis(&self) -> Option<Axis> {
        match self {
            Action::SplitHorizontal => Some(Axis::Horizontal),
            Action::SplitVertical => Some(Axis::Vertical),
            _ => None,
        }
    }

    pub fn focus_dir(&self) -> Option<Direction> {
        match self {
            Action::FocusLeft => Some(Direction::Left),
            Action::FocusRight => Some(Direction::Right),
            Action::FocusUp => Some(Direction::Up),
            Action::FocusDown => Some(Direction::Down),
            _ => None,
        }
    }

    pub fn resize_dir(&self) -> Option<Direction> {
        match self {
            Action::ResizeLeft => Some(Direction::Left),
            Action::ResizeRight => Some(Direction::Right),
            Action::ResizeUp => Some(Direction::Up),
            Action::ResizeDown => Some(Direction::Down),
            _ => None,
        }
    }

    /// Actions offered in the command palette, in display order. Omits the ones
    /// that are only meaningful as a key chord (directional focus/resize).
    pub fn palette_list() -> Vec<Action> {
        use Action::*;
        vec![
            CommandPalette,
            NewTab,
            CloseTab,
            NextTab,
            PrevTab,
            RenameTab,
            ReopenClosed,
            MoveTabLeft,
            MoveTabRight,
            SplitHorizontal,
            SplitVertical,
            ClosePane,
            Copy,
            Paste,
            CopyLastOutput,
            SearchScrollback,
            OpenScrollbackInEditor,
            HistorySearch,
            WatchKeyword,
            LaunchLayout,
            ScrollToTop,
            ScrollToBottom,
            JumpPrevPrompt,
            JumpNextPrompt,
            FontBigger,
            FontSmaller,
            FontReset,
            ShowDocs,
            ToggleAi,
            AskAiAboutError,
            AiCommand,
            AiExplain,
            SummarizeSession,
            QuickConnect,
            HintMode,
            LaunchClaude,
            Whisper,
            ToggleBroadcast,
            ToggleBroadcastGroup,
            ToggleZoom,
            Quit,
        ]
    }
}

/// A key chord: modifiers plus a base key, both normalised so lookup is exact.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    ctrl: bool,
    shift: bool,
    alt: bool,
    supr: bool,
    key: ChordKey,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum ChordKey {
    Char(char),
    Named(&'static str),
}

impl Chord {
    pub fn from_event(key: &Key, mods: ModifiersState) -> Option<Chord> {
        let base = match key {
            Key::Character(s) => {
                let c = s.chars().next()?.to_ascii_lowercase();
                ChordKey::Char(c)
            }
            Key::Named(named) => ChordKey::Named(named_id(*named)?),
            _ => return None,
        };
        Some(Chord {
            ctrl: mods.control_key(),
            shift: mods.shift_key(),
            alt: mods.alt_key(),
            supr: mods.super_key(),
            key: base,
        })
    }

    /// Parses `"ctrl+shift+t"`, `"alt+enter"`, `"super+1"`.
    pub fn parse(spec: &str) -> Option<Chord> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut supr = false;
        let mut key = None;
        for part in spec.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "opt" | "option" => alt = true,
                "super" | "cmd" | "win" | "meta" => supr = true,
                "" => return None,
                other => {
                    let k = if other.chars().count() == 1 {
                        ChordKey::Char(other.chars().next().unwrap())
                    } else {
                        ChordKey::Named(canonical_named(other)?)
                    };
                    key = Some(k);
                }
            }
        }
        Some(Chord { ctrl, shift, alt, supr, key: key? })
    }
}

/// User chords resolved against the built-in defaults.
pub struct Keymap {
    bindings: HashMap<Chord, Action>,
}

impl Keymap {
    pub fn new(user: &HashMap<String, String>) -> Self {
        let mut bindings = default_bindings();
        for (chord_spec, action_id) in user {
            match (Chord::parse(chord_spec), Action::parse(action_id)) {
                (Some(chord), Some(action)) => {
                    bindings.insert(chord, action);
                }
                (None, _) => eprintln!("runnir: unparseable key {chord_spec:?}"),
                (_, None) => eprintln!("runnir: unknown action {action_id:?}"),
            }
        }
        Self { bindings }
    }

    pub fn resolve(&self, key: &Key, mods: ModifiersState) -> Option<&Action> {
        Chord::from_event(key, mods).and_then(|c| self.bindings.get(&c))
    }
}

fn bind(map: &mut HashMap<Chord, Action>, spec: &str, action: Action) {
    if let Some(chord) = Chord::parse(spec) {
        map.insert(chord, action);
    } else {
        debug_assert!(false, "built-in chord {spec:?} does not parse");
    }
}

fn default_bindings() -> HashMap<Chord, Action> {
    use Action::*;
    let mut m = HashMap::new();
    // Modifier chosen to stay out of the shell's way: ctrl+shift and super, never
    // bare ctrl, which belongs to the program in the pane.
    bind(&mut m, "ctrl+shift+t", NewTab);
    bind(&mut m, "ctrl+shift+w", CloseTab);
    bind(&mut m, "ctrl+tab", NextTab);
    bind(&mut m, "ctrl+shift+tab", PrevTab);
    bind(&mut m, "ctrl+pageup", PrevTab);
    bind(&mut m, "ctrl+pagedown", NextTab);
    for n in 1..=9 {
        bind(&mut m, &format!("super+{n}"), GoToTab(n));
    }
    bind(&mut m, "ctrl+shift+r", RenameTab);
    bind(&mut m, "ctrl+shift+u", ReopenClosed);
    bind(&mut m, "ctrl+shift+left", MoveTabLeft);
    bind(&mut m, "ctrl+shift+right", MoveTabRight);

    bind(&mut m, "ctrl+shift+d", SplitHorizontal);
    bind(&mut m, "ctrl+shift+e", SplitVertical);
    bind(&mut m, "ctrl+shift+x", ClosePane);
    bind(&mut m, "ctrl+shift+h", FocusLeft);
    bind(&mut m, "ctrl+shift+l", FocusRight);
    bind(&mut m, "ctrl+shift+k", FocusUp);
    bind(&mut m, "ctrl+shift+j", FocusDown);
    bind(&mut m, "super+left", ResizeLeft);
    bind(&mut m, "super+right", ResizeRight);
    bind(&mut m, "super+up", ResizeUp);
    bind(&mut m, "super+down", ResizeDown);

    bind(&mut m, "ctrl+shift+c", Copy);
    bind(&mut m, "ctrl+shift+v", Paste);
    bind(&mut m, "ctrl+shift+o", CopyLastOutput);
    bind(&mut m, "ctrl+shift+f", SearchScrollback);
    bind(&mut m, "ctrl+shift+q", OpenScrollbackInEditor);
    bind(&mut m, "shift+pageup", ScrollPageUp);
    bind(&mut m, "shift+pagedown", ScrollPageDown);
    bind(&mut m, "ctrl+shift+home", ScrollToTop);
    bind(&mut m, "ctrl+shift+end", ScrollToBottom);
    bind(&mut m, "ctrl+shift+up", JumpPrevPrompt);
    bind(&mut m, "ctrl+shift+down", JumpNextPrompt);

    bind(&mut m, "ctrl+plus", FontBigger);
    bind(&mut m, "ctrl+equal", FontBigger);
    bind(&mut m, "ctrl+minus", FontSmaller);
    bind(&mut m, "ctrl+0", FontReset);

    bind(&mut m, "ctrl+shift+p", CommandPalette);
    bind(&mut m, "f1", ShowDocs);
    bind(&mut m, "ctrl+shift+a", ToggleAi);
    bind(&mut m, "ctrl+shift+g", AskAiAboutError);
    bind(&mut m, "ctrl+shift+m", AiCommand);
    bind(&mut m, "ctrl+shift+y", AiExplain);
    bind(&mut m, "ctrl+shift+i", SummarizeSession);
    bind(&mut m, "ctrl+shift+s", QuickConnect);
    bind(&mut m, "ctrl+shift+space", HintMode);
    bind(&mut m, "ctrl+shift+n", LaunchClaude);
    bind(&mut m, "ctrl+shift+enter", Whisper);
    bind(&mut m, "ctrl+shift+b", ToggleBroadcast);
    bind(&mut m, "ctrl+shift+z", ToggleZoom);
    m
}

/// Action id -> a readable chord, for the palette's right-hand hints. Uses the
/// default bindings; a user override changes behaviour but the hint stays the
/// canonical one, which is the shortcut worth teaching.
pub fn default_hints() -> HashMap<String, String> {
    let pretty = [
        ("new_tab", "Ctrl+Shift+T"),
        ("close_tab", "Ctrl+Shift+W"),
        ("split_horizontal", "Ctrl+Shift+D"),
        ("split_vertical", "Ctrl+Shift+E"),
        ("close_pane", "Ctrl+Shift+X"),
        ("copy", "Ctrl+Shift+C"),
        ("paste", "Ctrl+Shift+V"),
        ("copy_last_output", "Ctrl+Shift+O"),
        ("search", "Ctrl+Shift+F"),
        ("open_scrollback_in_editor", "Ctrl+Shift+Q"),
        ("command_palette", "Ctrl+Shift+P"),
        ("show_docs", "F1"),
        ("toggle_ai", "Ctrl+Shift+A"),
        ("ask_ai_about_error", "Ctrl+Shift+G"),
        ("ai_command", "Ctrl+Shift+M"),
        ("ai_explain", "Ctrl+Shift+Y"),
        ("summarize_session", "Ctrl+Shift+I"),
        ("quick_connect", "Ctrl+Shift+S"),
        ("hint_mode", "Ctrl+Shift+Space"),
        ("launch_claude", "Ctrl+Shift+N"),
        ("whisper", "Ctrl+Shift+Enter"),
        ("toggle_broadcast", "Ctrl+Shift+B"),
        ("toggle_zoom", "Ctrl+Shift+Z"),
        ("jump_prev_prompt", "Ctrl+Shift+Up"),
        ("jump_next_prompt", "Ctrl+Shift+Down"),
        ("font_bigger", "Ctrl++"),
        ("font_smaller", "Ctrl+-"),
        ("rename_tab", "Ctrl+Shift+R"),
        ("reopen_closed", "Ctrl+Shift+U"),
        ("move_tab_left", "Ctrl+Shift+Left"),
        ("move_tab_right", "Ctrl+Shift+Right"),
    ];
    pretty.iter().map(|(a, k)| (a.to_string(), k.to_string())).collect()
}

fn named_id(named: NamedKey) -> Option<&'static str> {
    Some(match named {
        NamedKey::Enter => "enter",
        NamedKey::Tab => "tab",
        NamedKey::Space => "space",
        NamedKey::Escape => "escape",
        NamedKey::Backspace => "backspace",
        NamedKey::Delete => "delete",
        NamedKey::ArrowUp => "up",
        NamedKey::ArrowDown => "down",
        NamedKey::ArrowLeft => "left",
        NamedKey::ArrowRight => "right",
        NamedKey::Home => "home",
        NamedKey::End => "end",
        NamedKey::PageUp => "pageup",
        NamedKey::PageDown => "pagedown",
        NamedKey::F1 => "f1",
        NamedKey::F2 => "f2",
        NamedKey::F3 => "f3",
        NamedKey::F4 => "f4",
        NamedKey::F5 => "f5",
        NamedKey::F6 => "f6",
        NamedKey::F7 => "f7",
        NamedKey::F8 => "f8",
        NamedKey::F9 => "f9",
        NamedKey::F10 => "f10",
        NamedKey::F11 => "f11",
        NamedKey::F12 => "f12",
        _ => return None,
    })
}

/// Maps config spellings to the canonical named-key ids, including symbols that
/// arrive as characters on some layouts (`plus`, `minus`).
fn canonical_named(name: &str) -> Option<&'static str> {
    Some(match name {
        "enter" | "return" => "enter",
        "tab" => "tab",
        "space" => "space",
        "escape" | "esc" => "escape",
        "backspace" => "backspace",
        "delete" | "del" => "delete",
        "up" => "up",
        "down" => "down",
        "left" => "left",
        "right" => "right",
        "home" => "home",
        "end" => "end",
        "pageup" | "pgup" => "pageup",
        "pagedown" | "pgdn" => "pagedown",
        "plus" => "plus",
        "minus" => "minus",
        "equal" | "equals" => "equal",
        "f1" => "f1",
        "f2" => "f2",
        "f3" => "f3",
        "f4" => "f4",
        "f5" => "f5",
        "f6" => "f6",
        "f7" => "f7",
        "f8" => "f8",
        "f9" => "f9",
        "f10" => "f10",
        "f11" => "f11",
        "f12" => "f12",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_id_round_trips() {
        // A palette entry whose id does not parse back is a dead command.
        for action in Action::palette_list() {
            let parsed = Action::parse(action.id());
            assert_eq!(parsed.as_ref(), Some(&action), "{:?} did not round-trip", action);
        }
    }

    #[test]
    fn go_to_tab_parses_its_number() {
        assert_eq!(Action::parse("go_to_tab_3"), Some(Action::GoToTab(3)));
        assert_eq!(Action::parse("go_to_tab_x"), None);
    }

    #[test]
    fn chords_parse_and_match_events() {
        let c = Chord::parse("ctrl+shift+t").unwrap();
        assert!(c.ctrl && c.shift && !c.alt);
        assert_eq!(c.key, ChordKey::Char('t'));
        assert_eq!(Chord::parse("super+1").unwrap().key, ChordKey::Char('1'));
        assert_eq!(Chord::parse("alt+enter").unwrap().key, ChordKey::Named("enter"));
        assert!(Chord::parse("ctrl+").is_none(), "a modifier with no key is invalid");
    }

    #[test]
    fn defaults_use_no_bare_ctrl_letter() {
        // Bare ctrl+letter belongs to the program in the pane (ctrl+c, ctrl+d…).
        // Every default must carry shift or super too, or it would shadow the shell.
        for chord in default_bindings().keys() {
            if chord.ctrl && !chord.shift && !chord.supr && !chord.alt {
                if let ChordKey::Char(c) = chord.key {
                    assert!(
                        !c.is_ascii_alphabetic(),
                        "ctrl+{c} would be swallowed before the shell sees it"
                    );
                }
            }
        }
    }

    #[test]
    fn user_binding_overrides_default() {
        let mut user = HashMap::new();
        user.insert("ctrl+shift+t".into(), "quit".into());
        let map = Keymap::new(&user);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        assert_eq!(map.bindings.get(&chord), Some(&Action::Quit));
    }

    #[test]
    fn default_palette_shortcut_is_bound() {
        let map = Keymap::new(&HashMap::new());
        let chord = Chord::parse("ctrl+shift+p").unwrap();
        assert_eq!(map.bindings.get(&chord), Some(&Action::CommandPalette));
    }
}
