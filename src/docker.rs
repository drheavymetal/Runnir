//! Docker: the data layer behind the panel.
//!
//! Everything here BLOCKS and belongs on a worker thread, exactly like `git.rs`:
//! a daemon on the other end of an ssh hop answers when it feels like it, and a
//! frame cannot wait for it.
//!
//! The daemon is spoken to over its HTTP socket rather than by parsing `docker`
//! output. Two of the things this panel exists for — `/events` and `/stats` — are
//! streams, and scraping a CLI for a stream is a losing game; going to the socket
//! for the lists too means one transport, one set of field names, and no second
//! format to keep up with.
//!
//! Three transports sit behind one request function, because the socket does not
//! reach everywhere: a local daemon is a unix socket, a remote one is the tunnel
//! the CLI itself uses (`ssh <host> docker system dial-stdio`), and Docker Hub is
//! an HTTP API somewhere else entirely (see `hub.rs`-shaped functions below).

use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::Value;

/// The API version asked for. 1.41 is Docker 20.10, which is old enough that every
/// server this will meet speaks it and new enough to have everything used here.
/// Pinning it means a newer daemon cannot rename a field under us.
const API: &str = "/v1.41";

/// How long a local request may take before it is given up on. A hung daemon is a
/// thing that happens (a full disk, a stuck storage driver), and the worker thread
/// it hangs is one this process never gets back.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Where a daemon lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// A unix socket on this machine.
    Unix(PathBuf),
    /// A daemon reached by ssh, through the same stdio tunnel the CLI opens. Not a
    /// TCP port: the tunnel is what `docker context` gives us and what already has
    /// the user's keys and config behind it.
    Ssh(String),
    /// Docker Hub, which is not a daemon at all. Kept in the same enum because the
    /// panel treats it as a host in the same column, and a caller that forgets it is
    /// different gets an error rather than a wrong answer.
    Hub,
}

impl Endpoint {
    /// Parses what `docker context ls` prints in its endpoint column.
    pub fn parse(endpoint: &str) -> Option<Endpoint> {
        let e = endpoint.trim();
        if let Some(path) = e.strip_prefix("unix://") {
            return Some(Endpoint::Unix(PathBuf::from(path)));
        }
        if let Some(rest) = e.strip_prefix("ssh://") {
            return Some(Endpoint::Ssh(rest.to_string()));
        }
        None
    }

    pub fn label(&self) -> String {
        match self {
            Endpoint::Unix(p) => p.display().to_string(),
            Endpoint::Ssh(h) => format!("ssh://{h}"),
            Endpoint::Hub => "hub.docker.com".to_string(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Endpoint::Unix(_))
    }
}

/// One entry of the hosts column: a docker context, or Docker Hub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub name: String,
    pub endpoint: Endpoint,
    /// What the daemon said its version was, once asked. `None` until then.
    pub version: Option<String>,
    /// Why it could not be reached. A host that is down is DRAWN as down and never
    /// stalls anything: the request already ran on a worker.
    pub error: Option<String>,
    pub current: bool,
}

impl Host {
    pub fn hub() -> Host {
        Host {
            name: "docker hub".into(),
            endpoint: Endpoint::Hub,
            version: None,
            error: None,
            current: false,
        }
    }
}

/// The hosts to show: every docker context, plus Hub.
///
/// The contexts come from the CLI rather than from `~/.docker/contexts`, because
/// the store's layout is an implementation detail of the CLI and the CLI is the
/// thing that has to agree with us about what a context is.
pub fn hosts() -> Vec<Host> {
    let mut out = parse_contexts(&context_ls());
    if out.is_empty() {
        // No CLI, or no contexts: the default socket is still worth offering, since
        // a daemon on it is the common case and the panel would otherwise be empty.
        out.push(Host {
            name: "default".into(),
            endpoint: Endpoint::Unix(PathBuf::from("/var/run/docker.sock")),
            version: None,
            error: None,
            current: true,
        });
    }
    out.push(Host::hub());
    out
}

fn context_ls() -> String {
    let out = std::process::Command::new("docker")
        .args(["context", "ls", "--format", "{{.Name}}\t{{.DockerEndpoint}}\t{{.Current}}"])
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}

/// Parses the tab-separated context list. Pure, so the format is testable.
pub fn parse_contexts(text: &str) -> Vec<Host> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split('\t');
            let name = it.next()?.trim().to_string();
            let endpoint = Endpoint::parse(it.next().unwrap_or(""))?;
            let current = it.next().unwrap_or("false").trim() == "true";
            (!name.is_empty()).then_some(Host { name, endpoint, version: None, error: None, current })
        })
        .collect()
}

// ---- the transport ---------------------------------------------------------

/// A duplex stream to a daemon: a socket, or an ssh child's stdio.
enum Stream {
    Unix(std::os::unix::net::UnixStream),
    Child(std::process::Child),
}

impl Stream {
    fn open(ep: &Endpoint) -> Result<Stream, String> {
        match ep {
            Endpoint::Unix(path) => {
                let s = std::os::unix::net::UnixStream::connect(path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                let _ = s.set_read_timeout(Some(TIMEOUT));
                let _ = s.set_write_timeout(Some(TIMEOUT));
                Ok(Stream::Unix(s))
            }
            Endpoint::Ssh(host) => {
                // The same tunnel the CLI opens. `-T` because there is no terminal
                // here and ssh would otherwise try to allocate one; BatchMode so a
                // host that wants a password fails instead of hanging on a prompt
                // nobody can see.
                //
                // The timeouts are not optional: the read below blocks until EOF and
                // NOTHING else bounds it, so a host that accepts the connection and
                // goes quiet would hang this worker for as long as the process runs
                // — and `probe` joins every one of them.
                let (dest, port) = ssh_destination(host);
                let mut cmd = std::process::Command::new("ssh");
                cmd.args([
                    "-T",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=10",
                    "-o",
                    "ServerAliveInterval=5",
                    "-o",
                    "ServerAliveCountMax=3",
                ]);
                if let Some(port) = port {
                    cmd.args(["-p", &port]);
                }
                let child = cmd
                    .args([&dest, "docker", "system", "dial-stdio"])
                    // Kept, not thrown away: an auth refusal or an unknown host key
                    // is the whole answer, and discarding it left every ssh failure
                    // reading as "no HTTP header in the answer".
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("ssh {host}: {e}"))?;
                Ok(Stream::Child(child))
            }
            Endpoint::Hub => Err("docker hub is not a daemon".into()),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Unix(s) => s.write_all(buf),
            Stream::Child(c) => c.stdin.as_mut().expect("piped").write_all(buf),
        }
    }

