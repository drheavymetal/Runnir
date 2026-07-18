//! Overlays: the command palette, the docs viewer, the rename prompt, the AI
//! panel, hint mode.
//!
//! Each overlay renders as an ordinary `Grid` drawn into a centered rect — the
//! renderer already draws grids, so overlays reuse that path instead of inventing
//! a second UI system. An overlay builds its grid on demand from its state.

use crate::actions::Action;
use crate::config::Theme;
use crate::grid::{Cell, Color, Flags, Grid, Pen};

/// Which overlay, if any, is capturing input. Only one is active at a time.
pub enum Overlay {
    Palette(Palette),
    Docs(Docs),
    Prompt(Prompt),
    Ai(AiPanel),
    Hints(Hints),
    Search(Search),
    Config(ConfigPanel),
    Theme(ThemePicker),
    ClipHistory(ClipHistoryPicker),
}

impl Overlay {
    /// Grids to draw for this overlay, each with its `(col, row)` cell origin
    /// inside a grid of `cols` x `rows`. The caller turns cell origins into pixels.
    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        match self {
            Overlay::Palette(p) => p.render(cols, rows, theme),
            Overlay::Docs(d) => d.render(cols, rows, theme),
            Overlay::Prompt(p) => p.render(cols, rows, theme),
            Overlay::Ai(a) => a.render(cols, rows, theme),
            Overlay::Hints(_) => Vec::new(), // Hints annotate panes, drawn elsewhere.
            Overlay::Search(s) => s.render(cols, rows, theme),
            Overlay::Config(c) => c.render(cols, rows, theme),
            Overlay::Theme(t) => t.render(cols, rows, theme),
            Overlay::ClipHistory(p) => p.render(cols, rows, theme),
        }
    }
}

/// Incremental scrollback search. The matches themselves are highlighted in the
/// pane by the renderer; this overlay is the little query bar at the bottom.
pub struct Search {
    pub query: String,
    /// Absolute `(row, col)` of each match, in order.
    pub matches: Vec<(usize, usize)>,
    pub current: usize,
}

impl Search {
    pub fn new() -> Self {
        Self { query: String::new(), matches: Vec::new(), current: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn set_matches(&mut self, matches: Vec<(usize, usize)>) {
        self.matches = matches;
        self.current = 0;
    }

    pub fn next(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + 1) % self.matches.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        }
    }

    pub fn current_match(&self) -> Option<(usize, usize)> {
        self.matches.get(self.current).copied()
    }

    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.min(60).max(20);
        let mut g = panel_grid(w, 1, theme);
        let count = if self.matches.is_empty() {
            if self.query.is_empty() { String::new() } else { " no matches".into() }
        } else {
            format!(" {}/{}", self.current + 1, self.matches.len())
        };
        let line = format!("/{}", self.query);
        write(&mut g, 0, 1, &line, normal());
        write(&mut g, 0, 1 + line.chars().count(), " ", selected());
        write(&mut g, 0, w.saturating_sub(count.chars().count() + 1), &count, dim());
        // Anchored to the bottom row, vim-style.
        vec![Panel { grid: g, col: 0, row: rows.saturating_sub(1) }]
    }
}

/// A grid plus where it sits, in cells.
pub struct Panel {
    pub grid: Grid,
    pub col: usize,
    pub row: usize,
}

// ---- shared drawing helpers -----------------------------------------------

fn panel_grid(cols: usize, rows: usize, theme: &Theme) -> Grid {
    let mut grid = Grid::new(cols, rows);
    // Fill with the panel background so it occludes the dimmed terminal behind.
    let bg = Pen { bg: Color::Rgb(0x1c, 0x1d, 0x22), ..Pen::default() };
    grid.fill(bg);
    let _ = theme;
    grid
}

fn write(grid: &mut Grid, row: usize, col: usize, text: &str, pen: Pen) {
    grid.write_str(row, col, text, pen);
}

const PANEL_BG: (u8, u8, u8) = (0x1c, 0x1d, 0x22);
const ACCENT: (u8, u8, u8) = (0x4c, 0x9f, 0xd4);
const DIMFG: (u8, u8, u8) = (0x8a, 0x8d, 0x94);

