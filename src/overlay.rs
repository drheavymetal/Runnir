//! Overlays: the command palette, the docs viewer, the rename prompt, the AI
//! panel, hint mode.
//!
//! Each overlay renders as an ordinary `Grid` drawn into a centered rect — the
//! renderer already draws grids, so overlays reuse that path instead of inventing
//! a second UI system. An overlay builds its grid on demand from its state.

use crate::actions::Action;
use crate::config::{SnippetDef, Theme};
use crate::grid::{Cell, Color, Flags, Grid, Pen};
use crate::media::{HalfCell, NowPlaying};

/// Which overlay, if any, is capturing input. Only one is active at a time.
pub enum Overlay {
    Palette(Palette),
    Docs(Docs),
    Prompt(Prompt),
    Ai(AiPanel),
    Hints(Hints),
    Search(Search),
    Config(ConfigPanel),
    Theme(ThemePicker),
    Snippets(SnippetPicker),
    ClipHistory(ClipHistoryPicker),
    Media(MediaOverlay),
    /// The native git panel: status, log, branches, stashes.
    Git(GitPanel),
    /// The native docker panel: hosts, objects, detail.
    Docker(DockerPanel),
    /// A file being read from the explorer sidebar: text or an image, never edited.
    Viewer(FileViewer),
    /// One path's properties, with its permission bits editable.
    Props(PropsPanel),
}

impl Overlay {
    /// Grids to draw for this overlay, each with its `(col, row)` cell origin
    /// inside a grid of `cols` x `rows`. The caller turns cell origins into pixels.
    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        match self {
            Overlay::Palette(p) => p.render(cols, rows, theme),
            Overlay::Docs(d) => d.render(cols, rows, theme),
            Overlay::Prompt(p) => p.render(cols, rows, theme),
            Overlay::Ai(a) => a.render(cols, rows, theme),
            Overlay::Hints(_) => Vec::new(), // Hints annotate panes, drawn elsewhere.
            Overlay::Search(s) => s.render(cols, rows, theme),
            Overlay::Config(c) => c.render(cols, rows, theme),
            Overlay::Theme(t) => t.render(cols, rows, theme),
            Overlay::Snippets(s) => s.render(cols, rows, theme),
            Overlay::ClipHistory(p) => p.render(cols, rows, theme),
            Overlay::Media(m) => m.render(cols, rows, theme),
            Overlay::Git(p) => p.render(cols, rows, theme),
            Overlay::Docker(p) => p.render(cols, rows, theme),
            Overlay::Viewer(v) => v.render(cols, rows, theme),
            Overlay::Props(p) => p.render(cols, rows, theme),
        }
    }
}

/// Incremental scrollback search. The matches themselves are highlighted in the
/// pane by the renderer; this overlay is the little query bar at the bottom.
pub struct Search {
    pub query: String,
    /// Absolute `(row, col)` of each match, in order.
    pub matches: Vec<(usize, usize)>,
    pub current: usize,
}

impl Search {
    pub fn new() -> Self {
        Self { query: String::new(), matches: Vec::new(), current: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn set_matches(&mut self, matches: Vec<(usize, usize)>) {
        self.matches = matches;
        self.current = 0;
    }

    pub fn next(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + 1) % self.matches.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        }
    }

    pub fn current_match(&self) -> Option<(usize, usize)> {
        self.matches.get(self.current).copied()
    }

    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.min(60).max(20);
        let mut g = panel_grid(w, 1, theme);
        let count = if self.matches.is_empty() {
            if self.query.is_empty() { String::new() } else { " no matches".into() }
        } else {
            format!(" {}/{}", self.current + 1, self.matches.len())
        };
        // Same rule as the prompt: the tail stays visible, because that is where
        // the caret is.
        let field = w.saturating_sub(count.chars().count() + 4);
        let line = format!("/{}", field_view(&self.query, field));
        write(&mut g, 0, 1, &line, normal());
        write(&mut g, 0, 1 + line.chars().count(), " ", selected());
        write(&mut g, 0, w.saturating_sub(count.chars().count() + 1), &count, dim());
        // Anchored to the bottom row, vim-style.
        vec![Panel { grid: g, col: 0, row: rows.saturating_sub(1) }]
    }
}

// ---- git panel -------------------------------------------------------------

/// Which list the git panel is showing. The preview pane on the right always shows
/// what the selection in the current list means: a file's diff, a commit's diff, a
/// branch's log, a stash's contents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitView {
    Status,
    Log,
    Branches,
    Stashes,
    Tags,
    /// Every position HEAD has held — the undo history for everything the panel
    /// deliberately refuses to bind.
    Reflog,
    Worktrees,
    /// Blame for one file: every line, with the commit that last touched it.
    /// Reached from the status view rather than by number, because it is about a
    /// file rather than about the repository.
    Blame,
}

impl GitView {
    pub fn title(self) -> &'static str {
        match self {
            GitView::Status => "status",
            GitView::Log => "log",
            GitView::Branches => "branches",
            GitView::Stashes => "stashes",
            GitView::Tags => "tags",
            GitView::Reflog => "reflog",
            GitView::Worktrees => "worktrees",
            GitView::Blame => "blame",
        }
    }

    pub fn next(self) -> Self {
        match self {
            GitView::Status => GitView::Log,
            GitView::Log => GitView::Branches,
            GitView::Branches => GitView::Stashes,
            GitView::Stashes => GitView::Tags,
            GitView::Tags => GitView::Reflog,
            GitView::Reflog => GitView::Worktrees,
            GitView::Worktrees => GitView::Status,
            // Blame is not part of the cycle: you enter it from a file and leave it
            // with Escape, like the commit drill-down.
            GitView::Blame => GitView::Status,
        }
    }
}

/// Which of the panel's columns the keyboard drives.
///
/// The columns are a hierarchy — a list, the files of what it selects, the diff of
/// the selected file — so `h`/`l` walk them the way they read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitFocus {
    List,
    /// The files of an open commit. Only reachable while `open_commit` is set.
    Files,
    Diff,
}

/// The narrowest a column may be dragged to. Below this a path column shows nothing
/// but ellipses, which is not a column, it is a hint that one is missing.
const MIN_COL: usize = 12;
/// The diff keeps more, because a diff clipped to nothing is the thing you opened
/// the panel to read.
const MIN_DIFF: usize = 20;

/// The native git panel: a list on the left, the selection's diff on the right.
///
/// It holds only DATA. Every read and every command runs on a worker and comes back
/// through `UserEvent::Git*`, so a slow `git push` cannot freeze the terminal it is
/// pushing from.
pub struct GitPanel {
    pub root: std::path::PathBuf,
    pub view: GitView,
    pub files: Vec<crate::git::FileEntry>,
    pub log: Vec<crate::git::Commit>,
    pub branches: Vec<String>,
    /// Remote-tracking branches, listed after the local ones in the same view: they
    /// are the same kind of thing, and splitting them into two views would mean
    /// switching views to answer "is my branch on the remote".
    pub remotes: Vec<String>,
    pub stashes: Vec<String>,
    pub tags: Vec<String>,
    pub reflog: Vec<crate::git::Commit>,
    pub worktrees: Vec<String>,
    /// Blame rows for `blame_path`, when the blame view is up.
    pub blame: Vec<crate::git::BlameLine>,
    pub blame_path: String,
    /// Which column the keyboard is driving. Line-level staging needs a cursor
    /// INSIDE the diff, and one set of j/k cannot mean two things.
    pub focus: GitFocus,
    /// Where the two column separators sit, as fractions of the panel's width.
    /// Dragged with the mouse; `split[1]` only matters while a commit is open.
    pub split: [f32; 2],
    /// One file's diff, filling the whole panel. A three-column layout is for
    /// finding the change; reading it wants the width back.
    pub zoom: bool,
    /// Cursor and selection anchor within `preview_rows`, for staging by line.
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    /// An interactive rebase being planned: one step per commit, oldest LAST here
    /// (the order the log shows them) and reversed when the todo is written.
    pub rebase: Option<RebasePlan>,
    pub current_branch: String,
    /// Selection per view, kept apart so switching back lands where you left off.
    cursors: [usize; 8],
    /// In the status view, whether the preview shows the STAGED diff. A file can be
    /// both staged and modified, and those are two different diffs of one path.
    pub show_staged: bool,
    /// Message filter for the log view, shown in the header so a narrowed list can
    /// never be mistaken for the whole history.
    pub log_filter: String,
    /// The commit whose FILES the list is showing, if we drilled into one. A commit
    /// of forty files read as one scrolling diff is a wall; the question is nearly
    /// always what it did to one file.
    pub open_commit: Option<String>,
    pub commit_files: Vec<crate::git::FileEntry>,
    pub commit_cursor: usize,
    pub preview: String,
    /// The preview, parsed into numbered diff rows. Kept beside the text so the
    /// draw path never re-parses on every frame.
    pub preview_rows: Vec<crate::git::DiffRow>,
    pub preview_scroll: usize,
    /// Which hunk of the preview is selected, for partial staging. Kept as an index
    /// into `hunk_ranges`, not a row, so it survives the preview being reparsed.
    pub hunk: usize,
    /// The last command's result, shown in the footer; `Err` is drawn red.
    pub message: Result<String, String>,
    /// A command is in flight: the footer says so and the keys that start another
    /// are ignored, so a double tap cannot fire two pushes.
    pub busy: bool,
    /// The panel's own leader layer: `None` when disarmed, else the group keys
    /// pressed so far (empty at the root).
    pub leader: Option<Vec<char>>,
}

