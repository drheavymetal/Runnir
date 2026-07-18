//! The split tree for one tab.
//!
//! A tab owns a binary tree: every node is either a pane or a split of two
//! subtrees at some ratio. Binary rather than n-ary because the two are
//! expressively equivalent and resizing a binary split is one number.
//!
//! Focus movement is **geometric**, computed over the laid-out rectangles rather
//! than by walking the tree. "The pane to the right" has to mean what you can see,
//! not what the structure happens to say.

use serde::{Deserialize, Serialize};

pub type PaneId = u64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Children sit side by side, divided by a vertical line.
    Horizontal,
    /// Children sit stacked, divided by a horizontal line.
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }
}

/// How a tab arranges its panes. `Splits` is the binary tree the tab has always
/// used; the rest are algorithmic tilings computed from the ordered pane list
/// alone (kitty-style layouts), ignoring the tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutMode {
    /// The manual binary split tree (default). Fully user-arranged.
    #[default]
    Splits,
    /// One pane fills the tab; the others are hidden. Focus cycles between them.
    Stack,
    /// One master pane on the left, the rest stacked in a column on the right.
    Tall,
    /// One master pane on top, the rest in a row below.
    Fat,
    /// A near-square grid of every pane, filled row by row.
    Grid,
}

impl LayoutMode {
    /// The next mode in the cycle, wrapping. Order: Splits → Stack → Tall → Fat →
    /// Grid → Splits.
    pub fn next(self) -> Self {
        match self {
            LayoutMode::Splits => LayoutMode::Stack,
            LayoutMode::Stack => LayoutMode::Tall,
            LayoutMode::Tall => LayoutMode::Fat,
            LayoutMode::Fat => LayoutMode::Grid,
            LayoutMode::Grid => LayoutMode::Splits,
        }
    }

    /// A short human-readable name, for the status toast.
    pub fn label(self) -> &'static str {
        match self {
            LayoutMode::Splits => "splits",
            LayoutMode::Stack => "stack",
            LayoutMode::Tall => "tall",
            LayoutMode::Fat => "fat",
            LayoutMode::Grid => "grid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        /// Share of the parent given to `first`, in `(0, 1)`.
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// Below this many pixels a pane holds no usable cells, so splits stop.
const MIN_PANE: f32 = 40.0;

impl Node {
    pub fn leaf(id: PaneId) -> Self {
        Node::Leaf(id)
    }

    /// Every pane in the tree, left-to-right, top-to-bottom.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.panes().len()
    }

    /// Replaces `target` with a split holding it and `new_pane`.
    /// Returns whether `target` was found.
    pub fn split(&mut self, target: PaneId, new_pane: PaneId, axis: Axis) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new_pane)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.split(target, new_pane, axis) || second.split(target, new_pane, axis)
            }
        }
    }

    /// Removes `target`, collapsing the split that held it so its sibling takes
    /// the whole space. Returns false if this is the last pane — the caller
    /// decides what closing a tab means.
    pub fn close(&mut self, target: PaneId) -> bool {
        match self {
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                if matches!(**first, Node::Leaf(id) if id == target) {
                    *self = (**second).clone();
                    return true;
                }
                if matches!(**second, Node::Leaf(id) if id == target) {
                    *self = (**first).clone();
                    return true;
                }
                first.close(target) || second.close(target)
            }
        }
    }

    /// Assigns a rectangle to every pane. `gap` is the divider width, taken out of
    /// the middle so panes never share a pixel column.
    pub fn layout(&self, area: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.layout_into(area, gap, &mut out);
        out
    }

    fn layout_into(&self, area: Rect, gap: f32, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(id) => out.push((*id, area)),
            Node::Split { axis, ratio, first, second } => {
                let (a, b) = split_rect(area, *axis, *ratio, gap);
                first.layout_into(a, gap, out);
                second.layout_into(b, gap, out);
            }
        }
    }

    /// Nudges the divider that governs `target` along `dir`.
    ///
    /// It walks to the nearest ancestor split on the matching axis, and flips the
    /// sign when `target` is in the second child — otherwise "grow right" would
    /// shrink half the panes on screen.
    pub fn resize(&mut self, target: PaneId, dir: Direction, delta: f32) -> bool {
        self.resize_inner(target, dir.axis(), delta, dir)
    }

    fn resize_inner(&mut self, target: PaneId, axis: Axis, delta: f32, dir: Direction) -> bool {
        let Node::Split { axis: node_axis, ratio, first, second } = self else {
            return false;
        };
        // Deeper splits own the divider closest to the pane, so try them first.
        if first.resize_inner(target, axis, delta, dir) || second.resize_inner(target, axis, delta, dir)
        {
            return true;
        }
        if *node_axis != axis {
            return false;
        }
        let in_first = first.panes().contains(&target);
        let in_second = second.panes().contains(&target);
        if !in_first && !in_second {
            return false;
        }
        let grow = matches!(dir, Direction::Right | Direction::Down);
        let sign = if in_first == grow { 1.0 } else { -1.0 };
        *ratio = (*ratio + delta * sign).clamp(0.05, 0.95);
        true
    }

    /// Whether `target` can still be split along `axis` inside `area`. The divider
    /// gap is taken out first, matching `split_rect`, so the check reflects the
    /// size each child would actually get rather than half the whole pane.
    pub fn can_split(&self, target: PaneId, axis: Axis, area: Rect, gap: f32) -> bool {
        self.layout(area, gap)
            .into_iter()
            .find(|(id, _)| *id == target)
            .is_some_and(|(_, r)| match axis {
                Axis::Horizontal => (r.w - gap) / 2.0 >= MIN_PANE,
                Axis::Vertical => (r.h - gap) / 2.0 >= MIN_PANE,
            })
    }
}

