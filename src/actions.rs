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
    CycleLayout,
    // Clipboard & scrollback
    Copy,
    Paste,
    ClipboardHistory,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    JumpPrevPrompt,
    JumpNextPrompt,
    CopyLastOutput,
    PipeLastOutput,
    PipeScrollback,
    SearchScrollback,
    OpenScrollbackInEditor,
    HistorySearch,
    WatchKeyword,
    LaunchLayout,
    OpenSnippets,
    CopyMode,
    FoldOutput,
    ToggleImageWatch,
    SetImageWatchDir,
    SaveProjectSession,
    RestoreProjectSession,
    // Media (now playing)
    NowPlaying,
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    MediaVolumeUp,
    MediaVolumeDown,
    // Font
    FontBigger,
    FontSmaller,
    FontReset,
    // Overlays
    CommandPalette,
    ShowDocs,
    OpenConfig,
    OpenThemePicker,
    ToggleAi,
    AskAiAboutError,
    AiCommand,
    FixLastCommand,
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
            CycleLayout => "cycle_layout",
            Copy => "copy",
            Paste => "paste",
            ClipboardHistory => "clipboard_history",
            ScrollUp => "scroll_up",
            ScrollDown => "scroll_down",
            ScrollPageUp => "scroll_page_up",
            ScrollPageDown => "scroll_page_down",
            ScrollToTop => "scroll_to_top",
            ScrollToBottom => "scroll_to_bottom",
            JumpPrevPrompt => "jump_prev_prompt",
            JumpNextPrompt => "jump_next_prompt",
            CopyLastOutput => "copy_last_output",
            PipeLastOutput => "pipe_last_output",
            PipeScrollback => "pipe_scrollback",
            SearchScrollback => "search",
            OpenScrollbackInEditor => "open_scrollback_in_editor",
            HistorySearch => "history_search",
            WatchKeyword => "watch_keyword",
            LaunchLayout => "launch_layout",
            OpenSnippets => "open_snippets",
            CopyMode => "copy_mode",
            FoldOutput => "fold_output",
            ToggleImageWatch => "toggle_image_watch",
            SetImageWatchDir => "set_image_watch_dir",
            SaveProjectSession => "save_project_session",
            RestoreProjectSession => "restore_project_session",
            NowPlaying => "now_playing",
            MediaPlayPause => "media_play_pause",
            MediaNext => "media_next",
            MediaPrev => "media_prev",
            MediaVolumeUp => "media_volume_up",
            MediaVolumeDown => "media_volume_down",
            FontBigger => "font_bigger",
            FontSmaller => "font_smaller",
            FontReset => "font_reset",
            CommandPalette => "command_palette",
            ShowDocs => "show_docs",
            OpenConfig => "open_config",
            OpenThemePicker => "open_theme_picker",
            ToggleAi => "toggle_ai",
            AskAiAboutError => "ask_ai_about_error",
            AiCommand => "ai_command",
            FixLastCommand => "fix_last_command",
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
            CycleLayout => "Cycle layout (splits/stack/tall/fat/grid)",
            Copy => "Copy selection",
            Paste => "Paste",
            ClipboardHistory => "Clipboard history",
            ScrollUp => "Scroll up",
            ScrollDown => "Scroll down",
            ScrollPageUp => "Scroll page up",
            ScrollPageDown => "Scroll page down",
            ScrollToTop => "Scroll to top",
            ScrollToBottom => "Scroll to bottom",
            JumpPrevPrompt => "Jump to previous command",
            JumpNextPrompt => "Jump to next command",
            CopyLastOutput => "Copy last command output",
            PipeLastOutput => "Pipe last output through command...",
            PipeScrollback => "Pipe scrollback through command...",
            SearchScrollback => "Search scrollback",
            OpenScrollbackInEditor => "Open scrollback in $EDITOR",
            HistorySearch => "Insert from shell history",
            WatchKeyword => "Watch pane for keyword",
            LaunchLayout => "Launch layout",
            OpenSnippets => "Insert command snippet",
            CopyMode => "Copy mode (keyboard select)",
            FoldOutput => "Fold / unfold all command output",
            ToggleImageWatch => "Auto-preview images: toggle on this pane's dir",
            SetImageWatchDir => "Auto-preview images: set / clear watched dir",
            SaveProjectSession => "Save session for this project",
            RestoreProjectSession => "Restore session for this project",
            NowPlaying => "Now playing (media overlay)",
            MediaPlayPause => "Media: play / pause",
            MediaNext => "Media: next track",
            MediaPrev => "Media: previous track",
            MediaVolumeUp => "Media: volume up",
            MediaVolumeDown => "Media: volume down",
            FontBigger => "Increase font size",
            FontSmaller => "Decrease font size",
            FontReset => "Reset font size",
            CommandPalette => "Command palette",
            ShowDocs => "Show documentation",
            OpenConfig => "Settings",
            OpenThemePicker => "Theme picker",
            ToggleAi => "Toggle AI assistant",
            AskAiAboutError => "Ask AI: why did this fail?",
            AiCommand => "AI: natural language to command",
            FixLastCommand => "AI: fix the last failed command",
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
            "cycle_layout" => CycleLayout,
            "copy" => Copy,
            "paste" => Paste,
            "clipboard_history" => ClipboardHistory,
            "scroll_up" => ScrollUp,
            "scroll_down" => ScrollDown,
            "scroll_page_up" => ScrollPageUp,
            "scroll_page_down" => ScrollPageDown,
            "scroll_to_top" => ScrollToTop,
            "scroll_to_bottom" => ScrollToBottom,
            "jump_prev_prompt" => JumpPrevPrompt,
            "jump_next_prompt" => JumpNextPrompt,
            "copy_last_output" => CopyLastOutput,
            "pipe_last_output" => PipeLastOutput,
            "pipe_scrollback" => PipeScrollback,
            "search" => SearchScrollback,
            "open_scrollback_in_editor" => OpenScrollbackInEditor,
            "history_search" => HistorySearch,
            "watch_keyword" => WatchKeyword,
            "launch_layout" => LaunchLayout,
            "open_snippets" => OpenSnippets,
            "copy_mode" => CopyMode,
            "fold_output" => FoldOutput,
            "toggle_image_watch" => ToggleImageWatch,
            "set_image_watch_dir" => SetImageWatchDir,
            "save_project_session" => SaveProjectSession,
            "restore_project_session" => RestoreProjectSession,
            "now_playing" => NowPlaying,
            "media_play_pause" => MediaPlayPause,
            "media_next" => MediaNext,
            "media_prev" => MediaPrev,
            "media_volume_up" => MediaVolumeUp,
            "media_volume_down" => MediaVolumeDown,
            "font_bigger" => FontBigger,
            "font_smaller" => FontSmaller,
            "font_reset" => FontReset,
            "command_palette" => CommandPalette,
            "show_docs" => ShowDocs,
            "open_config" => OpenConfig,
            "open_theme_picker" => OpenThemePicker,
            "toggle_ai" => ToggleAi,
            "ask_ai_about_error" => AskAiAboutError,
            "ai_command" => AiCommand,
            "fix_last_command" => FixLastCommand,
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
            CycleLayout,
            Copy,
            Paste,
            ClipboardHistory,
            CopyLastOutput,
            PipeLastOutput,
            PipeScrollback,
            SearchScrollback,
            OpenScrollbackInEditor,
            HistorySearch,
            WatchKeyword,
            LaunchLayout,
            OpenSnippets,
            CopyMode,
            FoldOutput,
            ToggleImageWatch,
            SetImageWatchDir,
            SaveProjectSession,
            RestoreProjectSession,
            NowPlaying,
            MediaPlayPause,
            MediaNext,
            MediaPrev,
            MediaVolumeUp,
            MediaVolumeDown,
            ScrollToTop,
            ScrollToBottom,
            JumpPrevPrompt,
            JumpNextPrompt,
            FontBigger,
            FontSmaller,
            FontReset,
            ShowDocs,
            OpenConfig,
            OpenThemePicker,
            ToggleAi,
            AskAiAboutError,
            AiCommand,
            FixLastCommand,
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

