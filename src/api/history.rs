use std::collections::BTreeMap;
use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::containers::{internal, parse_window_with_default, EventRow};
use super::error::ApiError;
use super::history_window::restarts_per_day;
use crate::db::{accounts::Role, Agg, EventFilter, EventRef, Range, Series, SysMetric};
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

fn read_event(event: EventRef<'_>) -> EventRow {
    EventRow {
        id: event.id,
        at: event.ts,
        kind: event.kind.into(),
        level: event.level.into(),
        subject: event.subject.into(),
        message: event.detail.into(),
    }
}

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
    let matches_level = |event_level: &str| match level {
        "all" => true,
        "incidents" => event_level != "info",
        _ => event_level == level,
    };
    let ((markers, mut events), mut cutoff) = db.scan_window_events(
        &filter,
        SCAN_LIMIT,
        |(markers, events): &mut (Vec<_>, Vec<_>), event| {
            let marker = markers.len() < 200 && event.level != "info";
            let page = before.is_none() && events.len() <= PAGE && matches_level(event.level);
            if (marker || page) && !hidden(event.subject) {
                let row = read_event(event);
                if marker && page {
                    markers.push(row.clone());
                    events.push(row);
                } else if marker {
                    markers.push(row);
                } else {
                    events.push(row);
                }
            }
        },
    );
    if let Some(before) = before {
        (events, cutoff) = db.scan_window_events(
            &EventFilter {
                before_id: Some(before),
                ..filter
            },
            SCAN_LIMIT,
            |events: &mut Vec<_>, event| {
                if events.len() <= PAGE && matches_level(event.level) && !hidden(event.subject) {
                    events.push(read_event(event));
                }
            },
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_history_preserves_scan_cutoff_and_window_markers() {
        let state = crate::testutil::state_with(vec![], "[containers.hidden]\nhide = true");
        let now = crate::auth::now_s();
        state.db.with((), |i| {
            i.conn.execute(
                "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<10002)
                 INSERT INTO events(id, ts, kind, level, subject, detail)
                 SELECT n, ?1, 'container.health',
                   CASE WHEN n IN (1, 10000, 10002) THEN 'warn' ELSE 'info' END,
                   CASE WHEN n=10002 THEN 'hidden' ELSE 'visible' END, '' FROM seq",
                [now - 1],
            )?;
            Ok(())
        });
        let first = read(&state, "range=1h", now, Role::Owner).unwrap();
        assert_eq!(
            first.events.iter().map(|e| e.id).collect::<Vec<_>>(),
            [10000]
        );
        assert_eq!(first.markers, first.events);
        assert_eq!(first.older, Some(3));

        let second = read(&state, "range=1h&before=3", now, Role::Owner).unwrap();
        assert_eq!(second.events.iter().map(|e| e.id).collect::<Vec<_>>(), [1]);
        assert_eq!(second.markers, first.markers);
        assert_eq!(second.older, None);
    }
}
