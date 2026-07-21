# runnir — DEVLOG

Working memory across sessions. When context runs out, read this first to resume.

Repo: `git@github.com:drheavymetal/Runnar.git` (repo **Runnar**, crate **runnir**).
Commits **unsigned** (`git -c commit.gpgsign=false commit`). Push after each unit.
Build: `cargo build` / test: `cargo test` / release: `cargo build --release`.
Shell is fish: NEVER put backticks in `git commit -m "..."` — fish command-substitutes
them even inside double quotes and silently drops the word. Use plain quotes.
Relaunch live: `pkill -x runnir; setsid ./target/release/runnir >/tmp/runnir-live.log 2>&1 </dev/null & disown`.
Verify headless: `runnir --dump '<cmd>'`, `runnir --render out.png '<cmd>' [ms]`, `runnir --demo out.png`.

## >>> INTEGRATED (2026-07-19) — parity batch ALL MERGED to main <<<
All 7 branches merged into main, commit f442b3d (amended: worktrees untracked +
.gitignore). `cargo test` → 221 passed, 0 warnings. tab.rs conflict resolved
(Pane::new rollback + shell_integration param); Spawn.env added to app_input.rs
new-pane path. Fable bug-hunt on the integrated batch: IN PROGRESS. History below:
1. Styled underlines (undercurl/dotted/dashed/double + SGR 58 color): branch
   `worktree-agent-a4570a098001fe55e` commit a67c644 — grid.rs, render.rs, shader.wgsl. Pen gained underline+underline_color; Instance +2 attrs.
2. Rectangular selection (Alt/Ctrl+drag, Mode::Block): branch
   `worktree-agent-ad2600a7fb8ef9951` commit b7cd93f — selection.rs, app_input.rs.
3. Kitty keyboard protocol (CSI u): branch `worktree-agent-ab4f132a097ab2b02`
   commit 4f35401 — grid.rs (kbd_flags_stack), keys.rs (encode_kitty), app_input.rs, main.rs (forward releases), pane.rs.
4. Multiple layouts (splits/stack/tall/fat/grid, CycleLayout palette): branch
   `worktree-agent-ad4a60d7e6818b706` commit 8345bf7 — layout.rs, tab.rs (order Vec), actions.rs, app_input.rs, session.rs.
5. Theme picker (28 themes, Overlay::Theme, live preview): branch
   `worktree-agent-afebf5b1b73052bb5` commit 291b120 — themes.rs (new), overlay.rs, actions.rs, app_input.rs, config.rs, docs.rs.
6. Remote control API (`runnir @`, UserEvent::Control, socket): branch
   `worktree-agent-a9f7132d0632d1031` commit 66be1c2 — control.rs (new), main.rs, app_input.rs.
7. OSC 52 + OSC 99/777 + auto shell-integration: branch
   `worktree-agent-a054731dbfd456984` commit 67b67d8 — grid.rs (osc_dispatch OSC 52/9/99/777 + clipboard_writes/notifications queues), main.rs (drain in periodic), pane.rs, pty.rs (Spawn.env), clipboard.rs, shell_integration.rs (NEW, behaviour.shell_integration default true).
ALL 7 CODE BRANCHES DONE — integration in progress. Mark each done here as merged.
Also running: React docs site → `worktree-agent-aa3ede6dbf9d25c23` (docs-site/ only, additive, merge trivially).
QUEUED after integration (touch grid/render, would conflict): Unicode/grapheme rigor, IME, Sixel, text-sizing protocol. Then optional: triggers, quick-select, navigable command blocks, file-transfer.
Merge order suggestion: independent-file first (control.rs, themes.rs, selection.rs, layout.rs) then the grid.rs/app_input.rs-heavy (underlines, keyboard, osc52) resolving cumulatively.
## >>> END PENDING INTEGRATION <<<

## 2026-07-19 — Minimap draws coloured text runs, not bars

Pedro: "solo se ven cubos" — the strip showed one bar per line, width = `row_fill`
(how far the line's last glyph reached). That only encodes line LENGTH, so it read
as a stack of blocks with no structure.

Now each sampled row is drawn as its runs of ink, in the text's own colour, so the
strip is a shrunken picture of the screen: indentation, blank lines and coloured
output are all recognisable. `Grid::row_runs_into(abs, &mut Vec<(col, len, Color)>)`
groups adjacent non-blank cells sharing a foreground colour; a space OR a colour
change breaks a run. Fills a caller-owned Vec instead of returning one — it runs
once per sampled row on every frame the grid changes, and a per-row allocation would
dominate. A spacer (right half of a wide glyph) extends a run rather than breaking it:
it carries no glyph but it is ink.
`app_draw` resolves each run's colour against the theme (Default → foreground,
Indexed → `render::xterm256`, now `pub(crate)`) and emits one SolidRect per run at
`x0 + col*cw`, `cw = strip_w / cols`. Sub-pixel runs still render — the quad covers
part of a pixel, which reads as a lighter mark. `row_fill` deleted (last caller gone).

Cost: decorations all join the single instanced draw call, so thousands of rects add
instances, not draw calls. Measured under identical load (3x a 350-line colourful
generator): 12 CPU ticks with the minimap OFF vs 9 with it ON — no regression outside
noise.

Verified by screenshot, since no headless path draws the minimap: real instance under
a scratch `XDG_CONFIG_HOME`, floated and sized via `hyprctl`, fed colour through a
script FILE (escape sequences do not survive the quoting layers of `@ send-text` —
first attempt rendered a literal `33[36m`), then `dms screenshot window`. The strip
shows green comment runs and per-line colour with indentation offsets.

## 2026-07-19 — Leader key + minimap column reserve (Hyprland fallout)

Two bugs Pedro hit on his Hyprland setup.

**1. The super layer never reached runnir.** A compositor grabs its modifiers before
the app sees the key, so every `super+*` default was dead: Hyprland binds super+1..9
(workspaces), super+arrows (movefocus), super+v (togglefloating), super+s
(togglespecialworkspace), super+p (pseudo). GNOME claims the same layer. This is not
fixable by picking a different super chord — the whole layer belongs to the WM.
Fix, two parts:
- The super defaults moved to `alt+shift` (resize arrows, clipboard-history V,
  snippets S, now-playing P, fix-last-command G). New test
  `defaults_avoid_the_compositor_owned_super_layer` fails the build if a super chord
  is ever added back.
- New LEADER LAYER (actions.rs): `alt+space`, release, then one plain key. Keymap
  gained `leader: Option<Chord>` + `leader_bindings` (a separate map — bare `v` is an
  action there and a literal `v` everywhere else). `config.leader` rebinds it, `""`
  disables it. User bindings take a `leader+` prefix in `[keys]`.
  Defaults: 1..9 go-to-tab (they could NOT go on alt+N — that is readline's
  digit-argument), hjkl/arrows resize, v/s/p/g as above.
  State is `Gpu.leader_armed: Option<Instant>` + `LEADER_TIMEOUT` 3s (an
  indefinitely armed leader would turn a keystroke typed minutes later into an
  action). Handled in `on_key` after the overlay/copy-mode gates: a modifier press
  alone does not consume the arming, ANY other key ends the sequence bound or not
  (falling through would leak a stray char into the shell after a mistype), and an
  expired arm falls through to the pane instead of being eaten.

**2. Text rendered underneath the minimap.** `app_draw` drew the 46px strip at the
pane's right edge, but `tab::cells_in` sized the grid from the FULL pane width, so
the last ~5 columns were painted under it. The tab bar and status bar had always
reserved their rows in `content_area`; the minimap never got the same treatment.
`cells_in` now takes the flag and subtracts `crate::MINIMAP_W` (the magic 46.0 was
duplicated in 3 places, now one const). Reserved on EVERY pane, not just the focused
one that draws the map: making it follow focus would reflow and re-wrap two grids on
every focus change. A pane too narrow to draw the strip does not pay for it, matching
the draw guard. `Tab.minimap` field + `set_minimap` so hot-reload can adopt a toggle
and reflow (mirrors how `status_bar` is handled in `apply_config`).

Verified end-to-end, not just by unit test — both headless modes (`--dump`,
`--render`) use a fixed 80x24 grid and never touch `Tab`, so neither can catch this
class of bug. Instead: launched a real instance under `XDG_CONFIG_HOME` pointed at a
scratch config (so the running terminal was untouched), floated+sized the window with
`hyprctl` (tiling made every launch a different width — measurements were pure noise
until pinned), and read `tput cols` back over the control socket, targeted by
`RUNNIR_LISTEN=$XDG_RUNTIME_DIR/runnir-<pid>.sock` so the client could not talk to
the wrong instance. 1000px window: 98 cols minimap off, 93 on — exactly the 5 columns
reserved. For the leader, `runnir @ send-text` is useless (it writes to the PTY,
bypassing `on_key`); `hyprctl dispatch sendshortcut` injects real key events, so
alt+space then `t` with `"leader+t" = "new_tab"` took the tab count 1 → 2, and
alt+space then an unbound `z` left the prompt with exactly one `z` (the leader ate
the first, the plain keypress after wrote the second).

283 tests pass. `shell_integration::fish_prepends_xdg_data_dirs` fails in this shell
both before and after these changes — it needs `XDG_DATA_DIRS` set, which fish here
does not export. Pre-existing, untouched.

## 2026-07-19 — AI fix-and-run for a failed command
FixLastCommand action (Super+Shift+G — every ctrl+shift letter was taken; mirrors
ask-why ctrl+shift+G on super; palette "AI: fix the last failed command"). Guards on
last_exit(): only fires when OSC 133 recorded a non-zero exit, else a toast. Sends the
failed command + output + exit code asking for ONLY a corrected command, then reuses
Purpose::InsertCommand to TYPE it at the prompt (never runs it). New grid field
last_command_line captured at OSC 133;C (B mark, falls back to prompt mark) with a
last_command_line() accessor on Grid+Pane, alongside last_command_output()/last_exit().
ai::clean_command now also strips a leading `$` prompt marker. Tests: clean_command
+prompt-marker cases, grid fix_last_command_captures_command_and_exit (capture+guard
data). cargo build + test 222 green, 0 warnings. app_ai.rs::fix_last_command.

## 2026-07-19 — image auto-preview watch (branch worktree-agent-a752120c0a0af7272)
New `[watch]` config block (enabled/directory/extensions/max_width, all defaulted,
serde round-trips TOML+JSON) + src/watch.rs: pure `WatchState`/`step` debounce state
machine (snapshot on arm so old files never fire; a new file waits one stable tick;
newest of a batch previewed). Polled from Gpu::poll_image_watch on the periodic tick
(WATCH_POLL_MS=700, self-sustaining WaitUntil added in about_to_wait next to blink;
skipped on alt-screen). preview_image decodes + downscales to max_width cells (capped
at max_texture_dimension_2d, the 8192 guard) then reuses Grid::place_image (no new
image drawing). Two palette actions: ToggleImageWatch (arms on focused pane cwd) and
SetImageWatchDir (PromptKind::ImageWatchDir, empty clears). 8 new tests. Caveat:
previews are injected at the live cursor, best viewed at an idle prompt.

## Status (2026-07-18)

Done: M0–M7 + 4 feature blocks + kitty graphics + mouse-to-TUIs + scrollback search
+ whisper + live-testing polish + DA1/fish fix. 39 commits, 148 tests, 0 warnings.
17 agent-found bugs fixed (7 default-model + 10 Fable 5). Logo installed.

Feature sprint this session (commits 26–39): 15 new features shipped —
WOW: W1 guardian, W3 named layouts, W4 keyword watch, W5 opacity/blur.
DIFF: D2 hot-reload, D3 history fuzzy, D4 bell, D5 primary selection, D7 AI summary,
D8 broadcast groups, D9 smooth scroll, D10 zoom, D11 scrollback→$EDITOR, D13 tab
reorder, D14 URL hover. Deferred (heavier, next session): W2 folding, D1 OSC 8
hyperlinks, D6 status gutter, D12 copy-mode vim, D15 cursor trail.
Fable 5 bug-hunt round 1 (4 parallel agents) → 25 findings, fixed in commit 40:
- Guardian: OSC 133;B mark → scan excludes prompt prefix (was missing rm -rf / in
  real shells!); split on && and trailing ; take last non-empty; skip on alt-screen.
- W4 watch: watch_mark (cursor-row high-water, not stable_end) + text_since_stable
  saturating_sub — fixes infinite notify spam + missed on-screen lines.
- Bell: about_to_wait drives flash decay (was frozen); check_bells all tabs (urgency)
  flash only active; RIS preserves bell_count (was phantom bell on `reset`).
- Opacity: only applied when PreMultiplied actually selected (else darkened).
- Config hot-reload: try_load keeps previous on parse error; apply_config rebuilds on
  family/ligature change and no longer snaps runtime font-zoom.
