use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::access::{Handler, Principal};
use super::containers::{event_row, internal, parse_window, EventRow};
use super::error::ApiError;
use super::history::HistoryMetric;
use crate::db::{
    accounts::Role,
    ctrx::{self, NetSource, XMetric},
    Agg, EventFilter,
};
use crate::docker;
use crate::http::server::Body;
use crate::state::AppState;
use crate::ui::fmt::{parse_query, query_get};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerInspect {
    image_id: String,
    image_digests: Vec<String>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    restart_count: u64,
    restart_policy: Option<String>,
    oom_killed: bool,
    memory_limit_bytes: i64,
    pid_limit: Option<i64>,
    privileged: bool,
    read_only: bool,
    network_mode: String,
    networks: Vec<String>,
    network_addresses: Vec<(String, String, String, String, String)>,
    health_schedule: Option<[i64; 4]>,
    user: String,
    work_dir: String,
    command: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    health_test: Option<Vec<String>>,
    env_keys: Vec<String>,
    mounts: Vec<ContainerMount>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerMount {
    kind: String,
    source: Option<String>,
    destination: String,
    writable: bool,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerEnvironment {
    env: Vec<(String, String)>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerProcesses {
    titles: Vec<String>,
    processes: Vec<Vec<String>>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerEvents {
    events: Vec<EventRow>,
    older: Option<i64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct ContainerMetrics {
    series: Vec<HistoryMetric>,
    policy_restarts: Option<u64>,
    starts_24h: u32,
    starts_7d: u32,
    oom_kills: Option<u64>,
    oom_killed: bool,
    pids: Option<u64>,
    pid_limit: Option<u64>,
    memory_limit_bytes: Option<u64>,
    throttled_periods: Option<u64>,
    cpu_periods: Option<u64>,
    throttled_usec: Option<u64>,
    network: &'static str,
}

fn upstream() -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "docker_unavailable",
        "Docker did not provide this data",
    )
}

fn run(
    state: &AppState,
    account: &Principal,
    name: &str,
    query: &str,
    handler: Handler,
) -> Result<Response<Body>, ApiError> {
    let config = state.config.current();
    let snapshot = state.snapshot();
    let visible = account.container_visibility(state);
    let c = snapshot
        .containers
        .iter()
        .find(|c| c.name == name)
        .filter(|c| {
            config.docker.enabled
                && !crate::ui::view::is_excluded(&config, c)
                && account.can_see_container(&visible, name)
        })
        .ok_or_else(ApiError::not_found)?;
    let admin = !account.is_scoped() && account.role >= Role::Admin;
    let socket = &state.env.docker_socket;
    let inspect = || docker::inspect_blocking(socket, &c.id).map_err(|_| upstream());
    let ok = StatusCode::OK;
    let response = match handler {
        Handler::ContainerInspect => {
            let i = inspect()?;
            let env_keys = i.config.env_keys().into_iter().map(str::to_owned).collect();
            let image_digests =
                docker::image_digests_blocking(socket, &i.image).unwrap_or_default();
            let timestamp = |value: &str| docker::parse_rfc3339(value).filter(|at| *at > 0);
            let mounts = i
                .mounts
                .into_iter()
                .map(|m| ContainerMount {
                    kind: m.kind,
                    source: admin.then_some(m.source),
                    destination: m.destination,
                    writable: m.rw,
                })
                .collect();
            super::json(
                ok,
                &ContainerInspect {
                    image_id: i.image,
                    image_digests,
                    started_at: timestamp(&i.state.started_at),
                    finished_at: timestamp(&i.state.finished_at),
                    restart_count: i.restart_count,
                    restart_policy: i.host_config.restart_policy.map(|p| p.name),
                    oom_killed: i.state.oom_killed,
                    memory_limit_bytes: i.host_config.memory,
                    pid_limit: i.host_config.pids_limit,
                    privileged: i.host_config.privileged,
                    read_only: i.host_config.readonly_rootfs,
                    network_mode: i.host_config.network_mode,
                    network_addresses: i
                        .network_settings
                        .networks
                        .as_ref()
                        .into_iter()
                        .flatten()
                        .map(|(name, endpoint)| {
                            (
                                name.clone(),
                                endpoint.ip_address.clone(),
                                endpoint.ipv6_address.clone(),
                                endpoint.gateway.clone(),
                                endpoint.mac_address.clone(),
                            )
                        })
                        .collect(),
                    health_schedule: i
                        .config
                        .healthcheck
                        .as_ref()
                        .map(|h| [h.interval, h.timeout, h.retries, h.start_period]),
                    networks: i
                        .network_settings
                        .networks
                        .unwrap_or_default()
                        .into_keys()
                        .collect(),
                    user: i.config.user,
                    work_dir: i.config.working_dir,
                    command: i.config.cmd.filter(|_| admin),
                    entrypoint: i.config.entrypoint.filter(|_| admin),
                    health_test: i.config.healthcheck.and_then(|h| h.test).filter(|_| admin),
                    env_keys,
                    mounts,
                },
            )
        }
        Handler::ContainerEnv => {
            let i = inspect()?;
            let env = i
                .config
                .env_pairs()
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();
            super::json(ok, &ContainerEnvironment { env })
        }
        Handler::ContainerTop => {
            if !c.is_running() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "container_stopped",
                    "The container is stopped",
                ));
            }
            let mut top = docker::top_blocking(socket, &c.id).map_err(|_| upstream())?;
            let q = parse_query(query);
            if let Some(column) = query_get(&q, "sort")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n < top.titles.len())
            {
                crate::ui::pages::container::sort_rows(
                    &mut top.processes,
                    column,
                    query_get(&q, "dir") != Some("asc"),
                );
            }
            super::json(
                ok,
                &ContainerProcesses {
                    titles: top.titles,
                    processes: top.processes,
                },
            )
        }
        Handler::ContainerEvents => {
            let q = parse_query(query);
            let kind = match query_get(&q, "kind") {
                None | Some("all") => None,
                Some("lifecycle") => Some("container."),
                Some("alerts") => Some("alert."),
                Some("die") => Some("container.die"),
                Some("health") => Some("container.health"),
                Some("oom") => Some("container.oom"),
                Some("start") => Some("container.start"),
                _ => return Err(ApiError::bad_request("unknown event kind", Some("kind"))),
            };
            let before_id = query_get(&q, "before")
                .map(|s| {
                    s.parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                        ApiError::bad_request("expected an event ID", Some("before"))
                    })
                })
                .transpose()?;
            let bounds = if query_get(&q, "from").is_some() || query_get(&q, "to").is_some() {
                let window = parse_window(query, crate::util::unix_now() as i64)?;
                (Some(window.from()), Some(window.to))
            } else {
                (None, None)
            };
            let mut events: Vec<_> = state
                .db
                .events(
                    &EventFilter {
                        subject: Some(name.to_owned()),
                        kind_prefix: kind.map(str::to_owned),
                        before_id,
                        since: bounds.0,
                        until: bounds.1,
                        limit: 101,
                        ..Default::default()
                    }
                    .for_admin(admin),
                )
                .iter()
                .map(event_row)
                .collect();
            let older = if events.len() > 100 {
                events.pop();
                events.last().map(|e| e.id)
            } else {
                None
            };
            super::json(ok, &ContainerEvents { events, older })
        }
        Handler::ContainerMetrics => {
            let now = crate::util::unix_now() as i64;
            let window = parse_window(query, now)?;
            let keys = [
                ("throttled", XMetric::Throttled),
                ("cache", XMetric::Cache),
                ("anon", XMetric::Anon),
                ("memory_limit", XMetric::Limit),
                ("oom", XMetric::OomKills),
                ("net_rx", XMetric::NetRx),
                ("net_tx", XMetric::NetTx),
                ("block_read", XMetric::BlkRead),
                ("block_write", XMetric::BlkWrite),
                ("pids", XMetric::Pids),
            ];
            let metrics: Vec<_> = keys
                .into_iter()
                .map(|(key, metric)| {
                    let read = |agg| {
                        state
                            .db
                            .ctrx_series(name, metric, window.range, window.to, agg)
                    };
                    let clip = |series: crate::db::Series| {
                        series
                            .points
                            .into_iter()
                            .filter(|p| p.0 >= window.from())
                            .collect()
                    };
                    let series = read(Agg::Avg);
                    HistoryMetric {
                        key: key.into(),
                        step: series.step,
                        points: clip(series),
                        max: (window.range.secs() >= 86400).then(|| clip(read(Agg::Max))),
                        yesterday: None,
                    }
                })
                .collect();
            let current = docker::container_cgroup_dir(&state.env.host_sys, &c.id)
                .filter(|_| c.is_running())
                .map(|dir| ctrx::read_cgroup(&dir))
                .unwrap_or_default();
            let ins = inspect().ok();
            let positive = |value| u64::try_from(value).ok().filter(|v| *v > 0);
            let starts = |span| {
                state
                    .db
                    .restart_counts(now - span)
                    .into_iter()
                    .find(|(n, _)| n == name)
                    .map_or(0, |(_, total)| total)
            };
            let network =
                match ctrx::net_source(&c.id).or_else(|| ins.as_ref().map(ctrx::net_source_of)) {
                    Some(NetSource::Host) => "host",
                    Some(NetSource::Shared) => "shared",
                    _ => "container",
                };
            super::json(
                ok,
                &ContainerMetrics {
                    series: metrics,
                    policy_restarts: ins.as_ref().map(|i| i.restart_count),
                    starts_24h: starts(86_400),
                    starts_7d: starts(7 * 86_400),
                    oom_kills: current.oom_kills,
                    oom_killed: ins.as_ref().is_some_and(|i| i.state.oom_killed),
                    pids: current.pids,
                    pid_limit: current
                        .pids_max
                        .or_else(|| ins.as_ref()?.host_config.pids_limit.and_then(positive)),
                    memory_limit_bytes: current
                        .limit
                        .or_else(|| positive(ins.as_ref()?.host_config.memory)),
                    throttled_periods: current.nr_throttled,
                    cpu_periods: current.nr_periods,
                    throttled_usec: current.throttled_usec,
                    network,
                },
            )
        }
        _ => unreachable!("container tabs route"),
    };
    Ok(response)
}

pub(super) async fn get(
    state: &Arc<AppState>,
    account: Principal,
    name: &str,
    query: &str,
    handler: Handler,
) -> Response<Body> {
    let (state, name, query) = (state.clone(), name.to_owned(), query.to_owned());
    match tokio::task::spawn_blocking(move || run(&state, &account, &name, &query, handler)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => error.response(),
        Err(_) => internal(),
    }
}