fn accent() -> Pen {
    Pen { fg: Color::Rgb(ACCENT.0, ACCENT.1, ACCENT.2), bg: bg(), ..Pen::default() }
}
fn dim() -> Pen {
    Pen { fg: Color::Rgb(DIMFG.0, DIMFG.1, DIMFG.2), bg: bg(), ..Pen::default() }
}
fn normal() -> Pen {
    Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), bg: bg(), ..Pen::default() }
}
fn selected() -> Pen {
    Pen { fg: Color::Rgb(0x0d, 0x0d, 0x0f), bg: Color::Rgb(ACCENT.0, ACCENT.1, ACCENT.2), ..Pen::default() }
}
fn bg() -> Color {
    Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2)
}

// ---- settings panel --------------------------------------------------------

use crate::config::Config;
use crate::settings::{self, Kind, Row};

/// The interactive settings editor. Holds a working `Config`; edits apply live and
/// are persisted to JSON on save. `dirty` signals the host to apply the change.
pub struct ConfigPanel {
    pub config: Config,
    rows: Vec<Row>,
    pub cursor: usize,
    /// Inline text-edit buffer when editing a Text setting, else `None`.
    pub editing: Option<String>,
    /// Set after a change so the host re-applies `config`; cleared by the host.
    pub dirty: bool,
    /// Transient status line ("saved", "save failed: …").
    pub status: String,
}

impl ConfigPanel {
    pub fn new(config: Config) -> Self {
        Self { config, rows: settings::rows(), cursor: 0, editing: None, dirty: false, status: String::new() }
    }

