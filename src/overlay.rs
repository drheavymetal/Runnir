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
#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// Which half of the panel the keyboard is driving. Line-level staging needs a
    /// cursor INSIDE the diff, and one set of j/k cannot mean two things.
    pub diff_focus: bool,
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
            diff_focus: false,
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
    pub fn layout(&self, cols: usize, rows: usize) -> GitLayout {
        let w = cols.saturating_sub(4).max(40);
        let h = rows.saturating_sub(4).max(12);
        GitLayout {
            col: 2,
            row: 2,
            w,
            h,
            list_w: (w * 2 / 5).clamp(24, 64).min(w.saturating_sub(10)),
            body_rows: h.saturating_sub(3),
        }
    }

    /// Whether the list is showing one commit's files rather than the view's own
    /// list.
    pub fn in_commit(&self) -> bool {
        self.open_commit.is_some()
    }

    pub fn len(&self) -> usize {
        if self.in_commit() {
            return self.commit_files.len();
        }
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
        if self.in_commit() {
            return self.commit_cursor.min(self.len().saturating_sub(1));
        }
        self.cursors[self.view_index()].min(self.len().saturating_sub(1))
    }

    pub fn set_cursor(&mut self, n: usize) {
        let n = n.min(self.len().saturating_sub(1));
        if self.in_commit() {
            self.commit_cursor = n;
        } else {
            let i = self.view_index();
            self.cursors[i] = n;
        }
        self.preview_scroll = 0;
    }

    /// Drills into a commit: the list becomes its files. `leave_commit` backs out,
    /// which is also what Escape does before it will close the panel.
    pub fn enter_commit(&mut self, sha: String) {
        self.open_commit = Some(sha);
        self.commit_files.clear();
        self.commit_cursor = 0;
        self.preview_scroll = 0;
    }

    pub fn leave_commit(&mut self) -> bool {
        let was = self.open_commit.take().is_some();
        self.commit_files.clear();
        self.preview_scroll = 0;
        was
    }

    /// The file selected inside an open commit.
    pub fn selected_commit_file(&self) -> Option<&crate::git::FileEntry> {
        self.commit_files.get(self.cursor())
    }

    pub fn down(&mut self) {
        let c = self.cursor();
        if c + 1 < self.len() {
            self.set_cursor(c + 1);
        }
    }

    pub fn up(&mut self) {
        let c = self.cursor();
        self.set_cursor(c.saturating_sub(1));
    }

    pub fn cycle_view(&mut self) {
        self.view = self.view.next();
        self.preview_scroll = 0;
    }

    pub fn set_view(&mut self, v: GitView) {
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
        self.diff_focus = true;
        self.scroll_to_diff_cursor();
    }

    pub fn leave_diff(&mut self) {
        self.diff_focus = false;
        self.diff_anchor = None;
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

    fn row_text(&self, i: usize, width: usize) -> (String, Pen) {
        let green = Pen { fg: Color::Rgb(0x7a, 0xc0, 0x7a), bg: bg(), ..Pen::default() };
        let red = Pen { fg: Color::Rgb(0xe0, 0x60, 0x60), bg: bg(), ..Pen::default() };
        if self.in_commit() {
            return match self.commit_files.get(i) {
                Some(f) => {
                    let pen = match f.index {
                        'A' => green,
                        'D' => red,
                        _ => normal(),
                    };
                    (format!("{} {}", f.index, elide(&f.path, width.saturating_sub(2))), pen)
                }
                None => (String::new(), normal()),
            };
        }
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
        if self.in_commit() {
            return "one commit's files · j k move · esc back to the list · y copy path";
        }
        match self.view {
            GitView::Status => {
                "space stage · a all · ]/[ hunk · s/u stage hunk · c commit · P push · p pull · S stash"
            }
            GitView::Log => "enter files of this commit · x checkout · c cherry-pick · / filter · y sha",
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
        let head_w = head.chars().count();
        if head_w > 0 && w > x + head_w + 2 {
            write(&mut g, 0, w - head_w - 2, &head, accent());
        }

        // Left: the list. Right: the preview, with a rule between them.
        let body_rows = l.body_rows;
        let scroll = self.cursor().saturating_sub(body_rows.saturating_sub(1));
        for line in 0..body_rows {
            let i = scroll + line;
            if i >= self.len() {
                break;
            }
            let (text, pen) = self.row_text(i, list_w.saturating_sub(2));
            let row = 2 + line;
            if i == self.cursor() {
                write(&mut g, row, 0, &" ".repeat(list_w), selected());
                write(&mut g, row, 1, &text, selected());
            } else {
                write(&mut g, row, 1, &text, pen);
            }
        }
        for line in 0..body_rows {
            write(&mut g, 2 + line, list_w, "\u{2502}", dim());
        }

        // The diff is drawn the way a review tool draws one: a line number, then the
        // code, on a row whose whole width is tinted. A raw `+`/`-` column makes
        // every changed line start one column further in than its neighbours, and
        // leaves you counting rows to find out which line it is.
        let prev_x = list_w + 2;
        let prev_w = w.saturating_sub(prev_x);
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
            if self.diff_focus {
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
        write(&mut g, h - 1, 2, &elide(&foot, w.saturating_sub(4)), pen);

        vec![Panel { grid: g, col: l.col, row: l.row }]
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
        if lc < l.list_w {
            let scroll = self.cursor().saturating_sub(l.body_rows.saturating_sub(1));
            return Some(GitHit::Row(scroll + line));
        }
        Some(GitHit::PreviewLine(self.preview_scroll + line))
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

/// The panel's geometry in cells, produced once and used by both the renderer and
/// the hit test.
pub struct GitLayout {
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    pub list_w: usize,
    pub body_rows: usize,
}

/// What a click landed on.
pub enum GitHit {
    View(GitView),
    /// A row of the list, by index into whatever the list is showing.
    Row(usize),
    /// A row of the preview, by index into `preview_rows`.
    PreviewLine(usize),
    Header,
}

/// First non-empty line, so a multi-line git message fits a one-row footer.
fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string()
}

/// Clips to `width` columns, marking the cut, so a truncated path is never mistaken
/// for a shorter one.
fn elide(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}


/// The visible window of a single-line input field that is wider than its box.
///
/// The caret always sits at the end of these fields (they take typing and
/// backspace, never arrow keys), so the END is what has to stay visible: a field
/// that clips the tail hides the character you just typed, which is the one thing a
/// text input may never do. When text is cut, a leading ellipsis says so.
pub fn field_view(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let n = text.chars().count();
    if n <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(1);
    let tail: String = text.chars().skip(n - keep).collect();
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
}

impl Prompt {
    pub fn new(kind: PromptKind, label: &str, suggestions: Vec<String>) -> Self {
        Self { kind, label: label.into(), input: String::new(), suggestions, cursor: 0 }
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

        // Drilled into a commit, the rows are that commit's files.
        p.enter_commit("abc1234".into());
        p.commit_files = vec![FileEntry { path: "a.rs".into(), index: 'M', worktree: '.' }];
        assert_eq!(p.len(), 1);
        assert!(matches!(p.hit(cols, rows, l.col + 1, l.row + 2), Some(GitHit::Row(0))));
        let _ = Commit::default();
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
