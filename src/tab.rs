//! A tab: one split tree, the panes it holds, and which pane has focus.
//!
//! The tab owns the panes (keyed by id) and the `layout::Node` that arranges them.
//! It knows nothing about the GPU — it hands out `(PaneId, Rect)` and lets the
//! caller draw. Pixel geometry is recomputed on demand from the current area, so
//! there is no cached layout to invalidate.

use std::collections::HashMap;

use crate::config::Config;
use crate::layout::{Axis, Direction, Node, PaneId, Rect, neighbour};
use crate::pane::Pane;
use crate::pty::Spawn;

pub struct Tab {
    pub tree: Node,
    pub panes: HashMap<PaneId, Pane>,
    pub focus: PaneId,
    pub title_override: Option<String>,
    /// Cell size in pixels, needed to translate a pane's pixel rect into a cell
    /// grid when it is created or resized.
    cell: (f32, f32),
    gap: f32,
    padding: f32,
}

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
        let pane = Pane::new(cols, rows, config.scrollback.lines, spawn, wake)?;

        let mut panes = HashMap::new();
        panes.insert(first_id, pane);
        Ok(Self {
            tree: Node::leaf(first_id),
            panes,
            focus: first_id,
            title_override: None,
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

    /// Pane rectangles for the current window `area`, in pixels.
    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        self.tree.layout(pad(area, self.padding), self.gap)
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
        if !self.tree.can_split(self.focus, axis, inner, self.gap) {
            return Ok(()); // Too small to divide usefully; ignore rather than error.
        }

        let spawn = Spawn {
            command: (!command.is_empty()).then_some(command),
            cwd: self.focused_cwd(),
        };
        // Size it to what it will actually get once the tree includes it.
        let mut tree = self.tree.clone();
        tree.split(self.focus, id, axis);
        let rect = tree
            .layout(inner, self.gap)
            .into_iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, r)| r)
            .unwrap_or(inner);
        let (cols, rows) = cells_in(rect, self.cell);

        let pane = Pane::new(cols, rows, config.scrollback.lines, &spawn, wake)?;
        self.tree = tree;
        self.panes.insert(id, pane);
        self.focus = id;
        self.reflow(area);
        Ok(())
    }

    /// Closes the focused pane. Returns false when it was the last one — the caller
    /// decides whether that closes the tab.
    pub fn close_focused(&mut self, area: Rect) -> bool {
        if self.tree.len() <= 1 {
            return false;
        }
        // Pick the neighbour that will take focus *before* mutating the tree.
        let rects = self.layout(area);
        let next = neighbour(&rects, self.focus, Direction::Left)
            .or_else(|| neighbour(&rects, self.focus, Direction::Right))
            .or_else(|| neighbour(&rects, self.focus, Direction::Up))
            .or_else(|| neighbour(&rects, self.focus, Direction::Down));

        let closed = self.focus;
        if self.tree.close(closed) {
            self.panes.remove(&closed);
            self.focus = next.unwrap_or_else(|| self.tree.panes()[0]);
            self.reflow(area);
            true
        } else {
            false
        }
    }

    /// Moves focus to the pane in `dir`, if any. Returns whether focus moved.
    pub fn focus_dir(&mut self, area: Rect, dir: Direction) -> bool {
        let rects = self.layout(area);
        if let Some(id) = neighbour(&rects, self.focus, dir) {
            self.focus = id;
            true
        } else {
            false
        }
    }

    /// Cycles focus to the next pane in reading order. A keyboard-only fallback for
    /// when directional movement is ambiguous.
    pub fn focus_next(&mut self) {
        let panes = self.tree.panes();
        if let Some(i) = panes.iter().position(|&p| p == self.focus) {
            self.focus = panes[(i + 1) % panes.len()];
        }
    }

    pub fn resize_focused(&mut self, area: Rect, dir: Direction) {
        self.tree.resize(self.focus, dir, 0.03);
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
            if self.tree.len() <= 1 {
                return false; // The last pane died: the tab is done.
            }
            let rects = self.layout(area);
            let next = neighbour(&rects, id, Direction::Left)
                .or_else(|| neighbour(&rects, id, Direction::Right))
                .or_else(|| neighbour(&rects, id, Direction::Up))
                .or_else(|| neighbour(&rects, id, Direction::Down));
            if self.tree.close(id) {
                self.panes.remove(&id);
                if self.focus == id {
                    self.focus = next.unwrap_or_else(|| self.tree.panes()[0]);
                }
            }
        }
        self.reflow(area);
        true
    }

    pub fn title(&self) -> String {
        self.title_override.clone().unwrap_or_else(|| self.focused_ref().title.clone())
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
            let mut pane = Pane::new(cols, rows, config.scrollback.lines, &spawn, wake(*id))?;
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

        Ok(Self {
            tree: state.tree.clone(),
            panes,
            focus,
            title_override: state.title.clone(),
            cell,
            gap,
            padding,
        })
    }

    fn focused_cwd(&self) -> Option<std::path::PathBuf> {
        // Read the child's cwd straight from /proc, so a split opens where the
        // focused shell actually is, not where the window was launched.
        let pane = self.focused_ref();
        let pid = pane.pty_pid()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
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
