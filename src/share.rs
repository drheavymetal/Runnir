//! Sharing what is playing: a local web server, a Cloudflare quick tunnel, and a link.
//!
//! Owned by the daemon, never by a window. Pedro's requirement, and the reason the
//! daemon exists at all in this order: a link that dies because you closed the terminal
//! that made it is not a link you would give anybody.
//!
//! ## What is served, and why not the samples
//!
//! The obvious thing is to tee the decoded audio on its way to the DAC. It is also the
//! wrong thing here: at 24 bit / 192 kHz that is 1.1 MB per second of raw PCM, and
//! sending it anywhere means adding an encoder — another dependency, another format
//! decision, CPU spent on every packet whether anyone is listening or not.
//!
//! What TIDAL sends is already compressed, so the listener gets THAT instead: the same
//! parts, fetched again for them. It costs one extra download of a song somebody is
//! already paying for, and it costs nothing at all while nobody is listening. It also
//! keeps the share completely out of the playback path — a slow listener cannot stall
//! the music, because there is no shared buffer to stall.
//!
//! ## What this is, plainly
//!
//! It re-transmits licensed audio to whoever holds the URL. The token is random and the
//! link dies with the session, which makes it a private link and not a service — but it
//! is worth being clear rather than describing it as anything cleverer.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::player::Snapshot;
use crate::tidal;

/// The port the tunnel points at. Fixed rather than ephemeral so a person can reach it
/// on the machine itself, and high enough not to collide with anything usual.
const PORT: u16 = 33344;

/// How long cloudflared is given to publish a URL before it is judged a failure.
const TUNNEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// The longest request line we will read. A sender that trickles bytes and never sends
/// a newline would otherwise grow a String until the machine gives up — and this runs
/// BEFORE the token is checked, so it needs no link to reach.
const MAX_REQUEST_LINE: u64 = 8 * 1024;

/// How many people may be listening at once. Each one holds a thread and re-downloads
/// the track, so an unbounded count is both a memory problem and a way to hammer
/// somebody's TIDAL account from a link they shared with a friend.
const MAX_LISTENERS: usize = 4;

/// A listener that stops reading must not pin a thread for ever.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What the panel needs to know about a share.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct State {
    pub url: String,
    pub listeners: usize,
    /// Set when starting failed, so the panel can say why rather than showing nothing.
    pub error: Option<String>,
}

/// A running share: the server, the tunnel, and the count of who is listening.
pub struct Share {
    token: String,
    url: String,
    listeners: Arc<AtomicUsize>,
    tunnel: Option<std::process::Child>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Share {
    /// Starts serving and opens a tunnel. Blocks until there is a URL to hand back.
    pub fn start(state: Arc<Mutex<Snapshot>>) -> Result<Share, String> {
        let token = random_token()?;
        let listener = TcpListener::bind(("127.0.0.1", PORT))
            .map_err(|e| format!("cannot listen on {PORT}: {e}"))?;
        let listeners = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        {
            let token = token.clone();
            let listeners = listeners.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("runnir-share".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let Ok(stream) = stream else { continue };
                        let token = token.clone();
                        let listeners = listeners.clone();
                        let state = state.clone();
                        std::thread::spawn(move || serve(stream, &token, &state, &listeners));
                    }
                })
                .ok();
        }

