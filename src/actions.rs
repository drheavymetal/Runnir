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
    /// Show/hide the file explorer sidebar, and put the keyboard in it.
    ToggleExplorer,
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
    /// The native git panel: status, log, branches, stashes.
    GitPanel,
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
            ToggleExplorer => "toggle_explorer",
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
            GitPanel => "git_panel",
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
            ToggleExplorer => "File explorer sidebar (tree of the project)",
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
            GitPanel => "Git panel (status, log, branches, stashes)",
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
            "toggle_explorer" => ToggleExplorer,
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
            "git_panel" => GitPanel,
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
            ToggleExplorer,
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
            GitPanel,
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
                // Spelled-out punctuation. These have to become the CHARACTER, not
                // a named key: the keyboard delivers them as `Key::Character('+')`,
                // so a `ChordKey::Named("plus")` could never match an event — which
                // is exactly why `ctrl+plus` and `leader +` silently did nothing.
                // The names exist because `+` is also the separator in a chord spec.
                "plus" => key = Some(ChordKey::Char('+')),
                "minus" | "dash" => key = Some(ChordKey::Char('-')),
                "equal" | "equals" => key = Some(ChordKey::Char('=')),
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

    /// Short label for the which-key panel: just the key, no modifiers, because
    /// everything on the leader layer is already unmodified.
    pub fn label(&self) -> String {
        let base = match &self.key {
            ChordKey::Char(c) => c.to_string(),
            // Named keys are stored by their config spelling (`equal`, `pageup`),
            // which is what you type in the config but not what is printed on the
            // key. The panel shows the glyph — a column of words like "equal" and
            // "pagedown" is unreadable at a glance and eats the width the titles
            // need.
            ChordKey::Named(n) => match *n {
                "left" => "←".into(),
                "right" => "→".into(),
                "up" => "↑".into(),
                "down" => "↓".into(),
                "pageup" => "PgUp".into(),
                "pagedown" => "PgDn".into(),
                "enter" => "⏎".into(),
                "space" => "␣".into(),
                "tab" => "⇥".into(),
                "escape" => "Esc".into(),
                other => other.to_string(),
            },
        };
        if self.shift { base.to_uppercase() } else { base }
    }

    /// Sort key for the panel: digits, then letters, then named keys, so a group
    /// lists in an order the eye can scan instead of hash order.
    fn sort_key(&self) -> (u8, String) {
        match &self.key {
            ChordKey::Char(c) if c.is_ascii_digit() => (0, c.to_string()),
            ChordKey::Char(c) => (1, c.to_string()),
            ChordKey::Named(n) => (2, (*n).to_string()),
        }
    }
}

/// One step on the leader layer: either an action to run, or a group that waits
/// for one more key.
///
/// Two levels exist because a flat layer cannot hold the ~40 actions runnir has:
/// there are 26 letters and `1..9` already belong to the tabs. Groups buy the
/// room and, with the which-key panel, make the layer self-teaching — the reason
/// tmux and every vim leader config land on the same shape.
pub enum LeaderNode {
    Run(Action),
    Group { title: &'static str, keys: HashMap<Chord, LeaderNode> },
}

impl LeaderNode {
    /// What the which-key panel writes next to the key.
    pub fn title(&self) -> &str {
        match self {
            LeaderNode::Run(a) => a.title(),
            LeaderNode::Group { title, .. } => title,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, LeaderNode::Group { .. })
    }
}

/// Looks a chord up in one level of the layer, dropping the modifiers still held
/// from the leader itself on a miss.
///
/// Nobody lets go of alt+shift between the leader and the next key, and an
/// exact-only match would swallow that keystroke. The exact chord is still tried
/// first, so `"leader+ctrl+r"` binds a genuinely modified key; shift is dropped
/// last so a shifted key finds its unshifted binding (`V` → `v`) only after a
/// real `shift+v` binding had its chance.
fn lookup<'a>(map: &'a HashMap<Chord, LeaderNode>, chord: &Chord) -> Option<&'a LeaderNode> {
    let relaxed = Chord { alt: false, supr: false, ctrl: false, ..chord.clone() };
    let bare = Chord { shift: false, ..relaxed.clone() };
    map.get(chord).or_else(|| map.get(&relaxed)).or_else(|| map.get(&bare))
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