    fn id(&self) -> settings::SettingId {
        self.rows[self.cursor].id
    }
    fn kind(&self) -> Kind {
        self.rows[self.cursor].kind
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn down(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }

    /// Left/right arrow or h/l: step numbers, toggle bools, cycle enums.
    pub fn adjust(&mut self, dir: i32) {
        let id = self.id();
        settings::adjust(&mut self.config, id, dir);
        self.dirty = true;
    }

    /// Space/Enter: toggle bool, cycle enum, or begin editing a text field.
    pub fn activate(&mut self) {
        match self.kind() {
            Kind::Bool | Kind::Enum => self.adjust(1),
            Kind::Text => {
                // Seed from the raw config value, not the display string (which shows
                // "(none)" for an unset path).
                let seed = match self.id() {
                    settings::SettingId::Background => {
                        self.config.window.background.clone().unwrap_or_default()
                    }
                    settings::SettingId::FontFamily => self.config.font.family.clone(),
                    _ => String::new(),
                };
                self.editing = Some(seed);
            }
            Kind::Float | Kind::Int => self.adjust(1),
        }
    }

    pub fn input_char(&mut self, c: char) {
        if let Some(buf) = self.editing.as_mut() {
            buf.push(c);
        }
    }
    pub fn backspace(&mut self) {
        if let Some(buf) = self.editing.as_mut() {
            buf.pop();
        }
    }
    /// Commit the inline text edit.
    pub fn commit_edit(&mut self) {
        if let Some(buf) = self.editing.take() {
            let id = self.id();
            settings::set_text(&mut self.config, id, buf);
            self.dirty = true;
        }
    }
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Persists the working config as JSON.
    pub fn save(&mut self) {
        self.status = match self.config.save_json() {
            Ok(()) => "saved to runnir.json".into(),
            Err(e) => format!("save failed: {e}"),
        };
        // Mark dirty so the host re-adopts and refreshes the config-file mtime — the
        // just-written file must not then trigger a redundant hot-reload + toast.
        self.dirty = true;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 7 / 10).clamp(44, 84).min(cols.saturating_sub(2).max(1));
        let visible = (rows.saturating_sub(6)).clamp(6, self.rows.len() + 8);
        let h = (visible + 4).min(rows.saturating_sub(2)).max(8);
        let mut g = panel_grid(w, h, theme);
        let _ = theme;

        write(&mut g, 0, 2, "Settings", accent());
        write(&mut g, 0, w.saturating_sub(30), "\u{2191}\u{2193} move  \u{2190}\u{2192} change  s save", dim());

        // Scroll so the cursor stays visible in the list area (rows 2..h-2).
        let list_h = h.saturating_sub(3);
        let top = self.cursor.saturating_sub(list_h.saturating_sub(1)).min(self.rows.len().saturating_sub(list_h).max(0));

        let mut last_section = "";
        for (screen, i) in (top..self.rows.len()).take(list_h).enumerate() {
            let row = 2 + screen;
            let r = &self.rows[i];
            let sel = i == self.cursor;
            if sel {
                for c in 0..w {
                    write(&mut g, row, c, " ", selected());
                }
            }
            let pen = if sel { selected() } else { normal() };
            let sec = if r.section != last_section { r.section } else { "" };
            last_section = r.section;
            write(&mut g, row, 2, sec, if sel { selected() } else { accent() });
            write(&mut g, row, 12, r.label, pen);
            let val = if sel && self.editing.is_some() {
                format!("{}\u{2588}", self.editing.as_deref().unwrap_or(""))
            } else {
                settings::value(&self.config, r.id)
            };
            let vcol = w.saturating_sub(val.chars().count() + 2);
            write(&mut g, row, vcol.max(38), &val, pen);
        }

        if !self.status.is_empty() {
            write(&mut g, h - 1, 2, &self.status, dim());
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 4;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- command palette -------------------------------------------------------

pub struct Palette {
    query: String,
    all: Vec<(Action, String)>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl Palette {
    pub fn new(keyhints: &std::collections::HashMap<String, String>) -> Self {
        let all: Vec<(Action, String)> = Action::palette_list()
            .into_iter()
            .map(|a| {
                let hint = keyhints.get(a.id()).cloned().unwrap_or_default();
                (a, hint)
            })
            .collect();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<Action> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0.clone())
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.all.len())
            .filter(|&i| fuzzy(&self.all[i].0.title().to_lowercase(), &q))
            .collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(30, 70).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len()).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Command palette", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        // Cursor block after the query.
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        // Scroll the list so the cursor is always on screen, even past `visible`.
        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let (action, hint) = &self.all[idx];
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            // Paint the whole selected row so the highlight is a bar, not just text.
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, action.title(), pen);
            if !hint.is_empty() {
                let hp = if sel { selected() } else { dim() };
                let x = w.saturating_sub(hint.chars().count() + 2);
                write(&mut g, row, x, hint, hp);
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- theme picker ----------------------------------------------------------

/// A fuzzy-filterable list of the bundled colour themes. Modelled on [`Palette`]:
/// arrows move, typing filters. What sets it apart is *live preview* — the host
/// applies the highlighted theme to the renderer as the selection moves, so the
/// terminal behind the picker updates immediately. The theme active when it opened
/// is stashed in `original` so cancelling can restore it untouched.
pub struct ThemePicker {
    query: String,
    all: Vec<(&'static str, Theme)>,
    filtered: Vec<usize>,
    cursor: usize,
    original: Theme,
}

impl ThemePicker {
    pub fn new(original: Theme) -> Self {
        let all = crate::themes::builtins();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0, original }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    /// The theme under the cursor — what the host previews live and keeps on Enter.
    pub fn selected_theme(&self) -> Option<Theme> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].1.clone())
    }

    /// Name of the highlighted theme, for a status toast on confirm.
    pub fn selected_name(&self) -> Option<&'static str> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0)
    }

