//! The server half of `pocket`: a WebSocket per viewer, fed snapshots from the frame.
//!
//! Phase 1 binds **loopback only**. The tunnel arrives with the PIN in phase 2, on
//! purpose: there must not be a commit in this history where the window is reachable
//! from the internet with nothing but a token in front of it.
//!
//! ## The UI thread never touches a socket
//!
//! `publish` is called from inside the frame, so it must not block for any reason. Each
//! viewer therefore owns an mpsc queue and a writer thread, and publishing is a `send`
//! onto that queue — the same shape `pty.rs` uses for the child's input, and for the
//! same reason: a slow reader at the far end must not be able to wedge the terminal.
//!
//! A queue that grows without bound is the other failure, so it is capped. A viewer
//! that cannot keep up is dropped rather than allowed to consume the window's memory.
//!
//! ## Why the tunnel cannot read any of this
//!
//! A quick tunnel terminates TLS at Cloudflare's edge, so everything would otherwise be
//! in the clear to them. Every frame is therefore encrypted with AES-256-GCM under a key
//! that **travels in the URL fragment** — the part after `#`, which browsers never send
//! to a server. The QR carries it; the tunnel never sees it; what crosses the edge is
//! opaque bytes.
//!
//! The honest limit, because it should not be discovered later: Cloudflare serves the
//! page that does the decrypting, so this defends against capture and logging, not
//! against an edge that rewrites the JavaScript it is handing over. Not needing to trust
//! anybody means not putting anybody in the middle — a private network rather than a
//! tunnel.
//!
//! Nonces are 12 random bytes per message rather than a counter. A counter would have to
//! be unique per key, and the key belongs to the SESSION while counters would belong to
//! each connection: two viewers would reuse the same nonces, which is the one thing
//! AES-GCM does not survive. Random costs 12 bytes a frame and cannot be got wrong.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use sha1::{Digest, Sha1};

use winit::event_loop::EventLoopProxy;

use crate::config::Theme;
use crate::control::{bridge, ControlRequest};
use crate::pocket::{diff, Snapshot};
use crate::UserEvent;

/// Loopback port. One above the share server's 33344, and fixed rather than ephemeral
/// so a person can reach it on the machine itself while testing.
const PORT: u16 = 33345;

/// RFC 6455's magic string. Not a secret and not a choice.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// How many frames may be waiting for one viewer before it is judged too slow. Two
/// seconds of a busy screen; past that the frames are stale anyway.
const MAX_QUEUED: usize = 120;

/// The longest request line accepted, read BEFORE anything is checked, so it needs no
/// credentials to reach. Same reasoning as `share.rs`.
const MAX_REQUEST_LINE: u64 = 8 * 1024;

/// What the window shows about a viewer in the status bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewer {
    pub id: u64,
    /// Typed by the visitor on the pairing screen. A label about themselves, never an
    /// identity — anyone through the door can type anything.
    pub name: Option<String>,
}

struct Client {
    id: u64,
    name: Option<String>,
    tx: mpsc::SyncSender<Vec<u8>>,
}

struct Shared {
    clients: Vec<Client>,
    /// The last snapshot sent to everyone. One shared baseline works because TCP
    /// delivers in order and without loss: a connected viewer has seen every frame
    /// before this one, and a new viewer is sent a complete snapshot on arrival.
    last: Option<Snapshot>,
    theme: Theme,
    /// Six digits shown in the window and NEVER in the link. A quick tunnel URL is a
    /// public URL and ends up in screenshots, history and clipboards; this is the
    /// factor that only ever exists on a screen in the room.
    pin: String,
    /// The session key, ready to use. Every frame in both directions goes through it.
    cipher: Aes256Gcm,
    /// The leader chord this window is configured with. The phone needs it to reach
    /// the layer that everything in runnir is behind, and it is a setting, so it cannot
    /// be assumed to be the default.
    leader: String,
    /// The rectangle a phone should scale to fill; see `pocket::Focus`.
    focus: Option<crate::pocket::Focus>,
    /// What the window is called right now, sent with every frame and with the
    /// snapshot a new viewer gets, so a phone always knows which machine it is holding.
    title: String,
    /// Wrong PINs since the last success. The whole server stops at the limit rather
    /// than that one attempt failing: one person is expected here, and they can read
    /// six digits off a screen in front of them.
    failures: u32,
}

/// Where the public link is in its life. The window shows all three, because a tunnel
/// that is still opening looks identical to one that failed if the panel only knows
/// how to show a URL.
#[derive(Clone, Debug, PartialEq)]
pub enum Tunnel {
    /// No public link asked for: loopback only.
    Off,
    Opening,
    Live(String),
    Failed(String),
}

