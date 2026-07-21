// Input handling for `Gpu`. Included into main.rs (crate root), not a module, so
// it shares the imports there.

impl Gpu {
    /// What is running in the window right now, one entry per pane, as `tab n: cmd`.
    ///
    /// A pane sitting at its shell prompt reports the shell itself as the foreground
    /// process; that is not work, so shells are filtered out. What is left is what
    /// closing the window would kill.
    pub fn running_commands(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            for pane in tab.panes.values() {
                let Some(fg) = pane.pty.foreground() else { continue };
                if is_shell(&fg.name) {
                    continue;
                }
                // The whole command line, not just the name: `claude` and `cargo
                // build` are both "a process", but only the argv says which one you
                // are about to kill.
                let line = if fg.argv.is_empty() { fg.name.clone() } else { fg.argv.join(" ") };
                out.push(format!("tab {}: {}", i + 1, line));
            }
        }
        out
    }

    /// Whether the window may close now.
    ///
    /// With `behaviour.confirm_close` on and something still running, the question
    /// goes on screen and this answers `false` — the caller must not exit. Nothing
    /// running (or the setting off) closes straight away: a confirm that fires on an
    /// idle shell is a confirm people learn to dismiss without reading.
    pub fn request_close(&mut self, config: &Config) -> bool {
        if !config.behaviour.confirm_close {
            return true;
        }
        // Already asking: a second click on the window's close button must not
        // stack another prompt, and must not be taken as an answer either.
        if matches!(&self.overlay, Some(Overlay::Prompt(p)) if p.kind == PromptKind::ConfirmQuit) {
            return false;
        }
        let running = self.running_commands();
        if running.is_empty() {
            return true;
        }
        let label = if running.len() == 1 {
            "Close runnir? 1 command is still running".to_string()
        } else {
            format!("Close runnir? {} commands are still running", running.len())
        };
        // Asked over whatever else is open — the question is about the window, not
        // about that overlay — but the displaced one is kept rather than dropped:
        // answering "no" has to leave the screen as it was, and a settings panel
        // mid-edit or a half-typed prompt is work too.
        self.overlay_under_confirm = self.overlay.take();
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::ConfirmQuit,
            &label,
            running.into_iter().take(6).collect(),
        )));
        self.window.request_redraw();
        false
    }

    /// Puts a confirm away, restoring whatever it was asked over.
    fn dismiss_confirm(&mut self) {
        self.overlay = self.overlay_under_confirm.take();
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta, config: &Config, mods: ModifiersState) {
        let cell_h = self.renderer.cell_size().1;
        // While an overlay owns input, the wheel scrolls it, not the terminal. Use
        // the real cell height so a touchpad's pixel deltas map to sane line counts.
        if self.overlay.is_some() {
            let lines = wheel_lines(delta, config.behaviour.wheel_lines, cell_h);
            // Over the panel's list the wheel moves the selection; over its diff it
            // scrolls the diff. Which one you get follows the pointer, the only
            // reading that matches what is under it.
            let over_list = self.git_pointer_over_list(self.cursor_px);
            let over_files = self.git_pointer_over_files(self.cursor_px);
            match self.overlay.as_mut() {
                Some(Overlay::Docs(d)) => d.scroll(-lines.round() as isize),
                Some(Overlay::Git(p)) => {
                    let step = -lines.round() as i32;
                    if over_files {
                        let cur = p.files_cursor() as i32 + step;
                        p.set_files_cursor(cur.max(0) as usize);
                    } else if over_list {
                        let cur = p.cursor() as i32 + step;
                        p.set_cursor(cur.max(0) as usize);
                    } else {
                        p.scroll_preview(step);
                    }
                }
                _ => {}
            }
            if over_list {
                self.git_preview();
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
        // The explorer's edge being dragged, for as long as the button is held.
        if self.explorer_resizing {
            self.explorer_drag(position);
            return;
        }
        // A git panel column separator being dragged, for as long as the button is
        // held — the same contract the pane dividers have.
        if self.git_drag.is_some() {
            self.git_drag_split(position);
            return;
        }
        // Over a git panel separator the pointer says so: a column you can drag with
        // no sign that you can is a column nobody ever drags.
        {
            let (cols, rows, col, row) = self.cell_at(position);
            // Closing the panel counts as "not over one", or the resize cursor
            // outlives the thing it was pointing at.
            let over = match &self.overlay {
                Some(Overlay::Git(p)) => p.separator_at(cols, rows, col, row).is_some(),
                _ => false,
            };
            if over != self.git_over_split {
                self.git_over_split = over;
                let icon = if over {
                    winit::window::CursorIcon::ColResize
                } else {
                    winit::window::CursorIcon::Default
                };
                self.window.set_cursor(icon);
            }
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
            self.git_drag = None;
            if self.explorer_resizing {
                // The panes learn their new size once, here: a PTY resized on every
                // frame of a drag is how a full-screen program ends up redrawing
                // itself into a corner.
                self.explorer_resizing = false;
                let area = self.active_area();
                self.tabs[self.active].reflow(area);
                self.window.request_redraw();
            }
        }
        // The git panel takes the mouse: it is a list and a diff, and both are
        // things people point at. Every other overlay still swallows clicks.
        if matches!(self.overlay, Some(Overlay::Git(_))) {
            if state == ElementState::Pressed && button == MouseButton::Left {
                self.git_panel_click(self.cursor_px, config);
            }
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        // A mouse press leaves copy-mode (keyboard mode) before it can redirect focus
        // onto another pane, which would otherwise strand its selection.
        if state == ElementState::Pressed && self.copy_mode.is_some() {
            self.exit_copy_mode(false);
        }
        // A left press in the focused pane's minimap strip jumps to that position.
        if state == ElementState::Pressed && button == MouseButton::Left && config.window.minimap {
            if self.minimap_jump(self.cursor_px) {
                return;
            }
        }

        // The explorer sidebar: its edge starts a resize, a row selects, and a click
        // anywhere in it moves the keyboard there. Checked before the panes, since
        // the sidebar sits outside their area and they would never see it anyway.
        if state == ElementState::Pressed && button == MouseButton::Left && self.overlay.is_none() {
            if self.explorer_edge_at(self.cursor_px) {
                self.explorer_resizing = true;
                return;
            }
            if let Some(row) = self.explorer_row_at(self.cursor_px) {
                let body = self.explorer_body_rows();
                let mut open = false;
                if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                    e.focused = true;
                    if let Some(i) = row {
                        // Clicking the row that is already selected opens it, the way
                        // the git panel and every file manager work.
                        open = e.cursor == i;
                        e.set_cursor(i, body);
                    }
                }
                if open {
                    self.explorer_key(&Key::Named(NamedKey::Enter), config);
                }
                self.window.request_redraw();
                return;
            }
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
            && self.open_hover(config)
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
                        // Alt (kitty default) or Ctrl held on the press starts a
                        // rectangular block selection; otherwise the click cadence
                        // picks char/word/line (double-click a word, triple a line).
                        // We reach this arm only when the click was NOT forwarded to a
                        // mouse-mode app (no mouse mode, or Shift held to override), so
                        // block never fights mouse forwarding and Shift-select still
                        // works — Shift+Alt/Shift+Ctrl block-selects inside such apps.
                        let mode = if mods.alt_key() || mods.control_key() {
                            SelMode::Block
                        } else {
                            self.click_mode(point)
                        };
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
        // Key-release events only reach the child under the kitty keyboard protocol
        // (the "report event types" flag); they never drive overlays, copy-mode, or
        // chords. Everything below this point is press-only.
        if !event.state.is_pressed() {
            let flags = self.tab().focused().keyboard_flags();
            if flags & keys::KITTY_REPORT_EVENTS != 0 && self.overlay.is_none() && self.copy_mode.is_none()
            {
                if let Some(bytes) = keys::encode_kitty(&event, mods, flags, true) {
                    self.write_key_bytes(&bytes);
                }
            }
            return;
        }

        // An overlay swallows all keys while open.
        if self.overlay.is_some() {
            self.overlay_key(&event.logical_key, mods, config);
            return;
        }

        // Copy-mode owns the keyboard: vim motions drive a virtual cursor/selection.
        if self.copy_mode.is_some() {
            self.copy_mode_key(&event, mods);
            return;
        }

        // With the tree focused its own leader answers first: the same chord, a tree
        // of file verbs instead of the window's.
        if self.explorer_leader_key(&event.logical_key, mods, config) {
            return;
        }
        if self.leader_key(&event.logical_key, mods, config, keymap, event_loop) {
            return;
        }

        // The sidebar takes the keyboard only while it has focus, and only after the
        // leader layer and the bound chords: a tree that swallowed ctrl+shift+t would
        // be a mode, and this is chrome.
        if self.explorer_focused() && !mods.control_key() && !mods.alt_key() && !mods.super_key() {
            if self.explorer_key(&event.logical_key, config) {
                return;
            }
        }

        // The XF86 media transport keys drive the media backend directly, wherever the
        // focus is (no overlay needed). Volume media keys are left to the system.
        if let Key::Named(n) = &event.logical_key {
            let media = match n {
                NamedKey::MediaPlayPause => Some(Action::MediaPlayPause),
                NamedKey::MediaTrackNext => Some(Action::MediaNext),
                NamedKey::MediaTrackPrevious => Some(Action::MediaPrev),
                _ => None,
            };
            if let Some(a) = media {
                self.run_action(a, config, event_loop);
                return;
            }
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

        // Otherwise it is input for the focused pane's process. Under the kitty
        // keyboard protocol the pane advertises non-zero flags and keys are encoded
        // as CSI-u; otherwise the byte-identical legacy encoding is used.
        let flags = self.tab().focused().keyboard_flags();
        let bytes = if flags != 0 {
            keys::encode_kitty(&event, mods, flags, false)
        } else {
            let mode = keys::KeyMode { app_cursor: self.tab().focused().app_cursor() };
            keys::encode(&event, mods, mode)
        };
        if let Some(bytes) = bytes {
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
            self.write_key_bytes(&bytes);
        }
    }

    /// Sends encoded key bytes to the focused pane (or all panes when broadcasting),
    /// snapping the view to the live output and clearing any selection first.
    fn write_key_bytes(&mut self, bytes: &[u8]) {
        self.scroll_glide = None;
        if self.tab().focused().snap_to_bottom() {
            self.window.request_redraw();
        }
        self.tab().focused().clear_selection();
        if self.broadcast {
            self.broadcast_bytes(bytes);
        } else {
            self.tab().focused().write(bytes);
        }
    }

    fn run_action(&mut self, action: Action, config: &Config, event_loop: &ActiveEventLoop) {
        let area = self.active_area();
        let wake = wake_fn(self.proxy.clone());
        match action {
            Action::Quit => {
                if !self.request_close(config) {
                    return;
                }
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
                    // Closing the last tab IS closing the window; it asks the same
                    // question, or a habit of ctrl+w would still kill running work.
                    if !self.request_close(config) {
                        return;
                    }
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
                    if !self.request_close(config) {
                        return;
                    }
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
            Action::FocusNext => self.tab().focus_next(area),
            Action::CycleLayout => self.cycle_layout(area),

            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste(),
            Action::ClipboardHistory => self.open_clip_history(),
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
            Action::OpenConfig => {
                self.overlay = Some(Overlay::Config(overlay::ConfigPanel::new(config.clone())));
            }
            Action::OpenThemePicker => {
                self.overlay =
                    Some(Overlay::Theme(overlay::ThemePicker::new(config.theme.clone())));
            }
            Action::ToggleAi => self.toggle_ai(config),
            Action::AskAiAboutError => self.ask_ai_about_error(config),
            Action::AiCommand => self.ai_command(),
            Action::FixLastCommand => self.fix_last_command(config),
            Action::GitPanel => self.open_git_panel(config),
            Action::AiExplain => self.ai_explain_selection(config),
            Action::SummarizeSession => self.summarize_session(config),
            Action::OpenScrollbackInEditor => self.open_scrollback_in_editor(config),
            Action::PipeLastOutput => self.open_pipe_prompt(PromptKind::PipeLastOutput),
            Action::PipeScrollback => self.open_pipe_prompt(PromptKind::PipeScrollback),
            Action::HistorySearch => self.history_search(),
            Action::WatchKeyword => self.watch_keyword(),
            Action::LaunchLayout => self.open_layout_picker(config),
            Action::OpenSnippets => self.open_snippet_picker(config),
            Action::CopyMode => self.enter_copy_mode(),
            Action::FoldOutput => self.tab().focused().toggle_fold_all(),
            Action::ToggleImageWatch => self.toggle_image_watch(config),
            Action::ToggleExplorer => self.toggle_explorer(config),
            Action::SetImageWatchDir => self.set_image_watch_dir(),
            Action::SaveProjectSession => self.save_project_session_cmd(),
            Action::RestoreProjectSession => self.restore_project_session_cmd(config),
            Action::NowPlaying => self.open_now_playing(),
            Action::MediaPlayPause => {
                crate::media::play_pause();
                self.toast("play / pause", 1);
            }
            Action::MediaNext => {
                crate::media::next();
                self.toast("next track", 1);
            }
            Action::MediaPrev => {
                crate::media::prev();
                self.toast("previous track", 1);
            }
            Action::MediaVolumeUp => {
                crate::media::volume(true);
                self.toast("volume +", 1);
            }
            Action::MediaVolumeDown => {
                crate::media::volume(false);
                self.toast("volume -", 1);
            }
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

    // ------------------------------------------------------------------
    // File explorer sidebar (explorer.rs). Chrome beside the panes: it takes the
    // keyboard only while focused, and gives it back with Escape.
    // ------------------------------------------------------------------

    /// Opens the sidebar and puts the keyboard in it; closes it when it already has
    /// the keyboard. Open-but-unfocused is a state you reach by clicking a pane, and
    /// the same key then focuses the tree again rather than hiding it — hiding what
    /// you just asked to look at is not a toggle anyone wants.
    fn toggle_explorer(&mut self, config: &Config) {
        let root = self
            .tab()
            .focused_ref()
            .cwd()
            .map(|d| crate::explorer::root_for(&d))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let side = crate::explorer::Side::parse(&config.explorer.side).unwrap_or_default();
        let width = config.explorer.width;
        let show_hidden = config.explorer.show_hidden;
        let tab = &mut self.tabs[self.active];
        match &mut tab.explorer {
            Some(e) if e.open && e.focused => {
                e.open = false;
                e.focused = false;
            }
            Some(e) => {
                e.open = true;
                e.focused = true;
                e.set_root(root);
            }
            None => {
                let mut e = crate::explorer::Explorer::new(root, width, side);
                e.show_hidden = show_hidden;
                tab.explorer = Some(e);
            }
        }
        self.explorer_read_pending();
        // The panes just lost (or gained) columns: their PTYs have to be told.
        let area = self.active_area();
        self.tabs[self.active].reflow(area);
        self.window.request_redraw();
    }

    /// The tree row under a pointer position, if it is over the sidebar at all.
    /// `Some(None)` means the sidebar but not a row (its header or footer).
    fn explorer_row_at(&self, pos: PhysicalPosition<f64>) -> Option<Option<usize>> {
        let e = self.tabs.get(self.active)?.explorer.as_ref().filter(|e| e.open)?;
        let cell = self.renderer.cell_size();
        let rect = e.rect(self.window_area(), cell);
        let (x, y) = (pos.x as f32, pos.y as f32);
        if x < rect.x || x >= rect.x + rect.w || y < rect.y || y >= rect.y + rect.h {
            return None;
        }
        let line = ((y - rect.y) / cell.1).floor() as usize;
        // Row 0 is the header and the last row is the footer.
        let body = (rect.h / cell.1).floor().max(1.0) as usize;
        if line == 0 || line + 1 >= body {
            return Some(None);
        }
        let i = e.scroll + line - 1;
        Some((i < e.rows.len()).then_some(i))
    }

    /// Whether a pointer is on the edge between the sidebar and the panes, where a
    /// drag resizes it. Two cells wide, like every other divider here: a one-cell
    /// target is a target you miss.
    fn explorer_edge_at(&self, pos: PhysicalPosition<f64>) -> bool {
        let Some(e) = self.tabs.get(self.active).and_then(|t| t.explorer.as_ref()) else {
            return false;
        };
        if !e.open {
            return false;
        }
        let cell = self.renderer.cell_size();
        let rect = e.rect(self.window_area(), cell);
        let edge = match e.side {
            crate::explorer::Side::Left => rect.x + rect.w,
            crate::explorer::Side::Right => rect.x,
        };
        let (x, y) = (pos.x as f32, pos.y as f32);
        y >= rect.y && y < rect.y + rect.h && (x - edge).abs() <= cell.0
    }

    /// Drags the sidebar's edge. The width follows the pointer live (the tree
    /// redraws), but the PANES are only reflowed on release: a reflow per frame
    /// resizes every PTY per frame, and full-screen programs do not survive it.
    fn explorer_drag(&mut self, pos: PhysicalPosition<f64>) {
        let cell = self.renderer.cell_size();
        let area = self.window_area();
        let Some(e) = self.tabs[self.active].explorer.as_mut() else { return };
        let cols = match e.side {
            crate::explorer::Side::Left => (pos.x as f32 - area.x) / cell.0,
            crate::explorer::Side::Right => (area.x + area.w - pos.x as f32) / cell.0,
        };
        e.width = cols.round().max(crate::explorer::MIN_WIDTH as f32) as usize;
        self.window.request_redraw();
    }

    /// Whether the keyboard is in the sidebar.
    fn explorer_focused(&self) -> bool {
        self.tabs
            .get(self.active)
            .and_then(|t| t.explorer.as_ref())
            .is_some_and(|e| e.open && e.focused)
    }

    /// How many rows of tree the sidebar is drawing, for the scrolling maths.
    fn explorer_body_rows(&self) -> usize {
        let (_, ch) = self.renderer.cell_size();
        let h = self.window_area().h;
        ((h / ch).floor().max(1.0) as usize).saturating_sub(2)
    }

    /// Starts a worker for every directory the tree has open and has not read.
    ///
    /// One thread per directory, tagged with the explorer's `seq`: a `read_dir` of
    /// `node_modules` or of a network mount takes long enough to drop frames, and
    /// the answer to a read the tree has moved past has to be droppable.
    fn explorer_read_pending(&mut self) {
        let tab_index = self.active;
        let Some(e) = self.tabs[self.active].explorer.as_mut() else { return };
        let (seq, hidden) = (e.seq, e.show_hidden);
        let want: Vec<std::path::PathBuf> =
            e.open_dirs().into_iter().filter(|d| e.needs_read(d)).collect();
        for dir in &want {
            e.loading.insert(dir.clone());
        }
        for dir in want {
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let entries = crate::explorer::read_dir(&dir, hidden);
                let _ = proxy.send_event(UserEvent::Explorer(tab_index, seq, dir, entries));
            });
        }
    }

    /// A finished directory read. Dropped when it belongs to a tree that has since
    /// been re-rooted (`seq`) or to a tab that is gone.
    fn on_explorer_read(
        &mut self,
        tab_index: usize,
        seq: u64,
        dir: std::path::PathBuf,
        entries: Vec<crate::explorer::Entry>,
    ) {
        let Some(tab) = self.tabs.get_mut(tab_index) else { return };
        let Some(e) = tab.explorer.as_mut() else { return };
        if e.seq != seq || !dir.starts_with(&e.root) {
            return;
        }
        e.insert_children(dir, entries);
        self.window.request_redraw();
    }

    /// Re-reads everything the tree has open, keeping what is folded folded.
    fn explorer_refresh(&mut self) {
        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
            e.children.clear();
            e.loading.clear();
            e.seq += 1;
        }
        self.explorer_read_pending();
    }

    /// The sidebar's leader layer. Returns whether it consumed the key.
    ///
    /// Same deal as the git panel's: the panel that has the keyboard gets its own
    /// tree, because with the tree focused the global "new tab" under the same
    /// letter is not what the hand means.
    fn explorer_leader_key(&mut self, key: &Key, mods: ModifiersState, config: &Config) -> bool {
        if !self.explorer_focused() {
            return false;
        }
        let is_leader = match (
            crate::actions::leader_chord(&config.leader),
            Chord::from_event(key, mods),
        ) {
            (Some(l), Some(c)) => l == c,
            _ => false,
        };
        let armed = self
            .tabs
            .get(self.active)
            .and_then(|t| t.explorer.as_ref())
            .is_some_and(|e| e.leader.is_some());
        if is_leader {
            if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                if armed {
                    e.cancel_leader();
                } else {
                    e.arm_leader();
                }
            }
            self.window.request_redraw();
            return true;
        }
        if !armed {
            return false;
        }
        // A character with ctrl/alt/super is a shortcut attempt, not a choice here.
        if matches!(key, Key::Character(_))
            && (mods.control_key() || mods.alt_key() || mods.super_key())
        {
            return false;
        }
        let press = {
            let Some(e) = self.tabs[self.active].explorer.as_mut() else { return false };
            match key {
                Key::Named(NamedKey::Escape) => {
                    e.cancel_leader();
                    None
                }
                Key::Character(c) => c.chars().next().and_then(|c| e.leader_key(c)),
                _ => None,
            }
        };
        if let Some(k) = press {
            let key = match k {
                crate::explorer::FileKey::Ch(c) => {
                    Key::Character(winit::keyboard::SmolStr::new(c.to_string()))
                }
                crate::explorer::FileKey::Enter => Key::Named(NamedKey::Enter),
            };
            self.explorer_key(&key, config);
        }
        self.window.request_redraw();
        true
    }

    /// Keys while the sidebar has the keyboard. Returns whether it consumed one.
    fn explorer_key(&mut self, key: &Key, config: &Config) -> bool {
        if !self.explorer_focused() {
            return false;
        }
        let body = self.explorer_body_rows();
        let mut read = false;
        let mut open = false;
        let mut refresh = false;
        let mut copy: Option<String> = None;
        let mut edit: Option<std::path::PathBuf> = None;
        let mut system: Option<std::path::PathBuf> = None;
        let mut rename = false;
        let mut create = false;
        let mut delete = false;
        let mut props = false;
        let mut unfocus = false;
        {
            let Some(e) = self.tabs[self.active].explorer.as_mut() else { return false };
            e.message = None;
            let selected = e.selected().cloned();
            match key {
                Key::Named(NamedKey::Escape) => unfocus = true,
                Key::Named(NamedKey::ArrowDown) => e.move_cursor(1, body),
                Key::Named(NamedKey::ArrowUp) => e.move_cursor(-1, body),
                Key::Named(NamedKey::PageDown) => e.move_cursor(body as i32 / 2, body),
                Key::Named(NamedKey::PageUp) => e.move_cursor(-(body as i32) / 2, body),
                Key::Named(NamedKey::Home) => e.set_cursor(0, body),
                Key::Named(NamedKey::End) => e.set_cursor(usize::MAX, body),
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::ArrowRight) => open = true,
                Key::Named(NamedKey::ArrowLeft) => {
                    if let Some(row) = selected {
                        // Left on an open directory folds it; on anything else it
                        // goes to the parent, which is what a tree's left means.
                        if row.entry.dir && row.open {
                            e.toggle(&row.entry.path);
                        } else if let Some(i) =
                            e.rows.iter().position(|r| Some(r.entry.path.as_path()) == row.entry.path.parent())
                        {
                            e.set_cursor(i, body);
                        }
                    }
                }
                Key::Character(c) => match c.as_str() {
                    "j" => e.move_cursor(1, body),
                    "k" => e.move_cursor(-1, body),
                    "g" => e.set_cursor(0, body),
                    "G" => e.set_cursor(usize::MAX, body),
                    "l" => {
                        if let Some(row) = selected.filter(|r| r.entry.dir && !r.open) {
                            read = e.toggle(&row.entry.path);
                        }
                    }
                    "h" => {
                        if let Some(row) = selected {
                            if row.entry.dir && row.open {
                                e.toggle(&row.entry.path);
                            } else if let Some(i) = e
                                .rows
                                .iter()
                                .position(|r| Some(r.entry.path.as_path()) == row.entry.path.parent())
                            {
                                e.set_cursor(i, body);
                            }
                        }
                    }
                    "." => {
                        e.show_hidden = !e.show_hidden;
                        refresh = true;
                    }
                    // `r` renames, as in every file manager. Re-reading the tree is
                    // `R`: the destructive-looking letter goes to the safe verb, not
                    // the other way round.
                    "R" => refresh = true,
                    "r" => rename = true,
                    "a" => create = true,
                    "d" => delete = true,
                    "p" => props = true,
                    "e" => edit = selected.as_ref().filter(|r| r.more.is_none()).map(|r| r.entry.path.clone()),
                    "o" => system = selected.as_ref().filter(|r| r.more.is_none()).map(|r| r.entry.path.clone()),
                    "y" => copy = selected.map(|r| r.entry.path.display().to_string()),
                    "q" => unfocus = true,
                    _ => {}
                },
                _ => {}
            }
        }
        if unfocus {
            if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                e.focused = false;
            }
        }
        if rename {
            self.explorer_rename_prompt();
        } else if create {
            self.explorer_create_prompt();
        } else if delete {
            self.explorer_delete_prompt();
        } else if props {
            self.explorer_props();
        } else if open {
            self.explorer_open(config);
        } else if refresh {
            self.explorer_refresh();
        } else if read {
            self.explorer_read_pending();
        }
        if let Some(text) = copy {
            self.set_clipboard(text);
            if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                e.message = Some("path copied".into());
            }
        }
        if let Some(path) = edit {
            self.explorer_edit(path, config);
        }
        // `o` forces the desktop handler for ANY file — except the two that would
        // execute something, which still ask first.
        if let Some(path) = system {
            if crate::explorer::is_desktop(&path) {
                self.overlay = Some(Overlay::Prompt(Prompt::new(
                    PromptKind::ExplorerRun,
                    "Run the handler in this .desktop file?",
                    vec![path.display().to_string()],
                )));
            } else {
                self.explorer_xdg_open(path);
            }
        }
        let _ = config;
        self.window.request_redraw();
        true
    }

    /// Opens the row under the cursor: a directory folds or unfolds, a file is
    /// opened according to what it IS.
    ///
    /// The type sniff and the permission check are two questions, kept apart on
    /// purpose. A script is text and runnable at once; deciding "it is executable,
    /// therefore run it" is how a panel loses the case where you wanted to read it.
    fn explorer_open(&mut self, config: &Config) {
        let Some(row) = self
            .tabs
            .get(self.active)
            .and_then(|t| t.explorer.as_ref())
            .and_then(|e| e.selected().cloned())
        else {
            return;
        };
        if row.more.is_some() {
            return;
        }
        let path = row.entry.path.clone();
        if row.entry.dir {
            let body = self.explorer_body_rows();
            let mut read = false;
            if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                if row.entry.link && !row.open {
                    e.message = Some("symlinked directory - not followed".into());
                } else {
                    read = e.toggle(&path);
                }
                let _ = body;
            }
            if read {
                self.explorer_read_pending();
            }
            self.window.request_redraw();
            return;
        }

        let kind = crate::explorer::kind_of(&path);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Anything that RUNS when opened is never opened by one keypress.
        if crate::explorer::is_desktop(&path) || (row.entry.exec && kind == crate::explorer::Kind::Binary) {
            let what = if crate::explorer::is_desktop(&path) {
                format!("Run the handler in {name}?")
            } else {
                format!("Run {name}?")
            };
            self.overlay = Some(Overlay::Prompt(Prompt::new(
                PromptKind::ExplorerRun,
                &what,
                vec![path.display().to_string()],
            )));
            self.window.request_redraw();
            return;
        }
        // An executable text file is legitimately three things. Ask which.
        if row.entry.exec && kind == crate::explorer::Kind::Text {
            self.overlay = Some(Overlay::Prompt(Prompt::new(
                PromptKind::ExplorerAction,
                &format!("{name} is executable - what with it?"),
                vec![
                    "view".to_string(),
                    "edit".to_string(),
                    "run".to_string(),
                    "open with the system".to_string(),
                ],
            )));
            self.window.request_redraw();
            return;
        }
        match kind {
            crate::explorer::Kind::Text | crate::explorer::Kind::Image => self.explorer_view(path),
            crate::explorer::Kind::Binary => {
                if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                    e.message = Some("binary file - o opens it with the system".into());
                }
                self.window.request_redraw();
            }
            crate::explorer::Kind::Directory => {}
        }
        let _ = config;
    }

    /// The path under the tree's cursor, if it is on a real row.
    fn explorer_selected_path(&self) -> Option<std::path::PathBuf> {
        let e = self.tabs.get(self.active)?.explorer.as_ref()?;
        e.selected().filter(|r| r.more.is_none()).map(|r| r.entry.path.clone())
    }

    /// Reads a file on a worker and opens the viewer on it.
    ///
    /// On a worker because this is the other call that hangs: a file on a network
    /// mount, or a 4 MB one, is not something the frame can wait for.
    fn explorer_view(&mut self, path: std::path::PathBuf) {
        let proxy = self.proxy.clone();
        let cell = self.renderer.cell_size();
        let screen = (self.surface_config.width as f32, self.surface_config.height as f32);
        // The art is sized here, where the cell size is known; the worker only
        // decodes. Half the panel's width, which is what the viewer gives an image.
        let cols = ((screen.0 / cell.0) as usize).saturating_sub(8).clamp(20, 200);
        let rows = ((screen.1 / cell.1) as usize).saturating_sub(8).clamp(10, 100);
        let aspect = cell.0 / cell.1.max(1.0);
        std::thread::spawn(move || {
            let read = crate::explorer::read_for_view(&path, cols, rows, aspect);
            let _ = proxy.send_event(UserEvent::FileRead(path, read));
        });
    }

    /// A finished file read: put the viewer up.
    fn on_file_read(&mut self, path: std::path::PathBuf, read: crate::explorer::ViewRead) {
        let body = match read.body {
            Ok(b) => b,
            Err(e) => crate::overlay::Viewed::Note(e),
        };
        self.overlay =
            Some(Overlay::Viewer(crate::overlay::FileViewer::new(path, body, read.bytes)));
        self.window.request_redraw();
    }

    /// Hands a path to `$EDITOR`, in the focused pane when it is idle and in a new
    /// split when it is not — the same "is anything running here" question the close
    /// confirm asks. The path is shell-quoted: a filename with a space or a `$` is
    /// otherwise an injection into the user's own shell.
    fn explorer_edit(&mut self, path: std::path::PathBuf, config: &Config) {
        let cmd = vec![editor_cmd(), path.display().to_string()];
        self.run_in_pane_or_split(cmd, config);
    }

    /// Runs a path as a command, with the same placement rule.
    fn explorer_run(&mut self, path: std::path::PathBuf, config: &Config) {
        self.run_in_pane_or_split(vec![path.display().to_string()], config);
    }

    /// Sends a command to the focused pane if it is sitting at its prompt, and to a
    /// new split if something is already running there. Typing a command into a pane
    /// that is running vim would type it INTO vim.
    fn run_in_pane_or_split(&mut self, cmd: Vec<String>, config: &Config) {
        let busy = self
            .tab()
            .focused_ref()
            .pty
            .foreground()
            .is_some_and(|fg| !is_shell(&fg.name));
        self.overlay = None;
        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
            e.focused = false;
        }
        if busy {
            self.split_running(config, cmd);
            return;
        }
        let line = cmd.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
        self.tab().focused().write(format!("{line}\r").as_bytes());
        self.window.request_redraw();
    }

    /// Opens a path with the desktop's handler, detached and with its output thrown
    /// away: `xdg-open` outlives the call, and over ssh or with no portal it fails
    /// slowly and noisily into whatever terminal it inherited.
    fn explorer_xdg_open(&mut self, path: std::path::PathBuf) {
        let msg = match std::process::Command::new("xdg-open")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                // Reaped on a thread: nothing else ever waits on it, and an unreaped
                // handler is a zombie for the life of the terminal.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                "handed to the desktop".to_string()
            }
            Err(e) => format!("xdg-open: {e}"),
        };
        if let Some(ex) = self.tabs[self.active].explorer.as_mut() {
            ex.message = Some(msg);
        }
        self.window.request_redraw();
    }

    /// Leaves a message in the sidebar's footer.
    fn explorer_note(&mut self, text: &str) {
        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
            e.message = Some(text.to_string());
        }
        self.window.request_redraw();
    }

    /// After an operation: re-read the tree, put the keyboard back in it, and land
    /// the cursor on what the operation produced when there is one.
    fn explorer_after_op(&mut self, land_on: Option<std::path::PathBuf>, what: &str) {
        self.overlay = None;
        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
            e.focused = true;
            e.message = Some(what.to_string());
            e.pending_cursor = land_on;
        }
        self.explorer_refresh();
    }

    /// Asks for a new name, pre-filled with the current one. Retyping a name to
    /// change one letter of it is not a rename box, it is a spelling test.
    fn explorer_rename_prompt(&mut self) {
        let Some(path) = self.explorer_selected_path() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.overlay = Some(Overlay::Prompt(Prompt::with_input(
            PromptKind::ExplorerRename,
            "Rename to",
            name,
        )));
        self.window.request_redraw();
    }

    /// Asks for a name to create. A trailing `/` makes a directory.
    fn explorer_create_prompt(&mut self) {
        if self.explorer_selected_path().is_none() {
            return;
        }
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::ExplorerCreate,
            "New name (end with / for a directory)",
            Vec::new(),
        )));
        self.window.request_redraw();
    }

    /// Confirms a delete, NAMING what is inside a directory.
    ///
    /// The count runs here on the UI thread only for a directory the tree already
    /// has open — otherwise it goes to a worker, because counting a tree walks it
    /// and `node_modules` is a tree.
    fn explorer_delete_prompt(&mut self) {
        let Some(path) = self.explorer_selected_path() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let proxy = self.proxy.clone();
        let tab = self.active;
        std::thread::spawn(move || {
            let counted = path.is_dir().then(|| crate::explorer::count_tree(&path));
            let label = match counted {
                Some((files, dirs)) if files + dirs > 0 => {
                    format!("Delete {name} and the {} inside?", crate::explorer::count_words(files, dirs))
                }
                Some(_) => format!("Delete the empty directory {name}?"),
                None => format!("Delete {name}?"),
            };
            let _ = proxy.send_event(UserEvent::ExplorerConfirm(tab, label));
        });
    }

    /// Puts a counted confirm on screen once its worker has counted.
    fn on_explorer_confirm(&mut self, tab: usize, label: String) {
        if tab != self.active {
            return;
        }
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::ExplorerDelete,
            &label,
            self.explorer_selected_path().map(|p| p.display().to_string()).into_iter().collect(),
        )));
        self.window.request_redraw();
    }

    /// Opens the properties panel for the selected row, reading it on a worker
    /// (counting a directory's contents walks the whole tree).
    fn explorer_props(&mut self) {
        let Some(path) = self.explorer_selected_path() else { return };
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let props = crate::explorer::props_of(&path);
            let _ = proxy.send_event(UserEvent::ExplorerProps(props.map_err(|e| e.to_string())));
        });
    }

    fn on_explorer_props(&mut self, props: Result<crate::explorer::Props, String>) {
        match props {
            Ok(p) => self.overlay = Some(Overlay::Props(crate::overlay::PropsPanel::new(p))),
            Err(e) => self.explorer_note(&e),
        }
        self.window.request_redraw();
    }

    /// Applies the edited permission bits. A recursive change confirms first, with
    /// the count of what it would touch — and Enter is not a yes there.
    fn explorer_apply_mode(&mut self) {
        let Some(Overlay::Props(p)) = &self.overlay else { return };
        if !p.dirty() && !p.recursive {
            self.overlay = None;
            if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                e.focused = true;
            }
            self.window.request_redraw();
            return;
        }
        if p.recursive {
            let (files, dirs) = p.props.contents.unwrap_or((0, 0));
            let mode = crate::explorer::mode_string(p.mode);
            let label = format!(
                "Set {mode} on this directory and the {} inside?",
                crate::explorer::count_words(files, dirs)
            );
            let path = p.props.path.display().to_string();
            // The confirm replaces the panel, and with it the bits being edited, so
            // they are parked here until the answer comes back.
            self.pending_mode = Some(p.mode);
            self.overlay =
                Some(Overlay::Prompt(Prompt::new(PromptKind::ExplorerChmod, &label, vec![path])));
            self.window.request_redraw();
            return;
        }
        self.explorer_chmod(false);
    }

    /// Writes the mode. `confirmed` says the recursive confirm has been answered,
    /// in which case the panel is behind the prompt and its state is gone — so the
    /// mode and the path are read back off the tree's selection.
    fn explorer_chmod(&mut self, confirmed: bool) {
        let (path, mode, recursive) = match &self.overlay {
            Some(Overlay::Props(p)) => (p.props.path.clone(), p.mode, p.recursive),
            _ if confirmed => {
                let Some(path) = self.explorer_selected_path() else { return };
                // The confirm replaced the panel; re-read what is on disk and apply
                // the requested bits to the tree from there.
                let Some(mode) = self.pending_mode.take() else { return };
                (path, mode, true)
            }
            _ => return,
        };
        match crate::explorer::set_mode(&path, mode, recursive) {
            Ok(n) => {
                let what = if n == 1 {
                    format!("permissions now {}", crate::explorer::mode_string(mode))
                } else {
                    format!("permissions set on {n} paths")
                };
                self.explorer_after_op(Some(path), &what);
            }
            Err(e) => self.explorer_note(&e),
        }
    }

    /// Re-anchors the tree when the focused pane moves to another REPOSITORY.
    ///
    /// Only on a change of root, never on every `cd`: re-anchoring per directory
    /// collapses the tree while you are navigating inside one project, which is
    /// precisely when you are using it.
    fn explorer_sync_root(&mut self) {
        if self.tabs[self.active].explorer.as_ref().is_none_or(|e| !e.open) {
            return;
        }
        let Some(cwd) = self.tab().focused_ref().cwd() else { return };
        let root = crate::explorer::root_for(&cwd);
        let changed = {
            let Some(e) = self.tabs[self.active].explorer.as_mut() else { return };
            if e.root == root {
                return;
            }
            e.seq += 1;
            e.set_root(root);
            true
        };
        if changed {
            self.explorer_read_pending();
            self.window.request_redraw();
        }
    }

    /// The leader layer, for one key. Returns whether it consumed it.
    ///
    /// Split out of `on_key` so the remote-control `key` command drives the same
    /// layer a hand does — a second implementation of a modal layer is a second
    /// set of bugs.
    fn leader_key(
        &mut self,
        key: &Key,
        mods: ModifiersState,
        config: &Config,
        keymap: &Keymap,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        // A modifier press alone must not consume the arming — the user is allowed
        // to reach for shift on the way to the second key.
        if matches!(
            key,
            Key::Named(
                NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super
                    | NamedKey::AltGraph | NamedKey::CapsLock
            )
        ) {
            return false;
        }
        if let Some(armed_at) = self.leader_armed {
            // An expired arm is treated as no arm at all: the key falls through to
            // the pane, so a stray keystroke is never silently eaten.
            if self.leader_timeout.is_some_and(|d| armed_at.elapsed() >= d) {
                self.cancel_leader();
            } else {
                // Escape backs out of the whole layer, the way it leaves every
                // other modal thing in runnir.
                if matches!(key, Key::Named(NamedKey::Escape)) {
                    self.cancel_leader();
                    self.window.request_redraw();
                    return true;
                }
                if let Some(chord) = Chord::from_event(key, mods) {
                    self.leader_path.push(chord);
                }
                match keymap.resolve_leader(&self.leader_path) {
                    // A group: stay armed, restart the clock (the panel is on
                    // screen now, so the user is reading, not stalling) and let
                    // the which-key panel show what this group holds.
                    Some(LeaderNode::Group { .. }) => {
                        self.leader_armed = Some(Instant::now());
                        self.leader_entries = keymap.leader_entries(&self.leader_path);
                    }
                    Some(LeaderNode::Run(action)) => {
                        let action = action.clone();
                        self.cancel_leader();
                        self.run_action(action, config, event_loop);
                    }
                    // A miss ends the sequence, bound or not. Falling through to
                    // the pane would leak a stray character into the shell after
                    // a mistyped sequence.
                    None => self.cancel_leader(),
                }
                self.window.request_redraw();
                return true;
            }
        }
        if keymap.is_leader(key, mods) {
            self.leader_armed = Some(Instant::now());
            self.leader_path.clear();
            self.leader_entries = keymap.leader_entries(&[]);
            // The armed state shows as a chip in the status bar, which lives
            // exactly as long as the arming (`build_status` reads `leader_armed`).
            // With the bar hidden there is nowhere to put it, so fall back to a
            // toast — an invisible modal layer is how you eat a keystroke and
            // leave the user wondering.
            if !self.status_bar {
                self.toast("leader\u{2026}", self.leader_timeout.map_or(30, |d| d.as_secs()));
            }
            self.window.request_redraw();
            return true;
        }
        false
    }

    /// A keypress with no `KeyEvent` behind it, for the remote-control `key`
    /// command: overlays, the leader layer and the bound actions, in the order
    /// `on_key` tries them.
    ///
    /// It deliberately stops short of the pane: text for the child goes through
    /// `send-text`, which does not have to guess an encoding.
    fn press_key(
        &mut self,
        key: &Key,
        mods: ModifiersState,
        config: &Config,
        keymap: &Keymap,
        event_loop: &ActiveEventLoop,
    ) {
        if self.overlay.is_some() {
            self.overlay_key(key, mods, config);
            return;
        }
        if self.explorer_leader_key(key, mods, config) {
            return;
        }
        if self.leader_key(key, mods, config, keymap, event_loop) {
            return;
        }
        if let Some(action) = keymap.resolve(key, mods) {
            let action = action.clone();
            self.run_action(action, config, event_loop);
            return;
        }
        // Same order as `on_key`: the sidebar takes what the leader and the bound
        // chords did not. Leaving this out is what made a scripted `j` do nothing
        // while the same key worked from the keyboard.
        if !mods.control_key() && !mods.alt_key() && !mods.super_key() {
            self.explorer_key(key, config);
        }
    }

    /// What is on screen now, for the answer to a scripted key or click.
    ///
    /// Enough to assert on: which overlay is up and, for the git panel, the state a
    /// key would have changed. A caller that had to screenshot to find out what its
    /// keypress did could not be a test.
    fn ui_state(&self) -> serde_json::Value {
        use serde_json::json;
        let (cw, ch) = self.renderer.cell_size();
        let cols = (self.surface_config.width as f32 / cw).floor().max(1.0) as usize;
        let rows = (self.surface_config.height as f32 / ch).floor().max(1.0) as usize;
        let overlay = match &self.overlay {
            None => "none",
            Some(Overlay::Git(_)) => "git",
            Some(Overlay::Prompt(_)) => "prompt",
            Some(Overlay::Palette(_)) => "palette",
            Some(Overlay::Search(_)) => "search",
            Some(Overlay::Docs(_)) => "docs",
            Some(Overlay::Viewer(_)) => "viewer",
            Some(Overlay::Props(_)) => "props",
            Some(_) => "other",
        };
        let mut out = json!({
            "overlay": overlay,
            "cols": cols,
            "rows": rows,
            "leader_armed": self.leader_armed.is_some(),
        });
        if let Some(Overlay::Props(p)) = &self.overlay {
            out["props"] = json!({
                "path": p.props.path.display().to_string(),
                "dir": p.props.dir,
                "mode": format!("{:o}", p.mode & 0o777),
                "mode_string": crate::explorer::mode_string(p.mode),
                "bit": p.bit,
                "dirty": p.dirty(),
                "recursive": p.recursive,
                "contents": p.props.contents.map(|(f, d)| json!({"files": f, "dirs": d})),
                "link_target": p.props.link_target.as_ref().map(|t| t.display().to_string()),
            });
        }
        if let Some(Overlay::Prompt(p)) = &self.overlay {
            out["prompt"] = json!({ "label": p.label, "input": p.input });
        }
        if let Some(Overlay::Viewer(v)) = &self.overlay {
            out["viewer"] = json!({
                "path": v.path.display().to_string(),
                "kind": match &v.body {
                    crate::overlay::Viewed::Text { .. } => "text",
                    crate::overlay::Viewed::Image { .. } => "image",
                    crate::overlay::Viewed::Note(_) => "note",
                },
                "lines": v.len(),
                "scroll": v.scroll,
                "bytes": v.bytes,
            });
        }
        if let Some(e) = self.tabs.get(self.active).and_then(|t| t.explorer.as_ref()) {
            out["explorer"] = json!({
                "open": e.open,
                "focused": e.focused,
                "root": e.root.display().to_string(),
                "width": e.width_in(self.window_area(), self.renderer.cell_size()),
                "side": e.side.label(),
                "cursor": e.cursor,
                "rows": e.rows.len(),
                "selected": e.selected().map(|r| r.entry.path.display().to_string()),
                "open_dirs": e.expanded.len(),
                "hidden": e.show_hidden,
                "message": e.message,
            });
        }
        if let Some(Overlay::Git(p)) = &self.overlay {
            let l = p.layout(cols, rows);
            out["git"] = json!({
                "view": p.view.title(),
                "focus": match p.focus {
                    crate::overlay::GitFocus::List => "list",
                    crate::overlay::GitFocus::Files => "files",
                    crate::overlay::GitFocus::Diff => "diff",
                },
                "zoom": p.zoom,
                "open_commit": p.open_commit,
                "cursor": p.cursor(),
                "rows": p.len(),
                "files_cursor": p.files_cursor(),
                "files": p.commit_files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                "leader": p.leader.as_ref().map(|path| path.iter().collect::<String>()),
                "columns": [l.list_w, l.files_w, l.prev_w()],
                // In SCREEN cells, unlike the widths: the panel is inset, and a
                // caller that has to add the origin itself to aim a drag will get
                // it wrong and click a row instead.
                "separators": [l.sep1().map(|s| s + l.col), l.sep2().map(|s| s + l.col)],
                "origin": [l.col, l.row],
                "preview_lines": p.preview_rows.len(),
                "message": match &p.message {
                    Ok(m) => m.clone(),
                    Err(e) => format!("error: {e}"),
                },
            });
        }
        out
    }

    /// The middle of a cell, in physical pixels — what a click at `col`/`row` means.
    fn cell_centre(&self, col: usize, row: usize) -> PhysicalPosition<f64> {
        let (cw, ch) = self.renderer.cell_size();
        PhysicalPosition::new(
            (col as f32 + 0.5) as f64 * cw as f64,
            (row as f32 + 0.5) as f64 * ch as f64,
        )
    }

    /// Keys while an overlay owns the keyboard. Takes the logical key rather than
    /// the `KeyEvent`, because a `KeyEvent` cannot be built outside winit and the
    /// remote-control `key` command has to reach exactly this path.
    fn overlay_key(&mut self, key: &Key, mods: ModifiersState, config: &Config) {

        // The git panel has a leader layer of its own, armed by the same chord as
        // the global one and drawn with the same which-key. It is checked before
        // everything else here, including the modifier filter below: the leader
        // chord is a modifier chord by definition.
        if self.git_leader_key(key, mods, config) {
            return;
        }

        // A character typed with ctrl/alt/super is a shortcut attempt, not text —
        // ignore it so Ctrl+V inside a prompt does not insert a literal 'v'. Named
        // keys (Escape, Enter, arrows) still act.
        if matches!(key, Key::Character(_))
            && (mods.control_key() || mods.alt_key() || mods.super_key())
        {
            return;
        }
        match self.overlay.as_mut().unwrap() {
            Overlay::Git(_) => self.git_panel_key(key, config),
            // The properties panel: move over the nine permission bits, toggle them,
            // and nothing touches the disk until Enter.
            Overlay::Props(p) => {
                let path = p.props.path.clone();
                let mut apply = false;
                let mut rename = false;
                let mut delete = false;
                match key {
                    Key::Named(NamedKey::Escape) => {
                        self.overlay = None;
                        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                            e.focused = true;
                        }
                    }
                    Key::Named(NamedKey::Enter) => apply = true,
                    Key::Named(NamedKey::Space) => p.toggle_bit(),
                    Key::Named(NamedKey::ArrowRight) => p.move_bit(1),
                    Key::Named(NamedKey::ArrowLeft) => p.move_bit(-1),
                    Key::Named(NamedKey::ArrowDown) => p.move_bit(3),
                    Key::Named(NamedKey::ArrowUp) => p.move_bit(-3),
                    Key::Character(c) => match c.as_str() {
                        "l" => p.move_bit(1),
                        "h" => p.move_bit(-1),
                        "j" => p.move_bit(3),
                        "k" => p.move_bit(-3),
                        "R" => p.recursive = !p.recursive,
                        "r" => rename = true,
                        "d" => delete = true,
                        "q" => self.overlay = None,
                        _ => {}
                    },
                    _ => {}
                }
                if apply {
                    self.explorer_apply_mode();
                } else if rename {
                    self.explorer_rename_prompt();
                } else if delete {
                    self.explorer_delete_prompt();
                }
                let _ = path;
            }
            // The viewer reads a file and nothing else: scroll, hand it to a real
            // editor, or leave. Escape goes back to the tree, which is where the
            // keyboard came from.
            Overlay::Viewer(v) => {
                let path = v.path.clone();
                let mut edit = false;
                let mut open = false;
                let mut copy = false;
                match key {
                    Key::Named(NamedKey::Escape) => {
                        self.overlay = None;
                        if let Some(e) = self.tabs[self.active].explorer.as_mut() {
                            e.focused = true;
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => v.scroll_by(1),
                    Key::Named(NamedKey::ArrowUp) => v.scroll_by(-1),
                    Key::Named(NamedKey::ArrowRight) => v.scroll_side(4),
                    Key::Named(NamedKey::ArrowLeft) => v.scroll_side(-4),
                    Key::Named(NamedKey::PageDown) => v.scroll_by(20),
                    Key::Named(NamedKey::PageUp) => v.scroll_by(-20),
                    Key::Named(NamedKey::Home) => v.scroll = 0,
                    Key::Named(NamedKey::End) => v.to_end(),
                    Key::Character(c) => match c.as_str() {
                        "j" => v.scroll_by(1),
                        "k" => v.scroll_by(-1),
                        "J" => v.scroll_by(20),
                        "K" => v.scroll_by(-20),
                        "l" => v.scroll_side(4),
                        "h" => v.scroll_side(-4),
                        "g" => v.scroll = 0,
                        "G" => v.to_end(),
                        "w" => v.wrap = !v.wrap,
                        "e" => edit = true,
                        "o" => open = true,
                        "y" => copy = true,
                        "q" => self.overlay = None,
                        _ => {}
                    },
                    _ => {}
                }
                if edit {
                    self.explorer_edit(path, config);
                } else if open {
                    self.overlay = None;
                    self.explorer_xdg_open(path);
                } else if copy {
                    self.set_clipboard(path.display().to_string());
                }
            }
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
            Overlay::Config(c) => {
                let editing = c.editing.is_some();
                match key {
                    Key::Named(NamedKey::Escape) => {
                        if editing {
                            c.cancel_edit();
                        } else {
                            self.overlay = None;
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if editing {
                            c.commit_edit();
                        } else {
                            c.activate();
                        }
                    }
                    Key::Named(NamedKey::Backspace) if editing => c.backspace(),
                    Key::Named(NamedKey::ArrowUp) if !editing => c.up(),
                    Key::Named(NamedKey::ArrowDown) if !editing => c.down(),
                    Key::Named(NamedKey::ArrowLeft) if !editing => c.adjust(-1),
                    Key::Named(NamedKey::ArrowRight) if !editing => c.adjust(1),
                    Key::Named(NamedKey::Space) if editing => c.input_char(' '),
                    Key::Named(NamedKey::Space) => c.activate(),
                    Key::Character(s) => {
                        if editing {
                            for ch in s.chars() {
                                c.input_char(ch);
                            }
                        } else {
                            match s.chars().next() {
                                Some('k') => c.up(),
                                Some('j') => c.down(),
                                Some('h') => c.adjust(-1),
                                Some('l' | ' ') => c.adjust(1),
                                Some('s') => c.save(),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Overlay::Theme(t) => match key {
                // Cancel: restore the theme that was live when the picker opened, so a
                // browse leaves no trace.
                Key::Named(NamedKey::Escape) => {
                    let original = t.original();
                    self.overlay = None;
                    self.renderer.set_theme(original);
                }
                // Confirm: keep the highlighted theme and persist it via the same
                // apply/pending/save path the settings panel uses.
                Key::Named(NamedKey::Enter) => {
                    let picked = t.selected_theme();
                    let name = t.selected_name();
                    self.overlay = None;
                    if let Some(theme) = picked {
                        self.keep_theme(theme, name, config);
                    }
                }
                Key::Named(NamedKey::ArrowUp) => t.up(),
                Key::Named(NamedKey::ArrowDown) => t.down(),
                Key::Named(NamedKey::Backspace) => t.backspace(),
                Key::Named(NamedKey::Space) => t.input(' '),
                Key::Character(s) => {
                    for c in s.chars() {
                        t.input(c);
                    }
                }
                _ => {}
            },
            Overlay::Snippets(sp) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Named(NamedKey::ArrowUp) => sp.up(),
                Key::Named(NamedKey::ArrowDown) => sp.down(),
                Key::Named(NamedKey::Backspace) => sp.backspace(),
                Key::Named(NamedKey::Enter) => {
                    let picked = sp.selected();
                    self.overlay = None;
                    if let Some(snip) = picked {
                        self.use_snippet(snip);
                    }
                }
                Key::Named(NamedKey::Space) => sp.input(' '),
                Key::Character(s) => {
                    for c in s.chars() {
                        sp.input(c);
                    }
                }
                _ => {}
            },
            Overlay::ClipHistory(p) => match key {
                Key::Named(NamedKey::Escape) => self.overlay = None,
                Key::Named(NamedKey::ArrowUp) => p.up(),
                Key::Named(NamedKey::ArrowDown) => p.down(),
                Key::Named(NamedKey::Backspace) => p.backspace(),
                Key::Named(NamedKey::Enter) => {
                    let sel = p.selected();
                    self.overlay = None;
                    // Paste through the normal paste path (bracketed-paste aware,
                    // control-byte sanitised) so re-paste behaves like any other paste.
                    if let Some(text) = sel {
                        self.paste_text(text);
                    }
                }
                Key::Named(NamedKey::Space) => p.input(' '),
                Key::Character(s) => {
                    for c in s.chars() {
                        p.input(c);
                    }
                }
                _ => {}
            },
            // The now-playing overlay drives the media backend directly: space toggles
            // play/pause, n/p change track, +/- change volume; each control is followed
            // by a quick metadata refresh so the shown status/track catches up.
            Overlay::Media(_) => match key {
                Key::Named(NamedKey::Escape) => {
                    self.overlay = None;
                    // Stop the waveform worker (and kill cava) as the overlay closes.
                    let _ = self.media_wave.take();
                }
                Key::Named(NamedKey::Space) => {
                    crate::media::play_pause();
                    self.spawn_media_fetch();
                }
                Key::Character(s) => match s.chars().next() {
                    Some('n') => {
                        crate::media::next();
                        self.spawn_media_fetch();
                    }
                    Some('p') => {
                        crate::media::prev();
                        self.spawn_media_fetch();
                    }
                    Some('+' | '=') => crate::media::volume(true),
                    Some('-' | '_') => crate::media::volume(false),
                    Some('q') => {
                        self.overlay = None;
                        let _ = self.media_wave.take();
                    }
                    _ => {}
                },
                _ => {}
            },
            // A yes/no confirm answers to y and n only. Enter is deliberately NOT a
            // yes: this prompt exists because a reflex keystroke closed a window
            // with work in it, and Enter is the reflex.
            Overlay::Prompt(p) if p.kind.is_confirm() => {
                let kind = p.kind;
                match key {
                    Key::Named(NamedKey::Escape) => self.dismiss_confirm(),
                    Key::Character(s) => match s.to_lowercase().as_str() {
                        "y" => {
                            self.overlay = None;
                            self.overlay_under_confirm = None;
                            self.confirm_prompt(kind, String::new(), config);
                        }
                        "n" | "q" => self.dismiss_confirm(),
                        _ => {}
                    },
                    _ => {}
                }
            }
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
                            overlay::HintResult::Chosen(text, kind, alt) => {
                                self.overlay = None;
                                self.act_on_hint(text, kind, alt, config);
                            }
                        }
                    }
                }
                _ => {}
            },
        }
        // A settings-panel edit applies live: adopt the working config into the
        // renderer now and hand it to `App` (which owns the config + keymap). Extract
        // the clone first so the overlay borrow ends before apply_config borrows self.
        let edited = match self.overlay.as_mut() {
            Some(Overlay::Config(c)) if c.dirty => {
                c.dirty = false;
                Some(c.config.clone())
            }
            _ => None,
        };
        if let Some(cfg) = edited {
            self.apply_config(&cfg);
            self.pending_config = Some(cfg);
        }
        // Theme picker: live-preview the highlighted theme so the terminal behind the
        // picker updates as the selection moves. This runs only while it is open and
        // navigating — Enter/Esc have already closed it (and applied or restored),
        // so `as_ref` is `None` on those and the preview is skipped.
        if let Some(Overlay::Theme(t)) = self.overlay.as_ref()
            && let Some(theme) = t.selected_theme()
        {
            self.renderer.set_theme(theme);
        }
        let _ = mods;
        self.window.request_redraw();
    }

    /// Keeps a theme picked from the theme picker: applies it live and persists it
    /// through the same path the settings panel uses (adopt into the renderer, hand
    /// to `App` via `pending_config`, and write the JSON config so it survives a
    /// restart). Refreshing the config via `pending_config` also updates the
    /// hot-reload mtime, so the just-written file does not trigger a redundant reload.
    fn keep_theme(&mut self, theme: crate::config::Theme, name: Option<&str>, config: &Config) {
        let mut cfg = config.clone();
        cfg.theme = theme;
        self.apply_config(&cfg);
        self.status = Some(match (cfg.save_json(), name) {
            (Ok(()), Some(n)) => format!("theme: {n}"),
            (Ok(()), None) => "theme applied".into(),
            (Err(e), _) => format!("theme applied (save failed: {e})"),
        });
        self.status_expiry = Some(Instant::now() + Duration::from_secs(2));
        self.pending_config = Some(cfg);
    }

    fn run_palette_action(&mut self, action: Action, config: &Config) {
        // The palette has no ActiveEventLoop to exit cleanly, so Quit exits the
        // process here — but must save the session first, exactly like the keyboard
        // and window-close paths, or picking "Quit" from the palette would lose it.
        if action == Action::Quit {
            if !self.request_close(config) {
                return;
            }
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
            Action::OpenConfig => self.overlay = Some(Overlay::Config(overlay::ConfigPanel::new(config.clone()))),
            Action::OpenThemePicker => {
                self.overlay = Some(Overlay::Theme(overlay::ThemePicker::new(config.theme.clone())))
            }
            Action::ToggleAi => self.toggle_ai(config),
            Action::AskAiAboutError => self.ask_ai_about_error(config),
            Action::AiCommand => self.ai_command(),
            Action::FixLastCommand => self.fix_last_command(config),
            Action::GitPanel => self.open_git_panel(config),
            Action::AiExplain => self.ai_explain_selection(config),
            Action::SummarizeSession => self.summarize_session(config),
            Action::OpenScrollbackInEditor => self.open_scrollback_in_editor(config),
            Action::PipeLastOutput => self.open_pipe_prompt(PromptKind::PipeLastOutput),
            Action::PipeScrollback => self.open_pipe_prompt(PromptKind::PipeScrollback),
            Action::HistorySearch => self.history_search(),
            Action::WatchKeyword => self.watch_keyword(),
            Action::LaunchLayout => self.open_layout_picker(config),
            Action::OpenSnippets => self.open_snippet_picker(config),
            Action::CopyMode => self.enter_copy_mode(),
            Action::FoldOutput => self.tab().focused().toggle_fold_all(),
            Action::ToggleImageWatch => self.toggle_image_watch(config),
            Action::ToggleExplorer => self.toggle_explorer(config),
            Action::SetImageWatchDir => self.set_image_watch_dir(),
            Action::SaveProjectSession => self.save_project_session_cmd(),
            Action::RestoreProjectSession => self.restore_project_session_cmd(config),
            Action::NowPlaying => self.open_now_playing(),
            Action::MediaPlayPause => {
                crate::media::play_pause();
                self.toast("play / pause", 1);
            }
            Action::MediaNext => {
                crate::media::next();
                self.toast("next track", 1);
            }
            Action::MediaPrev => {
                crate::media::prev();
                self.toast("previous track", 1);
            }
            Action::MediaVolumeUp => {
                crate::media::volume(true);
                self.toast("volume +", 1);
            }
            Action::MediaVolumeDown => {
                crate::media::volume(false);
                self.toast("volume -", 1);
            }
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
            Action::ClipboardHistory => self.open_clip_history(),
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
            Action::CycleLayout => self.cycle_layout(area),
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
        // Use the rect the minimap was DRAWN in (visible_rects honours zoom), not the
        // split layout, or a click maps to the wrong region when a pane is zoomed.
        let Some((_, r)) = self.visible_rects(area).into_iter().find(|(id, _)| *id == focus) else {
            return false;
        };
        let strip_w = crate::MINIMAP_W;
        // A pane narrower than the strip has no minimap (it would escape the pane).
        if r.w <= strip_w {
            return false;
        }
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

    /// Cycles the active tab's layout mode and shows the new mode as a brief toast.
    /// Reapplies any zoom so a zoomed pane stays full-size across the switch.
    fn cycle_layout(&mut self, area: Rect) {
        let mode = self.tabs[self.active].cycle_layout(area);
        self.reapply_zoom();
        self.status = Some(format!("layout: {}", mode.label()));
        self.status_expiry = Some(Instant::now() + Duration::from_secs(2));
        self.window.request_redraw();
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
            // Confirmed the close. Exits here rather than through the event loop
            // (which this path cannot reach) — the same save-then-exit the palette's
            // Quit does, so a confirmed close never loses the session.
            PromptKind::ConfirmQuit => {
                self.save_session(config);
                std::process::exit(0);
            }
            // The chooser for an executable text file. The path is the prompt's one
            // suggestion; the answer is the verb.
            PromptKind::ExplorerAction => {
                let Some(path) = self.explorer_selected_path() else { return };
                match value.as_str() {
                    "view" => self.explorer_view(path),
                    "edit" => self.explorer_edit(path, config),
                    "run" => self.explorer_run(path, config),
                    _ => self.explorer_xdg_open(path),
                }
            }
            // Confirmed running something that runs. `value` is empty (a y/n
            // confirm carries no text), so the path comes from the tree.
            PromptKind::ExplorerRun => {
                let Some(path) = self.explorer_selected_path() else { return };
                if crate::explorer::is_desktop(&path) {
                    self.explorer_xdg_open(path);
                } else {
                    self.explorer_run(path, config);
                }
            }
            PromptKind::ExplorerRename => {
                let Some(path) = self.explorer_selected_path() else { return };
                match crate::explorer::rename(&path, &value) {
                    Ok(to) => self.explorer_after_op(Some(to), "renamed"),
                    Err(e) => self.explorer_note(&e),
                }
            }
            PromptKind::ExplorerCreate => {
                // Created BESIDE the selected row, or inside it when the row is an
                // open directory: "new file here" means where you are looking.
                let Some(path) = self.explorer_selected_path() else { return };
                let open_dir = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.explorer.as_ref())
                    .and_then(|e| e.selected())
                    .is_some_and(|r| r.entry.dir && r.open);
                let parent = if open_dir {
                    path.clone()
                } else {
                    path.parent().map(|p| p.to_path_buf()).unwrap_or(path.clone())
                };
                match crate::explorer::create(&parent, &value) {
                    Ok(made) => self.explorer_after_op(Some(made), "created"),
                    Err(e) => self.explorer_note(&e),
                }
            }
            PromptKind::ExplorerDelete => {
                let Some(path) = self.explorer_selected_path() else { return };
                let dir = path.is_dir();
                match crate::explorer::delete(&path, dir) {
                    Ok(()) => self.explorer_after_op(None, "deleted"),
                    Err(e) => self.explorer_note(&e),
                }
            }
            PromptKind::ExplorerChmod => self.explorer_chmod(true),
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
            PromptKind::PipeLastOutput => {
                if !value.trim().is_empty() {
                    self.pipe_through(value, false, config);
                }
            }
            PromptKind::PipeScrollback => {
                if !value.trim().is_empty() {
                    self.pipe_through(value, true, config);
                }
            }
            // Both git prompts hand control back to the panel: it owns the overlay
            // slot, and its lists are refetched from the repository anyway.
            PromptKind::GitCommit => {
                if !value.trim().is_empty() {
                    self.open_git_panel(config);
                    self.git_exec(vec!["commit".into(), "-m".into(), value.trim().to_string()]);
                }
            }
            PromptKind::GitBranch => {
                if !value.trim().is_empty() {
                    self.open_git_panel(config);
                    self.git_exec(vec!["checkout".into(), "-b".into(), value.trim().to_string()]);
                }
            }
            // The filter is remembered on the panel, so reopening it keeps the view
            // you were looking at; an empty prompt clears it.
            PromptKind::GitLogFilter => {
                self.open_git_panel(config);
                if let Some(Overlay::Git(p)) = &mut self.overlay {
                    p.log_filter = value.trim().to_string();
                    p.set_view(crate::overlay::GitView::Log);
                }
                self.git_reload(config);
            }
            PromptKind::GitTag => {
                if !value.trim().is_empty() {
                    self.open_git_panel(config);
                    self.git_exec(vec!["tag".into(), value.trim().to_string()]);
                }
            }
            PromptKind::ImageWatchDir => {
                if value.trim().is_empty() {
                    self.image_watch = None;
                    self.toast("image auto-preview off", 2);
                } else {
                    let dir = crate::watch::expand_tilde(value.trim());
                    let shown = dir.display().to_string();
                    self.arm_image_watch(dir, config);
                    self.toast(&format!("watching {shown} for images"), 3);
                }
            }
        }
    }

    // ---- helpers used above --------------------------------------------------

    fn reflow_all(&mut self) {
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        for tab in &mut self.tabs {
            tab.set_cell(cell);
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

    // ---- per-project session (layout + cwd) ---------------------------------

    /// The project key for the active tab's focused pane: the nearest git ancestor of
    /// its working directory, or that directory itself. `None` when the cwd is
    /// unreadable (e.g. macOS with no OSC 7 report).
    fn current_project_key(&self) -> Option<std::path::PathBuf> {
        let cwd = self.tabs[self.active].focused_ref().cwd()?;
        Some(project_session::project_key(&cwd))
    }

    /// Records every tab's layout (split shape, mode and per-pane cwd — no scrollback)
    /// under the current project key. Shared by the palette command and the
    /// auto-save-on-exit hook. Returns the key so callers can report it.
    fn save_project_session(&self) -> anyhow::Result<std::path::PathBuf> {
        let key = self
            .current_project_key()
            .ok_or_else(|| anyhow::anyhow!("cannot read the working directory"))?;
        let mut store = project_session::ProjectSessions::load();
        store.upsert(project_session::ProjectEntry {
            key: key.clone(),
            active: self.active,
            tabs: self.tabs.iter().map(|t| t.to_project_layout()).collect(),
            saved_at: 0, // set by upsert
        });
        store.save()?;
        Ok(key)
    }

    /// Palette "Save session for this project": persist and toast the result.
    fn save_project_session_cmd(&mut self) {
        let msg = match self.save_project_session() {
            Ok(key) => format!("session saved for {}", abbreviate_home(&key)),
            Err(e) => format!("could not save session: {e}"),
        };
        self.status = Some(msg);
        self.status_expiry = Some(Instant::now() + Duration::from_secs(3));
        self.window.request_redraw();
    }

    /// Palette "Restore session for this project": rebuild the saved tabs (each pane a
    /// fresh shell in its recorded cwd) and append them, focusing the first restored
    /// tab. Non-destructive — existing tabs are left in place.
    fn restore_project_session_cmd(&mut self, config: &Config) {
        let Some(key) = self.current_project_key() else {
            self.toast("cannot read the working directory", 3);
            return;
        };
        let store = project_session::ProjectSessions::load();
        let Some(entry) = store.get(&key) else {
            self.toast(&format!("no saved session for {}", abbreviate_home(&key)), 3);
            return;
        };
        let area = self.active_area();
        let cell = self.renderer.cell_size();
        let first_new = self.tabs.len();
        // Renumber every restored pane with fresh ids from the seed: pane ids are
        // global across tabs (scroll glide, copy mode and remote control resolve a
        // pane by id through every tab), and the saved ids very likely already belong
        // to panes on screen — the startup tab is pane 1, exactly what a saved layout
        // starts at. Restoring verbatim (or twice) would duplicate ids and make those
        // features act on the wrong pane.
        let mut next_id = self.next_pane_seed + 1;
        let mut states = Vec::new();
        for layout in &entry.tabs {
            let (remapped, next) = layout.remapped_from(next_id);
            next_id = next;
            states.push(remapped.to_tab_state());
        }
        self.next_pane_seed = next_id.saturating_sub(1);
        for state in &states {
            let proxy = self.proxy.clone();
            let wake = move |_id| -> Box<dyn Fn() + Send + 'static> {
                let p = proxy.clone();
                Box::new(move || {
                    let _ = p.send_event(UserEvent::Redraw);
                })
            };
            match Tab::from_session(state, area, cell, config, wake) {
                Ok(tab) => self.tabs.push(tab),
                Err(e) => eprintln!("runnir: could not restore a tab: {e}"),
            }
        }
        if self.tabs.len() > first_new {
            self.active = first_new;
            self.toast(&format!("session restored for {}", abbreviate_home(&key)), 3);
        } else {
            self.toast("session had no tabs to restore", 3);
        }
        self.reflow_all();
    }

    /// Shows a short-lived status message and requests a redraw.
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
        } else if self.tab_repo_dirty(i) {
            // A tab sitting in a repository with uncommitted work. Ranked below the
            // other two on purpose: a failed command and unseen output are events,
            // this is a standing condition.
            Some(('\u{00b1}', (0x9a, 0x9d, 0xa4)))
        } else {
            None
        }
    }

    /// Whether tab `i` is in a repository with uncommitted changes. Reads the two
    /// maps the periodic tick maintains — no filesystem access, because this runs
    /// once per tab on every frame that draws the bar.
    fn tab_repo_dirty(&self, i: usize) -> bool {
        let Some(tab) = self.tabs.get(i) else { return false };
        let Some(root) = self.pane_repo.get(&tab.focus) else { return false };
        self.git_state.get(root).is_some_and(|s| s.dirty > 0 || s.staged > 0 || s.conflicts > 0)
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
        // Taken before the grid lock below: `focused_branches` reads the pane list,
        // and holding a grid mutex across that is how a deadlock starts.
        let branches = self.focused_branches();
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
                    for h in crate::hints::find(&grid, &crate::hints::Context { branches: &branches }) {
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

    /// Opens the git panel on the focused pane's repository.
    fn open_git_panel(&mut self, config: &Config) {
        let root = self.tabs[self.active].focused_ref().cwd().and_then(|p| crate::git::repo_root(&p));
        let Some(root) = root else {
            self.status = Some("not a git repository".into());
            self.status_expiry = Some(Instant::now() + Duration::from_secs(2));
            return;
        };
        self.overlay = Some(Overlay::Git(overlay::GitPanel::new(root)));
        self.git_reload(config);
        self.window.request_redraw();
    }

    /// Refetches every list the panel shows, plus the preview for the selection.
    ///
    /// All of it on workers: this is called after every command, and `git log` on a
    /// large repository is not something the UI thread may wait for.
    fn git_reload(&mut self, config: &Config) {
        let Some(Overlay::Git(p)) = &self.overlay else { return };
        let root = p.root.clone();
        let filter = p.log_filter.clone();
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            // Sent one at a time, cheapest first: the status view is what opens, so
            // it paints while the log of a big repository is still being read.
            let files = crate::git::status_files(&root);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Files(files)));
            let branches = crate::git::local_branches(&root);
            let remotes = crate::git::remote_branches(&root);
            let current = crate::git::head_branch(&root).unwrap_or_default();
            let _ = proxy.send_event(UserEvent::GitPanel(
                seq,
                crate::git::PanelMsg::Branches(branches, remotes, current),
            ));
            let log = crate::git::log_filtered(&root, 200, &filter);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Log(log)));
            let stashes = crate::git::stashes(&root);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Stashes(stashes)));
            let tags = crate::git::tags(&root);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Tags(tags)));
            let reflog = crate::git::reflog(&root, 200);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Reflog(reflog)));
            let mut trees = crate::git::worktrees(&root);
            trees.extend(crate::git::submodules(&root));
            let _ = proxy
                .send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Worktrees(trees)));
        });
        let _ = config;
    }

    /// Fetches the preview for whatever is selected now. Tagged with the current
    /// generation so a fast j/k run does not paint an older diff over a newer one.
    fn git_preview(&mut self) {
        let Some(Overlay::Git(p)) = &self.overlay else { return };
        let root = p.root.clone();
        // Inside a commit, the preview is one file's diff within that commit.
        if let (Some(sha), Some(f)) = (p.open_commit.clone(), p.selected_commit_file().cloned()) {
            let root = p.root.clone();
            self.git_gen += 1;
            let seq = self.git_gen;
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let text = crate::git::show_file(&root, &sha, &f.path);
                let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Preview(text)));
            });
            return;
        }
        let job = match p.view {
            overlay::GitView::Status => p
                .selected_file()
                .map(|f| {
                    // A file can be staged AND modified; `t` toggles which of the
                    // two diffs the preview shows.
                    let staged = p.show_staged || (f.is_staged() && !f.is_unstaged());
                    (f.path.clone(), staged, f.untracked())
                })
                .map(|(path, staged, untracked)| {
                    Box::new(move |root: &std::path::Path| {
                        crate::git::diff_file(root, &path, staged, untracked)
                    }) as Box<dyn FnOnce(&std::path::Path) -> String + Send>
                }),
            overlay::GitView::Log => p.selected_commit().map(|c| c.sha.clone()).map(|sha| {
                Box::new(move |root: &std::path::Path| crate::git::show(root, &sha))
                    as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
            overlay::GitView::Branches => p.selected_branch().map(|(b, _)| b.clone()).map(|b| {
                Box::new(move |root: &std::path::Path| crate::git::branch_log(root, &b))
                    as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
            overlay::GitView::Stashes => p.selected_stash().cloned().map(|st| {
                let name = st.split(':').next().unwrap_or("stash@{0}").to_string();
                Box::new(move |root: &std::path::Path| crate::git::stash_show(root, &name))
                    as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
            overlay::GitView::Tags => p.selected_tag().cloned().map(|t| {
                let name = t.split_whitespace().next().unwrap_or("").to_string();
                Box::new(move |root: &std::path::Path| crate::git::show(root, &name))
                    as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
            overlay::GitView::Reflog => p.selected_reflog().map(|c| c.sha.clone()).map(|sha| {
                Box::new(move |root: &std::path::Path| crate::git::show(root, &sha))
                    as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
            // Blame's preview is the commit that last touched the selected line.
            overlay::GitView::Blame => p
                .blame
                .get(p.cursor())
                .map(|b| b.sha.clone())
                .map(|sha| {
                    Box::new(move |root: &std::path::Path| crate::git::show(root, &sha))
                        as Box<dyn FnOnce(&std::path::Path) -> String + Send>
                }),
            overlay::GitView::Worktrees => p.selected_worktree().cloned().map(|w| {
                let path = crate::git::worktree_path(&w).to_string();
                Box::new(move |_root: &std::path::Path| {
                    let dir = std::path::PathBuf::from(&path);
                    crate::git::log(&dir, 30)
                        .iter()
                        .map(|c| format!("{} {}", c.sha, c.subject))
                        .collect::<Vec<_>>()
                        .join("\n")
                }) as Box<dyn FnOnce(&std::path::Path) -> String + Send>
            }),
        };
        let Some(job) = job else { return };
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let text = job(&root);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Preview(text)));
        });
    }

    /// Runs a git command for the panel and reloads when it lands.
    ///
    /// Only commands git can undo are ever passed here — see `git::run`. `busy`
    /// blocks a second one, so a repeated keypress cannot start two pushes.
    fn git_exec(&mut self, args: Vec<String>) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        if p.busy {
            return;
        }
        p.busy = true;
        p.message = Ok(String::new());
        let root = p.root.clone();
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let out = crate::git::run(&root, &args);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Ran(args, out)));
        });
        self.window.request_redraw();
    }

    /// Opens a new tab with its shell started in `dir`. Used to visit a worktree:
    /// the panel lists them, and a terminal's answer to "show me that one" is a
    /// shell already sitting in it.
    fn new_tab_in(&mut self, config: &Config, dir: std::path::PathBuf) {
        let area = self.active_area();
        let id = self.new_pane_id();
        let wake = wake_fn(self.proxy.clone());
        let spawn = Spawn { cwd: Some(dir), ..Default::default() };
        if let Ok(tab) = Tab::new(area, self.renderer.cell_size(), config, id, &spawn, wake) {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
            self.reflow_all();
        }
    }

    /// Cell coordinates of a pointer position, for the panel's hit test.
    fn cell_at(&self, pos: PhysicalPosition<f64>) -> (usize, usize, usize, usize) {
        let (cw, ch) = self.renderer.cell_size();
        let screen = (self.surface_config.width as f32, self.surface_config.height as f32);
        let cols = (screen.0 / cw).floor().max(1.0) as usize;
        let rows = (screen.1 / ch).floor().max(1.0) as usize;
        let col = (pos.x as f32 / cw).floor().max(0.0) as usize;
        let row = (pos.y as f32 / ch).floor().max(0.0) as usize;
        (cols, rows, col, row)
    }

    /// Whether the pointer is over one of the panel's LIST columns rather than its
    /// diff — which decides whether the wheel moves a selection or scrolls a diff.
    fn git_pointer_over_list(&self, pos: PhysicalPosition<f64>) -> bool {
        let Some(Overlay::Git(p)) = &self.overlay else { return false };
        let (cols, rows, col, row) = self.cell_at(pos);
        matches!(
            p.hit(cols, rows, col, row),
            Some(crate::overlay::GitHit::Row(_) | crate::overlay::GitHit::FileRow(_))
        )
    }

    /// Whether the pointer is over the open commit's file column.
    fn git_pointer_over_files(&self, pos: PhysicalPosition<f64>) -> bool {
        let Some(Overlay::Git(p)) = &self.overlay else { return false };
        let (cols, rows, col, row) = self.cell_at(pos);
        matches!(p.hit(cols, rows, col, row), Some(crate::overlay::GitHit::FileRow(_)))
    }

    /// Drops the column-resize pointer once the panel that owned it is gone.
    ///
    /// The pointer is otherwise only reconciled on motion, so closing the panel with
    /// `q` while hovering a separator left the resize arrow over the terminal until
    /// the mouse happened to move.
    fn sync_git_cursor(&mut self) {
        if self.git_over_split && !matches!(self.overlay, Some(Overlay::Git(_))) {
            self.git_over_split = false;
            self.window.set_cursor(winit::window::CursorIcon::Default);
        }
    }

    /// Drags a column separator to the pointer. Called from the motion handler for
    /// as long as the button is down, exactly like a pane divider.
    fn git_drag_split(&mut self, pos: PhysicalPosition<f64>) {
        let Some(sep) = self.git_drag else { return };
        let (cols, rows, col, _) = self.cell_at(pos);
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        let l = p.layout(cols, rows);
        p.drag_split(sep, col.saturating_sub(l.col), l.w);
        self.window.request_redraw();
    }

    /// A left click inside the git panel: a view tab switches view, a list row
    /// selects it — and a click on the row that is already selected opens it, the
    /// way a file manager works — a separator starts a resize, and a diff row picks
    /// the hunk a stage key acts on.
    fn git_panel_click(&mut self, pos: PhysicalPosition<f64>, config: &Config) {
        let (cols, rows, col, row) = self.cell_at(pos);
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        let Some(hit) = p.hit(cols, rows, col, row) else {
            // Outside the panel entirely reads as "put this away".
            self.overlay = None;
            self.sync_git_cursor();
            self.window.request_redraw();
            return;
        };
        let mut activate = false;
        let mut zoom = false;
        match hit {
            crate::overlay::GitHit::View(v) => p.set_view(v),
            crate::overlay::GitHit::Row(i) => {
                if i >= p.len() {
                    return;
                }
                // Clicking a column also moves the keyboard into it: the cursor and
                // the pointer disagreeing about which column is live is how a later
                // j/k lands somewhere nobody was looking.
                activate = p.cursor() == i && p.focus == crate::overlay::GitFocus::List;
                p.focus = crate::overlay::GitFocus::List;
                p.set_cursor(i);
            }
            crate::overlay::GitHit::FileRow(i) => {
                if i >= p.files_len() {
                    return;
                }
                // A second click on the file already selected opens it full width —
                // the same "click what is selected to open it" as the list.
                zoom = p.files_cursor() == i && p.focus == crate::overlay::GitFocus::Files;
                p.focus = crate::overlay::GitFocus::Files;
                p.set_files_cursor(i);
            }
            crate::overlay::GitHit::Separator(i) => {
                self.git_drag = Some(i);
                return;
            }
            crate::overlay::GitHit::PreviewLine(line) => {
                if let Some(h) = p.hunk_at(line) {
                    p.hunk = h;
                }
            }
            crate::overlay::GitHit::Header => {}
        }
        if zoom {
            p.toggle_zoom();
            self.window.request_redraw();
            return;
        }
        if activate {
            self.git_panel_key(&Key::Named(NamedKey::Enter), config);
            return;
        }
        self.git_preview();
        self.window.request_redraw();
    }

    /// The git panel's leader layer. Returns whether it consumed the key.
    ///
    /// The panel gets its own tree rather than the global one: with it open, the
    /// keyboard is for git, and "new tab" or "split pane" under the same letters
    /// would be a different meaning for the same muscle memory. Every leaf presses
    /// a key the panel already has, so the letters and the leader can never drift.
    fn git_leader_key(&mut self, key: &Key, mods: ModifiersState, config: &Config) -> bool {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return false };
        // A rebase being planned owns the keyboard, leader included.
        if p.rebase.is_some() {
            return false;
        }
        // Armed by the configured leader chord, so rebinding it rebinds this too —
        // resolved through `leader_chord`, the same fallback the global layer uses.
        // Parsing it raw here left an unparseable value with a working global layer
        // and an unreachable panel one.
        let configured = crate::actions::leader_chord(&config.leader);
        let is_leader = match (configured, Chord::from_event(key, mods)) {
            (Some(l), Some(c)) => l == c,
            _ => false,
        };
        if is_leader {
            // Pressing it again puts the layer away, the way it opened it.
            if p.leader.is_some() {
                p.cancel_leader();
            } else {
                p.arm_leader();
            }
            self.window.request_redraw();
            return true;
        }
        if p.leader.is_none() {
            return false;
        }
        // A character held with ctrl/alt/super is a shortcut attempt, not a choice
        // on this layer: Ctrl+C with the menu up must not descend into Commit. This
        // runs before `overlay_key`'s own modifier filter, so it has to repeat it —
        // and it has to come after the leader chord, which is a modifier chord.
        if matches!(key, Key::Character(_))
            && (mods.control_key() || mods.alt_key() || mods.super_key())
        {
            return false;
        }
        let press = match key {
            Key::Named(NamedKey::Escape) => {
                p.cancel_leader();
                None
            }
            Key::Named(NamedKey::Space) => p.leader_key(' '),
            Key::Character(s) => s.chars().next().and_then(|c| p.leader_key(c)),
            // A bare modifier on the way to the next key must not end the sequence.
            _ => None,
        };
        if let Some(press) = press {
            self.run_git_press(press, config);
        }
        self.window.request_redraw();
        true
    }

    /// Runs a leader leaf by pressing the panel key it stands for.
    fn run_git_press(&mut self, press: crate::overlay::GitPress, config: &Config) {
        use crate::overlay::{GitKey, GitPress};
        let as_key = |k: GitKey| match k {
            GitKey::Ch(c) => Key::Character(winit::keyboard::SmolStr::new(c.to_string())),
            GitKey::Space => Key::Named(NamedKey::Space),
            GitKey::Enter => Key::Named(NamedKey::Enter),
        };
        match press {
            crate::overlay::GitPress::View(v) => {
                if let Some(Overlay::Git(p)) = &mut self.overlay {
                    p.set_view(v);
                }
                self.git_preview();
                self.window.request_redraw();
            }
            GitPress::Key(k) | GitPress::In(_, k) | GitPress::InDiff(k) => {
                self.git_panel_key(&as_key(k), config)
            }
            GitPress::Then(v, k) => {
                if let Some(Overlay::Git(p)) = &mut self.overlay {
                    if p.view != v {
                        p.set_view(v);
                    }
                }
                self.git_panel_key(&as_key(k), config);
            }
        }
    }

    /// Loads blame for a file and switches to the blame view.
    fn git_load_blame(&mut self, path: String) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        let root = p.root.clone();
        p.blame_path = path.clone();
        p.set_view(crate::overlay::GitView::Blame);
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let rows = crate::git::blame(&root, &path);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::Blame(rows)));
        });
    }

    /// Opens the interactive-rebase planner for everything above `sha`.
    ///
    /// The base is the SELECTED commit: git replays what comes after it, so picking
    /// the commit you want to keep untouched is the reading that matches the list.
    fn git_start_rebase(&mut self, sha: String) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        // Commits newer than the base, in the order the log shows them. Graph-art
        // rows carry no sha and are not steps.
        let mut steps = Vec::new();
        for c in &p.log {
            if c.sha.is_empty() {
                continue;
            }
            if c.sha == sha {
                break;
            }
            steps.push(c.clone());
        }
        if steps.is_empty() {
            p.message = Err("nothing above that commit to rebase".into());
            return;
        }
        p.rebase = Some(crate::overlay::RebasePlan::new(sha, steps));
        self.window.request_redraw();
    }

    /// Keys while an interactive rebase is being planned.
    fn rebase_plan_key(&mut self, key: &Key, config: &Config) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        let Some(plan) = &mut p.rebase else { return };
        use crate::git::RebaseAction as A;
        let mut run = false;
        match key {
            Key::Named(NamedKey::Escape) => p.rebase = None,
            Key::Named(NamedKey::Enter) => run = true,
            Key::Named(NamedKey::ArrowDown) => {
                plan.cursor = (plan.cursor + 1).min(plan.steps.len().saturating_sub(1))
            }
            Key::Named(NamedKey::ArrowUp) => plan.cursor = plan.cursor.saturating_sub(1),
            Key::Character(c) => match c.as_str() {
                "j" => plan.cursor = (plan.cursor + 1).min(plan.steps.len().saturating_sub(1)),
                "k" => plan.cursor = plan.cursor.saturating_sub(1),
                "J" => plan.move_step(1),
                "K" => plan.move_step(-1),
                "p" => plan.set_action(A::Pick),
                "r" => plan.set_action(A::Reword),
                "e" => plan.set_action(A::Edit),
                "s" => plan.set_action(A::Squash),
                "f" => plan.set_action(A::Fixup),
                "d" => plan.set_action(A::Drop),
                "q" => p.rebase = None,
                _ => {}
            },
            _ => {}
        }
        if run {
            self.run_rebase_plan(config);
        }
        self.window.request_redraw();
    }

    /// Runs the planned rebase on a worker, with the todo we wrote.
    fn run_rebase_plan(&mut self, _config: &Config) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        let Some(plan) = p.rebase.take() else { return };
        if p.busy {
            return;
        }
        p.busy = true;
        p.message = Ok("rebasing…".into());
        let root = p.root.clone();
        let (onto, todo) = (plan.onto.clone(), plan.todo());
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let out = crate::git::run_rebase_interactive(&root, &onto, &todo);
            let _ = proxy.send_event(UserEvent::GitPanel(
                seq,
                crate::git::PanelMsg::Ran(vec!["rebase".into(), "-i".into()], out),
            ));
        });
        self.window.request_redraw();
    }

    /// Loads the file list of the commit the panel drilled into.
    fn git_load_commit_files(&mut self) {
        let Some(Overlay::Git(p)) = &self.overlay else { return };
        let (Some(sha), root) = (p.open_commit.clone(), p.root.clone()) else { return };
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let files = crate::git::commit_files(&root, &sha);
            let _ = proxy.send_event(UserEvent::GitPanel(seq, crate::git::PanelMsg::CommitFiles(files)));
        });
    }

    /// Stages or unstages one hunk, by feeding a rebuilt patch to `git apply
    /// --cached`. Same worker + busy discipline as `git_exec`.
    fn git_apply(&mut self, patch: String, reverse: bool) {
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        if p.busy {
            return;
        }
        p.busy = true;
        let root = p.root.clone();
        self.git_gen += 1;
        let seq = self.git_gen;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let out = crate::git::apply_patch(&root, &patch, reverse);
            let _ = proxy.send_event(UserEvent::GitPanel(
                seq,
                crate::git::PanelMsg::Ran(vec!["apply".into()], out),
            ));
        });
        self.window.request_redraw();
    }

    /// Applies a worker message to the panel. Stale generations are dropped, except
    /// a command's result, which is always worth showing.
    fn on_git_panel_msg(&mut self, seq: u64, msg: crate::git::PanelMsg, config: &Config) {
        let current = self.git_gen;
        let mut reload = false;
        let mut preview = false;
        let mut rerun: Option<Vec<String>> = None;
        if let Some(Overlay::Git(p)) = &mut self.overlay {
            match msg {
                crate::git::PanelMsg::Files(f) => {
                    p.files = f;
                    preview = true;
                }
                crate::git::PanelMsg::Log(l) => p.log = l,
                crate::git::PanelMsg::Branches(b, r, cur) => {
                    p.branches = b;
                    p.remotes = r;
                    p.current_branch = cur;
                }
                crate::git::PanelMsg::Blame(rows) => {
                    p.blame = rows;
                    preview = true;
                }
                // Guarded like the preview, and for a worse failure than a stale
                // diff: a slow commit's file list landing under a different sha
                // makes every preview after it ask that commit for a path it does
                // not have. A list for a commit that has since been closed is
                // dropped outright, or it would repopulate a column nobody opened.
                crate::git::PanelMsg::CommitFiles(f) => {
                    if seq == current && p.in_commit() {
                        p.commit_files = f;
                        p.commit_cursor = 0;
                        preview = true;
                    }
                }
                crate::git::PanelMsg::Tags(t) => p.tags = t,
                crate::git::PanelMsg::Reflog(r) => p.reflog = r,
                crate::git::PanelMsg::Worktrees(w) => p.worktrees = w,
                crate::git::PanelMsg::Stashes(s) => p.stashes = s,
                crate::git::PanelMsg::Preview(text) => {
                    // Only the newest request may paint: a fast j/k run would
                    // otherwise leave an older diff on screen.
                    if seq == current {
                        p.set_preview(text);
                    }
                }
                crate::git::PanelMsg::Ran(args, result) => {
                    p.busy = false;
                    // A credential prompt cannot be answered from a worker with no
                    // terminal. Rather than fail, hand the SAME command to a real
                    // pane, where ssh and git ask the way they always do.
                    if let Err(e) = &result {
                        if crate::git::needs_terminal(e) {
                            let mut cmd = vec!["git".to_string()];
                            cmd.extend(args);
                            rerun = Some(cmd);
                        }
                    }
                    p.message = result;
                    reload = true;
                }
            }
        }
        if let Some(cmd) = rerun {
            self.split_running(config, cmd);
            if let Some(Overlay::Git(p)) = &mut self.overlay {
                p.message = Err("needs a terminal — rerunning in a split".into());
            }
        }
        if reload {
            self.git_reload(config);
        } else if preview {
            self.git_preview();
        }
        self.window.request_redraw();
    }

    /// Keys inside the git panel. Every one of these acts at once — the panel binds
    /// nothing that discards uncommitted work, so there is nothing here to confirm.
    fn git_panel_key(&mut self, key: &Key, config: &Config) {
        use overlay::GitView;
        let Some(Overlay::Git(p)) = &mut self.overlay else { return };
        // A rebase being planned owns the keyboard: every key here means something
        // about the plan, and letting the ordinary bindings through would fire a
        // push or a checkout in the middle of writing one.
        if p.rebase.is_some() {
            self.rebase_plan_key(key, config);
            return;
        }
        let view = p.view;
        let mut moved = false;
        let mut exec: Option<Vec<String>> = None;
        let mut copy: Option<String> = None;
        let mut split: Option<Vec<String>> = None;
        let mut prompt: Option<(PromptKind, &str)> = None;
        let mut close = false;
        let mut reload = false;
        let mut open_dir: Option<String> = None;
        // A commit to drill into: the list becomes its files.
        let mut drill: Option<String> = None;
        let mut blame_path: Option<String> = None;
        let mut rebase_from: Option<String> = None;
        // A hunk patch to stage (or, reversed, unstage).
        let mut patch: Option<(String, bool)> = None;

        let s = |v: &str| v.to_string();
        match key {
            // Escape backs out of an open commit before it closes the panel: the
            // drill-down is a place you are in, not a mode you toggled.
            Key::Named(NamedKey::Escape) => {
                // Peel one layer at a time: the zoom and the diff focus (both undone
                // by `leave_diff`, which puts the keyboard back in the column that
                // chose this file), then an open commit, then the blame view, and
                // only then the panel itself.
                if p.zoom || p.diff_focus() {
                    p.leave_diff();
                } else if p.leave_commit() {
                    moved = true;
                } else if p.view == GitView::Blame {
                    p.set_view(GitView::Status);
                    moved = true;
                } else {
                    close = true;
                }
            }
            Key::Named(NamedKey::Tab) => {
                p.cycle_view();
                moved = true;
            }
            // The arrows are j/k, focus guard included: with the diff focused they
            // walk the DIFF. Moving the list from there would drop the zoom, close
            // the commit and change the selection, all on one keypress.
            Key::Named(NamedKey::ArrowDown) if p.diff_focus() => p.step_diff(1),
            Key::Named(NamedKey::ArrowUp) if p.diff_focus() => p.step_diff(-1),
            Key::Named(NamedKey::ArrowDown) => {
                p.down();
                moved = true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                p.up();
                moved = true;
            }
            // Space arrives as a NamedKey, not as the character " " — staging is the
            // panel's most-pressed key and binding it on the character alone made it
            // silently do nothing.
            Key::Named(NamedKey::Space) if view == GitView::Status => {
                if let Some(f) = p.selected_file() {
                    exec = Some(if f.is_staged() && !f.is_unstaged() {
                        vec![s("restore"), s("--staged"), f.path.clone()]
                    } else {
                        vec![s("add"), s("--"), f.path.clone()]
                    });
                }
            }
            Key::Named(NamedKey::PageDown) => p.scroll_preview(10),
            Key::Named(NamedKey::PageUp) => p.scroll_preview(-10),
            // Zoomed, Enter is the way back, exactly like Escape. It cannot mean
            // anything else: the columns the other Enter arms act on are the ones
            // the zoom hid, and zooming leaves the keyboard in the diff, so without
            // this Enter re-entered the commit it was already reading.
            Key::Named(NamedKey::Enter) if p.zoom => p.leave_diff(),
            // In the open commit's file column, Enter opens that file's diff full
            // width. The columns are for finding the change; reading it wants the
            // width back, and Escape brings the columns straight back.
            Key::Named(NamedKey::Enter) if p.focus == crate::overlay::GitFocus::Files => {
                if p.selected_commit_file().is_some() {
                    p.toggle_zoom();
                }
            }
            Key::Named(NamedKey::Enter) => match view {
                GitView::Branches => {
                    if let Some((b, remote)) = p.selected_branch() {
                        // A remote-tracking ref is checked out with --track, which
                        // creates the local branch that follows it; plain checkout
                        // of one lands you on a detached HEAD.
                        exec = Some(if remote {
                            vec![s("switch"), s("--track"), b.clone()]
                        } else {
                            vec![s("checkout"), b.clone()]
                        });
                    }
                }
                GitView::Stashes => {
                    if let Some(st) = p.selected_stash() {
                        let name = st.split(':').next().unwrap_or("stash@{0}").to_string();
                        exec = Some(vec![s("stash"), s("pop"), name]);
                    }
                }
                // Enter opens the commit's FILES. Checking a commit out is `x`:
                // reading a commit is what you do constantly, and moving HEAD onto
                // one is not.
                GitView::Log => {
                    if let Some(c) = p.selected_commit() {
                        drill = Some(c.sha.clone());
                    }
                }
                GitView::Tags => {
                    if let Some(t) = p.selected_tag() {
                        let name = t.split_whitespace().next().unwrap_or("").to_string();
                        exec = Some(vec![s("checkout"), name]);
                    }
                }
                // The reflog exists to get back to a position. Checking one out is
                // the whole point of showing it.
                GitView::Reflog => {
                    if let Some(c) = p.selected_reflog() {
                        drill = Some(c.sha.clone());
                    }
                }
                // A worktree is a directory: opening it is a new tab there, which is
                // what a terminal can do that a git client cannot.
                GitView::Worktrees => {
                    if let Some(w) = p.selected_worktree() {
                        open_dir = Some(crate::git::worktree_path(w).to_string());
                    }
                }
                GitView::Blame => {
                    if let Some(b) = p.blame.get(p.cursor()) {
                        drill = Some(b.sha.clone());
                    }
                }
                _ => {}
            },
            Key::Character(c) => match c.as_str() {
                "q" => close = true,
                // With the diff focused, j/k walk the DIFF, not the list: that is
                // the cursor line staging needs, and one pair of keys cannot mean
                // two things at once.
                "j" if p.diff_focus() => p.step_diff(1),
                "k" if p.diff_focus() => p.step_diff(-1),
                "j" => {
                    p.down();
                    moved = true;
                }
                "k" => {
                    p.up();
                    moved = true;
                }
                // Focus walks the columns, in the order they read.
                "l" if p.in_commit() => p.focus_right(),
                "h" if p.in_commit() => p.focus_left(),
                // Full width for the selected file's diff, and back.
                "z" => p.toggle_zoom(),
                // Focus follows the vim direction keys: l moves into the diff, h
                // back to the list.
                "l" if view == GitView::Status && !p.diff_focus() => {
                    p.enter_diff();
                }
                "h" if p.diff_focus() => p.leave_diff(),
                // v marks the start of a line selection, the way copy mode does.
                "v" if p.diff_focus() => p.toggle_anchor(),
                "J" => p.scroll_preview(5),
                "K" => p.scroll_preview(-5),
                // Hunk selection. `body_rows` is approximated from the panel's own
                // layout rule rather than threaded from the draw path: it only
                // decides when to scroll the hunk into view.
                "]" => p.step_hunk(1, 20),
                "[" => p.step_hunk(-1, 20),
                "s" | "u" if view == GitView::Status => {
                    let reverse = c.as_str() == "u";
                    // Focused on the diff, s and u act on the SELECTED LINES; on the
                    // list they act on the whole hunk. Same keys, and the focus says
                    // which, so there is nothing extra to remember.
                    patch = if p.diff_focus() {
                        p.line_patch().map(|text| (text, reverse))
                    } else {
                        p.hunk_patch().map(|text| (text, reverse))
                    };
                    if patch.is_none() {
                        p.message = Err("nothing to stage there — space stages the file".into());
                    }
                }
                "1" => {
                    p.set_view(GitView::Status);
                    moved = true;
                }
                "2" => {
                    p.set_view(GitView::Log);
                    moved = true;
                }
                "3" => {
                    p.set_view(GitView::Branches);
                    moved = true;
                }
                "4" => {
                    p.set_view(GitView::Stashes);
                    moved = true;
                }
                "5" => {
                    p.set_view(GitView::Tags);
                    moved = true;
                }
                "6" => {
                    p.set_view(GitView::Reflog);
                    moved = true;
                }
                "7" => {
                    p.set_view(GitView::Worktrees);
                    moved = true;
                }
                // A file can be staged and modified at once; show the other diff.
                "t" if view == GitView::Status => {
                    p.show_staged = !p.show_staged;
                    moved = true;
                }
                // Amend keeps the message and folds the staged set into the last
                // commit. Recoverable from the reflog, which is view 6.
                "A" if view == GitView::Status => {
                    exec = Some(vec![s("commit"), s("--amend"), s("--no-edit")])
                }
                // A commit message with a body needs a real editor, so this hands
                // the whole commit to a pane: git opens $EDITOR there, hooks and
                // all, exactly as it would at the prompt.
                "C" if view == GitView::Status => {
                    split = Some(vec![s("git"), s("commit")]);
                }
                // Conflict resolution. Guarded on the file actually being unmerged,
                // so a side can never be picked for a file that has no conflict.
                "O" | "T" if view == GitView::Status => {
                    if let Some(f) = p.selected_file() {
                        if crate::git::is_conflicted(&p.files, &f.path) {
                            let side = if c.as_str() == "O" { s("--ours") } else { s("--theirs") };
                            exec = Some(vec![s("checkout"), side, s("--"), f.path.clone()]);
                        } else {
                            p.message = Err("not a conflicted file".into());
                        }
                    }
                }
                // Open the file under the cursor: how you resolve a conflict by hand.
                "e" if view == GitView::Status => {
                    if let Some(f) = p.selected_file() {
                        split = Some(vec![editor_cmd(), f.path.clone()]);
                    }
                }
                // History of just this file, in a split, so the log view stays where
                // it was.
                "L" if view == GitView::Status => {
                    if let Some(f) = p.selected_file() {
                        split = Some(vec![
                            s("git"),
                            s("log"),
                            s("--follow"),
                            s("--patch"),
                            s("--"),
                            f.path.clone(),
                        ]);
                    }
                }
                // Blame is a VIEW: every line with the commit that last touched it,
                // and Enter opens that commit. A pager could show the same text but
                // could not take you from a line to its history.
                "b" if view == GitView::Status => {
                    if let Some(f) = p.selected_file() {
                        blame_path = Some(f.path.clone());
                    }
                }
                "m" if view == GitView::Branches => {
                    if let Some((b, _)) = p.selected_branch() {
                        exec = Some(vec![s("merge"), s("--no-edit"), b.clone()]);
                    }
                }
                "R" if view == GitView::Branches => {
                    if let Some((b, _)) = p.selected_branch() {
                        exec = Some(vec![s("rebase"), b.clone()]);
                    }
                }
                "x" if view == GitView::Log || view == GitView::Reflog => {
                    let sha = match view {
                        GitView::Log => p.selected_commit().map(|c| c.sha.clone()),
                        _ => p.selected_reflog().map(|c| c.sha.clone()),
                    };
                    if let Some(sha) = sha {
                        exec = Some(vec![s("checkout"), sha]);
                    }
                }
                // An interactive rebase of everything above the selected commit,
                // planned inside the panel rather than in an editor.
                "i" if view == GitView::Log => {
                    if let Some(c) = p.selected_commit() {
                        rebase_from = Some(c.sha.clone());
                    }
                }
                "c" if view == GitView::Log => {
                    if let Some(cm) = p.selected_commit() {
                        exec = Some(vec![s("cherry-pick"), cm.sha.clone()]);
                    }
                }
                "n" if view == GitView::Tags => prompt = Some((PromptKind::GitTag, "New tag")),
                "P" if view == GitView::Tags => {
                    exec = Some(vec![s("push"), s("--tags")]);
                }
                "r" => reload = true,
                "/" if view == GitView::Log => {
                    prompt = Some((PromptKind::GitLogFilter, "Filter log by message"))
                }
                "a" if view == GitView::Status => exec = Some(vec![s("add"), s("-A")]),
                "c" if view == GitView::Status => {
                    prompt = Some((PromptKind::GitCommit, "Commit message"))
                }
                "S" => exec = Some(vec![s("stash"), s("push"), s("-u")]),
                "n" if view == GitView::Branches => {
                    prompt = Some((PromptKind::GitBranch, "New branch"))
                }
                "y" if p.in_commit() => {
                    copy = p.selected_commit_file().map(|f| f.path.clone());
                }
                "y" => {
                    copy = match view {
                        GitView::Log => p.selected_commit().map(|c| c.sha.clone()),
                        GitView::Status => p.selected_file().map(|f| f.path.clone()),
                        GitView::Branches => p.selected_branch().map(|(b, _)| b.clone()),
                        GitView::Stashes => p.selected_stash().cloned(),
                        GitView::Tags => p.selected_tag().cloned(),
                        GitView::Reflog => p.selected_reflog().map(|c| c.sha.clone()),
                        GitView::Worktrees => {
                            p.selected_worktree().map(|w| crate::git::worktree_path(w).to_string())
                        }
                        GitView::Blame => p.blame.get(p.cursor()).map(|b| b.sha.clone()),
                    }
                }
                "o" if view == GitView::Log => {
                    split = p.selected_commit().map(|c| {
                        vec![s("git"), s("show"), s("--stat"), s("--patch"), c.sha.clone()]
                    })
                }
                // The first push of a branch has no upstream to push to; push_args
                // adds `-u origin HEAD` exactly then, so a new branch works without
                // the user having to know.
                "P" => exec = Some(crate::git::push_args(&p.root)),
                "p" => exec = Some(vec![s("pull"), s("--ff-only")]),
                "f" => exec = Some(vec![s("fetch"), s("--all"), s("--prune")]),
                _ => {}
            },
            _ => {}
        }

        if close {
            self.overlay = None;
        }
        if let Some((kind, label)) = prompt {
            // The panel is dropped for the prompt and rebuilt on confirm: the prompt
            // owns the overlay slot, and the panel's state is all refetched anyway.
            self.overlay = Some(Overlay::Prompt(Prompt::new(kind, label, Vec::new())));
        }
        if let Some(text) = copy {
            self.set_clipboard(text);
        }
        if let Some(cmd) = split {
            self.split_running(config, cmd);
            self.overlay = None;
        }
        if let Some(dir) = open_dir {
            self.overlay = None;
            self.new_tab_in(config, std::path::PathBuf::from(dir));
        }
        if let Some(path) = blame_path {
            self.git_load_blame(path);
        }
        if let Some(sha) = rebase_from {
            self.git_start_rebase(sha);
        }
        if let Some(sha) = drill {
            if let Some(Overlay::Git(p)) = &mut self.overlay {
                p.enter_commit(sha);
            }
            self.git_load_commit_files();
        }
        if let Some((text, reverse)) = patch {
            self.git_apply(text, reverse);
        } else if let Some(args) = exec {
            self.git_exec(args);
        } else if reload {
            self.git_reload(config);
        } else if moved {
            self.git_preview();
        }
        self.sync_git_cursor();
        self.window.request_redraw();
    }

    /// Acts on the hovered URL/path if the pointer is over one: opens a URL in the
    /// browser, copies a path or hash. Returns whether it consumed the click.
    fn open_hover(&mut self, config: &Config) -> bool {
        // Recompute against the pointer's current position first: a keyboard tab
        // switch or a scroll can leave a stale target under the old coordinates.
        self.update_hover(self.cursor_px);
        let Some(h) = self.hover_url.clone() else { return false };
        // Ctrl+click always takes the plain action. The alternate one is reachable
        // from hint mode, where a shifted label says so explicitly; a click cannot
        // carry that intent without stealing another modifier chord.
        match crate::hints::act(&h.text, h.kind, false) {
            crate::hints::HintAct::Copy(text) => self.set_clipboard(text),
            crate::hints::HintAct::Done => {}
            crate::hints::HintAct::Split(cmd) => self.split_running(config, cmd),
        }
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

    /// The single sink every copy runs through: it records the text in the in-memory
    /// clipboard history (for the Super+V picker) and then sets the system clipboard.
    fn set_clipboard(&mut self, text: String) {
        self.clip_history.push(&text);
        self.clipboard.set(&text);
    }

    /// Opens the clipboard-history picker over the current history snapshot.
    fn open_clip_history(&mut self) {
        self.overlay = Some(Overlay::ClipHistory(overlay::ClipHistoryPicker::new(
            self.clip_history.entries(),
        )));
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

    /// A file dropped onto the window types its full path at the prompt.
    ///
    /// `at` is where it landed, when the platform tells us; winit's X11 drop
    /// carries no coordinates, so `None` means the focused pane — the predictable
    /// answer, and the only honest one when we do not know where the pointer was.
    ///
    /// The path is typed, never run: no newline is ever sent, so what happens to
    /// it is the shell's business (or vim's, or whatever holds the pane).
    fn on_files_dropped(&mut self, paths: &[PathBuf], at: Option<PhysicalPosition<f64>>) {
        if paths.is_empty() {
            return;
        }
        if let Some(pos) = at {
            let area = self.active_area();
            if let Some((id, _)) = self.pane_at(pos, area) {
                self.tabs[self.active].focus = id;
            }
        }
        // Trailing space so a second drop lands as a second argument and you can
        // keep typing without one.
        let mut text = String::new();
        for p in paths {
            text.push_str(&shell_quote(&p.to_string_lossy()));
            text.push(' ');
        }
        self.paste_text(text);
        self.window.request_redraw();
    }

    /// Applies a freshly-loaded config live (hot-reload): theme, opacity and font.
    /// Key bindings are rebuilt by the caller (they live on `App`, not `Gpu`).
    fn apply_config(&mut self, config: &Config) {
        self.leader_timeout = crate::leader_timeout(config);
        // A status-bar toggle changes the content height, so reflow after.
        if self.status_bar != config.window.status_bar {
            self.status_bar = config.window.status_bar;
            self.reflow_all();
        }
        // Same for the minimap: toggling it changes how much width the text grid may
        // use, so every tab adopts the new setting before the reflow.
        if self.tabs.iter().any(|t| t.minimap() != config.window.minimap) {
            for tab in &mut self.tabs {
                tab.set_minimap(config.window.minimap);
            }
            self.reflow_all();
        }
        self.renderer.set_theme(config.theme.clone());
        // Adopt clipboard-history sizing/enablement (trims the ring if it shrank).
        self.clip_history.configure(config.clipboard.capacity, config.clipboard.enabled);
        // Opacity when the compositor shows through OR a background image is set
        // (same reasoning as at startup).
        let want_opacity = self.translucent || config.window.background.is_some();
        self.renderer
            .set_opacity(if want_opacity { config.window.opacity } else { 1.0 });
        // Reload the background only when its path/dim changed (decoding is expensive).
        let bg = (config.window.background.clone(), config.window.background_dim);
        if bg != self.applied_bg {
            self.applied_bg = bg.clone();
            crate::load_background(config, &self.device, &self.queue, &mut self.renderer);
        }
        // Rebuild the font only when the CONFIG's font actually changed (family, size
        // or ligatures) — compared against what the config last asked for, not the
        // live size, so a colour-only edit does not snap a runtime zoom back, and a
        // family/ligature change (same size) is applied.
        let want = (config.font.family.clone(), config.font.size, config.font.ligatures);
        if want != self.applied_font {
            match FontAtlas::new_with(&config.font.family, config.font.size * self.scale) {
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

    /// Sets the font size in *logical* pixels; the atlas is rasterised at
    /// `px * scale` so a given size looks the same on every monitor.
    fn set_font_px(&mut self, px: f32, config: &Config) {
        let px = px.clamp(6.0, 72.0);
        if (px - self.font_px).abs() < 0.5 {
            return;
        }
        if let Ok(mut font) = FontAtlas::new_with(&config.font.family, px * self.scale) {
            font.ligatures = config.font.ligatures;
            self.renderer.replace_font(&self.device, font);
            self.font_px = px;
            self.reflow_all();
        }
    }

    /// Adopts a new display scale factor, re-rasterising the atlas at the same
    /// logical font size. Separate from `set_font_px` because the logical size is
    /// unchanged here — its early-return would swallow the rebuild.
    fn set_scale(&mut self, scale: f32, config: &Config) {
        if !scale.is_finite() || scale <= 0.0 || (scale - self.scale).abs() < 0.01 {
            return;
        }
        self.scale = scale;
        if let Ok(mut font) = FontAtlas::new_with(&config.font.family, self.font_px * scale) {
            font.ligatures = config.font.ligatures;
            self.renderer.replace_font(&self.device, font);
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

    /// Opens the small text-input overlay to type a filter command; confirming
    /// pipes the captured text through it (see `pipe_through`).
    fn open_pipe_prompt(&mut self, kind: PromptKind) {
        let label = match kind {
            PromptKind::PipeScrollback => "Pipe scrollback through",
            _ => "Pipe last output through",
        };
        self.overlay = Some(Overlay::Prompt(Prompt::new(kind, label, Vec::new())));
        self.window.request_redraw();
    }

    /// Captures text from the focused pane — the last OSC 133 output block, or the
    /// whole scrollback when `whole` — writes it to a private temp file, and opens
    /// a new split running the user's command with that text on stdin.
    fn pipe_through(&mut self, command: String, whole: bool, config: &Config) {
        let Some(text) = self.tab().focused().pipe_text(whole) else {
            self.status = Some("no command output marked yet (needs shell integration)".into());
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        };
        // Per-pane filename (the pty pid) in $XDG_RUNTIME_DIR (a per-user 0700 dir),
        // so a fresh capture overwrites the stale one and the text — which may hold
        // secrets — is never left world-readable or predictably pre-emptable.
        let pid = self.tab().focused().pty_pid().unwrap_or(0);
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("runnir-pipe-{pid}.txt"));
        if let Err(e) = write_private(&path, text.as_bytes()) {
            self.status = Some(format!("could not write pipe input: {e}"));
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        }
        // Run the user's command through a POSIX shell with the captured text on
        // stdin. The file path is passed as $1 (not interpolated into the script)
        // so an odd path can neither break the redirect nor inject the command;
        // the command itself is the user's, so it keeps full shell power.
        let script = format!("{command} < \"$1\"");
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            script,
            "runnir-pipe".to_string(),
            path.to_string_lossy().into_owned(),
        ];
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

    /// Opens the snippet picker: fuzzy-choose a saved command bookmark. Selecting it
    /// types the command at the focused prompt for review (or runs it, if the snippet
    /// set `run_now`). Nothing happens if no `[[snippets]]` are configured.
    fn open_snippet_picker(&mut self, config: &Config) {
        if config.snippets.is_empty() {
            self.status = Some("no [[snippets]] configured — see the docs".into());
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        }
        self.overlay =
            Some(Overlay::Snippets(overlay::SnippetPicker::new(config.snippets.clone())));
        self.window.request_redraw();
    }

    /// Applies a chosen snippet. By default the command is typed at the prompt via the
    /// same review-first path the AI command-writer uses (`insert_command`), so you
    /// press Enter yourself. A snippet with `run_now = true` is submitted immediately,
    /// exactly as the assistant path does when it runs a command.
    fn use_snippet(&mut self, snip: crate::config::SnippetDef) {
        if snip.command.trim().is_empty() {
            return;
        }
        self.insert_command(snip.command);
        if snip.run_now {
            // insert_command already snapped to the bottom and typed the line; this is
            // just the Enter, sent to the same focused pane it was typed into.
            self.tab().focused().write(b"\r");
        }
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
        let spawn = Spawn { command: (!first.is_empty()).then_some(first), cwd: None, ..Default::default() };
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

    // ------------------------------------------------------------------
    // Image auto-preview watch: poll a directory and preview new drops.
    // ------------------------------------------------------------------

    /// Arms the watch on a directory: snapshots what is there now (so only new files
    /// fire) and remembers the extension filter and preview width from the config.
    fn arm_image_watch(&mut self, dir: std::path::PathBuf, config: &Config) {
        let exts = config.watch.extensions.clone();
        let listing = crate::watch::list_dir(&dir, &exts);
        let state = crate::watch::WatchState::armed(&listing);
        self.image_watch = Some(ImageWatch {
            dir,
            state,
            exts,
            max_width: config.watch.max_width.clamp(1, 1000),
            last_poll: Instant::now(),
        });
    }

    /// Toggles the watch on the focused pane's working directory: off if already
    /// watching, otherwise armed on that cwd. Shows a toast either way.
    fn toggle_image_watch(&mut self, config: &Config) {
        if self.image_watch.is_some() {
            self.image_watch = None;
            self.toast("image auto-preview off", 2);
            return;
        }
        match self.tab().focused_ref().cwd() {
            Some(dir) => {
                let shown = dir.display().to_string();
                self.arm_image_watch(dir, config);
                self.toast(&format!("watching {shown} for images"), 3);
            }
            None => self.toast("no working directory to watch", 3),
        }
    }

    /// Opens a prompt to set (or clear, with an empty line) the watched directory at
    /// runtime, pre-filled with the current one.
    fn set_image_watch_dir(&mut self) {
        let current = self
            .image_watch
            .as_ref()
            .map(|w| w.dir.display().to_string())
            .unwrap_or_default();
        let mut prompt = Prompt::new(
            PromptKind::ImageWatchDir,
            "Watch directory for images (empty clears)",
            Vec::new(),
        );
        for c in current.chars() {
            prompt.input_char(c);
        }
        self.overlay = Some(Overlay::Prompt(prompt));
        self.window.request_redraw();
    }

    /// Polls the watched directory (throttled to the poll interval) and previews the
    /// newest file that has become stable since the last poll. A no-op unless a watch
    /// is armed; never blocks. Skipped while the focused pane is on the alternate
    /// screen (vim/htop), so a full-screen app is never disrupted mid-use — the new
    /// files stay unseen and are picked up once the app exits.
    fn poll_image_watch(&mut self, _config: &Config) {
        let Some(w) = self.image_watch.as_ref() else { return };
        if w.last_poll.elapsed() < Duration::from_millis(WATCH_POLL_MS) {
            return;
        }
        if self.tab().focused_ref().grid.lock().unwrap().alt_screen() {
            return;
        }
        // Re-borrow mutably now the throttle and alt-screen guards have passed.
        let Some(w) = self.image_watch.as_mut() else { return };
        w.last_poll = Instant::now();
        let listing = crate::watch::list_dir(&w.dir, &w.exts);
        let ready = w.state.step(&listing);
        let max_width = w.max_width;
        // Only the newest of a batch is previewed (step returns oldest-first); the
        // rest are already marked seen, so they neither re-fire nor pile up.
        if let Some(path) = ready.into_iter().next_back() {
            self.preview_image(&path, max_width);
        }
    }

    /// Decodes an image file and places it inline in the focused pane through the
    /// existing kitty-graphics placement path (`Grid::place_image`), scaled down so
    /// it is at most `max_width` cells wide and never exceeds the GPU's max texture
    /// size (the 8192px guard the background path also honours). Reuses the image
    /// drawing code entirely — this only builds the decoded RGBA to hand it.
    fn preview_image(&mut self, path: &std::path::Path, max_width: usize) {
        let img = match image::open(path) {
            Ok(img) => img,
            Err(e) => {
                self.toast(&format!("preview failed: {e}"), 3);
                return;
            }
        };
        let (cw, _ch) = self.renderer.cell_size();
        // Target pixel width from the cell budget, capped by the texture limit so a
        // huge render can never become an invalid (panicking) texture.
        let cap = self.device.limits().max_texture_dimension_2d.max(1);
        let target_w = ((max_width as f32 * cw).round() as u32).clamp(1, cap);
        // Downscale only when the source is wider than the budget (or the cap); never
        // upscale a small image. thumbnail preserves aspect and clamps both axes.
        let img = if img.width() > target_w || img.height() > cap {
            img.thumbnail(target_w, cap)
        } else {
            img
        };
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        // cols/rows = 0 lets place_image derive the cell footprint from the pixel
        // size and the grid's cell size, so aspect is preserved.
        let decoded = crate::graphics::Image {
            id: 0,
            rgba: rgba.into_raw(),
            width,
            height,
            cols: 0,
            rows: 0,
        };
        {
            let pane = self.tab().focused();
            pane.snap_to_bottom();
            pane.grid.lock().unwrap().place_image(decoded);
        }
        self.window.request_redraw();
    }

    /// Leaves the leader layer: disarms it and forgets the keys pressed so far, so
    /// the next arming starts at the root instead of inside the last group.
    fn cancel_leader(&mut self) {
        self.leader_armed = None;
        self.leader_path.clear();
        self.leader_entries.clear();
    }

    /// Shows a transient toast for `secs` seconds. A small wrapper so the several
    /// image-watch messages read cleanly.
    fn toast(&mut self, msg: &str, secs: u64) {
        self.status = Some(msg.to_string());
        self.status_expiry = Some(Instant::now() + Duration::from_secs(secs));
        self.window.request_redraw();
    }

    // ------------------------------------------------------------------
    // Now-playing media (media.rs). Metadata + waveform are fetched on worker
    // threads and delivered via UserEvent::Media, so the playerctl / cava
    // subprocess never blocks the UI thread.
    // ------------------------------------------------------------------

    /// Opens the now-playing overlay. The metadata fetch is asynchronous: this only
    /// kicks off the worker; the overlay opens (or a "no player" toast shows) once the
    /// result arrives in [`Gpu::on_media_msg`].
    fn open_now_playing(&mut self) {
        self.spawn_media_fetch();
        self.media_last_refresh = Some(Instant::now());
        self.toast("loading now playing\u{2026}", 1);
    }

    /// Spawns a worker that fetches now-playing metadata and delivers it back through
    /// the event-loop proxy. Non-blocking; safe to call on a timer.
    fn spawn_media_fetch(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let np = crate::media::fetch();
            let _ = proxy.send_event(UserEvent::Media(crate::media::MediaMsg::NowPlaying(np)));
        });
    }

    /// Handles a media worker message: a metadata result (open/refresh the overlay, or
    /// toast when nothing is playing) or a waveform frame (fed to the open overlay).
    fn on_media_msg(&mut self, msg: crate::media::MediaMsg, config: &Config) {
        match msg {
            crate::media::MediaMsg::NowPlaying(Some(np)) => {
                // Reuse already-decoded art when the cover path is unchanged (a plain
                // refresh), so an open overlay does not re-decode the same file on every
                // timer tick. Only decode when opening fresh or the cover actually changed.
                let same_art = matches!(
                    self.overlay.as_ref(),
                    Some(Overlay::Media(m)) if m.art_path() == np.art.as_deref()
                );
                let art = if same_art { None } else { Some(self.decode_media_art(&np, config)) };
                match self.overlay.as_mut() {
                    // Already open: refresh in place, keep the waveform worker running.
                    Some(Overlay::Media(m)) => match art {
                        Some(a) => m.set_now_playing(np, a),
                        None => m.set_meta(np),
                    },
                    _ => {
                        // Fresh open: build the overlay and start the waveform worker.
                        let wave_on = config.media.waveform;
                        let overlay =
                            crate::overlay::MediaOverlay::new(np, art.unwrap_or_default(), wave_on);
                        self.overlay = Some(Overlay::Media(overlay));
                        self.media_wave = if wave_on {
                            crate::media::start_waveform(config.media.bars, self.proxy.clone())
                        } else {
                            None
                        };
                    }
                }
                self.window.request_redraw();
            }
            crate::media::MediaMsg::NowPlaying(None) => {
                // A refresh returning nothing while open leaves the last snapshot up; an
                // initial open with no player just toasts.
                if !matches!(self.overlay, Some(Overlay::Media(_))) {
                    self.toast("no media player active", 3);
                }
            }
            crate::media::MediaMsg::Waveform(bars) => {
                if let Some(Overlay::Media(m)) = self.overlay.as_mut() {
                    m.set_wave(bars);
                    self.window.request_redraw();
                }
                // A frame that arrives after the overlay closed is simply dropped.
            }
        }
    }

    /// Decodes an album-art file into half-block cells sized for the overlay, or an
    /// empty grid when there is no local art or it cannot be read. Downscales to a
    /// small thumbnail first so the sampling stays cheap.
    fn decode_media_art(
        &self,
        np: &crate::media::NowPlaying,
        config: &Config,
    ) -> Vec<Vec<crate::media::HalfCell>> {
        let Some(path) = np.art.as_ref() else { return Vec::new() };
        let cols = config.media.art_cells.clamp(4, 40);
        let rows = (cols / 2).max(2);
        let img = match image::open(path) {
            Ok(i) => i.thumbnail(256, 256),
            Err(_) => return Vec::new(),
        };
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        crate::media::halfblock_art(&rgba, w, h, cols, rows)
    }

    // ------------------------------------------------------------------
    // Remote control (control.rs). Runs on the UI thread via
    // `UserEvent::Control`, so it may touch tabs/panes/renderer freely.
    // ------------------------------------------------------------------

    /// Executes one remote-control request against the live terminal and returns the
    /// response the socket thread will serialise back to the client.
    fn handle_control(
        &mut self,
        req: crate::control::ControlRequest,
        config: &Config,
        keymap: &Keymap,
        event_loop: &ActiveEventLoop,
    ) -> crate::control::ControlResponse {
        use crate::control::{ControlRequest, ControlResponse, LaunchTarget};
        use serde_json::json;

        match req {
            // Input, delivered where a real one lands. The answer carries a snapshot
            // of whatever panel is open, so a script can assert what a key did
            // without taking a screenshot of it.
            ControlRequest::Key { chord } => {
                let Some((key, mods)) = crate::actions::chord_to_key(&chord) else {
                    return ControlResponse::error(format!("cannot parse chord {chord:?}"));
                };
                self.press_key(&key, mods, config, keymap, event_loop);
                self.window.request_redraw();
                ControlResponse::ok(self.ui_state())
            }
            ControlRequest::Click { col, row, button } => {
                let btn = match button.as_deref() {
                    None | Some("left") => MouseButton::Left,
                    Some("right") => MouseButton::Right,
                    Some("middle") => MouseButton::Middle,
                    Some(other) => return ControlResponse::error(format!("unknown button {other:?}")),
                };
                self.cursor_px = self.cell_centre(col, row);
                self.on_click(ElementState::Pressed, btn, ModifiersState::empty(), config);
                self.on_click(ElementState::Released, btn, ModifiersState::empty(), config);
                self.window.request_redraw();
                ControlResponse::ok(self.ui_state())
            }
            ControlRequest::Drag { col, row, to_col, to_row } => {
                let from = self.cell_centre(col, row);
                let to = self.cell_centre(to_col, to_row.unwrap_or(row));
                self.cursor_px = from;
                self.on_click(ElementState::Pressed, MouseButton::Left, ModifiersState::empty(), config);
                // Two steps, because a drag handler is allowed to care about motion
                // rather than about the final position — one jump would not exercise
                // what a hand does.
                let mid = PhysicalPosition::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
                self.on_cursor(mid, ModifiersState::empty());
                self.on_cursor(to, ModifiersState::empty());
                self.on_click(ElementState::Released, MouseButton::Left, ModifiersState::empty(), config);
                self.window.request_redraw();
                ControlResponse::ok(self.ui_state())
            }
            ControlRequest::Action { id } => {
                let Some(action) = Action::parse(&id) else {
                    return ControlResponse::error(format!("unknown action {id:?}"));
                };
                self.run_action(action, config, event_loop);
                self.window.request_redraw();
                ControlResponse::ok(self.ui_state())
            }
            ControlRequest::Ls => {
                let active = self.active;
                let tabs: Vec<_> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| {
                        let focus = tab.focused_ptr();
                        let mut panes: Vec<_> = tab
                            .panes
                            .iter()
                            .map(|(id, p)| {
                                json!({
                                    "id": id,
                                    "title": p.title,
                                    "cwd": p.cwd().map(|c| c.display().to_string()),
                                    "focused": *id == focus,
                                })
                            })
                            .collect();
                        // HashMap iteration order is unspecified; sort by id so the
                        // listing is stable across calls.
                        panes.sort_by_key(|p| p["id"].as_u64().unwrap_or(0));
                        json!({
                            "index": i,
                            "title": tab.title(),
                            "active": i == active,
                            "panes": panes,
                        })
                    })
                    .collect();
                ControlResponse::ok(json!({ "active": active, "tabs": tabs }))
            }

            ControlRequest::GetText { target } => match self.control_pane(target) {
                Some(pane) => {
                    let text = pane.scrollback_text().join("\n");
                    ControlResponse::ok(json!({ "text": text }))
                }
                None => ControlResponse::error("no such pane"),
            },

            ControlRequest::SendText { text, target } => match self.control_pane(target) {
                Some(pane) => {
                    pane.write(text.as_bytes());
                    pane.snap_to_bottom();
                    self.window.request_redraw();
                    ControlResponse::ok_empty()
                }
                None => ControlResponse::error("no such pane"),
            },

            ControlRequest::Launch { target, cmd } => {
                let argv = cmd.as_deref().map(argv_of).unwrap_or_default();
                match target {
                    LaunchTarget::Tab => self.control_new_tab(config, argv),
                    LaunchTarget::Split => {
                        self.split_running(config, argv);
                        let id = self.tab().focused_ptr();
                        ControlResponse::ok(json!({ "pane": id }))
                    }
                }
            }

            ControlRequest::NewTab { cmd } => {
                let argv = cmd.as_deref().map(argv_of).unwrap_or_default();
                self.control_new_tab(config, argv)
            }

            ControlRequest::FocusTab { index } => {
                if index < self.tabs.len() {
                    self.active = index;
                    self.window.request_redraw();
                    ControlResponse::ok_empty()
                } else {
                    ControlResponse::error(format!("no tab {index} (have {})", self.tabs.len()))
                }
            }

            ControlRequest::CloseTab { index } => {
                let idx = index.unwrap_or(self.active);
                if idx >= self.tabs.len() {
                    return ControlResponse::error(format!("no tab {idx} (have {})", self.tabs.len()));
                }
                if self.tabs.len() <= 1 {
                    return ControlResponse::error("cannot close the last tab");
                }
                // Remember it so ReopenClosed can bring it back, matching the keybind.
                self.closed_tabs.push(self.tabs[idx].to_session());
                self.tabs.remove(idx);
                self.active = self.active.min(self.tabs.len() - 1);
                self.reflow_all();
                self.window.request_redraw();
                ControlResponse::ok_empty()
            }

            ControlRequest::SetColors { opacity, foreground, background, accent, cursor } => {
                let mut cfg = config.clone();
                if let Some(o) = opacity {
                    cfg.window.opacity = o.clamp(0.0, 1.0);
                }
                let colors = [
                    (foreground, &mut cfg.theme.foreground),
                    (background, &mut cfg.theme.background),
                    (accent, &mut cfg.theme.accent),
                    (cursor, &mut cfg.theme.cursor),
                ];
                for (hex, slot) in colors {
                    if let Some(hex) = hex {
                        match serde_json::from_value::<crate::config::Rgb>(serde_json::Value::String(hex.clone())) {
                            Ok(rgb) => *slot = rgb,
                            Err(_) => return ControlResponse::error(format!("bad colour: {hex:?}")),
                        }
                    }
                }
                self.apply_config(&cfg);
                ControlResponse::ok_empty()
            }
        }
    }

    /// Resolves a control target to a pane: an explicit id (searched across every
    /// tab) or, when `None`, the focused pane of the active tab.
    fn control_pane(&mut self, target: Option<u64>) -> Option<&mut crate::pane::Pane> {
        match target {
            Some(id) => self.tabs.iter_mut().find_map(|t| t.panes.get_mut(&id)),
            None => Some(self.tabs[self.active].focused()),
        }
    }

    /// Opens a new tab (optionally running `command`) and focuses it, for the remote
    /// `launch --type tab` / `new-tab` commands. Returns the new pane id on success.
    fn control_new_tab(&mut self, config: &Config, command: Vec<String>) -> crate::control::ControlResponse {
        use crate::control::ControlResponse;
        let area = self.active_area();
        let id = self.new_pane_id();
        let spawn = Spawn { command: (!command.is_empty()).then_some(command), cwd: None, ..Default::default() };
        let wake = wake_fn(self.proxy.clone());
        match Tab::new(area, self.renderer.cell_size(), config, id, &spawn, wake) {
            Ok(tab) => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
                self.reflow_all();
                self.window.request_redraw();
                ControlResponse::ok(serde_json::json!({ "tab": self.active, "pane": id }))
            }
            Err(e) => ControlResponse::error(format!("could not spawn: {e}")),
        }
    }
}

/// The user's editor, for the panel's "open this file" keys.
fn editor_cmd() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

/// Whether a foreground process name is just the pane's shell sitting at its
/// prompt. Login shells arrive as `-fish`, so the leading dash is stripped first.
fn is_shell(name: &str) -> bool {
    let name = name.trim_start_matches('-');
    matches!(
        name,
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "csh" | "tcsh" | "nu" | "elvish"
            | "xonsh" | "ash" | "busybox" | "pwsh" | "powershell" | "ion" | "osh"
    )
}

fn wheel_lines(delta: MouseScrollDelta, wheel_lines: f32, cell_h: f32) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y * wheel_lines,
        MouseScrollDelta::PixelDelta(p) => p.y as f32 / cell_h.max(1.0),
    }
}

