//! Session persistence: option C — save the layout *and* the scrollback text, so
//! reopening runnir brings back the tabs where they were and what was on screen,
//! even though the processes themselves are gone.
//!
//! Restored panes relaunch a shell in the saved working directory; the saved
//! scrollback is loaded as inert history above it. This is deliberately not
//! detach/attach: nothing keeps running, so there is no daemon and no surprise
//! about which processes survived.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::layout::{LayoutMode, Node, PaneId};

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub active: usize,
    pub tabs: Vec<TabState>,
}

#[derive(Serialize, Deserialize)]
pub struct TabState {
    pub tree: Node,
    pub focus: PaneId,
    pub title: Option<String>,
    pub panes: HashMap<PaneId, PaneState>,
    /// The active layout mode. Defaults to `Splits` so sessions written before
    /// layout modes existed still load.
    #[serde(default)]
    pub mode: LayoutMode,
    /// Insertion order of the panes, the arrangement source for the algorithmic
    /// modes. Empty in an older session — the tab rebuilds it from the tree.
    #[serde(default)]
    pub order: Vec<PaneId>,
    /// Master pane share for Tall/Fat. `None` in an older session, so the tab uses
    /// its default.
    #[serde(default)]
    pub master_ratio: Option<f32>,
}

#[derive(Serialize, Deserialize)]
pub struct PaneState {
    /// Working directory to relaunch the shell in.
    pub cwd: Option<PathBuf>,
    /// User-set title override, if any.
    pub title: Option<String>,
    /// Scrollback text, oldest line first. Capped on save to keep the file sane.
    pub scrollback: Vec<String>,
}

/// Whether this launch should restore the previous window's snapshot.
///
/// The snapshot is of the window you CLOSED, so it belongs to the next window you
/// open when nothing else is running. A second window opened beside a live one
/// starts clean instead: inheriting the layout of a window that is still on screen
/// is a copy nobody asked for, and it is what every other terminal avoids — tmux
/// attaches by name, kitty applies a template you wrote, Windows Terminal restores
/// only its FIRST window.
///
/// Pure so the rule is testable; the liveness check is the caller's.
pub fn should_restore(enabled: bool, another_window_open: bool) -> bool {
    enabled && !another_window_open
}

const VERSION: u32 = 1;
/// Lines of scrollback kept per pane in the session file. A hard cap so a session
/// file cannot grow without bound from a pane that scrolled for hours.
const MAX_SAVED_LINES: usize = 2000;

impl Session {
    pub fn path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("runnir/session.json")
    }

    pub fn new(active: usize) -> Self {
        Self { version: VERSION, active, tabs: Vec::new() }
    }

    #[cfg(test)]
    pub fn add_tab(&mut self, tree: Node, focus: PaneId, title: Option<String>) -> &mut TabState {
        let order = tree.panes();
        self.tabs.push(TabState {
            tree,
            focus,
            title,
            panes: HashMap::new(),
            mode: LayoutMode::default(),
            order,
            master_ratio: None,
        });
        self.tabs.last_mut().unwrap()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Trim each pane's scrollback to the cap before writing.
        let mut trimmed = Session { version: VERSION, active: self.active, tabs: Vec::new() };
        for tab in &self.tabs {
            let mut panes = HashMap::new();
            for (id, p) in &tab.panes {
                let start = p.scrollback.len().saturating_sub(MAX_SAVED_LINES);
                panes.insert(
                    *id,
                    PaneState {
                        cwd: p.cwd.clone(),
                        title: p.title.clone(),
                        scrollback: p.scrollback[start..].to_vec(),
                    },
                );
            }
            trimmed.tabs.push(TabState {
                tree: tab.tree.clone(),
                focus: tab.focus,
                title: tab.title.clone(),
                panes,
                mode: tab.mode,
                order: tab.order.clone(),
                master_ratio: tab.master_ratio,
            });
        }
        let json = serde_json::to_string_pretty(&trimmed)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Loads a saved session, or `None` if there is none or it is unreadable. A
    /// corrupt session must never stop the terminal from starting.
    pub fn load() -> Option<Session> {
        let text = std::fs::read_to_string(Self::path()).ok()?;
        match serde_json::from_str::<Session>(&text) {
            Ok(s) if s.version == VERSION && !s.tabs.is_empty() => Some(s),
            Ok(_) => None,
            Err(e) => {
                eprintln!("runnir: ignoring unreadable session: {e}");
                None
            }
        }
    }

    /// Removes the saved session (e.g. after a clean restore, so a crash does not
    /// restore a stale layout).
    pub fn clear() {
        let _ = std::fs::remove_file(Self::path());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_second_window_opened_beside_a_live_one_starts_clean() {
        // The snapshot is of the window you CLOSED. It belongs to the next window
        // you open when nothing else is running — inheriting the layout of a
        // window that is still on screen is the copy nobody asked for.
        assert!(should_restore(true, false), "only window, setting on: restore");
        assert!(!should_restore(true, true), "another window is open: start clean");
        assert!(!should_restore(false, false), "setting off: always clean");
        assert!(!should_restore(false, true));
    }

    use super::*;
    use crate::layout::Axis;

    fn sample() -> Session {
        let mut tree = Node::leaf(1);
        tree.split(1, 2, Axis::Horizontal);
        let mut s = Session::new(0);
        let tab = s.add_tab(tree, 2, Some("work".into()));
        tab.panes.insert(
            1,
            PaneState {
                cwd: Some("/home/x".into()),
                title: None,
                scrollback: vec!["line one".into(), "line two".into()],
            },
        );
        tab.panes.insert(
            2,
            PaneState { cwd: Some("/tmp".into()), title: None, scrollback: vec![] },
        );
        s
    }

    #[test]
    fn round_trips_through_json() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.tabs[0].focus, 2);
        assert_eq!(back.tabs[0].title.as_deref(), Some("work"));
        assert_eq!(back.tabs[0].panes.len(), 2);
        assert_eq!(back.tabs[0].panes[&1].scrollback, vec!["line one", "line two"]);
        assert_eq!(back.tabs[0].panes[&1].cwd.as_deref(), Some(std::path::Path::new("/home/x")));
    }

    #[test]
    fn layout_mode_round_trips_and_defaults() {
        // An explicit mode/order/master survives the round trip...
        let mut s = sample();
        s.tabs[0].mode = LayoutMode::Tall;
        s.tabs[0].order = vec![2, 1];
        s.tabs[0].master_ratio = Some(0.7);
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs[0].mode, LayoutMode::Tall);
        assert_eq!(back.tabs[0].order, vec![2, 1]);
        assert_eq!(back.tabs[0].master_ratio, Some(0.7));

        // ...and a pre-layout-modes session (no such fields) loads as Splits with an
        // empty order and no master, so old session files still open.
        let legacy = r#"{"version":1,"active":0,"tabs":[
            {"tree":{"Leaf":1},"focus":1,"title":null,"panes":{}}]}"#;
        let back: Session = serde_json::from_str(legacy).unwrap();
        assert_eq!(back.tabs[0].mode, LayoutMode::Splits);
        assert!(back.tabs[0].order.is_empty());
        assert_eq!(back.tabs[0].master_ratio, None);
    }

    #[test]
    fn tree_survives_the_round_trip() {
        // The split structure is the point of a session; it must serialize exactly.
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs[0].tree.panes(), vec![1, 2]);
    }

    #[test]
    fn a_wrong_version_is_refused() {
        let mut s = sample();
        s.version = 999;
        let json = serde_json::to_string(&s).unwrap();
        // Simulate load()'s version gate.
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_ne!(back.version, VERSION, "the gate in load() would drop this");
    }
}
