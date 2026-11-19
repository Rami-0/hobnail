//! `GET /api/v1/containers` (the list) and `GET /api/v1/containers/<name>`
//! (one container's Overview), both Public and `no-store`.
//!
//! The JSON says what today's pages say: the list is `ui::view::build_groups`
//! (hidden, excluded and, with `show_stopped = false`, stopped containers
//! appear nowhere), the detail is `ui::pages::container` (why_red, rules,
//! labels, events for the viewer). Port choice is the client's: rows carry
//! the raw bindings and `link`, the configured URL template with `{host}`
//! left for the viewer's host.
//!
//! Session-only data (health-check probe output, Docker's last checks) is
//! left out for a visitor, never null-filled with a secret.

use std::collections::HashMap;

use serde::Serialize;

use super::error::ApiError;
use crate::db::{Agg, CtrMetric, Event, EventFilter, Range};
use crate::docker::Inspect;
use crate::model::{Container, Snapshot, StatsMap};
use crate::state::AppState;
use crate::ui::pages::container as page;
use crate::ui::pages::history::Window;
use crate::ui::view::{self, CardView, Sev};

/// Events in the detail's timeline (the Overview shows 12).
const DETAIL_EVENTS: usize = 12;
/// Chart markers at most (events of the container inside the window).
const MARKERS_MAX: usize = 200;
/// The first line of a failing probe is cut to this many characters.
const PROBE_LINE_MAX: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainersView {
    pub groups: Vec<GroupRow>,
    /// Unix seconds of the container list (0: never fetched).
    pub fetched_at: u64,
    /// Docker could not be read; the list is the last known one.
    pub docker_error: Option<String>,
    pub now: i64,
}

