//! The file explorer sidebar: a persistent tree of the project, beside the panes.
//!
//! It is CHROME, not an `Overlay` and not a `Pane` — see the design entry in
//! `docs/DEVLOG.md`. An overlay captures the keyboard and covers the pane; this has
//! to stay up while you work in the pane next to it. A `Pane` owns a non-optional
//! PTY, and making it an enum to hold a widget with no process would touch
//! everything; instead the sidebar reserves columns out of the tab's area and the
//! layout tree never learns it exists.
//!
//! Every directory read happens on a worker thread tagged with a sequence number,
//! like the git panel: a synchronous `read_dir` of `node_modules` or of an NFS mount
//! freezes the frame it happens in.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::render::Rect;

/// Which edge the sidebar sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Left,
    Right,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    pub fn flip(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// One entry of a directory, as the worker read it. Deliberately a snapshot: the
/// tree is redrawn from these, never from the filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// A symlink, whose target decides `dir`. Expanding one is refused (cycles).
    pub link: bool,
    pub exec: bool,
    pub size: u64,
    pub mtime: u64,
}

/// A drawn row of the tree: an entry at a depth, or the marker that says a
/// directory had more children than the cap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub entry: Entry,
    pub depth: usize,
    pub open: bool,
    /// `Some(n)` on the synthetic "… n more" row that closes a capped directory.
    pub more: Option<usize>,
    /// What git says about this path — for a directory, about anything under it.
    pub badge: Option<Badge>,
    /// Ignored by git. Only ever drawn when the tree was asked to show them.
    pub ignored: bool,
}

/// The order rows are drawn in inside one directory.
///
/// The second mode is the whole of what the "what is the agent touching right now"
/// idea became: a sort, not a second view to keep in step with the first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    /// Directories first, then case-insensitively by name — what every file manager
    /// does, and what the eye scans for.
    #[default]
    Name,
    /// Most recently modified first, directories mixed in: a directory's own mtime
    /// moves when something is created or removed in it, which is exactly the event
    /// this mode is for.
    Mtime,
}

impl Sort {
    pub fn flip(self) -> Sort {
        match self {
            Sort::Name => Sort::Mtime,
            Sort::Mtime => Sort::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Mtime => "modified",
        }
    }
}

/// What git says about one path, as one letter beside its name.
///
/// The unstaged letter wins over the staged one where a path has both: an unstaged
/// change is the one that is not written down anywhere yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Badge {
    Conflict,
    Untracked,
    Modified,
    Added,
    Deleted,
    /// Changed and already staged.
    Staged,
    /// Only on a directory: something below it has changed. A directory never
    /// borrows a letter from one of its children — `M` on a folder would read as
    /// "this folder was modified", which is not a thing git says.
    Dirty,
}

impl Badge {
    /// Maps a `--porcelain=v2` XY pair. `?` is untracked and `U` is a conflict, both
    /// of which git reports in both columns.
    pub fn from_status(index: char, worktree: char) -> Badge {
        if index == 'U' || worktree == 'U' {
            return Badge::Conflict;
        }
        if index == '?' {
            return Badge::Untracked;
        }
        match worktree {
            'M' => Badge::Modified,
            'D' => Badge::Deleted,
            'A' => Badge::Added,
            _ => match index {
                'A' => Badge::Added,
                'D' => Badge::Deleted,
                _ => Badge::Staged,
            },
        }
    }

    pub fn letter(self) -> char {
        match self {
            Badge::Conflict => '!',
            Badge::Untracked => '?',
            Badge::Modified => 'M',
            Badge::Added => 'A',
            Badge::Deleted => 'D',
            Badge::Staged => 'M',
            Badge::Dirty => '\u{b7}',
        }
    }
}

/// How many children of one directory are kept. A directory with more gets a
/// visible `… n more` row: a silent cap reads as "this is all of it".
pub const CHILD_CAP: usize = 2000;

/// The narrowest and the widest the sidebar may be. The maximum is a share of the
/// window rather than a constant, but the WIDTH is stored in columns: a fraction on
/// an ultrawide gives a 90-column tree nobody asked for.
pub const MIN_WIDTH: usize = 18;

pub struct Explorer {
    pub root: PathBuf,
    /// Children per directory, as read. The tree drawn is this plus `expanded`.
    pub children: HashMap<PathBuf, Vec<Entry>>,
    pub expanded: HashSet<PathBuf>,
    /// Directories whose read is in flight, so a second keypress does not queue a
    /// second read of the same place.
    pub loading: HashSet<PathBuf>,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub scroll: usize,
    pub width: usize,
    pub side: Side,
    /// Whether the sidebar is on screen at all. Kept (rather than dropping the
    /// whole struct) so closing and reopening does not collapse the tree.
    pub open: bool,
    /// Whether the KEYBOARD is in the sidebar. Open but unfocused is the normal
    /// state: you read the tree while you work in the pane.
    pub focused: bool,
    pub show_hidden: bool,
    /// Show what git ignores. Off by default: a Rust checkout's tree is `target/`
    /// and little else, and the whole reason to keep a tree open is that it is the
    /// project. Never a silent cut — the footer says how many rows this is holding
    /// back, and `I` brings them back dimmed.
    pub show_ignored: bool,
    pub sort: Sort,
    /// What git says about each changed FILE, absolute.
    pub git: HashMap<PathBuf, Badge>,
    /// The same, folded up onto every directory above a changed file.
    pub git_dirs: HashMap<PathBuf, Badge>,
    /// Ignored paths, absolute and collapsed to directories where git collapsed
    /// them: a path is ignored when it or an ancestor is in here.
    pub ignored: HashSet<PathBuf>,
    /// How many rows the last rebuild left out because they are ignored.
    pub hidden_by_ignore: usize,
    /// Read generation: an answer tagged with an older one is dropped, so a slow
    /// `read_dir` cannot land after the tree moved on.
    pub seq: u64,
    pub message: Option<String>,
    /// The sidebar's own leader layer: `None` when disarmed, else the group keys
    /// pressed so far. Same shape, and the same which-key, as the git panel's.
    pub leader: Option<Vec<char>>,
    /// A path the cursor should land on once the tree has been re-read — what an
    /// operation just produced. Cleared when it is used.
    pub pending_cursor: Option<PathBuf>,
}

