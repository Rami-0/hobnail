//! Read-only Docker client over the unix socket. Only GET requests, ever.
//!
//! Cost model: one `GET /containers/json?all=1` at start, then one long-lived
//! `GET /events` connection that sits idle until a container changes state.
//! A change triggers a debounced re-list. A safety re-list runs every 60s.
//! Nothing here polls faster than that, so it is cheap to keep running with no
//! viewers (alerting depends on fresh container state).

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::Method;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UnixStream;

use crate::http::client::{read_body, request_on_io};
use crate::model::{Container, ContainerStats, PortMapping, Snapshot, StatsMap};
use crate::state::AppState;

/// Largest container list we accept (35 containers is ~100 KiB).
const MAX_LIST_BODY: usize = 16 * 1024 * 1024;
/// A single event line longer than this is dropped (they are ~1 KiB).
const MAX_EVENT_LINE: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEBOUNCE: Duration = Duration::from_millis(300);
const SAFETY_RELIST: Duration = Duration::from_secs(60);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// How long `die` timestamps are remembered.
const DIE_WINDOW_SECS: u64 = 3600;

/// Container actions that can change what the page shows. Exec and
/// attach events (healthchecks fire `exec_*` constantly) are not in here.
const EVENTS: &[&str] = &[
    "create",
    "start",
    "restart",
    "die",
    "stop",
    "kill",
    "oom",
    "destroy",
    "pause",
    "unpause",
    "rename",
    "health_status",
];

/// `GET /containers/json?all=1`, mapped to our model.
pub async fn list_containers(socket: &str) -> Result<Vec<Container>, String> {
    let body = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let resp = get(socket, "/containers/json?all=1").await?;
        read_body(resp.into_body(), MAX_LIST_BODY).await
    })
    .await
    .map_err(|_| "docker: listing containers timed out".to_string())??;
    parse_containers(&body)
}

/// Open a GET request on the socket and check the status.
async fn get(socket: &str, path: &str) -> Result<hyper::Response<Incoming>, String> {
    let io = UnixStream::connect(socket)
        .await
        .map_err(|e| format!("cannot connect to {socket}: {e}"))?;
    let resp = request_on_io(io, Method::GET, "docker", path, &[], None).await?;
    let status = resp.status();
    if !status.is_success() {
        let body = read_body(resp.into_body(), 4096).await.unwrap_or_default();
        let msg = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("message")?.as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
        return Err(format!("docker returned {}: {msg}", status.as_u16()));
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// Parsing (pure, unit-tested)

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawContainer {
    id: String,
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    image: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    created: i64,
    #[serde(default)]
    labels: Option<BTreeMap<String, String>>,
    #[serde(default)]
    ports: Option<Vec<RawPort>>,
    /// Present on API >= 1.52 (`{"Status":"healthy"|"none",..}`).
    #[serde(default)]
    health: Option<RawHealth>,
}

#[derive(Deserialize)]
struct RawPort {
    #[serde(rename = "IP", default)]
    ip: String,
    #[serde(rename = "PrivatePort", default)]
    private_port: u16,
    #[serde(rename = "PublicPort", default)]
    public_port: Option<u16>,
    #[serde(rename = "Type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct RawHealth {
    #[serde(rename = "Status", default)]
    status: String,
}

/// Parse the JSON body of `GET /containers/json`.
pub fn parse_containers(body: &[u8]) -> Result<Vec<Container>, String> {
    let raw: Vec<RawContainer> =
        serde_json::from_slice(body).map_err(|e| format!("docker: bad container list: {e}"))?;
    let mut out: Vec<Container> = raw.into_iter().map(convert).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn convert(r: RawContainer) -> Container {
    let name = r
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_else(|| r.id.chars().take(12).collect());
    let labels = r.labels.unwrap_or_default();
    let health = parse_health(&r.status).or_else(|| {
        r.health
            .map(|h| h.status)
            .filter(|s| matches!(s.as_str(), "healthy" | "unhealthy" | "starting"))
    });
    let exit_code = parse_exit_code(&r.status);
    Container {
        project: labels.get("com.docker.compose.project").cloned(),
        service: labels.get("com.docker.compose.service").cloned(),
        ports: convert_ports(r.ports.unwrap_or_default()),
        id: r.id,
        name,
        image: r.image,
        state: r.state,
        status: r.status,
        health,
        exit_code,
        labels,
        created: r.created,
        die_times: Vec::new(),
    }
}

/// Published ports only, deduplicated, in a stable order. Docker reports a
/// port published on all interfaces twice (`0.0.0.0` and `::`); we keep the
/// IPv4 entry.
fn convert_ports(raw: Vec<RawPort>) -> Vec<PortMapping> {
    let mut ports: Vec<PortMapping> = raw
        .into_iter()
        .filter_map(|p| {
            Some(PortMapping {
                host_port: p.public_port.filter(|&n| n != 0)?,
                host_ip: p.ip,
                container_port: p.private_port,
                proto: if p.kind.is_empty() {
                    "tcp".into()
                } else {
                    p.kind
                },
            })
        })
        .collect();
    let has_v4_any = |ports: &[PortMapping], p: &PortMapping| {
        ports.iter().any(|q| {
            q.host_ip == "0.0.0.0"
                && q.host_port == p.host_port
                && q.container_port == p.container_port
                && q.proto == p.proto
        })
    };
    let dup_v6: Vec<bool> = ports
        .iter()
        .map(|p| p.host_ip == "::" && has_v4_any(&ports, p))
        .collect();
    let mut keep = dup_v6.iter().map(|d| !d);
    ports.retain(|_| keep.next().unwrap_or(true));
    ports.sort_by(|a, b| {
        (a.container_port, a.host_port, &a.proto, &a.host_ip).cmp(&(
            b.container_port,
            b.host_port,
            &b.proto,
            &b.host_ip,
        ))
    });
    ports.dedup();
    ports
}

/// `Up 3 hours (healthy)` → healthy, `(unhealthy)`, `(health: starting)` → starting.
pub fn parse_health(status: &str) -> Option<String> {
    if status.contains("(unhealthy)") {
        Some("unhealthy".into())
    } else if status.contains("(healthy)") {
        Some("healthy".into())
    } else if status.contains("(health: starting)") {
        Some("starting".into())
    } else {
        None
    }
}

/// `Exited (137) 3 hours ago` → 137 (also `Restarting (1) …`).
pub fn parse_exit_code(status: &str) -> Option<i64> {
    let rest = status
        .strip_prefix("Exited (")
        .or_else(|| status.strip_prefix("Restarting ("))?;
    rest.split(')').next()?.trim().parse().ok()
}

/// One relevant event from the `/events` stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub action: String,
    pub name: Option<String>,
    pub time: u64,
    /// `die`: the exit code; `health_status`: healthy / unhealthy / starting.
    pub detail: Option<String>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "Type", default)]
    kind: String,
    #[serde(rename = "Action", default)]
    action: String,
    #[serde(rename = "Actor", default)]
    actor: Option<RawActor>,
    #[serde(default)]
    time: u64,
}

