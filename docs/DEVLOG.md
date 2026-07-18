# runnir — DEVLOG

Working memory across sessions. When context runs out, read this first to resume.

Repo: `git@github.com:drheavymetal/Runnar.git` (repo **Runnar**, crate **runnir**).
Commits **unsigned** (`git -c commit.gpgsign=false commit`). Push after each unit.
Build: `cargo build` / test: `cargo test` / release: `cargo build --release`.
Shell is fish: NEVER put backticks in `git commit -m "..."` — fish command-substitutes
them even inside double quotes and silently drops the word. Use plain quotes.
Relaunch live: `pkill -x runnir; setsid ./target/release/runnir >/tmp/runnir-live.log 2>&1 </dev/null & disown`.
Verify headless: `runnir --dump '<cmd>'`, `runnir --render out.png '<cmd>' [ms]`, `runnir --demo out.png`.

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

## Gotchas (do not re-learn)

- wgpu 30 API differs from all tutorials — read vendored source in ~/.cargo/registry/src/*/wgpu-30.0.0.
- sRGB: shader must emit LINEAR (surface is *UnormSrgb). Use to_linear() on every colour.
- Wayland: request_redraw from PTY thread does NOT wake Wait — use EventLoopProxy::send_event(Redraw).
- winit synthetic key events (is_synthetic) on focus must NOT go to PTY (the "sssh" bug).
- Only ONE mutex: Mutex<Grid>. Writes go via mpsc channel to a writer thread (never lock+block).
- docs.rs HELP is a double-quoted const — NEVER put `"` inside example lines (closes the string).
- Panics on the PTY reader thread poison the grid mutex → whole-app crash. Keep parser paths panic-free.
- Every new Action → add to id/title/parse/palette_list/bindings/hints. Update run_action AND run_palette_action.
- After any feature: document it in this file, in docs.rs (F1 help), commit+push.