impl Explorer {
    pub fn new(root: PathBuf, width: usize, side: Side) -> Self {
        let mut e = Self {
            root: root.clone(),
            children: HashMap::new(),
            expanded: HashSet::new(),
            loading: HashSet::new(),
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            width,
            side,
            open: true,
            focused: true,
            show_hidden: false,
            show_ignored: false,
            sort: Sort::default(),
            git: HashMap::new(),
            git_dirs: HashMap::new(),
            ignored: HashSet::new(),
            hidden_by_ignore: 0,
            seq: 0,
            message: None,
            leader: None,
            pending_cursor: None,
        };
        e.expanded.insert(root);
        e
    }

    /// Moves the tree to another root, keeping the sidebar's size and side. Called
    /// only when the git root CHANGES: re-anchoring on every `cd` collapses the tree
    /// while you are navigating inside one repository.
    pub fn set_root(&mut self, root: PathBuf) {
        if self.root == root {
            return;
        }
        self.root = root.clone();
        self.children.clear();
        self.expanded.clear();
        self.loading.clear();
        self.expanded.insert(root);
        self.rows.clear();
        self.cursor = 0;
        self.scroll = 0;
        // The marks belong to the repository that was left: keeping them would badge
        // paths of the new tree with another project's status.
        self.git.clear();
        self.git_dirs.clear();
        self.ignored.clear();
        self.hidden_by_ignore = 0;
    }

    /// Records what git says about the tree: a badge per changed file, folded up onto
    /// the directories above it, plus what git ignores.
    ///
    /// The fold happens here rather than while drawing because it is O(changes ×
    /// depth) once, against O(rows × changes) on every rebuild — and rebuilds happen
    /// on every keypress that moves a fold.
    pub fn set_git(&mut self, files: Vec<(PathBuf, Badge)>, ignored: HashSet<PathBuf>) {
        self.git.clear();
        self.git_dirs.clear();
        for (path, badge) in files {
            let mut cur = path.parent().map(|p| p.to_path_buf());
            while let Some(dir) = cur {
                if !dir.starts_with(&self.root) {
                    break;
                }
                let slot = self.git_dirs.entry(dir.clone()).or_insert(Badge::Dirty);
                // A conflict is the one thing a directory does say out loud: it is
                // the state where a stray keystroke loses a merge.
                if badge == Badge::Conflict {
                    *slot = Badge::Conflict;
                }
                if dir == self.root {
                    break;
                }
                cur = dir.parent().map(|p| p.to_path_buf());
            }
            self.git.insert(path, badge);
        }
        self.ignored = ignored;
        self.rebuild();
    }

    /// Whether git ignores a path, itself or through an ancestor it collapsed.
    pub fn is_ignored(&self, path: &Path) -> bool {
        if self.ignored.is_empty() {
            return false;
        }
        let mut cur = Some(path);
        while let Some(p) = cur {
            if self.ignored.contains(p) {
                return true;
            }
            if p == self.root {
                break;
            }
            cur = p.parent();
        }
        false
    }

    /// The badge a row gets: its own for a file, the folded one for a directory.
    fn badge_for(&self, entry: &Entry) -> Option<Badge> {
        if entry.dir {
            self.git_dirs.get(&entry.path).copied()
        } else {
            self.git.get(&entry.path).copied()
        }
    }

    /// The area left for the panes: the sidebar's columns, taken off one side.
    pub fn reserve(&self, area: Rect, cell: (f32, f32)) -> Rect {
        if !self.open {
            return area;
        }
        let w = (self.width_in(area, cell) as f32 * cell.0).min(area.w);
        match self.side {
            Side::Left => Rect { x: area.x + w, w: area.w - w, ..area },
            Side::Right => Rect { w: area.w - w, ..area },
        }
    }

    /// Where the sidebar itself is drawn.
    pub fn rect(&self, area: Rect, cell: (f32, f32)) -> Rect {
        let w = (self.width_in(area, cell) as f32 * cell.0).min(area.w);
        match self.side {
            Side::Left => Rect { w, ..area },
            Side::Right => Rect { x: area.x + area.w - w, w, ..area },
        }
    }

    /// The width actually used, clamped against the window. Clamping here rather
    /// than when it is set means a window that shrinks cannot leave the sidebar
    /// wider than the tab it is in.
    pub fn width_in(&self, area: Rect, cell: (f32, f32)) -> usize {
        let cols = (area.w / cell.0).floor().max(1.0) as usize;
        let max = (cols * 2 / 5).max(MIN_WIDTH).min(cols.saturating_sub(10).max(MIN_WIDTH));
        self.width.clamp(MIN_WIDTH.min(max), max)
    }

