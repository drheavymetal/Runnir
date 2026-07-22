//! The in-terminal manual, shown by F1.
//!
//! Lines beginning `# ` are headings, `@ ` are key-hint lines; the overlay styles
//! them. Keeping the docs in the binary means they can never fall out of step with
//! the build the user is running.

pub const HELP: &str = "\
# runnir — the whole thing in one screen

runnir is a GPU-accelerated terminal. This page lives in the binary, so it always
matches the version you are running. Scroll with the arrows or PageUp/PageDown.
Press Esc to close.

# The leader key

Compositors win every modifier race. Hyprland and GNOME both claim most of the
Super layer for workspaces, and a key they grab never reaches runnir at all — an
app cannot bind around that. So runnir keeps its own layer behind a leader key.

Press Alt+Shift+Space and let go. The status bar shows LEADER (a toast, if you
turned the bar off with `window.status_bar`) and a panel lists what the next key
does, so the layer teaches itself — you never have to remember a table. Then press a key: the hot ones act immediately, the rest open a group
that takes one more key. Escape or any unbound key backs out, and nothing leaks
to the shell. Modifiers you are still holding are ignored, so you can keep
Alt+Shift down through the whole sequence. Ten seconds per step by default;
`leader_timeout` changes it, and 0 means it waits as long as you do.

Bare Alt+Space is deliberately NOT the leader: it is the window menu on Windows
and GNOME, krunner on KDE and PowerToys Run on Windows, so it would never reach
runnir on most desktops.

On a programmable keyboard there is a way OUT of the modifier race entirely.
F13 to F24 are real keycodes that no desktop claims, because no keyboard has
shipped with them for decades. Put one on a layer in your board's firmware and
set the leader setting to f13 in the config: no modifiers, nothing for a compositor to
grab, one key. F13-F20 also reach the shell as their standard escape sequences
when they are not bound; F21-F24 have no agreed encoding, so runnir binds them
but sends nothing.

Straight away, no group — the things you do constantly:

@ Leader 1..9      jump to tab N
@ Leader hjkl      focus the pane left/down/up/right
@ Leader HJKL      resize the focused pane (arrows do this too)
@ Leader U         catch up: one headline per pane after time away
@ Leader V         clipboard history
@ Leader G         fix the last failed command
@ Leader Z / Shift+Z  font bigger / smaller (+, = and - work too; the letters
@                        are the binding that survives a layout where = is shifted)
@ Leader 0         reset the font size

Then the groups. The letter is the noun:

@ Leader T ...     tabs: T new, N/P next/prev, W close, R rename, U reopen,
@                        H/L (or the arrows) move the tab in the bar
@ Leader P ...     panes: D split left/right, E split up/down, X close, Z zoom,
@                        C cycle layout, N focus next, B broadcast,
@                        G add/remove this pane from the broadcast group
@ Leader C ...     clipboard: C copy, V paste, H history, O last output,
@                        M copy mode, P/S pipe output/scrollback
@ Leader F ...     find & scroll: F search, H history search, I hint mode,
@                        N/P jump between commands, T/B top/bottom,
@                        U/D (or PageUp/PageDown) page,
@                        E open scrollback in an editor, W watch, O fold
@ Leader A ...     ai: A toggle, G fix last command, W why did this fail,
@                        M run a described command, E explain, S summarise
@ Leader R ...     run & launch: C Claude, W whisper, S ssh, M now playing,
@                        L a saved layout
@ Leader O V       how this repo is worked: the verbs learned from what you
                   actually type here. Enter puts one at the prompt (never runs it),
                   X forgets everything learned about this repo. Off until you set
                   verbs.enabled: only the verb is ever stored, never arguments, and
                   never inside the repo.
@ Leader O ...     open: C config, T theme, D these docs, S snippets,
@                        P the palette, I/W image watch
@ Leader S ...     session: S save, R restore, C clear, Q quit

The layer is a strict superset: every chord that exists still works (some on
Ctrl+Shift, a few on Alt+Shift), so nothing you already learned stopped working —
but the reverse does not hold. Tab switching, cycle layout, focus next, broadcast
groups, copy mode, piping output, history search, watch, fold, saved layouts,
config, the theme picker, image watch, project sessions, clear and quit have no
chord at all and are reachable only from here or the palette.

Rebind the leader itself with the `leader` setting (an empty string turns the
layer off), and bind your own sequences with a `leader+` prefix in [keys]: a
space separates the steps, so `leader+r c` is the leader, then R, then C. See
the config section below.

Everything here is also in the command palette (Leader O P, or Ctrl+Shift+P),
which stays the way to find a command you have not memorised.

# Tabs

@ Ctrl+Shift+T     new tab
@ Ctrl+Shift+W     close tab
@ Ctrl+PageUp/Dn   previous / next tab
@ Ctrl+Shift+Left/Right  move the current tab left / right in the bar
@ Leader 1..9      jump to tab N
@ Ctrl+Shift+R     rename the current tab

