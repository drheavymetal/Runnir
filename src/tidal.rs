//! TIDAL: signing in, the catalogue calls, and the two shapes a stream arrives as.
//!
//! Pure request/response. Nothing here knows about the grid, the overlay or the audio
//! device — same relationship `docker.rs` and `git.rs` have to their panels. Every call
//! blocks, so every caller is a worker thread that answers through the event proxy.
//!
//! Authentication is the OAuth **device flow**: runnir asks for a code, the person
//! approves it at `link.tidal.com`, and we poll until it is granted. That flow exists
//! for televisions and other keyboards-are-painful devices, which is exactly what a
//! terminal is for this purpose — no browser redirect, no local callback server.
//!
//! Credentials are NEVER compiled in. This repository is public; they come from
//! `[tidal]` in the config (or the environment) and the panel does not exist without
//! them.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2";
const API_URL: &str = "https://api.tidal.com/v1";

/// The scope every TIDAL client asks for. `w_sub` is what makes the subscription's
/// quality tiers visible; without it a valid token still only reaches previews.
const SCOPE: &str = "r_usr w_usr w_sub";

/// Network timeout for a single call. Generous because `playbackinfopostpaywall` can
/// take a moment, short enough that a dead network does not hang a worker forever.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Refresh this long before the token actually expires. A token that dies mid-request
/// costs a retry and an error the user sees; a minute of slack costs nothing.
const REFRESH_MARGIN: u64 = 60;

/// What the config supplies. Both halves are required — TIDAL's device flow rejects a
/// client id it does not recognise, and the secret is what proves the client is the one
/// the id belongs to.
#[derive(Clone, Debug, Default)]
pub struct Creds {
    pub client_id: String,
    pub client_secret: String,
}

impl Creds {
    pub fn is_empty(&self) -> bool {
        self.client_id.is_empty() || self.client_secret.is_empty()
    }
}

/// A signed-in session, as persisted between runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds. Absolute rather than the `expires_in` TIDAL returns, because the
    /// relative form stops meaning anything the moment it is written to disk.
    pub expires_at: u64,
    /// TIDAL prices and licenses per country, and every catalogue call carries it.
    pub country_code: String,
    pub user_id: Option<u64>,
}

impl Session {
    fn expired(&self) -> bool {
        now() + REFRESH_MARGIN >= self.expires_at
    }

