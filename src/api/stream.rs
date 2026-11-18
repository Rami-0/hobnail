//! Signed-in server-sent events for the React client.
//!
//! - `hello` [`Hello`] first, then `system` once when requested and available.
//! - `containers` [`ContainersView`] when container metadata or state changes.
//! - `stats` [`StatsEvent`] at the container stats refresh interval.
//! - `system` [`SystemView`] on host samples.
//! - `boards` [`super::boards::BoardEvent`] for boards the current account can see.
//! - `config {}` on configuration changes and `resync {}` after a lagged subscription.
//!
//! Unknown topics are ignored. No recognized topics means all topics. The
//! connection counts as a viewer while its body is alive, and a `: ping`
//! comment every 25 seconds keeps proxies open.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Frame;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use serde::Serialize;
use std::collections::BTreeMap;
use tokio::sync::{broadcast, mpsc};

use super::containers::ContainersView;
use crate::http::server::{full, set, sse_event, Body};
use crate::state::{AppState, ViewerGuard};

/// Messages queued per connection before the sender waits.
const QUEUE: usize = 16;
const PING: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Hello {
    /// Build SHA; a client built from another one offers a reload.
    pub sha: String,
    /// Hash of the build inputs.
    pub content: String,
    pub api: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StatsEvent {
    /// Unix seconds.
    pub at: i64,
    /// Listed containers only.
    pub containers: BTreeMap<String, StatsRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StatsRow {
    pub cpu_pct: Option<f64>,
    pub mem_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SystemView {
    /// Unix seconds of the sample.
    pub at: u64,
    /// 0..=100 across all cores; null until two samples exist.
    pub cpu_pct: Option<f64>,
    pub cores: u32,
    pub load: [f64; 3],
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskRow>,
    /// Bytes per second; null on the first sample.
    pub net_rx_bps: Option<f64>,
    pub net_tx_bps: Option<f64>,
    pub temp_c: Option<f64>,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiskRow {
    pub label: String,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
}

/// Which topics a connection asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topics {
    pub containers: bool,
    pub stats: bool,
    pub system: bool,
    pub boards: bool,
    pub widget: bool,
    pub alerts: bool,
}

impl Topics {
    pub const ALL: Topics = Topics {
        containers: true,
        stats: true,
        system: true,
        boards: true,
        widget: false,
        alerts: false,
    };

    pub(super) fn requested(query: &str) -> Topics {
        let q = crate::ui::fmt::parse_query(query);
        let mut t = Topics {
            containers: false,
            stats: false,
            system: false,
            boards: false,
            widget: false,
            alerts: false,
        };
        for v in q.iter().filter(|(k, _)| k == "topics").map(|(_, v)| v) {
            for name in v.split(',').map(str::trim) {
                match name {
                    "containers" => t.containers = true,
                    "stats" => t.stats = true,
                    "system" => t.system = true,
                    "boards" => t.boards = true,
                    "widget" => t.widget = true,
                    "alerts" => t.alerts = true,
                    _ => {}
                }
            }
        }
        t
    }

    pub fn parse(query: &str) -> Topics {
        let t = Self::requested(query);
        if t.containers || t.stats || t.system || t.boards || t.widget || t.alerts {
            t
        } else {
            Topics::ALL
        }
    }
}

pub fn hello(state: &AppState) -> Hello {
    Hello {
        sha: state.build.sha.to_string(),
        content: super::CONTENT_HASH.to_string(),
        api: super::session::API_VERSION,
    }
}

pub fn stats_event(v: &ContainersView, at: i64) -> StatsEvent {
    StatsEvent {
        at,
        containers: v
            .groups
            .iter()
            .flat_map(|g| &g.containers)
            .map(|r| {
                (
                    r.name.clone(),
                    StatsRow {
                        cpu_pct: r.cpu_pct,
                        mem_bytes: r.mem_bytes,
                    },
                )
            })
            .collect(),
    }
}

/// The current host sample (CPU seeded from history before the sampler has
/// two readings), or `None` before the first sample.
pub fn system_view(state: &AppState) -> Option<SystemView> {
    let mut s = state.system.lock().unwrap().1.clone();
    crate::ui::view::seed_cpu(state, &mut s, crate::util::unix_now() as i64);
    s.map(|s| SystemView {
        at: s.sampled_at,
        cpu_pct: s.cpu_percent,
        cores: s.cpu_cores,
        load: s.load,
        mem_total: s.mem_total,
        mem_used: s.mem_used,
        swap_total: s.swap_total,
        swap_used: s.swap_used,
        disks: s
            .disks
            .into_iter()
            .map(|d| DiskRow {
                label: d.label,
                total: d.total,
                used: d.used,
                avail: d.avail,
            })
            .collect(),
        net_rx_bps: s.net_rx_bps,
        net_tx_bps: s.net_tx_bps,
        temp_c: s.temp_c,
        uptime_secs: s.uptime_secs,
    })
}

/// What decides a `containers` event: the view without the numbers and
/// clocks, plus Docker's status text of each listed container (so a
/// restart between two polls, which changes "Up 2 hours" to "Up 3
/// seconds", is a change too).
pub fn signature(v: &ContainersView, snap: &crate::model::Snapshot) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut v = v.clone();
    v.fetched_at = 0;
    v.now = 0;
    for r in v.groups.iter_mut().flat_map(|g| g.containers.iter_mut()) {
        r.cpu_pct = r.cpu_pct.map(|_| 0.0);
        r.mem_bytes = r.mem_bytes.map(|_| 0);
        r.started_at = None;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&v).unwrap_or_default().hash(&mut h);
    for r in v.groups.iter().flat_map(|g| &g.containers) {
        if let Some(c) = snap.containers.iter().find(|c| c.name == r.name) {
            c.status.hash(&mut h);
        }
    }
    h.finish()
}

fn event(name: &str, v: &impl Serialize) -> Bytes {
    sse_event(
        name,
        &serde_json::to_string(v).unwrap_or_else(|_| "{}".into()),
    )
}

/// SSE body backed by a channel, holding the viewer guard for its lifetime.
struct StreamBody {
    rx: mpsc::Receiver<Bytes>,
    _guard: ViewerGuard,
}

impl hyper::body::Body for StreamBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.rx.poll_recv(cx).map(|o| o.map(|b| Ok(Frame::data(b))))
    }
}

