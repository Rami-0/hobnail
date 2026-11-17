//! `/`: health strip, host vitals with 1 h charts, what needs attention, top
//! consumers, widgets, recent events and bookmarks.

use maud::{html, Markup};
use std::collections::HashMap;
use std::sync::Arc;

use crate::alerts::Status;
use crate::config::Config;
use crate::db::{Db, EventFilter, Range, SysMetric};
use crate::model::{Snapshot, SystemStats};
use crate::ui::alerts::{subject_href, subject_link};
pub use crate::ui::chart::live_spark;
use crate::ui::chart::{line_chart, line_chart_ex, seed, ChartSpec, Unit};
use crate::ui::fmt::{clock_tag, url_encode};
use crate::ui::pages::{crumb_title, event_label};
use crate::ui::shell::{disk_percent, layout, level, mem_percent, Ctx, Nav};
use crate::ui::view::{build_groups, CardView, GroupView, Sev};
use crate::ui::{empty, ic, panel_head, pill, skel_word, status_dot, Empty};
use crate::util::{human_bytes, human_duration};

const NONE: &str = "—";

/// What the live regions of several pages are built from (one snapshot).
pub struct Live {
    pub snap: Arc<Snapshot>,
    pub groups: Vec<GroupView>,
    pub status: Status,
    /// When each unhealthy container turned unhealthy (its newest
    /// `container.health` event), for "unhealthy for 41 min".
    pub unhealthy_since: HashMap<String, i64>,
    /// The first line of each unhealthy container's last failing probe.
    /// **Signed in only** (probe output can carry secrets, NAV-11): filled by
    /// [`Live::with_probes`] for a page with a session, never for the live
    /// fragments, which are rendered without one.
    pub probes: HashMap<String, String>,
}