pub struct Session {
    token: String,
    /// Base64url of the 32-byte key, for the link's fragment. Kept as text because that
    /// is the only form it is ever handed out in.
    key_b64: String,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
    tunnel: Arc<Mutex<Tunnel>>,
    /// Killed explicitly rather than left to notice its parent died: an orphaned
    /// cloudflared holds a public hostname pointing at a port nobody is serving.
    /// `share.rs` found one alive three hours after its daemon had been replaced.
    child: Arc<Mutex<Option<std::process::Child>>>,
}

/// How many wrong PINs end the whole session.
const MAX_FAILURES: u32 = 5;

impl Session {
    /// Binds and starts accepting. Returns immediately; nothing is served until the
    /// first `publish`, because there is nothing to serve.
    pub fn start(theme: Theme, proxy: EventLoopProxy<UserEvent>) -> Result<Session, String> {
        let token = random_token()?;
        let key_bytes = random_bytes::<32>()?;
        let key_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_bytes);
        let listener = TcpListener::bind(("127.0.0.1", PORT)).map_err(|e| {
            // The port is fixed, so this is nearly always the other window rather than
            // a stranger. Saying which it is saves someone hunting for a process.
            if e.kind() == std::io::ErrorKind::AddrInUse {
                "another window is already sharing - stop that one first".to_string()
            } else {
                format!("cannot listen on {PORT}: {e}")
            }
        })?;

        let shared = Arc::new(Mutex::new(Shared {
            clients: Vec::new(),
            last: None,
            theme,
            title: String::new(),
            leader: String::new(),
            focus: None,
            pin: random_pin()?,
            cipher: Aes256Gcm::new_from_slice(&key_bytes)
                .map_err(|e| format!("cannot build the cipher: {e}"))?,
            failures: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let next_id = Arc::new(AtomicU64::new(1));

        {
            let (shared, stop, next_id) = (shared.clone(), stop.clone(), next_id.clone());
            let token = token.clone();
            std::thread::Builder::new()
                .name("runnir-pocket".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let Ok(stream) = stream else { continue };
                        let (shared, token, next_id) =
                            (shared.clone(), token.clone(), next_id.clone());
                        let proxy = proxy.clone();
                        std::thread::spawn(move || serve(stream, &token, &shared, &next_id, &proxy));
                    }
                })
                .map_err(|e| format!("cannot start server thread: {e}"))?;
        }

        Ok(Session {
            token,
            key_b64,
            shared,
            stop,
            next_id,
            tunnel: Arc::new(Mutex::new(Tunnel::Off)),
            child: Arc::new(Mutex::new(None)),
        })
    }

    /// Asks for a public link, in the background.
    ///
    /// Opening one takes the better part of a minute — cloudflared has to connect, DNS
    /// has to appear, and the edge has to start routing — and this is called from the
    /// UI thread. So it returns at once and the window watches `tunnel()` instead of
    /// freezing with a spinner it cannot animate.
    pub fn open_tunnel(&self) {
        {
            let Ok(mut state) = self.tunnel.lock() else { return };
            if matches!(*state, Tunnel::Opening | Tunnel::Live(_)) {
                return;
            }
            *state = Tunnel::Opening;
        }
        let (state, child, path) = (self.tunnel.clone(), self.child.clone(), self.public_path());
        std::thread::Builder::new()
            .name("runnir-pocket-tunnel".into())
            .spawn(move || {
                // This thread must OUTLIVE the tunnel, not merely start it.
                //
                // `PR_SET_PDEATHSIG` fires when the parent THREAD dies, not the parent
                // process — a distinction the man page makes and that costs an
                // afternoon to rediscover. Spawning here and returning killed
                // cloudflared moments after it had successfully published a URL, and
                // the only symptom was a link that worked once and then answered 530
                // with a `<defunct>` child nobody had reaped.
                //
                // So the draining loop lives here instead of in a thread of its own:
                // it keeps cloudflared's stderr moving (a full pipe would block it)
                // and it keeps this thread alive for exactly as long as the child.
                let (mut proc, host, mut stderr) = match spawn_tunnel() {
                    Ok(started) => started,
                    Err(e) => {
                        *state.lock().unwrap() = Tunnel::Failed(e);
                        return;
                    }
                };
                let url = format!("https://{host}{path}");
                // Probed WITHOUT the fragment: it is not part of a request, and an
                // HTTP client handed one either strips it or chokes on it.
                let probe = format!("https://{host}{}", path.split('#').next().unwrap_or("/"));
                match wait_until_reachable(&host, &probe) {
                    Ok(()) => *state.lock().unwrap() = Tunnel::Live(url),
                    Err(why) => {
                        *state.lock().unwrap() = Tunnel::Failed(why);
                        let _ = proc.kill();
                        let _ = proc.wait();
                        return;
                    }
                }
                *child.lock().unwrap() = Some(proc);

                let mut sink = String::new();
                while stderr.read_line(&mut sink).unwrap_or(0) > 0 {
                    sink.clear();
                }
            })
            .ok();
    }