impl GitPanel {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            root,
            view: GitView::Status,
            files: Vec::new(),
            log: Vec::new(),
            branches: Vec::new(),
            remotes: Vec::new(),
            stashes: Vec::new(),
            tags: Vec::new(),
            reflog: Vec::new(),
            worktrees: Vec::new(),
            blame: Vec::new(),
            blame_path: String::new(),
            focus: GitFocus::List,
            split: [0.34, 0.58],
            zoom: false,
            diff_cursor: 0,
            diff_anchor: None,
            rebase: None,
            current_branch: String::new(),
            cursors: [0; 8],
            show_staged: false,
            log_filter: String::new(),
            open_commit: None,
            commit_files: Vec::new(),
            commit_cursor: 0,
            preview: String::new(),
            preview_rows: Vec::new(),
            preview_scroll: 0,
            hunk: 0,
            message: Ok(String::new()),
            busy: false,
            leader: None,
        }
    }

    /// Arms the panel's leader layer at the root.
    pub fn arm_leader(&mut self) {
        self.leader = Some(Vec::new());
    }

    pub fn cancel_leader(&mut self) {
        self.leader = None;
    }

    /// The level the leader layer is at now, or `None` when it is disarmed.
    fn leader_level(&self) -> Option<&'static [GitEntry]> {
        let path = self.leader.as_ref()?;
        let mut level: &'static [GitEntry] = GIT_LEADER;
        for key in path {
            match level.iter().find(|e| e.key == *key) {
                Some(GitEntry { node: GitNode::Group(next), .. }) => level = next,
                // A path that no longer resolves (the tree changed under it) shows
                // the root rather than nothing at all.
                _ => return Some(GIT_LEADER),
            }
        }
        Some(level)
    }

    /// Whether a leaf can act on what is on screen right now. A verb bound to
    /// another view's selection is not offered from here: showing it and then doing
    /// nothing is worse than not showing it.
    fn leader_applies(&self, press: GitPress) -> bool {
        match press {
            GitPress::In(v, _) => self.view == v && !self.in_commit(),
            GitPress::InDiff(_) => self.diff_focus(),
            _ => true,
        }
    }

    /// What this level offers, as `(key, title, is_group)` — the shape the which-key
    /// panel already draws for the global leader.
    pub fn leader_entries(&self) -> Vec<(String, String, bool)> {
        let Some(level) = self.leader_level() else { return Vec::new() };
        let mut out: Vec<(String, String, bool)> = level
            .iter()
            .filter(|e| match &e.node {
                GitNode::Leaf(p) => self.leader_applies(*p),
                GitNode::Group(_) => true,
            })
            .map(|e| {
                let key = if e.key == ' ' { "space".to_string() } else { e.key.to_string() };
                (key, e.title.to_string(), matches!(e.node, GitNode::Group(_)))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The keys pressed since the layer was armed, for the which-key header.
    pub fn leader_path(&self) -> Vec<String> {
        self.leader
            .as_ref()
            .map(|p| p.iter().map(|c| c.to_string()).collect())
            .unwrap_or_default()
    }

    /// Feeds a key to the leader layer. `Some` is a verb to run; `None` means the
    /// layer either descended into a group or gave up — either way the key is
    /// consumed, because falling through would fire an unrelated panel binding
    /// after a mistyped sequence.
    pub fn leader_key(&mut self, c: char) -> Option<GitPress> {
        let level = self.leader_level()?;
        match level.iter().find(|e| e.key == c) {
            Some(GitEntry { node: GitNode::Group(_), .. }) => {
                if let Some(path) = &mut self.leader {
                    path.push(c);
                }
                None
            }
            Some(GitEntry { node: GitNode::Leaf(p), .. }) if self.leader_applies(*p) => {
                let press = *p;
                self.leader = None;
                Some(press)
            }
            // A miss, or a verb this view cannot do: end the sequence rather than
            // leave the layer armed on a key the user thinks did something.
            _ => {
                self.leader = None;
                None
            }
        }
    }

    fn view_index(&self) -> usize {
        match self.view {
            GitView::Status => 0,
            GitView::Log => 1,
            GitView::Branches => 2,
            GitView::Stashes => 3,
            GitView::Tags => 4,
            GitView::Reflog => 5,
            GitView::Worktrees => 6,
            GitView::Blame => 7,
        }
    }

    /// Where the panel's parts sit, in cells. Shared by the renderer and the mouse,
    /// so a click can never land somewhere other than what it looks like it hit.
    ///
    /// Two columns normally; three while a commit is open (list, its files, the
    /// selected file's diff); one while zoomed. The widths come from `split`, which
    /// the mouse drags, and are clamped here rather than at drag time so a window
    /// resize can never leave a column at zero.
    pub fn layout(&self, cols: usize, rows: usize) -> GitLayout {
        let w = cols.saturating_sub(4).max(40);
        let h = rows.saturating_sub(4).max(12);
        let body_rows = h.saturating_sub(3);
        let base = GitLayout { col: 2, row: 2, w, h, list_w: 0, files_w: 0, body_rows };
        // Zoom gives the whole box to the diff: no columns, nothing to hit but it.
        if self.zoom {
            return base;
        }
        let at = |f: f32| (w as f32 * f).round() as usize;
        // Three columns need room for both minimums plus the diff's; a window too
        // narrow for that drops the FILE column rather than drawing three columns
        // that are each less than the panel promises. The commit stays open — only
        // its column is gone, and the diff it feeds is still what you came to read.
        let three = self.in_commit() && w >= MIN_COL * 2 + MIN_DIFF + 4;
        if !three {
            // Two columns: one separator, and the diff takes the rest.
            let list_w = at(self.split[0]).clamp(MIN_COL, w.saturating_sub(MIN_DIFF + 2).max(MIN_COL));
            return GitLayout { list_w, ..base };
        }
        // Three. The list is clamped first, then the files column against what the
        // list left, so dragging one never squeezes the diff out of existence.
        let room = w.saturating_sub(MIN_DIFF + MIN_COL + 4).max(MIN_COL);
        let list_w = at(self.split[0]).clamp(MIN_COL, room);
        let files_w = at(self.split[1])
            .saturating_sub(list_w + 2)
            .clamp(MIN_COL, w.saturating_sub(list_w + MIN_DIFF + 4).max(MIN_COL));
        GitLayout { list_w, files_w, ..base }
    }

    /// Whether the list is showing one commit's files rather than the view's own
    /// list.
    pub fn in_commit(&self) -> bool {
        self.open_commit.is_some()
    }

    /// How many rows the LIST column holds. An open commit does not change this:
    /// its files are a column of their own, and the list stays where it was.
    pub fn len(&self) -> usize {
        match self.view {
            GitView::Status => self.files.len(),
            GitView::Log => self.log.len(),
            GitView::Branches => self.branches.len() + self.remotes.len(),
            GitView::Stashes => self.stashes.len(),
            GitView::Tags => self.tags.len(),
            GitView::Reflog => self.reflog.len(),
            GitView::Worktrees => self.worktrees.len(),
            GitView::Blame => self.blame.len(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursors[self.view_index()].min(self.len().saturating_sub(1))
    }

    pub fn set_cursor(&mut self, n: usize) {
        let n = n.min(self.len().saturating_sub(1));
        let i = self.view_index();
        let moved = self.cursors[i] != n;
        self.cursors[i] = n;
        self.preview_scroll = 0;
        // Moving the list closes an open commit: its file column belongs to the row
        // you just left, and a column of another commit's files beside a different
        // selection is a lie the panel would keep telling until Escape.
        if moved && self.in_commit() {
            self.leave_commit();
        }
    }

    pub fn files_len(&self) -> usize {
        self.commit_files.len()
    }

    pub fn files_cursor(&self) -> usize {
        self.commit_cursor.min(self.files_len().saturating_sub(1))
    }

    pub fn set_files_cursor(&mut self, n: usize) {
        self.commit_cursor = n.min(self.files_len().saturating_sub(1));
        self.preview_scroll = 0;
    }

    /// Opens a commit: its files appear in a column of their own, beside the list
    /// that selected it, and the keyboard moves there. `leave_commit` backs out,
    /// which is also what Escape does before it will close the panel.
    pub fn enter_commit(&mut self, sha: String) {
        self.open_commit = Some(sha);
        self.commit_files.clear();
        self.commit_cursor = 0;
        self.preview_scroll = 0;
        // A zoom carried in from the previous selection would hide the very column
        // this opens, and the keyboard would be driving one nobody can see.
        self.zoom = false;
        self.focus = GitFocus::Files;
    }

    pub fn leave_commit(&mut self) -> bool {
        let was = self.open_commit.take().is_some();
        self.commit_files.clear();
        self.preview_scroll = 0;
        self.zoom = false;
        self.focus = GitFocus::List;
        was
    }

    /// The file selected inside an open commit.
    pub fn selected_commit_file(&self) -> Option<&crate::git::FileEntry> {
        self.commit_files.get(self.files_cursor())
    }

    /// Whether the keyboard is inside the diff, where staging acts on lines.
    pub fn diff_focus(&self) -> bool {
        self.focus == GitFocus::Diff
    }

    /// Moves the cursor of whichever column has the keyboard.
    pub fn down(&mut self) {
        match self.focus {
            GitFocus::Files => {
                let c = self.files_cursor();
                if c + 1 < self.files_len() {
                    self.set_files_cursor(c + 1);
                }
            }
            _ => {
                let c = self.cursor();
                if c + 1 < self.len() {
                    self.set_cursor(c + 1);
                }
            }
        }
    }

    pub fn up(&mut self) {
        match self.focus {
            GitFocus::Files => {
                let c = self.files_cursor();
                self.set_files_cursor(c.saturating_sub(1));
            }
            _ => {
                let c = self.cursor();
                self.set_cursor(c.saturating_sub(1));
            }
        }
    }

    /// Walks the columns left/right. The columns are a hierarchy, so this is the
    /// same motion as drilling in and backing out — `h` from the diff of an open
    /// commit lands on its files, not on the list behind them.
    pub fn focus_right(&mut self) {
        self.focus = match self.focus {
            GitFocus::List if self.in_commit() => GitFocus::Files,
            GitFocus::List => GitFocus::Diff,
            GitFocus::Files => GitFocus::Diff,
            GitFocus::Diff => GitFocus::Diff,
        };
        if self.focus == GitFocus::Diff {
            self.enter_diff();
        }
    }

    pub fn focus_left(&mut self) {
        if self.focus == GitFocus::Diff {
            self.diff_anchor = None;
        }
        self.focus = match self.focus {
            GitFocus::Diff if self.in_commit() => GitFocus::Files,
            GitFocus::Diff => GitFocus::List,
            GitFocus::Files => GitFocus::List,
            GitFocus::List => GitFocus::List,
        };
    }

    /// Fills the panel with the selected file's diff, and back. Zooming with the
    /// keyboard in a list column moves it to the diff: there is nothing else on
    /// screen to drive.
    pub fn toggle_zoom(&mut self) {
        self.zoom = !self.zoom;
        if self.zoom {
            // Through `enter_diff` so the line cursor lands on the first CHANGED
            // line: setting the focus alone leaves it on row 0, which is the `diff
            // --git` header, and the cursor marks a line nobody can stage.
            self.enter_diff();
        }
    }

    /// Moves a column separator to a pointer position, as a fraction of the panel
    /// width. `sep` is 0 for the list/files edge and 1 for the files/diff edge; the
    /// clamping that keeps every column usable happens in `layout`.
    pub fn drag_split(&mut self, sep: usize, x: usize, w: usize) {
        if w == 0 || sep > 1 {
            return;
        }
        let f = (x as f32 / w as f32).clamp(0.05, 0.95);
        self.split[sep] = f;
        // The separators may not cross: dragging the first past the second takes the
        // second with it, which is what a column being pushed looks like.
        if sep == 0 && self.split[0] > self.split[1] {
            self.split[1] = self.split[0];
        }
        if sep == 1 && self.split[1] < self.split[0] {
            self.split[0] = self.split[1];
        }
    }

    pub fn cycle_view(&mut self) {
        self.set_view(self.view.next());
    }

    /// Switches view, which also leaves any open commit — its file column and the
    /// zoom over one of its files belong to a list that is no longer on screen.
    /// Done here rather than at every call site: the keyboard paths used to switch
    /// bare and left the columns attached to the wrong view.
    pub fn set_view(&mut self, v: GitView) {
        self.leave_commit();
        self.view = v;
        self.preview_scroll = 0;
    }

    pub fn selected_file(&self) -> Option<&crate::git::FileEntry> {
        matches!(self.view, GitView::Status).then(|| self.files.get(self.cursor()))?
    }

    pub fn selected_commit(&self) -> Option<&crate::git::Commit> {
        matches!(self.view, GitView::Log).then(|| self.log.get(self.cursor()))?
    }

    /// The selected branch, and whether it is a remote-tracking one — which decides
    /// whether switching to it needs `--track`.
    pub fn selected_branch(&self) -> Option<(&String, bool)> {
        if !matches!(self.view, GitView::Branches) {
            return None;
        }
        let i = self.cursor();
        match self.branches.get(i) {
            Some(b) => Some((b, false)),
            None => self.remotes.get(i - self.branches.len()).map(|r| (r, true)),
        }
    }

    pub fn selected_tag(&self) -> Option<&String> {
        matches!(self.view, GitView::Tags)
            .then(|| self.tags.get(self.cursor()))?
            .map(|t| t)
    }

    pub fn selected_reflog(&self) -> Option<&crate::git::Commit> {
        matches!(self.view, GitView::Reflog).then(|| self.reflog.get(self.cursor()))?
    }

    pub fn selected_worktree(&self) -> Option<&String> {
        matches!(self.view, GitView::Worktrees).then(|| self.worktrees.get(self.cursor()))?
    }

    pub fn selected_stash(&self) -> Option<&String> {
        matches!(self.view, GitView::Stashes).then(|| self.stashes.get(self.cursor()))?
    }

    /// Replaces the preview and reparses it into rows.
    pub fn set_preview(&mut self, text: String) {
        self.preview_rows = crate::git::parse_diff(&text);
        self.preview = text;
        self.preview_scroll = 0;
        self.hunk = 0;
    }

    /// Moves the keyboard into the diff, starting the line cursor at the top of the
    /// selected hunk — the lines a stage key would have acted on a moment ago.
    pub fn enter_diff(&mut self) {
        let hunks = self.hunks();
        // Land on the first CHANGED line of the hunk, not its first line: a context
        // line is not something you can stage, so starting there means the first
        // keypress does nothing and reads as broken.
        self.diff_cursor = hunks
            .get(self.hunk)
            .and_then(|&(start, end)| {
                (start..end.min(self.preview_rows.len())).find(|&i| {
                    matches!(
                        self.preview_rows[i].kind,
                        crate::git::DiffKind::Added | crate::git::DiffKind::Removed
                    )
                })
            })
            .unwrap_or(0);
        self.diff_anchor = None;
        self.focus = GitFocus::Diff;
        self.scroll_to_diff_cursor();
    }

    /// Leaves the diff for the column that fed it: an open commit's files, else the
    /// list. Zoom hides those columns, so it ends too.
    pub fn leave_diff(&mut self) {
        self.focus = if self.in_commit() { GitFocus::Files } else { GitFocus::List };
        self.diff_anchor = None;
        self.zoom = false;
    }

    /// Moves the line cursor, keeping it on screen and keeping `hunk` in step so the
    /// hunk-level keys stay meaningful after using the line-level ones.
    pub fn step_diff(&mut self, delta: i32) {
        let n = self.preview_rows.len();
        if n == 0 {
            return;
        }
        self.diff_cursor =
            (self.diff_cursor as i32 + delta).clamp(0, n as i32 - 1) as usize;
        if let Some(h) = self.hunk_at(self.diff_cursor) {
            self.hunk = h;
        }
        self.scroll_to_diff_cursor();
    }

    fn scroll_to_diff_cursor(&mut self) {
        // The body height is not known here; 20 rows is the panel's usual body and
        // only decides when to nudge the scroll, never what is drawn.
        const WINDOW: usize = 20;
        if self.diff_cursor < self.preview_scroll {
            self.preview_scroll = self.diff_cursor;
        } else if self.diff_cursor >= self.preview_scroll + WINDOW {
            self.preview_scroll = self.diff_cursor + 1 - WINDOW;
        }
    }

    /// Starts or clears a line selection at the cursor.
    pub fn toggle_anchor(&mut self) {
        self.diff_anchor = match self.diff_anchor {
            Some(_) => None,
            None => Some(self.diff_cursor),
        };
    }

    /// The selected line range, which is just the cursor when nothing is anchored.
    pub fn line_range(&self) -> (usize, usize) {
        match self.diff_anchor {
            Some(a) => (a.min(self.diff_cursor), a.max(self.diff_cursor)),
            None => (self.diff_cursor, self.diff_cursor),
        }
    }

    /// A patch for the selected lines only.
    pub fn line_patch(&self) -> Option<String> {
        let hunk = *self.hunks().get(self.hunk_at(self.diff_cursor)?)?;
        crate::git::patch_for_lines(&self.preview_rows, hunk, self.line_range())
    }

    pub fn hunks(&self) -> Vec<(usize, usize)> {
        crate::git::hunk_ranges(&self.preview_rows)
    }

    /// Moves the hunk selection and scrolls the preview so the whole hunk is in
    /// view — selecting something off screen would be an invisible state, and this
    /// selection decides what a keypress stages.
    pub fn step_hunk(&mut self, delta: i32, body_rows: usize) {
        let hunks = self.hunks();
        if hunks.is_empty() {
            return;
        }
        let next = (self.hunk as i32 + delta).clamp(0, hunks.len() as i32 - 1) as usize;
        self.hunk = next;
        let (start, end) = hunks[next];
        if start < self.preview_scroll || end > self.preview_scroll + body_rows {
            self.preview_scroll = start;
        }
    }

    /// The patch for the selected hunk, or `None` when the preview is not a diff
    /// (an untracked file, a branch log).
    pub fn hunk_patch(&self) -> Option<String> {
        let hunks = self.hunks();
        crate::git::patch_for_hunk(&self.preview_rows, *hunks.get(self.hunk)?)
    }

    pub fn scroll_preview(&mut self, delta: i32) {
        let lines = self.preview_rows.len() as i32;
        let next = self.preview_scroll as i32 + delta;
        self.preview_scroll = next.clamp(0, (lines - 1).max(0)) as usize;
    }

    /// One row of the open commit's file column: its status letter, then the path
    /// with the DIRECTORY elided rather than the name — which file it is lives at
    /// the end of a path, and a column narrow enough to clip shows the wrong half.
    fn file_row_text(&self, i: usize, width: usize) -> (String, Pen) {
        let green = Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), bg: bg(), ..Pen::default() };
        let red = Pen { fg: Color::Rgb(0xe0, 0x60, 0x60), bg: bg(), ..Pen::default() };
        match self.commit_files.get(i) {
            Some(f) => {
                let pen = match f.index {
                    'A' => green,
                    'D' => red,
                    _ => normal(),
                };
                (format!("{} {}", f.index, elide_left(&f.path, width.saturating_sub(2))), pen)
            }
            None => (String::new(), normal()),
        }
    }

    fn row_text(&self, i: usize, width: usize) -> (String, Pen) {
        let green = Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), bg: bg(), ..Pen::default() };
        let red = Pen { fg: Color::Rgb(0xe0, 0x60, 0x60), bg: bg(), ..Pen::default() };
        match self.view {
            GitView::Status => match self.files.get(i) {
                Some(f) => {
                    // Two columns, like git's own short status: index then worktree.
                    let mark = if f.index == 'U' {
                        "!!"
                    } else if f.untracked() {
                        "??"
                    } else if f.is_staged() && f.is_unstaged() {
                        "M+"
                    } else if f.is_staged() {
                        " +"
                    } else {
                        " M"
                    };
                    let pen = if f.index == 'U' {
                        red
                    } else if f.is_staged() {
                        green
                    } else {
                        normal()
                    };
                    (format!("{mark} {}", elide(&f.path, width.saturating_sub(3))), pen)
                }
                None => (String::new(), normal()),
            },
            GitView::Log => match self.log.get(i) {
                Some(c) if c.sha.is_empty() => {
                    // A graph-art row: topology only, dimmed, and not selectable.
                    (c.graph.clone(), dim())
                }
                Some(c) => {
                    let head =
                        if c.refs.is_empty() { String::new() } else { format!("({}) ", c.refs) };
                    let body = format!("{head}{}", c.subject);
                    let used = c.graph.chars().count() + 9;
                    (
                        format!("{}{} {}", c.graph, c.sha, elide(&body, width.saturating_sub(used))),
                        normal(),
                    )
                }
                None => (String::new(), normal()),
            },
            GitView::Branches => match self.branches.get(i) {
                Some(b) => {
                    let here = *b == self.current_branch;
                    let pen = if here { accent() } else { normal() };
                    (format!("{} {}", if here { "*" } else { " " }, elide(b, width - 2)), pen)
                }
                // Past the local ones come the remote-tracking refs, dimmed: they
                // are somewhere else's branches.
                None => match self.remotes.get(i - self.branches.len()) {
                    Some(r) => (format!("  {}", elide(r, width - 2)), dim()),
                    None => (String::new(), normal()),
                },
            },
            GitView::Tags => match self.tags.get(i) {
                Some(t) => (elide(t, width), normal()),
                None => (String::new(), normal()),
            },
            GitView::Reflog => match self.reflog.get(i) {
                Some(c) => {
                    // %gd (the HEAD@{n} selector) rides in `when`, and the action in
                    // `subject`: "HEAD@{2} reset: moving to HEAD~1".
                    (
                        format!("{} {} {}", c.sha, c.when, elide(&c.subject, width.saturating_sub(20))),
                        normal(),
                    )
                }
                None => (String::new(), normal()),
            },
            GitView::Worktrees => match self.worktrees.get(i) {
                Some(w) => (elide(w, width), normal()),
                None => (String::new(), normal()),
            },
            GitView::Blame => match self.blame.get(i) {
                Some(b) => (
                    format!(
                        "{} {:>4} {}",
                        b.sha.chars().take(7).collect::<String>(),
                        b.line,
                        elide(&b.text, width.saturating_sub(13))
                    ),
                    normal(),
                ),
                None => (String::new(), normal()),
            },
            GitView::Stashes => match self.stashes.get(i) {
                Some(s) => (elide(s, width), normal()),
                None => (String::new(), normal()),
            },
        }
    }

    /// The key legend for the current view. Spelled out rather than left implicit:
    /// every one of these acts immediately, so the user has to be able to read what
    /// a key does before pressing it.
    fn keys_legend(&self) -> &'static str {
        if self.zoom {
            return "one file, full width · J K scroll · z or esc back to the columns";
        }
        if self.in_commit() {
            return "this commit's files · j k move · h l column · enter full width · esc back";
        }
        match self.view {
            GitView::Status => {
                "space stage · a all · ]/[ hunk · s/u stage hunk · c commit · P push · p pull · S stash"
            }
            GitView::Log => "enter this commit's files · x checkout · c cherry-pick · / filter · y sha",
            GitView::Branches => "enter switch · n new · m merge into HEAD · R rebase onto · f fetch",
            GitView::Stashes => "enter pop · S stash push",
            GitView::Tags => "enter checkout · n new tag · P push tags",
            GitView::Reflog => "enter checkout this position · y copy sha (nothing here rewrites)",
            GitView::Worktrees => "enter open it in a new tab · y copy path",
            GitView::Blame => "enter the commit behind this line · esc back · y copy sha",
        }
    }

    /// The rebase plan, drawn instead of the usual two panes: it is a decision to
    /// make, not something to browse, so it gets the whole box.
    fn render_rebase(&self, plan: &RebasePlan, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let l = self.layout(cols, rows);
        let mut g = panel_grid(l.w, l.h, theme);
        write(&mut g, 0, 2, &format!("interactive rebase onto {}", plan.onto), accent());
        for (i, (action, c)) in plan.steps.iter().take(l.body_rows).enumerate() {
            let row = 2 + i;
            let sel = i == plan.cursor;
            if sel {
                write(&mut g, row, 0, &" ".repeat(l.w), selected());
            }
            let pen = if sel {
                selected()
            } else if *action == crate::git::RebaseAction::Drop {
                Pen { fg: Color::Rgb(0xe0, 0x60, 0x60), bg: bg(), ..Pen::default() }
            } else {
                normal()
            };
            let line = format!(
                "{:<7} {} {}",
                action.word(),
                c.sha,
                elide(&c.subject, l.w.saturating_sub(20))
            );
            write(&mut g, row, 2, &line, pen);
        }
        write(
            &mut g,
            l.h - 1,
            2,
            "p pick · r reword · e edit · s squash · f fixup · d drop · K J move · enter run · esc cancel",
            dim(),
        );
        vec![Panel { grid: g, col: l.col, row: l.row }]
    }

    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        if let Some(plan) = &self.rebase {
            return self.render_rebase(plan, cols, rows, theme);
        }
        let l = self.layout(cols, rows);
        let (w, h, list_w) = (l.w, l.h, l.list_w);
        let mut g = panel_grid(w, h, theme);

        // Header: the four views, the active one reversed, then the branch.
        let mut x = 2;
        for v in Self::VIEWS {
            let label = format!(" {} ", v.title());
            let pen = if v == self.view { selected() } else { dim() };
            write(&mut g, 0, x, &label, pen);
            x += label.chars().count() + 1;
        }
        let mut head = if self.current_branch.is_empty() {
            String::new()
        } else {
            format!("\u{e0a0} {}", self.current_branch)
        };
        if matches!(self.view, GitView::Log) && !self.log_filter.is_empty() {
            head = format!("/{}  {head}", self.log_filter);
        }
        if let Some(sha) = &self.open_commit {
            head = format!("{sha}  {head}");
        }
        // Zoomed, the columns that said which file this is are gone, so the header
        // has to say it instead.
        if self.zoom {
            let path = self
                .selected_commit_file()
                .map(|f| f.path.clone())
                .or_else(|| self.selected_file().map(|f| f.path.clone()))
                .unwrap_or_default();
            if !path.is_empty() {
                head = format!("{path}  {head}");
            }
        }
        let head_w = head.chars().count();
        if head_w > 0 && w > x + head_w + 2 {
            write(&mut g, 0, w - head_w - 2, &head, accent());
        }

        // Left: the list. Middle (only with a commit open): its files. Right: the
        // preview, with a rule between each pair.
        let body_rows = l.body_rows;
        let scroll = self.cursor().saturating_sub(body_rows.saturating_sub(1));
        for line in 0..body_rows {
            let i = scroll + line;
            // Zoom leaves no list column to draw into.
            if list_w == 0 || i >= self.len() {
                break;
            }
            let (text, pen) = self.row_text(i, list_w.saturating_sub(2));
            let row = 2 + line;
            // A selection in a column the keyboard has left is still the selection —
            // it just is not the cursor. Dimming it says which column j/k drives
            // without hiding what the other columns are showing.
            if i == self.cursor() {
                let pen = if self.focus == GitFocus::List { selected() } else { inactive() };
                write(&mut g, row, 0, &" ".repeat(list_w), pen);
                write(&mut g, row, 1, &text, pen);
            } else {
                write(&mut g, row, 1, &text, pen);
            }
        }
        if let Some(sep) = l.sep1() {
            for line in 0..body_rows {
                write(&mut g, 2 + line, sep, "\u{2502}", dim());
            }
        }

        // The open commit's files.
        if l.files_w > 0 {
            let fx = l.files_x();
            let fscroll = self.files_cursor().saturating_sub(body_rows.saturating_sub(1));
            for line in 0..body_rows {
                let i = fscroll + line;
                if i >= self.files_len() {
                    break;
                }
                let (text, pen) = self.file_row_text(i, l.files_w.saturating_sub(1));
                let row = 2 + line;
                if i == self.files_cursor() {
                    let pen = if self.focus == GitFocus::Files { selected() } else { inactive() };
                    write(&mut g, row, fx, &" ".repeat(l.files_w), pen);
                    write(&mut g, row, fx, &text, pen);
                } else {
                    write(&mut g, row, fx, &text, pen);
                }
            }
            // An empty column with a commit open means the files are still being
            // read, not that the commit touched nothing.
            if self.files_len() == 0 {
                write(&mut g, 2, fx, "reading\u{2026}", dim());
            }
        }
        if let Some(sep) = l.sep2() {
            for line in 0..body_rows {
                write(&mut g, 2 + line, sep, "\u{2502}", dim());
            }
        }

        // The diff is drawn the way a review tool draws one: a line number, then the
        // code, on a row whose whole width is tinted. A raw `+`/`-` column makes
        // every changed line start one column further in than its neighbours, and
        // leaves you counting rows to find out which line it is.
        let prev_x = l.prev_x();
        let prev_w = l.prev_w();
        let add_bg = Color::Rgb(0x12, 0x2c, 0x18);
        let del_bg = Color::Rgb(0x35, 0x14, 0x18);
        let num_fg = Color::Rgb(0x6a, 0x6d, 0x74);
        let hunks = self.hunks();
        let selected_hunk = hunks.get(self.hunk).copied();
        let hunk_marker = matches!(self.view, GitView::Status) && hunks.len() > 1;
        for (line, row) in
            self.preview_rows.iter().skip(self.preview_scroll).take(body_rows).enumerate()
        {
            let y = 2 + line;
            let idx = self.preview_scroll + line;
            let row_bg = match row.kind {
                crate::git::DiffKind::Added => add_bg,
                crate::git::DiffKind::Removed => del_bg,
                _ => bg(),
            };
            // Tint the whole row first, so the change reads as a band rather than as
            // coloured text that competes with the syntax colours.
            if !matches!(row.kind, crate::git::DiffKind::Context | crate::git::DiffKind::Meta) {
                write(&mut g, y, prev_x, &" ".repeat(prev_w), Pen { bg: row_bg, ..Pen::default() });
            }
            let num = match row.number {
                Some(n) => format!("{n:>5} "),
                None => "      ".to_string(),
            };
            write(&mut g, y, prev_x, &num, Pen { fg: num_fg, bg: row_bg, ..Pen::default() });
            // With the diff focused, the line cursor and its selection outrank the
            // hunk bar: they are what a stage key will act on now.
            if self.diff_focus() {
                let (lo, hi) = self.line_range();
                if idx >= lo && idx <= hi {
                    write(
                        &mut g,
                        y,
                        prev_x,
                        "\u{258c}",
                        Pen { fg: Color::Rgb(0x4c, 0x9f, 0xd4), bg: row_bg, ..Pen::default() },
                    );
                }
                if idx == self.diff_cursor {
                    write(
                        &mut g,
                        y,
                        prev_x + 1,
                        "\u{25b8}",
                        Pen { fg: Color::Rgb(0xf5, 0xd5, 0x43), bg: row_bg, ..Pen::default() },
                    );
                }
            } else if hunk_marker {
                if let Some((hs, he)) = selected_hunk {
                    if idx >= hs && idx < he {
                        write(
                            &mut g,
                            y,
                            prev_x,
                            "\u{258c}",
                            Pen { fg: Color::Rgb(0xf5, 0xd5, 0x43), bg: row_bg, ..Pen::default() },
                        );
                    }
                }
            }
            let text_x = prev_x + 6;
            let text_w = prev_w.saturating_sub(6);
            let fg = match row.kind {
                crate::git::DiffKind::Meta => Color::Rgb(DIMFG.0, DIMFG.1, DIMFG.2),
                _ => Color::Rgb(0xd4, 0xd6, 0xd9),
            };
            write(
                &mut g,
                y,
                text_x,
                &elide(&row.text, text_w),
                Pen { fg, bg: row_bg, ..Pen::default() },
            );
        }

        // Footer: what just happened, or the legend when there is nothing to report.
        let (foot, pen) = if self.busy {
            ("working\u{2026}".to_string(), accent())
        } else {
            match &self.message {
                Err(e) => (
                    first_line(e),
                    Pen { fg: Color::Rgb(0xe0, 0x70, 0x70), bg: bg(), ..Pen::default() },
                ),
                Ok(m) if !m.is_empty() => (first_line(m), normal()),
                _ => (self.keys_legend().to_string(), dim()),
            }
        };
        // The hint that there IS a menu goes right of the legend, always: a leader
        // layer nobody knows about is a layer nobody uses.
        let hint = "leader \u{2192} menu";
        let hint_w = hint.chars().count();
        let foot_w = w.saturating_sub(hint_w + 6);
        write(&mut g, h - 1, 2, &elide(&foot, foot_w), pen);
        if w > hint_w + 4 {
            write(&mut g, h - 1, w - hint_w - 2, hint, dim());
        }

        let mut out = vec![Panel { grid: g, col: l.col, row: l.row }];
        // The leader layer's which-key, drawn as a panel of its own along the bottom
        // of the box. It has to be part of the OVERLAY, not the screen chrome the
        // global leader uses: chrome is drawn under the overlay's dimmed backdrop,
        // which is exactly where you cannot read it.
        if self.leader.is_some() {
            let entries = self.leader_entries();
            if !entries.is_empty() {
                let wk = crate::whichkey_grid(&entries, &self.leader_path(), l.w, &theme.leader_palette());
                let rows = wk.rows();
                out.push(Panel {
                    grid: wk,
                    col: l.col,
                    row: (l.row + l.h).saturating_sub(rows),
                });
            }
        }
        out
    }

    /// Where a click landed inside the panel, in panel-local cells. `None` when the
    /// click was outside it entirely.
    pub fn hit(&self, cols: usize, rows: usize, col: usize, row: usize) -> Option<GitHit> {
        let l = self.layout(cols, rows);
        let (lc, lr) = (col.checked_sub(l.col)?, row.checked_sub(l.row)?);
        if lc >= l.w || lr >= l.h {
            return None;
        }
        if lr == 0 {
            // The view tabs, measured exactly as they are drawn.
            let mut x = 2;
            for v in Self::VIEWS {
                let width = v.title().chars().count() + 2;
                if lc >= x && lc < x + width {
                    return Some(GitHit::View(v));
                }
                x += width + 1;
            }
            return Some(GitHit::Header);
        }
        if lr < 2 || lr >= 2 + l.body_rows {
            return Some(GitHit::Header);
        }
        let line = lr - 2;
        // The separators are checked first and are one cell wider than they are
        // drawn: a one-cell drag target is a target you miss.
        if let Some(sep) = l.sep1() {
            if lc + 1 >= sep && lc <= sep + 1 {
                return Some(GitHit::Separator(0));
            }
        }
        if let Some(sep) = l.sep2() {
            if lc + 1 >= sep && lc <= sep + 1 {
                return Some(GitHit::Separator(1));
            }
        }
        if l.list_w > 0 && lc < l.list_w {
            let scroll = self.cursor().saturating_sub(l.body_rows.saturating_sub(1));
            return Some(GitHit::Row(scroll + line));
        }
        if l.files_w > 0 && lc >= l.files_x() && lc < l.files_x() + l.files_w {
            let scroll = self.files_cursor().saturating_sub(l.body_rows.saturating_sub(1));
            return Some(GitHit::FileRow(scroll + line));
        }
        Some(GitHit::PreviewLine(self.preview_scroll + line))
    }

    /// The separator under a pointer, if any — for the resize cursor and for
    /// starting a drag. Same geometry as the hit test, deliberately.
    pub fn separator_at(&self, cols: usize, rows: usize, col: usize, row: usize) -> Option<usize> {
        match self.hit(cols, rows, col, row) {
            Some(GitHit::Separator(i)) => Some(i),
            _ => None,
        }
    }

    /// The hunk containing a preview row, for click-to-select-hunk.
    pub fn hunk_at(&self, row: usize) -> Option<usize> {
        self.hunks().iter().position(|&(s, e)| row >= s && row < e)
    }

    const VIEWS: [GitView; 7] = [
        GitView::Status,
        GitView::Log,
        GitView::Branches,
        GitView::Stashes,
        GitView::Tags,
        GitView::Reflog,
        GitView::Worktrees,
    ];
}

/// An interactive rebase being planned inside the panel.
///
/// The list is kept newest-first, the way the log shows it, and reversed when the
/// todo file is written — git replays oldest first, but a plan that reads in the
/// opposite order to the list it came from is a plan people get wrong.
pub struct RebasePlan {
    /// The commit the rebase is based on: everything after it is replayed.
    pub onto: String,
    pub steps: Vec<(crate::git::RebaseAction, crate::git::Commit)>,
    pub cursor: usize,
}

impl RebasePlan {
    pub fn new(onto: String, commits: Vec<crate::git::Commit>) -> Self {
        let steps = commits.into_iter().map(|c| (crate::git::RebaseAction::Pick, c)).collect();
        Self { onto, steps, cursor: 0 }
    }

    /// The todo git will run: oldest first.
    pub fn todo(&self) -> String {
        let steps: Vec<_> =
            self.steps.iter().rev().map(|(a, c)| (*a, c.sha.clone())).collect();
        crate::git::rebase_todo(&steps)
    }

    pub fn set_action(&mut self, action: crate::git::RebaseAction) {
        if let Some(step) = self.steps.get_mut(self.cursor) {
            step.0 = action;
        }
    }

    /// Moves the selected commit within the plan, carrying the cursor with it.
    pub fn move_step(&mut self, delta: i32) {
        let n = self.steps.len();
        if n == 0 {
            return;
        }
        let to = (self.cursor as i32 + delta).clamp(0, n as i32 - 1) as usize;
        if to != self.cursor {
            self.steps.swap(self.cursor, to);
            self.cursor = to;
        }
    }
}

// ---- the git panel's own leader layer --------------------------------------

/// A key the panel already understands. The leader layer does not reimplement the
/// panel's verbs — it presses their keys — so a verb can never behave one way from
/// a letter and another way from the leader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitKey {
    Ch(char),
    Space,
    Enter,
}

/// What a leader leaf does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitPress {
    /// Press it in whatever view is up.
    Key(GitKey),
    /// Switch to a view first. Only for verbs that do not depend on what is
    /// selected there — a new branch, a new tag — since switching moves the
    /// selection.
    Then(GitView, GitKey),
    /// Only offered while that view is up, because it acts on ITS selection:
    /// "merge this branch" means nothing from the stash list.
    In(GitView, GitKey),
    /// Only offered while the keyboard is in the DIFF column, because that is the
    /// only place the panel binds it. A view is not enough of a guard for these:
    /// `v` anchors a line selection and is ignored from a list column.
    InDiff(GitKey),
    /// Go to a view and stop, so the next choice is made looking at it.
    View(GitView),
}

