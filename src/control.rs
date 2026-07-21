//! Remote-control API — script a running runnir from outside, like `kitty @`.
//!
//! A running terminal listens on a per-user Unix socket at
//! `$XDG_RUNTIME_DIR/runnir-<pid>.sock` (temp dir as a fallback) and exports that
//! path to child processes as `RUNNIR_LISTEN`, so a shell or tool inside a pane can
//! find its own terminal. The socket thread reads one JSON request line per
//! connection, hands it to the UI thread through an `EventLoopProxy`, and writes the
//! JSON response back.
//!
//! Trust boundary: the socket lives in a per-user 0700 runtime dir and is created
//! 0600 — the same model kitty uses. It is never bound to the network.
//!
//! This module owns three concerns kept deliberately separate so the pure parts are
//! testable without a live window: the wire types (`ControlRequest`,
//! `ControlResponse`) and their serde shape; `parse_client_args`, turning
//! `runnir @ <cmd> [flags]` into a request; and the socket plumbing (listener +
//! client), which is only I/O. The command *execution* against the live terminal
//! lives in `Gpu::handle_control` (app_input.rs), reached over the proxy bridge.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

/// Where a `launch` request puts the new process: a fresh tab or a split of the
/// focused pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchTarget {
    #[default]
    Tab,
    Split,
}

/// A command sent to a running terminal. The wire form is
/// `{"cmd":"<kebab>","args":{...}}` (serde adjacently-tagged); a unit variant like
/// `ls` is simply `{"cmd":"ls"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "kebab-case")]
pub enum ControlRequest {
    /// Run an optional command in a new tab or split.
    Launch {
        #[serde(default, rename = "type")]
        target: LaunchTarget,
        #[serde(default)]
        cmd: Option<String>,
    },
    /// Write text to a pane's PTY (default: the focused pane).
    SendText {
        text: String,
        #[serde(default)]
        target: Option<u64>,
    },
    /// Return a pane's scrollback + on-screen text (default: the focused pane).
    GetText {
        #[serde(default)]
        target: Option<u64>,
    },
    /// Make tab `index` (0-based) active.
    FocusTab { index: usize },
    /// List tabs and panes with ids, titles and cwds.
    Ls,
    /// Open a new tab, optionally running a command.
    NewTab {
        #[serde(default)]
        cmd: Option<String>,
    },
    /// Close a tab (default: the active one). Refuses to close the last tab.
    CloseTab {
        #[serde(default)]
        index: Option<usize>,
    },
    /// Press a key AT THE TERMINAL, not at the pane: the chord goes through the
    /// same path a real keypress does, so it drives overlays, the leader layer and
    /// the bound actions. `send-text` writes to the child; this drives runnir
    /// itself, which is the only way to script (or test) a panel from outside.
    Key { chord: String },
    /// Click at a cell of the window. Cells, not pixels, because everything runnir
    /// draws is laid out in them and a cell is what a caller can compute.
    Click {
        col: usize,
        row: usize,
        #[serde(default)]
        button: Option<String>,
    },
    /// Press at one cell, move to another, release — a drag, for the things that
    /// only exist under one (a pane divider, a git panel column separator).
    Drag {
        col: usize,
        row: usize,
        #[serde(rename = "to-col")]
        to_col: usize,
        #[serde(default, rename = "to-row")]
        to_row: Option<usize>,
    },
    /// Run an action by the id the config and the palette use (`git_panel`,
    /// `new_tab`, …), with no binding needed.
    Action { id: String },
    /// Apply colours/opacity live through the config apply path.
    SetColors {
        #[serde(default)]
        opacity: Option<f32>,
        #[serde(default)]
        foreground: Option<String>,
        #[serde(default)]
        background: Option<String>,
        #[serde(default)]
        accent: Option<String>,
        #[serde(default)]
        cursor: Option<String>,
    },
}

/// The reply to a `ControlRequest`. `data` carries command-specific JSON (e.g. the
/// tab list for `ls`, the text for `get-text`); it is omitted from the wire when
/// null so a bare acknowledgement is just `{"ok":true}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl ControlResponse {
    pub fn ok(data: Value) -> Self {
        Self { ok: true, error: None, data }
    }

    pub fn ok_empty() -> Self {
        Self { ok: true, error: None, data: Value::Null }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self { ok: false, error: Some(msg.into()), data: Value::Null }
    }
}

// ---------------------------------------------------------------------------
// Client-side argument parsing (pure, testable).
// ---------------------------------------------------------------------------