    pub fn tunnel(&self) -> Tunnel {
        self.tunnel.lock().map(|t| t.clone()).unwrap_or(Tunnel::Off)
    }

    fn public_path(&self) -> String {
        // The fragment is part of what the QR must carry and NOT part of what is
        // requested: browsers never send it, which is exactly why the key rides there.
        format!("/r/{}/#k={}", self.token, self.key_b64)
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{PORT}/r/{}/#k={}", self.token, self.key_b64)
    }

    /// The six digits the window shows. Read every time rather than cached, because
    /// rotating has to change what the window displays in the same frame.
    pub fn pin(&self) -> String {
        self.shared.lock().map(|s| s.pin.clone()).unwrap_or_default()
    }

    /// Ends every existing session and issues a new PIN.
    ///
    /// This is the action that means "you are not coming back in". Dropping a viewer
    /// does not: they still hold the link and the digits. Phones already connected are
    /// unaffected — they are through the door — which is what makes this usable while
    /// somebody you invited is still reading.
    pub fn rotate_pin(&self) -> String {
        let Ok(mut shared) = self.shared.lock() else { return String::new() };
        if let Ok(pin) = random_pin() {
            shared.pin = pin;
        }
        shared.failures = 0;
        shared.pin.clone()
    }

    /// Whether the server shut itself down after too many wrong PINs, so the window
    /// does not keep a dead session in its status bar.
    pub fn is_dead(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Ends one viewer's session: their socket closes and their cookie stops working.
    /// They still hold the link and the PIN, so this is not a ban — see `rotate_pin`.
    pub fn drop_viewer(&self, id: u64) {
        let Ok(mut shared) = self.shared.lock() else { return };
        shared.clients.retain(|c| c.id != id);
    }

    /// Who is watching, for the status bar.
    pub fn viewers(&self) -> Vec<Viewer> {
        self.shared
            .lock()
            .map(|s| {
                s.clients
                    .iter()
                    .map(|c| Viewer {
                        id: c.id,
                        name: c.name.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sends what changed since the last call. Called from the frame: never blocks,
    /// never writes to a socket, and does nothing at all when nobody is connected.
    pub fn publish(
        &self,
        snapshot: Snapshot,
        title: &str,
        leader: &str,
        focus: Option<crate::pocket::Focus>,
    ) {
        let Ok(mut shared) = self.shared.lock() else { return };
        shared.title = title.to_string();
        if shared.leader != leader {
            shared.leader = leader.to_string();
        }
        shared.focus = focus;
        if shared.clients.is_empty() {
            // Keep the baseline anyway: the next viewer gets a full snapshot on
            // arrival, so a stale `last` would only make the FIRST diff after that
            // wrong. Cheap to keep, subtle to debug if dropped.
            shared.last = Some(snapshot);
            return;
        }

        let theme = shared.theme.clone();
        let rows = diff(shared.last.as_ref(), &snapshot, &theme);
        if rows.is_empty() && shared.last.is_some() {
            return;
        }

        let payload = serde_json::json!({
            "cols": snapshot.cols,
            "rows": snapshot.rows,
            "cursor": snapshot.cursor,
            "title": title,
            "leader": shared.leader,
            "focus": shared.focus,
            "updates": rows,
        });
        let frame = match serde_json::to_vec(&payload) {
            Ok(v) => match seal(&shared.cipher, &v) {
                Some(sealed) => binary_frame(&sealed),
                None => return,
            },
            Err(_) => return,
        };

        shared.clients.retain(|c| c.tx.try_send(frame.clone()).is_ok());
        shared.last = Some(snapshot);
    }

}

impl Drop for Session {
    fn drop(&mut self) {
        // A window that went away must not leave a port answering, and must not leave
        // a public hostname pointing at it either.
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", PORT));
        if let Ok(mut shared) = self.shared.lock() {
            shared.clients.clear();
        }
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut proc) = child.take() {
                let _ = proc.kill();
                let _ = proc.wait();
            }
        }
        let _ = &self.next_id;
    }
}

/// Starts cloudflared and reads the hostname it announces.
///
/// Returns the still-open stderr so the CALLER can keep draining it on the thread that
/// spawned the child — see `open_tunnel` for why that thread must not end.
type Started = (std::process::Child, String, BufReader<std::process::ChildStderr>);

fn spawn_tunnel() -> Result<Started, String> {
    let mut cmd = std::process::Command::new(crate::platform::find_tool("cloudflared"));
    cmd.args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{PORT}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // Die with the window, whatever kills it: neither Drop nor any teardown runs when
    // the process is signalled, and that is how a terminal usually ends.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // SAFETY: async-signal-safe, and touches nothing but this new process.
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() == 1 {
                std::process::exit(0);
            }
            Ok(())
        });
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cloudflared did not start ({e}) - is it installed?"))?;
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        return Err("cloudflared gave no output".into());
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    let mut reader = BufReader::new(stderr);
    let mut host = None;
    while std::time::Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if let Some(found) = line
            .split_whitespace()
            .find(|w| w.starts_with("https://") && w.contains("trycloudflare.com"))
        {
            host = Some(found.trim_start_matches("https://").trim_end_matches('/').to_string());
            break;
        }
    }
    match host {
        Some(host) => Ok((child, host, reader)),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err("cloudflared never announced a URL".into())
        }
    }
}