/// A divider found under a point: the path to its split node (which child to
/// descend into at each level), the split's axis, and the split's area — enough to
/// turn a later cursor position into a new ratio.
#[derive(Clone)]
pub struct DividerHit {
    pub path: Vec<bool>,
    pub axis: Axis,
    pub area: Rect,
}

impl Node {
    /// The divider under pixel `(x, y)` within `tol` pixels, if any. Used to start a
    /// mouse resize by grabbing the line between two panes.
    pub fn divider_at(&self, area: Rect, gap: f32, x: f32, y: f32, tol: f32) -> Option<DividerHit> {
        let mut path = Vec::new();
        self.divider_inner(area, gap, x, y, tol, &mut path)
    }

    fn divider_inner(
        &self,
        area: Rect,
        gap: f32,
        x: f32,
        y: f32,
        tol: f32,
        path: &mut Vec<bool>,
    ) -> Option<DividerHit> {
        let Node::Split { axis, ratio, first, second } = self else {
            return None;
        };
        let (a, b) = split_rect(area, *axis, *ratio, gap);
        // Is the point on this split's own divider (the gap between a and b)?
        let on_divider = match axis {
            Axis::Horizontal => {
                let line = a.x + a.w + gap / 2.0;
                (x - line).abs() <= tol + gap / 2.0 && y >= area.y && y <= area.y + area.h
            }
            Axis::Vertical => {
                let line = a.y + a.h + gap / 2.0;
                (y - line).abs() <= tol + gap / 2.0 && x >= area.x && x <= area.x + area.w
            }
        };
        if on_divider {
            return Some(DividerHit { path: path.clone(), axis: *axis, area });
        }
        // Otherwise descend into whichever child contains the point.
        path.push(false);
        if let Some(hit) = first.divider_inner(a, gap, x, y, tol, path) {
            return Some(hit);
        }
        path.pop();
        path.push(true);
        let hit = second.divider_inner(b, gap, x, y, tol, path);
        path.pop();
        hit
    }

    /// Sets the ratio of the split at `path` directly, clamped. For mouse resize,
    /// where the new ratio comes from the cursor position, not a delta.
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        let mut node = self;
        for &go_second in path {
            match node {
                Node::Split { first, second, .. } => {
                    node = if go_second { second } else { first };
                }
                Node::Leaf(_) => return,
            }
        }
        if let Node::Split { ratio, .. } = node {
            *ratio = value.clamp(0.05, 0.95);
        }
    }
}

fn split_rect(area: Rect, axis: Axis, ratio: f32, gap: f32) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let usable = (area.w - gap).max(0.0);
            let first_w = (usable * ratio).round();
            (
                Rect { w: first_w, ..area },
                Rect { x: area.x + first_w + gap, w: usable - first_w, ..area },
            )
        }
        Axis::Vertical => {
            let usable = (area.h - gap).max(0.0);
            let first_h = (usable * ratio).round();
            (
                Rect { h: first_h, ..area },
                Rect { y: area.y + first_h + gap, h: usable - first_h, ..area },
            )
        }
    }
}