pub enum GitNode {
    Leaf(GitPress),
    Group(&'static [GitEntry]),
}

pub struct GitEntry {
    pub key: char,
    pub title: &'static str,
    pub node: GitNode,
}

const fn leaf(key: char, title: &'static str, press: GitPress) -> GitEntry {
    GitEntry { key, title, node: GitNode::Leaf(press) }
}

use GitKey::{Ch, Enter as En, Space as Sp};
use GitPress::{In, InDiff, Key, Then, View};

/// The panel's leader tree: the nouns at the root, the verbs under them.
///
/// It is a discovery layer, not a second set of bindings — every leaf presses a key
/// the panel already has, and the single letters keep working untouched.
pub static GIT_LEADER: &[GitEntry] = &[
    GitEntry {
        key: 'f',
        title: "File",
        node: GitNode::Group(&[
            leaf(' ', "stage / unstage file", In(GitView::Status, Sp)),
            leaf('a', "stage everything", In(GitView::Status, Ch('a'))),
            leaf('t', "show the other diff", In(GitView::Status, Ch('t'))),
            leaf('e', "open in the editor", In(GitView::Status, Ch('e'))),
            leaf('b', "blame", In(GitView::Status, Ch('b'))),
            leaf('l', "history of this file", In(GitView::Status, Ch('L'))),
            leaf('o', "take ours (conflict)", In(GitView::Status, Ch('O'))),
            leaf('T', "take theirs (conflict)", In(GitView::Status, Ch('T'))),
            leaf('y', "copy path / sha", Key(Ch('y'))),
            leaf('z', "full width, and back", Key(Ch('z'))),
        ]),
    },
    GitEntry {
        key: 'd',
        title: "Diff",
        node: GitNode::Group(&[
            leaf(']', "next hunk", Key(Ch(']'))),
            leaf('[', "previous hunk", Key(Ch('['))),
            leaf('l', "line cursor in the diff", In(GitView::Status, Ch('l'))),
            leaf('v', "start a line selection", InDiff(Ch('v'))),
            leaf('s', "stage hunk / lines", In(GitView::Status, Ch('s'))),
            leaf('u', "unstage hunk / lines", In(GitView::Status, Ch('u'))),
            leaf('t', "staged or worktree", In(GitView::Status, Ch('t'))),
        ]),
    },
    GitEntry {
        key: 'c',
        title: "Commit",
        node: GitNode::Group(&[
            leaf('c', "commit the staged set", Then(GitView::Status, Ch('c'))),
            leaf('e', "commit in a pane (editor)", Then(GitView::Status, Ch('C'))),
            leaf('a', "amend, keeping the message", Then(GitView::Status, Ch('A'))),
            leaf('l', "the log\u{2026}", View(GitView::Log)),
            leaf('p', "cherry-pick this one", In(GitView::Log, Ch('c'))),
            leaf('x', "check this one out", In(GitView::Log, Ch('x'))),
            leaf('i', "interactive rebase from here", In(GitView::Log, Ch('i'))),
            leaf('o', "open it in a pane", In(GitView::Log, Ch('o'))),
            leaf('/', "filter the log", In(GitView::Log, Ch('/'))),
        ]),
    },
    GitEntry {
        key: 'b',
        title: "Branch",
        node: GitNode::Group(&[
            leaf('b', "branches\u{2026}", View(GitView::Branches)),
            leaf('n', "new branch", Then(GitView::Branches, Ch('n'))),
            leaf('s', "switch to this one", In(GitView::Branches, En)),
            leaf('m', "merge it into HEAD", In(GitView::Branches, Ch('m'))),
            leaf('r', "rebase HEAD onto it", In(GitView::Branches, Ch('R'))),
            leaf('f', "fetch --all --prune", Key(Ch('f'))),
            leaf('p', "pull --ff-only", Key(Ch('p'))),
            leaf('P', "push", Key(Ch('P'))),
        ]),
    },
    GitEntry {
        key: 's',
        title: "Stash",
        node: GitNode::Group(&[
            leaf('s', "stash everything", Key(Ch('S'))),
            leaf('l', "stashes\u{2026}", View(GitView::Stashes)),
            leaf('p', "pop this one", In(GitView::Stashes, En)),
        ]),
    },
    GitEntry {
        key: 't',
        title: "Tag",
        node: GitNode::Group(&[
            leaf('t', "tags\u{2026}", View(GitView::Tags)),
            leaf('n', "new tag", Then(GitView::Tags, Ch('n'))),
            leaf('x', "check this tag out", In(GitView::Tags, En)),
            leaf('P', "push --tags", Then(GitView::Tags, Ch('P'))),
        ]),
    },
    GitEntry {
        key: 'r',
        title: "Remote & repo",
        node: GitNode::Group(&[
            leaf('f', "fetch --all --prune", Key(Ch('f'))),
            leaf('p', "pull --ff-only", Key(Ch('p'))),
            leaf('P', "push", Key(Ch('P'))),
            leaf('t', "push --tags", Then(GitView::Tags, Ch('P'))),
            leaf('r', "reread everything", Key(Ch('r'))),
            leaf('w', "worktrees\u{2026}", View(GitView::Worktrees)),
            leaf('l', "reflog\u{2026}", View(GitView::Reflog)),
        ]),
    },
    GitEntry {
        key: 'v',
        title: "View",
        node: GitNode::Group(&[
            leaf('1', "status", View(GitView::Status)),
            leaf('2', "log", View(GitView::Log)),
            leaf('3', "branches", View(GitView::Branches)),
            leaf('4', "stashes", View(GitView::Stashes)),
            leaf('5', "tags", View(GitView::Tags)),
            leaf('6', "reflog", View(GitView::Reflog)),
            leaf('7', "worktrees", View(GitView::Worktrees)),
        ]),
    },
    leaf('z', "Full width, and back", Key(Ch('z'))),
    leaf('q', "Close the panel", Key(Ch('q'))),
];

/// The panel's geometry in cells, produced once and used by both the renderer and
/// the hit test.
pub struct GitLayout {
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    /// Width of the list column. Zero while zoomed.
    pub list_w: usize,
    /// Width of the open commit's file column. Zero unless a commit is open.
    pub files_w: usize,
    pub body_rows: usize,
}

impl GitLayout {
    /// The column the first separator is drawn in, if there is one.
    pub fn sep1(&self) -> Option<usize> {
        (self.list_w > 0).then_some(self.list_w)
    }

    /// Where the file column starts.
    pub fn files_x(&self) -> usize {
        self.list_w + 2
    }

    /// The column the second separator is drawn in, if there is one.
    pub fn sep2(&self) -> Option<usize> {
        (self.files_w > 0).then_some(self.files_x() + self.files_w)
    }

    /// Where the diff starts, and how wide it is.
    pub fn prev_x(&self) -> usize {
        match self.sep2() {
            Some(s) => s + 2,
            None if self.list_w > 0 => self.list_w + 2,
            None => 0,
        }
    }

    pub fn prev_w(&self) -> usize {
        self.w.saturating_sub(self.prev_x())
    }
}

/// What a click landed on.
pub enum GitHit {
    View(GitView),
    /// A row of the list, by index into whatever the list is showing.
    Row(usize),
    /// A row of the open commit's file column, by index into `commit_files`.
    FileRow(usize),
    /// A row of the preview, by index into `preview_rows`.
    PreviewLine(usize),
    /// A column separator, by index (0 = list/files, 1 = files/diff). Dragging it
    /// resizes the columns either side.
    Separator(usize),
    Header,
}

/// First non-empty line, so a multi-line git message fits a one-row footer.
fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string()
}

/// Clips to `width` columns, marking the cut, so a truncated path is never mistaken
/// for a shorter one.
pub fn elide(s: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if width == 0 {
        return String::new();
    }
    // In CELLS: `write` advances by display width, so a char budget lets wide
    // characters spill past the box they were cut for.
    let total: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width.saturating_sub(1) {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('\u{2026}');
    out
}

/// Clips to `width` columns from the LEFT, keeping the end. For a path that is the
/// file name, which is the part that says which file this is; clipping the tail
/// instead leaves a column of identical directory prefixes.
fn elide_left(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    let keep = width.saturating_sub(1);
    let tail: String = s.chars().skip(n - keep).collect();
    format!("\u{2026}{tail}")
}


/// The visible window of a single-line input field that is wider than its box.
///
/// The caret always sits at the end of these fields (they take typing and
/// backspace, never arrow keys), so the END is what has to stay visible: a field
/// that clips the tail hides the character you just typed, which is the one thing a
/// text input may never do. When text is cut, a leading ellipsis says so.
pub fn field_view(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if width == 0 {
        return String::new();
    }
    // Budgeted in CELLS, not in chars. A CJK or emoji name counts one char and
    // takes two columns, so a char budget lets a name run under whatever was
    // written to the right of it — the badge column, the symlink arrow.
    let total: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(1);
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0usize;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > keep {
            break;
        }
        used += w;
        tail.push(c);
    }
    tail.reverse();
    let tail: String = tail.into_iter().collect();
    format!("\u{2026}{tail}")
}

/// A grid plus where it sits, in cells.
pub struct Panel {
    pub grid: Grid,
    pub col: usize,
    pub row: usize,
}

// ---- shared drawing helpers -----------------------------------------------

fn panel_grid(cols: usize, rows: usize, theme: &Theme) -> Grid {
    let mut grid = Grid::new(cols, rows);
    // Fill with the panel background so it occludes the dimmed terminal behind.
    let bg = Pen { bg: Color::Rgb(0x1c, 0x1d, 0x22), ..Pen::default() };
    grid.fill(bg);
    let _ = theme;
    grid
}

fn write(grid: &mut Grid, row: usize, col: usize, text: &str, pen: Pen) {
    grid.write_str(row, col, text, pen);
}

const PANEL_BG: (u8, u8, u8) = (0x1c, 0x1d, 0x22);
const ACCENT: (u8, u8, u8) = (0x4c, 0x9f, 0xd4);
const DIMFG: (u8, u8, u8) = (0x8a, 0x8d, 0x94);

fn accent() -> Pen {
    Pen { fg: Color::Rgb(ACCENT.0, ACCENT.1, ACCENT.2), bg: bg(), ..Pen::default() }
}
fn dim() -> Pen {
    Pen { fg: Color::Rgb(DIMFG.0, DIMFG.1, DIMFG.2), bg: bg(), ..Pen::default() }
}
fn normal() -> Pen {
    Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), bg: bg(), ..Pen::default() }
}
fn selected() -> Pen {
    Pen { fg: Color::Rgb(0x0d, 0x0d, 0x0f), bg: Color::Rgb(ACCENT.0, ACCENT.1, ACCENT.2), ..Pen::default() }
}
/// The selected row of a column the keyboard is NOT driving. Still a selection —
/// it decides what the columns to its right show — but visibly not the cursor.
fn inactive() -> Pen {
    Pen { fg: Color::Rgb(0xd4, 0xd6, 0xd9), bg: Color::Rgb(0x30, 0x33, 0x3c), ..Pen::default() }
}
fn bg() -> Color {
    Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2)
}

// ---- settings panel --------------------------------------------------------

use crate::config::Config;
use crate::settings::{self, Kind, Row};

/// The interactive settings editor. Holds a working `Config`; edits apply live and
/// are persisted to JSON on save. `dirty` signals the host to apply the change.
pub struct ConfigPanel {
    pub config: Config,
    rows: Vec<Row>,
    pub cursor: usize,
    /// Inline text-edit buffer when editing a Text setting, else `None`.
    pub editing: Option<String>,
    /// Set after a change so the host re-applies `config`; cleared by the host.
    pub dirty: bool,
    /// Transient status line ("saved", "save failed: …").
    pub status: String,
}

impl ConfigPanel {
    pub fn new(config: Config) -> Self {
        Self { config, rows: settings::rows(), cursor: 0, editing: None, dirty: false, status: String::new() }
    }

    fn id(&self) -> settings::SettingId {
        self.rows[self.cursor].id
    }
    fn kind(&self) -> Kind {
        self.rows[self.cursor].kind
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn down(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }

    /// Left/right arrow or h/l: step numbers, toggle bools, cycle enums.
    pub fn adjust(&mut self, dir: i32) {
        let id = self.id();
        settings::adjust(&mut self.config, id, dir);
        self.dirty = true;
    }

    /// Space/Enter: toggle bool, cycle enum, or begin editing a text field.
    pub fn activate(&mut self) {
        match self.kind() {
            Kind::Bool | Kind::Enum => self.adjust(1),
            Kind::Text => {
                // Seed from the raw config value, not the display string (which shows
                // "(none)" for an unset path).
                let seed = match self.id() {
                    settings::SettingId::Background => {
                        self.config.window.background.clone().unwrap_or_default()
                    }
                    settings::SettingId::FontFamily => self.config.font.family.clone(),
                    _ => String::new(),
                };
                self.editing = Some(seed);
            }
            Kind::Float | Kind::Int => self.adjust(1),
        }
    }

    pub fn input_char(&mut self, c: char) {
        if let Some(buf) = self.editing.as_mut() {
            buf.push(c);
        }
    }
    pub fn backspace(&mut self) {
        if let Some(buf) = self.editing.as_mut() {
            buf.pop();
        }
    }
    /// Commit the inline text edit.
    pub fn commit_edit(&mut self) {
        if let Some(buf) = self.editing.take() {
            let id = self.id();
            settings::set_text(&mut self.config, id, buf);
            self.dirty = true;
        }
    }
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Persists the working config as JSON.
    pub fn save(&mut self) {
        self.status = match self.config.save_json() {
            Ok(()) => "saved to runnir.json".into(),
            Err(e) => format!("save failed: {e}"),
        };
        // Mark dirty so the host re-adopts and refreshes the config-file mtime — the
        // just-written file must not then trigger a redundant hot-reload + toast.
        self.dirty = true;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 7 / 10).clamp(44, 84).min(cols.saturating_sub(2).max(1));
        let visible = (rows.saturating_sub(6)).clamp(6, self.rows.len() + 8);
        let h = (visible + 4).min(rows.saturating_sub(2)).max(8);
        let mut g = panel_grid(w, h, theme);
        let _ = theme;

        write(&mut g, 0, 2, "Settings", accent());
        write(&mut g, 0, w.saturating_sub(30), "\u{2191}\u{2193} move  \u{2190}\u{2192} change  s save", dim());

        // Scroll so the cursor stays visible in the list area (rows 2..h-2).
        let list_h = h.saturating_sub(3);
        let top = self.cursor.saturating_sub(list_h.saturating_sub(1)).min(self.rows.len().saturating_sub(list_h).max(0));

        let mut last_section = "";
        for (screen, i) in (top..self.rows.len()).take(list_h).enumerate() {
            let row = 2 + screen;
            let r = &self.rows[i];
            let sel = i == self.cursor;
            if sel {
                for c in 0..w {
                    write(&mut g, row, c, " ", selected());
                }
            }
            let pen = if sel { selected() } else { normal() };
            let sec = if r.section != last_section { r.section } else { "" };
            last_section = r.section;
            write(&mut g, row, 2, sec, if sel { selected() } else { accent() });
            write(&mut g, row, 12, r.label, pen);
            let val = if sel && self.editing.is_some() {
                format!("{}\u{2588}", self.editing.as_deref().unwrap_or(""))
            } else {
                settings::value(&self.config, r.id)
            };
            let vcol = w.saturating_sub(val.chars().count() + 2);
            write(&mut g, row, vcol.max(38), &val, pen);
        }

        if !self.status.is_empty() {
            write(&mut g, h - 1, 2, &self.status, dim());
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 4;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- command palette -------------------------------------------------------

pub struct Palette {
    query: String,
    all: Vec<(Action, String)>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl Palette {
    pub fn new(keyhints: &std::collections::HashMap<String, String>) -> Self {
        let all: Vec<(Action, String)> = Action::palette_list()
            .into_iter()
            .map(|a| {
                let hint = keyhints.get(a.id()).cloned().unwrap_or_default();
                (a, hint)
            })
            .collect();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<Action> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0.clone())
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.all.len())
            .filter(|&i| fuzzy(&self.all[i].0.title().to_lowercase(), &q))
            .collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(30, 70).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len()).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Command palette", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        // Cursor block after the query.
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        // Scroll the list so the cursor is always on screen, even past `visible`.
        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let (action, hint) = &self.all[idx];
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            // Paint the whole selected row so the highlight is a bar, not just text.
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, action.title(), pen);
            if !hint.is_empty() {
                let hp = if sel { selected() } else { dim() };
                let x = w.saturating_sub(hint.chars().count() + 2);
                write(&mut g, row, x, hint, hp);
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- theme picker ----------------------------------------------------------

/// A fuzzy-filterable list of the bundled colour themes. Modelled on [`Palette`]:
/// arrows move, typing filters. What sets it apart is *live preview* — the host
/// applies the highlighted theme to the renderer as the selection moves, so the
/// terminal behind the picker updates immediately. The theme active when it opened
/// is stashed in `original` so cancelling can restore it untouched.
pub struct ThemePicker {
    query: String,
    all: Vec<(&'static str, Theme)>,
    filtered: Vec<usize>,
    cursor: usize,
    original: Theme,
}

impl ThemePicker {
    pub fn new(original: Theme) -> Self {
        let all = crate::themes::builtins();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0, original }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    /// The theme under the cursor — what the host previews live and keeps on Enter.
    pub fn selected_theme(&self) -> Option<Theme> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].1.clone())
    }

    /// Name of the highlighted theme, for a status toast on confirm.
    pub fn selected_name(&self) -> Option<&'static str> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0)
    }