/// Default chord that arms the leader layer, on the same alt+shift layer as the
/// resize and clipboard binds.
///
/// Bare `alt+space` was the first choice and was wrong: it is the window menu in
/// GNOME/GTK and on Windows, krunner's default on KDE, and PowerToys Run's on
/// Windows — three of the four big desktops eat it before runnir sees it. Adding
/// shift dodges all three (they are all unshifted), and readline owns no
/// alt+shift binding. Not the super layer, ever: a compositor wins that race.
///
/// Residual risk, accepted: Windows and X11's `grp:alt_shift_toggle` switch
/// keyboard layout on alt+shift. Both fire on *release with no other key*, so
/// the space in between spares us — but a multi-layout user who still trips it
/// has `leader` in the config to rebind, or `""` to turn the layer off.
/// `ctrl+alt+space` is NOT the fallback to suggest: ctrl+alt is AltGr on the
/// Spanish and most EU layouts, where AltGr+space types a non-breaking space.
pub const DEFAULT_LEADER: &str = "alt+shift+space";

/// Prefix marking a user binding as living on the leader layer: `"leader+v"`.
const LEADER_PREFIX: &str = "leader+";

/// User chords resolved against the built-in defaults.
pub struct Keymap {
    bindings: HashMap<Chord, Action>,
    /// The chord that arms the leader layer, if one is configured.
    leader: Option<Chord>,
    /// Actions reached by pressing the leader, releasing it, then this chord.
    /// Kept apart from `bindings` because these are *unmodified* keys — bare `v`
    /// is an action here and a literal `v` everywhere else.
    leader_bindings: HashMap<Chord, Action>,
}

