//! `/history`: host charts with a range picker, container restarts per day and
//! the incident timeline. Everything comes from the history database; with it
//! off the charts show their empty state.
//!
//! The timeline and the chart markers show the chart window (a preset range
//! or a zoomed `from`/`to`), and the timeline pages back with `before=<id>`
//! (LG-43). The window moves and zooms with plain links, so touch and a page
//! without the script can do what a drag does (LG-46, LG-50).

use maud::{html, Markup};
use std::collections::{BTreeMap, HashSet};

use crate::db::{Agg, Event, EventFilter, Range, SysMetric};
use crate::ui::chart::{
    line_chart_ex, markers_from_events, shifted, ChartSpec, Extras, Marker, Unit,
};
use crate::ui::fmt::{clock_tag, parse_query, query_get, url_encode};
use crate::ui::pages::{kind_label, page_sub};
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::{ic, panel_head, pill};

const DAYS: usize = 7;

pub fn page(ctx: &Ctx) -> String {
    let q = parse_query(ctx.query);
    let preset = query_get(&q, "range")
        .and_then(Range::parse)
        .unwrap_or(Range::H24);
    let all_events = query_get(&q, "events") == Some("all");
    let compare = query_get(&q, "cmp") == Some("1");
    let before = query_get(&q, "before").and_then(|v| v.parse::<i64>().ok());
    let db = &ctx.state.db;
    let now_i = ctx.now as i64;
    let w = Window::parse(&q, preset, now_i);
    let (range, now) = (w.range, w.to);
    let series = |m| db.system_series(m, range, now);
    // Rollups carry bucket maxima: from 24 h up the chart shows them as a band.
    let band = |m| (range.secs() >= 86_400).then(|| db.system_series_agg(m, range, now, Agg::Max));
    let can_compare = w.secs <= 86_400;
    let yday = |m| {
        (compare && can_compare).then(|| shifted(&db.system_series(m, range, now - 86_400), 86_400))
    };
    let signed = ctx.user.is_some();
    // The events inside the chart window, newest first (LG-43): they feed
    // the markers and the timeline, so both show the same window.
    let in_window = scan(db, signed, w.from(), w.to, None);
    let tl = match before {
        Some(_) => Timeline::page(&scan(db, signed, w.from(), w.to, before), all_events),
        None => Timeline::page(&in_window, all_events),
    };
    // The current window as a query, for links that keep it.
    let here = w.query(preset);
    let cmp_q = if compare { "&cmp=1" } else { "" };
    let ev_q = if all_events { "&events=all" } else { "" };
    let shown: HashSet<i64> = tl.rows.iter().map(|e| e.id).collect();
    let marks = point_markers(
        markers_from_events(&in_window.events),
        &in_window.events,
        &shown,
        &format!("{here}{cmp_q}"),
        all_events,
    );
    let spec = |title, unit, color, cap, zero_based| ChartSpec {
        title,
        unit,
        color,
        range_secs: w.secs,
        now,
        cap,
        zero_based,
        compact: false,
    };
    let chart = |title, unit, color, cap, zero_based, m: SysMetric| {
        let (mx, cmp) = (band(m), yday(m));
        let x = Extras {
            max: mx.as_ref(),
            compare: cmp.as_ref(),
            markers: &marks,
            zoom: true,
        };
        line_chart_ex(
            &spec(title, unit, color, cap, zero_based),
            &series(m),
            db.is_enabled(),
            &x,
        )
    };
    let en = db.is_enabled();
    let per_day = restarts_per_day(
        (1..=DAYS)
            .map(|d| db.restart_counts(now_i - d as i64 * 86_400))
            .collect(),
    );
    let names = crate::logs::tailable(ctx.state);
    let steps = w.steps(now_i);
    let step = |label: &str, to: Option<&str>, hint: &str| {
        html! {
            @match to {
                Some(q) => a.btn.sm href={"/history?" (q) (cmp_q) (ev_q)} title=(hint) { (label) },
                None => span.btn.sm aria-disabled="true" title=(hint) { (label) },
            }
        }
    };
    let body = html! {
        div.page {
            (page_sub(html! { "Host metrics and incidents from the local history database" }))
            div.toolbar.hist-bar {
                nav.seg aria-label="Range" {
                    @for r in Range::ALL {
                        a href={"/history?range=" (r.as_str()) (cmp_q) (ev_q)} aria-current=[(!w.zoomed && r == preset).then_some("true")] { (r.as_str()) }
                    }
                }
                // Moving and zooming the window are plain links: by touch,
                // keyboard or without the script (LG-46, LG-50).
                nav.zoom-nav aria-label="Move or zoom the window" {
                    (step("‹ Earlier", Some(&steps.earlier), "The half window before this one"))
                    (step("Zoom in", steps.zoom_in.as_deref(), "Half this window around its middle (5 min at least)"))
                    (step("Zoom out", Some(&steps.zoom_out), "Twice this window"))
                    (step("Later ›", steps.later.as_deref(), "The half window after this one"))
                }
                // In the same place at every range, with a reason when it
                // cannot apply (LG-52).
                @if can_compare {
                    a.tgl href={"/history?" (here) @if !compare { "&cmp=1" } (ev_q)} aria-pressed=(if compare { "true" } else { "false" })
                        title="Draw the same window one day earlier as a faint line" {
                        i.tgl-box {} "Compare with yesterday"
                    }
                } @else {
                    span.tgl aria-disabled="true" title="Only for windows of 24 h or less" {
                        i.tgl-box {} "Compare with yesterday"
                    }
                }
                @if w.zoomed {
                    span.zoomed {
                        (ic("search")) "Zoomed: " time.num data-t=(w.from()) data-abs="1" { (crate::ui::fmt::short_utc(w.from().max(0) as u64)) }
                        " – " time.num data-t=(w.to) data-abs="1" { (crate::ui::fmt::short_utc(w.to.max(0) as u64)) }
                        a.btn.sm href={"/history?range=" (preset.as_str()) (cmp_q) (ev_q)} { "Reset zoom" }
                    }
                } @else {
                    span.hint.muted.js-only { "Drag across a chart to zoom in" }
                }
            }
            @if !en { (crate::ui::pages::history_off_banner()) }
            div.charts.two.sync {
                (chart("CPU", Unit::Percent, "--ch-cpu", Some(100.0), true, SysMetric::Cpu))
                (chart("Load (1 min)", Unit::Plain, "--ch-load", None, true, SysMetric::Load1))
                (chart("Memory", Unit::Bytes, "--ch-mem", None, true, SysMetric::Mem))
                (chart("Swap", Unit::Bytes, "--ch-swap", None, true, SysMetric::Swap))
                (chart("Disk used", Unit::Percent, "--ch-disk", Some(100.0), true, SysMetric::Disk))
                (chart("Temperature", Unit::Celsius, "--ch-temp", None, false, SysMetric::Temp))
                (chart("Network in", Unit::BytesPerSec, "--ch-rx", None, true, SysMetric::NetRx))
                (chart("Network out", Unit::BytesPerSec, "--ch-tx", None, true, SysMetric::NetTx))
            }
            section.panel.sec aria-label="Restarts per day" {
                (panel_head("Container starts per day", None))
                (restarts_table(ctx.now, &per_day, en, &names))
            }
            section.panel.sec #events aria-label="Incident timeline" {
                div.panel-h {
                    h2 { "Incident timeline" }
                    nav.seg aria-label="Events" {
                        a href={"/history?" (here) (cmp_q) "#events"} aria-current=[(!all_events).then_some("true")] { "Incidents" }
                        a href={"/history?" (here) (cmp_q) "&events=all#events"} aria-current=[all_events.then_some("true")] { "All events" }
                    }
                }
                p.muted.tl-scope {
                    "In the chart window, newest first."
                    @if before.is_some() {
                        " " a href={"/history?" (here) (cmp_q) (ev_q) "#events"} { "Back to the newest" }
                    }
                }
                @if tl.rows.is_empty() {
                    p.muted {
                        @if !en { "The timeline fills in once history is recording." }
                        @else if all_events { "Nothing was recorded in this window." }
                        @else {
                            "No warnings or errors in this window. "
                            a href={"/history?" (here) (cmp_q) "&events=all#events"} { "All events" }
                            " lists starts and stops too."
                        }
                    }
                } @else {
                    ul.events.tl { @for e in &tl.rows { (tl_row(&ctx.cfg, e, &names)) } }
                }
                @if let Some(b) = tl.older {
                    p.tl-more {
                        a.btn.sm href={"/history?" (here) (cmp_q) (ev_q) "&before=" (b) "#events"} { "Older events" }
                    }
                }
            }
        }
    };
    layout(
        ctx,
        Nav::History,
        "History",
        crate::ui::pages::crumb_title("History"),
        body,
    )
}

