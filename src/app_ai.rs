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
        if let Err(e) =
            ai::ask(&mut self.ai, config, &provider, prompt, ai::Purpose::InsertCommand, self.proxy.clone())
        {
            eprintln!("runnir: ai command failed: {e}");
        }
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

    /// Types an AI-produced command at the focused shell prompt without running it.
    fn insert_command(&mut self, cmd: String) {
        if !cmd.is_empty() {
            self.tab().focused().snap_to_bottom();
            self.tab().focused().write(cmd.as_bytes());
            self.window.request_redraw();
        }
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
