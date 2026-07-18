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
        if self.cursor + 1 < self.visible().len() {
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
        let list = visible.len().min(8);
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
