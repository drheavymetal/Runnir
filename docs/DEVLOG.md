# runnir — DEVLOG

Working memory across sessions. When context runs out, read this first to resume.

Repo: `git@github.com:drheavymetal/Runnar.git` (repo **Runnar**, crate **runnir**).
Commits **unsigned** (`git -c commit.gpgsign=false commit`). Push after each unit.
Build: `cargo build` / test: `cargo test` / release: `cargo build --release`.
Relaunch live: `pkill -x runnir; setsid ./target/release/runnir >/tmp/runnir-live.log 2>&1 </dev/null & disown`.
Verify headless: `runnir --dump '<cmd>'`, `runnir --render out.png '<cmd>' [ms]`, `runnir --demo out.png`.

## Status (2026-07-18)

Done: M0–M7 + 4 feature blocks + kitty graphics + mouse-to-TUIs + scrollback search
+ whisper + live-testing polish + DA1/fish fix. 23 commits, 138 tests, 0 warnings.
17 agent-found bugs fixed (7 default-model + 10 Fable 5). Logo installed.

## Current task: add 15–20 differential features, 4–5 "wow". Document each here.

User wants all of these, in batches, committing each, updating this file. Then run
Fable 5 agents until no bugs. Fable 5 IS available now (Pedro has credits).

### Planned features (mark [x] when shipped, with commit)

WOW (aim 4–5):
- [ ] W1 AI command guardian — intercept dangerous commands (rm -rf /, dd of=, mkfs,
      DROP TABLE, git push -f, :(){...}) before Enter; confirm/explain. Reads current
      input line from OSC 133 command region or prompt→cursor.
- [ ] W2 Semantic command folding — collapse a command's output block (OSC 133 marks);
      fold/unfold current, fold-all. Big cargo build → one line.
- [ ] W3 Named layouts / workspaces — config `[[layout]]` defines panes+commands;
      `runnir layout <name>` or palette launches N splits running X (e.g. ssh .3/.7/.9/.188).
- [ ] W4 Keyword alert / watch — per-pane regex; notify (desktop) when output matches
      ("deploy OK", "error", "panic"). For monitoring builds/servers.
- [ ] W5 Background blur + opacity — window.opacity already in config; wire it + compositor
      blur hint. (Pedro's kitty had blur 32.)

DIFFERENTIAL (aim ~15 total incl. above):
- [ ] D1 OSC 8 hyperlinks — clickable links apps emit (ls --hyperlink, gcc/cargo).
- [ ] D2 Config hot-reload — watch ~/.config/runnir/runnir.toml, apply live.
- [ ] D3 Shell-history fuzzy in palette — read fish/bash/zsh history, fuzzy pick, insert.
- [ ] D4 Visual + audible bell — flash pane / urgency hint on BEL.
- [ ] D5 Primary selection (middle-click paste of last selection) via wl-copy --primary.
- [ ] D6 Command status gutter — exit code + duration next to each prompt (OSC 133 D + timing).
- [ ] D7 Session summarizer (AI) — "what did I do here" from scrollback.
- [ ] D8 Broadcast groups — broadcast to a named subset of panes.
- [ ] D9 Smooth/momentum scroll.
- [x] D10 Pane zoom (Ctrl+Shift+Z) — visible_rects override + resize_one. commit 25.
- [ ] D11 Open scrollback in $EDITOR / pipe pane to editor.
- [ ] D12 Copy-mode with vim motions (keyboard scrollback nav + select).
- [ ] D13 Tab reordering (drag / move-left/right).
- [ ] D14 URL/path auto-underline on hover (not just hint mode).
- [ ] D15 Config: cursor trail / animations toggle (optional flair).

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