    /// What the transport itself said when it failed — `ssh`'s stderr, which is
    /// where "Permission denied (publickey)" and "Could not resolve hostname" live.
    fn failure_text(&mut self) -> Option<String> {
        let Stream::Child(c) = self else { return None };
        let mut text = String::new();
        c.stderr.as_mut()?.read_to_string(&mut text).ok()?;
        let line = text.lines().find(|l| !l.trim().is_empty())?;
        Some(line.trim().to_string())
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Unix(s) => s.read(buf),
            Stream::Child(c) => c.stdout.as_mut().expect("piped").read(buf),
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // An ssh child outlives its request otherwise: the tunnel stays open, the
        // process stays alive, and a panel that refreshes leaks one per refresh.
        if let Stream::Child(c) = self {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Splits a docker context's ssh endpoint into a destination and a port.
///
/// `ssh://root@host:2222` is a URI; as a bare argument `root@host:2222` is a
/// HOSTNAME to ssh, which then fails to resolve it. The port has to become `-p`.
pub fn ssh_destination(endpoint: &str) -> (String, Option<String>) {
    // Only a trailing `:<digits>` is a port. A bare IPv6 address has colons too, and
    // splitting one of those apart would break a host that works today.
    if let Some((host, port)) = endpoint.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) && !host.ends_with(':') {
            return (host.to_string(), Some(port.to_string()));
        }
    }
    (endpoint.to_string(), None)
}

/// One request, one connection, closed after the answer.
///
/// `Connection: close` on purpose: keeping a pool per host would mean owning
/// reconnection, and the expensive part of a docker request is the daemon's work,
/// not the socket. The streams that DO stay open (`/events`, `/stats`, logs) are
/// their own function, because their lifetime is the panel's, not a call's.
pub fn request(ep: &Endpoint, method: &str, path: &str, body: Option<&str>) -> Result<Vec<u8>, String> {
    let mut stream = Stream::open(ep)?;
    let mut req = format!(
        "{method} {API}{path} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    match body {
        Some(b) => {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
                b.len()
            ));
        }
        None => req.push_str("\r\n"),
    }
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    // An ssh transport that failed says why on ITS stderr, and the socket answer is
    // then empty. Reporting "no HTTP header in the answer" for a refused key throws
    // away the only sentence that explains the panel's most common failure.
    let (status, body) = match split_response(&raw) {
        Ok(pair) => pair,
        Err(e) => return Err(stream.failure_text().unwrap_or(e)),
    };
    if !(200..300).contains(&status) {
        return Err(error_message(&body, status));
    }
    Ok(body)
}

/// Splits an HTTP/1.1 response into its status and its decoded body.
///
/// Chunked has to be handled here and not wished away: the daemon uses it for
/// anything it streams, and a body read as-is then carries the chunk lengths in the
/// middle of the JSON.
pub fn split_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no HTTP header in the answer")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("unparseable HTTP status")?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &raw[split + 4..];
    Ok((status, if chunked { dechunk(body) } else { body.to_vec() }))
}

/// Reassembles a chunked body. A malformed length ends the body rather than
/// panicking: this is data off a socket, not something we produced.
pub fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut rest = body;
    loop {
        let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") else { break };
        let size_line = String::from_utf8_lossy(&rest[..eol]);
        let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16);
        let Ok(size) = size else { break };
        if size == 0 {
            break;
        }
        let start = eol + 2;
        let end = (start + size).min(rest.len());
        out.extend_from_slice(&rest[start..end]);
        if end + 2 > rest.len() {
            break;
        }
        rest = &rest[end + 2..];
    }
    out
}

/// The daemon's own error text, which is a JSON object with a `message`. Falling
/// back to the status code alone would turn "no such container" into "404".
fn error_message(body: &[u8], status: u16) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str().map(str::to_string)))
        .unwrap_or_else(|| format!("docker answered {status}"))
}

fn get_json(ep: &Endpoint, path: &str) -> Result<Value, String> {
    let body = request(ep, "GET", path, None)?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

// ---- what the panel shows --------------------------------------------------

/// A container as the list endpoint describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    /// Without the leading `/` the API puts on it.
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `created`, `paused`…
    pub state: String,
    /// The human line: `Up 3 days (healthy)`.
    pub status: String,
    /// Health, split OUT of the status line. A healthcheck failing is not the same
    /// fact as a container being up, and folding them into one word hides the case
    /// that matters: up and unhealthy.
    pub health: Option<Health>,
    pub project: Option<String>,
    pub service: Option<String>,
    /// Published ports as `(host, container, proto)`. Only the published ones: an
    /// unpublished port is not something you can open.
    pub ports: Vec<(u16, u16, String)>,
    /// Named volumes this container mounts. Anonymous ones are left out: they have
    /// no row of their own, and a delete confirm can only name what has one.
    pub volumes: Vec<String>,
    /// The compose files this project was brought up from, off its labels — which
    /// is how compose itself finds them again, and the only way to run a compose
    /// verb on a project the panel did not start.
    pub config_files: Vec<String>,
    pub created: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Unhealthy,
    Starting,
}

impl Health {
    pub fn mark(self) -> char {
        match self {
            Health::Healthy => '\u{2713}',
            Health::Unhealthy => '\u{2717}',
            Health::Starting => '\u{25cc}',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Unhealthy => "unhealthy",
            Health::Starting => "starting",
        }
    }
}

impl Container {
    pub fn running(&self) -> bool {
        self.state == "running"
    }