# Splits (panes)

A tab can be divided into panes. Every pane is its own shell; a split inherits the
working directory of the pane you were in, so it opens where you already are.

@ Ctrl+Shift+D     split left / right
@ Ctrl+Shift+E     split up / down
@ Ctrl+Shift+X     close the focused pane
@ Ctrl+Shift+Z     zoom the focused pane to fill the tab (toggle)
@ Ctrl+Shift+HJKL  move focus left / down / up / right (vim directions)
@ Alt+Shift+arrows resize the focused pane (also Leader HJKL / arrows)

A pane that rings the terminal bell (BEL) flashes briefly; if the window is not
focused it also raises the compositor urgency hint, so a finished build in the
background gets your attention without stealing focus.

Focus movement is geometric: 'focus right' goes to the pane you see to the right,
whatever order you built the splits in.

# Scrollback and selection

@ wheel / Shift+PageUp/Dn   scroll the history
@ drag                      select text (copied on release)
@ Ctrl+Shift+C / V          copy / paste
@ Alt+Shift+V               clipboard history: re-paste a recent copy (see below)
@ middle click              paste the primary selection (the last text selected)
@ middle DRAG               tactile pipe: grab a command's output block and drop it
                            on another pane. The output is written to a private file
                            and its path is left at that pane's prompt for you to
                            use — it is never run for you. Needs shell integration
                            (the blocks come from the OSC 133 marks); a pane running
                            a full-screen app refuses the drop.
@ Ctrl+Shift+Home/End       jump to top / live output
@ Ctrl+Shift+F              search the scrollback; Enter/Up next/prev, Esc closes
@ Ctrl+Shift+Q              dump the scrollback to $EDITOR in a new split

# Clipboard history

Every copy runnir makes is remembered in a small in-memory ring, newest first:
selection copies, Ctrl+Shift+C, copy-mode yanks, copy-last-output, hint copies and
OSC 52 writes from programs all land there. Open the picker to re-paste an earlier
copy without hunting for it again.

@ Alt+Shift+V open the clipboard-history picker (also in the palette)

Type to fuzzy-search the entries, arrows to move, Enter pastes the highlighted one
into the focused pane through the normal paste path, Esc closes. Each row shows a
one-line preview (a pilcrow marks a multi-line entry); re-copying an entry moves it
to the top instead of duplicating it. The history is never written to disk, since
the clipboard often holds secrets. Size it with clipboard.capacity (default 50) and
turn it off with clipboard.enabled = false.

# Mouse in full-screen apps

Clicks, drags and the wheel are forwarded to programs that ask for the mouse
(vim, tmux, htop, less), so clicking a pane in tmux or a process in htop works.
Hold Shift to override and select text instead, even inside such an app.

Any key you type snaps the view back to the live output, so you never type into a
scrolled-back screen and wonder why nothing happens.

# Copy mode (keyboard selection)

From the palette, Copy mode starts a keyboard cursor in the scrollback — no mouse
needed:

@ h j k l / arrows   move the cursor
@ 0 / $              start / end of line
@ g / G              top / bottom
@ v or Space        start (or drop) a selection
@ y or Enter        yank the selection to the clipboard and exit
@ Esc or q          exit

The view scrolls to follow the cursor, so you can select text far up the history
without reaching for the wheel.

# Shell integration (OSC 133)

If your shell emits OSC 133 marks, runnir understands where each command's prompt,
input and output begin. That powers:

@ Ctrl+Shift+Up/Down   jump to the previous / next command
@ Ctrl+Shift+O         copy the output of the last command
@ Ctrl+Shift+G         ask the AI why the last command failed

Each command's prompt row also gets a status bar at the left edge: green when it
exited 0, red when it failed, dim while it runs — a glanceable pass/fail history.

From the palette, Fold / unfold all command output collapses every finished
command's output into a one-line summary, so a screen full of build noise becomes
a list of commands. Click a summary line to unfold just that one. Needs OSC 133.

For fish, add to config.fish:
  function runnir_prompt --on-event fish_prompt
    printf '\\e]133;A\\e\\\\'
  end
  function runnir_preexec --on-event fish_preexec
    printf '\\e]133;C\\e\\\\'
  end
  function runnir_postexec --on-event fish_postexec
    printf '\\e]133;D\\e\\\\'
  end

# Pipe output through a command

From the palette, Pipe last output through command opens a small input where you
type a filter — grep error, sort -u, jq . — and runnir runs it in a new split with
the last command output fed to it on stdin. Pipe scrollback through command does
the same but feeds the whole scrollback. The command runs through sh, so pipes and
redirection work. The last-output variant needs OSC 133 to know where the block is.

@ example    Pipe last output through, then type: grep -i error
@ example    Pipe scrollback through, then type: sort | uniq -c | sort -rn

