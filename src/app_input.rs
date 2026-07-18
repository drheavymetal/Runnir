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
            self.overlay_key(&event, mods, config);
            return;
        }

        // Copy-mode owns the keyboard: vim motions drive a virtual cursor/selection.
        if self.copy_mode.is_some() {
            self.copy_mode_key(&event, mods);
            return;
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
        let strip_w = 46.0;
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
        // Bump the pane-id seed above every restored id so a later split in a restored
        // tab never reuses one of its own pane ids.
        let max_id = entry.tabs.iter().flat_map(|t| t.tree.panes()).max().unwrap_or(0);
        self.next_pane_seed = self.next_pane_seed.max(max_id);
        for layout in &entry.tabs {
            let state = layout.to_tab_state();
            let proxy = self.proxy.clone();
            let wake = move |_id| -> Box<dyn Fn() + Send + 'static> {
                let p = proxy.clone();
                Box::new(move || {
                    let _ = p.send_event(UserEvent::Redraw);
                })
            };
            match Tab::from_session(&state, area, cell, config, wake) {
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
        if let Some(text) = crate::hints::act(&h.text, h.kind) {
            self.set_clipboard(text);
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

    /// Applies a freshly-loaded config live (hot-reload): theme, opacity and font.
    /// Key bindings are rebuilt by the caller (they live on `App`, not `Gpu`).
    fn apply_config(&mut self, config: &Config) {
        // A status-bar toggle changes the content height, so reflow after.
        if self.status_bar != config.window.status_bar {
            self.status_bar = config.window.status_bar;
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
    ) -> crate::control::ControlResponse {
        use crate::control::{ControlRequest, ControlResponse, LaunchTarget};
        use serde_json::json;

        match req {
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
        // libc's constant, so the flag is correct on both Linux and macOS (the raw
        // value differs between them).
        .custom_flags(libc::O_NOFOLLOW)
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