/// Point each marker at its timeline row (LG-44): a plain anchor when the
/// row is on this page, else the timeline page that starts with it (the
/// same window; "All events" when the event is not an incident).
fn point_markers(
    marks: Vec<Marker>,
    evs: &[Event],
    shown: &HashSet<i64>,
    here: &str,
    all_events: bool,
) -> Vec<Marker> {
    marks
        .into_iter()
        .map(|mut m| {
            m.href = Some(if shown.contains(&m.id) {
                format!("#ev-{}", m.id)
            } else {
                let info = evs.iter().any(|e| e.id == m.id && e.level == "info");
                let all = if info || all_events {
                    "&events=all"
                } else {
                    ""
                };
                format!("/history?{here}{all}&before={}#ev-{}", m.id + 1, m.id)
            });
            m
        })
        .collect()
}

/// How many events one timeline page shows.
const TL_PAGE: usize = 60;
/// Events read per query while walking back through a window, and the most
/// queries one page load makes. Read on a page load only, never by a poll;
/// a window with more events than this pages on with "Older events".
const SCAN_BATCH: usize = 500;
const SCAN_PAGES: usize = 20;

/// Events inside a window, newest first, read with the existing paged
/// query (`before_id`) back to the window's start.
#[derive(Debug, Clone, Default)]
struct Scan {
    events: Vec<Event>,
    /// The scan stopped at its page limit before the window's start: the
    /// next page starts below this id.
    cut: Option<i64>,
}