    /// The row under the cursor.
    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor.min(self.rows.len().saturating_sub(1)))
    }

    pub fn move_cursor(&mut self, delta: i32, body_rows: usize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as i32;
        self.cursor = (self.cursor as i32 + delta).clamp(0, n - 1) as usize;
        self.scroll_into_view(body_rows);
    }

    pub fn set_cursor(&mut self, i: usize, body_rows: usize) {
        self.cursor = i.min(self.rows.len().saturating_sub(1));
        self.scroll_into_view(body_rows);
    }

    fn scroll_into_view(&mut self, body_rows: usize) {
        let body = body_rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + body {
            self.scroll = self.cursor + 1 - body;
        }
    }

    /// Whether a directory's children still have to be read.
    pub fn needs_read(&self, dir: &Path) -> bool {
        !self.children.contains_key(dir) && !self.loading.contains(dir)
    }

    /// Records a finished read and rebuilds the visible rows.
    pub fn insert_children(&mut self, dir: PathBuf, entries: Vec<Entry>) {
        self.loading.remove(&dir);
        self.children.insert(dir, entries);
        self.rebuild();
    }

    /// Folds a directory shut, or opens it — answering whether a read is needed.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
            self.rebuild();
            return false;
        }
        self.expanded.insert(path.to_path_buf());
        // Only REPORTS that a read is needed; marking it in flight belongs to
        // whoever spawns the worker. Marking it here made `needs_read` false before
        // the spawn ran, so the directory opened and never loaded.
        let need = self.needs_read(path);
        self.rebuild();
        need
    }

    /// Flattens the expanded tree into the rows the sidebar draws.
    pub fn rebuild(&mut self) {
        let keep = self.selected().map(|r| r.entry.path.clone());
        let mut rows = Vec::new();
        let mut hidden = 0usize;
        self.walk(&self.root.clone(), 0, &mut rows, &mut hidden);
        self.rows = rows;
        self.hidden_by_ignore = hidden;
        // A path an operation just produced wins: after a rename, the cursor belongs
        // on the new name, not on where the old one used to be.
        if let Some(want) = self.pending_cursor.clone() {
            if let Some(i) = self.rows.iter().position(|r| r.entry.path == want) {
                self.pending_cursor = None;
                self.cursor = i;
                return;
            }
        }
        // Keep the cursor on the same PATH across a rebuild: a tree that moves the
        // selection every time a directory finishes loading is unusable on a slow
        // filesystem, which is exactly where the reads are slow.
        if let Some(path) = keep {
            if let Some(i) = self.rows.iter().position(|r| r.entry.path == path) {
                self.cursor = i;
            }
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    fn walk(&self, dir: &Path, depth: usize, out: &mut Vec<Row>, hidden: &mut usize) {
        let Some(entries) = self.children.get(dir) else { return };
        // Ignored rows are dropped BEFORE the cap, so hiding `target/` cannot be what
        // pushes a directory past 2000 children.
        let mut visible: Vec<&Entry> = Vec::with_capacity(entries.len());
        for entry in entries {
            if !self.show_ignored && self.is_ignored(&entry.path) {
                *hidden += 1;
                continue;
            }
            visible.push(entry);
        }
        if self.sort == Sort::Mtime {
            // Newest first, with the name as the tie-break so the order is stable
            // between rebuilds (a whole checkout shares one mtime to the second).
            visible.sort_by(|a, b| {
                b.mtime.cmp(&a.mtime).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        }
        let total = visible.len();
        let shown = total.min(CHILD_CAP);
        for entry in &visible[..shown] {
            let open = entry.dir && self.expanded.contains(&entry.path);
            out.push(Row {
                entry: (*entry).clone(),
                depth,
                open,
                more: None,
                badge: self.badge_for(entry),
                ignored: self.show_ignored && self.is_ignored(&entry.path),
            });
            if open {
                self.walk(&entry.path.clone(), depth + 1, out, hidden);
            }
        }
        if total > shown {
            let n = total - shown;
            let entry = Entry {
                name: format!("\u{2026} {n} more"),
                path: dir.join(format!("\u{2026}{n}")),
                dir: false,
                link: false,
                exec: false,
                size: 0,
                mtime: 0,
            };
            out.push(Row { entry, depth, open: false, more: Some(n), badge: None, ignored: false });
        }
    }

    /// Everything the tree has open, so a rebuild after a refresh can restore it.
    pub fn open_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        dirs.sort();
        dirs
    }
}

// ---- the sidebar's own leader layer ----------------------------------------
//
// Same contract as the git panel's (`overlay::GIT_LEADER`): every leaf PRESSES a
// key the sidebar already binds, so a verb cannot mean one thing from its letter
// and another from the menu, and a leaf that this row cannot do is not offered.

/// A key the sidebar understands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKey {
    Ch(char),
    Enter,
}

/// What a leaf does, and when it is offered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilePress {
    /// Always available.
    Key(FileKey),
    /// Only on a FILE row — "edit" and "open with the system" mean nothing on a
    /// directory, and offering them there is how a menu teaches the wrong thing.
    OnFile(FileKey),
    /// Only on a directory row.
    OnDir(FileKey),
}

pub enum FileNode {
    Leaf(FilePress),
    Group(&'static [FileEntry]),
}

pub struct FileEntry {
    pub key: char,
    pub title: &'static str,
    pub node: FileNode,
}

const fn fleaf(key: char, title: &'static str, press: FilePress) -> FileEntry {
    FileEntry { key, title, node: FileNode::Leaf(press) }
}

use FileKey::{Ch as FCh, Enter as FEnter};
use FilePress::{Key as FKey, OnDir, OnFile};

/// The sidebar's leader tree. It grows with the panel: the verbs that are not built
/// yet (properties, rename, delete) are not listed, because a menu entry that does
/// nothing is worse than a missing one.
pub static FILE_LEADER: &[FileEntry] = &[
    FileEntry {
        key: 'f',
        title: "File",
        node: FileNode::Group(&[
            fleaf('o', "open it (view, or ask)", OnFile(FEnter)),
            fleaf('e', "edit it in $EDITOR", OnFile(FCh('e'))),
            fleaf('s', "open with the system", OnFile(FCh('o'))),
            fleaf('p', "properties & permissions", FKey(FCh('p'))),
            fleaf('n', "new file or directory", FKey(FCh('a'))),
            fleaf('r', "rename it", FKey(FCh('r'))),
            fleaf('d', "delete it", FKey(FCh('d'))),
            fleaf('y', "copy the path", FKey(FCh('y'))),
        ]),
    },
    FileEntry {
        key: 'd',
        title: "Directory",
        node: FileNode::Group(&[
            fleaf('o', "fold / unfold", OnDir(FEnter)),
            fleaf('u', "go to the parent", FKey(FCh('h'))),
            fleaf('p', "properties & permissions", OnDir(FCh('p'))),
            fleaf('n', "new file or directory inside", OnDir(FCh('a'))),
            fleaf('r', "rename it", OnDir(FCh('r'))),
            fleaf('d', "delete it (asks, with a count)", OnDir(FCh('d'))),
            fleaf('y', "copy the path", FKey(FCh('y'))),
        ]),
    },
    FileEntry {
        key: 'v',
        title: "View",
        node: FileNode::Group(&[
            fleaf('.', "hidden files", FKey(FCh('.'))),
            fleaf('i', "files git ignores", FKey(FCh('I'))),
            fleaf('s', "sort by name / by date", FKey(FCh('s'))),
            fleaf('r', "reread the tree", FKey(FCh('R'))),
            fleaf('t', "to the top", FKey(FCh('g'))),
            fleaf('b', "to the bottom", FKey(FCh('G'))),
        ]),
    },
    fleaf('q', "Back to the pane", FKey(FCh('q'))),
];

/// The sidebar's leader state, mirroring `GitPanel`'s.
impl Explorer {
    pub fn arm_leader(&mut self) {
        self.leader = Some(Vec::new());
    }