/// Waits until the link actually works, which is later than it looks twice over.
///
/// cloudflared prints the URL several seconds before the edge routes to it — `share.rs`
/// learned that one. The DNS record appears later still, and that one is worse: asking
/// before it exists earns a real NXDOMAIN that the asker's resolver caches for its
/// negative TTL, so retrying is just re-reading a cached no. This code was written
/// without that guard, pressed the button, and sat for ninety seconds doing exactly
/// that — the third time the same trap was walked into in one day.
///
/// Hence the order: ask **1.1.1.1 over DoH** first, which cannot poison anything
/// because its own address is a literal and needs no lookup, and only touch the system
/// resolver once the record is known to exist. Nothing is published until a request
/// really succeeds, because a QR that scans to nothing is worse than no QR.
fn wait_until_reachable(host: &str, url: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

    while std::time::Instant::now() < deadline {
        if dns_exists_upstream(host) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    let mut last = String::from("the name never appeared in public DNS");
    while std::time::Instant::now() < deadline {
        match std::net::ToSocketAddrs::to_socket_addrs(&(host, 443)) {
            Ok(_) => match ureq::get(url).call() {
                // Any answer means the edge is routing to us; a 404 from an
                // unauthenticated path proves it as well as a 200 would.
                Ok(_) | Err(ureq::Error::StatusCode(_)) => return Ok(()),
                Err(e) => last = format!("the edge is not routing yet ({e})"),
            },
            Err(e) => last = format!("this machine cannot resolve it ({e})"),
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Err(last)
}

/// Whether the name exists according to a resolver that is not ours.
///
/// Queried by IP literal on purpose: no lookup happens to ask the question, so asking
/// early costs nothing and cannot leave a negative answer behind in any cache.
fn dns_exists_upstream(host: &str) -> bool {
    let url = format!("https://1.1.1.1/dns-query?name={host}&type=A");
    let Ok(mut resp) = ureq::get(&url).header("accept", "application/dns-json").call() else {
        return false;
    };
    let Ok(body) = resp.body_mut().read_to_string() else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
        return false;
    };
    parsed
        .get("Answer")
        .and_then(|a| a.as_array())
        .is_some_and(|answers| answers.iter().any(|a| a.get("type").and_then(|t| t.as_u64()) == Some(1)))
}

fn serve(
    mut stream: TcpStream,
    token: &str,
    shared: &Arc<Mutex<Shared>>,
    next_id: &Arc<AtomicU64>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(15)));

    let mut head = String::new();
    let mut reader = BufReader::new((&stream).take(MAX_REQUEST_LINE));
    if reader.read_line(&mut head).is_err() {
        return;
    }
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.trim().is_empty() => break,
            Ok(_) => headers.push(line.trim().to_string()),
            Err(_) => return,
        }
    }

    let Some(target) = head.split_whitespace().nth(1) else { return };
    let prefix = format!("/r/{token}");
    let Some(rest) = target.strip_prefix(&prefix) else {
        respond(&mut stream, "404 Not Found", "text/plain", b"no");
        return;
    };

    match rest {
        "" | "/" => respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        "/font.ttf" => {
            // The same bytes `font.rs` draws with. A phone has no Nerd Font, and a
            // prompt without them is mostly missing glyphs.
            respond_cached(&mut stream, "font/ttf", crate::font::EMBEDDED_NERD_REGULAR)
        }
        p if p.starts_with("/ws") => upgrade(stream, &headers, shared, next_id, proxy),
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"no"),
    }
}