/// The events inside `from..=to`, newest first, as the History page reads
/// them (the same bounded scan), for other charts' markers (Host, LG-39).
pub fn window_events(db: &crate::db::Db, signed: bool, from: i64, to: i64) -> Vec<Event> {
    scan(db, signed, from, to, None).events
}

fn scan(db: &crate::db::Db, signed: bool, from: i64, to: i64, before: Option<i64>) -> Scan {
    let mut out = Scan::default();
    let mut cursor = before;
    for _ in 0..SCAN_PAGES {
        let page = db.events(
            &EventFilter {
                limit: SCAN_BATCH,
                before_id: cursor,
                ..Default::default()
            }
            .for_viewer(signed),
        );
        let Some(last) = page.last() else {
            return out;
        };
        cursor = Some(last.id);
        let full = page.len() == SCAN_BATCH;
        for e in page {
            if e.ts < from {
                return out;
            }
            if e.ts <= to {
                out.events.push(e);
            }
        }
        if !full {
            return out;
        }
    }
    out.cut = cursor;
    out
}

/// One page of the timeline and where the next one starts.
#[derive(Debug, Clone, Default)]
struct Timeline {
    rows: Vec<Event>,
    older: Option<i64>,
}

impl Timeline {
    fn page(s: &Scan, all: bool) -> Self {
        let mut rows: Vec<Event> = s
            .events
            .iter()
            .filter(|e| all || e.level != "info")
            .take(TL_PAGE + 1)
            .cloned()
            .collect();
        let older = if rows.len() > TL_PAGE {
            rows.pop();
            rows.last().map(|e| e.id)
        } else {
            s.cut
        };
        Timeline { rows, older }
    }
}

