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

Press Alt+Shift+Space, let go, then press one plain key. It has three seconds
before it lapses; any unbound key just cancels it, and nothing leaks to the shell.

@ Leader 1..9      jump to tab N
@ Leader hjkl      resize the focused pane (arrows work too)
@ Leader V         clipboard history
@ Leader S         command snippets
@ Leader P         now playing
@ Leader G         fix the last failed command

Rebind the leader itself with the `leader` setting (an empty string turns the
layer off), and bind your own sequences with a `leader+` prefix in [keys] — see
the config section below.

Everything on the leader layer is also in the command palette (Ctrl+Shift+P),
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
@ Alt+Shift+arrows resize the focused pane (also Leader hjkl / arrows)

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

runnir talks to an assistant without leaving the terminal. Claude runs through the
Claude Code CLI against your subscription — no API key. Other providers (OpenAI,
Gemini, DeepSeek, Z.ai) use their HTTP APIs, with the key taken from an environment
variable named in the config, never stored in the file.

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
@ Ctrl+Shift+I     summarize this session (commands, results, errors and fixes)

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
SQL DROP/TRUNCATE, git force-push and the classic fork bomb. Only a bare Enter at
the live prompt is guarded, so editing history and full-screen apps are untouched.
Turn it off with behaviour.command_guardian = false.

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
you launch runnir inside that project. Add behaviour.session_auto_save = true to also
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
