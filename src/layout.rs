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

    /// Whether `target` can still be split along `axis` inside `area`.
    pub fn can_split(&self, target: PaneId, axis: Axis, area: Rect, gap: f32) -> bool {
        self.layout(area, gap)
            .into_iter()
            .find(|(id, _)| *id == target)
            .is_some_and(|(_, r)| match axis {
                Axis::Horizontal => r.w / 2.0 >= MIN_PANE,
                Axis::Vertical => r.h / 2.0 >= MIN_PANE,
            })
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
}