    /// Where the session lives: `dirs::data_dir()/runnir/tidal-session.json`, 0600.
    ///
    /// `data_dir` and not `config_dir` because that is where `session.rs` already
    /// writes. The two directories disagreeing is known debt in this program; adding a
    /// third opinion would make it worse.
    pub fn path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("runnir").join("tidal-session.json"))
    }

    pub fn load() -> Option<Session> {
        let path = Self::path()?;
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes the session with owner-only permissions. The mode is set BEFORE the
    /// tokens are written, not after: a file that is briefly world-readable while it
    /// holds a refresh token is the same leak as one that always was.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no data directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_private(&path, json.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Signs out. Wired to the panel in phase 1; kept beside `save` so the pair that
    /// writes and removes the token file stays in one place.
    #[allow(dead_code)]
    pub fn forget() -> Result<(), String> {
        let Some(path) = Self::path() else { return Ok(()) };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// What the user has to do, and what we poll with while they do it.
#[derive(Clone, Debug)]
pub struct DeviceAuth {
    pub device_code: String,
    /// The six characters to type. Shown large, because it is read off one screen and
    /// typed into another.
    pub user_code: String,
    /// The URL with the code already in it — worth offering as a hint, since the
    /// terminal can make it clickable.
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// Step one: ask TIDAL for a code.
pub fn start_device_auth(creds: &Creds) -> Result<DeviceAuth, String> {
    if creds.is_empty() {
        return Err("no TIDAL credentials in [tidal]".into());
    }
    let body = post_form(
        &format!("{AUTH_URL}/device_authorization"),
        &[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("scope", SCOPE),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    let device_code = v["deviceCode"].as_str().unwrap_or_default().to_string();
    let user_code = v["userCode"].as_str().unwrap_or_default().to_string();
    if device_code.is_empty() || user_code.is_empty() {
        return Err(format!("no device code in response: {body}"));
    }
    // TIDAL returns the verification URI without a scheme ("link.tidal.com/ABCDE").
    let uri = v["verificationUriComplete"]
        .as_str()
        .or_else(|| v["verificationUri"].as_str())
        .unwrap_or("link.tidal.com")
        .to_string();
    let verification_uri = if uri.starts_with("http") { uri } else { format!("https://{uri}") };
    Ok(DeviceAuth {
        device_code,
        user_code,
        verification_uri,
        // Both fields are advisory; the defaults are TIDAL's own documented ones and
        // keep the poll loop sane if a field ever goes missing.
        interval: v["interval"].as_u64().unwrap_or(2).max(1),
        expires_in: v["expiresIn"].as_u64().unwrap_or(300),
    })
}

/// True when TIDAL refused the device flow because this client is not registered for
/// it. Not a failure to report as one: it means "sign in the other way", and the other
/// way ([`start_pkce`]) works with exactly the same credentials.
pub fn is_not_a_device_client(err: &str) -> bool {
    err.contains("Limited Input Device") || err.contains("sub_status\":1002")
}

/// PKCE sign-in, for clients that are not registered as limited-input devices — which
/// is most of them, including every web player client id.
///
/// The browser does the login and TIDAL redirects to a URL that carries the grant code
/// in its query string. That redirect target belongs to TIDAL's own apps and shows
/// nothing useful, which is fine: the code is in the address bar, and pasting the whole
/// URL back is the terminal's version of catching a callback. No local web server, no
/// port to keep free, nothing listening.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pkce {
    pub authorize_url: String,
    pub verifier: String,
    pub client_unique_key: String,
    /// The exchange must present the SAME redirect the authorize call named, even
    /// though nothing is redirected a second time. Storing it removes the chance of the
    /// two drifting apart when one of them is a port number chosen at runtime.
    pub redirect: String,
}

impl Pkce {
    /// The sign-in spans two commands — one to get the URL, one to hand back what the
    /// browser showed — so the verifier has to outlive the first process. It is a
    /// secret for those few minutes, hence 0600 and hence deleted the moment it is
    /// spent: a leftover verifier is a live half of an exchange lying on disk.
    fn path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("runnir").join("tidal-pkce.json"))
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no data directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        write_private(&path, json.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load() -> Option<Pkce> {
        let text = std::fs::read_to_string(Self::path()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn clear() {
        if let Some(path) = Self::path() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Where TIDAL sends the browser after a successful login.
///
/// A redirect URI is registered against the client id, so this is not freely ours to
/// choose — but a loopback address is the one form an OAuth client is normally allowed
/// to name for itself, and it is the only one that lets a local program CATCH the
/// answer instead of asking someone to copy it out of an address bar.
pub fn loopback_redirect(port: u16) -> String {
    format!("http://localhost:{port}/callback")
}

/// The redirect TIDAL's own mobile client uses. Kept as the fallback for when the
/// loopback one is refused: the login still works, but the code has to be pasted back
/// because it lands on a page belonging to TIDAL.
pub const APP_REDIRECT: &str = "https://tidal.com/android/login/auth";

pub fn start_pkce(creds: &Creds, redirect: &str) -> Result<Pkce, String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>()?);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    // TIDAL ties the grant to a per-installation key, and the exchange must present the
    // same one the authorize call did.
    let client_unique_key =
        random_bytes::<8>()?.iter().map(|b| format!("{b:02x}")).collect::<String>();

    let authorize_url = format!(
        "https://login.tidal.com/authorize?response_type=code\
         &redirect_uri={encoded}&client_id={client}&lang=EN&appMode=android\
         &client_unique_key={key}&code_challenge={challenge}\
         &code_challenge_method=S256&restrict_signup=true",
        encoded = urlencode(redirect),
        client = creds.client_id,
        key = client_unique_key,
    );
    Ok(Pkce {
        authorize_url,
        verifier,
        client_unique_key,
        redirect: redirect.to_string(),
    })
}

/// Trades the code from the redirect URL for a session.
pub fn finish_pkce(creds: &Creds, pkce: &Pkce, code: &str) -> Result<Session, String> {
    let body = post_form(
        &format!("{AUTH_URL}/token"),
        &[
            ("code", code),
            ("client_id", creds.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", pkce.redirect.as_str()),
            ("scope", SCOPE),
            ("code_verifier", pkce.verifier.as_str()),
            ("client_unique_key", pkce.client_unique_key.as_str()),
        ],
    )?;
    let mut session = session_from_token_response(&body, None)?;
    fill_session_details(&mut session)?;
    Ok(session)
}

/// A session lifted from somewhere that is already signed in — the TIDAL web player's
/// own storage, most usefully.
///
/// This exists because the sign-in flows are gated by what a client id is REGISTERED
/// for, and a web player id is registered for neither the device flow (`sub_status
/// 1002`) nor an app's redirect URI (`error 11102`). A refresh token has no such gate:
/// it is the grant itself. Whoever is already signed in somewhere can hand that over
/// and skip the dance entirely.
pub struct Imported {
    pub client_id: Option<String>,
    pub refresh_token: String,
}

/// Finds a client id and a refresh token in pasted text, whatever shape it came in.
///
/// People paste a JSON blob out of devtools, or a form body, or a curl command they
/// copied. Rather than demand one of those, this looks for the field names in any of
/// them — the alternative is an error message about formatting for someone who is
/// holding the right data.
pub fn parse_import(text: &str) -> Option<Imported> {
    let refresh_token = find_field(text, &["refresh_token", "refreshToken"])?;
    Some(Imported { client_id: find_field(text, &["client_id", "clientId"]), refresh_token })
}

/// Looks for `"name": "value"`, `name=value` or `name": value` — the three ways these
/// fields appear in the wild.
fn find_field(text: &str, names: &[&str]) -> Option<String> {
    for name in names {
        for pattern in [format!("\"{name}\""), format!("{name}=")] {
            let Some(at) = text.find(&pattern) else { continue };
            let rest = &text[at + pattern.len()..];
            let value: String = rest
                .trim_start_matches([':', '=', ' ', '"', '\''])
                .chars()
                .take_while(|c| !matches!(c, '"' | '\'' | ',' | '&' | '}' | '\n' | ' '))
                .collect();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Turns an imported refresh token into a working session.
pub fn adopt(creds: &Creds, imported: &Imported) -> Result<Session, String> {
    let stub = Session {
        refresh_token: imported.refresh_token.clone(),
        ..Default::default()
    };
    let mut session = refresh(creds, &stub)?;
    fill_session_details(&mut session)?;
    Ok(session)
}

/// Waits for the browser to come back with the grant code.
///
/// A real callback rather than a copied address bar: TIDAL redirects to
/// `http://localhost:<port>/callback?code=…`, this answers that one request with a page
/// saying it worked, and hands the code back.
///
/// Written on a bare `TcpListener` — twenty lines of HTTP for one request that arrives
/// once — rather than by taking on a web framework and an async runtime for it.
pub fn wait_for_callback(port: u16, timeout: Duration) -> Result<String, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("cannot set the listener non-blocking: {e}"))?;

    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => match handle_callback(stream) {
                // A browser asks for /favicon.ico too, and Chrome sometimes probes the
                // port before following the redirect. Anything without a code is
                // answered and ignored rather than taken as the answer.
                Some(code) => return Ok(code),
                None => continue,
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("callback listener failed: {e}")),
        }
    }
    Err("the browser did not come back in time".into())
}

fn handle_callback(mut stream: std::net::TcpStream) -> Option<String> {
    use std::io::Write;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let read = stream.read(&mut buf).ok()?;
    let request = String::from_utf8_lossy(&buf[..read]);
    // "GET /callback?code=abc&state=x HTTP/1.1"
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?;
    let code = code_from_redirect(target).filter(|c| !c.is_empty());

    let (status, body) = match &code {
        Some(_) => ("200 OK", CALLBACK_OK),
        None if target.starts_with("/callback") => ("400 Bad Request", CALLBACK_FAILED),
        None => ("404 Not Found", "not here"),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    code
}

const CALLBACK_OK: &str = "<!doctype html><meta charset=utf-8><title>runnir</title>\
<body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;margin:0\">\
<div><h1 style=\"font-weight:600\">Signed in</h1>\
<p>runnir has the session. You can close this tab.</p></div>";

const CALLBACK_FAILED: &str = "<!doctype html><meta charset=utf-8><title>runnir</title>\
<body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;margin:0\">\
<div><h1 style=\"font-weight:600\">No grant code</h1>\
<p>TIDAL came back without one. The terminal has the details.</p></div>";

/// Pulls the `code` parameter out of whatever the browser ended up showing. Accepts the
/// whole pasted URL or a bare code, because both are what people actually paste.
pub fn code_from_redirect(pasted: &str) -> Option<String> {
    let pasted = pasted.trim();
    if pasted.is_empty() {
        return None;
    }
    if !pasted.contains("://")
        && !pasted.contains('?')
        && !pasted.contains('&')
        && !pasted.starts_with('/')
    {
        return Some(pasted.to_string());
    }
    let query = pasted.split_once('?').map(|(_, q)| q).unwrap_or(pasted);
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "code").then(|| urldecode(v))
    })
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
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

/// Cryptographic randomness without a crate for it: the kernel's own pool. A PKCE
/// verifier guessed by an attacker defeats the whole exchange, so this must never fall
/// back to anything weaker — a machine with no `/dev/urandom` gets an error, not a
/// predictable secret.
fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut buf = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| format!("no secure randomness available: {e}"))?;
    Ok(buf)
}

/// The answer to one poll. `Pending` is the normal case for as long as the person is
/// still typing the code, and must not be treated as an error.
pub enum Poll {
    Pending,
    Granted(Box<Session>),
}

/// Step two, called on a timer until it grants or the code expires.
pub fn poll_device_token(creds: &Creds, device_code: &str) -> Result<Poll, String> {
    let (status, body) = post_form_raw(
        &format!("{AUTH_URL}/token"),
        &[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("scope", SCOPE),
        ],
    )?;
    // 400 carries two meanings here. `authorization_pending` and `slow_down` are the
    // flow working as designed; anything else at 400 is a real failure.
    if status == 400 && (body.contains("authorization_pending") || body.contains("slow_down")) {
        return Ok(Poll::Pending);
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {body}"));
    }
    let mut session = session_from_token_response(&body, None)?;
    fill_session_details(&mut session)?;
    Ok(Poll::Granted(Box::new(session)))
}

/// Trades the refresh token for a new access token.
pub fn refresh(creds: &Creds, session: &Session) -> Result<Session, String> {
    let body = post_form(
        &format!("{AUTH_URL}/token"),
        &[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", session.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
            ("scope", SCOPE),
        ],
    )?;
    // A refresh response usually omits `refresh_token`: the old one stays valid, and
    // dropping it because the field was absent would sign the user out on the next run.
    let mut next = session_from_token_response(&body, Some(session))?;
    if next.country_code.is_empty() {
        next.country_code = session.country_code.clone();
    }
    Ok(next)
}

/// Returns a session good for the next call, refreshing it first if it is close to
/// expiry. The refreshed session is written back to disk here so a caller cannot
/// forget to and leave the stored copy stale.
pub fn ensure_fresh(creds: &Creds, session: &Session) -> Result<Session, String> {
    if !session.expired() {
        return Ok(session.clone());
    }
    let next = refresh(creds, session)?;
    next.save()?;
    Ok(next)
}

fn session_from_token_response(body: &str, prev: Option<&Session>) -> Result<Session, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("{e}: {body}"))?;
    let access_token = v["access_token"].as_str().unwrap_or_default().to_string();
    if access_token.is_empty() {
        return Err(format!("no access token in response: {body}"));
    }
    let refresh_token = v["refresh_token"]
        .as_str()
        .map(str::to_string)
        .or_else(|| prev.map(|p| p.refresh_token.clone()))
        .unwrap_or_default();
    let expires_in = v["expires_in"].as_u64().unwrap_or(3600);
    Ok(Session {
        access_token,
        refresh_token,
        expires_at: now() + expires_in,
        country_code: v["user"]["countryCode"].as_str().unwrap_or_default().to_string(),
        user_id: v["user"]["userId"].as_u64().or_else(|| prev.and_then(|p| p.user_id)),
    })
}

/// Fills in country and user id from `/sessions` when the token response did not carry
/// them. Every catalogue call needs the country code, so a session without one is not
/// actually usable and it is better to fail here, at sign-in, than at the first search.
fn fill_session_details(session: &mut Session) -> Result<(), String> {
    if !session.country_code.is_empty() && session.user_id.is_some() {
        return Ok(());
    }
    let body = get_body(&format!("{API_URL}/sessions"), session, &[])?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    if session.country_code.is_empty() {
        session.country_code = v["countryCode"].as_str().unwrap_or("US").to_string();
    }
    if session.user_id.is_none() {
        session.user_id = v["userId"].as_u64();
    }
    Ok(())
}

/// One track, as much of it as the panel and the badge need.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u32,
    /// TIDAL's own words for the tier: `HI_RES_LOSSLESS`, `LOSSLESS`, `HIGH`, `LOW`.
    pub quality: String,
}