/// The current list and its signature.
fn current(state: &AppState, account: &super::access::Principal) -> (ContainersView, u64) {
    let now = crate::util::unix_now() as i64;
    let v = account
        .container_visibility(state)
        .filter_view(super::containers::current_view(state, now));
    let sig = signature(&v, &state.snapshot());
    (v, sig)
}

async fn run(
    st: Arc<AppState>,
    topics: Topics,
    session_hash: String,
    mut sub: broadcast::Receiver<Arc<str>>,
    tx: mpsc::Sender<Bytes>,
) {
    if tx.send(event("hello", &hello(&st))).await.is_err() {
        return;
    }
    let list = topics.containers || topics.stats;
    let Some(account) = st.db.account_session(&session_hash, crate::auth::now_s()) else {
        return;
    };
    let mut sig = list.then(|| current(&st, &account).1);
    if topics.system && !account.is_scoped() {
        if let Some(s) = system_view(&st) {
            if tx.send(event("system", &s)).await.is_err() {
                return;
            }
        }
    }
    let mut ping = tokio::time::interval(PING);
    ping.tick().await;
    loop {
        let msg = tokio::select! {
            m = sub.recv() => match m {
                Ok(id) => {
                    let Some(account) = st.db.account_session(&session_hash, crate::auth::now_s()) else { break };
                    match &*id {
                    "groups" if list => {
                        let (v, s) = current(&st, &account);
                        if topics.containers && sig != Some(s) {
                            sig = Some(s);
                            event("containers", &v)
                        } else if topics.stats {
                            event("stats", &stats_event(&v, v.now))
                        } else {
                            continue;
                        }
                    }
                    "sys" if topics.system && !account.is_scoped() => match system_view(&st) {
                        Some(s) => event("system", &s),
                        None => continue,
                    },
                    "reload" => {
                        // The client refetches the list.
                        if list {
                            sig = Some(current(&st, &account).1);
                        }
                        event("config", &serde_json::json!({}))
                    }
                    "alerts" if topics.alerts && !account.is_scoped() => {
                        event("alerts", &serde_json::json!({"firing": super::alerts::firing(&st).len()}))
                    }
                    raw if topics.widget && !account.is_scoped() && raw.starts_with("w-") => {
                        let config = st.config.current();
                        let Some(widget) = config.widgets.iter().find(|w| w.id == raw[2..]) else { continue };
                        event("widget", &super::widgets::view(&st, widget))
                    }
                    raw if topics.boards && raw.starts_with("board:") => {
                        let Some((id, version)) = raw[6..].split_once(':') else { continue };
                        let Ok(version) = version.parse() else { continue };
                        if !account.can_see_board(id) { continue; }
                        event("boards", &super::boards::BoardEvent { id: id.into(), version })
                    }
                    _ => continue,
                    }
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    sig = None;
                    event("resync", &serde_json::json!({}))
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ping.tick() => Bytes::from_static(b": ping\n\n"),
            _ = tx.closed() => break,
        };
        if !st.db.session_is_valid(&session_hash, crate::auth::now_s())
            || tx.send(msg).await.is_err()
        {
            break;
        }
    }
}

pub fn get(state: &Arc<AppState>, query: &str, head: bool, session_hash: String) -> Response<Body> {
    let mut r = if head {
        // Headers only: no subscription, no viewer.
        full(StatusCode::OK, Bytes::new())
    } else {
        let (tx, rx) = mpsc::channel::<Bytes>(QUEUE);
        let sub = state.events.subscribe();
        let guard = state.viewer_guard();
        tokio::spawn(run(
            state.clone(),
            Topics::parse(query),
            session_hash,
            sub,
            tx,
        ));
        Response::new(StreamBody { rx, _guard: guard }.boxed_unsync())
    };
    set(&mut r, CONTENT_TYPE, "text/event-stream");
    set(&mut r, CACHE_CONTROL, "no-store");
    r
}
