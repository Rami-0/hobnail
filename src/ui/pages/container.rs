//! `/containers/<name>`: the container detail page, in tabs (`?tab=`):
//! Overview (live numbers, usage, timeline, ports, rules, labels), Logs,
//! Metrics (CPU and throttling, memory, network, block I/O, PIDs, OOM
//! kills, restarts; from `db::ctrx`), Inspect (`docker inspect`, environment
//! values masked), Processes (`docker top`) and Events. A one-line "why is
//! this red?" summary sits above the tabs when something is wrong.
//!
//! Docker is only ever asked with GET (`docker::inspect_blocking`,
//! `docker::top_blocking`). Environment values never reach the page: the
//! page lists keys, and a signed-in viewer's "Reveal" button fetches
//! `GET /api/containers/<name>/env`, which answers 401 without a session.

use hyper::header::CACHE_CONTROL;
use hyper::{HeaderMap, StatusCode};
use maud::{html, Markup, PreEscaped};

use crate::alerts::RuleKind;
use crate::db::ctrx::{self, NetSource, XMetric};
use crate::db::{Agg, CtrMetric, EventFilter, Range, Series};
use crate::docker::{HealthLog, Inspect, Top};
use crate::model::Container;
use crate::ui::chart::{line_chart, ChartSpec, Unit};
use crate::ui::fmt::{parse_query, pct, query_get, short_utc, time_tag, url_decode, url_encode};
use crate::ui::pages::overview::{event_row, live_spark};
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::view::{binding_link, card_for, CardView, GroupView};
use crate::ui::{empty_state, ic, panel_head, pill, status_dot};
use crate::util::{glob_match, human_bytes, human_duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Logs,
    Metrics,
    Inspect,
    Processes,
    Events,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Logs,
        Tab::Metrics,
        Tab::Inspect,
        Tab::Processes,
        Tab::Events,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.slug() == s)
    }

    pub fn slug(self) -> &'static str {
        match self {
            Tab::Overview => "overview",
            Tab::Logs => "logs",
            Tab::Metrics => "metrics",
            Tab::Inspect => "inspect",
            Tab::Processes => "processes",
            Tab::Events => "events",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Logs => "Logs",
            Tab::Metrics => "Metrics",
            Tab::Inspect => "Inspect",
            Tab::Processes => "Processes",
            Tab::Events => "Events",
        }
    }
}

/// Event filters offered on the Events tab: `(query value, label, kind prefix)`.
const EVENT_KINDS: &[(&str, &str, &str)] = &[
    ("all", "All", ""),
    ("lifecycle", "Lifecycle", "container."),
    ("alerts", "Alerts", "alert."),
    ("die", "Exits", "container.die"),
    ("health", "Health", "container.health"),
    ("oom", "OOM", "container.oom"),
    ("start", "Starts", "container.start"),
];

/// Health-check outputs shown (Docker keeps five).
const HEALTH_LOGS: usize = 5;
/// Health-check output is cut to this many characters.
const HEALTH_OUTPUT_MAX: usize = 600;

pub fn page(ctx: &Ctx, name: &str) -> (StatusCode, String) {
    let snap = ctx.state.snapshot();
    let Some(c) = snap.containers.iter().find(|c| c.name == name) else {
        return missing(ctx, name, &snap.containers, snap.error.as_deref());
    };
    let stats = crate::ui::view::stats_with_history(ctx.state, &snap);
    let Some(card) = card_for(&ctx.cfg, &snap, &stats, ctx.host, name) else {
        return missing(ctx, name, &snap.containers, snap.error.as_deref());
    };
    let q = parse_query(ctx.query);
    let tab = query_get(&q, "tab")
        .and_then(Tab::parse)
        .unwrap_or(Tab::Overview);
    let range = query_get(&q, "range")
        .and_then(Range::parse)
        .unwrap_or(Range::H6);
    let socket = &ctx.state.env.docker_socket;
    // One inspect per page view (a few ms on the socket); the page renders
    // on the blocking pool.
    let ins = crate::docker::inspect_blocking(socket, &c.id).map(|i| {
        if ctx.user.is_some() {
            i
        } else {
            redact_for_anonymous(i)
        }
    });
    let now = ctx.now as i64;
    let starts_10m = ctx
        .state
        .db
        .events(&EventFilter {
            subject: Some(name.to_string()),
            kind_prefix: Some("container.start".into()),
            limit: 50,
            ..Default::default()
        })
        .iter()
        .filter(|e| e.ts >= now - 600)
        .count() as u32;
    let unhealthy_since = last_unhealthy_event(ctx, name);
    let why = why_red(c, ins.as_ref().ok(), starts_10m, unhealthy_since, now);
    let base = format!("/containers/{}", url_encode(name));
    let heading = html! {
        nav.crumb aria-label="Breadcrumb" {
            a href="/containers" { "Containers" }
            (ic("chev-r"))
            h1.title.mono { (c.name) }
        }
    };
    let body_tab = match tab {
        Tab::Overview => overview_tab(ctx, &card, c, range, &base),
        Tab::Logs => logs_tab(ctx, c, range, &base),
        Tab::Metrics => metrics_tab(ctx, c, ins.as_ref().ok(), range, &base),
        Tab::Inspect => inspect_tab(ctx, c, &ins),
        Tab::Processes => processes_tab(ctx, c, &q, &base),
        Tab::Events => events_tab(ctx, c, &q, &base),
    };
    let body = html! {
        div.page.cd {
            header.ctr-head {
                (crate::ui::icon(card.icon.as_deref(), &card.title))
                div.ctr-id {
                    h2 { (status_dot(&card)) (card.title) }
                    p.mono.muted {
                        (c.name) " · " (c.image)
                        @if card.stack != c.name { " · stack " a href={"/containers?stack=" (url_encode(&card.stack))} { (card.stack) } }
                    }
                }
                div.ctr-actions {
                    @if let Some(u) = &card.url { a.btn href=(u) target="_blank" rel="noopener noreferrer" { (ic("external")) "Open" } }
                    a.btn href={(base) "/logs"} { (ic("logs")) "Logs" }
                    @if let Some(u) = &card.metrics_url { a.btn href=(u) target="_blank" rel="noopener noreferrer" title="Open this host in Beszel" { (ic("activity")) "Beszel" } }
                    @if ctx.user.is_some() {
                        a.btn.primary href={"/settings/containers?edit=override:" (url_encode(name))} { (ic("edit")) "Customize" }
                    }
                }
            }
            @if let Some(err) = &snap.error {
                p.cd-why.warn role="status" {
                    (ic("alert")) span { "Docker is not reachable (" (err) "). Showing the last known state." }
                }
            }
            @if let Some(w) = &why {
                p.cd-why.bad role="status" {
                    (ic("alert"))
                    span { strong { "Why is this red? " } (w.text) }
                    a href={(base) "/logs" (w.ts.map(|t| format!("#t={t}")).unwrap_or_default())} { "Logs around then" }
                }
            }
            nav.cd-tabs aria-label="Container sections" {
                @for t in Tab::ALL {
                    a href={(base) "?tab=" (t.slug()) @if matches!(t, Tab::Overview | Tab::Metrics | Tab::Logs) { "&range=" (range.as_str()) }}
                        aria-current=[(t == tab).then_some("page")] { (t.label()) }
                }
            }
            (body_tab)
        }
    };
    (
        StatusCode::OK,
        layout(ctx, Nav::Containers, &card.title, heading, body),
    )
}

fn range_picker(base: &str, tab: Tab, range: Range) -> Markup {
    html! {
        nav.seg aria-label="Range" {
            @for r in Range::ALL {
                a href={(base) "?tab=" (tab.slug()) "&range=" (r.as_str())} aria-current=[(r == range).then_some("true")] { (r.as_str()) }
            }
        }
    }
}

fn spec<'a>(title: &'a str, unit: Unit, color: &'a str, range: Range, now: i64) -> ChartSpec<'a> {
    ChartSpec {
        title,
        unit,
        color,
        range_secs: range.secs(),
        now,
        cap: None,
        zero_based: true,
        compact: false,
    }
}