        let opened = open_tunnel();
        let (tunnel, url) = match opened {
            Ok(pair) => pair,
            Err(e) => {
                // The listener is already bound and its thread already running. Without
                // this the port stays held for the life of the daemon, and the SECOND
                // attempt fails with "address already in use" — an error that hides the
                // real one (cloudflared missing) and never goes away.
                stop.store(true, Ordering::Relaxed);
                let _ = TcpStream::connect(("127.0.0.1", PORT));
                return Err(e);
            }
        };
        let url = format!("{url}/r/{token}");
        // cloudflared prints the URL as soon as it has one, several seconds BEFORE the
        // edge will route to it — the first attempt at this returned nothing at all
        // from a link that had just been announced as ready. Handing somebody a URL
        // that does not answer yet is handing them a broken link, so it is not returned
        // until it answers.
        wait_until_live(&url);
        Ok(Share {
            url,
            token,
            listeners,
            tunnel: Some(tunnel),
            stop,
        })
    }

    pub fn state(&self) -> State {
        State {
            url: self.url.clone(),
            listeners: self.listeners.load(Ordering::Relaxed),
            error: None,
        }
    }

    /// Ends the share. The tunnel child is killed rather than left to notice, so the
    /// URL stops answering the moment the person said stop.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Poke the listener so its blocking accept returns and the thread can see the
        // flag. Connecting to ourselves is the cheapest way to do that.
        let _ = TcpStream::connect(("127.0.0.1", PORT));
        if let Some(mut child) = self.tunnel.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = &self.token;
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        // A share whose owner went away must not leave a public URL answering, so the
        // tunnel dies with the struct however the struct dies.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.tunnel.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// One request.
fn serve(mut stream: TcpStream, token: &str, state: &Arc<Mutex<Snapshot>>, listeners: &Arc<AtomicUsize>) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    // Without this a listener that stops reading blocks a write for ever, holding a
    // thread and the whole track's bytes with it.
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let mut head = String::new();
    // Bounded, and bounded BEFORE the token is checked: the read timeout is per-recv,
    // so a sender trickling bytes with no newline never trips it and grows this string
    // until something dies. Nothing is needed to reach this point.
    if BufReader::new((&stream).take(MAX_REQUEST_LINE)).read_line(&mut head).is_err() {
        return;
    }
    let Some(target) = head.split_whitespace().nth(1) else { return };

    // Everything is behind the token. A share with a guessable path is a share of
    // whatever the machine is playing to whoever scans the tunnel.
    let Some(rest) = target.strip_prefix(&format!("/r/{token}")) else {
        respond(&mut stream, "404 Not Found", "text/plain", b"no");
        return;
    };
    let snapshot = state.lock().map(|s| s.clone()).unwrap_or_default();
    match rest {
        "" | "/" => {
            let page = landing_page(&snapshot, token);
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", page.as_bytes());
        }
        "/state" => {
            let track = snapshot.now_playing();
            let json = serde_json::json!({
                "title": track.map(|t| t.title.clone()),
                "artist": track.map(|t| t.artist.clone()),
                "album": track.map(|t| t.album.clone()),
                "quality": track.map(|t| t.quality.clone()),
                "position": snapshot.position_secs,
                "duration": track.map(|t| t.duration_secs),
                "playing": snapshot.playing && !snapshot.paused,
            });
            respond(&mut stream, "200 OK", "application/json", json.to_string().as_bytes());
        }
        "/stream" => {
            // Each listener costs a thread and a fresh download of the track from
            // TIDAL. Unbounded, that is both a memory problem and a way to hammer the
            // account of whoever shared the link.
            if listeners.load(Ordering::Relaxed) >= MAX_LISTENERS {
                respond(&mut stream, "503 Service Unavailable", "text/plain", b"too many listeners");
                return;
            }
            listeners.fetch_add(1, Ordering::Relaxed);
            stream_track(&mut stream, &snapshot);
            listeners.fetch_sub(1, Ordering::Relaxed);
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"no"),
    }
}

