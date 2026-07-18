use std::collections::VecDeque;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthChar;
use vte::{Params, Perform};

/// Sessions persist scrollback, so every type reachable from a `Cell` is part of
/// an on-disk format. Changing them is a migration, not a refactor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// The shape of an underline decoration. `None` means the cell is not
/// underlined; the rest map to the styled-underline forms neovim/LSP emit for
/// diagnostics (SGR `4:1`..`4:5`). Kept a plain repr-able enum so it packs into
/// one byte in the instance stream.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl UnderlineStyle {
    /// Small integer the shader reads to pick a decoration path.
    pub fn code(self) -> u32 {
        match self {
            UnderlineStyle::None => 0,
            UnderlineStyle::Single => 1,
            UnderlineStyle::Double => 2,
            UnderlineStyle::Curly => 3,
            UnderlineStyle::Dotted => 4,
            UnderlineStyle::Dashed => 5,
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
    pub struct Flags: u8 {
        const BOLD      = 1 << 0;
        const DIM       = 1 << 1;
        const ITALIC    = 1 << 2;
        const UNDERLINE = 1 << 3;
        const REVERSE   = 1 << 4;
        const HIDDEN    = 1 << 5;
        const STRIKE    = 1 << 6;
    }
}

/// Current text attributes. Applied to every cell as it is printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
    /// Underline shape. `Flags::UNDERLINE` is kept in sync (set iff this is not
    /// `None`) so code that only asks "is it underlined?" still works.
    pub underline: UnderlineStyle,
    /// Colour of the underline decoration. `Color::Default` means "follow the
    /// foreground" (SGR 59); anything else is an explicit colour set by SGR 58.
    pub underline_color: Color,
    /// OSC 8 hyperlink id: 0 = none, else index+1 into the grid's `links` table.
    /// Kept as an id (not the URI) so a cell stays `Copy` and cheap.
    pub link: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub ch: char,
    pub pen: Pen,
}

/// One screen row in a fold-aware display plan (W2): a real grid row, a collapsed
/// fold summary standing in for `lines` hidden rows, or blank padding past content.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanRow {
    Real(usize),
    Fold { local: usize, lines: usize },
    Blank,
}

/// Second half of a double-width glyph. Holds no character of its own: the glyph
/// in the cell to its left is drawn across both. Never a printable codepoint, so
/// it cannot collide with real content.
pub const SPACER: char = '\0';

impl Cell {
    pub fn is_spacer(&self) -> bool {
        self.ch == SPACER
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: ' ', pen: Pen::default() }
    }
}

#[derive(Clone, Copy)]
struct Saved {
    row: usize,
    col: usize,
    pen: Pen,
}

/// What mouse events the running program wants forwarded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseMode {
    #[default]
    Off,
    /// Press and release only (DECSET 1000).
    Click,
    /// Press, release, and motion while a button is held (1002).
    Drag,
    /// All motion (1003).
    Motion,
}

pub struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    col: usize,
    row: usize,
    pen: Pen,
    /// Deferred wrap (DEC autowrap). After printing into the last column the
    /// cursor stays put; it wraps only when the *next* printable arrives. Without
    /// this, anything that fills the final column scrolls one line too early.
    wrap_pending: bool,
    saved: Option<Saved>,
    /// Primary screen, parked while the alternate screen is active. Full-screen
    /// apps switch to the alternate so that quitting restores whatever was on
    /// screen before them.
    parked: Option<(Vec<Cell>, Saved)>,
    /// Lines that have scrolled off the top, oldest first.
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_limit: usize,
    /// Total rows ever dropped off the front of scrollback. Lets absolute
    /// coordinates stay stable as the ring buffer evicts: an absolute row is
    /// `dropped + index_into(scrollback ++ screen)`.
    dropped: usize,
    /// Rows scrolled back from the live bottom. 0 = following new output.
    display_offset: usize,
    /// OSC 133 shell-integration marks, in absolute rows. Each is the row where a
    /// command's output began. Powers "jump to prompt" and "copy last output".
    prompt_marks: Vec<usize>,
    /// Stable row where the command currently running started its output, set at
    /// OSC 133;C and cleared at OSC 133;D.
    command_start: Option<usize>,
    /// Where the command input begins (OSC 133;B, end of prompt): stable row and
    /// column. The guardian scans from here so the prompt text is not part of what
    /// it inspects. Cleared at OSC 133;C (command submitted).
    command_input: Option<(usize, usize)>,
    /// OSC 8 hyperlink URIs. A cell's `pen.link` is index+1 into this (0 = none).
    links: Vec<String>,
    /// Progress reported via OSC 9;4: `(state, percent)`. state 1 = normal, 2 =
    /// error, 3 = indeterminate, 4 = paused; `None` = no progress. Drives the tab
    /// progress bar.
    progress: Option<(u8, u8)>,
    /// The shell's working directory as reported by OSC 7 (`file://host/path`). The
    /// portable cwd source — works on macOS where `/proc` doesn't exist.
    cwd: Option<std::path::PathBuf>,
    /// Exit code of finished commands, keyed by the stable row of the prompt that
    /// launched each (OSC 133;D;code). Drives the pass/fail status gutter.
    cmd_exits: Vec<(usize, i32)>,
    /// Output regions of finished commands as stable (start, end) inclusive rows
    /// (OSC 133 C→D). Backs "fold all output" (W2).
    outputs: Vec<(usize, usize)>,
    /// Folded output regions: stable (start, end) inclusive rows hidden behind a
    /// one-line summary. A subset of `outputs` (W2).
    folds: Vec<(usize, usize)>,
    /// Stable (start, end) rows of the last finished command's output.
    last_output: Option<(usize, usize)>,
    /// Count of commands finished (OSC 133;D), for completion notifications.
    command_seq: u64,
    /// Inclusive row bounds that scrolling is confined to (DECSTBM).
    scroll_top: usize,
    scroll_bot: usize,
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub cursor_visible: bool,
    /// Mouse tracking the app requested, and whether it wants SGR-encoded reports.
    pub mouse_mode: MouseMode,
    pub mouse_sgr: bool,
    autowrap: bool,
    pub title: String,
    pub dirty: bool,
    /// Count of BEL (0x07) received. The UI compares it against a last-seen value
    /// to flash the pane and raise window urgency once per bell.
    pub bell_count: u64,
    /// Replies the terminal owes the program (Device Attributes, cursor position).
    /// The PTY reader thread drains these and writes them back to the child. Without
    /// answering DA1, fish waits 10s per query and then disables features.
    responses: Vec<Vec<u8>>,
    /// Inline images (kitty graphics protocol), anchored to a stable row so they
    /// scroll with the content that placed them.
    images: Vec<GridImage>,
    image_serial: u64,
    /// Cell size in pixels, needed to size an image's cell footprint. Zero until
    /// set, in which case images fall back to their control-supplied rows.
    cell_px: (f32, f32),
    /// Kitty keyboard protocol (CSI u) progressive-enhancement flags, kept as a
    /// stack per the spec: apps push their desired flags on entry and pop on exit.
    /// The active flags are the top of the stack (0 if empty). See `keyboard_flags`.
    /// Simplification: the alternate screen shares this one stack, and the flags
    /// are reset to empty on alt-screen enter/leave rather than kept as a separate
    /// stack — enough for full-screen apps (vim/nvim) that set flags after switching.
    kbd_flags_stack: Vec<u8>,
}

