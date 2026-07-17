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

/// Per-window AI state. The transcript lives in the overlay panel; this only
/// tracks which request is current.
pub struct Session {
    next_id: AtomicU64,
    /// Id of the request whose answer we are still waiting for. A reply with a
    /// different id is stale (the user asked again) and is ignored.
    pending: Option<u64>,
    pub provider: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1), pending: None, provider: None }
    }

    fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Delivers a reply to the panel, unless it is stale.
    pub fn receive(&mut self, reply: Reply, overlay: Option<&mut crate::overlay::Overlay>) {
        if self.pending != Some(reply.id) {
            return; // Superseded by a newer question.
        }
        self.pending = None;
        if let Some(crate::overlay::Overlay::Ai(panel)) = overlay {
            panel.busy = false;
            match reply.result {
                Ok(text) => panel.push(crate::overlay::Who::Assistant, text),
                Err(err) => panel.push(crate::overlay::Who::System, format!("error: {err}")),
            }
        }
    }
}

/// Fires a question at `provider` on a worker thread. The answer arrives later as
/// a `UserEvent::Ai` through `proxy`.
pub fn ask(
    session: &mut Session,
    config: &Config,
    provider_name: &str,
    prompt: String,
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

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }

    // A crude timeout: wait in a thread, kill if it overruns. Good enough for a
    // helper that should answer in seconds.
    let out = wait_with_timeout(child, timeout)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("claude exited with {}: {}", out.status, err.trim()))
    }
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: u64,
) -> Result<std::process::Output, String> {
    use std::time::{Duration, Instant};
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout) {
                    let _ = child.kill();
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
