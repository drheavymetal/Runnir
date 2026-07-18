// AI and hint-mode methods for `Gpu`. Included into main.rs.

impl Gpu {
    fn toggle_ai(&mut self, config: &Config) {
        match &self.overlay {
            Some(Overlay::Ai(_)) => self.overlay = None,
            _ => {
                let provider = config.ai.default.clone();
                self.ai.provider = Some(provider.clone());
                let mut panel = overlay::AiPanel::new(provider);
                if config.ai.providers.is_empty() {
                    panel.push(overlay::Who::System, "No AI providers configured.".into());
                }
                self.overlay = Some(Overlay::Ai(panel));
            }
        }
    }

    /// Sends the last command, its output and (best-effort) its exit status to the
    /// model. This is the integration that earns its keep — "why did this break?"
    /// answered in place.
    fn ask_ai_about_error(&mut self, config: &Config) {
        let output = self.tab().focused().last_command_output();
        let provider = config.ai.default.clone();

        // Make sure the panel is open to show the exchange.
        if !matches!(self.overlay, Some(Overlay::Ai(_))) {
            self.ai.provider = Some(provider.clone());
            self.overlay = Some(Overlay::Ai(overlay::AiPanel::new(provider)));
        }

        let question = match output {
            Some(out) if !out.trim().is_empty() => format!(
                "This command failed in my terminal. Explain why in two or three \
                 sentences and give the exact fix. Output:\n\n{}",
                truncate(&out, 4000)
            ),
            _ => "The last command may have failed but I have no captured output \
                  (shell integration is not enabled). Explain how to enable OSC 133 \
                  marks so I can capture command output."
                .to_string(),
        };
        self.send_ai(question, config);
    }

    fn send_ai(&mut self, question: String, config: &Config) {
        let provider = self.ai.provider.clone().unwrap_or_else(|| config.ai.default.clone());
        if let Some(Overlay::Ai(panel)) = self.overlay.as_mut() {
            panel.push(overlay::Who::You, question.clone());
            panel.busy = true;
        }
        if let Err(e) =
            ai::ask(&mut self.ai, config, &provider, question, ai::Purpose::Panel, self.proxy.clone())
        {
            if let Some(Overlay::Ai(panel)) = self.overlay.as_mut() {
                panel.busy = false;
                panel.push(overlay::Who::System, format!("error: {e}"));
            }
        }
        self.window.request_redraw();
    }

