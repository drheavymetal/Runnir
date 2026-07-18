//! The AI assistant.
//!
//! Two transports, kept distinct because they really are different:
//!
//! - **Claude Code** runs the `claude` CLI as a subprocess, billing against the
//!   user's subscription. No API key ever touches this path.
//! - **API** is any OpenAI-compatible chat endpoint (OpenAI, Gemini's compat
//!   layer, DeepSeek, Z.ai), paid per token, keyed by an environment variable.
//!
//! Requests run on a worker thread and their answers come back through the winit
//! event loop, so the UI never blocks on the model.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{Config, Provider};

/// A model's answer, delivered to the UI thread.
pub struct Reply {
    /// Monotonic id of the request this answers, so a stale reply can be dropped.
    pub id: u64,
    pub result: Result<String, String>,
}

/// What to do with a completed answer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Show it in the assistant panel.
    Panel,
    /// Type it at the shell prompt (a natural-language → command translation),
    /// without pressing Enter — the user reviews then runs it.
    InsertCommand,
    /// A "whisper": the answer is a JSON plan of terminal actions to execute.
    Whisper,
}

/// What the UI thread should do with a delivered reply.
pub enum Delivery {
    Nothing,
    ToPanel,
    Insert(String),
    /// A JSON action plan from a whisper, for the app to parse and run.
    Whisper(String),
}

/// Per-window AI state. The transcript lives in the overlay panel; this only
/// tracks which request is current.
pub struct Session {
    next_id: AtomicU64,
    /// Id of the request whose answer we are still waiting for. A reply with a
    /// different id is stale (the user asked again) and is ignored.
    pending: Option<u64>,
    pending_purpose: Purpose,
    pub provider: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1), pending: None, pending_purpose: Purpose::Panel, provider: None }
    }

    fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Handles a reply: routes a panel answer into the panel here, and returns what
    /// the caller must still do (insert a command at the prompt). Stale replies are
    /// dropped.
    pub fn receive(&mut self, reply: Reply, overlay: Option<&mut crate::overlay::Overlay>) -> Delivery {
        if self.pending != Some(reply.id) {
            return Delivery::Nothing; // Superseded by a newer question.
        }
        self.pending = None;
        match self.pending_purpose {
            Purpose::InsertCommand => match reply.result {
                Ok(text) => Delivery::Insert(clean_command(&text)),
                Err(_) => Delivery::Nothing,
            },
            Purpose::Whisper => match reply.result {
                Ok(text) => Delivery::Whisper(text),
                Err(_) => Delivery::Nothing,
            },
            Purpose::Panel => {
                if let Some(crate::overlay::Overlay::Ai(panel)) = overlay {
                    panel.busy = false;
                    match reply.result {
                        Ok(text) => panel.push(crate::overlay::Who::Assistant, text),
                        Err(err) => panel.push(crate::overlay::Who::System, format!("error: {err}")),
                    }
                }
                Delivery::ToPanel
            }
        }
    }
}

/// Strips markdown fences and surrounding whitespace from a model's command reply,
/// leaving a single line ready to type at the prompt. Models wrap commands in
/// ```` ```sh ```` blocks even when told not to.
fn clean_command(text: &str) -> String {
    let mut s = text.trim();
    // The fence may sit after a prose preamble ("Here is the fix:\n```sh\n…"),
    // so find it anywhere, not just at the start — the fenced content is the
    // command; the surrounding prose never is.
    if let Some(idx) = s.find("```") {
        let rest = &s[idx + 3..];
        // Drop the opening fence's language-tag line; a one-line fence
        // (```ls -la```) has no newline, so its content is used whole.
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        s = rest.trim_end_matches("```").trim();
    }
    s = s.trim_start_matches('`').trim_end_matches('`');
    // Keep only the first line: a command, not a paragraph.
    let mut line = s.lines().next().unwrap_or("").trim();
    // Models like to echo a shell prompt marker in front of the command. Strip a
    // leading `$` only when followed by whitespace ("$ cmd"): a `$` glued to a
    // word is a variable — `$EDITOR notes.txt` is a real command, not an echo.
    if let Some(rest) = line.strip_prefix('$') {
        if rest.starts_with(char::is_whitespace) {
            line = rest.trim_start();
        }
    }
    line.to_string()
}

