//! Logs inside the app (Wave 1 item 1, docs/V3-PLAN.md). GET-only toward
//! Docker: `GET /containers/{id}/json` and `GET /containers/{id}/logs`.
//!
//! - `client`: the two Docker requests; `frame`: the multiplexed/raw stream
//!   parser (lines capped at 16 KiB), timestamps, ANSI stripping.
//! - `tail`: live tail over SSE (one Docker stream per viewer per container,
//!   only while connected) and plain-text downloads.
//! - `level`: error/warn/info/debug detection; `matcher`: the small regex
//!   engine used by `log_match` alert rules (no `regex` crate).
//! - `watch`: the background task for `log_match` rules and per-minute
//!   level counts (`db::logcounts`).
//!
//! The React client retains a bounded ring and renders ANSI colors,
//! searches and filters in the browser.
//!
//! Log text is never written to the app's own log, the database or alert
//! messages. Logs require a session. Tails are limited to visible containers.

pub mod client;
pub mod frame;
pub mod level;
pub mod matcher;
pub mod tail;
#[cfg(test)]
mod tests;
pub mod watch;

use hyper::StatusCode;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::http::query::{parse_query, query_get};
use crate::http::server::{json, text, Body};
use crate::state::AppState;

/// `[logs]` in the config file.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogsConfig {
    /// Accepted for compatibility. All log routes require a session.
    #[serde(rename = "require_login")]
    _require_login: bool,
    /// Containers (name globs) tailed in the background only to count their
    /// lines per level per minute (the level chart). Containers watched by
    /// a `log_match` rule are always counted. Each tailed container is one
    /// mostly idle Docker connection (see docs/measurements.md).
    pub count: Vec<String>,
    /// Most containers tailed in the background (alert-watched first).
    pub max_watched: usize,
    /// Lines the browser keeps per view (1,000 to 50,000).
    pub ring: usize,
    /// Lines loaded when a page opens (100 to 5,000).
    pub tail: u32,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            _require_login: true,
            count: vec![],
            max_watched: 20,
            ring: 5000,
            tail: 500,
        }
    }
}

impl LogsConfig {
    pub fn ring(&self) -> usize {
        self.ring.clamp(1000, 50_000)
    }
    pub fn tail(&self) -> u32 {
        self.tail.clamp(100, client::MAX_TAIL)
    }
}

fn tailable_c(cfg: &crate::config::Config, c: &crate::model::Container) -> bool {
    client::valid_name(&c.name) && !crate::read_model::is_excluded(cfg, c)
}

/// A container whose logs may be read: it exists in the current snapshot and
/// passes `tailable_c`.
pub fn may_tail(state: &AppState, name: &str) -> bool {
    let cfg = state.config.current();
    state
        .snapshot()
        .containers
        .iter()
        .any(|c| c.name == name && tailable_c(&cfg, c))
}

/// Names of containers whose logs may be read by the logs picker.
pub fn tailable(state: &AppState) -> std::collections::HashSet<String> {
    let cfg = state.config.current();
    state
        .snapshot()
        .containers
        .iter()
        .filter(|c| tailable_c(&cfg, c))
        .map(|c| c.name.clone())
        .collect()
}

/// A logs link that opens the viewer at a moment (`#t=` in unix ms).
fn targets(
    state: &AppState,
    q: &[(String, String)],
    max: usize,
    visible: &crate::api::access::ContainerVisibility,
) -> Vec<String> {
    if let Some(stack) = query_get(q, "stack") {
        let cfg = state.config.current();
        let mut names: Vec<_> = state
            .snapshot()
            .containers
            .iter()
            .filter(|c| visible.allows(&c.name) && in_stack(&cfg, c, stack))
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        names.truncate(24);
        return names;
    }
    let mut out: Vec<String> = vec![];
    for n in query_get(q, "c").unwrap_or("").split(',') {
        let n = n.trim();
        if visible.allows(n) && may_tail(state, n) && !out.iter().any(|o| o == n) && out.len() < max
        {
            out.push(n.to_string());
        }
    }
    out
}

/// A container of compose project `stack` whose logs may be read.
fn in_stack(cfg: &crate::config::Config, c: &crate::model::Container, stack: &str) -> bool {
    c.project.as_deref() == Some(stack) && tailable_c(cfg, c)
}

/// Whether `/stacks/<stack>/logs` has anything to show: a compose project
/// of that name with a container whose logs may be read (a config group is
/// not a project, LG-09). For the sidebar and the `/logs` list.
pub fn is_project(cfg: &crate::config::Config, snap: &crate::model::Snapshot, stack: &str) -> bool {
    snap.containers.iter().any(|c| in_stack(cfg, c, stack))
}