/// Whether the offered digits are the expected ones, without short-circuiting.
///
/// With five attempts before the server stops, timing is not the threat here. It is
/// written this way because it costs nothing, and because a credential comparison that
/// returns early is the kind of thing nobody wants to have to defend later.
fn pin_matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn upgrade(
    mut stream: TcpStream,
    headers: &[String],
    shared: &Arc<Mutex<Shared>>,
    next_id: &Arc<AtomicU64>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    // A WebSocket upgrade may be made by any page that knows the URL, and the URL is
    // the one part of this the tunnel can read. `Origin` is the standard check: a
    // browser sets it and cannot be talked out of it, so a page on some other site
    // cannot open this socket even holding the link.
    //
    // Absent is allowed on purpose - that is a non-browser client (a script, a test),
    // which is not the thing this defends against. Present and foreign is refused.
    if let Some(origin) = header(headers, "origin") {
        let host = header(headers, "host").unwrap_or_default();
        let ok = origin
            .strip_prefix("https://")
            .or_else(|| origin.strip_prefix("http://"))
            .is_some_and(|o| o == host);
        if !ok {
            respond(&mut stream, "403 Forbidden", "text/plain", b"bad origin");
            return;
        }
    }

    let key = headers.iter().find_map(|h| {
        let (k, v) = h.split_once(':')?;
        k.trim().eq_ignore_ascii_case("sec-websocket-key").then(|| v.trim().to_string())
    });
    let Some(key) = key else {
        respond(&mut stream, "400 Bad Request", "text/plain", b"not a websocket");
        return;
    };

    let accept = {
        let mut h = Sha1::new();
        h.update(key.as_bytes());
        h.update(WS_GUID.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(h.finalize())
    };
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(None);

    let cipher = match shared.lock() {
        Ok(s) => s.cipher.clone(),
        Err(_) => return,
    };

    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(MAX_QUEUED);

    // The writer thread owns the socket's write half. Nothing else ever writes to it,
    // so the UI thread cannot be blocked by this viewer's connection.
    let Ok(writer) = stream.try_clone() else { return };
    std::thread::spawn(move || {
        let mut writer = writer;
        while let Ok(frame) = rx.recv() {
            if writer.write_all(&frame).is_err() {
                return;
            }
        }
        // The channel closed: this viewer was dropped. Say so rather than leaving the
        // socket half-open, so the page's reconnect logic starts immediately.
        let _ = writer.write_all(&[0x88, 0x00]);
    });

    // Nothing is sent until the PIN arrives, and the PIN arrives INSIDE the encrypted
    // channel. That is the point of moving pairing here from an HTTP route: a POST
    // carried it in the clear past the edge, and a cookie did the same on every
    // reconnect afterwards.
    let mut paired = false;
    let mut conn = stream;
    loop {
        let (opcode, payload) = match read_frame(&mut conn) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(_) => break,
        };
        match opcode {
            0x8 => break,
            0x9 => {
                let mut pong = vec![0x8A, payload.len().min(125) as u8];
                pong.extend_from_slice(&payload[..payload.len().min(125)]);
                if conn.write_all(&pong).is_err() {
                    break;
                }
            }
            0x2 => {
                // Anything that does not authenticate is not a client of ours, and
                // there is nothing useful to answer it with.
                let Some(plain) = open_sealed(&cipher, &payload) else { break };
                if paired {
                    if let Some(req) = request_from(&plain) {
                        // The same bridge `runnir @` uses, so a phone drives the
                        // terminal through the machinery a script does - leader layer,
                        // overlays and the guardian included.
                        let _ = bridge(req, proxy);
                    }
                    continue;
                }
                match try_pair(&plain, shared, id, &tx) {
                    Pairing::Ok => paired = true,
                    Pairing::Wrong(left) => {
                        let body = format!("{{\"t\":\"denied\",\"left\":{left}}}");
                        if let Some(frame) = seal(&cipher, body.as_bytes()) {
                            let _ = tx.try_send(binary_frame(&frame));
                        }
                        if left == 0 {
                            break;
                        }
                    }
                    Pairing::Gone => break,
                }
            }
            _ => {}
        }
    }

    if let Ok(mut s) = shared.lock() {
        s.clients.retain(|c| c.id != id);
    }
}