    /// What a row shows on the right: the status, shortened. `Up 3 days` is what
    /// people read; the rest of the line is the health, which has its own mark.
    pub fn short_status(&self) -> String {
        // Only the HEALTH parenthesis comes off. Splitting on the first ` (` also
        // ate the exit code and the age of every stopped container — `Exited (0) 2
        // days ago` became `Exited`, which is the row that most needed the rest.
        for suffix in [" (healthy)", " (unhealthy)"] {
            if let Some(rest) = self.status.strip_suffix(suffix) {
                return rest.to_string();
            }
        }
        match self.status.find(" (health: ") {
            Some(at) => self.status[..at].to_string(),
            None => self.status.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub id: String,
    pub tags: Vec<String>,
    /// `repo@sha256:…`, which is the ONLY thing that can be compared with a
    /// registry. The local `Id` says nothing about what a remote holds.
    pub digests: Vec<String>,
    pub size: u64,
    pub created: i64,
}

impl Image {
    /// The name a row shows: its first tag, or a short id for an untagged layer.
    pub fn label(&self) -> String {
        match self.tags.iter().find(|t| *t != "<none>:<none>") {
            Some(t) => t.clone(),
            None => format!("<none> {}", short_id(&self.id)),
        }
    }

    /// The digest for one repository, if this image carries one.
    pub fn digest_for(&self, repo: &str) -> Option<&str> {
        self.digests.iter().find_map(|d| {
            let (r, sha) = d.split_once('@')?;
            (r == repo).then_some(sha)
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub project: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
}

/// Everything one host holds, as one snapshot. Read together so the panel never
/// draws a container list from one moment beside an image list from another.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub containers: Vec<Container>,
    pub images: Vec<Image>,
    pub volumes: Vec<Volume>,
    pub networks: Vec<Network>,
    pub version: Option<String>,
}

/// Reads a whole host. Blocking: worker only.
pub fn snapshot(ep: &Endpoint) -> Result<Snapshot, String> {
    // The version doubles as the ping: if this fails the host is down, and the
    // three lists after it would each fail the same way, slowly.
    let version = get_json(ep, "/version")?
        .get("Version")
        .and_then(|v| v.as_str().map(str::to_string));
    Ok(Snapshot {
        containers: parse_containers(&get_json(ep, "/containers/json?all=1")?),
        images: parse_images(&get_json(ep, "/images/json")?),
        volumes: parse_volumes(&get_json(ep, "/volumes")?),
        networks: parse_networks(&get_json(ep, "/networks")?),
        version,
    })
}

pub fn parse_containers(v: &Value) -> Vec<Container> {
    let Some(list) = v.as_array() else { return Vec::new() };
    let mut out: Vec<Container> = list
        .iter()
        .map(|c| {
            let labels = c.get("Labels");
            let label = |k: &str| {
                labels
                    .and_then(|l| l.get(k))
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let status = str_of(c, "Status");
            Container {
                id: str_of(c, "Id"),
                // The API hands names back with a leading slash, which is a detail
                // of how it namespaces them and not part of the name.
                name: c
                    .get("Names")
                    .and_then(|n| n.as_array())
                    .and_then(|a| a.first())
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .trim_start_matches('/')
                    .to_string(),
                image: str_of(c, "Image"),
                state: str_of(c, "State"),
                health: health_of(&status),
                status,
                project: label("com.docker.compose.project"),
                service: label("com.docker.compose.service"),
                ports: parse_ports(c.get("Ports")),
                volumes: parse_mounts(c.get("Mounts")),
                config_files: label("com.docker.compose.project.config_files")
                    .map(|f| f.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default(),
                created: c.get("Created").and_then(|x| x.as_i64()).unwrap_or(0),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The health inside a status line. The list endpoint has no health FIELD — it is
/// only ever in the parenthesis of `Up 3 days (healthy)`.
pub fn health_of(status: &str) -> Option<Health> {
    let s = status.to_ascii_lowercase();
    if s.contains("(healthy)") {
        Some(Health::Healthy)
    } else if s.contains("(unhealthy)") {
        Some(Health::Unhealthy)
    } else if s.contains("health: starting") {
        Some(Health::Starting)
    } else {
        None
    }
}

fn parse_ports(v: Option<&Value>) -> Vec<(u16, u16, String)> {
    let Some(list) = v.and_then(|p| p.as_array()) else { return Vec::new() };
    let mut out: Vec<(u16, u16, String)> = list
        .iter()
        .filter_map(|p| {
            // No PublicPort means nothing is published, and an unpublished port is
            // not something the panel can offer to open.
            let public = p.get("PublicPort").and_then(|x| x.as_u64())? as u16;
            let private = p.get("PrivatePort").and_then(|x| x.as_u64()).unwrap_or(0) as u16;
            let proto = p.get("Type").and_then(|x| x.as_str()).unwrap_or("tcp").to_string();
            Some((public, private, proto))
        })
        .collect();
    // The API repeats a published port once per host address (v4 and v6).
    out.sort();
    out.dedup();
    out
}

/// The NAMED volumes a container mounts.
fn parse_mounts(v: Option<&Value>) -> Vec<String> {
    let Some(list) = v.and_then(|m| m.as_array()) else { return Vec::new() };
    list.iter()
        .filter(|m| m.get("Type").and_then(|t| t.as_str()) == Some("volume"))
        .filter_map(|m| m.get("Name").and_then(|n| n.as_str()).map(str::to_string))
        .collect()
}

pub fn parse_images(v: &Value) -> Vec<Image> {
    let Some(list) = v.as_array() else { return Vec::new() };
    let mut out: Vec<Image> = list
        .iter()
        .map(|i| Image {
            id: str_of(i, "Id"),
            tags: strings(i.get("RepoTags")),
            digests: strings(i.get("RepoDigests")),
            size: i.get("Size").and_then(|x| x.as_u64()).unwrap_or(0),
            created: i.get("Created").and_then(|x| x.as_i64()).unwrap_or(0),
        })
        .collect();
    // Newest first: an image list is read to find what was just built.
    out.sort_by(|a, b| b.created.cmp(&a.created).then(a.label().cmp(&b.label())));
    out
}

pub fn parse_volumes(v: &Value) -> Vec<Volume> {
    let Some(list) = v.get("Volumes").and_then(|x| x.as_array()) else { return Vec::new() };
    let mut out: Vec<Volume> = list
        .iter()
        .map(|x| Volume {
            name: str_of(x, "Name"),
            driver: str_of(x, "Driver"),
            mountpoint: str_of(x, "Mountpoint"),
            project: x
                .get("Labels")
                .and_then(|l| l.get("com.docker.compose.project"))
                .and_then(|s| s.as_str())
                .map(str::to_string),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn parse_networks(v: &Value) -> Vec<Network> {
    let Some(list) = v.as_array() else { return Vec::new() };
    let mut out: Vec<Network> = list
        .iter()
        .map(|n| Network {
            id: str_of(n, "Id"),
            name: str_of(n, "Name"),
            driver: str_of(n, "Driver"),
            subnet: n
                .get("IPAM")
                .and_then(|i| i.get("Config"))
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("Subnet"))
                .and_then(|s| s.as_str())
                .map(str::to_string),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// `sha256:abcdef…` down to the twelve characters everything else prints.
pub fn short_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

/// Sizes the way `docker images` writes them.
pub fn human_size(bytes: u64) -> String {
    const UNIT: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < UNIT.len() {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, UNIT[0])
    } else {
        format!("{v:.1} {}", UNIT[i])
    }
}

/// "3 days ago" from a unix timestamp, with `now` passed in so it is testable.
pub fn ago(created: i64, now: i64) -> String {
    let d = (now - created).max(0);
    match d {
        0..=59 => format!("{d}s ago"),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- inspect ---------------------------------------------------------------

/// One object's full JSON, pretty-printed for the detail column.
pub fn inspect(ep: &Endpoint, kind: Kind, id: &str) -> Result<String, String> {
    let path = match kind {
        Kind::Containers => format!("/containers/{id}/json"),
        Kind::Images => format!("/images/{id}/json"),
        Kind::Volumes => format!("/volumes/{id}"),
        Kind::Networks => format!("/networks/{id}"),
    };
    let v = get_json(ep, &path)?;
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// The last `tail` lines of a container's logs.
///
/// The daemon multiplexes stdout and stderr into 8-byte-headed frames whenever the
/// container has no TTY, so the bytes cannot be shown as they arrive — `demux`
/// unwraps them. A container WITH a tty sends them raw, and the same function
/// leaves those alone.
pub fn logs(ep: &Endpoint, id: &str, tail: usize) -> Result<String, String> {
    let path = format!("/containers/{id}/logs?stdout=1&stderr=1&timestamps=0&tail={tail}");
    let raw = request(ep, "GET", &path, None)?;
    Ok(String::from_utf8_lossy(&demux(&raw)).into_owned())
}

/// Unwraps the daemon's stream framing: `[stream, 0,0,0, len:u32be]` then payload.
///
/// A body that does not look framed is passed through untouched, which is what a
/// TTY container's logs are.
pub fn demux(raw: &[u8]) -> Vec<u8> {
    let framed = raw.len() >= 8 && raw[0] <= 2 && raw[1] == 0 && raw[2] == 0 && raw[3] == 0;
    if !framed {
        return raw.to_vec();
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i + 8 <= raw.len() {
        let len = u32::from_be_bytes([raw[i + 4], raw[i + 5], raw[i + 6], raw[i + 7]]) as usize;
        let start = i + 8;
        let end = (start + len).min(raw.len());
        out.extend_from_slice(&raw[start..end]);
        if end == start && len > 0 {
            break;
        }
        i = end;
    }
    out
}

// ---- what the object column groups by --------------------------------------

/// Which kind of object the middle column is listing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Containers,
    Images,
    Volumes,
    Networks,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Containers, Kind::Images, Kind::Volumes, Kind::Networks];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Containers => "containers",
            Kind::Images => "images",
            Kind::Volumes => "volumes",
            Kind::Networks => "networks",
        }
    }

    pub fn letter(self) -> char {
        match self {
            Kind::Containers => 'C',
            Kind::Images => 'I',
            Kind::Volumes => 'V',
            Kind::Networks => 'N',
        }
    }

    pub fn next(self) -> Kind {
        let i = Kind::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Kind::ALL[(i + 1) % Kind::ALL.len()]
    }

    pub fn prev(self) -> Kind {
        let i = Kind::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Kind::ALL[(i + Kind::ALL.len() - 1) % Kind::ALL.len()]
    }
}

/// The compose projects in a snapshot, in the order the rows will show them, with
/// the loose containers last under `None`.
///
/// Compose is the level this is grouped by because it is the unit the work is
/// actually done in: nobody deploys a container, they deploy a project.
pub fn projects(containers: &[Container]) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = Vec::new();
    for c in containers {
        let key = c.project.clone();
        if !out.contains(&key) {
            out.push(key);
        }
    }
    // Loose containers last: a project is a heading with things under it, and a
    // heading after a flat list reads as part of the list.
    out.sort_by_key(|p| p.is_none());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_line_becomes_a_host_and_an_unknown_endpoint_is_dropped() {
        let hosts = parse_contexts(
            "default\tunix:///var/run/docker.sock\ttrue\n\
             cloudmax\tssh://root@cloudmax\tfalse\n\
             weird\ttcp://1.2.3.4:2375\tfalse\n\
             \t\t\n",
        );
        assert_eq!(hosts.len(), 2, "tcp is not a transport this speaks: {hosts:?}");
        assert_eq!(hosts[0].name, "default");
        assert!(hosts[0].current);
        assert_eq!(hosts[1].endpoint, Endpoint::Ssh("root@cloudmax".into()));
        assert!(!hosts[1].endpoint.is_local());
    }

    #[test]
    fn a_chunked_body_is_reassembled_and_a_plain_one_is_not_touched() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[{\"\r\n3\r\na\":\r\n3\r\n1}]\r\n0\r\n\r\n";
        let (status, body) = split_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(String::from_utf8_lossy(&body), "[{\"a\":1}]");

        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(split_response(plain).unwrap(), (200, b"[]".to_vec()));

        // Garbage in the middle ends the body instead of panicking: this is data off
        // a socket, not something we wrote.
        assert!(dechunk(b"zz\r\nnope").is_empty());
    }

    #[test]
    fn the_daemons_own_words_survive_an_error_status() {
        let raw = b"HTTP/1.1 404 Not Found\r\nContent-Length: 33\r\n\r\n{\"message\":\"No such container: x\"}";
        let (status, body) = split_response(raw).unwrap();
        assert_eq!(status, 404);
        assert_eq!(error_message(&body, status), "No such container: x");
        // ...and a body that is not JSON still says something better than nothing.
        assert_eq!(error_message(b"<html>", 500), "docker answered 500");
    }

    #[test]
    fn a_container_carries_its_compose_project_and_its_health_apart_from_its_state() {
        let v: Value = serde_json::from_str(
            r#"[{"Id":"abc123def4567","Names":["/cromowin-dev-postgres"],"Image":"postgres:16",
                 "State":"running","Status":"Up 24 minutes (healthy)","Created":100,
                 "Labels":{"com.docker.compose.project":"cromowin-dev",
                           "com.docker.compose.service":"postgres"},
                 "Ports":[{"IP":"0.0.0.0","PrivatePort":5432,"PublicPort":5433,"Type":"tcp"},
                          {"IP":"::","PrivatePort":5432,"PublicPort":5433,"Type":"tcp"}]},
                {"Id":"def","Names":["/loose"],"Image":"nginx","State":"exited",
                 "Status":"Exited (0) 2 days ago","Created":50,"Labels":{},"Ports":[]}]"#,
        )
        .unwrap();
        let cs = parse_containers(&v);
        assert_eq!(cs.len(), 2);
        let pg = &cs[0];
        assert_eq!(pg.name, "cromowin-dev-postgres", "the leading slash is not part of it");
        assert_eq!(pg.project.as_deref(), Some("cromowin-dev"));
        assert_eq!(pg.health, Some(Health::Healthy));
        assert!(pg.running());
        assert_eq!(pg.short_status(), "Up 24 minutes", "the health has its own mark");
        // One published port, not one per host address.
        assert_eq!(pg.ports, [(5433, 5432, "tcp".to_string())]);

        let loose = &cs[1];
        assert_eq!(loose.health, None);
        assert!(!loose.running());
        assert_eq!(loose.project, None);

        // Grouped: the project first, the loose ones under the empty heading last.
        assert_eq!(projects(&cs), [Some("cromowin-dev".to_string()), None]);

        // Up and UNHEALTHY is the case that must not fold into "up".
        assert_eq!(health_of("Up 2 hours (unhealthy)"), Some(Health::Unhealthy));
        assert_eq!(health_of("Up 1 second (health: starting)"), Some(Health::Starting));
    }

    #[test]
    fn an_image_is_named_by_a_tag_and_compared_by_a_digest() {
        let v: Value = serde_json::from_str(
            r#"[{"Id":"sha256:6315049057080481de47","RepoTags":["go2chaindev/facturation:front-1.0.0"],
                 "RepoDigests":["go2chaindev/facturation@sha256:aaa","other/thing@sha256:bbb"],
                 "Size":77252313,"Created":200},
                {"Id":"sha256:0000111122223333","RepoTags":["<none>:<none>"],"RepoDigests":[],
                 "Size":10,"Created":300}]"#,
        )
        .unwrap();
        let images = parse_images(&v);
        // Newest first: an image list is read to find what was just built.
        assert_eq!(images[0].label(), "<none> 000011112222");
        assert_eq!(images[1].label(), "go2chaindev/facturation:front-1.0.0");
        // The digest is per REPOSITORY, and it is the only thing a registry can be
        // asked about — the local Id says nothing about a remote.
        assert_eq!(images[1].digest_for("go2chaindev/facturation"), Some("sha256:aaa"));
        assert_eq!(images[1].digest_for("go2chaindev/nothing"), None);
        assert_eq!(human_size(77252313), "77.3 MB");
        assert_eq!(human_size(512), "512 B");
    }

    #[test]
    fn volumes_and_networks_keep_what_a_row_has_to_say() {
        let v: Value = serde_json::from_str(
            r#"{"Volumes":[{"Name":"aios_pg","Driver":"local","Mountpoint":"/var/lib/docker/volumes/aios_pg/_data",
                            "Labels":{"com.docker.compose.project":"go2chain-aios"}}]}"#,
        )
        .unwrap();
        let vols = parse_volumes(&v);
        assert_eq!(vols[0].project.as_deref(), Some("go2chain-aios"));

        let n: Value = serde_json::from_str(
            r#"[{"Id":"58d2","Name":"docker_default","Driver":"bridge",
                 "IPAM":{"Config":[{"Subnet":"172.18.0.0/16","Gateway":"172.18.0.1"}]}},
                {"Id":"aa","Name":"host","Driver":"host","IPAM":{"Config":[]}}]"#,
        )
        .unwrap();
        let nets = parse_networks(&n);
        assert_eq!(nets[0].subnet.as_deref(), Some("172.18.0.0/16"));
        assert_eq!(nets[1].subnet, None, "a host network has no subnet, and says so");
    }

    #[test]
    fn log_frames_are_unwrapped_and_raw_output_is_left_alone() {
        // `[stream, 0,0,0, len]` then the payload, which is what a container with no
        // TTY sends. Shown as-is, the header bytes land in the middle of the text.
        let mut raw = vec![1u8, 0, 0, 0, 0, 0, 0, 5];
        raw.extend_from_slice(b"hello");
        raw.extend_from_slice(&[2u8, 0, 0, 0, 0, 0, 0, 4]);
        raw.extend_from_slice(b"err!");
        assert_eq!(String::from_utf8_lossy(&demux(&raw)), "helloerr!");

        // A TTY container sends the bytes plain, and they must survive untouched.
        assert_eq!(demux(b"plain output\n"), b"plain output\n");
    }

    #[test]
    fn the_kind_strip_wraps_both_ways() {
        assert_eq!(Kind::Containers.next(), Kind::Images);
        assert_eq!(Kind::Networks.next(), Kind::Containers);
        assert_eq!(Kind::Containers.prev(), Kind::Networks);
        assert_eq!(Kind::default(), Kind::Containers);
    }


    #[test]
    fn credentials_come_from_the_file_the_cli_already_wrote() {
        // base64("go2chaindev:secret")
        let auth = decode_basic("Z28yY2hhaW5kZXY6c2VjcmV0").expect("decoded");
        assert_eq!(auth.username, "go2chaindev");
        assert_eq!(auth.secret, "secret");
        // Anything that is not `user:secret` is not credentials, and half of one
        // would authenticate as nobody with a confusing error.
        assert!(decode_basic("bm90aGluZw==").is_none());
        assert!(decode_basic("!!!!").is_none());
    }

    #[test]
    fn a_tag_is_compared_by_digest_and_never_by_id() {
        let published = "sha256:aaa";
        let local = vec![Image {
            id: "sha256:localid".into(),
            tags: vec!["go2chaindev/api:1.0".into(), "go2chaindev/api:old".into()],
            digests: vec!["go2chaindev/api@sha256:aaa".into()],
            ..Image::default()
        }];
        assert_eq!(drift(&local, "go2chaindev/api", "1.0", Some(published)), Drift::Same);
        assert_eq!(drift(&local, "go2chaindev/api", "1.0", Some("sha256:bbb")), Drift::Differs);
        assert_eq!(drift(&local, "go2chaindev/api", "2.0", Some(published)), Drift::NotLocal);
        // Still coming: not a verdict.
        assert_eq!(drift(&local, "go2chaindev/api", "1.0", None), Drift::Unknown);

        // Built here under the name and never pushed: it looks published on any
        // list that goes by tag, and it is the case a deploy gets wrong.
        let built = vec![Image {
            id: "sha256:x".into(),
            tags: vec!["go2chaindev/api:1.0".into()],
            digests: vec!["someone/else@sha256:ccc".into()],
            ..Image::default()
        }];
        assert_eq!(drift(&built, "go2chaindev/api", "1.0", Some(published)), Drift::NoDigest);
    }

    #[test]
    fn the_repo_list_falls_back_to_what_the_local_images_name() {
        let images = vec![
            Image { tags: vec!["go2chaindev/api:1.0".into(), "go2chaindev/api:2.0".into()], ..Image::default() },
            Image { tags: vec!["postgres:16".into()], ..Image::default() },
            Image { tags: vec!["cromowin-html2image:dev".into()], ..Image::default() },
            Image { tags: vec!["go2chaindev/web:1.0".into()], ..Image::default() },
        ];
        let repos: Vec<String> = repos_from_images(&images).into_iter().map(|r| r.name).collect();
        // One row per repository, not per tag; and only what a registry could hold:
        // `postgres:16` is a library image and `cromowin-html2image:dev` is a local
        // build nobody published.
        assert_eq!(repos, ["go2chaindev/api", "go2chaindev/web"]);
    }

    #[test]
    fn hub_answers_are_parsed_or_explained() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"count":2,"results":[{"name":"api","is_private":true,"last_updated":"2026-07-01T10:00:00Z"},
                                      {"name":"web","is_private":false,"last_updated":""}]}"#,
        )
        .unwrap();
        let repos = parse_hub_repos(&v, "go2chaindev").unwrap();
        assert_eq!(repos[0].name, "go2chaindev/api");
        assert!(repos[0].private);

        // Hub's own words survive a refusal, which is the only useful part of its
        // errors — and an organisation token being refused is the NORMAL case here.
        let err: serde_json::Value =
            serde_json::from_str(r#"{"message":"token issued from organization access token is not allowed"}"#)
                .unwrap();
        assert_eq!(
            parse_hub_repos(&err, "go2chaindev").unwrap_err(),
            "token issued from organization access token is not allowed"
        );

        let tags: serde_json::Value =
            serde_json::from_str(r#"{"name":"go2chaindev/api","tags":["2.0","1.0"]}"#).unwrap();
        let parsed = parse_tag_list(&tags);
        assert_eq!(parsed.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["1.0", "2.0"]);
        assert!(parsed[0].digest.is_none(), "a digest is one request per tag, asked for on demand");
    }


    #[test]
    fn the_registrys_next_page_is_found_in_its_link_header() {
        let link = "</v2/go2chaindev/api/tags/list?n=200&last=1.9>; rel=\"next\"";
        assert_eq!(
            next_link(link).as_deref(),
            Some("https://registry-1.docker.io/v2/go2chaindev/api/tags/list?n=200&last=1.9")
        );
        // A header that is only a previous link is not a next page.
        assert_eq!(next_link("</v2/x/tags/list?n=1>; rel=\"prev\""), None);
        assert_eq!(next_link(""), None);
    }

    #[test]
    fn an_ssh_endpoint_keeps_its_port_out_of_the_hostname() {
        assert_eq!(ssh_destination("root@cloudmax"), ("root@cloudmax".into(), None));
        assert_eq!(ssh_destination("root@cloudmax:2222"), ("root@cloudmax".into(), Some("2222".into())));
        // Not a port: a trailing colon, or something that is not a number. Splitting
        // either off would break a host that works today.
        assert_eq!(ssh_destination("host:name"), ("host:name".into(), None));
        assert_eq!(ssh_destination("host:"), ("host:".into(), None));
    }

    #[test]
    fn a_stopped_containers_status_keeps_its_exit_code_and_its_age() {
        let stopped = Container { status: "Exited (137) 2 days ago".into(), ..Container::default() };
        // The health parenthesis is what comes off — not the exit code, which is
        // the most useful thing on the row.
        assert_eq!(stopped.short_status(), "Exited (137) 2 days ago");
        let up = Container { status: "Up 3 days (healthy)".into(), ..Container::default() };
        assert_eq!(up.short_status(), "Up 3 days");
        let starting = Container { status: "Up 1 second (health: starting)".into(), ..Container::default() };
        assert_eq!(starting.short_status(), "Up 1 second");
        let restarting = Container { status: "Restarting (1) 5 seconds ago".into(), ..Container::default() };
        assert_eq!(restarting.short_status(), "Restarting (1) 5 seconds ago");
    }


    /// Talks to the daemon on this machine. Ignored by default — a test suite that
    /// needs a running docker is a suite that fails on the machine without one.
    #[test]
    #[ignore]
    fn reads_the_real_daemon() {
        let ep = Endpoint::Unix(PathBuf::from("/var/run/docker.sock"));
        let snap = snapshot(&ep).expect("a daemon on the default socket");
        println!(
            "version {:?}: {} containers, {} images, {} volumes, {} networks",
            snap.version,
            snap.containers.len(),
            snap.images.len(),
            snap.volumes.len(),
            snap.networks.len()
        );
        assert!(snap.version.is_some());
        if let Some(c) = snap.containers.first() {
            let text = inspect(&ep, Kind::Containers, &c.id).expect("inspect");
            assert!(text.contains("\"Id\""));
            let _ = logs(&ep, &c.id, 5).expect("logs");
        }
    }

    #[test]
    fn ages_read_the_way_docker_writes_them() {
        assert_eq!(ago(100, 130), "30s ago");
        assert_eq!(ago(0, 3600 * 5), "5h ago");
        assert_eq!(ago(0, 86400 * 3 + 10), "3d ago");
        assert_eq!(ago(500, 100), "0s ago", "a clock that went backwards is not an error");
    }
}

// ---- what a worker sends back ----------------------------------------------

/// One answer from a docker worker, tagged by the panel's request generation.
///
/// Everything the panel shows arrives this way. Nothing here is ever computed on
/// the UI thread: the cheapest call in this file opens a socket, and the dearest
/// one opens an ssh connection to another machine.
pub enum PanelMsg {
    /// The hosts, with each one's version filled in (or the reason it is down).
    Hosts(Vec<Host>),
    /// Everything one host holds. The index says which host asked.
    Snapshot(usize, Result<Snapshot, String>),
    /// The detail column's text for one object, in the mode it was asked for.
    Detail(String, crate::overlay::DockerDetail, Result<String, String>),
    /// An operation finished, on the host at this index.
    Done(usize, Result<String, String>),
    /// The repositories on Docker Hub, and how they were found: the web API, or
    /// the local images when the API refused the credentials.
    Repos(Result<Vec<HubRepo>, String>, String),
    /// One repository's tags, and whether the list stopped at the page cap.
    Tags(String, Result<(Vec<HubTag>, bool), String>),
    /// One tag's manifest digest, which is what the local one is compared against.
    Digest(String, String, Result<String, String>),
}

/// Asks every host for its version, in parallel, so one unreachable machine does
/// not hold up the list. Blocking: worker only.
pub fn probe(mut hosts: Vec<Host>) -> Vec<Host> {
    let handles: Vec<_> = hosts
        .iter()
        .map(|h| {
            let ep = h.endpoint.clone();
            std::thread::spawn(move || match ep {
                // Hub is not asked: it has no version, and a request to it here
                // would cost a round trip to the internet before the panel opens.
                Endpoint::Hub => (None, None),
                ep => match get_json(&ep, "/version") {
                    Ok(v) => (v.get("Version").and_then(|x| x.as_str().map(str::to_string)), None),
                    Err(e) => (None, Some(e)),
                },
            })
        })
        .collect();
    for (host, handle) in hosts.iter_mut().zip(handles) {
        match handle.join() {
            Ok((version, error)) => {
                host.version = version;
                host.error = error;
            }
            Err(_) => host.error = Some("the probe panicked".into()),
        }
    }
    hosts
}

// ---- operations ------------------------------------------------------------

/// Something to do to one object, on the daemon that holds it.
///
/// Only the SHORT operations are here. Anything that takes minutes or prints a
/// progress bar — build, push, pull, `compose up`, `exec` — goes to a real pane
/// instead (see `compose_command`), because a pane already has colour, Ctrl-C and
/// scrollback and this panel would have to grow all three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// `start`, `stop`, `restart`, `kill`, `pause`, `unpause`.
    Container { id: String, verb: &'static str },
    RemoveContainer { id: String, force: bool },
    RemoveImage { id: String },
    RemoveVolume { name: String },
    RemoveNetwork { id: String },
}

impl Op {
    /// What the footer says when it worked. Written from the OP rather than from
    /// the daemon's answer, which is empty for every one of these.
    pub fn done(&self) -> String {
        match self {
            // Spelled out rather than built from the verb: "stop" + "ed" is
            // "stoped", and a panel that cannot spell reads as one that guessed.
            Op::Container { verb, .. } => match *verb {
                "start" => "started".into(),
                "stop" => "stopped".into(),
                "restart" => "restarted".into(),
                "kill" => "killed".into(),
                "pause" => "paused".into(),
                "unpause" => "unpaused".into(),
                other => other.to_string(),
            },
            Op::RemoveContainer { .. } => "container removed".into(),
            Op::RemoveImage { .. } => "image removed".into(),
            Op::RemoveVolume { .. } => "volume removed".into(),
            Op::RemoveNetwork { .. } => "network removed".into(),
        }
    }
}

/// Runs one operation. Blocking: worker only.
pub fn run_op(ep: &Endpoint, op: &Op) -> Result<String, String> {
    let (method, path) = match op {
        Op::Container { id, verb } => ("POST", format!("/containers/{id}/{verb}")),
        // `v=1` takes the volumes that were created ANONYMOUSLY for this container
        // with it. Named volumes are not touched by it — those are their own row.
        Op::RemoveContainer { id, force } => {
            ("DELETE", format!("/containers/{id}?v=1&force={}", if *force { 1 } else { 0 }))
        }
        Op::RemoveImage { id } => ("DELETE", format!("/images/{id}")),
        Op::RemoveVolume { name } => ("DELETE", format!("/volumes/{name}")),
        Op::RemoveNetwork { id } => ("DELETE", format!("/networks/{id}")),
    };
    request(ep, method, &path, None)?;
    Ok(op.done())
}

/// How to reach a host from the COMMAND LINE, for the operations that run in a
/// pane. A context is passed with `-c`, which is what the user would type.
pub fn cli_prefix(host: &Host) -> Vec<String> {
    match host.endpoint {
        // The default socket needs no flag, and adding one would break for anyone
        // whose current context is not called "default".
        Endpoint::Unix(_) if host.current => vec!["docker".into()],
        _ => vec!["docker".into(), "-c".into(), host.name.clone()],
    }
}

/// The command line for a compose verb on one project.
///
/// Compose is not in the daemon API at all: it is a client that reads yaml and
/// talks to the daemon, so this is the one place where the CLI is not a shortcut
/// but the only way. The project's config files come from the labels its own
/// containers carry, which is how compose itself finds them again.
pub fn compose_command(host: &Host, project: &str, files: &[String], verb: &[&str]) -> Vec<String> {
    match &host.endpoint {
        // Over ssh the whole thing runs THERE. `docker -c <ctx>` only redirects the
        // daemon connection, while the compose client still reads `-f` from the
        // local filesystem — and those paths came off the containers' labels, which
        // are paths on the remote machine. So a remote compose has to be a remote
        // command, not a local client pointed at a remote daemon.
        Endpoint::Ssh(dest) => ssh_wrap(dest, &shell_join(&compose_argv("docker", project, files, verb))),
        _ => {
            let mut cmd = cli_prefix(host);
            cmd.extend(compose_argv("", project, files, verb).into_iter().skip(1));
            cmd
        }
    }
}

/// `docker compose -p <project> [-f <file>…] <verb>` as argv.
fn compose_argv(program: &str, project: &str, files: &[String], verb: &[&str]) -> Vec<String> {
    let mut cmd = vec![program.to_string(), "compose".into(), "-p".into(), project.to_string()];
    for f in files {
        cmd.push("-f".into());
        cmd.push(f.clone());
    }
    cmd.extend(verb.iter().map(|v| v.to_string()));
    cmd
}

/// Wraps one remote command line in an `ssh` invocation, port and all.
fn ssh_wrap(dest: &str, line: &str) -> Vec<String> {
    let (dest, port) = ssh_destination(dest);
    let mut cmd = vec!["ssh".to_string(), "-t".into()];
    if let Some(port) = port {
        cmd.push("-p".into());
        cmd.push(port);
    }
    cmd.push(dest);
    // One argument: ssh joins what follows with spaces and hands it to a remote
    // shell, so a path with a space would fall apart if it were split here.
    cmd.push(line.to_string());
    cmd
}

/// The command line that publishes one image tag.
///
/// A push is the one docker verb with consequences outside this machine: it is
/// what `go2chaindev/*` on Hub becomes, so it asks first and it goes to a pane —
/// it is minutes of progress bars and it can be Ctrl-C'd.
pub fn push_command(host: &Host, tag: &str) -> Vec<String> {
    let mut cmd = cli_prefix(host);
    cmd.push("push".into());
    cmd.push(tag.to_string());
    cmd
}

/// The deploy, as ONE command line: pull what was published, then bring the
/// project up on it.
///
/// Chained with `&&` rather than run as two: if the pull fails there is nothing
/// new to bring up, and an `up` after a failed pull silently restarts the project
/// on the image it was already running — the deploy that looks like it worked.
pub fn deploy_command(host: &Host, project: &str, files: &[String]) -> Vec<String> {
    // Every caller of a command here treats it as ARGV — one word per element — so
    // a chain cannot be expressed as three elements with a bare `&&` in the middle:
    // the shell would look for a program whose name is the whole first command.
    // It has to be handed to a shell explicitly.
    match &host.endpoint {
        Endpoint::Ssh(dest) => {
            let pull = shell_join(&compose_argv("docker", project, files, &["pull"]));
            let up = shell_join(&compose_argv("docker", project, files, &["up", "-d"]));
            ssh_wrap(dest, &format!("{pull} && {up}"))
        }
        _ => {
            let pull = shell_join(&compose_command(host, project, files, &["pull"]));
            let up = shell_join(&compose_command(host, project, files, &["up", "-d"]));
            vec!["sh".into(), "-c".into(), format!("{pull} && {up}")]
        }
    }
}

/// Joins one command into a single shell word-safe string, for chaining.
fn shell_join(cmd: &[String]) -> String {
    cmd.iter()
        .map(|a| {
            if a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=".contains(c)) {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command line for a shell inside a container.
///
/// `bash` if it is there and `sh` if it is not, decided INSIDE the container:
/// a lot of images have no bash, and picking from out here would need another
/// round trip to find out.
pub fn exec_command(host: &Host, id: &str) -> Vec<String> {
    let mut cmd = cli_prefix(host);
    cmd.extend(["exec".into(), "-it".into(), id.to_string()]);
    cmd.push("sh".into());
    cmd.push("-c".into());
    cmd.push("command -v bash >/dev/null 2>&1 && exec bash || exec sh".into());
    cmd
}

/// Which containers use a volume, so a delete confirm can NAME them. Cheap: the
/// snapshot is already in hand, so this is a scan of it and not a request.
pub fn volume_users<'a>(containers: &'a [Container], name: &str) -> Vec<&'a str> {
    containers
        .iter()
        .filter(|c| c.volumes.iter().any(|v| v == name))
        .map(|c| c.name.as_str())
        .collect()
}

/// Which containers were started from an image, by tag or by id.
pub fn image_users<'a>(containers: &'a [Container], image: &Image) -> Vec<&'a str> {
    let short = short_id(&image.id);
    containers
        .iter()
        .filter(|c| image.tags.iter().any(|t| *t == c.image) || c.image.contains(&short))
        .map(|c| c.name.as_str())
        .collect()
}

// ---- Docker Hub ------------------------------------------------------------
//
// Not a daemon: a registry (`registry-1.docker.io`, the v2 protocol) with a web API
// beside it (`hub.docker.com`). Both are needed, and they authenticate DIFFERENTLY,
// which is the whole shape of this section.
//
// The credentials are read from `~/.docker/config.json` and its credential helper
// FIRST: if `docker login` already happened there is nothing for runnir to store,
// and a terminal asking again for a token the machine already has is a terminal
// inventing a secret to look after.

/// A username and a secret for Docker Hub, as `docker login` left them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubAuth {
    pub username: String,
    pub secret: String,
}

/// The server key `docker login` files Hub credentials under. It is not
/// `hub.docker.com`, and looking there finds nothing.
const HUB_SERVER: &str = "https://index.docker.io/v1/";

/// Reads the credentials `docker login` stored, from the helper or from the file.
pub fn hub_credentials() -> Option<HubAuth> {
    let path = dirs::home_dir()?.join(".docker/config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let cfg: Value = serde_json::from_str(&text).ok()?;
    // A helper wins over the file: when one is configured the file's `auths` entry
    // is an empty placeholder, which parses fine and authenticates as nobody.
    let helper = cfg
        .get("credHelpers")
        .and_then(|h| h.get(HUB_SERVER))
        .or_else(|| cfg.get("credsStore"))
        .and_then(|s| s.as_str());
    if let Some(helper) = helper {
        if let Some(auth) = credential_helper(helper) {
            return Some(auth);
        }
    }
    let encoded = cfg.get("auths")?.get(HUB_SERVER)?.get("auth")?.as_str()?;
    decode_basic(encoded)
}

/// Asks `docker-credential-<helper>` for one server's credentials, the way the CLI
/// does: the server URL on stdin, a JSON object back.
fn credential_helper(helper: &str) -> Option<HubAuth> {
    use std::io::Write;
    let mut child = std::process::Command::new(format!("docker-credential-{helper}"))
        .arg("get")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // Written before the wait, and a failed write does NOT return early: the child
    // has to be reaped either way, or a helper that exits at once leaves a zombie
    // for the life of the terminal.
    let wrote = child
        .stdin
        .as_mut()
        .map(|stdin| stdin.write_all(HUB_SERVER.as_bytes()).is_ok())
        .unwrap_or(false);
    let out = child.wait_with_output().ok()?;
    if !wrote || !out.status.success() {
        return None;
    }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let username = v.get("Username")?.as_str()?.to_string();
    let secret = v.get("Secret")?.as_str()?.to_string();
    (!username.is_empty() && !secret.is_empty()).then_some(HubAuth { username, secret })
}

/// `base64(user:secret)`, which is what the file holds when there is no helper.
pub fn decode_basic(encoded: &str) -> Option<HubAuth> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (username, secret) = text.split_once(':')?;
    (!username.is_empty() && !secret.is_empty())
        .then(|| HubAuth { username: username.into(), secret: secret.into() })
}

/// One repository on Hub, as the panel lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HubRepo {
    /// `namespace/name`, which is also what a local tag is prefixed with.
    pub name: String,
    pub private: bool,
    pub last_updated: String,
}

/// One tag of one repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HubTag {
    pub name: String,
    /// The manifest digest, which is what a local `RepoDigest` can be compared with.
    /// `None` until it has been asked for: one request per tag, so they are fetched
    /// for what is on screen and not for the whole list.
    pub digest: Option<String>,
}