/// `since`: unix seconds, a duration back from now (`15m`, `1h`), or
/// `start` (since the container started; resolved per container).
fn parse_since(s: &str, now: f64) -> Option<f64> {
    if s == "start" {
        return Some(-1.0);
    }
    if let Ok(v) = s.parse::<f64>() {
        return (v.is_finite() && v >= 0.0).then_some(v);
    }
    crate::util::parse_duration(s)
        .ok()
        .map(|d| (now - d.as_secs_f64()).max(0.0))
}

fn range_query(q: &[(String, String)], default_tail: Option<u32>) -> client::Query {
    let now = crate::util::unix_now() as f64;
    let since = query_get(q, "since").and_then(|s| parse_since(s, now));
    let until = query_get(q, "until")
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0);
    let tail = match query_get(q, "tail") {
        Some("all") => None,
        Some(t) => t.parse::<u32>().ok().map(|n| n.min(client::MAX_TAIL)),
        None => default_tail,
    };
    client::Query {
        follow: until.is_none() && query_get(q, "follow") != Some("0"),
        tail,
        since,
        until,
        cap: None,
    }
}

/// What the viewer's stream reads. The browser ring bounds what is shown, so
/// never more than `MAX_TAIL` lines. A closed window (`since` and `until`: a
/// custom range, "Load older lines") is read from its start and cut at that
/// many lines by the server: Docker applies `tail` to the whole log before
/// the window, which leaves nothing once more lines came after it.
fn stream_query(q: &[(String, String)], default_tail: u32) -> client::Query {
    let mut rq = range_query(q, Some(default_tail));
    let n = rq.tail.unwrap_or(client::MAX_TAIL).min(client::MAX_TAIL);
    if rq.since.is_some() && rq.until.is_some() {
        rq.tail = None;
        rq.cap = Some(n);
    } else {
        rq.tail = Some(n);
    }
    rq
}

/// `/api/logs/*` (GET only): `stream` (SSE), `download` (text), `levels`
/// (level counts JSON), `status` (background tails).
pub async fn route(
    state: &Arc<AppState>,
    req: &hyper::Request<hyper::body::Incoming>,
    session_hash: &str,
    visible: &crate::api::access::ContainerVisibility,
) -> hyper::Response<Body> {
    let q = parse_query(req.uri().query().unwrap_or(""));
    let cfg = state.config.current();
    match req.uri().path() {
        "/api/logs/stream" => {
            let names = targets(state, &q, 4, visible);
            if names.is_empty() {
                return text(StatusCode::BAD_REQUEST, "no container (use c= or stack=)\n");
            }
            tail::sse(
                state,
                names,
                stream_query(&q, cfg.logs.tail()),
                session_hash.to_owned(),
            )
        }
        "/api/logs/download" => {
            let names = targets(state, &q, 4, visible);
            if names.is_empty() {
                return text(StatusCode::BAD_REQUEST, "no container (use c= or stack=)\n");
            }
            let rq = range_query(&q, None);
            let base = query_get(&q, "stack")
                .filter(|s| client::valid_name(s))
                .unwrap_or(&names[0]);
            let stamp: String = crate::util::fmt_utc(crate::util::unix_now())
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == 'T')
                .collect();
            let filename = format!("{base}-{stamp}.log");
            tail::download(state, names, rq, &filename, session_hash.to_owned())
        }
        "/api/logs/levels" => {
            let Some(name) = query_get(&q, "c").filter(|n| client::valid_name(n)) else {
                return text(StatusCode::BAD_REQUEST, "c= is required\n");
            };
            if !visible.allows(name) || !may_tail(state, name) {
                return text(StatusCode::NOT_FOUND, "container not found\n");
            }
            let range = query_get(&q, "range")
                .and_then(crate::db::Range::parse)
                .unwrap_or(crate::db::Range::H24);
            let (db, name, now) = (
                state.db.clone(),
                name.to_string(),
                crate::util::unix_now() as i64,
            );
            let s = tokio::task::spawn_blocking(move || db.log_level_series(&name, range, now))
                .await
                .unwrap_or_default();
            json(&json!({
                "range": range.as_str(),
                "step": s.step,
                "levels": ["error", "warn", "info", "debug", "other"],
                "points": s.points.iter().map(|(t, c)| json!([t, c])).collect::<Vec<_>>(),
            }))
        }
        "/api/logs/status" => {
            let s = watch::shared().lock().unwrap();
            json(&json!({
                "watched": s.watched,
                "over_cap": s.over_cap,
                "cap": s.cap,
                "bad_rules": s.bad_rules,
                "lines_seen": s.lines,
                "viewer_streams": tail::OPEN_STREAMS.load(std::sync::atomic::Ordering::Relaxed),
            }))
        }
        _ => text(StatusCode::NOT_FOUND, "not found\n"),
    }
}

/// Background task: bounded tails for `log_match` alert rules and per-minute
/// level counts. Does nothing when no rule needs it and `[logs] count` is empty.
pub async fn run(state: Arc<AppState>) {
    watch::run(state).await
}