enum Pairing {
    Ok,
    /// Wrong PIN, with the attempts remaining. Zero means the session is over.
    Wrong(u32),
    Gone,
}

/// Checks the PIN and, if it is right, registers the viewer and sends them a screen.
fn try_pair(
    plain: &[u8],
    shared: &Arc<Mutex<Shared>>,
    id: u64,
    tx: &mpsc::SyncSender<Vec<u8>>,
) -> Pairing {
    let msg: serde_json::Value = match serde_json::from_slice(plain) {
        Ok(v) => v,
        Err(_) => return Pairing::Gone,
    };
    if msg.get("t").and_then(|t| t.as_str()) != Some("pair") {
        return Pairing::Gone;
    }
    let offered = msg.get("pin").and_then(|v| v.as_str()).unwrap_or("");
    let name = msg
        .get("name")
        .and_then(|v| v.as_str())
        .map(|n| n.trim().chars().take(24).collect::<String>())
        .filter(|n| !n.is_empty());

    let Ok(mut s) = shared.lock() else { return Pairing::Gone };

    if !pin_matches(&s.pin, offered) {
        s.failures += 1;
        return Pairing::Wrong(MAX_FAILURES.saturating_sub(s.failures));
    }
    s.failures = 0;

    // Everything this viewer needs to draw a screen, before any diff reaches them.
    if let Some(snapshot) = s.last.clone() {
        let theme = s.theme.clone();
        let title = s.title.clone();
        let payload = serde_json::json!({
            "cols": snapshot.cols,
            "rows": snapshot.rows,
            "cursor": snapshot.cursor,
            "title": title,
            "updates": diff(None, &snapshot, &theme),
        });
        if let Ok(v) = serde_json::to_vec(&payload) {
            if let Some(frame) = seal(&s.cipher, &v) {
                let _ = tx.try_send(binary_frame(&frame));
            }
        }
    }
    s.clients.push(Client { id, name, tx: tx.clone() });
    Pairing::Ok
}

/// One header, by name.
fn header(headers: &[String], name: &str) -> Option<String> {
    headers.iter().find_map(|h| {
        let (k, v) = h.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    })
}

/// What a phone asked for, as a control request.
///
/// Only four shapes are accepted, and the omissions are deliberate:
///
/// * `key` — a chord, taking the path a real keypress takes. This is how Enter always
///   arrives, and Enter is where `guardian` asks "run this?". A phone must not be able
///   to run a dangerous command without that question, so **newlines are refused in
///   `text` below** and the only way to submit a line is a key.
/// * `text` — printable characters, for typing and pasting, with control characters
///   stripped. Without this every keystroke on a phone keyboard would be a chord
///   lookup, and autocorrect would be impossible to represent at all.
/// * `click` — a cell. The whole window is on the phone, so tapping a tab or a row of
///   the git panel means the same thing it means with a mouse.
/// * `wheel` — the scrollback, which keys alone cannot move without losing your place.
/// * `action` — a named action by its config id, for the phone's menu.
///
/// Anything else is ignored rather than guessed at: this is input arriving from the
/// internet, and the list of what it may do belongs here, in one place.
fn request_from(payload: &[u8]) -> Option<ControlRequest> {
    let msg: serde_json::Value = serde_json::from_slice(payload).ok()?;
    match msg.get("t")?.as_str()? {
        "key" => Some(ControlRequest::Key {
            chord: msg.get("chord")?.as_str()?.to_string(),
        }),
        "text" => {
            let text: String = msg
                .get("text")?
                .as_str()?
                .chars()
                // A newline here would reach the child without passing Enter, and
                // Enter is where the guardian stands.
                .filter(|c| !c.is_control())
                .take(4096)
                .collect();
            (!text.is_empty()).then_some(ControlRequest::SendText { text, target: None })
        }
        "click" => Some(ControlRequest::Click {
            col: msg.get("col")?.as_u64()? as usize,
            row: msg.get("row")?.as_u64()? as usize,
            button: None,
        }),
        // Named actions, for the phone's own menu. This is what lets a phone switch
        // pane or open a panel WITHOUT arming the leader layer — which would put a
        // which-key menu on the desk's screen and then wait for a letter the phone
        // cannot comfortably type. It adds no reach: everything here is already
        // typeable by whoever is through the door.
        "action" => Some(ControlRequest::Action {
            id: msg.get("id")?.as_str()?.chars().take(64).collect(),
        }),
        "wheel" => Some(ControlRequest::Wheel {
            col: msg.get("col")?.as_u64()? as usize,
            row: msg.get("row")?.as_u64()? as usize,
            // Signed the way a wheel is: positive is up, away from the user.
            lines: msg.get("lines").and_then(|v| v.as_f64()).map(|l| l as f32),
        }),
        _ => None,
    }
}