/// One stack (group) that holds at least one listed container.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupRow {
    /// Group key (compose project, override or label value).
    pub name: String,
    pub title: String,
    pub icon: Option<String>,
    pub collapsed: bool,
    pub running: u32,
    pub stopped: u32,
    pub containers: Vec<ContainerRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerRow {
    pub name: String,
    /// Display name (override, label, compose service, else the name).
    pub title: String,
    pub image: String,
    /// Group key of the stack it is listed under.
    pub stack: String,
    pub icon: Option<String>,
    /// The configured link, `{host}` left for the client to fill with the
    /// viewer's host; null: the client picks one from `ports`.
    pub link: Option<String>,
    #[cfg_attr(test, ts(type = r#""ok" | "warn" | "bad" | "off""#))]
    pub dot: String,
    /// Status word: running, healthy, unhealthy, starting, restarting,
    /// paused, stopped, created, dead.
    pub label: String,
    /// Docker `State` (running, exited, ...).
    pub state: String,
    pub health: Option<String>,
    /// Non-zero exit code of a stopped container.
    pub exit_code: Option<i64>,
    /// `die` events in the last hour, when 2 or more (else 0).
    pub restarts: u32,
    /// Unix seconds this run started. In the list it is read from Docker's
    /// "Up 3 hours" (as precise as that text); the detail has the exact
    /// time from an inspect when Docker answers. Null when not running.
    pub started_at: Option<i64>,
    /// Unix seconds.
    pub created: i64,
    pub ports: Vec<PortBinding>,
    /// Percent of one core; null when stats are off, stopped or not sampled.
    pub cpu_pct: Option<f64>,
    pub mem_bytes: Option<u64>,
    pub attention: Option<Attention>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PortBinding {
    /// "0.0.0.0", "::", "127.0.0.1", a specific address, or "" (any).
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    #[cfg_attr(test, ts(type = r#""tcp" | "udp""#))]
    pub proto: String,
}

/// Why a container needs someone (`CardView::attention`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Attention {
    #[cfg_attr(test, ts(type = r#""bad" | "warn""#))]
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContainerDetail {
    pub row: ContainerRow,
    /// Full 64-character id.
    pub id: String,
    pub created: i64,
    /// "Why is this red?"; null when nothing is wrong.
    pub why: Option<Why>,
    /// `container.start` events in the last 7 days.
    pub restarts_7d: u32,
    /// The 12 newest events of this container (no `auth.*` for a visitor).
    pub events: Vec<EventRow>,
    /// Labels minus Compose and image bookkeeping (`com.docker.compose.*`,
    /// `org.opencontainers.image.*`, `com.docker.desktop.*`).
    pub labels: Vec<(String, String)>,
    /// Container alert rules that cover this container.
    pub rules: Vec<RuleRow>,
    pub series: SeriesSet,
    pub now: i64,
}

/// "Why is this red?" (`ui::pages::container::why_red`). Today's page shows
/// it in red whatever the cause, so `severity` is `bad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Why {
    #[cfg_attr(test, ts(type = r#""bad" | "warn""#))]
    pub severity: String,
    /// "Health check failing, 3 in a row", or the first thing that is wrong
    /// ("Exited with code 137: killed (...)").
    pub title: String,
    /// The rest, joined with " · " ("" when there is nothing more).
    pub detail: String,
    /// Since when (unix seconds): the failing health check's start, else
    /// when it happened.
    pub since: Option<i64>,
    /// Where the logs link opens (unix seconds); null: the newest logs.
    pub logs_at: Option<i64>,
    /// First line of the newest failing probe's output (120 characters at
    /// most). Session only: always null for a visitor.
    pub probe: Option<String>,
    /// Docker's last health checks, newest first (output cut to 600
    /// characters). Session only: always empty for a visitor.
    pub checks: Vec<WhyCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WhyCheck {
    /// Unix seconds the check started; null when Docker's time is unreadable.
    pub at: Option<i64>,
    pub exit_code: i64,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EventRow {
    pub id: i64,
    /// Unix seconds.
    pub at: i64,
    /// Dotted kind: `container.start`, `container.die`, `alert.fired`, ...
    pub kind: String,
    #[cfg_attr(test, ts(type = r#""info" | "warn" | "error""#))]
    pub level: String,
    pub subject: String,
    pub message: String,
}

/// One alert rule covering the container (as the Overview's rules panel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuleRow {
    pub name: String,
    /// Rule kind (`container`).
    pub kind: String,
    /// The incident's state when firing, else "disabled", else
    /// "watching: stopped, unhealthy, restarting".
    pub summary: String,
    pub firing: bool,
    /// Notification channels; empty: the rule shows on the page only.
    pub channels: Vec<String>,
}

/// CPU and memory history for the charts. Gaps are missing points: the
/// client breaks a line where two points are more than 1.5 × `step` apart.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SeriesSet {
    pub from: i64,
    pub to: i64,
    /// Seconds per point.
    pub step: i64,
    /// The preset range that served the window (1h, 6h, 24h, 7d, 30d).
    #[cfg_attr(test, ts(type = r#""1h" | "6h" | "24h" | "7d" | "30d""#))]
    pub range: String,
    /// `[unix seconds, percent of one core]`, oldest first.
    pub cpu: Vec<(i64, f64)>,
    /// `[unix seconds, bytes]`.
    pub mem: Vec<(i64, f64)>,
    /// Bucket maxima, for windows of 24 h and more (else null).
    pub cpu_max: Option<Vec<(i64, f64)>>,
    pub mem_max: Option<Vec<(i64, f64)>>,
    /// Events of this container inside `[from, to]`, newest first.
    pub markers: Vec<EventRow>,
}

// ---------------------------------------------------------------- list

/// The list view: `build_groups` without bookmarks, groups without a
/// container left out. Pure apart from what it is given.
pub fn view(
    cfg: &crate::config::Config,
    snap: &Snapshot,
    stats: &StatsMap,
    now: i64,
) -> ContainersView {
    // The host only fills card URLs, which the API does not send.
    let groups = view::build_groups(cfg, snap, stats, "");
    let by_name: HashMap<&str, &Container> = snap
        .containers
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let groups = groups
        .iter()
        .filter_map(|g| {
            let containers: Vec<ContainerRow> = g
                .cards
                .iter()
                .filter(|c| c.container)
                .filter_map(|card| {
                    let c = by_name.get(card.name.as_str())?;
                    Some(row(cfg, card, c, snap.fetched_at))
                })
                .collect();
            (!containers.is_empty()).then(|| GroupRow {
                name: g.name.clone(),
                title: g.title.clone(),
                icon: g.icon.clone(),
                collapsed: g.collapsed,
                running: g.running as u32,
                stopped: g.stopped as u32,
                containers,
            })
        })
        .collect();
    ContainersView {
        groups,
        fetched_at: snap.fetched_at,
        docker_error: docker_error(cfg, snap),
        now,
    }
}

/// The list as served now (live stats, seeded from history).
pub fn current_view(state: &AppState, now: i64) -> ContainersView {
    let cfg = state.config.current();
    let snap = state.snapshot();
    let stats = view::stats_with_history(state, &snap);
    view(&cfg, &snap, &stats, now)
}

