use crate::grid::Grid;

/// A point in absolute grid space: `row` indexes `scrollback ++ screen`.
pub type Point = (usize, usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Char,
    Word,
    Line,
    /// Rectangular (block) selection: the cells inside the bounding box
    /// `[min_row..=max_row] x [min_col..=max_col]`, not the linear text flow.
    Block,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection {
    anchor: Point,
    head: Point,
    mode: Mode,
}

impl Selection {
    pub fn new(at: Point, mode: Mode) -> Self {
        Self { anchor: at, head: at, mode }
    }

    pub fn update(&mut self, to: Point) {
        self.head = to;
    }

    /// Normalized (start, end) with the expansion for the current mode applied.
    pub fn range(&self, grid: &Grid) -> (Point, Point) {
        let (mut start, mut end) =
            if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) };

        match self.mode {
            Mode::Char => {}
            Mode::Word => {
                start.1 = word_start(grid, start);
                end.1 = word_end(grid, end);
            }
            Mode::Line => {
                start.1 = 0;
                end.1 = grid.cols() - 1;
            }
            Mode::Block => {
                // The bounding box of anchor/head: columns are independent of rows,
                // so the min/max corner need not sit on the row-normalized endpoints.
                start = (self.anchor.0.min(self.head.0), self.anchor.1.min(self.head.1));
                end = (self.anchor.0.max(self.head.0), self.anchor.1.max(self.head.1));
            }
        }
        (start, end)
    }

    pub fn contains(&self, grid: &Grid, at: Point) -> bool {
        let (start, end) = self.range(grid);
        if self.mode == Mode::Block {
            // Rectangular hit-test: inside the row band AND the column band, rather
            // than the linear `start..=end` flow used by the other modes.
            at.0 >= start.0 && at.0 <= end.0 && at.1 >= start.1 && at.1 <= end.1
        } else {
            at >= start && at <= end
        }
    }

    pub fn is_empty(&self, grid: &Grid) -> bool {
        self.mode == Mode::Char && self.anchor == self.head && {
            let (start, end) = self.range(grid);
            start == end && grid.abs_cell(start.0, start.1).ch == ' '
        }
    }

    pub fn text(&self, grid: &Grid) -> String {
        let (start, end) = self.range(grid);
        if self.mode == Mode::Block {
            // Block copy: each row contributes only its `[min_col..=max_col]` slice,
            // joined by a newline. Spacers (the right half of a wide glyph) carry no
            // char, so they are skipped, and each row's trailing blanks are trimmed —
            // mirroring `Grid::text_range` for the linear modes.
            let last_row = grid.total_rows().saturating_sub(1);
            let last_col = grid.cols().saturating_sub(1);
            // Clamp once and compare against the clamped row below: checking the
            // raw `end.0` would append a newline after the final row whenever the
            // drag ran past the bottom of the grid.
            let end_row = end.0.min(last_row);
            let mut out = String::new();
            for abs in start.0..=end_row {
                let line: String = (start.1..=end.1.min(last_col))
                    .map(|c| grid.abs_cell(abs, c))
                    .filter(|cell| !cell.is_spacer())
                    .map(|cell| cell.ch)
                    .collect();
                out.push_str(line.trim_end());
                if abs != end_row {
                    out.push('\n');
                }
            }
            out
        } else {
            grid.text_range(start, end)
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || "_-./~:@".contains(c)
}

fn word_start(grid: &Grid, at: Point) -> usize {
    let mut col = at.1;
    while col > 0 {
        let prev = grid.abs_cell(at.0, col - 1);
        if prev.is_spacer() {
            // The right half of a wide glyph: step over it iff its leader joins,
            // or a double-click on CJK would stop at the first spacer.
            if col >= 2 && is_word(grid.abs_cell(at.0, col - 2).ch) {
                col -= 2;
            } else {
                break;
            }
        } else if is_word(prev.ch) {
            col -= 1;
        } else {
            break;
        }
    }
    col
}

fn word_end(grid: &Grid, at: Point) -> usize {
    let mut col = at.1;
    while col + 1 < grid.cols() {
        let next = grid.abs_cell(at.0, col + 1);
        if next.is_spacer() {
            // The spacer belongs to the glyph at `col`; swallow it and go on.
            col += 1;
        } else if is_word(next.ch) {
            col += 1;
        } else {
            break;
        }
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with(text: &str) -> Grid {
        let mut g = Grid::new(20, 2);
        vte::Parser::new().advance(&mut g, text.as_bytes());
        g
    }

    #[test]
    fn word_selection_spans_wide_chars() {
        // Regression: the spacer half of a wide glyph is '\0', not a word char,
        // so double-clicking CJK selected a single character.
        let g = grid_with("日本語 abc");
        // Double-click on 本 (leader at col 2).
        let sel = Selection::new((0, 2), Mode::Word);
        assert_eq!(sel.text(&g), "日本語");
        // Clicking the spacer half (col 3) selects the same word.
        let sel = Selection::new((0, 3), Mode::Word);
        assert_eq!(sel.text(&g), "日本語");
        // ASCII words still stop at the boundary.
        let sel = Selection::new((0, 8), Mode::Word);
        assert_eq!(sel.text(&g), "abc");
    }

    fn grid_rows(rows: &[&str]) -> Grid {
        let mut g = Grid::new(20, rows.len().max(1));
        let mut p = vte::Parser::new();
        for (i, line) in rows.iter().enumerate() {
            if i > 0 {
                p.advance(&mut g, b"\r\n");
            }
            p.advance(&mut g, line.as_bytes());
        }
        g
    }

    #[test]
    fn block_contains_is_rectangular() {
        // A 3x3 block anchored at (0,1), head at (2,3): the rectangle
        // rows 0..=2 x cols 1..=3.
        let g = grid_rows(&["abcdef", "ghijkl", "mnopqr"]);
        let sel = Selection::new((0, 1), Mode::Block);
        let mut sel = sel;
        sel.update((2, 3));

        // Inside the box on every row (not just the linear start row).
        for row in 0..=2 {
            for col in 1..=3 {
                assert!(sel.contains(&g, (row, col)), "({row},{col}) should be in the block");
            }
        }
        // Columns left/right of the band are excluded even on interior rows — the
        // key difference from linear Char selection, which would sweep the tail of
        // row 0, all of row 1, and the head of row 2.
        assert!(!sel.contains(&g, (1, 0)));
        assert!(!sel.contains(&g, (1, 4)));
        assert!(!sel.contains(&g, (0, 0)));
        assert!(!sel.contains(&g, (2, 4)));
        // Rows outside the band are excluded.
        assert!(!sel.contains(&g, (3, 2)));
    }

    #[test]
    fn block_text_slices_columns_per_row() {
        let g = grid_rows(&["abcdef", "ghijkl", "mnopqr"]);
        let mut sel = Selection::new((0, 1), Mode::Block);
        sel.update((2, 3));
        // Each row yields its own [1..=3] slice, joined by newlines.
        assert_eq!(sel.text(&g), "bcd\nhij\nnop");
    }

    #[test]
    fn block_normalizes_anchor_below_right_of_head() {
        // Anchor at the bottom-right corner, head at the top-left: range() must
        // still produce the same bounding box and the same block text.
        let g = grid_rows(&["abcdef", "ghijkl", "mnopqr"]);
        let mut sel = Selection::new((2, 3), Mode::Block);
        sel.update((0, 1));
        let (start, end) = sel.range(&g);
        assert_eq!(start, (0, 1));
        assert_eq!(end, (2, 3));
        assert_eq!(sel.text(&g), "bcd\nhij\nnop");
        // Mixed diagonal: anchor bottom-left, head top-right still normalizes.
        let mut sel = Selection::new((2, 1), Mode::Block);
        sel.update((0, 3));
        assert_eq!(sel.range(&g), ((0, 1), (2, 3)));
        assert_eq!(sel.text(&g), "bcd\nhij\nnop");
    }

    #[test]
    fn block_overshooting_the_grid_clamps_without_a_trailing_newline() {
        // Dragging past the bottom-right corner of the grid: the box is clamped
        // to the real rows/cols, and the last real row must not be followed by a
        // spurious newline (the loop clamps but the join must clamp identically).
        let g = grid_rows(&["abcdef", "ghijkl"]);
        let mut sel = Selection::new((0, 0), Mode::Block);
        sel.update((5, 30)); // far below and right of the 2x20 grid
        assert_eq!(sel.text(&g), "abcdef\nghijkl");
        // A block entirely below the grid selects nothing.
        let mut sel = Selection::new((4, 0), Mode::Block);
        sel.update((5, 3));
        assert_eq!(sel.text(&g), "");
    }

    #[test]
    fn single_cell_block_selects_one_char() {
        let g = grid_rows(&["abcdef"]);
        let sel = Selection::new((0, 2), Mode::Block);
        assert_eq!(sel.text(&g), "c");
        assert!(sel.contains(&g, (0, 2)));
        assert!(!sel.contains(&g, (0, 1)));
        assert!(!sel.contains(&g, (0, 3)));
    }

    #[test]
    fn block_over_wide_chars_skips_spacers() {
        // A block spanning a CJK glyph must not emit the spacer half's '\0'.
        // "日本語" occupies cols 0..=5 (each glyph = leader + spacer).
        let g = grid_rows(&["日本語x", "ABCDEFG"]);
        // Cols 0..=3 cover 日(leader 0, spacer 1) 本(leader 2, spacer 3) on the CJK
        // row, and the first four ASCII cells on the other — spacers drop out.
        let mut sel = Selection::new((0, 0), Mode::Block);
        sel.update((1, 3));
        assert_eq!(sel.text(&g), "日本\nABCD");
    }
}