/// The chord that arms the leader layer for a config value: none when it is empty
/// (which is how the layer is turned off, rather than binding some fallback chord
/// the user never asked for), the default when it does not parse.
///
/// Shared with the git panel's own leader, which cannot see the `Keymap` and has to
/// resolve the same value the same way — parsing it raw there left a typo with a
/// working global layer and an unreachable panel one.
pub fn leader_chord(spec: &str) -> Option<Chord> {
    if spec.trim().is_empty() {
        return None;
    }
    Chord::parse(spec).or_else(|| Chord::parse(DEFAULT_LEADER))
}

/// User chords resolved against the built-in defaults.
pub struct Keymap {
    bindings: HashMap<Chord, Action>,
    /// The chord that arms the leader layer, if one is configured.
    leader: Option<Chord>,
    /// The leader layer's root. Kept apart from `bindings` because these are
    /// *unmodified* keys — bare `v` is an action here and a literal `v`
    /// everywhere else — and because it is a tree, not a flat map.
    leader_bindings: HashMap<Chord, LeaderNode>,
}

impl Keymap {
    pub fn new(user: &HashMap<String, String>, leader: &str) -> Self {
        let mut bindings = default_bindings();
        let mut leader_bindings = default_leader_bindings();
        for (chord_spec, action_id) in user {
            let Some(action) = Action::parse(action_id) else {
                eprintln!("runnir: unknown action {action_id:?}");
                continue;
            };
            match chord_spec.trim().strip_prefix(LEADER_PREFIX) {
                // A leader spec is a *sequence*: `"leader+t n"` is the leader, then
                // `t`, then `n`. Spaces separate the steps because `+` already joins
                // the modifiers inside one step.
                Some(rest) => {
                    let path: Option<Vec<Chord>> = rest.split_whitespace().map(Chord::parse).collect();
                    match path {
                        Some(p) if !p.is_empty() => insert_leader(&mut leader_bindings, &p, action),
                        _ => eprintln!("runnir: unparseable leader sequence {chord_spec:?}"),
                    }
                }
                None => match Chord::parse(chord_spec) {
                    Some(chord) => {
                        bindings.insert(chord, action);
                    }
                    None => eprintln!("runnir: unparseable key {chord_spec:?}"),
                },
            }
        }
        // The warning belongs here and not in `leader_chord`: the git panel resolves
        // the same value on every keypress, and a fallback that printed there would
        // print for as long as the panel is open.
        if !leader.trim().is_empty() && Chord::parse(leader).is_none() {
            eprintln!("runnir: unparseable leader key {leader:?}, using {DEFAULT_LEADER}");
        }
        let leader = leader_chord(leader);
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

    /// Walks a sequence of keys pressed since the leader was armed. `None` means
    /// the sequence is unbound and the layer should just cancel.
    pub fn resolve_leader(&self, path: &[Chord]) -> Option<&LeaderNode> {
        let mut level = &self.leader_bindings;
        let mut node = None;
        for chord in path {
            node = lookup(level, chord);
            match node {
                Some(LeaderNode::Group { keys, .. }) => level = keys,
                // An action mid-path, or a miss: either way there is nowhere
                // further to walk.
                _ => return node,
            }
        }
        node
    }

    /// The which-key panel's contents for the group reached by `path` (the root
    /// when it is empty): `(key label, what it does, is it a group)`, in scan order.
    pub fn leader_entries(&self, path: &[Chord]) -> Vec<(String, String, bool)> {
        let level = match self.resolve_leader(path) {
            _ if path.is_empty() => &self.leader_bindings,
            Some(LeaderNode::Group { keys, .. }) => keys,
            _ => return Vec::new(),
        };
        let mut out: Vec<_> = level.iter().collect();
        out.sort_by_key(|(c, _)| c.sort_key());
        out.iter().map(|(c, n)| (c.label(), n.title().to_string(), n.is_group())).collect()
    }
}

/// Inserts an action at a path, creating the groups it passes through. A user
/// binding that lands on an existing action replaces it; one that tunnels
/// *through* an action (`"leader+v x"` when `v` runs something) turns that step
/// into a group, since the config asked for a sequence and a config that parses
/// should do what it says.
fn insert_leader(map: &mut HashMap<Chord, LeaderNode>, path: &[Chord], action: Action) {
    let Some((first, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        map.insert(first.clone(), LeaderNode::Run(action));
        return;
    }
    let entry = map
        .entry(first.clone())
        .or_insert_with(|| LeaderNode::Group { title: "custom", keys: HashMap::new() });
    if !entry.is_group() {
        *entry = LeaderNode::Group { title: "custom", keys: HashMap::new() };
    }
    if let LeaderNode::Group { keys, .. } = entry {
        insert_leader(keys, rest, action);
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
fn default_leader_bindings() -> HashMap<Chord, LeaderNode> {
    use Action::*;
    let mut m = HashMap::new();

    // --- Direct keys. These earn their place at the top level by frequency: you
    // switch tab and move focus far more often than you do anything else, and a
    // group would double the keystrokes for exactly the hottest paths.
    for n in 1..=9 {
        leaf(&mut m, &n.to_string(), GoToTab(n));
    }
    leaf(&mut m, "h", FocusLeft);
    leaf(&mut m, "j", FocusDown);
    leaf(&mut m, "k", FocusUp);
    leaf(&mut m, "l", FocusRight);
    // Resize is the shifted vim row and the arrows: same fingers as focus, one
    // modifier apart, so the pair is learned as one thing.
    leaf(&mut m, "shift+h", ResizeLeft);
    leaf(&mut m, "shift+j", ResizeDown);
    leaf(&mut m, "shift+k", ResizeUp);
    leaf(&mut m, "shift+l", ResizeRight);
    leaf(&mut m, "left", ResizeLeft);
    leaf(&mut m, "down", ResizeDown);
    leaf(&mut m, "up", ResizeUp);
    leaf(&mut m, "right", ResizeRight);
    // Kept from the flat layer: both were already muscle memory, and neither
    // letter is needed as a group.
    leaf(&mut m, "v", ClipboardHistory);
    leaf(&mut m, "g", GitPanel);
    // `e` for the explorer, beside `g` for git: both are a whole surface, both are
    // one key. It is also the letter LazyVim uses for the same sidebar.
    leaf(&mut m, "e", ToggleExplorer);
    // Font size, where every terminal already puts it — but +, - and = are not all
    // one keypress on every layout (on the Spanish one `=` is shift+0), so the
    // letters are the binding that always works and the symbols are the alias.
    leaf(&mut m, "z", FontBigger);
    leaf(&mut m, "shift+z", FontSmaller);
    leaf(&mut m, "plus", FontBigger);
    leaf(&mut m, "equal", FontBigger);
    leaf(&mut m, "minus", FontSmaller);
    leaf(&mut m, "0", FontReset);

    // --- Groups. The letter is the noun: t=tabs, p=panes, c=clipboard, f=find,
    // a=ai, l=launch, o=open, s=session.
    group(&mut m, "t", "Tabs", |g| {
        leaf(g, "t", NewTab);
        leaf(g, "n", NextTab);
        leaf(g, "p", PrevTab);
        leaf(g, "w", CloseTab);
        leaf(g, "r", RenameTab);
        leaf(g, "u", ReopenClosed);
        leaf(g, "h", MoveTabLeft);
        leaf(g, "l", MoveTabRight);
        leaf(g, "left", MoveTabLeft);
        leaf(g, "right", MoveTabRight);
    });
    group(&mut m, "p", "Panes", |g| {
        leaf(g, "d", SplitHorizontal);
        leaf(g, "e", SplitVertical);
        leaf(g, "x", ClosePane);
        leaf(g, "z", ToggleZoom);
        leaf(g, "c", CycleLayout);
        leaf(g, "n", FocusNext);
        leaf(g, "b", ToggleBroadcast);
        leaf(g, "g", ToggleBroadcastGroup);
    });
    group(&mut m, "c", "Clipboard", |g| {
        leaf(g, "c", Copy);
        leaf(g, "v", Paste);
        leaf(g, "h", ClipboardHistory);
        leaf(g, "o", CopyLastOutput);
        leaf(g, "m", CopyMode);
        leaf(g, "p", PipeLastOutput);
        leaf(g, "s", PipeScrollback);
    });
    group(&mut m, "f", "Find & scroll", |g| {
        leaf(g, "f", SearchScrollback);
        leaf(g, "h", HistorySearch);
        leaf(g, "i", HintMode);
        leaf(g, "e", OpenScrollbackInEditor);
        leaf(g, "w", WatchKeyword);
        leaf(g, "o", FoldOutput);
        leaf(g, "n", JumpNextPrompt);
        leaf(g, "p", JumpPrevPrompt);
        leaf(g, "t", ScrollToTop);
        leaf(g, "b", ScrollToBottom);
        // Page scroll, on the vim half-page letters and on the page keys.
        leaf(g, "u", ScrollPageUp);
        leaf(g, "d", ScrollPageDown);
        leaf(g, "pageup", ScrollPageUp);
        leaf(g, "pagedown", ScrollPageDown);
    });
    group(&mut m, "a", "AI", |g| {
        leaf(g, "a", ToggleAi);
        leaf(g, "g", FixLastCommand);
        leaf(g, "w", AskAiAboutError);
        leaf(g, "m", AiCommand);
        leaf(g, "e", AiExplain);
        leaf(g, "s", SummarizeSession);
    });
    // `r` for run, not `l` for launch: `l` is focus-right on the vim row and that
    // is not negotiable — the letter that is free wins over the nicer mnemonic.
    group(&mut m, "r", "Run & launch", |g| {
        leaf(g, "c", LaunchClaude);
        leaf(g, "w", Whisper);
        leaf(g, "s", QuickConnect);
        leaf(g, "m", NowPlaying);
        leaf(g, "l", LaunchLayout);
    });
    group(&mut m, "o", "Open", |g| {
        leaf(g, "c", OpenConfig);
        leaf(g, "t", OpenThemePicker);
        leaf(g, "d", ShowDocs);
        leaf(g, "s", OpenSnippets);
        leaf(g, "p", CommandPalette);
        leaf(g, "i", ToggleImageWatch);
        leaf(g, "w", SetImageWatchDir);
    });
    group(&mut m, "s", "Session", |g| {
        leaf(g, "s", SaveProjectSession);
        leaf(g, "r", RestoreProjectSession);
        leaf(g, "c", ClearSelectionOrScrollback);
        leaf(g, "q", Quit);
    });
    m
}

/// Binds one action at this level.
fn leaf(map: &mut HashMap<Chord, LeaderNode>, spec: &str, action: Action) {
    match Chord::parse(spec) {
        Some(chord) => {
            map.insert(chord, LeaderNode::Run(action));
        }
        None => debug_assert!(false, "built-in leader chord {spec:?} does not parse"),
    }
}

/// Opens a group at this level and fills it.
fn group(
    map: &mut HashMap<Chord, LeaderNode>,
    spec: &str,
    title: &'static str,
    fill: impl FnOnce(&mut HashMap<Chord, LeaderNode>),
) {
    let mut keys = HashMap::new();
    fill(&mut keys);
    match Chord::parse(spec) {
        Some(chord) => {
            map.insert(chord, LeaderNode::Group { title, keys });
        }
        None => debug_assert!(false, "built-in leader group {spec:?} does not parse"),
    }
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
    let mut hints: HashMap<String, String> =
        pretty.iter().map(|(a, k)| (a.to_string(), k.to_string())).collect();
    // Actions with no chord of their own — roughly a third of them, and every one
    // of the leader-only ones — would otherwise show a blank hint column, which
    // reads as "no way to reach this but the palette". Fall back to the leader
    // path, walked from the real bindings so the palette teaches the layer.
    fn walk(level: &HashMap<Chord, LeaderNode>, path: &mut Vec<String>, out: &mut HashMap<String, String>) {
        for (chord, node) in level {
            // The label verbatim, not upper-cased: `h` is focus and `H` (shift+h)
            // is resize, so folding the case would print a hint for the wrong one.
            path.push(chord.label());
            match node {
                LeaderNode::Run(action) => {
                    let hint = format!("Leader {}", path.join(" "));
                    // Several keys can reach one action (H/L and the arrows both
                    // move a tab). Keep the shortest, then the alphabetically
                    // first, so the hint is stable across runs of the HashMap.
                    out.entry(action.id().to_string())
                        .and_modify(|prev| {
                            if (hint.len(), &hint) < (prev.len(), prev) {
                                *prev = hint.clone();
                            }
                        })
                        .or_insert_with(|| hint.clone());
                }
                LeaderNode::Group { keys, .. } => walk(keys, path, out),
            }
            path.pop();
        }
    }
    let mut leader = HashMap::new();
    walk(&default_leader_bindings(), &mut Vec::new(), &mut leader);
    for (id, hint) in leader {
        hints.entry(id).or_insert(hint);
    }
    hints
}

#[cfg(test)]
mod chord_roundtrip_tests {
    use super::*;

    #[test]
    fn a_chord_spec_becomes_the_keypress_that_produces_it() {
        // The remote-control `key` command stands on this: what `chord_to_key`
        // builds must chord back to what was asked for, or a scripted keypress is
        // not the keypress it names.
        for spec in [
            "alt+shift+space",
            "ctrl+shift+t",
            "enter",
            "escape",
            "j",
            "shift+j",
            "f1",
            "pagedown",
            "]",
        ] {
            let (key, mods) = chord_to_key(spec).unwrap_or_else(|| panic!("{spec} does not parse"));
            assert_eq!(
                Chord::from_event(&key, mods),
                Chord::parse(spec),
                "{spec} did not round-trip"
            );
        }
        assert!(chord_to_key("not+a+key").is_none());

        // A shifted letter has to arrive as the UPPERCASE character, because that is
        // what a keyboard sends and what `"G"`-style arms match. Sending "g" with a
        // shift modifier made every shifted letter do the unshifted thing.
        let (key, mods) = chord_to_key("shift+g").unwrap();
        assert_eq!(key, Key::Character("G".into()));
        assert!(mods.shift_key());
        let (key, _) = chord_to_key("shift+]").unwrap();
        assert_eq!(key, Key::Character("]".into()), "punctuation is not upper-cased");
    }
}

/// Turns a chord spec into the key and modifiers a real press of it would carry.
///
/// The inverse of `Chord::from_event`, for the remote-control `key` command: a
/// `winit::KeyEvent` cannot be built outside winit, so a scripted keypress has to
/// enter the app as the (key, modifiers) pair the handlers actually read. Going
/// through `Chord::parse` means the spellings are the SAME ones the config accepts.
pub fn chord_to_key(spec: &str) -> Option<(Key, ModifiersState)> {
    let chord = Chord::parse(spec)?;
    let key = match chord.key {
        // With shift held, a keyboard delivers the UPPERCASE character, and the
        // handlers match on it (`"G"` is a different binding from `"g"`). Sending
        // the lowercase one with a shift modifier is not what a hand produces, and
        // every shifted letter would quietly do the unshifted thing.
        ChordKey::Char(c) if chord.shift && c.is_alphabetic() => Key::Character(
            winit::keyboard::SmolStr::new(c.to_uppercase().collect::<String>()),
        ),
        ChordKey::Char(c) => Key::Character(winit::keyboard::SmolStr::new(c.to_string())),
        ChordKey::Named(id) => Key::Named(named_key(id)?),
    };
    let mut mods = ModifiersState::empty();
    mods.set(ModifiersState::CONTROL, chord.ctrl);
    mods.set(ModifiersState::SHIFT, chord.shift);
    mods.set(ModifiersState::ALT, chord.alt);
    mods.set(ModifiersState::SUPER, chord.supr);
    Some((key, mods))
}

/// The `NamedKey` behind a canonical id — the inverse of `named_id`.
fn named_key(id: &str) -> Option<NamedKey> {
    Some(match id {
        "enter" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "escape" => NamedKey::Escape,
        "backspace" => NamedKey::Backspace,
        "delete" => NamedKey::Delete,
        "up" => NamedKey::ArrowUp,
        "down" => NamedKey::ArrowDown,
        "left" => NamedKey::ArrowLeft,
        "right" => NamedKey::ArrowRight,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        _ => return None,
    })
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

    /// Walks a written sequence (`"t n"`) the way the key handler would, and
    /// returns the action it lands on — `None` for a miss or a bare group.
    fn seq<'a>(map: &'a Keymap, spec: &str) -> Option<&'a Action> {
        let path: Vec<Chord> = spec.split_whitespace().map(|s| Chord::parse(s).unwrap()).collect();
        match map.resolve_leader(&path) {
            Some(LeaderNode::Run(a)) => Some(a),
            _ => None,
        }
    }

    /// The same, but from real key events, so the modifier relaxation is exercised.
    fn seq_events<'a>(map: &'a Keymap, keys: &[(Key, ModifiersState)]) -> Option<&'a Action> {
        let path: Vec<Chord> =
            keys.iter().map(|(k, m)| Chord::from_event(k, *m).unwrap()).collect();
        match map.resolve_leader(&path) {
            Some(LeaderNode::Run(a)) => Some(a),
            _ => None,
        }
    }