/// An inline image placed in the grid.
#[derive(Clone)]
pub struct GridImage {
    /// Monotonic id for GPU-texture caching in the renderer.
    pub serial: u64,
    /// Protocol image id, for targeted deletion (0 = none).
    pub id: u32,
    pub rgba: std::sync::Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Stable row (dropped + local) of the image's top-left cell.
    pub anchor: usize,
    pub cols: usize,
    pub rows: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            col: 0,
            row: 0,
            pen: Pen::default(),
            wrap_pending: false,
            saved: None,
            parked: None,
            scrollback: VecDeque::new(),
            scrollback_limit: 3000,
            dropped: 0,
            display_offset: 0,
            prompt_marks: Vec::new(),
            command_start: None,
            command_input: None,
            links: Vec::new(),
            progress: None,
            cwd: None,
            cmd_exits: Vec::new(),
            outputs: Vec::new(),
            folds: Vec::new(),
            last_output: None,
            command_seq: 0,
            scroll_top: 0,
            scroll_bot: rows - 1,
            app_cursor: false,
            bracketed_paste: false,
            cursor_visible: true,
            mouse_mode: MouseMode::Off,
            mouse_sgr: false,
            autowrap: true,
            title: String::new(),
            dirty: true,
            bell_count: 0,
            responses: Vec::new(),
            images: Vec::new(),
            image_serial: 0,
            cell_px: (0.0, 0.0),
            kbd_flags_stack: Vec::new(),
        }
    }

    /// Active kitty keyboard protocol flags: the top of the enhancement stack, or
    /// 0 when no app has pushed any (legacy keyboard). The input layer reads this to
    /// decide between legacy and CSI-u key encoding.
    pub fn keyboard_flags(&self) -> u8 {
        self.kbd_flags_stack.last().copied().unwrap_or(0)
    }

    pub fn set_cell_px(&mut self, w: f32, h: f32) {
        self.cell_px = (w, h);
    }

    /// Drains the replies owed to the child (Device Attributes, cursor position),
    /// which the PTY reader writes back. Called after each parse.
    pub fn take_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.responses)
    }

    /// Places a decoded image at the cursor, reserving its rows so following text
    /// flows below it. The anchor is a stable row, so the image scrolls with the
    /// content and evicts with it.
    pub fn place_image(&mut self, img: crate::graphics::Image) {
        let (cw, ch) = self.cell_px;
        let rows = if img.rows > 0 {
            img.rows as usize
        } else if ch > 0.0 {
            (img.height as f32 / ch).ceil() as usize
        } else {
            1
        };
        let cols = if img.cols > 0 {
            img.cols as usize
        } else if cw > 0.0 {
            (img.width as f32 / cw).ceil() as usize
        } else {
            1
        };

        self.image_serial += 1;
        self.images.push(GridImage {
            serial: self.image_serial,
            id: img.id,
            rgba: std::sync::Arc::new(img.rgba),
            width: img.width,
            height: img.height,
            anchor: self.local_to_stable(self.cursor_abs()),
            cols: cols.max(1),
            rows: rows.max(1),
        });
        // Reserve the rows: move the cursor down past the image so text does not
        // overwrite it. Column returns to the left, as after a newline.
        self.col = 0;
        for _ in 0..rows.max(1) {
            self.linefeed();
        }
        self.dirty = true;
    }

    /// Deletes images: `all` clears every one, else only the given protocol id.
    pub fn clear_images(&mut self, all: bool, id: u32) {
        if all {
            self.images.clear();
        } else {
            self.images.retain(|im| im.id != id);
        }
        self.dirty = true;
    }

    /// Live images with their current viewport row (`None` if scrolled out of
    /// view). Drops images evicted from scrollback along the way.
    pub fn images(&self) -> Vec<(GridImage, isize)> {
        self.images
            .iter()
            .filter_map(|im| {
                let local = self.stable_to_local(im.anchor)?;
                // Viewport row: local rows are scrollback ++ screen; the top of the
                // view is scrollback.len() - display_offset. With folds active the row
                // comes from the plan (an image inside a fold is hidden).
                if self.has_folds() {
                    return Some((im.clone(), self.screen_row_of(local)? as isize));
                }
                let top = self.scrollback.len() - self.display_offset;
                let row = local as isize - top as isize;
                Some((im.clone(), row))
            })
            .collect()
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Keeps the top-left content and clamps the cursor. Real reflow — rewrapping
    /// long lines to the new width — needs scrollback to put the extra rows in, so
    /// it lands in M4.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if cols == self.cols && rows == self.rows {
            return;
        }
        let mut cells = vec![Cell::default(); cols * rows];
        for row in 0..rows.min(self.rows) {
            for col in 0..cols.min(self.cols) {
                cells[row * cols + col] = self.cells[row * self.cols + col];
            }
        }
        self.cells = cells;
        // The parked primary screen must track the new size too, or leaving the
        // alternate screen would restore a buffer of the wrong length.
        if let Some((primary, _)) = &mut self.parked {
            let mut resized = vec![Cell::default(); cols * rows];
            for row in 0..rows.min(self.rows) {
                for col in 0..cols.min(self.cols) {
                    resized[row * cols + col] = primary[row * self.cols + col];
                }
            }
            *primary = resized;
        }
        self.cols = cols;
        self.rows = rows;
        self.col = self.col.min(cols - 1);
        self.row = self.row.min(rows - 1);
        self.scroll_top = 0;
        self.scroll_bot = rows - 1;
        self.display_offset = self.display_offset.min(self.scrollback.len());
        self.wrap_pending = false;
        self.dirty = true;
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells[row * self.cols + col]
    }

    // ---- Absolute addressing -------------------------------------------------
    //
    // Rows are numbered across `scrollback ++ screen`, so a coordinate keeps
    // meaning something when the view scrolls. Selections and (later) OSC 133
    // command marks live in this space; viewport coordinates would drift.

    pub fn total_rows(&self) -> usize {
        self.scrollback.len() + self.rows
    }

    pub fn display_offset(&self) -> usize {
        self.display_offset
    }

    /// Absolute row currently drawn at viewport row `row`.
    pub fn abs_row(&self, row: usize) -> usize {
        self.scrollback.len() - self.display_offset + row
    }

    pub fn abs_cell(&self, abs_row: usize, col: usize) -> Cell {
        let sb = self.scrollback.len();
        if abs_row < sb {
            self.scrollback[abs_row].get(col).copied().unwrap_or_default()
        } else {
            let row = abs_row - sb;
            if row < self.rows && col < self.cols {
                self.cells[row * self.cols + col]
            } else {
                Cell::default()
            }
        }
    }

    /// Scrolls the view by `delta` rows (positive = back into history). Returns
    /// whether anything moved.
    pub fn scroll_display(&mut self, delta: isize) -> bool {
        let max = self.scrollback.len() as isize;
        let next = (self.display_offset as isize + delta).clamp(0, max) as usize;
        if next == self.display_offset {
            return false;
        }
        self.display_offset = next;
        self.dirty = true;
        true
    }

    /// Jumps back to live output. Any keystroke should do this — typing into a
    /// scrolled-back view and seeing nothing happen is maddening.
    pub fn scroll_to_bottom(&mut self) -> bool {
        self.scroll_display(-(self.display_offset as isize))
    }

    /// Paints every cell with `pen` and a space. Used to give an overlay grid a
    /// solid background so it occludes the panes behind it.
    pub fn fill(&mut self, pen: Pen) {
        let blank = Cell { ch: ' ', pen };
        self.cells.fill(blank);
        self.pen = pen;
        self.dirty = true;
    }

    /// Writes `text` starting at `(row, col)`, clipped to the row. For overlays,
    /// which compose their own content rather than parsing a stream.
    pub fn write_str(&mut self, row: usize, col: usize, text: &str, pen: Pen) {
        if row >= self.rows {
            return;
        }
        let mut c = col;
        for ch in text.chars() {
            if c >= self.cols {
                break;
            }
            let w = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            self.cells[row * self.cols + c] = Cell { ch, pen };
            if w == 2 && c + 1 < self.cols {
                self.cells[row * self.cols + c + 1] = Cell { ch: SPACER, pen };
            }
            c += w;
        }
        self.dirty = true;
    }

    /// Scrollback *and the visible screen* as plain text, oldest first, for session
    /// persistence. The on-screen rows matter: without them a session would drop
    /// the last screenful of output — usually the part you most want back. Trailing
    /// blank lines are dropped so a mostly-empty screen does not save a wall of
    /// nothing. Skipped entirely on the alternate screen, whose content is a
    /// full-screen app's transient UI, not history.
    pub fn scrollback_text(&self) -> Vec<String> {
        let row_text = |row: &[Cell]| -> String {
            let s: String = row.iter().filter(|c| !c.is_spacer()).map(|c| c.ch).collect();
            s.trim_end().to_string()
        };
        let mut lines: Vec<String> = self.scrollback.iter().map(|r| row_text(r)).collect();
        if self.parked.is_none() {
            for row in 0..self.rows {
                let start = row * self.cols;
                lines.push(row_text(&self.cells[start..start + self.cols]));
            }
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }

    /// Restores saved history as inert scrollback above the live screen. Colour is
    /// not persisted — restored history is plain text, which is what a session is
    /// for (reading what happened), not re-running it.
    pub fn preload_scrollback(&mut self, lines: &[String]) {
        for line in lines {
            let mut row = vec![Cell::default(); self.cols];
            for (i, ch) in line.chars().take(self.cols).enumerate() {
                row[i] = Cell { ch, pen: Pen::default() };
            }
            self.scrollback.push_back(row);
        }
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
            self.dropped += 1;
        }
    }

    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback_limit = limit.max(1);
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
            self.dropped += 1;
        }
    }

    // ---- Stable coordinates & OSC 133 ---------------------------------------
    //
    // Absolute rows above (`abs_row`) index `scrollback ++ screen`, which shifts
    // when the ring evicts. Shell-integration marks must outlive eviction, so they
    // are stored as `dropped + local`, a monotonic line number that a given line
    // keeps forever. `stable_to_local` maps one back, or `None` once evicted.

    fn local_to_stable(&self, local: usize) -> usize {
        self.dropped + local
    }

    fn stable_to_local(&self, stable: usize) -> Option<usize> {
        stable.checked_sub(self.dropped).filter(|&l| l < self.total_rows())
    }

    /// Absolute row (local) where the cursor sits.
    fn cursor_abs(&self) -> usize {
        self.scrollback.len() + self.row
    }

    /// Whether the alternate screen is active (a full-screen app like vim/htop).
    /// The guardian skips these — there is no shell command line to inspect.
    pub fn alt_screen(&self) -> bool {
        self.parked.is_some()
    }

    /// Rows evicted off the front of scrollback so far. Copy-mode uses the delta to
    /// keep its cursor on the same line as the ring evicts.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Fraction (0..1) of the absolute (local) row `abs` that holds non-blank cells,
    /// for the minimap. Cheap: one pass over the row.
    pub fn row_fill(&self, abs: usize) -> f32 {
        if self.cols == 0 {
            return 0.0;
        }
        let last = (0..self.cols)
            .rev()
            .find(|&c| {
                let cell = self.abs_cell(abs, c);
                !cell.is_spacer() && cell.ch != ' '
            })
            .map(|c| c + 1)
            .unwrap_or(0);
        last as f32 / self.cols as f32
    }

    /// Exit code of the most recently finished command, if OSC 133;D carried one.
    /// Drives the tab fail badge.
    pub fn last_exit(&self) -> Option<i32> {
        self.cmd_exits.last().map(|&(_, e)| e)
    }

    /// Reported progress `(state, percent)` from OSC 9;4, or `None`. state 1 normal,
    /// 2 error, 3 indeterminate, 4 paused.
    pub fn progress(&self) -> Option<(u8, u8)> {
        self.progress
    }

    /// The shell's working directory from OSC 7, if it has reported one.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        self.cwd.clone()
    }

    // ---- Command output folding (W2) ----------------------------------------

    /// Whether any output region is currently folded AND folds should apply now.
    /// Folds never apply on the alternate screen (a full-screen app owns the rows;
    /// the folded stable rows would index the alt buffer). One gate covers render,
    /// hit-testing and click-toggle.
    pub fn has_folds(&self) -> bool {
        !self.folds.is_empty() && self.parked.is_none()
    }

    /// Folds every finished command's output region that spans more than one row.
    pub fn fold_all(&mut self) {
        self.folds.clear();
        for &(s, e) in &self.outputs {
            if e > s {
                self.folds.push((s, e));
            }
        }
        self.folds.sort_unstable();
        self.folds.dedup();
        self.dirty = true;
    }

    /// Removes every fold (shows all output again).
    pub fn unfold_all(&mut self) {
        if !self.folds.is_empty() {
            self.folds.clear();
            self.dirty = true;
        }
    }

    /// Toggles the fold covering the command at absolute (local) `row`: if that row
    /// is inside a fold, unfold it; else if it is inside a finished command's output,
    /// fold that. Used for click-to-toggle on a summary line or a command's output.
    pub fn toggle_fold_at(&mut self, local_row: usize) {
        let stable = self.local_to_stable(local_row);
        if let Some(i) = self.folds.iter().position(|&(s, e)| stable >= s && stable <= e) {
            self.folds.remove(i);
            self.dirty = true;
            return;
        }
        if let Some(&(s, e)) = self.outputs.iter().find(|&&(s, e)| stable >= s && stable <= e && e > s)
        {
            self.folds.push((s, e));
            self.folds.sort_unstable();
            self.dirty = true;
        }
    }

    /// The rows to display for the current viewport, folds collapsed. Exactly `rows`
    /// entries (padded with blanks past the end of content).
    pub fn display_plan(&self) -> Vec<PlanRow> {
        let top = self.scrollback.len() - self.display_offset;
        let total = self.total_rows();
        let mut plan = Vec::with_capacity(self.rows);
        let mut abs = top;
        while plan.len() < self.rows {
            if abs >= total {
                plan.push(PlanRow::Blank);
                abs += 1;
                continue;
            }
            let stable = self.local_to_stable(abs);
            if let Some(&(fs, fe)) = self.folds.iter().find(|&&(s, e)| stable >= s && stable <= e) {
                let lines = fe - fs + 1;
                plan.push(PlanRow::Fold { local: abs, lines });
                // Jump past the folded region (its end mapped back to local). If the
                // end fell past the buffer (a window shrink truncated the screen),
                // jump to the end so the summary is not repeated down the viewport.
                match self.stable_to_local(fe) {
                    Some(le) => abs = le + 1,
                    None => abs = total,
                }
            } else {
                plan.push(PlanRow::Real(abs));
                abs += 1;
            }
        }
        plan
    }

    /// The text currently on the command line: from the OSC 133;B command-input mark
    /// (end of prompt) when the shell emits one, else the last prompt-start mark,
    /// else the cursor's own row — down to the cursor. Used by the command guardian
    /// so the prompt's own text is not mistaken for the command about to run.
    pub fn current_command_text(&self) -> String {
        let cur = self.cursor_abs();
        // Scan to the END of the cursor row, not the cursor column: pressing Enter
        // with the cursor moved back (Home, or an edit mid-line) must still see the
        // whole typed command, or the guardian is trivially bypassed. text_range
        // trims trailing blanks per row, so the full width is safe.
        let last_col = self.cols.saturating_sub(1);
        // Prefer the B mark (prompt end): scan from exactly where input begins.
        if let Some((stable, col)) = self.command_input {
            if let Some(row) = self.stable_to_local(stable).filter(|&r| r <= cur) {
                let start_col = if row == cur { col.min(last_col) } else { col };
                return self.text_range((row, start_col), (cur, last_col));
            }
        }
        let start_row = self
            .prompt_marks
            .last()
            .and_then(|&s| self.stable_to_local(s))
            .filter(|&r| r <= cur)
            .unwrap_or(cur);
        self.text_range((start_row, 0), (cur, last_col))
    }

    /// The stable index one past the last row that actually holds output — the
    /// cursor's row — for the keyword watcher's high-water mark. Using the cursor
    /// row (not the screen height) means blank rows below the cursor are not counted
    /// as already-scanned, so a line later written there is still seen (W4).
    pub fn watch_mark(&self) -> usize {
        self.dropped + self.scrollback.len() + self.row + 1
    }

    /// Text of every row from `from_stable` up to the cursor row, plus the new
    /// high-water mark. When `from_stable` is at or past the cursor row nothing new
    /// has arrived and the text is empty; an evicted mark saturates to the oldest
    /// surviving row, all of which are newer than an evicted mark, so no re-scan.
    pub fn text_since_stable(&self, from_stable: usize) -> (String, usize) {
        let from_local = from_stable.saturating_sub(self.dropped);
        let last = self.scrollback.len() + self.row;
        if from_local > last {
            return (String::new(), self.watch_mark());
        }
        let text = self.text_range((from_local, 0), (last, self.cols.saturating_sub(1)));
        (text, self.watch_mark())
    }

    /// Scroll offsets, oldest first, that put each prompt at the top of the view.
    /// Backs "jump to previous/next command".
    pub fn prompt_offsets(&self) -> Vec<usize> {
        self.prompt_marks
            .iter()
            .filter_map(|&s| self.stable_to_local(s))
            .map(|local| self.scrollback.len().saturating_sub(local))
            .collect()
    }

    /// The prompt line to pin at the top while scrolled back: the most recent
    /// prompt at or above the current top row, so you always see which command's
    /// output you are reading. `None` when following live output, when there are no
    /// marks, or when the pinned prompt would be the very top row already.
    pub fn sticky_prompt(&self) -> Option<String> {
        if self.display_offset == 0 {
            return None;
        }
        let top = self.scrollback.len().saturating_sub(self.display_offset);
        // Prompt marks are stable rows; convert to local and find the last one at
        // or above the viewport top.
        let mark = self
            .prompt_marks
            .iter()
            .filter_map(|&s| self.stable_to_local(s))
            .filter(|&local| local < top)
            .max()?;
        // `mark` is already an absolute (local) row index, which abs_cell takes.
        let text: String = (0..self.cols)
            .map(|c| self.abs_cell(mark, c))
            .filter(|cell| !cell.is_spacer())
            .map(|cell| cell.ch)
            .collect();
        let text = text.trim_end().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Number of commands finished so far (OSC 133;D count).
    pub fn command_seq(&self) -> u64 {
        self.command_seq
    }

    /// Whether a command's output is currently being produced (between C and D).
    pub fn command_running(&self) -> bool {
        self.command_start.is_some()
    }

    /// Text of the most recently finished command's output, if OSC 133 marked it.
    pub fn last_command_output(&self) -> Option<String> {
        let (start, end) = self.last_output?;
        let from = self.stable_to_local(start)?;
        let to = self.stable_to_local(end).unwrap_or(self.total_rows().saturating_sub(1));
        Some(self.text_range((from, 0), (to.max(from), self.cols.saturating_sub(1))))
    }

    /// Finds every occurrence of `needle` (case-insensitive) across scrollback and
    /// screen, returning each match's absolute `(row, col)` start. Matches within a
    /// single row only — a query does not span a line wrap.
    pub fn search(&self, needle: &str) -> Vec<(usize, usize)> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle = needle.to_lowercase();
        let mut hits = Vec::new();
        for abs in 0..self.total_rows() {
            // Spacers are skipped, so the string index and the grid column diverge
            // whenever wide chars precede the match; `col_map` records the grid
            // column of each char so a hit lands on the right cell.
            let mut row = String::new();
            let mut col_map = Vec::new();
            for c in 0..self.cols {
                let cell = self.abs_cell(abs, c);
                if cell.is_spacer() {
                    continue;
                }
                row.push(cell.ch.to_ascii_lowercase());
                col_map.push(c);
            }
            let mut start = 0;
            while let Some(pos) = row[start..].find(&needle) {
                let col = col_map[row[..start + pos].chars().count()];
                hits.push((abs, col));
                // Advance past the whole matched substring. Stepping a single byte
                // would land inside a multibyte char when `needle` begins with one
                // (e.g. searching "é" over "café"), and the next `row[start..]`
                // slice would panic on the non-boundary index.
                start += pos + needle.len();
            }
        }
        hits
    }

    /// Scrolls so the given absolute row sits in the middle of the view.
    pub fn scroll_to_abs(&mut self, abs: usize) {
        let target_top = abs.saturating_sub(self.rows / 2);
        let offset = self.scrollback.len().saturating_sub(target_top);
        self.display_offset = offset.min(self.scrollback.len());
        self.dirty = true;
    }

    /// Text of the rows in `[from, to]` absolute range, trailing blanks trimmed.
    pub fn text_range(&self, from: (usize, usize), to: (usize, usize)) -> String {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        let mut out = String::new();
        for abs in from.0..=to.0.min(self.total_rows().saturating_sub(1)) {
            let start = if abs == from.0 { from.1 } else { 0 };
            let end = if abs == to.0 { to.1 + 1 } else { self.cols };
            // Spacers carry no character: skipping them is what makes copied CJK
            // come back as text rather than text riddled with holes.
            let line: String = (start..end.min(self.cols))
                .map(|c| self.abs_cell(abs, c))
                .filter(|cell| !cell.is_spacer())
                .map(|cell| cell.ch)
                .collect();
            out.push_str(line.trim_end());
            if abs != to.0 {
                out.push('\n');
            }
        }
        out
    }

    /// Erased cells keep the current background (background-colour erase). Using
    /// a default-pen blank instead leaves stripes when a themed shell clears.
    fn blank(&self) -> Cell {
        Cell { ch: ' ', pen: Pen { bg: self.pen.bg, ..Pen::default() } }
    }

    fn index(&self) -> usize {
        self.row * self.cols + self.col
    }

    /// Overwriting one half of a double-width glyph must blank the other half, or
    /// a stale leader keeps drawing across a cell that now belongs to something
    /// else.
    fn clear_wide_partner(&mut self, idx: usize) {
        let col = idx % self.cols;
        let blank = Cell { ch: ' ', pen: self.pen };
        if self.cells[idx].is_spacer() {
            if col > 0 {
                self.cells[idx - 1] = blank;
            }
        } else if col + 1 < self.cols && self.cells[idx + 1].is_spacer() {
            self.cells[idx + 1] = blank;
        }
    }

    /// Scrolls the region up. `feed` decides whether the displaced top rows enter
    /// scrollback: only a linefeed does that. Delete-lines and explicit scroll-up
    /// (CSI M / S) discard the content instead — feeding it would archive lines the
    /// program just deleted, and the eviction could even bump `dropped`.
    fn scroll_up(&mut self, n: usize) {
        self.scroll_up_impl(n, true);
    }

    /// A region scroll that never touches scrollback (delete-lines, CSI S).
    fn scroll_up_region(&mut self, n: usize) {
        self.scroll_up_impl(n, false);
    }

    fn scroll_up_impl(&mut self, n: usize, feed: bool) {
        let n = n.min(self.scroll_bot - self.scroll_top + 1);

        // Only a full-screen linefeed scroll of the primary screen feeds
        // scrollback. A region scroll (htop's process list, a vim split), anything
        // on the alternate screen, or a delete/scroll-up control must not: one
        // minute of htop would otherwise evict everything worth keeping.
        if feed && self.scroll_top == 0 && self.scroll_bot == self.rows - 1 && self.parked.is_none() {
            for row in 0..n {
                let start = row * self.cols;
                self.scrollback.push_back(self.cells[start..start + self.cols].to_vec());
            }
            let before = self.dropped;
            while self.scrollback.len() > self.scrollback_limit {
                self.scrollback.pop_front();
                self.dropped += 1;
            }
            // Drop anything anchored to a row that scrolled out of the ring, so these
            // lists cannot grow without bound: images, prompt marks and their exit
            // codes. (Otherwise a long session grows them forever and old commands'
            // status is lost by the count-based cap while still scrollable.)
            if self.dropped != before {
                let dropped = self.dropped;
                self.images.retain(|im| im.anchor >= dropped);
                self.prompt_marks.retain(|&m| m >= dropped);
                self.cmd_exits.retain(|&(p, _)| p >= dropped);
                self.outputs.retain(|&(s, _)| s >= dropped);
                self.folds.retain(|&(s, _)| s >= dropped);
            }
            // Keep the viewport pinned to the same content while scrolled back,
            // instead of letting it drift as new lines arrive.
            if self.display_offset > 0 {
                self.display_offset = (self.display_offset + n).min(self.scrollback.len());
            }
        }

        let (start, end) = (self.scroll_top * self.cols, (self.scroll_bot + 1) * self.cols);
        let blank = self.blank();
        self.cells[start..end].rotate_left(n * self.cols);
        self.cells[end - n * self.cols..end].fill(blank);
    }

    fn scroll_down(&mut self, n: usize) {
        let (start, end) = (self.scroll_top * self.cols, (self.scroll_bot + 1) * self.cols);
        let n = n.min(self.scroll_bot - self.scroll_top + 1);
        let blank = self.blank();
        self.cells[start..end].rotate_right(n * self.cols);
        self.cells[start..start + n * self.cols].fill(blank);
    }

    fn linefeed(&mut self) {
        if self.row == self.scroll_bot {
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    /// Insert/delete lines act like a scroll of the region below the cursor.
    fn insert_lines(&mut self, n: usize) {
        if self.row < self.scroll_top || self.row > self.scroll_bot {
            return;
        }
        let saved_top = self.scroll_top;
        self.scroll_top = self.row;
        self.scroll_down(n);
        self.scroll_top = saved_top;
    }

    fn delete_lines(&mut self, n: usize) {
        if self.row < self.scroll_top || self.row > self.scroll_bot {
            return;
        }
        let saved_top = self.scroll_top;
        self.scroll_top = self.row;
        self.scroll_up_region(n);
        self.scroll_top = saved_top;
    }

    fn insert_chars(&mut self, n: usize) {
        let start = self.row * self.cols;
        let line = &mut self.cells[start..start + self.cols];
        let n = n.min(self.cols - self.col);
        line[self.col..].rotate_right(n);
        let blank = Cell { ch: ' ', pen: Pen { bg: self.pen.bg, ..Pen::default() } };
        line[self.col..self.col + n].fill(blank);
    }

    fn delete_chars(&mut self, n: usize) {
        let start = self.row * self.cols;
        let line = &mut self.cells[start..start + self.cols];
        let n = n.min(self.cols - self.col);
        line[self.col..].rotate_left(n);
        let blank = Cell { ch: ' ', pen: Pen { bg: self.pen.bg, ..Pen::default() } };
        line[self.cols - n..].fill(blank);
    }

    fn erase_chars(&mut self, n: usize) {
        let start = self.row * self.cols + self.col;
        let end = (start + n).min((self.row + 1) * self.cols);
        let blank = self.blank();
        self.cells[start..end].fill(blank);
    }

    fn enter_alt_screen(&mut self) {
        if self.parked.is_some() {
            return;
        }
        let blank = self.blank();
        let primary = std::mem::replace(&mut self.cells, vec![blank; self.cols * self.rows]);
        self.parked = Some((primary, Saved { row: self.row, col: self.col, pen: self.pen }));
        self.row = 0;
        self.col = 0;
        // Spec: the alternate screen keeps its own keyboard-flags stack. We share a
        // single stack and reset it, so an app inherits legacy keys on entry and the
        // shell is back to legacy on exit.
        self.kbd_flags_stack.clear();
    }

    fn leave_alt_screen(&mut self) {
        if let Some((primary, cursor)) = self.parked.take() {
            self.cells = primary;
            self.row = cursor.row.min(self.rows - 1);
            self.col = cursor.col.min(self.cols - 1);
            self.pen = cursor.pen;
        }
        self.kbd_flags_stack.clear();
    }

    /// `?`-prefixed CSI h/l. Only the modes runnir actually honours are listed;
    /// silently ignoring the rest is correct behaviour.
    fn private_mode(&mut self, mode: u16, on: bool) {
        match mode {
            1 => self.app_cursor = on,
            7 => self.autowrap = on,
            25 => self.cursor_visible = on,
            2004 => self.bracketed_paste = on,
            47 | 1047 | 1049 => {
                if on {
                    self.enter_alt_screen();
                } else {
                    self.leave_alt_screen();
                }
            }
            // Mouse tracking modes. 1000 = click, 1002 = button-drag, 1003 = any
            // motion; 1006 = SGR (extended) coordinate encoding.
            1000 => self.mouse_mode = if on { MouseMode::Click } else { MouseMode::Off },
            1002 => self.mouse_mode = if on { MouseMode::Drag } else { MouseMode::Off },
            1003 => self.mouse_mode = if on { MouseMode::Motion } else { MouseMode::Off },
            1006 => self.mouse_sgr = on,
            _ => {}
        }
    }

    fn erase_display(&mut self, mode: u16) {
        // Mode 3 ("erase saved lines", xterm) clears scrollback and the marks
        // that point into it — and nothing else: the visible screen is 2J's
        // job, and `clear` sends both when it wants both.
        if mode == 3 {
            // Stable rows are `dropped + local`. Clearing scrollback shifts every
            // local index down by its length, so account the cleared rows as
            // dropped — otherwise every surviving mark (OSC 133, image anchors)
            // would point `scrollback.len()` rows too low.
            self.dropped += self.scrollback.len();
            self.scrollback.clear();
            let dropped = self.dropped;
            // Marks and images anchored in the erased history are gone; those on
            // the live screen keep their (still valid) stable rows.
            self.prompt_marks.retain(|&m| m >= dropped);
            self.cmd_exits.retain(|&(p, _)| p >= dropped);
            self.outputs.retain(|&(s, _)| s >= dropped);
            self.folds.retain(|&(s, _)| s >= dropped);
            self.images.retain(|im| im.anchor >= dropped);
            self.display_offset = 0;
            return;
        }
        let blank = self.blank();
        let idx = self.index();
        let range = match mode {
            0 => idx..self.cells.len(),
            1 => 0..idx + 1,
            2 => 0..self.cells.len(),
            _ => return,
        };
        self.cells[range].fill(blank);
        // Erasing the screen in place invalidates any fold/output anchored on the
        // erased rows: stable coords only track content under bottom-line scroll, so
        // fresh output there would otherwise render collapsed under a stale summary.
        // Only the erased rows are affected, so a prompt redraw's clr_eos (ED0) does
        // not pop every fold on the screen.
        let screen_top = self.dropped + self.scrollback.len();
        let (lo, hi) = match mode {
            0 => (screen_top + self.row, screen_top + self.rows.saturating_sub(1)),
            1 => (screen_top, screen_top + self.row),
            _ => (screen_top, screen_top + self.rows.saturating_sub(1)),
        };
        self.invalidate_folds_in(lo, hi);
    }

    /// Drops folds, banked output regions and the last-output range that intersect
    /// the stable row range `[lo, hi]`. A no-op on the alternate screen, whose erases
    /// touch the parked alt buffer, not the primary rows the folds anchor to.
    fn invalidate_folds_in(&mut self, lo: usize, hi: usize) {
        if self.parked.is_some() {
            return;
        }
        let hit = |s: usize, e: usize| !(e < lo || s > hi);
        self.folds.retain(|&(s, e)| !hit(s, e));
        self.outputs.retain(|&(s, e)| !hit(s, e));
        if self.last_output.is_some_and(|(s, e)| hit(s, e)) {
            self.last_output = None;
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let blank = self.blank();
        let start = self.row * self.cols;
        let range = match mode {
            0 => start + self.col..start + self.cols,
            1 => start..start + self.col + 1,
            2 => start..start + self.cols,
            _ => return,
        };
        self.cells[range].fill(blank);
    }

    /// Sets the underline style and keeps `Flags::UNDERLINE` in sync so the old
    /// "is it underlined?" checks (and the legacy shader gate) keep working.
    fn set_underline(&mut self, style: UnderlineStyle) {
        self.pen.underline = style;
        self.pen.flags.set(Flags::UNDERLINE, style != UnderlineStyle::None);
    }

    fn sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.pen = Pen::default();
            return;
        }
        let mut iter = params.iter();
        while let Some(sub) = iter.next() {
            match sub[0] {
                0 => self.pen = Pen::default(),
                1 => self.pen.flags.insert(Flags::BOLD),
                2 => self.pen.flags.insert(Flags::DIM),
                3 => self.pen.flags.insert(Flags::ITALIC),
                // `4` alone is a single underline; `4:n` (a colon sub-param) picks
                // a styled form. vte hands us the whole group in `sub`, so the
                // sub-param, if any, is `sub[1]`.
                4 => {
                    let style = match sub.get(1) {
                        None | Some(1) => UnderlineStyle::Single,
                        Some(0) => UnderlineStyle::None,
                        Some(2) => UnderlineStyle::Double,
                        Some(3) => UnderlineStyle::Curly,
                        Some(4) => UnderlineStyle::Dotted,
                        Some(5) => UnderlineStyle::Dashed,
                        // An unknown sub-param still means "underlined": fall back
                        // to a single rather than dropping the attribute.
                        _ => UnderlineStyle::Single,
                    };
                    self.set_underline(style);
                }
                7 => self.pen.flags.insert(Flags::REVERSE),
                8 => self.pen.flags.insert(Flags::HIDDEN),
                9 => self.pen.flags.insert(Flags::STRIKE),
                21 => self.set_underline(UnderlineStyle::Double),
                22 => self.pen.flags.remove(Flags::BOLD | Flags::DIM),
                23 => self.pen.flags.remove(Flags::ITALIC),
                24 => {
                    self.set_underline(UnderlineStyle::None);
                    self.pen.underline_color = Color::Default;
                }
                27 => self.pen.flags.remove(Flags::REVERSE),
                28 => self.pen.flags.remove(Flags::HIDDEN),
                29 => self.pen.flags.remove(Flags::STRIKE),
                30..=37 => self.pen.fg = Color::Indexed((sub[0] - 30) as u8),
                38 => {
                    if let Some(c) = ext_color(sub, &mut iter) {
                        self.pen.fg = c;
                    }
                }
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Indexed((sub[0] - 40) as u8),
                48 => {
                    if let Some(c) = ext_color(sub, &mut iter) {
                        self.pen.bg = c;
                    }
                }
                49 => self.pen.bg = Color::Default,
                // SGR 58 sets the underline's own colour (same wire forms as 38/48);
                // 59 resets it to "follow the foreground".
                58 => {
                    if let Some(c) = ext_color(sub, &mut iter) {
                        self.pen.underline_color = c;
                    }
                }
                59 => self.pen.underline_color = Color::Default,
                90..=97 => self.pen.fg = Color::Indexed((sub[0] - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((sub[0] - 100 + 8) as u8),
                _ => {}
            }
        }
    }

    /// Rows as strings with trailing blanks trimmed. Verification aid for M1 —
    /// the real renderer reads cells directly.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for row in 0..self.rows {
            let line: String = (0..self.cols)
                .map(|c| self.cell(row, c))
                .filter(|cell| !cell.is_spacer())
                .map(|cell| cell.ch)
                .collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out.trim_end().to_string()
    }
}

/// Parses the 256-colour and truecolour forms of SGR 38/48.
///
/// Two wire forms exist and both are in the wild: `38;5;n` (separate params) and
/// `38:5:n` (one subparam group). The colon form additionally has a variant
/// carrying a colourspace id that must be skipped: `38:2::r:g:b`.
fn ext_color<'a>(sub: &[u16], iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    if sub.len() > 1 {
        return match sub[1] {
            5 => sub.get(2).map(|&n| Color::Indexed(n as u8)),
            2 => match sub.len() {
                5 => Some(Color::Rgb(sub[2] as u8, sub[3] as u8, sub[4] as u8)),
                6 => Some(Color::Rgb(sub[3] as u8, sub[4] as u8, sub[5] as u8)),
                _ => None,
            },
            _ => None,
        };
    }
    match iter.next()?[0] {
        5 => Some(Color::Indexed(iter.next()?[0] as u8)),
        2 => {
            let r = iter.next()?[0] as u8;
            let g = iter.next()?[0] as u8;
            let b = iter.next()?[0] as u8;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Decodes `%XX` escapes in an OSC 7 path (spaces etc. arrive percent-encoded).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Perform for Grid {
    fn print(&mut self, c: char) {
        // Zero-width (combining marks, ZWJ) attach to the previous cell; drawing
        // them as their own width-1 cell would drift the cursor and split NFD text
        // or emoji sequences. Dropping them keeps the layout correct until proper
        // combining support lands. `unwrap_or(0)` (not the earlier `unwrap_or(1)`)
        // ensures a control-ish char with no width is dropped, not widened.
        let width = c.width().unwrap_or(0);
        if width == 0 {
            return;
        }
        // A double-width glyph needs two columns. In a one-column grid it can never
        // be placed: writing its trailing spacer would spill into the next row and,
        // for the last cell, index one past the buffer end and panic. Drop it.
        if width == 2 && self.cols < 2 {
            return;
        }

        if self.wrap_pending {
            self.col = 0;
            self.linefeed();
            self.wrap_pending = false;
        }
        // A double-width glyph cannot straddle the right edge, so wrap it early
        // rather than split it across two rows.
        if width == 2 && self.col + 2 > self.cols {
            if !self.autowrap {
                return;
            }
            self.col = 0;
            self.linefeed();
        }

        let idx = self.index();
        self.clear_wide_partner(idx);
        self.cells[idx] = Cell { ch: c, pen: self.pen };
        if width == 2 {
            self.clear_wide_partner(idx + 1);
            self.cells[idx + 1] = Cell { ch: SPACER, pen: self.pen };
        }

        if self.col + width >= self.cols {
            // With autowrap off the cursor sticks in the last column and each
            // further glyph overwrites it.
            self.wrap_pending = self.autowrap;
            self.col = self.cols - 1;
        } else {
            self.col += width;
        }
        self.dirty = true;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL: count it so the UI can flash the pane and mark the window
                // urgent. Does not move the cursor or clear a pending wrap.
                self.bell_count = self.bell_count.wrapping_add(1);
                self.dirty = true;
                return;
            }
            0x08 => self.col = self.col.saturating_sub(1),
            0x09 => self.col = (((self.col / 8) + 1) * 8).min(self.cols - 1),
            0x0a | 0x0b | 0x0c => self.linefeed(),
            0x0d => self.col = 0,
            _ => return,
        }
        self.wrap_pending = false;
        self.dirty = true;
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let ps: Vec<u16> = params.iter().map(|sub| sub[0]).collect();

        if intermediates == b"?" {
            match action {
                'h' | 'l' => {
                    for &mode in &ps {
                        self.private_mode(mode, action == 'h');
                    }
                    self.dirty = true;
                }
                // Kitty keyboard protocol query: CSI ? u. Reply with the current
                // (top-of-stack) flags: CSI ? <flags> u.
                'u' => {
                    let flags = self.keyboard_flags();
                    self.responses.push(format!("\x1b[?{flags}u").into_bytes());
                }
                _ => {}
            }
            return;
        }
        // Secondary Device Attributes: CSI > c. Report a VT220-class terminal,
        // version 0. Programs use this alongside DA1 for capability detection.
        if intermediates == b">" {
            match action {
                'c' => self.responses.push(b"\x1b[>1;0;0c".to_vec()),
                // Kitty keyboard protocol: CSI > <flags> u pushes flags on the stack.
                'u' => {
                    let flags = (ps.first().copied().unwrap_or(0) & 0x1f) as u8;
                    // Bound the stack so a misbehaving app cannot grow it forever.
                    if self.kbd_flags_stack.len() >= 128 {
                        self.kbd_flags_stack.remove(0);
                    }
                    self.kbd_flags_stack.push(flags);
                }
                _ => {}
            }
            return;
        }
        // Kitty keyboard protocol: CSI < <number> u pops `number` (default 1) entries.
        if intermediates == b"<" {
            if action == 'u' {
                let n = ps.first().copied().filter(|&v| v != 0).unwrap_or(1) as usize;
                let keep = self.kbd_flags_stack.len().saturating_sub(n);
                self.kbd_flags_stack.truncate(keep);
            }
            return;
        }
        // Kitty keyboard protocol: CSI = <flags> ; <mode> u sets the current flags.
        // mode 1 = set all, 2 = set bits, 3 = clear bits (default 1).
        if intermediates == b"=" {
            if action == 'u' {
                let flags = (ps.first().copied().unwrap_or(0) & 0x1f) as u8;
                let mode = ps.get(1).copied().unwrap_or(1);
                let cur = self.keyboard_flags();
                let new = match mode {
                    2 => cur | flags,
                    3 => cur & !flags,
                    _ => flags,
                };
                if let Some(top) = self.kbd_flags_stack.last_mut() {
                    *top = new;
                } else {
                    self.kbd_flags_stack.push(new);
                }
            }
            return;
        }
        if !intermediates.is_empty() {
            return;
        }
        // Two accessors because the default differs by sequence: counts and
        // positions treat a missing-or-zero param as 1, erase modes treat 0 as a
        // meaningful mode.
        let count = |i: usize| ps.get(i).copied().filter(|&v| v != 0).unwrap_or(1) as usize;
        let mode = |i: usize| ps.get(i).copied().unwrap_or(0);

        match action {
            // CUU/CUD stop at the scroll-region margin when the cursor starts on
            // its side of it (DEC STD 070); only a cursor already outside the
            // region may travel the rest of the screen.
            'A' => {
                let top = if self.row >= self.scroll_top { self.scroll_top } else { 0 };
                self.row = self.row.saturating_sub(count(0)).max(top);
            }
            'B' => {
                let bot = if self.row <= self.scroll_bot { self.scroll_bot } else { self.rows - 1 };
                self.row = (self.row + count(0)).min(bot);
            }
            'C' => self.col = (self.col + count(0)).min(self.cols - 1),
            'D' => self.col = self.col.saturating_sub(count(0)),
            'G' => self.col = (count(0) - 1).min(self.cols - 1),
            'd' => self.row = (count(0) - 1).min(self.rows - 1),
            'H' | 'f' => {
                self.row = (count(0) - 1).min(self.rows - 1);
                self.col = (count(1) - 1).min(self.cols - 1);
            }
            'J' => self.erase_display(mode(0)),
            'K' => self.erase_line(mode(0)),
            'm' => self.sgr(params),
            'L' => self.insert_lines(count(0)),
            'M' => self.delete_lines(count(0)),
            '@' => self.insert_chars(count(0)),
            'P' => self.delete_chars(count(0)),
            'X' => self.erase_chars(count(0)),
            'S' => self.scroll_up_region(count(0)),
            'T' => self.scroll_down(count(0)),
            's' => self.saved = Some(Saved { row: self.row, col: self.col, pen: self.pen }),
            'u' => {
                if let Some(s) = self.saved {
                    self.row = s.row.min(self.rows - 1);
                    self.col = s.col.min(self.cols - 1);
                    self.pen = s.pen;
                }
            }
            'r' => {
                // DECSTBM. An out-of-order or oversized region is ignored, and
                // setting it homes the cursor.
                let top = count(0) - 1;
                let bot = ps.get(1).copied().filter(|&v| v != 0).map_or(self.rows, |v| v as usize) - 1;
                let bot = bot.min(self.rows - 1);
                if top < bot {
                    self.scroll_top = top;
                    self.scroll_bot = bot;
                    self.row = 0;
                    self.col = 0;
                }
            }
            // Primary Device Attributes (CSI c / CSI 0 c): claim a VT220 with ANSI
            // colour. Answering is what stops fish waiting 10s and disabling
            // features. `\x1b[?62;22c` = service class 62 (VT220), 22 = colour.
            'c' => self.responses.push(b"\x1b[?62;22c".to_vec()),
            // Device Status Report. 5 = "are you ok" -> \x1b[0n; 6 = cursor
            // position -> CSI row ; col R (1-based).
            'n' => match mode(0) {
                5 => self.responses.push(b"\x1b[0n".to_vec()),
                6 => {
                    let r = self.row + 1;
                    let c = self.col + 1;
                    self.responses.push(format!("\x1b[{r};{c}R").into_bytes());
                }
                _ => {}
            },
            _ => return,
        }
        // SGR only changes the pen; it must not cancel a deferred wrap, or a
        // colour change after printing into the last column makes the next
        // glyph overwrite that column instead of wrapping.
        if action != 'm' {
            self.wrap_pending = false;
        }
        self.dirty = true;
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'D' => self.linefeed(),
            b'E' => {
                self.col = 0;
                self.linefeed();
            }
            b'M' => {
                if self.row == self.scroll_top {
                    self.scroll_down(1);
                } else {
                    self.row = self.row.saturating_sub(1);
                }
            }
            b'7' => self.saved = Some(Saved { row: self.row, col: self.col, pen: self.pen }),
            b'8' => {
                if let Some(s) = self.saved {
                    self.row = s.row.min(self.rows - 1);
                    self.col = s.col.min(self.cols - 1);
                    self.pen = s.pen;
                }
            }
            b'c' => {
                // RIS: reset the screen but keep scrollback and the configured
                // limit — real terminals do not wipe history on a reset.
                let limit = self.scrollback_limit;
                let scrollback = std::mem::take(&mut self.scrollback);
                let dropped = self.dropped;
                // The cell pixel size is renderer environment, not terminal
                // state: losing it would mis-size images placed after a reset.
                let cell_px = self.cell_px;
                // Counters the pane compares against are UI state, not screen state:
                // zeroing bell_count fakes a bell, and zeroing command_seq (compared
                // with `>`) silently suppresses every completion notification until
                // the count climbs back. The title is likewise kept, as xterm does.
                let bells = self.bell_count;
                let cmd_seq = self.command_seq;
                let title = std::mem::take(&mut self.title);
                // Cells kept in scrollback still reference link ids; carry the table
                // so those ids resolve to their real URIs, not aliases of new links.
                let links = std::mem::take(&mut self.links);
                *self = Grid::new(self.cols, self.rows);
                self.scrollback_limit = limit;
                self.scrollback = scrollback;
                self.dropped = dropped;
                self.cell_px = cell_px;
                self.bell_count = bells;
                self.command_seq = cmd_seq;
                self.title = title;
                self.links = links;
            }
            _ => return,
        }
        self.wrap_pending = false;
        self.dirty = true;
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        match params {
            [b"0" | b"2", title, ..] => {
                self.title = String::from_utf8_lossy(title).into_owned();
                self.dirty = true;
            }
            // OSC 7: the shell reports its working directory as `file://host/path`.
            // The portable cwd source (macOS has no /proc).
            [b"7", url, ..] => {
                let s = String::from_utf8_lossy(url);
                if let Some(rest) = s.strip_prefix("file://") {
                    // Drop the host component; keep the absolute path after it.
                    let path = rest.find('/').map(|i| &rest[i..]).unwrap_or(rest);
                    let decoded = percent_decode(path);
                    if !decoded.is_empty() {
                        self.cwd = Some(std::path::PathBuf::from(decoded));
                    }
                }
            }
            // OSC 133: shell integration (FinalTerm/iTerm2). The shell brackets its
            // prompt and each command's output, letting the terminal navigate and
            // extract by command with no guessing. `D` may carry an exit code
            // (`133;D;1`) which drives the pass/fail status gutter.
            [b"133", kind, extra @ ..] => {
                let exit = extra
                    .first()
                    .and_then(|c| std::str::from_utf8(c).ok())
                    .and_then(|s| s.trim().parse::<i32>().ok());
                self.shell_integration(kind, exit);
            }
            // OSC 8: hyperlink. `8 ; params ; URI` opens a link that following cells
            // belong to; `8 ; ; ` (empty URI) closes it. vte split the OSC on ';',
            // so a URI containing ';' arrives as extra parts — rejoin them.
            [b"8", _params, rest @ ..] => {
                let uri = rest
                    .iter()
                    .map(|p| String::from_utf8_lossy(p))
                    .collect::<Vec<_>>()
                    .join(";");
                self.set_hyperlink(&uri);
            }
            // OSC 9;4: progress (ConEmu/Windows Terminal). `9;4;state;percent`.
            [b"9", b"4", rest @ ..] => {
                let num = |i: usize| {
                    rest.get(i).and_then(|b| std::str::from_utf8(b).ok()).and_then(|s| s.trim().parse::<u8>().ok())
                };
                let state = num(0).unwrap_or(0);
                self.progress = if state == 0 {
                    None
                } else {
                    Some((state, num(1).unwrap_or(0).min(100)))
                };
                self.dirty = true;
            }
            _ => {}
        }
    }
}