/// How a local image compares with the tag of the same name on Hub.
///
/// This is the thing lazydocker cannot do and the reason the hub column exists: a
/// row that says whether what is running here is what is published there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drift {
    /// The local image is the published one, digest for digest.
    Same,
    /// Both exist and they are different images.
    Differs,
    /// Nothing local carries this tag.
    NotLocal,
    /// The image is here but carries no digest for THIS repository — it was built
    /// locally under the name and never pushed or pulled under it. Worth saying:
    /// it looks like the published one on every list that goes by tag, and it is
    /// the case a deploy gets wrong.
    NoDigest,
    /// Not asked yet.
    Unknown,
}

impl Drift {
    pub fn label(self) -> &'static str {
        match self {
            Drift::Same => "same as local",
            Drift::Differs => "differs from local",
            Drift::NotLocal => "not pulled here",
            Drift::NoDigest => "local, never pushed",
            Drift::Unknown => "",
        }
    }
}

/// Compares one repository tag against the local images.
///
/// By DIGEST, never by id: the local id is a content hash of the image as this
/// machine stored it and says nothing about what a registry holds. The digest is
/// per repository, which is why the repo has to be passed in.
pub fn drift(images: &[Image], repo: &str, tag: &str, remote: Option<&str>) -> Drift {
    let full = format!("{repo}:{tag}");
    let Some(local) = images.iter().find(|i| i.tags.iter().any(|t| *t == full)) else {
        return Drift::NotLocal;
    };
    let Some(remote) = remote else { return Drift::Unknown };
    let Some(local_digest) = local.digest_for(repo) else { return Drift::NoDigest };
    if local_digest == remote { Drift::Same } else { Drift::Differs }
}