/// Turns `runnir @ <cmd> [flags]` into a `ControlRequest`.
///
/// Two input styles are accepted:
///   * flags — `--text "ls\n"`, `--type split`, `--cmd htop`, `--index 2`;
///   * a single raw-JSON args object — `send-text '{"text":"ls\n"}'`.
///
/// In the flag form the `text` value is unescaped (`\n`, `\t`, `\r`, `\e`, `\\`) so
/// `--text "ls\n"` sends a real newline, matching kitty's ergonomics.
pub fn parse_client_args(cmd: &str, flags: &[String]) -> Result<ControlRequest, String> {
    // Raw-JSON escape hatch: a single arg that looks like an object.
    if let [only] = flags
        && only.trim_start().starts_with('{')
    {
        let args: Value = serde_json::from_str(only).map_err(|e| format!("bad JSON args: {e}"))?;
        let wire = serde_json::json!({ "cmd": cmd, "args": args });
        return serde_json::from_value(wire).map_err(|e| format!("bad request: {e}"));
    }

    let m = parse_flags(flags)?;
    let req = match cmd {
        "ls" => ControlRequest::Ls,
        "get-text" => ControlRequest::GetText { target: opt_u64(&m, "target")? },
        "send-text" => ControlRequest::SendText {
            text: unescape(m.get("text").ok_or("send-text needs --text")?),
            target: opt_u64(&m, "target")?,
        },
        "launch" => ControlRequest::Launch {
            target: match m.get("type").map(String::as_str) {
                None | Some("tab") => LaunchTarget::Tab,
                Some("split") => LaunchTarget::Split,
                Some(other) => return Err(format!("unknown --type {other:?} (want tab|split)")),
            },
            cmd: m.get("cmd").cloned(),
        },
        "new-tab" => ControlRequest::NewTab { cmd: m.get("cmd").cloned() },
        "focus-tab" => ControlRequest::FocusTab {
            index: opt_usize(&m, "index")?.ok_or("focus-tab needs --index")?,
        },
        "close-tab" => ControlRequest::CloseTab { index: opt_usize(&m, "index")? },
        "key" => ControlRequest::Key {
            chord: m
                .get("chord")
                .or_else(|| m.get("key"))
                .ok_or("key needs --chord (e.g. --chord 'alt+shift+space')")?
                .clone(),
        },
        "click" => ControlRequest::Click {
            col: opt_usize(&m, "col")?.ok_or("click needs --col")?,
            row: opt_usize(&m, "row")?.ok_or("click needs --row")?,
            button: m.get("button").cloned(),
        },
        "drag" => ControlRequest::Drag {
            col: opt_usize(&m, "col")?.ok_or("drag needs --col")?,
            row: opt_usize(&m, "row")?.ok_or("drag needs --row")?,
            to_col: opt_usize(&m, "to-col")?.ok_or("drag needs --to-col")?,
            to_row: opt_usize(&m, "to-row")?,
        },
        "action" => ControlRequest::Action {
            id: m.get("id").ok_or("action needs --id (e.g. --id git_panel)")?.clone(),
        },
        "set-colors" => ControlRequest::SetColors {
            opacity: opt_f32(&m, "opacity")?,
            foreground: m.get("foreground").or_else(|| m.get("fg")).cloned(),
            background: m.get("background").or_else(|| m.get("bg")).cloned(),
            accent: m.get("accent").cloned(),
            cursor: m.get("cursor").cloned(),
        },
        other => return Err(format!("unknown command {other:?}")),
    };
    Ok(req)
}

/// Collects `--key value` / `--key=value` pairs into a map. Rejects a bare token
/// (no `--`) or a trailing flag with no value, so a typo fails loudly.
fn parse_flags(flags: &[String]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < flags.len() {
        let raw = &flags[i];
        let key = raw.strip_prefix("--").ok_or_else(|| format!("expected --flag, got {raw:?}"))?;
        if let Some((k, v)) = key.split_once('=') {
            map.insert(k.to_string(), v.to_string());
            i += 1;
        } else {
            let v = flags.get(i + 1).ok_or_else(|| format!("flag --{key} needs a value"))?;
            map.insert(key.to_string(), v.clone());
            i += 2;
        }
    }
    Ok(map)
}

fn opt_u64(m: &HashMap<String, String>, key: &str) -> Result<Option<u64>, String> {
    m.get(key)
        .map(|v| v.parse::<u64>().map_err(|_| format!("--{key} wants a number, got {v:?}")))
        .transpose()
}

fn opt_usize(m: &HashMap<String, String>, key: &str) -> Result<Option<usize>, String> {
    m.get(key)
        .map(|v| v.parse::<usize>().map_err(|_| format!("--{key} wants a number, got {v:?}")))
        .transpose()
}