/// Writes `data` to `path` with 0600 permissions, replacing an existing file we
/// own. The file is recreated, never opened in place: `mode(0o600)` only applies
/// at creation, so writing into a pre-existing file would keep its old mode and
/// owner — a stale loose file, or one another user planted in the shared /tmp
/// fallback, would hold the text world-readable. Unlinking our own stale file
/// always succeeds; a hostile one in a sticky temp dir cannot be unlinked, and
/// then `create_new` (O_EXCL, which also refuses a planted symlink) fails closed
/// instead of leaking through it.
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)
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

/// Wraps a string so a POSIX shell reads it back as exactly one word.
///
/// Single quotes, because inside them every byte is literal — spaces, `$`, `*`,
/// backslashes and even a newline. The one character that cannot appear is `'`
/// itself, which is closed, escaped and reopened (`it's` -> `'it'\''s'`). Dropped
/// paths go through this: a file called `my report (final).txt` must arrive as one
/// argument, and one called `; rm -rf ~` must arrive as a filename.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod shell_quote_tests {
    use super::shell_quote;

    #[test]
    fn a_plain_path_is_just_quoted() {
        assert_eq!(shell_quote("/home/pedro/notes.md"), "'/home/pedro/notes.md'");
    }

    #[test]
    fn spaces_and_globs_stay_one_word() {
        assert_eq!(shell_quote("/tmp/my report (final).txt"), "'/tmp/my report (final).txt'");
        assert_eq!(shell_quote("/tmp/*.rs"), "'/tmp/*.rs'");
        assert_eq!(shell_quote("/tmp/$HOME"), "'/tmp/$HOME'");
    }

    #[test]
    fn an_apostrophe_closes_escapes_and_reopens() {
        assert_eq!(shell_quote("/tmp/it's here"), r"'/tmp/it'\''s here'");
    }

    #[test]
    fn a_hostile_filename_stays_a_filename() {
        // The whole point: a file named like a command must not become one. The
        // quoting leaves nothing outside the quotes for the shell to act on.
        let q = shell_quote("/tmp/; rm -rf ~");
        assert_eq!(q, "'/tmp/; rm -rf ~'");
        assert!(!q[1..q.len() - 1].contains('\''), "no quote escapes the wrapper");
        // A newline in a name is legal on Unix; quoted, it is data, not a new line
        // of input to run.
        assert_eq!(shell_quote("/tmp/a\nb"), "'/tmp/a\nb'");
    }
}

#[cfg(test)]
mod write_private_tests {
    use super::write_private;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn recreates_with_0600_over_a_looser_preexisting_file() {
        let path = std::env::temp_dir()
            .join(format!("runnir-write-private-test-{}", std::process::id()));
        // A pre-existing loose file at the path (a stale capture, or a file
        // another user planted in a shared /tmp): the captured text must not
        // land in it with those permissions.
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();

        write_private(&path, b"secret").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the capture must be private, not the old file's mode");
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        let _ = std::fs::remove_file(&path);
    }
}