    /// The theme that was in effect when the picker opened; restored on cancel.
    pub fn original(&self) -> Theme {
        self.original.clone()
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.all.len())
            .filter(|&i| fuzzy(&self.all[i].0.to_lowercase(), &q))
            .collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(34, 74).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len()).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Theme picker", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        // A swatch strip previews each theme's palette inline: background, the six
        // vivid ANSI colours, then foreground — enough to judge a theme at a glance
        // without moving the selection onto it.
        const SWATCH: usize = 8;
        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let (name, t) = &self.all[idx];
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, name, pen);
            // Draw the swatch flush-right, one cell per colour.
            if w > SWATCH + 4 {
                let cols_of = |t: &Theme| -> [(u8, u8, u8); SWATCH] {
                    let a = &t.ansi;
                    let g = |i: usize| a.get(i).map(|c| (c.0, c.1, c.2)).unwrap_or((0, 0, 0));
                    [
                        (t.background.0, t.background.1, t.background.2),
                        g(1),
                        g(2),
                        g(3),
                        g(4),
                        g(5),
                        g(6),
                        (t.foreground.0, t.foreground.1, t.foreground.2),
                    ]
                };
                let strip = cols_of(t);
                let base = w.saturating_sub(SWATCH + 1);
                for (i, (r, gg, b)) in strip.iter().enumerate() {
                    let cell = Pen { bg: Color::Rgb(*r, *gg, *b), ..Pen::default() };
                    write(&mut g, row, base + i, " ", cell);
                }
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- snippet picker --------------------------------------------------------

/// A fuzzy-filterable list of the user's command snippets (bookmarks). Modelled on
/// [`Palette`]: arrows move, typing filters. Matching is a subsequence over the
/// snippet's name *and* description, so you can find one by either. Confirming does
/// not run anything here — the host types the chosen command at the prompt (or, if
/// the snippet opts into `run_now`, submits it), so review stays the default.
pub struct SnippetPicker {
    query: String,
    all: Vec<SnippetDef>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl SnippetPicker {
    pub fn new(snippets: Vec<SnippetDef>) -> Self {
        let filtered = (0..snippets.len()).collect();
        Self { query: String::new(), all: snippets, filtered, cursor: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    /// The snippet under the cursor — what the host inserts (or runs) on Enter.
    pub fn selected(&self) -> Option<SnippetDef> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].clone())
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.all.len())
            .filter(|&i| {
                // Match against name and description together so either can find it.
                let hay = format!("{} {}", self.all[i].name, self.all[i].description);
                fuzzy(&hay.to_lowercase(), &q)
            })
            .collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(34, 74).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len()).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Command snippets", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let snip = &self.all[idx];
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            write(&mut g, row, 2, &snip.name, pen);
            // The description trails the name, dimmed, clipped to fit the panel.
            if !snip.description.is_empty() {
                let x = 2 + snip.name.chars().count() + 2;
                if x + 1 < w {
                    let room = w.saturating_sub(x + 1);
                    let desc: String = snip.description.chars().take(room).collect();
                    let dp = if sel { selected() } else { dim() };
                    write(&mut g, row, x, &desc, dp);
                }
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- clipboard history picker ---------------------------------------------

/// A fuzzy-filterable list of recent clipboard copies, newest first. Modelled on
/// [`Palette`]: arrows move, typing filters. Each row shows a one-line, truncated
/// preview of the entry; confirming pastes the full entry into the focused pane via
/// the normal paste path. The full text is kept alongside the preview so a
/// multi-line copy pastes whole even though only its first line is shown.
pub struct ClipHistoryPicker {
    query: String,
    /// (full entry, one-line preview), newest first.
    all: Vec<(String, String)>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl ClipHistoryPicker {
    pub fn new(entries: &std::collections::VecDeque<String>) -> Self {
        let all: Vec<(String, String)> =
            entries.iter().map(|e| (e.clone(), clip_preview(e))).collect();
        let filtered = (0..all.len()).collect();
        Self { query: String::new(), all, filtered, cursor: 0 }
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
        }
    }

    /// The full text of the highlighted entry — what gets pasted on confirm.
    pub fn selected(&self) -> Option<String> {
        self.filtered.get(self.cursor).map(|&i| self.all[i].0.clone())
    }

    fn refilter(&mut self) {
        // Filter against the whole entry, not just the shown preview, so a match on a
        // later line still surfaces it.
        let q = self.query.to_lowercase();
        self.filtered =
            (0..self.all.len()).filter(|&i| fuzzy(&self.all[i].0.to_lowercase(), &q)).collect();
        self.cursor = 0;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = (cols * 6 / 10).clamp(30, 80).min(cols.saturating_sub(2));
        let visible = 12.min(self.filtered.len().max(1)).max(1);
        let h = visible + 3;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Clipboard history", accent());
        let prompt = format!("> {}", self.query);
        write(&mut g, 1, 2, &prompt, normal());
        write(&mut g, 1, 2 + prompt.chars().count(), " ", selected());

        if self.all.is_empty() {
            write(&mut g, 3, 2, "nothing copied yet", dim());
        } else if self.filtered.is_empty() {
            write(&mut g, 3, 2, "no matches", dim());
        }

        let scroll = self.cursor.saturating_sub(visible - 1);
        for (line, &idx) in self.filtered.iter().skip(scroll).take(visible).enumerate() {
            let sel = scroll + line == self.cursor;
            let preview = &self.all[idx].1;
            let row = 3 + line;
            let pen = if sel { selected() } else { normal() };
            if sel {
                write(&mut g, row, 0, &" ".repeat(w), selected());
            }
            let clipped: String = preview.chars().take(w.saturating_sub(4)).collect();
            write(&mut g, row, 2, &clipped, pen);
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

/// A one-line, length-capped preview of a clipboard entry for the picker list: the
/// first non-blank line, trimmed, with a marker when more lines follow.
fn clip_preview(entry: &str) -> String {
    const CAP: usize = 76;
    let first = entry.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let multiline = entry.lines().filter(|l| !l.trim().is_empty()).count() > 1;
    let mut out: String = if first.chars().count() > CAP {
        let mut s: String = first.chars().take(CAP - 1).collect();
        s.push('\u{2026}');
        s
    } else {
        first.to_string()
    };
    if multiline {
        out.push_str(" \u{00b6}"); // pilcrow: this entry spans more than one line
    }
    out
}

// ---- now playing (media) ---------------------------------------------------

/// The now-playing overlay: album art on the left (rendered as half-block cells),
/// track metadata and playback status on the right, an optional live waveform below,
/// and a one-line hint of the in-overlay control keys. It holds a snapshot captured
/// when it opened; the host refreshes the snapshot (and the waveform) while it is open
/// by calling [`MediaOverlay::set_now_playing`] / [`MediaOverlay::set_wave`].
pub struct MediaOverlay {
    np: NowPlaying,
    /// Cover art as half-block cells (`rows` x `cols`); empty when there is no art.
    art: Vec<Vec<HalfCell>>,
    /// The latest waveform frame (one amplitude byte per bar); empty until one lands.
    bars: Vec<u8>,
    /// Whether a waveform is expected (config on and cava available), so the layout
    /// reserves a row for it even before the first frame arrives.
    wave_on: bool,
}

impl MediaOverlay {
    pub fn new(np: NowPlaying, art: Vec<Vec<HalfCell>>, wave_on: bool) -> Self {
        Self { np, art, bars: Vec::new(), wave_on }
    }

    /// Replaces the metadata snapshot (and its decoded art) on a refresh.
    pub fn set_now_playing(&mut self, np: NowPlaying, art: Vec<Vec<HalfCell>>) {
        self.np = np;
        self.art = art;
    }

    /// Replaces only the metadata, keeping the already-decoded art — used on a refresh
    /// where the cover-art path has not changed, so the file is not re-decoded.
    pub fn set_meta(&mut self, np: NowPlaying) {
        self.np = np;
    }

    /// The current cover-art path, so a refresh can tell whether to re-decode.
    pub fn art_path(&self) -> Option<&std::path::Path> {
        self.np.art.as_deref()
    }

    /// Stores the newest waveform frame for the next repaint.
    pub fn set_wave(&mut self, bars: Vec<u8>) {
        self.bars = bars;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let art_rows = self.art.len();
        let art_cols = self.art.first().map(|r| r.len()).unwrap_or(0);
        let meta_x = if art_cols > 0 { art_cols + 4 } else { 2 };
        let meta_w = 46usize;
        let w = (meta_x + meta_w).clamp(34, cols.saturating_sub(2).max(34));
        // Body height: the art, or the four metadata lines, whichever is taller.
        let body = art_rows.max(4);
        let wave_h = if self.wave_on { 1 } else { 0 };
        // header(1) + gap(1) + body + wave + gap(1) + hint(1)
        let h = (4 + body + wave_h).min(rows.saturating_sub(2).max(8));
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "Now playing", accent());

        // Album art (half-blocks) on the left: one '▀' per cell, upper half in the top
        // pixel's colour, the cell background in the bottom pixel's.
        for (r, line) in self.art.iter().enumerate() {
            let row = 2 + r;
            if row >= h.saturating_sub(1) {
                break;
            }
            for (c, cell) in line.iter().enumerate() {
                let pen = Pen {
                    fg: Color::Rgb(cell.top.0, cell.top.1, cell.top.2),
                    bg: Color::Rgb(cell.bottom.0, cell.bottom.1, cell.bottom.2),
                    ..Pen::default()
                };
                write(&mut g, row, 2 + c, "\u{2580}", pen);
            }
        }

        // Metadata on the right, clipped to the panel.
        let room = w.saturating_sub(meta_x + 1);
        let clip = |s: &str| -> String { s.chars().take(room).collect() };
        let title = if self.np.title.is_empty() {
            "(unknown title)".to_string()
        } else {
            self.np.title.clone()
        };
        write(&mut g, 2, meta_x, &clip(&title), normal());
        if !self.np.artist.is_empty() {
            write(&mut g, 3, meta_x, &clip(&self.np.artist), accent());
        }
        if !self.np.album.is_empty() {
            write(&mut g, 4, meta_x, &clip(&self.np.album), dim());
        }
        let status = match self.np.status {
            crate::media::Status::Playing => "\u{25b6} playing",
            crate::media::Status::Paused => "\u{23f8} paused",
            crate::media::Status::Stopped => "\u{25a0} stopped",
        };
        write(&mut g, 5, meta_x, status, dim());

        // Waveform row, just below the body. A green bar per amplitude byte; before the
        // first frame (or on silence) it is a flat baseline.
        if wave_h > 0 {
            let wy = (2 + body).min(h.saturating_sub(2));
            let frame = if self.bars.is_empty() {
                vec![0u8; art_cols.max(24).min(w.saturating_sub(3))]
            } else {
                self.bars.clone()
            };
            let wave_pen = Pen { fg: Color::Rgb(0x3f, 0xb9, 0x50), bg: bg(), ..Pen::default() };
            for (i, b) in frame.iter().enumerate() {
                let col = 2 + i;
                if col >= w.saturating_sub(1) {
                    break;
                }
                write(&mut g, wy, col, &crate::media::bar_block(*b).to_string(), wave_pen);
            }
        }

        // Control hint on the bottom row.
        let hint = "space play/pause   n/p next/prev   +/- volume   Esc close";
        let hint: String = hint.chars().take(w.saturating_sub(4)).collect();
        write(&mut g, h.saturating_sub(1), 2, &hint, dim());

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- docs ------------------------------------------------------------------

// ---- file viewer -----------------------------------------------------------

/// What the viewer is showing: text lines, or an image as half-block art.
///
/// Read-only, and deliberately so. runnir is a terminal: its editor is whatever
/// runs in a pane (`e` sends the path to `$EDITOR`). A real editor — undo,
/// encodings, huge files, LSP — is a bigger project than the sidebar it hangs off,
/// and it would compete with neovim forever.
pub enum Viewed {
    Text {
        lines: Vec<String>,
        /// The file was longer than the viewer's limit and is cut here.
        truncated: bool,
    },
    Image {
        art: Vec<Vec<HalfCell>>,
        /// Real pixel size, which the art cannot show.
        size: (u32, u32),
        /// The picture itself, for the GPU. `None` when it could not be decoded to
        /// pixels, and then the art is what gets drawn — it is a fallback, not a
        /// lesser mode: it is also what still works when there is no texture path.
        texture: Option<ViewTexture>,
    },
    /// A file that is neither, or one that could not be read.
    Note(String),
}

/// A decoded picture ready to be handed to the renderer.
///
/// Already scaled to the box it will be drawn in: uploading a 6000x4000 photo to
/// show it 60 cells wide is megabytes of texture for a picture nobody sees at that
/// size, and the scaling happens on the worker that decoded it either way.
pub struct ViewTexture {
    /// Minted once, when the file is read. The panel is rebuilt every frame, and a
    /// serial minted per frame re-uploads the texture per frame.
    pub serial: u64,
    pub rgba: std::sync::Arc<Vec<u8>>,
    pub px: (u32, u32),
    /// The box it was scaled for, in CELLS, aspect already corrected for a cell
    /// being taller than it is wide.
    pub cells: (usize, usize),
}

pub struct FileViewer {
    pub path: std::path::PathBuf,
    pub body: Viewed,
    pub scroll: usize,
    /// Wrap long lines instead of clipping them. Off by default: code reads better
    /// clipped, and the horizontal offset says what was cut.
    pub wrap: bool,
    pub left: usize,
    pub bytes: u64,
}

impl FileViewer {
    pub fn new(path: std::path::PathBuf, body: Viewed, bytes: u64) -> Self {
        Self { path, body, scroll: 0, wrap: false, left: 0, bytes }
    }

    pub fn len(&self) -> usize {
        match &self.body {
            Viewed::Text { lines, .. } => lines.len(),
            // A real texture is drawn whole, in a box that fits the panel: there is
            // nothing below the fold to scroll to, and a key that moves a number
            // nobody can see is a key that looks broken.
            Viewed::Image { texture: Some(_), .. } => 1,
            Viewed::Image { art, .. } => art.len(),
            Viewed::Note(_) => 1,
        }
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.len().saturating_sub(1) as isize;
        self.scroll = (self.scroll as isize + delta).clamp(0, max) as usize;
    }

    pub fn scroll_side(&mut self, delta: isize) {
        self.left = (self.left as isize + delta).max(0) as usize;
    }

    pub fn to_end(&mut self) {
        self.scroll = self.len().saturating_sub(1);
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.saturating_sub(4).max(40);
        let h = rows.saturating_sub(4).max(10);
        let mut g = panel_grid(w, h, theme);
        let body_rows = h.saturating_sub(3);

        // Header: the path, elided from the LEFT so the file name survives, and the
        // size, which is the other thing you want before reading.
        let path = elide_left(&self.path.display().to_string(), w.saturating_sub(20));
        write(&mut g, 0, 2, &path, accent());
        let meta = match &self.body {
            Viewed::Image { size, .. } => format!("{}\u{d7}{}  {}", size.0, size.1, human(self.bytes)),
            _ => format!("{} lines  {}", self.len(), human(self.bytes)),
        };
        let mw = meta.chars().count();
        if w > mw + 4 {
            write(&mut g, 0, w - mw - 2, &meta, dim());
        }

        match &self.body {
            Viewed::Text { lines, truncated } => {
                let num_w = lines.len().to_string().chars().count().max(3) + 1;
                for (i, line) in lines.iter().skip(self.scroll).take(body_rows).enumerate() {
                    let y = 2 + i;
                    let n = self.scroll + i + 1;
                    write(
                        &mut g,
                        y,
                        2,
                        &format!("{n:>width$} ", width = num_w),
                        Pen { fg: Color::Rgb(0x6a, 0x6d, 0x74), bg: bg(), ..Pen::default() },
                    );
                    let x = 2 + num_w + 1;
                    let room = w.saturating_sub(x + 1);
                    // Tabs are expanded here rather than in the file: the grid has no
                    // tab stops, so a raw \t collapses to one cell and the column
                    // structure of every indented file disappears.
                    let text = expand_tabs(line);
                    let shown: String = text.chars().skip(self.left).collect();
                    write(&mut g, y, x, &elide(&shown, room), normal());
                }
                if *truncated {
                    let y = h.saturating_sub(2);
                    write(&mut g, y, 2, "\u{2026} file is longer than the viewer reads", dim());
                }
            }
            // The real thing: the decoded picture as a texture, placed in the panel's
            // grid and drawn by the renderer over the cells it reserves.
            Viewed::Image { texture: Some(t), .. } => {
                let (avail_c, avail_r) = (w.saturating_sub(4).max(1), body_rows.max(1));
                // The box was chosen when the file was read, against a guess at the
                // panel size. Shrink both sides by the SAME factor if it does not
                // fit: clamping them apart is how a picture gets stretched.
                let k = (avail_c as f32 / t.cells.0.max(1) as f32)
                    .min(avail_r as f32 / t.cells.1.max(1) as f32)
                    .min(1.0);
                let c = ((t.cells.0 as f32 * k).round() as usize).clamp(1, avail_c);
                let r = ((t.cells.1 as f32 * k).round() as usize).clamp(1, avail_r);
                // Centred both ways: the panel is sized to the window and not to the
                // picture, so a picture pinned to its top-left reads as a panel that
                // failed to lay out rather than as a picture.
                let x = 2 + (avail_c - c) / 2;
                let y = 2 + (avail_r - r) / 2;
                g.place_image_at(t.serial, t.rgba.clone(), t.px, (y, x), (c, r));
            }
            Viewed::Image { art, .. } => {
                // Half-block art: one cell carries two vertical pixels. The fallback
                // for a picture that decoded to art but not to pixels.
                for (r, line) in art.iter().skip(self.scroll).take(body_rows).enumerate() {
                    for (c, cell) in line.iter().enumerate() {
                        if 2 + c >= w {
                            break;
                        }
                        let pen = Pen {
                            fg: Color::Rgb(cell.top.0, cell.top.1, cell.top.2),
                            bg: Color::Rgb(cell.bottom.0, cell.bottom.1, cell.bottom.2),
                            ..Pen::default()
                        };
                        write(&mut g, 2 + r, 2 + c, "\u{2580}", pen);
                    }
                }
            }
            Viewed::Note(text) => {
                write(&mut g, 2, 2, &elide(text, w.saturating_sub(4)), normal());
            }
        }

        let legend = match &self.body {
            Viewed::Image { texture: Some(_), .. } => {
                "e $EDITOR \u{b7} o open with the system \u{b7} y path \u{b7} esc back"
            }
            Viewed::Image { .. } => "j k scroll \u{b7} e $EDITOR \u{b7} o open with the system \u{b7} esc back",
            _ => "j k / J K scroll \u{b7} h l sideways \u{b7} e $EDITOR \u{b7} o system \u{b7} y path \u{b7} esc back",
        };
        write(&mut g, h - 1, 2, &elide(legend, w.saturating_sub(4)), dim());

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 2;
        vec![Panel { grid: g, col, row }]
    }
}

/// Expands tabs to the next 4-column stop. The grid has no tab stops of its own, so
/// an unexpanded `\t` eats the indentation of every file it appears in.
fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c == '\t' {
            let pad = 4 - (out.chars().count() % 4);
            out.push_str(&" ".repeat(pad));
        } else {
            out.push(c);
        }
    }
    out
}

/// A byte count a person can read at a glance.
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut n = bytes as f64;
    let mut u = 0;
    while n >= 1024.0 && u + 1 < UNITS.len() {
        n /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{n:.1}{}", UNITS[u]) }
}

// ---- properties panel ------------------------------------------------------

/// One path's properties, with its permission bits editable in place.
///
/// The bits are a 3x3 grid because that is what they are — owner/group/other by
/// read/write/execute — and a grid you move around in is the one shape where you
/// can see what you are about to change. Nothing is written until Enter.
pub struct PropsPanel {
    pub props: crate::explorer::Props,
    /// The mode being edited; `props.mode` is what is on disk.
    pub mode: u32,
    /// Which of the nine bits the cursor is on, 0 = owner-read.
    pub bit: usize,
    /// Apply to everything inside, for a directory. Off by default, and the confirm
    /// names the count before it happens.
    pub recursive: bool,
    pub message: Option<String>,
}

impl PropsPanel {
    pub fn new(props: crate::explorer::Props) -> Self {
        let mode = props.mode.unwrap_or(0o644) & 0o777;
        Self { props, mode, bit: 0, recursive: false, message: None }
    }

    pub fn move_bit(&mut self, delta: i32) {
        self.bit = (self.bit as i32 + delta).rem_euclid(9) as usize;
    }

    /// Flips the bit under the cursor. Bit 0 is owner-read, which is the high bit
    /// of the nine — the same order `rwxrwxrwx` reads in.
    pub fn toggle_bit(&mut self) {
        self.mode ^= 1 << (8 - self.bit);
    }

    pub fn dirty(&self) -> bool {
        self.props.mode.map(|m| m & 0o777) != Some(self.mode & 0o777)
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.saturating_sub(6).clamp(44, 78);
        let h = 16usize.min(rows.saturating_sub(4)).max(12);
        let mut g = panel_grid(w, h, theme);

        let name = self
            .props
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.props.path.display().to_string());
        write(&mut g, 0, 2, &elide(&name, w.saturating_sub(4)), accent());
        write(&mut g, 1, 2, &elide_left(&self.props.path.display().to_string(), w.saturating_sub(4)), dim());

        let kind = if self.props.dir { "directory" } else { "file" };
        let line = match self.props.contents {
            Some((files, dirs)) => {
                format!("{kind} \u{b7} {files} files, {dirs} directories inside")
            }
            None => format!("{kind} \u{b7} {}", human(self.props.size)),
        };
        write(&mut g, 3, 2, &elide(&line, w.saturating_sub(4)), normal());
        write(&mut g, 4, 2, &format!("modified {}", stamp(self.props.mtime)), dim());

        // A symlink says, before anything is changed, that the change lands on the
        // target: `set_permissions` follows links and there is no portable way not
        // to, so the only honest thing is to name what will actually be touched.
        if let Some(target) = &self.props.link_target {
            write(
                &mut g,
                5,
                2,
                &elide(
                    &format!("symlink \u{2192} {} (permissions apply THERE)", target.display()),
                    w.saturating_sub(4),
                ),
                Pen { fg: Color::Rgb(0xf5, 0xd5, 0x43), bg: bg(), ..Pen::default() },
            );
        }

        // The bit grid: three groups of three, the cursor boxed.
        let y = 7;
        write(&mut g, y, 2, "permissions", dim());
        let labels = ["owner", "group", "other"];
        let letters = ['r', 'w', 'x'];
        for (gi, label) in labels.iter().enumerate() {
            let row = y + 1 + gi;
            write(&mut g, row, 4, label, dim());
            for (li, letter) in letters.iter().enumerate() {
                let idx = gi * 3 + li;
                let on = self.mode >> (8 - idx) & 1 == 1;
                let text = if on { letter.to_string() } else { "-".to_string() };
                let pen = if idx == self.bit {
                    selected()
                } else if on {
                    Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), bg: bg(), ..Pen::default() }
                } else {
                    dim()
                };
                write(&mut g, row, 12 + li * 4, &format!(" {text} "), pen);
            }
        }
        let octal = format!("{:o}  {}", self.mode & 0o777, crate::explorer::mode_string(self.mode));
        write(&mut g, y + 1, 28, &octal, if self.dirty() { accent() } else { dim() });
        if self.props.dir {
            let mark = if self.recursive { "[x]" } else { "[ ]" };
            write(&mut g, y + 2, 28, &format!("{mark} everything inside (R)"), dim());
        }

        let foot = match &self.message {
            Some(m) => m.clone(),
            None => {
                "hjkl move \u{b7} space toggle \u{b7} enter apply \u{b7} r rename \u{b7} d delete \u{b7} esc back"
                    .to_string()
            }
        };
        write(&mut g, h - 1, 2, &elide(&foot, w.saturating_sub(4)), dim());

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

/// A unix timestamp as a local date and time.
fn stamp(secs: u64) -> String {
    // No chrono in the tree, and a date is not worth one: this is the civil-date
    // algorithm (Howard Hinnant's days_from_civil, inverted), in UTC.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

pub struct Docs {
    lines: Vec<(String, Pen)>,
    scroll: usize,
}

impl Docs {
    pub fn new(text: &str) -> Self {
        let lines = text
            .lines()
            .map(|l| {
                // A leading '#' marks a heading; '@' a key hint line.
                if let Some(h) = l.strip_prefix("# ") {
                    (h.to_string(), accent())
                } else if let Some(h) = l.strip_prefix("@ ") {
                    (h.to_string(), dim())
                } else {
                    (l.to_string(), normal())
                }
            })
            .collect();
        Self { lines, scroll: 0 }
    }

    pub fn scroll(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.lines.len().saturating_sub(1) as isize) as usize;
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let w = cols.saturating_sub(6).clamp(20, 100);
        let h = rows.saturating_sub(4).max(6);
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, "runnir — help   (Esc to close, ↑/↓ to scroll)", accent());
        let body = h - 2;
        for (line, (text, pen)) in self.lines.iter().skip(self.scroll).take(body).enumerate() {
            let clipped: String = text.chars().take(w.saturating_sub(4)).collect();
            write(&mut g, 2 + line, 2, &clipped, *pen);
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 2;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- prompt (rename, ask, connect) ----------------------------------------

pub struct Prompt {
    pub kind: PromptKind,
    pub label: String,
    pub input: String,
    pub suggestions: Vec<String>,
    pub cursor: usize,
}

/// How many suggestion rows a prompt renders (and thus how far Down navigates).
const PROMPT_ROWS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    RenameTab,
    QuickConnect,
    /// Natural-language description to translate into a shell command.
    AiCommand,
    /// A whisper: a natural-language instruction turned into terminal actions.
    Whisper,
    /// A destructive command held at Enter by the guardian: confirm to run it.
    GuardedCommand,
    /// A shell-history line to type at the prompt (fuzzy-picked, not run).
    HistoryInsert,
    /// A keyword to watch for in the focused pane's output (empty clears it).
    WatchKeyword,
    /// A named layout to launch in a new tab.
    LaunchLayout,
    /// A shell command to pipe the last command's output through, in a new split.
    PipeLastOutput,
    /// A shell command to pipe the whole scrollback through, in a new split.
    PipeScrollback,
    /// A directory to auto-preview new images from (empty clears the watch).
    ImageWatchDir,
    /// A commit message, typed in the git panel. Confirming commits the staged set
    /// and reopens the panel on the result.
    GitCommit,
    /// A new branch name, typed in the git panel; confirming creates and switches.
    GitBranch,
    /// A new tag name, typed in the git panel.
    GitTag,
    /// Text to narrow the panel's log to, matched against commit messages.
    GitLogFilter,
    /// What to do with an executable text file from the explorer: view, edit, run,
    /// or hand it to the desktop. A chooser rather than a guess — a script is all
    /// of those things at once.
    ExplorerAction,
    /// Confirmation before something that RUNS is run: an executable binary, or a
    /// `.desktop` file whose handler `xdg-open` would execute.
    ExplorerRun,
    /// A new name for the path under the tree's cursor.
    ExplorerRename,
    /// A name to create beside it; a trailing `/` makes a directory.
    ExplorerCreate,
    /// Confirmation before deleting, naming what is inside when it is a directory.
    ExplorerDelete,
    /// Confirmation before a recursive permission change, naming the count.
    ExplorerChmod,
    /// Confirmation before removing a docker object, naming what goes with it.
    DockerRemove,
    /// Confirmation before a docker command that reaches another machine, or that
    /// stops a whole compose project. The host is NAMED in the label.
    DockerRemote,
    /// Closing the window (or the last pane) while something is still running.
    /// Answered with y/n, never with typing: the question is whether to kill work,
    /// and a text field would invite Enter — the one key most likely to be pressed
    /// by reflex.
    ConfirmQuit,
}

impl PromptKind {
    /// Whether this prompt is a yes/no question rather than a field to type in.
    /// A confirm draws no input line and takes no characters.
    pub fn is_confirm(self) -> bool {
        matches!(
            self,
            PromptKind::ConfirmQuit
                | PromptKind::ExplorerRun
                | PromptKind::ExplorerDelete
                | PromptKind::ExplorerChmod
                | PromptKind::DockerRemove
                | PromptKind::DockerRemote
        )
    }
}

impl Prompt {
    pub fn new(kind: PromptKind, label: &str, suggestions: Vec<String>) -> Self {
        Self { kind, label: label.into(), input: String::new(), suggestions, cursor: 0 }
    }

    /// A prompt that starts with text already in it — a rename box that made you
    /// retype the name would be a worse way to change one letter of it.
    pub fn with_input(kind: PromptKind, label: &str, input: String) -> Self {
        Self { kind, label: label.into(), input, suggestions: Vec::new(), cursor: 0 }
    }

    pub fn input_char(&mut self, c: char) {
        self.input.push(c);
        self.cursor = 0;
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        // Only the first PROMPT_ROWS suggestions are rendered, so navigation stops
        // there — otherwise the highlight would leave the visible list and Enter
        // would insert an entry the user never saw. Type to narrow the list instead.
        let cap = self.visible().len().min(PROMPT_ROWS);
        if self.cursor + 1 < cap {
            self.cursor += 1;
        }
    }

    /// Suggestions matching the current input.
    pub fn visible(&self) -> Vec<String> {
        if self.input.is_empty() {
            return self.suggestions.clone();
        }
        let q = self.input.to_lowercase();
        self.suggestions.iter().filter(|s| s.to_lowercase().contains(&q)).cloned().collect()
    }

    /// What confirming yields: the highlighted suggestion, or the raw input.
    pub fn value(&self) -> String {
        self.visible().get(self.cursor).cloned().unwrap_or_else(|| self.input.clone())
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let visible = self.visible();
        // The box grows with what is being typed, up to the window, and only then
        // does the text scroll inside it. Both halves matter: a fixed box hides a
        // long answer, and a box that only ever grows would be a modal wider than
        // the screen.
        let base = (cols * 6 / 10).clamp(30, 70);
        let want = self.input.chars().count() + self.label.chars().count().min(20) + 8;
        let w = base.max(want).min(cols.saturating_sub(4)).max(30);
        let list = visible.len().min(PROMPT_ROWS);
        let h = 3 + list;
        let mut g = panel_grid(w, h, theme);

        write(&mut g, 0, 2, &field_view(&self.label, w.saturating_sub(4)), accent());
        if self.kind.is_confirm() {
            // A confirm has no field and no highlighted row: nothing here is being
            // picked, and a selection bar would read as "this one is about to
            // happen". The answer keys go where the field would have been.
            write(&mut g, 1, 2, "y", accent());
            write(&mut g, 1, 4, "close it", normal());
            write(&mut g, 1, 15, "\u{b7}", dim());
            write(&mut g, 1, 17, "n", accent());
            write(&mut g, 1, 19, "stay here (esc)", normal());
            for (i, s) in visible.iter().take(list).enumerate() {
                write(&mut g, 3 + i, 2, &elide(s, w.saturating_sub(4)), dim());
            }
        } else {
            // Room for "> ", the caret cell and a right margin.
            let field = w.saturating_sub(6);
            let shown = field_view(&self.input, field);
            let line = format!("> {shown}");
            write(&mut g, 1, 2, &line, normal());
            write(&mut g, 1, 2 + line.chars().count(), " ", selected());

            for (i, s) in visible.iter().take(list).enumerate() {
                let row = 3 + i;
                let pen = if i == self.cursor { selected() } else { normal() };
                if i == self.cursor {
                    write(&mut g, row, 0, &" ".repeat(w), selected());
                }
                write(&mut g, row, 2, s, pen);
            }
        }

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 3;
        vec![Panel { grid: g, col, row }]
    }
}

// ---- AI panel --------------------------------------------------------------

pub struct AiPanel {
    pub provider: String,
    pub input: String,
    pub transcript: Vec<AiLine>,
    pub busy: bool,
    scroll: usize,
}

pub struct AiLine {
    pub who: Who,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Who {
    You,
    Assistant,
    System,
}

impl AiPanel {
    pub fn new(provider: String) -> Self {
        Self { provider, input: String::new(), transcript: Vec::new(), busy: false, scroll: 0 }
    }

    pub fn push(&mut self, who: Who, text: String) {
        self.transcript.push(AiLine { who, text });
        self.scroll = 0;
    }

    pub fn input_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        // Anchored to the right third, full height: a side panel, not a modal, so
        // you can read the terminal and the answer at once.
        let w = (cols / 3).clamp(30, 60).min(cols.saturating_sub(2));
        let h = rows.saturating_sub(2).max(6);
        let mut g = panel_grid(w, h, theme);

        let head = format!("AI · {}{}", self.provider, if self.busy { " · thinking…" } else { "" });
        write(&mut g, 0, 2, &head, accent());

        // Wrap the transcript into the panel width, newest at the bottom.
        let inner = w.saturating_sub(4);
        let mut wrapped: Vec<(Who, String)> = Vec::new();
        for line in &self.transcript {
            for chunk in wrap(&line.text, inner) {
                wrapped.push((line.who, chunk));
            }
            wrapped.push((line.who, String::new()));
        }
        let body = h.saturating_sub(4);
        let start = wrapped.len().saturating_sub(body + self.scroll);
        for (i, (who, text)) in wrapped.iter().skip(start).take(body).enumerate() {
            let pen = match who {
                Who::You => accent(),
                Who::Assistant => normal(),
                Who::System => dim(),
            };
            write(&mut g, 2 + i, 2, text, pen);
        }

        let prompt = format!("> {}", self.input);
        write(&mut g, h - 1, 2, &prompt, normal());
        write(&mut g, h - 1, 2 + prompt.chars().count(), " ", selected());

        let col = cols.saturating_sub(w);
        vec![Panel { grid: g, col, row: 1 }]
    }
}

// ---- hint mode -------------------------------------------------------------

/// A screen target the user can jump to by typing its label.
pub struct Hint {
    pub label: String,
    pub abs_row: usize,
    pub col: usize,
    pub text: String,
    pub kind: HintKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintKind {
    Url,
    Path,
    Hash,
    /// A local branch of the pane's repository. Recognised by name against the real
    /// ref list, never by shape — see `hints::Context`.
    Branch,
}

pub struct Hints {
    pub hints: Vec<Hint>,
    pub typed: String,
    /// Set when any character of the label was typed in upper case, which asks for
    /// the alternate action ("show me this" rather than "copy this"). Sticky across
    /// a two-character label, so `Ab` and `aB` both mean the same thing — a shift
    /// held for one key of a chord is not a different intent.
    alt: bool,
}

impl Hints {
    pub fn new(hints: Vec<Hint>) -> Self {
        Self { hints, typed: String::new(), alt: false }
    }

    pub fn input(&mut self, c: char) -> HintResult {
        self.alt |= c.is_uppercase();
        self.typed.push(c.to_ascii_lowercase());
        let matches: Vec<&Hint> =
            self.hints.iter().filter(|h| h.label.starts_with(&self.typed)).collect();
        match matches.as_slice() {
            [] => HintResult::NoMatch,
            [only] if only.label == self.typed => {
                HintResult::Chosen(only.text.clone(), only.kind, self.alt)
            }
            _ => HintResult::More,
        }
    }
}

pub enum HintResult {
    More,
    NoMatch,
    /// The chosen target, its kind, and whether the alternate action was asked for.
    Chosen(String, HintKind, bool),
}

/// Two-letter labels from a home-row alphabet, enough for ~600 targets, assigned
/// so no label is a prefix of another.
pub fn hint_labels(n: usize) -> Vec<String> {
    const ALPHA: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";
    if n <= ALPHA.len() {
        return ALPHA.iter().take(n).map(|&b| (b as char).to_string()).collect();
    }
    let mut out = Vec::new();
    for &a in ALPHA {
        for &b in ALPHA {
            out.push(format!("{}{}", a as char, b as char));
            if out.len() == n {
                return out;
            }
        }
    }
    out
}

// ---- fuzzy + wrap ----------------------------------------------------------

/// Subsequence match: every character of `needle`, in order, appears in `hay`.
/// The palette does not need ranking, only filtering.
fn fuzzy(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut chars = hay.chars();
    needle.chars().all(|nc| chars.any(|hc| hc == nc))
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in para.split(' ') {
            if line.chars().count() + word.chars().count() + 1 > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            // A word longer than the panel is hard-split rather than overflowing.
            if word.chars().count() > width {
                for chunk in word.chars().collect::<Vec<_>>().chunks(width) {
                    out.push(chunk.iter().collect());
                }
            } else {
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

// Silence unused warnings for helper kept for symmetry.
#[allow(dead_code)]
fn _cell_marker() -> Cell {
    Cell { ch: ' ', pen: Pen { flags: Flags::empty(), ..Pen::default() } }
}

#[cfg(test)]
mod docker_tests {
    use super::*;
    use crate::docker::{Container, Endpoint, Health, Host, Image, Kind, Network, Snapshot, Volume};

    fn container(name: &str, project: Option<&str>, running: bool) -> Container {
        Container {
            id: format!("id-{name}"),
            name: name.into(),
            image: "img:1".into(),
            state: if running { "running".into() } else { "exited".into() },
            status: if running { "Up 3 days".into() } else { "Exited (0) 1 day ago".into() },
            health: None,
            project: project.map(str::to_string),
            service: Some(name.into()),
            ports: Vec::new(),
            volumes: Vec::new(),
            config_files: vec!["/srv/app/docker-compose.yml".into()],
            created: 0,
        }
    }

    fn panel(containers: Vec<Container>) -> DockerPanel {
        let hosts = vec![
            Host {
                name: "default".into(),
                endpoint: Endpoint::Unix("/var/run/docker.sock".into()),
                from_context: true,
                version: Some("29.5.2".into()),
                error: None,
                current: true,
            },
            Host::hub(),
        ];
        let mut p = DockerPanel::new(hosts);
        p.snap = Snapshot { containers, ..Snapshot::default() };
        p.rebuild();
        p
    }

    #[test]
    fn containers_are_grouped_under_the_compose_project_they_belong_to() {
        let mut p = panel(vec![
            container("api", Some("qlaios"), true),
            container("db", Some("qlaios"), false),
            container("stray", None, true),
        ]);
        // A heading per project, its members under it, and the loose ones LAST:
        // a heading after a flat list reads as part of the list.
        assert_eq!(p.rows.len(), 5);
        assert!(matches!(&p.rows[0], DockerRow::Project { name: Some(n), total: 2, up: 1, .. } if n == "qlaios"));
        assert!(matches!(&p.rows[3], DockerRow::Project { name: None, total: 1, up: 1, .. }));

        // Folding a project hides its members and nothing else.
        p.set_cursor(0);
        assert!(p.toggle_project());
        assert_eq!(p.rows.len(), 3, "the two members are folded away");
        assert!(p.toggle_project());
        assert_eq!(p.rows.len(), 5);

        // A container row answers for its project too, so a compose verb works
        // from the member you happen to be standing on.
        p.set_cursor(1);
        assert_eq!(p.selected_project().as_deref(), Some("qlaios"));
        assert_eq!(p.selected_name().as_deref(), Some("api"));
    }

    #[test]
    fn a_refresh_holds_the_cursor_on_the_same_container_not_the_same_row() {
        let mut p = panel(vec![
            container("api", Some("qlaios"), true),
            container("db", Some("qlaios"), true),
            container("web", Some("qlaios"), true),
        ]);
        p.set_cursor(2);
        assert_eq!(p.selected_name().as_deref(), Some("db"));
        // `api` goes away between reads: every row under it shifts up, and a cursor
        // that stayed on the index would be on `web` just as a key is pressed.
        let mut snap = p.snap.clone();
        snap.containers.remove(0);
        p.apply_snapshot(snap);
        assert_eq!(p.selected_name().as_deref(), Some("db"));
    }

    #[test]
    fn switching_kind_keeps_a_cursor_per_kind_and_never_asks_for_logs_of_a_network() {
        let mut p = panel(vec![container("api", None, true)]);
        p.snap.networks = vec![
            Network { id: "n1".into(), name: "bridge".into(), driver: "bridge".into(), subnet: None },
            Network { id: "n2".into(), name: "qlaios-net".into(), driver: "bridge".into(), subnet: None },
        ];
        p.snap.images = vec![Image { id: "sha256:abc".into(), tags: vec!["a:1".into()], ..Image::default() }];
        p.detail = DockerDetail::Logs;
        p.set_kind(Kind::Networks);
        // Logs are a container word. Landing on a network with the detail still set
        // to "logs" would ask the daemon for something that does not exist.
        assert_eq!(p.detail, DockerDetail::Summary);
        p.set_cursor(1);
        assert_eq!(p.selected_name().as_deref(), Some("qlaios-net"));

        // Each kind remembers its own cursor.
        p.set_kind(Kind::Containers);
        assert_eq!(p.cursor(), 0);
        p.set_kind(Kind::Networks);
        assert_eq!(p.selected_name().as_deref(), Some("qlaios-net"));
    }

    #[test]
    fn the_summary_says_what_a_row_cannot_fit() {
        let mut c = container("api", Some("qlaios"), true);
        c.health = Some(Health::Unhealthy);
        c.ports = vec![(8080, 80, "tcp".into())];
        let mut p = panel(vec![c]);
        p.set_cursor(1);
        let text = p.summary().join("\n");
        // Up and UNHEALTHY is the state this panel exists to make visible.
        assert!(text.contains("unhealthy"), "{text}");
        assert!(text.contains("8080"), "{text}");
        assert!(text.contains("qlaios / api"), "{text}");
    }

    #[test]
    fn three_columns_shrink_to_two_before_any_of_them_goes_under_its_minimum() {
        let p = panel(vec![container("api", None, true)]);
        let wide = p.layout(200, 50);
        assert!(wide.hosts_w >= MIN_COL && wide.objects_w >= MIN_COL);
        assert!(wide.detail_w() >= MIN_DETAIL);
        assert!(wide.sep2() > wide.objects_x());

        // Narrow: the HOSTS column is the one that goes. It is a list of two names
        // you can also reach with a key; the other two are the panel itself.
        let narrow = p.layout(52, 20);
        assert_eq!(narrow.hosts_w, 0);
        assert!(narrow.objects_w >= MIN_COL);
        assert_eq!(narrow.separators().len(), 1);

        // Zoomed there is nothing to hit but the detail.
        let mut z = panel(vec![container("api", None, true)]);
        z.zoom = true;
        let l = z.layout(200, 50);
        assert_eq!((l.hosts_w, l.objects_w), (0, 0));
        assert!(l.separators().is_empty());
    }

    #[test]
    fn a_click_lands_on_what_it_looks_like_it_hit() {
        let p = panel(vec![container("api", Some("qlaios"), true)]);
        let (cols, rows) = (120, 30);
        let l = p.layout(cols, rows);
        // The kind strip, at the columns it is drawn at.
        assert_eq!(p.hit(cols, rows, l.col + 2, l.row), Some(DockerHit::Kind(Kind::Containers)));
        assert_eq!(p.hit(cols, rows, l.col + 5, l.row), Some(DockerHit::Kind(Kind::Images)));
        // A host row, an object row, and the separator between their columns.
        assert_eq!(p.hit(cols, rows, l.col + 1, l.row + 2), Some(DockerHit::HostRow(0)));
        assert_eq!(p.hit(cols, rows, l.col + l.hosts_w, l.row + 4), Some(DockerHit::Separator(0)));
        assert_eq!(p.hit(cols, rows, l.col + l.objects_x(), l.row + 2), Some(DockerHit::Row(0)));
        assert_eq!(p.hit(cols, rows, l.col + l.detail_x(), l.row + 3), Some(DockerHit::Detail));
        // Outside the panel is not a hit at all, which is what closes it.
        assert_eq!(p.hit(cols, rows, 0, 0), None);
    }

    #[test]
    fn dragging_a_separator_cannot_turn_the_panel_inside_out() {
        let mut p = panel(vec![container("api", None, true)]);
        // Dragging the first separator past the second pushes the second along
        // instead of leaving a column with a negative width.
        p.drag_split(0, 90, 100);
        assert!(p.split[1] > p.split[0]);
        p.drag_split(1, 2, 100);
        assert!(p.split[1] > p.split[0]);
        // And the layout still gives every column its minimum.
        let l = p.layout(200, 40);
        assert!(l.hosts_w >= MIN_COL && l.objects_w >= MIN_COL && l.detail_w() >= MIN_DETAIL);
    }


    #[test]
    fn the_detail_column_scrolls_without_overflowing_or_pinning_itself() {
        let mut p = panel(vec![container("api", Some("qlaios"), true)]);
        p.detail = DockerDetail::Inspect;
        p.detail_lines = (0..50).map(|i| format!("line {i}")).collect();
        // `G` passes i32::MAX. Adding that to a scroll that is already past zero
        // overflows — a panic with the debug profile's checks on, and a jump to the
        // TOP in release. Twice in a row is the sequence that finds it.
        p.scroll_detail(i32::MAX);
        p.scroll_detail(i32::MAX);
        assert_eq!(p.detail_scroll, 49);
        p.scroll_detail(-i32::MAX);
        assert_eq!(p.detail_scroll, 0);

        // The summary is built on the fly, so scrolling has to measure what is
        // DRAWN: against the stored lines it was pinned at zero.
        p.detail = DockerDetail::Summary;
        p.set_cursor(0);
        assert!(p.detail_len() > 1, "a project heading lists its members");
        p.scroll_detail(1);
        assert_eq!(p.detail_scroll, 1);
    }

    #[test]
    fn a_rebuild_that_loses_the_selection_drops_the_detail_with_it() {
        let mut p = panel(vec![
            container("api", Some("qlaios"), true),
            container("db", Some("qlaios"), true),
        ]);
        p.set_cursor(1);
        p.detail = DockerDetail::Logs;
        p.detail_lines = vec!["a log line".into()];
        p.detail_for = Some(("id-api".into(), DockerDetail::Logs));

        // `api` is removed elsewhere: the cursor falls onto its neighbour, and the
        // logs on screen belong to the container that left — under the new row's
        // name, which is the confusion the title exists to prevent.
        let mut snap = p.snap.clone();
        snap.containers.remove(0);
        p.apply_snapshot(snap);
        assert!(p.detail_lines.is_empty());
        assert_eq!(p.detail_for, None);
    }

    #[test]
    fn the_leader_only_offers_what_this_row_can_do() {
        let mut p = panel(vec![container("api", Some("qlaios"), true)]);
        // On the project heading: the compose verbs, not the container ones.
        p.set_cursor(0);
        p.arm_leader();
        p.leader_key('c');
        assert!(
            !p.leader_entries().iter().any(|(k, _, _)| k == "s"),
            "start is a container verb: {:?}",
            p.leader_entries()
        );
        assert_eq!(p.leader_key('s'), None, "and pressing it ends the sequence");
        assert!(p.leader.is_none());

        // On the container: the same group offers it, and it presses the key the
        // panel already binds.
        p.set_cursor(1);
        p.arm_leader();
        p.leader_key('c');
        assert_eq!(p.leader_key('s'), Some(DockerPress::OnContainer(DockerKey::Ch('s'))));

        // The compose verbs are offered from the member row too, because the row
        // belongs to a project.
        p.arm_leader();
        p.leader_key('p');
        assert_eq!(p.leader_key('u'), Some(DockerPress::OnProject(DockerKey::Ch('U'))));

        // A leaf can take you somewhere before it acts, the way the git panel's
        // menu can jump into the log and act in one sequence: the image verbs are
        // offered from a container row and switch the column on the way.
        p.arm_leader();
        p.leader_key('i');
        assert_eq!(
            p.leader_key('i'),
            Some(DockerPress::Switch(crate::docker::Kind::Images)),
            "and the plain switch is offered too"
        );

        // Hub verbs are not offered on a daemon, and the kind strip is not offered
        // on hub: a menu entry that cannot work is worse than a missing one.
        p.arm_leader();
        p.leader_key('b');
        assert!(!p.leader_entries().iter().any(|(k, _, _)| k == "o"), "{:?}", p.leader_entries());
        p.cancel_leader();
        p.host_cursor = 1;
        p.arm_leader();
        p.leader_key('o');
        assert!(
            !p.leader_entries().iter().any(|(k, _, _)| k == "c"),
            "no kind strip on hub: {:?}",
            p.leader_entries()
        );
    }

    #[test]
    fn hub_is_a_host_in_the_same_column_but_has_no_kind_strip() {
        let mut p = panel(Vec::new());
        assert!(!p.on_hub());
        p.host_cursor = 1;
        assert!(p.on_hub());
        // No strip means no kind to hit in the header — but the header is still
        // INSIDE the panel, and a click inside it must not put the panel away.
        assert_eq!(p.hit(120, 30, 4, 2), Some(DockerHit::Inside));
        assert_eq!(p.hit(120, 30, 0, 0), None, "outside is still outside");
    }

    #[test]
    fn the_command_lines_carry_the_host_and_the_compose_files() {
        let local = Host {
            name: "default".into(),
            endpoint: Endpoint::Unix("/var/run/docker.sock".into()),
            from_context: true,
            version: None,
            error: None,
            current: true,
        };
        let remote = Host {
            name: "cloudmax".into(),
            endpoint: Endpoint::Ssh("root@cloudmax".into()),
            from_context: true,
            version: None,
            error: None,
            current: false,
        };
        // The current local context needs no flag; anything else is named, exactly
        // as it would be typed.
        assert_eq!(crate::docker::cli_prefix(&local), ["docker"]);
        assert_eq!(crate::docker::cli_prefix(&remote), ["docker", "-c", "cloudmax"]);

        let files = vec!["/srv/app/docker-compose.yml".to_string()];
        // Locally, the CLI with the context flag.
        assert_eq!(
            crate::docker::compose_command(&local, "qlaios", &files, &["up", "-d"]),
            ["docker", "compose", "-p", "qlaios", "-f", "/srv/app/docker-compose.yml", "up", "-d"]
        );
        // Over ssh, the whole compose runs THERE: `-c` would redirect only the
        // daemon connection while the local client read `-f` off the local disk,
        // and those paths came off labels written on the remote machine.
        assert_eq!(
            crate::docker::compose_command(&remote, "qlaios", &files, &["up", "-d"]),
            [
                "ssh",
                "-t",
                "root@cloudmax",
                "docker compose -p qlaios -f /srv/app/docker-compose.yml up -d"
            ]
        );

        // A deploy is a CHAIN, so it has to reach a shell: as argv with a bare `&&`
        // in the middle the shell would look for a program named after the whole
        // first command.
        let deploy = crate::docker::deploy_command(&local, "qlaios", &files);
        assert_eq!(&deploy[..2], ["sh", "-c"]);
        assert!(deploy[2].contains("pull && "), "{deploy:?}");
        let remote_deploy = crate::docker::deploy_command(&remote, "qlaios", &files);
        assert_eq!(remote_deploy[0], "ssh");
        assert!(remote_deploy.last().unwrap().contains("pull && "), "{remote_deploy:?}");

        // A port in the context endpoint is a `-p`, never part of the hostname.
        assert_eq!(
            crate::docker::ssh_destination("root@host:2222"),
            ("root@host".to_string(), Some("2222".to_string()))
        );
        assert_eq!(crate::docker::ssh_destination("root@host"), ("root@host".to_string(), None));
        // The shell is picked INSIDE the container: plenty of images have no bash.
        let exec = crate::docker::exec_command(&local, "abc");
        assert_eq!(&exec[..4], ["docker", "exec", "-it", "abc"]);
        assert!(exec.last().unwrap().contains("exec bash"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn palette_filters_by_subsequence() {
        let mut p = Palette::new(&HashMap::new());
        let before = p.filtered.len();
        assert!(before > 5);
        for c in "split".chars() {
            p.input(c);
        }
        assert!(p.filtered.len() < before);
        // Every survivor must actually contain the subsequence.
        for &i in &p.filtered {
            assert!(fuzzy(&p.all[i].0.title().to_lowercase(), "split"));
        }
    }

    #[test]
    fn palette_selection_moves_and_clamps() {
        let mut p = Palette::new(&HashMap::new());
        p.up(); // already at top
        assert_eq!(p.cursor, 0);
        p.down();
        assert_eq!(p.cursor, 1);
        assert!(p.selected().is_some());
    }

    #[test]
    fn fuzzy_needs_every_char_in_order() {
        assert!(fuzzy("new tab", "ntab"));
        assert!(fuzzy("new tab", "nt"));
        assert!(!fuzzy("new tab", "tn"), "order matters");
        assert!(fuzzy("anything", ""));
    }

    #[test]
    fn hint_labels_never_prefix_each_other() {
        // If one label were a prefix of another, typing it could not disambiguate.
        let labels = hint_labels(200);
        assert_eq!(labels.len(), 200);
        assert!(labels.iter().all(|l| l.len() == 2), "past the alphabet all are 2 chars");
        let set: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(set.len(), 200, "labels must be unique");
    }

    #[test]
    fn a_long_prompt_keeps_the_end_of_what_you_typed_visible() {
        // The caret sits at the end of these fields, so the end is what must stay
        // on screen — clipping the tail hides the character just typed.
        assert_eq!(field_view("hello", 10), "hello");
        // Five cells total: the ellipsis costs one, so four characters survive.
        assert_eq!(field_view("abcdefghij", 5), "\u{2026}ghij");
        assert_eq!(field_view("", 5), "");
        assert_eq!(field_view("abc", 0), "");
    }

    #[test]
    fn the_prompt_box_grows_with_the_text_then_scrolls_inside_it() {
        let long = "a".repeat(400);
        let mut p = Prompt::new(PromptKind::AiCommand, "Describe the command", Vec::new());
        for c in "short".chars() {
            p.input_char(c);
        }
        let narrow = p.render(100, 40, &Theme::default());
        let w_short = narrow[0].grid.cols();

        p.input = long.clone();
        let wide = p.render(100, 40, &Theme::default());
        let w_long = wide[0].grid.cols();
        assert!(w_long > w_short, "the box has to grow: {w_short} -> {w_long}");
        // ...but never past the window it sits in.
        assert!(w_long <= 96, "a modal wider than the screen is not a fix: {w_long}");

        // And with the box at its limit, the text scrolls so the tail shows.
        let row: String = (0..wide[0].grid.cols())
            .map(|c| wide[0].grid.abs_cell(1, c).ch)
            .collect();
        assert!(row.contains('\u{2026}'), "a clipped field must say so: {row:?}");
        assert!(row.trim_end().ends_with('a'), "the end of the input must be visible: {row:?}");
    }

    #[test]
    fn a_confirm_prompt_shows_the_answers_and_no_field() {
        let p = Prompt::new(
            PromptKind::ConfirmQuit,
            "Close runnir? 2 commands are still running",
            vec!["tab 1: claude".into(), "tab 2: cargo build".into()],
        );
        let panels = p.render(120, 40, &Theme::default());
        let g = &panels[0].grid;
        let row = |r: usize| -> String { (0..g.cols()).map(|c| g.abs_cell(r, c).ch).collect() };

        assert!(row(0).contains("2 commands are still running"));
        // The answer keys, where a typing prompt would have put its field. A `>`
        // there would invite Enter, which this prompt must never accept.
        let answers = row(1);
        assert!(answers.contains('y') && answers.contains('n'), "{answers:?}");
        assert!(!answers.contains('>'), "a confirm has no input field: {answers:?}");
        // What closing would kill, listed and never highlighted as a choice.
        assert!(row(3).contains("claude"), "{:?}", row(3));
        assert!(row(4).contains("cargo build"), "{:?}", row(4));
    }

    #[test]
    fn the_git_panel_hit_test_agrees_with_what_it_draws() {
        use crate::git::{Commit, FileEntry};
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.files = (0..40)
            .map(|i| FileEntry { path: format!("src/f{i}.rs"), index: '.', worktree: 'M' })
            .collect();
        let (cols, rows) = (120usize, 40usize);
        let l = p.layout(cols, rows);

        // Outside the panel is not a hit, so a click there can mean "close".
        assert!(p.hit(cols, rows, 0, 0).is_none());

        // The header's view labels, measured the way they are written: two cells of
        // padding either side of the title, one cell between.
        let mut x = l.col + 2;
        for v in GitPanel::VIEWS {
            let hit = p.hit(cols, rows, x + 1, l.row);
            assert!(matches!(hit, Some(GitHit::View(w)) if w == v), "{} at {x}", v.title());
            x += v.title().chars().count() + 3;
        }

        // A list row maps to the entry drawn on it, including when the list has
        // scrolled: the same `cursor - (body_rows - 1)` the renderer uses.
        assert!(matches!(p.hit(cols, rows, l.col + 1, l.row + 2), Some(GitHit::Row(0))));
        assert!(matches!(p.hit(cols, rows, l.col + 1, l.row + 5), Some(GitHit::Row(3))));
        p.set_cursor(39);
        let scrolled = p.cursor() - (l.body_rows - 1);
        assert!(
            matches!(p.hit(cols, rows, l.col + 1, l.row + 2), Some(GitHit::Row(i)) if i == scrolled),
            "a scrolled list must not report the row that used to be there"
        );

        // Past the list divider is the diff, reported by row of the preview.
        assert!(matches!(
            p.hit(cols, rows, l.col + l.list_w + 3, l.row + 2),
            Some(GitHit::PreviewLine(0))
        ));

        // A commit opens a THIRD column beside the list, which keeps its own rows.
        p.set_cursor(0);
        p.enter_commit("abc1234".into());
        p.commit_files = vec![
            FileEntry { path: "a.rs".into(), index: 'M', worktree: '.' },
            FileEntry { path: "b.rs".into(), index: 'A', worktree: '.' },
        ];
        assert_eq!(p.len(), 40, "the list is still the list");
        assert_eq!(p.files_len(), 2);
        let l3 = p.layout(cols, rows);
        assert!(l3.files_w >= MIN_COL && l3.prev_w() >= MIN_DIFF, "three usable columns");
        assert!(matches!(p.hit(cols, rows, l3.col + 1, l3.row + 2), Some(GitHit::Row(0))));
        assert!(matches!(
            p.hit(cols, rows, l3.col + l3.files_x() + 1, l3.row + 3),
            Some(GitHit::FileRow(1))
        ));
        assert!(matches!(
            p.hit(cols, rows, l3.col + l3.prev_x() + 1, l3.row + 2),
            Some(GitHit::PreviewLine(0))
        ));
        // Both separators are hittable, or a column cannot be dragged.
        for (i, sep) in [l3.sep1(), l3.sep2()].into_iter().enumerate() {
            let sep = sep.expect("three columns means two separators");
            assert!(
                matches!(p.hit(cols, rows, l3.col + sep, l3.row + 4), Some(GitHit::Separator(n)) if n == i),
                "separator {i} at {sep}"
            );
        }

        // Dragging the first separator moves it, and the layout follows.
        let before = p.layout(cols, rows).list_w;
        p.drag_split(0, 20, l3.w);
        let after = p.layout(cols, rows).list_w;
        assert!(after < before, "dragging left narrows the list: {before} -> {after}");
        // ...but never past the point where a column stops being one.
        p.drag_split(0, 0, l3.w);
        let l4 = p.layout(cols, rows);
        assert!(l4.list_w >= MIN_COL && l4.files_w >= MIN_COL && l4.prev_w() >= MIN_DIFF);

        // Zoomed, the diff is the only column, and it starts at the left edge.
        p.toggle_zoom();
        let lz = p.layout(cols, rows);
        assert_eq!((lz.list_w, lz.files_w, lz.prev_x()), (0, 0, 0));
        assert_eq!(lz.prev_w(), lz.w);
        assert!(matches!(p.hit(cols, rows, lz.col + 1, lz.row + 2), Some(GitHit::PreviewLine(0))));
        let _ = Commit::default();
    }

    #[test]
    fn the_panel_leader_descends_and_only_offers_what_this_view_can_do() {
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.view = GitView::Status;
        assert!(p.leader_entries().is_empty(), "disarmed shows nothing");

        p.arm_leader();
        let root = p.leader_entries();
        assert!(root.iter().any(|(k, _, group)| k == "c" && *group), "Commit is a group");
        assert!(root.iter().any(|(k, _, group)| k == "q" && !*group), "close is a leaf");

        // Descending into a group keeps the layer armed and shows its verbs.
        assert!(p.leader_key('f').is_none(), "a group does not run anything");
        assert_eq!(p.leader_path(), vec!["f".to_string()]);
        let files = p.leader_entries();
        assert!(files.iter().any(|(k, _, _)| k == "space"), "stage is on space: {files:?}");
        assert!(files.iter().any(|(k, _, _)| k == "b"), "blame belongs to the status view");

        // In the log view the status-only verbs are not offered, and pressing one
        // ends the sequence instead of doing something unrelated.
        p.cancel_leader();
        p.view = GitView::Log;
        p.arm_leader();
        p.leader_key('f');
        let files = p.leader_entries();
        assert!(!files.iter().any(|(k, _, _)| k == "b"), "blame needs the status view: {files:?}");
        assert_eq!(p.leader_key('b'), None);
        assert!(p.leader.is_none(), "a miss disarms rather than waiting for another key");

        // A verb that stands on its own runs from anywhere.
        p.arm_leader();
        p.leader_key('c');
        assert_eq!(p.leader_key('l'), Some(GitPress::View(GitView::Log)));
        assert!(p.leader.is_none(), "running a leaf disarms");
    }

    #[test]
    fn every_leader_leaf_presses_a_key_the_panel_answers() {
        // The tree is a discovery layer over the panel's own keys. A leaf whose key
        // the panel does not handle would be a menu entry that does nothing.
        fn walk(level: &'static [GitEntry], out: &mut Vec<(char, GitPress)>) {
            for e in level {
                match &e.node {
                    GitNode::Leaf(p) => out.push((e.key, *p)),
                    GitNode::Group(g) => walk(g, out),
                }
            }
        }
        let mut leaves = Vec::new();
        walk(GIT_LEADER, &mut leaves);
        assert!(leaves.len() > 30, "the tree should cover the panel: {}", leaves.len());

        // Keys the panel binds, as characters. Kept beside `git_panel_key`: adding a
        // leaf for a key that is not here is the mistake this catches.
        const BOUND: &str = "qjklhzvJK][su1234567taACOTeLbmRxicnPpfyor/S";
        for (key, press) in leaves {
            let k = match press {
                GitPress::View(_) => continue,
                GitPress::Key(k)
                | GitPress::Then(_, k)
                | GitPress::In(_, k)
                | GitPress::InDiff(k) => k,
            };
            if let GitKey::Ch(c) = k {
                assert!(BOUND.contains(c), "leader {key:?} presses {c:?}, which the panel ignores");
            }
        }
    }

    #[test]
    fn moving_the_list_closes_the_commit_column_it_no_longer_matches() {
        use crate::git::FileEntry;
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.view = GitView::Log;
        p.log = (0..5).map(|i| crate::git::Commit { sha: format!("sha{i}"), ..Default::default() }).collect();
        p.enter_commit("sha0".into());
        p.commit_files = vec![FileEntry { path: "a.rs".into(), index: 'M', worktree: '.' }];
        assert_eq!(p.focus, GitFocus::Files, "opening a commit moves the keyboard into it");

        p.down();
        assert!(p.in_commit(), "j in the file column moves files, not the log");

        p.focus = GitFocus::List;
        p.down();
        assert!(!p.in_commit(), "the log moved: those files were the other commit's");
        assert_eq!(p.focus, GitFocus::List);
    }

    /// A panel with one commit open on one file, its diff loaded and zoomed.
    fn zoomed_on_a_commit_file() -> GitPanel {
        use crate::git::FileEntry;
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.view = GitView::Log;
        p.log = (0..5)
            .map(|i| crate::git::Commit { sha: format!("sha{i}"), ..Default::default() })
            .collect();
        p.enter_commit("sha0".into());
        p.commit_files = vec![FileEntry { path: "a.rs".into(), index: 'M', worktree: '.' }];
        p.set_preview("diff --git a/x b/x\n@@ -1 +1,2 @@\n one\n+two\n".into());
        p.toggle_zoom();
        p
    }

    #[test]
    fn switching_view_takes_the_open_commit_and_its_zoom_with_it() {
        let mut p = zoomed_on_a_commit_file();
        assert!(p.zoom && p.in_commit());

        // The keyboard paths (1..7 and Tab) used to switch bare, which left a
        // commit from the log open beside the status list, zoom and all.
        p.set_view(GitView::Status);
        assert!(!p.in_commit(), "the commit belonged to the view we left");
        assert!(!p.zoom, "and so did the zoom over one of its files");
        assert!(p.commit_files.is_empty());
        assert_eq!(p.focus, GitFocus::List);

        let mut p = zoomed_on_a_commit_file();
        p.cycle_view();
        assert!(!p.in_commit() && !p.zoom, "Tab is the same switch");
        assert_eq!(p.focus, GitFocus::List);
    }

    #[test]
    fn leaving_a_zoomed_diff_lands_in_the_column_that_chose_it() {
        let mut p = zoomed_on_a_commit_file();
        assert!(p.diff_focus(), "zoom hands the keyboard to the diff");

        // What Escape does from a zoom, and what Enter has to do too: Enter used to
        // fall through to the Log arm and re-enter the commit it was reading, ending
        // full width with the keyboard driving a column that is not on screen.
        p.leave_diff();
        assert!(!p.zoom);
        assert!(p.in_commit(), "backing out of the zoom is not backing out of the commit");
        assert_eq!(p.focus, GitFocus::Files);

        // With no commit open, the column that chose the file is the list.
        p.leave_commit();
        p.toggle_zoom();
        p.leave_diff();
        assert!(!p.zoom);
        assert_eq!(p.focus, GitFocus::List);

        // A commit opened while a zoom is still set does not inherit it: the zoom
        // hides the very column being opened.
        p.zoom = true;
        p.enter_commit("sha1".into());
        assert!(!p.zoom);
        assert_eq!(p.focus, GitFocus::Files);
    }

    #[test]
    fn the_focused_diff_is_what_a_step_moves() {
        let mut p = zoomed_on_a_commit_file();
        assert!(p.diff_focus());
        p.step_diff(1);
        assert!(p.zoom && p.in_commit(), "stepping the diff leaves the columns alone");

        // `down()` does not consult the focus for the list: it moves the list from
        // anywhere but the file column. That is why j/k and the arrows are both
        // guarded on `diff_focus()` — one unguarded arrow drops the zoom, closes the
        // commit and moves the log, all at once.
        p.down();
        assert!(!p.in_commit() && !p.zoom, "the list moved, and took the rest with it");
    }

    #[test]
    fn no_column_is_ever_drawn_narrower_than_its_minimum() {
        use crate::git::FileEntry;
        let file = |n: &str| FileEntry { path: n.into(), index: 'M', worktree: '.' };
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.files = vec![file("a.rs")];
        let check = |p: &GitPanel, cols: usize| {
            let l = p.layout(cols, 40);
            assert!(l.list_w >= MIN_COL, "list {} at {cols} cols", l.list_w);
            assert!(l.files_w == 0 || l.files_w >= MIN_COL, "files {} at {cols}", l.files_w);
            assert!(l.prev_w() >= MIN_DIFF, "diff {} at {cols} cols", l.prev_w());
            l
        };
        for cols in 20..=200 {
            assert_eq!(check(&p, cols).files_w, 0, "no commit open, no file column");
        }

        // Three columns, at every width and wherever the separators were dragged to.
        p.enter_commit("abc1234".into());
        p.commit_files = vec![file("a.rs")];
        for cols in 20..=200 {
            for first in [0.05f32, 0.2, 0.34, 0.6, 0.95] {
                p.split = [first, (first + 0.24).min(0.95)];
                check(&p, cols);
            }
        }

        // Too narrow for three, the FILE column is the one that goes: the diff is
        // what the panel promises, and 12 + 12 + 20 plus the rules does not fit.
        p.split = [0.34, 0.58];
        assert_eq!(p.layout(51, 40).files_w, 0, "three would each be under their minimum");
        assert!(p.layout(52, 40).files_w >= MIN_COL, "and the first width that fits draws it");
    }

    #[test]
    fn the_diff_group_offers_the_line_selection_only_where_it_works() {
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.view = GitView::Status;
        p.arm_leader();
        p.leader_key('d');
        let from_list = p.leader_entries();
        assert!(
            !from_list.iter().any(|(k, _, _)| k == "v"),
            "`v` is bound only inside the diff: {from_list:?}"
        );
        assert!(
            from_list.iter().any(|(k, _, _)| k == "l"),
            "the way into the diff is what this column can do: {from_list:?}"
        );
        assert_eq!(p.leader_key('v'), None, "pressing it anyway ends the sequence");

        p.set_preview("diff --git a/x b/x\n@@ -1 +1,2 @@\n one\n+two\n".into());
        p.enter_diff();
        p.arm_leader();
        p.leader_key('d');
        let from_diff = p.leader_entries();
        assert!(
            from_diff.iter().any(|(k, _, _)| k == "v"),
            "with the diff focused it is a real verb: {from_diff:?}"
        );
        assert_eq!(p.leader_key('v'), Some(GitPress::InDiff(GitKey::Ch('v'))));
    }

    #[test]
    fn line_staging_selects_a_range_inside_the_focused_hunk() {
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.set_preview(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,4 @@\n one\n+two\n+three\n four\n"
                .into(),
        );
        p.enter_diff();
        // The cursor starts on the first CHANGED line, skipping context: a context
        // line cannot be staged, so starting there would make the first key do
        // nothing.
        assert_eq!(p.diff_cursor, 5);
        p.toggle_anchor();
        p.step_diff(1);
        assert_eq!(p.line_range(), (5, 6), "anchor to cursor, in order");
        let patch = p.line_patch().expect("a patch for the picked lines");
        assert!(patch.contains("+two"));
        assert!(patch.contains("+three"));

        // With only one line picked, the other addition must not be in the patch.
        p.toggle_anchor();
        p.step_diff(-1);
        let patch = p.line_patch().expect("a patch");
        assert!(patch.contains("+two"), "{patch}");
        assert!(!patch.contains("+three"), "{patch}");
    }

    #[test]
    fn a_rebase_plan_reverses_into_git_order_and_moves_steps() {
        use crate::git::{Commit, RebaseAction};
        let c = |sha: &str| Commit { sha: sha.into(), ..Commit::default() };
        // The panel lists newest first, git replays oldest first.
        let mut plan = RebasePlan::new("base000".into(), vec![c("ccc"), c("bbb"), c("aaa")]);
        assert_eq!(plan.todo(), "pick aaa\npick bbb\npick ccc\n");

        plan.cursor = 0;
        plan.set_action(RebaseAction::Fixup);
        assert_eq!(plan.todo(), "pick aaa\npick bbb\nfixup ccc\n");

        // Moving a step carries the cursor with it, or the next key would act on a
        // different commit than the one that just moved.
        plan.move_step(1);
        assert_eq!(plan.cursor, 1);
        assert_eq!(plan.todo(), "pick aaa\nfixup ccc\npick bbb\n");
    }

    #[test]
    fn a_click_on_a_diff_row_finds_its_hunk() {
        let mut p = GitPanel::new(std::path::PathBuf::from("/tmp/repo"));
        p.set_preview(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n@@ -9,2 +9,2 @@\n nine\n-ten\n+TEN\n"
                .into(),
        );
        // Row 4 is inside the first hunk, row 9 inside the second.
        assert_eq!(p.hunk_at(4), Some(0));
        assert_eq!(p.hunk_at(9), Some(1));
        // Metadata above the first @@ belongs to no hunk, so a click there stages
        // nothing by accident.
        assert_eq!(p.hunk_at(0), None);
    }

    #[test]
    fn hints_resolve_on_full_label() {
        let hints = vec![
            Hint { label: "a".into(), abs_row: 0, col: 0, text: "x".into(), kind: HintKind::Url },
            Hint { label: "s".into(), abs_row: 1, col: 0, text: "y".into(), kind: HintKind::Path },
        ];
        let mut h = Hints::new(hints);
        assert!(matches!(h.input('a'), HintResult::Chosen(_, _, false)));

        // The same label typed shifted asks for the alternate action instead.
        let hints = vec![Hint {
            label: "a".into(),
            abs_row: 0,
            col: 0,
            text: "x".into(),
            kind: HintKind::Hash,
        }];
        let mut h = Hints::new(hints);
        assert!(matches!(h.input('A'), HintResult::Chosen(_, _, true)));
    }

    #[test]
    fn wrap_breaks_at_width_and_hard_splits_long_words() {
        let lines = wrap("the quick brown fox", 9);
        assert!(lines.iter().all(|l| l.chars().count() <= 9), "{lines:?}");
        let long = wrap("supercalifragilistic", 5);
        assert!(long.iter().all(|l| l.chars().count() <= 5));
    }

    #[test]
    fn theme_picker_filters_and_navigates() {
        let mut p = ThemePicker::new(Theme::default());
        let before = p.filtered.len();
        assert!(before >= 20, "the picker should list every builtin");
        // Typing narrows the list to matching names.
        for c in "nord".chars() {
            p.input(c);
        }
        assert!(p.filtered.len() < before);
        assert!(p.selected_name().unwrap().to_lowercase().contains("nord"));
        // Backspacing widens it again, and refiltering resets the cursor to the top.
        for _ in 0..4 {
            p.backspace();
        }
        assert_eq!(p.filtered.len(), before);
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn theme_picker_selection_moves_clamps_and_previews() {
        let original = Theme::default();
        let mut p = ThemePicker::new(original.clone());
        p.up(); // already at the top: must not underflow
        assert_eq!(p.cursor, 0);
        let first = p.selected_theme().unwrap();
        p.down();
        assert_eq!(p.cursor, 1);
        let second = p.selected_theme().unwrap();
        assert_ne!(first.background, second.background, "moving must preview a new theme");
        // The theme active on open is preserved verbatim for a cancel to restore.
        assert_eq!(p.original().background, original.background);
        assert_eq!(p.original().ansi.len(), 16);
    }

    fn snippet(name: &str, command: &str, description: &str, run_now: bool) -> SnippetDef {
        SnippetDef {
            name: name.into(),
            command: command.into(),
            description: description.into(),
            run_now,
        }
    }

    #[test]
    fn snippet_picker_fuzzy_matches_name_and_description() {
        let snips = vec![
            snippet("deploy", "git push", "ship the branch to prod", false),
            snippet("logs", "journalctl -f", "tail the service logs", false),
            snippet("build", "cargo build", "compile the crate", true),
        ];
        let mut p = SnippetPicker::new(snips);
        assert_eq!(p.filtered.len(), 3, "empty query lists every snippet");

        // Subsequence over the name.
        for c in "dep".chars() {
            p.input(c);
        }
        assert_eq!(p.selected().unwrap().name, "deploy");

        // Filtering by a word only in the description still finds the snippet, and
        // the returned snippet carries its command and run_now flag intact.
        p.backspace();
        p.backspace();
        p.backspace();
        for c in "prod".chars() {
            p.input(c);
        }
        let hit = p.selected().unwrap();
        assert_eq!(hit.name, "deploy");
        assert_eq!(hit.command, "git push");
        assert!(!hit.run_now);
    }

    #[test]
    fn snippet_picker_selection_moves_clamps_and_refilters() {
        let snips = vec![
            snippet("one", "echo 1", "", false),
            snippet("two", "echo 2", "", true),
        ];
        let mut p = SnippetPicker::new(snips);
        p.up(); // already at the top: must not underflow
        assert_eq!(p.cursor, 0);
        assert_eq!(p.selected().unwrap().name, "one");
        p.down();
        assert_eq!(p.cursor, 1);
        let two = p.selected().unwrap();
        assert_eq!(two.name, "two");
        assert!(two.run_now, "the run_now flag rides along with the selection");
        p.down(); // past the end: clamps
        assert_eq!(p.cursor, 1);

        // Typing refilters and snaps the cursor back to the top.
        p.input('o');
        p.input('n');
        p.input('e');
        assert_eq!(p.cursor, 0);
        assert_eq!(p.selected().unwrap().name, "one");

        // A query that matches nothing leaves no selection rather than panicking.
        p.input('z');
        assert!(p.selected().is_none());
    }

    #[test]
    fn clip_picker_previews_filters_and_pastes_full_entry() {
        let entries: std::collections::VecDeque<String> =
            ["first line\nsecond line", "cargo build", "hello world"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let mut p = ClipHistoryPicker::new(&entries);
        // Selecting the top entry yields its full (multi-line) text, not the preview.
        assert_eq!(p.selected().as_deref(), Some("first line\nsecond line"));
        // The preview is one line, first non-blank, marked as multi-line.
        assert!(p.all[0].1.starts_with("first line"));
        assert!(p.all[0].1.contains('\u{00b6}'), "multi-line entries are marked");
        // Typing filters against the full entry text; a match on a body line surfaces it.
        for c in "second".chars() {
            p.input(c);
        }
        assert_eq!(p.filtered.len(), 1);
        assert_eq!(p.selected().as_deref(), Some("first line\nsecond line"));
    }

    #[test]
    fn prompt_value_prefers_highlighted_suggestion() {
        let mut p = Prompt::new(PromptKind::QuickConnect, "ssh", vec!["a".into(), "b".into()]);
        assert_eq!(p.value(), "a");
        p.down();
        assert_eq!(p.value(), "b");
        // With no match, raw input is returned so you can type a new host.
        p.input_char('z');
        assert_eq!(p.value(), "z");
    }
}

// ---- the docker panel ------------------------------------------------------
//
// Same contract as the git panel: three columns, resizable, zoomable, with a leader
// layer of its own. Not a sidebar (a container table does not fit in thirty
// columns) and not a full-screen modal (it would cover the shell, which is where
// the thing the panel is watching gets run).

/// Which column the keyboard is driving.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerFocus {
    Hosts,
    Objects,
    Detail,
}

/// What the detail column is showing about the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DockerDetail {
    /// Fields worth reading, laid out. What `docker ps` cannot tell you.
    #[default]
    Summary,
    /// The container's log tail.
    Logs,
    /// The whole inspect JSON.
    Inspect,
}

impl DockerDetail {
    pub fn label(self) -> &'static str {
        match self {
            DockerDetail::Summary => "summary",
            DockerDetail::Logs => "logs",
            DockerDetail::Inspect => "inspect",
        }
    }
}

/// One row of the object column. A compose project is a LEVEL of this tree, not a
/// filter: the work here is done in projects, and a flat container list makes you
/// reassemble one in your head every time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockerRow {
    /// A compose project heading: its name, how many containers, how many of them
    /// are up. `None` is the heading for containers that belong to no project.
    Project { name: Option<String>, total: usize, up: usize, open: bool },
    Container(usize),
    Image(usize),
    Volume(usize),
    Network(usize),
    /// A Docker Hub repository, and one of its tags. Hub has no kinds and no
    /// compose projects: it is repos, and inside a repo, tags.
    Repo(usize),
    Tag(usize),
}

pub struct DockerPanel {
    pub hosts: Vec<crate::docker::Host>,
    pub host_cursor: usize,
    pub kind: crate::docker::Kind,
    pub snap: crate::docker::Snapshot,
    pub rows: Vec<DockerRow>,
    /// Cursor per kind, so switching the strip and coming back lands where you left.
    cursors: [usize; 4],
    pub focus: DockerFocus,
    /// Compose projects that are folded shut, by name. `None` (the loose group) is
    /// never folded: it has no name to remember it by.
    pub collapsed: std::collections::HashSet<String>,
    pub split: [f32; 2],
    pub zoom: bool,
    pub detail: DockerDetail,
    pub detail_lines: Vec<String>,
    pub detail_scroll: usize,
    /// What the detail column is describing — the object AND the mode, so an answer
    /// that arrives after the cursor moved (or after the mode changed) is dropped
    /// instead of drawn under the wrong title. Logs of a chatty container come back
    /// slower than its inspect, and log lines under the word "inspect" are exactly
    /// the confusion the title was added to prevent.
    pub detail_for: Option<(String, DockerDetail)>,
    /// Docker Hub: the repositories, the one drilled into, and its tags.
    pub repos: Vec<crate::docker::HubRepo>,
    pub open_repo: Option<String>,
    pub tags: Vec<crate::docker::HubTag>,
    /// The images of the last DAEMON read, kept so the hub column can compare a
    /// published tag against what this machine holds. Hub's own snapshot is empty,
    /// and the comparison is the whole reason the hub column exists.
    pub local_images: Vec<crate::docker::Image>,
    /// Where the repository list came from, said out loud: an org token cannot read
    /// Hub's API, and a list quietly built from local images would read as the
    /// account's whole catalogue.
    pub repos_note: String,
    pub message: Result<String, String>,
    pub busy: bool,
    /// A read is in flight for the selected host.
    pub loading: bool,
    pub leader: Option<Vec<char>>,
}

impl DockerPanel {
    pub fn new(hosts: Vec<crate::docker::Host>) -> Self {
        Self {
            hosts,
            host_cursor: 0,
            kind: crate::docker::Kind::default(),
            snap: crate::docker::Snapshot::default(),
            rows: Vec::new(),
            cursors: [0; 4],
            focus: DockerFocus::Objects,
            collapsed: std::collections::HashSet::new(),
            split: [0.22, 0.58],
            zoom: false,
            detail: DockerDetail::default(),
            detail_lines: Vec::new(),
            detail_scroll: 0,
            detail_for: None,
            repos: Vec::new(),
            open_repo: None,
            tags: Vec::new(),
            local_images: Vec::new(),
            repos_note: String::new(),
            message: Ok(String::new()),
            busy: false,
            loading: false,
            leader: None,
        }
    }

    pub fn host(&self) -> Option<&crate::docker::Host> {
        self.hosts.get(self.host_cursor.min(self.hosts.len().saturating_sub(1)))
    }

    /// Whether the selected host is Docker Hub, which is not a daemon: the kind
    /// strip means nothing there and the columns hold repos and tags instead.
    pub fn on_hub(&self) -> bool {
        matches!(self.host().map(|h| &h.endpoint), Some(crate::docker::Endpoint::Hub))
    }

    fn kind_index(&self) -> usize {
        crate::docker::Kind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0)
    }

    pub fn cursor(&self) -> usize {
        self.cursors[self.kind_index()].min(self.rows.len().saturating_sub(1))
    }

    /// Drops whatever the detail column was showing.
    pub fn clear_detail(&mut self) {
        self.detail_lines.clear();
        self.detail_for = None;
        self.detail_scroll = 0;
    }

    pub fn set_cursor(&mut self, n: usize) {
        let n = n.min(self.rows.len().saturating_sub(1));
        let i = self.kind_index();
        if self.cursors[i] != n {
            // The detail belongs to the row it was read for; keeping it beside a
            // different selection is a lie the panel would keep telling.
            self.detail_lines.clear();
            self.detail_for = None;
            self.detail_scroll = 0;
        }
        self.cursors[i] = n;
    }

    pub fn move_cursor(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let n = (self.cursor() as i32 + delta).clamp(0, self.rows.len() as i32 - 1) as usize;
        self.set_cursor(n);
    }

    /// The row under the cursor.
    pub fn selected(&self) -> Option<&DockerRow> {
        self.rows.get(self.cursor())
    }

    pub fn selected_container(&self) -> Option<&crate::docker::Container> {
        match self.selected() {
            Some(DockerRow::Container(i)) => self.snap.containers.get(*i),
            _ => None,
        }
    }

    /// The id (or name) of whatever is selected, which is what every operation and
    /// every detail read is keyed by.
    pub fn selected_id(&self) -> Option<String> {
        match self.selected()? {
            DockerRow::Container(i) => self.snap.containers.get(*i).map(|c| c.id.clone()),
            DockerRow::Image(i) => self.snap.images.get(*i).map(|m| m.id.clone()),
            DockerRow::Volume(i) => self.snap.volumes.get(*i).map(|v| v.name.clone()),
            DockerRow::Network(i) => self.snap.networks.get(*i).map(|n| n.id.clone()),
            DockerRow::Repo(i) => self.repos.get(*i).map(|r| r.name.clone()),
            // A tag is identified by the repository it is in: two repositories both
            // have a `latest`, and a detail keyed on the bare name would answer for
            // whichever one asked last.
            DockerRow::Tag(i) => {
                let repo = self.open_repo.clone()?;
                self.tags.get(*i).map(|t| format!("{repo}:{}", t.name))
            }
            DockerRow::Project { .. } => None,
        }
    }

    /// The name a confirm or a message calls the selection by.
    pub fn selected_name(&self) -> Option<String> {
        match self.selected()? {
            DockerRow::Container(i) => self.snap.containers.get(*i).map(|c| c.name.clone()),
            DockerRow::Image(i) => self.snap.images.get(*i).map(|m| m.label()),
            DockerRow::Volume(i) => self.snap.volumes.get(*i).map(|v| v.name.clone()),
            DockerRow::Network(i) => self.snap.networks.get(*i).map(|n| n.name.clone()),
            DockerRow::Repo(i) => self.repos.get(*i).map(|r| r.name.clone()),
            DockerRow::Tag(i) => self.tags.get(*i).map(|t| t.name.clone()),
            DockerRow::Project { name, .. } => {
                Some(name.clone().unwrap_or_else(|| "(no project)".into()))
            }
        }
    }

    /// The compose project the cursor is in: the heading itself, or the project of
    /// the container under it. Operations on a project are offered from both.
    pub fn selected_project(&self) -> Option<String> {
        match self.selected()? {
            DockerRow::Project { name, .. } => name.clone(),
            DockerRow::Container(i) => self.snap.containers.get(*i)?.project.clone(),
            _ => None,
        }
    }

    /// Rebuilds the object column from the snapshot.
    ///
    /// Containers are grouped by compose project; everything else is a flat list,
    /// because a volume's project label is a hint about where it came from and not
    /// the level anyone navigates by.
    pub fn rebuild(&mut self) {
        let keep = self.selected_id();
        self.rebuild_keeping(keep);
    }

    /// Replaces the snapshot and rebuilds, holding the cursor on the object it was
    /// on. The id has to be read BEFORE the new snapshot lands: a row holds an
    /// INDEX into the container list, and the moment that list is replaced the index
    /// means a different container — reading it after is how a cursor "stays put"
    /// onto its neighbour.
    pub fn apply_snapshot(&mut self, snap: crate::docker::Snapshot) {
        let keep = self.selected_id();
        self.snap = snap;
        self.rebuild_keeping(keep);
    }

    fn rebuild_keeping(&mut self, keep: Option<String>) {
        let mut rows = Vec::new();
        // Hub has no kinds: it is repositories, and inside one, its tags.
        if self.on_hub() {
            if self.open_repo.is_some() {
                rows.extend((0..self.tags.len()).map(DockerRow::Tag));
            } else {
                rows.extend((0..self.repos.len()).map(DockerRow::Repo));
            }
            self.rows = rows;
            if let Some(id) = keep {
                if let Some(i) = (0..self.rows.len())
                    .find(|i| self.id_of(&self.rows[*i]).as_deref() == Some(id.as_str()))
                {
                    self.cursors[self.kind_index()] = i;
                    return;
                }
            }
            let i = self.kind_index();
            self.cursors[i] = self.cursors[i].min(self.rows.len().saturating_sub(1));
            return;
        }
        match self.kind {
            crate::docker::Kind::Containers => {
                for project in crate::docker::projects(&self.snap.containers) {
                    let members: Vec<usize> = self
                        .snap
                        .containers
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.project == project)
                        .map(|(i, _)| i)
                        .collect();
                    let up = members.iter().filter(|i| self.snap.containers[**i].running()).count();
                    let open = project
                        .as_ref()
                        .map(|p| !self.collapsed.contains(p))
                        .unwrap_or(true);
                    rows.push(DockerRow::Project {
                        name: project.clone(),
                        total: members.len(),
                        up,
                        open,
                    });
                    if open {
                        rows.extend(members.into_iter().map(DockerRow::Container));
                    }
                }
            }
            crate::docker::Kind::Images => {
                rows.extend((0..self.snap.images.len()).map(DockerRow::Image));
            }
            crate::docker::Kind::Volumes => {
                rows.extend((0..self.snap.volumes.len()).map(DockerRow::Volume));
            }
            crate::docker::Kind::Networks => {
                rows.extend((0..self.snap.networks.len()).map(DockerRow::Network));
            }
        }
        self.rows = rows;
        // Hold the cursor on the same OBJECT across a refresh: a container that
        // stops shifts every row under it, and a cursor that stays on an index
        // lands on a neighbour just as you press a key.
        if let Some(id) = keep {
            let at = (0..self.rows.len()).find(|i| {
                let row = &self.rows[*i];
                self.id_of(row).as_deref() == Some(id.as_str())
            });
            if let Some(i) = at {
                self.cursors[self.kind_index()] = i;
                return;
            }
            // The object it was on is GONE. The cursor lands on a neighbour, and
            // the logs still on screen belong to the one that left — under the new
            // row's name, which is the confusion the title exists to prevent.
            self.clear_detail();
        }
        let i = self.kind_index();
        self.cursors[i] = self.cursors[i].min(self.rows.len().saturating_sub(1));
    }

    fn id_of(&self, row: &DockerRow) -> Option<String> {
        match row {
            DockerRow::Container(i) => self.snap.containers.get(*i).map(|c| c.id.clone()),
            DockerRow::Image(i) => self.snap.images.get(*i).map(|m| m.id.clone()),
            DockerRow::Volume(i) => self.snap.volumes.get(*i).map(|v| v.name.clone()),
            DockerRow::Network(i) => self.snap.networks.get(*i).map(|n| n.id.clone()),
            DockerRow::Repo(i) => self.repos.get(*i).map(|r| r.name.clone()),
            DockerRow::Tag(i) => self.tags.get(*i).map(|t| t.name.clone()),
            DockerRow::Project { name, .. } => name.clone(),
        }
    }

    /// Folds a compose project shut, or opens it. Returns whether anything moved,
    /// so a caller knows whether to redraw.
    pub fn toggle_project(&mut self) -> bool {
        let Some(DockerRow::Project { name: Some(name), .. }) = self.selected().cloned() else {
            return false;
        };
        if !self.collapsed.remove(&name) {
            self.collapsed.insert(name);
        }
        self.rebuild();
        true
    }

    pub fn set_kind(&mut self, kind: crate::docker::Kind) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        self.detail_lines.clear();
        self.detail_for = None;
        self.detail_scroll = 0;
        // Logs and a summary are container words. Landing on an image with the
        // detail still set to "logs" would ask the daemon for something that does
        // not exist and show the error as if it were the image's.
        if !matches!(kind, crate::docker::Kind::Containers) {
            self.detail = DockerDetail::Summary;
        }
        self.rebuild();
    }

    /// Where the panel's parts sit, in cells. Shared by the renderer and the mouse,
    /// so a click lands where it looks like it should.
    pub fn layout(&self, cols: usize, rows: usize) -> DockerLayout {
        let w = cols.saturating_sub(4).max(40);
        let h = rows.saturating_sub(4).max(12);
        let body_rows = h.saturating_sub(3);
        let base = DockerLayout { col: 2, row: 2, w, h, hosts_w: 0, objects_w: 0, body_rows };
        // Zoomed, the detail gets the whole box: there is nothing else to hit.
        if self.zoom {
            return base;
        }
        let at = |f: f32| (w as f32 * f).round() as usize;
        // The widths have to add up to the box: the columns, plus a rule and a
        // gutter after each of the first two, plus the detail's own right margin.
        // Clamping each one against that total rather than against `w` is what keeps
        // the detail from ending up a column under its minimum when both separators
        // are dragged to the right.
        const THREE_CHROME: usize = 5;
        const TWO_CHROME: usize = 4;
        // Below this there is not room for three columns that each keep their
        // minimum, and the HOSTS column is the one that goes: it is a list of two or
        // three names you can also reach with a key, while the other two are the
        // panel itself.
        let three = w >= MIN_COL * 2 + MIN_DETAIL + THREE_CHROME;
        if !three {
            let room = w.saturating_sub(MIN_DETAIL + TWO_CHROME).max(MIN_COL);
            let objects_w = at(self.split[1]).clamp(MIN_COL, room);
            return DockerLayout { objects_w, ..base };
        }
        let room = w.saturating_sub(MIN_DETAIL + MIN_COL + THREE_CHROME).max(MIN_COL);
        let hosts_w = at(self.split[0]).clamp(MIN_COL, room);
        let objects_w = at(self.split[1])
            .saturating_sub(hosts_w + 2)
            .clamp(MIN_COL, w.saturating_sub(hosts_w + MIN_DETAIL + THREE_CHROME).max(MIN_COL));
        DockerLayout { hosts_w, objects_w, ..base }
    }

    /// Moves the keyboard between columns. The detail is only reachable when there
    /// is something in it — a focus on an empty column is a focus that swallows
    /// j/k for no reason.
    pub fn cycle_focus(&mut self, forward: bool, cols: usize, rows: usize) {
        let l = self.layout(cols, rows);
        let order = [DockerFocus::Hosts, DockerFocus::Objects, DockerFocus::Detail];
        // A column that is not DRAWN cannot take the keyboard: the hosts column is
        // dropped in a narrow window and both are dropped by zoom, and j/k there
        // would move an invisible selection — with Enter on it opening an ssh
        // connection to a host nobody could see.
        let usable = |f: DockerFocus| match f {
            DockerFocus::Hosts => l.hosts_w > 0,
            DockerFocus::Objects => l.objects_w > 0,
            // Empty, it swallows j/k for nothing — unless it is the only column
            // there is, which is what zoom leaves.
            DockerFocus::Detail => self.detail_len() > 0 || l.objects_w == 0,
        };
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(1);
        let n = order.len();
        let mut next = i;
        for _ in 0..n {
            next = if forward { (next + 1) % n } else { (next + n - 1) % n };
            if usable(order[next]) {
                break;
            }
        }
        if usable(order[next]) {
            self.focus = order[next];
        }
    }

    /// Puts the keyboard somewhere that exists, after a layout change (a zoom, or a
    /// window narrow enough to drop a column) took the focused one away.
    pub fn sync_focus(&mut self, cols: usize, rows: usize) {
        let l = self.layout(cols, rows);
        let gone = match self.focus {
            DockerFocus::Hosts => l.hosts_w == 0,
            DockerFocus::Objects => l.objects_w == 0,
            DockerFocus::Detail => false,
        };
        if gone {
            self.focus =
                if l.objects_w > 0 { DockerFocus::Objects } else { DockerFocus::Detail };
        }
    }

    /// Scrolls the detail column. `len` is what is actually DRAWN there, which in
    /// summary mode is not `detail_lines` at all — the summary is built on the fly,
    /// so scrolling against the stored lines pinned it at zero.
    pub fn scroll_detail(&mut self, delta: i32) {
        let len = self.detail_len();
        let max = len.saturating_sub(1) as i64;
        // Saturating: `G` passes `i32::MAX`, and adding that to a scroll position
        // that is already past zero overflows — a panic in the debug profile and a
        // jump to the TOP in release.
        let next = (self.detail_scroll as i64).saturating_add(delta as i64);
        self.detail_scroll = next.clamp(0, max.max(0)) as usize;
    }

    /// How many lines the detail column has, whichever mode it is in.
    pub fn detail_len(&self) -> usize {
        if self.detail == DockerDetail::Summary {
            self.summary().len()
        } else {
            self.detail_lines.len()
        }
    }

    /// The summary lines for the selection: what a row cannot fit and an inspect
    /// buries. Built here rather than fetched, so it needs no worker at all.
    pub fn summary(&self) -> Vec<String> {
        let now = crate::docker::now_secs();
        match self.selected() {
            Some(DockerRow::Container(i)) => {
                let Some(c) = self.snap.containers.get(*i) else { return Vec::new() };
                let mut out = vec![
                    format!("name     {}", c.name),
                    format!("image    {}", c.image),
                    format!("id       {}", crate::docker::short_id(&c.id)),
                    format!("state    {}", c.status),
                ];
                if let Some(h) = c.health {
                    out.push(format!("health   {} {}", h.mark(), h.label()));
                }
                if let (Some(p), Some(s)) = (&c.project, &c.service) {
                    out.push(format!("compose  {p} / {s}"));
                }
                out.push(format!("created  {}", crate::docker::ago(c.created, now)));
                if c.ports.is_empty() {
                    out.push("ports    none published".to_string());
                } else {
                    for (host, cont, proto) in &c.ports {
                        out.push(format!("port     {host} \u{2192} {cont}/{proto}"));
                    }
                }
                out
            }
            Some(DockerRow::Image(i)) => {
                let Some(m) = self.snap.images.get(*i) else { return Vec::new() };
                let mut out = vec![
                    format!("id       {}", crate::docker::short_id(&m.id)),
                    format!("size     {}", crate::docker::human_size(m.size)),
                    format!("created  {}", crate::docker::ago(m.created, now)),
                ];
                for t in &m.tags {
                    out.push(format!("tag      {t}"));
                }
                // The digest is what a registry can be asked about; the id cannot.
                for d in &m.digests {
                    out.push(format!("digest   {d}"));
                }
                let users: Vec<&str> = self
                    .snap
                    .containers
                    .iter()
                    .filter(|c| m.tags.iter().any(|t| *t == c.image))
                    .map(|c| c.name.as_str())
                    .collect();
                if !users.is_empty() {
                    out.push(format!("used by  {}", users.join(", ")));
                }
                out
            }
            Some(DockerRow::Volume(i)) => {
                let Some(v) = self.snap.volumes.get(*i) else { return Vec::new() };
                let mut out = vec![
                    format!("name     {}", v.name),
                    format!("driver   {}", v.driver),
                    format!("mount    {}", v.mountpoint),
                ];
                if let Some(p) = &v.project {
                    out.push(format!("compose  {p}"));
                }
                out
            }
            Some(DockerRow::Network(i)) => {
                let Some(n) = self.snap.networks.get(*i) else { return Vec::new() };
                let mut out = vec![
                    format!("name     {}", n.name),
                    format!("driver   {}", n.driver),
                    format!("id       {}", crate::docker::short_id(&n.id)),
                ];
                out.push(match &n.subnet {
                    Some(s) => format!("subnet   {s}"),
                    None => "subnet   none".to_string(),
                });
                out
            }
            Some(DockerRow::Repo(i)) => {
                let Some(r) = self.repos.get(*i) else { return Vec::new() };
                let mut out = vec![format!("repo     {}", r.name)];
                if r.private {
                    out.push("access   private".to_string());
                }
                if !r.last_updated.is_empty() {
                    out.push(format!("updated  {}", r.last_updated));
                }
                let local: Vec<&str> = self
                    .local_images
                    .iter()
                    .flat_map(|i| i.tags.iter())
                    .filter_map(|t| t.strip_prefix(&format!("{}:", r.name)))
                    .collect();
                out.push(if local.is_empty() {
                    "local    nothing from this repo".to_string()
                } else {
                    format!("local    {}", local.join(", "))
                });
                out.push(String::new());
                out.push("enter    read its tags".to_string());
                out
            }
            Some(DockerRow::Tag(i)) => {
                let Some(t) = self.tags.get(*i) else { return Vec::new() };
                let repo = self.open_repo.clone().unwrap_or_default();
                let d = crate::docker::drift(&self.local_images, &repo, &t.name, t.digest.as_deref());
                let mut out = vec![format!("tag      {repo}:{}", t.name)];
                out.push(match &t.digest {
                    Some(d) => format!("digest   {d}"),
                    None => "digest   reading\u{2026}".to_string(),
                });
                let local = self
                    .local_images
                    .iter()
                    .find(|i| i.tags.iter().any(|x| *x == format!("{repo}:{}", t.name)))
                    .and_then(|i| i.digest_for(&repo).map(str::to_string));
                out.push(match local {
                    Some(l) => format!("local    {l}"),
                    None => "local    not pulled here".to_string(),
                });
                if !d.label().is_empty() {
                    out.push(format!("state    {}", d.label()));
                }
                out
            }
            Some(DockerRow::Project { name, total, up, .. }) => {
                let label = name.clone().unwrap_or_else(|| "(no compose project)".into());
                let mut out =
                    vec![format!("project  {label}"), format!("running  {up} of {total}")];
                for c in self.snap.containers.iter().filter(|c| c.project == *name) {
                    let mark = if c.running() { '\u{25cf}' } else { '\u{25cb}' };
                    out.push(format!(
                        "{mark} {:<16} {}",
                        c.service.clone().unwrap_or_else(|| c.name.clone()),
                        c.short_status()
                    ));
                }
                out
            }
            None => Vec::new(),
        }
    }

    /// The text of one object row, and the pen it is drawn with.
    fn row_text(&self, i: usize, width: usize) -> (String, Pen) {
        let Some(row) = self.rows.get(i) else { return (String::new(), dim()) };
        match row {
            DockerRow::Project { name, total, up, open } => {
                let arrow = if *open { '\u{25be}' } else { '\u{25b8}' };
                let label = name.clone().unwrap_or_else(|| "(loose)".into());
                let count = format!("{up}/{total}");
                let text = format!("{arrow} {label}");
                let pad = width.saturating_sub(text.chars().count() + count.chars().count() + 1);
                (
                    elide(&format!("{text}{}{count}", " ".repeat(pad + 1)), width),
                    Pen { flags: Flags::BOLD, ..accent() },
                )
            }
            DockerRow::Container(ci) => {
                let Some(c) = self.snap.containers.get(*ci) else { return (String::new(), dim()) };
                let mark = if c.running() { '\u{25cf}' } else { '\u{25cb}' };
                // The health mark is its own column: up-and-unhealthy is the state
                // this panel exists to make visible, and folding it into the dot
                // would hide exactly that.
                let health = c.health.map(|h| h.mark()).unwrap_or(' ');
                let name = c.service.clone().unwrap_or_else(|| c.name.clone());
                let indent = if c.project.is_some() { "  " } else { "" };
                let status = c.short_status();
                let left = format!("{indent}{mark}{health} {name}");
                let pad = width
                    .saturating_sub(left.chars().count() + status.chars().count() + 1)
                    .max(1);
                let pen = match (c.running(), c.health) {
                    (_, Some(crate::docker::Health::Unhealthy)) => {
                        Pen { fg: Color::Rgb(0xff, 0x6b, 0x6b), ..normal() }
                    }
                    (true, _) => Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), ..normal() },
                    (false, _) => dim(),
                };
                (elide(&format!("{left}{}{status}", " ".repeat(pad)), width), pen)
            }
            DockerRow::Image(ii) => {
                let Some(m) = self.snap.images.get(*ii) else { return (String::new(), dim()) };
                let size = crate::docker::human_size(m.size);
                let label = m.label();
                let pad =
                    width.saturating_sub(label.chars().count() + size.chars().count() + 1).max(1);
                (elide(&format!("{label}{}{size}", " ".repeat(pad)), width), normal())
            }
            DockerRow::Volume(vi) => {
                let Some(v) = self.snap.volumes.get(*vi) else { return (String::new(), dim()) };
                (elide(&v.name, width), normal())
            }
            DockerRow::Repo(ri) => {
                let Some(r) = self.repos.get(*ri) else { return (String::new(), dim()) };
                let right = if r.private { "private".to_string() } else { String::new() };
                let pad =
                    width.saturating_sub(r.name.chars().count() + right.chars().count() + 1).max(1);
                (elide(&format!("{}{}{right}", r.name, " ".repeat(pad)), width), normal())
            }
            DockerRow::Tag(ti) => {
                let Some(t) = self.tags.get(*ti) else { return (String::new(), dim()) };
                let repo = self.open_repo.clone().unwrap_or_default();
                let drift = crate::docker::drift(
                    &self.local_images,
                    &repo,
                    &t.name,
                    t.digest.as_deref(),
                );
                // The drift is the point of this column: a row that says whether what
                // runs here is what is published there.
                let right = drift.label().to_string();
                let pad =
                    width.saturating_sub(t.name.chars().count() + right.chars().count() + 1).max(1);
                let pen = match drift {
                    crate::docker::Drift::Same => Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), ..normal() },
                    crate::docker::Drift::Differs => {
                        Pen { fg: Color::Rgb(0xe0, 0xaf, 0x68), ..normal() }
                    }
                    _ => normal(),
                };
                (elide(&format!("{}{}{right}", t.name, " ".repeat(pad)), width), pen)
            }
            DockerRow::Network(ni) => {
                let Some(n) = self.snap.networks.get(*ni) else { return (String::new(), dim()) };
                let right = n.driver.clone();
                let pad = width
                    .saturating_sub(n.name.chars().count() + right.chars().count() + 1)
                    .max(1);
                (elide(&format!("{}{}{right}", n.name, " ".repeat(pad)), width), normal())
            }
        }
    }

    pub fn render(&self, cols: usize, rows: usize, theme: &Theme) -> Vec<Panel> {
        let l = self.layout(cols, rows);
        let (w, h) = (l.w, l.h);
        let mut g = panel_grid(w, h, theme);
        let body_rows = l.body_rows;

        // Header: the kind strip, then the host and what it answered.
        let mut x = 2;
        if self.on_hub() {
            // No kind strip on hub: it has none, and a strip that switched to
            // something that does not exist there would be a lie in one keystroke.
            // The note in its place says where the repository list came from.
            let head = match &self.open_repo {
                Some(repo) => format!("{repo} \u{b7} tags"),
                None => self.repos_note.clone(),
            };
            let head = elide(&head, w.saturating_sub(28));
            write(&mut g, 0, x, &head, dim());
            x += head.chars().count();
        } else {
            for k in crate::docker::Kind::ALL {
                let label = format!(" {} ", k.letter());
                let pen = if k == self.kind { selected() } else { dim() };
                write(&mut g, 0, x, &label, pen);
                x += label.chars().count();
            }
            x += 1;
            write(&mut g, 0, x, self.kind.label(), dim());
        }
        let head = match self.host() {
            Some(host) => match (&host.error, &host.version) {
                (Some(e), _) => format!("{} \u{b7} {e}", host.name),
                (None, Some(v)) => format!("{} \u{b7} docker {v}", host.name),
                _ => host.name.clone(),
            },
            None => "no host".to_string(),
        };
        let head = elide(&head, w.saturating_sub(x + 4));
        let hw = head.chars().count();
        if w > x + hw + 2 {
            write(&mut g, 0, w - hw - 2, &head, accent());
        }

        // Column 1: hosts, plus Hub as a pseudo-host below a rule.
        if l.hosts_w > 0 {
            for (i, host) in self.hosts.iter().enumerate().take(body_rows) {
                let row = 2 + i;
                let mark = match (&host.endpoint, &host.error) {
                    (crate::docker::Endpoint::Hub, _) => '\u{2601}',
                    (_, Some(_)) => '\u{2717}',
                    _ if host.current => '\u{25cf}',
                    _ => '\u{25cb}',
                };
                let text = elide(&format!("{mark} {}", host.name), l.hosts_w.saturating_sub(2));
                if i == self.host_cursor {
                    let pen = if self.focus == DockerFocus::Hosts { selected() } else { inactive() };
                    write(&mut g, row, 0, &" ".repeat(l.hosts_w), pen);
                    write(&mut g, row, 1, &text, pen);
                } else {
                    let pen = if host.error.is_some() { dim() } else { normal() };
                    write(&mut g, row, 1, &text, pen);
                }
            }
            for line in 0..body_rows {
                write(&mut g, 2 + line, l.hosts_w, "\u{2502}", dim());
            }
        }

        // Column 2: the objects.
        let ox = l.objects_x();
        if l.objects_w > 0 {
            let scroll = self.cursor().saturating_sub(body_rows.saturating_sub(1));
            for line in 0..body_rows {
                let i = scroll + line;
                if i >= self.rows.len() {
                    break;
                }
                let (text, pen) = self.row_text(i, l.objects_w.saturating_sub(1));
                let row = 2 + line;
                if i == self.cursor() {
                    let pen =
                        if self.focus == DockerFocus::Objects { selected() } else { inactive() };
                    write(&mut g, row, ox, &" ".repeat(l.objects_w), pen);
                    write(&mut g, row, ox, &text, pen);
                } else {
                    write(&mut g, row, ox, &text, pen);
                }
            }
            if self.rows.is_empty() {
                let note = if self.loading { "reading\u{2026}" } else { "nothing here" };
                write(&mut g, 2, ox, note, dim());
            }
            for line in 0..body_rows {
                write(&mut g, 2 + line, l.sep2(), "\u{2502}", dim());
            }
        }

        // Column 3: the detail. Its own title line, because "summary" and "logs" of
        // the same row look alike for the first few lines and reading the wrong one
        // is the kind of mistake that costs a deploy.
        let dx = l.detail_x();
        let dw = l.detail_w();
        let title = format!(
            "{} \u{b7} {}",
            self.selected_name().unwrap_or_else(|| "\u{2014}".into()),
            self.detail.label()
        );
        write(&mut g, 1, dx, &elide(&title, dw), Pen { flags: Flags::BOLD, ..dim() });
        let lines: Vec<String> =
            if self.detail == DockerDetail::Summary { self.summary() } else { self.detail_lines.clone() };
        for (line, text) in lines.iter().skip(self.detail_scroll).take(body_rows - 1).enumerate() {
            write(&mut g, 3 + line, dx, &elide(text, dw), normal());
        }
        if lines.is_empty() && self.detail != DockerDetail::Summary {
            write(&mut g, 3, dx, "reading\u{2026}", dim());
        }

        // Footer: the last thing that happened, or the keys.
        let foot = match &self.message {
            Err(e) => (elide(e, w.saturating_sub(4)), Pen { fg: Color::Rgb(0xff, 0x6b, 0x6b), ..normal() }),
            Ok(m) if !m.is_empty() => (elide(m, w.saturating_sub(4)), dim()),
            _ => (
                elide(
                    "tab column \u{b7} C I V N kind \u{b7} enter fold \u{b7} L logs \u{b7} \
                     i inspect \u{b7} y id \u{b7} r reread \u{b7} esc",
                    w.saturating_sub(4),
                ),
                dim(),
            ),
        };
        let busy = if self.busy {
            " \u{b7} working\u{2026}"
        } else if self.loading {
            " \u{b7} reading\u{2026}"
        } else {
            ""
        };
        write(&mut g, h - 1, 2, &format!("{}{busy}", foot.0), foot.1);

        let col = (cols.saturating_sub(w)) / 2;
        let row = (rows.saturating_sub(h)) / 2;
        let mut out = vec![Panel { grid: g, col, row }];
        // The leader's which-key, as a panel of its own along the bottom of the box
        // and not as screen chrome: chrome is drawn UNDER the overlay's dimmed
        // backdrop, which is exactly where it cannot be read.
        if self.leader.is_some() {
            let entries = self.leader_entries();
            if !entries.is_empty() {
                let wk = crate::whichkey_grid(&entries, &self.leader_path(), w, &theme.leader_palette());
                let wk_rows = wk.rows();
                out.push(Panel { grid: wk, col, row: (row + h).saturating_sub(wk_rows) });
            }
        }
        out
    }
}

/// The narrowest the detail column may be before the hosts column is dropped.
const MIN_DETAIL: usize = 24;

/// Where the docker panel's parts sit, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockerLayout {
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    /// Width of the hosts column. Zero when it is dropped or zoomed.
    pub hosts_w: usize,
    pub objects_w: usize,
    pub body_rows: usize,
}

impl DockerLayout {
    pub fn objects_x(&self) -> usize {
        if self.hosts_w == 0 { 1 } else { self.hosts_w + 2 }
    }

    /// The column the second separator is drawn in.
    pub fn sep2(&self) -> usize {
        self.objects_x() + self.objects_w
    }

    pub fn detail_x(&self) -> usize {
        if self.objects_w == 0 { 2 } else { self.sep2() + 2 }
    }

    pub fn detail_w(&self) -> usize {
        self.w.saturating_sub(self.detail_x() + 1)
    }

    /// The separators a mouse can grab, as `(index, column)`.
    pub fn separators(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.hosts_w > 0 {
            out.push((0, self.hosts_w));
        }
        if self.objects_w > 0 {
            out.push((1, self.sep2()));
        }
        out
    }
}

/// What a click landed on in the docker panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerHit {
    /// One letter of the kind strip.
    Kind(crate::docker::Kind),
    HostRow(usize),
    Row(usize),
    /// A column separator, by index: 0 = hosts/objects, 1 = objects/detail.
    Separator(usize),
    Detail,
    /// Inside the panel but on nothing in particular: the header, the footer, the
    /// blank below a short list. NOT the same as missing the panel, which is what
    /// closes it — a click on the empty half of a column must not put it away.
    Inside,
}