- Layouts: toast when splits truncated (window too small).
- Paste: strip bracketed-paste markers (injection); insert_command flattens newlines
  (history multiline could self-execute).
- Clipboard: reap wl-copy (zombie leak); X11 PRIMARY via arboard ext.
- Scroll: zero-delta guard (spurious wheel), docs-overlay uses real cell height.
- move_tab: remove+insert (swap scrambled at wrap). History: single-pass fish
  unescape, zsh unmetafy. Hover: real grid columns + display width (wide chars).
- Scrollback dump: $XDG_RUNTIME_DIR + 0600 + O_NOFOLLOW (was predictable /tmp).
- Picker: Down clamped to rendered window. 155 tests.
Fable 5 rounds 2-4 (convergence): r2 5 bugs (commit 41), r3 6 bugs incl. guardian
cursor-bypass + zoom desync (commit 42), r4 2 bugs (commit 43): zoom tab-switch
reflow-wrong-tab regression → sync_zoom now reflow_all; RPROMPT "+2" false-positive
in git force-push → +refspec must carry a letter. r4 verified fixes 3/4/5 clean.
Documented limitations (conservative guardian, not a security boundary): a command
wrapped across multiple rows + Home + Enter can scan short of the wrapped tail;
RPROMPT tokens on the input row are in scan range. Pre-existing/deferred: dead
copy_on_select/confirm_close config, ctrl+plus/minus chord unreachable, RIS wipes
prompt_marks, broadcast-group last-member widening.
D1 c44, D6 c45, D12 c46, D15 c47. 15 of 16 planned features done (all but W2).
Fable 5 bug-hunt on the 4 new features → 11 findings fixed (commit 48):
- D6 gutter: skip on alt-screen (was overdrawing vim); prune prompt_marks/cmd_exits
  on eviction (unbounded growth + lost status).
- D1 OSC 8: preserve links table across RIS (stale ids aliased new URIs); table-full
  degrades to untagged not aliased.
- D12 copy-mode: bind ops to cm.pane not focused() (mouse focus → wrong-pane yank);
  exit on any click; rebase cur/anchor by dropped-delta on eviction; start visible
  when scrolled back; Ctrl/Alt/Super exits instead of mis-reading as motion.
- D15 trail: prune in about_to_wait (was 60Hz spin-loop forever when occluded); skip
  sampling while scrolled back; key last_cursor_rect to pane (no ghost on focus jump).