# SSH awareness

runnir watches the foreground process of each pane. When it is ssh, the pane is
tinted a colour derived from the host name — the same host is always the same
shade, on every machine, with nothing to configure. sudo/root panes tint red,
docker blue. It launches the real ssh, so your ~/.ssh/config, jump hosts and
1Password agent all work unchanged.

@ Ctrl+Shift+S     quick connect: fuzzy-pick a host from ~/.ssh/config

# Hint mode (keyboard, no mouse)

@ Ctrl+Shift+F     label every URL, path and git hash on screen; type a label to
                   open or copy it. This removes most of the reasons to reach for
                   the mouse.

Hovering the pointer over a URL or path also underlines it; Ctrl+click opens a URL
in the browser or copies a path/hash, without entering hint mode. OSC 8 hyperlinks
(the explicit links ls --hyperlink, gcc and cargo emit) are honoured too: the exact
link the program declared underlines on hover and opens on Ctrl+click.

# AI assistant

runnir talks to an assistant without leaving the terminal, through whichever
provider you configure. No provider is baked in.

Three shapes are supported. A CLI (kind = claude_code) spawns Claude Code against
your subscription — no API key at all. An OpenAI-compatible endpoint (kind = api)
covers OpenAI, Gemini, DeepSeek, Z.ai and anything else that speaks
/chat/completions — point base_url at it and name the model. Anthropic's own
Messages API (kind = anthropic) is its own shape: a different path, an x-api-key
header instead of a bearer token, a pinned version header, and a required
max_tokens — so it is a separate kind rather than an api entry that would look
right and fail at request time.

Keys are never stored in the config. Each provider names an ENVIRONMENT VARIABLE
(api_key_env) and runnir reads the key from there, so the config file is safe to
keep in a dotfile repo.

Switch provider without editing anything: Ctrl+Shift+, opens the settings panel,
and the AI row cycles through every provider you have configured, showing which
model is behind each one.

You can also route ONE TASK to a different provider than the rest, which is about
cost rather than taste: summarising a whole session is long and cheap on a flat-rate
subscription, while turning one sentence into a command wants the lowest latency you
can get. In [ai.tasks], name a task and the provider it should use — panel, command,
fix, explain, summarize, whisper. Anything not named there uses the default. A task
name that is not one of those six, or a provider that does not exist, is reported
when the config loads rather than silently falling back for ever.

@ Ctrl+Shift+A     open / close the assistant panel
@ Ctrl+Shift+G     send the last command, its output and its exit code to the model
@ Ctrl+Shift+N     launch Claude Code in a new split
@ Ctrl+Shift+M     describe a command in plain language; the model writes it and
                   types it at the prompt for you to review and run (not run for you)
@ Alt+Shift+G      fix the last failed command: the model reads the command, its
                   output and its non-zero exit code, then types a corrected command
                   at the prompt for you to review and run (never run for you). For
                   example, after mkdr foo fails it types mkdir foo at the prompt.
@ Ctrl+Shift+Y     explain the current selection in the assistant panel
@ Ctrl+Shift+I     summarize this session (commands, results, errors and fixes).
                   Reads the whole window, not just the focused pane, and includes
                   the screen parked behind a full-screen app — so asking from
                   inside vim or Claude Code summarises the work, not nothing.

# Whisper — talk to the terminal

@ Ctrl+Shift+Enter  open the whisper bar and say what you want in plain language.
                    A model turns it into terminal actions and runnir runs them.

The name fits: 'rún' is a whisper to the machine. Whisper drives runnir itself,
not just the shell — one instruction can split panes, open ssh sessions, search,
launch tools. Examples:

  split in four and ssh to 192.168.1.3, .7, .9 and .188
  search the scrollback for the word panic
  make the font bigger and open the docs

Runnir actions run immediately; a shell command it decides on is typed at the
prompt for you to review and run, never executed for you.

# Command guardian

When you press Enter on a command that matches a known destructive pattern, runnir
pauses and asks you to confirm instead of running it blindly. Enter confirms and
runs it; Escape returns to the line so you can fix or cancel it. It catches things
like recursive force-removes of a root or home path, dd onto a raw device, mkfs,
SQL DROP/TRUNCATE, git force-push and the classic fork bomb.

It also guards the git commands that destroy work no commit holds: reset --hard,
clean -f, checkout of a path, restore, stash clear/drop, branch -D, push
--delete/--mirror, and dropping the reflog with gc --prune=now or reflog expire.
The safe shapes are deliberately left alone — clean -n is a dry run, checkout of a
branch is a switch git refuses when it would lose edits, restore --staged only
unstages, and branch -d already refuses an unmerged branch.

Only a bare Enter at
the live prompt is guarded, so editing history and full-screen apps are untouched.
Turn it off with behaviour.command_guardian = false.

