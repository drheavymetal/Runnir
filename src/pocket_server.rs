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

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use base64::Engine;
use sha1::{Digest, Sha1};

use crate::config::Theme;
use crate::pocket::{diff, Snapshot};

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
}

pub struct Session {
    token: String,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
}

impl Session {
    /// Binds and starts accepting. Returns immediately; nothing is served until the
    /// first `publish`, because there is nothing to serve.
    pub fn start(theme: Theme) -> Result<Session, String> {
        let token = random_token()?;
        let listener = TcpListener::bind(("127.0.0.1", PORT))
            .map_err(|e| format!("cannot listen on {PORT}: {e}"))?;

        let shared = Arc::new(Mutex::new(Shared {
            clients: Vec::new(),
            last: None,
            theme,
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
                        std::thread::spawn(move || serve(stream, &token, &shared, &next_id));
                    }
                })
                .map_err(|e| format!("cannot start server thread: {e}"))?;
        }

        Ok(Session {
            token,
            shared,
            stop,
            next_id,
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{PORT}/r/{}/", self.token)
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
    pub fn publish(&self, snapshot: Snapshot) {
        let Ok(mut shared) = self.shared.lock() else { return };
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
            "updates": rows,
        });
        let frame = match serde_json::to_vec(&payload) {
            Ok(v) => text_frame(&v),
            Err(_) => return,
        };

        shared.clients.retain(|c| c.tx.try_send(frame.clone()).is_ok());
        shared.last = Some(snapshot);
    }

}

impl Drop for Session {
    fn drop(&mut self) {
        // A window that went away must not leave a port answering.
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", PORT));
        if let Ok(mut shared) = self.shared.lock() {
            shared.clients.clear();
        }
        let _ = &self.next_id;
    }
}

fn serve(mut stream: TcpStream, token: &str, shared: &Arc<Mutex<Shared>>, next_id: &Arc<AtomicU64>) {
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
        p if p.starts_with("/ws") => {
            let name = query_param(p, "name");
            upgrade(stream, &headers, shared, next_id, name)
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"no"),
    }
}

fn upgrade(
    mut stream: TcpStream,
    headers: &[String],
    shared: &Arc<Mutex<Shared>>,
    next_id: &Arc<AtomicU64>,
    name: Option<String>,
) {
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

    // Everything this viewer needs to draw a screen, before any diff reaches them.
    {
        let Ok(mut s) = shared.lock() else { return };
        if let Some(snapshot) = s.last.clone() {
            let theme = s.theme.clone();
            let payload = serde_json::json!({
                "cols": snapshot.cols,
                "rows": snapshot.rows,
                "cursor": snapshot.cursor,
                "updates": diff(None, &snapshot, &theme),
            });
            if let Ok(v) = serde_json::to_vec(&payload) {
                let _ = tx.try_send(text_frame(&v));
            }
        }
        s.clients.push(Client { id, name, tx });
    }

    // Read until the viewer goes away. Phase 1 is read-only, so anything they send is
    // discarded — but the frames still have to be PARSED, because a close frame is how
    // a browser says goodbye and a ping expects a pong.
    let mut conn = stream;
    loop {
        match read_frame(&mut conn) {
            Ok(Some((opcode, payload))) => match opcode {
                0x8 => break,
                0x9 => {
                    let mut pong = vec![0x8A, payload.len().min(125) as u8];
                    pong.extend_from_slice(&payload[..payload.len().min(125)]);
                    if conn.write_all(&pong).is_err() {
                        break;
                    }
                }
                _ => {}
            },
            Ok(None) => {}
            Err(_) => break,
        }
    }

    if let Ok(mut s) = shared.lock() {
        s.clients.retain(|c| c.id != id);
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

/// A server-to-client text frame. Never masked, which is the rule for this direction.
fn text_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x81);
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

fn query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no randomness available: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

const PAGE: &str = include_str!("pocket.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_frame_uses_the_shortest_length_form() {
        assert_eq!(text_frame(b"hi")[..2], [0x81, 2]);
        let medium = text_frame(&vec![b'x'; 200]);
        assert_eq!(medium[0], 0x81);
        assert_eq!(medium[1], 126);
        assert_eq!(u16::from_be_bytes([medium[2], medium[3]]), 200);
        let large = text_frame(&vec![b'x'; 70000]);
        assert_eq!(large[1], 127);
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
    fn a_query_name_survives_encoding() {
        assert_eq!(query_param("/ws?name=Pedro", "name").as_deref(), Some("Pedro"));
        assert_eq!(query_param("/ws?name=Jos%C3%A9", "name").as_deref(), Some("José"));
        assert_eq!(query_param("/ws?name=two+words", "name").as_deref(), Some("two words"));
        assert_eq!(query_param("/ws", "name"), None);
    }

    #[test]
    fn a_token_is_long_enough_to_be_worth_nothing_to_guess() {
        let t = random_token().unwrap();
        assert_eq!(t.len(), 32);
        assert_ne!(t, random_token().unwrap());
    }
}