/// One timeline row (LG-47, LG-48): an anchor (`#ev-<id>`, what a chart
/// marker opens), the clock time, what happened in a word, the subject as a
/// link to it, and a container's logs at that moment.
fn tl_row(cfg: &crate::config::Config, e: &Event, logs: &HashSet<String>) -> Markup {
    let kind = match e.level.as_str() {
        "error" => "bad",
        "warn" => "warn",
        _ => "",
    };
    let word = match e.kind.as_str() {
        "alert.fired" => "fired",
        "alert.resolved" => "resolved",
        "alert.delivery_failed" => "not delivered",
        k => kind_label(k),
    };
    let href: Option<(String, bool)> = if e.subject.is_empty() {
        None
    } else if e.kind.starts_with("container.") {
        Some((format!("/containers/{}", url_encode(&e.subject)), false))
    } else if e.kind == "alert.fired" || e.kind == "alert.resolved" {
        e.detail
            .split_once(": ")
            .and_then(|(rule, _)| crate::ui::alerts::subject_href(cfg, rule, &e.subject))
    } else {
        None
    };
    html! {
        li.ev id={"ev-" (e.id)} {
            span.when { (clock_tag(e.ts.max(0) as u64)) }
            (pill(kind, word))
            @match href {
                Some((u, true)) => a.nm href=(u) target="_blank" rel="noopener noreferrer" { (e.subject) " ↗" },
                Some((u, false)) => a.nm href=(u) { (e.subject) },
                None => @if !e.subject.is_empty() { span.nm { (e.subject) } },
            }
            @if !e.detail.is_empty() { span.muted.detail { (crate::ui::pages::alerts::without_echo(&e.detail)) } }
            @if logs.contains(&e.subject) {
                a.ia.logs href=(crate::logs::logs_at(&e.subject, e.ts)) title="Logs at that moment"
                    aria-label={"Logs of " (e.subject) " at that moment"} { (ic("logs")) }
            }
        }
    }
}

/// The narrowest window the charts show (five minutes).
const MIN_WINDOW: i64 = 300;

/// The time window the charts show: a preset range ending now, or a zoomed
/// `from`/`to` (unix seconds, from a drag on a chart or a toolbar step)
/// read at the smallest preset resolution that covers it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// The preset whose tables and bucket size the series use.
    pub range: Range,
    pub to: i64,
    pub secs: i64,
    pub zoomed: bool,
}

/// Where the toolbar moves the window, as queries: earlier and later by
/// half its length, zoomed in or out around its middle. `None` when the
/// step cannot be taken (past now, narrower than five minutes).
#[derive(Debug, Clone, PartialEq)]
pub struct Steps {
    pub earlier: String,
    pub later: Option<String>,
    pub zoom_in: Option<String>,
    pub zoom_out: String,
}

impl Window {
    pub fn parse(q: &[(String, String)], preset: Range, now: i64) -> Self {
        let num = |k| query_get(q, k).and_then(|v| v.parse::<i64>().ok());
        if let (Some(f), Some(t)) = (num("from"), num("to")) {
            let t = t.min(now);
            let f = f.max(now - Range::D30.secs());
            // A window under five minutes (a short drag) snaps to five
            // minutes around its middle instead of doing nothing (LG-50).
            let (f, t) = if t > f && t - f < MIN_WINDOW {
                let t = ((f + t) / 2 + MIN_WINDOW / 2).min(now);
                (t - MIN_WINDOW, t)
            } else {
                (f, t)
            };
            if t - f >= MIN_WINDOW {
                let secs = t - f;
                let range = Range::ALL
                    .into_iter()
                    .find(|r| r.secs() >= secs)
                    .unwrap_or(Range::D30);
                return Window {
                    range,
                    to: t,
                    secs,
                    zoomed: true,
                };
            }
        }
        Window {
            range: preset,
            to: now,
            secs: preset.secs(),
            zoomed: false,
        }
    }

