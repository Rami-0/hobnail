//! `/`: health strip, host vitals with 1 h charts, what needs attention, top
//! consumers, widgets, recent events and bookmarks.

use maud::{html, Markup};
use std::sync::Arc;

use crate::alerts::Status;
use crate::db::{Db, EventFilter, Range, SysMetric};
use crate::model::{Snapshot, SystemStats};
pub use crate::ui::chart::live_spark;
use crate::ui::chart::{line_chart, line_chart_ex, seed, ChartSpec, Unit};
use crate::ui::fmt::{pct, time_tag, url_encode};
use crate::ui::pages::{crumb_title, kind_label};
use crate::ui::shell::{disk_percent, layout, level, mem_percent, Ctx, Nav};
use crate::ui::view::{build_groups, CardView, GroupView, Sev};
use crate::ui::{ic, panel_head, pill, status_dot};
use crate::util::{human_bytes, human_duration};

const NONE: &str = "—";

/// What the live regions of several pages are built from (one snapshot).
pub struct Live {
    pub snap: Arc<Snapshot>,
    pub groups: Vec<GroupView>,
    pub status: Status,
}

impl Live {
    pub fn new(ctx: &Ctx) -> Self {
        let snap = ctx.state.snapshot();
        let stats = crate::ui::view::stats_with_history(ctx.state, &ctx.state.snapshot());
        let groups = build_groups(&ctx.cfg, &snap, &stats, ctx.host);
        let status = ctx.state.alerts.lock().unwrap().clone();
        Self {
            snap,
            groups,
            status,
        }
    }

    pub fn containers(&self) -> impl Iterator<Item = &CardView> {
        self.groups
            .iter()
            .flat_map(|g| g.cards.iter())
            .filter(|c| c.container)
    }

    fn known(&self) -> bool {
        self.snap.fetched_at != 0 && self.snap.error.is_none()
    }
}

pub fn page(ctx: &Ctx) -> String {
    let cfg = &ctx.cfg;
    let v = Live::new(ctx);
    let db = &ctx.state.db;
    let now = ctx.now as i64;
    let mut sys = ctx.state.system.lock().unwrap().1.clone();
    crate::ui::view::seed_cpu(ctx.state, &mut sys, now);
    let strips = crate::ui::view::strips(ctx.state, now);
    let has_marks = v
        .groups
        .iter()
        .any(|g| g.cards.iter().any(|c| !c.container));
    let body = html! {
        div.page.ov {
            div.ov-head {
                div.ov-hi {
                    h1.display #greeting data-text=[cfg.page.greeting.as_deref()] {
                        (cfg.page.greeting.as_deref().unwrap_or("Hello"))
                    }
                    p.since #since hidden aria-live="polite" {}
                }
                div.ov-meta {
                    span #clock.num { "--:--" }
                    span.upd #upd title="Time since the last live update" {}
                }
            }
            section.panel.primary.pulse aria-label="Status" {
                (health(&v))
                (attention(ctx.now, &v))
                (stack_strips(&v, &strips))
            }
            @if cfg.system.enabled {
                section.sec aria-labelledby="h-sys" {
                    div.sec-h {
                        h2.label #h-sys { "Host" }
                        a.more href="/host" { "Host details" (ic("arrow-right")) }
                    }
                    (sys_strip(sys.as_ref(), Some((db, now))))
                    (hour_charts(ctx))
                }
            }
            (top(&v, cfg.page.show_stats))
            @if !cfg.widgets.is_empty() {
                section.sec.ghost aria-labelledby="h-w" {
                    div.sec-h { h2.label #h-w { "Widgets" } }
                    div.widgets {
                        @for w in &cfg.widgets {
                            @let out = ctx.state.widgets.lock().unwrap().get(&w.id).cloned();
                            (crate::ui::widgets::render(w, out.as_ref()))
                        }
                    }
                }
            }
            div.cols.c-ev[!has_marks] {
                (recent_events(ctx))
                @if has_marks { (bookmarks(&v)) }
            }
        }
    };
    layout(
        ctx,
        Nav::Overview,
        "Overview",
        crumb_title("Overview"),
        body,
    )
}

// ---------------------------------------------------------------- health strip