impl DockerPanel {
    /// What is under a cell. The renderer and the mouse read the same layout, so a
    /// click cannot land somewhere other than what it looks like it hit.
    pub fn hit(&self, cols: usize, rows: usize, col: usize, row: usize) -> Option<DockerHit> {
        let l = self.layout(cols, rows);
        let (x, y) = (col.checked_sub(l.col)?, row.checked_sub(l.row)?);
        if x >= l.w || y >= l.h {
            return None;
        }
        // The header carries the kind strip, at the same columns it is drawn at.
        if y == 0 {
            if self.on_hub() {
                return Some(DockerHit::Inside);
            }
            let mut at = 2;
            for k in crate::docker::Kind::ALL {
                if x >= at && x < at + 3 {
                    return Some(DockerHit::Kind(k));
                }
                at += 3;
            }
            return Some(DockerHit::Inside);
        }
        // A separator wins over the columns it sits between: it is one cell wide and
        // the thing you are aiming at when you are near it.
        for (i, sep) in l.separators() {
            if x == sep {
                return Some(DockerHit::Separator(i));
            }
        }
        let Some(line) = y.checked_sub(2) else { return Some(DockerHit::Inside) };
        if line >= l.body_rows {
            return Some(DockerHit::Inside);
        }
        if l.hosts_w > 0 && x < l.hosts_w {
            return Some(match self.hosts.get(line) {
                Some(_) => DockerHit::HostRow(line),
                None => DockerHit::Inside,
            });
        }
        if l.objects_w > 0 && x < l.sep2() {
            let scroll = self.cursor().saturating_sub(l.body_rows.saturating_sub(1));
            let i = scroll + line;
            return Some(if i < self.rows.len() { DockerHit::Row(i) } else { DockerHit::Inside });
        }
        Some(DockerHit::Detail)
    }