/// The repositories a namespace holds, from the Hub web API.
///
/// This one needs a bearer minted from the PAT (`/v2/auth/token`); an
/// ORGANISATION access token is refused by that API even though the registry
/// accepts it, so the caller has a fallback and this returning `Err` is normal.
pub fn hub_repos(auth: &HubAuth, namespace: &str) -> Result<Vec<HubRepo>, String> {
    let token = hub_bearer(auth)?;
    let mut url = Some(format!(
        "https://hub.docker.com/v2/repositories/{namespace}/?page_size=100&ordering=name"
    ));
    let mut out = Vec::new();
    // Followed to the end, not just the first page: a namespace with 120 repos
    // would otherwise lose 20 of them with nothing said. The cap is a runaway
    // guard, not a limit anyone should reach.
    for _ in 0..PAGE_CAP {
        let Some(next) = url.take() else { break };
        let mut resp = ureq::get(&next)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build()
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| format!("hub: {e}"))?;
        let v: Value = resp.body_mut().read_json().map_err(|e| format!("hub: {e}"))?;
        out.extend(parse_hub_repos(&v, namespace)?);
        url = v.get("next").and_then(|n| n.as_str()).map(str::to_string);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// How many pages of a paginated answer are followed. Ten pages of repositories is
/// a thousand; a namespace past that is not a namespace this panel can help with.
const PAGE_CAP: usize = 10;

/// Exchanges a personal (or organisation) access token for a Hub API bearer.
fn hub_bearer(auth: &HubAuth) -> Result<String, String> {
    let body = serde_json::json!({ "identifier": auth.username, "secret": auth.secret });
    let mut resp = ureq::post("https://hub.docker.com/v2/auth/token")
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("hub login: {e}"))?;
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("hub login: {e}"))?;
    v.get("access_token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| hub_message(&v))
}

