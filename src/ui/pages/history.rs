//! `/history`: host charts with a range picker, container restarts per day and
//! the incident timeline. Everything comes from the history database; with it
//! off the charts show their empty state.

use maud::{html, Markup};
use std::collections::BTreeMap;

use crate::db::{Agg, EventFilter, Range, SysMetric};
use crate::ui::chart::{line_chart_ex, markers_from_events, shifted, ChartSpec, Extras, Unit};
use crate::ui::fmt::{parse_query, query_get, url_encode};
use crate::ui::pages::overview::event_row;
use crate::ui::pages::page_sub;
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::{ic, panel_head};

const DAYS: usize = 7;

pub fn page(ctx: &Ctx) -> String {
    let q = parse_query(ctx.query);
    let preset = query_get(&q, "range")
        .and_then(Range::parse)
        .unwrap_or(Range::H24);
    let all_events = query_get(&q, "events") == Some("all");
    let compare = query_get(&q, "cmp") == Some("1");
    let db = &ctx.state.db;
    let w = Window::parse(&q, preset, ctx.now as i64);
    let (range, now) = (w.range, w.to);
    let series = |m| db.system_series(m, range, now);
    // Rollups carry bucket maxima: from 24 h up the chart shows them as a band.
    let band = |m| (range.secs() >= 86_400).then(|| db.system_series_agg(m, range, now, Agg::Max));
    let yday = |m| {
        (compare && w.secs <= 86_400)
            .then(|| shifted(&db.system_series(m, range, now - 86_400), 86_400))
    };
    let evs = db.events(
        &EventFilter {
            limit: 500,
            ..Default::default()
        }
        .for_viewer(false),
    );
    let marks = markers_from_events(&evs);
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
    // The current window as a query, for links that keep it.
    let here = if w.zoomed {
        format!("from={}&to={}", w.from(), w.to)
    } else {
        format!("range={}", preset.as_str())
    };
    let cmp_q = if compare { "&cmp=1" } else { "" };
    let en = db.is_enabled();
    let per_day = restarts_per_day(
        (1..=DAYS)
            .map(|d| db.restart_counts(now - d as i64 * 86_400))
            .collect(),
    );
    let events: Vec<_> = db
        .events(
            &EventFilter {
                limit: 200,
                ..Default::default()
            }
            .for_viewer(ctx.user.is_some()),
        )
        .into_iter()
        .filter(|e| all_events || e.level != "info")
        .take(60)
        .collect();
    let names = crate::logs::tailable(ctx.state);
    let body = html! {
        div.page {
            (page_sub(html! { "Host metrics and incidents from the local history database" }))
            div.toolbar {
                nav.seg aria-label="Range" {
                    @for r in Range::ALL {
                        a href={"/history?range=" (r.as_str()) (cmp_q)} aria-current=[(!w.zoomed && r == preset).then_some("true")] { (r.as_str()) }
                    }
                }
                @if w.secs <= 86_400 {
                    a.tgl href={"/history?" (here) @if !compare { "&cmp=1" }} aria-pressed=(if compare { "true" } else { "false" })
                        title="Draw the same window one day earlier as a faint line" {
                        i.tgl-box {} "Compare with yesterday"
                    }
                }
                @if w.zoomed {
                    span.zoomed {
                        (ic("search")) "Zoomed: " time.num data-t=(w.from()) data-abs="1" { (crate::ui::fmt::short_utc(w.from().max(0) as u64)) }
                        " – " time.num data-t=(w.to) data-abs="1" { (crate::ui::fmt::short_utc(w.to.max(0) as u64)) }
                        a.btn.sm href={"/history?range=" (preset.as_str()) (cmp_q)} { "Reset zoom" }
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
                        a href={"/history?" (here) "#events"} aria-current=[(!all_events).then_some("true")] { "Incidents" }
                        a href={"/history?" (here) "&events=all#events"} aria-current=[all_events.then_some("true")] { "All events" }
                    }
                }
                @if events.is_empty() {
                    p.muted {
                        @if en { "Nothing to show. " @if !all_events { "Warnings and errors will appear here; switch to all events to see starts and stops." } }
                        @else { "The timeline fills in once history is recording." }
                    }
                } @else {
                    ul.events { @for e in &events { (event_row(ctx.now, e, true, Some(&names))) } }
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

/// The time window the charts show: a preset range ending now, or a zoomed
/// `from`/`to` (unix seconds, from a drag on a chart) read at the smallest
/// preset resolution that covers it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// The preset whose tables and bucket size the series use.
    pub range: Range,
    pub to: i64,
    pub secs: i64,
    pub zoomed: bool,
}

impl Window {
    pub fn parse(q: &[(String, String)], preset: Range, now: i64) -> Self {
        let num = |k| query_get(q, k).and_then(|v| v.parse::<i64>().ok());
        if let (Some(f), Some(t)) = (num("from"), num("to")) {
            let t = t.min(now);
            let f = f.max(now - Range::D30.secs());
            if t - f >= 300 {
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
                        @for i in (0..DAYS).rev() { th.num scope="col" { (day_label(i)) } }
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
        p.foot-line { (ic("info")) "A start is a " code { "container.start" } " event; a crash loop shows up as a big number." }
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
        // Future end is clamped; too-short and garbage windows fall back.
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
    }
}