Verify round on those 11 fixes → all correct, only 3 lows (commit 49): trail ghost on
relayout, CSI 3J cmd_exits prune, copy-mode end_selection. CONVERGED.
ALL 16 planned features DONE (W2 = commit 50, view-only fold plan, no coord rewrite).
Fable 5 bug-hunt on W2 → 5 bugs fixed (commit 51): OSC 133;D banked the next
prompt row as output end → fold swallowed the live prompt+cursor (exclude cursor
row when col==0); folds applied on alt-screen (has_folds gated on !parked); gutter/
images/hover/hints used raw local-top → added grid.screen_row_of() fold-aware
mapping; clear/2J left stale live-screen folds → invalidate_screen_folds; resize
shrink duplicated summary → jump to total on evicted fold end. + label clamp,
fold-click clears selection. W2 verify → fix 4 regressed (moderate, commit 52): invalidate ran on alt-screen 2J
wiping primary folds → gated on !parked; now range-based (only erased rows, not all
screen, so prompt-redraw clr_eos doesn't pop folds); also invalidates last_output.
Fixes 1/2/3/5/6 verified correct. ALL 16 FEATURES DONE + BUG-HUNTED. CONVERGED.

## 2026-07-18 later — "resultonas y bonitas" batch (commits 54+)
Pedro picked all 4 flashy blocks + reported the tab bar overflowing.
Done: tab-bar horizontal scroll to active (fix, commit 54); tab icons (nerd-font per
app) + activity/fail badges (55); scroll position thumb + note search N/M already
existed (56); smooth scroll GLIDE on jumps (behaviour.smooth_scroll, 57); OSC 9;4
progress bar on pane bottom (58); bottom STATUS BAR cwd/git-branch/clock
(window.status_bar default on, 59). PENDING (heaviest, own batch): background image
(needs draw-first bg quad + opacity<1) and scrollback minimap.
Bug-hunt (2 agents) → 6 bugs incl. 2 CRITICAL (deadlock ≥2 tabs: build_chrome
re-locked guards' grids via tab badges → moved before guards; startup panic on
uptime<60s: Instant-60s → Option<Instant>) + HIGH (build_chrome drew its own label
not tab_label → wrong clicks/badge) + unicode width + date-storm + glide input
swallowed. commit 61. Verify → all correct, 4 lows; fixed glide-cancel gaps
(search Esc / clear), tab-click under reserved tags, context-tag width. commit 62.
CONVERGED.
Background image (commit 63): bg_shader.wgsl fullscreen tri, premultiplied, drawn
first; Background struct (own pipeline+texture+uniform, cover-crop scale); needs
opacity<1 to show; load_background at startup + hot-reload. Minimap (commit 64):
window.minimap, decorations strip on focused pane right edge (row_fill per sampled
line + viewport highlight), minimap_jump click-to-scroll. bg+minimap hunt → 4 HIGH (img>8192 panic; opacity forced 1.0 hid bg;
minimap_jump zoom-rect mismatch; narrow-pane escape) + lows, fixed commit 65.
Verify → all correct, 1 minor regression (narrow focused pane lost its thumb),
fixed commit 66. ALL 4 FLASHY BLOCKS DONE + BUG-HUNTED. CONVERGED.

## 2026-07-19 — Clipboard history (branch worktree-agent-a3c7d5968d006b569)
In-memory ring of recent copies + fuzzy picker to re-paste. clipboard.rs gained
ClipHistory (VecDeque, bounded, dedup-to-top on repeat, skips blank, off when
disabled; never persisted — privacy). set_clipboard() (app_input.rs) is now the ONE
sink: it pushes to history then sets the OS clipboard. Every copy routes through it —
selection/Ctrl+Shift+C (copy_selection), copy-mode yank (exit_copy_mode),
copy-last-output, OSC 52 (main.rs periodic drain, inlined push+set to keep the field
borrow disjoint from &mut self.tabs), and hint copies (hints::act now RETURNS
Option<String> instead of taking &mut Clipboard, callers route through set_clipboard).
Config: new [clipboard] block {capacity=50, enabled=true}, serde default + JSON/TOML
round-trip. Action::ClipboardHistory (id clipboard_history) bound to Super+V — every
ctrl+shift+letter was already taken, so it lives on the super layer (Win+V mnemonic).
Overlay::ClipHistory(ClipHistoryPicker) mirrors Palette/ThemePicker: filters on the
full entry text, one-line preview per row (pilcrow marks multi-line), Enter pastes
the FULL entry via paste_text (bracketed-paste + control-byte sanitised), Esc closes.
apply_config re-configures the ring on hot-reload. docs.rs: new # Clipboard history
section + @ Super+V lines + config flags. Tests: ring (evict/dedup/disabled/
configure/blank), config round-trip JSON+TOML, picker preview/filter. 228 pass, 0
warnings.

## Current task: add 15–20 differential features, 4–5 "wow". Document each here.

User wants all of these, in batches, committing each, updating this file. Then run
Fable 5 agents until no bugs. Fable 5 IS available now (Pedro has credits).

### Planned features (mark [x] when shipped, with commit)

WOW (aim 4–5):
- [x] W1 command guardian — src/guardian.rs danger() scans the line (rm -rf root/home,
      dd of=/dev, mkfs, DROP/TRUNCATE, git push -f, fork bomb, chmod 777 /). on_key
      intercepts bare Enter → Grid::current_command_text (last prompt mark→cursor) →
      PromptKind::GuardedCommand confirm; Enter runs, Esc edits. behaviour.command_guardian
      (default true). 6 unit tests. commit 28. (Rule-based, not the LLM — instant, offline.)
- [x] W2 Command output folding — grid.folds/outputs (stable ranges, OSC 133 C→D),
      PlanRow enum + display_plan() (fold-aware row plan); render pane_instances iterates
      the plan, emit_fold_summary draws "N lines folded", cursor_screen from plan;
      point_in/fold_row_at map clicks through the plan (click summary → unfold);
      FoldOutput palette action (toggle fold-all). View-only, no coord mutation. commit 50.
- [x] W3 Named layouts — config [[layouts]] {name, commands[]}; palette LaunchLayout
      → Prompt(LaunchLayout) → launch_layout: new Tab running cmd[0], splits alternating
      axis for the rest (argv_of whitespace-split). commit 35.
- [x] W4 Keyword watch — Pane.watch (lowercased kw) + watch_stable high-water; grid
      stable_end/text_since_stable; periodic scans new lines → notify(). WatchKeyword
      palette action → Prompt(WatchKeyword). Substring (case-insensitive), not regex.
      commit 34.
- [x] W5 Background blur + opacity — surface alpha_mode=PreMultiplied when opacity<1;
      shaders emit premultiplied (premul() + PREMULTIPLIED_ALPHA_BLENDING); clear +
      default-bg cells carry alpha=opacity; text/explicit-bg stay opaque. Compositor
      blur works behind. Renderer::set_opacity. commit 27.

DIFFERENTIAL (aim ~15 total incl. above):
- [x] D1 OSC 8 hyperlinks — Pen.link id → Grid.links table; osc_dispatch [b"8",..]
      set_hyperlink (dedup+cap); link_span; update_hover prefers real OSC 8 link over
      text detection; Ctrl+click opens. commit 44.
- [x] D2 Config hot-reload — App::maybe_reload_config (mtime, ~1Hz throttle) in
      about_to_wait → Config::load + Keymap rebuild + Gpu::apply_config (theme/opacity/font).
      Opacity opaque↔translucent still needs restart. commit 33.
- [x] D3 Shell-history fuzzy — src/history.rs recent() (fish/zsh/bash parse+dedup);
      HistorySearch palette action → Prompt(HistoryInsert) → insert_command (typed, not run).
      4 tests. commit 32.
- [x] D4 Visual + audible bell — BEL bumps grid.bell_count; Pane::take_bell; Gpu
      bell_flash+check_bells+bell_alpha; render() flash param → white fullscreen quad;
      request_user_attention(Critical) when unfocused. commit 26.
- [x] D5 Primary selection — clipboard set_primary/get_primary (wl-copy/paste --primary);
      copy_selection seeds PRIMARY; middle-click → paste_primary. commit 36.
- [x] D6 Command status gutter — grid.cmd_exits (OSC 133;D;code → prompt stable row);
      command_markers() visible prompt rows+exit; app_draw draws a green/red/grey left
      bar per prompt row (SolidRect). commit 45.
- [x] D7 Session summarizer (AI) — SummarizeSession (Ctrl+Shift+I) → scrollback_text
      → AI panel with a concise-summary prompt (tail-truncated 8k). commit 30.
- [x] D8 Broadcast groups — Pane.in_group + ToggleBroadcastGroup (palette); broadcast_bytes
      scopes to members when any exist, else all panes. commit 37.
- [x] D9 Smooth scroll — Gpu.scroll_accum carries fractional lines; slow touchpad
      sub-line pixel deltas accumulate instead of truncating to zero. commit 38.
- [x] D10 Pane zoom (Ctrl+Shift+Z) — visible_rects override + resize_one. commit 25.
- [x] D11 Open scrollback in $EDITOR — dump scrollback_text to temp file, split_running $EDITOR/$VISUAL/vi. commit 31.
- [x] D12 Copy-mode — Gpu.copy_mode (CopyMode{pane,cur,anchor}); enter_copy_mode +
      copy_mode_key (hjkl/arrows/0/$/g/G/v/y/Esc) drives a virtual cursor, mirrors to
      pane.selection, scrolls viewport to follow; y yanks. CopyMode palette action. commit 46.
- [x] D13 Tab reordering — MoveTabLeft/Right (Ctrl+Shift+Left/Right), Vec::swap +
      keep focus, wrap-around. commit 29.
- [x] D14 URL/path hover underline — Gpu.hover_url via hints::find on mouse move;
      underline drawn as SolidRect decoration; Ctrl+click → hints::act. commit 39.
- [x] D15 Cursor trail — config cursor.trail (default off); Gpu.cursor_trail ghosts
      (rect+Instant) left on cursor jump, drawn as bg↔cursor pre-blended decorations,
      animated to fade in about_to_wait. commit 47.

## 2026-07-19 — Pipe scrollback / last output through a command
Palette: Pipe last output through command / Pipe scrollback through command. Each
opens the reused Prompt overlay (PromptKind::PipeLastOutput/PipeScrollback) to type
a filter; confirm captures text (Grid::pipe_text(whole): last OSC 133 output block
vs whole scrollback), writes it 0600 to $XDG_RUNTIME_DIR/runnir-pipe-<pid>.txt
(write_private), then split_running sh -c 'CMD < "$1"' with the path as $1 (no
interpolation → no injection). Reuses the D11 $EDITOR-dump temp plumbing. actions.rs
(2 new actions, palette-only, no chord), overlay.rs (2 PromptKind), app_input.rs
(open_pipe_prompt/pipe_through + confirm_prompt + both dispatch tables), grid.rs
(pipe_text + 2 tests), pane.rs (wrapper), docs.rs (new section). 223 tests, 0 warns.

## Architecture cheatsheet (for fast edits)

- `main.rs` App/Gpu, event loop, UserEvent{Ai,Redraw}, about_to_wait (blink/status/resize).
- `app_input.rs` on_key/on_click/on_cursor/on_wheel, run_action + run_palette_action (KEEP IN SYNC),
  overlay_key, confirm_prompt. Actions dispatched from `actions.rs` Action enum.
- `app_ai.rs` AI+hints+whisper Gpu methods. `app_draw.rs` render orchestration, chrome, toast, borders.
- `actions.rs` Action enum + id()/title()/parse()/palette_list()/default_bindings()/default_hints()
  — a new action must be added to ALL of these. Chords: letters only (punct chords are layout-fragile);
  no bare ctrl+letter (belongs to shell).
- `grid.rs` VT parser (vte::Perform), scrollback (stable coords dropped+local), OSC 133, mouse mode,
  images, search, responses (DA1/DSR). `pty.rs` reader thread (graphics pre-scan, writer channel,
  Drop reaps child). `render.rs` one-draw-call instances + ImageLayer + SolidRect borders + FLAG_*.
- `overlay.rs` Palette/Docs/Prompt/AiPanel/Hints/Search — all are Grids drawn in a rect.
- `config.rs` TOML, `session.rs` opt-C persistence, `whisper.rs` NL→actions, `clipboard.rs` wl-clipboard,
  `mouse.rs` SGR/X10, `hints.rs`, `boxdraw.rs`, `layout.rs` split tree + divider_at/set_ratio, `docs.rs` F1 help.

## Per-project session (layout+cwd by project dir)

`project_session.rs` (NEW): persists the split layout + per-pane cwd (NOT scrollback,
NOT processes) keyed by project. `project_key(path)` = nearest `.git`-dir ancestor,
else the dir itself (pure, unit-tested). Store = bounded LRU (50 projects) JSON at
`~/.config/runnir/sessions.json`, written atomically (temp + `O_NOFOLLOW` 0600 +
rename), mirroring `write_private`. Reuses `session::TabState`/`Tab::from_session` for
rebuild via `TabLayout::to_tab_state()` (empty scrollback) → `ProjectEntry::to_session()`.
`Tab::to_project_layout()` captures the descriptor. Config `[behaviour]`: `session_restore`
(auto-restore on open, default false) + `session_auto_save` (save on exit, default false).
Palette: SaveProjectSession / RestoreProjectSession (restore APPENDS tabs, non-destructive,
bumps next_pane_seed). Startup restore in `init_gpu` takes precedence over `restore_session`.
Caveat: macOS cwd is OSC-7-only (`platform::cwd` returns None) — no shell integration ⇒
no cwd to key/restore; rely on `behaviour.shell_integration` (default on).

## 2026-07-20 — HiDPI / per-monitor scale, and Shift+Enter

Pedro: "en la pantalla central se ve muy pequeño" + "shift+enter no hace salto de línea".
Setup: `eDP-2` 1920x1080 scale 1.0, `DP-6` 1920x1080 scale 1.0, `DP-9` 3840x2160
**scale 1.5**. Only the 4K was wrong — `hyprctl monitors -j` is how you confirm this
in one command; do that FIRST before reading any code.

**Scale.** runnir had *no* DPI handling at all: `init_gpu` built the atlas at
`config.font.size` in raw physical px, and `WindowEvent::ScaleFactorChanged` fell
into the catch-all arm. winit binds `wp_fractional_scale_v1` for us — the scale
arrives at the app boundary and was thrown away. Fix: `font_px` is now the *logical*
size (zoom steps and the 6..72 clamp stay logical, so a size means the same thing on
every monitor) and a new `Gpu.scale` multiplies it wherever the atlas is built —
`init_gpu`, `set_font_px`, the config hot-reload. New `set_scale()` handles the event;
it is separate from `set_font_px` because the logical size does NOT change there and
`set_font_px`'s `< 0.5` early-return would swallow the rebuild.

**The real bug behind "las líneas se salen por abajo".** `Tab` caches the cell size
in `self.cell` (tab.rs) at construction and `reflow` divides pane rects by it —
**nothing ever reassigned it**. So after any font change the PTY got sized with the
OLD cell: on DP-9 the renderer drew 63 rows while `stty size` inside the pane said
92, and everything below row 63 was invisible. This was ALREADY broken for the
`Ctrl +/-` font zoom; the scale work only made it obvious. Fix: `Tab::set_cell()`
(also pushes to each pane's grid via `Pane::set_cell_px`, for inline-image sizing),
called from both reflow sites — `reflow_all()` and `Gpu::resize()`.

Debug technique worth repeating: compare what the renderer thinks against what the
child actually got, from OUTSIDE the terminal —
`for c in $(pgrep -P $(pgrep -n runnir)); do stty size < /proc/$c/fd/0; done`.
A mismatch there is the whole bug class. Temporary `RUNNIR_SCALE_DEBUG` eprintln in
`resize`/`set_scale` (surface, cell, derived cols/rows) pinpointed it; removed after.

**Shift+Enter.** The kitty keyboard protocol was already fully implemented (`CSI ? u`
query answered, Shift+Enter → `CSI 13;2u`). The problem is Claude Code **never sends
the query**: its binary contains the reply parser `/^\x1b\[\?(\d+)u$/` but the string
`[?u` appears nowhere, and `TERM` is `xterm-256color` with no `TERM_PROGRAM`. So it
stays in legacy mode, where Enter and Shift+Enter were both a bare `\r`. Fix in
`keys.rs::named_key`: legacy Shift+Enter now sends `\x1b\r` (ESC-CR) — exactly the
sequence Claude Code's own `/terminal-setup` installs for Alacritty and VS Code, so
apps already understand it; the rest read it as Alt+Enter, which is harmless.

## 2026-07-20 — Leader: status-bar indicator, and a default that actually reaches us

Two changes to the leader layer, both from Pedro trying to use it.

**The armed state is now a status-bar chip, not a toast.** `leader_armed` was
signalled with `toast("leader…")`, which put it in the floating overlay in the
middle of the screen — the wrong place for "runnir is holding your next
keystroke". `build_status` now reads `leader_armed` directly and draws a reversed
accent ` LEADER ` chip at the far left of the bottom bar, pushing cwd/branch
right. The toast survives only as the fallback when `status_bar` is off — an
invisible modal layer is how you eat a keystroke and leave the user wondering.
Expiry: `about_to_wait` clears `leader_armed` past the deadline and folds it into
`extra_wake` alongside the image-watch and media timers. Do NOT early-return there
like the AI-spinner arm does; that stalls the scroll glide and bell flash for up
to 3 s.

**Default leader is `alt+shift+space`, was `alt+space`.** The old comment claimed
alt+space was "not universally free"; the honest version is that it is taken by
default on three of the four big desktops, so the layer simply never armed
anywhere except the Hyprland box it was written on:

| Desktop | owns bare `alt+space` |
| --- | --- |
| Windows | window system menu (since 3.x); PowerToys Run; clashes with the Win11 Copilot bind |
| KDE | krunner (with `alt+F2`) |
| GNOME/GTK | window menu (moved GTK → gnome-shell in 3.14, still bound) |
| macOS | `option+space` types U+00A0 — we intercept it first, but muscle memory says NBSP |
| Hyprland / sway / i3 | free (their defaults live on super) |

Adding shift dodges all of them, since every one of those is unshifted, and
readline owns no alt+shift chord. Residual risk accepted: Windows and X11's
`grp:alt_shift_toggle` switch keyboard layout on alt+shift — both fire on
*release with no other key*, so the space spares us, and a multi-layout user has
`leader` in the config.

Do not "improve" this to `ctrl+alt+space`: ctrl+alt IS AltGr on the Spanish and
most EU layouts, and AltGr+space types a non-breaking space there. That is a
worse bug than the one being fixed.

## 2026-07-20 — The leader layer becomes two levels, with a which-key panel

Pedro asked where Claude was on the leader layer, found it was not (only 17 of
~40 actions were), and said all of them should be. Right instinct, but a flat
layer cannot hold them: 26 letters, and `1..9` already belong to the tabs.

So `leader_bindings` is now a tree — `LeaderNode::Run(Action) | Group { title,
keys }` — walked by `Keymap::resolve_leader(&[Chord])`. `Gpu` tracks how deep it
is in `leader_path`; a group re-arms the timeout (the panel is up, the user is
reading) and anything else disarms. Escape backs out.

Layout: direct keys for what you do constantly (`1..9`, `hjkl` focus, `HJKL` and
arrows resize, `v`, `g`, font `+ - 0`), then groups whose letter is the noun —
`t` tabs, `p` panes, `c` clipboard, `f` find/scroll, `a` ai, `r` run/launch,
`o` open, `s` session. Launch is `r`, not `l`, because `l` is focus-right and the
vim row outranks a nicer mnemonic. Every ctrl+shift chord survives as an alias;
the layer is a superset, and a test enforces exactly that
(`every_action_with_a_normal_binding_is_reachable_from_the_leader`) — it caught
two real holes while being written, `scroll_page_up` and the `l` collision.

The which-key panel is chrome, NOT an `Overlay`. An Overlay captures the
keyboard, and the entire point is that the next key reaches the leader resolver.
It reads `Gpu.leader_entries`, a snapshot taken in the key handler, because the
keymap lives in `App` and the draw path only sees `Gpu`.

## 2026-07-20 — `ctrl+plus` never worked (and neither did `leader +`)

Pedro: "el = sigo sin poder hacerlo con una unica tecla y el + y el - tambien".
Sounded like a layout ergonomics complaint; it was a dead binding.

`Chord::parse("plus")` went through `canonical_named` and produced
`ChordKey::Named("plus")`. But `Chord::from_event` turns a real keypress into
`ChordKey::Char('+')`, because winit delivers `Key::Character("+")` — `named_id`
has no NamedKey for punctuation and never could. Named != Char, so the chord
never matched anything. `ctrl+plus`, `ctrl+minus`, `ctrl+equal` and the leader
aliases have all been inert since they were written; the font zoom that "worked"
was `ctrl+shift+plus`-free coincidence — nobody had tested the bare chords.

Fix: `parse` maps `plus`/`minus`/`equal` (+ `dash`, `equals`) straight to
`ChordKey::Char`, and the entries come OUT of `canonical_named`. The names exist
only because `+` is the chord separator, so `"ctrl++"` cannot be written.

Test `spelled_out_punctuation_matches_the_key_you_actually_press` compares a
parsed spec against a synthesised keypress — the assertion that was missing.

## 2026-07-20 — The 1.8s cold start was a sleeping discrete GPU

Pedro: "runnir tarda mucho en abrir el primero, luego va más rápido, pero la
primera vez tras X segundos o minutos sin abrir runnir abre más lento".

The `[boot]` marks in `init_gpu` pinned it immediately: `Instance::new` took
1.84s and everything after it ~100ms. The cause is hybrid graphics. This laptop
has an Intel Iris Xe plus an RTX 4060 whose PCI device sits at
`power/control=auto`, so the kernel runtime-suspends it after an idle stretch.
Bringing it back up costs ~1.8s of blocking wait — exactly the "first launch
after a while" pattern, and exactly why the second launch is instant (the dGPU
is still `active` from the first one).

Two separate things woke it, and fixing only one changed nothing:
- the GL backend, which wgpu also initialises when `backends` is `all`. EGL via
  GLVND loads the NVIDIA driver. `VK_LOADER_DRIVERS_DISABLE` does not touch EGL.
- the Vulkan loader, which enumerates every ICD in `/usr/share/vulkan/icd.d/`
  including `nvidia_icd.json`, regardless of the `LowPower` preference we pass
  to `request_adapter` — the preference only picks among adapters already
  enumerated, far too late to matter.

Fix in `init_gpu`: ask for the native backend only (VULKAN on Linux, METAL on
macOS, DX12 on Windows) AND set `VK_LOADER_DRIVERS_DISABLE=nvidia_icd.json`
before `vkCreateInstance`. The env var is only set on Linux and only when the
user has not set `VK_LOADER_DRIVERS_DISABLE`/`_SELECT` themselves. Fallback
cascade if no adapter turns up: native without the discrete ICD → native with
it → `Backends::all()`. NVIDIA-only machines still boot.

Measured with the dGPU confirmed `suspended` beforehand
(`/sys/bus/pci/devices/0000:01:00.0/power/runtime_status`):
1.94s → 73ms to `renderer_new`, and the dGPU stays `suspended` afterwards, so
runnir no longer costs battery either. Warm case ~100ms → ~73ms.

Ruled out along the way: `fontdb::load_system_fonts()` is 10ms for 976 faces
because fontdb 0.23 memory-maps rather than reads (676MB of fonts on this box
are never touched); the 27MB binary and page-cache eviction are not the issue.

## 2026-07-21 — Drag-and-drop of files, spoken to Wayland directly

Dropping a file on runnir did nothing on Hyprland: **winit 0.30's Wayland backend
implements no drag-and-drop at all**. `WindowEvent::DroppedFile` exists and is raised
on X11, macOS and Windows only — which is every platform except the one runnir
actually runs on.

`src/dnd.rs` (new) speaks `wl_data_device` itself. It attaches to the connection winit
already opened (`Backend::from_foreign_display` on the raw display handle from
`RawDisplayHandle::Wayland`) and opens its OWN event queue on it. That is legal and
cheap: a Wayland queue only receives events for proxies created from it, so our
registry, seat and data device are entirely ours and winit never sees them. Binding a
second `wl_seat` does not disturb the first — seats are compositor state, not a
client-side claim. It runs on its own thread blocking in `blocking_dispatch`, and
reports drops through the same `EventLoopProxy` the PTY and AI workers use
(`UserEvent::FilesDropped`). `start_wayland_dnd` is a no-op on an X11 display, or the
path would be typed twice.

MIME is `text/uri-list` (RFC 2483: one URI per line, CRLF, `#` comments). The drop
event carries no position, so the last `motion` is stashed in an `AtomicU64` with both
f32s packed into one word — a two-field version could hand the reader an x from one
motion and a y from the next.

`on_files_dropped` focuses the pane under the drop (Wayland gives surface-LOGICAL
coordinates; multiply by `gpu.scale` before the hit test) and TYPES the paths — no
newline, ever. Each is wrapped by `shell_quote` in single quotes, where every byte is
literal; a `'` closes, escapes and reopens. A file named `; rm -rf ~` has to arrive as
a filename, not a command. Tested for spaces, globs, `$`, apostrophes and a newline in
the name. winit's own `DroppedFile` (X11/macOS/Windows) is one event per file and
carries no coordinates, so it passes `None` and lands in the focused pane.

## 2026-07-21 — Synthwave Electric (Pedro's kitty palette) bundled

Pedro: "me gusta el tema de kitty que tengo en este equipo y no lo veo en runnir".
It is not a kitty theme — **DankMaterialShell generates it**. `dms` writes
`~/.config/kitty/dank-theme.conf` (+ `dank-tabs.conf`) from
`~/.config/DankMaterialShell/themes/synthwaveElectric/theme.json`, so the file is
regenerated whenever the dms theme changes. Transcribed as a static builtin anyway
(Pedro's call): bg `#0a0a15`, fg `#e6f0ff`, selection `#0080ff`, accent `#ff6600`
(kitty's `url_color`/`color6`), dim `#a5968c` (`color8`). `color0` equals the
background in the source and is kept that way. themes.rs only, one row of hex.

Consequence to remember: it is a SNAPSHOT. Switch dms to another theme and runnir
does not follow. The general fix, if it ever comes up again, is a `theme_file`
config pointing at a kitty `.conf` plus a parser for `color0..15`/`foreground`/
`background`/`cursor`/`selection_*` — that would also unlock all ~300 kitty-themes.

Not visually verified: `render::offscreen` (and `--demo`) hardcode
`Theme::default()`, so no headless mode can render a builtin. Only the picker's live
preview shows it.

## 2026-07-21 — 45 more themes, and where a palette can honestly come from

Pedro: "tenemos pocos". 29 builtins → **74**. Chosen against what people actually
run (dotfyle's install counts for 2026: tokyonight, catppuccin, kanagawa, rose-pine,
nightfox, onedark, gruvbox-material, github, everforest; plus the light half, which
was thin — 8 of the old 29).

Palettes are NOT typed from memory. They come from `mbadolato/iTerm2-Color-Schemes`,
which keeps a kitty `.conf` per scheme generated from each project's published
colours; `curl` + a 40-line generator emitted the Rust rows, so a hex digit cannot
drift on the way in.

Two fields cannot come from that source and were filled per theme by hand:
- **selection.** Those ports set `selection_background` to the FOREGROUND, because
  kitty renders a selection as reverse video. runnir's `Theme.selection` is a real
  background, so copying it would paint a bright block over the text. Each theme's
  own published selection/visual colour is used instead.
- **accent.** A terminal palette has no notion of "the colour this project puts on
  its links and highlights", which is what runnir draws its tab bar, palette and
  panels with. Picked per theme (Catppuccin → mauve, Poimandres → mint, Srcery →
  amber, and so on).
`colour8` is NOT a safe stand-in for either: in Poimandres it equals the foreground,
in Melange Light it is nearly the background.

The picker needed no change — it already scrolls (`scroll = cursor - (visible - 1)`,
12 rows) and fuzzy-filters, so 74 entries behave like 29. Verified by driving a real
instance: leader `o t` opened it, typing `oxo` narrowed to Oxocarbon with its swatch
strip. That run used a scratch `XDG_DATA_HOME` as well as `XDG_CONFIG_HOME` — see the
session-file gotcha below.

The test now also asserts selection/accent/dim differ from the background. A
transcription slip that produces an invisible selection or a background-coloured tab
bar is exactly the failure this class of change invites.

Docs site was lying about this feature: `theme-picker` was `status: 'dev'`, claimed
"20-30 temas" and told people colours are only set in the config. Corrected.

## 2026-07-21 — Git in the terminal, step 1: the guardian learns what git destroys

Pedro wants git worked into runnir itself, in four steps: guardian rules, repo state
in the status bar, git-aware hints, then a native panel with the full operation set
(log, diff of an old commit, stage/commit/push). This is step 1.

The guardian knew exactly one git hazard — force-push, the loud famous one. The ones
people actually lose an afternoon to are quiet, and share a property worth naming:
**they destroy work that is in no commit**, which is the only thing git cannot hand
back. `git_destroys_work` covers reset --hard/--merge/--keep, clean with -f, a path
checkout (`--` or `.`), restore that touches the worktree, stash clear/drop, branch
-D, push --delete/--mirror, and gc --prune=now / reflog expire — the last two because
the reflog is what makes every other mistake on the list survivable.

Deliberately NOT flagged, each with a test: `clean -n` (dry run), `checkout main` (a
switch git refuses when it would lose edits), `restore --staged` (unstages only),
`reset --soft`, `branch -d` (already refuses an unmerged branch), `stash pop`. The
guardian is a confirmation, not a wall, and a rule that fires on daily commands is
one the user turns off.

One trap: `danger` lowercases the line, and `-D` versus `-d` is the ENTIRE difference
between force-deleting a branch and being refused. The branch rule reads a
case-preserving copy of the tail; a test asserts both halves.

## 2026-07-21 — Git step 2 (T1): the status bar knows the repo, without polling

`src/git.rs` (new). The bar was showing a branch and nothing else; it now shows
` main ↓1 ↑2 +2 ●4 !1` — behind, ahead, staged, dirty, conflicts — and only the
parts that are non-zero, so a clean repo still reads as just its branch.

**Two sources, deliberately.** `head_branch` reads `.git/HEAD` (one file read, safe
from the draw path, correct the instant a checkout finishes). The counts come from
`read_state`, which shells out and can take seconds in a big repo, so it runs on a
worker and answers via `UserEvent::Git(root, Option<RepoState>)`. The branch is
never taken from the slow path — a stale count is a cosmetic lag, a stale branch is
a lie.

**`--no-optional-locks` is load-bearing.** A plain `git status` refreshes the index
and takes `index.lock`. A status poll doing that in the background can make the
user's OWN git command in that pane fail with "another git process seems to be
running". The flag plus `GIT_OPTIONAL_LOCKS=0` (for any git it invokes itself) is
the whole reason this is safe to run behind the user's back.

**The refresh trigger is the OSC 133 command counter, not a timer.** `refresh_git`
runs on the periodic wake but only spawns when `pane.command_seq()` moved past what
that repo root was last seen at. Nothing but a command can change a repository, and
`cd` is a command, so entering a repo triggers it too. An idle terminal sitting in a
repository spawns no git at all — the trail-at-60Hz lesson, applied before it bit.

Cache is keyed by REPO ROOT, not pane: two panes in one repo share one entry and one
process. `git_pending` holds one in-flight per root, so a repository slow enough to
still be running cannot accumulate a process per wake.

Parsing is `--porcelain=v2 --branch`, split into a pure `parse_porcelain_v2` with 7
tests, so the format is covered without needing a repository in the test run. Note
`XY`: X is staged, Y is worktree, `.` is unchanged — an `MM` file counts as BOTH,
which is what it is. Conflicts (`u`) are counted apart from dirt: they need
resolving, not committing.

## 2026-07-21 — Git step 3 (T2): hints become git objects, and stop hiding the screen

Hint mode already found hashes and only knew how to copy them. Now:

- **Branches are recognised by NAME, never by shape.** `hints::Context` carries the
  repo's local branches, snapshotted from the status worker's cache (`git.rs`
  gained `local_branches`, which reads `refs/heads` + `packed-refs` — files, no
  subprocess). `main`, `dev` and `wip` are ordinary English words; guessing them
  would put a label on half the prose on screen. A token is a branch only if the
  repo has one by exactly that name, and the branch reading wins over the hex one
  for a token like `deadbeef` that could be either.
- **Repo-relative paths** (`src/main.rs`, `src/main.rs:412:7`) are hints now, which
  is how git and every compiler name a file. No filesystem check: `find` runs on
  every mouse move for the hover underline, and a stat storm per motion is not worth
  it. The shape carries it — separator, real extension, ordinary first character —
  and a test pins the near-misses that must NOT match (`21/07/2026`, `+2/-1`,
  `he/him`).
- **UPPER CASE label = the alternate action.** Lower case is the old behaviour
  (copy, or open a URL); shifted is "show me this": `git show` for a hash, `git log
  --graph` for a branch, `$EDITOR +line` for a path. A test asserts every command
  reachable this way is read-only — a hint is one keystroke on a target picked by
  sight, so a mistyped label must never be able to move a branch.

**Bug found while verifying, older than this work: hint mode blanked the pane.**
`build_hints` fills a pane-sized grid with `Color::Default` and draws labels into
it, commented "transparent-ish". It never was: the renderer emits an instance for
every non-spacer cell, and a blank cell with a default background paints the pane
background over whatever is beneath. So the labels covered the very output they
point at. `PaneDraw.transparent` (new) makes the instance loop skip blank
default-background cells, and only the hint layer sets it. Screenshot before/after
is the only way this shows up — no test renders.

## 2026-07-21 - Git step 4 (T3): a native git panel

Pedro's scope change: not lazygit in a split, the operation set inside runnir. Full
panel in one go, keys acting immediately with no confirmation (his call, asked
explicitly).

`Overlay::Git(GitPanel)` (overlay.rs) with four lists - status, log, branches,
stashes - and the selection's diff beside them. `git.rs` grew the data layer:
`log` (a `%h US %s US ...` format, unit-separated because a subject can contain any
printable character), `status_files`, `show`, `diff_file`, `stashes`,
`branch_log`, and `run` for the mutating half. Parsers are pure and tested:
`parse_log`, `parse_status_files`, `parse_diff`.

**Nothing that can lose uncommitted work is bound.** That is the direct consequence
of "no confirmation": no reset --hard, no clean, no discard-changes, no stash drop,
no branch -D. Everything the panel does, git can undo - stage, unstage, commit,
fetch, pull --ff-only, push, switch branch, stash push, stash pop, new branch. The
destructive set stays at the prompt, where the guardian already asks.

**Every call is on a worker.** `UserEvent::GitPanel(seq, PanelMsg)` carries the
answers back. `seq` exists because a fast j/k run would otherwise let an older
`git show` paint over a newer one - lists always apply, the preview only when the
sequence still matches. `busy` blocks a second command, so a repeated P cannot fire
two pushes.

**Diffs are drawn, not echoed.** Pedro compared the raw unified diff against a review
tool's rendering: a `+`/`-` column shifts every changed line one column away from
its context, and there is no line number, so you count rows to find out what
changed. `parse_diff` turns the diff into numbered rows (numbers walked forward from
the `@@` header - new file's number for added and context lines, old file's for
removed) and the panel tints the whole row instead of prefixing it.

**Space is `NamedKey::Space`, not `Character(" ")`.** Staging silently did nothing
until that was fixed; found by pressing it against a real repository and asking git,
not by reading the code.

The leader root `g` now opens the panel. It was FixLastCommand, which also lives at
`leader a g`, so nothing lost a binding and git gets the letter everyone reaches for.

## 2026-07-21 - Worktrees had no branch at all, and the bar hid an unfinished rebase

Two blind spots, both found by asking "what is still missing" rather than by a test.

**In a worktree (and a submodule) `.git` is a FILE**, holding `gitdir: <path>`.
`head_branch` read `<root>/.git/HEAD`, which cannot work there, so every worktree
showed no branch in the status bar, no branches in hint mode, and no HEAD in the
panel. This repo keeps its agent worktrees under `.claude/worktrees/`, so it was
broken in the place it is used most. `git_dir()` now resolves the pointer file.

Refs need a second hop: a worktree's own git dir holds HEAD and the index, but
`refs/heads` and `packed-refs` live in the MAIN one, named by its `commondir` file.
`common_dir()` follows that, or a worktree lists zero branches while sitting on one.

**An unfinished operation is now the first thing the bar says**: REBASE, MERGE,
CHERRY-PICK, REVERT, BISECT, from the marker files in the git dir. It leads the
line because it changes what the rest of it means - "2 ahead" mid-rebase is not the
same fact as 2 ahead on a finished branch.

## 2026-07-21 - Git: credentials in a real pane, a deadline, and hunk staging

**A background git cannot be asked for a password.** `run` sets
`GIT_TERMINAL_PROMPT=0` AND `GIT_SSH_COMMAND=ssh -o BatchMode=yes`, so a push that
needs a passphrase, a username or an unknown host key fails IMMEDIATELY instead of
blocking on a /dev/tty that is not there. `needs_terminal` recognises those
failures by their message and the panel reruns the same argv in a split, where ssh
and git ask the way they always do. A plain rejection (non-fast-forward) is
deliberately not in that set: nothing can be typed to fix it, and a split per failed
push is noise.

**Every command has a 60s deadline.** The child is spawned rather than `output()`d,
a thread waits on it, and `recv_timeout` decides: past the deadline it is SIGKILLed
and the panel says so. Without it a hung remote pinned the panel in `busy` for the
rest of the session, with no way back short of closing it.

**Hunk staging.** `hunk_ranges` splits the parsed diff at each `@@`, `patch_for_hunk`
rebuilds a one-hunk patch — putting the `+`/`-` column back from the row kind, and
carrying the file header, without which git has nothing to apply against — and
`apply_patch` pipes it to `git apply --cached`. `--cached` is the whole safety
argument: the index moves, the working tree does not, so a mistaken hunk stage
cannot lose an edit. `]`/`[` move the selection (a yellow bar marks its rows, drawn
only when there is more than one hunk), `s` stages it, `u` unstages it.

Verified against the real repo: staged one hunk of `src/git.rs` and `git diff
--cached --stat` showed 38 insertions while 196 stayed unstaged in the same file.

## 2026-07-21 - Git: the rest of the operation set

Seven views now (1-7 or Tab): status, log, branches, stashes, tags, reflog,
worktrees. What each one added:

- **branches** also lists remote-tracking refs, dimmed, after the local ones -
  splitting them into two views would mean switching views to answer "is my branch
  on the remote". Enter on a remote uses `switch --track`, since a plain checkout of
  one lands you on a detached HEAD. `m` merges into HEAD, `R` rebases onto it.
- **tags**: `--sort=-creatordate`, because alphabetical puts v10 before v9. `n`
  creates, `P` pushes them.
- **reflog**: the undo history for everything the panel refuses to bind. Enter
  checks a position out. Showing it is worth more than binding the operations that
  make you need it.
- **worktrees**: Enter opens one in a NEW TAB with the shell already there - the
  thing a terminal can do that a git client cannot. This repo keeps agent branches
  in worktrees, so it is the view that earns its place here.
- **status** gained `t` (a file can be staged AND modified - two different diffs of
  one path), `A` amend, `C` (hands the whole commit to a pane so $EDITOR opens for a
  message with a body), `e` open the file, `L` its history, `b` blame, and `O`/`T`
  for ours/theirs - guarded on the file actually being unmerged.
- **log** gained `/` (a `--grep` filter, shown in the header so a narrowed list is
  never mistaken for the whole history), `c` cherry-pick, Enter checkout.

`P` now goes through `push_args`, which adds `-u origin HEAD` exactly when the
branch has no upstream. Without it the first push of a new branch fails with "has
no upstream branch", which is a thing to know rather than a thing to be told.

**The bar refreshes on changes made elsewhere.** The OSC 133 counter only sees what
ran in that pane; an editor writing a file, a git in another pane, a rebase in a
second window were all invisible until something ran. `state_stamp` xors the mtimes
of `index` and `HEAD` - two stats per tick - and a change in either triggers the
same refresh. Walking the working tree would not be worth it; those two files cover
staging, commits, switches and every step of a rebase.

Verification note, learned the hard way: pick the scratch instance by
`/proc/<pid>/exe`, never by excluding known pids. A stale exclusion list sent
keystrokes into PEDRO'S terminal and floated his window onto another monitor.
`scratchpad/shot.py` now resolves the window by binary path.

## 2026-07-21 - Git: submodules, and a per-tab dirty marker

Submodules are listed beside the worktrees, not in a view of their own: they answer
the same question ("what other checkouts hang off this repo") and both answers are
a directory you may want a shell in. `worktree_path` handles both row shapes.

**A tab in a dirty repository carries a marker.** The badge ranks BELOW the failed
command and unseen-output ones on purpose - those are events, this is a standing
condition.

Getting there needed the refresh to stop being about the focused pane. `refresh_git`
now walks every tab's focused pane, keeps `pane_repo: pane id -> repo root` so the
draw path can ask "is this tab dirty" with no filesystem access at all, and spawns
**at most one git per wake**, active tab's repository first. Eight tabs in eight
repositories must not answer a keystroke with eight processes.

## 2026-07-21 - Git: the mouse, and reading a commit one file at a time

Pedro: no mouse in the panel, and a commit could only be read as one long diff.

**Drill-down.** Enter on a commit (log or reflog) makes the list that commit's
FILES - `git show --name-status` - and selecting one previews just that file's diff
inside the commit (`git show --format= --patch <sha> -- <path>`; `--format=` drops
the message, which is already on screen and would otherwise push the diff off the
top). Escape backs OUT of the commit before it will close the panel. Checking a
commit out moved to `x`: reading a commit is what you do constantly, moving HEAD
onto one is not, and Enter should be the common one.

**Mouse.** The renderer and the hit test now share `GitPanel::layout(cols, rows)`,
so a click cannot land somewhere other than what it looks like it hit - the bug that
would otherwise appear the first time the list width formula changed in one place.
`hit()` returns View / Row / PreviewLine / Header; a click on the already-selected
row does what Enter does (file-manager behaviour), a click on a diff row selects
that HUNK (so `s` then stages exactly what you pointed at), the wheel moves the
selection over the list and scrolls the diff over the diff, and a click outside
closes the panel.

No click injector exists on this box (no ydotool/wtype), so the mouse is covered by
unit tests instead: `the_git_panel_hit_test_agrees_with_what_it_draws` walks the
view labels at their drawn positions, checks row mapping including the scrolled
case, and `a_click_on_a_diff_row_finds_its_hunk` pins hunk lookup. The drill-down
itself was verified on screen.

## 2026-07-21 - A prompt that hid what you were typing

Pedro, with a screenshot: type a long answer into the AI prompt and the text runs
off the right edge - you cannot see the words you are writing.

The box was a fixed `(cols * 6 / 10).clamp(30, 70)` and `write` clips at the grid
edge, so everything past ~66 characters was drawn into nothing, caret included.

Both halves of the fix, because either alone is wrong:
- The box now GROWS with the input, up to `cols - 4`. A fixed box hides a long
  answer; a box that only grows would become a modal wider than the screen.
- Past that, the text SCROLLS. `field_view` keeps the END of the string, because
  the caret in these fields is always at the end (they take typing and backspace,
  never arrow keys) - a field that clips the tail hides the character you just
  typed, which is the one thing a text input may never do. A leading `…` marks the
  cut.

The search bar got the same treatment; its query could outgrow its 60-cell bar the
same way. Tests pin `field_view` and the grow-then-scroll behaviour by rendering a
400-character input and reading the row back out of the grid.

## 2026-07-21 - Git: graph, blame view, staging by line, interactive rebase

The four Pedro asked for, plus the answer to "I cannot see a commit's files": that
was already there (Enter on a commit) and is now also visible as `--stat` at the top
of every commit preview.

**Graph.** `log --graph` and `parse_log` keeps the art. Art-only rows (`|\`, `|/`)
arrive with no sha and are kept as dimmed, unselectable rows: dropping them would
leave a graph with holes.

**Blame is a view, not a pager.** `parse_blame` turns `git blame` into rows (sha,
author, date, line, text); the preview is the commit behind the selected line, and
Enter drills into that commit's files. Two traps: `git blame --no-color` is
AMBIGUOUS in this git (against `--no-color-lines`), so it uses `-c color.ui=false`;
and an author name has spaces, so the fields are parsed from the RIGHT.

**Staging by line.** `l` moves the keyboard into the diff, `v` anchors a selection,
`s`/`u` act on exactly those lines. `patch_for_lines` does what git's own edit mode
expects: an unpicked `+` is dropped, an unpicked `-` becomes context. Two things
made it work: the cursor lands on the first CHANGED line (starting on context means
the first keypress does nothing and reads as broken), and `git apply` needs
**`--recount`** - the `@@` counts come from the original hunk and no longer match
after dropping lines. Without it: `error: corrupt patch at <stdin>:14`.

**Interactive rebase, planned in the panel.** `i` on a commit builds a plan of
everything above it; p/r/e/s/f/d set the action, K/J reorder, Enter runs. The trick
that avoids an editor: git invokes `$GIT_SEQUENCE_EDITOR <todo path>`, so setting it
to `cp <our file>` makes the copy the edit. `GIT_EDITOR=true` keeps reword/squash
from blocking on a terminal. The plan is newest-first (as the log shows it) and
reversed when written, since git replays oldest-first.

## 2026-07-21 - The window closed on running work without asking

Pedro closed the window by reflex with Claude working in it. Nothing asked, nothing
came back. The cause was not a missing feature: `behaviour.confirm_close` had been
in `config.rs` (default `true`) and in the settings panel since the beginning, and
**no code ever read it**. A dead setting is worse than no setting — it says the
window is guarded when it is not.

Four paths exited the app and all four now go through `Gpu::request_close`:
`WindowEvent::CloseRequested`, `Action::Quit`, closing the LAST tab, and closing the
last pane. The palette's Quit too (it exits the process directly, having no event
loop).

What counts as "running" is the foreground process of each pane with shells filtered
out (`is_shell`, dash-stripped for login shells like `-fish`): a pane idling at its
prompt reports the shell itself, and a confirm that fires on an idle window is one
people learn to dismiss without reading. Nothing running still closes instantly.

The prompt is a `PromptKind::ConfirmQuit`, which `PromptKind::is_confirm` marks as a
question rather than a field: no input line, no selected row, and the running command
lines listed under it dimmed. **Enter is not a yes.** This prompt exists because a
reflex keystroke killed work, and Enter is the reflex — only `y` closes, `n`/Esc/`q`
stay. Verified on a real instance on DP-6: `SHELL=/usr/bin/cat` (a non-shell
foreground) + `hyprctl dispatch closewindow` leaves the window up with the prompt;
with a normal shell idle, the same dispatch closes it at once.

## 2026-07-21 - Git panel: a column per level, dragged, zoomed, and a leader of its own

Pedro, on a commit with five files: "los veo todos uno encima de otro, me falta un
subnivel más de fichero modificado". The drill-down existed but REPLACED the list,
so the hierarchy was never on screen at once.

**Three columns.** `open_commit` no longer swaps what the list shows: the commit's
files get a column of their own between the list and the diff. `len()`/`cursor()` are
the list's again and `files_len()`/`files_cursor()` are the new column's — one pair of
accessors that meant two different lists was what made the old code fight itself.
`GitFocus{List,Files,Diff}` says which column j/k drives, and the selected row of an
unfocused column is drawn dimmed (`inactive()`), because it is still the selection
that decides what the columns to its right show. Moving the LIST closes the file
column: those files belonged to the row you just left.

**Columns drag.** `split: [f32; 2]` holds the separators as fractions; `layout`
clamps them (MIN_COL 12, MIN_DIFF 20) so a window resize can never leave a column at
zero, and `drag_split` only stores. A press on a separator sets `Gpu::git_drag` and
motion drags it — the same shape as the pane dividers, including that the motion
handler has to run BEFORE the `overlay.is_some()` early return. The pointer turns
into `ColResize` over one; without that nobody discovers the drag.

**Zoom.** `z`, or Enter on a file, gives the diff the whole box; the header then
carries the path, since the columns that said which file it was are gone. It goes
through `enter_diff`, so the line cursor lands on the first changed line rather than
on the `diff --git` header.

**A leader layer inside the panel.** Same chord as the global one, same which-key
grid, but a tree of git verbs (`GIT_LEADER`). Two things keep it honest: every leaf
PRESSES a key the panel already has (`GitPress::Key/Then/In` → `git_panel_key`), so a
verb cannot behave differently from its letter, and `In(view, key)` entries are hidden
unless that view is up, so the menu never offers something that would do nothing. A
test walks the tree and asserts every leaf's key is one the panel binds.

The which-key had to be drawn as a PANEL of the overlay, not as screen chrome like
the global leader's: chrome is drawn under the overlay's dimmed backdrop.

Verified with a new headless scene, `runnir --demo out.png git:commit|zoom|leader[keys]`,
which renders the real `GitPanel` over the real repository through the same
`overlay.render` the app uses — a three-column layout cannot be checked from a unit
test, and this cannot drift from what the app draws.

## 2026-07-21 - File explorer: the sidebar and its tree (step 1 of the design below)

The first two build steps of the design entry below, minus the viewer: the sidebar
itself, the tree, focus, the mouse and a leader layer of its own. The design section
stays until the whole thing is built; this is what exists now.

**It reserves columns, and that is the whole trick.** `Gpu::active_area()` returns
the window minus the sidebar; `window_area()` is the undivided one, for the sidebar
itself. Everything downstream — `Tab::layout`, `reflow`, the pane hit tests, the
divider drags — asks `active_area()` and never learns a sidebar exists. No `Overlay`
(it would capture the keyboard and dim the pane), no `Pane` (it owns a non-optional
PTY).

**Every read is on a worker**, keyed by the tree's `seq` and by the tab index, and
the answer is dropped if either moved on. `read_dir` of `node_modules` or of an NFS
mount is not something the frame can wait for.

**The cursor follows the PATH across a rebuild**, not the index: a directory
finishing its read inserts rows above the selection, and on the slow filesystems
where the reads are slow, a selection that jumps every time one lands is unusable.

**Its own leader tree** (`FILE_LEADER`), same contract as the git panel's: every leaf
presses a key the sidebar already binds, and `OnFile`/`OnDir` leaves are hidden when
the row under the cursor is the other kind. It draws along the bottom of the window
rather than inside the sidebar — 30 columns of which-key is one entry per line.

Two bugs the remote control caught immediately, both invisible to the tests:
- `press_key` (the scripted path) skipped the sidebar entirely, so a scripted `j`
  did nothing while the same key worked by hand. The scripted path has to mirror
  `on_key`'s ORDER, not just its handlers.
- `toggle()` marked the directory as loading and `explorer_read_pending()` then
  skipped it as already in flight: a directory opened and never loaded. Reporting
  "this needs a read" and marking it in flight are two jobs, and the one that spawns
  the thread owns the second.

Still to come, in the design's order: the viewer, `$EDITOR`, properties and
operations, git badges and mtime sort, real images.

## 2026-07-21 - File explorer: the viewer, $EDITOR, and what opening a file means

Steps 2 and 3 of the design below. `Enter` on a file now does something, and what it
does comes from what the file IS.

**The type sniff is content, not name.** `kind_of` reads the first 8 KB: image by
magic bytes (extension only as a fallback for headers it does not know), then binary
if there is a NUL, else text. A log with no extension is text; a `.dat` may be too; a
PNG called `.txt` is still a PNG. Tests pin all four.

**The viewer is read-only and says so.** Text with line numbers, tabs expanded to
4-column stops (the grid has no tab stops, so a raw `\t` eats every indented file's
structure), horizontal scroll, and a limit of 4 MB with a line that SAYS it stopped.
It reads at most the limit rather than checking the size first: `/proc` files report
zero bytes and still have content worth reading. Images render as half-block art.

**The image aspect bug worth remembering**: half-block art packs two vertical pixels
per cell, so a cell is square in IMAGE terms — but not on screen, where a cell is
10x22 px. Fitting by image aspect alone drew a square logo twice as tall as it was
wide. The fit needs `cw/ch` passed in from where the cell size is known.

**Nothing that runs is run by a keypress.** An executable text file raises a chooser
(view / edit / run / open with the system) because a script is legitimately all of
those; an executable binary or a `.desktop` file gets a y/n confirm naming what would
launch, since `xdg-open` on those executes a handler and a cloned repo can carry one.
`$EDITOR` and a chosen `run` go to the focused pane when it is at its prompt and to a
split when it is busy — the same foreground-minus-shells predicate `confirm_close`
uses — with the path shell-quoted.

`xdg-open` is spawned detached with its output to `/dev/null` and reaped on a thread:
nothing else waits on it, and an unreaped handler is a zombie for the life of the
terminal.

One more remote-control fidelity bug, found the same way as the last two:
`chord_to_key` built `"g"` + SHIFT for `shift+g`, but a keyboard sends `"G"` and the
handlers match on the character. Every shifted letter quietly did the unshifted
thing. A scripted key has to be the key a hand produces, not the one the chord
grammar names.

## 2026-07-21 - File explorer: properties, permissions and the operations

Step 4 of the design below: `p` properties, `a` create, `r` rename, `d` delete. The
refresh moved off `r` to `R` — the design's key list is what a file manager's is, and
the destructive-looking letter belongs to the safe verb, not the other way round.

**Permissions are a 3x3 grid**, because that is what they are: owner/group/other by
read/write/execute. You move around it, space flips a bit, and NOTHING is written
until Enter. A directory can mark the change recursive, which then confirms with the
count of what it would touch.

**Every refusal is a refusal to lose work**:
- `rename` will not overwrite an existing name, and `check_name` rejects anything
  with a separator or `..` — a rename box must not be able to move a file out of the
  tree it is a view of.
- `create` uses `create_new`, so a race cannot truncate a file that appeared while
  the prompt was open.
- `delete` needs the recursive flag for a non-empty directory, and the flag is only
  set by a confirm that COUNTED first. `count_tree` and `delete` both refuse to
  follow symlinks: following one is how a count (and then a delete) walks out of the
  tree it was handed.
- `set_permissions` follows symlinks and there is no portable way not to, so the
  panel says, before anything is changed, that the change lands on the target.

The counting runs on a worker and the confirm goes up when it comes back
(`UserEvent::ExplorerConfirm`): counting `node_modules` is a tree walk, and doing it
on the UI thread is the same mistake as reading a directory there.

`count_words` exists because "1 directories" reads as generated text, and generated
text is what people stop reading — the one thing a delete confirm cannot afford.

After an operation the tree re-reads and `pending_cursor` lands the selection on what
the operation produced: after a rename you are on the new name, not on the hole where
the old one was.

## 2026-07-21 - File explorer: git badges, a date sort, and what git ignores

Step 5 of the design below, and the last of the original plan before the images. It
is where the rejected "what is the agent touching right now" view actually shipped:
as a **sort mode plus a badge per row**, not as a second view to keep in step.

**A badge is a letter at the right edge**, from `--porcelain=v2`'s XY pair via
`Badge::from_status`: `!` conflict, `?` untracked, `M` modified (yellow) or staged
(green), `A`, `D`. The unstaged letter wins over the staged one where a path has
both — the unstaged change is the one not written down anywhere yet.

**A directory never borrows a child's letter.** `M` on a folder claims the folder
itself was edited, which is not a thing git says; it gets a dot meaning "something
below here changed", and a conflict is the one state it repeats out loud. The fold
onto ancestors happens once in `set_git` (O(changes × depth)) rather than per
rebuild (O(rows × changes)) — rebuilds happen on every keypress that moves a fold.

**What git ignores is hidden by default.** A Rust checkout's tree is `target/` and
little else. Never a silent cut: the footer says how many rows are being held back
and `I` brings them back dimmed. The ignored set comes from `ls-files --others
--ignored --exclude-standard --directory`, which collapses a whole build tree to one
line — asking for every ignored PATH would list all of `target/` and walk it. Since
it answers with collapsed directories, a path is ignored when IT or an ancestor is
in the set. The filter runs before the 2000-child cap, so hiding `target/` can never
be what pushes a directory past it.

**`s` sorts by mtime**, directories mixed in rather than pinned on top: a directory's
own mtime moves when something is created or removed in it, which is exactly the
event this mode is for. Name is the tie-break, or a fresh checkout (one mtime to the
second) reorders itself between rebuilds. Both `s` and `I` are views of what is
already read and neither re-reads the filesystem — a sort that costs a `read_dir` of
the whole open tree is a sort you stop using.

**Nothing polls.** The marks are re-read on the same two triggers the status bar
uses: the repository stamp (index/HEAD moved, from anywhere) and the pane's command
counter (something ran here). Two `stat`s per wake, no process unless one moved. One
read in flight at a time, and an answer that lands after a re-root or a re-read is
dropped by `seq` — then immediately re-asked, because a dropped answer otherwise
leaves a tree with no marks until something else goes stale.

`status_files` and `ignored_paths` both run with `-c core.quotePath=false`. Without
it git escapes every non-ASCII path as `"caf\303\251.txt"`, and matching a status
line against a real filename misses every accented name in the tree.

Verified on a real instance through the remote control, which now reports the tree
rows and their badges in `ui_state` (capped at 200, and the cap is reported): six
modified files badged `M`, `src` carrying the dot, `target` hidden with `1 ignored
(I)` in the footer, and the date sort putting the file edited last on top.

## 2026-07-21 - File explorer: the viewer draws the real picture

Step 6, the last of the explorer design. The viewer showed half-block art; it now
hands the decoded pixels to the renderer and the picture is drawn as a texture over
the cells it reserves. The art stays as the fallback for an image that decodes to
art but not to pixels — and it is what `media.rs` still uses for cover art.

Three things had to change, and two of them were bugs that were already there:

**`prepare_images` only walked panes.** The overlay's panels are `PaneDraw`s too, so
they are simply chained in. While an overlay is up the PANES' images are now left
out: images are recorded after every other instance, so a pane image would draw at
full brightness on top of the panel that is dimming it, and read as part of it.

**Image serials were per grid.** `Grid.image_serial` counted from 1 inside each
grid, and the renderer caches textures by serial ALONE — two panes each showing
their first image asked for the same texture, and the second one drew the first
one's picture. The counter is now a process-wide atomic. A panel is rebuilt every
frame, so `place_image_at` takes the serial from the caller: one minted per frame
re-uploads the texture per frame, and `FileViewer` keeps the one the read gave it.

**`GridImage` had no column.** Every image drew at its pane's left edge, which is
right for `icat` at a prompt and wrong for a picture centred in a panel.

The picture is scaled to its box ON THE WORKER that decoded it, never in the frame:
a 6000x4000 photo shown 60 cells wide is a 600x600-ish texture, and `MAX_TEXTURE_PX`
caps it regardless of how big the window is. `fit_cells` is the aspect maths, split
out and tested — the box is chosen by the aspect ON SCREEN, not in cells, which is
the bug that shipped once already and drew a square logo twice as tall as wide.

`j`/`k` do nothing on a real texture and the legend stops offering them: the picture
is drawn whole, so there is nothing below the fold, and a key that moves a number
nobody can see is a key that looks broken.

Verified on a real instance: the logo drawn at texture quality, centred in the
panel, scroll pinned at 0.

## THE DESIGN the file explorer was built from (decided 2026-07-21, all six steps shipped)

Four sessions of design with Pedro, written down before any code so it is not
re-litigated from zero next time. Kept verbatim now that it is built: the six
entries above are what each step actually did, and this is what they were held to.

**What it is.** A VS Code-style file explorer: a persistent sidebar with a tree,
opening a file to view or edit, and changing its properties (rename, permissions,
delete). NOT a `cd`-the-shell navigator, and NOT a "what is Claude touching now"
activity feed — both were considered and rejected as the framing. The activity view
survives as a *sort mode* (by mtime) plus git badges on the rows, not as its own
feature.

**Decided, with the reasoning that produced it:**

- **Chrome, not an `Overlay`.** An overlay captures input by design and covers the
  pane; this sidebar has to stay up while you work in the pane beside it. Same call
  the hints layer already makes.
- **Not a `Pane` either.** `Pane` owns a non-optional `pty: Pty`; making it an enum
  to hold a non-PTY widget would touch everything. Unnecessary: `Tab::reflow(area)`
  already takes the area, so the sidebar simply reserves columns out of it and the
  whole layout tree is untouched.
- **Per tab**, state on `Tab`.
- **Root = the git root of the focused pane's cwd**, falling back to the cwd when
  that is not a repo. Re-anchored **only when the git root changes**, never on every
  `cd`: re-anchoring per `cd` collapses the tree while you navigate inside one repo.
- **No built-in editor.** In VS Code the explorer opens VS Code's editor because
  VS Code *is* the editor; runnir is a terminal, so its editor is whatever runs in a
  pane. `Enter` opens a built-in read-only viewer, `e` runs `$EDITOR <path>`. A real
  editor (undo, encodings, huge files, LSP) is a bigger project than the sidebar and
  would compete with neovim forever.
- **Where `$EDITOR` lands**: reuse the focused pane when it sits at its prompt, split
  when something is already running in it — the same foreground-process-minus-shells
  predicate `confirm_close` uses. Prefer the OSC 133 prompt state over the foreground
  process where marks are available.
- **Opening by type**: image → the inline viewer (this is what makes the panel worth
  the inline-image work); text → viewer/`$EDITOR`; anything else → `xdg-open`, with
  `o` forcing `xdg-open` for any file. Text vs binary is decided by **content** (NUL
  bytes in the first 8 KB), not by extension.
- **An executable is never opened on a single keypress; it asks.** Two cases, and they
  are not the same question:
  - *Executable text* (a `.sh`, a shebang) is legitimately three things — read it,
    edit it, run it — and the panel must let you **pick**, not guess. `Enter` raises a
    chooser: view / edit / run / xdg-open. Running goes to the focused pane when it is
    at its prompt and to a new split otherwise, the same rule as `$EDITOR`, with the
    path shell-quoted.
  - *Executable binary* (ELF) and `.desktop` files: no default action at all. A
    confirm that names exactly what would be launched, because `xdg-open` on them
    **executes a handler** and a cloned repo can carry one.
- **Left by default, `explorer.side` in config** — and the setting must be read by
  code AND shown in the settings panel. All three or none.
- **Width in columns** (default 30, clamped `[18, 40% of the window]`), not a
  fraction: a fraction on an ultrawide gives a 90-column tree. Resizable by mouse
  (divider drag, same tolerance as `Layout::divider_at`) and by `H`/`L` when the
  sidebar has focus. Reflow **on release**, with a preview line while dragging: a
  reflow per frame resizes the PTY per frame and TUIs do not survive it.
- **Keys**: `<leader>e` toggles and focuses (the letter is free, and it is LazyVim's),
  symmetric with `<leader>g` for git. Inside: `j/k` move, `h/l` fold, `Enter` view,
  `e` edit, `o` xdg-open, `p` properties, `a` create, `r` rename, `d` delete, `y` copy
  path, `s` sort, `.` hidden, `I` ignore, `Esc` back to the pane.

**Traps identified up front:**

- Directory reads go on a thread with a `seq`, like the git panel. A synchronous
  `read_dir` of `node_modules` or an NFS mount freezes the frame.
- Cap children per directory (~2000) with a visible `… N more` row. Never a silent cap.
- Do not follow symlinked directories when expanding — cycles.
- `chmod` on a symlink acts on its target. Say so in the UI.
- Deleting a non-empty directory or a recursive `chmod` confirms while **counting the
  affected files**, and Enter is not a yes.
- The path handed to `$EDITOR` through the PTY must be shell-quoted: a filename with
  a space or a `$` otherwise injects into the user's shell.
- `xdg-open` must be spawned detached with stdout/stderr to `/dev/null` and reaped,
  and it fails silently (or hangs) over SSH or with no portal — needs a timeout and a
  message. It also **executes a handler**: never open a `.desktop` file or an
  executable automatically (see the chooser above), confirm and show what would launch.
- The executable bit is not a file *type*. A script is text AND runnable, so the type
  sniff (`is_image` / `is_text` / else) and the permission check are two independent
  questions; collapsing them into one match arm loses the "edit this script" case.
- Persistence has to pick one of `session.rs` (`data_dir()`) or `project_session.rs`
  (`config_dir()`); they disagree on the directory (see Gotchas). Store only width and
  open/closed until they agree.
- Real images inside the sidebar need `prepare_images` (`render.rs`) to walk overlay
  grids too, not only panes. `Grid::place_image` already exists; half-block art
  (`media.rs`) is the tier-1 fallback that works today.

Build order: sidebar+tree+focus → viewer → `$EDITOR` → properties+ops → git badges and
mtime sort → real images. Roughly 1300 lines; the first two steps are usable alone.
(All six built on 2026-07-21: `b8c1f6c`, `f66dbe8`, `63d29c0`, `a218b8f` and the two
entries above.)

## 2026-07-21 - Bug audit of the two commits above (Fable found, Opus fixed)

A read-only agent audited `657c7e9` + `b86227d` and found 11 real bugs; a second
agent fixed them. No panics: every `clamp` in `layout` was safe because each max
argument carried its own `.max(MIN_COL)`. What it did find was state desync.

Four mattered. **Switching view by keyboard** (`1`..`7`, Tab) left the open commit
and the zoom attached to the view you left — the mouse and leader paths called
`leave_commit()` first and the keyboard did not, so the rule moved INTO `set_view`
and the three call sites lost their copy. **`PanelMsg::CommitFiles` had no
generation guard** (`Preview`, three lines below it, did): a slow commit's file list
landed under another sha and every preview after it asked that commit for a path it
does not have. **The arrows moved the list from inside the diff** (`j`/`k` were
guarded, the arrows were not), so one arrow in a zoom dropped the zoom, closed the
commit and moved the log. **Enter while zoomed re-drilled the commit** it was already
reading, ending with `zoom` set and the keyboard on an invisible column, because
`toggle_zoom` goes through `enter_diff` and leaves the focus on `Diff`, not `Files`.

The rest: a leader entry (`d v`) that pressed a key only bound with the diff focused
(new `GitPress::InDiff`, since `leader_applies` only knew about views); Ctrl+C
descending into the Commit group, because the panel's leader runs BEFORE
`overlay_key`'s modifier filter and has to repeat it; `config.leader` parsed raw in
the panel while `Keymap` falls back to the default, so a typo left a working global
layer and an unreachable panel one (now both go through `actions::leader_chord`);
three columns each under their minimum below ~52 cols (the file column is dropped
instead); the `ColResize` pointer outliving the panel; `is_shell` missing pwsh and
friends; and the close confirm destroying the overlay it was asked over — it now
stashes it and puts it back on "no".

The split is the point: one agent that may only READ and must verify each claim in
the source, then a second that fixes with the list in hand. The finder cannot talk
itself into its own fix.

## 2026-07-21 - Remote control can drive runnir itself, not just the child

Everything above was verified with headless scenes and one hand on the keyboard,
because there is no `wtype`/`ydotool` on this machine and `send-text` writes to the
PTY, not to the app. So a panel could not be driven from outside at all.

Four commands fix that: `key`, `click`, `drag` and `action`. They go in where a real
event does — `key` through the overlays, the leader layer and the bound actions;
`click`/`drag` through `on_click`/`on_cursor` with the pointer parked on a cell.

Three things it needed:
- **A `KeyEvent` cannot be built outside winit** (`platform_specific` is private), so
  the handlers had to stop asking for one. `overlay_key` and `git_leader_key` now
  take `(&Key, ModifiersState)`, which is all they ever read.
- **`actions::chord_to_key`**, the inverse of `Chord::from_event`, so a scripted key
  is spelled exactly like a config binding (`alt+shift+space`, `pagedown`, `]`). A
  test round-trips every shape.
- **The leader block moved out of `on_key`** into `leader_key(...) -> bool`, shared
  with the scripted path. A second implementation of a modal layer is a second set of
  bugs.

Every one of them answers with `ui_state()`: which overlay is up and, for the git
panel, its view, focus, zoom, cursor, open commit, column widths and — in SCREEN
cells — where its separators are. That last field exists because the first drag this
tool ever ran missed: the widths are panel-local, the panel is inset two cells, and
aiming at `list_w` clicked a row instead of the rule beside it. A caller that has to
add an origin it cannot see will get it wrong, so the panel reports the number to
aim at.

With it, the whole feature above was driven from a shell: open the panel, switch to
the log, Enter a commit, walk its files, zoom, escape, drag both separators, arm the
leader and descend into a group — each step asserted on the JSON that came back.

## 2026-07-21 - The docker panel, step 1: the daemon, the objects, the verbs

The design below, built as far as the local and remote DAEMONS go. Hub is a row in
the hosts column that says it is not built yet, and the deploy action is not here.

**The daemon is spoken to over its socket, not by parsing `docker`.** Two of the
things this panel exists for — `/events` and `/stats` — are streams, and scraping a
CLI for a stream is a losing game; going to the socket for the lists too means one
transport and one set of field names. `docker.rs` is a tiny HTTP/1.1 client over a
`UnixStream`, `Connection: close` per request, with the chunked decoding the daemon
uses for anything it streams. A remote context opens `ssh <host> docker system
dial-stdio` and speaks the same HTTP down its stdio — the tunnel the CLI itself
uses, so it inherits the user's keys and config. The `Stream` is killed and reaped
on drop: an ssh child that outlives its request is one leaked per refresh.

**One place still uses the CLI, and it has to**: compose. Compose is a client that
reads yaml and talks to the daemon; the daemon has never heard of a project. The
files come off the containers' own `com.docker.compose.project.config_files` label,
which is how compose finds them again too.

**Containers are grouped by compose project**, because that is the unit the work is
done in — nobody deploys a container. Health is parsed out of the status line into
its own mark: `Up 3 days (healthy)` is two facts, and the one that matters is the
one folding them into a green dot would hide.

**Long output goes to a pane, short output stays inline.** `logs`, `inspect` and the
summary render in column 3; a shell, `compose up`, `compose pull` open a pane and
run there. Same call the git panel makes for a command that needs a terminal.

**Every delete confirms, naming what goes with it**: the container's named volumes
(which do NOT go with it, and saying so is the difference between a delete and a
lost database), the containers still using an image, the containers still using a
volume. `compose down` names what it would stop. Anything on a host that is not
this machine names the host. The panel is parked while the confirm is up and put
back on either answer, so "no" leaves the screen as it was.

Two bugs the wiring itself produced, both worth remembering:

- **A row holds an INDEX into the container list**, so reading "what is selected"
  AFTER replacing the snapshot reads a different container. `apply_snapshot` takes
  the id first and then rebuilds; a test kills the container above the cursor and
  asserts the cursor stayed on the same NAME.
- **A good read used to clear the footer**, which meant the message from the
  operation that triggered the read never survived long enough to be read. Only an
  error is cleared now.

Verified against the real daemon on this machine, driven by remote control: seven
containers in four compose projects with their health marks, the `desktop-linux`
context drawn as down with its reason, images/volumes/networks, logs (500 lines,
unwrapped from the daemon's 8-byte stream framing), inspect, then stop / start /
remove of a throwaway container including the confirm and its "no".

## 2026-07-21 - The docker panel, step 2: Docker Hub, the drift, and the deploy

**Hub is a host in the same column, and it authenticates twice.** The registry
(`registry-1.docker.io`, the v2 protocol) and the web API (`hub.docker.com`) are
two services with two token schemes, and the credentials come from
`~/.docker/config.json` and its credential helper FIRST — if `docker login` already
happened there is nothing for runnir to store, and a terminal asking again for a
token the machine already has is a terminal inventing a secret to look after.

**An organisation access token is refused by Hub's web API and accepted by the
registry.** That is this account's normal case, so the repository list falls back to
the repositories the LOCAL images name — and the header says which of the two you
are looking at, because a fallback list passed off as the account's catalogue is
worse than no list. The tags come from the registry either way, which is what makes
the fallback work at all.

**The drift is the point.** Each tag says how it compares with what is here: the
same image, a different one, not pulled here, or — the case that matters —
`local, never pushed`: built here under the name, so it looks published on every
list that goes by tag, and it is the one a deploy gets wrong. Compared by DIGEST,
never by id: a local id is a content hash of how this machine stored the image and
says nothing about what a registry holds. The `Accept` headers on the manifest
request are load-bearing — without them the registry converts a multi-architecture
image to an old single-arch manifest and answers with the digest of the CONVERSION,
which matches nothing local. One digest request per tag, on the tag under the
cursor: Hub rate-limits, and fetching a list of two hundred would spend the limit
on rows nobody looked at.

**The deploy is one command line**: `compose pull && compose up -d` on whichever
host the panel is pointed at, in a pane, after a confirm that names the host. The
`&&` is not decoration: an `up` after a failed pull silently restarts the project on
the image it was already running, which is the deploy that looks like it worked.
`>` publishes an image the same way — a push is the one verb here with consequences
outside this machine.

**A generation counter is for the thing it guards.** The first version bumped the
panel's `seq` in every worker, including the detail and digest reads, so opening a
host and then moving the cursor dropped the host's own answer and the panel sat on
`reading…` forever. Only a RELOAD is a new generation now; a detail is keyed by the
object it is for and a digest by its repo and tag.

**Still not built** from the design below: the `/events` stream (so a refresh is
pushed rather than asked for), `/stats` for the selection, logs through the hint
layer, a container's death in the status bar, browsing a volume with the explorer,
and `system df` / prune.

## DESIGN, PARTLY BUILT — the Docker / Docker Hub panel (decided 2026-07-21)

Discussed with Pedro on 2026-07-21, before a line of code. Nothing here is built. The
prose version, in Spanish, is in the qlaios personal wiki under `runnir`.

**What it is.** Native management of Docker from the terminal, seen graphically:
local containers/images/volumes/networks, containers on remote daemons over SSH, and
the private repos on Docker Hub. Read AND act: start/stop, build, push, pull, deploy.

**Shape: the git panel's contract, three columns.** Resizable, zoomable, with a
leader of its own (`DOCKER_LEADER`). Not a sidebar, not a modal layer.

```
┌ hosts ───────┬ objects ────────────┬ detail ───────────┐
│ ● local      │ [C] [I] [V] [N]     │ logs / inspect    │
│   cloudmax   │ ▾ qlaios (compose)  │ stats             │
│   demo       │    ● api    up 3d   │ ports, mounts     │
│ ── hub ──    │    ● db     up 3d   │ env               │
│   go2chaindev│    ○ web    exited  │ layers / history  │
└──────────────┴─────────────────────┴───────────────────┘
```

- Column 1 is hosts (docker contexts) plus Docker Hub as a PSEUDO-HOST. A host that
  is down is marked as down; it never stalls the frame.
- Column 2 carries a strip for the object kind, and inside it the compose project is
  a LEVEL of the tree, grouped by the `com.docker.compose.project` label — because
  the real workflow here (qlaios, cromowin) is compose, not loose containers. On the
  hub pseudo-host the strip disappears: repos → tags.
- Column 3 is the detail of what is selected.

**Three transports behind one layer.** The daemon socket is the primary one
(`/var/run/docker.sock`, HTTP), because `/events` and `/stats` are streams and
parsing CLI output for those is a losing game. But the socket does NOT reach the
other two: Docker Hub is the registry v2 + Hub HTTP API and needs its own client,
and a remote host needs the tunnel the CLI itself uses
(`ssh … docker system dial-stdio`). One common layer, three transports.

**Refresh is pushed, not polled.** `/events` announces starts, deaths and pulls;
`stats` is streamed only for the SELECTED container — every container at once is
expensive locally and impossible over SSH.

**Long output goes to a real pane; short output stays inline.** `logs`, `stats` and
`inspect` render in column 3. `build`, `push`, `pull` and `exec` open a pane and run
there: progress bars, colour, Ctrl-C and scrollback come free, and they are minutes
long. Same call as the git credentials prompt.

**Operations, by object:**
- Container: start, stop, restart, pause, kill, rm; `exec` shell → pane; live logs
  with a filter; inspect; copy id; open a published port in the browser. Healthcheck
  state is its own marker, not folded into up/exited.
- Compose project: up, down, restart, pull, show the yaml. `down -v` confirms
  separately, counting volumes.
- Image: rm, tag, push, pull, layer history, which containers use it; build from the
  current repo's Dockerfile → pane.
- Volume: rm confirmed by counting users; and browsing its contents with the explorer
  sidebar that already exists (via an ephemeral container when it is remote).
- Network: subnet, connected containers, rm.
- Docker Hub: the account/org repos, public and private; per repo the tags, digest,
  size, last push, architectures; pull a tag, delete a tag, search.
- Across all of it: a live event feed, and `system df` broken down into what a prune
  WOULD free, shown before it runs.

**The four things lazydocker does not do**, which are the reason to build this at all:
1. **Local ↔ Hub diff.** Compare the local `RepoDigest` against the remote tag's
   digest, so a row says whether local is behind, or whether what runs on cloudmax is
   not what is published.
2. **Deploy as one action**: build → push → (ssh) pull + up, steps visible, each
   cancellable. It is what the `desplegar-*` skills do, inside the panel.
3. **Logs through runnir's hint layer** — paths, URLs and hashes clickable in a log,
   exactly as in the terminal.
4. **A container that dies says so in the status bar**, like the per-tab dirty marker.

**Credentials.** Read `~/.docker/config.json` and its credential helpers FIRST: if
`docker login` already happened there is nothing to store. Only if there is nothing,
offer a login and keep the PAT OUT of `config.toml` — a separate 0600 file or the
system keyring — with the settings panel showing `••••` and a date, never the value.
A Docker Hub PAT with read/write scope can publish to `go2chaindev/*`.

**Traps identified up front:**
- `stats` for the selection only. All of them at once is unsustainable over SSH.
- One persistent multiplexed connection per SSH host, read on a thread with a
  sequence number, like the git panel. A slow host must not block a frame.
- `prune`, `rm -v` and `down -v` destroy data: they belong to the guardian, and they
  count before they ask.
- `exec` on a host that is not local confirms with the HOST NAMED.
- Compare by `RepoDigests`, never by `Id` — the local Id says nothing about a remote.
- Docker Hub rate-limits: cache tags, do not repoll them.
- The settings rule from the explorer still holds: an option lives in the config, in
  the settings panel AND in the code that reads it — all three or none.

## Gotchas (do not re-learn)

- Half-block art is square per CELL, not on screen. Any image fit needs the cell
  aspect (`cw/ch`) or it comes out twice as tall as it should.
- A scripted input path must mirror the real one's ORDER, not only its handlers.
  `press_key` had every handler and the wrong order, and a whole panel was unreachable.
- Handlers that take a `winit::event::KeyEvent` cannot be called by anything but
  winit. Take `(&Key, ModifiersState)` and they stay scriptable and testable.
- Any coordinate a remote API hands out has to be in the space the caller will aim
  in. Panel-local widths plus an unstated origin is a trap that fires on first use.
- One guard on `j`/`k` is half a guard: the arrows are the same motion and get
  reached by reflex. Bind them in the same arm or they will diverge.
- A worker message that mutates panel state needs the SAME `seq == current` guard the
  preview has. Anything slower than the user is a race.
- Two accessors that switch meaning by mode (`len()` = list OR commit files) will
  eventually be read in the wrong mode. Give the second thing its own pair.
- A setting that exists in `config.rs` and in the settings panel is NOT a feature.
  `confirm_close` was both, and read by nobody, for months. Grep for every field of
  `Behaviour` before trusting one.
- `cargo test` fails `shell_integration::fish_prepends_xdg_data_dirs` when run from a
  shell that runnir itself launched: the injected `XDG_DATA_DIRS` is already
  prepended, so `apply` skips and the test finds no env. Run it as
  `env -u XDG_DATA_DIRS cargo test`. Not a regression.
- A binding spec and a keypress must produce the SAME `ChordKey` variant.
  Punctuation is always `Char`; only real NamedKeys (arrows, F-keys, pageup) are
  `Named`. Test a new spec against `Chord::from_event`, never just against parse.
- The leader layer is a TREE. Adding an action means adding it to a group in
  `default_leader_bindings` — the superset test fails otherwise, on purpose.
- A hint that must not steal the keyboard is chrome (a grid appended in `draw`),
  never an `Overlay`. Overlays capture input by design.
- `Gpu` cannot see the `Keymap` (it lives in `App`). Anything the draw path needs
  from it has to be snapshotted into a `Gpu` field when the input handler runs.
- Check a candidate default chord against Windows/KDE/GNOME/macOS BEFORE shipping
  it, not after. `alt+space` shipped broken on 3 of 4 desktops. Also never
  ctrl+alt (= AltGr on EU layouts) and never super (the compositor wins).
- A modal keyboard layer needs a PERSISTENT indicator in fixed chrome, and a
  self-scheduled wake to clear it, or an idle window draws it forever.
- Font size is LOGICAL (`Gpu.font_px`); the atlas is always `font_px * scale`. Never
  pass a raw config size to `FontAtlas::new_with` — multiply by `self.scale`.
- Any font/cell change MUST call `Tab::set_cell()` before `Tab::reflow()`, or the PTY
  is sized with a stale cell and output runs off the bottom of the window.
- `ScaleFactorChanged` arrives BEFORE `Resized` on Wayland; reflowing in it uses the
  old surface size. Harmless because the `Resized` reflow follows — don't "fix" it.
- Claude Code does not probe for the kitty keyboard protocol. Terminal-side protocol
  support alone is not enough; the legacy encoding has to carry Shift+Enter as ESC-CR.

- A hand-built patch needs `git apply --recount`: the `@@` counts come from the
  hunk it was cut from and stop matching the moment a line is dropped or converted.
- `git blame --no-color` is ambiguous (`--no-color-lines`, `--no-color-by-age`).
  Use `-c color.ui=false` instead.
- A single-line input drawn into a fixed-width panel WILL clip, and `write` clips
  silently. Grow the box, then scroll the text keeping its end visible - the caret
  lives there.
- `pkill -f <pattern>` kills its OWN shell when the pattern appears in the command
  line that invoked it. Killing a build's binary by path needs `pgrep -x` plus a
  `/proc/<pid>/exe` check, not `pkill -f target/debug/...`.
- Target a test instance by `/proc/<pid>/exe`, never by "not these pids". Pedro
  opens runnir windows too, and keys sent to the wrong one land in his session.
- `.git` is not always a directory: in a worktree and in a submodule it is a file
  holding `gitdir: <path>`, and the refs then live in the `commondir` it points at.
  Never join `.git/<thing>` onto a repo root - go through `git::git_dir()`.
- winit delivers space as `Key::Named(NamedKey::Space)`, never as
  `Key::Character(" ")`. An overlay binding it on the character compiles, runs, and
  does nothing.
- A grid drawn on top of a pane is OPAQUE, even where it looks empty: every
  non-spacer cell emits an instance, and a blank cell with `Color::Default` paints
  the pane background. An annotation layer must set `PaneDraw.transparent`, or it
  hides what it annotates.
- A scratch `XDG_CONFIG_HOME` does NOT isolate a test instance. `session.rs` writes to
  `dirs::data_dir()` (`~/.local/share/runnir/session.json`) while `project_session.rs`
  uses `dirs::config_dir()`, so a "clean" instance restores the real session and
  overwrites it on exit. Set `XDG_DATA_HOME` too, and kill the instance with `kill -9`
  rather than quitting it, until those two agree on a directory.
- `dms screenshot window` captures whatever dms thinks is focused, which is not
  necessarily the window you just focused with `hyprctl`. To shoot a specific window,
  capture its output (`dms screenshot output -o <name>`) and crop to the geometry from
  `hyprctl clients -j` with `magick -crop WxH+X+Y`.
- Never ask wgpu for `Backends::all()` on a hybrid laptop. Enumerating GL wakes a
  runtime-suspended discrete GPU (~1.8s) even when you asked for `LowPower` —
  that preference only ranks adapters that were already enumerated.
- When a startup feels slow "only the first time", check
  `/sys/bus/pci/devices/*/power/runtime_status` BEFORE blaming page cache. Time
  it once from a confirmed `suspended` state, not from whatever state you left.
- wgpu 30 API differs from all tutorials — read vendored source in ~/.cargo/registry/src/*/wgpu-30.0.0.
- sRGB: shader must emit LINEAR (surface is *UnormSrgb). Use to_linear() on every colour.
- Wayland: request_redraw from PTY thread does NOT wake Wait — use EventLoopProxy::send_event(Redraw).
- winit synthetic key events (is_synthetic) on focus must NOT go to PTY (the "sssh" bug).
- Only ONE mutex: Mutex<Grid>. Writes go via mpsc channel to a writer thread (never lock+block).
- docs.rs HELP is a double-quoted const — NEVER put `"` inside example lines (closes the string).
- Panics on the PTY reader thread poison the grid mutex → whole-app crash. Keep parser paths panic-free.
- Every new Action → add to id/title/parse/palette_list/bindings/hints. Update run_action AND run_palette_action.
- After any feature: document it in this file, in docs.rs (F1 help), commit+push.
