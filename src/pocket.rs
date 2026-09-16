//! The window in a phone's hand — composing the screen into one grid, and the row
//! diff that keeps a phone up to date over a socket.
//!
//! ## Why this is a composition and not a read
//!
//! Everything runnir draws is already a `Grid` with an origin: `build_chrome` returns
//! `(Grid, f32, f32)`, `PaneDraw` is a grid plus a pixel origin, and the overlay panels
//! go through the same path. At the moment of painting, the window IS a list of layers,
//! stacked in vector order by the painter's algorithm. This module flattens that list
//! into one window-sized grid, which is what a phone can be shown.
//!
//! It therefore has to run where those layers exist — inside the frame — because the
//! chrome and the overlays are built fresh each time and live nowhere else. A pane
//! could have been read from any thread through its `Arc<Mutex<Grid>>`; the tab bar
//! could not.
//!
//! ## What is deliberately lost
//!
//! Anything that is not a cell: inline images (kitty graphics), the optical-transfer
//! QR, the player's waveform. They are textures, they are not in the layer list, and on
//! the phone they are holes. That was accepted when the scope widened to the window.
//!
//! The pure half lives here so it can be tested without a window, a GPU or a socket.

use crate::config::{Rgb, Theme};
use crate::grid::{Cell, Color, Flags, Grid, PlanRow, SPACER};

/// One layer to compose: a grid and where its top-left cell lands, **in cells**.
///
/// The caller converts pixel origins with the cell size; this module never sees a
/// pixel. Keeping it in cells is what lets `compose` be tested against hand-built
/// grids.
pub struct Layer<'a> {
    pub grid: &'a Grid,
    pub col: usize,
    pub row: usize,
    /// Draw only what this layer actually marks, letting the layer below show through
    /// its blanks. Set for annotation layers like the hint overlay — without it, a
    /// full-window transparent layer erases everything it was meant to annotate.
    pub transparent: bool,
    /// Where this layer's cursor sits, in the layer's own coordinates, if it owns one.
    pub cursor: Option<(usize, usize)>,
    /// Dim every cell this layer contributes, for the unfocused panes the renderer
    /// draws at 0.62. Carried so the phone shows the same "which pane is live" cue the
    /// screen does.
    pub dim: bool,
}

/// The whole window as cells, ready to diff and send.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `cols * rows` long.
    pub cells: Vec<Cell>,
    /// Cells this snapshot dimmed, parallel to `cells`.
    pub dim: Vec<bool>,
    /// Cursor position in window coordinates, if one is visible.
    pub cursor: Option<(usize, usize)>,
}

impl Snapshot {
    pub fn blank(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            dim: vec![false; cols * rows],
            cursor: None,
        }
    }

    pub fn row(&self, row: usize) -> &[Cell] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }

    fn dim_row(&self, row: usize) -> &[bool] {
        &self.dim[row * self.cols..(row + 1) * self.cols]
    }
}

/// The rows a grid actually shows, fold-aware, exactly as `pane_instances` computes
/// them. A chrome grid has no folds and no scrollback, so this is the identity for it.
fn row_plan(grid: &Grid) -> Vec<PlanRow> {
    if grid.has_folds() {
        grid.display_plan()
    } else {
        (0..grid.rows()).map(|r| PlanRow::Real(grid.abs_row(r))).collect()
    }
}

/// Flattens stacked layers into one window-sized grid.
///
/// Later layers win, which is the painter's algorithm the renderer already applies by
/// pushing instances in order — the snapshot must agree with it or the phone shows a
/// different window from the screen.
pub fn compose(layers: &[Layer], cols: usize, rows: usize) -> Snapshot {
    let mut out = Snapshot::blank(cols, rows);

    for layer in layers {
        let plan = row_plan(layer.grid);
        for (lrow, prow) in plan.iter().enumerate() {
            let y = layer.row + lrow;
            if y >= rows {
                break;
            }
            let abs = match prow {
                PlanRow::Real(a) => *a,
                // A collapsed fold stands in for hidden rows. The renderer draws a
                // summary bar; here it is one line of text, which is the honest
                // approximation until the summary is factored out of the renderer.
                PlanRow::Fold { lines, .. } => {
                    write_fold(&mut out, y, layer.col, layer.grid.cols(), *lines);
                    continue;
                }
                PlanRow::Blank => continue,
            };

            for lcol in 0..layer.grid.cols() {
                let x = layer.col + lcol;
                if x >= cols {
                    break;
                }
                let cell = layer.grid.abs_cell(abs, lcol);
                // An annotation layer contributes only its own marks.
                if layer.transparent && cell.ch == ' ' && matches!(cell.pen.bg, Color::Default) {
                    continue;
                }
                // The spacer half of a wide glyph is KEPT, unlike in the renderer,
                // which skips it because the left cell draws across both. Here it has
                // to occupy its column so that a layer stacked on top lands on the
                // right one; the serialiser drops it instead, where a monospaced font
                // is already giving the wide glyph two columns of its own.
                out.cells[y * cols + x] = cell;
                out.dim[y * cols + x] = layer.dim;
            }
        }

        if let Some((crow, ccol)) = layer.cursor {
            let (y, x) = (layer.row + crow, layer.col + ccol);
            if y < rows && x < cols {
                out.cursor = Some((y, x));
            }
        }
    }

    out
}

