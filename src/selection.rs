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
    while col > 0 && is_word(grid.abs_cell(at.0, col - 1).ch) {
        col -= 1;
    }
    col
}

fn word_end(grid: &Grid, at: Point) -> usize {
    let mut col = at.1;
    while col + 1 < grid.cols() && is_word(grid.abs_cell(at.0, col + 1).ch) {
        col += 1;
    }
    col
}