fn opt_f32(m: &HashMap<String, String>, key: &str) -> Result<Option<f32>, String> {
    m.get(key)
        .map(|v| v.parse::<f32>().map_err(|_| format!("--{key} wants a number, got {v:?}")))
        .transpose()
}

/// Interprets the common backslash escapes so a shell-quoted `--text "ls\n"` sends a
/// real newline. Unknown escapes are left as-is (backslash kept).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('e') => out.push('\x1b'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Socket paths.
// ---------------------------------------------------------------------------

/// The per-user runtime directory: `$XDG_RUNTIME_DIR` (already 0700 per-user) or the
/// system temp dir as a fallback.
fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// This process's control-socket path, `runnir-<pid>.sock` in the runtime dir.
pub fn socket_path() -> PathBuf {
    runtime_dir().join(format!("runnir-{}.sock", std::process::id()))
}

/// Finds the newest live-looking runnir socket in the runtime dir, for a client that
/// has no `RUNNIR_LISTEN` to go on (e.g. run from a different terminal).
fn discover_socket() -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(runtime_dir()).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("runnir-") && name.ends_with(".sock")) {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else { continue };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

// ---------------------------------------------------------------------------
// Server side: listener started by the running terminal.
// ---------------------------------------------------------------------------

/// Binds the control socket and starts the accept thread. Also exports
/// `RUNNIR_LISTEN` in this process's environment so panes spawned afterwards inherit
/// it. Call this in `main` before the event loop spawns the first pane. Failure is
/// non-fatal: the terminal runs fine without a control socket.
pub fn start_listener(proxy: EventLoopProxy<UserEvent>) {
    let path = socket_path();
    // A socket file from a crashed prior run with our (recycled) pid would block the
    // bind; clear it first. Safe — a live server of ours would not share our pid.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("runnir: control socket unavailable ({e}); remote control disabled");
            return;
        }
    };
    // Owner-only, belt-and-braces on top of the 0700 runtime dir.
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    // SAFETY (Rust 2024): set at startup, before the event loop spawns any pane and
    // before other threads read the environment.
    unsafe {
        std::env::set_var("RUNNIR_LISTEN", &path);
    }

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                // One short-lived thread per connection so a slow UI reply can't wedge
                // the accept loop.
                Ok(stream) => {
                    let proxy = proxy.clone();
                    std::thread::spawn(move || handle_conn(stream, proxy));
                }
                Err(_) => break,
            }
        }
    });
}

/// The longest request line the server will buffer. A well-formed request is a
/// few hundred bytes; the cap keeps a client that streams data without a newline
/// from growing the terminal's memory without bound.
const MAX_REQUEST: u64 = 64 * 1024;

/// Reads one bounded request line off `reader` and parses it. Any failure —
/// oversized line, unreadable bytes, malformed JSON — yields the error response
/// to send back, so a misbehaving client always gets an answer instead of a
/// silent hangup.
fn read_request(reader: impl std::io::Read) -> Result<ControlRequest, ControlResponse> {
    let mut line = String::new();
    if let Err(e) = BufReader::new(reader.take(MAX_REQUEST)).read_line(&mut line) {
        return Err(ControlResponse::error(format!("bad request: {e}")));
    }
    if !line.ends_with('\n') && line.len() as u64 >= MAX_REQUEST {
        return Err(ControlResponse::error("request too large"));
    }
    serde_json::from_str(line.trim()).map_err(|e| ControlResponse::error(format!("bad request: {e}")))
}

/// Reads one JSON request line, bridges it to the UI thread, writes the JSON reply.
fn handle_conn(stream: UnixStream, proxy: EventLoopProxy<UserEvent>) {
    let Ok(read_half) = stream.try_clone() else { return };
    let resp = match read_request(read_half) {
        Ok(req) => bridge(req, &proxy),
        Err(resp) => resp,
    };
    let mut out = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    out.push(b'\n');
    let mut stream = stream;
    let _ = stream.write_all(&out);
    let _ = stream.flush();
}

/// Hands a request to the UI thread and waits for its response. Bounded so a hung or
/// gone UI can never block the socket thread forever.
fn bridge(req: ControlRequest, proxy: &EventLoopProxy<UserEvent>) -> ControlResponse {
    let (tx, rx) = mpsc::channel();
    if proxy.send_event(UserEvent::Control(req, tx)).is_err() {
        return ControlResponse::error("terminal is shutting down");
    }
    rx.recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|_| ControlResponse::error("timed out waiting for the UI thread"))
}