    /// The theme that was in effect when the picker opened; restored on cancel.
    pub fn original(&self) -> Theme {
        self.original.clone()
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.all.len())
            .filter(|&i| fuzzy(&self.all[i].0.to_lowercase(), &q))
            .collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(34, 74).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len()).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Theme picker", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        // A swatch strip previews each theme's palette inline: background, the six
        // vivid ANSI colours, then foreground — enough to judge a theme at a glance
        // without moving the selection onto it.
        const SWATCH: usize = 8;
        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let (name, t) = &self.all[idx];
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, name, pen);
            // Draw the swatch flush-right, one cell per colour.
            if w > SWATCH + 4 {
                let cols_of = |t: &Theme| -> [(u8, u8, u8); SWATCH] {
                    let a = &t.ansi;
                    let g = |i: usize| a.get(i).map(|c| (c.0, c.1, c.2)).unwrap_or((0, 0, 0));
                    [
                        (t.background.0, t.background.1, t.background.2),
                        g(1),
                        g(2),
                        g(3),
                        g(4),
                        g(5),
                        g(6),
                        (t.foreground.0, t.foreground.1, t.foreground.2),
                    ]
                };
                let strip = cols_of(t);
                let base = w.saturating_sub(SWATCH + 1);
                for (i, (r, gg, b)) in strip.iter().enumerate() {
                    let cell = Pen { bg: Color::Rgb(*r, *gg, *b), ..Pen::default() };
                    write(&mut g, row, base + i, " ", cell);
                }
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- clipboard history picker ---------------------------------------------

/// A fuzzy-filterable list of recent clipboard copies, newest first. Modelled on
/// [`Palette`]: arrows move, typing filters. Each row shows a one-line, truncated
/// preview of the entry; confirming pastes the full entry into the focused pane via
/// the normal paste path. The full text is kept alongside the preview so a
/// multi-line copy pastes whole even though only its first line is shown.
pub struct ClipHistoryPicker {
    query: String,
    /// (full entry, one-line preview), newest first.
    all: Vec<(String, String)>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl ClipHistoryPicker {
    pub fn new(entries: &std::collections::VecDeque<String>) -> Self {
        let all: Vec<(String, String)> =
            entries.iter().map(|e| (e.clone(), clip_preview(e))).collect();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    /// The full text of the highlighted entry — what gets pasted on confirm.
    pub fn selected(&self) -> Option<String> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0.clone())
    }

    fn refilter(&mut self) {
        // Filter against the whole entry, not just the shown preview, so a match on a
        // later line still surfaces it.
        let q = self.query.to_lowercase();
        self.filtered =
            (0..self.all.len()).filter(|&i| fuzzy(&self.all[i].0.to_lowercase(), &q)).collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(30, 80).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len().max(1)).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Clipboard history", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        if self.all.is_empty() {
            write(&mut g, 3, 2, "nothing copied yet", dim());
        } else if self.filtered.is_empty() {
            write(&mut g, 3, 2, "no matches", dim());
        }

        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let preview = &self.all[idx].1;
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            let clipped: String = preview.chars().take(w.saturating_sub(4)).collect();
            write(&mut g, row, 2, &clipped, pen);
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

/// A one-line, length-capped preview of a clipboard entry for the picker list: the
/// first non-blank line, trimmed, with a marker when more lines follow.
fn clip_preview(entry: &str) -> String {
    const CAP: usize = 76;
    let first = entry.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let multiline = entry.lines().filter(|l| !l.trim().is_empty()).count() > 1;
    let mut out: String = if first.chars().count() > CAP {
        let mut s: String = first.chars().take(CAP - 1).collect();
        s.push('\u{2026}');
        s
    } else {
        first.to_string()
    };
    if multiline {
        out.push_str(" \u{00b6}"); // pilcrow: this entry spans more than one line
    }
    out
}

// ---- docs ------------------------------------------------------------------

pub struct Docs {
    lines: Vec<(String, Pen)>,
    scroll: usize,
}

impl Docs {
    pub fn new(text: &str) -> Self {
        let lines = text
            .lines()
            .map(|l| {
                // A leading '#' marks a heading; '@' a key hint line.
                if let Some(h) = l.strip_prefix("# ") {
                    (h.to_string(), accent())
                } else if let Some(h) = l.strip_prefix("@ ") {
                    (h.to_string(), dim())
                } else {
                    (l.to_string(), normal())
                }
            })
            .collect();
        Self { lines, scroll: 0 }
    }

    pub fn scroll(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.lines.len().saturating_sub(1) as isize) as usize;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.saturating_sub(6).clamp(20, 100);
        let h = rows.saturating_sub(4).max(6);
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "runnir — help   (Esc to close, ↑/↓ to scroll)", accent());
        let body = h - 2;
        for (line, (text, pen)) in self.lines.iter().skip(self.scroll).take(body).enumerate() {
            let clipped: String = text.chars().take(w.saturating_sub(4)).collect();
            write(&mut g, 2 + line, 2, &clipped, *pen);
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 2;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- prompt (rename, ask, connect) ----------------------------------------

pub struct Prompt {
    pub kind: PromptKind,
    pub label: String,
    pub input: String,
    pub suggestions: Vec<String>,
    pub cursor: usize,
}

/// How many suggestion rows a prompt renders (and thus how far Down navigates).
const PROMPT_ROWS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    RenameTab,
    QuickConnect,
    /// Natural-language description to translate into a shell command.
    AiCommand,
    /// A whisper: a natural-language instruction turned into terminal actions.
    Whisper,
    /// A destructive command held at Enter by the guardian: confirm to run it.
    GuardedCommand,
    /// A shell-history line to type at the prompt (fuzzy-picked, not run).
    HistoryInsert,
    /// A keyword to watch for in the focused pane's output (empty clears it).
    WatchKeyword,
    /// A named layout to launch in a new tab.
    LaunchLayout,
}

impl Prompt {
    pub fn new(kind: PromptKind, label: &str, suggestions: Vec<String>) -> Self {
        Self { kind, label: label.into(), input: String::new(), suggestions, cursor: 0 }
    }

    pub fn input_char(&mut self, c: char) {
        self.input.push(c);
        self.cursor = 0;
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        // Only the first PROMPT_ROWS suggestions are rendered, so navigation stops
        // there — otherwise the highlight would leave the visible list and Enter
        // would insert an entry the user never saw. Type to narrow the list instead.
        let cap = self.visible().len().min(PROMPT_ROWS);
        if self.cursor + 1 < cap {
            self.cursor += 1;
        }
    }

    /// Suggestions matching the current input.
    pub fn visible(&self) -> Vec<String> {
        if self.input.is_empty() {
            return self.suggestions.clone();
        }
        let q = self.input.to_lowercase();
        self.suggestions.iter().filter(|s| s.to_lowercase().contains(&q)).cloned().collect()
    }

    /// What confirming yields: the highlighted suggestion, or the raw input.
    pub fn value(&self) -> String {
        self.visible().get(self.cursor).cloned().unwrap_or_else(|| self.input.clone())
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let visible = self.visible();
        let w = (cols * 6 / 10).clamp(30, 70);
        let list = visible.len().min(PROMPT_ROWS);
        let h = 3 + list;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, &self.label, accent());
        let line = format!("> {}", self.input);
        write(&mut g, 1, 2, &line, normal());
        write(&mut g, 1, 2 + line.chars().count(), " ", selected());

        for (i, s) in visible.iter().take(list).enumerate() {
            let row = 3 + i;
            let pen = if i == self.cursor { selected() } else { normal() };
            if i == self.cursor {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, s, pen);
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- AI panel --------------------------------------------------------------

pub struct AiPanel {
    pub provider: String,
    pub input: String,
    pub transcript: Vec<AiLine>,
    pub busy: bool,
    scroll: usize,
}

pub struct AiLine {
    pub who: Who,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Who {
    You,
    Assistant,
    System,
}

impl AiPanel {
    pub fn new(provider: String) -> Self {
        Self { provider, input: String::new(), transcript: Vec::new(), busy: false, scroll: 0 }
    }

    pub fn push(&mut self, who: Who, text: String) {
        self.transcript.push(AiLine { who, text });
        self.scroll = 0;
    }

    pub fn input_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        // Anchored to the right third, full height: a side panel, not a modal, so
        // you can read the terminal and the answer at once.
        let w = (cols / 3).clamp(30, 60).min(cols.saturating_sub(2));
        let h = rows.saturating_sub(2).max(6);
        let mut g = panel_grid(w, h, theme);

        let head = format!("AI · {}{}", self.provider, if self.busy { " · thinking…" } else { "" });
        write(&mut g, 0, 2, &head, accent());

        // Wrap the transcript into the panel width, newest at the bottom.
        let inner = w.saturating_sub(4);
        let mut wrapped: Vec<(Who, String)> = Vec::new();
        for line in &self.transcript {
            for chunk in wrap(&line.text, inner) {
                wrapped.push((line.who, chunk));
            }
            wrapped.push((line.who, String::new()));
        }
        let body = h.saturating_sub(4);
        let start = wrapped.len().saturating_sub(body + self.scroll);
        for (i, (who, text)) in wrapped.iter().skip(start).take(body).enumerate() {
            let pen = match who {
                Who::You => accent(),
                Who::Assistant => normal(),
                Who::System => dim(),
            };
            write(&mut g, 2 + i, 2, text, pen);
        }

        let prompt = format!("> {}", self.input);
        write(&mut g, h - 1, 2, &prompt, normal());
        write(&mut g, h - 1, 2 + prompt.chars().count(), " ", selected());

        let col = cols.saturating_sub(w);
        vec![Panel { grid: g, col, row: 1 }]
    }
}

// ---- hint mode -------------------------------------------------------------

/// A screen target the user can jump to by typing its label.
pub struct Hint {
    pub label: String,
    pub abs_row: usize,
    pub col: usize,
    pub text: String,
    pub kind: HintKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintKind {
    Url,
    Path,
    Hash,
}

pub struct Hints {
    pub hints: Vec<Hint>,
    pub typed: String,
}

impl Hints {
    pub fn new(hints: Vec<Hint>) -> Self {
        Self { hints, typed: String::new() }
    }

    pub fn input(&mut self, c: char) -> HintResult {
        self.typed.push(c.to_ascii_lowercase());
        let matches: Vec<&Hint> =
            self.hints.iter().filter(|h| h.label.starts_with(&self.typed)).collect();
        match matches.as_slice() {
            [] => HintResult::NoMatch,
            [only] if only.label == self.typed => {
                HintResult::Chosen(only.text.clone(), only.kind)
            }
            _ => HintResult::More,
        }
    }
}

pub enum HintResult {
    More,
    NoMatch,
    Chosen(String, HintKind),
}

/// Two-letter labels from a home-row alphabet, enough for ~600 targets, assigned
/// so no label is a prefix of another.
pub fn hint_labels(n: usize) -> Vec<String> {
    const ALPHA: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";
    if n <= ALPHA.len() {
        return ALPHA.iter().take(n).map(|&b| (b as char).to_string()).collect();
    }
    let mut out = Vec::new();
    for &a in ALPHA {
        for &b in ALPHA {
            out.push(format!("{}{}", a as char, b as char));
            if out.len() == n {
                return out;
            }
        }
    }
    out
}

// ---- fuzzy + wrap ----------------------------------------------------------

/// Subsequence match: every character of `needle`, in order, appears in `hay`.
/// The palette does not need ranking, only filtering.
fn fuzzy(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut chars = hay.chars();
    needle.chars().all(|nc| chars.any(|hc| hc == nc))
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in para.split(' ') {
            if line.chars().count() + word.chars().count() + 1 > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            // A word longer than the panel is hard-split rather than overflowing.
            if word.chars().count() > width {
                for chunk in word.chars().collect::<Vec<_>>().chunks(width) {
                    out.push(chunk.iter().collect());
                }
            } else {
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

// Silence unused warnings for helper kept for symmetry.
#[allow(dead_code)]
fn _cell_marker() -> Cell {
    Cell { ch: ' ', pen: Pen { flags: Flags::empty(), ..Pen::default() } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn palette_filters_by_subsequence() {
        let mut p = Palette::new(&HashMap::new());
        let before = p.filtered.len();
        assert!(before > 5);
        for c in "split".chars() {
            p.input(c);
        }
        assert!(p.filtered.len() < before);
        // Every survivor must actually contain the subsequence.
        for &i in &p.filtered {
            assert!(fuzzy(&p.all[i].0.title().to_lowercase(), "split"));
        }
    }

    #[test]
    fn palette_selection_moves_and_clamps() {
        let mut p = Palette::new(&HashMap::new());
        p.up(); // already at top
        assert_eq!(p.cursor, 0);
        p.down();
        assert_eq!(p.cursor, 1);
        assert!(p.selected().is_some());
    }

    #[test]
    fn fuzzy_needs_every_char_in_order() {
        assert!(fuzzy("new tab", "ntab"));
        assert!(fuzzy("new tab", "nt"));
        assert!(!fuzzy("new tab", "tn"), "order matters");
        assert!(fuzzy("anything", ""));
    }

    #[test]
    fn hint_labels_never_prefix_each_other() {
        // If one label were a prefix of another, typing it could not disambiguate.
        let labels = hint_labels(200);
        assert_eq!(labels.len(), 200);
        assert!(labels.iter().all(|l| l.len() == 2), "past the alphabet all are 2 chars");
        let set: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(set.len(), 200, "labels must be unique");
    }

    #[test]
    fn hints_resolve_on_full_label() {
        let hints = vec![
            Hint { label: "a".into(), abs_row: 0, col: 0, text: "x".into(), kind: HintKind::Url },
            Hint { label: "s".into(), abs_row: 1, col: 0, text: "y".into(), kind: HintKind::Path },
        ];
        let mut h = Hints::new(hints);
        assert!(matches!(h.input('a'), HintResult::Chosen(_, _)));
    }

    #[test]
    fn wrap_breaks_at_width_and_hard_splits_long_words() {
        let lines = wrap("the quick brown fox", 9);
        assert!(lines.iter().all(|l| l.chars().count() <= 9), "{lines:?}");
        let long = wrap("supercalifragilistic", 5);
        assert!(long.iter().all(|l| l.chars().count() <= 5));
    }

    #[test]
    fn theme_picker_filters_and_navigates() {
        let mut p = ThemePicker::new(Theme::default());
        let before = p.filtered.len();
        assert!(before >= 20, "the picker should list every builtin");
        // Typing narrows the list to matching names.
        for c in "nord".chars() {
            p.input(c);
        }
        assert!(p.filtered.len() < before);
        assert!(p.selected_name().unwrap().to_lowercase().contains("nord"));
        // Backspacing widens it again, and refiltering resets the cursor to the top.
        for _ in 0..4 {
            p.backspace();
        }
        assert_eq!(p.filtered.len(), before);
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn theme_picker_selection_moves_clamps_and_previews() {
        let original = Theme::default();
        let mut p = ThemePicker::new(original.clone());
        p.up(); // already at the top: must not underflow
        assert_eq!(p.cursor, 0);
        let first = p.selected_theme().unwrap();
        p.down();
        assert_eq!(p.cursor, 1);
        let second = p.selected_theme().unwrap();
        assert_ne!(first.background, second.background, "moving must preview a new theme");
        // The theme active on open is preserved verbatim for a cancel to restore.
        assert_eq!(p.original().background, original.background);
        assert_eq!(p.original().ansi.len(), 16);
    }

    #[test]
    fn clip_picker_previews_filters_and_pastes_full_entry() {
        let entries: std::collections::VecDeque<String> =
            ["first line\nsecond line", "cargo build", "hello world"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let mut p = ClipHistoryPicker::new(&entries);
        // Selecting the top entry yields its full (multi-line) text, not the preview.
        assert_eq!(p.selected().as_deref(), Some("first line\nsecond line"));
        // The preview is one line, first non-blank, marked as multi-line.
        assert!(p.all[0].1.starts_with("first line"));
        assert!(p.all[0].1.contains('\u{00b6}'), "multi-line entries are marked");
        // Typing filters against the full entry text; a match on a body line surfaces it.
        for c in "second".chars() {
            p.input(c);
        }
        assert_eq!(p.filtered.len(), 1);
        assert_eq!(p.selected().as_deref(), Some("first line\nsecond line"));
    }

    #[test]
    fn prompt_value_prefers_highlighted_suggestion() {
        let mut p = Prompt::new(PromptKind::QuickConnect, "ssh", vec!["a".into(), "b".into()]);
        assert_eq!(p.value(), "a");
        p.down();
        assert_eq!(p.value(), "b");
        // With no match, raw input is returned so you can type a new host.
        p.input_char('z');
        assert_eq!(p.value(), "z");
    }
}