impl Grid {
    /// Opens (non-empty URI) or closes (empty) the current OSC 8 hyperlink. The URI
    /// is de-duplicated against the table and the id is capped so a link-heavy
    /// session cannot grow it without bound.
    fn set_hyperlink(&mut self, uri: &str) {
        if uri.is_empty() {
            self.pen.link = 0;
            return;
        }
        if let Some(i) = self.links.iter().position(|u| u == uri) {
            self.pen.link = (i + 1) as u16;
            return;
        }
        if self.links.len() >= u16::MAX as usize - 1 {
            // Table full: leave the run untagged rather than alias it to an unrelated
            // URI. The text-based URL hint still catches plain http links.
            self.pen.link = 0;
            return;
        }
        self.links.push(uri.to_string());
        self.pen.link = self.links.len() as u16;
    }

    /// The contiguous run of cells on `abs_row` sharing the hyperlink under `col`:
    /// `(start_col, width, uri)`. Backs the hover underline and click for OSC 8 links.
    pub fn link_span(&self, abs_row: usize, col: usize) -> Option<(usize, usize, String)> {
        let id = self.abs_cell(abs_row, col).pen.link;
        if id == 0 {
            return None;
        }
        let mut start = col;
        while start > 0 && self.abs_cell(abs_row, start - 1).pen.link == id {
            start -= 1;
        }
        let mut end = col;
        while end + 1 < self.cols && self.abs_cell(abs_row, end + 1).pen.link == id {
            end += 1;
        }
        let uri = self.links.get(id as usize - 1).cloned()?;
        Some((start, end - start + 1, uri))
    }