/// Docker's error, when Docker is enabled at all (as the docker banner).
pub fn docker_error(cfg: &crate::config::Config, snap: &Snapshot) -> Option<String> {
    snap.error.clone().filter(|_| cfg.docker.enabled)
}

fn row(
    cfg: &crate::config::Config,
    card: &CardView,
    c: &Container,
    fetched_at: u64,
) -> ContainerRow {
    ContainerRow {
        name: card.name.clone(),
        title: card.title.clone(),
        image: card.image.clone(),
        stack: card.stack.clone(),
        icon: card.icon.clone(),
        link: view::link_template(cfg, c),
        dot: card.dot.map_or("off", |d| d.class()).to_string(),
        label: card.label.to_string(),
        state: card.state.clone(),
        health: card.health.clone(),
        exit_code: card.exit_code,
        restarts: card.restarts as u32,
        started_at: c
            .is_running()
            .then(|| started_from_status(&c.status, fetched_at))
            .flatten(),
        created: c.created,
        ports: c
            .ports
            .iter()
            .map(|p| PortBinding {
                host_ip: p.host_ip.clone(),
                host_port: p.host_port,
                container_port: p.container_port,
                proto: p.proto.clone(),
            })
            .collect(),
        cpu_pct: card.cpu,
        mem_bytes: card.mem,
        attention: card.attention().map(|(sev, reason)| Attention {
            severity: sev_word(sev).into(),
            reason,
        }),
    }
}

fn sev_word(s: Sev) -> &'static str {
    match s {
        Sev::Bad => "bad",
        Sev::Warn => "warn",
    }
}

/// When a running container started, from Docker's status ("Up 3 hours",
/// "Up About a minute (healthy)", "Up Less than a second") and the time
/// the list was read. `None` for anything else.
pub fn started_from_status(status: &str, fetched_at: u64) -> Option<i64> {
    let rest = status.strip_prefix("Up ")?;
    let rest = rest.split(" (").next().unwrap_or(rest).trim();
    let lower = rest.to_ascii_lowercase();
    let mut w = lower.split_whitespace();
    let n: i64 = match w.next()? {
        "less" => return Some(fetched_at as i64),
        "about" | "an" | "a" => {
            if lower.starts_with("about") {
                w.next(); // "a" / "an"
            }
            1
        }
        x => x.parse().ok()?,
    };
    let unit = match w.next()?.trim_end_matches('s') {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86_400,
        "week" => 7 * 86_400,
        "month" => 30 * 86_400,
        "year" => 365 * 86_400,
        _ => return None,
    };
    if fetched_at == 0 {
        return None;
    }
    Some(fetched_at as i64 - n * unit)
}

// ---------------------------------------------------------------- detail

/// `?range=` or `?from=&to=` (unix seconds) → the chart window, as the
/// History page reads it (`Window`): `to` clamped to now, `from` to the
/// last 30 days, served from the smallest preset covering it. Bad input is
/// a 400 naming the field.
pub fn parse_window(query: &str, now: i64) -> Result<Window, ApiError> {
    parse_window_with_default(query, now, Range::H6)
}

pub(super) fn parse_window_with_default(
    query: &str,
    now: i64,
    default: Range,
) -> Result<Window, ApiError> {
    let q = crate::ui::fmt::parse_query(query);
    let get = |k| crate::ui::fmt::query_get(&q, k);
    let preset = match get("range") {
        None => default,
        Some(r) => Range::parse(r).ok_or_else(|| {
            ApiError::bad_request("range must be one of 1h, 6h, 24h, 7d, 30d", Some("range"))
        })?,
    };
    let num = |k: &'static str| -> Result<Option<i64>, ApiError> {
        match get(k) {
            None => Ok(None),
            Some(v) => v
                .parse::<i64>()
                .map(Some)
                .map_err(|_| ApiError::bad_request(format!("{k} must be unix seconds"), Some(k))),
        }
    };
    let (from, to) = (num("from")?, num("to")?);
    let (from, to) = match (from, to) {
        (None, None) => return Ok(Window::parse(&[], preset, now)),
        (Some(f), Some(t)) => (f, t),
        (Some(_), None) => return Err(ApiError::bad_request("to is missing", Some("to"))),
        (None, Some(_)) => return Err(ApiError::bad_request("from is missing", Some("from"))),
    };
    if from >= to {
        return Err(ApiError::bad_request(
            "from must be before to",
            Some("from"),
        ));
    }
    if from.max(now - Range::D30.secs()) >= to.min(now) {
        return Err(ApiError::bad_request(
            "the window must overlap the last 30 days",
            Some("from"),
        ));
    }
    let pairs = vec![
        ("from".to_string(), from.to_string()),
        ("to".to_string(), to.to_string()),
    ];
    Ok(Window::parse(&pairs, preset, now))
}

