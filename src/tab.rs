//! A tab: one split tree, the panes it holds, and which pane has focus.
//!
//! The tab owns the panes (keyed by id) and the `layout::Node` that arranges them.
//! It knows nothing about the GPU — it hands out `(PaneId, Rect)` and lets the
//! caller draw. Pixel geometry is recomputed on demand from the current area, so
//! there is no cached layout to invalidate.

use std::collections::HashMap;

use crate::config::Config;
use crate::layout::{Axis, Direction, LayoutMode, Node, PaneId, Rect, neighbour};
use crate::pane::Pane;
use crate::pty::Spawn;

pub struct Tab {
    pub tree: Node,
    pub panes: HashMap<PaneId, Pane>,
    pub focus: PaneId,
    pub title_override: Option<String>,
    /// How the panes are arranged. `Splits` (the tree) is the default; the other
    /// modes tile `order` algorithmically. The tree is kept up to date in every
    /// mode, so switching back to `Splits` restores the manual arrangement.
    pub mode: LayoutMode,
    /// Pane insertion order, the source of arrangement for the algorithmic modes.
    /// Always the same set of ids as `panes`.
    order: Vec<PaneId>,
    /// Master pane's share of the tab in Tall/Fat, in `(0, 1)`.
    master_ratio: f32,
    /// Cell size in pixels, needed to translate a pane's pixel rect into a cell
    /// grid when it is created or resized.
    cell: (f32, f32),
    gap: f32,
    padding: f32,
}

/// Default master share for Tall/Fat: a touch over half, so the master reads as
/// the primary pane.
const DEFAULT_MASTER: f32 = 0.6;

impl Tab {
    /// Creates a tab with one pane filling `area`.
    pub fn new(
        area: Rect,
        cell: (f32, f32),
        config: &Config,
        first_id: PaneId,
        spawn: &Spawn,
        wake: impl Fn() + Send + Clone + 'static,
    ) -> anyhow::Result<Self> {
        let padding = config.window.padding;
        let inner = pad(area, padding);
        let (cols, rows) = cells_in(inner, cell);
        let pane = Pane::new(cols, rows, config.scrollback.lines, cell, spawn, wake)?;

        let mut panes = HashMap::new();
        panes.insert(first_id, pane);
        Ok(Self {
            tree: Node::leaf(first_id),
            panes,
            focus: first_id,
            title_override: None,
            mode: LayoutMode::default(),
            order: vec![first_id],
            master_ratio: DEFAULT_MASTER,
            cell,
            gap: config.window.padding.max(6.0),
            padding,
        })
    }

    pub fn focused(&mut self) -> &mut Pane {
        self.panes.get_mut(&self.focus).expect("focus always names a live pane")
    }

    pub fn focused_ref(&self) -> &Pane {
        self.panes.get(&self.focus).expect("focus always names a live pane")
    }

    pub fn focused_ptr(&self) -> PaneId {
        self.focus
    }