    fn shell_integration(&mut self, kind: &[u8], exit: Option<i32>) {
        match kind.first() {
            // A: a fresh prompt begins here.
            Some(b'A') => {
                let mark = self.local_to_stable(self.cursor_abs());
                // Collapse duplicates: some shells emit A twice per prompt.
                if self.prompt_marks.last() != Some(&mark) {
                    self.prompt_marks.push(mark);
                }
            }
            // B: the prompt ends / command input begins, at the cursor.
            Some(b'B') => {
                self.command_input = Some((self.local_to_stable(self.cursor_abs()), self.col));
            }
            // C: the command's output starts here. Input is done being edited.
            Some(b'C') => {
                self.command_start = Some(self.local_to_stable(self.cursor_abs()));
                self.command_input = None;
            }
            // D: the command finished. Bank its output range for "copy last output",
            // and record the exit code against the prompt that started it so the
            // status gutter can mark it pass (green) or fail (red).
            Some(b'D') => {
                if let Some(start) = self.command_start.take() {
                    // Output ends with a newline, so the cursor sits at column 0 of
                    // the fresh row the NEXT prompt is about to use. Exclude that row
                    // from the output range, or a fold would swallow the live prompt
                    // (and the "N lines" count / copy-last-output would be off by one).
                    let end_local = if self.col == 0 {
                        self.cursor_abs().saturating_sub(1)
                    } else {
                        self.cursor_abs()
                    };
                    let end = self.local_to_stable(end_local).max(start);
                    self.last_output = Some((start, end));
                    // Bank the region for "fold all output"; bound the list.
                    self.outputs.push((start, end));
                    if self.outputs.len() > 512 {
                        self.outputs.drain(0..256);
                    }
                }
                if let (Some(code), Some(&prompt)) = (exit, self.prompt_marks.last()) {
                    self.cmd_exits.push((prompt, code));
                    // Bound the record: it only needs to cover on-screen prompts, and
                    // eviction prunes old prompt marks anyway.
                    if self.cmd_exits.len() > 512 {
                        self.cmd_exits.drain(0..256);
                    }
                }
                self.command_seq += 1;
            }
            _ => {}
        }
    }