/// The most probe lines one Overview view looks up (one `docker inspect`
/// each): the rest of the unhealthy rows say "unhealthy for …" without one.
pub const PROBES_MAX: usize = 5;

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
            unhealthy_since: HashMap::new(),
            probes: HashMap::new(),
        }
    }

    /// Adds since when each unhealthy container has been unhealthy (one
    /// small query each), for the views that render "Needs attention".
    pub fn with_since(mut self, ctx: &Ctx) -> Self {
        for c in self.snap.containers.iter() {
            if !c.is_running() || c.health.as_deref() != Some("unhealthy") {
                continue;
            }
            if let Some(t) = crate::ui::pages::container::unhealthy_since(&ctx.state.db, c, None) {
                self.unhealthy_since.insert(c.name.clone(), t);
            }
        }
        self
    }

    /// Adds the probe lines of the unhealthy containers, for a session only
    /// (one `docker inspect` each, for the first [`PROBES_MAX`] of them;
    /// nothing for a visitor).
    pub fn with_probes(mut self, ctx: &Ctx) -> Self {
        if ctx.user.is_none() {
            return self;
        }
        let sick = self
            .snap
            .containers
            .iter()
            .filter(|c| c.is_running() && c.health.as_deref() == Some("unhealthy"))
            .take(PROBES_MAX);
        for c in sick {
            let Ok(i) = crate::docker::inspect_blocking(&ctx.state.env.docker_socket, &c.id) else {
                continue;
            };
            if let Some(line) = crate::ui::pages::container::probe_line(&i) {
                self.probes.insert(c.name.clone(), line);
            }
        }
        self
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
    let v = Live::new(ctx).with_since(ctx).with_probes(ctx);
    let db = &ctx.state.db;
    let now = ctx.now as i64;
    let mut sys = ctx.state.system.lock().unwrap().1.clone();
    crate::ui::view::seed_cpu(ctx.state, &mut sys, now);
    let strips = crate::ui::view::strips(ctx.state, now);
    let has_marks = v
        .groups
        .iter()
        .any(|g| g.cards.iter().any(|c| !c.container));
    // Signed in: "Add" on Bookmarks and Widgets opens the Settings dialog
    // here (OV-12; the dialogs, script and routes are the tabs' own).
    let embed = crate::ui::settings::embed(ctx, "bookmarks widgets");
    let add = embed.as_ref().map(|e| e.hash.as_str());
    let body = html! {
        @if let Some(e) = &embed { (e.head) }
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
                (attention(cfg, ctx.now, &v))
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
            @if !cfg.widgets.is_empty() || add.is_some() {
                section.sec.ghost #ov-w aria-labelledby="h-w" {
                    div.sec-h {
                        h2.label #h-w { "Widgets" }
                        @if let Some(h) = add { (add_link("widget", "widgets", h)) }
                    }
                    @if cfg.widgets.is_empty() {
                        (empty(Empty::First, "inline", "zap", "No widgets yet", html! { "A widget shows a number or a status from any JSON or text URL. Use Add above." }, None))
                    }
                    div.widgets {
                        @for w in &cfg.widgets {
                            @let out = ctx.state.widgets.lock().unwrap().get(&w.id).cloned();
                            (crate::ui::widgets::render(w, out.as_ref()))
                        }
                    }
                }
            }
            div.cols.c-ev[!has_marks && add.is_none()] {
                (recent_events(ctx))
                @if has_marks || add.is_some() { (bookmarks(&v, add)) }
            }
        }
        @if let Some(e) = &embed { (e.body) (e.tail) }
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
    // The same count and word as the Containers page's "Problems" filter,
    // where this cell leads (OV-16).
    let unhealthy = cards.iter().filter(|c| c.attention().is_some()).count();
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
                        None => (skel_word()),
                    }
                }
                span.hs-l { (label) }
            }
        }
    };
    html! {
        section #ov-health.hs aria-label="Container health" data-wait=[(!known && v.snap.error.is_none()).then_some("")] {
            (cell(known.then_some(running), "running", "/containers?state=running", false, "Containers that are running"))
            (cell(known.then_some(stopped), "stopped", "/containers?state=stopped", false, "Containers that are not running"))
            (cell(known.then_some(unhealthy), "problems", "/containers?state=problem", true, "Unhealthy, restarting, paused or stopped with an error"))
            (cell(Some(firing), "alerts firing", "/alerts", true, "Open incidents"))
            @if known && !cards.is_empty() {
                div.hs-bar role="img" aria-label={(running) " of " (cards.len()) " running"} {
                    span.r style={"width:" (format!("{:.2}", running as f64 / total * 100.0)) "%"} {}
                    @if unhealthy > 0 {
                        span.u style={"width:" (format!("{:.2}", unhealthy as f64 / total * 100.0)) "%"} {}
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- attention

/// The logs action at the end of a row that names a container (§11.9).
pub fn logs_ia(c: &CardView) -> Markup {
    html! {
        a.ia.logs href=(crate::logs::logs_url(&c.name)) title="Logs" aria-label={"Logs of " (c.title)} { (ic("logs")) }
    }
}

/// "Needs attention": one row per subject, the whole row a link to the
/// cause with the reason inline and, for a container, its logs beside it
/// (OV-01, OV-04). A firing alert about something that is not a container
/// opens its subject (the Host page, the widget, the checked URL) and names
/// its rule. An unhealthy container says for how long and, for a session,
/// the first line of its failing probe (NAV-11).
/// Rows "Needs attention" shows before "N more on the Containers page".
const ATTN_MAX: usize = 8;

pub fn attention(cfg: &Config, now: u64, v: &Live) -> Markup {
    // Names that repeat across stacks ("svc05" in three stacks) carry their
    // stack, so each row says which one it is (ROB-06).
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in v.containers() {
        *seen.entry(c.title.as_str()).or_default() += 1;
    }
    let stack_of = |c: &CardView| {
        html! { @if seen.get(c.title.as_str()).is_some_and(|n| *n > 1) && c.stack != c.name { span.stk { " · " (c.stack) } } }
    };
    // One row per subject, ranked by severity: a container with a firing
    // alert is one red row ("unhealthy · alert firing 5m"), not two.
    let mut rows: Vec<(Sev, String, Markup)> = Vec::new();
    let names: std::collections::HashSet<&str> = v.containers().map(|c| c.name.as_str()).collect();
    for i in v
        .status
        .incidents
        .iter()
        .filter(|i| !names.contains(i.subject.as_str()))
    {
        let href = subject_href(cfg, &i.rule, &i.subject).unwrap_or(("/alerts".into(), false));
        rows.push((
            Sev::Bad,
            format!("a:{}:{}", i.rule, i.subject),
            html! {
                span.dot.bad role="img" aria-label="alert firing" {}
                (subject_link(Some(href), &i.subject, "nm"))
                span.why { (i.state) span.muted { " · rule " (i.rule) } }
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
                format!("c:{}", c.name),
                html! {
                    (status_dot(c))
                    a.nm.row-link href={"/containers/" (url_encode(&c.name))} data-c=(c.name) { (c.title) (stack_of(c)) }
                    // `data-since` keys the probe line: the script keeps the
                    // one the page was served with only while the failing
                    // run it belongs to goes on (NAV-11).
                    @let since = v.unhealthy_since.get(&c.name).filter(|_| c.label == "unhealthy");
                    span.why data-since=[since] {
                        (why)
                        @if let Some(t) = since {
                            " for " (human_duration(now.saturating_sub(*t as u64)))
                        }
                        @if let Some(p) = v.probes.get(&c.name).filter(|_| c.label == "unhealthy") {
                            span.probe data-since=[since] title="The check's output when this page was loaded" { " · " (p) }
                        }
                    }
                    @if let Some(i) = inc {
                        // A status in words, in the status colour (never the accent, A11Y-17).
                        span.when.num { a.bad href="/alerts" { "alert firing " (human_duration(now.saturating_sub(i.since))) } }
                    } @else {
                        span.when { (c.state_text.as_deref().unwrap_or("")) }
                    }
                    (logs_ia(c))
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
                    format!("c:{}", c.name),
                    html! {
                        (status_dot(c))
                        a.nm.row-link href={"/containers/" (url_encode(&c.name))} data-c=(c.name) { (c.title) (stack_of(c)) }
                        span.why { "recovered after: " (why) }
                        span.when { "last hour" }
                        (logs_ia(c))
                    },
                ));
            }
        }
    }
    rows.sort_by_key(|(s, _, _)| *s != Sev::Bad);
    // A long list is cut at eight rows with a link to the rest (ROB-06).
    let all_rows = rows.len();
    rows.truncate(ATTN_MAX);
    let total = v.containers().count();
    let connecting = v.snap.fetched_at == 0 && v.snap.error.is_none();
    html! {
        section #ov-attn.attn.quiet[rows.is_empty()] aria-label="Needs attention" data-wait=[connecting.then_some("")] {
            div.attn-h {
                h2.label { "Needs attention" }
                @if !rows.is_empty() { span.count-pill.bad { (all_rows) } }
            }
            @if rows.is_empty() {
                @if v.snap.error.is_some() {
                    p.ok-line.unknown { (ic("alert")) "Unknown while Docker cannot be reached." }
                } @else if connecting {
                    div.sk-rows { span.sk {} span.sk {} span.sk-t { "Connecting to Docker…" } }
                } @else if total == 0 {
                    p.ok-line.unknown { (ic("box")) "No containers yet." }
                } @else {
                    p.ok-line { (ic("check")) "All clear. " span.muted { (total) " containers, nothing needs you." } }
                }
            } @else {
                ul.rows data-flash {
                    @for (_, k, r) in &rows { li.rl data-key=(k) { (r) } }
                }
                @if all_rows > rows.len() {
                    p.foot-line {
                        a href="/containers?state=problem&group=0" { (all_rows - rows.len()) " more on the Containers page" }
                    }
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
    // Nothing to sample is not the same as no sample yet (ROB-01). Both
    // panels say why in the standard empty state; each its own title.
    let why: Option<(Empty, &str, &str)> = if v.snap.error.is_some() {
        Some((
            Empty::Error,
            "alert",
            "Docker cannot be reached, so nothing is sampled.",
        ))
    } else if v.known() && cards.is_empty() {
        Some((
            Empty::First,
            "box",
            "Hobnail lists the containers Docker runs on this host. Hidden ones are under Settings, Containers and groups.",
        ))
    } else if v.known() && !cards.iter().any(|c| c.running) {
        Some((
            Empty::First,
            "box",
            "Stopped containers use no CPU or memory.",
        ))
    } else {
        None
    };
    let why_box = |title: &str| {
        why.map(|(kind, icon, body)| {
            let title = match (kind, title) {
                (Empty::Error, _) => "No numbers",
                _ if cards.is_empty() => "No containers yet",
                (_, t) => t,
            };
            empty(kind, "compact", icon, title, html! { (body) }, None)
        })
    };
    let board = |title: &str,
                 color: &str,
                 key: fn(&CardView) -> Option<f64>,
                 fmt: fn(f64) -> String| {
        let mut v: Vec<(&CardView, f64)> = cards
            .iter()
            .filter_map(|c| key(c).filter(|x| x.is_finite()).map(|x| (*c, x)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        // Eight rows fill the panel beside the treemap; a full bar is one
        // whole core (100 %), not the top row, so 11 % never looks maxed
        // out (OV-14).
        v.truncate(8);
        let max = v.first().map(|x| x.1).unwrap_or(0.0).max(100.0);
        html! {
            div.board style={"--s:var(" (color) ")"} {
                @if !title.is_empty() { h3.label { (title) } }
                @if v.is_empty() {
                    div.sk-rows data-wait { span.sk {} span.sk {} span.sk {} span.sk-t { "Waiting for the first sample…" } }
                } @else {
                    // Ranked (HB.morph, data-rank): re-used by position, so
                    // nothing moves when ranks change; keyed by container,
                    // so a focused link stays that container's.
                    ol.bars data-rank {
                        @for (c, x) in v {
                            li data-key=(c.name) {
                                a.nm href={"/containers/" (url_encode(&c.name))} { (c.title) }
                                span.val { (fmt(x)) }
                                (logs_ia(c))
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
                    a.more href="/containers?sort=mem&group=0" { "All" (ic("arrow-right")) }
                }
                @if !show_stats {
                    p.muted { "Live stats are off (" code { "page.show_stats = false" } ")." }
                } @else if let Some(w) = why_box("Nothing is running") {
                    (w)
                } @else {
                    (treemap(&cards))
                }
            }
            div.panel.res-cpu {
                div.panel-h {
                    h2 { "Top CPU" }
                    a.more href="/containers?sort=cpu&group=0" { "All" (ic("arrow-right")) }
                }
                @if show_stats {
                    @if let Some(w) = why_box("Nothing to rank") {
                        (w)
                    } @else {
                        // One number format for every row: "0.4%", "11.0%".
                        div.boards { (board("", "--ch-cpu", |c| c.cpu, |x| format!("{x:.1}%"))) }
                    }
                }
            }
        }
    }
}

/// The treemap's box on a phone (px), the smallest it is drawn: a tile must
/// hold its name there, or it folds into "+N others" (Y10, OV-09).
const TM_W: f64 = 326.0;
const TM_H: f64 = 200.0;
/// The smallest tile that holds a one-line name (px, on that box).
const TM_MIN_W: f64 = 64.0;
const TM_MIN_H: f64 = 24.0;

/// The layout: the boxes (in % of the treemap) of the `k` biggest and, when
/// any are left, one more for the rest. `k` is the most tiles (at most 18)
/// whose boxes, and the rest's, all hold a name on a phone.
fn tm_layout(w: &[f64]) -> (usize, Vec<(f64, f64, f64, f64)>) {
    let fits = |r: &(f64, f64, f64, f64)| r.2 * TM_W >= TM_MIN_W && r.3 * TM_H >= TM_MIN_H;
    let mut k = w.len().min(18);
    loop {
        let rest: f64 = w[k..].iter().sum();
        let mut weights = w[..k].to_vec();
        if rest > 0.0 {
            weights.push(rest);
        }
        let rects: Vec<_> = squarify(&weights, 0.0, 0.0, 1.0, 1.0);
        if k == 0 || rects.iter().all(fits) {
            let pct = rects
                .iter()
                .map(|r| (r.0 * 100.0, r.1 * 100.0, r.2 * 100.0, r.3 * 100.0))
                .collect();
            return (k, pct);
        }
        k -= 1;
    }
}

/// "Where did my RAM go": a squarified treemap of container memory,
/// coloured by stack, as positioned HTML boxes (crisp text, no script).
/// A tile is a box holding a stretched name link to the container and, as
/// a sibling (never a link inside a link), its logs icon, which shows when
/// the tile is big enough (CSS container query: 64 × 40 px, 88 × 56 px on
/// touch; OV-01). Tiles too small for a name on a phone fold into "+N
/// others", a link to the containers table sorted by memory (OV-10).
pub fn treemap(cards: &[&CardView]) -> Markup {
    let mut items: Vec<(&CardView, f64)> = cards
        .iter()
        .filter_map(|c| c.mem.filter(|m| *m > 0).map(|m| (*c, m as f64)))
        .collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));
    if items.is_empty() {
        return html! {
            div.tmap-wrap data-wait {
                p.tmap-sum { span.sk {} span.sk-t { "Waiting for the first sample…" } }
                div.tmap.sk-box { span.sk.block {} }
            }
        };
    }
    let total: f64 = items.iter().map(|x| x.1).sum();
    let weights: Vec<f64> = items.iter().map(|x| x.1).collect();
    let (k, rects) = tm_layout(&weights);
    let rest: f64 = weights[k..].iter().sum();
    let rest_n = items.len() - k;
    let hue = |s: &str| {
        s.bytes()
            .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
            % 360
    };
    html! {
        div.tmap-wrap {
            p.tmap-sum.muted {
                b.num { (human_bytes(total as u64)) } " in " (items.len()) " containers"
            }
            div.tmap role="list" aria-label="Memory by container" data-rank {
                @for (i, r) in rects.iter().enumerate() {
                    @let style = format!("left:{:.2}%;top:{:.2}%;width:{:.2}%;height:{:.2}%", r.0, r.1, r.2, r.3);
                    // Under two lines' height on a phone: one line.
                    @let fit = if r.3 * TM_H / 100.0 < 40.0 { "tm short" } else { "tm" };
                    @if let Some((c, m)) = items.get(i).filter(|_| i < k) {
                        div class=(fit) role="listitem" data-key=(c.name) style={(style) ";--h:" (hue(&c.stack))}
                            title={(c.title) ": " (human_bytes(*m as u64)) " (" (format!("{:.0}", m / total * 100.0)) "%)"} {
                            a.tm-a.row-link href={"/containers/" (url_encode(&c.name))} {
                                span.tm-n { (c.title) }
                                span.tm-v { (human_bytes(*m as u64)) }
                            }
                            a.ia.logs.tm-logs href=(crate::logs::logs_url(&c.name)) title="Logs" aria-label={"Logs of " (c.title)} { (ic("logs")) }
                        }
                    } @else {
                        a class={(fit) " tm-rest"} role="listitem" data-key="+rest" href="/containers?sort=mem&group=0" style=(style)
                            title={(rest_n) " smaller containers: " (human_bytes(rest as u64)) ". Open them in the table, by memory"} {
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
                            // "running 98%", or "running 100% of 30m" while the
                            // history is young (OV-03); the full line on hover.
                            @let sum = strip_summary(s, st);
                            span.sl-s.muted title=(sum) { (sum.split(" of the last").next().unwrap_or("").trim_end_matches(" with data")) }
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
        None => skel_word(),
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
    // The network line starts drawn like the others: received + sent over
    // the last hour (OV-11). Page loads only; live re-renders pass no `db`.
    let net_hist = db.and_then(|(db, now)| {
        let rx = db.system_series(SysMetric::NetRx, Range::H1, now);
        let tx: HashMap<i64, f64> = db
            .system_series(SysMetric::NetTx, Range::H1, now)
            .points
            .into_iter()
            .collect();
        let points = rx
            .points
            .iter()
            .filter_map(|(t, v)| tx.get(t).map(|w| (*t, v + w)))
            .collect();
        seed(
            &crate::db::Series {
                step: rx.step,
                points,
            },
            40,
        )
    });
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
        div #ov-sys.strip aria-label="Host vitals" data-wait=[s.is_none().then_some("")] {
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
                (live_spark("s:net", net_total, "--ch-rx", net_hist))
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
    let names = crate::logs::tailable(ctx.state);
    html! {
        section.panel aria-label="Recent events" {
            (panel_head("Recent events", Some(("/history#events", "Timeline"))))
            @if evs.is_empty() {
                @if ctx.state.db.is_enabled() {
                    (empty(Empty::First, "inline", "history", "No events yet", html! { "Starts, stops, health changes and alerts appear here as they happen." }, None))
                } @else {
                    (empty(Empty::First, "inline", "history", "History is off", html! { "Starts, stops, health changes and alerts appear here once history is recording." }, None))
                }
            } @else {
                // Identical rows in a row (five "Hobnail started" after a few
                // restarts) are one row with a count (OV-05).
                ul.events.recent {
                    @for (e, n) in collapse(&evs) { (event_row_n(ctx.now, e, true, Some(&names), n, Some(&ctx.cfg))) }
                }
            }
        }
    }
}

/// One timeline row. `link` makes a container subject a link; `logs` (the
/// containers whose logs may be read) adds a logs icon that opens the logs
/// at the moment of the event when the subject is one of them.
pub fn event_row(
    now: u64,
    e: &crate::db::Event,
    link: bool,
    logs: Option<&std::collections::HashSet<String>>,
) -> Markup {
    event_row_n(now, e, link, logs, 1, None)
}

/// Consecutive events with the same kind, subject and detail, newest first,
/// as `(newest, count)`.
fn collapse(evs: &[crate::db::Event]) -> Vec<(&crate::db::Event, usize)> {
    let mut out: Vec<(&crate::db::Event, usize)> = Vec::new();
    for e in evs {
        match out.last_mut() {
            Some((p, n)) if p.kind == e.kind && p.subject == e.subject && p.detail == e.detail => {
                *n += 1
            }
            _ => out.push((e, 1)),
        }
    }
    out
}

/// `event_row` for `n` identical events in a row ("× 5"). The kind reads in
/// plain words (OV-05); the time is a clock time (LG-47); a row links to its
/// subject the way the History timeline does (LG-48): a container to its
/// page, an alert to what it watches (`cfg`; the Alerts page without it).
fn event_row_n(
    _now: u64,
    e: &crate::db::Event,
    link: bool,
    logs: Option<&std::collections::HashSet<String>>,
    n: usize,
    cfg: Option<&Config>,
) -> Markup {
    let logs = logs.filter(|n| n.contains(&e.subject));
    let kind = match e.level.as_str() {
        "error" => "bad",
        "warn" => "warn",
        _ => "",
    };
    let href: Option<(String, bool)> = if !link || e.subject.is_empty() {
        None
    } else if e.kind.starts_with("container.") {
        Some((format!("/containers/{}", url_encode(&e.subject)), false))
    } else if e.kind.starts_with("alert.") {
        e.detail
            .split_once(": ")
            .zip(cfg)
            .and_then(|((rule, _), cfg)| subject_href(cfg, rule, &e.subject))
            .or_else(|| Some(("/alerts".into(), false)))
    } else {
        None
    };
    html! {
        li.ev {
            span.when { (clock_tag(e.ts.max(0) as u64)) }
            (pill(kind, event_label(&e.kind)))
            @if n > 1 { span.muted.num.ev-n { "× " (n) } }
            @match href {
                Some((u, true)) => a.nm href=(u) target="_blank" rel="noopener noreferrer" { (e.subject) " ↗" },
                Some((u, false)) => a.nm href=(u) { (e.subject) },
                None => @if !e.subject.is_empty() { span.nm { (e.subject) } },
            }
            @if !e.detail.is_empty() { span.muted.detail { (e.detail) } }
            @if logs.is_some() {
                a.ia.logs href=(crate::logs::logs_at(&e.subject, e.ts)) title="Logs at that moment"
                    aria-label={"Logs of " (e.subject)} { (ic("logs")) }
            }
        }
    }
}

/// "Add" in a panel head, signed in only (OV-12): the Settings dialog on
/// this page, or that tab's `?add=` page without the script. `hash`: the
/// config version the page shows (the dialog's base).
fn add_link(kind: &str, tab: &str, hash: &str) -> Markup {
    html! {
        span.head-add data-hash=(hash) {
            a.btn.sm href={"/settings/" (tab) "?add=" (kind)} data-dlg={"dlg-" (kind)} { (ic("plus")) "Add" span.sr { " " (kind) } }
        }
    }
}

/// Bookmarks (`#ov-marks`); `add`: the config version, for a session.
fn bookmarks(v: &Live, add: Option<&str>) -> Markup {
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
        section.panel #ov-marks aria-label="Bookmarks" {
            div.panel-h {
                h2 { "Bookmarks" }
                @if let Some(h) = add { (add_link("bookmark", "bookmarks", h)) }
            }
            @if marks.is_empty() {
                (empty(Empty::First, "inline", "bookmark", "No bookmarks yet", html! {
                    @if add.is_some() { "Links to things that are not containers, like the router. Use Add above." }
                    @else { "Links to things that are not containers. They are added in Settings." }
                }, None))
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
            unhealthy_since: HashMap::new(),
            probes: HashMap::new(),
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
        // One word for one thing: the Containers page's "Problems" (OV-16).
        assert!(
            html.contains(r#"<span class="hs-l">problems</span>"#),
            "{html}"
        );
        assert!(html.contains("hs-cell hot"), "unhealthy is hot: {html}");
        assert!(html.contains("alerts firing"));
        // Docker error: counts are unknown, not zero.
        let v = live(vec![card("a", "running", true)], Some("down"));
        let html = health(&v).into_string();
        assert!(html.contains(r#"<span class="hs-n tw">—</span>"#));
        // The cause is the page's banner (ROB-09), not a raw line here, and
        // nothing is "loading" when Docker is down.
        assert!(
            !html.contains("Docker unreachable") && !html.contains("data-wait"),
            "{html}"
        );
        // Connecting: skeleton numbers on a region the script makes busy
        // (`data-wait`; never `aria-busy` from the server, §11.9).
        let mut v = live(vec![], None);
        v.snap = Arc::new(Snapshot::default());
        let html = health(&v).into_string();
        assert!(
            html.contains(r#"data-wait="""#)
                && !html.contains("aria-busy")
                && html.contains(
                    r#"<span class="sk" aria-hidden="true"></span><span class="sk-t">—</span>"#
                ),
            "{html}"
        );
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
        let html = attention(&Config::default(), 100, &v).into_string();
        assert!(html.contains("sick") && html.contains("unhealthy"));
        assert!(html.contains("stopped, exit 1"));
        assert!(!html.contains(">fine<"));
        assert!(html.contains("1 container is stopped cleanly"));
        // Bad rows sort before warnings.
        assert!(html.find("sick").unwrap() < html.find("crashed").unwrap());
        // Every container row ends with its logs action (one click, §11.9).
        assert!(html.contains(r#"<a class="ia logs" href="/containers/sick/logs" title="Logs" aria-label="Logs of sick">"#), "{html}");
        assert!(html.contains(r#"href="/containers/crashed/logs""#));
        let html = attention(&Config::default(), 100, &live(vec![fine], None)).into_string();
        assert!(html.contains("All clear"));
    }

    /// ROB-06: at most eight rows and a link to the rest; a name that
    /// repeats across stacks carries its stack.
    #[test]
    fn attention_is_capped_and_names_the_stack_of_repeats() {
        let cards: Vec<CardView> = (0..11)
            .map(|i| {
                let mut c = card(&format!("stack{i}-svc-1"), "unhealthy", true);
                c.title = "svc".into();
                c.stack = format!("stack{i}");
                c
            })
            .collect();
        let html = attention(&Config::default(), 100, &live(cards, None)).into_string();
        assert_eq!(html.matches("<li class=\"rl\"").count(), 8, "{html}");
        assert!(html.contains(r#"<span class="count-pill bad">11</span>"#));
        assert!(html.contains("3 more on the Containers page"));
        assert!(
            html.contains(r#"svc<span class="stk"> · stack0</span>"#),
            "{html}"
        );
    }

    /// Rows open their cause, say why inline and how long an unhealthy
    /// container has been unhealthy; the probe line only when the page
    /// put one in (a session, `Live::with_probes`) (OV-04, NAV-11).
    #[test]
    fn attention_rows_open_their_cause() {
        let mut sick = card("sick", "unhealthy", true);
        sick.dot = Some(crate::ui::view::Dot::Bad);
        let mut v = live(vec![sick], None);
        v.unhealthy_since.insert("sick".into(), 10_000 - 2460);
        let cfg = crate::config::parse(
            "[[alerts]]\nname = \"mem\"\ntype = \"host\"\nmetric = \"memory\"\nabove = 90\n",
            "t.toml",
        )
        .unwrap();
        let html = attention(&cfg, 10_000, &v).into_string();
        // Rows are keyed by subject, so a live update keeps the row a reader
        // is on (A11Y-02), and a new one flashes once.
        assert!(html.contains(r#"<ul class="rows" data-flash>"#), "{html}");
        assert!(
            html.contains(r#"<li class="rl" data-key="c:sick">"#),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<a class="nm row-link" href="/containers/sick" data-c="sick">sick</a>"#
            ),
            "{html}"
        );
        assert!(html.contains("unhealthy for 41m"), "{html}");
        assert!(
            !html.contains("probe"),
            "no probe line without a session: {html}"
        );
        assert!(html.contains(r#"href="/containers/sick/logs""#));
        v.probes.insert("sick".into(), "curl: (7) <refused>".into());
        let html = attention(&cfg, 10_000, &v).into_string();
        assert!(
            html.contains(r#"<span class="why" data-since="7540">unhealthy for 41m<span class="probe" data-since="7540" title="The check's output when this page was loaded"> · curl: (7) &lt;refused&gt;</span></span>"#),
            "{html}"
        );
        // A firing alert about the host opens the Host page and names its rule.
        v.status.incidents.push(crate::alerts::Incident {
            rule: "mem".into(),
            subject: "memory".into(),
            state: "92% > 90%".into(),
            value: "92".into(),
            since: 9_000,
            notified: true,
        });
        let html = attention(&cfg, 10_000, &v).into_string();
        assert!(
            html.contains(r#"<a class="nm row-link" href="/host">memory</a>"#),
            "{html}"
        );
        assert!(html.contains(" · rule mem"), "{html}");
        // A container with a firing alert: "alert firing" in the error
        // colour (a status class), never the accent of a plain link.
        v.status.incidents.push(crate::alerts::Incident {
            rule: "mem".into(),
            subject: "sick".into(),
            state: "x".into(),
            value: "1".into(),
            since: 9_400,
            notified: true,
        });
        let html = attention(&cfg, 10_000, &v).into_string();
        assert!(
            html.contains(r#"<a class="bad" href="/alerts">alert firing 10m</a>"#),
            "{html}"
        );
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
        // A full bar is one whole core, one decimal everywhere (OV-14).
        assert!(
            html.contains("width:90.0%") && html.contains("width:40.0%"),
            "{html}"
        );
        assert!(
            html.contains(">90.0%<") && html.contains(">40.0%<"),
            "{html}"
        );
        assert!(!html.contains("<i>a</i>"));
        assert!(
            html.contains(r#"href="/containers/b/logs""#),
            "Top CPU rows carry logs"
        );
        // Bars and tiles are keyed by container (HB.morph keeps a focused
        // link on its container when the ranks change).
        assert!(html.contains(r#"<li data-key="b">"#), "{html}");
        assert!(
            html.contains(r#"<li data-key="&lt;i&gt;a&lt;/i&gt;">"#),
            "{html}"
        );
        assert!(html.contains(r#"role="listitem" data-key="b""#), "{html}");
        assert!(top(&live(vec![a], None), false)
            .into_string()
            .contains("show_stats"));
        // Nothing to sample is said as such (ROB-01); waiting is a skeleton.
        let none = top(&live(vec![], None), true).into_string();
        assert!(
            none.contains("No containers yet") && !none.contains("data-wait"),
            "{none}"
        );
        let mut idle = card("c", "running", true);
        idle.cpu = None;
        idle.mem = None;
        let wait = top(&live(vec![idle], None), true).into_string();
        assert!(wait.contains("Waiting for the first sample"), "{wait}");
        assert!(
            wait.contains("data-wait")
                && !wait.contains("aria-busy")
                && wait.contains(r#"class="sk"#),
            "{wait}"
        );
        let down = top(&live(vec![], Some("no socket")), true).into_string();
        assert!(
            down.contains("empty bad") && down.contains("cannot be reached"),
            "{down}"
        );
        // Top CPU says why in the standard empty state too, not a bare line.
        assert_eq!(
            down.matches(r#"<div class="empty bad compact">"#).count(),
            2,
            "{down}"
        );
        assert!(!down.contains("Nothing to rank."), "{down}");
        let idle = top(&live(vec![card("c", "exited", false)], None), true).into_string();
        assert!(
            idle.contains("<h3>Nothing to rank</h3>")
                && idle.contains("<h3>Nothing is running</h3>"),
            "{idle}"
        );
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
        // Tiles too small for a name on a phone fold into "+N others"
        // (Y10, OV-09), a link to the table sorted by memory (OV-10).
        let count = |h: &str| {
            h.matches(r#"<div class="tm" role"#).count()
                + h.matches(r#"<div class="tm short" role"#).count()
        };
        let tiles = count(&html);
        assert!((1..=18).contains(&tiles), "{tiles}");
        assert!(html.contains(&format!("+{} others", 25 - tiles)), "{html}");
        assert!(
            html.contains(
                r#"<a class="tm tm-rest" role="listitem" data-key="+rest" href="/containers?sort=mem&amp;group=0""#
            ),
            "{html}"
        );
        let (k, rects) = tm_layout(&(0..25).rev().map(|i| (i + 1) as f64).collect::<Vec<_>>());
        assert_eq!(k, tiles);
        for r in &rects {
            assert!(
                r.2 * TM_W / 100.0 >= TM_MIN_W - 1e-6 && r.3 * TM_H / 100.0 >= TM_MIN_H - 1e-6,
                "{r:?}"
            );
        }
        // A tile: the name link stretched over it and, as its sibling (never
        // a link inside a link), the logs icon (OV-01).
        assert!(html.contains(r#"<a class="tm-a row-link" href="/containers/c24%3C"><span class="tm-n">c24&lt;</span><span class="tm-v">25.0 MiB</span></a><a class="ia logs tm-logs" href="/containers/c24%3C/logs""#), "{html}");
        assert!(html.contains("c24&lt;") && !html.contains("c24<"));
        assert!(treemap(&[]).into_string().contains("Waiting"));
        // Few containers: no fold.
        let few: Vec<&CardView> = cards.iter().rev().take(3).collect();
        let html = treemap(&few).into_string();
        assert_eq!(count(&html), 3, "{html}");
        assert!(!html.contains("others"));
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
        let html = event_row(1_794_660_400, &e, true, None).into_string();
        assert!(html.contains("pill bad") && html.contains(">exited<"));
        assert!(html.contains("href=\"/containers/web%3C1%3E\""), "{html}");
        assert!(html.contains("exit 137 &amp; more") && html.contains("web&lt;1&gt;"));
        assert!(!html.contains("/logs"), "no logs link without the names");
        let plain = event_row(1_794_660_400, &e, false, None).into_string();
        assert!(!plain.contains("href="));
        // A container whose logs may be read: one click to its logs at that
        // moment (unix ms), a sibling of the name link, never inside it.
        let names: std::collections::HashSet<String> = ["web<1>".to_string()].into();
        let html = event_row(1_794_660_400, &e, true, Some(&names)).into_string();
        assert!(
            html.contains(
                "<a class=\"ia logs\" href=\"/containers/web%3C1%3E/logs#t=1794660300000\""
            ),
            "{html}"
        );
        assert!(html.contains("aria-label=\"Logs of web&lt;1&gt;\""));
        assert!(!html.contains("<a class=\"nm\" href=\"/containers/web%3C1%3E\">web&lt;1&gt;<a"));
        // OV-05: five identical events in a row are one row "× 5".
        let five: Vec<crate::db::Event> = (0..5)
            .map(|i| crate::db::Event { id: i, ..e.clone() })
            .collect();
        assert_eq!(collapse(&five).len(), 1);
        assert_eq!(collapse(&five)[0].1, 5);
        let n = event_row_n(1, &e, true, None, 5, None).into_string();
        assert!(n.contains(">× 5<"), "{n}");
        // LG-47: a clock time, not "N ago"; LG-48: an alert row links to
        // what its rule watches, as the History timeline does.
        assert!(
            html.contains(r#"data-abs="1""#) && !html.contains(" ago<"),
            "{html}"
        );
        let cfg = crate::config::parse(
            "[[alerts]]\nname = \"disk\"\ntype = \"host\"\nmetric = \"disk\"\nabove = 90\n",
            "c.toml",
        )
        .unwrap();
        let al = crate::db::Event {
            kind: "alert.fired".into(),
            level: "warn".into(),
            subject: "/".into(),
            detail: "disk: 93%".into(),
            ..e.clone()
        };
        let a = event_row_n(1, &al, true, None, 1, Some(&cfg)).into_string();
        assert!(a.contains(r#"<a class="nm" href="/host">/</a>"#), "{a}");
        let a = event_row_n(1, &al, true, None, 1, None).into_string();
        assert!(a.contains(r#"href="/alerts""#), "{a}");
        assert_eq!(event_label("app.start"), "Hobnail started");
        assert_eq!(event_label("something.new"), "new");
        let other: std::collections::HashSet<String> = ["db".to_string()].into();
        assert!(!event_row(1, &e, true, Some(&other))
            .into_string()
            .contains("/logs"));
    }
}
