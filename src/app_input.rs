// Input handling for `Gpu`. Included into main.rs (crate root), not a module, so
// it shares the imports there.

impl Gpu {
    fn on_wheel(&mut self, delta: MouseScrollDelta, config: &Config, mods: ModifiersState) {
        // While an overlay owns input, the wheel scrolls it, not the terminal.
        if let Some(ov) = self.overlay.as_mut() {
            let lines = wheel_lines(delta, config.behaviour.wheel_lines, 1.0);
            if let Overlay::Docs(d) = ov {
                d.scroll(-lines as isize);
            }
            self.window.request_redraw();
            return;
        }
        // A mouse-mode app (unless Shift is held) gets the wheel as button events.
        let lines = wheel_lines(delta, config.behaviour.wheel_lines, self.renderer.cell_size().1);
        if !mods.shift_key() && self.forward_wheel(lines) {
            return;
        }
        if self.tab().focused().scroll(lines as isize) {
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

    fn on_click(&mut self, state: ElementState, button: MouseButton, mods: ModifiersState) {
        // Left release always ends a divider drag, even over an overlay.
        if state == ElementState::Released && button == MouseButton::Left {
            self.resizing = None;
        }
        if self.overlay.is_some() {
            return;
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
            (ElementState::Pressed, MouseButton::Middle) => self.paste(),
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
            && !self.broadcast
        {
            let line = self.tab().focused().grid.lock().unwrap().current_command_text();
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
                let rows = self.tab().focused().grid.lock().unwrap().rows() as isize;
                self.tab().focused().scroll(rows);
            }
            Action::ScrollPageDown => {
                let rows = self.tab().focused().grid.lock().unwrap().rows() as isize;
                self.tab().focused().scroll(-rows);
            }
            Action::ScrollToTop => {
                self.tab().focused().scroll(isize::MAX / 2);
            }
            Action::ScrollToBottom => {
                self.tab().focused().snap_to_bottom();
            }
            Action::ScrollUp => {
                self.tab().focused().scroll(3);
            }
            Action::ScrollDown => {
                self.tab().focused().scroll(-3);
            }
            Action::JumpPrevPrompt => self.jump_prompt(-1),
            Action::JumpNextPrompt => self.jump_prompt(1),
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
            Action::QuickConnect => self.open_quick_connect(),
            Action::HintMode => self.open_hints(),
            Action::LaunchClaude => self.launch_claude(config),
            Action::Whisper => self.whisper(),
            Action::ToggleBroadcast => self.broadcast = !self.broadcast,
            Action::ToggleZoom => self.toggle_zoom(),
            Action::ClearSelectionOrScrollback => {
                if !self.tab().focused().clear_selection() {
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
                self.tab().focused().scroll(isize::MAX / 2);
            }
            Action::ScrollToBottom => {
                self.tab().focused().snap_to_bottom();
            }
            Action::JumpPrevPrompt => self.jump_prompt(-1),
            Action::JumpNextPrompt => self.jump_prompt(1),
            Action::FontBigger => self.set_font_px(self.font_px + 1.0, config),
            Action::FontSmaller => self.set_font_px(self.font_px - 1.0, config),
            Action::FontReset => self.set_font_px(config.font.size, config),
            Action::ToggleBroadcast => self.broadcast = !self.broadcast,
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
        self.tabs.swap(self.active, to);
        self.active = to;
        self.window.request_redraw();
    }

    /// Zooms the focused pane to fill the tab, or unzooms. Resizes its PTY so the
    /// program sees the bigger size, and restores every pane on unzoom.
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
                // already typed in the shell, so this is just the Enter we withheld.
                self.tab().focused().write(b"\r");
            }
            PromptKind::HistoryInsert => {
                // Type the chosen history line at the prompt; the user runs it.
                self.insert_command(value);
            }
        }
    }

    // ---- helpers used above --------------------------------------------------

    fn reflow_all(&mut self) {
        let area = self.active_area();
        for tab in &mut self.tabs {
            tab.reflow(area);
        }
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
        let mut x = 1;
        for (i, tab) in self.tabs.iter().enumerate() {
            let label_len = format!(" {} {} ", i + 1, tab.title()).chars().count();
            if click_col >= x && click_col < x + label_len {
                return Some(i);
            }
            x += label_len + 1;
        }
        None
    }

    fn pane_at(&self, pos: PhysicalPosition<f64>, area: Rect) -> Option<(u64, Rect)> {
        let (px, py) = (pos.x as f32, pos.y as f32);
        self.visible_rects(area)
            .into_iter()
            .find(|(_, r)| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h)
    }

    fn point_in(&self, id: u64, rect: Rect, pos: PhysicalPosition<f64>) -> Option<selection::Point> {
        let (cw, ch) = self.renderer.cell_size();
        let col = (((pos.x as f32 - rect.x) / cw).floor().max(0.0)) as usize;
        let row = (((pos.y as f32 - rect.y) / ch).floor().max(0.0)) as usize;
        let pane = self.tabs[self.active].panes.get(&id)?;
        let grid = pane.grid.lock().unwrap();
        let row = row.min(grid.rows().saturating_sub(1));
        Some((grid.abs_row(row), col.min(grid.cols().saturating_sub(1))))
    }

    fn jump_prompt(&mut self, dir: isize) {
        let pane = self.tab().focused();
        let mut grid = pane.grid.lock().unwrap();
        let offsets = grid.prompt_offsets();
        if offsets.is_empty() {
            return;
        }
        let current = grid.display_offset();
        // Offsets are how far back each prompt sits; pick the next one in `dir`.
        let target = if dir < 0 {
            offsets.iter().copied().filter(|&o| o > current).min()
        } else {
            offsets.iter().copied().filter(|&o| o < current).max()
        };
        if let Some(t) = target {
            let delta = t as isize - current as isize;
            grid.scroll_display(delta);
        }
    }

    fn broadcast_bytes(&mut self, bytes: &[u8]) {
        for pane in self.tab().panes.values_mut() {
            pane.write(bytes);
        }
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.tabs[self.active].focused_ref().selection_text() {
            self.set_clipboard(text);
        }
    }

    fn set_clipboard(&mut self, text: String) {
        self.clipboard.set(&text);
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.get() else {
            return;
        };
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
        let path = std::env::temp_dir().join(format!("runnir-scrollback-{pid}.txt"));
        if let Err(e) = std::fs::write(&path, text) {
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