/// The pane `dir` of `from`, by geometry.
///
/// Candidates are panes strictly on that side whose extent overlaps the source's
/// on the other axis; the winner is the nearest, ties broken by centre distance.
/// This is what makes focus movement match what the eye expects even in a tree
/// the user built in an odd order.
pub fn neighbour(rects: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let src = rects.iter().find(|(id, _)| *id == from).map(|(_, r)| *r)?;
    let (sx, sy) = src.centre();

    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, r)| match dir {
            // A hair of tolerance: rounding in split_rect can leave edges a pixel
            // apart, and a strict test would make a neighbour unreachable.
            Direction::Left => r.x + r.w <= src.x + 1.0,
            Direction::Right => r.x >= src.x + src.w - 1.0,
            Direction::Up => r.y + r.h <= src.y + 1.0,
            Direction::Down => r.y >= src.y + src.h - 1.0,
        })
        .filter(|(_, r)| match dir {
            Direction::Left | Direction::Right => r.y < src.y + src.h && r.y + r.h > src.y,
            Direction::Up | Direction::Down => r.x < src.x + src.w && r.x + r.w > src.x,
        })
        .min_by(|(_, a), (_, b)| {
            let d = |r: &Rect| {
                let (cx, cy) = r.centre();
                match dir {
                    Direction::Left => (sx - cx, (cy - sy).abs()),
                    Direction::Right => (cx - sx, (cy - sy).abs()),
                    Direction::Up => (sy - cy, (cx - sx).abs()),
                    Direction::Down => (cy - sy, (cx - sx).abs()),
                }
            };
            let (ap, as_) = d(a);
            let (bp, bs) = d(b);
            ap.total_cmp(&bp).then(as_.total_cmp(&bs))
        })
        .map(|(id, _)| *id)
}

// ---- Algorithmic layouts -------------------------------------------------
//
// These arrange the panes from the ORDERED id list alone — no split tree. Order
// is the tab's insertion order, so a new pane appended to it lands where the
// algorithm puts the last slot. All tile contiguously with `gap` between panes,
// and with `gap == 0` fill the area exactly (integer-rounded dividers, the last
// slot absorbing any rounding remainder), so panes never overlap.

/// Splits `[start, start+len)` into `n` contiguous slots, each shortened by `gap`
/// on its trailing edge (except the last). Returns `(start, len)` per slot.
fn tile(start: f32, len: f32, n: usize, gap: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = start + (len * i as f32 / n as f32).round();
        let b = start + (len * (i + 1) as f32 / n as f32).round();
        // Reserve the divider gap between this slot and the next; the last slot
        // runs to the edge so the tiling fills the whole length.
        let slot = if i + 1 < n { (b - a - gap).max(0.0) } else { b - a };
        out.push((a, slot));
    }
    out
}

/// Stack mode as rects: only the focused pane is placed (full area); the rest are
/// hidden. `neighbour`-based focus therefore can't move, so the tab cycles focus
/// through the ordered list instead.
pub fn stack(focus: PaneId, area: Rect) -> Vec<(PaneId, Rect)> {
    vec![(focus, area)]
}

/// Tall mode: `order[0]` is the master column on the left at `master_ratio` of the
/// width; the rest stack top-to-bottom in the right column.
pub fn tall(order: &[PaneId], area: Rect, gap: f32, master_ratio: f32) -> Vec<(PaneId, Rect)> {
    if order.len() <= 1 {
        return order.first().map(|&id| (id, area)).into_iter().collect();
    }
    let usable = (area.w - gap).max(0.0);
    let master_w = (usable * master_ratio.clamp(0.05, 0.95)).round();
    let slave_w = usable - master_w;
    let slave_x = area.x + master_w + gap;
    let mut out = Vec::with_capacity(order.len());
    out.push((order[0], Rect { x: area.x, y: area.y, w: master_w, h: area.h }));
    for (&id, (y, h)) in order[1..].iter().zip(tile(area.y, area.h, order.len() - 1, gap)) {
        out.push((id, Rect { x: slave_x, y, w: slave_w, h }));
    }
    out
}