// ---------------------------------------------------------------------------
// Client side: `runnir @ <cmd> [flags]`.
// ---------------------------------------------------------------------------

/// Entry point for the `@` subcommand. Parses the request, finds the socket, sends
/// it, prints the pretty-printed JSON response. Exits non-zero on any failure.
pub fn client_main(args: &[String]) {
    let Some(cmd) = args.first() else {
        eprintln!("usage: runnir @ <command> [--flag value ...]");
        eprintln!("commands: launch, send-text, get-text, focus-tab, ls, new-tab, close-tab, set-colors");
        std::process::exit(2);
    };
    let req = match parse_client_args(cmd, &args[1..]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runnir @: {e}");
            std::process::exit(2);
        }
    };
    let path = std::env::var_os("RUNNIR_LISTEN")
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(discover_socket);
    let Some(path) = path else {
        eprintln!("runnir @: no running terminal found (open runnir, or set RUNNIR_LISTEN)");
        std::process::exit(1);
    };
    match send_request(&path, &req) {
        Ok(resp) => {
            println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
            if !resp.ok {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("runnir @: could not reach {} ({e})", path.display());
            std::process::exit(1);
        }
    }
}

/// Connects, writes one request line, reads one response line.
fn send_request(path: &std::path::Path, req: &ControlRequest) -> std::io::Result<ControlResponse> {
    let mut stream = UnixStream::connect(path)?;
    let mut body = serde_json::to_vec(req).map_err(std::io::Error::other)?;
    body.push(b'\n');
    stream.write_all(&body)?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(line.trim()).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_json() {
        let cases = [
            ControlRequest::Ls,
            ControlRequest::GetText { target: Some(7) },
            ControlRequest::SendText { text: "ls\n".into(), target: None },
            ControlRequest::Launch { target: LaunchTarget::Split, cmd: Some("htop".into()) },
            ControlRequest::FocusTab { index: 2 },
            ControlRequest::NewTab { cmd: None },
            ControlRequest::CloseTab { index: Some(1) },
            ControlRequest::SetColors {
                opacity: Some(0.9),
                foreground: Some("#ffffff".into()),
                background: None,
                accent: None,
                cursor: None,
            },
        ];
        for req in cases {
            let json = serde_json::to_string(&req).unwrap();
            let back: ControlRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(req, back, "round trip failed for {json}");
        }
    }

    #[test]
    fn ls_is_tagged_without_an_args_object() {
        assert_eq!(serde_json::to_string(&ControlRequest::Ls).unwrap(), r#"{"cmd":"ls"}"#);
    }

    #[test]
    fn send_text_has_the_documented_wire_shape() {
        let req = ControlRequest::SendText { text: "hi".into(), target: Some(3) };
        let v: Value = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(v["cmd"], "send-text");
        assert_eq!(v["args"]["text"], "hi");
        assert_eq!(v["args"]["target"], 3);
    }

    #[test]
    fn response_omits_null_fields() {
        assert_eq!(serde_json::to_string(&ControlResponse::ok_empty()).unwrap(), r#"{"ok":true}"#);
        let err = ControlResponse::error("boom");
        assert_eq!(serde_json::to_string(&err).unwrap(), r#"{"ok":false,"error":"boom"}"#);
    }

    #[test]
    fn response_round_trips_with_data() {
        let resp = ControlResponse::ok(serde_json::json!({"text": "hello"}));
        let back: ControlResponse = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(resp, back);
    }

    fn flags(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_send_text_with_escape_expansion() {
        let req = parse_client_args("send-text", &flags(&["--text", "ls\\n"])).unwrap();
        assert_eq!(req, ControlRequest::SendText { text: "ls\n".into(), target: None });
    }

    #[test]
    fn parses_send_text_target() {
        let req = parse_client_args("send-text", &flags(&["--text", "x", "--target", "42"])).unwrap();
        assert_eq!(req, ControlRequest::SendText { text: "x".into(), target: Some(42) });
    }

    #[test]
    fn parses_launch_split() {
        let req = parse_client_args("launch", &flags(&["--type", "split", "--cmd", "htop"])).unwrap();
        assert_eq!(req, ControlRequest::Launch { target: LaunchTarget::Split, cmd: Some("htop".into()) });
    }

    #[test]
    fn launch_defaults_to_a_tab() {
        let req = parse_client_args("launch", &[]).unwrap();
        assert_eq!(req, ControlRequest::Launch { target: LaunchTarget::Tab, cmd: None });
    }

    #[test]
    fn parses_ls_and_get_text() {
        assert_eq!(parse_client_args("ls", &[]).unwrap(), ControlRequest::Ls);
        assert_eq!(
            parse_client_args("get-text", &flags(&["--target", "5"])).unwrap(),
            ControlRequest::GetText { target: Some(5) }
        );
    }

    #[test]
    fn parses_focus_tab_and_close_tab() {
        assert_eq!(
            parse_client_args("focus-tab", &flags(&["--index", "3"])).unwrap(),
            ControlRequest::FocusTab { index: 3 }
        );
        assert_eq!(
            parse_client_args("close-tab", &[]).unwrap(),
            ControlRequest::CloseTab { index: None }
        );
    }

    #[test]
    fn parses_the_input_commands() {
        assert_eq!(
            parse_client_args("key", &flags(&["--chord", "alt+shift+space"])).unwrap(),
            ControlRequest::Key { chord: "alt+shift+space".into() }
        );
        assert_eq!(
            parse_client_args("click", &flags(&["--col", "4", "--row", "9"])).unwrap(),
            ControlRequest::Click { col: 4, row: 9, button: None }
        );
        assert_eq!(
            parse_client_args("drag", &flags(&["--col", "40", "--row", "5", "--to-col", "60"]))
                .unwrap(),
            ControlRequest::Drag { col: 40, row: 5, to_col: 60, to_row: None }
        );
        assert_eq!(
            parse_client_args("action", &flags(&["--id", "git_panel"])).unwrap(),
            ControlRequest::Action { id: "git_panel".into() }
        );
        // A missing coordinate has to fail loudly: a click at a defaulted 0,0 would
        // land on the tab bar and look like the command did something else.
        assert!(parse_client_args("click", &flags(&["--col", "4"])).is_err());
        assert!(parse_client_args("key", &[]).is_err());
    }

    #[test]
    fn parses_set_colors_with_aliases() {
        let req = parse_client_args("set-colors", &flags(&["--opacity", "0.8", "--fg", "#112233"])).unwrap();
        assert_eq!(
            req,
            ControlRequest::SetColors {
                opacity: Some(0.8),
                foreground: Some("#112233".into()),
                background: None,
                accent: None,
                cursor: None,
            }
        );
    }

    #[test]
    fn key_equals_value_form_works() {
        let req = parse_client_args("focus-tab", &flags(&["--index=4"])).unwrap();
        assert_eq!(req, ControlRequest::FocusTab { index: 4 });
    }

    #[test]
    fn raw_json_args_are_accepted() {
        let req = parse_client_args("send-text", &flags(&[r#"{"text":"hi","target":9}"#])).unwrap();
        assert_eq!(req, ControlRequest::SendText { text: "hi".into(), target: Some(9) });
    }

    #[test]
    fn unknown_command_is_an_error() {
        assert!(parse_client_args("teleport", &[]).is_err());
    }

    #[test]
    fn missing_required_flag_is_an_error() {
        assert!(parse_client_args("send-text", &[]).is_err());
        assert!(parse_client_args("focus-tab", &[]).is_err());
    }

    #[test]
    fn a_bare_token_is_rejected() {
        assert!(parse_client_args("send-text", &flags(&["oops"])).is_err());
    }

    #[test]
    fn read_request_parses_a_valid_line() {
        let req = read_request(&b"{\"cmd\":\"ls\"}\n"[..]).unwrap();
        assert_eq!(req, ControlRequest::Ls);
    }

    #[test]
    fn read_request_rejects_an_oversized_line_instead_of_buffering_it() {
        // A newline-free stream longer than the cap must come back as an error
        // response (bounded read), not be buffered until the client stops.
        let big = vec![b'a'; (MAX_REQUEST as usize) + 512];
        let resp = read_request(&big[..]).unwrap_err();
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("request too large"));
    }

    #[test]
    fn read_request_answers_unreadable_bytes_with_an_error_response() {
        // Invalid UTF-8 used to drop the connection with no reply; the client
        // deserves a response it can print.
        let resp = read_request(&[0xff, 0xfe, 0xfd][..]).unwrap_err();
        assert!(!resp.ok);
        assert!(resp.error.unwrap().starts_with("bad request"));
    }

    #[test]
    fn read_request_answers_malformed_json_with_an_error_response() {
        let resp = read_request(&b"not json\n"[..]).unwrap_err();
        assert!(!resp.ok);
        assert!(resp.error.unwrap().starts_with("bad request"));
    }

    #[test]
    fn unescape_covers_the_common_escapes() {
        assert_eq!(unescape("a\\nb\\tc\\\\d\\e"), "a\nb\tc\\d\x1b");
        // An unknown escape keeps its backslash.
        assert_eq!(unescape("\\q"), "\\q");
    }
}