/// Fires a question at `provider` on a worker thread. The answer arrives later as
/// a `UserEvent::Ai` through `proxy`.
pub fn ask(
    session: &mut Session,
    config: &Config,
    provider_name: &str,
    prompt: String,
    purpose: Purpose,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
) -> Result<(), String> {
    let provider = config
        .ai
        .providers
        .get(provider_name)
        .cloned()
        .ok_or_else(|| format!("no such provider: {provider_name}"))?;
    let timeout = config.ai.timeout_secs;

    let id = session.next();
    session.pending = Some(id);
    session.pending_purpose = purpose;

    std::thread::spawn(move || {
        let result = match provider {
            Provider::ClaudeCode { command, args, dangerously_skip_permissions } => {
                run_claude_code(&command, &args, dangerously_skip_permissions, &prompt, timeout)
            }
            Provider::Api { base_url, model, api_key_env } => {
                run_api(&base_url, &model, &api_key_env, &prompt, timeout)
            }
        };
        let _ = proxy.send_event(crate::UserEvent::Ai(Reply { id, result }));
    });
    Ok(())
}

/// The argv to launch Claude Code interactively in a pane (not `-p`). Kept here so
/// the same config drives both a one-shot question and a launched session.
pub fn claude_launch_command(config: &Config) -> Vec<String> {
    match config.ai.providers.get(&config.ai.default) {
        Some(Provider::ClaudeCode { command, args, dangerously_skip_permissions }) => {
            let mut cmd = vec![command.clone()];
            cmd.extend(args.clone());
            if *dangerously_skip_permissions {
                cmd.push("--dangerously-skip-permissions".into());
            }
            cmd
        }
        // Fall back to the plain binary even when the default is an API provider:
        // "launch Claude Code" always means the CLI.
        _ => vec!["claude".into()],
    }
}