fn overview_tab(ctx: &Ctx, card: &CardView, c: &Container, range: Range, base: &str) -> Markup {
    let now = ctx.now as i64;
    let db = &ctx.state.db;
    let name = c.name.as_str();
    let cpu = db.container_series(name, CtrMetric::Cpu, range, now);
    let mem = db.container_series(name, CtrMetric::Mem, range, now);
    let events = db.events(
        &EventFilter {
            subject: Some(name.to_string()),
            limit: 12,
            ..Default::default()
        }
        .for_viewer(ctx.user.is_some()),
    );
    let restarts = db
        .restart_counts(now - Range::D7.secs())
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, n)| n);
    html! {
        (head_live(card, c, restarts))
        // Filled by live updates (see `live_json`).
        div #ctr-live hidden data-j="{\"c\":{}}" {}
        section.sec aria-label="History" {
            div.panel-h {
                h2 { "Usage" }
                (range_picker(base, Tab::Overview, range))
            }
            div.charts.two {
                (line_chart(&spec("CPU", Unit::Percent, "--ch-cpu", range, now), &cpu, db.is_enabled()))
                (line_chart(&spec("Memory", Unit::Bytes, "--ch-mem", range, now), &mem, db.is_enabled()))
            }
            p.muted.cd-more { a href={(base) "?tab=metrics&range=" (range.as_str())} { "Network, disk I/O, PIDs and throttling on the Metrics tab" } }
        }
        div.cols {
            section.panel aria-label="Timeline" {
                (panel_head("Timeline", Some((&format!("{base}?tab=events"), "All events"))))
                @if events.is_empty() {
                    p.muted {
                        @if db.is_enabled() { "No events recorded for this container yet." }
                        @else { "Starts, stops and health changes appear here once history is recording." }
                    }
                } @else {
                    ul.events { @for e in &events { (event_row(ctx.now, e, false)) } }
                }
            }
            section.panel aria-label="Ports and links" {
                (panel_head("Ports and links", None))
                (ports_table(c, ctx.host))
            }
        }
        div.cols {
            (rules_panel(ctx, c))
            (labels_panel(c))
        }
    }
}