/// Hub's own words for a failure, which are the only useful part of its errors.
fn hub_message(v: &Value) -> String {
    v.get("message")
        .or_else(|| v.get("detail"))
        .and_then(|m| m.as_str())
        .unwrap_or("hub refused the token")
        .to_string()
}

pub fn parse_hub_repos(v: &Value, namespace: &str) -> Result<Vec<HubRepo>, String> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return Err(hub_message(v));
    };
    Ok(results
        .iter()
        .map(|r| HubRepo {
            name: format!("{namespace}/{}", str_of(r, "name")),
            private: r.get("is_private").and_then(|p| p.as_bool()).unwrap_or(false),
            last_updated: str_of(r, "last_updated"),
        })
        .collect())
}

/// The repositories the LOCAL images name, as the fallback for a namespace whose
/// list cannot be read.
///
/// It is a smaller answer than Hub's, and for the question this panel asks — is
/// what runs here what is published there — it is the RIGHT answer: a repository
/// with nothing local is a repository with nothing to compare.
pub fn repos_from_images(images: &[Image]) -> Vec<HubRepo> {
    let mut out: Vec<HubRepo> = Vec::new();
    for image in images {
        for tag in &image.tags {
            let Some((repo, _)) = tag.rsplit_once(':') else { continue };
            // Only what DOCKER HUB could hold: an unqualified `postgres:16` is a
            // library image, `foo-api:dev` is a local build nobody published, and
            // `ghcr.io/org/app` lives on another registry entirely — asking Hub
            // about that one answers "not on the registry" about an image that is
            // published, which is a wrong answer to the question this panel asks.
            let first = repo.split('/').next().unwrap_or("");
            let elsewhere = first.contains('.') || first.contains(':') || first == "localhost";
            if !repo.contains('/') || elsewhere || out.iter().any(|r| r.name == repo) {
                continue;
            }
            out.push(HubRepo { name: repo.to_string(), private: false, last_updated: String::new() });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A registry bearer for one repository, from the token service the registry
/// points at. This is the path an ORGANISATION token can take, which is why the
/// tags come from the registry and not from the web API.
fn registry_token(auth: Option<&HubAuth>, repo: &str, scope: &str) -> Result<String, String> {
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{repo}:{scope}"
    );
    let req = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build();
    let req = match auth {
        Some(a) => {
            use base64::Engine;
            let basic = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", a.username, a.secret));
            req.header("Authorization", &format!("Basic {basic}"))
        }
        None => req,
    };
    let mut resp = req.call().map_err(|e| format!("registry auth: {e}"))?;
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("registry auth: {e}"))?;
    v.get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "registry refused the credentials".to_string())
}

