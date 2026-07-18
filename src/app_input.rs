// Input handling for `Gpu`. Included into main.rs (crate root), not a module, so
// it shares the imports there.

impl Gpu {
    fn on_wheel(&mut self, delta: MouseScrollDelta, config: &Config, mods: ModifiersState) {
        let cell_h = self.renderer.cell_size().1;
        // While an overlay owns input, the wheel scrolls it, not the terminal. Use
        // the real cell height so a touchpad's pixel deltas map to sane line counts.
        if let Some(ov) = self.overlay.as_mut() {
            let lines = wheel_lines(delta, config.behaviour.wheel_lines, cell_h);
            if let Overlay::Docs(d) = ov {
                d.scroll(-lines.round() as isize);
            }
            self.window.request_redraw();
            return;
        }
        // A mouse-mode app (unless Shift is held) gets the wheel as button events.
        let lines = wheel_lines(delta, config.behaviour.wheel_lines, cell_h);
        // A zero delta (horizontal-only scroll event) must not synthesise a vertical
        // wheel report or a scroll — bail before either path.
        if lines == 0.0 {
            return;
        }
        // The user took over scrolling: cancel any in-flight glide animation.
        self.scroll_glide = None;
        if !mods.shift_key() && self.forward_wheel(lines) {
            self.scroll_accum = 0.0;
            return;
        }
        // Accumulate fractional lines so a slow touchpad swipe (sub-line pixel
        // deltas) scrolls smoothly instead of being truncated to nothing. The whole
        // part moves now; the remainder carries to the next event.
        self.scroll_accum += lines;
        let whole = self.scroll_accum.trunc();
        self.scroll_accum -= whole;
        if whole != 0.0 && self.tab().focused().scroll(whole as isize) {
            self.window.request_redraw();
        }
    }