    pub fn separator_at(&self, cols: usize, rows: usize, col: usize, row: usize) -> Option<usize> {
        match self.hit(cols, rows, col, row) {
            Some(DockerHit::Separator(i)) => Some(i),
            _ => None,
        }
    }

    /// Drags one separator to a column, as a fraction of the panel's width.
    ///
    /// Clamped loosely here and properly in `layout`: a drag that could pin a
    /// column at its minimum from here would stick there when the window grows.
    pub fn drag_split(&mut self, sep: usize, x: usize, w: usize) {
        if w == 0 || sep > 1 {
            return;
        }
        let f = (x as f32 / w as f32).clamp(0.05, 0.95);
        self.split[sep] = f;
        // The second separator can never end up left of the first: a negative
        // column width is a panel that draws itself inside out.
        if self.split[1] < self.split[0] + 0.05 {
            self.split[1] = (self.split[0] + 0.05).min(0.95);
        }
        if self.split[0] > self.split[1] - 0.05 {
            self.split[0] = (self.split[1] - 0.05).max(0.05);
        }
    }
}

// ---- the docker panel's leader layer ---------------------------------------
//
// Same contract as the git panel's and the sidebar's: every leaf PRESSES a key the
// panel already binds, and a leaf that this row cannot do is not offered. A verb
// cannot mean one thing from its letter and another from the menu.

