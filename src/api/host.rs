use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use super::containers::{
    event_row, excluded_subject, internal, parse_window_with_default, EventRow,
};
use super::error::ApiError;
use super::history::HistoryMetric;
use crate::db::{accounts::Role, Agg, EventFilter, Range};
use crate::host::{collect::Host, parse::SensorKind};
use crate::http::server::Body;
use crate::state::AppState;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HostValue {
    label: String,
    unit: &'static str,
    value: Option<f64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HostGroup {
    name: String,
    values: Vec<HostValue>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HostProcess {
    pid: u32,
    command: String,
    container: Option<String>,
    uid: Option<u32>,
    cpu_pct: f64,
    rss_bytes: u64,
    threads: u64,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HostView {
    at: u64,
    hostname: String,
    uptime_secs: u64,
    window_secs: f64,
    cpu_cores: usize,
    cpu_pct: Option<f64>,
    load: [f64; 3],
    groups: Vec<HostGroup>,
    processes: Option<Vec<HostProcess>>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct HostHistory {
    from: i64,
    to: i64,
    metrics: Vec<HistoryMetric>,
    markers: Vec<EventRow>,
}

fn facts(host: &Host, admin: bool) -> Vec<HostGroup> {
    let mut out = Vec::new();
    let mut group = |name: &str, values: Vec<HostValue>| {
        if !values.is_empty() {
            out.push(HostGroup {
                name: name.into(),
                values,
            });
        }
    };
    let val = |label: &str, unit, value| HostValue {
        label: label.into(),
        unit,
        value,
    };
    group(
        "CPU time",
        [
            ("User", host.cpu.map(|c| c.user)),
            ("System", host.cpu.map(|c| c.system)),
            ("I/O wait", host.cpu.map(|c| c.iowait)),
            ("Interrupts", host.cpu.map(|c| c.irq)),
            ("Steal", host.cpu.map(|c| c.steal)),
        ]
        .into_iter()
        .map(|(label, value)| val(label, "pct", value))
        .collect(),
    );
    group(
        "CPU cores",
        host.cores
            .iter()
            .map(|(n, c)| val(&format!("Core {n}"), "pct", c.map(|c| c.busy)))
            .collect(),
    );
    if host.psi.is_some() {
        group(
            "Pressure",
            crate::host::collect::PSI_RES
                .iter()
                .zip(host.psi_now)
                .flat_map(|(resource, (some, full))| {
                    [
                        val(&format!("{resource} some"), "pct", some),
                        val(&format!("{resource} full"), "pct", full),
                    ]
                })
                .collect(),
        );
    }
    if host.mem_ok {
        let m = &host.mem;
        group(
            "Memory",
            [
                ("Total", m.total),
                ("Used", m.used()),
                ("Available", m.available),
                ("Buffers", m.buffers),
                ("Page cache", m.cache()),
                ("Slab", m.slab),
                ("Shared", m.shmem),
                ("Dirty", m.dirty),
                ("Swap total", m.swap_total),
                ("Swap used", m.swap_used()),
            ]
            .into_iter()
            .map(|(label, value)| val(label, "bytes", Some(value as f64)))
            .collect(),
        );
        group(
            "Paging activity",
            [
                ("Swap in", host.swap_in),
                ("Swap out", host.swap_out),
                ("Major faults", host.majflt),
            ]
            .into_iter()
            .map(|(label, value)| val(label, "per_sec", value))
            .collect(),
        );
    }
    group(
        "Disks",
        host.disks
            .iter()
            .flat_map(|d| {
                [
                    val(
                        &format!("{} capacity", d.name),
                        "bytes",
                        d.size.map(|n| n as f64),
                    ),
                    val(&format!("{} read", d.name), "rate", d.rd_bps),
                    val(&format!("{} write", d.name), "rate", d.wr_bps),
                    val(&format!("{} busy", d.name), "pct", d.util),
                    val(&format!("{} I/O operations", d.name), "per_sec", d.iops),
                ]
            })
            .collect(),
    );
    if admin {
        group(
            "Mounts",
            host.mounts
                .iter()
                .flat_map(|m| {
                    [
                        val(
                            &format!("{} used", m.mnt),
                            "bytes",
                            m.usage.map(|(_, used, _)| used as f64),
                        ),
                        val(
                            &format!("{} available", m.mnt),
                            "bytes",
                            m.usage.map(|(_, _, avail)| avail as f64),
                        ),
                    ]
                })
                .collect(),
        );
    }
    group(
        "Network",
        host.nets
            .iter()
            .flat_map(|n| {
                [
                    val(&format!("{} received", n.name), "rate", n.rx_bps),
                    val(&format!("{} sent", n.name), "rate", n.tx_bps),
                    val(
                        &format!("{} errors / drops", n.name),
                        "count",
                        Some(n.new_errs as f64),
                    ),
                ]
            })
            .collect(),
    );
    group(
        "Sensors",
        host.sensors
            .iter()
            .map(|s| {
                val(
                    &format!("{} {}", s.chip, s.label),
                    if s.kind == SensorKind::Temp {
                        "temp"
                    } else {
                        "rpm"
                    },
                    Some(s.value),
                )
            })
            .collect(),
    );
    group(
        "GPU",
        host.gpus
            .iter()
            .map(|gpu| val(&format!("{} ({})", gpu.card, gpu.driver), "pct", gpu.busy))
            .collect(),
    );
    out
}

pub(super) fn live(state: &AppState, admin: bool, subscribed: bool) -> HostView {
    let (host, processes) = crate::host::live_sample(state, admin && subscribed);
    let config = state.config.current();
    let snapshot = state.snapshot();
    let processes = processes.filter(|_| admin).map(|report| {
        report
            .top
            .iter()
            .filter_map(|p| {
                let container = match &p.cid {
                    None => None,
                    Some(cid) => {
                        let c = snapshot
                            .containers
                            .iter()
                            .find(|c| c.id.starts_with(cid) || cid.starts_with(&c.id))?;
                        if crate::ui::view::is_excluded(&config, c) {
                            return None;
                        }
                        Some(c.name.clone())
                    }
                };
                Some(HostProcess {
                    pid: p.pid,
                    command: p.cmd.clone(),
                    container,
                    uid: p.uid,
                    cpu_pct: p.cpu,
                    rss_bytes: p.rss,
                    threads: p.threads,
                })
            })
            .collect()
    });
    HostView {
        at: crate::util::unix_now(),
        hostname: state.env.hostname.clone(),
        uptime_secs: host.uptime,
        window_secs: host.window,
        cpu_cores: host.ncores,
        cpu_pct: host.cpu.map(|c| c.busy),
        load: host.load,
        groups: facts(&host, admin),
        processes,
    }
}

fn history(state: &AppState, query: &str, admin: bool) -> Result<HostHistory, ApiError> {
    let now = crate::util::unix_now() as i64;
    let window = parse_window_with_default(query, now, Range::H1)?;
    let from = window.from();
    let to = window.to;
    let db = &state.db;
    let config = state.config.current();
    let snapshot = state.snapshot();
    let markers = db
        .window_events(
            &EventFilter {
                since: Some(from),
                until: Some(to),
                ..Default::default()
            }
            .for_viewer(admin),
            1000,
        )
        .iter()
        .filter(|e| !excluded_subject(&config, &snapshot, &e.subject))
        .take(200)
        .map(event_row)
        .collect();
    let metrics = db
        .hostx_keys("")
        .into_iter()
        .filter(|key| admin || !key.starts_with("mount."))
        .map(|key| {
            let read = |agg| db.hostx_series(&key, window.range, to, agg);
            let clip =
                |s: crate::db::Series| s.points.into_iter().filter(|p| p.0 >= from).collect();
            let series = read(Agg::Avg);
            let step = series.step;
            let points = clip(series);
            let max = (window.range.secs() >= 86400).then(|| clip(read(Agg::Max)));
            HistoryMetric {
                key,
                step,
                points,
                max,
                yesterday: None,
            }
        })
        .collect();
    Ok(HostHistory {
        from,
        to,
        metrics,
        markers,
    })
}

pub(super) async fn get(
    state: &Arc<AppState>,
    query: &str,
    role: Role,
    series: bool,
) -> Response<Body> {
    let (state, query) = (state.clone(), query.to_owned());
    let admin = role >= Role::Admin;
    match tokio::task::spawn_blocking(move || {
        if series {
            history(&state, &query, admin).map(|v| super::json(StatusCode::OK, &v))
        } else {
            Ok(super::json(StatusCode::OK, &live(&state, admin, false)))
        }
    })
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => error.response(),
        Err(_) => internal(),
    }
}