pub fn health(v: &Live) -> Markup {
    let cards: Vec<&CardView> = v.containers().collect();
    let running = cards.iter().filter(|c| c.running).count();
    let stopped = cards.len() - running;
    let unhealthy = cards
        .iter()
        .filter(|c| matches!(c.label, "unhealthy" | "restarting" | "dead"))
        .count();
    let firing = v.status.incidents.len();
    let known = v.known();
    let total = cards.len().max(1) as f64;
    // Calm by default: a zero is small and grey, a problem count takes its
    // status colour (DESIGN §10).
    let cell = |n: Option<usize>, label: &str, href: &str, bad: bool, hint: &str| {
        let hot = bad && n.unwrap_or(0) > 0;
        let zero = n == Some(0);
        html! {
            a.hs-cell.hot[hot].zero[zero] href=(href) title=(hint) {
                span.hs-n.tw {
                    @match n {
                        Some(n) => (n),
                        None if v.snap.error.is_some() => (NONE),
                        None => span.sk { span.sr { "loading" } },
                    }
                }
                span.hs-l { (label) }
            }
        }
    };
    html! {
        section #ov-health.hs aria-label="Container health" {
            (cell(known.then_some(running), "running", "/containers?state=running", false, "Containers that are running"))
            (cell(known.then_some(stopped), "stopped", "/containers?state=stopped", false, "Containers that are not running"))
            (cell(known.then_some(unhealthy), "unhealthy", "/containers?state=problem", true, "Unhealthy, restarting or dead"))
            (cell(Some(firing), "alerts firing", "/alerts", true, "Open incidents"))
            @if known && !cards.is_empty() {
                div.hs-bar role="img" aria-label={(running) " of " (cards.len()) " running"} {
                    span.r style={"width:" (format!("{:.2}", running as f64 / total * 100.0)) "%"} {}
                    @if unhealthy > 0 {
                        span.u style={"width:" (format!("{:.2}", unhealthy as f64 / total * 100.0)) "%"} {}
                    }
                }
            } @else if let Some(e) = &v.snap.error {
                p.hs-err { (ic("alert")) "Docker unreachable: " (e) }
            }
        }
    }
}

// ---------------------------------------------------------------- attention