impl Keymap {
    pub fn new(user: &HashMap<String, String>, leader: &str) -> Self {
        let mut bindings = default_bindings();
        let mut leader_bindings = default_leader_bindings();
        for (chord_spec, action_id) in user {
            let (spec, map) = match chord_spec.trim().strip_prefix(LEADER_PREFIX) {
                Some(rest) => (rest, &mut leader_bindings),
                None => (chord_spec.as_str(), &mut bindings),
            };
            match (Chord::parse(spec), Action::parse(action_id)) {
                (Some(chord), Some(action)) => {
                    map.insert(chord, action);
                }
                (None, _) => eprintln!("runnir: unparseable key {chord_spec:?}"),
                (_, None) => eprintln!("runnir: unknown action {action_id:?}"),
            }
        }
        // An empty leader spec disables the layer rather than binding some fallback
        // chord the user never asked for.
        let leader = if leader.trim().is_empty() {
            None
        } else {
            Chord::parse(leader).or_else(|| {
                eprintln!("runnir: unparseable leader key {leader:?}, using {DEFAULT_LEADER}");
                Chord::parse(DEFAULT_LEADER)
            })
        };
        Self { bindings, leader, leader_bindings }
    }

    pub fn resolve(&self, key: &Key, mods: ModifiersState) -> Option<&Action> {
        Chord::from_event(key, mods).and_then(|c| self.bindings.get(&c))
    }

    /// True when this key arms the leader layer.
    pub fn is_leader(&self, key: &Key, mods: ModifiersState) -> bool {
        match (&self.leader, Chord::from_event(key, mods)) {
            (Some(l), Some(c)) => *l == c,
            _ => false,
        }
    }