    pub fn cancel_leader(&mut self) {
        self.leader = None;
    }

    fn leader_level(&self) -> Option<&'static [FileEntry]> {
        let path = self.leader.as_ref()?;
        let mut level: &'static [FileEntry] = FILE_LEADER;
        for key in path {
            match level.iter().find(|e| e.key == *key) {
                Some(FileEntry { node: FileNode::Group(next), .. }) => level = next,
                _ => return Some(FILE_LEADER),
            }
        }
        Some(level)
    }

    /// Whether a leaf can act on the row under the cursor.
    fn leader_applies(&self, press: FilePress) -> bool {
        let dir = self.selected().map(|r| r.entry.dir && r.more.is_none());
        match press {
            FilePress::OnFile(_) => dir == Some(false),
            FilePress::OnDir(_) => dir == Some(true),
            FilePress::Key(_) => true,
        }
    }

    pub fn leader_entries(&self) -> Vec<(String, String, bool)> {
        let Some(level) = self.leader_level() else { return Vec::new() };
        let mut out: Vec<(String, String, bool)> = level
            .iter()
            .filter(|e| match &e.node {
                FileNode::Leaf(p) => self.leader_applies(*p),
                FileNode::Group(_) => true,
            })
            .map(|e| (e.key.to_string(), e.title.to_string(), matches!(e.node, FileNode::Group(_))))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn leader_path(&self) -> Vec<String> {
        self.leader.as_ref().map(|p| p.iter().map(|c| c.to_string()).collect()).unwrap_or_default()
    }

    /// Feeds a key to the layer. `Some` is a key for the sidebar to act on.
    pub fn leader_key(&mut self, c: char) -> Option<FileKey> {
        let level = self.leader_level()?;
        match level.iter().find(|e| e.key == c) {
            Some(FileEntry { node: FileNode::Group(_), .. }) => {
                if let Some(path) = &mut self.leader {
                    path.push(c);
                }
                None
            }
            Some(FileEntry { node: FileNode::Leaf(p), .. }) if self.leader_applies(*p) => {
                let key = match *p {
                    FilePress::Key(k) | FilePress::OnFile(k) | FilePress::OnDir(k) => k,
                };
                self.leader = None;
                Some(key)
            }
            _ => {
                self.leader = None;
                None
            }
        }
    }
}

/// Reads one directory for the tree. Runs on a worker: this is the call that hangs
/// on a network mount.
///
/// Sorted directories-first then case-insensitively by name, which is the order
/// every file manager uses and the one people scan for.
pub fn read_dir(dir: &Path, show_hidden: bool) -> Vec<Entry> {
    let Ok(iter) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in iter.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // `metadata` follows symlinks (so a link to a directory reads as one, which
        // is what the row should say); `symlink_metadata` says whether it IS one.
        let link = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        let meta = std::fs::metadata(&path).ok();
        let dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let exec = meta.as_ref().map(is_executable).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(Entry { name, path, dir, link, exec, size, mtime });
    }
    sort_entries(&mut out);
    out
}

pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

// ---- what a file IS, and therefore what opening it means -------------------

/// What the sidebar decided a path is. The executable BIT is deliberately not one
/// of these: a script is text and runnable at once, so "what is it" and "may it be
/// run" are two questions, and collapsing them loses the "edit this script" case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Directory,
    Image,
    Text,
    Binary,
}

/// How much of a file is sniffed, and how much the viewer will load.
const SNIFF_BYTES: usize = 8192;
pub const VIEW_LIMIT: u64 = 4 * 1024 * 1024;

/// Decides what a path is by looking at it, not at its name.
///
/// Text vs binary is decided by CONTENT (a NUL byte in the first 8 KB): a log with
/// no extension is text and a `.dat` may well be. Images are the exception —
/// magic bytes first, extension only as a fallback for formats whose header this
/// does not know.
pub fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Directory;
    }
    let head = read_head(path);
    if is_image_magic(&head) || has_image_extension(path) {
        return Kind::Image;
    }
    if head.contains(&0) {
        return Kind::Binary;
    }
    Kind::Text
}

fn read_head(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let mut buf = vec![0u8; SNIFF_BYTES];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    buf
}

fn is_image_magic(head: &[u8]) -> bool {
    head.starts_with(b"\x89PNG\r\n\x1a\n")
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || head.starts_with(b"BM")
        || (head.len() > 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP")
        || head.starts_with(b"qoif")
}

fn has_image_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return false };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tif" | "tiff" | "avif" | "qoi"
    )
}

/// A `.desktop` file, which `xdg-open` EXECUTES. Never opened without a confirm
/// that names what would run: a cloned repository can carry one.
pub fn is_desktop(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
}

/// What a worker read for the viewer.
pub struct ViewRead {
    pub body: Result<crate::overlay::Viewed, String>,
    pub bytes: u64,
}

/// Reads a path for the viewer, on a worker. Never called on the UI thread: a file
/// on a network mount blocks for as long as the mount feels like it.
///
/// `cols`/`rows` size an image's half-block art, which has to be decided where the
/// cell size is known and is passed in rather than guessed here.
pub fn read_for_view(path: &Path, cols: usize, rows: usize, cell_aspect: f32) -> ViewRead {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let body = match kind_of(path) {
        Kind::Image => decode_image(path, cols, rows, cell_aspect),
        Kind::Binary => Err("binary file".to_string()),
        Kind::Directory => Err("that is a directory".to_string()),
        Kind::Text => read_text(path, bytes),
    };
    ViewRead { body, bytes }
}

