use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::containers::{event_row, internal, EventRow, RuleRow};
use super::error::ApiError;
use crate::alerts::RuleKind;
use crate::db::EventFilter;
use crate::http::server::Body;
use crate::state::AppState;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct AlertIncident {
    pub rule: String,
    pub subject: String,
    pub container: bool,
    pub href: String,
    pub state: String,
    pub since: u64,
    pub notified: bool,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct AlertChannel {
    name: String,
    kind: String,
    status: String,
    last_success: Option<u64>,
    last_failure: Option<u64>,
    pending: usize,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AlertsView {
    firing: Vec<AlertIncident>,
    rules: Vec<RuleRow>,
    channels: Vec<AlertChannel>,
    log: Vec<EventRow>,
    older: Option<i64>,
}

pub(super) fn firing(state: &AppState) -> Vec<AlertIncident> {
    let config = state.config.current();
    let snapshot = state.snapshot();
    let status = state.alerts.lock().unwrap();
    status
        .incidents
        .iter()
        .filter_map(|incident| {
            let rule = config.alerts.iter().find(|r| r.name == incident.rule)?;
            let container = matches!(rule.kind, RuleKind::Container | RuleKind::LogMatch);
            if container
                && super::containers::excluded_subject(&config, &snapshot, &incident.subject)
            {
                return None;
            }
            let href = match rule.kind {
                RuleKind::Container | RuleKind::LogMatch if !incident.subject.is_empty() => {
                    format!(
                        "/app/containers/{}",
                        crate::http::query::url_encode(&incident.subject)
                    )
                }
                RuleKind::Host if !incident.subject.is_empty() => "/app/host".into(),
                RuleKind::Widget if !incident.subject.is_empty() => {
                    format!("/app/overview#w-{}", incident.subject)
                }
                _ => "/app/alerts".into(),
            };
            Some(AlertIncident {
                rule: incident.rule.clone(),
                subject: incident.subject.clone(),
                container,
                href,
                state: incident.state.clone(),
                since: incident.since,
                notified: incident.notified,
            })
        })
        .collect()
}

pub(crate) fn read(state: &AppState, query: &str) -> Result<AlertsView, ApiError> {
    use crate::alerts::Health;
    use crate::http::query::{parse_query, query_get};
    let q = parse_query(query);
    let parse = |name| {
        query_get(&q, name)
            .map(|v| {
                v.parse::<i64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| ApiError::bad_request("must be a positive integer", Some(name)))
            })
            .transpose()
    };
    let (since, until, before_id) = (parse("from")?, parse("to")?, parse("before")?);
    if matches!((since, until), (Some(from), Some(to)) if from >= to) {
        return Err(ApiError::bad_request(
            "from must be before to",
            Some("from"),
        ));
    }
    let firing = firing(state);
    let config = state.config.current();
    let snapshot = state.snapshot();
    let status = state.alerts.lock().unwrap();
    let rules = config
        .alerts
        .iter()
        .map(|r| {
            let firing = firing.iter().any(|i| i.rule == r.name);
            RuleRow {
                name: r.name.clone(),
                kind: r.kind.as_str().into(),
                summary: if !r.enabled() {
                    "Disabled"
                } else if firing {
                    "Firing"
                } else {
                    "Watching"
                }
                .into(),
                firing,
                channels: r.channels.clone(),
            }
        })
        .collect();
    let channels = config
        .channels
        .iter()
        .map(|(name, cfg)| {
            let c = status.channels.get(name);
            let health = c.map_or(Health::Idle, |c| c.health);
            AlertChannel {
                name: name.clone(),
                kind: cfg.kind.as_str().into(),
                status: match health {
                    Health::Ok => "ok",
                    Health::Failing => "failing",
                    Health::Misconfigured => "misconfigured",
                    Health::Idle => "idle",
                }
                .into(),
                last_success: c.and_then(|c| c.last_success),
                last_failure: c.and_then(|c| c.last_failure),
                pending: c.map_or(0, |c| c.pending),
            }
        })
        .collect();
    drop(status);
    let scan = state.db.events(&EventFilter {
        kind_prefix: Some("alert.".into()),
        since,
        until,
        before_id,
        limit: 500,
        ..Default::default()
    });
    let mut log: Vec<_> = scan
        .iter()
        .filter(|e| !super::containers::excluded_subject(&config, &snapshot, &e.subject))
        .take(51)
        .map(|e| {
            let mut row = event_row(e);
            if e.kind == "alert.delivery_failed" {
                row.message.clear();
            }
            row
        })
        .collect();
    let older = if log.len() > 50 {
        log.pop();
        log.last().map(|e| e.id)
    } else {
        (scan.len() == 500)
            .then(|| scan.last().map(|e| e.id))
            .flatten()
    };
    Ok(AlertsView {
        firing,
        rules,
        channels,
        log,
        older,
    })
}

pub(super) async fn get(state: &Arc<AppState>, query: &str) -> Response<Body> {
    let (state, query) = (state.clone(), query.to_owned());
    match tokio::task::spawn_blocking(move || read(&state, &query)).await {
        Ok(Ok(data)) => super::json(StatusCode::OK, &data),
        Ok(Err(error)) => error.response(),
        Err(_) => internal(),
    }
}