    /// Resolves a key pressed while the leader layer is armed.
    ///
    /// The exact chord wins, so `"leader+ctrl+r"` still binds a modified key. On a
    /// miss the modifiers held to reach the leader are dropped and it tries again:
    /// nobody lets go of alt+shift between the leader and `1`, and an exact-only match
    /// would swallow that keystroke instead of switching tabs. Shift is dropped
    /// last so a shifted key still finds its unshifted binding (`leader+V` → `v`).
    pub fn resolve_leader(&self, key: &Key, mods: ModifiersState) -> Option<&Action> {
        let chord = Chord::from_event(key, mods)?;
        let relaxed = Chord { alt: false, supr: false, ctrl: false, ..chord.clone() };
        let bare = Chord { shift: false, ..relaxed.clone() };
        self.leader_bindings
            .get(&chord)
            .or_else(|| self.leader_bindings.get(&relaxed))
            .or_else(|| self.leader_bindings.get(&bare))
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
    // Tab switching lives on the leader layer only. `super+N` was grabbed by every
    // tiling compositor tried (Hyprland/GNOME bind it to workspaces), and `alt+N` is
    // readline's digit-argument, so neither can be a default here.
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
    // The alt+shift layer replaces the old super chords: a compositor almost always
    // owns super, and an app can never win that race — the key never reaches us.
    bind(&mut m, "alt+shift+left", ResizeLeft);
    bind(&mut m, "alt+shift+right", ResizeRight);
    bind(&mut m, "alt+shift+up", ResizeUp);
    bind(&mut m, "alt+shift+down", ResizeDown);

    bind(&mut m, "ctrl+shift+c", Copy);
    bind(&mut m, "ctrl+shift+v", Paste);
    // Every ctrl+shift+letter is already taken, so this lives on the alt+shift layer
    // alongside the resize chords. Also on the leader layer as `leader v`.
    bind(&mut m, "alt+shift+v", ClipboardHistory);
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
    // Fix-last-command mirrors ask-why (…+G) on the alt+shift layer, so it still sits
    // with the assistant family without shadowing the shell.
    bind(&mut m, "alt+shift+g", FixLastCommand);
    bind(&mut m, "ctrl+shift+m", AiCommand);
    bind(&mut m, "ctrl+shift+y", AiExplain);
    bind(&mut m, "ctrl+shift+i", SummarizeSession);
    bind(&mut m, "ctrl+shift+s", QuickConnect);
    bind(&mut m, "ctrl+shift+space", HintMode);
    bind(&mut m, "ctrl+shift+n", LaunchClaude);
    bind(&mut m, "ctrl+shift+enter", Whisper);
    bind(&mut m, "ctrl+shift+b", ToggleBroadcast);
    bind(&mut m, "ctrl+shift+z", ToggleZoom);
    bind(&mut m, "alt+shift+s", OpenSnippets);
    // Now-playing overlay ('p' for playing). Media transport also binds to the XF86
    // media keys in the input layer, and the overlay has its own space/n/p/+/- keys.
    bind(&mut m, "alt+shift+p", NowPlaying);
    m
}

/// Actions on the leader layer: press the leader, release it, then one plain key.
///
/// This layer exists because a compositor wins every modifier race — Hyprland and
/// GNOME both claim most of the super layer, and an app cannot bind around that.
/// After the leader is armed the keys are unmodified, so the namespace is wide and
/// nothing outside runnir can take it.
fn default_leader_bindings() -> HashMap<Chord, Action> {
    use Action::*;
    let mut m = HashMap::new();
    for n in 1..=9 {
        bind(&mut m, &n.to_string(), GoToTab(n));
    }
    // Both the arrows and the vim row resize, matching ctrl+shift+hjkl for focus.
    bind(&mut m, "left", ResizeLeft);
    bind(&mut m, "right", ResizeRight);
    bind(&mut m, "up", ResizeUp);
    bind(&mut m, "down", ResizeDown);
    bind(&mut m, "h", ResizeLeft);
    bind(&mut m, "l", ResizeRight);
    bind(&mut m, "k", ResizeUp);
    bind(&mut m, "j", ResizeDown);

    bind(&mut m, "v", ClipboardHistory);
    bind(&mut m, "s", OpenSnippets);
    bind(&mut m, "p", NowPlaying);
    bind(&mut m, "g", FixLastCommand);
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
        ("clipboard_history", "Alt+Shift+V"),
        ("copy_last_output", "Ctrl+Shift+O"),
        ("search", "Ctrl+Shift+F"),
        ("open_scrollback_in_editor", "Ctrl+Shift+Q"),
        ("command_palette", "Ctrl+Shift+P"),
        ("show_docs", "F1"),
        ("toggle_ai", "Ctrl+Shift+A"),
        ("ask_ai_about_error", "Ctrl+Shift+G"),
        ("fix_last_command", "Alt+Shift+G"),
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
        ("open_snippets", "Alt+Shift+S"),
        ("now_playing", "Alt+Shift+P"),
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
        let map = Keymap::new(&user, DEFAULT_LEADER);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        assert_eq!(map.bindings.get(&chord), Some(&Action::Quit));
    }