fn read_text(path: &Path, bytes: u64) -> Result<crate::overlay::Viewed, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    // Read at most the limit, rather than checking the size and refusing: /proc and
    // other synthetic files report zero and still have content worth showing.
    let mut buf = Vec::new();
    let n = f
        .by_ref()
        .take(VIEW_LIMIT + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    let truncated = n as u64 > VIEW_LIMIT || bytes > VIEW_LIMIT;
    buf.truncate(VIEW_LIMIT as usize);
    // Lossy on purpose: a file that is UTF-8 apart from one bad byte is still a
    // file you want to read, and the viewer never writes anything back.
    let text = String::from_utf8_lossy(&buf).into_owned();
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    Ok(crate::overlay::Viewed::Text { lines, truncated })
}

fn decode_image(
    path: &Path,
    cols: usize,
    rows: usize,
    cell_aspect: f32,
) -> Result<crate::overlay::Viewed, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    // Fit inside the box keeping the aspect ON SCREEN, which is not the aspect in
    // cells: a cell here is 10x22 px, so a picture drawn as many rows as columns
    // comes out twice as tall as it is wide. `cell_aspect` is cw/ch and converts
    // between the two.
    let (mut c, mut r) = (cols, rows);
    if w > 0 && h > 0 {
        let want = (cols as f32 * h as f32 / w as f32 * cell_aspect).round().max(1.0) as usize;
        if want > rows {
            c = (rows as f32 * w as f32 / h as f32 / cell_aspect).round().max(1.0) as usize;
            r = rows;
        } else {
            r = want;
        }
    }
    let art = crate::media::halfblock_art(&rgba, w, h, c.max(1), r.max(1));
    Ok(crate::overlay::Viewed::Image { art, size: (w, h) })
}

// ---- properties and operations ---------------------------------------------

/// What the properties panel shows about one path. A snapshot, read on a worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Props {
    pub path: PathBuf,
    pub dir: bool,
    /// The path a symlink points at. Permissions apply to the TARGET, and the panel
    /// has to say so — a `chmod` on a link silently changes something else.
    pub link_target: Option<PathBuf>,
    pub size: u64,
    pub mtime: u64,
    /// Unix permission bits (the low 9), or `None` where the platform has none.
    pub mode: Option<u32>,
    pub readonly: bool,
    /// For a directory: how much is inside, counted on the worker that read this.
    /// The count is what a delete confirm has to name.
    pub contents: Option<(usize, usize)>,
}

/// Reads a path's properties. On a worker: counting a directory tree walks it.
pub fn props_of(path: &Path) -> Result<Props, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let link_target = meta.is_symlink().then(|| std::fs::read_link(path).ok()).flatten();
    // Everything but the link itself is asked of the TARGET, which is what opening
    // or chmod-ing the path would act on.
    let target = std::fs::metadata(path).ok();
    let dir = target.as_ref().map(|m| m.is_dir()).unwrap_or(false);
    Ok(Props {
        path: path.to_path_buf(),
        dir,
        link_target,
        size: target.as_ref().map(|m| m.len()).unwrap_or(0),
        mtime: target
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
        mode: target.as_ref().map(mode_of),
        readonly: target.as_ref().map(|m| m.permissions().readonly()).unwrap_or(false),
        contents: dir.then(|| count_tree(path)),
    })
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

/// Counts a tree as `(files, directories)`, not following symlinks. Bounded by
/// nothing but the tree: this is why it runs on a worker.
pub fn count_tree(root: &Path) -> (usize, usize) {
    let (mut files, mut dirs) = (0usize, 0usize);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(iter) = std::fs::read_dir(&dir) else { continue };
        for entry in iter.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                // A symlink is one entry, never a doorway: following them is how a
                // count (and a delete) walks out of the tree it was given.
                files += 1;
            } else if ft.is_dir() {
                dirs += 1;
                stack.push(entry.path());
            } else {
                files += 1;
            }
        }
    }
    (files, dirs)
}

/// Renames a path in place. Refuses to overwrite: a rename that silently replaces
/// another file is how work disappears with no undo anywhere.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = new_name.trim();
    check_name(name)?;
    let parent = path.parent().ok_or("that path has no parent")?;
    let target = parent.join(name);
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("{name} already exists"));
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Creates a file, or a directory when the name ends in `/`.
pub fn create(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let raw = name.trim();
    let is_dir = raw.ends_with('/');
    let name = raw.trim_end_matches('/');
    check_name(name)?;
    let target = parent.join(name);
    if target.exists() {
        return Err(format!("{name} already exists"));
    }
    if is_dir {
        std::fs::create_dir(&target).map_err(|e| e.to_string())?;
    } else {
        // `create_new` so a race cannot truncate a file that appeared meanwhile.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
    }
    Ok(target)
}

/// A name that is a name and not a path: no separators, no `..`, nothing empty.
/// Typing `../x` into a rename box must not move a file out of the tree.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a name is needed".into());
    }
    if name == "." || name == ".." {
        return Err("that is not a name".into());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("a name cannot contain a path separator".into());
    }
    Ok(())
}

/// Deletes a path. A directory needs `recursive`, which the caller only sets after
/// a confirm that counted what is inside.
pub fn delete(path: &Path, recursive: bool) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    // A symlink is removed as the link, never followed: deleting the link must not
    // delete what it points at.
    if meta.is_symlink() || !meta.is_dir() {
        return std::fs::remove_file(path).map_err(|e| e.to_string());
    }
    if recursive {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_dir(path).map_err(|e| e.to_string())
    }
}