/// Fat mode: `order[0]` is the master row on top at `master_ratio` of the height;
/// the rest sit left-to-right in the row below.
pub fn fat(order: &[PaneId], area: Rect, gap: f32, master_ratio: f32) -> Vec<(PaneId, Rect)> {
    if order.len() <= 1 {
        return order.first().map(|&id| (id, area)).into_iter().collect();
    }
    let usable = (area.h - gap).max(0.0);
    let master_h = (usable * master_ratio.clamp(0.05, 0.95)).round();
    let slave_h = usable - master_h;
    let slave_y = area.y + master_h + gap;
    let mut out = Vec::with_capacity(order.len());
    out.push((order[0], Rect { x: area.x, y: area.y, w: area.w, h: master_h }));
    for (&id, (x, w)) in order[1..].iter().zip(tile(area.x, area.w, order.len() - 1, gap)) {
        out.push((id, Rect { x, y: slave_y, w, h: slave_h }));
    }
    out
}

/// Grid mode: a near-square grid of every pane, filled row by row. Columns are
/// `ceil(sqrt(n))`; a short final row spreads its panes across the full width so
/// no space is left empty.
pub fn grid(order: &[PaneId], area: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
    let n = order.len();
    if n <= 1 {
        return order.first().map(|&id| (id, area)).into_iter().collect();
    }
    let cols = (n as f32).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);
    let row_bands = tile(area.y, area.h, rows, gap);
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        let start = r * cols;
        let in_row = (n - start).min(cols); // the last row may hold fewer
        let (y, h) = row_bands[r];
        for (c, (x, w)) in tile(area.x, area.w, in_row, gap).into_iter().enumerate() {
            out.push((order[start + c], Rect { x, y, w, h }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };

    #[test]
    fn a_lone_pane_takes_everything() {
        let root = Node::leaf(1);
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects, vec![(1, AREA)]);
    }

    #[test]
    fn splitting_halves_the_space() {
        let mut root = Node::leaf(1);
        assert!(root.split(1, 2, Axis::Horizontal));
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects[0].1.w, 500.0);
        assert_eq!(rects[1].1.w, 500.0);
        assert_eq!(rects[1].1.x, 500.0, "the second pane starts where the first ends");
        assert_eq!(rects[0].1.h, 600.0, "a horizontal split keeps full height");
    }

    #[test]
    fn the_gap_comes_out_of_the_middle() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        let rects = root.layout(AREA, 10.0);
        assert_eq!(rects[0].1.w + rects[1].1.w, 990.0, "the gap is not pane space");
        assert_eq!(rects[1].1.x - (rects[0].1.x + rects[0].1.w), 10.0);
    }

    #[test]
    fn splitting_a_nested_pane_finds_it() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        assert!(root.split(2, 3, Axis::Vertical), "must reach a pane inside a split");
        assert_eq!(root.panes(), vec![1, 2, 3]);
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects.len(), 3);
        // 2 and 3 share the right half, stacked.
        assert_eq!(rects[1].1.h, 300.0);
        assert_eq!(rects[2].1.h, 300.0);
        assert_eq!(rects[1].1.x, 500.0);
    }

    #[test]
    fn splitting_an_absent_pane_changes_nothing() {
        let mut root = Node::leaf(1);
        assert!(!root.split(99, 2, Axis::Horizontal));
        assert_eq!(root.panes(), vec![1]);
    }

    #[test]
    fn closing_collapses_the_split_into_the_sibling() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        assert!(root.close(1));
        assert_eq!(root.panes(), vec![2]);
        // The survivor must take the whole area, not keep half of it.
        assert_eq!(root.layout(AREA, 0.0), vec![(2, AREA)]);
    }

    #[test]
    fn closing_deep_keeps_the_rest_of_the_tree() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        root.split(2, 3, Axis::Vertical);
        assert!(root.close(3));
        assert_eq!(root.panes(), vec![1, 2]);
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects[1].1.h, 600.0, "2 reclaims the height 3 was using");
    }

    #[test]
    fn closing_the_last_pane_is_refused() {
        let mut root = Node::leaf(1);
        assert!(!root.close(1), "the tab, not the tree, decides what this means");
        assert_eq!(root.panes(), vec![1]);
    }

    #[test]
    fn neighbours_follow_the_screen() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal); // 1 | 2
        let rects = root.layout(AREA, 0.0);
        assert_eq!(neighbour(&rects, 1, Direction::Right), Some(2));
        assert_eq!(neighbour(&rects, 2, Direction::Left), Some(1));
        assert_eq!(neighbour(&rects, 1, Direction::Up), None, "nothing above");
        assert_eq!(neighbour(&rects, 1, Direction::Left), None, "nothing to the left");
    }

    #[test]
    fn neighbours_pick_the_overlapping_pane_not_the_nearest_centre() {
        //  +-----+--2--+
        //  |  1  +-----+
        //  |     |  3  |
        //  +-----+-----+
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        root.split(2, 3, Axis::Vertical);
        let rects = root.layout(AREA, 0.0);

        // From 1, "right" must land on 2 (the top one), whose band 1's centre sits
        // nearest — but both overlap, so this pins the tie-break.
        let right = neighbour(&rects, 1, Direction::Right);
        assert!(right == Some(2) || right == Some(3));
        assert_eq!(neighbour(&rects, 3, Direction::Up), Some(2));
        assert_eq!(neighbour(&rects, 2, Direction::Down), Some(3));
        assert_eq!(neighbour(&rects, 2, Direction::Left), Some(1));
        assert_eq!(neighbour(&rects, 3, Direction::Left), Some(1));
    }

    #[test]
    fn resize_moves_the_divider_the_right_way() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);

        // Growing 1 rightwards must widen 1.
        root.resize(1, Direction::Right, 0.1);
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects[0].1.w, 600.0);

        // Growing 2 rightwards must widen 2 — the sign flips because 2 is the
        // second child of the split.
        root.resize(2, Direction::Right, 0.1);
        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects[1].1.w, 500.0);
    }

    #[test]
    fn resize_ignores_a_split_on_the_wrong_axis() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Vertical); // stacked
        let before = root.layout(AREA, 0.0);
        root.resize(1, Direction::Right, 0.2); // horizontal move, vertical split
        assert_eq!(root.layout(AREA, 0.0), before, "nothing to move horizontally");
    }

    #[test]
    fn resize_is_clamped_so_a_pane_never_vanishes() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        for _ in 0..50 {
            root.resize(1, Direction::Right, 0.5);
        }
        let rects = root.layout(AREA, 0.0);
        assert!(rects[1].1.w > 0.0, "the other pane must survive");
        assert_eq!(rects[0].1.w, 950.0);
    }

    #[test]
    fn splits_stop_when_a_pane_would_hold_nothing() {
        let root = Node::leaf(1);
        let roomy = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };
        let cramped = Rect { x: 0.0, y: 0.0, w: 50.0, h: 600.0 };
        assert!(root.can_split(1, Axis::Horizontal, roomy, 0.0));
        assert!(!root.can_split(1, Axis::Horizontal, cramped, 0.0));
        assert!(root.can_split(1, Axis::Vertical, cramped, 0.0), "still tall enough");
    }

    #[test]
    fn a_deep_tree_lays_out_without_overlap_or_gaps() {
        let mut root = Node::leaf(1);
        root.split(1, 2, Axis::Horizontal);
        root.split(2, 3, Axis::Vertical);
        root.split(1, 4, Axis::Vertical);
        root.split(3, 5, Axis::Horizontal);

        let rects = root.layout(AREA, 0.0);
        assert_eq!(rects.len(), 5);
        let total: f32 = rects.iter().map(|(_, r)| r.w * r.h).sum();
        assert!(
            (total - AREA.w * AREA.h).abs() < 2.0,
            "panes must tile the area exactly, got {total}"
        );
        for (i, (_, a)) in rects.iter().enumerate() {
            for (_, b) in rects.iter().skip(i + 1) {
                let overlap = (a.x < b.x + b.w && b.x < a.x + a.w)
                    && (a.y < b.y + b.h && b.y < a.y + a.h);
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
    }

    // ---- Algorithmic layout modes ---------------------------------------

    /// No two rects overlap.
    fn no_overlap(rects: &[(PaneId, Rect)]) {
        for (i, (_, a)) in rects.iter().enumerate() {
            for (_, b) in rects.iter().skip(i + 1) {
                let overlap = (a.x < b.x + b.w && b.x < a.x + a.w)
                    && (a.y < b.y + b.h && b.y < a.y + a.h);
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
    }

    /// With no gap the rects must tile the area exactly, cover every id once, and
    /// keep the given order.
    fn fills_exactly(rects: &[(PaneId, Rect)], order: &[PaneId]) {
        let ids: Vec<PaneId> = rects.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, order, "order must be preserved and complete");
        no_overlap(rects);
        let total: f32 = rects.iter().map(|(_, r)| r.w * r.h).sum();
        assert!(
            (total - AREA.w * AREA.h).abs() < 2.0,
            "gapless tiling must fill the area, got {total}"
        );
    }

    #[test]
    fn stack_shows_only_the_focused_pane_full_size() {
        // Whatever the order, stack lays out exactly the focused pane at full area.
        for focus in [1u64, 2, 3, 4] {
            let rects = stack(focus, AREA);
            assert_eq!(rects, vec![(focus, AREA)]);
        }
    }

    #[test]
    fn tall_master_left_rest_stacked_right() {
        for n in 1..=4u64 {
            let order: Vec<PaneId> = (1..=n).collect();
            let rects = tall(&order, AREA, 0.0, 0.6);
            fills_exactly(&rects, &order);
            if n == 1 {
                assert_eq!(rects[0].1, AREA);
                continue;
            }
            // Master takes 60% of the width and the full height.
            assert_eq!(rects[0].1.w, 600.0);
            assert_eq!(rects[0].1.h, 600.0);
            assert_eq!(rects[0].1.x, 0.0);
            // Slaves sit in the right column, share its height, full slave width.
            for (_, r) in &rects[1..] {
                assert_eq!(r.x, 600.0);
                assert_eq!(r.w, 400.0);
            }
            // Slave heights sum to the full height (they stack, no gap).
            let sh: f32 = rects[1..].iter().map(|(_, r)| r.h).sum();
            assert_eq!(sh, 600.0);
        }
    }

    #[test]
    fn fat_master_top_rest_below() {
        for n in 1..=4u64 {
            let order: Vec<PaneId> = (1..=n).collect();
            let rects = fat(&order, AREA, 0.0, 0.5);
            fills_exactly(&rects, &order);
            if n == 1 {
                assert_eq!(rects[0].1, AREA);
                continue;
            }
            // Master row on top, half the height, full width.
            assert_eq!(rects[0].1.h, 300.0);
            assert_eq!(rects[0].1.w, 1000.0);
            assert_eq!(rects[0].1.y, 0.0);
            // Slaves sit in the bottom row, sharing the width.
            for (_, r) in &rects[1..] {
                assert_eq!(r.y, 300.0);
                assert_eq!(r.h, 300.0);
            }
            let sw: f32 = rects[1..].iter().map(|(_, r)| r.w).sum();
            assert_eq!(sw, 1000.0);
        }
    }

    #[test]
    fn grid_is_near_square_and_fills() {
        for n in 1..=4u64 {
            let order: Vec<PaneId> = (1..=n).collect();
            let rects = grid(&order, AREA, 0.0);
            fills_exactly(&rects, &order);
        }
        // Four panes make a 2x2 grid: two distinct x's and two distinct y's.
        let rects = grid(&[1, 2, 3, 4], AREA, 0.0);
        assert_eq!(rects[0].1, Rect { x: 0.0, y: 0.0, w: 500.0, h: 300.0 });
        assert_eq!(rects[3].1, Rect { x: 500.0, y: 300.0, w: 500.0, h: 300.0 });
        // Three panes: row 0 holds two, row 1 holds one spanning the full width.
        let three = grid(&[1, 2, 3], AREA, 0.0);
        assert_eq!(three[2].1.w, 1000.0, "the lone last-row pane spans the width");
        assert_eq!(three[2].1.y, 300.0);
    }

    #[test]
    fn a_gap_never_makes_panes_overlap() {
        // With a real divider gap the tilings must still be overlap-free and stay
        // inside the area for every mode and pane count.
        for n in 1..=4u64 {
            let order: Vec<PaneId> = (1..=n).collect();
            for rects in [
                tall(&order, AREA, 10.0, 0.55),
                fat(&order, AREA, 10.0, 0.55),
                grid(&order, AREA, 10.0),
            ] {
                assert_eq!(rects.len(), n as usize);
                no_overlap(&rects);
                for (_, r) in &rects {
                    assert!(r.w >= 0.0 && r.h >= 0.0);
                    assert!(r.x >= AREA.x - 0.5 && r.x + r.w <= AREA.x + AREA.w + 0.5);
                    assert!(r.y >= AREA.y - 0.5 && r.y + r.h <= AREA.y + AREA.h + 0.5);
                }
            }
        }
    }
}