# Closing with work still running

Closing the window while a command is still running asks first, and lists what is
running in each tab. Press y to close anyway, n or Escape to stay. Enter is not an
answer here on purpose: this exists to survive a reflex keystroke.

A pane sitting at its shell prompt is not work, so an idle window closes at once
without asking. The same question guards the last tab and the last pane, since
closing either of those closes the window. Turn it off with
behaviour.confirm_close = false.

# Git panel

Leader G opens a git panel over the terminal: seven lists, and the selection's diff
beside them.

  1..7 or Tab            status, log, branches, stashes, tags, reflog, worktrees
  j k / arrows           move          J K / PageUp PageDown   scroll the diff
  y                      copy the sha, path, branch or tag under the cursor
  r  refresh             q or Escape   close

status   space stages or unstages the file, a stages everything, ] [ pick a hunk,
         s and u stage and unstage just that hunk, t switches between the staged
         and unstaged diff of the same file, c writes a commit message, C hands the
         commit to a pane so your editor opens for a longer one, A amends, e opens
         the file, L its history, b blame, S stashes. On a conflicted file, O takes
         ours and T takes theirs.

         l moves into the diff itself (h comes back). There j and k walk the lines,
         v starts a selection, and s and u stage or unstage EXACTLY those lines -
         the working tree never moves, only the index. A blue bar marks the
         selection and an arrow the line under the cursor.
log      drawn with the branch graph down the left. Enter opens the commit's FILES
         in a THIRD column beside the log, and the file selected there is the diff
         on the right. h and l walk the columns, Enter on a file gives it the whole
         panel, Escape backs out one step at a time. Moving the log closes the file
         column, since those files were the other commit's.
         x checks the commit out, c cherry-picks it, o opens it in a split,
         / filters by message, i plans an interactive rebase of everything above
         the selected commit.

         In the rebase planner: p pick, r reword, e edit, s squash, f fixup,
         d drop, K and J move a commit up or down, Enter runs it, Escape cancels.
         Nothing happens until you press Enter. A rebase that stops on a conflict
         leaves the repository mid-rebase, which the status bar says out loud.
branches local ones first, then remote-tracking. Enter switches (with --track for a
         remote one), n creates, m merges it into HEAD, R rebases onto it.
tags     Enter checks one out, n creates, P pushes tags.
reflog   every position HEAD has held. Enter opens its files, x returns to it.
         This is the way back from a mistake, which is why it is here.
worktrees and submodules; Enter opens one in a new tab with the shell already there.
blame    b on a file in the status view. Every line with the commit that last
         touched it; Enter opens that commit's files, Escape comes back.

Anywhere: P pushes (adding -u the first time a branch is pushed), p pulls
fast-forward only, f fetches and prunes.

The mouse works too: click a view name to switch to it, click a row to select it,
click it again to open it, click a line of the diff to pick that hunk, and the
wheel moves the selection over the list or scrolls the diff over the diff. A click
outside the panel closes it.

Drag the rule between two columns to resize them; the pointer turns into a resize
cursor over one. The widths are kept while the panel is open, and no column can be
dragged to nothing.

z gives the selected file's diff the whole panel and takes it back - the columns
find the change, the width reads it. Escape leaves the zoom before it leaves
anything else.

The leader key opens a menu INSIDE the panel: the same which-key, but of git verbs
(f file, d diff, c commit, b branch, s stash, t tag, r remote, v view). It only
offers what the view you are in can actually do, and every entry presses a key the
panel already has, so the letters and the menu can never mean different things.

Every key acts at once, with no confirmation, so nothing that can lose uncommitted
work is bound here at all - no reset --hard, no clean, no discard, no stash drop,
no branch -D. Those stay at the prompt, where the guardian sees them and asks.

Commands run in the background with a 60 second deadline. One that needs a password
- an ssh passphrase, an https login, an unknown host key - cannot be answered from
there, so the panel reruns it in a split where git asks you normally.

Diffs are drawn with line numbers and a full-width tint per changed line instead of
a + / - column, so the code stays aligned with its context.

# Hint mode knows git

Hint mode (Ctrl+Shift+Space, or Leader I) labels every URL, path and commit hash on
screen, and in a repository it also labels branch names - by name, against the real
ref list, so only a branch this repo actually has is a target. Paths git prints
relatively (src/main.rs, and src/main.rs:412 from a compiler) are labelled too.

Type the label in lower case for the plain action: copy, or open for a URL. Type it
in UPPER CASE for the alternate one, which shows you the thing in a split - a hash
opens git show, a branch its log, a path opens in your editor at that line, and a
URL is copied instead of opened. Everything on the shifted key only reads: a
mistyped label can never move a branch or touch the working tree.

# Repository state in the status bar

In a git repository the status bar shows any unfinished operation first (REBASE,
MERGE, CHERRY-PICK, REVERT, BISECT), then the branch, then only what is not clean:
down/up arrows for commits behind and ahead of the upstream, + for staged files,
a dot for files with unstaged or untracked changes, ! for conflicts. A clean tree
level with its upstream shows just the branch.

The branch is read from HEAD, so it is right the moment a checkout finishes - and
that works inside a worktree, where .git is a file pointing elsewhere. The counts
come from git status on a worker, refreshed when a command finishes in that pane or
when the repository changes from outside it (an editor, another pane, another
window). Nothing polls: an idle terminal in an untouched repository never runs git.

A tab whose repository has uncommitted work carries a small marker in the tab bar,
behind the failed-command and unseen-output badges.

# Named layouts (workspaces)

Define layouts in the config and launch one from the palette (Launch layout): it
opens a fresh tab split into one pane per command, tiling them. Perfect for a
servers layout that ssh's into several machines at once. In the config:

  [[layouts]]
  name = servers
  commands = [ssh 192.168.1.3, ssh 192.168.1.7, ssh 192.168.1.9, htop]

An empty command opens a plain shell pane. Commands are split on whitespace (not a
full shell parse), which covers ssh host, journalctl -f and the like.

# Command snippets (bookmarks)

@ Alt+Shift+S      fuzzy-pick a saved command; it is typed at the prompt to review.

Save commands you run often as snippets and recall them from the palette (Insert
command snippet) or the keybind. Type to filter on name or description. Selecting a
snippet TYPES its command at the focused prompt for you to check and run yourself —
the same review-first rule as the AI command-writer, never executed behind your back.
A snippet may set run_now = true to submit itself immediately instead. In the config:

  [[snippets]]
  name = deploy
  command = git push && ssh server bin/deploy
  description = ship the current branch to prod

  [[snippets]]
  name = tail
  command = journalctl -fu runnir
  run_now = true

description and run_now are optional; run_now defaults to false, so a snippet is
inserted, not executed, unless you opt in.

# Keyword watch

From the palette, Watch pane for keyword arms the focused pane: when a later line
of its output contains that word (case-insensitive), runnir raises a desktop
notification with the matching line. Point it at a build, a deploy or a tail -f
and walk away — you get pinged on deploy OK, error, panic, whatever you set. An
empty keyword clears the watch. Scanning starts from the current bottom, so old
scrollback never fires.

# Broadcast input

@ Ctrl+Shift+B     toggle broadcast: what you type goes to every pane in the tab at
                   once. Useful for driving several servers together.

From the palette, Toggle pane in broadcast group marks the focused pane as a group
member. Once any pane in a tab is a member, broadcast is scoped to just the group
instead of the whole tab — so you can broadcast to three of five panes and leave a
log tail and a monitor untouched. With no members, broadcast covers every pane.

# Reopen a closed tab

@ Ctrl+Shift+U     bring back the last tab you closed, with its layout, working
                   directories and scrollback — like a browser's reopen-closed-tab.

# The window you closed last

With behaviour.restore_session on (the default), runnir remembers the window you
closed - tabs, split layout, each pane's directory and its scrollback - and gives it
back to the next window you open. The processes do not come back; the shells are
relaunched where they were, with the saved output above them as inert history.

It applies only when this is the ONLY window running. A second runnir opened beside
a live one starts clean, because inheriting the layout of a window that is still on
screen is a copy nobody asked for. Every terminal draws this line somewhere: tmux
attaches to a session by name, kitty applies a template you wrote rather than a
snapshot, Windows Terminal restores only its first window.

Turn it off (behaviour.restore_session = false) and every launch starts with one
fresh tab. Saving a layout deliberately is a separate thing - see below.

# Per-project sessions

runnir can remember the pane and tab layout you last used in a project and rebuild
it when you open the terminal there again. The project is the nearest git repository
above your working directory (or that directory itself when you are outside a repo),
so a layout saved anywhere inside a repo comes back for the whole repo. Only the
split shape and each pane's working directory are restored — never the scrollback and
never the running processes, which do not survive.

From the palette: Save session for this project records the current tabs and panes;
Restore session for this project rebuilds them in fresh tabs, each shell reopened in
its recorded directory.

Set behaviour.session_restore = true to rebuild the saved layout automatically when
you launch runnir inside that project. This one is a TEMPLATE you saved on purpose,
so a second window opened in the same project gets it too - unlike the snapshot of
the window you closed, which only the first window takes. Add behaviour.session_auto_save = true to also
save it on exit, closing the loop with no keystroke at all. Both are off by default.
The store keeps the 50 most recently saved projects in a small file at
~/.config/runnir/sessions.json, written atomically.

  [behaviour]
  session_restore = true
  session_auto_save = true

# Sticky command

While you scroll back, the prompt line of the command whose output you are reading
is pinned at the top of the pane, so you never lose track of which command produced
what you see. Automatic; needs OSC 133 shell integration.

# Quake (dropdown) mode

Launch with `runnir --quake` for a borderless window with the Wayland app-id
`runnir-quake`, meant to drop down from the top on a global key. Wayland gives no
application global hotkeys, so the toggle is the compositor's job. For Hyprland:

  windowrulev2 = float, class:^(runnir-quake)$
  windowrulev2 = size 100% 45%, class:^(runnir-quake)$
  windowrulev2 = move 0 0, class:^(runnir-quake)$
  windowrulev2 = workspace special:runnir, class:^(runnir-quake)$
  bind = , F12, togglespecialworkspace, runnir
  exec-once = runnir --quake

# File explorer sidebar

Leader E opens a tree of the project beside the panes, and puts the keyboard in it.
It is chrome, not a modal layer: it stays up while you work in the pane next to it,
and it takes keys only while it has focus (Escape gives them back).

  j k / arrows      move            l or Enter   unfold a directory
  h                 fold, or go to the parent    g G   top, bottom
  .                 hidden files    I  files git ignores
  s                 sort by name / by date
  y                 copy the path   Esc or q     back to the pane
  Enter on a file   open it        e  $EDITOR    o  the desktop's handler
  p properties & permissions   a new file (a/ for a directory)
  r rename          d delete       R  reread the tree

Each row carries what git says about it: M modified (yellow) or staged (green),
A added, D deleted, ? untracked, ! conflicted, and a dot on a directory meaning
something below it changed. A directory never borrows a child's letter - an M on a
folder would claim the folder itself was edited.

What git ignores is hidden, and the footer says how many rows that is - I shows
them again, dimmed. s switches between sorting by name (directories first) and by
date changed, newest first with directories mixed in: a directory's own mtime moves
when something is created or removed in it, which is the event that sort is for.
The marks are re-read when the index or HEAD moves, or when a command finishes in
the pane. Nothing polls.

What Enter does depends on what the file IS, decided by its bytes and not by its
name: an image opens in the viewer, text opens in the viewer, and a binary says so
rather than being shown as mojibake. The viewer is read-only on purpose — runnir is
a terminal, so its editor is whatever runs in a pane.

  In the viewer: j k J K scroll, h l sideways, g G ends, e $EDITOR, o the desktop,
  y copy the path, Esc back to the tree.

An image is drawn as the real picture, scaled to the panel by the worker that
decoded it and centred there - not as text art. Scrolling is off for it because the
whole picture is on screen. Half-block art is still what you get if a file decodes
to art but not to pixels.

$EDITOR (and a script you choose to run) goes to the focused pane when it is sitting
at its prompt and to a new split when something is already running there, with the
path shell-quoted.

Properties (p) shows what a path is, how big, when it changed, and its permission
bits as a grid you move around in: hjkl or the arrows pick a bit, space flips it,
Enter writes it. Nothing touches the disk until Enter. On a directory, R marks the
change as recursive and Enter then asks first, counting what it would touch. On a
symlink the panel says out loud that permissions land on the TARGET, because
chmod follows links and there is no portable way not to.

Renaming refuses to overwrite an existing name and refuses a name that is a path,
so a rename box can never move a file out of the tree. Deleting asks first and, for
a directory, counts the files and directories inside by name before you answer —
and Enter is not a yes to any of those confirms.

Nothing that RUNS is ever run by one keypress. An executable text file — a script —
is three things at once, so Enter asks which: view, edit, run, or hand it to the
system. An executable binary or a .desktop file has no default action at all: it
asks first, naming what would be launched, because xdg-open on those executes a
handler and a cloned repository can carry one.

The root is the git repository of the focused pane's directory, or that directory
when it is not a repo. It follows the shell into another REPOSITORY, not into every
cd: re-anchoring on each directory would collapse the tree while you navigate.

Click a row to select it, click it again to open it, and drag the sidebar's edge to
resize it — the panes are only resized when you let go, because a PTY resized on
every frame of a drag is one full-screen program redrawing itself into a corner.

With the tree focused, the leader key opens a menu of file verbs, the same which-key
the git panel uses: F file, D directory, V view, Q back to the pane. It offers only
what the row under the cursor can do - the file verbs are not offered on a directory
and the directory ones are not offered on a file - and every leaf presses a key the
sidebar already binds.

Config: explorer.side (left/right), explorer.width in COLUMNS (not a fraction: a
fraction on an ultrawide gives a 90-column tree), explorer.show_hidden. All three
are in the settings panel too.

# Docker panel

Leader D opens it: three columns - the docker hosts (your contexts, plus Docker Hub
as a host of its own), the objects on the selected host, and the detail of what is
selected. Same shape as the git panel, and the same keys where they mean the same
thing.

  tab / h l        move between columns    j k / arrows  move in one
  C I V N          containers, images, volumes, networks   [ ]  the same, in order
  enter            fold a compose project, or read the selection
  u L i            summary, logs, the whole inspect JSON
  s x R p          start (or unpause), stop, restart, pause
  d                remove it - asks first, naming what goes with it
  e                a shell inside it, in a pane
  w                open its published port in the browser
  U W P            compose up -d, compose down (asks), compose pull
  T                deploy the project: compose pull, then up -d (asks)
  >                publish the image or tag: docker push (asks)
  y z r            copy the id, zoom the detail, reread this host
  K H B            kill it, the hosts column, jump to Docker Hub
  leader           the panel's own menu of verbs   esc or q   close it

The leader inside the panel is the git panel's, in shape and in depth: C container
verbs, P compose project, I images, O objects, H hosts, B Docker Hub, D detail,
V view, Z zoom, Q close. It only offers what the row under the cursor can do - no
compose verbs on a network, no hub verbs on a daemon, no kind strip on hub - and a
leaf can TAKE you somewhere before it acts: leader I P publishes an image from
wherever you are standing, switching the column on the way. Every leaf presses a
key the panel already binds, so a verb cannot mean one thing from its letter and
another from the menu.

Containers are grouped by their compose project, because that is the unit the work
is done in: nobody deploys a container, they deploy a project. The heading counts
how many of its containers are up. Health is its own mark beside the state, never
folded into it - up and unhealthy is the state worth seeing.

Everything is read over the daemon's own socket, on a worker thread, and a remote
context is reached through the tunnel the CLI uses (ssh <host> docker system
dial-stdio). A host that cannot be reached is drawn as down and never stalls the
window. A host is only READ when you choose it, so moving over one costs nothing.

Docker Hub is the last host in the column. It lists the repositories your login
can see; when the stored credential is an ORGANISATION token, Hub's web API
refuses it (the registry does not), so the list falls back to the repositories your
local images name - and the header says which of the two you are looking at.
Enter reads a repository's tags, and each tag says how it compares with what is
here: the same image, a different one, not pulled here, or built here and never
pushed. Compared by DIGEST, never by id - a local id says nothing about a registry.

Short operations run on the socket and answer in the footer. Anything that takes
minutes or prints progress - a shell, compose up, compose pull - goes to a real
pane instead, because a pane already has colour, Ctrl-C and scrollback. A command
that reaches another machine asks first, with the host named.

# Remote control

A running runnir listens on a per-user socket and exports its path to the panes as
RUNNIR_LISTEN, so anything inside a pane can drive its own terminal:

  runnir @ ls                          tabs, panes, ids, cwds
  runnir @ send-text --text 'ls\n'     write to a pane's shell
  runnir @ get-text                    read a pane's text back
  runnir @ launch --type split --cmd htop
  runnir @ new-tab / focus-tab --index 2 / close-tab
  runnir @ set-colors --opacity 0.85 --bg '#101014'

send-text talks to the CHILD. These five talk to runnir itself, so they reach the
overlays, the leader layer and everything bound to a key:

  runnir @ action --id git_panel       run an action by its config id
  runnir @ key --chord enter           press a key (same spellings as the config)
  runnir @ click --col 30 --row 6      click a cell of the window
  runnir @ drag --col 40 --row 6 --to-col 60    press, move, release
  runnir @ wheel --col 4 --row 10 --lines -3    turn the wheel there (+ is up)

They answer with what is on screen — which overlay is up, and for the git panel its
view, focus, cursor, column widths and open commit — so a script can check what it
just did instead of guessing. This is how the panels are tested.

# Inline images (kitty graphics protocol)

runnir understands the kitty graphics protocol, so tools that speak it draw real
images in the grid — image previews, plots, icons.

  kitten icat photo.png
  # matplotlib, timg, chafa --format kitty, etc.

Images scroll with the text that placed them and evict with the scrollback. A
support query is answered so tools auto-detect it.

# Auto-preview of generated images

Point runnir at a directory your image pipeline writes to (SDXL, ComfyUI, Wan and
the like) and every new file it drops is previewed inline in the focused pane,
scaled down to fit. It reuses the same image path as the kitty graphics protocol,
so the preview looks exactly like an icat one.

From the palette:

@ Auto-preview images: toggle on this pane's dir   arms the watch on the focused
                   pane's working directory, or turns it off if already on
@ Auto-preview images: set / clear watched dir     type a directory to watch; an
                   empty line clears the watch

Only files created or modified after you arm the watch fire, so the existing
contents of the folder never flood the pane. A file still being written is held
back until its size settles, so you never see a half-rendered image. When several
land at once only the newest is shown. A preview is skipped while a full-screen app
(vim, htop) has the focused pane, and picked up once you leave it.

It can also start on its own. In the config:

  [watch]
  enabled = true
  directory = ~/comfyui/output
  extensions = [ png, jpg, webp ]
  max_width = 40

An empty extensions list previews any file. max_width is the widest a preview is
drawn, in cells; a bigger image is scaled down to it, a smaller one left alone.

# Now playing (media)

See and control whatever is playing without leaving the terminal. An overlay shows
the current track (title, artist, album), the album art, the playback state, and a
live audio waveform.

@ Alt+Shift+P      open the now-playing overlay

Inside the overlay:

@ space            play / pause
@ n / p            next / previous track
@ + / -            volume up / down
@ Esc              close

The XF86 media keys on your keyboard (play/pause, next, previous) also work
anywhere, and every command is in the palette (Media: play / pause, and so on) for
a keyboard-only workflow. If no player is active, a brief toast says so.

The album art renders as coloured half-block characters, so it shows on any GPU
without extra plumbing. The waveform is drawn as Unicode bars that rise and fall
with the sound.

Requirements: on Linux, playerctl (any MPRIS player: mpv, Spotify, browsers, Music
Assistant) for metadata and control, and cava for the waveform. On macOS,
nowplaying-cli if installed, otherwise AppleScript against Music or Spotify; album
art and the waveform are skipped there. A missing tool degrades gracefully to a
toast or a plainer overlay, never an error.

In the config:

  [media]
  waveform = true
  bars = 24
  art_cells = 18

waveform draws the cava wave (it shows nothing when cava is absent); bars is how
many wave columns to compute; art_cells is the album-art width in cells.

# Font

@ Ctrl++ / Ctrl+-  bigger / smaller (live, no restart)
@ Ctrl+0           reset to the configured size

# Transparency and blur

Set window.opacity below 1.0 (e.g. 0.9) in the config for a translucent window.
The default background shows the compositor through, so a blur rule behind runnir
takes effect; text and colored cells stay fully opaque and readable. For Hyprland:

  windowrulev2 = opacity 1.0 override, class:^(runnir)$
  blur = yes  (in decoration {}), then the compositor blurs behind runnir

Only the default background is translucent; explicit backgrounds, selections and
matches stay solid.

# Configuration

From the palette, Settings opens an interactive panel for every option: arrows (or
j/k) move, left/right (or h/l) change a value, Enter edits a text field, s saves.
Saving writes ~/.config/runnir/runnir.json, which is loaded in preference to the
TOML file; edits apply live as you make them.

Config is TOML at ~/.config/runnir/runnir.toml (or JSON at runnir.json, which wins).
Run `runnir --write-config` to write a fully-commented default. Every setting has a default, so a partial or
missing file is fine. API keys are referenced by environment-variable name, so the
file is safe to keep in a dotfiles repo.

@ window.opacity   0.1..1.0 window translucency (needs a compositor; 1.0 = opaque)
@ window.status_bar  true shows a bottom bar (cwd, git branch, clock); costs a row
@ window.background  path to an image drawn behind the terminal (needs opacity < 1)
@ window.background_dim  0..1 how bright the background image is (default 0.35)
@ window.minimap   true shows a scrollback minimap on the focused pane; click to jump
@ cursor.trail     true draws a brief fading trail behind the cursor (flair, off)
@ behaviour.smooth_scroll  true glides on scroll jumps instead of teleporting
@ behaviour.shell_integration  true auto-injects OSC 133/7 hooks into fish/zsh/bash (no rc edits)
@ clipboard.capacity   how many recent copies the clipboard history keeps (default 50)
@ clipboard.enabled    false stops recording copies into the history
@ leader           the chord that arms the leader layer (default alt+shift+space;
@                    an empty string turns the layer off entirely)
@ leader_timeout   seconds the armed layer waits per step (default 10; 0 = it
@                    waits as long as you do)
@ [keys]           action id -> chord. A `leader+` prefix binds on the leader
@                    layer, and a space separates the steps: `leader+r c` is the
@                    leader, then R, then C

Each tab shows an icon for its foreground app and a badge: an amber dot for a
background tab with unseen output, a red cross if its last command failed. The tab
bar scrolls to keep the active tab visible. A pane scrolled back shows a thin
position thumb; a tool emitting OSC 9;4 progress draws a bar along its bottom edge.

The config hot-reloads: save the file and runnir applies the new theme, font and
key bindings within a second, no restart. Toggling window.opacity between opaque
and translucent is the one change that still needs a restart.

@ Ctrl+Shift+P     command palette — every command, fuzzy-searchable

The palette also hosts commands with no default chord, including Insert from
shell history (fuzzy-pick a past command from fish/zsh/bash history and type it
at the prompt), Open scrollback in $EDITOR, and the Theme picker (browse the
bundled colour themes with live preview; Enter keeps and saves one, Esc restores).

# Why 'runnir'

From Old Norse 'run' (secret, whisper) and the '-nir' of Mjolnir and Gungnir. The
rune-artifact: a place to whisper to the machine.
";
