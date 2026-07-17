use crate::grid::Grid;

/// A point in absolute grid space: `row` indexes `scrollback ++ screen`.
pub type Point = (usize, usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Char,
    Word,
    Line,
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
        }
        (start, end)
    }

    pub fn contains(&self, grid: &Grid, at: Point) -> bool {
        let (start, end) = self.range(grid);
        at >= start && at <= end
    }

    pub fn is_empty(&self, grid: &Grid) -> bool {
        self.mode == Mode::Char && self.anchor == self.head && {
            let (start, end) = self.range(grid);
            start == end && grid.abs_cell(start.0, start.1).ch == ' '
        }
    }

    pub fn text(&self, grid: &Grid) -> String {
        let (start, end) = self.range(grid);
        grid.text_range(start, end)
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
}