/// The tags of one repository, from the registry.
pub fn hub_tags(auth: Option<&HubAuth>, repo: &str) -> Result<(Vec<HubTag>, bool), String> {
    let token = registry_token(auth, repo, "pull")?;
    let mut url = Some(format!("https://registry-1.docker.io/v2/{repo}/tags/list?n=200"));
    let mut out: Vec<HubTag> = Vec::new();
    let mut truncated = false;
    for page in 0..PAGE_CAP {
        let Some(next) = url.take() else { break };
        let resp = ureq::get(&next)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build()
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| registry_error(e, repo))?;
        // The registry paginates with a `Link` header, so the answer is read
        // BEFORE the body: a repository with hundreds of tags would otherwise show
        // an alphabetically early slice and call it the tag list.
        let link = resp
            .headers()
            .get("link")
            .and_then(|h| h.to_str().ok())
            .map(str::to_string);
        let mut resp = resp;
        let v: Value = resp.body_mut().read_json().map_err(|e| format!("registry: {e}"))?;
        out.extend(parse_tag_list(&v));
        url = link.as_deref().and_then(next_link);
        truncated = url.is_some() && page + 1 == PAGE_CAP;
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    Ok((out, truncated))
}

/// The `next` URL out of a registry `Link` header, made absolute.
pub fn next_link(link: &str) -> Option<String> {
    let part = link.split(',').find(|p| p.contains("rel=\"next\""))?;
    let start = part.find('<')? + 1;
    let end = part[start..].find('>')? + start;
    let path = &part[start..end];
    Some(match path.starts_with("http") {
        true => path.to_string(),
        false => format!("https://registry-1.docker.io{path}"),
    })
}

