use std::collections::BTreeMap;
use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::containers::{event_row, internal, parse_window_with_default, EventRow};
use super::error::ApiError;
use super::history_window::restarts_per_day;
use crate::db::{accounts::Role, Agg, EventFilter, Range, Series, SysMetric};
use crate::http::query::{parse_query, query_get};
use crate::http::server::Body;
use crate::state::AppState;

type Points = Vec<(i64, f64)>;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HistoryMetric {
    pub(super) key: String,
    pub(super) step: i64,
    pub(super) points: Points,
    pub(super) max: Option<Points>,
    pub(super) yesterday: Option<Points>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HistoryView {
    from: i64,
    to: i64,
    now: i64,
    range: String,
    recording: bool,
    metrics: Vec<HistoryMetric>,
    starts_per_day: BTreeMap<String, [u32; 7]>,
    events: Vec<EventRow>,
    markers: Vec<EventRow>,
    older: Option<i64>,
}

const METRICS: [SysMetric; 8] = [
    SysMetric::Cpu,
    SysMetric::Load1,
    SysMetric::Mem,
    SysMetric::Swap,
    SysMetric::Disk,
    SysMetric::Temp,
    SysMetric::NetRx,
    SysMetric::NetTx,
];
const PAGE: usize = 60;
const SCAN_LIMIT: usize = 10_000;

fn read(state: &AppState, query: &str, now: i64, role: Role) -> Result<HistoryView, ApiError> {
    let window = parse_window_with_default(query, now, Range::H24)?;
    let (from, to, range) = (window.from(), window.to, window.range);
    let query = parse_query(query);
    let compare = query_get(&query, "cmp") == Some("1") && window.secs <= 86_400;
    let before = query_get(&query, "before")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| {
                    ApiError::bad_request("before must be a positive event ID", Some("before"))
                })
        })
        .transpose()?;
    let level =
        query_get(&query, "level").unwrap_or(if query_get(&query, "events") == Some("all") {
            "all"
        } else {
            "incidents"
        });
    if !matches!(level, "all" | "incidents" | "info" | "warn" | "error") {
        return Err(ApiError::bad_request("unknown event level", Some("level")));
    }
    let db = &state.db;
    let clip = |series: Series, shift| {
        series
            .points
            .into_iter()
            .map(|(at, value)| (at + shift, value))
            .filter(|(at, _)| (from..=to).contains(at))
            .collect()
    };
    let metrics = METRICS
        .into_iter()
        .map(|metric| {
            let series = db.system_series(metric, range, to);
            HistoryMetric {
                key: metric.as_str().into(),
                step: series.step,
                points: clip(series, 0),
                max: (range.secs() >= Range::H24.secs())
                    .then(|| clip(db.system_series_agg(metric, range, to, Agg::Max), 0)),
                yesterday: compare
                    .then(|| clip(db.system_series(metric, range, to - 86_400), 86_400)),
            }
        })
        .collect();
    let config = state.config.current();
    let snap = state.snapshot();
    let hidden = |name: &str| super::containers::excluded_subject(&config, &snap, name);
    let starts_per_day = restarts_per_day(
        (1..=7)
            .map(|day| {
                db.restart_counts(now - day * 86_400)
                    .into_iter()
                    .filter(|(name, _)| !hidden(name))
                    .collect()
            })
            .collect(),
    );
    let filter = EventFilter {
        since: Some(from),
        until: Some(to),
        ..Default::default()
    }
    .for_admin(role >= Role::Admin);
    let mut scanned = db.window_events(&filter, SCAN_LIMIT);
    let markers = scanned
        .iter()
        .filter(|e| !hidden(&e.subject) && e.level != "info")
        .take(200)
        .map(event_row)
        .collect();
    if let Some(before) = before {
        scanned = db.window_events(
            &EventFilter {
                before_id: Some(before),
                ..filter
            },
            SCAN_LIMIT,
        );
    }
    let cutoff = (scanned.len() == SCAN_LIMIT)
        .then(|| scanned.last().map(|e| e.id))
        .flatten();
    let mut events: Vec<_> = scanned
        .iter()
        .filter(|e| !hidden(&e.subject))
        .filter(|e| match level {
            "all" => true,
            "incidents" => e.level != "info",
            _ => e.level == level,
        })
        .take(PAGE + 1)
        .map(event_row)
        .collect();
    let older = if events.len() > PAGE {
        events.pop();
        events.last().map(|e| e.id)
    } else {
        cutoff
    };
    Ok(HistoryView {
        from,
        to,
        now,
        range: range.as_str().into(),
        recording: db.is_enabled(),
        metrics,
        starts_per_day,
        events,
        markers,
        older,
    })
}

pub(super) async fn get(state: &Arc<AppState>, query: &str, role: Role) -> Response<Body> {
    let (state, query) = (state.clone(), query.to_owned());
    match tokio::task::spawn_blocking(move || {
        read(&state, &query, crate::util::unix_now() as i64, role)
    })
    .await
    {
        Ok(Ok(data)) => super::json(StatusCode::OK, &data),
        Ok(Err(error)) => error.response(),
        Err(_) => internal(),
    }
}