pub fn attention(now: u64, v: &Live) -> Markup {
    // One row per subject, ranked by severity: a container with a firing
    // alert is one red row ("unhealthy · alert firing 5m"), not two.
    let mut rows: Vec<(Sev, Markup)> = Vec::new();
    let names: std::collections::HashSet<&str> = v.containers().map(|c| c.name.as_str()).collect();
    for i in v
        .status
        .incidents
        .iter()
        .filter(|i| !names.contains(i.subject.as_str()))
    {
        rows.push((
            Sev::Bad,
            html! {
                span.dot.bad role="img" aria-label="alert firing" {}
                a.nm href="/alerts" { (i.subject) }
                span.why { (i.state) }
                span.when.num { "firing " (human_duration(now.saturating_sub(i.since))) }
            },
        ));
    }
    let mut clean_stopped = 0usize;
    for c in v.containers() {
        let inc = v.status.incidents.iter().find(|i| i.subject == c.name);
        let why = match (c.attention(), inc) {
            (Some((_, why)), Some(_)) => Some((Sev::Bad, why)),
            (Some(x), None) => Some(x),
            (None, Some(i)) => Some((Sev::Bad, i.state.clone())),
            (None, None) => None,
        };
        match why {
            Some((sev, why)) => rows.push((
                sev,
                html! {
                    (status_dot(c))
                    a.nm href={"/containers/" (url_encode(&c.name))} { (c.title) }
                    span.why { (why) }
                    @if let Some(i) = inc {
                        span.when.num { a href="/alerts" { "alert firing " (human_duration(now.saturating_sub(i.since))) } }
                    } @else {
                        span.when { (c.state_text.as_deref().unwrap_or("")) }
                    }
                },
            )),
            None if !c.running => clean_stopped += 1,
            None => {}
        }
    }
    // Running and fine now, but crashed within the last hour (from the
    // health strips the page computed; the live re-render reuses them).
    if let Some(st) = crate::db::strips::last() {
        let n = st.buckets();
        for c in v
            .containers()
            .filter(|c| c.running && c.attention().is_none())
        {
            let Some(s) = st.by_name.get(&c.name) else {
                continue;
            };
            let hit = (n.saturating_sub(4)..n)
                .rev()
                .find(|&i| s.cells.get(i) == Some(&crate::db::strips::Cell::Bad));
            if let Some(i) = hit {
                let why = s.why.get(&i).map(String::as_str).unwrap_or("problem");
                rows.push((
                    Sev::Warn,
                    html! {
                        (status_dot(c))
                        a.nm href={"/containers/" (url_encode(&c.name))} { (c.title) }
                        span.why { "recovered after: " (why) }
                        span.when { "last hour" }
                    },
                ));
            }
        }
    }
    rows.sort_by_key(|(s, _)| *s != Sev::Bad);
    let total = v.containers().count();
    html! {
        section #ov-attn.attn.quiet[rows.is_empty()] aria-label="Needs attention" {
            div.attn-h {
                h2.label { "Needs attention" }
                @if !rows.is_empty() { span.count-pill.bad { (rows.len()) } }
            }
            @if rows.is_empty() {
                p.ok-line {
                    (ic("check"))
                    @if v.known() { "All clear. " span.muted { (total) " containers, nothing needs you." } }
                    @else { "Nothing needs attention." }
                }
            } @else {
                ul.rows {
                    @for (_, r) in &rows { li { (r) } }
                }
            }
            @if clean_stopped > 0 {
                p.foot-line {
                    a href="/containers?state=stopped" {
                        (clean_stopped) @if clean_stopped == 1 { " container is" } @else { " containers are" }
                        " stopped cleanly"
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- top consumers

pub fn top(v: &Live, show_stats: bool) -> Markup {
    let cards: Vec<&CardView> = v.containers().collect();
    let board = |title: &str,
                 color: &str,
                 key: fn(&CardView) -> Option<f64>,
                 fmt: fn(f64) -> String| {
        let mut v: Vec<(&CardView, f64)> = cards
            .iter()
            .filter_map(|c| key(c).filter(|x| x.is_finite()).map(|x| (*c, x)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(5);
        let max = v.first().map(|x| x.1).unwrap_or(0.0).max(f64::MIN_POSITIVE);
        html! {
            div.board style={"--s:var(" (color) ")"} {
                @if !title.is_empty() { h3.label { (title) } }
                @if v.is_empty() {
                    p.muted { "Waiting for the first sample…" }
                } @else {
                    ol.bars {
                        @for (c, x) in v {
                            li {
                                a.nm href={"/containers/" (url_encode(&c.name))} { (c.title) }
                                span.val { (fmt(x)) }
                                span.meter { span style={"width:" (format!("{:.1}", (x / max * 100.0).clamp(0.0, 100.0))) "%"} {} }
                            }
                        }
                    }
                }
            }
        }
    };
    html! {
        section #ov-top.res aria-label="Where resources go" {
            div.panel.res-mem {
                div.panel-h {
                    h2 { "Memory by container" }
                    a.more href="/containers?sort=mem" { "All" (ic("arrow-right")) }
                }
                @if !show_stats {
                    p.muted { "Live stats are off (" code { "page.show_stats = false" } ")." }
                } @else {
                    (treemap(&cards))
                }
            }
            div.panel.res-cpu {
                div.panel-h {
                    h2 { "Top CPU" }
                    a.more href="/containers?sort=cpu" { "All" (ic("arrow-right")) }
                }
                @if show_stats {
                    div.boards { (board("", "--ch-cpu", |c| c.cpu, pct)) }
                }
            }
        }
    }
}

/// "Where did my RAM go": a squarified treemap of container memory (the 18
/// biggest, the rest folded into "others"), coloured by stack, as positioned
/// HTML boxes (crisp text, no script). Each box links to its container.
pub fn treemap(cards: &[&CardView]) -> Markup {
    let mut items: Vec<(&CardView, f64)> = cards
        .iter()
        .filter_map(|c| c.mem.filter(|m| *m > 0).map(|m| (*c, m as f64)))
        .collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));
    if items.is_empty() {
        return html! { div.tmap.sk-box { span.muted { "Waiting for the first sample…" } } };
    }
    let total: f64 = items.iter().map(|x| x.1).sum();
    const MAX: usize = 18;
    let rest: f64 = items.iter().skip(MAX).map(|x| x.1).sum();
    let rest_n = items.len().saturating_sub(MAX);
    items.truncate(MAX);
    let mut weights: Vec<f64> = items.iter().map(|x| x.1).collect();
    if rest > 0.0 {
        weights.push(rest);
    }
    let rects = squarify(&weights, 0.0, 0.0, 100.0, 100.0);
    let hue = |s: &str| {
        s.bytes()
            .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
            % 360
    };
    html! {
        div.tmap-wrap {
            p.tmap-sum.muted {
                b.num { (human_bytes(total as u64)) } " in " (items.len() + rest_n) " containers"
            }
            div.tmap role="list" aria-label="Memory by container" {
                @for (i, r) in rects.iter().enumerate() {
                    @let style = format!("left:{:.2}%;top:{:.2}%;width:{:.2}%;height:{:.2}%", r.0, r.1, r.2, r.3);
                    // Boxes too small for two lines get one; tiny ones none
                    // (the title still says what they are).
                    @let fit = if r.3 < 9.0 || r.2 < 6.0 { "tm tiny" } else if r.3 < 19.0 { "tm short" } else { "tm" };
                    @if let Some((c, m)) = items.get(i) {
                        a class=(fit) role="listitem" href={"/containers/" (url_encode(&c.name))}
                            style={(style) ";--h:" (hue(&c.stack))}
                            title={(c.title) ": " (human_bytes(*m as u64)) " (" (format!("{:.0}", m / total * 100.0)) "%)"} {
                            span.tm-n { (c.title) }
                            span.tm-v { (human_bytes(*m as u64)) }
                        }
                    } @else {
                        span.tm.tm-rest role="listitem" style=(style) title={(rest_n) " smaller containers: " (human_bytes(rest as u64))} {
                            span.tm-n { "+" (rest_n) " others" }
                            span.tm-v { (human_bytes(rest as u64)) }
                        }
                    }
                }
            }
        }
    }
}

/// Squarified treemap layout (Bruls, Huizing, van Wijk) of `w` (sorted,
/// biggest first) into the box `x, y, width, height`; returns one
/// `(x, y, w, h)` per weight, in order.
pub fn squarify(w: &[f64], x: f64, y: f64, width: f64, height: f64) -> Vec<(f64, f64, f64, f64)> {
    let total: f64 = w.iter().sum();
    if w.is_empty() || total <= 0.0 || width <= 0.0 || height <= 0.0 {
        return vec![(x, y, 0.0, 0.0); w.len()];
    }
    let scale = width * height / total;
    let areas: Vec<f64> = w.iter().map(|v| v * scale).collect();
    let mut out = Vec::with_capacity(w.len());
    let (mut x, mut y, mut width, mut height) = (x, y, width, height);
    let mut i = 0;
    let worst = |row: &[f64], side: f64| {
        let s: f64 = row.iter().sum();
        let (mx, mn) = row
            .iter()
            .fold((0f64, f64::INFINITY), |(a, b), &v| (a.max(v), b.min(v)));
        ((side * side * mx) / (s * s)).max((s * s) / (side * side * mn))
    };
    while i < areas.len() {
        let side = width.min(height);
        let mut j = i + 1;
        while j < areas.len() && worst(&areas[i..=j], side) <= worst(&areas[i..j], side) {
            j += 1;
        }
        let row = &areas[i..j];
        let s: f64 = row.iter().sum();
        if width >= height {
            // Column on the left.
            let cw = s / height;
            let mut yy = y;
            for a in row {
                let h = a / cw;
                out.push((x, yy, cw, h));
                yy += h;
            }
            x += cw;
            width -= cw;
        } else {
            let rh = s / width;
            let mut xx = x;
            for a in row {
                let wv = a / rh;
                out.push((xx, y, wv, rh));
                xx += wv;
            }
            y += rh;
            height -= rh;
        }
        i = j;
    }
    out
}

/// The 24 h strip of every stack (signature element), worst first, then by
/// name. Rendered once per page view (the strips are cached for 30 s).
fn stack_strips(v: &Live, st: &crate::db::strips::Strips) -> Markup {
    use crate::ui::view::{health_strip, stack_strip, strip_summary, window_label, StripStyle};
    let mut rows: Vec<(&GroupView, crate::db::strips::Strip)> = v
        .groups
        .iter()
        .filter(|g| g.cards.iter().any(|c| c.container))
        .map(|g| (g, stack_strip(st, g)))
        .collect();
    if rows.is_empty() {
        return html! {};
    }
    let worst = |s: &crate::db::strips::Strip| s.cells.iter().copied().max();
    rows.sort_by(|a, b| {
        worst(&b.1)
            .cmp(&worst(&a.1))
            .then(a.0.title.cmp(&b.0.title))
    });
    let none = st.by_name.is_empty();
    html! {
        div.strips data-cost-us=(st.cost_us) {
            div.strips-h {
                h2.label { "Last " (window_label(st)) }
                span.strips-key aria-hidden="true" {
                    i.o {} "running" i.w {} "restarted" i.b {} "problem" i.f {} "stopped"
                }
            }
            @if none {
                p.muted { "Health strips fill in as history records (one cell per 15 minutes)." }
            } @else {
                ul.strip-list {
                    @for (g, s) in &rows {
                        @let ctrs = g.cards.iter().filter(|c| c.container).count();
                        li {
                            a.sl-n href={"/containers?stack=" (url_encode(&g.name))} title=(g.counts()) { (g.title) }
                            span.sl-c.num { (g.running) "/" (ctrs) }
                            (health_strip(s, st, &g.title, StripStyle::Runs))
                            span.sl-s.muted { (strip_summary(s, st).split(" of the last").next().unwrap_or("")) }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- host vitals

fn meter(p: Option<f64>, color: &str) -> Markup {
    let v = p.filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 100.0));
    html! {
        div class={"meter " (level(v))} style={"--s:var(" (color) ")"} role="img"
            aria-label={"used: " (v.map(|v| format!("{v:.0}%")).unwrap_or_else(|| NONE.into()))} {
            span style={"width:" (format!("{:.1}", v.unwrap_or(0.0))) "%"} {}
        }
    }
}

/// A skeleton bar while there is no sample yet; "—" when a sample exists
/// but this value is missing from it.
fn skel(s: Option<&SystemStats>) -> Markup {
    match s {
        Some(_) => html! { (NONE) },
        None => html! { span.sk { span.sr { "loading" } } },
    }
}

fn cell_head(icon: &str, label: &str) -> Markup {
    html! { div.cell-h { (ic(icon)) span.label { (label) } } }
}

/// The host instrument strip. `db` (with the time) seeds the sparklines with
/// the last hour; live re-renders pass `None` (the script keeps the line).
pub fn sys_strip(s: Option<&SystemStats>, db: Option<(&Db, i64)>) -> Markup {
    let hist =
        |m: SysMetric| db.and_then(|(db, now)| seed(&db.system_series(m, Range::H1, now), 40));
    let cpu = s.and_then(|s| s.cpu_percent);
    let memp = s.and_then(mem_percent);
    let diskp = s.and_then(disk_percent);
    let temp = s.and_then(|s| s.temp_c);
    let rate = |v: Option<f64>| match v {
        Some(b) if b.is_finite() => format!("{}/s", human_bytes(b.max(0.0) as u64)),
        _ => NONE.into(),
    };
    let net_total = s.and_then(|s| match (s.net_rx_bps, s.net_tx_bps) {
        (Some(a), Some(b)) => Some(a + b),
        _ => None,
    });
    let temp_cls = match temp {
        Some(t) if t >= 90.0 => "bad",
        Some(t) if t >= 80.0 => "warn",
        _ => "",
    };
    html! {
        div #ov-sys.strip aria-label="Host vitals" {
            div.cell {
                (cell_head("cpu", "CPU"))
                div class={"big " (level(cpu))} { @match cpu { Some(p) => { span.tw { (format!("{p:.0}")) } small { "%" } } None => (skel(s)) } }
                div.sub title=[s.map(|s| format!("load average 1, 5, 15 min · {} cores", s.cpu_cores))] {
                    @if let Some(s) = s {
                        "load " (format!("{:.2} {:.2} {:.2}", s.load[0], s.load[1], s.load[2]))
                    } @else { (NONE) }
                }
                (live_spark("s:cpu", cpu, "--ch-cpu", hist(SysMetric::Cpu)))
            }
            div.cell {
                (cell_head("memory", "Memory"))
                div class={"big " (level(memp))} {
                    @if let Some(s) = s.filter(|s| s.mem_total > 0) {
                        (human_bytes(s.mem_used)) small { (format!("{:.0}", memp.unwrap_or(0.0))) "%" }
                    } @else { (NONE) }
                }
                div.sub {
                    @if let Some(s) = s.filter(|s| s.mem_total > 0) {
                        "of " (human_bytes(s.mem_total))
                        @if s.swap_total > 0 { " · swap " (human_bytes(s.swap_used)) }
                    } @else { (NONE) }
                }
                (meter(memp, "--ch-mem"))
            }
            div.cell {
                (cell_head("disk", "Disk"))
                @match s.and_then(|s| s.disks.iter().find(|d| d.total > 0)) {
                    Some(d) => {
                        div class={"big " (level(diskp))} { (format!("{:.0}", diskp.unwrap_or(0.0))) small { "% of " (human_bytes(d.total)) } }
                        div.sub {
                            (human_bytes(d.avail)) " free on " span.mono { (d.label) }
                            @if let Some(s) = s { @if s.disks.len() > 1 { " · +" (s.disks.len() - 1) " more" } }
                        }
                        (meter(diskp, "--ch-disk"))
                    }
                    None => {
                        div.big { (NONE) }
                        div.sub { (NONE) }
                        (meter(None, "--ch-disk"))
                    }
                }
            }
            div.cell {
                (cell_head("activity", "Network"))
                div.big.net {
                    span title="received" { (ic("arrow-down")) (rate(s.and_then(|s| s.net_rx_bps))) }
                    span title="sent" { (ic("arrow-up")) (rate(s.and_then(|s| s.net_tx_bps))) }
                }
                div.sub { span.mono { (s.and_then(|s| s.net_iface.as_deref()).unwrap_or(NONE)) } }
                (live_spark("s:net", net_total, "--ch-rx", None))
            }
            div.cell {
                (cell_head("thermo", "Temperature"))
                div class={"big " (temp_cls)} { @match temp { Some(t) => { span.tw { (format!("{t:.0}")) } small { " °C" } } None => (skel(s)) } }
                div.sub { "CPU package" }
                (live_spark("s:temp", temp, "--ch-temp", hist(SysMetric::Temp)))
            }
            div.cell {
                (cell_head("clock", "Uptime"))
                div.big { @match s.filter(|s| s.uptime_secs > 0) { Some(s) => (human_duration(s.uptime_secs)), None => (NONE) } }
                div.sub { "since last boot" }
            }
        }
    }
}

// ---------------------------------------------------------------- 1 h charts

fn hour_charts(ctx: &Ctx) -> Markup {
    let db = &ctx.state.db;
    let now = ctx.now as i64;
    let spec = |title, unit, color, cap| ChartSpec {
        title,
        unit,
        color,
        range_secs: Range::H1.secs(),
        now,
        cap,
        zero_based: true,
        compact: true,
    };
    let cpu = db.system_series(SysMetric::Cpu, Range::H1, now);
    let mem = db.system_series(SysMetric::Mem, Range::H1, now);
    let rx = db.system_series(SysMetric::NetRx, Range::H1, now);
    let tx = db.system_series(SysMetric::NetTx, Range::H1, now);
    let any = [&cpu, &mem, &rx, &tx].iter().any(|s| !s.points.is_empty());
    let evs = db.events(
        &EventFilter {
            limit: 100,
            ..Default::default()
        }
        .for_viewer(false),
    );
    let marks = crate::ui::chart::markers_from_events(&evs);
    let x = crate::ui::chart::Extras {
        markers: &marks,
        ..Default::default()
    };
    html! {
        @if any {
            div.charts.four {
                (line_chart_ex(&spec("CPU · 1 h", Unit::Percent, "--ch-cpu", Some(100.0)), &cpu, true, &x))
                (line_chart_ex(&spec("Memory · 1 h", Unit::Bytes, "--ch-mem", None), &mem, true, &x))
                (line_chart(&spec("Network in · 1 h", Unit::BytesPerSec, "--ch-rx", None), &rx, true))
                (line_chart(&spec("Network out · 1 h", Unit::BytesPerSec, "--ch-tx", None), &tx, true))
            }
        } @else {
            div.charts { (crate::ui::chart::empty("Last hour", db.is_enabled(), true)) }
        }
    }
}

// ---------------------------------------------------------------- events and bookmarks

fn recent_events(ctx: &Ctx) -> Markup {
    let evs = ctx.state.db.events(
        &EventFilter {
            limit: 15,
            ..Default::default()
        }
        .for_viewer(ctx.user.is_some()),
    );
    html! {
        section.panel aria-label="Recent events" {
            (panel_head("Recent events", Some(("/history#events", "Timeline"))))
            @if evs.is_empty() {
                p.muted {
                    @if ctx.state.db.is_enabled() { "No events recorded yet." }
                    @else { "Starts, stops, health changes and alerts appear here once history is recording." }
                }
            } @else {
                ul.events {
                    @for e in &evs { (event_row(ctx.now, e, true)) }
                }
            }
        }
    }
}

/// One timeline row. `link` makes a container subject a link.
pub fn event_row(now: u64, e: &crate::db::Event, link: bool) -> Markup {
    let kind = match e.level.as_str() {
        "error" => "bad",
        "warn" => "warn",
        _ => "",
    };
    html! {
        li.ev {
            span.when { (time_tag(now, e.ts.max(0) as u64)) }
            (pill(kind, kind_label(&e.kind)))
            @if !e.subject.is_empty() {
                @if link && e.kind.starts_with("container.") {
                    a.nm href={"/containers/" (url_encode(&e.subject))} { (e.subject) }
                } @else {
                    span.nm { (e.subject) }
                }
            }
            @if !e.detail.is_empty() { span.muted.detail { (e.detail) } }
        }
    }
}

fn bookmarks(v: &Live) -> Markup {
    let marks: Vec<(&GroupView, Vec<&CardView>)> = v
        .groups
        .iter()
        .map(|g| {
            (
                g,
                g.cards.iter().filter(|c| !c.container).collect::<Vec<_>>(),
            )
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    html! {
        section.panel aria-label="Bookmarks" {
            (panel_head("Bookmarks", None))
            @if marks.is_empty() {
                p.muted { "No bookmarks. Add " code { "[[bookmarks]]" } " entries to the config." }
            }
            @for (g, cards) in marks {
                div.mark-group {
                    @if v.groups.len() > 1 || g.title != "Bookmarks" {
                        h3.label { (g.title) }
                    }
                    ul.marks {
                        @for c in cards {
                            @if let Some(u) = &c.url {
                                li {
                                    a.mark href=(u) target="_blank" rel="noopener noreferrer" data-search=(c.search) {
                                        (crate::ui::icon(c.icon.as_deref(), &c.title))
                                        span.nm { (c.title) }
                                        @if let Some(s) = &c.sub { span.muted.mono { (s) } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DiskStats;

    fn stats() -> SystemStats {
        SystemStats {
            cpu_percent: Some(12.4),
            cpu_cores: 4,
            load: [3.67, 3.65, 3.39],
            mem_total: 8 << 30,
            mem_used: 7 << 30,
            swap_total: 20 << 30,
            swap_used: 6 << 30,
            disks: vec![DiskStats {
                label: "/".into(),
                total: 436 << 30,
                used: 79 << 30,
                avail: 335 << 30,
            }],
            net_rx_bps: Some(57_222.0),
            net_tx_bps: Some(25_605.0),
            net_iface: Some("enp1s0".into()),
            temp_c: Some(91.0),
            uptime_secs: 1_820_698,
            sampled_at: 1,
        }
    }

    #[test]
    fn strip_renders_values_and_levels() {
        let s = stats();
        let h = sys_strip(Some(&s), None).into_string();
        assert!(h.starts_with(r#"<div class="strip" id="ov-sys""#), "{h}");
        assert!(h.contains("12</span><small>%</small>") && h.contains("4 cores"));
        assert!(h.contains(r#"data-k="s:cpu" data-v="12.40""#), "{h}");
        assert!(
            h.contains("7.0 GiB") && h.contains("class=\"meter warn\""),
            "{h}"
        );
        assert!(h.contains("of 8.0 GiB · swap 6.0 GiB"), "{h}");
        assert!(h.contains("335 GiB free"));
        assert!(h.contains("55.9 KiB/s") && h.contains("enp1s0"));
        assert!(h.contains("big bad"), "91 °C is bad: {h}");
        assert!(h.contains("21d 1h"));
    }

    #[test]
    fn strip_before_the_first_sample() {
        let h = sys_strip(None, None).into_string();
        assert!(h.contains(NONE));
        assert!(!h.contains("NaN"));
        assert_eq!(disk_percent(&stats()).map(|p| p.round()), Some(19.0));
    }

    fn card(name: &str, label: &'static str, running: bool) -> CardView {
        CardView {
            name: name.into(),
            title: name.into(),
            container: true,
            running,
            label,
            ..Default::default()
        }
    }

    fn live(cards: Vec<CardView>, error: Option<&str>) -> Live {
        Live {
            snap: Arc::new(Snapshot {
                containers: vec![],
                fetched_at: 1,
                error: error.map(str::to_string),
            }),
            groups: vec![GroupView {
                name: "g".into(),
                title: "g".into(),
                icon: None,
                columns: 4,
                layout: Default::default(),
                collapsed: false,
                running: 0,
                stopped: 0,
                links: 0,
                cards,
            }],
            status: Status::default(),
        }
    }

    #[test]
    fn health_counts_and_links() {
        let v = live(
            vec![
                card("a", "running", true),
                card("b", "unhealthy", true),
                card("c", "stopped", false),
            ],
            None,
        );
        let html = health(&v).into_string();
        assert!(html.contains(r#"href="/containers?state=running""#));
        assert!(html.contains(r#"<span class="hs-n tw">2</span>"#), "{html}");
        assert!(html.contains("hs-cell hot"), "unhealthy is hot: {html}");
        assert!(html.contains("alerts firing"));
        // Docker error: counts are unknown, not zero.
        let v = live(vec![card("a", "running", true)], Some("down"));
        let html = health(&v).into_string();
        assert!(html.contains(r#"<span class="hs-n tw">—</span>"#));
        assert!(html.contains("Docker unreachable"));
    }

    #[test]
    fn attention_lists_problems_and_summarises_clean_stops() {
        let mut sick = card("sick", "unhealthy", true);
        sick.dot = Some(crate::ui::view::Dot::Bad);
        let mut crashed = card("crashed", "stopped", false);
        crashed.exit_code = Some(1);
        let clean = card("clean", "stopped", false);
        let fine = card("fine", "running", true);
        let v = live(vec![crashed, sick, clean, fine.clone()], None);
        let html = attention(100, &v).into_string();
        assert!(html.contains("sick") && html.contains("unhealthy"));
        assert!(html.contains("stopped, exit 1"));
        assert!(!html.contains(">fine<"));
        assert!(html.contains("1 container is stopped cleanly"));
        // Bad rows sort before warnings.
        assert!(html.find("sick").unwrap() < html.find("crashed").unwrap());
        let html = attention(100, &live(vec![fine], None)).into_string();
        assert!(html.contains("All clear"));
    }

    #[test]
    fn top_sorts_and_escapes() {
        let mut a = card("<i>a</i>", "running", true);
        a.cpu = Some(40.0);
        a.mem = Some(1 << 20);
        let mut b = card("b", "running", true);
        b.cpu = Some(90.0);
        b.mem = Some(4 << 20);
        let html = top(&live(vec![a.clone(), b], None), true).into_string();
        assert!(
            html.find(">b<").unwrap() < html.find("&lt;i&gt;").unwrap(),
            "{html}"
        );
        assert!(html.contains("width:100.0%"));
        assert!(!html.contains("<i>a</i>"));
        assert!(top(&live(vec![a], None), false)
            .into_string()
            .contains("show_stats"));
        assert!(top(&live(vec![], None), true)
            .into_string()
            .contains("Waiting for the first sample"));
    }

    #[test]
    fn treemap_fills_the_box_and_folds_the_tail() {
        let w = [50.0, 25.0, 12.5, 6.25, 6.25];
        let r = squarify(&w, 0.0, 0.0, 100.0, 100.0);
        assert_eq!(r.len(), 5);
        let area: f64 = r.iter().map(|x| x.2 * x.3).sum();
        assert!((area - 10_000.0).abs() < 1e-6, "{area}");
        for (i, x) in r.iter().enumerate() {
            assert!(
                x.0 >= -1e-9
                    && x.1 >= -1e-9
                    && x.0 + x.2 <= 100.0 + 1e-6
                    && x.1 + x.3 <= 100.0 + 1e-6
            );
            assert!(
                (x.2 * x.3 - w[i] * 100.0).abs() < 1e-6,
                "box {i} keeps its share"
            );
        }
        assert!(squarify(&[], 0.0, 0.0, 1.0, 1.0).is_empty());
        let cards: Vec<CardView> = (0..25)
            .map(|i| {
                let mut c = card(&format!("c{i}<"), "running", true);
                c.mem = Some((i + 1) << 20);
                c
            })
            .collect();
        let refs: Vec<&CardView> = cards.iter().collect();
        let html = treemap(&refs).into_string();
        assert_eq!(html.matches("<a class=\"tm").count(), 18, "{html}");
        assert!(html.contains("+7 others"));
        assert!(html.contains("c24&lt;") && !html.contains("c24<"));
        assert!(treemap(&[]).into_string().contains("Waiting"));
    }

    #[test]
    fn event_rows() {
        let e = crate::db::Event {
            id: 1,
            ts: 1_794_660_300,
            kind: "container.die".into(),
            level: "error".into(),
            subject: "web<1>".into(),
            detail: "exit 137 & more".into(),
        };
        let html = event_row(1_794_660_400, &e, true).into_string();
        assert!(html.contains("pill bad") && html.contains(">die<"));
        assert!(html.contains("href=\"/containers/web%3C1%3E\""), "{html}");
        assert!(html.contains("exit 137 &amp; more") && html.contains("web&lt;1&gt;"));
        let plain = event_row(1_794_660_400, &e, false).into_string();
        assert!(!plain.contains("href="));
    }
}
