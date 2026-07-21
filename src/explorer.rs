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
    /// Read generation: an answer tagged with an older one is dropped, so a slow
    /// `read_dir` cannot land after the tree moved on.
    pub seq: u64,
    pub message: Option<String>,
    /// The sidebar's own leader layer: `None` when disarmed, else the group keys
    /// pressed so far. Same shape, and the same which-key, as the git panel's.
    pub leader: Option<Vec<char>>,
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
            seq: 0,
            message: None,
            leader: None,
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
        self.walk(&self.root.clone(), 0, &mut rows);
        self.rows = rows;
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

    fn walk(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let Some(entries) = self.children.get(dir) else { return };
        let shown = entries.len().min(CHILD_CAP);
        for entry in &entries[..shown] {
            let open = entry.dir && self.expanded.contains(&entry.path);
            out.push(Row { entry: entry.clone(), depth, open, more: None });
            if open {
                self.walk(&entry.path.clone(), depth + 1, out);
            }
        }
        if entries.len() > shown {
            let n = entries.len() - shown;
            let entry = Entry {
                name: format!("\u{2026} {n} more"),
                path: dir.join(format!("\u{2026}{n}")),
                dir: false,
                link: false,
                exec: false,
                size: 0,
                mtime: 0,
            };
            out.push(Row { entry, depth, open: false, more: Some(n) });
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
            fleaf('y', "copy the path", FKey(FCh('y'))),
        ]),
    },
    FileEntry {
        key: 'd',
        title: "Directory",
        node: FileNode::Group(&[
            fleaf('o', "fold / unfold", OnDir(FEnter)),
            fleaf('u', "go to the parent", FKey(FCh('h'))),
            fleaf('y', "copy the path", FKey(FCh('y'))),
        ]),
    },
    FileEntry {
        key: 'v',
        title: "View",
        node: FileNode::Group(&[
            fleaf('.', "hidden files", FKey(FCh('.'))),
            fleaf('r', "reread the tree", FKey(FCh('r'))),
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