/// One frame from the client. Client frames are always masked; an unmasked one is a
/// protocol error and the connection ends rather than being guessed at.
fn read_frame(conn: &mut TcpStream) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 2];
    conn.read_exact(&mut head)?;
    let opcode = head[0] & 0x0F;
    let masked = head[1] & 0x80 != 0;
    let mut len = (head[1] & 0x7F) as usize;
    if len == 126 {
        let mut ext = [0u8; 2];
        conn.read_exact(&mut ext)?;
        len = u16::from_be_bytes(ext) as usize;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        conn.read_exact(&mut ext)?;
        len = u64::from_be_bytes(ext) as usize;
    }
    if !masked || len > 1 << 20 {
        return Err(std::io::Error::other("bad frame"));
    }
    let mut mask = [0u8; 4];
    conn.read_exact(&mut mask)?;
    let mut payload = vec![0u8; len];
    conn.read_exact(&mut payload)?;
    for (i, b) in payload.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
    Ok(Some((opcode, payload)))
}

/// A server-to-client BINARY frame. Never masked, which is the rule for this
/// direction. Binary because what travels is ciphertext, not text.
fn binary_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x82);
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n < 65536 => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn respond_cached(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: max-age=86400\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// Six digits, uniformly.
///
/// Rejection sampling rather than `% 1_000_000`: a modulo over 2^32 favours the low
/// end of the range, which is exactly the kind of quiet bias that is never noticed and
/// never wanted in a credential.
fn random_pin() -> Result<String, String> {
    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|e| format!("no randomness available: {e}"))?;
    loop {
        let mut bytes = [0u8; 4];
        f.read_exact(&mut bytes)
            .map_err(|e| format!("no randomness available: {e}"))?;
        let n = u32::from_le_bytes(bytes);
        // The largest multiple of 1_000_000 that fits in a u32; anything above it
        // would make the first 294_967_296 values twice as likely.
        const LIMIT: u32 = 4_294_000_000;
        if n < LIMIT {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no randomness available: {e}"))?;
    Ok(bytes)
}

fn random_token() -> Result<String, String> {
    let bytes = random_bytes::<16>()?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Wraps a message for the wire: 12 random nonce bytes, then the sealed payload.
fn seal(cipher: &Aes256Gcm, plaintext: &[u8]) -> Option<Vec<u8>> {
    let nonce_bytes = random_bytes::<12>().ok()?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let sealed = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad: b"" })
        .ok()?;
    let mut out = Vec::with_capacity(12 + sealed.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Some(out)
}

/// Unwraps one. Returns `None` for anything that does not authenticate, which is the
/// only answer a tampered or replayed-with-changes frame ever gets.
fn open_sealed(cipher: &Aes256Gcm, frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() < 12 + 16 {
        return None;
    }
    let (nonce_bytes, sealed) = frame.split_at(12);
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), Payload { msg: sealed, aad: b"" })
        .ok()
}