fn write_fold(out: &mut Snapshot, row: usize, col: usize, width: usize, lines: usize) {
    let text = format!("⋯ {lines} lines folded");
    let pen = crate::grid::Pen {
        fg: Color::Indexed(8),
        ..Default::default()
    };
    for (i, ch) in text.chars().enumerate() {
        let x = col + i;
        if i >= width || x >= out.cols {
            break;
        }
        out.cells[row * out.cols + x] = Cell { ch, pen };
    }
}

/// A stretch of one row sharing every visual attribute, which is how a row is sent.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Run {
    pub text: String,
    /// Resolved to RGB here rather than sent as an index, so the phone cannot pick a
    /// different palette than the screen is using.
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    /// Bold/italic/underline/strike, packed as `Flags` bits so the page can test them
    /// without a second vocabulary.
    pub flags: u8,
}

/// One row that changed, with its index.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RowUpdate {
    pub row: usize,
    pub runs: Vec<Run>,
}

/// Resolves a cell colour the way the renderer does, so the phone and the screen agree.
///
/// `REVERSE` is applied here rather than left to the page: it is a property of the
/// cell, and a client that forgot it would silently show selected text as unselected.
fn colours(cell: &Cell, theme: &Theme, dim: bool) -> ((u8, u8, u8), (u8, u8, u8)) {
    let resolve = |c: Color, is_fg: bool| -> Rgb {
        match c {
            Color::Default if is_fg => theme.foreground,
            Color::Default => theme.background,
            Color::Rgb(r, g, b) => Rgb(r, g, b),
            Color::Indexed(i) => crate::render::xterm256(i, &theme.ansi),
        }
    };
    let mut fg = resolve(cell.pen.fg, true);
    let mut bg = resolve(cell.pen.bg, false);
    if cell.pen.flags.contains(Flags::REVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.pen.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    let scale = |v: u8, f: f32| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    if dim || cell.pen.flags.contains(Flags::DIM) {
        let f = if dim { 0.62 } else { 0.7 };
        fg = Rgb(scale(fg.0, f), scale(fg.1, f), scale(fg.2, f));
    }
    ((fg.0, fg.1, fg.2), (bg.0, bg.1, bg.2))
}

/// Packs one row into runs, merging neighbouring cells that look the same.
pub fn row_runs(snapshot: &Snapshot, row: usize, theme: &Theme) -> Vec<Run> {
    let cells = snapshot.row(row);
    let dims = snapshot.dim_row(row);
    let mut runs: Vec<Run> = Vec::new();

    for (cell, &dim) in cells.iter().zip(dims) {
        // The wide glyph to its left already occupies this column in any monospaced
        // font; emitting the spacer too would push the rest of the row one cell right.
        if cell.ch == SPACER {
            continue;
        }
        let (fg, bg) = colours(cell, theme, dim);
        let flags = cell.pen.flags.bits();
        match runs.last_mut() {
            Some(last) if last.fg == fg && last.bg == bg && last.flags == flags => {
                last.text.push(cell.ch);
            }
            _ => runs.push(Run {
                text: cell.ch.to_string(),
                fg,
                bg,
                flags,
            }),
        }
    }

    // A row that is blank to its end costs nothing to say so: trailing spaces on the
    // theme background are what most of a screen is.
    if let Some(last) = runs.last() {
        if last.text.chars().all(|c| c == ' ') && last.bg == (theme.background.0, theme.background.1, theme.background.2) {
            runs.pop();
        }
    }
    runs
}

/// Rows that differ between two snapshots, as the page needs them.
///
/// `None` for `prev` means "the client just connected": every row is sent. A client
/// that missed a frame asks with the last snapshot it has and is made whole, so there
/// is no resync path of its own — the general case already is the resync.
pub fn diff(prev: Option<&Snapshot>, next: &Snapshot, theme: &Theme) -> Vec<RowUpdate> {
    let mut out = Vec::new();
    let same_shape = prev.is_some_and(|p| p.cols == next.cols && p.rows == next.rows);

    for row in 0..next.rows {
        let changed = match prev {
            Some(p) if same_shape => p.row(row) != next.row(row) || p.dim_row(row) != next.dim_row(row),
            _ => true,
        };
        if changed {
            out.push(RowUpdate {
                row,
                runs: row_runs(next, row, theme),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Pen;

    fn grid_with(text: &str, cols: usize) -> Grid {
        let mut g = Grid::new(cols, 1);
        g.write_str(0, 0, text, Pen::default());
        g
    }

    fn theme() -> Theme {
        Theme::default()
    }

    #[test]
    fn a_later_layer_covers_an_earlier_one() {
        let under = grid_with("aaaaaa", 6);
        let over = grid_with("XY", 2);
        let snap = compose(
            &[
                Layer { grid: &under, col: 0, row: 0, transparent: false, cursor: None, dim: false },
                Layer { grid: &over, col: 2, row: 0, transparent: false, cursor: None, dim: false },
            ],
            6,
            1,
        );
        let line: String = snap.row(0).iter().map(|c| c.ch).collect();
        assert_eq!(line, "aaXYaa", "the later layer wins, which is the painter's order");
    }

    #[test]
    fn a_transparent_layer_lets_blanks_through() {
        let under = grid_with("abcdef", 6);
        let over = grid_with("  Z   ", 6);
        let snap = compose(
            &[
                Layer { grid: &under, col: 0, row: 0, transparent: false, cursor: None, dim: false },
                Layer { grid: &over, col: 0, row: 0, transparent: true, cursor: None, dim: false },
            ],
            6,
            1,
        );
        let line: String = snap.row(0).iter().map(|c| c.ch).collect();
        assert_eq!(line, "abZdef", "only the mark lands; the rest shows through");
    }

    #[test]
    fn a_layer_is_clipped_to_the_window() {
        let wide = grid_with("abcdef", 6);
        let snap = compose(
            &[Layer { grid: &wide, col: 4, row: 0, transparent: false, cursor: None, dim: false }],
            6,
            1,
        );
        let line: String = snap.row(0).iter().map(|c| c.ch).collect();
        assert_eq!(line, "    ab", "a layer hanging off the edge does not wrap or panic");
    }

    #[test]
    fn a_layer_below_the_window_is_dropped() {
        let g = grid_with("abc", 3);
        let snap = compose(
            &[Layer { grid: &g, col: 0, row: 5, transparent: false, cursor: None, dim: false }],
            3,
            2,
        );
        assert!(snap.cells.iter().all(|c| c.ch == ' '));
    }

    #[test]
    fn the_cursor_lands_in_window_coordinates() {
        let g = grid_with("abc", 3);
        let snap = compose(
            &[Layer { grid: &g, col: 10, row: 4, transparent: false, cursor: Some((0, 2)), dim: false }],
            20,
            10,
        );
        assert_eq!(snap.cursor, Some((4, 12)));
    }

    #[test]
    fn runs_merge_while_the_look_holds() {
        let mut g = Grid::new(6, 1);
        g.write_str(0, 0, "aa", Pen { fg: Color::Indexed(1), ..Pen::default() });
        g.write_str(0, 2, "bb", Pen { fg: Color::Indexed(2), ..Pen::default() });
        let snap = compose(
            &[Layer { grid: &g, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            6,
            1,
        );
        let runs = row_runs(&snap, 0, &theme());
        assert_eq!(runs.len(), 2, "two colours, two runs - the trailing blank is dropped");
        assert_eq!(runs[0].text, "aa");
        assert_eq!(runs[1].text, "bb");
    }

    #[test]
    fn reverse_video_is_resolved_here_not_on_the_phone() {
        let mut g = Grid::new(1, 1);
        g.write_str(0, 0, "x", Pen { flags: Flags::REVERSE, ..Pen::default() });
        let snap = compose(
            &[Layer { grid: &g, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            1,
            1,
        );
        let t = theme();
        let runs = row_runs(&snap, 0, &t);
        assert_eq!(runs[0].fg, (t.background.0, t.background.1, t.background.2));
        assert_eq!(runs[0].bg, (t.foreground.0, t.foreground.1, t.foreground.2));
    }

    #[test]
    fn a_first_diff_sends_every_row() {
        let g = grid_with("hello", 5);
        let snap = compose(
            &[Layer { grid: &g, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            5,
            3,
        );
        assert_eq!(diff(None, &snap, &theme()).len(), 3);
    }

    #[test]
    fn a_diff_sends_only_what_moved() {
        let before = grid_with("aaa", 3);
        let after = grid_with("aba", 3);
        let first = compose(
            &[Layer { grid: &before, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            3,
            2,
        );
        let second = compose(
            &[Layer { grid: &after, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            3,
            2,
        );
        let d = diff(Some(&first), &second, &theme());
        assert_eq!(d.len(), 1, "only row 0 moved");
        assert_eq!(d[0].row, 0);
    }

    #[test]
    fn a_resize_resends_everything() {
        let g = grid_with("ab", 2);
        let small = compose(
            &[Layer { grid: &g, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            2,
            1,
        );
        let big = compose(
            &[Layer { grid: &g, col: 0, row: 0, transparent: false, cursor: None, dim: false }],
            4,
            2,
        );
        assert_eq!(diff(Some(&small), &big, &theme()).len(), 2, "a client cannot patch a reshaped screen");
    }
}