    pub fn from(&self) -> i64 {
        self.to - self.secs
    }

    /// The window as a query, for links that keep it.
    pub fn query(&self, preset: Range) -> String {
        if self.zoomed {
            format!("from={}&to={}", self.from(), self.to)
        } else {
            format!("range={}", preset.as_str())
        }
    }

    /// The toolbar's four moves (`from=&to=`, or `range=30d` when zooming
    /// out reaches the longest range).
    pub fn steps(&self, now: i64) -> Steps {
        let q = |f: i64, t: i64| format!("from={f}&to={t}");
        let half = self.secs / 2;
        let f = (self.from() - half).max(now - Range::D30.secs());
        let earlier = q(f, f + self.secs);
        let later = (self.to < now).then(|| {
            let t = (self.to + half).min(now);
            q(t - self.secs, t)
        });
        let mid = self.to - half;
        let zoom_in = (half >= MIN_WINDOW).then(|| q(mid - half / 2, mid - half / 2 + half));
        let zoom_out = if self.secs * 2 >= Range::D30.secs() {
            "range=30d".to_string()
        } else {
            let t = (mid + self.secs).min(now);
            q(t - self.secs * 2, t)
        };
        Steps {
            earlier,
            later,
            zoom_in,
            zoom_out,
        }
    }
}

/// Turn cumulative "starts since N days ago" counts (index 0 = since 1 day
/// ago) into per-day counts, newest day first. Containers without any start in
/// the window are left out.
pub fn restarts_per_day(cum: Vec<Vec<(String, u32)>>) -> BTreeMap<String, [u32; DAYS]> {
    let mut out: BTreeMap<String, [u32; DAYS]> = BTreeMap::new();
    for (i, rows) in cum.iter().enumerate().take(DAYS) {
        for (name, n) in rows {
            let prev = if i == 0 {
                0
            } else {
                cum[i - 1]
                    .iter()
                    .find(|(m, _)| m == name)
                    .map(|x| x.1)
                    .unwrap_or(0)
            };
            let e = out.entry(name.clone()).or_insert([0; DAYS]);
            e[i] = n.saturating_sub(prev);
        }
    }
    out.retain(|_, v| v.iter().any(|&n| n > 0));
    out
}