    #[test]
    fn defaults_avoid_the_compositor_owned_super_layer() {
        // A compositor grabs super before the app ever sees the key, so a super
        // default is a binding that silently does nothing (Hyprland and GNOME both
        // claim most of that layer). The leader layer exists to replace it.
        for chord in default_bindings().keys() {
            assert!(!chord.supr, "super chord {chord:?} would never reach runnir");
        }
    }

    #[test]
    fn leader_layer_binds_plain_keys_without_touching_the_normal_map() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let v = Chord::parse("v").unwrap();
        assert_eq!(map.leader_bindings.get(&v), Some(&Action::ClipboardHistory));
        // A bare `v` must stay a literal `v` outside the leader layer.
        assert_eq!(map.bindings.get(&v), None);
        assert_eq!(map.leader_bindings.get(&Chord::parse("3").unwrap()), Some(&Action::GoToTab(3)));
    }

    #[test]
    fn user_can_bind_and_disable_the_leader_layer() {
        let mut user = HashMap::new();
        user.insert("leader+z".into(), "quit".into());
        let map = Keymap::new(&user, "alt+shift+j");
        assert_eq!(map.leader_bindings.get(&Chord::parse("z").unwrap()), Some(&Action::Quit));
        assert_eq!(map.leader, Chord::parse("alt+shift+j"));
        // The default leader entries survive a user addition.
        assert_eq!(map.leader_bindings.get(&Chord::parse("v").unwrap()), Some(&Action::ClipboardHistory));

        // An empty spec turns the layer off rather than falling back to a chord the
        // user did not ask for.
        assert_eq!(Keymap::new(&HashMap::new(), "").leader, None);
    }

    #[test]
    fn leader_layer_ignores_the_modifiers_still_held_from_the_leader() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let one = Key::Character("1".into());
        // Nobody lets go of alt+shift between the leader and `1`.
        assert_eq!(map.resolve_leader(&one, ModifiersState::ALT), Some(&Action::GoToTab(1)));
        assert_eq!(
            map.resolve_leader(&one, ModifiersState::ALT | ModifiersState::SHIFT),
            Some(&Action::GoToTab(1))
        );
        assert_eq!(map.resolve_leader(&one, ModifiersState::empty()), Some(&Action::GoToTab(1)));
        // A shifted letter falls back to its unshifted binding.
        let v = Key::Character("V".into());
        assert_eq!(
            map.resolve_leader(&v, ModifiersState::ALT | ModifiersState::SHIFT),
            Some(&Action::ClipboardHistory)
        );
        // An unbound key is still unbound, however it is modified.
        assert_eq!(map.resolve_leader(&Key::Character("q".into()), ModifiersState::ALT), None);
    }

    #[test]
    fn exact_leader_chord_wins_over_the_relaxed_match() {
        let mut user = HashMap::new();
        user.insert("leader+ctrl+v".into(), "quit".into());
        let map = Keymap::new(&user, DEFAULT_LEADER);
        let v = Key::Character("v".into());
        assert_eq!(map.resolve_leader(&v, ModifiersState::CONTROL), Some(&Action::Quit));
        assert_eq!(map.resolve_leader(&v, ModifiersState::empty()), Some(&Action::ClipboardHistory));
    }

    #[test]
    fn default_palette_shortcut_is_bound() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let chord = Chord::parse("ctrl+shift+p").unwrap();
        assert_eq!(map.bindings.get(&chord), Some(&Action::CommandPalette));
    }
}