/// Streams the track that is playing right now, from its beginning.
///
/// From the beginning and not from the current second: the parts are fetched afresh for
/// the listener, and seeking into a stream nobody has buffered would mean guessing which
/// segment matches which moment. Hearing the same song from the top is honest and
/// obvious; pretending to be in sync when we are not would be neither.
fn stream_track(stream: &mut TcpStream, snapshot: &Snapshot) {
    let Some(track) = snapshot.now_playing() else {
        respond(stream, "409 Conflict", "text/plain", b"nothing is playing");
        return;
    };
    let Some(session) = tidal::Session::load() else {
        respond(stream, "503 Service Unavailable", "text/plain", b"not signed in");
        return;
    };
    // The tier the listener gets is the tier that is playing, because the badge on the
    // page says so and it must not be a different claim from what is sent.
    let quality = if track.quality.is_empty() { "LOSSLESS" } else { track.quality.as_str() };
    let info = match tidal::stream_info(&session, track.id, quality) {
        Ok(i) => i,
        Err(e) => {
            // The reason goes to the terminal, not to the listener: an upstream error
            // carries the verbatim API response, which is account detail a stranger
            // holding a link has no business reading.
            eprintln!("runnir: share could not resolve a stream: {e}");
            respond(stream, "502 Bad Gateway", "text/plain", b"cannot reach the stream");
            return;
        }
    };
    let parts = match crate::player::parts_of(&info) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("runnir: share could not read a manifest: {e}");
            respond(stream, "502 Bad Gateway", "text/plain", b"cannot read the stream");
            return;
        }
    };
    let kind = if info.mime.contains("dash") { "audio/mp4" } else { "audio/flac" };
    // Chunked would be more correct, but a browser playing a stream of unknown length
    // is happier with a plain close-delimited body, and that is what this is.
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\n\
         Connection: close\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    for part in parts {
        let bytes = match &part {
            crate::player::Part::Url(url) => match tidal::fetch(url, 64 * 1024 * 1024) {
                Ok(b) => b,
                Err(_) => return,
            },
            crate::player::Part::File(path) => match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => return,
            },
        };
        // A listener who closed the tab ends the loop here, which is the only signal
        // there is and the only one needed.
        if stream.write_all(&bytes).is_err() {
            return;
        }
    }
    let _ = stream.flush();
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// The page a listener lands on: what is playing, and a way to hear it.
fn landing_page(snapshot: &Snapshot, token: &str) -> String {
    let (title, artist, quality) = match snapshot.now_playing() {
        Some(t) => (t.title.clone(), t.artist.clone(), t.quality.clone()),
        None => ("Nothing playing".into(), String::new(), String::new()),
    };
    let tier = match quality.as_str() {
        "HI_RES_LOSSLESS" | "HI_RES" => "hi-res lossless",
        "LOSSLESS" => "lossless",
        "HIGH" => "high",
        _ => "",
    };
    // Escaped. These come from a catalogue rather than from the listener, but the page
    // carries the token in its own URL and the token IS the authentication — so a title
    // that could run script would be a permanent grant of the stream to whoever
    // collected it. One line, and the whole question goes away.
    let (title, artist) = (escape(&title), escape(&artist));
    format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <style>body{{font:16px/1.5 system-ui;margin:0;display:grid;place-items:center;\
         min-height:100vh;background:#12131a;color:#e6e8ee}}\
         main{{text-align:center;padding:2rem}}h1{{font-size:1.4rem;margin:.2rem 0}}\
         p{{color:#8a8d94;margin:.2rem 0}}audio{{margin-top:1.5rem;width:min(90vw,28rem)}}\
         .tier{{font-size:.8rem;letter-spacing:.08em;text-transform:uppercase;color:#7aa2f7}}\
         </style><main><h1>{title}</h1><p>{artist}</p><p class=tier>{tier}</p>\
         <audio controls autoplay src=\"/r/{token}/stream\"></audio>\
         <p style=\"margin-top:2rem;font-size:.8rem\">shared from a runnir terminal</p></main>"
    )
}

/// The five characters that turn text into markup.
fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Spawns `cloudflared` and waits for it to publish a URL.
fn open_tunnel() -> Result<(std::process::Child, String), String> {
    let mut child = std::process::Command::new("cloudflared")
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://localhost:{PORT}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "cloudflared is not installed".to_string()
            } else {
                format!("cannot start cloudflared: {e}")
            }
        })?;

    let stderr = child.stderr.take().ok_or("cloudflared has no stderr")?;
    let (tx, rx) = std::sync::mpsc::channel();
    // cloudflared keeps logging for the whole life of the tunnel. If we stopped reading
    // after finding the URL its pipe would fill within seconds and it would block or
    // die — which shows up as a dead link rather than as an error here.
    std::thread::Builder::new()
        .name("runnir-tunnel".into())
        .spawn(move || {
            let mut tx = Some(tx);
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if let Some(url) = find_tunnel_url(&line) {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(url);
                    }
                }
            }
        })
        .ok();
    // stdout is drained for the same reason, even though quick tunnels rarely use it.
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = BufReader::new(stdout).read_to_end(&mut sink);
        });
    }

    match rx.recv_timeout(TUNNEL_TIMEOUT) {
        Ok(url) => Ok((child, url)),
        Err(_) => {
            let _ = child.kill();
            Err(format!(
                "cloudflared published no URL in {}s",
                TUNNEL_TIMEOUT.as_secs()
            ))
        }
    }
}

