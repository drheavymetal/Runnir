//! Per-project session: the pane/tab layout a project directory was last left in.
//!
//! Distinct from [`crate::session`], which snapshots the *whole window* (including
//! scrollback text) to a single file so a restart brings back exactly what was on
//! screen. This module is narrower and keyed by *project*: it records only the
//! shape of the splits and each pane's working directory, so opening runnir in a
//! project — or invoking the restore command — rebuilds the same arrangement of
//! shells, each in the directory it was in. No scrollback, no processes: layout and
//! cwd only.
//!
//! The store is a small JSON file holding the last few dozen projects, most-recent
//! first (an LRU), so it never grows without bound.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::{LayoutMode, Node, PaneId};

/// How many projects the store keeps. Beyond this the least-recently-saved project
/// is dropped, so the file stays a handful of kilobytes however many repos you open.
const MAX_PROJECTS: usize = 50;

const VERSION: u32 = 1;

/// The nearest ancestor of `start` that is a git repository (contains a `.git`
/// directory), or `start` itself when it is not inside one.
///
/// This is the "project key": two panes anywhere under the same repo map to the
/// same key, so the layout you saved from `~/proj/src` restores when you reopen in
/// `~/proj`. The path is canonicalized first, because the same project is reported
/// through different spellings — OSC 7 gives the shell's logical (symlinked) path
/// while `/proc` and `env::current_dir()` give the resolved one — and the key must
/// be identical for all of them or a saved session is never found again. A path
/// that cannot be resolved (already gone) is used as given.
pub fn project_key(start: &Path) -> PathBuf {
    let canon = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let mut cur = Some(canon.as_path());
    while let Some(dir) = cur {
        if dir.join(".git").is_dir() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    canon
}

/// One tab's layout: the split tree, the arrangement mode, and where each pane's
/// shell was working. The minimal descriptor — deliberately no grid, no scrollback,
/// no GPU state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabLayout {
    pub tree: Node,
    pub focus: PaneId,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub mode: LayoutMode,
    #[serde(default)]
    pub order: Vec<PaneId>,
    #[serde(default)]
    pub master_ratio: Option<f32>,
    /// Working directory per pane id. A pane missing here (its cwd was unreadable)
    /// restores in the default directory.
    #[serde(default)]
    pub cwds: HashMap<PaneId, PathBuf>,
}

impl TabLayout {
    /// Converts to a [`crate::session::TabState`] so the existing tab-rebuild path
    /// (`Tab::from_session`) can be reused verbatim — it already relaunches one shell
    /// per pane in its saved cwd. Scrollback is empty: this feature restores layout
    /// and cwd, nothing else.
    pub fn to_tab_state(&self) -> crate::session::TabState {
        let mut panes = HashMap::new();
        for id in self.tree.panes() {
            panes.insert(
                id,
                crate::session::PaneState {
                    cwd: self.cwds.get(&id).cloned(),
                    title: None,
                    scrollback: Vec::new(),
                },
            );
        }
        crate::session::TabState {
            tree: self.tree.clone(),
            focus: self.focus,
            title: self.title.clone(),
            panes,
            mode: self.mode,
            order: self.order.clone(),
            master_ratio: self.master_ratio,
        }
    }

    /// A copy of this layout with every pane id rewritten to fresh, sequential ids
    /// starting at `first_id`, returning the layout and the next unused id.
    ///
    /// Pane ids are global across the whole window (scroll animation, copy mode and
    /// remote control all resolve a pane by id across every tab), so an *additive*
    /// restore must never re-introduce the ids the layout was saved with — they very
    /// likely already belong to open panes, and a second restore of the same entry
    /// would certainly duplicate them. Tree shape, focus, order and cwds all follow
    /// the same old→new mapping, so the layout is identical up to renumbering.
    pub fn remapped_from(&self, first_id: PaneId) -> (TabLayout, PaneId) {
        let mut next = first_id;
        let mut map: HashMap<PaneId, PaneId> = HashMap::new();
        for old in self.tree.panes() {
            map.entry(old).or_insert_with(|| {
                let id = next;
                next += 1;
                id
            });
        }
        let renamed = |id: PaneId| map.get(&id).copied().unwrap_or(id);
        let layout = TabLayout {
            tree: remap_node(&self.tree, &map),
            focus: renamed(self.focus),
            title: self.title.clone(),
            mode: self.mode,
            order: self.order.iter().map(|&id| renamed(id)).collect(),
            master_ratio: self.master_ratio,
            cwds: self.cwds.iter().map(|(&id, p)| (renamed(id), p.clone())).collect(),
        };
        (layout, next)
    }
}