/// Sets the permission bits, optionally through a whole tree.
///
/// On a symlink this changes the TARGET — `set_permissions` follows links and there
/// is no portable way not to — which is why the panel says so before it is used.
pub fn set_mode(path: &Path, mode: u32, recursive: bool) -> Result<usize, String> {
    #[cfg(not(unix))]
    {
        let _ = (path, mode, recursive);
        return Err("permissions are a unix thing".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut changed = 0usize;
        let mut stack = vec![path.to_path_buf()];
        while let Some(p) = stack.pop() {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("{}: {e}", p.display()))?;
            changed += 1;
            if !recursive {
                break;
            }
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if meta.is_dir() && !meta.is_symlink() {
                if let Ok(iter) = std::fs::read_dir(&p) {
                    stack.extend(iter.flatten().map(|e| e.path()));
                }
            }
        }
        Ok(changed)
    }
}

/// "2 files and 1 directory", agreeing with the counts. A confirm that says
/// "1 directories" reads as generated text, and generated text is what people stop
/// reading — which is the one thing a delete confirm cannot afford.
pub fn count_words(files: usize, dirs: usize) -> String {
    let f = format!("{files} file{}", if files == 1 { "" } else { "s" });
    let d = format!("{dirs} director{}", if dirs == 1 { "y" } else { "ies" });
    match (files, dirs) {
        (0, _) => d,
        (_, 0) => f,
        _ => format!("{f} and {d}"),
    }
}

/// `rwxr-xr-x`, the way `ls -l` writes it.
pub fn mode_string(mode: u32) -> String {
    let bit = |shift: u32, c: char| if mode >> shift & 1 == 1 { c } else { '-' };
    format!(
        "{}{}{}{}{}{}{}{}{}",
        bit(8, 'r'),
        bit(7, 'w'),
        bit(6, 'x'),
        bit(5, 'r'),
        bit(4, 'w'),
        bit(3, 'x'),
        bit(2, 'r'),
        bit(1, 'w'),
        bit(0, 'x')
    )
}

/// What git says about a tree, read on a worker: a badge per changed file and the
/// set of ignored paths, both absolute.
///
/// Absolute here rather than at the far end because git answers in paths relative to
/// the repository root, and the tree only ever holds absolute ones — converting in
/// one place is the difference between one join and a join at every comparison.
pub type GitMarks = (Vec<(PathBuf, Badge)>, HashSet<PathBuf>);

/// Reads both. Blocking (two `git` processes): worker only.
pub fn read_git(root: &Path) -> GitMarks {
    let files = crate::git::status_files(root)
        .into_iter()
        .map(|f| (root.join(&f.path), Badge::from_status(f.index, f.worktree)))
        .collect();
    let ignored = crate::git::ignored_paths(root).into_iter().map(|p| root.join(p)).collect();
    (files, ignored)
}