    fn on_cursor(&mut self, position: PhysicalPosition<f64>, mods: ModifiersState) {
        self.cursor_px = position;
        // Dragging a divider resizes, regardless of overlay state.
        if let Some(hit) = self.resizing.clone() {
            let area = self.active_area();
            self.tabs[self.active].drag_divider(area, &hit, position.x as f32, position.y as f32);
            self.window.request_redraw();
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        // Underline a URL/path under the pointer (D14); repaint when it changes.
        if self.update_hover(position) {
            self.window.request_redraw();
        }
        // Report drag motion to a mouse-mode app while a button is held.
        if !mods.shift_key() && self.mouse_down.is_some() {
            if self.forward_mouse(mouse::Kind::Move, self.mouse_down.unwrap(), position) {
                return;
            }
        }
        let area = self.active_area();
        if let Some((id, rect)) = self.pane_at(position, area) {
            if self.tab().focused_ptr() == id && self.tab().focused().selecting {
                if let Some(point) = self.point_in(id, rect, position) {
                    if self.tab().focused().update_selection(point) {
                        self.window.request_redraw();
                    }
                }
            }
        }
    }

    fn on_click(&mut self, state: ElementState, button: MouseButton, mods: ModifiersState, config: &Config) {
        // Left release always ends a divider drag, even over an overlay.
        if state == ElementState::Released && button == MouseButton::Left {
            self.resizing = None;
        }
        if self.overlay.is_some() {
            return;
        }
        // A left press in the focused pane's minimap strip jumps to that position.
        if state == ElementState::Pressed && button == MouseButton::Left && config.window.minimap {
            if self.minimap_jump(self.cursor_px) {
                return;
            }
        }
        // A mouse press leaves copy-mode (keyboard mode) before it can redirect focus
        // onto another pane, which would otherwise strand its selection.
        if state == ElementState::Pressed && self.copy_mode.is_some() {
            self.exit_copy_mode(false);
        }

        // A left press on the tab bar switches tab; on a divider starts a resize.
        if state == ElementState::Pressed && button == MouseButton::Left {
            if let Some(i) = self.tab_bar_hit(self.cursor_px) {
                self.active = i;
                self.window.request_redraw();
                return;
            }
            let area = self.active_area();
            let (x, y) = (self.cursor_px.x as f32, self.cursor_px.y as f32);
            if let Some(hit) = self.tabs[self.active].divider_at(area, x, y) {
                self.resizing = Some(hit);
                return;
            }
        }

        // Ctrl+left on a hovered URL/path opens/copies it instead of selecting.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && mods.control_key()
            && self.open_hover()
        {
            return;
        }

        // Focus the pane under the pointer on any press first.
        if state == ElementState::Pressed {
            let area = self.active_area();
            if let Some((id, _)) = self.pane_at(self.cursor_px, area) {
                self.tab().focus = id;
            }
        }

        // A mouse-mode app (unless Shift held) receives the click instead of the
        // terminal acting on it.
        let btn = match button {
            MouseButton::Left => Some(mouse::Button::Left),
            MouseButton::Middle => Some(mouse::Button::Middle),
            MouseButton::Right => Some(mouse::Button::Right),
            _ => None,
        };
        if !mods.shift_key() {
            if let Some(btn) = btn {
                let kind = if state == ElementState::Pressed {
                    mouse::Kind::Press
                } else {
                    mouse::Kind::Release
                };
                if self.forward_mouse(kind, btn, self.cursor_px) {
                    self.mouse_down = (state == ElementState::Pressed).then_some(btn);
                    return;
                }
            }
        }

        match (state, button) {
            (ElementState::Pressed, MouseButton::Left) => {
                let area = self.active_area();
                if let Some((id, rect)) = self.pane_at(self.cursor_px, area) {
                    // A click on a fold summary unfolds it instead of selecting.
                    if let Some(local) = self.fold_row_at(id, rect, self.cursor_px) {
                        let pane = self.tabs[self.active].panes.get_mut(&id).unwrap();
                        pane.toggle_fold_at(local);
                        // Drop any stale selection so the coming left-release does not
                        // re-copy it as if this were a normal click.
                        pane.clear_selection();
                        self.window.request_redraw();
                        return;
                    }
                    if let Some(point) = self.point_in(id, rect, self.cursor_px) {
                        // Double-click selects a word, triple a line.
                        let mode = self.click_mode(point);
                        self.tab().focused().begin_selection(point, mode);
                        self.window.request_redraw();
                    }
                }
            }
            (ElementState::Released, MouseButton::Left) => {
                self.tab().focused().end_selection();
                if self.tabs[self.active].focused_ref().selection.is_some() {
                    self.copy_selection();
                }
            }
            (ElementState::Pressed, MouseButton::Middle) => self.paste_primary(),
            _ => {}
        }
    }

    /// Forwards a mouse event to the focused pane's process if it is in mouse mode.
    /// Returns whether it was consumed.
    fn forward_mouse(
        &mut self,
        kind: mouse::Kind,
        button: mouse::Button,
        pos: PhysicalPosition<f64>,
    ) -> bool {
        let area = self.active_area();
        let Some((id, rect)) = self.pane_at(pos, area) else { return false };
        if id != self.tab().focused_ptr() {
            return false;
        }
        let (mode, sgr) = {
            let g = self.tab().focused().grid.lock().unwrap();
            (g.mouse_mode, g.mouse_sgr)
        };
        let (cw, ch) = self.renderer.cell_size();
        let col = (((pos.x as f32 - rect.x) / cw).floor().max(0.0)) as usize;
        let row = (((pos.y as f32 - rect.y) / ch).floor().max(0.0)) as usize;
        if let Some(bytes) = mouse::encode(mode, sgr, button, kind, col, row) {
            self.tab().focused().write(&bytes);
            true
        } else {
            false
        }
    }

    fn forward_wheel(&mut self, lines: f32) -> bool {
        let button = if lines > 0.0 { mouse::Button::WheelUp } else { mouse::Button::WheelDown };
        // One report per line of scroll, so a fast wheel moves several rows.
        let n = lines.abs().round().max(1.0) as usize;
        let mut consumed = false;
        for _ in 0..n {
            if self.forward_mouse(mouse::Kind::Press, button, self.cursor_px) {
                consumed = true;
            }
        }
        consumed
    }

    fn on_key(
        &mut self,
        event: winit::event::KeyEvent,
        mods: ModifiersState,
        config: &Config,
        keymap: &Keymap,
        event_loop: &ActiveEventLoop,
    ) {
        // An overlay swallows all keys while open.
        if self.overlay.is_some() {
            self.overlay_key(&event, mods, config);
            return;
        }

        // Copy-mode owns the keyboard: vim motions drive a virtual cursor/selection.
        if self.copy_mode.is_some() {
            self.copy_mode_key(&event, mods);
            return;
        }

        // A bound chord runs its action and never reaches the child.
        if let Some(action) = keymap.resolve(&event.logical_key, mods) {
            self.run_action(action.clone(), config, event_loop);
            return;
        }

        // Command guardian: a plain Enter about to submit a destructive command
        // opens a confirmation first. Only bare Enter (no modifiers) with the view
        // at the live prompt is guarded, so history editing and TUIs are untouched.
        if config.behaviour.command_guardian
            && matches!(event.logical_key, Key::Named(NamedKey::Enter))
            && event.state.is_pressed()
            && mods.is_empty()
        {
            let line = {
                let g = self.tab().focused().grid.lock().unwrap();
                // A full-screen app (vim, htop) has no shell command line to guard;
                // scanning its buffer would pop the confirm over unrelated content.
                if g.alt_screen() { String::new() } else { g.current_command_text() }
            };
            if let Some(reason) = crate::guardian::danger(&line) {
                let label = format!("Run this? {reason}");
                self.overlay = Some(Overlay::Prompt(Prompt::new(
                    PromptKind::GuardedCommand,
                    &label,
                    vec![line.trim().to_string()],
                )));
                self.window.request_redraw();
                return;
            }
        }

        // Otherwise it is input for the focused pane's process.
        let mode = keys::KeyMode { app_cursor: self.tab().focused().app_cursor() };
        if let Some(bytes) = keys::encode(&event, mods, mode) {
            // Diagnostic: RUNNIR_KEYLOG=1 logs each keypress → bytes it sends, with
            // the focused pane id and the winit repeat flag, to catch duplication.
            if std::env::var("RUNNIR_KEYLOG").is_ok() {
                eprintln!(
                    "keylog pane={} repeat={} key={:?} -> {:?}",
                    self.tab().focused_ptr(),
                    event.repeat,
                    event.logical_key,
                    String::from_utf8_lossy(&bytes)
                );
            }
            self.scroll_glide = None;
            if self.tab().focused().snap_to_bottom() {
                self.window.request_redraw();
            }
            self.tab().focused().clear_selection();
            if self.broadcast {
                self.broadcast_bytes(&bytes);
            } else {
                self.tab().focused().write(&bytes);
            }
        }
    }

    fn run_action(&mut self, action: Action, config: &Config, event_loop: &ActiveEventLoop) {
        let area = self.active_area();
        let wake = wake_fn(self.proxy.clone());
        match action {
            Action::Quit => {
                self.save_session(config);
                event_loop.exit();
            }

            Action::NewTab => {
                let id = self.new_pane_id();
                if let Ok(tab) = Tab::new(area, self.renderer.cell_size(), config, id, &Spawn::default(), wake) {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                    self.reflow_all();
                }
            }
            Action::CloseTab => {
                if self.tabs.len() > 1 {
                    // Remember it so ReopenClosed can bring it back.
                    self.closed_tabs.push(self.tabs[self.active].to_session());
                    self.tabs.remove(self.active);
                    self.active = self.active.min(self.tabs.len() - 1);
                    self.reflow_all();
                } else {
                    self.save_session(config);
                    event_loop.exit();
                }
            }
            Action::ReopenClosed => self.reopen_closed(config),
            Action::NextTab => self.active = (self.active + 1) % self.tabs.len(),
            Action::PrevTab => {
                self.active = (self.active + self.tabs.len() - 1) % self.tabs.len()
            }
            Action::GoToTab(n) => {
                if n >= 1 && n <= self.tabs.len() {
                    self.active = n - 1;
                }
            }
            Action::RenameTab => {
                self.overlay = Some(Overlay::Prompt(Prompt::new(
                    PromptKind::RenameTab,
                    "Rename tab",
                    Vec::new(),
                )));
            }
            Action::MoveTabLeft => self.move_tab(-1),
            Action::MoveTabRight => self.move_tab(1),

            Action::SplitHorizontal | Action::SplitVertical => {
                let axis = action.split_axis().unwrap();
                let id = self.new_pane_id();
                let _ = self.tab().split_with_id(area, axis, config, id, wake);
            }
            Action::ClosePane => {
                if !self.tab().close_focused(area) && self.tabs.len() > 1 {
                    self.tabs.remove(self.active);
                    self.active = self.active.min(self.tabs.len() - 1);
                    self.reflow_all();
                } else if self.tabs.len() == 1 && self.tab().tree.len() == 1 {
                    self.save_session(config);
                    event_loop.exit();
                }
            }
            a if a.focus_dir().is_some() => {
                self.tab().focus_dir(area, a.focus_dir().unwrap());
            }
            a if a.resize_dir().is_some() => {
                self.tab().resize_focused(area, a.resize_dir().unwrap());
            }
            Action::FocusNext => self.tab().focus_next(),

            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste(),
            Action::CopyLastOutput => {
                if let Some(text) = self.tab().focused().last_command_output() {
                    self.set_clipboard(text);
                }
            }
            Action::ScrollPageUp => {
                self.scroll_glide = None;
                let rows = self.tab().focused().grid.lock().unwrap().rows() as isize;
                self.tab().focused().scroll(rows);
            }
            Action::ScrollPageDown => {
                self.scroll_glide = None;
                let rows = self.tab().focused().grid.lock().unwrap().rows() as isize;
                self.tab().focused().scroll(-rows);
            }
            Action::ScrollToTop => {
                let max = self.focused_scrollback_len();
                self.glide_focused_to(max, config.behaviour.smooth_scroll);
            }
            Action::ScrollToBottom => {
                self.glide_focused_to(0.0, config.behaviour.smooth_scroll);
            }
            Action::ScrollUp => {
                self.scroll_glide = None;
                self.tab().focused().scroll(3);
            }
            Action::ScrollDown => {
                self.scroll_glide = None;
                self.tab().focused().scroll(-3);
            }
            Action::JumpPrevPrompt => self.jump_prompt(-1, config.behaviour.smooth_scroll),
            Action::JumpNextPrompt => self.jump_prompt(1, config.behaviour.smooth_scroll),
            Action::SearchScrollback => self.overlay = Some(Overlay::Search(overlay::Search::new())),

            Action::FontBigger => self.set_font_px(self.font_px + 1.0, config),
            Action::FontSmaller => self.set_font_px(self.font_px - 1.0, config),
            Action::FontReset => self.set_font_px(config.font.size, config),

            Action::CommandPalette => {
                self.overlay = Some(Overlay::Palette(Palette::new(&keyhints())));
            }
            Action::ShowDocs => {
                self.overlay = Some(Overlay::Docs(overlay::Docs::new(docs::HELP)));
            }
            Action::ToggleAi => self.toggle_ai(config),
            Action::AskAiAboutError => self.ask_ai_about_error(config),
            Action::AiCommand => self.ai_command(),
            Action::AiExplain => self.ai_explain_selection(config),
            Action::SummarizeSession => self.summarize_session(config),
            Action::OpenScrollbackInEditor => self.open_scrollback_in_editor(config),
            Action::HistorySearch => self.history_search(),
            Action::WatchKeyword => self.watch_keyword(),
            Action::LaunchLayout => self.open_layout_picker(config),
            Action::CopyMode => self.enter_copy_mode(),
            Action::FoldOutput => self.tab().focused().toggle_fold_all(),
            Action::QuickConnect => self.open_quick_connect(),
            Action::HintMode => self.open_hints(),
            Action::LaunchClaude => self.launch_claude(config),
            Action::Whisper => self.whisper(),
            Action::ToggleBroadcast => self.broadcast = !self.broadcast,
            Action::ToggleBroadcastGroup => self.toggle_broadcast_group(),
            Action::ToggleZoom => self.toggle_zoom(),
            Action::ClearSelectionOrScrollback => {
                if !self.tab().focused().clear_selection() {
                    self.scroll_glide = None;
                    self.tab().focused().snap_to_bottom();
                }
            }
            // The focus/resize directional actions are dispatched by the guarded
            // arms above; this arm is unreachable for them but keeps the match
            // exhaustive.
            _ => {}
        }
        self.window.request_redraw();
    }

    fn overlay_key(&mut self, event: &winit::event::KeyEvent, mods: ModifiersState, config: &Config) {
        let key = &event.logical_key;

        // A character typed with ctrl/alt/super is a shortcut attempt, not text —
        // ignore it so Ctrl+V inside a prompt does not insert a literal 'v'. Named
        // keys (Escape, Enter, arrows) still act.
        if matches!(key, Key::Character(_))
            && (mods.control_key() || mods.alt_key() || mods.super_key())
        {
            return;
        }
        match self.overlay.as_mut().unwrap() {
            Overlay::Palette(p) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Named(NamedKey::ArrowUp) => p.up(),
                Key::Named(NamedKey::ArrowDown) => p.down(),
                Key::Named(NamedKey::Backspace) => p.backspace(),
                Key::Named(NamedKey::Enter) => {
                    let sel = p.selected();
                    self.overlay = None;
                    if let Some(action) = sel {
                        self.run_palette_action(action, config);
                    }
                }
                Key::Character(s) => {
                    for c in s.chars() {
                        p.input(c);
                    }
                }
                _ => {}
            },
            Overlay::Docs(d) => match key {
                Key::Named(NamedKey::Escape) | Key::Named(NamedKey::Enter) => self.overlay = None,
                Key::Named(NamedKey::ArrowUp) => d.scroll(-1),
                Key::Named(NamedKey::ArrowDown) => d.scroll(1),
                Key::Named(NamedKey::PageUp) => d.scroll(-15),
                Key::Named(NamedKey::PageDown) => d.scroll(15),
                _ => {}
            },
            Overlay::Prompt(p) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Named(NamedKey::ArrowUp) => p.up(),
                Key::Named(NamedKey::ArrowDown) => p.down(),
                Key::Named(NamedKey::Backspace) => p.backspace(),
                Key::Named(NamedKey::Enter) => {
                    let kind = p.kind;
                    let value = p.value();
                    self.overlay = None;
                    self.confirm_prompt(kind, value, config);
                }
                Key::Character(s) => {
                    for c in s.chars() {
                        p.input_char(c);
                    }
                }
                Key::Named(NamedKey::Space) => p.input_char(' '),
                _ => {}
            },
            Overlay::Ai(panel) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Named(NamedKey::Backspace) => panel.backspace(),
                Key::Named(NamedKey::Enter) => {
                    let q = panel.take_input();
                    if !q.is_empty() {
                        self.send_ai(q, config);
                    }
                }
                Key::Named(NamedKey::Space) => panel.input_char(' '),
                Key::Character(s) => {
                    for c in s.chars() {
                        panel.input_char(c);
                    }
                }
                _ => {}
            },
            Overlay::Search(s) => {
                let mut changed = false;
                match key {
                    Key::Named(NamedKey::Escape) => {
                        self.overlay = None;
                        self.scroll_glide = None;
                        self.tab().focused().snap_to_bottom();
                    }
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::ArrowDown) => {
                        s.next();
                        changed = true;
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        s.prev();
                        changed = true;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        s.backspace();
                        self.recompute_search();
                        changed = true;
                    }
                    Key::Named(NamedKey::Space) => {
                        s.input(' ');
                        self.recompute_search();
                        changed = true;
                    }
                    Key::Character(txt) => {
                        for c in txt.chars() {
                            s.input(c);
                        }
                        self.recompute_search();
                        changed = true;
                    }
                    _ => {}
                }
                if changed {
                    self.scroll_to_current_match();
                }
            }
            Overlay::Hints(h) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Character(s) => {
                    if let Some(c) = s.chars().next() {
                        match h.input(c) {
                            overlay::HintResult::More => {}
                            overlay::HintResult::NoMatch => self.overlay = None,
                            overlay::HintResult::Chosen(text, kind) => {
                                self.overlay = None;
                                self.act_on_hint(text, kind);
                            }
                        }
                    }
                }
                _ => {}
            },
        }
        let _ = mods;
        self.window.request_redraw();
    }

    fn run_palette_action(&mut self, action: Action, config: &Config) {
        // The palette has no ActiveEventLoop to exit cleanly, so Quit exits the
        // process here — but must save the session first, exactly like the keyboard
        // and window-close paths, or picking "Quit" from the palette would lose it.
        if action == Action::Quit {
            self.save_session(config);
            std::process::exit(0);
        }
        // Reuse run_action by faking an event loop is not possible; inline the ones
        // the palette exposes that do not need the loop.
        let area = self.active_area();
        let wake = wake_fn(self.proxy.clone());
        match action {
            Action::NewTab => {
                let id = self.new_pane_id();
                if let Ok(tab) =
                    Tab::new(area, self.renderer.cell_size(), config, id, &Spawn::default(), wake)
                {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                    self.reflow_all();
                }
            }
            Action::SplitHorizontal | Action::SplitVertical => {
                let id = self.new_pane_id();
                let _ = self.tab().split_with_id(area, action.split_axis().unwrap(), config, id, wake);
            }
            Action::CommandPalette => {
                self.overlay = Some(Overlay::Palette(Palette::new(&keyhints())))
            }
            Action::ShowDocs => self.overlay = Some(Overlay::Docs(overlay::Docs::new(docs::HELP))),
            Action::ToggleAi => self.toggle_ai(config),
            Action::AskAiAboutError => self.ask_ai_about_error(config),
            Action::AiCommand => self.ai_command(),
            Action::AiExplain => self.ai_explain_selection(config),
            Action::SummarizeSession => self.summarize_session(config),
            Action::OpenScrollbackInEditor => self.open_scrollback_in_editor(config),
            Action::HistorySearch => self.history_search(),
            Action::WatchKeyword => self.watch_keyword(),
            Action::LaunchLayout => self.open_layout_picker(config),
            Action::CopyMode => self.enter_copy_mode(),
            Action::FoldOutput => self.tab().focused().toggle_fold_all(),
            Action::Whisper => self.whisper(),
            Action::SearchScrollback => self.overlay = Some(Overlay::Search(overlay::Search::new())),
            Action::QuickConnect => self.open_quick_connect(),
            Action::HintMode => self.open_hints(),
            Action::LaunchClaude => self.launch_claude(config),
            Action::RenameTab => {
                self.overlay = Some(Overlay::Prompt(Prompt::new(
                    PromptKind::RenameTab,
                    "Rename tab",
                    Vec::new(),
                )))
            }
            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste(),
            Action::CopyLastOutput => {
                if let Some(text) = self.tab().focused().last_command_output() {
                    self.set_clipboard(text);
                }
            }
            Action::CloseTab => {
                if self.tabs.len() > 1 {
                    self.closed_tabs.push(self.tabs[self.active].to_session());
                    self.tabs.remove(self.active);
                    self.active = self.active.min(self.tabs.len() - 1);
                    self.reflow_all();
                }
            }
            Action::ReopenClosed => self.reopen_closed(config),
            Action::NextTab => self.active = (self.active + 1) % self.tabs.len(),
            Action::PrevTab => {
                self.active = (self.active + self.tabs.len() - 1) % self.tabs.len()
            }
            Action::ClosePane => {
                self.tab().close_focused(area);
            }
            Action::ScrollToTop => {
                let max = self.focused_scrollback_len();
                self.glide_focused_to(max, config.behaviour.smooth_scroll);
            }
            Action::ScrollToBottom => {
                self.glide_focused_to(0.0, config.behaviour.smooth_scroll);
            }
            Action::JumpPrevPrompt => self.jump_prompt(-1, config.behaviour.smooth_scroll),
            Action::JumpNextPrompt => self.jump_prompt(1, config.behaviour.smooth_scroll),
            Action::FontBigger => self.set_font_px(self.font_px + 1.0, config),
            Action::FontSmaller => self.set_font_px(self.font_px - 1.0, config),
            Action::FontReset => self.set_font_px(config.font.size, config),
            Action::ToggleBroadcast => self.broadcast = !self.broadcast,
            Action::ToggleBroadcastGroup => self.toggle_broadcast_group(),
            Action::ToggleZoom => self.toggle_zoom(),
            Action::MoveTabLeft => self.move_tab(-1),
            Action::MoveTabRight => self.move_tab(1),
            _ => {}
        }
    }

    /// Reorders the active tab one slot left (-1) or right (+1), wrapping around,
    /// and keeps it focused. The tab bar reflects the new order immediately.
    fn move_tab(&mut self, delta: isize) {
        let n = self.tabs.len();
        if n < 2 {
            return;
        }
        let to = (self.active as isize + delta).rem_euclid(n as isize) as usize;
        // Remove + insert, not swap: at the wrap boundary a swap would fling the far
        // tab across the bar; remove+insert genuinely shifts one slot in every case.
        let tab = self.tabs.remove(self.active);
        self.tabs.insert(to, tab);
        self.active = to;
        self.window.request_redraw();
    }

    /// Zooms the focused pane to fill the tab, or unzooms. Resizes its PTY so the
    /// program sees the bigger size, and restores every pane on unzoom.
    /// Enters keyboard copy-mode (D12): a virtual cursor starts at the pane's live
    /// cursor; hjkl/arrows move it, v anchors a selection, y/Enter yanks, Esc/q exit.
    fn enter_copy_mode(&mut self) {
        self.scroll_glide = None;
        let pane_id = self.tab().focused_ptr();
        let (start, dropped) = {
            let g = self.tab().focused().grid.lock().unwrap();
            // Start where the user is looking: the live cursor when following output,
            // else the top of the scrolled-back view, so it is never off-screen.
            let start = if g.display_offset() > 0 {
                (g.abs_row(0), 0)
            } else {
                (g.total_rows() - g.rows() + g.cursor().0, g.cursor().1)
            };
            (start, g.dropped())
        };
        self.tab().focused().clear_selection();
        self.copy_mode = Some(CopyMode { pane: pane_id, cur: start, anchor: None, dropped });
        self.sync_copy_selection();
        self.status = Some("copy-mode — hjkl move, v select, y yank, Esc exit".into());
        self.status_expiry = Some(Instant::now() + Duration::from_secs(3));
        self.window.request_redraw();
    }

    /// The pane copy-mode is bound to, wherever it lives, so a focus/tab change can't
    /// redirect copy-mode onto a different pane.
    fn copy_pane_mut(&mut self) -> Option<&mut crate::pane::Pane> {
        let id = self.copy_mode.as_ref()?.pane;
        self.tabs.iter_mut().find_map(|t| t.panes.get_mut(&id))
    }

    /// Mirrors the copy-mode cursor/anchor onto its pane's selection so the existing
    /// highlight rendering shows both the cursor cell and any selection.
    fn sync_copy_selection(&mut self) {
        let Some(cm) = self.copy_mode.as_ref() else { return };
        let (anchor, cur) = (cm.anchor.unwrap_or(cm.cur), cm.cur);
        if let Some(pane) = self.copy_pane_mut() {
            pane.begin_selection(anchor, crate::selection::Mode::Char);
            pane.update_selection(cur);
            // Not an active mouse drag: leave `selecting` false so bare pointer
            // motion can't drag the copy-mode selection out from under the keyboard.
            pane.end_selection();
        }
    }

    /// Ends copy-mode, optionally copying the selection first. Operates on the bound
    /// pane, not the focused one.
    fn exit_copy_mode(&mut self, yank: bool) {
        let anchored = self.copy_mode.as_ref().is_some_and(|cm| cm.anchor.is_some());
        let text = if yank && anchored {
            self.copy_pane_mut().and_then(|p| p.selection_text())
        } else {
            None
        };
        if let Some(pane) = self.copy_pane_mut() {
            pane.clear_selection();
        }
        if let Some(text) = text {
            self.clipboard.set_primary(&text);
            self.set_clipboard(text);
        }
        self.copy_mode = None;
        self.status = None;
        self.window.request_redraw();
    }

    fn copy_mode_key(&mut self, event: &winit::event::KeyEvent, mods: ModifiersState) {
        use winit::keyboard::{Key, NamedKey};
        // A modified chord (Ctrl+C, Ctrl+Q, …) leaves copy-mode rather than being
        // mis-read as a motion key, and hands control back to the shell/bindings.
        if mods.control_key() || mods.alt_key() || mods.super_key() {
            self.exit_copy_mode(false);
            return;
        }
        let Some(cm) = self.copy_mode.as_ref() else { return };
        let pane_id = cm.pane;
        // The bound pane must be in the active tab; otherwise (a tab switch) leave.
        if !self.tabs[self.active].panes.contains_key(&pane_id) {
            self.exit_copy_mode(false);
            return;
        }
        let (rows, cols, total, top, dropped) = {
            let g = self.tabs[self.active].panes[&pane_id].grid.lock().unwrap();
            (g.rows(), g.cols(), g.total_rows(), g.abs_row(0), g.dropped())
        };
        let last_col = cols.saturating_sub(1);
        let last_row = total.saturating_sub(1);
        let mut cm = self.copy_mode.take().unwrap();
        // Rebase for any eviction since the last key: the abs index space shifts down
        // one per dropped row, so subtract the delta to stay on the same content.
        let shift = dropped.saturating_sub(cm.dropped);
        if shift > 0 {
            cm.cur.0 = cm.cur.0.saturating_sub(shift);
            if let Some(a) = cm.anchor.as_mut() {
                a.0 = a.0.saturating_sub(shift);
            }
        }
        cm.dropped = dropped;
        let (mut yank, mut exit) = (false, false);

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => exit = true,
            Key::Named(NamedKey::Enter) => yank = true,
            Key::Named(NamedKey::ArrowLeft) => cm.cur.1 = cm.cur.1.saturating_sub(1),
            Key::Named(NamedKey::ArrowRight) => cm.cur.1 = (cm.cur.1 + 1).min(last_col),
            Key::Named(NamedKey::ArrowUp) => cm.cur.0 = cm.cur.0.saturating_sub(1),
            Key::Named(NamedKey::ArrowDown) => cm.cur.0 = (cm.cur.0 + 1).min(last_row),
            Key::Character(s) => {
                for c in s.chars() {
                    match c {
                        'q' => exit = true,
                        'y' => yank = true,
                        'v' | ' ' => {
                            cm.anchor = if cm.anchor.is_some() { None } else { Some(cm.cur) }
                        }
                        'h' => cm.cur.1 = cm.cur.1.saturating_sub(1),
                        'l' => cm.cur.1 = (cm.cur.1 + 1).min(last_col),
                        'k' => cm.cur.0 = cm.cur.0.saturating_sub(1),
                        'j' => cm.cur.0 = (cm.cur.0 + 1).min(last_row),
                        '0' => cm.cur.1 = 0,
                        '$' => cm.cur.1 = last_col,
                        'g' => cm.cur.0 = 0,
                        'G' => cm.cur.0 = last_row,
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        self.copy_mode = Some(cm);
        if exit {
            self.exit_copy_mode(false);
            return;
        }
        // Keep the cursor on screen: scroll the bound pane when it leaves the view.
        let cur0 = self.copy_mode.as_ref().unwrap().cur.0;
        if let Some(pane) = self.copy_pane_mut() {
            if cur0 < top {
                pane.scroll((top - cur0) as isize);
            } else if cur0 > top + rows.saturating_sub(1) {
                pane.scroll(-((cur0 - (top + rows - 1)) as isize));
            }
        }
        self.sync_copy_selection();
        if yank {
            self.exit_copy_mode(true);
        } else {
            self.window.request_redraw();
        }
    }

    /// Scrolls the focused pane to an absolute scrollback offset, gliding there with
    /// easing when `smooth`, else jumping. `target` is clamped by the grid.
    /// If `pos` is inside the focused pane's minimap strip (right edge), scrolls the
    /// pane to the corresponding scrollback position and returns true.
    fn minimap_jump(&mut self, pos: PhysicalPosition<f64>) -> bool {
        let area = self.active_area();
        let focus = self.tab().focused_ptr();
        let Some((_, r)) = self.tabs[self.active].layout(area).into_iter().find(|(id, _)| *id == focus)
        else {
            return false;
        };
        let strip_w = 46.0;
        let (x, y) = (pos.x as f32, pos.y as f32);
        if x < r.x + r.w - strip_w || x > r.x + r.w || y < r.y || y > r.y + r.h {
            return false;
        }
        let frac = ((y - r.y) / r.h).clamp(0.0, 1.0);
        let pane = self.tab().focused();
        let (total, rows, sb) = {
            let g = pane.grid.lock().unwrap();
            (g.total_rows(), g.rows(), g.total_rows() - g.rows())
        };
        // frac 0 = oldest line, 1 = newest; put that line at the viewport top.
        let target_top = (frac * total as f32) as usize;
        let offset = sb.saturating_sub(target_top);
        let cur = pane.grid.lock().unwrap().display_offset();
        pane.scroll(offset as isize - cur as isize);
        let _ = rows;
        self.window.request_redraw();
        true
    }

    fn glide_focused_to(&mut self, target: f32, smooth: bool) {
        let id = self.tab().focused_ptr();
        let cur = self.tab().focused().grid.lock().unwrap().display_offset() as f32;
        if !smooth || (target - cur).abs() < 1.0 {
            let delta = target as isize - cur as isize;
            self.tab().focused().scroll(delta);
            self.scroll_glide = None;
        } else {
            self.scroll_glide = Some((id, cur, target.max(0.0)));
            self.window.request_redraw();
        }
    }

    /// Max scrollback offset (fully scrolled back) of the focused pane.
    fn focused_scrollback_len(&mut self) -> f32 {
        let g = self.tab().focused().grid.lock().unwrap();
        (g.total_rows() - g.rows()) as f32
    }

    fn toggle_zoom(&mut self) {
        let area = self.active_area();
        if self.zoomed.take().is_some() {
            self.tabs[self.active].reflow(area);
        } else {
            let focus = self.tab().focused_ptr();
            self.zoomed = Some(focus);
            let rect = self.tabs[self.active].full_rect(area);
            self.tabs[self.active].resize_one(focus, rect);
        }
        self.window.request_redraw();
    }

    /// Drops a zoom that can no longer hold: the zoomed pane was closed, lost focus
    /// (a focus move, a split, a tab switch), so it is not the one on screen. Without
    /// this, focus/input would land on a pane the zoom keeps hidden. Called each
    /// frame before laying out.
    fn sync_zoom(&mut self) {
        if let Some(id) = self.zoomed {
            let tab = &self.tabs[self.active];
            if !tab.panes.contains_key(&id) || tab.focus != id {
                self.zoomed = None;
                // Reflow ALL tabs, not just the active one: when the zoom is dropped
                // by switching away, the zoomed pane lives in the PREVIOUS tab, whose
                // grid/PTY are still stretched to full-rect and would overdraw its
                // siblings until the next global reflow.
                self.reflow_all();
            }
        }
    }

    /// Re-stretches the zoomed pane to fill the tab after a reflow (window resize,
    /// font change), so its grid/PTY match what is drawn instead of the small layout
    /// rect reflow gave it.
    fn reapply_zoom(&mut self) {
        if let Some(id) = self.zoomed {
            if self.tabs[self.active].panes.contains_key(&id) {
                let area = self.active_area();
                let rect = self.tabs[self.active].full_rect(area);
                self.tabs[self.active].resize_one(id, rect);
            }
        }
    }

    fn confirm_prompt(&mut self, kind: PromptKind, value: String, config: &Config) {
        match kind {
            PromptKind::RenameTab => {
                self.tab().title_override = Some(value).filter(|s| !s.is_empty());
            }
            PromptKind::QuickConnect => {
                if !value.is_empty() {
                    self.split_running(config, vec!["ssh".into(), value]);
                }
            }
            PromptKind::AiCommand => {
                if !value.is_empty() {
                    self.send_ai_command(value, config);
                }
            }
            PromptKind::Whisper => {
                if !value.is_empty() {
                    self.send_whisper(value, config);
                }
            }
            PromptKind::GuardedCommand => {
                // Confirmed: submit the command that was held back. The line is
                // already typed in the shell, so this is just the Enter we withheld —
                // broadcast it to the group if broadcast is on, exactly as the
                // original keystroke would have gone.
                if self.broadcast {
                    self.broadcast_bytes(b"\r");
                } else {
                    self.tab().focused().write(b"\r");
                }
            }
            PromptKind::HistoryInsert => {
                // Type the chosen history line at the prompt; the user runs it.
                self.insert_command(value);
            }
            PromptKind::WatchKeyword => {
                let watching = !value.trim().is_empty();
                self.tab().focused().set_watch(value);
                self.status = Some(if watching {
                    "watching this pane".into()
                } else {
                    "watch cleared".into()
                });
                self.status_expiry = Some(Instant::now() + Duration::from_secs(2));
            }
            PromptKind::LaunchLayout => {
                if !value.is_empty() {
                    self.launch_layout(value, config);
                }
            }
        }
    }

    // ---- helpers used above --------------------------------------------------

    fn reflow_all(&mut self) {
        let area = self.active_area();
        for tab in &mut self.tabs {
            tab.reflow(area);
        }
        self.reapply_zoom();
        // A relayout moves the cursor's pixel rect without the cursor "moving": drop
        // the trail baseline so it does not spawn a phantom ghost at the old spot.
        self.last_cursor_rect = None;
        self.window.request_redraw();
    }

    /// Reruns the search against the focused pane and updates the match list.
    fn recompute_search(&mut self) {
        let query = match &self.overlay {
            Some(Overlay::Search(s)) => s.query.clone(),
            _ => return,
        };
        let matches = self.tab().focused().grid.lock().unwrap().search(&query);
        if let Some(Overlay::Search(s)) = self.overlay.as_mut() {
            s.set_matches(matches);
        }
    }

    /// Scrolls the focused pane so the current search match is visible.
    fn scroll_to_current_match(&mut self) {
        let target = match &self.overlay {
            Some(Overlay::Search(s)) => s.current_match(),
            _ => None,
        };
        if let Some((abs, _)) = target {
            self.tab().focused().grid.lock().unwrap().scroll_to_abs(abs);
        }
        self.window.request_redraw();
    }

    /// Restores the most recently closed tab, with its layout, working dirs and
    /// scrollback — like a browser's reopen-closed-tab.
    fn reopen_closed(&mut self, config: &Config) {
        let Some(state) = self.closed_tabs.pop() else { return };
        let area = self.active_area();
        let proxy = self.proxy.clone();
        let wake = move |_id| -> Box<dyn Fn() + Send + 'static> {
            let p = proxy.clone();
            Box::new(move || { let _ = p.send_event(UserEvent::Redraw); })
        };
        match Tab::from_session(&state, area, self.renderer.cell_size(), config, wake) {
            Ok(tab) => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
                self.reflow_all();
            }
            Err(e) => eprintln!("runnir: could not reopen tab: {e}"),
        }
    }

    /// Selection mode from click cadence: 1 char, 2 word, 3+ line. Two clicks count
    /// as a double only when they land on the same cell within the double-click
    /// window; otherwise the counter resets.
    fn click_mode(&mut self, point: selection::Point) -> SelMode {
        let now = Instant::now();
        let quick = now.duration_since(self.last_click.0) < Duration::from_millis(400);
        if quick && self.last_click.1 == point {
            self.click_count += 1;
        } else {
            self.click_count = 1;
        }
        self.last_click = (now, point);
        match self.click_count {
            2 => SelMode::Word,
            n if n >= 3 => SelMode::Line,
            _ => SelMode::Char,
        }
    }

    /// Which tab, if any, was clicked in the tab bar. Mirrors the label layout in
    /// `build_chrome` so the hit-test matches what is drawn.
    fn tab_bar_hit(&self, pos: PhysicalPosition<f64>) -> Option<usize> {
        if self.tabs.len() <= 1 {
            return None; // No bar shown with a single tab.
        }
        let (cw, ch) = self.renderer.cell_size();
        if (pos.y as f32) >= ch {
            return None; // Below the one-row bar.
        }
        let click_col = (pos.x as f32 / cw).floor() as usize;
        let cols = (self.surface_config.width as f32 / cw).floor().max(1.0) as usize;
        let (offset, avail_end) = self.tab_scroll(cols);
        // A click in the right-reserved region (broadcast/context tags) is not a tab.
        if click_col >= avail_end {
            return None;
        }
        // Un-scroll the click into label space and find the tab it lands on.
        let mut x = 1;
        for i in 0..self.tabs.len() {
            let w = Self::label_w(&self.tab_label(i));
            let drawn = (x as isize - offset as isize) as isize;
            if drawn >= 1 && (drawn as usize) < avail_end {
                let d = drawn as usize;
                if click_col >= d && click_col < (d + w).min(avail_end) {
                    return Some(i);
                }
            }
            x += w + 1;
        }
        None
    }

    /// Display width (cells) of a string, honouring wide (CJK) glyphs so tab layout,
    /// click hit-testing and badge placement all agree with what `write_str` draws.
    fn label_w(s: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(s)
    }

    /// The label drawn for tab `i` in the bar: " <icon> N title <badge> ". The icon is
    /// a nerd-font glyph for the foreground app; the badge is a dot for a background
    /// tab with unseen output, or a red dot if its last command failed.
    fn tab_label(&self, i: usize) -> String {
        let tab = &self.tabs[i];
        let icon = tab_icon(&tab.proc_name());
        let badge = match self.tab_badge(i) {
            Some((ch, _)) => format!(" {ch}"),
            None => String::new(),
        };
        format!(" {icon} {} {}{badge} ", i + 1, tab.title())
    }

    /// The status badge for tab `i` (char + colour), or `None`: a red ✗ if its last
    /// command failed, else an amber ● if a background tab has unseen output.
    fn tab_badge(&self, i: usize) -> Option<(char, (u8, u8, u8))> {
        let tab = &self.tabs[i];
        if tab.failed() {
            Some(('\u{2717}', (0xe0, 0x4f, 0x4f)))
        } else if i != self.active && tab.has_activity() {
            Some(('\u{25cf}', (0xe8, 0xb3, 0x39)))
        } else {
            None
        }
    }

    /// Cells reserved on the right of the tab bar for the broadcast / context tags,
    /// so the scrollable tab region does not run under them.
    fn tab_right_reserved(&self) -> usize {
        let mut r = 0;
        if self.broadcast {
            r += " BROADCAST ".len() + 1;
        }
        if let Some(label) = self.tabs[self.active].focused_ref().context.label() {
            r += Self::label_w(&format!(" {label} ")) + 1;
        }
        r
    }

    /// Horizontal scroll of the tab bar so the active tab is always visible when the
    /// tabs overflow. Returns `(offset_cells, avail_end_col)`: draw tab `i` at its
    /// natural x minus `offset`, clipped to `[1, avail_end)`.
    fn tab_scroll(&self, cols: usize) -> (usize, usize) {
        let avail_end = cols.saturating_sub(self.tab_right_reserved()).max(2);
        // Natural start column of each tab (1-based, gap of 1 between).
        let mut starts = Vec::with_capacity(self.tabs.len());
        let mut x = 1usize;
        for i in 0..self.tabs.len() {
            starts.push(x);
            x += Self::label_w(&self.tab_label(i)) + 1;
        }
        let total_end = x; // one past the last tab
        if total_end <= avail_end {
            return (0, avail_end); // everything fits, no scroll
        }
        let active = self.active.min(self.tabs.len() - 1);
        let aw = Self::label_w(&self.tab_label(active));
        let a_end = starts[active] + aw;
        // Show the active tab and as many preceding tabs as fit before its right edge.
        let mut first = active;
        while first > 0 && a_end.saturating_sub(starts[first - 1] - 1) <= avail_end {
            first -= 1;
        }
        (starts[first] - 1, avail_end)
    }

    fn pane_at(&self, pos: PhysicalPosition<f64>, area: Rect) -> Option<(u64, Rect)> {
        let (px, py) = (pos.x as f32, pos.y as f32);
        self.visible_rects(area)
            .into_iter()
            .find(|(_, r)| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h)
    }

    /// Recomputes which URL/path sits under the pointer (D14). Returns whether the
    /// hovered target changed, so the caller only repaints when the underline moves.
    fn update_hover(&mut self, pos: PhysicalPosition<f64>) -> bool {
        let prev = self.hover_url.clone();
        self.hover_url = None;
        if self.overlay.is_none() {
            let area = self.active_area();
            if let Some((id, rect)) = self.pane_at(pos, area) {
                if let Some((abs_row, col)) = self.point_in(id, rect, pos) {
                    let grid = self.tabs[self.active].panes[&id].grid.lock().unwrap();
                    // A real OSC 8 hyperlink on the cell wins over text detection: it
                    // carries an explicit URI the app declared, not a guess.
                    if let Some((start, len, uri)) = grid.link_span(abs_row, col) {
                        self.hover_url = Some(HoverUrl {
                            pane: id,
                            abs_row,
                            col: start,
                            len,
                            text: uri,
                            kind: crate::overlay::HintKind::Url,
                        });
                        return self.hover_url != prev;
                    }
                    for h in crate::hints::find(&grid) {
                        // Display width (not char count): a wide glyph spans two grid
                        // cells, so the underline and hit-zone must too.
                        let len = unicode_width::UnicodeWidthStr::width(h.text.as_str()).max(1);
                        if h.abs_row == abs_row && col >= h.col && col < h.col + len {
                            self.hover_url = Some(HoverUrl {
                                pane: id,
                                abs_row,
                                col: h.col,
                                len,
                                text: h.text,
                                kind: h.kind,
                            });
                            break;
                        }
                    }
                }
            }
        }
        self.hover_url != prev
    }

    /// Acts on the hovered URL/path if the pointer is over one: opens a URL in the
    /// browser, copies a path or hash. Returns whether it consumed the click.
    fn open_hover(&mut self) -> bool {
        // Recompute against the pointer's current position first: a keyboard tab
        // switch or a scroll can leave a stale target under the old coordinates.
        self.update_hover(self.cursor_px);
        let Some(h) = self.hover_url.clone() else { return false };
        crate::hints::act(&h.text, h.kind, &mut self.clipboard);
        true
    }

    fn point_in(&self, id: u64, rect: Rect, pos: PhysicalPosition<f64>) -> Option<selection::Point> {
        let (cw, ch) = self.renderer.cell_size();
        let col = (((pos.x as f32 - rect.x) / cw).floor().max(0.0)) as usize;
        let row = (((pos.y as f32 - rect.y) / ch).floor().max(0.0)) as usize;
        let pane = self.tabs[self.active].panes.get(&id)?;
        let grid = pane.grid.lock().unwrap();
        let row = row.min(grid.rows().saturating_sub(1));
        // With folds active a screen row maps through the display plan; a click on a
        // fold summary or blank padding is not a real cell (returns None).
        let abs = if grid.has_folds() {
            match grid.display_plan().get(row) {
                Some(crate::grid::PlanRow::Real(a)) => *a,
                _ => return None,
            }
        } else {
            grid.abs_row(row)
        };
        Some((abs, col.min(grid.cols().saturating_sub(1))))
    }

    /// If a mouse click at `pos` lands on a fold summary row, the local row to toggle
    /// (unfold). `None` otherwise.
    fn fold_row_at(&self, id: u64, rect: Rect, pos: PhysicalPosition<f64>) -> Option<usize> {
        let (_, ch) = self.renderer.cell_size();
        let row = (((pos.y as f32 - rect.y) / ch).floor().max(0.0)) as usize;
        let pane = self.tabs[self.active].panes.get(&id)?;
        let grid = pane.grid.lock().unwrap();
        if !grid.has_folds() {
            return None;
        }
        match grid.display_plan().get(row) {
            Some(crate::grid::PlanRow::Fold { local, .. }) => Some(*local),
            _ => None,
        }
    }

    fn jump_prompt(&mut self, dir: isize, smooth: bool) {
        let target = {
            let grid = self.tab().focused().grid.lock().unwrap();
            let offsets = grid.prompt_offsets();
            if offsets.is_empty() {
                return;
            }
            let current = grid.display_offset();
            // Offsets are how far back each prompt sits; pick the next one in `dir`.
            if dir < 0 {
                offsets.iter().copied().filter(|&o| o > current).min()
            } else {
                offsets.iter().copied().filter(|&o| o < current).max()
            }
        };
        if let Some(t) = target {
            self.glide_focused_to(t as f32, smooth);
        }
    }

    fn broadcast_bytes(&mut self, bytes: &[u8]) {
        // If any pane is a group member, broadcast is scoped to the group; with no
        // members it falls back to every pane (the simple whole-tab broadcast).
        let scoped = self.tab().panes.values().any(|p| p.in_group);
        for pane in self.tab().panes.values_mut() {
            if !scoped || pane.in_group {
                pane.write(bytes);
            }
        }
    }

    /// Toggles the focused pane's membership in the broadcast group (D8).
    fn toggle_broadcast_group(&mut self) {
        let member = {
            let p = self.tab().focused();
            p.in_group = !p.in_group;
            p.in_group
        };
        self.status = Some(if member {
            "pane added to broadcast group".into()
        } else {
            "pane removed from broadcast group".into()
        });
        self.status_expiry = Some(Instant::now() + Duration::from_secs(2));
        self.window.request_redraw();
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.tabs[self.active].focused_ref().selection_text() {
            // Also seed the PRIMARY selection so middle-click pastes it, matching
            // the X11/Wayland convention that selecting text makes it available.
            self.clipboard.set_primary(&text);
            self.set_clipboard(text);
        }
    }

    fn set_clipboard(&mut self, text: String) {
        self.clipboard.set(&text);
    }

    fn paste(&mut self) {
        if let Some(text) = self.clipboard.get() {
            self.paste_text(text);
        }
    }

    /// Middle-click paste: uses the PRIMARY selection (the last text selected),
    /// falling back to the clipboard where primary is unavailable.
    fn paste_primary(&mut self) {
        if let Some(text) = self.clipboard.get_primary() {
            self.paste_text(text);
        }
    }

    fn paste_text(&mut self, text: String) {
        // Sanitize the payload: drop ESC and every other C0 control byte except tab,
        // newline and carriage return. This removes the bracketed-paste end marker
        // (`ESC[201~`) a hostile clipboard/PRIMARY might carry — and does so without
        // the single-pass `replace` splicing trap (`ESC[2`+`ESC[201~`+`01~` would
        // re-form a marker), since no ESC survives at all to start any sequence.
        let text: String = text
            .chars()
            .filter(|&c| c == '\t' || c == '\n' || c == '\r' || !c.is_control())
            .collect();
        let bracketed = self.tab().focused().bracketed_paste();
        let pane = self.tab().focused();
        if bracketed {
            pane.write(b"\x1b[200~");
            pane.write(text.as_bytes());
            pane.write(b"\x1b[201~");
        } else {
            pane.write(text.as_bytes());
        }
    }

    /// Applies a freshly-loaded config live (hot-reload): theme, opacity and font.
    /// Key bindings are rebuilt by the caller (they live on `App`, not `Gpu`).
    fn apply_config(&mut self, config: &Config) {
        // A status-bar toggle changes the content height, so reflow after.
        if self.status_bar != config.window.status_bar {
            self.status_bar = config.window.status_bar;
            self.reflow_all();
        }
        self.renderer.set_theme(config.theme.clone());
        // Opacity only when the surface composites with alpha; on an opaque surface
        // it would darken rather than reveal, same guard as at startup.
        self.renderer
            .set_opacity(if self.translucent { config.window.opacity } else { 1.0 });
        crate::load_background(config, &self.device, &self.queue, &mut self.renderer);
        // Rebuild the font only when the CONFIG's font actually changed (family, size
        // or ligatures) — compared against what the config last asked for, not the
        // live size, so a colour-only edit does not snap a runtime zoom back, and a
        // family/ligature change (same size) is applied.
        let want = (config.font.family.clone(), config.font.size, config.font.ligatures);
        if want != self.applied_font {
            match FontAtlas::new_with(&config.font.family, config.font.size) {
                Ok(mut font) => {
                    font.ligatures = config.font.ligatures;
                    self.renderer.replace_font(&self.device, font);
                    self.font_px = config.font.size;
                    // Only record success: a failed load must be retried on the next
                    // save, not remembered as if it applied.
                    self.applied_font = want;
                    self.reflow_all();
                }
                Err(e) => {
                    self.status = Some(format!("font '{}' failed: {e}", config.font.family));
                    self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
                }
            }
        }
        self.window.request_redraw();
    }

    fn set_font_px(&mut self, px: f32, config: &Config) {
        let px = px.clamp(6.0, 72.0);
        if (px - self.font_px).abs() < 0.5 {
            return;
        }
        if let Ok(mut font) = FontAtlas::new_with(&config.font.family, px) {
            font.ligatures = config.font.ligatures;
            self.renderer.replace_font(&self.device, font);
            self.font_px = px;
            self.reflow_all();
        }
    }

    /// Dumps the focused pane's scrollback to a temp file and opens it in $EDITOR
    /// (or $VISUAL, else vi) in a new split — for searching, copying or saving long
    /// output with a real editor instead of the terminal's own scrollback.
    fn open_scrollback_in_editor(&mut self, config: &Config) {
        let text = self.tab().focused().scrollback_text().join("\n");
        // A per-pane filename (the pty pid) so repeated dumps of the same pane reuse
        // one path and a fresh dump overwrites the stale one.
        let pid = self.tab().focused().pty_pid().unwrap_or(0);
        // Prefer $XDG_RUNTIME_DIR (a per-user 0700 dir) over world-writable /tmp, so a
        // predictable filename can't be pre-empted by a symlink from another user, and
        // scrollback (which may hold secrets) isn't left world-readable.
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("runnir-scrollback-{pid}.txt"));
        if let Err(e) = write_private(&path, text.as_bytes()) {
            self.status = Some(format!("could not write scrollback: {e}"));
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        }
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "vi".into());
        // $EDITOR may carry args (e.g. "code -w"); split on whitespace and append the
        // file. Not a full shell parse, but it covers the common cases.
        let mut argv: Vec<String> = editor.split_whitespace().map(str::to_string).collect();
        argv.push(path.to_string_lossy().into_owned());
        self.split_running(config, argv);
    }

    /// Opens the layout picker (W3): choose a named layout from the config and it
    /// launches a fresh tab split into one pane per command.
    fn open_layout_picker(&mut self, config: &Config) {
        if config.layouts.is_empty() {
            self.status = Some("no [[layouts]] configured — see the docs".into());
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        }
        let names: Vec<String> = config.layouts.iter().map(|l| l.name.clone()).collect();
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::LaunchLayout,
            "Launch layout",
            names,
        )));
        self.window.request_redraw();
    }

    /// Launches a named layout: a new tab split into one pane per command. Splits
    /// alternate axis so several commands tile rather than stacking one way.
    fn launch_layout(&mut self, name: String, config: &Config) {
        let Some(layout) = config.layouts.iter().find(|l| l.name == name).cloned() else {
            return;
        };
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        let cmds = if layout.commands.is_empty() {
            vec![String::new()]
        } else {
            layout.commands.clone()
        };

        // First command opens the new tab's initial pane.
        let id = self.new_pane_id();
        let first = argv_of(&cmds[0]);
        let spawn = Spawn { command: (!first.is_empty()).then_some(first), cwd: None };
        let wake = wake_fn(self.proxy.clone());
        let Ok(mut tab) = Tab::new(area, cell, config, id, &spawn, wake) else { return };
        tab.title_override = Some(layout.name.clone());
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;

        // The rest become splits of the newest pane, alternating axis for a tile.
        for (i, cmd) in cmds.iter().enumerate().skip(1) {
            let axis = if i % 2 == 1 { Axis::Vertical } else { Axis::Horizontal };
            let id = self.new_pane_id();
            let wake = wake_fn(self.proxy.clone());
            let _ = self.tab().split_running_with_id(area, axis, config, id, argv_of(cmd), wake);
        }
        // A split silently no-ops when the window is too small, so a command may have
        // been dropped: tell the user rather than leaving a connection missing.
        let got = self.tab().panes.len();
        if got < cmds.len() {
            self.status = Some(format!(
                "layout truncated: only {got} of {} panes fit — resize and relaunch",
                cmds.len()
            ));
            self.status_expiry = Some(Instant::now() + Duration::from_secs(5));
        }
        self.reflow_all();
        self.window.request_redraw();
    }

    fn split_running(&mut self, config: &Config, command: Vec<String>) {
        let area = self.active_area();
        let id = self.new_pane_id();
        let wake = wake_fn(self.proxy.clone());
        let _ = self.tab().split_running_with_id(area, Axis::Horizontal, config, id, command, wake);
        self.window.request_redraw();
    }

    fn open_quick_connect(&mut self) {
        let hosts = ssh_hosts();
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::QuickConnect,
            "SSH connect to",
            hosts,
        )));
    }

    fn launch_claude(&mut self, config: &Config) {
        let cmd = ai::claude_launch_command(config);
        self.split_running(config, cmd);
    }
}