    /// Pane rectangles for the current window `area`, in pixels. The active `mode`
    /// decides the arrangement: `Splits` uses the tree, the rest tile `order`.
    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let inner = pad(area, self.padding);
        match self.mode {
            LayoutMode::Splits => self.tree.layout(inner, self.gap),
            // Stack shows only the focused pane; the others are hidden, so the whole
            // tab is theirs to fill.
            LayoutMode::Stack => crate::layout::stack(self.focus, inner),
            LayoutMode::Tall => crate::layout::tall(&self.order, inner, self.gap, self.master_ratio),
            LayoutMode::Fat => crate::layout::fat(&self.order, inner, self.gap, self.master_ratio),
            LayoutMode::Grid => crate::layout::grid(&self.order, inner, self.gap),
        }
    }

    /// Cycles to the next layout mode and returns it, reflowing so every pane's PTY
    /// learns the size the new arrangement gives it.
    pub fn cycle_layout(&mut self, area: Rect) -> LayoutMode {
        self.mode = self.mode.next();
        self.reflow(area);
        self.mode
    }

    /// The whole padded area, for a zoomed (maximized) pane.
    pub fn full_rect(&self, area: Rect) -> Rect {
        pad(area, self.padding)
    }

    /// Resizes one pane's grid/PTY to `rect` (used when zooming a single pane).
    pub fn resize_one(&mut self, id: PaneId, rect: Rect) {
        let (cols, rows) = cells_in(rect, self.cell);
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.resize(cols, rows);
        }
    }

    /// Splits the focused pane along `axis`, giving the new pane focus. Inherits
    /// the focused pane's working directory so a split lands where you were.
    pub fn split_with_id(
        &mut self,
        area: Rect,
        axis: Axis,
        config: &Config,
        id: PaneId,
        wake: impl Fn() + Send + Clone + 'static,
    ) -> anyhow::Result<()> {
        self.split_running_with_id(area, axis, config, id, Vec::new(), wake)
    }

    /// Like `split_with_id`, but runs `command` in the new pane instead of a shell.
    /// An empty command means a shell.
    pub fn split_running_with_id(
        &mut self,
        area: Rect,
        axis: Axis,
        config: &Config,
        id: PaneId,
        command: Vec<String>,
        wake: impl Fn() + Send + Clone + 'static,
    ) -> anyhow::Result<()> {
        let inner = pad(area, self.padding);
        // The tree's minimum-size gate only governs the manual `Splits` mode. The
        // algorithmic modes redistribute space among all panes, so a new pane always
        // fits — gating them on tree geometry (which isn't even on screen) would
        // wrongly refuse.
        if self.mode == LayoutMode::Splits
            && !self.tree.can_split(self.focus, axis, inner, self.gap)
        {
            return Ok(()); // Too small to divide usefully; ignore rather than error.
        }

        let spawn = Spawn {
            command: (!command.is_empty()).then_some(command),
            cwd: self.focused_cwd(),
        };
        // The tree is kept current in every mode so switching to `Splits` restores a
        // real arrangement. Size the new pane to what the *active* mode will give it.
        let mut tree = self.tree.clone();
        tree.split(self.focus, id, axis);
        self.tree = tree;
        self.order.push(id);
        let rect = self
            .provisional_layout(area, id)
            .unwrap_or(inner);
        let (cols, rows) = cells_in(rect, self.cell);

        let pane = match Pane::new(cols, rows, config.scrollback.lines, self.cell, &spawn, wake) {
            Ok(p) => p,
            Err(e) => {
                // Roll back the structural change so a failed spawn leaves the tab
                // exactly as it was.
                self.tree.close(id);
                self.order.retain(|&p| p != id);
                return Err(e);
            }
        };
        self.panes.insert(id, pane);
        self.focus = id;
        self.reflow(area);
        Ok(())
    }

    /// The rect the active mode would give pane `id`, computed once `id` is already
    /// in `tree`/`order`. Used to size a freshly-created pane before its PTY exists.
    fn provisional_layout(&self, area: Rect, id: PaneId) -> Option<Rect> {
        self.layout(area).into_iter().find(|(pid, _)| *pid == id).map(|(_, r)| r)
    }

    /// Closes the focused pane. Returns false when it was the last one — the caller
    /// decides whether that closes the tab.
    pub fn close_focused(&mut self, area: Rect) -> bool {
        if self.order.len() <= 1 {
            return false;
        }
        // Pick the pane that will take focus *before* mutating anything. Geometry
        // first (matches what the eye expects); in Stack, where only one rect is
        // laid out, geometry finds nothing so the ordered neighbour is used.
        let rects = self.layout(area);
        let next = neighbour(&rects, self.focus, Direction::Left)
            .or_else(|| neighbour(&rects, self.focus, Direction::Right))
            .or_else(|| neighbour(&rects, self.focus, Direction::Up))
            .or_else(|| neighbour(&rects, self.focus, Direction::Down))
            .or_else(|| self.order_neighbour(1));

        let closed = self.focus;
        self.tree.close(closed);
        self.order.retain(|&p| p != closed);
        self.panes.remove(&closed);
        self.focus = next.filter(|n| *n != closed).unwrap_or_else(|| self.order[0]);
        self.reflow(area);
        true
    }

    /// The pane `step` places away from the focused one in insertion order,
    /// wrapping. Used to cycle focus in Stack and as a close fallback.
    fn order_neighbour(&self, step: isize) -> Option<PaneId> {
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        let i = self.order.iter().position(|&p| p == self.focus)?;
        let j = (i as isize + step).rem_euclid(n as isize) as usize;
        Some(self.order[j])
    }

    /// Moves focus to the pane in `dir`, if any. Returns whether focus moved.
    ///
    /// The tiled modes (Splits/Tall/Fat/Grid) lay out non-overlapping rects, so
    /// geometric neighbouring works for all of them. Stack shows one pane at a time,
    /// so a horizontal/vertical press instead cycles the ordered list (prev on
    /// Left/Up, next on Right/Down) and reflows the newly shown pane to full size.
    pub fn focus_dir(&mut self, area: Rect, dir: Direction) -> bool {
        if self.mode == LayoutMode::Stack {
            let step = match dir {
                Direction::Left | Direction::Up => -1,
                Direction::Right | Direction::Down => 1,
            };
            if let Some(id) = self.order_neighbour(step) {
                if id != self.focus {
                    self.focus = id;
                    self.reflow(area);
                    return true;
                }
            }
            return false;
        }
        let rects = self.layout(area);
        if let Some(id) = neighbour(&rects, self.focus, dir) {
            self.focus = id;
            true
        } else {
            false
        }
    }

    /// Cycles focus to the next pane in insertion order. The primary way to move in
    /// Stack; a keyboard fallback elsewhere when directional movement is ambiguous.
    pub fn focus_next(&mut self, area: Rect) {
        if let Some(id) = self.order_neighbour(1) {
            self.focus = id;
            if self.mode == LayoutMode::Stack {
                self.reflow(area);
            }
        }
    }

    /// Resizes the focused pane. In `Splits` this nudges the tree divider; in
    /// Tall/Fat it grows or shrinks the master (Right/Down grow it, Left/Up shrink);
    /// Stack and Grid have no adjustable size, so it is a no-op.
    pub fn resize_focused(&mut self, area: Rect, dir: Direction) {
        match self.mode {
            LayoutMode::Splits => {
                self.tree.resize(self.focus, dir, 0.03);
            }
            LayoutMode::Tall | LayoutMode::Fat => {
                let sign = match dir {
                    Direction::Right | Direction::Down => 1.0,
                    Direction::Left | Direction::Up => -1.0,
                };
                self.master_ratio = (self.master_ratio + sign * 0.03).clamp(0.05, 0.95);
            }
            LayoutMode::Stack | LayoutMode::Grid => {}
        }
        self.reflow(area);
    }

    /// The divider under a pixel point, for starting a mouse resize.
    pub fn divider_at(&self, area: Rect, x: f32, y: f32) -> Option<crate::layout::DividerHit> {
        // A grab tolerance a little wider than the visible line, so it is easy to hit.
        self.tree.divider_at(pad(area, self.padding), self.gap, x, y, 5.0)
    }

    /// Drags the divider identified by `hit` to the cursor, updating the split
    /// ratio and reflowing so both children's PTYs learn their new size.
    pub fn drag_divider(&mut self, area: Rect, hit: &crate::layout::DividerHit, x: f32, y: f32) {
        let a = hit.area;
        let ratio = match hit.axis {
            Axis::Horizontal => (x - a.x) / a.w,
            Axis::Vertical => (y - a.y) / a.h,
        };
        self.tree.set_ratio(&hit.path, ratio);
        self.reflow(area);
    }

    /// Reapplies the layout to every pane after the tree or the window changed, so
    /// each child PTY learns its true size.
    pub fn reflow(&mut self, area: Rect) {
        for (id, rect) in self.layout(area) {
            let (cols, rows) = cells_in(rect, self.cell);
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.resize(cols, rows);
            }
        }
    }

    /// Removes panes whose process has exited. Returns false when that empties the
    /// tab. Focus follows a survivor.
    pub fn reap_dead(&mut self, area: Rect) -> bool {
        let dead: Vec<PaneId> =
            self.tree.panes().into_iter().filter(|id| !self.panes[id].alive()).collect();
        if dead.is_empty() {
            return true;
        }
        for id in dead {
            if self.order.len() <= 1 {
                return false; // The last pane died: the tab is done.
            }
            let rects = self.layout(area);
            let next = neighbour(&rects, id, Direction::Left)
                .or_else(|| neighbour(&rects, id, Direction::Right))
                .or_else(|| neighbour(&rects, id, Direction::Up))
                .or_else(|| neighbour(&rects, id, Direction::Down))
                .or_else(|| self.order_neighbour(1));
            self.tree.close(id);
            self.order.retain(|&p| p != id);
            self.panes.remove(&id);
            if self.focus == id {
                self.focus = next.filter(|n| *n != id).unwrap_or_else(|| self.order[0]);
            }
        }
        self.reflow(area);
        true
    }

    pub fn title(&self) -> String {
        self.title_override.clone().unwrap_or_else(|| self.focused_ref().title.clone())
    }

    /// The focused pane's process name, for choosing a tab icon.
    pub fn proc_name(&self) -> String {
        self.focused_ref().title.clone()
    }

    /// Any pane changed since its last render — a background tab's grid stays dirty
    /// until shown, so this is "has unseen output" for the activity badge.
    pub fn has_activity(&self) -> bool {
        self.panes.values().any(|p| p.is_dirty())
    }

    /// Whether any pane's most recent command failed (non-zero exit), for the fail
    /// badge.
    pub fn failed(&self) -> bool {
        self.panes.values().any(|p| matches!(p.last_exit(), Some(c) if c != 0))
    }

    /// Captures this tab's layout and scrollback for the session file.
    pub fn to_session(&self) -> crate::session::TabState {
        let mut panes = std::collections::HashMap::new();
        for (id, pane) in &self.panes {
            panes.insert(
                *id,
                crate::session::PaneState {
                    cwd: pane.cwd(),
                    title: pane.title_override.clone(),
                    scrollback: pane.scrollback_text(),
                },
            );
        }
        crate::session::TabState {
            tree: self.tree.clone(),
            focus: self.focus,
            title: self.title_override.clone(),
            panes,
            mode: self.mode,
            order: self.order.clone(),
            master_ratio: Some(self.master_ratio),
        }
    }

    /// Rebuilds a tab from a saved state: relaunches a shell per pane in its saved
    /// cwd and loads the saved scrollback above it. Panes named in the tree but
    /// missing from `panes` fall back to a default shell.
    pub fn from_session(
        state: &crate::session::TabState,
        area: Rect,
        cell: (f32, f32),
        config: &Config,
        mut wake: impl FnMut(PaneId) -> Box<dyn Fn() + Send + 'static>,
    ) -> anyhow::Result<Self> {
        let padding = config.window.padding;
        let gap = padding.max(6.0);
        let rects = state.tree.layout(pad(area, padding), gap);

        let mut panes = HashMap::new();
        for (id, rect) in &rects {
            let (cols, rows) = cells_in(*rect, cell);
            let saved = state.panes.get(id);
            let spawn = Spawn {
                command: None,
                cwd: saved.and_then(|s| s.cwd.clone()),
            };
            // A single pane that cannot spawn (e.g. its saved cwd is gone) must not
            // discard the whole tab and everyone else's restored history. Retry in
            // the home directory; only give up on the pane if that fails too.
            let mut pane = match Pane::new(cols, rows, config.scrollback.lines, cell, &spawn, wake(*id)) {
                Ok(p) => p,
                Err(_) => {
                    let fallback = Spawn { command: None, cwd: dirs::home_dir() };
                    Pane::new(cols, rows, config.scrollback.lines, cell, &fallback, wake(*id))?
                }
            };
            if let Some(s) = saved {
                pane.title_override = s.title.clone();
                pane.grid.lock().unwrap().preload_scrollback(&s.scrollback);
            }
            panes.insert(*id, pane);
        }

        // The saved focus may name a pane that failed to spawn; fall back.
        let focus = if panes.contains_key(&state.focus) {
            state.focus
        } else {
            *panes.keys().next().expect("a tab always has at least one pane")
        };

        // Rebuild the insertion order: use the saved one, but drop any ids that
        // failed to spawn and append any tree pane it is missing (e.g. an older
        // session with no saved order at all), so `order` always matches `panes`.
        let mut order: Vec<PaneId> =
            state.order.iter().copied().filter(|id| panes.contains_key(id)).collect();
        for id in state.tree.panes() {
            if panes.contains_key(&id) && !order.contains(&id) {
                order.push(id);
            }
        }

        Ok(Self {
            tree: state.tree.clone(),
            panes,
            focus,
            title_override: state.title.clone(),
            mode: state.mode,
            order,
            master_ratio: state.master_ratio.unwrap_or(DEFAULT_MASTER),
            cell,
            gap,
            padding,
        })
    }

    fn focused_cwd(&self) -> Option<std::path::PathBuf> {
        // A split opens where the focused shell actually is, not where the window was
        // launched. Goes through Pane::cwd → platform (Linux /proc, macOS libproc).
        self.focused_ref().cwd()
    }
}

fn pad(area: Rect, padding: f32) -> Rect {
    Rect {
        x: area.x + padding,
        y: area.y + padding,
        w: (area.w - 2.0 * padding).max(1.0),
        h: (area.h - 2.0 * padding).max(1.0),
    }
}

fn cells_in(rect: Rect, cell: (f32, f32)) -> (usize, usize) {
    ((rect.w / cell.0).floor().max(1.0) as usize, (rect.h / cell.1).floor().max(1.0) as usize)
}
