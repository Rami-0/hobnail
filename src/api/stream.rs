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
use crate::state::AppState;

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
    pub host: bool,
}

impl Topics {
    pub const ALL: Topics = Topics {
        containers: true,
        stats: true,
        system: true,
        boards: true,
        widget: false,
        alerts: false,
        host: false,
    };

    pub(super) fn requested(query: &str) -> Topics {
        let q = crate::http::query::parse_query(query);
        let mut t = Topics {
            containers: false,
            stats: false,
            system: false,
            boards: false,
            widget: false,
            alerts: false,
            host: false,
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
                    "host" => t.host = true,
                    _ => {}
                }
            }
        }
        t
    }

    pub fn parse(query: &str) -> Topics {
        let t = Self::requested(query);
        if t.containers || t.stats || t.system || t.boards || t.widget || t.alerts || t.host {
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
    crate::read_model::seed_cpu(state, &mut s, crate::util::unix_now() as i64);
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

enum Queued {
    Event(Bytes),
    Provider(u64, Bytes),
}

impl From<Bytes> for Queued {
    fn from(bytes: Bytes) -> Self {
        Self::Event(bytes)
    }
}

/// SSE body backed by a channel.
struct StreamBody {
    rx: mpsc::Receiver<Queued>,
    state: Arc<AppState>,
}

impl hyper::body::Body for StreamBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        loop {
            match std::task::ready!(self.rx.poll_recv(cx)) {
                Some(Queued::Event(bytes)) => return Poll::Ready(Some(Ok(Frame::data(bytes)))),
                Some(Queued::Provider(generation, bytes))
                    if generation == self.state.db.provider_scope_generation() =>
                {
                    return Poll::Ready(Some(Ok(Frame::data(bytes))));
                }
                Some(Queued::Provider(..)) => continue,
                None => return Poll::Ready(None),
            }
        }
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

async fn account(state: &Arc<AppState>, hash: &Arc<str>) -> Option<super::access::Principal> {
    let state = state.clone();
    let hash = hash.clone();
    tokio::task::spawn_blocking(move || state.db.account_session(&hash, crate::auth::now_s()))
        .await
        .ok()
        .flatten()
}

/// None represents unrestricted access; a failed scope read fails closed.
async fn widget_targets(
    state: &Arc<AppState>,
    account: &super::access::Principal,
    requested: bool,
) -> Option<std::collections::HashSet<String>> {
    if !requested || !account.is_scoped() {
        return None;
    }
    let state = state.clone();
    let account = account.clone();
    Some(
        tokio::task::spawn_blocking(move || {
            state.db.dashboard(|conn| account.widget_targets(conn))
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .unwrap_or_default(),
    )
}

async fn run(
    st: Arc<AppState>,
    topics: Topics,
    session_hash: String,
    board: Option<String>,
    mut sub: broadcast::Receiver<Arc<str>>,
    tx: mpsc::Sender<Queued>,
) {
    if tx.send(event("hello", &hello(&st)).into()).await.is_err() {
        return;
    }
    let session_hash: Arc<str> = session_hash.into();
    let list = topics.containers || topics.stats;
    let Some(mut principal) = account(&st, &session_hash).await else {
        return;
    };
    let mut targets = widget_targets(&st, &principal, topics.widget).await;
    let visible_widget = |targets: &Option<std::collections::HashSet<String>>, id: &str| {
        targets.as_ref().is_none_or(|ids| ids.contains(id))
    };
    let widget_viewer = |targets: &Option<std::collections::HashSet<String>>| {
        topics.widget
            && st
                .config
                .current()
                .widgets
                .iter()
                .any(|w| visible_widget(targets, &w.id))
    };
    let mut guard = st.viewer_guard(widget_viewer(&targets));
    let mut provider_viewer = super::provider_widgets::Subscription::new(&st);
    let provider_targets = |account: &super::access::Principal| {
        super::provider_widgets::board_targets(
            &st,
            account,
            board.as_deref().filter(|_| topics.widget),
        )
    };
    provider_viewer.set(provider_targets(&principal));
    if let Some(board) = &board {
        for id in provider_viewer.ids() {
            let Ok(permit) = tx.reserve().await else {
                return;
            };
            let generation = st.db.provider_scope_generation();
            let Some(account) = account(&st, &session_hash).await else {
                return;
            };
            if let Some(view) = super::provider_widgets::view(&st, &account, board, id) {
                permit.send(Queued::Provider(generation, event("widget", &view)));
            }
        }
    }
    let mut sig = list.then(|| current(&st, &principal).1);
    if topics.widget {
        let config = st.config.current();
        for widget in config
            .widgets
            .iter()
            .filter(|w| visible_widget(&targets, &w.id))
        {
            if tx
                .send(event("widget", &super::widgets::view(&st, widget)).into())
                .await
                .is_err()
            {
                return;
            }
        }
    }
    if topics.system && !principal.is_scoped() {
        if let Some(s) = system_view(&st) {
            if tx.send(event("system", &s).into()).await.is_err() {
                return;
            }
        }
    }
    let mut host_tick = tokio::time::interval(Duration::from_secs(3));
    host_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut ping = tokio::time::interval(PING);
    ping.tick().await;
    loop {
        let msg = tokio::select! {
            m = sub.recv() => match m {
                Ok(id) => {
                    let Some(account) = account(&st, &session_hash).await else { break };
                    if account != principal || id.starts_with("board:") || &*id == "reload" {
                        targets = widget_targets(&st, &account, topics.widget).await;
                        guard.set_widgets(widget_viewer(&targets));
                        provider_viewer.set(provider_targets(&account));
                        principal = account.clone();
                    }
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
                    raw if raw.starts_with("w-provider:") && provider_viewer.includes(&raw[2..]) => {
                        let Some(board) = &board else { continue };
                        let Ok(permit) = tx.reserve().await else { break };
                        let generation = st.db.provider_scope_generation();
                        let Some(account) = self::account(&st, &session_hash).await else { break };
                        if let Some(widget) = super::provider_widgets::view(&st, &account, board, &raw[2..]) {
                            permit.send(Queued::Provider(generation, event("widget", &widget)));
                        }
                        continue;
                    }
                    raw if topics.widget && raw.starts_with("w-") && visible_widget(&targets, &raw[2..]) => {
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
                    let Some(account) = account(&st, &session_hash).await else { break };
                    targets = widget_targets(&st, &account, topics.widget).await;
                    guard.set_widgets(widget_viewer(&targets));
                    provider_viewer.set(provider_targets(&account));
                    principal = account;
                    sig = None;
                    event("resync", &serde_json::json!({}))
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = host_tick.tick(), if topics.host => {
                let Some(account) = account(&st, &session_hash).await else { break };
                if account.is_scoped() { continue; }
                let admin = account.role >= crate::db::accounts::Role::Admin;
                let state = st.clone();
                let Ok(view) = tokio::task::spawn_blocking(move || super::host::live(&state, admin, true)).await else { continue; };
                event("host", &view)
            }
            _ = ping.tick() => {
                let Some(account) = account(&st, &session_hash).await else { break };
                targets = widget_targets(&st, &account, topics.widget).await;
                guard.set_widgets(widget_viewer(&targets));
                provider_viewer.set(provider_targets(&account));
                principal = account;
                Bytes::from_static(b": ping\n\n")
            },
            _ = tx.closed() => break,
        };
        if tx.send(msg.into()).await.is_err() {
            break;
        }
    }
}

pub fn get(state: &Arc<AppState>, query: &str, head: bool, session_hash: String) -> Response<Body> {
    let mut r = if head {
        // Headers only: no subscription, no viewer.
        full(StatusCode::OK, Bytes::new())
    } else {
        let (tx, rx) = mpsc::channel(QUEUE);
        let sub = state.events.subscribe();
        let topics = Topics::parse(query);
        let pairs = crate::http::query::parse_query(query);
        let board = crate::http::query::query_get(&pairs, "board")
            .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned);
        tokio::spawn(run(state.clone(), topics, session_hash, board, sub, tx));
        let mut r = Response::new(
            StreamBody {
                rx,
                state: state.clone(),
            }
            .boxed_unsync(),
        );
        crate::http::conn::mark_streaming(&mut r);
        r
    };
    set(&mut r, CONTENT_TYPE, "text/event-stream");
    set(&mut r, CACHE_CONTROL, "no-store");
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::accounts::{AccountChange, Scopes};

    #[tokio::test]
    async fn revoking_a_scope_drops_queued_provider_frames() {
        let state = crate::testutil::state_with(Vec::new(), "");
        state
            .db
            .dashboard(|conn| {
                conn.execute_batch(
                    "INSERT INTO users(id,name,password_hash,role,created,updated,scoped)
                    VALUES (2,'viewer','fixture','viewer',0,0,1);
                 INSERT INTO access_grants(user_id,kind,target_id)
                    VALUES (2,'integration','fixture');",
                )
            })
            .unwrap();
        let (tx, rx) = mpsc::channel(QUEUE);
        let mut stream = StreamBody {
            rx,
            state: state.clone(),
        };
        for _ in 0..QUEUE - 1 {
            tx.try_send(Queued::Provider(
                state.db.provider_scope_generation(),
                Bytes::from_static(b"withheld"),
            ))
            .unwrap();
        }
        tx.try_send(Queued::Event(Bytes::from_static(b"config")))
            .unwrap();
        state
            .db
            .change_account(2, AccountChange::Scopes(Some(Scopes::default())), 0)
            .unwrap();
        drop(tx);
        let frame = stream.frame().await.unwrap().unwrap();
        assert_eq!(frame.into_data().unwrap(), "config");
        assert!(stream.frame().await.is_none());
    }
}