    /// Opens a prompt that turns a natural-language description into a shell
    /// command and types it at the prompt (not run — you review, then press Enter).
    fn ai_command(&mut self) {
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::AiCommand,
            "Describe the command (natural language)",
            Vec::new(),
        )));
    }

    fn send_ai_command(&mut self, description: String, config: &Config) {
        let provider = config.ai.default.clone();
        let prompt = format!(
            "Translate this request into a single shell command for a Linux system. \
             Output ONLY the command, no explanation, no markdown, no backticks.\n\n{description}"
        );
        self.status = Some(format!("asking {provider} for a command…"));
        self.status_expiry = None; // In-flight: the reply clears the toast.
        if let Err(e) =
            ai::ask(&mut self.ai, config, &provider, prompt, ai::Purpose::InsertCommand, self.proxy.clone())
        {
            // No worker spawned, so no reply will ever clear this. Expire it.
            self.status = Some(format!("ai command failed: {e}"));
            self.status_expiry = Some(Instant::now() + Duration::from_secs(5));
        }
        self.window.request_redraw();
    }

    /// Sends the current selection to the assistant to be explained.
    fn ai_explain_selection(&mut self, config: &Config) {
        let Some(text) = self.tab().focused().selection_text() else { return };
        let provider = config.ai.default.clone();
        if !matches!(self.overlay, Some(Overlay::Ai(_))) {
            self.ai.provider = Some(provider.clone());
            self.overlay = Some(Overlay::Ai(overlay::AiPanel::new(provider)));
        }
        self.send_ai(format!("Explain this concisely:\n\n{}", truncate(&text, 4000)), config);
    }

    /// Asks the assistant to summarise this pane's session: what was done, what
    /// broke, how it was fixed. Reads the scrollback, so it works retroactively on a
    /// session you have been in for a while.
    fn summarize_session(&mut self, config: &Config) {
        let text = self.tab().focused().scrollback_text().join("\n");
        if text.trim().is_empty() {
            self.status = Some("nothing in the scrollback to summarize yet".into());
            self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
            self.window.request_redraw();
            return;
        }
        let provider = config.ai.default.clone();
        if !matches!(self.overlay, Some(Overlay::Ai(_))) {
            self.ai.provider = Some(provider.clone());
            self.overlay = Some(Overlay::Ai(overlay::AiPanel::new(provider)));
        }
        let question = format!(
            "This is a terminal session transcript. Summarise what was done: the key \
             commands, what they accomplished, any errors and how they were resolved. \
             Be concise — a short bulleted list, no preamble.\n\n{}",
            truncate(&text, 8000)
        );
        self.send_ai(question, config);
    }

    /// Types an AI-produced command at the focused shell prompt without running it.
    fn insert_command(&mut self, cmd: String) {
        if !cmd.is_empty() {
            self.tab().focused().snap_to_bottom();
            self.tab().focused().write(cmd.as_bytes());
            self.window.request_redraw();
        }
    }

    /// Opens the whisper bar: tell the terminal what to do in plain language.
    fn whisper(&mut self) {
        self.overlay = Some(Overlay::Prompt(Prompt::new(
            PromptKind::Whisper,
            "Whisper — what should runnir do?",
            Vec::new(),
        )));
    }

    fn send_whisper(&mut self, request: String, config: &Config) {
        let provider = config.ai.default.clone();
        let prompt = crate::whisper::prompt(&request);
        self.status = Some(format!("whispering to {provider}…"));
        self.status_expiry = None; // In-flight: the reply clears the toast.
        if let Err(e) =
            ai::ask(&mut self.ai, config, &provider, prompt, ai::Purpose::Whisper, self.proxy.clone())
        {
            // No worker spawned, so no reply will ever clear this. Expire it.
            self.status = Some(format!("whisper failed: {e}"));
            self.status_expiry = Some(Instant::now() + Duration::from_secs(5));
        }
        self.window.request_redraw();
    }

    /// Executes a whisper plan: each step is a runnir action or a shell command.
    /// Actions run immediately; a `run` step is typed at the prompt (not executed)
    /// so a model-written command is always reviewed before it runs.
    fn execute_whisper(&mut self, plan_json: String, config: &Config) {
        let plan = crate::whisper::parse(&plan_json);
        let area = self.active_area();
        for step in plan {
            let wake = wake_fn(self.proxy.clone());
            match step.action.as_str() {
                "new_tab" => {
                    let id = self.new_pane_id();
                    if let Ok(tab) =
                        Tab::new(area, self.renderer.cell_size(), config, id, &Spawn::default(), wake)
                    {
                        self.tabs.push(tab);
                        self.active = self.tabs.len() - 1;
                        self.reflow_all();
                    }
                }
                "split_h" => {
                    let id = self.new_pane_id();
                    let _ = self.tab().split_with_id(area, Axis::Horizontal, config, id, wake);
                }
                "split_v" => {
                    let id = self.new_pane_id();
                    let _ = self.tab().split_with_id(area, Axis::Vertical, config, id, wake);
                }
                "close_pane" => {
                    self.tab().close_focused(area);
                }
                "focus_left" => {
                    self.tab().focus_dir(area, crate::layout::Direction::Left);
                }
                "focus_right" => {
                    self.tab().focus_dir(area, crate::layout::Direction::Right);
                }
                "focus_up" => {
                    self.tab().focus_dir(area, crate::layout::Direction::Up);
                }
                "focus_down" => {
                    self.tab().focus_dir(area, crate::layout::Direction::Down);
                }
                "ssh" if !step.arg.is_empty() => {
                    self.split_running(config, vec!["ssh".into(), step.arg]);
                }
                "run" if !step.arg.is_empty() => {
                    // Typed, not executed: the user reviews then presses Enter.
                    // Keep only the first line so a model-emitted newline (trailing
                    // or embedded) can never auto-run the command — that would break
                    // the review-before-run contract this action rests on.
                    let typed = step.arg.split(['\n', '\r']).next().unwrap_or("");
                    if !typed.is_empty() {
                        self.tab().focused().write(typed.as_bytes());
                    }
                }
                "search" if !step.arg.is_empty() => {
                    let mut search = overlay::Search::new();
                    for c in step.arg.chars() {
                        search.input(c);
                    }
                    self.overlay = Some(Overlay::Search(search));
                    self.recompute_search();
                    self.scroll_to_current_match();
                }
                "font_bigger" => self.set_font_px(self.font_px + 1.0, config),
                "font_smaller" => self.set_font_px(self.font_px - 1.0, config),
                "broadcast" => self.broadcast = !self.broadcast,
                "launch_claude" => self.launch_claude(config),
                "docs" => self.overlay = Some(Overlay::Docs(overlay::Docs::new(docs::HELP))),
                other => eprintln!("runnir: whisper: unknown action {other:?}"),
            }
        }
        self.window.request_redraw();
    }

    fn open_hints(&mut self) {
        let hints = {
            let pane = self.tab().focused();
            let grid = pane.grid.lock().unwrap();
            hints::find(&grid)
        };
        if hints.is_empty() {
            return; // Nothing to point at; do not enter a mode with no targets.
        }
        self.overlay = Some(Overlay::Hints(overlay::Hints::new(hints)));
    }

    fn act_on_hint(&mut self, text: String, kind: overlay::HintKind) {
        hints::act(&text, kind, &mut self.clipboard);
    }
}

/// Clips long output so an AI prompt stays a reasonable size, keeping the tail —
/// the error is almost always at the end. Snaps to a char boundary so non-ASCII
/// output (common) cannot panic on a mid-codepoint slice.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let cut = s.len() - max;
    let start = (cut..=s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(s.len());
    format!("…(truncated)…\n{}", &s[start..])
}