/// Rebuilds a split tree with every leaf's pane id passed through `map` (an id the
/// map does not know — impossible for a map built from this very tree — is kept).
fn remap_node(node: &Node, map: &HashMap<PaneId, PaneId>) -> Node {
    match node {
        Node::Leaf(id) => Node::Leaf(map.get(id).copied().unwrap_or(*id)),
        Node::Split { axis, ratio, first, second } => Node::Split {
            axis: *axis,
            ratio: *ratio,
            first: Box::new(remap_node(first, map)),
            second: Box::new(remap_node(second, map)),
        },
    }
}

/// A project's saved arrangement: every tab's layout plus which tab was active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    /// The project key (a repo root, or a bare directory).
    pub key: PathBuf,
    pub active: usize,
    pub tabs: Vec<TabLayout>,
    /// Unix seconds when this was saved, for display and as the LRU tiebreak.
    #[serde(default)]
    pub saved_at: u64,
}

impl ProjectEntry {
    /// Rebuilds a [`crate::session::Session`] so `restore_tabs` (the startup path)
    /// and `Tab::from_session` (the per-tab path) can rebuild everything with no new
    /// lock or spawn code.
    pub fn to_session(&self) -> crate::session::Session {
        let mut s = crate::session::Session::new(self.active.min(self.tabs.len().saturating_sub(1)));
        for tab in &self.tabs {
            s.tabs.push(tab.to_tab_state());
        }
        s
    }
}

/// The whole store: a bounded, most-recent-first list of projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSessions {
    pub version: u32,
    pub projects: Vec<ProjectEntry>,
}

impl Default for ProjectSessions {
    fn default() -> Self {
        Self { version: VERSION, projects: Vec::new() }
    }
}

impl ProjectSessions {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("runnir/sessions.json")
    }

    /// Loads the store, or an empty one when the file is missing or unreadable. A
    /// corrupt store must never stop the terminal from starting, so a parse error is
    /// reported and swallowed.
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(s) if s.version == VERSION => s,
            Ok(_) => Self::default(),
            Err(e) => {
                eprintln!("runnir: ignoring unreadable project sessions: {e}");
                Self::default()
            }
        }
    }

    /// The saved arrangement for `key`, if any (without removing it).
    pub fn get(&self, key: &Path) -> Option<&ProjectEntry> {
        self.projects.iter().find(|e| e.key == key)
    }

    /// Inserts or replaces the entry for its key and moves it to the front (most
    /// recent), trimming the oldest projects past the cap. This is the LRU update.
    pub fn upsert(&mut self, mut entry: ProjectEntry) {
        entry.saved_at = now_secs();
        self.projects.retain(|e| e.key != entry.key);
        self.projects.insert(0, entry);
        self.projects.truncate(MAX_PROJECTS);
    }

    /// Persists the store atomically: writes a sibling temp file with private
    /// permissions and no symlink-following, then renames it over the target so a
    /// reader never sees a half-written file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        write_atomic(&path, json.as_bytes())
    }
}