    #[test]
    fn leader_layer_binds_plain_keys_without_touching_the_normal_map() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        assert_eq!(seq(&map, "v"), Some(&Action::ClipboardHistory));
        assert_eq!(seq(&map, "3"), Some(&Action::GoToTab(3)));
        // A bare `v` must stay a literal `v` outside the leader layer.
        assert_eq!(map.bindings.get(&Chord::parse("v").unwrap()), None);
    }

    #[test]
    fn leader_groups_take_a_second_key() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        assert_eq!(seq(&map, "r c"), Some(&Action::LaunchClaude));
        assert_eq!(seq(&map, "t t"), Some(&Action::NewTab));
        assert_eq!(seq(&map, "c v"), Some(&Action::Paste));
        // A group on its own runs nothing — it waits.
        assert!(matches!(map.resolve_leader(&[Chord::parse("r").unwrap()]), Some(LeaderNode::Group { .. })));
        assert_eq!(seq(&map, "r"), None);
        // And a miss inside a group is a miss, not a fall back to the root.
        assert_eq!(seq(&map, "r 1"), None);
    }

    #[test]
    fn every_action_with_a_normal_binding_is_reachable_from_the_leader() {
        // The layer is meant to be a superset: anything on ctrl+shift must also
        // have a leader path, or "everything is under the leader" is a lie.
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let mut reachable = Vec::new();
        fn walk(level: &HashMap<Chord, LeaderNode>, out: &mut Vec<String>) {
            for node in level.values() {
                match node {
                    LeaderNode::Run(a) => out.push(a.id().to_string()),
                    LeaderNode::Group { keys, .. } => walk(keys, out),
                }
            }
        }
        walk(&map.leader_bindings, &mut reachable);
        for action in default_bindings().values() {
            assert!(
                reachable.iter().any(|id| id == action.id()),
                "{} has a chord but no leader path",
                action.id()
            );
        }
    }

    #[test]
    fn leader_layer_ignores_the_modifiers_still_held_from_the_leader() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let one = Key::Character("1".into());
        // Nobody lets go of alt+shift between the leader and `1`.
        assert_eq!(seq_events(&map, &[(one.clone(), ModifiersState::ALT)]), Some(&Action::GoToTab(1)));
        assert_eq!(
            seq_events(&map, &[(one.clone(), ModifiersState::ALT | ModifiersState::SHIFT)]),
            Some(&Action::GoToTab(1))
        );
        assert_eq!(seq_events(&map, &[(one, ModifiersState::empty())]), Some(&Action::GoToTab(1)));
        // Held modifiers are dropped at every level, not just the first.
        assert_eq!(
            seq_events(
                &map,
                &[
                    (Key::Character("r".into()), ModifiersState::ALT),
                    (Key::Character("c".into()), ModifiersState::ALT),
                ]
            ),
            Some(&Action::LaunchClaude)
        );
        // An unbound key is still unbound, however it is modified.
        assert_eq!(seq_events(&map, &[(Key::Character("ñ".into()), ModifiersState::ALT)]), None);
    }

    #[test]
    fn a_shifted_key_binds_apart_from_its_unshifted_one() {
        // Focus is hjkl, resize is HJKL: the exact chord has to win, or the shifted
        // row would relax straight back into focus.
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let h = Key::Character("h".into());
        assert_eq!(seq_events(&map, &[(h.clone(), ModifiersState::empty())]), Some(&Action::FocusLeft));
        assert_eq!(seq_events(&map, &[(h, ModifiersState::SHIFT)]), Some(&Action::ResizeLeft));
    }

    #[test]
    fn user_can_bind_and_disable_the_leader_layer() {
        let mut user = HashMap::new();
        user.insert("leader+z".into(), "quit".into());
        // A sequence binds into a group, creating it if it does not exist.
        user.insert("leader+t x".into(), "close_tab".into());
        user.insert("leader+w a b".into(), "new_tab".into());
        let map = Keymap::new(&user, "alt+shift+j");
        assert_eq!(seq(&map, "z"), Some(&Action::Quit));
        assert_eq!(seq(&map, "t x"), Some(&Action::CloseTab));
        assert_eq!(seq(&map, "w a b"), Some(&Action::NewTab));
        assert_eq!(map.leader, Chord::parse("alt+shift+j"));
        // The defaults survive a user addition, in the same group and elsewhere.
        assert_eq!(seq(&map, "t t"), Some(&Action::NewTab));
        assert_eq!(seq(&map, "v"), Some(&Action::ClipboardHistory));

        // An empty spec turns the layer off rather than falling back to a chord the
        // user did not ask for.
        assert_eq!(Keymap::new(&HashMap::new(), "").leader, None);
    }

    #[test]
    fn exact_leader_chord_wins_over_the_relaxed_match() {
        let mut user = HashMap::new();
        user.insert("leader+ctrl+v".into(), "quit".into());
        let map = Keymap::new(&user, DEFAULT_LEADER);
        let v = Key::Character("v".into());
        assert_eq!(seq_events(&map, &[(v.clone(), ModifiersState::CONTROL)]), Some(&Action::Quit));
        assert_eq!(
            seq_events(&map, &[(v, ModifiersState::empty())]),
            Some(&Action::ClipboardHistory)
        );
    }

    #[test]
    fn the_panel_labels_named_keys_with_their_glyph() {
        // "equal"/"pagedown" is what you write in the config, not what the panel
        // should print — a column of words eats the width the titles need.
        assert_eq!(Chord::parse("equal").unwrap().label(), "=");
        assert_eq!(Chord::parse("minus").unwrap().label(), "-");
        assert_eq!(Chord::parse("down").unwrap().label(), "↓");
        assert_eq!(Chord::parse("pageup").unwrap().label(), "PgUp");
        // A shifted letter shows as the capital you actually press.
        assert_eq!(Chord::parse("shift+z").unwrap().label(), "Z");
    }

    #[test]
    fn spelled_out_punctuation_matches_the_key_you_actually_press() {
        // The regression this guards: `plus` used to parse to a NAMED key, while
        // the keyboard delivers `Key::Character('+')`. The two could never be equal,
        // so ctrl+plus and leader + silently did nothing for as long as they existed.
        for (spec, ch) in [("plus", '+'), ("minus", '-'), ("equal", '=')] {
            let parsed = Chord::parse(spec).unwrap();
            let pressed = Chord::from_event(&Key::Character(ch.to_string().into()), ModifiersState::empty()).unwrap();
            assert_eq!(parsed, pressed, "{spec} does not match a real {ch} keypress");
            // And the panel shows the glyph, since that is what the chord now holds.
            assert_eq!(parsed.label(), ch.to_string());
        }
        // The same key with a modifier still parses as one chord: `+` is the
        // separator, so the spelled-out name is the only way to write it.
        assert_eq!(
            Chord::parse("ctrl+plus"),
            Chord::from_event(&Key::Character("+".into()), ModifiersState::CONTROL)
        );
    }

    #[test]
    fn font_zoom_has_a_layout_independent_binding() {
        // +, - and = are not one keypress on every layout (on the Spanish one `=`
        // is shift+0), so the letters have to work on their own.
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        assert_eq!(seq(&map, "z"), Some(&Action::FontBigger));
        assert_eq!(seq(&map, "shift+z"), Some(&Action::FontSmaller));
        assert_eq!(seq(&map, "0"), Some(&Action::FontReset));
        // The symbols stay as aliases for the layouts where they are one key.
        assert_eq!(seq(&map, "minus"), Some(&Action::FontSmaller));
        assert_eq!(seq(&map, "plus"), Some(&Action::FontBigger));
    }

    #[test]
    fn the_which_key_panel_lists_a_level_in_scan_order() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let root = map.leader_entries(&[]);
        // Digits first, then letters: hash order would make the panel unreadable.
        assert_eq!(root.first().map(|(k, ..)| k.as_str()), Some("0"));
        let launch = map.leader_entries(&[Chord::parse("r").unwrap()]);
        assert!(launch.iter().any(|(k, t, group)| k == "c" && t.contains("Claude") && !group));
        // Groups are flagged so the panel can colour them apart from actions.
        assert!(root.iter().any(|(k, _, group)| k == "r" && *group));
        // ...and hjkl stay actions at the root, not groups.
        assert!(root.iter().any(|(k, _, group)| k == "l" && !*group));
        // A level that is an action, not a group, has nothing to list.
        assert!(map.leader_entries(&[Chord::parse("v").unwrap()]).is_empty());
    }

    #[test]
    fn every_action_the_palette_lists_shows_a_way_to_reach_it() {
        // A blank hint column reads as "the palette is the only way in", which was
        // false for the ~19 leader-only actions. Each one falls back to its path.
        let hints = default_hints();
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        // Every action the leader layer can reach shows how to reach it. (The
        // media keys are deliberately palette-only — nothing binds them.)
        fn reachable(level: &HashMap<Chord, LeaderNode>, out: &mut Vec<&'static str>) {
            for node in level.values() {
                match node {
                    LeaderNode::Run(a) => out.push(a.id()),
                    LeaderNode::Group { keys, .. } => reachable(keys, out),
                }
            }
        }
        let mut ids = Vec::new();
        reachable(&default_leader_bindings(), &mut ids);
        for id in ids {
            let hint = hints.get(id).unwrap_or_else(|| panic!("{id} has no palette hint"));
            assert!(!hint.is_empty(), "{id} has an empty palette hint");
        }
        // The leader-only ones name the layer, and name it correctly.
        assert_eq!(hints.get("quit").map(String::as_str), Some("Leader s q"));
        assert_eq!(hints.get("cycle_layout").map(String::as_str), Some("Leader p c"));
        // An action with a chord keeps the chord: it is the shorter thing to type.
        assert_eq!(hints.get("copy").map(String::as_str), Some("Ctrl+Shift+C"));
        // Shifted steps keep their case: `J` is resize, `j` is focus, and a hint
        // that folded them together would point at the wrong action.
        assert_eq!(hints.get("resize_down").map(String::as_str), Some("Leader J"));
        assert_eq!(hints.get("focus_down").map(String::as_str), Some("Leader j"));
        let _ = &map;
    }

    #[test]
    fn default_palette_shortcut_is_bound() {
        let map = Keymap::new(&HashMap::new(), DEFAULT_LEADER);
        let chord = Chord::parse("ctrl+shift+p").unwrap();
        assert_eq!(map.bindings.get(&chord), Some(&Action::CommandPalette));
    }
}