pub fn parse_tag_list(v: &Value) -> Vec<HubTag> {
    let mut out: Vec<HubTag> = strings(v.get("tags"))
        .into_iter()
        .map(|name| HubTag { name, digest: None })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// What a registry failure MEANS, in the words of the thing that failed.
///
/// A 404 here is the common and interesting case: a locally built image tagged
/// `org/thing` that was never pushed. "http status: 404" makes that read like a
/// bug in runnir; it is the answer to the question.
fn registry_error(e: ureq::Error, repo: &str) -> String {
    match e {
        ureq::Error::StatusCode(404) => format!("{repo} is not on the registry"),
        ureq::Error::StatusCode(401 | 403) => {
            format!("{repo}: the stored docker login cannot read it")
        }
        other => format!("registry: {other}"),
    }
}

/// The manifest digest of one tag — the number the local `RepoDigest` is compared
/// against.
///
/// The `Accept` headers matter: without them the registry converts a modern
/// multi-architecture image into an old single-arch manifest and answers with the
/// digest of the CONVERSION, which never matches anything local.
pub fn hub_digest(auth: Option<&HubAuth>, repo: &str, tag: &str) -> Result<String, String> {
    let token = registry_token(auth, repo, "pull")?;
    let url = format!("https://registry-1.docker.io/v2/{repo}/manifests/{tag}");
    let resp = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .header("Authorization", &format!("Bearer {token}"))
        .header(
            "Accept",
            "application/vnd.oci.image.index.v1+json, \
             application/vnd.oci.image.manifest.v1+json, \
             application/vnd.docker.distribution.manifest.list.v2+json, \
             application/vnd.docker.distribution.manifest.v2+json",
        )
        .call()
        .map_err(|e| registry_error(e, repo))?;
    resp.headers()
        .get("docker-content-digest")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "the registry sent no digest".to_string())
}