    /// Visible prompt rows with the exit code of the command each launched, as
    /// `(screen_row, Option<exit>)`: `Some(0)` ok, `Some(n)` failed, `None` unknown
    /// or still running. `screen_row` is in `0..rows`. Drives the status gutter (D6).
    pub fn command_markers(&self) -> Vec<(usize, Option<i32>)> {
        // A full-screen app owns the screen; the shell's prompt rows underneath are
        // not visible, so no status bars belong there.
        if self.parked.is_some() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &stable in &self.prompt_marks {
            if let Some(local) = self.stable_to_local(stable) {
                if let Some(screen) = self.screen_row_of(local) {
                    let exit =
                        self.cmd_exits.iter().rev().find(|(p, _)| *p == stable).map(|(_, e)| *e);
                    out.push((screen, exit));
                }
            }
        }
        out
    }

    /// The screen row (0..rows) an absolute (local) row lands on, fold-aware: with
    /// folds active it is the row's position in the display plan, else `local - top`.
    /// `None` if the row is off-screen or hidden inside a fold.
    pub fn screen_row_of(&self, local: usize) -> Option<usize> {
        let top = self.scrollback.len().saturating_sub(self.display_offset);
        if !self.has_folds() {
            return local.checked_sub(top).filter(|&r| r < self.rows);
        }
        self.display_plan()
            .iter()
            .position(|p| matches!(p, PlanRow::Real(a) if *a == local))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(grid: &mut Grid, bytes: &str) {
        vte::Parser::new().advance(grid, bytes.as_bytes());
    }

    #[test]
    fn prints_and_wraps_lines() {
        let mut g = Grid::new(10, 3);
        feed(&mut g, "hola\r\nmundo");
        assert_eq!(g.dump(), "hola\nmundo");
        assert_eq!(g.cursor(), (1, 5));
    }

    #[test]
    fn defers_wrap_at_last_column() {
        let mut g = Grid::new(4, 3);
        // Filling the last column must NOT move the cursor to the next row yet.
        feed(&mut g, "abcd");
        assert_eq!(g.cursor(), (0, 3));
        feed(&mut g, "e");
        assert_eq!(g.dump(), "abcd\ne");
        assert_eq!(g.cursor(), (1, 1));
    }

    #[test]
    fn scrolls_when_past_last_row() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "a\r\nb\r\nc");
        assert_eq!(g.dump(), "b\nc");
    }

    #[test]
    fn cursor_position_is_one_based() {
        let mut g = Grid::new(10, 5);
        feed(&mut g, "\x1b[3;5HX");
        assert_eq!(g.cell(2, 4).ch, 'X');
        // A bare CUP homes the cursor.
        feed(&mut g, "\x1b[HY");
        assert_eq!(g.cell(0, 0).ch, 'Y');
    }

    #[test]
    fn erases_display_and_line() {
        let mut g = Grid::new(5, 2);
        feed(&mut g, "abcde\r\nfghij");
        feed(&mut g, "\x1b[2;3H\x1b[K");
        assert_eq!(g.dump(), "abcde\nfg");
        feed(&mut g, "\x1b[2J");
        assert_eq!(g.dump(), "");
    }

    #[test]
    fn double_width_glyph_in_one_column_grid_does_not_panic() {
        // A 1-column grid cannot hold a double-width glyph. Printing one must not
        // spill a spacer into the next row nor index past the buffer end.
        let mut g = Grid::new(1, 1);
        feed(&mut g, "中"); // was: index-out-of-bounds panic at cells[idx + 1]
        assert_eq!(g.cell(0, 0).ch, ' ', "the glyph is dropped, not half-written");

        // Multi-row, single-column: the spacer must not leak into row 1 either.
        let mut g = Grid::new(1, 3);
        feed(&mut g, "中");
        assert!(!g.cell(1, 0).is_spacer(), "no spacer bleeds into the next row");
    }

    #[test]
    fn backspace_and_tab_move_cursor() {
        let mut g = Grid::new(20, 2);
        feed(&mut g, "ab\x08X");
        assert_eq!(g.dump(), "aX");
        feed(&mut g, "\r\n\tZ");
        assert_eq!(g.cell(1, 8).ch, 'Z');
    }

    #[test]
    fn sgr_sets_attributes_and_colors() {
        let mut g = Grid::new(10, 1);
        feed(&mut g, "\x1b[1;31mR");
        let c = g.cell(0, 0);
        assert_eq!(c.pen.fg, Color::Indexed(1));
        assert!(c.pen.flags.contains(Flags::BOLD));

        // Reset must clear both colour and flags.
        feed(&mut g, "\x1b[0mP");
        let c = g.cell(0, 1);
        assert_eq!(c.pen.fg, Color::Default);
        assert!(c.pen.flags.is_empty());
    }

    #[test]
    fn sgr_parses_both_truecolor_forms() {
        let mut g = Grid::new(10, 1);
        feed(&mut g, "\x1b[38;2;255;128;0mS");
        assert_eq!(g.cell(0, 0).pen.fg, Color::Rgb(255, 128, 0));

        feed(&mut g, "\x1b[38:2::10:20:30mC");
        assert_eq!(g.cell(0, 1).pen.fg, Color::Rgb(10, 20, 30));

        feed(&mut g, "\x1b[48;5;99mI");
        assert_eq!(g.cell(0, 2).pen.bg, Color::Indexed(99));
    }

    #[test]
    fn sgr_styled_underlines() {
        let mut g = Grid::new(10, 1);

        // Plain `SGR 4` is a single underline and sets the legacy flag too.
        feed(&mut g, "\x1b[4mA");
        assert_eq!(g.cell(0, 0).pen.underline, UnderlineStyle::Single);
        assert!(g.cell(0, 0).pen.flags.contains(Flags::UNDERLINE));

        // Colon sub-params select the styled forms.
        feed(&mut g, "\x1b[4:3mB");
        assert_eq!(g.cell(0, 1).pen.underline, UnderlineStyle::Curly);
        assert!(g.cell(0, 1).pen.flags.contains(Flags::UNDERLINE));

        feed(&mut g, "\x1b[4:2mC");
        assert_eq!(g.cell(0, 2).pen.underline, UnderlineStyle::Double);
        feed(&mut g, "\x1b[4:4mD");
        assert_eq!(g.cell(0, 3).pen.underline, UnderlineStyle::Dotted);
        feed(&mut g, "\x1b[4:5mE");
        assert_eq!(g.cell(0, 4).pen.underline, UnderlineStyle::Dashed);

        // `SGR 21` is a double underline; `4:0` and `24` turn it off.
        feed(&mut g, "\x1b[21mF");
        assert_eq!(g.cell(0, 5).pen.underline, UnderlineStyle::Double);
        feed(&mut g, "\x1b[4:0mG");
        assert_eq!(g.cell(0, 6).pen.underline, UnderlineStyle::None);
        assert!(!g.cell(0, 6).pen.flags.contains(Flags::UNDERLINE));
    }

    #[test]
    fn sgr_underline_color() {
        let mut g = Grid::new(10, 1);

        // 58 truecolour (colon form with a colourspace slot) and reset with 59.
        feed(&mut g, "\x1b[4:3;58:2::10:20:30mA");
        assert_eq!(g.cell(0, 0).pen.underline, UnderlineStyle::Curly);
        assert_eq!(g.cell(0, 0).pen.underline_color, Color::Rgb(10, 20, 30));

        // 58 indexed form.
        feed(&mut g, "\x1b[58;5;42mB");
        assert_eq!(g.cell(0, 1).pen.underline_color, Color::Indexed(42));

        // 59 resets the colour to "follow the foreground".
        feed(&mut g, "\x1b[59mC");
        assert_eq!(g.cell(0, 2).pen.underline_color, Color::Default);

        // `SGR 24` clears both the style and any custom colour.
        feed(&mut g, "\x1b[4;58;5;9m\x1b[24mD");
        assert_eq!(g.cell(0, 3).pen.underline, UnderlineStyle::None);
        assert_eq!(g.cell(0, 3).pen.underline_color, Color::Default);
    }

    #[test]
    fn answers_device_attributes_and_cursor_position() {
        // Regression: without a DA1 reply, fish waits 10s and disables features.
        let mut g = Grid::new(20, 5);
        feed(&mut g, "\x1b[c");
        assert_eq!(g.take_responses(), vec![b"\x1b[?62;22c".to_vec()]);
        feed(&mut g, "\x1b[>c");
        assert_eq!(g.take_responses(), vec![b"\x1b[>1;0;0c".to_vec()]);
        // DSR cursor position, 1-based.
        feed(&mut g, "\x1b[3;5H\x1b[6n");
        assert_eq!(g.take_responses(), vec![b"\x1b[3;5R".to_vec()]);
        // Responses are drained: a second take is empty.
        assert!(g.take_responses().is_empty());
    }

    #[test]
    fn osc_sets_title() {
        let mut g = Grid::new(10, 1);
        feed(&mut g, "\x1b]0;runnir\x07");
        assert_eq!(g.title, "runnir");
    }

    #[test]
    fn alt_screen_restores_primary_on_exit() {
        let mut g = Grid::new(6, 3);
        feed(&mut g, "antes");
        feed(&mut g, "\x1b[?1049h");
        assert_eq!(g.dump(), "", "the alternate screen starts blank");
        feed(&mut g, "vim!");
        assert_eq!(g.dump(), "vim!");
        // Quitting a full-screen app must give back exactly what was there.
        feed(&mut g, "\x1b[?1049l");
        assert_eq!(g.dump(), "antes");
        assert_eq!(g.cursor(), (0, 5));
    }

    #[test]
    fn scroll_region_confines_scrolling() {
        let mut g = Grid::new(3, 5);
        feed(&mut g, "a\r\nb\r\nc\r\nd\r\ne");
        // Confine scrolling to rows 2..4 (1-based), leaving row 1 and 5 pinned.
        feed(&mut g, "\x1b[2;4r");
        feed(&mut g, "\x1b[4;1Hx\n");
        assert_eq!(g.dump(), "a\nc\nx\n\ne", "rows outside the region must not move");
    }

    #[test]
    fn reverse_index_scrolls_down_at_region_top() {
        let mut g = Grid::new(3, 4);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        feed(&mut g, "\x1b[1;1H\x1bM");
        assert_eq!(g.dump(), "\na\nb\nc");
    }

    #[test]
    fn insert_and_delete_lines() {
        let mut g = Grid::new(3, 4);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        feed(&mut g, "\x1b[2;1H\x1b[L");
        assert_eq!(g.dump(), "a\n\nb\nc");
        feed(&mut g, "\x1b[2;1H\x1b[M");
        assert_eq!(g.dump(), "a\nb\nc");
    }

    #[test]
    fn insert_and_delete_chars() {
        let mut g = Grid::new(8, 1);
        feed(&mut g, "abcdef");
        feed(&mut g, "\x1b[1;3H\x1b[2P");
        assert_eq!(g.dump(), "abef");
        feed(&mut g, "\x1b[1;3H\x1b[2@");
        assert_eq!(g.dump(), "ab  ef");
    }

    #[test]
    fn private_modes_toggle_state() {
        let mut g = Grid::new(4, 2);
        assert!(g.cursor_visible && !g.app_cursor && !g.bracketed_paste);
        feed(&mut g, "\x1b[?25l\x1b[?1h\x1b[?2004h");
        assert!(!g.cursor_visible && g.app_cursor && g.bracketed_paste);
        feed(&mut g, "\x1b[?25h\x1b[?1l\x1b[?2004l");
        assert!(g.cursor_visible && !g.app_cursor && !g.bracketed_paste);
    }

    #[test]
    fn autowrap_off_sticks_in_last_column() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "\x1b[?7l");
        feed(&mut g, "abcdef");
        // Each glyph past the edge overwrites the last cell instead of wrapping.
        assert_eq!(g.dump(), "abcf");
        assert_eq!(g.cursor(), (0, 3));
    }

    #[test]
    fn delete_lines_does_not_feed_scrollback() {
        // Regression: DL (CSI M) at row 0 with no scroll region used to archive the
        // deleted lines into history as if they had scrolled off.
        let mut g = Grid::new(4, 3);
        feed(&mut g, "a\r\nb\r\nc");
        let before = g.total_rows();
        feed(&mut g, "\x1b[1;1H\x1b[2M"); // home, delete 2 lines
        assert_eq!(g.total_rows(), before, "deleted lines must not enter scrollback");
    }

    #[test]
    fn combining_marks_are_dropped_not_widened() {
        // Regression: width-0 chars used to be forced to width 1, drifting the
        // cursor and splitting NFD text.
        let mut g = Grid::new(10, 1);
        feed(&mut g, "e\u{301}x"); // e + combining acute + x
        assert_eq!(g.cell(0, 0).ch, 'e');
        assert_eq!(g.cell(0, 1).ch, 'x', "the mark must not occupy its own cell");
        assert_eq!(g.cursor(), (0, 2));
    }

    #[test]
    fn ris_keeps_scrollback_and_limit() {
        // Regression: ESC c rebuilt the grid from scratch, losing scrollback and
        // reverting a configured limit.
        let mut g = Grid::new(4, 2);
        g.set_scrollback_limit(50);
        feed(&mut g, "a\r\nb\r\nc\r\nd"); // pushes lines into scrollback
        let sb = g.total_rows();
        feed(&mut g, "\x1bc");
        assert_eq!(g.scrollback_limit, 50, "the configured limit must survive RIS");
        assert!(g.total_rows() >= sb - g.rows(), "scrollback must survive RIS");
    }

    #[test]
    fn ed_mode_3_clears_scrollback() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        assert!(g.scrollback.len() > 0);
        feed(&mut g, "\x1b[3J");
        assert_eq!(g.scrollback.len(), 0, "3J must erase saved lines");
        // Regression: 3J also blanked the visible screen; xterm's "erase saved
        // lines" touches history only — clearing the screen is 2J's job.
        assert_eq!(g.dump(), "c\nd", "3J must leave the visible screen alone");
    }

    #[test]
    fn search_finds_matches_across_scrollback_and_screen() {
        let mut g = Grid::new(20, 2);
        feed(&mut g, "error here\r\nall fine\r\nanother error now");
        let hits = g.search("error");
        assert_eq!(hits.len(), 2, "both 'error' occurrences, in scrollback and screen");
        // Case-insensitive.
        feed(&mut g, "\r\nERROR shouting");
        assert_eq!(g.search("error").len(), 3);
        assert!(g.search("nope").is_empty());
    }

    #[test]
    fn search_with_multibyte_needle_does_not_panic() {
        // Regression: advancing by a single byte after a multibyte match landed
        // inside a UTF-8 char, so the next slice panicked ("not a char boundary").
        let mut g = Grid::new(20, 2);
        feed(&mut g, "café société");
        assert_eq!(g.search("é").len(), 3, "all three accented chars found");
        assert_eq!(g.search("café").len(), 1);
    }

    #[test]
    fn sticky_prompt_pins_the_command_being_read() {
        let mut g = Grid::new(20, 3);
        // A first command with a marked prompt, then lots of later output so the
        // first prompt ends up well above the viewport when scrolled part-way back.
        feed(&mut g, "\x1b]133;A\x1b\\$ first\r\n");
        for i in 0..8 {
            feed(&mut g, &format!("out{i}\r\n"));
        }
        // Following live output: nothing pinned.
        assert!(g.sticky_prompt().is_none());
        // Scroll back a little — the first prompt sits above the top row now.
        g.scroll_display(3);
        let sticky = g.sticky_prompt();
        assert_eq!(sticky.as_deref(), Some("$ first"), "the command's prompt must pin");
    }

    #[test]
    fn scrollback_text_includes_the_visible_screen() {
        // Regression: session save dropped the on-screen rows, losing the last
        // screenful of output.
        let mut g = Grid::new(10, 3);
        feed(&mut g, "one\r\ntwo\r\nthree");
        let text = g.scrollback_text();
        assert!(text.contains(&"three".to_string()), "visible rows must be saved: {text:?}");
        assert!(text.contains(&"one".to_string()));
    }

    #[test]
    fn scrolled_off_lines_reach_scrollback() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        assert_eq!(g.dump(), "c\nd", "the screen only holds the last two");
        assert_eq!(g.total_rows(), 4, "the other two are in scrollback");
        // Absolute addressing sees the whole history.
        assert_eq!(g.abs_cell(0, 0).ch, 'a');
        assert_eq!(g.abs_cell(1, 0).ch, 'b');
        assert_eq!(g.abs_cell(3, 0).ch, 'd');
    }

    #[test]
    fn alt_screen_never_pollutes_scrollback() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "keep\r\n");
        let before = g.total_rows();
        feed(&mut g, "\x1b[?1049h");
        // A full-screen app churning through output must not evict history.
        for _ in 0..50 {
            feed(&mut g, "junk\r\n");
        }
        feed(&mut g, "\x1b[?1049l");
        assert_eq!(g.total_rows(), before, "htop must not eat the scrollback");
        assert_eq!(g.abs_cell(0, 0).ch, 'k');
    }

    #[test]
    fn region_scroll_never_pollutes_scrollback() {
        let mut g = Grid::new(4, 4);
        feed(&mut g, "\x1b[1;3r");
        let before = g.total_rows();
        for _ in 0..20 {
            feed(&mut g, "x\r\n");
        }
        assert_eq!(g.total_rows(), before, "a region scroll is not history");
    }

    #[test]
    fn scrollback_is_capped() {
        let mut g = Grid::new(4, 2);
        g.scrollback_limit = 3;
        for i in 0..20 {
            feed(&mut g, &format!("{}\r\n", i % 10));
        }
        assert_eq!(g.total_rows(), 5, "3 scrollback + 2 screen rows");
    }

    #[test]
    fn viewport_scrolls_back_and_returns() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        assert_eq!(g.abs_row(0), 2, "at the bottom the viewport starts after history");

        assert!(g.scroll_display(2));
        assert_eq!(g.abs_row(0), 0, "scrolled back to the oldest line");
        assert_eq!(g.abs_cell(g.abs_row(0), 0).ch, 'a');

        // Clamped: you cannot scroll past the start.
        assert!(!g.scroll_display(5));
        assert!(g.scroll_to_bottom());
        assert_eq!(g.abs_row(0), 2);
    }

    #[test]
    fn scrolled_back_view_does_not_drift() {
        let mut g = Grid::new(4, 2);
        feed(&mut g, "a\r\nb\r\nc\r\nd");
        g.scroll_display(2);
        let pinned = g.abs_cell(g.abs_row(0), 0).ch;
        // New output arrives while the user reads history.
        feed(&mut g, "\r\ne\r\nf");
        assert_eq!(g.abs_cell(g.abs_row(0), 0).ch, pinned, "the view must stay put");
    }

    #[test]
    fn text_range_spans_scrollback_and_screen() {
        let mut g = Grid::new(6, 2);
        feed(&mut g, "uno\r\ndos\r\ntres\r\ncuatro");
        assert_eq!(g.text_range((0, 0), (3, 5)), "uno\ndos\ntres\ncuatro");
        assert_eq!(g.text_range((1, 0), (2, 3)), "dos\ntres");
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let mut g = Grid::new(10, 1);
        feed(&mut g, "a世b");
        assert_eq!(g.cell(0, 0).ch, 'a');
        assert_eq!(g.cell(0, 1).ch, '世');
        assert!(g.cell(0, 2).is_spacer(), "the right half is a spacer");
        assert_eq!(g.cell(0, 3).ch, 'b');
        assert_eq!(g.cursor(), (0, 4));
        // Copied text must not contain the spacer.
        assert_eq!(g.dump(), "a世b");
    }

    #[test]
    fn wide_char_wraps_early_rather_than_splitting() {
        let mut g = Grid::new(4, 2);
        // Three columns used, one free: 世 needs two, so it must move down.
        feed(&mut g, "abc世");
        assert_eq!(g.cell(0, 3).ch, ' ', "the odd column is left empty");
        assert_eq!(g.cell(1, 0).ch, '世');
        assert!(g.cell(1, 1).is_spacer());
    }

    #[test]
    fn overwriting_half_a_wide_char_clears_the_other_half() {
        let mut g = Grid::new(6, 1);
        feed(&mut g, "世界");
        // Land on the spacer of the first glyph and overwrite it.
        feed(&mut g, "\x1b[1;2HX");
        assert_eq!(g.cell(0, 0).ch, ' ', "the orphaned leader must be blanked");
        assert_eq!(g.cell(0, 1).ch, 'X');
        assert_eq!(g.dump(), " X界");

        // Now land on a leader and overwrite it.
        feed(&mut g, "\x1b[1;3HY");
        assert_eq!(g.cell(0, 2).ch, 'Y');
        assert_eq!(g.cell(0, 3).ch, ' ', "the orphaned spacer must be blanked");
    }

    #[test]
    fn sgr_does_not_cancel_pending_wrap() {
        // Regression: any CSI cleared the deferred-wrap flag, so a colour change
        // right after filling the last column made the next glyph overwrite that
        // column instead of wrapping.
        let mut g = Grid::new(4, 3);
        feed(&mut g, "abcd\x1b[31me");
        assert_eq!(g.dump(), "abcd\ne", "the glyph after SGR must wrap");
        assert_eq!(g.cell(1, 0).pen.fg, Color::Indexed(1));
        assert_eq!(g.cursor(), (1, 1));
    }

    #[test]
    fn cursor_up_down_stop_at_scroll_region_margins() {
        // Regression: CUU/CUD ignored DECSTBM, letting the cursor drift out of
        // the region and write over pinned rows.
        let mut g = Grid::new(3, 5);
        feed(&mut g, "\x1b[2;4r"); // region rows 2..4 (1-based)
        feed(&mut g, "\x1b[3;1H\x1b[9A");
        assert_eq!(g.cursor().0, 1, "CUU stops at the top margin");
        feed(&mut g, "\x1b[9B");
        assert_eq!(g.cursor().0, 3, "CUD stops at the bottom margin");
        // A cursor outside the region is not confined by it.
        feed(&mut g, "\x1b[5;1H\x1b[9B");
        assert_eq!(g.cursor().0, 4, "below the region CUD reaches the last row");
    }

    #[test]
    fn erase_saved_lines_keeps_stable_marks_valid() {
        // Regression: 3J cleared scrollback without accounting the cleared rows
        // as dropped, so every stable row (OSC 133 marks, image anchors) pointed
        // scrollback.len() rows too low afterwards.
        let mut g = Grid::new(10, 3);
        feed(&mut g, "a\r\nb\r\nc\r\nd\r\n"); // pushes rows into scrollback
        assert!(g.scrollback.len() > 0);
        feed(&mut g, "\x1b]133;C\x07out\r\n");
        feed(&mut g, "\x1b[3J"); // clear saved lines mid-command
        feed(&mut g, "\x1b]133;D\x07");
        let text = g.last_command_output().expect("output range must survive 3J");
        assert!(text.contains("out"), "the marked output must still resolve: {text:?}");
    }

    #[test]
    fn ris_preserves_cell_pixel_size() {
        // Regression: ESC c rebuilt the grid and zeroed cell_px, so images placed
        // after a reset fell back to a bogus 1-row footprint.
        let mut g = Grid::new(10, 3);
        g.set_cell_px(8.0, 16.0);
        feed(&mut g, "\x1bc");
        assert_eq!(g.cell_px, (8.0, 16.0), "cell_px is renderer state, not terminal state");
    }

    #[test]
    fn search_reports_grid_columns_with_wide_chars() {
        // Regression: the match column was counted over the spacer-filtered
        // string, so wide chars before the match shifted the highlight left.
        let mut g = Grid::new(20, 1);
        feed(&mut g, "日本 abc");
        // 日 spans cols 0-1, 本 cols 2-3, the space col 4, so 'a' sits at col 5.
        assert_eq!(g.search("abc"), vec![(0, 5)]);
        assert_eq!(g.search("本"), vec![(0, 2)]);
    }

    #[test]
    fn save_and_restore_cursor() {
        let mut g = Grid::new(10, 3);
        feed(&mut g, "\x1b[2;5H\x1b7");
        feed(&mut g, "\x1b[1;1HX\x1b8Y");
        assert_eq!(g.cell(0, 0).ch, 'X');
        assert_eq!(g.cell(1, 4).ch, 'Y');
    }

    #[test]
    fn command_text_excludes_the_prompt_via_osc133_b() {
        let mut g = Grid::new(40, 3);
        // Prompt start (A), the prompt itself, prompt end / input begins (B), command.
        feed(&mut g, "\x1b]133;A\x07pedro$ \x1b]133;B\x07rm -rf /");
        assert_eq!(g.current_command_text(), "rm -rf /");
    }

    #[test]
    fn fold_collapses_command_output_in_the_plan() {
        let mut g = Grid::new(20, 6);
        // Prompt, command, three lines of output, done.
        feed(&mut g, "\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07a\r\nb\r\nc\r\n\x1b]133;D;0\x07");
        assert!(!g.has_folds());
        g.fold_all();
        assert!(g.has_folds(), "an output region of >1 row should fold");
        let plan = g.display_plan();
        assert_eq!(plan.len(), g.rows());
        let folds = plan.iter().filter(|p| matches!(p, PlanRow::Fold { .. })).count();
        assert_eq!(folds, 1, "the output collapses to one summary row: {plan:?}");
        // Unfolding restores plain rows.
        g.unfold_all();
        assert!(!g.has_folds());
        assert!(g.display_plan().iter().all(|p| !matches!(p, PlanRow::Fold { .. })));
    }

    #[test]
    fn clear_drops_live_screen_folds() {
        let mut g = Grid::new(20, 6);
        feed(&mut g, "\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07a\r\nb\r\n\x1b]133;D;0\x07");
        g.fold_all();
        assert!(g.has_folds());
        // A clear (CSI 2J) erases the live screen in place: the fold anchored there
        // must go, or fresh output would render under a stale summary.
        feed(&mut g, "\x1b[2J");
        assert!(!g.has_folds(), "clear must drop live-screen folds");
    }

    #[test]
    fn fold_keeps_the_live_prompt_visible() {
        let mut g = Grid::new(20, 6);
        // A finished command, then the NEXT prompt is emitted (as a real shell does).
        feed(
            &mut g,
            "\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07a\r\nb\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ",
        );
        g.fold_all();
        let plan = g.display_plan();
        // The cursor's row must still be a Real row (not swallowed by the fold), or
        // the user would be typing into an invisible prompt.
        let cur = g.cursor_abs();
        assert!(
            plan.iter().any(|p| matches!(p, PlanRow::Real(a) if *a == cur)),
            "the live prompt row must survive folding: {plan:?}"
        );
    }

    #[test]
    fn status_gutter_records_exit_codes_per_prompt() {
        let mut g = Grid::new(20, 4);
        // Prompt A on row 0, command runs, finishes with exit 0.
        feed(&mut g, "\x1b]133;A\x07$ true\r\n\x1b]133;C\x07\x1b]133;D;0\x07");
        let m = g.command_markers();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, Some(0), "first command succeeded");
        // A second prompt further down, command fails with exit 1.
        feed(&mut g, "\x1b]133;A\x07$ false\r\n\x1b]133;C\x07\x1b]133;D;1\x07");
        let m = g.command_markers();
        assert!(m.iter().any(|&(_, e)| e == Some(1)), "second command failed: {m:?}");
    }

    #[test]
    fn osc7_reports_working_directory() {
        let mut g = Grid::new(20, 3);
        assert_eq!(g.cwd(), None);
        feed(&mut g, "\x1b]7;file://host/home/pedro/my%20dir\x07");
        assert_eq!(g.cwd(), Some(std::path::PathBuf::from("/home/pedro/my dir")));
    }

    #[test]
    fn osc9_4_sets_and_clears_progress() {
        let mut g = Grid::new(20, 3);
        assert_eq!(g.progress(), None);
        feed(&mut g, "\x1b]9;4;1;42\x07");
        assert_eq!(g.progress(), Some((1, 42)));
        feed(&mut g, "\x1b]9;4;2;90\x07");
        assert_eq!(g.progress(), Some((2, 90)), "error state");
        feed(&mut g, "\x1b]9;4;0\x07");
        assert_eq!(g.progress(), None, "state 0 clears");
        // Percent is clamped to 100.
        feed(&mut g, "\x1b]9;4;1;250\x07");
        assert_eq!(g.progress().map(|(_, p)| p), Some(100));
    }

    #[test]
    fn osc8_hyperlink_tags_its_cells() {
        let mut g = Grid::new(20, 3);
        // Open a link, print text under it, close it, print plain text.
        feed(&mut g, "\x1b]8;;https://go2chain.es\x07link\x1b]8;;\x07 x");
        // The 4 'link' cells share one hyperlink span; the trailing text has none.
        let span = g.link_span(g.abs_row(0), 1).expect("link on the tagged cells");
        assert_eq!(span.0, 0, "span starts at column 0");
        assert_eq!(span.1, 4, "span covers 'link'");
        assert_eq!(span.2, "https://go2chain.es");
        assert!(g.link_span(g.abs_row(0), 6).is_none(), "plain text carries no link");
    }

    #[test]
    fn ris_preserves_ui_counters() {
        let mut g = Grid::new(20, 3);
        g.bell_count = 3;
        // Two finished commands (OSC 133;D) bump command_seq.
        feed(&mut g, "\x1b]133;D\x07\x1b]133;D\x07");
        let seq = g.command_seq();
        assert!(seq >= 2);
        // RIS (ESC c) must not zero these, or the pane sees a phantom bell and
        // suppresses completion notifications (compared with `>`).
        feed(&mut g, "\x1bc");
        assert_eq!(g.bell_count, 3, "bell_count reset by RIS");
        assert_eq!(g.command_seq(), seq, "command_seq reset by RIS");
    }

    #[test]
    fn command_text_seen_even_with_cursor_moved_back() {
        let mut g = Grid::new(40, 3);
        // Type a command, then send CR to move the cursor to column 0 (as Home / ^A
        // would). The whole line must still be scanned, not just up to the cursor.
        feed(&mut g, "\x1b]133;A\x07$ \x1b]133;B\x07rm -rf /\r");
        assert_eq!(g.current_command_text(), "rm -rf /");
    }

    #[test]
    fn command_text_falls_back_to_cursor_row_without_marks() {
        let mut g = Grid::new(40, 3);
        feed(&mut g, "some text here");
        assert_eq!(g.current_command_text(), "some text here");
    }

    #[test]
    fn watch_scan_reports_nothing_when_no_new_output() {
        let mut g = Grid::new(20, 3);
        feed(&mut g, "hello\r\n");
        let mark = g.watch_mark();
        let (text, next) = g.text_since_stable(mark);
        assert!(text.is_empty(), "no new output since the mark, got {text:?}");
        assert_eq!(next, g.watch_mark());
    }

    #[test]
    fn bel_bumps_the_bell_count_without_printing() {
        let mut g = Grid::new(10, 3);
        assert_eq!(g.bell_count, 0);
        feed(&mut g, "a\x07b");
        assert_eq!(g.bell_count, 1);
        // The BEL itself leaves no glyph: only 'a' and 'b' land.
        assert_eq!(g.cell(0, 0).ch, 'a');
        assert_eq!(g.cell(0, 1).ch, 'b');
    }

    #[test]
    fn kitty_keyboard_flag_stack_push_pop_query_set() {
        let mut g = Grid::new(20, 3);
        // Empty stack reports 0.
        assert_eq!(g.keyboard_flags(), 0);
        feed(&mut g, "\x1b[?u");
        assert_eq!(g.take_responses(), vec![b"\x1b[?0u".to_vec()]);

        // Push flags 5, then 9: top wins, query reflects it.
        feed(&mut g, "\x1b[>5u");
        assert_eq!(g.keyboard_flags(), 5);
        feed(&mut g, "\x1b[>9u");
        assert_eq!(g.keyboard_flags(), 9);
        feed(&mut g, "\x1b[?u");
        assert_eq!(g.take_responses(), vec![b"\x1b[?9u".to_vec()]);

        // Set-current (mode 1 default) replaces the top of the stack.
        feed(&mut g, "\x1b[=3u");
        assert_eq!(g.keyboard_flags(), 3);
        // Mode 2 = set bits (OR).
        feed(&mut g, "\x1b[=8;2u");
        assert_eq!(g.keyboard_flags(), 11);
        // Mode 3 = clear bits (AND NOT).
        feed(&mut g, "\x1b[=1;3u");
        assert_eq!(g.keyboard_flags(), 10);

        // Pop 1 (default) drops back to the 5 pushed first.
        feed(&mut g, "\x1b[<u");
        assert_eq!(g.keyboard_flags(), 5);
        // Pop more than present clamps to empty (0), never panics.
        feed(&mut g, "\x1b[<9u");
        assert_eq!(g.keyboard_flags(), 0);
    }

    #[test]
    fn kitty_flags_reset_on_alt_screen_and_ris() {
        let mut g = Grid::new(20, 3);
        feed(&mut g, "\x1b[>15u");
        assert_eq!(g.keyboard_flags(), 15);
        // Entering the alternate screen resets the (shared) stack.
        feed(&mut g, "\x1b[?1049h");
        assert_eq!(g.keyboard_flags(), 0);
        feed(&mut g, "\x1b[>7u");
        assert_eq!(g.keyboard_flags(), 7);
        // Leaving it resets again.
        feed(&mut g, "\x1b[?1049l");
        assert_eq!(g.keyboard_flags(), 0);
        // RIS clears any pushed flags.
        feed(&mut g, "\x1b[>7u\x1bc");
        assert_eq!(g.keyboard_flags(), 0);
    }
}