/// Seconds since the Unix epoch, or 0 if the clock is before it (never, in practice).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Writes `data` to `path` atomically. A sibling temp file is opened with `O_NOFOLLOW`
/// (so a planted symlink cannot redirect the write) and mode 0600, flushed, then
/// renamed over `path`. The rename is atomic on the same filesystem, so a concurrent
/// reader sees either the old file or the new one, never a truncated one. Mirrors the
/// safety of `write_private` in the input layer.
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let tmp = path.with_extension("json.tmp");
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            // libc's constant, correct on both Linux and macOS (the raw value differs).
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    };
    // Any failure past creating the temp file must remove it, or an aborted save
    // strands a stale .tmp next to the store forever.
    let result = write().and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Axis;

    #[test]
    fn key_is_the_dir_itself_outside_a_repo() {
        // No .git anywhere: the key is the directory you launched in.
        let tmp = tempdir();
        let key = project_key(&tmp);
        assert_eq!(key, tmp);
        cleanup(&tmp);
    }

    #[test]
    fn key_walks_up_to_the_nearest_git_ancestor() {
        // repo/.git, repo/a/b — launching in a/b keys to repo.
        let repo = tempdir();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let deep = repo.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();

        assert_eq!(project_key(&deep), repo, "a nested path keys to the repo root");
        assert_eq!(project_key(&repo), repo, "the repo root keys to itself");
        cleanup(&repo);
    }

    #[test]
    fn key_picks_the_nearest_repo_not_an_outer_one() {
        // outer/.git and outer/inner/.git — inner/x keys to inner, not outer.
        let outer = tempdir();
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        let inner = outer.join("inner");
        std::fs::create_dir_all(inner.join(".git")).unwrap();
        let x = inner.join("x");
        std::fs::create_dir_all(&x).unwrap();

        assert_eq!(project_key(&x), inner, "the nearest .git wins");
        cleanup(&outer);
    }

    #[test]
    fn a_symlinked_path_keys_to_the_same_project_as_the_real_one() {
        // The shell's OSC 7 cwd may spell the project through a symlink while the
        // startup cwd is resolved; both must produce the same key or the saved
        // session is invisible on restart.
        let root = tempdir();
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let link = root.join("alias");
        std::os::unix::fs::symlink(&repo, &link).unwrap();

        assert_eq!(project_key(&link), project_key(&repo), "symlink and real path agree");
        assert_eq!(project_key(&link), repo, "and the key is the resolved repo root");
        cleanup(&root);
    }

    #[test]
    fn a_git_file_is_not_a_repo_dir() {
        // A `.git` *file* (submodule/worktree pointer) is not a `.git` dir; the walk
        // must not stop on it, matching the documented "contains a .git dir" rule.
        let dir = tempdir();
        std::fs::write(dir.join(".git"), b"gitdir: /elsewhere").unwrap();
        assert_eq!(project_key(&dir), dir, "no .git dir found, so the key is the dir");
        cleanup(&dir);
    }

    #[test]
    fn layout_descriptor_round_trips_through_json() {
        // The whole point of the descriptor is that it serializes and comes back
        // identical, so a saved layout rebuilds the same tree, mode and cwds.
        let mut tree = Node::leaf(1);
        tree.split(1, 2, Axis::Horizontal);
        tree.split(2, 3, Axis::Vertical);
        let mut cwds = HashMap::new();
        cwds.insert(1, PathBuf::from("/home/x/proj"));
        cwds.insert(2, PathBuf::from("/home/x/proj/src"));
        cwds.insert(3, PathBuf::from("/tmp"));
        let entry = ProjectEntry {
            key: PathBuf::from("/home/x/proj"),
            active: 0,
            saved_at: 42,
            tabs: vec![TabLayout {
                tree,
                focus: 3,
                title: Some("work".into()),
                mode: LayoutMode::Tall,
                order: vec![1, 2, 3],
                master_ratio: Some(0.7),
                cwds,
            }],
        };
        let store = ProjectSessions { version: VERSION, projects: vec![entry] };

        let json = serde_json::to_string_pretty(&store).unwrap();
        let back: ProjectSessions = serde_json::from_str(&json).unwrap();

        assert_eq!(back.projects.len(), 1);
        let e = &back.projects[0];
        assert_eq!(e.key, PathBuf::from("/home/x/proj"));
        let t = &e.tabs[0];
        assert_eq!(t.tree.panes(), vec![1, 2, 3], "the split shape survives");
        assert_eq!(t.focus, 3);
        assert_eq!(t.mode, LayoutMode::Tall);
        assert_eq!(t.order, vec![1, 2, 3]);
        assert_eq!(t.master_ratio, Some(0.7));
        assert_eq!(t.cwds[&2], PathBuf::from("/home/x/proj/src"));
        assert_eq!(t.title.as_deref(), Some("work"));
    }

    #[test]
    fn to_tab_state_carries_cwds_and_leaves_scrollback_empty() {
        let mut tree = Node::leaf(1);
        tree.split(1, 2, Axis::Horizontal);
        let mut cwds = HashMap::new();
        cwds.insert(1, PathBuf::from("/a"));
        // Pane 2 deliberately has no cwd — it must still get a pane state.
        let layout = TabLayout {
            tree,
            focus: 1,
            title: None,
            mode: LayoutMode::Splits,
            order: vec![1, 2],
            master_ratio: None,
            cwds,
        };
        let st = layout.to_tab_state();
        assert_eq!(st.panes.len(), 2);
        assert_eq!(st.panes[&1].cwd.as_deref(), Some(Path::new("/a")));
        assert_eq!(st.panes[&2].cwd, None);
        assert!(st.panes[&1].scrollback.is_empty(), "no scrollback is persisted here");
    }

    #[test]
    fn remapping_renumbers_every_pane_reference_consistently() {
        // An additive restore must never reuse the saved pane ids (they collide with
        // panes already open); the remap gives fresh ids while tree shape, focus,
        // order and cwds all follow the same mapping.
        let mut tree = Node::leaf(1);
        tree.split(1, 2, Axis::Horizontal);
        tree.split(2, 3, Axis::Vertical);
        let mut cwds = HashMap::new();
        cwds.insert(1, PathBuf::from("/a"));
        cwds.insert(3, PathBuf::from("/c"));
        let layout = TabLayout {
            tree,
            focus: 2,
            title: None,
            mode: LayoutMode::Tall,
            order: vec![3, 1, 2],
            master_ratio: Some(0.6),
            cwds,
        };

        let (out, next) = layout.remapped_from(1001);
        // Tree traversal order is 1, 2, 3 → 1001, 1002, 1003.
        assert_eq!(out.tree.panes(), vec![1001, 1002, 1003], "fresh ids, same shape");
        assert_eq!(next, 1004, "the next unused id is reported");
        assert_eq!(out.focus, 1002, "focus follows its pane");
        assert_eq!(out.order, vec![1003, 1001, 1002], "order follows the mapping");
        assert_eq!(out.cwds[&1001], PathBuf::from("/a"), "cwds follow their panes");
        assert_eq!(out.cwds[&1003], PathBuf::from("/c"));
        assert!(!out.cwds.contains_key(&1), "no stale old-id entries remain");
        // The crux: none of the original ids survive, so nothing can collide with
        // panes already on screen.
        assert!(out.tree.panes().iter().all(|id| ![1u64, 2, 3].contains(id)));
    }

    #[test]
    fn upsert_is_lru_and_bounded() {
        let mut store = ProjectSessions::default();
        // Fill past the cap; each new key lands at the front.
        for i in 0..(MAX_PROJECTS + 10) {
            store.upsert(entry_for(&format!("/p/{i}")));
        }
        assert_eq!(store.projects.len(), MAX_PROJECTS, "the store is capped");
        assert_eq!(
            store.projects[0].key,
            PathBuf::from(format!("/p/{}", MAX_PROJECTS + 9)),
            "the newest save is at the front"
        );

        // Re-saving an existing key moves it to the front rather than duplicating.
        let existing = store.projects[5].key.clone();
        let before = store.projects.len();
        store.upsert(entry_for(existing.to_str().unwrap()));
        assert_eq!(store.projects.len(), before, "an existing key is not duplicated");
        assert_eq!(store.projects[0].key, existing, "a re-save moves it to the front");
    }

    #[test]
    fn a_failed_atomic_write_does_not_strand_a_temp_file() {
        // Force the final rename to fail (the target is a directory): the write must
        // report the error AND clean up its temp file, not leave a stale .tmp beside
        // the store forever.
        let dir = tempdir();
        let target = dir.join("sessions.json");
        std::fs::create_dir_all(&target).unwrap();

        let err = write_atomic(&target, b"{}");
        assert!(err.is_err(), "renaming over a directory must fail");
        assert!(
            !dir.join("sessions.json.tmp").exists(),
            "the temp file must be removed on failure"
        );
        cleanup(&dir);
    }

    #[test]
    fn get_finds_a_saved_project() {
        let mut store = ProjectSessions::default();
        store.upsert(entry_for("/one"));
        store.upsert(entry_for("/two"));
        assert!(store.get(Path::new("/two")).is_some());
        assert!(store.get(Path::new("/missing")).is_none());
    }

    // ---- test helpers ----------------------------------------------------

    fn entry_for(key: &str) -> ProjectEntry {
        ProjectEntry {
            key: PathBuf::from(key),
            active: 0,
            saved_at: 0,
            tabs: vec![TabLayout {
                tree: Node::leaf(1),
                focus: 1,
                title: None,
                mode: LayoutMode::Splits,
                order: vec![1],
                master_ratio: None,
                cwds: HashMap::new(),
            }],
        }
    }

    /// A fresh unique temp directory. Avoids a dev-dependency by using the process id
    /// and a counter; the tests create and clean up their own trees.
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("runnir-projtest-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Canonicalized, so comparisons against project_key (which canonicalizes)
        // hold even where the system temp dir is itself behind a symlink.
        dir.canonicalize().unwrap()
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
