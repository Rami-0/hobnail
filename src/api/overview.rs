use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::containers::{current_view, event_row, excluded_subject, internal, EventRow};
use super::history::HistoryMetric;
use crate::db::{accounts::Role, EventFilter, Range, SysMetric};
use crate::http::server::Body;
use crate::state::AppState;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct OverviewAttention {
    name: String,
    title: String,
    reason: String,
    severity: String,
    detail: String,
    since: Option<i64>,
    probe: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct OverviewConsumer {
    name: String,
    title: String,
    cpu_pct: Option<f64>,
    mem_bytes: Option<u64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct OverviewView {
    greeting: String,
    now: i64,
    known: bool,
    running: u32,
    stopped: u32,
    problems: usize,
    firing: usize,
    attention: Vec<OverviewAttention>,
    consumers: Vec<OverviewConsumer>,
    events: Vec<EventRow>,
    markers: Vec<EventRow>,
    hour: Vec<HistoryMetric>,
    starts_since: usize,
    crashes_since: usize,
    alerts_since: usize,
}

fn read(state: &AppState, role: Role, since: Option<i64>, probes: bool) -> OverviewView {
    use crate::ui::pages::container::{probe_line, unhealthy_since};
    let now = crate::util::unix_now() as i64;
    let config = state.config.current();
    let snapshot = state.snapshot();
    let containers = current_view(state, now);
    let firing = super::alerts::firing(state);
    let rows: Vec<_> = containers
        .groups
        .iter()
        .flat_map(|g| &g.containers)
        .collect();
    let mut attention: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            let incident = firing
                .iter()
                .find(|incident| incident.container && incident.subject == row.name);
            let status = row.attention.as_ref();
            let detail = status
                .map(|a| a.reason.clone())
                .or_else(|| incident.map(|i| i.state.clone()))?;
            Some(OverviewAttention {
                name: row.name.clone(),
                title: row.title.clone(),
                reason: if row.exit_code.is_some() {
                    "exit"
                } else if row.restarts > 1 {
                    "repeated_exits"
                } else {
                    &row.label
                }
                .to_owned(),
                severity: if incident.is_some() {
                    "bad"
                } else {
                    status.map_or("warn", |a| &a.severity)
                }
                .to_owned(),
                detail,
                since: incident.map(|i| i.since as i64),
                probe: None,
            })
        })
        .collect();
    attention.sort_by_key(|row| row.severity != "bad");
    attention.truncate(8);
    let mut remaining_probes = crate::ui::pages::overview::PROBES_MAX;
    for row in attention.iter_mut().filter(|r| r.reason == "unhealthy") {
        let Some(container) = snapshot.containers.iter().find(|c| c.name == row.name) else {
            continue;
        };
        row.since = unhealthy_since(&state.db, container, None).or(row.since);
        if role >= Role::Admin && probes && remaining_probes > 0 {
            remaining_probes -= 1;
            row.probe = crate::docker::inspect_blocking(&state.env.docker_socket, &container.id)
                .ok()
                .as_ref()
                .and_then(probe_line);
        }
    }
    let scan = state.db.events(
        &EventFilter {
            since: since.filter(|t| *t > 0).map(|t| (t + 1).min(now - 3600)),
            limit: 200,
            ..Default::default()
        }
        .for_viewer(role >= Role::Admin),
    );
    let filtered: Vec<_> = scan
        .into_iter()
        .filter(|e| !excluded_subject(&config, &snapshot, &e.subject))
        .collect();
    let unseen = || filtered.iter().filter(|e| since.is_some_and(|t| e.ts > t));
    OverviewView {
        greeting: config
            .page
            .greeting
            .clone()
            .unwrap_or_else(|| "Hello".into()),
        now,
        known: snapshot.fetched_at > 0 && snapshot.error.is_none(),
        running: containers.groups.iter().map(|g| g.running).sum(),
        stopped: containers.groups.iter().map(|g| g.stopped).sum(),
        problems: rows.iter().filter(|r| r.attention.is_some()).count(),
        firing: firing.len(),
        attention,
        consumers: rows
            .into_iter()
            .filter(|r| r.cpu_pct.is_some() || r.mem_bytes.is_some())
            .map(|r| OverviewConsumer {
                name: r.name.clone(),
                title: r.title.clone(),
                cpu_pct: r.cpu_pct,
                mem_bytes: r.mem_bytes,
            })
            .collect(),
        events: filtered.iter().take(15).map(event_row).collect(),
        markers: filtered
            .iter()
            .filter(|e| e.ts >= now - 3600)
            .map(event_row)
            .collect(),
        hour: if config.system.enabled {
            [
                SysMetric::Cpu,
                SysMetric::Mem,
                SysMetric::NetRx,
                SysMetric::NetTx,
            ]
            .into_iter()
            .map(|metric| {
                let series = state.db.system_series(metric, Range::H1, now);
                HistoryMetric {
                    key: metric.as_str().into(),
                    step: series.step,
                    points: series.points,
                    max: None,
                    yesterday: None,
                }
            })
            .collect()
        } else {
            Vec::new()
        },
        starts_since: unseen().filter(|e| e.kind == "container.start").count(),
        crashes_since: unseen()
            .filter(|e| {
                e.kind == "container.oom" || (e.kind == "container.die" && e.level != "info")
            })
            .count(),
        alerts_since: unseen()
            .filter(|e| e.kind.starts_with("alert.fired"))
            .count(),
    }
}

pub(super) async fn get(state: &Arc<AppState>, query: &str, role: Role) -> Response<Body> {
    let q = crate::ui::fmt::parse_query(query);
    let since = crate::ui::fmt::query_get(&q, "since").and_then(|t| t.parse::<i64>().ok());
    let probes = crate::ui::fmt::query_get(&q, "probes") == Some("1");
    let state = state.clone();
    match tokio::task::spawn_blocking(move || read(&state, role, since, probes)).await {
        Ok(data) => super::json(StatusCode::OK, &data),
        Err(_) => internal(),
    }
}