#[derive(Deserialize)]
struct RawActor {
    #[serde(rename = "Attributes", default)]
    attributes: Option<HashMap<String, String>>,
}

/// Parse one event line; None if it is not a container event we care about.
pub fn parse_event(line: &[u8]) -> Option<Event> {
    let raw: RawEvent = serde_json::from_slice(line).ok()?;
    if raw.kind != "container" {
        return None;
    }
    // `health_status: healthy` → `health_status`.
    let base = raw.action.split(':').next().unwrap_or("").trim();
    if !EVENTS.contains(&base) {
        return None;
    }
    let mut attrs = raw.actor.and_then(|a| a.attributes).unwrap_or_default();
    let name = attrs.remove("name");
    let detail = match base {
        "die" => attrs.remove("exitCode"),
        "health_status" => raw
            .action
            .split_once(':')
            .map(|(_, s)| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    };
    Some(Event {
        action: base.to_string(),
        name,
        time: raw.time,
        detail,
    })
}

/// Accumulates stream bytes and yields complete newline-terminated lines.
/// Frames can split a JSON object anywhere.
#[derive(Debug, Default)]
pub struct LineBuffer {
    buf: Vec<u8>,
}

impl LineBuffer {
    pub fn push(&mut self, data: &[u8], mut on_line: impl FnMut(&[u8])) {
        self.buf.extend_from_slice(data);
        let mut start = 0;
        while let Some(i) = self.buf[start..].iter().position(|&b| b == b'\n') {
            let line = &self.buf[start..start + i];
            if !line.iter().all(u8::is_ascii_whitespace) {
                on_line(line);
            }
            start += i + 1;
        }
        self.buf.drain(..start);
        if self.buf.len() > MAX_EVENT_LINE {
            self.buf.clear();
        }
        if self.buf.capacity() > 64 * 1024 && self.buf.is_empty() {
            self.buf.shrink_to(4096);
        }
    }
}

/// Recent `die` timestamps per container name.
#[derive(Debug, Default)]
pub struct DieTracker {
    by_name: HashMap<String, Vec<u64>>,
}

impl DieTracker {
    pub fn record(&mut self, name: &str, time: u64) {
        self.by_name.entry(name.to_string()).or_default().push(time);
    }

    pub fn prune(&mut self, now: u64) {
        let cutoff = now.saturating_sub(DIE_WINDOW_SECS);
        self.by_name.retain(|_, v| {
            v.retain(|&t| t >= cutoff);
            !v.is_empty()
        });
    }

    /// Fill `die_times` on each container.
    pub fn apply(&mut self, containers: &mut [Container], now: u64) {
        self.prune(now);
        for c in containers {
            c.die_times = self.by_name.get(&c.name).cloned().unwrap_or_default();
        }
    }
}

// ---------------------------------------------------------------------------
// Background task

/// Background task: keeps `state.containers` fresh. Lists once at start,
/// follows `GET /events` (push, near-zero cost) and re-lists on container
/// events (debounced), plus a slow safety re-list. Reconnects with backoff.
/// Calls `state.publish_containers(..)` with each new snapshot.
pub async fn run(state: Arc<AppState>) {
    let mut w = Watcher {
        state,
        dies: DieTracker::default(),
        last: Vec::new(),
        last_error: None,
    };
    let mut backoff = BACKOFF_MIN;
    loop {
        if !w.state.config.current().docker.enabled {
            if !w.last.is_empty() || w.last_error.is_some() || w.state.snapshot().fetched_at == 0 {
                w.last.clear();
                w.last_error = None;
                w.state.publish_containers(Snapshot {
                    containers: vec![],
                    fetched_at: crate::util::unix_now(),
                    error: None,
                });
            }
            tokio::time::sleep(SAFETY_RELIST).await;
            continue;
        }
        let err = w.session(&mut backoff).await;
        if w.last_error.as_deref() != Some(err.as_str()) {
            crate::warn!("docker unavailable, retrying with backoff", error = err);
        }
        w.publish_error(err);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

struct Watcher {
    state: Arc<AppState>,
    dies: DieTracker,
    last: Vec<Container>,
    last_error: Option<String>,
}

impl Watcher {
    fn socket(&self) -> String {
        self.state.env.docker_socket.clone()
    }

    /// One connected session. Returns the error that ended it.
    async fn session(&mut self, backoff: &mut Duration) -> String {
        // Subscribe before listing so nothing between the two is missed.
        let path = format!("/events?filters={}", events_filter());
        let resp = match tokio::time::timeout(REQUEST_TIMEOUT, get(&self.socket(), &path)).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return e,
            Err(_) => return "docker: events request timed out".into(),
        };
        if let Err(e) = self.relist().await {
            return e;
        }
        *backoff = BACKOFF_MIN;
        let mut body = resp.into_body();
        let mut lines = LineBuffer::default();
        let mut dirty_since: Option<Instant> = None;
        let mut next_safety = Instant::now() + SAFETY_RELIST;
        loop {
            let deadline = match dirty_since {
                Some(t) => (t + DEBOUNCE).min(next_safety),
                None => next_safety,
            };
            tokio::select! {
                frame = body.frame() => {
                    let data: Bytes = match frame {
                        None => return "docker: events stream closed".into(),
                        Some(Err(e)) => return format!("docker: events stream: {e}"),
                        Some(Ok(f)) => match f.into_data() {
                            Ok(d) => d,
                            Err(_) => continue,
                        },
                    };
                    let mut relevant = false;
                    let dies = &mut self.dies;
                    lines.push(&data, |line| {
                        if let Some(ev) = parse_event(line) {
                            crate::db::record_docker_event(&self.state.db, &ev);
                            if ev.action == "die" {
                                if let Some(n) = &ev.name {
                                    dies.record(n, if ev.time > 0 { ev.time } else { crate::util::unix_now() });
                                }
                            }
                            relevant = true;
                        }
                    });
                    if relevant && dirty_since.is_none() {
                        dirty_since = Some(Instant::now());
                    }
                }
                _ = tokio::time::sleep_until(deadline.into()) => {
                    if !self.state.config.current().docker.enabled {
                        return "docker: disabled in config".into();
                    }
                    dirty_since = None;
                    next_safety = Instant::now() + SAFETY_RELIST;
                    if let Err(e) = self.relist().await {
                        return e;
                    }
                }
            }
        }
    }

    async fn relist(&mut self) -> Result<(), String> {
        let mut containers = list_containers(&self.socket()).await?;
        let now = crate::util::unix_now();
        self.dies.apply(&mut containers, now);
        if containers != self.last
            || self.last_error.is_some()
            || self.state.snapshot().fetched_at == 0
        {
            if self.last_error.take().is_some() || self.last.is_empty() {
                crate::info!("docker connected", containers = containers.len());
            }
            self.last = containers.clone();
            self.state.publish_containers(Snapshot {
                containers,
                fetched_at: now,
                error: None,
            });
        }
        Ok(())
    }

    fn publish_error(&mut self, err: String) {
        if self.last_error.as_deref() == Some(err.as_str()) {
            return;
        }
        self.last_error = Some(err.clone());
        let fetched_at = self.state.snapshot().fetched_at;
        self.state.publish_containers(Snapshot {
            containers: self.last.clone(),
            fetched_at,
            error: Some(err),
        });
    }
}

/// URL-encoded `{"type":["container"],"event":[...]}`. The server-side event
/// filter keeps healthcheck `exec_*` noise off the connection entirely.
fn events_filter() -> String {
    let json = serde_json::json!({ "type": ["container"], "event": EVENTS }).to_string();
    let mut out = String::with_capacity(json.len() * 3);
    for b in json.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// cgroup v2 stats

/// Per-container CPU% and memory from cgroup v2 files under `RH_HOST_SYS`
/// (`fs/cgroup/system.slice/docker-<id>.scope/`). Keeps previous CPU counters
/// to compute rates. Only called while someone is viewing.
#[derive(Debug, Default)]
pub struct StatsSampler {
    root: PathBuf,
    /// id → (usage_usec, when read).
    prev: HashMap<String, (u64, Instant)>,
}

impl StatsSampler {
    pub fn new(host_sys: &str) -> Self {
        Self {
            root: Path::new(host_sys).join("fs/cgroup"),
            prev: HashMap::new(),
        }
    }

    /// Sample every running container. A container's first sample only
    /// primes the CPU counter and is left out of the result (no fake 0%).
    pub fn sample(&mut self, containers: &[Container]) -> StatsMap {
        self.sample_at(containers, Instant::now())
    }

    fn sample_at(&mut self, containers: &[Container], now: Instant) -> StatsMap {
        let mut out = StatsMap::new();
        let mut seen: HashMap<String, (u64, Instant)> = HashMap::new();
        for c in containers.iter().filter(|c| c.is_running()) {
            let Some(dir) = self.cgroup_dir(&c.id) else {
                continue;
            };
            let Some(usage) = read_cpu_usage(&dir) else {
                continue;
            };
            let mem = read_memory(&dir);
            let entry = match self.prev.get(&c.id) {
                Some(&(prev_usage, prev_t)) if usage >= prev_usage => {
                    let wall = now.saturating_duration_since(prev_t).as_micros() as f64;
                    if wall < 100_000.0 {
                        // Sampled too soon after the previous read: keep the
                        // older baseline for a meaningful rate next time.
                        seen.insert(c.id.clone(), (prev_usage, prev_t));
                        continue;
                    }
                    Some(ContainerStats {
                        cpu_percent: (usage - prev_usage) as f64 / wall * 100.0,
                        mem_bytes: mem.unwrap_or(0),
                    })
                }
                _ => None,
            };
            seen.insert(c.id.clone(), (usage, now));
            if let Some(s) = entry {
                out.insert(c.id.clone(), s);
            }
        }
        self.prev = seen;
        out
    }

    fn cgroup_dir(&self, id: &str) -> Option<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        // systemd cgroup driver, then the cgroupfs driver layout.
        let systemd = self
            .root
            .join("system.slice")
            .join(format!("docker-{id}.scope"));
        if systemd.is_dir() {
            return Some(systemd);
        }
        let cgroupfs = self.root.join("docker").join(id);
        cgroupfs.is_dir().then_some(cgroupfs)
    }
}

fn read_cpu_usage(dir: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(dir.join("cpu.stat")).ok()?;
    stat_value(&text, "usage_usec")
}

/// `memory.current` minus `inactive_file`, which is what `docker stats` shows.
fn read_memory(dir: &Path) -> Option<u64> {
    let current: u64 = std::fs::read_to_string(dir.join("memory.current"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let inactive = std::fs::read_to_string(dir.join("memory.stat"))
        .ok()
        .and_then(|t| stat_value(&t, "inactive_file"))
        .unwrap_or(0);
    Some(current.saturating_sub(inactive))
}

/// Value of `key` in a flat-keyed cgroup file (`key value` per line).
fn stat_value(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(' ')?;
        (k == key).then(|| v.trim().parse().ok())?
    })
}

// ---------------------------------------------------------------------------
// Container detail: inspect, top, image digests (GET only)
//
// A tiny blocking HTTP/1.0 client over the socket: the detail page renders
// on the blocking pool and the sampler refreshes a PID only when a container
// changed, so an async round trip buys nothing. HTTP/1.0 makes Docker answer
// without chunking and close the connection; chunked bodies are decoded
// anyway, in case a proxy sits in front of the socket.

/// Largest inspect / top body accepted.
const MAX_DETAIL_BODY: usize = 4 * 1024 * 1024;
const DETAIL_TIMEOUT: Duration = Duration::from_secs(3);

/// `GET path` on the Docker socket, blocking. Only ever a GET.
pub fn get_blocking(socket: &str, path: &str) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    if !path.starts_with('/') || path.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err("bad request path".into());
    }
    let mut s = std::os::unix::net::UnixStream::connect(socket)
        .map_err(|e| format!("cannot connect to {socket}: {e}"))?;
    let _ = s.set_read_timeout(Some(DETAIL_TIMEOUT));
    let _ = s.set_write_timeout(Some(DETAIL_TIMEOUT));
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: docker\r\n\r\n").as_bytes())
        .map_err(|e| format!("docker: {e}"))?;
    let mut buf = Vec::new();
    s.take(MAX_DETAIL_BODY as u64 + 64 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("docker: {e}"))?;
    let (status, body) = split_response(&buf)?;
    if !(200..300).contains(&status) {
        let msg = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("message")?.as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
        return Err(format!("docker returned {status}: {msg}"));
    }
    Ok(body)
}