fn restarts_table(
    now: u64,
    rows: &BTreeMap<String, [u32; DAYS]>,
    db: bool,
    logs: &std::collections::HashSet<String>,
) -> Markup {
    if rows.is_empty() {
        return html! {
            p.muted {
                @if db { "No container started in the last 7 days." }
                @else { "Starts per container appear once history is recording." }
            }
        };
    }
    let mut v: Vec<(&String, &[u32; DAYS])> = rows.iter().collect();
    v.sort_by_key(|(n, d)| (std::cmp::Reverse(d.iter().sum::<u32>()), (*n).clone()));
    let max = v
        .iter()
        .flat_map(|(_, d)| d.iter())
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    let day_label = |i: usize| -> String {
        let t = now.saturating_sub(i as u64 * 86_400);
        crate::ui::fmt::short_utc(t)
            .split(' ')
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    };
    html! {
        div.scroll-x {
            table.tbl.small.heat {
                thead {
                    tr {
                        th { "Container" }
                        // On a phone only the day's number shows, so the
                        // week fits the screen (the month is in the title).
                        @for i in (0..DAYS).rev() {
                            @let l = day_label(i);
                            @let (d, m) = l.split_once(' ').unwrap_or((l.as_str(), ""));
                            th.num scope="col" title=(l) { (d) span.hm { " " (m) } }
                        }
                        th.num { "Total" }
                    }
                }
                tbody {
                    @for (name, d) in v {
                        tr {
                            th scope="row" {
                                span.nm-l {
                                    a href={"/containers/" (url_encode(name))} { (name) }
                                    @if logs.contains(name.as_str()) {
                                        a.ia.logs href=(crate::logs::logs_url(name)) title="Logs" aria-label={"Logs of " (name)} { (ic("logs")) }
                                    }
                                }
                            }
                            @for i in (0..DAYS).rev() {
                                @let n = d[i];
                                td.num.hot[n > 0] style={"--h:" (format!("{:.2}", n as f64 / max as f64))} { @if n > 0 { (n) } @else { span.muted { "·" } } }
                            }
                            td.num { b { (d.iter().sum::<u32>()) } }
                        }
                    }
                }
            }
        }
        // One sentence in one element: the line is a flex row (LG-54).
        p.foot-line { (ic("info")) span { "A start is a " code { "container.start" } " event; a crash loop shows up as a big number." } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(rows: &[(&str, u32)]) -> Vec<(String, u32)> {
        rows.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    #[test]
    fn cumulative_counts_become_days() {
        // since 1d ago: a=2; since 2d: a=5, b=1; since 3d: a=5, b=1; ... b only on day 2.
        let cum = vec![
            r(&[("a", 2)]),
            r(&[("a", 5), ("b", 1)]),
            r(&[("a", 5), ("b", 1)]),
            r(&[("a", 5), ("b", 1)]),
            r(&[("a", 5), ("b", 1)]),
            r(&[("a", 5), ("b", 1)]),
            r(&[("a", 9), ("b", 1)]),
        ];
        let d = restarts_per_day(cum);
        assert_eq!(d["a"], [2, 3, 0, 0, 0, 0, 4]);
        assert_eq!(d["b"], [0, 1, 0, 0, 0, 0, 0]);
        assert!(restarts_per_day(vec![]).is_empty());
        assert!(
            restarts_per_day(vec![r(&[("x", 0)])]).is_empty(),
            "zero rows are dropped"
        );
    }

    #[test]
    fn zoom_window() {
        let q = |s: &str| parse_query(s);
        let now = 1_800_000_000;
        let w = Window::parse(&q(""), Range::H6, now);
        assert_eq!(
            (w.range, w.to, w.secs, w.zoomed),
            (Range::H6, now, 6 * 3600, false)
        );
        // Two hours ending an hour ago: read at 6 h resolution.
        let w = Window::parse(
            &q(&format!("from={}&to={}", now - 3 * 3600, now - 3600)),
            Range::H24,
            now,
        );
        assert_eq!((w.range, w.secs, w.zoomed), (Range::H6, 7200, true));
        assert_eq!(w.from(), now - 3 * 3600);
        // Future end is clamped; garbage windows fall back.
        let w = Window::parse(
            &q(&format!("from={}&to={}", now - 600, now + 9999)),
            Range::H1,
            now,
        );
        assert_eq!((w.to, w.secs), (now, 600));
        assert!(!Window::parse(&q("from=10&to=20"), Range::H1, now).zoomed);
        assert!(!Window::parse(&q("from=x&to=1"), Range::H1, now).zoomed);
        // Older than 30 days is cut to 30 days.
        let w = Window::parse(&q(&format!("from=0&to={now}")), Range::H1, now);
        assert_eq!((w.range, w.secs), (Range::D30, Range::D30.secs()));
    }

    /// A drag under five minutes snaps to five minutes around its middle
    /// (LG-50); at the right edge it ends now.
    #[test]
    fn short_windows_snap_to_five_minutes() {
        let now = 1_800_000_000;
        let q = |f: i64, t: i64| parse_query(&format!("from={f}&to={t}"));
        let w = Window::parse(&q(now - 4000, now - 3940), Range::H1, now);
        assert_eq!((w.zoomed, w.secs, w.from()), (true, 300, now - 4120));
        let w = Window::parse(&q(now - 30, now), Range::H1, now);
        assert_eq!((w.to, w.secs), (now, 300));
        assert!(!Window::parse(&q(now - 10, now - 20), Range::H1, now).zoomed);
    }

    /// The toolbar's moves: plain queries for touch and no script (LG-46).
    #[test]
    fn steps_move_and_zoom_the_window() {
        let now = 1_800_000_000;
        let w = Window::parse(&parse_query(""), Range::H1, now);
        let s = w.steps(now);
        assert_eq!(s.earlier, format!("from={}&to={}", now - 5400, now - 1800));
        assert_eq!(s.later, None, "nothing later than now");
        assert_eq!(
            s.zoom_in,
            Some(format!("from={}&to={}", now - 2700, now - 900))
        );
        assert_eq!(s.zoom_out, format!("from={}&to={now}", now - 7200));
        // A zoomed window in the past can move later, up to now.
        let w = Window::parse(
            &parse_query(&format!("from={}&to={}", now - 7200, now - 3600)),
            Range::H1,
            now,
        );
        let s = w.steps(now);
        assert_eq!(
            s.later,
            Some(format!("from={}&to={}", now - 5400, now - 1800))
        );
        // Five minutes cannot be halved; 30 days is as far out as it goes.
        let w = Window::parse(
            &parse_query(&format!("from={}&to={}", now - 300, now)),
            Range::H1,
            now,
        );
        assert_eq!(w.steps(now).zoom_in, None);
        let w = Window::parse(&parse_query(""), Range::D30, now);
        assert_eq!(w.steps(now).zoom_out, "range=30d");
        assert_eq!(w.query(Range::D30), "range=30d");
    }

    fn ev(id: i64, ts: i64, kind: &str, level: &str, subject: &str, detail: &str) -> Event {
        Event {
            id,
            ts,
            kind: kind.into(),
            level: level.into(),
            subject: subject.into(),
            detail: detail.into(),
        }
    }

    /// A timeline page: incidents only unless "all", 60 rows, and the next
    /// page starts below the last row shown (LG-43).
    #[test]
    fn timeline_pages_back() {
        let evs: Vec<Event> = (0..130)
            .rev()
            .map(|i| {
                let lv = if i % 2 == 0 { "warn" } else { "info" };
                ev(i, 1000 + i, "container.die", lv, "web", "")
            })
            .collect();
        let s = Scan {
            events: evs,
            cut: None,
        };
        let t = Timeline::page(&s, false);
        assert_eq!(t.rows.len(), TL_PAGE);
        assert!(t.rows.iter().all(|e| e.level == "warn"));
        assert_eq!(t.rows[0].id, 128);
        assert_eq!(t.older, Some(t.rows[TL_PAGE - 1].id));
        let t = Timeline::page(&s, true);
        assert_eq!((t.rows.len(), t.older), (TL_PAGE, Some(70)));
        // A short window: no older page, unless the scan was cut short.
        let few = Scan {
            events: s.events[..3].to_vec(),
            cut: None,
        };
        assert_eq!(Timeline::page(&few, true).older, None);
        let cut = Scan {
            events: s.events[..3].to_vec(),
            cut: Some(42),
        };
        assert_eq!(Timeline::page(&cut, true).older, Some(42));
    }

    /// The scan reads the window only, back to its start, with the paged
    /// query the JSON API uses.
    #[test]
    fn scan_reads_the_window() {
        let db = crate::db::Db::open_in_memory().unwrap();
        for i in 0..1200 {
            db.record_event(10_000 + i, "container.die", "warn", "web", "");
        }
        db.record_event(20_000, "auth.login", "info", "admin", "");
        let s = scan(&db, false, 10_100, 10_999, None);
        assert_eq!(s.events.len(), 900);
        assert_eq!(s.events[0].ts, 10_999);
        assert_eq!(s.events.last().unwrap().ts, 10_100);
        assert_eq!(s.cut, None);
        // Paging: below an id only.
        let below = s.events[10].id;
        let s2 = scan(&db, false, 10_100, 10_999, Some(below));
        assert_eq!(s2.events[0].id, below - 1);
        // Signed out, login events are never read.
        let s = scan(&db, false, 0, 30_000, None);
        assert!(s.events.iter().all(|e| !e.kind.starts_with("auth.")));
        let s = scan(&db, true, 0, 30_000, None);
        assert!(s.events.iter().any(|e| e.kind == "auth.login"));
    }

    /// Rows: an anchor for the marker, a clock time, the subject linked,
    /// logs at that moment (LG-44, LG-47, LG-48).
    #[test]
    fn timeline_rows_link_and_anchor() {
        let cfg = crate::config::parse(
            "[[alerts]]\nname = \"mem\"\ntype = \"host\"\nmetric = \"memory\"\nabove = 90\n",
            "t.toml",
        )
        .unwrap();
        let names: HashSet<String> = ["web".to_string()].into();
        let h = tl_row(
            &cfg,
            &ev(
                7,
                1_700_000_000,
                "container.die",
                "warn",
                "web",
                "exit code 1",
            ),
            &names,
        )
        .into_string();
        assert!(h.starts_with(r#"<li class="ev" id="ev-7">"#), "{h}");
        assert!(h.contains(r#"data-abs="1""#), "clock time: {h}");
        assert!(h.contains(r#"<a class="nm" href="/containers/web">web</a>"#));
        assert!(h.contains(r#"href="/containers/web/logs#t=1700000000000""#));
        let h = tl_row(
            &cfg,
            &ev(
                8,
                1_700_000_000,
                "alert.fired",
                "warn",
                "memory",
                "mem: 92% > 90%",
            ),
            &names,
        )
        .into_string();
        assert!(
            h.contains(">fired<") && h.contains(r#"href="/host""#),
            "{h}"
        );
        let h = tl_row(&cfg, &ev(9, 1, "config.saved", "info", "<x>", ""), &names).into_string();
        assert!(!h.contains("href") && h.contains("&lt;x&gt;"), "{h}");
    }

    /// A marker opens its row: an anchor when the row is on the page, else
    /// the timeline page that starts with it, in the same window.
    #[test]
    fn markers_point_at_their_rows() {
        let evs = vec![
            ev(5, 100, "container.oom", "error", "web", ""),
            ev(4, 90, "container.die", "warn", "db", "exit code 1"),
        ];
        let marks = markers_from_events(&evs);
        let shown: HashSet<i64> = [5].into();
        let m = point_markers(marks, &evs, &shown, "range=24h", false);
        assert_eq!(m[0].href.as_deref(), Some("#ev-5"));
        assert_eq!(
            m[1].href.as_deref(),
            Some("/history?range=24h&before=5#ev-4")
        );
    }

    #[test]
    fn table_states() {
        let none = std::collections::HashSet::new();
        let empty = restarts_table(1_800_000_000, &BTreeMap::new(), false, &none).into_string();
        assert!(empty.contains("once history is recording"));
        let mut m = BTreeMap::new();
        m.insert("<x>".to_string(), [1, 0, 0, 0, 0, 0, 2]);
        m.insert("gone".to_string(), [1, 0, 0, 0, 0, 0, 0]);
        let names = ["<x>".to_string()].into();
        let html = restarts_table(1_800_000_000, &m, true, &names).into_string();
        assert!(html.contains("&lt;x&gt;") && !html.contains("<x>"));
        assert!(html.contains("<b>3</b>"));
        // One click to the logs of a container that still exists.
        assert!(
            html.contains(r#"<a class="ia logs" href="/containers/%3Cx%3E/logs""#),
            "{html}"
        );
        assert!(!html.contains("/containers/gone/logs"), "no dead logs link");
        // The week fits a phone: the day's number, the month hidden there
        // and in the title (History at 390 px).
        assert!(
            html.contains(
                r#"<th class="num" scope="col" title="15 Jan">15<span class="hm"> Jan</span></th>"#
            ),
            "{html}"
        );
        assert!(include_str!("../../../static/alerts.css")
            .contains("@media (max-width:699px){.heat .hm{display:none}"));
    }
}