fn logs_tab(ctx: &Ctx, c: &Container, range: Range, base: &str) -> Markup {
    let _ = (ctx, range);
    html! {
        section.panel aria-label="Logs" {
            (panel_head("Logs", None))
            p {
                "Live tail, search and download are on the logs page. "
                a.btn.primary href={(base) "/logs"} { (ic("logs")) "Open logs of " span.mono { (c.name) } }
            }
            (crate::logs::level_chart(ctx.state, &c.name, range))
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics

fn metrics_tab(
    ctx: &Ctx,
    c: &Container,
    ins: Option<&Inspect>,
    range: Range,
    base: &str,
) -> Markup {
    let now = ctx.now as i64;
    let db = &ctx.state.db;
    let on = db.is_enabled();
    let name = c.name.as_str();
    let x = |m, agg| db.ctrx_series(name, m, range, now, agg);
    let cpu = db.container_series(name, CtrMetric::Cpu, range, now);
    let mem = db.container_series(name, CtrMetric::Mem, range, now);
    let cg = crate::docker::container_cgroup_dir(&ctx.state.env.host_sys, &c.id)
        .filter(|_| c.is_running())
        .map(|d| ctrx::read_cgroup(&d));
    let limit = cg.as_ref().and_then(|g| g.limit).or_else(|| {
        ins.map(|i| i.host_config.memory)
            .filter(|m| *m > 0)
            .map(|m| m as u64)
    });
    let starts = |secs: i64| {
        db.restart_counts(now - secs)
            .into_iter()
            .find(|(n, _)| n == name)
            .map_or(0, |(_, n)| n)
    };
    let net = ctrx::net_source(&c.id).or_else(|| ins.map(ctrx::net_source_of));
    let mem_of_limit = limit.filter(|l| *l > 0).map(|l| Series {
        step: mem.step,
        points: mem
            .points
            .iter()
            .map(|(t, v)| (*t, v / l as f64 * 100.0))
            .collect(),
    });
    let chart =
        |title, unit, color, s: &Series| line_chart(&spec(title, unit, color, range, now), s, on);
    let running = c.is_running();
    html! {
        section.strip.facts aria-label="Now" {
            div.cell {
                div.cell-h { span.label { "Restarts" } }
                div.big.num { (ins.map_or(0, |i| i.restart_count)) }
                div.sub { "by policy · " (starts(86_400)) " starts in 24 h, " (starts(Range::D7.secs())) " in 7 d" }
            }
            div.cell {
                div.cell-h { span.label { "OOM kills" } }
                @let oom = cg.as_ref().and_then(|g| g.oom_kills).unwrap_or(0);
                div.big.num.bad[oom > 0 || ins.is_some_and(|i| i.state.oom_killed)] { (oom) }
                div.sub {
                    @if ins.is_some_and(|i| i.state.oom_killed) { "last exit was an OOM kill" }
                    @else if running { "this run" } @else { "stopped" }
                }
            }
            div.cell {
                div.cell-h { span.label { "PIDs" } }
                @match cg.as_ref().and_then(|g| g.pids) {
                    Some(p) => {
                        div.big.num { (p) }
                        div.sub { "limit " (cg.as_ref().and_then(|g| g.pids_max).map_or("none".to_string(), |m| m.to_string())) }
                    }
                    None => { div.big.muted { "—" } div.sub { (if running { "not readable" } else { "stopped" }) } }
                }
            }
            div.cell {
                div.cell-h { span.label { "Memory limit" } }
                div.big.num { (limit.map_or("none".to_string(), human_bytes)) }
                div.sub {
                    @match (&cg, limit) {
                        (Some(g), Some(l)) if l > 0 => { (pct((g.anon.unwrap_or(0) + g.cache.unwrap_or(0)) as f64 / l as f64 * 100.0)) " used incl. cache" }
                        (None, _) if !running => "stopped",
                        _ => "memory.max is unlimited",
                    }
                }
            }
            div.cell {
                div.cell-h { span.label { "CPU throttling" } }
                @match cg.as_ref().and_then(|g| g.nr_throttled.zip(g.nr_periods)) {
                    Some((t, p)) => {
                        div.big.num { (t) }
                        div.sub {
                            "of " (p) " periods · "
                            (human_duration(cg.as_ref().and_then(|g| g.throttled_usec).unwrap_or(0) / 1_000_000)) " throttled"
                        }
                    }
                    None => { div.big.muted { "—" } div.sub { (if running { "no CPU quota" } else { "stopped" }) } }
                }
            }
        }
        section.sec aria-label="Metrics history" {
            div.panel-h {
                h2 { "History" }
                (range_picker(base, Tab::Metrics, range))
            }
            @if !running { p.muted { "The container is stopped: history up to when it stopped is shown below." } }
            h3.cd-group { "CPU" }
            div.charts.two {
                (chart("CPU", Unit::Percent, "--ch-cpu", &cpu))
                (chart("Throttled periods", Unit::Percent, "--ch-cpu", &x(XMetric::Throttled, Agg::Max)))
            }
            h3.cd-group { "Memory" }
            div.charts.two {
                (chart("Memory (resident)", Unit::Bytes, "--ch-mem", &mem))
                @if let Some(s) = &mem_of_limit {
                    (chart("Memory of limit", Unit::Percent, "--ch-mem", s))
                } @else {
                    (chart("Page cache", Unit::Bytes, "--ch-mem", &x(XMetric::Cache, Agg::Avg)))
                }
                (chart("Anonymous", Unit::Bytes, "--ch-mem", &x(XMetric::Anon, Agg::Avg)))
                @if mem_of_limit.is_some() {
                    (chart("Page cache", Unit::Bytes, "--ch-mem", &x(XMetric::Cache, Agg::Avg)))
                } @else {
                    (chart("OOM kills (this run)", Unit::Plain, "--bad", &x(XMetric::OomKills, Agg::Max)))
                }
            }
            h3.cd-group { "Network" }
            @match net {
                Some(NetSource::Host) => p.muted { "Uses the host network (network_mode: host): its traffic is the host's and is not counted per container. See the host page." },
                Some(NetSource::Shared) => p.muted { "Shares another container's network namespace (or has none): traffic is counted on that container." },
                _ => div.charts.two {
                    (chart("Received", Unit::BytesPerSec, "--ch-rx", &x(XMetric::NetRx, Agg::Avg)))
                    (chart("Sent", Unit::BytesPerSec, "--ch-tx", &x(XMetric::NetTx, Agg::Avg)))
                },
            }
            h3.cd-group { "Disk and processes" }
            div.charts.two {
                (chart("Block read", Unit::BytesPerSec, "--ch-rx", &x(XMetric::BlkRead, Agg::Avg)))
                (chart("Block write", Unit::BytesPerSec, "--ch-tx", &x(XMetric::BlkWrite, Agg::Avg)))
                (chart("PIDs", Unit::Plain, "--ch-cpu", &x(XMetric::Pids, Agg::Max)))
                @if mem_of_limit.is_some() {
                    (chart("OOM kills (this run)", Unit::Plain, "--bad", &x(XMetric::OomKills, Agg::Max)))
                }
            }
            (crate::logs::level_chart(ctx.state, &c.name, range))
        }
    }
}

// ---------------------------------------------------------------------------
// Inspect

fn join_cmd(v: &Option<Vec<String>>) -> String {
    match v {
        Some(v) if !v.is_empty() => v
            .iter()
            .map(|a| {
                if a.is_empty() || a.contains([' ', '"', '\'']) {
                    format!("{a:?}")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn ns(v: i64) -> String {
    if v <= 0 {
        "default".into()
    } else if v < 1_000_000_000 {
        format!("{} ms", v / 1_000_000)
    } else {
        human_duration((v / 1_000_000_000) as u64)
    }
}

fn kv_row(k: &str, v: Markup) -> Markup {
    html! { dt { (k) } dd { (v) } }
}

fn mono_or_none(s: &str) -> Markup {
    if s.is_empty() {
        html! { span.muted { "none" } }
    } else {
        html! { code { (s) } }
    }
}

pub fn inspect_tab(ctx: &Ctx, c: &Container, ins: &Result<Inspect, String>) -> Markup {
    let i = match ins {
        Ok(i) => i,
        Err(e) => {
            return empty_state(
                "alert",
                "Docker did not answer",
                html! { "Could not inspect " code { (c.name) } ": " (e) },
            )
        }
    };
    let digests = crate::docker::image_digests_blocking(&ctx.state.env.docker_socket, &i.image)
        .unwrap_or_default();
    let signed_in = ctx.user.is_some();
    let h = &i.host_config;
    let cpus = if h.nano_cpus > 0 {
        format!("{:.2}", h.nano_cpus as f64 / 1e9)
    } else if h.cpu_quota > 0 && h.cpu_period > 0 {
        format!("{:.2}", h.cpu_quota as f64 / h.cpu_period as f64)
    } else {
        "unlimited".into()
    };
    let bytes_or = |v: i64, none: &str| {
        if v > 0 {
            human_bytes(v as u64)
        } else {
            none.to_string()
        }
    };
    let rp = h.restart_policy.clone().unwrap_or_default();
    let env_keys = i.config.env_keys();
    let nets: Vec<_> = i.network_settings.networks.iter().flatten().collect();
    let started = crate::docker::parse_rfc3339(&i.state.started_at);
    let finished = crate::docker::parse_rfc3339(&i.state.finished_at);
    html! {
        div.cols {
            section.panel aria-label="Image and command" {
                (panel_head("Image and command", None))
                dl.kv.cd-kv {
                    (kv_row("Image", mono_or_none(&i.config.image)))
                    (kv_row("Image id", mono_or_none(&i.image)))
                    (kv_row("Digest", html! {
                        @if digests.is_empty() { span.muted { "none (built locally)" } }
                        @for d in &digests { code { (d) } br; }
                    }))
                    (kv_row("Command", mono_or_none(&join_cmd(&i.config.cmd))))
                    (kv_row("Entrypoint", mono_or_none(&join_cmd(&i.config.entrypoint))))
                    (kv_row("Working dir", mono_or_none(&i.config.working_dir)))
                    (kv_row("User", if i.config.user.is_empty() { html! { span.muted { "image default (often root)" } } } else { html! { code { (i.config.user) } } }))
                }
            }
            section.panel aria-label="State and limits" {
                (panel_head("State and limits", None))
                dl.kv.cd-kv {
                    (kv_row("State", html! { (i.state.status) @if i.state.pid > 0 { span.muted { " · pid " (i.state.pid) } } }))
                    (kv_row("Started", html! { @if let Some(t) = started { (time_tag(ctx.now, t.max(0) as u64)) } @else { span.muted { "never" } } }))
                    @if !i.state.running {
                        (kv_row("Finished", html! {
                            @if let Some(t) = finished { (time_tag(ctx.now, t.max(0) as u64)) } @else { span.muted { "—" } }
                            " · exit " (i.state.exit_code) " (" (exit_meaning(i.state.exit_code)) ")"
                            @if i.state.oom_killed { " · " (pill("bad", "OOM killed")) }
                        }))
                    }
                    @if !i.state.error.is_empty() { (kv_row("Error", html! { code { (i.state.error) } })) }
                    (kv_row("Restart policy", html! {
                        code { (if rp.name.is_empty() { "no" } else { rp.name.as_str() }) }
                        @if rp.maximum_retry_count > 0 { " · max " (rp.maximum_retry_count) " retries" }
                        " · restarted " (i.restart_count) " times"
                    }))
                    (kv_row("Memory", html! { (bytes_or(h.memory, "unlimited"))
                        @if h.memory_reservation > 0 { " · reservation " (human_bytes(h.memory_reservation as u64)) }
                        @if h.memory_swap > 0 { " · swap+mem " (human_bytes(h.memory_swap as u64)) } }))
                    (kv_row("CPUs", html! { (cpus)
                        @if !h.cpuset_cpus.is_empty() { " · cpuset " code { (h.cpuset_cpus) } }
                        @if h.cpu_shares > 0 { " · shares " (h.cpu_shares) } }))
                    (kv_row("PIDs limit", html! { @match h.pids_limit.filter(|p| *p > 0) { Some(p) => (p), None => "unlimited" } }))
                    (kv_row("Network mode", mono_or_none(&h.network_mode)))
                    (kv_row("Security", html! {
                        @if h.privileged { (pill("warn", "privileged")) " " }
                        @if h.readonly_rootfs { (pill("ok", "read-only root")) " " }
                        @for cap in h.cap_add.iter().flatten() { (pill("", cap)) " " }
                        @if !h.privileged && !h.readonly_rootfs && h.cap_add.as_ref().is_none_or(|v| v.is_empty()) { span.muted { "defaults" } }
                    }))
                }
            }
        }
        section.panel aria-label="Networks" {
            (panel_head("Networks", None))
            @if nets.is_empty() { p.muted { "Not attached to any network." } } @else {
                div.cd-scroll { table.tbl.small {
                    thead { tr { th scope="col" { "Network" } th scope="col" { "IP" } th scope="col" { "Gateway" } th scope="col" { "MAC" } th scope="col" { "Aliases" } } }
                    tbody { @for (n, e) in &nets { tr {
                        td.mono { (n) }
                        td.mono { (e.ip_address) @if !e.ipv6_address.is_empty() { br; (e.ipv6_address) } }
                        td.mono { (e.gateway) }
                        td.mono { (e.mac_address) }
                        td.mono { (e.aliases.as_deref().unwrap_or(&[]).join(", ")) }
                    } } }
                } }
            }
        }
        section.panel aria-label="Mounts" {
            (panel_head("Mounts", None))
            @if i.mounts.is_empty() { p.muted { "No mounts." } } @else {
                div.cd-scroll { table.tbl.small {
                    thead { tr { th scope="col" { "Type" } th scope="col" { "Source" } th scope="col" { "Destination" } th scope="col" { "Mode" } } }
                    tbody { @for m in &i.mounts { tr {
                        td { (m.kind) }
                        td.mono { @if m.kind == "volume" && !m.name.is_empty() { (m.name) } @else { (m.source) } }
                        td.mono { (m.destination) }
                        td { @if m.rw { "read-write" } @else { (pill("ok", "read-only")) } @if !m.mode.is_empty() { span.muted.mono { " " (m.mode) } } }
                    } } }
                } }
            }
        }
        section.panel #cd-env aria-label="Environment" {
            div.panel-h {
                h2 { "Environment" }
                @if signed_in && !env_keys.is_empty() {
                    button.btn.sm type="button" #cd-reveal data-url={"/api/containers/" (url_encode(&c.name)) "/env"} aria-pressed="false" { (ic("lock")) "Reveal values" }
                } @else if !env_keys.is_empty() {
                    a.more href={"/login?next=" (url_encode(ctx.path)) (url_encode("?tab=inspect"))} { "Sign in to reveal" }
                }
            }
            @if env_keys.is_empty() { p.muted { "No environment variables." } } @else {
                dl.kv.cd-kv.cd-env {
                    @for (n, k) in env_keys.iter().enumerate() {
                        dt.mono { (k) }
                        dd.mono data-env=(n) { span.masked aria-label="hidden value" { "••••••" } }
                    }
                }
                p.muted.small { "Values are never part of this page; revealing fetches them for your session only." }
            }
        }
        (health_panel(ctx, i))
        @if signed_in && !env_keys.is_empty() { script { (PreEscaped(REVEAL_JS)) } }
    }
}

/// Reveal / hide environment values (signed-in viewers only). Values are set
/// with `textContent`, never as HTML.
const REVEAL_JS: &str = r#"(function(){var b=document.getElementById('cd-reveal');if(!b)return;var shown=false,vals=null;
function put(){document.querySelectorAll('#cd-env dd[data-env]').forEach(function(d){var i=+d.getAttribute('data-env');var s=document.createElement('span');
if(shown&&vals&&i<vals.length){s.textContent=vals[i][1];}else{s.className='masked';s.textContent='••••••';}d.replaceChildren(s);});
b.setAttribute('aria-pressed',shown?'true':'false');b.lastChild.textContent=shown?'Hide values':'Reveal values';}
b.addEventListener('click',function(){if(shown){shown=false;vals=null;put();return;}
fetch(b.getAttribute('data-url'),{credentials:'same-origin',cache:'no-store'}).then(function(r){if(!r.ok)throw new Error(r.status);return r.json();})
.then(function(j){vals=j.env||[];shown=true;put();}).catch(function(e){b.lastChild.textContent='Could not reveal ('+e.message+')';});});})();"#;

fn health_panel(ctx: &Ctx, i: &Inspect) -> Markup {
    let hc = i.config.healthcheck.as_ref();
    let test = hc.map(|h| join_cmd(&h.test)).unwrap_or_default();
    let disabled = test.starts_with("NONE");
    let logs: Vec<&HealthLog> = i
        .state
        .health
        .as_ref()
        .and_then(|h| h.log.as_ref())
        .map(|l| l.iter().rev().take(HEALTH_LOGS).collect())
        .unwrap_or_default();
    html! {
        section.panel aria-label="Health check" {
            (panel_head("Health check", None))
            @match hc.filter(|_| !test.is_empty() && !disabled) {
                None => p.muted { "No health check configured." },
                Some(h) => {
                    dl.kv.cd-kv {
                        (kv_row("Test", html! { code { (test) } }))
                        (kv_row("Timing", html! { "every " (ns(h.interval)) " · timeout " (ns(h.timeout)) " · start period " (ns(h.start_period)) " · " (if h.retries > 0 { h.retries } else { 3 }) " retries" }))
                        @if let Some(st) = &i.state.health {
                            (kv_row("Status", html! { (pill(match st.status.as_str() { "healthy" => "ok", "unhealthy" => "bad", _ => "warn" }, &st.status))
                                @if st.failing_streak > 0 { " · failing " (st.failing_streak) " in a row" } }))
                        }
                    }
                    h3.cd-group { "Last " (logs.len()) " checks" }
                    @if logs.is_empty() { p.muted { "No checks have run yet." } } @else {
                        ol.cd-hlog {
                            @for l in &logs {
                                li {
                                    div.cd-hl-h {
                                        (pill(if l.exit_code == 0 { "ok" } else { "bad" }, &format!("exit {}", l.exit_code)))
                                        @if let Some(t) = crate::docker::parse_rfc3339(&l.start) {
                                            " " (time_tag(ctx.now, t.max(0) as u64))
                                            @if let Some(e) = crate::docker::parse_rfc3339(&l.end) { span.muted { " · took " (human_duration((e - t).max(0) as u64)) } }
                                        }
                                    }
                                    @let out = truncate(l.output.trim(), HEALTH_OUTPUT_MAX);
                                    @if out.is_empty() { p.muted.small { "(no output)" } } @else { pre.cd-out { (out) } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

// ---------------------------------------------------------------------------
// Processes

/// A sort key: numbers (with `%`, or a `ps` elapsed time) sort as numbers.
fn sort_key(s: &str) -> Result<f64, String> {
    let t = s.trim().trim_end_matches('%');
    if let Ok(v) = t.parse::<f64>() {
        return Ok(v);
    }
    // [[dd-]hh:]mm:ss
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) if d.chars().all(|c| c.is_ascii_digit()) => {
            (d.parse::<f64>().unwrap_or(0.0), r)
        }
        _ => (0.0, t),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        let v = parts
            .iter()
            .fold(0.0, |a, p| a * 60.0 + p.parse::<f64>().unwrap_or(0.0));
        return Ok(days * 86_400.0 + v);
    }
    Err(s.to_lowercase())
}

/// Sort rows by column `col`; numbers before text.
pub fn sort_rows(rows: &mut [Vec<String>], col: usize, desc: bool) {
    rows.sort_by(|a, b| {
        let ka = sort_key(a.get(col).map_or("", String::as_str));
        let kb = sort_key(b.get(col).map_or("", String::as_str));
        let o = match (ka, kb) {
            (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(x), Err(y)) => x.cmp(&y),
        };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
}

pub fn processes_view(
    c: &Container,
    top: &Result<Top, String>,
    q: &[(String, String)],
    base: &str,
) -> Markup {
    if !c.is_running() {
        return empty_state(
            "box",
            "Stopped",
            html! { "The container is not running, so it has no processes." },
        );
    }
    let t = match top {
        Ok(t) => t,
        Err(e) => {
            return empty_state(
                "alert",
                "Docker did not answer",
                html! { "Could not list processes: " (e) },
            )
        }
    };
    let default_col = t.titles.iter().position(|h| h == "%CPU");
    let col = query_get(q, "sort")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|c| *c < t.titles.len())
        .or(default_col);
    let desc = query_get(q, "dir").map_or(col == default_col, |d| d != "asc");
    let mut rows = t.processes.clone();
    if let Some(col) = col {
        sort_rows(&mut rows, col, desc);
    }
    let rss = t.titles.iter().position(|h| h == "RSS");
    html! {
        section.panel aria-label="Processes" {
            (panel_head(&format!("{} processes", rows.len()), None))
            div.cd-scroll {
                table.tbl.small.cd-procs {
                    thead { tr {
                        @for (n, h) in t.titles.iter().enumerate() {
                            @let here = col == Some(n);
                            @let next = if here && desc { "asc" } else { "desc" };
                            th scope="col" aria-sort=[here.then_some(if desc { "descending" } else { "ascending" })] {
                                a href={(base) "?tab=processes&sort=" (n) "&dir=" (next)} {
                                    (h) @if here { span aria-hidden="true" { (if desc { " ↓" } else { " ↑" }) } }
                                }
                            }
                        }
                    } }
                    tbody {
                        @for r in &rows { tr {
                            @for (n, v) in r.iter().enumerate() {
                                @if Some(n) == rss {
                                    td.num title=(v) { (v.parse::<u64>().map(|k| human_bytes(k * 1024)).unwrap_or_else(|_| v.clone())) }
                                } @else if n + 1 == r.len() {
                                    td.mono.cd-args { (v) }
                                } @else {
                                    td.mono { (v) }
                                }
                            }
                        } }
                    }
                }
            }
        }
    }
}

/// What an anonymous viewer may see of `inspect`. Command lines, health
/// check commands and their output, and host paths of mounts can carry
/// secrets (`--password=…`, tokens in URLs), so like env values they need a
/// session (the same rule as command lines on /host).
pub const HIDDEN: &str = "hidden: sign in to view";
pub fn redact_for_anonymous(mut i: crate::docker::Inspect) -> crate::docker::Inspect {
    let hide = || Some(vec![HIDDEN.to_string()]);
    if i.config.cmd.is_some() {
        i.config.cmd = hide();
    }
    if i.config.entrypoint.is_some() {
        i.config.entrypoint = hide();
    }
    i.config.env = i.config.env.map(|e| {
        e.into_iter()
            .map(|kv| kv.split('=').next().unwrap_or("").to_string())
            .collect()
    });
    if let Some(h) = i.config.healthcheck.as_mut() {
        if h.test.is_some() {
            h.test = hide();
        }
    }
    if let Some(h) = i.state.health.as_mut() {
        for l in h.log.iter_mut().flatten() {
            l.output = HIDDEN.to_string();
        }
    }
    for m in &mut i.mounts {
        if m.kind == "bind" {
            m.source = HIDDEN.to_string();
        }
    }
    i
}

fn processes_tab(ctx: &Ctx, c: &Container, q: &[(String, String)], base: &str) -> Markup {
    if ctx.user.is_none() {
        return html! {
            section.panel aria-label="Processes" {
                p.muted { "Sign in to see processes: command lines can contain secrets. "
                    a href={"/login?next=" (crate::ui::fmt::url_encode(&format!("{base}?tab=processes")))} { "Sign in" } }
            }
        };
    }
    let top = if c.is_running() {
        crate::docker::top_blocking(&ctx.state.env.docker_socket, &c.id)
    } else {
        Err("stopped".into())
    };
    processes_view(c, &top, q, base)
}

// ---------------------------------------------------------------------------
// Events

fn events_tab(ctx: &Ctx, c: &Container, q: &[(String, String)], base: &str) -> Markup {
    let kind = query_get(q, "kind")
        .and_then(|k| EVENT_KINDS.iter().find(|e| e.0 == k))
        .unwrap_or(&EVENT_KINDS[0]);
    let before = query_get(q, "before").and_then(|b| b.parse::<i64>().ok());
    const PAGE: usize = 100;
    let events = ctx.state.db.events(
        &EventFilter {
            subject: Some(c.name.clone()),
            kind_prefix: (!kind.2.is_empty()).then(|| kind.2.to_string()),
            before_id: before,
            limit: PAGE,
            ..Default::default()
        }
        .for_viewer(ctx.user.is_some()),
    );
    html! {
        section.panel aria-label="Events" {
            div.panel-h {
                h2 { "Events" }
                nav.seg aria-label="Kind" {
                    @for k in EVENT_KINDS {
                        a href={(base) "?tab=events&kind=" (k.0)} aria-current=[(k.0 == kind.0).then_some("true")] { (k.1) }
                    }
                }
            }
            @if events.is_empty() {
                p.muted {
                    @if !ctx.state.db.is_enabled() { "History is off, so no events are recorded." }
                    @else if before.is_some() { "No older events." }
                    @else { "No " (kind.1.to_lowercase()) " events recorded for this container." }
                }
            } @else {
                ul.events { @for e in &events { (event_row(ctx.now, e, false)) } }
                @if events.len() == PAGE {
                    @if let Some(last) = events.last() {
                        p { a.btn.sm href={(base) "?tab=events&kind=" (kind.0) "&before=" (last.id)} { "Older" } }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Why is this red?

/// What an exit code usually means.
pub fn exit_meaning(code: i64) -> String {
    match code {
        0 => "clean exit".into(),
        1 => "application error".into(),
        2 => "misuse or bad arguments".into(),
        125 => "Docker could not run it".into(),
        126 => "command not executable".into(),
        127 => "command not found".into(),
        130 => "interrupted (SIGINT)".into(),
        134 => "aborted (SIGABRT)".into(),
        137 => "killed (SIGKILL: out of memory or docker kill)".into(),
        139 => "crashed (SIGSEGV)".into(),
        143 => "terminated (SIGTERM)".into(),
        c if c > 128 && c < 160 => format!("killed by signal {}", c - 128),
        _ => "error".into(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Why {
    pub text: String,
    /// When it happened (unix s), for the logs link.
    pub ts: Option<i64>,
}

/// The newest `container.health` event for `name` if it says unhealthy.
fn last_unhealthy_event(ctx: &Ctx, name: &str) -> Option<i64> {
    ctx.state
        .db
        .events(&EventFilter {
            subject: Some(name.to_string()),
            kind_prefix: Some("container.health".into()),
            limit: 1,
            ..Default::default()
        })
        .first()
        .filter(|e| e.detail == "unhealthy")
        .map(|e| e.ts)
}

/// One line on what is wrong, or `None` when nothing is. `unhealthy_since`
/// is the newest "unhealthy" health event from the database.
pub fn why_red(
    c: &Container,
    ins: Option<&Inspect>,
    starts_10m: u32,
    unhealthy_since: Option<i64>,
    now: i64,
) -> Option<Why> {
    let mut parts: Vec<String> = Vec::new();
    let mut ts = None;
    let st = ins.map(|i| &i.state);
    let running = c.is_running();
    if st.is_some_and(|s| s.oom_killed) && !running {
        parts.push("Killed: out of memory (OOM)".into());
        ts = st.and_then(|s| crate::docker::parse_rfc3339(&s.finished_at));
    }
    let code = st.filter(|_| !running).map(|s| s.exit_code).or(c.exit_code);
    if !running && c.state != "created" {
        if let Some(code) = code.filter(|c| *c != 0) {
            parts.push(format!("Exited with code {code}: {}", exit_meaning(code)));
            ts = ts.or_else(|| st.and_then(|s| crate::docker::parse_rfc3339(&s.finished_at)));
        }
    }
    if c.state == "dead" {
        parts.push("Dead: Docker could not stop or remove it".into());
    }
    if c.state == "restarting" {
        parts.push("Restarting".into());
    }
    let health = st.and_then(|s| s.health.as_ref());
    if running && c.health.as_deref() == Some("unhealthy") {
        // Start of the trailing failing run in Docker's last five checks.
        let logs = health.and_then(|h| h.log.as_deref()).unwrap_or(&[]);
        let run: Vec<&HealthLog> = logs.iter().rev().take_while(|l| l.exit_code != 0).collect();
        let first = run
            .last()
            .and_then(|l| crate::docker::parse_rfc3339(&l.start));
        let since = unhealthy_since.or(first);
        let streak = health.map_or(0, |h| h.failing_streak);
        let mut t = String::from("Health check failing");
        if let Some(s) = since {
            t.push_str(&format!(
                " since {} ({} ago)",
                short_utc(s.max(0) as u64),
                human_duration((now - s).max(0) as u64)
            ));
        }
        if streak > 0 {
            t.push_str(&format!(", {streak} in a row"));
        }
        if let Some(out) = run
            .first()
            .map(|l| l.output.trim())
            .filter(|o| !o.is_empty())
        {
            let line = out.lines().next().unwrap_or("");
            t.push_str(&format!(": {}", truncate(line, 120)));
        }
        parts.push(t);
        ts = ts.or(since);
    }
    if starts_10m >= 3
        || c.die_times
            .iter()
            .filter(|t| **t as i64 >= now - 600)
            .count()
            >= 3
    {
        let n = starts_10m.max(
            c.die_times
                .iter()
                .filter(|t| **t as i64 >= now - 600)
                .count() as u32,
        );
        parts.push(format!("Restart loop: {n} restarts in 10 min"));
        ts = ts.or_else(|| c.die_times.iter().max().map(|t| *t as i64));
    }
    if parts.is_empty() {
        return None;
    }
    Some(Why {
        text: parts.join(" · "),
        ts,
    })
}

/// The proper 404: same shell, a sentence, and near matches.
fn missing(ctx: &Ctx, name: &str, all: &[Container], err: Option<&str>) -> (StatusCode, String) {
    if let Some(err) = err {
        let body = html! {
            div.page {
                (empty_state("alert", "Docker is not reachable", html! {
                    "The container list could not be read (" (err) "), so " code { (name) } " cannot be shown. "
                    "The page fills in again once Docker answers."
                }))
            }
        };
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            layout(
                ctx,
                Nav::Containers,
                "Docker unreachable",
                html! { nav.crumb { a href="/containers" { "Containers" } (ic("chev-r")) h1.title { "Docker unreachable" } } },
                body,
            ),
        );
    }
    let needle = name.to_lowercase();
    let near: Vec<&Container> = all
        .iter()
        .filter(|c| {
            !needle.is_empty()
                && (c.name.to_lowercase().contains(&needle)
                    || needle.contains(&c.name.to_lowercase()))
        })
        .take(6)
        .collect();
    let body = html! {
        div.page {
            (empty_state("box", "No such container", html! {
                "There is no container named " code { (name) } ". It may have been removed or renamed. "
                a href="/containers" { "See all containers" }
            }))
            @if !near.is_empty() {
                section.panel {
                    (panel_head("Did you mean", None))
                    ul.rows {
                        @for c in near { li { a.nm href={"/containers/" (url_encode(&c.name))} { (c.name) } } }
                    }
                }
            }
        }
    };
    (
        StatusCode::NOT_FOUND,
        layout(
            ctx,
            Nav::Containers,
            "Container not found",
            html! { nav.crumb { a href="/containers" { "Containers" } (ic("chev-r")) h1.title { "Not found" } } },
            body,
        ),
    )
}

/// Live numbers for every listed container, sent with each container update
/// instead of re-rendering whole tables. The script updates the details
/// header and the containers table in place; `sig` changes when anything
/// but the numbers changed (a container came or went, its state, ports or
/// links), and then the script fetches a fresh table.
/// `{"sig": "...", "c": {name: [label, dot class, state text, cpu text, mem text, cpu, mem]}}`.
pub fn live_json(groups: &[GroupView]) -> Markup {
    let (sig, m) = live_data(groups);
    let j = serde_json::json!({ "sig": sig, "c": m }).to_string();
    html! { div #ctr-live hidden data-j=(j) {} }
}

/// The table's signature (set on `#ctable`, compared by the script).
pub fn live_sig(groups: &[GroupView]) -> String {
    live_data(groups).0
}

fn live_data(groups: &[GroupView]) -> (String, serde_json::Map<String, serde_json::Value>) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut m = serde_json::Map::new();
    for g in groups {
        g.name.hash(&mut h);
        for c in g.cards.iter().filter(|c| c.container) {
            (
                &c.name,
                &c.title,
                c.label,
                &c.state_text,
                &c.image,
                &c.url,
                &c.logs_url,
            )
                .hash(&mut h);
            (c.restarts, c.exit_code, c.cpu.is_some()).hash(&mut h);
            for p in &c.ports {
                (&p.label, &p.url, p.local).hash(&mut h);
            }
            m.insert(
                c.name.clone(),
                serde_json::json!([
                    c.label,
                    c.dot.map(|d| d.class()).unwrap_or("off"),
                    c.state_text.as_deref().unwrap_or(""),
                    c.cpu.map(pct).unwrap_or_else(|| "—".into()),
                    c.mem.map(human_bytes).unwrap_or_else(|| "—".into()),
                    c.cpu,
                    c.mem,
                ]),
            );
        }
    }
    (format!("{:016x}", h.finish()), m)
}

fn head_live(card: &CardView, c: &Container, restarts_7d: Option<u32>) -> Markup {
    let dot = card.dot.map(|d| d.class()).unwrap_or("off");
    html! {
        section.strip.facts data-cd-name=(c.name) aria-label="Status" {
            div.cell {
                div.cell-h { span.label { "State" } }
                div.big {
                    span class={"st-l lg " (dot)} data-cd="l" { (card.label) }
                }
                div.sub data-cd="s" { (card.state_text.as_deref().unwrap_or("")) }
            }
            div.cell {
                div.cell-h { (ic("cpu")) span.label { "CPU" } }
                div.big.num data-cd="c" { @match card.cpu { Some(v) => (pct(v)), None => { "—" } } }
                div.sub { "of one core" }
                (live_spark(&format!("c:{}:cpu", c.name), card.cpu, "--ch-cpu", None))
            }
            div.cell {
                div.cell-h { (ic("memory")) span.label { "Memory" } }
                div.big.num data-cd="m" { @match card.mem { Some(m) => (human_bytes(m)), None => { "—" } } }
                div.sub { "resident" }
                (live_spark(&format!("c:{}:mem", c.name), card.mem.map(|m| m as f64), "--ch-mem", None))
            }
            div.cell {
                div.cell-h { (ic("refresh")) span.label { "Exits, last hour" } }
                div.big.num { (c.die_times.len()) }
                div.sub {
                    @if let Some(n) = restarts_7d { (n) " starts in 7 days" }
                    @else if let Some(code) = card.exit_code { "last exit code " (code) }
                    @else { "since the app started watching" }
                }
            }
            div.cell {
                div.cell-h { (ic("activity")) span.label { "Health check" } }
                div.big { (c.health.as_deref().unwrap_or("none")) }
                div.sub { "id " code { (c.id.chars().take(12).collect::<String>()) } }
            }
        }
    }
}

fn ports_table(c: &Container, host: &str) -> Markup {
    if c.ports.is_empty() {
        return html! { p.muted { "No published ports." } };
    }
    html! {
        table.tbl.small {
            thead { tr { th scope="col" { "Host" } th scope="col" { "Container" } th scope="col" { "Link" } } }
            tbody {
                @for p in &c.ports {
                    tr {
                        td.mono { (if p.host_ip.is_empty() { "*" } else { p.host_ip.as_str() }) ":" (p.host_port) }
                        td.mono { (p.container_port) "/" (p.proto) }
                        td {
                            @match binding_link(p, host) {
                                Ok(u) => a.mono href=(u) target="_blank" rel="noopener noreferrer" { (u) },
                                Err(why) => span.muted { (why) },
                            }
                        }
                    }
                }
            }
        }
    }
}

fn labels_panel(c: &Container) -> Markup {
    html! {
        section.panel aria-label="Labels" {
            (panel_head("Labels", None))
            @if c.labels.is_empty() {
                p.muted { "No labels." }
            } @else {
                dl.kv {
                    @for (k, v) in &c.labels {
                        dt.mono { (k) }
                        dd.mono { (v) }
                    }
                }
            }
        }
    }
}

/// Container alert rules that cover `name` (same include/exclude globs the
/// engine uses), with whether each is firing for this container right now.
pub fn matching_rules<'a>(
    rules: &'a [crate::alerts::RuleConfig],
    name: &str,
) -> Vec<&'a crate::alerts::RuleConfig> {
    rules
        .iter()
        .filter(|r| r.kind == RuleKind::Container)
        .filter(|r| {
            let inc = r.containers.as_deref().unwrap_or(&[]);
            let exc = r.exclude.as_deref().unwrap_or(&[]);
            (inc.is_empty() || inc.iter().any(|p| glob_match(p, name)))
                && !exc.iter().any(|p| glob_match(p, name))
        })
        .collect()
}

fn rules_panel(ctx: &Ctx, c: &Container) -> Markup {
    let rules = matching_rules(&ctx.cfg.alerts, &c.name);
    let status = ctx.state.alerts.lock().unwrap().clone();
    html! {
        section.panel aria-label="Alert rules" {
            (panel_head("Alert rules", Some(("/alerts", "All alerts"))))
            @if rules.is_empty() {
                p.muted { "No alert rule covers this container." }
            } @else {
                ul.rows {
                    @for r in rules {
                        @let firing = status.incidents.iter().find(|i| i.rule == r.name && i.subject == c.name);
                        li {
                            @if firing.is_some() { span.dot.bad role="img" aria-label="firing" {} } @else { span.dot.ok role="img" aria-label="ok" {} }
                            span.nm { (r.name) }
                            span.why {
                                @if let Some(i) = firing { (i.state) }
                                @else if !r.enabled() { "disabled" }
                                @else { "watching: " (r.on.as_deref().map(|v| v.join(", ")).unwrap_or_else(|| "stopped, unhealthy, restarting".into())) }
                            }
                            @if r.channels.is_empty() { span.when { "page only" } }
                            @else { span.when { (r.channels.join(", ")) } }
                        }
                    }
                }
            }
        }
    }
}

/// `/api/containers/<name>/<what>` (GET): `env` (values; signed-in only,
/// 401 otherwise), `health` (last health-check outputs), `top` (processes).
pub async fn api(
    state: &std::sync::Arc<crate::state::AppState>,
    req: &hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<crate::http::server::Body> {
    let path = req.uri().path().to_string();
    let headers = req.headers().clone();
    let st = state.clone();
    match tokio::task::spawn_blocking(move || {
        let socket = st.env.docker_socket.clone();
        api_inner(
            &st,
            &path,
            &headers,
            &|id| crate::docker::inspect_blocking(&socket, id),
            &|id| crate::docker::top_blocking(&socket, id),
        )
    })
    .await
    {
        Ok(r) => r,
        Err(_) => crate::db::api::json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed"),
    }
}

type InspectFn<'a> = &'a dyn Fn(&str) -> Result<Inspect, String>;
type TopFn<'a> = &'a dyn Fn(&str) -> Result<Top, String>;

fn api_inner(
    state: &crate::state::AppState,
    path: &str,
    headers: &HeaderMap,
    inspect: InspectFn,
    top: TopFn,
) -> hyper::Response<crate::http::server::Body> {
    use crate::db::api::json_error;
    let rest = path.strip_prefix("/api/containers/").unwrap_or("");
    let Some((raw, what)) = rest.rsplit_once('/') else {
        return json_error(StatusCode::NOT_FOUND, "not found");
    };
    let name = url_decode(raw);
    let snap = state.snapshot();
    let Some(c) = snap.containers.iter().find(|c| c.name == name) else {
        return json_error(StatusCode::NOT_FOUND, "no such container");
    };
    let v = match what {
        "env" => {
            // The session check comes first: nothing is fetched for an
            // anonymous request.
            if crate::auth::current_user(state, headers).is_none() {
                return json_error(
                    StatusCode::UNAUTHORIZED,
                    "sign in to reveal environment values",
                );
            }
            match inspect(&c.id) {
                Ok(i) => serde_json::json!({ "env": i.config.env_pairs() }),
                Err(e) => return json_error(StatusCode::BAD_GATEWAY, &e),
            }
        }
        "health" | "top" if crate::auth::current_user(state, headers).is_none() => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "sign in to see commands and health-check output",
            );
        }
        "health" => match inspect(&c.id) {
            Ok(i) => {
                let h = i.state.health.unwrap_or_default();
                let log: Vec<_> = h
                    .log
                    .unwrap_or_default()
                    .into_iter()
                    .rev()
                    .take(HEALTH_LOGS)
                    .map(|l| {
                        serde_json::json!({
                            "start": l.start, "end": l.end, "exit_code": l.exit_code,
                            "output": truncate(&l.output, HEALTH_OUTPUT_MAX),
                        })
                    })
                    .collect();
                serde_json::json!({ "status": h.status, "failing_streak": h.failing_streak, "log": log })
            }
            Err(e) => return json_error(StatusCode::BAD_GATEWAY, &e),
        },
        "top" => {
            if !c.is_running() {
                return json_error(StatusCode::CONFLICT, "container is not running");
            }
            match top(&c.id) {
                Ok(t) => serde_json::json!({ "titles": t.titles, "processes": t.processes }),
                Err(e) => return json_error(StatusCode::BAD_GATEWAY, &e),
            }
        }
        _ => return json_error(StatusCode::NOT_FOUND, "not found"),
    };
    let mut r = crate::http::server::json(&v);
    crate::http::server::set(&mut r, CACHE_CONTROL, "no-store");
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::RuleConfig;

    fn rules(toml: &str) -> Vec<RuleConfig> {
        crate::config::parse(toml, "t.toml").unwrap().alerts
    }

    #[test]
    fn rule_matching_uses_include_and_exclude_globs() {
        let rs = rules(
            r#"
[[alerts]]
name = "all"
type = "container"
[[alerts]]
name = "web"
type = "container"
containers = ["web-*"]
exclude = ["web-test"]
[[alerts]]
name = "disk"
type = "host"
metric = "disk"
above = 90
"#,
        );
        let names = |n: &str| -> Vec<String> {
            matching_rules(&rs, n)
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        assert_eq!(names("web-1"), ["all", "web"]);
        assert_eq!(names("web-test"), ["all"]);
        assert_eq!(names("db"), ["all"]);
    }

    // ---- container detail ----

    use crate::ui::testutil::{sample, state_with};
    use std::sync::Arc;

    const SECRET: &str = "s3cr3t-VALUE-9f8e7d";

    /// Trimmed `docker inspect` of a real container (Docker 29.5), with a
    /// secret in the environment and markup in a health-check output.
    fn fixture() -> String {
        format!(
            r#"{{"Id":"6e6562000000000000000000000000000000000000000000000000000000000000","Created":"2026-11-14T08:00:00.123456789Z",
"Path":"/entry.sh","Args":["serve"],
"State":{{"Status":"running","Running":true,"Paused":false,"Restarting":false,"OOMKilled":false,"Dead":false,
  "Pid":4242,"ExitCode":0,"Error":"","StartedAt":"2026-11-14T09:00:00.5Z","FinishedAt":"0001-01-01T00:00:00Z",
  "Health":{{"Status":"unhealthy","FailingStreak":4,"Log":[
    {{"Start":"2026-11-14T09:58:00+01:00","End":"2026-11-14T09:58:01+01:00","ExitCode":0,"Output":"ok"}},
    {{"Start":"2026-11-14T10:00:00+01:00","End":"2026-11-14T10:00:02+01:00","ExitCode":1,"Output":"<b>connection refused</b> on :8080\nmore"}},
    {{"Start":"2026-11-14T10:01:00+01:00","End":"2026-11-14T10:01:02+01:00","ExitCode":1,"Output":"<b>connection refused</b> on :8080"}}]}}}},
"Image":"sha256:abcdef0123","Name":"/web","RestartCount":3,
"HostConfig":{{"NetworkMode":"shop_default","RestartPolicy":{{"Name":"unless-stopped","MaximumRetryCount":0}},
  "Memory":536870912,"NanoCpus":1500000000,"PidsLimit":256,"Privileged":false,"ReadonlyRootfs":true,"CapAdd":["NET_ADMIN"]}},
"Mounts":[{{"Type":"bind","Source":"/srv/web","Destination":"/data","Mode":"ro","RW":false}},
          {{"Type":"volume","Name":"web_cache","Source":"/var/lib/docker/volumes/web_cache/_data","Destination":"/cache","Mode":"z","RW":true}}],
"Config":{{"User":"1000:1000","Env":["PATH=/usr/bin","DB_PASSWORD={SECRET}","EMPTY="],"Cmd":["serve","--port","8080"],
  "Image":"img/web:latest","WorkingDir":"/app","Entrypoint":["/entry.sh"],
  "Healthcheck":{{"Test":["CMD-SHELL","curl -f http://localhost:8080/ || exit 1"],"Interval":30000000000,"Timeout":5000000000,"Retries":3}}}},
"NetworkSettings":{{"Networks":{{"shop_default":{{"IPAddress":"172.18.0.5","Gateway":"172.18.0.1","MacAddress":"02:42:ac:12:00:05","GlobalIPv6Address":"","Aliases":["web"]}}}}}}}}"#
        )
    }

    fn ins() -> Inspect {
        crate::docker::parse_inspect(fixture().as_bytes()).unwrap()
    }

    #[test]
    fn anonymous_inspect_hides_commands_health_and_env_values() {
        let r = redact_for_anonymous(ins());
        let dump = format!("{r:?}");
        for secret in [SECRET, "--port", "curl -f", "/entry.sh"] {
            assert!(!dump.contains(secret), "{secret} leaked to anonymous view");
        }
        assert!(dump.contains("DB_PASSWORD"), "env keys stay visible");
        // Signed in sees the full data.
        assert!(format!("{:?}", ins()).contains("--port"));
    }

    fn ctx_for<'a>(st: &'a Arc<crate::state::AppState>, query: &'a str, user: bool) -> Ctx<'a> {
        Ctx {
            state: st,
            cfg: st.config.current(),
            host: "localhost",
            path: "/containers/web",
            query,
            user: user.then(|| "admin".to_string()),
            csrf: None,
            now: 1_794_650_000,
        }
    }

    /// A valid session cookie (stored the way `auth` stores it).
    fn signed_in(st: &Arc<crate::state::AppState>) -> HeaderMap {
        let token = "cd".repeat(32);
        let hash: String = ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let now = crate::util::unix_now() as i64;
        assert!(st.db.create_session(&hash, now, now + 3600));
        let mut h = HeaderMap::new();
        h.insert(
            "cookie",
            hyper::header::HeaderValue::from_str(&format!("rh_session={token}")).unwrap(),
        );
        h
    }

    #[test]
    fn parses_inspect_fixture() {
        let i = ins();
        assert_eq!(i.name, "/web");
        assert_eq!(i.restart_count, 3);
        assert_eq!(i.state.pid, 4242);
        assert_eq!(ctrx::net_source_of(&i), NetSource::Pid(4242));
        assert_eq!(i.config.env_keys(), ["PATH", "DB_PASSWORD", "EMPTY"]);
        assert_eq!(i.config.env_pairs()[1], ("DB_PASSWORD", SECRET));
        assert_eq!(i.config.env_pairs()[2], ("EMPTY", ""));
        assert_eq!(i.host_config.memory, 536_870_912);
        assert_eq!(
            i.host_config.restart_policy.as_ref().unwrap().name,
            "unless-stopped"
        );
        assert_eq!(i.mounts.len(), 2);
        assert!(!i.mounts[0].rw);
        let h = i.state.health.as_ref().unwrap();
        assert_eq!(h.failing_streak, 4);
        assert_eq!(h.log.as_ref().unwrap().len(), 3);
        assert_eq!(join_cmd(&i.config.cmd), "serve --port 8080");
        // Docker's zero time is "never".
        assert_eq!(crate::docker::parse_rfc3339(&i.state.finished_at), None);
        assert_eq!(crate::docker::parse_rfc3339("1970-01-02T00:00:00Z"), None);
        assert_eq!(
            crate::docker::parse_rfc3339("2026-11-14T10:00:00+01:00"),
            Some(1_794_646_800)
        );
        assert_eq!(
            crate::docker::parse_rfc3339("2026-11-14T09:00:00.999Z"),
            Some(1_794_646_800)
        );
        assert!(crate::docker::parse_inspect(b"[1,2]").is_err());
    }

    #[test]
    fn env_values_never_reach_the_page() {
        let st = state_with(sample(), "");
        let i: Result<Inspect, String> = Ok(ins());
        for user in [false, true] {
            let ctx = ctx_for(&st, "tab=inspect", user);
            let html = inspect_tab(&ctx, &sample()[0], &i).into_string();
            assert!(!html.contains(SECRET), "value leaked (signed in: {user})");
            assert!(html.contains("DB_PASSWORD"));
            assert_eq!(
                html.contains("cd-reveal"),
                user,
                "reveal button only when signed in"
            );
            // Health output is escaped.
            assert!(html.contains("&lt;b&gt;connection refused&lt;/b&gt;"));
            assert!(!html.contains("<b>connection"));
        }
    }

    #[test]
    fn env_api_needs_a_session() {
        let st = state_with(sample(), "");
        let calls = std::cell::Cell::new(0);
        let insp = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(ins())
        };
        let top = |_: &str| Err("no docker".to_string());
        let body = |r: hyper::Response<crate::http::server::Body>| {
            let s = r.status();
            let b = futures_body(r);
            (s, b)
        };
        // Anonymous: 401, nothing fetched, no value.
        let (s, b) = body(api_inner(
            &st,
            "/api/containers/web/env",
            &HeaderMap::new(),
            &insp,
            &top,
        ));
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(!b.contains(SECRET));
        assert_eq!(calls.get(), 0);
        // A forged cookie is still anonymous.
        let mut forged = HeaderMap::new();
        forged.insert("cookie", "rh_session=00".parse().unwrap());
        let (s, b) = body(api_inner(
            &st,
            "/api/containers/web/env",
            &forged,
            &insp,
            &top,
        ));
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(!b.contains(SECRET));
        // Other anonymous endpoints carry no env either.
        let (s, b) = body(api_inner(
            &st,
            "/api/containers/web/health",
            &HeaderMap::new(),
            &insp,
            &top,
        ));
        // Health output and process lists need a session (they can carry secrets).
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(!b.contains(SECRET));
        let (s, _) = body(api_inner(
            &st,
            "/api/containers/web/top",
            &HeaderMap::new(),
            &insp,
            &top,
        ));
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = body(api_inner(
            &st,
            "/api/containers/nope/env",
            &HeaderMap::new(),
            &insp,
            &top,
        ));
        assert_eq!(s, StatusCode::NOT_FOUND);
        // Signed in: the values.
        let h = signed_in(&st);
        let (s, b) = body(api_inner(&st, "/api/containers/web/env", &h, &insp, &top));
        assert_eq!(s, StatusCode::OK);
        assert!(b.contains(SECRET));
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["env"][1], serde_json::json!(["DB_PASSWORD", SECRET]));
    }

    /// Collect a full body synchronously (it is always a `Full`).
    fn futures_body(r: hyper::Response<crate::http::server::Body>) -> String {
        use http_body_util::BodyExt;
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let b = rt.block_on(r.into_body().collect()).unwrap().to_bytes();
        String::from_utf8(b.to_vec()).unwrap()
    }

    #[test]
    fn parses_and_sorts_top() {
        let t = crate::docker::parse_top(
            br#"{"Processes":[["7","root","0.5","0.1","2048","01:02","sh -c x"],["12","app","12.0","3.0","51200","1-02:03:04","/srv <app>"],["9","app","2.5","1.0","100","05:00","b"]],"Titles":["PID","USER","%CPU","%MEM","RSS","ELAPSED","COMMAND"]}"#,
        )
        .unwrap();
        let mut rows = t.processes.clone();
        sort_rows(&mut rows, 2, true);
        assert_eq!(
            rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(),
            ["12", "9", "7"]
        );
        sort_rows(&mut rows, 5, false);
        assert_eq!(
            rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(),
            ["7", "9", "12"]
        );
        sort_rows(&mut rows, 1, false);
        assert_eq!(rows[0][1], "app");
        let c = &sample()[0];
        let q = parse_query("tab=processes&sort=4&dir=asc");
        let html = processes_view(c, &Ok(t.clone()), &q, "/containers/web").into_string();
        assert!(html.contains("&lt;app&gt;"));
        assert!(html.contains("aria-sort=\"ascending\""));
        assert!(html.contains("50.0 MiB"), "{html}");
        let stopped = &sample()[1];
        assert!(processes_view(stopped, &Ok(t), &q, "/x")
            .into_string()
            .contains("Stopped"));
        assert!(processes_view(c, &Err("boom".into()), &q, "/x")
            .into_string()
            .contains("boom"));
    }

    #[test]
    fn why_is_this_red() {
        let now = 1_794_650_000;
        let mut c = sample()[0].clone();
        // Healthy and running: nothing.
        assert_eq!(why_red(&c, None, 0, None, now), None);
        // Unhealthy: since when, streak, last output.
        c.health = Some("unhealthy".into());
        let w = why_red(&c, Some(&ins()), 0, None, now).unwrap();
        assert!(
            w.text.starts_with("Health check failing since"),
            "{}",
            w.text
        );
        assert!(w.text.contains("4 in a row"));
        assert!(w.text.contains("<b>connection refused</b> on :8080"));
        assert_eq!(w.ts, Some(1_794_646_800)); // 10:00 +01:00, start of the failing run
                                               // The database event wins when there is one.
        let w = why_red(&c, Some(&ins()), 0, Some(now - 7200), now).unwrap();
        assert_eq!(w.ts, Some(now - 7200));
        // Exited 137 with OOMKilled.
        let mut i = ins();
        i.state.running = false;
        i.state.oom_killed = true;
        i.state.exit_code = 137;
        i.state.finished_at = "2026-11-14T10:30:00Z".into();
        let mut d = sample()[1].clone();
        d.exit_code = Some(137);
        let w = why_red(&d, Some(&i), 0, None, now).unwrap();
        assert!(
            w.text.contains("out of memory") && w.text.contains("code 137"),
            "{}",
            w.text
        );
        assert_eq!(w.ts, crate::docker::parse_rfc3339("2026-11-14T10:30:00Z"));
        // 143 and 1, from the list alone.
        d.exit_code = Some(143);
        assert!(why_red(&d, None, 0, None, now)
            .unwrap()
            .text
            .contains("SIGTERM"));
        d.exit_code = Some(1);
        assert!(why_red(&d, None, 0, None, now)
            .unwrap()
            .text
            .contains("application error"));
        d.exit_code = Some(0);
        assert_eq!(why_red(&d, None, 0, None, now), None);
        // Restart loop.
        let mut r = sample()[0].clone();
        r.die_times = vec![(now - 60) as u64, (now - 200) as u64, (now - 400) as u64];
        let w = why_red(&r, None, 2, None, now).unwrap();
        assert!(w.text.contains("Restart loop: 3 restarts in 10 min"));
        assert_eq!(w.ts, Some(now - 60));
        assert!(why_red(&r, None, 5, None, now)
            .unwrap()
            .text
            .contains("5 restarts"));
        assert_eq!(
            exit_meaning(137),
            "killed (SIGKILL: out of memory or docker kill)"
        );
        assert_eq!(exit_meaning(130 + 11), "killed by signal 13");
    }

    #[test]
    fn every_tab_renders_and_escapes() {
        let st = state_with(sample(), "");
        let evil = "<script>alert(1)</script>";
        st.db.record_event(
            1_794_640_000,
            "container.die",
            "warn",
            evil,
            "exit code <1>",
        );
        st.db.record_event(
            1_794_640_001,
            "auth.login_failed",
            "warn",
            evil,
            "ip 10.0.0.9",
        );
        for tab in Tab::ALL {
            for name in ["web", "db", evil] {
                let q = format!("tab={}&range=7d", tab.slug());
                let ctx = ctx_for(&st, &q, false);
                let (s, html) = page(&ctx, name);
                assert_eq!(s, StatusCode::OK, "{name} {tab:?}");
                assert!(!html.contains(evil), "unescaped name on {tab:?}");
                assert!(html.contains("aria-current=\"page\""));
                if tab == Tab::Events && name == evil {
                    assert!(html.contains("exit code &lt;1&gt;"));
                    assert!(!html.contains("10.0.0.9"), "auth events shown anonymously");
                }
            }
        }
        // Stopped container: the why line, and the live sections say stopped.
        let ctx = ctx_for(&st, "tab=processes", false);
        let (_, html) = page(&ctx, "db");
        assert!(html.contains("Why is this red?") && html.contains("code 1"));
        assert!(
            html.contains("Sign in to see processes"),
            "anonymous: no process list"
        );
        let ctx = ctx_for(&st, "tab=processes", true);
        let (_, html) = page(&ctx, "db");
        assert!(html.contains("not running"));
        // Unknown container and bad tab values.
        let ctx = ctx_for(&st, "tab=nope&range=99y&sort=x&before=-1", false);
        assert_eq!(page(&ctx, "web").0, StatusCode::OK);
        assert_eq!(page(&ctx, "missing").0, StatusCode::NOT_FOUND);
        // Docker unreachable and the container unknown: say so.
        let mut snap = (*st.snapshot()).clone();
        snap.error = Some("connection refused".into());
        st.containers.send_replace(Arc::new(snap));
        let (s, html) = page(&ctx, "missing");
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert!(html.contains("Docker is not reachable"));
        assert!(page(&ctx, "web").1.contains("Docker is not reachable"));
    }
}