/// A key the docker panel understands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerKey {
    Ch(char),
    Enter,
}

/// What a leaf does, and when it is offered.
///
/// The shape the git panel's leader already has: a leaf can be gated on what the
/// cursor is standing on, and it can TAKE you somewhere before it acts — a menu
/// that can only press keys in the context you are already in is a menu you have to
/// navigate to before you can use it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerPress {
    Key(DockerKey),
    /// Only on a container row: `logs` of a network is not a thing.
    OnContainer(DockerKey),
    /// Only on a compose project heading or a container inside one.
    OnProject(DockerKey),
    /// Only on Docker Hub — a repository or one of its tags.
    OnHub(DockerKey),
    /// Switches the object column to this kind and stops there.
    Switch(crate::docker::Kind),
    /// Switches to this kind and then presses. Offered whatever is selected: it is
    /// the way to reach a verb of another kind without walking there first.
    InKind(crate::docker::Kind, DockerKey),
}

pub enum DockerNode {
    Leaf(DockerPress),
    Group(&'static [DockerEntry]),
}

pub struct DockerEntry {
    pub key: char,
    pub title: &'static str,
    pub node: DockerNode,
}

const fn dleaf(key: char, title: &'static str, press: DockerPress) -> DockerEntry {
    DockerEntry { key, title, node: DockerNode::Leaf(press) }
}

use DockerKey::{Ch as DCh, Enter as DEnter};
use DockerPress::{InKind, Key as DKey, OnContainer, OnHub, OnProject};

/// Spelled out so the leader table below stays readable.
const IMAGES: crate::docker::Kind = crate::docker::Kind::Images;

pub static DOCKER_LEADER: &[DockerEntry] = &[
    DockerEntry {
        key: 'c',
        title: "Container",
        node: DockerNode::Group(&[
            dleaf('s', "start it (or unpause)", OnContainer(DCh('s'))),
            dleaf('x', "stop it", OnContainer(DCh('x'))),
            dleaf('r', "restart it", OnContainer(DCh('R'))),
            dleaf('p', "pause / unpause", OnContainer(DCh('p'))),
            dleaf('k', "kill it now", OnContainer(DCh('K'))),
            dleaf('e', "shell inside it", OnContainer(DCh('e'))),
            dleaf('w', "open its port in the browser", OnContainer(DCh('w'))),
            dleaf('l', "its logs", OnContainer(DCh('L'))),
            dleaf('i', "inspect it", OnContainer(DCh('i'))),
            dleaf('y', "copy its id", OnContainer(DCh('y'))),
            dleaf('d', "remove it (asks)", OnContainer(DCh('d'))),
        ]),
    },
    DockerEntry {
        key: 'p',
        title: "Compose project",
        node: DockerNode::Group(&[
            dleaf('u', "up -d", OnProject(DCh('U'))),
            dleaf('w', "down (asks)", OnProject(DCh('W'))),
            dleaf('p', "pull", OnProject(DCh('P'))),
            dleaf('t', "deploy: pull, then up -d (asks)", OnProject(DCh('T'))),
            dleaf('o', "fold / unfold", DKey(DEnter)),
        ]),
    },
    DockerEntry {
        key: 'i',
        title: "Images",
        node: DockerNode::Group(&[
            // These SWITCH to the image list first, so the verb is reachable from
            // wherever the cursor happens to be — the way the git panel's menu can
            // jump into the log and act in one sequence.
            dleaf('i', "the image list", DockerPress::Switch(crate::docker::Kind::Images)),
            // These SWITCH to the image list and then act, so an image verb is one
            // sequence from anywhere — the way `leader c p` cherry-picks from the
            // git panel whatever view you were in.
            dleaf('p', "publish it: docker push (asks)", InKind(IMAGES, DCh('>'))),
            dleaf('s', "what uses it, size, digests", InKind(IMAGES, DCh('u'))),
            dleaf('j', "inspect it", InKind(IMAGES, DCh('i'))),
            dleaf('d', "remove it (asks)", InKind(IMAGES, DCh('d'))),
        ]),
    },
    DockerEntry {
        key: 'o',
        title: "Objects",
        node: DockerNode::Group(&[
            dleaf('c', "containers", DockerPress::Switch(crate::docker::Kind::Containers)),
            dleaf('i', "images", DockerPress::Switch(crate::docker::Kind::Images)),
            dleaf('v', "volumes", DockerPress::Switch(crate::docker::Kind::Volumes)),
            dleaf('n', "networks", DockerPress::Switch(crate::docker::Kind::Networks)),
            dleaf('d', "remove what is selected (asks)", DKey(DCh('d'))),
            dleaf('y', "copy its id", DKey(DCh('y'))),
        ]),
    },
    DockerEntry {
        key: 'h',
        title: "Hosts",
        node: DockerNode::Group(&[
            dleaf('h', "pick a host", DKey(DCh('H'))),
            dleaf('r', "reread this host", DKey(DCh('r'))),
            dleaf('u', "docker hub", DKey(DCh('B'))),
        ]),
    },
    DockerEntry {
        key: 'b',
        title: "Docker Hub",
        node: DockerNode::Group(&[
            dleaf('u', "go to hub", DKey(DCh('B'))),
            dleaf('o', "open this repository", OnHub(DEnter)),
            dleaf('p', "publish this tag (asks)", OnHub(DCh('>'))),
            dleaf('r', "reread it", OnHub(DCh('r'))),
        ]),
    },
    DockerEntry {
        key: 'd',
        title: "Detail",
        node: DockerNode::Group(&[
            dleaf('s', "summary", DKey(DCh('u'))),
            dleaf('l', "logs", OnContainer(DCh('L'))),
            dleaf('i', "inspect (the whole JSON)", DKey(DCh('i'))),
            dleaf('y', "copy the id", DKey(DCh('y'))),
            dleaf('z', "full width, and back", DKey(DCh('z'))),
        ]),
    },
    DockerEntry {
        key: 'v',
        title: "View",
        node: DockerNode::Group(&[
            dleaf('1', "containers", DockerPress::Switch(crate::docker::Kind::Containers)),
            dleaf('2', "images", DockerPress::Switch(crate::docker::Kind::Images)),
            dleaf('3', "volumes", DockerPress::Switch(crate::docker::Kind::Volumes)),
            dleaf('4', "networks", DockerPress::Switch(crate::docker::Kind::Networks)),
            dleaf('t', "to the top", DKey(DCh('g'))),
            dleaf('b', "to the bottom", DKey(DCh('G'))),
            dleaf('r', "reread this host", DKey(DCh('r'))),
            dleaf('z', "full width, and back", DKey(DCh('z'))),
        ]),
    },
    dleaf('z', "Full width, and back", DKey(DCh('z'))),
    dleaf('q', "Close the panel", DKey(DCh('q'))),
];

impl DockerPanel {
    pub fn arm_leader(&mut self) {
        self.leader = Some(Vec::new());
    }

