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

## Gotchas (do not re-learn)

- Font size is LOGICAL (`Gpu.font_px`); the atlas is always `font_px * scale`. Never
  pass a raw config size to `FontAtlas::new_with` — multiply by `self.scale`.
- Any font/cell change MUST call `Tab::set_cell()` before `Tab::reflow()`, or the PTY
  is sized with a stale cell and output runs off the bottom of the window.
- `ScaleFactorChanged` arrives BEFORE `Resized` on Wayland; reflowing in it uses the
  old surface size. Harmless because the `Resized` reflow follows — don't "fix" it.
- Claude Code does not probe for the kitty keyboard protocol. Terminal-side protocol
  support alone is not enough; the legacy encoding has to carry Shift+Enter as ESC-CR.

- wgpu 30 API differs from all tutorials — read vendored source in ~/.cargo/registry/src/*/wgpu-30.0.0.
- sRGB: shader must emit LINEAR (surface is *UnormSrgb). Use to_linear() on every colour.
- Wayland: request_redraw from PTY thread does NOT wake Wait — use EventLoopProxy::send_event(Redraw).
- winit synthetic key events (is_synthetic) on focus must NOT go to PTY (the "sssh" bug).
- Only ONE mutex: Mutex<Grid>. Writes go via mpsc channel to a writer thread (never lock+block).
- docs.rs HELP is a double-quoted const — NEVER put `"` inside example lines (closes the string).
- Panics on the PTY reader thread poison the grid mutex → whole-app crash. Keep parser paths panic-free.
- Every new Action → add to id/title/parse/palette_list/bindings/hints. Update run_action AND run_palette_action.
- After any feature: document it in this file, in docs.rs (F1 help), commit+push.