fn wheel_lines(delta: MouseScrollDelta, wheel_lines: f32, cell_h: f32) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y * wheel_lines,
        MouseScrollDelta::PixelDelta(p) => p.y as f32 / cell_h.max(1.0),
    }
}

/// Writes `data` to `path` with 0600 permissions, truncating an existing file we
/// own. `O_NOFOLLOW` refuses to follow a symlink planted at the path, so a shared
/// temp dir cannot be used to clobber another file through us.
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc_o_nofollow())
        .open(path)?;
    f.write_all(data)
}

/// `O_NOFOLLOW` without a libc dependency. The value is stable across Linux archs.
fn libc_o_nofollow() -> i32 {
    0o400000
}

/// A nerd-font icon for a foreground process name (the default font is a Nerd Font).
/// Falls back to a generic terminal glyph.
fn tab_icon(name: &str) -> char {
    let n = name.to_ascii_lowercase();
    let base = n.rsplit('/').next().unwrap_or(&n);
    match base {
        "vim" | "nvim" | "vi" => '\u{e62b}',
        "git" | "lazygit" | "tig" => '\u{f1d3}',
        "ssh" | "mosh" => '\u{f233}',
        "docker" | "podman" | "kubectl" => '\u{f308}',
        "python" | "python3" | "ipython" => '\u{e606}',
        "node" | "npm" | "pnpm" | "yarn" | "deno" | "bun" => '\u{e718}',
        "cargo" | "rustc" | "rust" | "rustup" => '\u{e7a8}',
        "htop" | "btop" | "top" | "glances" => '\u{f080}',
        "claude" | "aichat" | "ollama" => '\u{f544}',
        "fish" | "bash" | "zsh" | "sh" | "dash" | "shell" => '\u{f489}',
        _ => '\u{f120}',
    }
}

/// Splits a layout command string into an argv on whitespace. Not a shell parse —
/// good enough for `ssh host`, `htop`, `journalctl -f`; an empty string yields an
/// empty argv, which spawns the default shell.
fn argv_of(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_string).collect()
}

/// Key hints for the palette: action id -> the first chord bound to it.
fn keyhints() -> std::collections::HashMap<String, String> {
    crate::actions::default_hints()
}

/// SSH hosts from `~/.ssh/config`, for quick connect.
fn ssh_hosts() -> Vec<String> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(home.join(".ssh/config")) else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Host ").or_else(|| line.strip_prefix("host ")) {
            for h in rest.split_whitespace() {
                // Skip wildcard patterns: they are not connectable names.
                if !h.contains('*') && !h.contains('?') {
                    hosts.push(h.to_string());
                }
            }
        }
    }
    hosts.sort();
    hosts.dedup();
    hosts
}