/// Status code and (de-chunked) body of a raw HTTP/1.x response.
fn split_response(buf: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("docker: truncated response")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("docker: bad status line")?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &buf[end + 4..];
    if !chunked {
        return Ok((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let nl = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("docker: bad chunk")?;
        let size_txt = String::from_utf8_lossy(&rest[..nl]);
        let size = usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "docker: bad chunk size")?;
        rest = &rest[nl + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size {
            return Err("docker: truncated chunk".into());
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or(&[]);
    }
    Ok((status, out))
}

/// The parts of `GET /containers/{id}/json` the detail page shows.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Inspect {
    pub id: String,
    pub name: String,
    pub created: String,
    /// Image id (`sha256:…`).
    pub image: String,
    pub restart_count: u64,
    pub config: InspectConfig,
    pub state: InspectState,
    pub host_config: InspectHost,
    pub network_settings: InspectNet,
    pub mounts: Vec<InspectMount>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectConfig {
    pub image: String,
    pub cmd: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub working_dir: String,
    pub user: String,
    /// `KEY=value`. Values are secrets: never render them into a page.
    pub env: Option<Vec<String>>,
    pub healthcheck: Option<HealthConfig>,
}

impl InspectConfig {
    /// Environment keys only, in order.
    pub fn env_keys(&self) -> Vec<&str> {
        self.env
            .iter()
            .flatten()
            .map(|e| e.split_once('=').map_or(e.as_str(), |(k, _)| k))
            .collect()
    }

    /// `(key, value)` pairs. Only for a signed-in viewer.
    pub fn env_pairs(&self) -> Vec<(&str, &str)> {
        self.env
            .iter()
            .flatten()
            .map(|e| e.split_once('=').unwrap_or((e.as_str(), "")))
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct HealthConfig {
    pub test: Option<Vec<String>>,
    /// Nanoseconds (0 = Docker's default).
    pub interval: i64,
    pub timeout: i64,
    pub start_period: i64,
    pub retries: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectState {
    pub status: String,
    pub running: bool,
    pub restarting: bool,
    #[serde(rename = "OOMKilled")]
    pub oom_killed: bool,
    pub dead: bool,
    pub pid: i64,
    pub exit_code: i64,
    pub error: String,
    pub started_at: String,
    pub finished_at: String,
    pub health: Option<InspectHealth>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectHealth {
    pub status: String,
    pub failing_streak: u64,
    pub log: Option<Vec<HealthLog>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct HealthLog {
    pub start: String,
    pub end: String,
    pub exit_code: i64,
    pub output: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectHost {
    pub network_mode: String,
    pub restart_policy: Option<RestartPolicy>,
    /// Bytes; 0 = unlimited.
    pub memory: i64,
    pub memory_reservation: i64,
    pub memory_swap: i64,
    pub nano_cpus: i64,
    pub cpu_shares: i64,
    pub cpu_quota: i64,
    pub cpu_period: i64,
    pub cpuset_cpus: String,
    pub pids_limit: Option<i64>,
    pub privileged: bool,
    pub readonly_rootfs: bool,
    pub cap_add: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RestartPolicy {
    pub name: String,
    pub maximum_retry_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectNet {
    pub networks: Option<BTreeMap<String, InspectEndpoint>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectEndpoint {
    #[serde(rename = "IPAddress")]
    pub ip_address: String,
    #[serde(rename = "GlobalIPv6Address")]
    pub ipv6_address: String,
    pub gateway: String,
    pub mac_address: String,
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InspectMount {
    #[serde(rename = "Type")]
    pub kind: String,
    pub name: String,
    pub source: String,
    pub destination: String,
    pub mode: String,
    #[serde(rename = "RW")]
    pub rw: bool,
}

pub fn parse_inspect(body: &[u8]) -> Result<Inspect, String> {
    serde_json::from_slice(body).map_err(|e| format!("docker: bad inspect: {e}"))
}

fn valid_ref(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// `GET /containers/{id}/json`.
pub fn inspect_blocking(socket: &str, id: &str) -> Result<Inspect, String> {
    if !valid_ref(id) {
        return Err("bad container id".into());
    }
    parse_inspect(&get_blocking(socket, &format!("/containers/{id}/json"))?)
}

/// `RepoDigests` of an image (`GET /images/{id}/json`).
pub fn image_digests_blocking(socket: &str, image_id: &str) -> Result<Vec<String>, String> {
    if !valid_ref(image_id) {
        return Err("bad image id".into());
    }
    let body = get_blocking(socket, &format!("/images/{image_id}/json"))?;
    let v: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("docker: bad image: {e}"))?;
    Ok(v.get("RepoDigests")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// `GET /containers/{id}/top`: column titles and rows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Top {
    pub titles: Vec<String>,
    pub processes: Vec<Vec<String>>,
}

pub fn parse_top(body: &[u8]) -> Result<Top, String> {
    let t: Top =
        serde_json::from_slice(body).map_err(|e| format!("docker: bad process list: {e}"))?;
    Ok(t)
}

/// Columns asked for first; Docker runs `ps` on the host with them. When the
/// host `ps` refuses (busybox), the default columns are used instead.
pub const TOP_ARGS: &str = "-eo pid,user,%cpu,%mem,rss,etime,args";

pub fn top_blocking(socket: &str, id: &str) -> Result<Top, String> {
    if !valid_ref(id) {
        return Err("bad container id".into());
    }
    let q = TOP_ARGS
        .bytes()
        .map(|b| match b {
            b' ' => "%20".to_string(),
            b'%' => "%25".to_string(),
            b',' => "%2C".to_string(),
            b => (b as char).to_string(),
        })
        .collect::<String>();
    match get_blocking(socket, &format!("/containers/{id}/top?ps_args={q}")) {
        Ok(b) => parse_top(&b),
        Err(_) => parse_top(&get_blocking(socket, &format!("/containers/{id}/top"))?),
    }
}

/// RFC 3339 (as Docker writes it) to unix seconds. `None` for Docker's
/// zero time (`0001-01-01T00:00:00Z`) and anything unparsable.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if y < 1971 || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    // Skip fractional seconds, then the zone.
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let off = match b.get(i)? {
        b'Z' | b'z' => 0,
        c @ (b'+' | b'-') => {
            let oh: i64 = s.get(i + 1..i + 3)?.parse().ok()?;
            let om: i64 = s.get(i + 4..i + 6)?.parse().ok()?;
            let v = oh * 3600 + om * 60;
            if *c == b'+' {
                v
            } else {
                -v
            }
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + se - off)
}

/// The cgroup v2 directory of a container under `host_sys` (systemd or
/// cgroupfs driver layout), if it exists.
pub fn container_cgroup_dir(host_sys: &str, id: &str) -> Option<PathBuf> {
    StatsSampler::new(host_sys).cgroup_dir(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed but otherwise verbatim output of Docker 29.5 on the dev host,
    /// plus a stopped container and an unhealthy one.
    const SAMPLE: &str = r#"[
 {"Id":"1dcb32aee455d50d91c4ddd92278db4eb058654511ae22478f8f957893bcafbd","Names":["/rmli-hotel-portal-mailpit-1"],
  "Image":"axllent/mailpit:v1.31.6","ImageID":"sha256:aa","Command":"/mailpit","Created":1794189099,
  "Ports":[{"IP":"","PrivatePort":1110,"Type":"tcp"},{"IP":"100.64.0.10","PrivatePort":8025,"PublicPort":8125,"Type":"tcp"},
           {"IP":"127.0.0.1","PrivatePort":8025,"PublicPort":8125,"Type":"tcp"},{"IP":"","PrivatePort":1025,"Type":"tcp"}],
  "Labels":{"com.docker.compose.project":"rmli-hotel-portal","com.docker.compose.service":"mailpit","com.docker.compose.oneoff":"False"},
  "State":"running","Status":"Up 5 days (healthy)","HostConfig":{"NetworkMode":"rmli_default"},
  "Health":{"Status":"healthy","FailingStreak":0},"NetworkSettings":{"Networks":{}},"Mounts":[]},
 {"Id":"00fc7aedfce035c9220da998238c1383336dc37e89b691869432c0dc9a9fccdd","Names":["/homepage"],
  "Image":"ghcr.io/gethomepage/homepage:latest","Created":1793056864,
  "Ports":[{"IP":"0.0.0.0","PrivatePort":3000,"PublicPort":3000,"Type":"tcp"},{"IP":"::","PrivatePort":3000,"PublicPort":3000,"Type":"tcp"}],
  "Labels":{},"State":"running","Status":"Up 5 days (healthy)"},
 {"Id":"85577b59c2ab78e151cf9c7cbc25e01ef517e37357baf35a4cc163ee01efcb0a","Names":["/webshop-dev-odoo20-1"],
  "Image":"odoo:20.0","Created":1793881335,
  "Ports":[{"IP":"","PrivatePort":8072,"Type":"tcp"},{"IP":"100.64.0.10","PrivatePort":8069,"PublicPort":8070,"Type":"tcp"},
           {"IP":"127.0.0.1","PrivatePort":8069,"PublicPort":8070,"Type":"tcp"}],
  "Labels":{"com.docker.compose.project":"webshop-dev","com.docker.compose.service":"odoo20"},
  "State":"running","Status":"Up 9 days","Health":{"Status":"none","FailingStreak":0}},
 {"Id":"aaaa","Names":["/old-job"],"Image":"busybox","Created":1700000000,"Ports":[],"Labels":null,
  "State":"exited","Status":"Exited (137) 3 hours ago"},
 {"Id":"bbbb","Names":["/flaky"],"Image":"nginx","Created":1700000001,"Ports":null,
  "Labels":{"hobnail.name":"Flaky"},"State":"running","Status":"Up 2 minutes (unhealthy)"},
 {"Id":"cccc","Names":["/booting"],"Image":"nginx","Created":1700000002,
  "State":"running","Status":"Up 3 seconds (health: starting)"}
]"#;

    fn by_name<'a>(cs: &'a [Container], n: &str) -> &'a Container {
        cs.iter().find(|c| c.name == n).unwrap()
    }

    #[test]
    fn parses_container_list() {
        let cs = parse_containers(SAMPLE.as_bytes()).unwrap();
        assert_eq!(cs.len(), 6);
        let m = by_name(&cs, "rmli-hotel-portal-mailpit-1");
        assert_eq!(m.project.as_deref(), Some("rmli-hotel-portal"));
        assert_eq!(m.service.as_deref(), Some("mailpit"));
        assert_eq!(m.health.as_deref(), Some("healthy"));
        assert_eq!(m.exit_code, None);
        assert!(m.is_running());
        assert_eq!(m.created, 1794189099);
        assert_eq!(m.image, "axllent/mailpit:v1.31.6");
        // Unpublished ports skipped; two bindings of 8025 kept, sorted.
        assert_eq!(
            m.ports,
            vec![
                PortMapping {
                    host_ip: "100.64.0.10".into(),
                    host_port: 8125,
                    container_port: 8025,
                    proto: "tcp".into()
                },
                PortMapping {
                    host_ip: "127.0.0.1".into(),
                    host_port: 8125,
                    container_port: 8025,
                    proto: "tcp".into()
                },
            ]
        );
        assert_eq!(m.labels["com.docker.compose.oneoff"], "False");
    }

    #[test]
    fn dedupes_v4_v6_any() {
        let cs = parse_containers(SAMPLE.as_bytes()).unwrap();
        let h = by_name(&cs, "homepage");
        assert_eq!(h.ports.len(), 1);
        assert_eq!(h.ports[0].host_ip, "0.0.0.0");
        assert_eq!(h.project, None);
    }

    #[test]
    fn ipv6_only_any_is_kept() {
        let body = br#"[{"Id":"x","Names":["/v6"],"Ports":[{"IP":"::","PrivatePort":80,"PublicPort":8080,"Type":"tcp"}],"State":"running","Status":"Up"}]"#;
        let cs = parse_containers(body).unwrap();
        assert_eq!(cs[0].ports[0].host_ip, "::");
    }

    #[test]
    fn stopped_unhealthy_starting() {
        let cs = parse_containers(SAMPLE.as_bytes()).unwrap();
        let o = by_name(&cs, "old-job");
        assert_eq!(o.state, "exited");
        assert_eq!(o.exit_code, Some(137));
        assert!(!o.is_running());
        assert!(o.labels.is_empty());
        assert_eq!(by_name(&cs, "flaky").health.as_deref(), Some("unhealthy"));
        assert_eq!(by_name(&cs, "booting").health.as_deref(), Some("starting"));
        // "none" in the Health field means no healthcheck.
        assert_eq!(by_name(&cs, "webshop-dev-odoo20-1").health, None);
    }

    #[test]
    fn health_and_exit_code_from_status() {
        assert_eq!(
            parse_health("Up 3 hours (healthy)").as_deref(),
            Some("healthy")
        );
        assert_eq!(
            parse_health("Up 3 hours (unhealthy)").as_deref(),
            Some("unhealthy")
        );
        assert_eq!(
            parse_health("Up 1 second (health: starting)").as_deref(),
            Some("starting")
        );
        assert_eq!(parse_health("Up 3 hours"), None);
        assert_eq!(parse_exit_code("Exited (0) 5 minutes ago"), Some(0));
        assert_eq!(parse_exit_code("Exited (137) 3 hours ago"), Some(137));
        assert_eq!(parse_exit_code("Restarting (1) 2 seconds ago"), Some(1));
        assert_eq!(parse_exit_code("Up 3 hours"), None);
        assert_eq!(parse_exit_code("Created"), None);
    }

    #[test]
    fn bad_json_is_an_error() {
        assert!(parse_containers(b"{\"message\":\"x\"}").is_err());
        assert!(parse_containers(b"[]").unwrap().is_empty());
    }

    const EV_DIE: &str = r#"{"status":"die","id":"85577b59c2ab","from":"odoo:20.0","Type":"container","Action":"die","Actor":{"ID":"85577b59c2ab","Attributes":{"exitCode":"137","image":"odoo:20.0","name":"webshop-dev-odoo20-1"}},"scope":"local","time":1794650000,"timeNano":1794650000123456789}"#;

    #[test]
    fn parses_events() {
        let e = parse_event(EV_DIE.as_bytes()).unwrap();
        assert_eq!(e.action, "die");
        assert_eq!(e.name.as_deref(), Some("webshop-dev-odoo20-1"));
        assert_eq!(e.time, 1794650000);
        assert_eq!(e.detail.as_deref(), Some("137"));
        let h = r#"{"Type":"container","Action":"health_status: unhealthy","Actor":{"ID":"x","Attributes":{"name":"web"}},"time":5}"#;
        let he = parse_event(h.as_bytes()).unwrap();
        assert_eq!(he.action, "health_status");
        assert_eq!(he.detail.as_deref(), Some("unhealthy"));
        let exec = r#"{"Type":"container","Action":"exec_start: /bin/sh -c curl","Actor":{"ID":"x","Attributes":{"name":"web"}},"time":5}"#;
        assert!(parse_event(exec.as_bytes()).is_none());
        let net = r#"{"Type":"network","Action":"connect","Actor":{"ID":"n"},"time":5}"#;
        assert!(parse_event(net.as_bytes()).is_none());
        assert!(parse_event(b"not json").is_none());
    }

    #[test]
    fn line_buffer_handles_split_frames() {
        let stream = format!("{EV_DIE}\n{EV_DIE}\n");
        let bytes = stream.as_bytes();
        let mut lb = LineBuffer::default();
        let mut got = Vec::new();
        // Feed in awkward chunk sizes.
        for chunk in bytes.chunks(37) {
            lb.push(chunk, |l| got.push(parse_event(l).unwrap()));
        }
        assert_eq!(got.len(), 2);
        assert!(lb.buf.is_empty());
        // Two objects in one frame, then a partial line left over.
        let mut n = 0;
        lb.push(format!("{EV_DIE}\n\n{EV_DIE}\n{{\"Ty").as_bytes(), |_| {
            n += 1
        });
        assert_eq!(n, 2);
        assert_eq!(lb.buf, b"{\"Ty");
    }

    #[test]
    fn die_tracker_keeps_last_hour() {
        let mut t = DieTracker::default();
        t.record("a", 1000);
        t.record("a", 5000);
        t.record("b", 4000);
        let mut cs = vec![
            Container {
                name: "a".into(),
                ..Default::default()
            },
            Container {
                name: "b".into(),
                ..Default::default()
            },
        ];
        t.apply(&mut cs, 5000 + 100);
        assert_eq!(cs[0].die_times, vec![5000]);
        assert_eq!(cs[1].die_times, vec![4000]);
        t.apply(&mut cs, 4000 + DIE_WINDOW_SECS + 1);
        assert_eq!(cs[0].die_times, vec![5000]);
        assert!(cs[1].die_times.is_empty());
        assert!(!t.by_name.contains_key("b"));
    }

    #[test]
    fn events_filter_is_urlencoded() {
        let f = events_filter();
        assert!(f.contains("%22type%22%3A%5B%22container%22%5D"), "{f}");
        assert!(f.contains("%22health_status%22"), "{f}");
        assert!(!f.contains('"') && !f.contains('{') && !f.contains(' '));
    }

    // -- cgroup stats --

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "rh-docker-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_cg(root: &Path, id: &str, usage: u64, current: u64, inactive: u64) {
        let d = root.join(format!("fs/cgroup/system.slice/docker-{id}.scope"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("cpu.stat"),
            format!("usage_usec {usage}\nuser_usec 1\nsystem_usec 2\nnr_periods 0\n"),
        )
        .unwrap();
        std::fs::write(d.join("memory.current"), format!("{current}\n")).unwrap();
        std::fs::write(
            d.join("memory.stat"),
            format!(
                "anon 20525056\nfile 13135872\ninactive_file {inactive}\nactive_file 8556544\n"
            ),
        )
        .unwrap();
    }

    fn running(id: &str) -> Container {
        Container {
            id: id.into(),
            name: id.into(),
            state: "running".into(),
            ..Default::default()
        }
    }

    #[test]
    fn stat_parsing() {
        let t = "usage_usec 111133252\nuser_usec 73369295\n";
        assert_eq!(stat_value(t, "usage_usec"), Some(111133252));
        assert_eq!(stat_value(t, "user_usec"), Some(73369295));
        assert_eq!(stat_value(t, "usage"), None);
    }

    #[test]
    fn sampler_rates_and_memory() {
        let tmp = TempDir::new("stats");
        let root = tmp.0.to_str().unwrap();
        let (a, b) = ("aa11", "bb22");
        write_cg(&tmp.0, a, 1_000_000, 34_881_536, 4_583_424);
        write_cg(&tmp.0, b, 500, 1000, 2000);
        let mut s = StatsSampler::new(root);
        let t0 = Instant::now();
        let cs = vec![running(a), running(b)];
        // First sample primes counters only.
        assert!(s.sample_at(&cs, t0).is_empty());
        // +0.5 s of CPU over 2 s wall = 25% of a core.
        write_cg(&tmp.0, a, 1_500_000, 34_881_536, 4_583_424);
        let m = s.sample_at(&cs, t0 + Duration::from_secs(2));
        let sa = m[a];
        assert!((sa.cpu_percent - 25.0).abs() < 1e-9, "{}", sa.cpu_percent);
        assert_eq!(sa.mem_bytes, 34_881_536 - 4_583_424);
        // inactive_file larger than current must not underflow.
        assert_eq!(m[b].mem_bytes, 0);
        assert_eq!(m[b].cpu_percent, 0.0);
    }

    #[test]
    fn sampler_skips_missing_and_drops_gone() {
        let tmp = TempDir::new("gone");
        let root = tmp.0.to_str().unwrap();
        write_cg(&tmp.0, "aa11", 10, 100, 0);
        let mut s = StatsSampler::new(root);
        let t0 = Instant::now();
        let mut stopped = running("cc33");
        stopped.state = "exited".into();
        let cs = vec![running("aa11"), running("dd44"), stopped, running("../x")];
        assert!(s.sample_at(&cs, t0).is_empty());
        assert_eq!(s.prev.len(), 1);
        // Container disappears: its counters are dropped.
        assert!(s.sample_at(&[], t0 + Duration::from_secs(1)).is_empty());
        assert!(s.prev.is_empty());
        // Comes back: primes again rather than computing against stale data.
        assert!(s
            .sample_at(&cs[..1], t0 + Duration::from_secs(2))
            .is_empty());
    }

    #[test]
    fn sampler_counter_reset_reprimes() {
        let tmp = TempDir::new("reset");
        let root = tmp.0.to_str().unwrap();
        write_cg(&tmp.0, "aa11", 1_000_000, 100, 0);
        let mut s = StatsSampler::new(root);
        let t0 = Instant::now();
        let cs = vec![running("aa11")];
        s.sample_at(&cs, t0);
        write_cg(&tmp.0, "aa11", 10, 100, 0);
        assert!(s.sample_at(&cs, t0 + Duration::from_secs(1)).is_empty());
        write_cg(&tmp.0, "aa11", 100_010, 100, 0);
        let m = s.sample_at(&cs, t0 + Duration::from_secs(2));
        assert!((m["aa11"].cpu_percent - 10.0).abs() < 1e-9);
    }

    #[test]
    fn cgroupfs_layout_fallback() {
        let tmp = TempDir::new("cgfs");
        let d = tmp.0.join("fs/cgroup/docker/ee55");
        std::fs::create_dir_all(&d).unwrap();
        let s = StatsSampler::new(tmp.0.to_str().unwrap());
        assert_eq!(s.cgroup_dir("ee55"), Some(d));
        assert_eq!(s.cgroup_dir("ff66"), None);
    }
}