pub type InspectFn<'a> = &'a dyn Fn(&str) -> Result<Inspect, String>;

/// The detail of `name` for a viewer (`signed`: has a session). 404 for an
/// unknown container and for one the owner hid (`view::is_excluded`).
pub fn detail(
    state: &AppState,
    signed: bool,
    name: &str,
    query: &str,
    now: i64,
    inspect: InspectFn,
) -> Result<ContainerDetail, ApiError> {
    let cfg = state.config.current();
    let snap = state.snapshot();
    let c = snap
        .containers
        .iter()
        .find(|c| c.name == name)
        .filter(|c| cfg.docker.enabled && !view::is_excluded(&cfg, c))
        .ok_or_else(ApiError::not_found)?;
    let window = parse_window(query, now)?;
    let stats = view::stats_with_history(state, &snap);
    let card = view::card_for(&cfg, &snap, &stats, "", name).ok_or_else(ApiError::not_found)?;
    let mut row = row(&cfg, &card, c, snap.fetched_at);

    // One inspect, redacted for a visitor (no probe output then anyway).
    let ins = inspect(&c.id).ok().map(|i| {
        if signed {
            i
        } else {
            page::redact_for_anonymous(i)
        }
    });
    let started = ins
        .as_ref()
        .and_then(|i| crate::docker::parse_rfc3339(&i.state.started_at));
    if c.is_running() {
        if let Some(s) = started.filter(|s| *s > 0) {
            row.started_at = Some(s);
        }
    }
    let db = &state.db;
    let starts_10m = db
        .events(&EventFilter {
            subject: Some(c.name.clone()),
            kind_prefix: Some("container.start".into()),
            limit: 50,
            ..Default::default()
        })
        .iter()
        .filter(|e| e.ts >= now - 600)
        .count() as u32;
    let since = page::unhealthy_since(db, c, started);
    let why = page::why_red(c, ins.as_ref(), starts_10m, since, now).map(|w| why(&w, signed));

    let restarts_7d = db
        .restart_counts(now - Range::D7.secs())
        .into_iter()
        .find(|(n, _)| n == name)
        .map_or(0, |(_, n)| n);
    let events = db
        .events(
            &EventFilter {
                subject: Some(c.name.clone()),
                limit: DETAIL_EVENTS,
                ..Default::default()
            }
            .for_viewer(signed),
        )
        .iter()
        .map(event_row)
        .collect();
    let labels = c
        .labels
        .iter()
        .filter(|(k, _)| !page::is_bookkeeping_label(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let status = state.alerts.lock().unwrap().clone();
    let rules = page::matching_rules(&cfg.alerts, &c.name)
        .into_iter()
        .map(|r| {
            let firing = status
                .incidents
                .iter()
                .find(|i| i.rule == r.name && i.subject == c.name);
            let summary = match firing {
                Some(i) => i.state.clone(),
                None if !r.enabled() => "disabled".into(),
                None => format!(
                    "watching: {}",
                    r.on.as_deref()
                        .map(|v| v.join(", "))
                        .unwrap_or_else(|| "stopped, unhealthy, restarting".into())
                ),
            };
            RuleRow {
                name: r.name.clone(),
                kind: r.kind.as_str().into(),
                summary,
                firing: firing.is_some(),
                channels: r.channels.clone(),
            }
        })
        .collect();
    let series = series(db, &c.name, &window, signed);
    Ok(ContainerDetail {
        row,
        id: c.id.clone(),
        created: c.created,
        why,
        restarts_7d,
        events,
        labels,
        rules,
        series,
        now,
    })
}

/// Historical subjects can outlive the current Docker snapshot.
pub(crate) fn excluded_subject(
    config: &crate::config::Config,
    snapshot: &Snapshot,
    name: &str,
) -> bool {
    if let Some(hide) = config.containers.get(name).and_then(|entry| entry.hide) {
        return hide;
    }
    config
        .docker
        .exclude
        .iter()
        .any(|pattern| crate::util::glob_match(pattern, name))
        || snapshot
            .containers
            .iter()
            .any(|c| c.name == name && view::is_excluded(config, c))
}

pub(super) fn event_row(e: &Event) -> EventRow {
    EventRow {
        id: e.id,
        at: e.ts,
        kind: e.kind.clone(),
        level: e.level.clone(),
        subject: e.subject.clone(),
        message: e.detail.clone(),
    }
}

/// The page's `Why` as JSON; probe output and checks only for a session.
fn why(w: &page::Why, signed: bool) -> Why {
    let mut parts: Vec<String> = if w.text.is_empty() {
        Vec::new()
    } else {
        w.text.split(" · ").map(str::to_string).collect()
    };
    let (title, since, probe, checks) = match &w.health {
        Some(h) => {
            let mut title = "Health check failing".to_string();
            if h.streak > 0 {
                title.push_str(&format!(", {} in a row", h.streak));
            }
            let probe = h
                .probes
                .iter()
                .find(|p| p.1 != 0)
                .and_then(|p| p.2.trim().lines().next())
                .map(|l| page::truncate(l.trim(), PROBE_LINE_MAX))
                .filter(|l| !l.is_empty());
            let checks = h
                .probes
                .iter()
                .map(|(at, code, out)| WhyCheck {
                    at: *at,
                    exit_code: *code,
                    output: page::truncate(out.trim(), page::HEALTH_OUTPUT_MAX),
                })
                .collect();
            (title, h.since.or(w.ts), probe, checks)
        }
        None => {
            let title = if parts.is_empty() {
                String::new()
            } else {
                parts.remove(0)
            };
            (title, w.ts, None, Vec::new())
        }
    };
    Why {
        severity: "bad".into(),
        title,
        detail: parts.join(" · "),
        since,
        logs_at: w.ts,
        probe: probe.filter(|_| signed),
        checks: if signed { checks } else { Vec::new() },
    }
}

fn series(db: &crate::db::Db, name: &str, w: &Window, signed: bool) -> SeriesSet {
    let (from, to, range) = (w.from(), w.to, w.range);
    let keep = |s: crate::db::Series| -> Vec<(i64, f64)> {
        s.points.into_iter().filter(|p| p.0 >= from).collect()
    };
    let cpu = db.container_series(name, CtrMetric::Cpu, range, to);
    let step = cpu.step;
    let mem = db.container_series(name, CtrMetric::Mem, range, to);
    // Rollups carry bucket maxima: from 24 h up the charts show a band.
    let band = |m| {
        (range.secs() >= 86_400)
            .then(|| keep(db.container_series_agg(name, m, range, to, Agg::Max)))
    };
    let markers = db
        .events(
            &EventFilter {
                subject: Some(name.to_string()),
                since: Some(from),
                until: Some(to),
                limit: MARKERS_MAX,
                ..Default::default()
            }
            .for_viewer(signed),
        )
        .iter()
        .map(event_row)
        .collect();
    SeriesSet {
        from,
        to,
        step,
        range: range.as_str().into(),
        cpu: keep(cpu),
        mem: keep(mem),
        cpu_max: band(CtrMetric::Cpu),
        mem_max: band(CtrMetric::Mem),
        markers,
    }
}

// ---------------------------------------------------------------- handlers

pub async fn list(
    state: &std::sync::Arc<AppState>,
    visible: super::access::ContainerVisibility,
) -> hyper::Response<crate::http::server::Body> {
    let st = state.clone();
    let now = crate::util::unix_now() as i64;
    match tokio::task::spawn_blocking(move || visible.filter_view(current_view(&st, now))).await {
        Ok(v) => super::json(hyper::StatusCode::OK, &v),
        Err(_) => internal(),
    }
}

pub async fn get(
    state: &std::sync::Arc<AppState>,
    name: &str,
    query: &str,
    admin: bool,
) -> hyper::Response<crate::http::server::Body> {
    let (st, name, query) = (state.clone(), name.to_string(), query.to_string());
    let now = crate::util::unix_now() as i64;
    let job = move || {
        let socket = st.env.docker_socket.clone();
        detail(&st, admin, &name, &query, now, &|id| {
            crate::docker::inspect_blocking(&socket, id)
        })
    };
    match tokio::task::spawn_blocking(job).await {
        Ok(Ok(d)) => super::json(hyper::StatusCode::OK, &d),
        Ok(Err(e)) => e.response(),
        Err(_) => internal(),
    }
}

pub(super) fn internal() -> hyper::Response<crate::http::server::Body> {
    ApiError::new(
        hyper::StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "the request failed",
    )
    .response()
}
