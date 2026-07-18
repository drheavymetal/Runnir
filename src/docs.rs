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

# Tabs

@ Ctrl+Shift+T     new tab
@ Ctrl+Shift+W     close tab
@ Ctrl+PageUp/Dn   previous / next tab
@ Ctrl+Shift+Left/Right  move the current tab left / right in the bar
@ Super+1..9       jump to tab N
@ Ctrl+Shift+R     rename the current tab

# Splits (panes)

A tab can be divided into panes. Every pane is its own shell; a split inherits the
working directory of the pane you were in, so it opens where you already are.

@ Ctrl+Shift+D     split left / right
@ Ctrl+Shift+E     split up / down
@ Ctrl+Shift+X     close the focused pane
@ Ctrl+Shift+Z     zoom the focused pane to fill the tab (toggle)
@ Ctrl+Shift+HJKL  move focus left / down / up / right (vim directions)
@ Super+arrows     resize the focused pane

A pane that rings the terminal bell (BEL) flashes briefly; if the window is not
focused it also raises the compositor urgency hint, so a finished build in the
background gets your attention without stealing focus.

Focus movement is geometric: 'focus right' goes to the pane you see to the right,
whatever order you built the splits in.

# Scrollback and selection

@ wheel / Shift+PageUp/Dn   scroll the history
@ drag                      select text (copied on release)
@ Ctrl+Shift+C / V          copy / paste
@ middle click              paste the primary selection (the last text selected)
@ Ctrl+Shift+Home/End       jump to top / live output
@ Ctrl+Shift+F              search the scrollback; Enter/Up next/prev, Esc closes
@ Ctrl+Shift+Q              dump the scrollback to $EDITOR in a new split

# Mouse in full-screen apps

Clicks, drags and the wheel are forwarded to programs that ask for the mouse
(vim, tmux, htop, less), so clicking a pane in tmux or a process in htop works.
Hold Shift to override and select text instead, even inside such an app.

Any key you type snaps the view back to the live output, so you never type into a
scrolled-back screen and wonder why nothing happens.

# Shell integration (OSC 133)

If your shell emits OSC 133 marks, runnir understands where each command's prompt,
input and output begin. That powers:

@ Ctrl+Shift+Up/Down   jump to the previous / next command
@ Ctrl+Shift+O         copy the output of the last command
@ Ctrl+Shift+G         ask the AI why the last command failed

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

# Reopen a closed tab

@ Ctrl+Shift+U     bring back the last tab you closed, with its layout, working
                   directories and scrollback — like a browser's reopen-closed-tab.

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

Config is TOML at ~/.config/runnir/runnir.toml. Run `runnir --write-config` to
write a fully-commented default. Every setting has a default, so a partial or
missing file is fine. API keys are referenced by environment-variable name, so the
file is safe to keep in a dotfiles repo.

@ window.opacity   0.1..1.0 window translucency (needs a compositor; 1.0 = opaque)

The config hot-reloads: save the file and runnir applies the new theme, font and
key bindings within a second, no restart. Toggling window.opacity between opaque
and translucent is the one change that still needs a restart.

@ Ctrl+Shift+P     command palette — every command, fuzzy-searchable

The palette also hosts commands with no default chord, including Insert from
shell history (fuzzy-pick a past command from fish/zsh/bash history and type it
at the prompt) and Open scrollback in $EDITOR.

# Why 'runnir'

From Old Norse 'run' (secret, whisper) and the '-nir' of Mjolnir and Gungnir. The
rune-artifact: a place to whisper to the machine.
";