const PAGE: &str = include_str!("pocket.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_uses_the_shortest_length_form() {
        assert_eq!(binary_frame(b"hi")[..2], [0x82, 2]);
        let medium = binary_frame(&vec![b'x'; 200]);
        assert_eq!(medium[0], 0x82);
        assert_eq!(medium[1], 126);
        assert_eq!(u16::from_be_bytes([medium[2], medium[3]]), 200);
        let large = binary_frame(&vec![b'x'; 70000]);
        assert_eq!(large[1], 127);
    }

    #[test]
    fn a_sealed_message_survives_the_round_trip_and_nothing_else_does() {
        let key = random_bytes::<32>().unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let sealed = seal(&cipher, b"hello phone").unwrap();
        assert_eq!(open_sealed(&cipher, &sealed).as_deref(), Some(&b"hello phone"[..]));

        // A different key is a different conversation.
        let other = Aes256Gcm::new_from_slice(&random_bytes::<32>().unwrap()).unwrap();
        assert!(open_sealed(&other, &sealed).is_none());

        // One flipped bit anywhere fails the tag rather than decrypting to something.
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open_sealed(&cipher, &tampered).is_none());
        let mut nonce_tampered = sealed.clone();
        nonce_tampered[0] ^= 1;
        assert!(open_sealed(&cipher, &nonce_tampered).is_none());

        // Too short to hold a nonce and a tag is refused before anything is attempted.
        assert!(open_sealed(&cipher, &[0u8; 8]).is_none());
    }

    #[test]
    fn two_seals_of_the_same_message_differ() {
        // Random nonces, so identical screens do not produce identical ciphertext -
        // which would tell an observer that nothing changed between two frames.
        let cipher = Aes256Gcm::new_from_slice(&random_bytes::<32>().unwrap()).unwrap();
        let a = seal(&cipher, b"same").unwrap();
        let b = seal(&cipher, b"same").unwrap();
        assert_ne!(a, b);
        assert_eq!(open_sealed(&cipher, &a), open_sealed(&cipher, &b));
    }

    #[test]
    fn the_handshake_matches_the_rfc_example() {
        // RFC 6455 section 1.3, so a broken SHA-1 or base64 fails here rather than in
        // a browser that merely refuses to connect.
        let mut h = Sha1::new();
        h.update(b"dGhlIHNhbXBsZSBub25jZQ==");
        h.update(WS_GUID.as_bytes());
        let accept = base64::engine::general_purpose::STANDARD.encode(h.finalize());
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn a_pin_is_six_digits_and_not_the_same_one_twice() {
        let a = random_pin().unwrap();
        assert_eq!(a.len(), 6, "six digits, zero-padded - {a:?}");
        assert!(a.chars().all(|c| c.is_ascii_digit()));
        // Not a strong statement about randomness, just that it is not a constant.
        let differs = (0..20).any(|_| random_pin().unwrap() != a);
        assert!(differs);
    }

    #[test]
    fn a_pin_comparison_rejects_prefixes_and_padding() {
        assert!(pin_matches("012345", "012345"));
        assert!(!pin_matches("012345", "01234"), "a prefix is not a match");
        assert!(!pin_matches("012345", "0123456"), "nor is a longer string");
        assert!(!pin_matches("012345", ""), "nor is nothing at all");
        assert!(!pin_matches("012345", "012346"));
    }

    #[test]
    fn a_newline_cannot_arrive_as_text() {
        // Enter is where the guardian asks "run this?". Text that could carry a
        // newline would reach the child without passing it, so the filter here is a
        // safety property and not tidiness.
        let req = request_from(br#"{"t":"text","text":"rm -rf /\n"}"#).unwrap();
        match req {
            ControlRequest::SendText { text, .. } => assert_eq!(text, "rm -rf /"),
            other => panic!("expected text, got {other:?}"),
        }
        assert!(
            request_from(br#"{"t":"text","text":"\n"}"#).is_none(),
            "a newline on its own leaves nothing to send"
        );
        assert!(request_from(br#"{"t":"text","text":"\u0003"}"#).is_none(), "nor does a control byte");
    }

    #[test]
    fn only_three_shapes_are_accepted() {
        assert!(matches!(
            request_from(br#"{"t":"key","chord":"ctrl+c"}"#),
            Some(ControlRequest::Key { .. })
        ));
        assert!(matches!(
            request_from(br#"{"t":"click","col":4,"row":9}"#),
            Some(ControlRequest::Click { col: 4, row: 9, .. })
        ));
        // Everything else is ignored rather than guessed at - this is input from the
        // internet, and what it may do is decided in one place.
        assert!(request_from(br#"{"t":"launch","cmd":"sh"}"#).is_none());
        assert!(request_from(br#"{"t":"transfer","path":"/etc/passwd"}"#).is_none());
        assert!(request_from(br#"{"cmd":"key","args":{"chord":"a"}}"#).is_none());
        assert!(request_from(b"not json at all").is_none());
        assert!(request_from(b"{}").is_none());
    }

    #[test]
    fn a_wheel_carries_its_direction() {
        match request_from(br#"{"t":"wheel","col":5,"row":9,"lines":-3}"#) {
            Some(ControlRequest::Wheel { col, row, lines }) => {
                assert_eq!((col, row), (5, 9));
                // Signed the way a wheel is; a client that lost the sign would scroll
                // the wrong way and look like a broken gesture rather than a bug.
                assert_eq!(lines, Some(-3.0));
            }
            other => panic!("expected a wheel, got {other:?}"),
        }
        // Missing `lines` is allowed - the terminal's own default applies.
        assert!(matches!(
            request_from(br#"{"t":"wheel","col":0,"row":0}"#),
            Some(ControlRequest::Wheel { lines: None, .. })
        ));
    }

    #[test]
    fn a_token_is_long_enough_to_be_worth_nothing_to_guess() {
        let t = random_token().unwrap();
        assert_eq!(t.len(), 32);
        assert_ne!(t, random_token().unwrap());
    }
}