/// The tree's root for a directory: the repository it is in, else the directory
/// itself. A project is the unit people keep a tree of, and a repo is how a project
/// says where it ends.
pub fn root_for(dir: &Path) -> PathBuf {
    crate::git::repo_root(dir).unwrap_or_else(|| dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, dir: bool) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from("/r").join(name),
            dir,
            link: false,
            exec: false,
            size: 0,
            mtime: 0,
        }
    }

    #[test]
    fn directories_sort_first_then_case_insensitively() {
        let mut v = vec![
            entry("zebra.rs", false),
            entry("Alpha", true),
            entry("beta.rs", false),
            entry("charlie", true),
        ];
        sort_entries(&mut v);
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "charlie", "beta.rs", "zebra.rs"]);
    }

    #[test]
    fn the_tree_only_shows_what_is_expanded() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(
            PathBuf::from("/r"),
            vec![entry("src", true), entry("main.rs", false)],
        );
        assert_eq!(e.rows.len(), 2, "the root's children, and nothing below them");

        // Opening a directory asks for a read; its rows appear when the read lands.
        assert!(e.toggle(Path::new("/r/src")), "an unread directory needs a read");
        assert!(
            e.needs_read(Path::new("/r/src")),
            "toggle must not mark it in flight — the spawn does, and it would skip \
             a directory that already looked busy"
        );
        assert_eq!(e.rows.len(), 2, "and shows nothing until it arrives");
        e.insert_children(PathBuf::from("/r/src"), vec![entry("lib.rs", false)]);
        assert_eq!(e.rows.len(), 3);
        assert_eq!(e.rows[1].depth, 1, "a child is drawn one level in");

        // Folding it away does not need a second read.
        assert!(!e.toggle(Path::new("/r/src")));
        assert_eq!(e.rows.len(), 2);
        assert!(!e.toggle(Path::new("/r/src")), "and reopening reuses what was read");
    }

    #[test]
    fn a_rebuild_keeps_the_cursor_on_the_same_file() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(
            PathBuf::from("/r"),
            vec![entry("a", true), entry("b.rs", false), entry("c.rs", false)],
        );
        e.set_cursor(2, 20);
        assert_eq!(e.selected().unwrap().entry.name, "c.rs");

        // A directory above it finishes loading and inserts rows: the selection has
        // to follow the FILE, not the index it happened to be at.
        e.toggle(Path::new("/r/a"));
        e.insert_children(PathBuf::from("/r/a"), vec![entry("x.rs", false)]);
        assert_eq!(e.selected().unwrap().entry.name, "c.rs");
    }

    #[test]
    fn a_capped_directory_says_how_much_it_is_not_showing() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        let many: Vec<Entry> = (0..CHILD_CAP + 7).map(|i| entry(&format!("f{i}"), false)).collect();
        e.insert_children(PathBuf::from("/r"), many);
        assert_eq!(e.rows.len(), CHILD_CAP + 1);
        assert_eq!(e.rows.last().unwrap().more, Some(7));
    }

    #[test]
    fn what_a_file_is_comes_from_its_bytes_not_its_name() {
        let dir = std::env::temp_dir().join(format!("runnir-kind-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let write = |name: &str, bytes: &[u8]| {
            let p = dir.join(name);
            std::fs::write(&p, bytes).unwrap();
            p
        };

        // No extension at all, and still text.
        assert_eq!(kind_of(&write("logfile", b"2026-07-21 started\nok\n")), Kind::Text);
        // A `.dat` that happens to be text is text.
        assert_eq!(kind_of(&write("readings.dat", b"1,2,3\n4,5,6\n")), Kind::Text);
        // A NUL in the first block makes it binary, whatever it is called.
        assert_eq!(kind_of(&write("notes.txt", b"header\0\x01\x02rest")), Kind::Binary);
        // An image is caught by its magic bytes even with the wrong extension.
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        assert_eq!(kind_of(&write("screenshot.txt", &png)), Kind::Image);
        // ...and by extension when the header is one this does not know.
        assert_eq!(kind_of(&write("art.avif", b"\0\0\0 ftypavif")), Kind::Image);
        assert_eq!(kind_of(&dir), Kind::Directory);

        assert!(is_desktop(Path::new("/usr/share/applications/x.desktop")));
        assert!(!is_desktop(Path::new("/tmp/x.desktopish")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_viewer_reads_text_and_says_when_it_stopped() {
        let dir = std::env::temp_dir().join(format!("runnir-view-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let small = dir.join("small.txt");
        std::fs::write(&small, "one\ntwo\nthree\n").unwrap();
        let read = read_for_view(&small, 40, 20, 0.45);
        match read.body.expect("text") {
            crate::overlay::Viewed::Text { lines, truncated } => {
                assert_eq!(lines, ["one", "two", "three"]);
                assert!(!truncated);
            }
            _ => panic!("a text file reads as text"),
        }
        assert_eq!(read.bytes, 14);

        // Past the limit the viewer stops and SAYS it stopped: a file that just ends
        // early, silently, is a file you draw the wrong conclusion from.
        let big = dir.join("big.txt");
        std::fs::write(&big, "x".repeat(VIEW_LIMIT as usize + 100)).unwrap();
        match read_for_view(&big, 40, 20, 0.45).body.expect("text") {
            crate::overlay::Viewed::Text { truncated, .. } => assert!(truncated),
            _ => panic!("still text"),
        }

        // A binary is refused rather than shown as mojibake.
        let bin = dir.join("thing.bin");
        std::fs::write(&bin, [0u8, 1, 2, 3]).unwrap();
        assert!(read_for_view(&bin, 40, 20, 0.45).body.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn operations_refuse_the_things_that_lose_work() {
        let dir = std::env::temp_dir().join(format!("runnir-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();

        // A rename over an existing file is refused: there is no undo anywhere for
        // the file it would have replaced.
        assert!(rename(&a, "b.txt").is_err());
        // A name is a name: a rename box must not be able to move a file out of the
        // tree, and `..` is how that would be done.
        assert!(rename(&a, "../escaped.txt").is_err());
        assert!(rename(&a, "..").is_err());
        assert!(rename(&a, "  ").is_err());
        let moved = rename(&a, "renamed.txt").unwrap();
        assert!(moved.exists() && !a.exists());

        // Creating: a trailing slash means a directory, and neither overwrites.
        let sub = create(&dir, "sub/").unwrap();
        assert!(sub.is_dir());
        let made = create(&dir, "new.txt").unwrap();
        assert!(made.is_file());
        assert!(create(&dir, "new.txt").is_err(), "an existing name is refused");

        // Counting is what a delete confirm names, and it does not follow links.
        std::fs::write(sub.join("inner.txt"), "x").unwrap();
        std::fs::create_dir(sub.join("deeper")).unwrap();
        std::fs::write(sub.join("deeper/deep.txt"), "x").unwrap();
        assert_eq!(count_tree(&sub), (2, 1));

        // A non-empty directory is not deleted without the recursive flag the
        // confirm sets.
        assert!(delete(&sub, false).is_err());
        assert!(delete(&sub, true).is_ok());
        assert!(!sub.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_confirm_counts_in_words_that_agree() {
        assert_eq!(count_words(2, 1), "2 files and 1 directory");
        assert_eq!(count_words(1, 3), "1 file and 3 directories");
        assert_eq!(count_words(4, 0), "4 files");
        assert_eq!(count_words(0, 1), "1 directory");
    }

    #[cfg(unix)]
    #[test]
    fn permissions_read_back_the_way_they_were_written() {
        let dir = std::env::temp_dir().join(format!("runnir-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("script.sh");
        std::fs::write(&f, "#!/bin/sh\n").unwrap();

        set_mode(&f, 0o644, false).unwrap();
        assert_eq!(props_of(&f).unwrap().mode.unwrap() & 0o777, 0o644);
        assert_eq!(mode_string(0o644), "rw-r--r--");

        set_mode(&f, 0o755, false).unwrap();
        assert_eq!(mode_string(props_of(&f).unwrap().mode.unwrap() & 0o777), "rwxr-xr-x");
        assert_eq!(kind_of(&f), Kind::Text, "the execute bit is not a file type");

        // Recursive touches everything under the directory, and says how much.
        std::fs::write(dir.join("one"), "1").unwrap();
        std::fs::create_dir(dir.join("d")).unwrap();
        std::fs::write(dir.join("d/two"), "2").unwrap();
        let n = set_mode(&dir, 0o755, true).unwrap();
        assert_eq!(n, 5, "the directory, script.sh, one, d and d/two");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sidebar_leader_only_offers_what_this_row_can_do() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(PathBuf::from("/r"), vec![entry("src", true), entry("main.rs", false)]);

        // On a directory: the Directory group's verbs, not the File group's.
        e.set_cursor(0, 20);
        e.arm_leader();
        assert!(e.leader_entries().iter().any(|(k, _, g)| k == "d" && *g));
        e.leader_key('f');
        assert!(
            !e.leader_entries().iter().any(|(k, _, _)| k == "o"),
            "\"open it\" is a file verb: {:?}",
            e.leader_entries()
        );
        assert_eq!(e.leader_key('o'), None, "and pressing it ends the sequence");
        assert!(e.leader.is_none());

        // On a file, the same group offers it, and it presses the key the sidebar
        // already binds for opening a row.
        e.set_cursor(1, 20);
        e.arm_leader();
        e.leader_key('f');
        assert!(e.leader_entries().iter().any(|(k, _, _)| k == "o"));
        assert_eq!(e.leader_key('o'), Some(FileKey::Enter));

        // A verb that stands on its own works from either row.
        e.set_cursor(0, 20);
        e.arm_leader();
        e.leader_key('v');
        assert_eq!(e.leader_key('.'), Some(FileKey::Ch('.')));
    }

    fn at(name: &str, dir: bool, mtime: u64) -> Entry {
        Entry { mtime, ..entry(name, dir) }
    }

    #[test]
    fn a_status_letter_becomes_the_badge_the_row_shows() {
        // Untracked and conflicted come back in BOTH columns; git says so with `?`
        // and `U`, and neither is a worktree letter to be read as one.
        assert_eq!(Badge::from_status('?', '?'), Badge::Untracked);
        assert_eq!(Badge::from_status('U', 'U'), Badge::Conflict);
        assert_eq!(Badge::from_status('M', 'U'), Badge::Conflict);
        // The unstaged letter wins over the staged one: it is the change that is not
        // written down anywhere yet.
        assert_eq!(Badge::from_status('M', 'M'), Badge::Modified);
        assert_eq!(Badge::from_status('M', '.'), Badge::Staged);
        assert_eq!(Badge::from_status('A', '.'), Badge::Added);
        assert_eq!(Badge::from_status('.', 'D'), Badge::Deleted);
        assert_eq!(Badge::from_status('R', '.'), Badge::Staged);
        assert_eq!(Badge::Dirty.letter(), '\u{b7}');
    }

    #[test]
    fn a_directory_says_something_below_it_changed_and_never_borrows_a_letter() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(PathBuf::from("/r"), vec![entry("src", true), entry("README", false)]);
        e.toggle(Path::new("/r/src"));
        e.insert_children(
            PathBuf::from("/r/src"),
            vec![Entry { path: "/r/src/main.rs".into(), ..entry("main.rs", false) }],
        );

        e.set_git(vec![(PathBuf::from("/r/src/main.rs"), Badge::Modified)], HashSet::new());
        fn row(e: &Explorer, name: &str) -> Row {
            e.rows.iter().find(|r| r.entry.name == name).unwrap().clone()
        }
        assert_eq!(row(&e, "main.rs").badge, Some(Badge::Modified));
        assert_eq!(row(&e, "src").badge, Some(Badge::Dirty), "a folder is not itself modified");
        assert_eq!(row(&e, "README").badge, None);

        // A conflict is the one state a directory repeats, because it is the one
        // where a stray keystroke loses a merge.
        e.set_git(vec![(PathBuf::from("/r/src/main.rs"), Badge::Conflict)], HashSet::new());
        assert_eq!(row(&e, "src").badge, Some(Badge::Conflict));

        // Moving to another repository drops the marks: badging the new tree with the
        // old project's status is worse than badging nothing.
        e.set_root(PathBuf::from("/other"));
        assert!(e.git.is_empty() && e.git_dirs.is_empty() && e.ignored.is_empty());
    }

    #[test]
    fn what_git_ignores_is_hidden_but_never_silently() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(
            PathBuf::from("/r"),
            vec![entry("target", true), entry("src", true), entry("Cargo.toml", false)],
        );
        // git collapses an ignored directory to one line, so the tree has to test a
        // path against its ANCESTORS, not just against itself.
        e.set_git(Vec::new(), HashSet::from([PathBuf::from("/r/target")]));
        assert!(e.is_ignored(Path::new("/r/target/debug/runnir")));
        assert!(!e.is_ignored(Path::new("/r/src/main.rs")));

        let names: Vec<&str> = e.rows.iter().map(|r| r.entry.name.as_str()).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);
        assert_eq!(e.hidden_by_ignore, 1, "and the footer can say how many");

        e.show_ignored = true;
        e.rebuild();
        assert_eq!(e.rows.len(), 3);
        assert!(e.rows[0].ignored, "shown, but dimmed as what it is");
        assert_eq!(e.hidden_by_ignore, 0);
    }

    #[test]
    fn sorting_by_date_puts_the_newest_first_whatever_it_is() {
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        e.insert_children(
            PathBuf::from("/r"),
            vec![at("old_dir", true, 10), at("new.rs", false, 300), at("mid.rs", false, 200)],
        );
        // By name: directories first, as every file manager does it.
        let names = |e: &Explorer| -> Vec<String> {
            e.rows.iter().map(|r| r.entry.name.clone()).collect()
        };
        assert_eq!(names(&e), ["old_dir", "new.rs", "mid.rs"]);

        // By date: the directory sinks. Mixing them is the point — a directory's own
        // mtime moves when something is created or deleted in it, which is exactly
        // the event this mode exists to surface.
        e.sort = Sort::Mtime;
        e.rebuild();
        assert_eq!(names(&e), ["new.rs", "mid.rs", "old_dir"]);
        assert_eq!(e.sort.flip(), Sort::Name);
    }

    #[test]
    fn the_sidebar_takes_its_columns_off_the_side_it_is_on() {
        let area = Rect { x: 0.0, y: 20.0, w: 1000.0, h: 500.0 };
        let cell = (10.0, 20.0);
        let mut e = Explorer::new(PathBuf::from("/r"), 30, Side::Left);
        assert_eq!(e.rect(area, cell).x, 0.0);
        assert_eq!(e.reserve(area, cell).x, 300.0);
        assert_eq!(e.reserve(area, cell).w, 700.0);

        e.side = Side::Right;
        assert_eq!(e.rect(area, cell).x, 700.0);
        assert_eq!(e.reserve(area, cell).x, 0.0);
        assert_eq!(e.reserve(area, cell).w, 700.0);

        // Closed, it takes nothing at all.
        e.open = false;
        assert_eq!(e.reserve(area, cell).w, area.w);

        // A window too narrow for the configured width still leaves room to work in.
        e.open = true;
        e.width = 400;
        let narrow = Rect { w: 500.0, ..area };
        assert!(e.reserve(narrow, cell).w >= 100.0, "the panes keep something");
    }
}