    pub fn cancel_leader(&mut self) {
        self.leader = None;
    }

    fn leader_level(&self) -> Option<&'static [DockerEntry]> {
        let path = self.leader.as_ref()?;
        let mut level: &'static [DockerEntry] = DOCKER_LEADER;
        for key in path {
            match level.iter().find(|e| e.key == *key) {
                Some(DockerEntry { node: DockerNode::Group(next), .. }) => level = next,
                _ => return Some(DOCKER_LEADER),
            }
        }
        Some(level)
    }

    fn leader_applies(&self, press: DockerPress) -> bool {
        match press {
            DockerPress::OnContainer(_) => {
                matches!(self.selected(), Some(DockerRow::Container(_)))
            }
            DockerPress::OnProject(_) => self.selected_project().is_some(),
            DockerPress::OnHub(_) => self.on_hub(),
            // The kind strip does not exist on hub, so neither does anything that
            // would switch it.
            DockerPress::Switch(_) | DockerPress::InKind(..) => !self.on_hub(),
            DockerPress::Key(_) => true,
        }
    }

    pub fn leader_entries(&self) -> Vec<(String, String, bool)> {
        let Some(level) = self.leader_level() else { return Vec::new() };
        let mut out: Vec<(String, String, bool)> = level
            .iter()
            .filter(|e| match &e.node {
                DockerNode::Leaf(p) => self.leader_applies(*p),
                DockerNode::Group(_) => true,
            })
            .map(|e| {
                (e.key.to_string(), e.title.to_string(), matches!(e.node, DockerNode::Group(_)))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn leader_path(&self) -> Vec<String> {
        self.leader.as_ref().map(|p| p.iter().map(|c| c.to_string()).collect()).unwrap_or_default()
    }

    /// Feeds a key to the layer. `Some` is a leaf for the panel to run.
    pub fn leader_key(&mut self, c: char) -> Option<DockerPress> {
        let level = self.leader_level()?;
        match level.iter().find(|e| e.key == c) {
            Some(DockerEntry { node: DockerNode::Group(_), .. }) => {
                if let Some(path) = &mut self.leader {
                    path.push(c);
                }
                None
            }
            Some(DockerEntry { node: DockerNode::Leaf(p), .. }) if self.leader_applies(*p) => {
                let press = *p;
                self.leader = None;
                Some(press)
            }
            _ => {
                self.leader = None;
                None
            }
        }
    }
}