/// Waits for the public URL to start answering.
///
/// Best effort: if it never does, the link is still handed over — it may be routing a
/// moment later, and refusing to give somebody a URL that already exists would be
/// worse than giving one that needs another second.
fn wait_until_live(url: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        let answered = ureq::get(url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(5)))
            .http_status_as_error(false)
            .build()
            .call()
            .is_ok();
        if answered {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
    }
}

/// Finds the `https://….trycloudflare.com` in a line of cloudflared's chatter.
pub fn find_tunnel_url(line: &str) -> Option<String> {
    let at = line.find("https://")?;
    let rest = &line[at..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|' || c == '"')
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches('/');
    url.ends_with(".trycloudflare.com").then(|| url.to_string())
}

/// A token nobody can guess. The link IS the authentication, so this is the only thing
/// standing between a tunnel and whatever the machine is playing.
fn random_token() -> Result<String, String> {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf))
        .map_err(|e| format!("no secure randomness available: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

#[test]
    fn a_title_cannot_carry_markup_onto_the_page() {
        // The page holds the token in its own URL, and the token is the only
        // authentication there is — so anything that could run script on this page is a
        // permanent grant of the stream to whoever collects it.
        let snapshot = Snapshot {
            queue: vec![crate::tidal::Track {
                title: r#"<script>fetch('/'+location)</script>"#.into(),
                artist: r#"a" onload="x"#.into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let page = landing_page(&snapshot, "tok");
        assert!(!page.contains("<script>"), "{page}");
        assert!(page.contains("&lt;script&gt;"));
        assert!(!page.contains(r#"a" onload="#), "an attribute break got through");
    }

    #[test]
    fn escaping_leaves_ordinary_titles_alone() {
        // Including the ones this catalogue is full of.
        assert_eq!(escape("Rodrigo: Concierto de Aranjuez"), "Rodrigo: Concierto de Aranjuez");
        assert_eq!(escape("Alfadhirhaiti"), "Alfadhirhaiti");
        assert_eq!(escape("Simon & Garfunkel"), "Simon &amp; Garfunkel");
    }

    #[test]
    fn the_tunnel_url_is_found_in_cloudflareds_chatter() {
        let line = "2026-08-01T12:00:00Z INF |  https://kind-words-appear.trycloudflare.com  |";
        assert_eq!(
            find_tunnel_url(line).as_deref(),
            Some("https://kind-words-appear.trycloudflare.com")
        );
        // Its own documentation URLs must not be mistaken for the tunnel.
        assert_eq!(find_tunnel_url("see https://developers.cloudflare.com/args"), None);
        assert_eq!(find_tunnel_url("INF Starting tunnel"), None);
    }

    #[test]
    fn a_token_is_long_and_never_the_same_twice() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        // 16 bytes as hex. The link is the only authentication there is.
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn the_page_says_what_is_playing_and_asks_for_the_stream_behind_the_token() {
        let snapshot = Snapshot {
            queue: vec![crate::tidal::Track {
                title: "Dreams".into(),
                artist: "Fleetwood Mac".into(),
                quality: "HI_RES_LOSSLESS".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let page = landing_page(&snapshot, "abc123");
        assert!(page.contains("Dreams"));
        assert!(page.contains("Fleetwood Mac"));
        assert!(page.contains("hi-res lossless"));
        // The audio element must go through the token, or the page would be the only
        // thing protected and the sound would not be.
        assert!(page.contains("/r/abc123/stream"), "{page}");
    }

    #[test]
    fn nothing_playing_still_renders_a_page_rather_than_failing() {
        let page = landing_page(&Snapshot::default(), "t");
        assert!(page.contains("Nothing playing"));
    }
}