/// Runs the Claude Code CLI once in headless mode (`-p`) and returns its output.
///
/// This bills against the user's Claude subscription; there is no API key. The
/// `--dangerously-skip-permissions` flag is passed only when configured, and only
/// matters for agentic actions.
fn run_claude_code(
    command: &str,
    args: &[String],
    skip_permissions: bool,
    prompt: &str,
    timeout: u64,
) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(command);
    cmd.args(args);
    if skip_permissions {
        cmd.arg("--dangerously-skip-permissions");
    }
    // `-p` (print) runs one prompt and exits; the prompt goes on stdin so a long
    // question is never truncated by an argv limit.
    cmd.arg("-p");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run '{command}': {e} (is Claude Code installed?)"))?;

    // Drain stdout and stderr on their own threads *while* the child runs. If we
    // instead waited to read until after exit, a child that writes more than the
    // OS pipe buffer (~64 KB) would block on write and never exit — so every large
    // answer would look like a timeout. The prompt is written on a thread too, so
    // a prompt larger than the pipe buffer cannot deadlock against unread output.
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let prompt_owned = prompt.to_string();
    let writer = std::thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(prompt_owned.as_bytes());
        }
        // Dropping stdin closes it, signalling EOF so claude stops reading.
        drop(stdin);
    });
    let out_reader = drain(stdout);
    let err_reader = drain(stderr);

    let status = wait_with_timeout(&mut child, timeout);
    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

    match status {
        Ok(status) if status.success() => Ok(String::from_utf8_lossy(&stdout).trim().to_string()),
        Ok(status) => Err(format!(
            "claude exited with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        )),
        Err(e) => Err(e),
    }
}

/// Reads a child pipe to the end on its own thread.
fn drain<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

/// Waits for the child, killing and reaping it if it overruns `timeout`. Reaping
/// after kill matters: `Child::kill` sends the signal but does not wait, so
/// skipping the `wait` would leave a zombie for every timed-out request.
fn wait_with_timeout(child: &mut std::process::Child, timeout: u64) -> Result<std::process::ExitStatus, String> {
    use std::time::{Duration, Instant};
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout) {
                    let _ = child.kill();
                    let _ = child.wait(); // reap, or it becomes a zombie
                    return Err(format!("timed out after {timeout}s"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Posts to an OpenAI-compatible chat endpoint and returns the assistant text.
fn run_api(
    base_url: &str,
    model: &str,
    api_key_env: &str,
    prompt: &str,
    timeout: u64,
) -> Result<String, String> {
    let key = std::env::var(api_key_env)
        .map_err(|_| format!("${api_key_env} is not set (put your key there)"))?;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": false,
    });

    let response = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(timeout)))
        .build()
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .send_json(&body);

    let mut response = match response {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!("{model}: HTTP {code}"));
        }
        Err(e) => return Err(format!("{model}: {e}")),
    };

    let json: serde_json::Value =
        response.body_mut().read_json().map_err(|e| format!("bad response: {e}"))?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("no content in response: {json}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_appends_skip_flag_only_when_set() {
        let mut cfg = Config::default();
        // Default 'claude' asks permission.
        let cmd = claude_launch_command(&cfg);
        assert_eq!(cmd, vec!["claude".to_string()]);

        cfg.ai.default = "claude-yolo".into();
        let cmd = claude_launch_command(&cfg);
        assert!(cmd.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn launch_command_falls_back_to_cli_for_api_default() {
        let mut cfg = Config::default();
        cfg.ai.default = "openai".into();
        // "Launch Claude Code" must still be the CLI, never an API call.
        assert_eq!(claude_launch_command(&cfg), vec!["claude".to_string()]);
    }

    #[test]
    fn clean_command_strips_markdown_and_keeps_one_line() {
        assert_eq!(clean_command("ls -la"), "ls -la");
        assert_eq!(clean_command("`ls -la`"), "ls -la");
        assert_eq!(clean_command("```sh\nfind . -name '*.rs'\n```"), "find . -name '*.rs'");
        assert_eq!(clean_command("```\ngit status\n```"), "git status");
        // A model that adds a paragraph: keep only the command line.
        assert_eq!(clean_command("rm -rf build\nThis deletes the build dir."), "rm -rf build");
        // A leading shell-prompt marker the model echoed is stripped.
        assert_eq!(clean_command("$ cargo build"), "cargo build");
        // All three wrappers at once: fence + backtick residue + prompt marker.
        assert_eq!(clean_command("```sh\n$ ls -la\n```"), "ls -la");
    }

    #[test]
    fn clean_command_survives_prose_around_the_fence() {
        // A model that disobeys "no prose" and prefixes the fence: the fenced
        // command must win, or English prose gets typed at the shell prompt.
        assert_eq!(
            clean_command("Here is the corrected command:\n\n```sh\nmkdir foo\n```"),
            "mkdir foo"
        );
        // Trailing prose after the closing fence is ignored too.
        assert_eq!(clean_command("```\nls -la\n```\nThis lists the files."), "ls -la");
        // An unterminated fence (reply cut off) still yields the command.
        assert_eq!(clean_command("```sh\ncargo build"), "cargo build");
        // An empty fence collapses to nothing (the app then types nothing).
        assert_eq!(clean_command("```sh\n```"), "");
    }

    #[test]
    fn clean_command_keeps_a_leading_variable_reference() {
        // `$` glued to a word is a variable expansion, not a prompt echo: the
        // command must arrive intact, not beheaded into "EDITOR notes.txt".
        assert_eq!(clean_command("$EDITOR notes.txt"), "$EDITOR notes.txt");
        assert_eq!(clean_command("`$BROWSER https://crates.io`"), "$BROWSER https://crates.io");
        // With whitespace after it, it is the echoed prompt marker: strip it.
        assert_eq!(clean_command("$ $EDITOR notes.txt"), "$EDITOR notes.txt");
    }

    #[test]
    fn stale_replies_are_ignored() {
        let mut s = Session::new();
        s.pending = Some(5);
        // A reply for a superseded request must not touch the panel.
        s.receive(Reply { id: 4, result: Ok("stale".into()) }, None);
        assert_eq!(s.pending, Some(5), "pending must be untouched by a stale reply");
        s.receive(Reply { id: 5, result: Ok("fresh".into()) }, None);
        assert_eq!(s.pending, None, "the matching reply clears pending");
    }
}