pub fn parse_track(v: &serde_json::Value) -> Track {
    Track {
        id: v["id"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        // `artists[0]` is the credited main artist; `artist` is a legacy field that is
        // missing on plenty of newer entries, so it is the fallback and not the source.
        artist: v["artists"][0]["name"]
            .as_str()
            .or_else(|| v["artist"]["name"].as_str())
            .unwrap_or_default()
            .to_string(),
        album: v["album"]["title"].as_str().unwrap_or_default().to_string(),
        duration_secs: v["duration"].as_u64().unwrap_or(0) as u32,
        quality: v["audioQuality"].as_str().unwrap_or_default().to_string(),
    }
}

pub fn track(session: &Session, id: u64) -> Result<Track, String> {
    let body = get_body(
        &format!("{API_URL}/tracks/{id}"),
        session,
        &[("countryCode", session.country_code.as_str())],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(parse_track(&v))
}

pub fn search_tracks(session: &Session, query: &str, limit: u32) -> Result<Vec<Track>, String> {
    let limit = limit.to_string();
    let body = get_body(
        &format!("{API_URL}/search/tracks"),
        session,
        &[
            ("countryCode", session.country_code.as_str()),
            ("query", query),
            ("limit", limit.as_str()),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(v["items"].as_array().map(|a| a.iter().map(parse_track).collect()).unwrap_or_default())
}

// ---- the catalogue --------------------------------------------------------

/// An album, as much of one as a list needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Album {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub tracks: u32,
    pub year: Option<u32>,
    /// TIDAL's word for the best tier it holds. Worth showing BEFORE playing: an album
    /// listed as HI_RES_LOSSLESS is the reason to pick it over another master.
    pub quality: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Artist {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playlist {
    /// Playlists are keyed by uuid, not by a number like everything else.
    pub uuid: String,
    pub title: String,
    pub tracks: u32,
    /// Who made it: `mine` for the user's own, else the curator TIDAL names.
    pub owner: String,
    pub mine: bool,
}

/// Everything one search turned up.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

impl Found {
#[allow(dead_code)] // wired to the panel next
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
    }
}

pub fn parse_album(v: &serde_json::Value) -> Album {
    Album {
        id: v["id"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        artist: v["artists"][0]["name"]
            .as_str()
            .or_else(|| v["artist"]["name"].as_str())
            .unwrap_or_default()
            .to_string(),
        tracks: v["numberOfTracks"].as_u64().unwrap_or(0) as u32,
        // "2001-05-21" — only the year is worth the width in a list.
        year: v["releaseDate"]
            .as_str()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok()),
        quality: v["audioQuality"].as_str().unwrap_or_default().to_string(),
    }
}

pub fn parse_artist(v: &serde_json::Value) -> Artist {
    Artist {
        id: v["id"].as_u64().unwrap_or(0),
        name: v["name"].as_str().unwrap_or_default().to_string(),
    }
}

pub fn parse_playlist(v: &serde_json::Value, user_id: Option<u64>) -> Playlist {
    let owner_id = v["creator"]["id"].as_u64();
    Playlist {
        uuid: v["uuid"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        tracks: v["numberOfTracks"].as_u64().unwrap_or(0) as u32,
        owner: v["creator"]["name"]
            .as_str()
            .or_else(|| v["promotedArtists"][0]["name"].as_str())
            .unwrap_or("TIDAL")
            .to_string(),
        // A playlist with no creator id is one of TIDAL's own editorial ones.
        mine: matches!((owner_id, user_id), (Some(a), Some(b)) if a == b),
    }
}

/// One search across everything TIDAL indexes.
///
/// Four types in one request rather than four requests: the panel shows them together,
/// and a person typing does not want the artists to arrive a second after the tracks.
pub fn search(session: &Session, query: &str, limit: u32) -> Result<Found, String> {
    let limit = limit.to_string();
    let body = get_body(
        &format!("{API_URL}/search"),
        session,
        &[
            ("query", query),
            ("countryCode", session.country_code.as_str()),
            ("limit", limit.as_str()),
            ("offset", "0"),
            ("types", "ARTISTS,ALBUMS,TRACKS,PLAYLISTS"),
            ("includeUserPlaylists", "true"),
            ("supportsUserData", "true"),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    let items = |key: &str| -> Vec<serde_json::Value> {
        v[key]["items"].as_array().cloned().unwrap_or_default()
    };
    Ok(Found {
        tracks: items("tracks").iter().map(parse_track).collect(),
        albums: items("albums").iter().map(parse_album).collect(),
        artists: items("artists").iter().map(parse_artist).collect(),
        playlists: items("playlists")
            .iter()
            .map(|p| parse_playlist(p, session.user_id))
            .collect(),
    })
}

/// The tracks of an album, in album order.
pub fn album_tracks(session: &Session, album_id: u64) -> Result<Vec<Track>, String> {
    items_of(session, &format!("{API_URL}/albums/{album_id}/tracks"), &[])
}

/// What an artist is known for. TIDAL orders these by popularity, which is the right
/// order for a list somebody is about to press play on.
pub fn artist_top_tracks(session: &Session, artist_id: u64) -> Result<Vec<Track>, String> {
    items_of(session, &format!("{API_URL}/artists/{artist_id}/toptracks"), &[("limit", "50")])
}

#[allow(dead_code)] // wired to the panel next
pub fn playlist_tracks(session: &Session, uuid: &str) -> Result<Vec<Track>, String> {
    items_of(session, &format!("{API_URL}/playlists/{uuid}/tracks"), &[("limit", "100")])
}

/// The user's own playlists.
pub fn my_playlists(session: &Session) -> Result<Vec<Playlist>, String> {
    let user = session.user_id.ok_or("no user id in the session")?;
    let body = get_body(
        &format!("{API_URL}/users/{user}/playlists"),
        session,
        &[("countryCode", session.country_code.as_str()), ("limit", "100")],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(v["items"]
        .as_array()
        .map(|a| a.iter().map(|p| parse_playlist(p, session.user_id)).collect())
        .unwrap_or_default())
}

/// Favourites. TIDAL wraps each one in an envelope with the date it was added, so the
/// thing itself is one level down — a detail worth handling here rather than in four
/// callers.
pub fn favourite_tracks(session: &Session) -> Result<Vec<Track>, String> {
    let user = session.user_id.ok_or("no user id in the session")?;
    let body = get_body(
        &format!("{API_URL}/users/{user}/favorites/tracks"),
        session,
        &[
            ("countryCode", session.country_code.as_str()),
            ("limit", "100"),
            ("order", "DATE"),
            ("orderDirection", "DESC"),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(v["items"]
        .as_array()
        .map(|a| a.iter().map(|i| parse_track(&i["item"])).collect())
        .unwrap_or_default())
}

#[allow(dead_code)] // wired to the panel next
pub fn favourite_albums(session: &Session) -> Result<Vec<Album>, String> {
    let user = session.user_id.ok_or("no user id in the session")?;
    let body = get_body(
        &format!("{API_URL}/users/{user}/favorites/albums"),
        session,
        &[
            ("countryCode", session.country_code.as_str()),
            ("limit", "100"),
            ("order", "DATE"),
            ("orderDirection", "DESC"),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(v["items"]
        .as_array()
        .map(|a| a.iter().map(|i| parse_album(&i["item"])).collect())
        .unwrap_or_default())
}

fn items_of(session: &Session, url: &str, extra: &[(&str, &str)]) -> Result<Vec<Track>, String> {
    let mut query = vec![("countryCode", session.country_code.as_str())];
    query.extend_from_slice(extra);
    let body = get_body(url, session, &query)?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    // Some endpoints answer with a bare array, others with `{ items: [...] }`.
    let list = v["items"].as_array().or_else(|| v.as_array());
    Ok(list.map(|a| a.iter().map(parse_track).collect()).unwrap_or_default())
}

/// Words, and when to show them.
#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    /// The whole thing as plain text, which is what most tracks have.
    pub plain: String,
    /// One line per moment, in seconds. Empty when TIDAL has no timed version — and
    /// that is the difference between lyrics you read and lyrics that follow the song.
    pub timed: Vec<(f64, String)>,
}

impl Lyrics {
#[allow(dead_code)] // wired to the panel next
    pub fn is_empty(&self) -> bool {
        self.plain.is_empty() && self.timed.is_empty()
    }

    /// Which line is current at `secs`, as an index into `timed`.
#[allow(dead_code)] // wired to the panel next
    pub fn line_at(&self, secs: f64) -> Option<usize> {
        if self.timed.is_empty() {
            return None;
        }
        // The last line whose moment has passed. `partition_point` rather than a scan:
        // this runs on every frame while lyrics are open.
        let after = self.timed.partition_point(|(at, _)| *at <= secs);
        after.checked_sub(1)
    }
}

pub fn lyrics(session: &Session, track_id: u64) -> Result<Lyrics, String> {
    let body = get_body(
        &format!("{API_URL}/tracks/{track_id}/lyrics"),
        session,
        &[("countryCode", session.country_code.as_str())],
    )?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))?;
    Ok(Lyrics {
        plain: v["lyrics"].as_str().unwrap_or_default().to_string(),
        timed: parse_lrc(v["subtitles"].as_str().unwrap_or_default()),
    })
}

/// Parses the LRC subtitles TIDAL returns: `[mm:ss.cc] the words`.
///
/// Lines without a stamp are dropped rather than guessed at — a lyric shown at the
/// wrong moment is worse than one not shown.
fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((stamp, words)) = rest.split_once(']') else { continue };
        let Some((mins, secs)) = stamp.split_once(':') else { continue };
        let (Ok(m), Ok(s)) = (mins.trim().parse::<f64>(), secs.trim().parse::<f64>()) else {
            continue;
        };
        out.push((m * 60.0 + s, words.trim().to_string()));
    }
    // TIDAL sends them in order, but a sorted list is what `line_at` assumes and it is
    // cheap to guarantee here rather than to trust.
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Where the audio actually is. Two shapes, because TIDAL serves the two tiers
/// differently and the difference reaches all the way down to the decoder.
#[derive(Clone, Debug, PartialEq)]
pub enum Media {
    /// A whole file (or a short list of them) to fetch and hand to the decoder. This is
    /// what `vnd.tidal.bts` carries, and what LOSSLESS arrives as.
    Direct(Vec<String>),
    /// Fragmented MP4: an initialisation segment followed by N media segments that must
    /// be concatenated in order. Hi-res arrives this way.
    Dash { init: Option<String>, segments: Vec<String> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamInfo {
    pub media: Option<Media>,
    pub codec: String,
    pub mime: String,
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    /// ReplayGain, in dB, album then track. Carried even when normalisation is off:
    /// the signal-path badge reports what was AVAILABLE and what was applied, and those
    /// are different statements.
    pub album_replay_gain: Option<f32>,
    pub track_replay_gain: Option<f32>,
}

impl StreamInfo {
    pub fn media(&self) -> Result<&Media, String> {
        self.media.as_ref().ok_or_else(|| "no playable media in manifest".to_string())
    }
}

/// Asks TIDAL where a track's audio is, at the best tier the subscription allows.
pub fn stream_info(session: &Session, track_id: u64, quality: &str) -> Result<StreamInfo, String> {
    let body = get_body(
        &format!("{API_URL}/tracks/{track_id}/playbackinfopostpaywall"),
        session,
        &[
            ("countryCode", session.country_code.as_str()),
            ("audioquality", quality),
            ("playbackmode", "STREAM"),
            ("assetpresentation", "FULL"),
        ],
    )?;
    parse_playback_info(&body)
}

/// Splits the JSON envelope from the base64 manifest inside it. Kept separate from the
/// request so the parsing can be tested against captured responses.
pub fn parse_playback_info(body: &str) -> Result<StreamInfo, String> {
    use base64::Engine;
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("{e}: {body}"))?;
    let mime = v["manifestMimeType"].as_str().unwrap_or_default().to_string();
    let raw = v["manifest"].as_str().unwrap_or_default();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|e| format!("manifest is not base64: {e}"))?;
    let manifest = String::from_utf8(decoded).map_err(|e| format!("manifest is not utf-8: {e}"))?;

    let mut info = StreamInfo {
        mime: mime.clone(),
        quality: v["audioQuality"].as_str().unwrap_or_default().to_string(),
        bit_depth: v["bitDepth"].as_u64().map(|n| n as u32),
        sample_rate: v["sampleRate"].as_u64().map(|n| n as u32),
        album_replay_gain: v["albumReplayGain"].as_f64().map(|n| n as f32),
        track_replay_gain: v["trackReplayGain"].as_f64().map(|n| n as f32),
        ..Default::default()
    };

    if mime.contains("vnd.tidal.bts") || manifest.trim_start().starts_with('{') {
        let m: serde_json::Value =
            serde_json::from_str(&manifest).map_err(|e| format!("{e}: {manifest}"))?;
        let urls: Vec<String> = m["urls"]
            .as_array()
            .map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if urls.is_empty() {
            return Err("no urls in BTS manifest".into());
        }
        info.codec = m["codec"].as_str().unwrap_or_default().to_string();
        info.media = Some(Media::Direct(urls));
    } else if mime.contains("dash+xml") || manifest.trim_start().starts_with('<') {
        let dash = parse_mpd(&manifest)?;
        info.codec = dash.codec;
        info.media = Some(Media::Dash { init: dash.init, segments: dash.segments });
    } else {
        return Err(format!("unknown manifest type {mime:?}"));
    }
    Ok(info)
}

struct Mpd {
    codec: String,
    init: Option<String>,
    segments: Vec<String>,
}

/// Expands TIDAL's MPD into the exact list of URLs to fetch, in order.
///
/// Parsed by hand rather than with an XML crate. These manifests are machine-written
/// and narrow — one Period, one Representation, one SegmentTemplate — and a dependency
/// that can parse arbitrary XML buys nothing for a document whose shape is fixed. What
/// the hand parser must not do is guess: anything it does not recognise is an error,
/// never a silently short playlist, because a short playlist truncates the song.
fn parse_mpd(xml: &str) -> Result<Mpd, String> {
    let representation = tag(xml, "Representation").ok_or("no Representation in MPD")?;
    let codec = attr(representation, "codecs").unwrap_or_default().to_string();
    let rep_id = attr(representation, "id").unwrap_or_default().to_string();

    let template = tag(xml, "SegmentTemplate").ok_or("no SegmentTemplate in MPD")?;
    let media = attr(template, "media").ok_or("SegmentTemplate has no media")?.to_string();
    let init = attr(template, "initialization").map(str::to_string);
    let start_number: u64 =
        attr(template, "startNumber").and_then(|s| s.parse().ok()).unwrap_or(1);

    let count = if let Some(timeline) = between(xml, "<SegmentTimeline", "</SegmentTimeline>") {
        // Each <S> is one segment unless it carries `r`, which repeats it r more times.
        let mut n = 0u64;
        for s in timeline.split("<S ").skip(1) {
            let repeats: u64 = attr_in(s, "r").and_then(|v| v.parse().ok()).unwrap_or(0);
            n += 1 + repeats;
        }
        if n == 0 {
            return Err("SegmentTimeline has no segments".into());
        }
        n
    } else {
        // No timeline: every segment is the same length, so the count comes from the
        // period's duration. Rounded UP — a partial last segment is still a segment,
        // and rounding down would cut the end off every track.
        let timescale: f64 =
            attr(template, "timescale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let seg: f64 = attr(template, "duration")
            .and_then(|s| s.parse().ok())
            .ok_or("SegmentTemplate has neither a timeline nor a duration")?;
        let total = mpd_duration(xml).ok_or("MPD has no duration to count segments with")?;
        if seg <= 0.0 || timescale <= 0.0 {
            return Err("SegmentTemplate duration is not positive".into());
        }
        (total / (seg / timescale)).ceil() as u64
    };

    let expand = |s: &str, number: Option<u64>| {
        let out = expand_identifier(s, "RepresentationID", |width| pad(&rep_id, width));
        match number {
            Some(n) => expand_identifier(&out, "Number", |width| pad(&n.to_string(), width)),
            None => out,
        }
    };
    // A manifest may name its segments relative to a <BaseURL>. TIDAL sends absolute
    // URLs today, but a relative one silently becomes an unfetchable path rather than
    // an error, so the base is honoured when it is there.
    let base = between(xml, "<BaseURL>", "</BaseURL>")
        .and_then(|b| b.strip_prefix("<BaseURL>"))
        .map(str::trim)
        .unwrap_or("");
    let absolute = |u: String| {
        if base.is_empty() || u.contains("://") {
            u
        } else if base.ends_with('/') {
            format!("{base}{u}")
        } else {
            format!("{base}/{u}")
        }
    };

    let segments = (0..count)
        .map(|i| absolute(expand(&media, Some(start_number + i))))
        .collect::<Vec<_>>();
    Ok(Mpd { codec, init: init.map(|i| absolute(expand(&i, None))), segments })
}

/// Replaces one DASH identifier wherever it appears, with or without a width.
///
/// The spec allows `$Number$` and `$Number%05d$`, and encoders use both — ffmpeg writes
/// the padded form by default. Handling only the bare one produces URLs that 404, which
/// reads downstream as a truncated song rather than as a parsing bug, so it is worth
/// doing properly rather than with two `replace` calls.
fn expand_identifier(template: &str, name: &str, value: impl Fn(usize) -> String) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let open = format!("${name}");
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        // What follows is either `$` (bare) or a `%0Nd$` width specifier. Anything else
        // means this was a different identifier that merely starts the same way.
        let (width, consumed) = if let Some(tail) = after.strip_prefix('$') {
            let _ = tail;
            (0usize, 1usize)
        } else if let Some(spec_end) = after.find('$') {
            let spec = &after[..spec_end];
            match parse_width(spec) {
                Some(w) => (w, spec_end + 1),
                None => {
                    out.push_str(&rest[..at + open.len()]);
                    rest = &rest[at + open.len()..];
                    continue;
                }
            }
        } else {
            break;
        };
        out.push_str(&rest[..at]);
        out.push_str(&value(width));
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

/// `%05d` -> 5. Only the zero-padded integer form DASH defines.
fn parse_width(spec: &str) -> Option<usize> {
    let digits = spec.strip_prefix("%0")?.strip_suffix('d')?;
    digits.parse().ok()
}

fn pad(value: &str, width: usize) -> String {
    if value.len() >= width {
        value.to_string()
    } else {
        format!("{}{}", "0".repeat(width - value.len()), value)
    }
}

/// `mediaPresentationDuration="PT1234.5S"` — only the shapes TIDAL actually emits
/// (hours, minutes, seconds) are understood.
fn mpd_duration(xml: &str) -> Option<f64> {
    let raw = attr(tag(xml, "MPD")?, "mediaPresentationDuration")?;
    let body = raw.strip_prefix("PT")?;
    let mut total = 0.0;
    let mut number = String::new();
    for ch in body.chars() {
        match ch {
            '0'..='9' | '.' => number.push(ch),
            'H' => total += number.parse::<f64>().ok()? * 3600.0,
            'M' => total += number.parse::<f64>().ok()? * 60.0,
            'S' => total += number.parse::<f64>().ok()?,
            _ => return None,
        }
        if !matches!(ch, '0'..='9' | '.') {
            number.clear();
        }
    }
    Some(total)
}

/// The text of the first `<name ...>` element's attributes.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let start = xml.find(&open)?;
    let rest = &xml[start + open.len()..];
    // A tag ends at the first `>`; `/>` ends it too and the `/` is not an attribute.
    let end = rest.find('>')?;
    Some(&rest[..end])
}

/// An attribute of the element whose attribute text this is.
fn attr<'a>(tag_text: &'a str, name: &str) -> Option<&'a str> {
    attr_in(tag_text, name)
}

fn attr_in<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    // Matched with the `="` included so `id` cannot match inside `RepresentationID`,
    // and preceded by whitespace so `r` cannot match inside `timescale`.
    let needle = format!("{name}=\"");
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(&needle) {
        let at = from + pos;
        let preceded_ok = at == 0 || text[..at].ends_with([' ', '\t', '\n', '\r']);
        if preceded_ok {
            let value_start = at + needle.len();
            let end = text[value_start..].find('"')?;
            return Some(&text[value_start..value_start + end]);
        }
        from = at + needle.len();
    }
    None
}

fn between<'a>(hay: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = hay.find(open)?;
    let end = hay[start..].find(close)? + start;
    Some(&hay[start..end])
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<String, String> {
    let (status, body) = post_form_raw(url, form)?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {body}"));
    }
    Ok(body)
}

/// Posts a form and returns the status alongside the body — the device-code poll needs
/// to read a 400 rather than be handed an error for it.
fn post_form_raw(url: &str, form: &[(&str, &str)]) -> Result<(u16, String), String> {
    let response = ureq::post(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .send_form(form.iter().copied());
    let mut response = response.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok((status, body))
}

fn get_body(url: &str, session: &Session, query: &[(&str, &str)]) -> Result<String, String> {
    let mut req = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .header("Authorization", &format!("Bearer {}", session.access_token));
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut response = req.call().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {body}"));
    }
    Ok(body)
}

/// Fetches one URL into memory. Used for a whole FLAC or one DASH segment; both are
/// bounded by the length of a song, which is why this can be a `Vec` rather than a
/// stream. `limit` guards against a redirect to something that is not a track at all.
pub fn fetch(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD_TIMELINE: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT180.5S">
 <Period>
  <AdaptationSet mimeType="audio/mp4">
   <Representation id="1" codecs="flac" bandwidth="1411000">
    <SegmentTemplate timescale="44100" initialization="https://x/init.mp4"
                     media="https://x/seg-$Number$.mp4" startNumber="1">
     <SegmentTimeline>
      <S d="441000" r="2" />
      <S d="220500" />
     </SegmentTimeline>
    </SegmentTemplate>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;

    const MPD_FIXED: &str = r#"<MPD mediaPresentationDuration="PT10S">
 <Representation id="7" codecs="flac">
  <SegmentTemplate timescale="1000" duration="4000" startNumber="0"
                   initialization="https://x/$RepresentationID$-init.mp4"
                   media="https://x/$RepresentationID$-$Number$.mp4" />
 </Representation>
</MPD>"#;

    /// Written by ffmpeg, not by hand: it uses the padded `$Number%05d$` form and puts
    /// `$RepresentationID$` in the initialisation name, which is exactly the pair that
    /// a bare `$Number$` replacement gets wrong.
    const MPD_FFMPEG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static"
     mediaPresentationDuration="PT6.0S" maxSegmentDuration="PT2.0S">
 <Period id="0" start="PT0.0S">
  <AdaptationSet id="0" contentType="audio">
   <Representation id="0" mimeType="audio/mp4" codecs="flac" audioSamplingRate="96000">
    <SegmentTemplate timescale="96000" initialization="init-stream$RepresentationID$.m4s"
                     media="chunk-stream$RepresentationID$-$Number%05d$.m4s" startNumber="1">
     <SegmentTimeline>
      <S t="0" d="196608" r="1" />
      <S d="182784" />
     </SegmentTimeline>
    </SegmentTemplate>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;

    #[test]
    fn a_real_encoder_mpd_expands_to_the_files_that_exist() {
        let mpd = parse_mpd(MPD_FFMPEG).unwrap();
        assert_eq!(mpd.init.as_deref(), Some("init-stream0.m4s"));
        assert_eq!(
            mpd.segments,
            [
                "chunk-stream0-00001.m4s",
                "chunk-stream0-00002.m4s",
                "chunk-stream0-00003.m4s"
            ]
        );
    }

    #[test]
    fn relative_segments_are_resolved_against_the_base_url() {
        let mpd = MPD_FFMPEG.replace(
            "<Period id=\"0\" start=\"PT0.0S\">",
            "<BaseURL>https://cdn.example/audio/</BaseURL><Period id=\"0\" start=\"PT0.0S\">",
        );
        let parsed = parse_mpd(&mpd).unwrap();
        assert_eq!(parsed.init.as_deref(), Some("https://cdn.example/audio/init-stream0.m4s"));
        assert!(parsed.segments[0].starts_with("https://cdn.example/audio/chunk-"));
    }

    #[test]
    fn an_absolute_segment_is_left_alone_even_with_a_base_url() {
        let mpd = MPD_TIMELINE.replace(
            "<Period>",
            "<BaseURL>https://cdn.example/</BaseURL><Period>",
        );
        let parsed = parse_mpd(&mpd).unwrap();
        assert_eq!(parsed.segments[0], "https://x/seg-1.mp4");
    }

    #[test]
    fn a_width_specifier_pads_and_a_bare_identifier_does_not() {
        assert_eq!(expand_identifier("s-$Number%05d$.m4s", "Number", |w| pad("7", w)),
                   "s-00007.m4s");
        assert_eq!(expand_identifier("s-$Number$.m4s", "Number", |w| pad("7", w)), "s-7.m4s");
        // A number already wider than the field is not truncated.
        assert_eq!(expand_identifier("$Number%02d$", "Number", |w| pad("12345", w)), "12345");
        // An identifier that merely starts with the same letters is left alone.
        assert_eq!(
            expand_identifier("$NumberOfThings$", "Number", |w| pad("7", w)),
            "$NumberOfThings$"
        );
    }

    #[test]
    fn timeline_repeats_count_as_segments() {
        let mpd = parse_mpd(MPD_TIMELINE).unwrap();
        // Three from the repeated <S>, one from the last: r="2" means two MORE.
        assert_eq!(mpd.segments.len(), 4);
        assert_eq!(mpd.init.as_deref(), Some("https://x/init.mp4"));
        assert_eq!(mpd.segments[0], "https://x/seg-1.mp4");
        assert_eq!(mpd.segments[3], "https://x/seg-4.mp4");
        assert_eq!(mpd.codec, "flac");
    }

    #[test]
    fn a_partial_last_segment_still_counts() {
        // 10 s of audio in 4 s segments is three segments, not two and a half.
        let mpd = parse_mpd(MPD_FIXED).unwrap();
        assert_eq!(mpd.segments.len(), 3);
        assert_eq!(mpd.segments[0], "https://x/7-0.mp4");
        assert_eq!(mpd.init.as_deref(), Some("https://x/7-init.mp4"));
    }

    #[test]
    fn attributes_do_not_match_inside_other_attributes() {
        let text = r#"id="1" timescale="44100" RepresentationID="7""#;
        assert_eq!(attr_in(text, "id"), Some("1"));
        // `r` appears inside `timescale` and `RepresentationID`, and must not be found.
        assert_eq!(attr_in(text, "r"), None);
    }

    #[test]
    fn an_unparseable_mpd_is_an_error_not_an_empty_playlist() {
        let no_template = r#"<MPD><Representation id="1" codecs="flac"/></MPD>"#;
        assert!(parse_mpd(no_template).is_err());
        // A timeline with no <S> would otherwise produce a zero-segment "success".
        let empty = r#"<MPD><Representation id="1" codecs="flac"><SegmentTemplate
            media="m-$Number$" ><SegmentTimeline></SegmentTimeline></SegmentTemplate>
            </Representation></MPD>"#;
        assert!(parse_mpd(empty).is_err());
    }

    #[test]
    fn iso_durations_add_up() {
        assert_eq!(mpd_duration(r#"<MPD mediaPresentationDuration="PT180.5S">"#), Some(180.5));
        assert_eq!(mpd_duration(r#"<MPD mediaPresentationDuration="PT1H2M3S">"#), Some(3723.0));
    }

    #[test]
    fn bts_manifest_yields_direct_urls() {
        use base64::Engine;
        let manifest = r#"{"mimeType":"audio/flac","codec":"flac","urls":["https://a/1.flac"]}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(manifest);
        let body = format!(
            r#"{{"manifestMimeType":"application/vnd.tidal.bts","manifest":"{encoded}",
                "audioQuality":"LOSSLESS","bitDepth":16,"sampleRate":44100,
                "trackReplayGain":-7.2}}"#
        );
        let info = parse_playback_info(&body).unwrap();
        assert_eq!(info.media, Some(Media::Direct(vec!["https://a/1.flac".into()])));
        assert_eq!(info.bit_depth, Some(16));
        assert_eq!(info.sample_rate, Some(44100));
        assert_eq!(info.track_replay_gain, Some(-7.2));
    }

    #[test]
    fn dash_manifest_yields_segments() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(MPD_TIMELINE);
        let body = format!(
            r#"{{"manifestMimeType":"application/dash+xml","manifest":"{encoded}",
                "audioQuality":"HI_RES_LOSSLESS","bitDepth":24,"sampleRate":192000}}"#
        );
        let info = parse_playback_info(&body).unwrap();
        match info.media.unwrap() {
            Media::Dash { init, segments } => {
                assert!(init.is_some());
                assert_eq!(segments.len(), 4);
            }
            other => panic!("expected DASH, got {other:?}"),
        }
        assert_eq!(info.bit_depth, Some(24));
    }

    #[test]
    fn a_stale_session_asks_for_a_refresh_before_it_actually_expires() {
        let mut s = Session { expires_at: now() + 3600, ..Default::default() };
        assert!(!s.expired());
        // Inside the margin: still valid to TIDAL, but we refresh anyway rather than
        // race a request against the expiry.
        s.expires_at = now() + REFRESH_MARGIN - 1;
        assert!(s.expired());
    }

    #[test]
    fn the_callback_answers_the_browser_and_keeps_the_code() {
        use std::io::{Read as _, Write as _};
        // Its own listener on an ephemeral port: the test must not depend on a
        // particular port being free on whatever machine runs it.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = std::thread::spawn(move || {
            let mut sock = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            sock.write_all(b"GET /callback?code=grant-42&state=x HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut reply = String::new();
            let _ = sock.read_to_string(&mut reply);
            reply
        });
        let (stream, _) = listener.accept().unwrap();
        let code = handle_callback(stream);
        let reply = client.join().unwrap();

        assert_eq!(code.as_deref(), Some("grant-42"));
        // The browser is left on a page that says it worked — a blank tab or a
        // connection error reads as a failed sign-in even when it succeeded.
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.contains("Signed in"), "{reply}");
    }

    #[test]
    fn a_request_that_is_not_the_callback_is_answered_and_ignored() {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = std::thread::spawn(move || {
            // Browsers ask for this unprompted; taking it as the answer would end the
            // wait with no code at all.
            let mut sock = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            sock.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
            let mut reply = String::new();
            let _ = sock.read_to_string(&mut reply);
            reply
        });
        let (stream, _) = listener.accept().unwrap();
        assert!(handle_callback(stream).is_none());
        assert!(client.join().unwrap().starts_with("HTTP/1.1 404"));
    }

#[test]
    fn lrc_subtitles_become_moments_and_unstamped_lines_are_dropped() {
        let lrc = "[00:12.30]First line\n[01:05.00]Second line\nno stamp here\n[00:40.5]Middle";
        let timed = parse_lrc(lrc);
        // Sorted, whatever order they arrived in, because line_at assumes it.
        assert_eq!(
            timed,
            [
                (12.3, "First line".to_string()),
                (40.5, "Middle".to_string()),
                (65.0, "Second line".to_string())
            ]
        );
    }

    #[test]
    fn the_current_lyric_is_the_last_one_whose_moment_has_passed() {
        let l = Lyrics {
            plain: String::new(),
            timed: vec![
                (10.0, "one".into()),
                (20.0, "two".into()),
                (30.0, "three".into()),
            ],
        };
        // Before the first moment there is no line yet — an intro is not a lyric.
        assert_eq!(l.line_at(0.0), None);
        assert_eq!(l.line_at(9.9), None);
        assert_eq!(l.line_at(10.0), Some(0));
        assert_eq!(l.line_at(29.9), Some(1));
        assert_eq!(l.line_at(300.0), Some(2));
        // Nothing timed at all: the panel shows plain text and never highlights.
        assert_eq!(Lyrics::default().line_at(5.0), None);
    }

    #[test]
    fn a_playlist_is_mine_only_when_the_creator_is_me() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"uuid":"abc","title":"Mix","numberOfTracks":12,"creator":{"id":42,"name":"Pedro"}}"#,
        )
        .unwrap();
        assert!(parse_playlist(&json, Some(42)).mine);
        assert!(!parse_playlist(&json, Some(7)).mine);
        // An editorial playlist has no creator id, so it can never be mistaken for mine.
        let editorial: serde_json::Value =
            serde_json::from_str(r#"{"uuid":"x","title":"Jazz","creator":{}}"#).unwrap();
        let p = parse_playlist(&editorial, Some(42));
        assert!(!p.mine);
        assert_eq!(p.owner, "TIDAL");
    }

    #[test]
    fn an_album_keeps_only_the_year_of_its_release_date() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"id":1,"title":"Rumours","releaseDate":"1977-02-04","numberOfTracks":11,
                "audioQuality":"HI_RES_LOSSLESS","artists":[{"name":"Fleetwood Mac"}]}"#,
        )
        .unwrap();
        let a = parse_album(&json);
        assert_eq!(a.year, Some(1977));
        assert_eq!(a.artist, "Fleetwood Mac");
        assert_eq!(a.quality, "HI_RES_LOSSLESS");
    }

    #[test]
    fn a_pasted_session_is_read_out_of_any_of_the_shapes_people_paste() {
        // Straight out of devtools.
        let json = r#"{"clientId":"abc123","refreshToken":"eyJhbG.rest","userId":42}"#;
        let got = parse_import(json).unwrap();
        assert_eq!(got.client_id.as_deref(), Some("abc123"));
        assert_eq!(got.refresh_token, "eyJhbG.rest");

        // A form body, snake case.
        let form = "grant_type=refresh_token&refresh_token=tok-1&client_id=cid-1";
        let got = parse_import(form).unwrap();
        assert_eq!(got.refresh_token, "tok-1");
        assert_eq!(got.client_id.as_deref(), Some("cid-1"));

        // The client id is optional; the refresh token is the thing that grants.
        let only_token = r#"{"refresh_token": "just-this"}"#;
        assert_eq!(parse_import(only_token).unwrap().refresh_token, "just-this");
        assert!(parse_import(only_token).unwrap().client_id.is_none());

        // Nothing usable in it at all.
        assert!(parse_import("{\"access_token\":\"short-lived\"}").is_none());
    }

    #[test]
    fn the_grant_code_is_found_in_whatever_the_browser_showed() {
        let redirect = "https://tidal.com/android/login/auth?code=abc123&state=x";
        assert_eq!(code_from_redirect(redirect).as_deref(), Some("abc123"));
        // Pasted with the code first, or with nothing but the code.
        assert_eq!(code_from_redirect("code=abc123&x=1").as_deref(), Some("abc123"));
        assert_eq!(code_from_redirect("  abc123  ").as_deref(), Some("abc123"));
        // A redirect that carries an error instead must not be read as a code.
        assert_eq!(code_from_redirect("https://x/auth?error=access_denied"), None);
    }

    #[test]
    fn an_escaped_code_survives_the_paste() {
        assert_eq!(code_from_redirect("https://x/a?code=a%2Fb%2Bc").as_deref(), Some("a/b+c"));
    }

    #[test]
    fn the_redirect_uri_is_escaped_exactly_as_tidal_registered_it() {
        assert_eq!(
            urlencode("https://tidal.com/android/login/auth"),
            "https%3A%2F%2Ftidal.com%2Fandroid%2Flogin%2Fauth"
        );
    }

    #[test]
    fn the_device_flow_refusal_is_recognised_rather_than_reported_as_a_failure() {
        let body = r#"{"error":"invalid_request","error_description":"Client is not a Limited Input Device client","sub_status":1002}"#;
        assert!(is_not_a_device_client(body));
        assert!(!is_not_a_device_client("HTTP 401: token expired"));
    }

    #[test]
    fn a_refresh_response_without_a_refresh_token_keeps_the_old_one() {
        let prev = Session {
            refresh_token: "keep-me".into(),
            country_code: "ES".into(),
            user_id: Some(42),
            ..Default::default()
        };
        let body = r#"{"access_token":"new","expires_in":3600}"#;
        let next = session_from_token_response(body, Some(&prev)).unwrap();
        assert_eq!(next.refresh_token, "keep-me");
        assert_eq!(next.user_id, Some(42));
    }
}
