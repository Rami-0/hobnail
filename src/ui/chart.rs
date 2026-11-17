//! Inline SVG charts, drawn on the server from a `db::Series` (or live values).
//!
//! The plot is an SVG with `preserveAspectRatio="none"` and non-scaling
//! strokes, so it stretches to any width without distorting lines; axis labels
//! are HTML around it, so text stays crisp. Hover read-out data travels in
//! `data-*` attributes (`data-v` holds one value per time slot, empty for a
//! gap) and `static/app.js` turns it into a tooltip. Without JS the chart is
//! still readable: axis labels, min/max/last and an `aria-label` summary.

use maud::{html, Markup};

use crate::db::Series;
use crate::util::{fmt_utc, human_bytes};

/// What a value means; decides axis scaling and number formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Bytes,
    BytesPerSec,
    Celsius,
    Plain,
}

impl Unit {
    /// Tag read by the script's formatter (`static/app.js`, `fmtVal`).
    fn tag(self) -> &'static str {
        match self {
            Unit::Percent => "pct",
            Unit::Bytes => "b",
            Unit::BytesPerSec => "bps",
            Unit::Celsius => "c",
            Unit::Plain => "n",
        }
    }

    fn is_bytes(self) -> bool {
        matches!(self, Unit::Bytes | Unit::BytesPerSec)
    }

    /// Full-precision label ("12.4%", "1.5 GiB", "3.2 MiB/s", "61 °C").
    pub fn fmt(self, v: f64) -> String {
        if !v.is_finite() {
            return "—".into();
        }
        match self {
            Unit::Percent if v.abs() < 10.0 => format!("{v:.1}%"),
            Unit::Percent => format!("{v:.0}%"),
            Unit::Bytes => human_bytes(v.max(0.0) as u64),
            Unit::BytesPerSec => format!("{}/s", human_bytes(v.max(0.0) as u64)),
            Unit::Celsius => format!("{v:.0} °C"),
            // Counts (PIDs, OOM kills, pages) are whole numbers (OV-49).
            Unit::Plain if v.fract() == 0.0 => format!("{v:.0}"),
            Unit::Plain if v.abs() < 10.0 => format!("{v:.2}"),
            Unit::Plain => format!("{v:.1}"),
        }
    }

    /// Short axis label: "0" for zero, no decimals when avoidable.
    fn axis(self, v: f64) -> String {
        if v == 0.0 {
            return "0".into();
        }
        match self {
            Unit::Percent => format!("{}%", trim(v)),
            Unit::Celsius => format!("{}°", trim(v)),
            Unit::Plain => trim(v),
            Unit::Bytes => human_bytes(v as u64),
            Unit::BytesPerSec => format!("{}/s", human_bytes(v as u64)),
        }
    }
}

/// `12.50` -> "12.5", `3.00` -> "3".
fn trim(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What to draw.
#[derive(Debug, Clone)]
pub struct ChartSpec<'a> {
    pub title: &'a str,
    pub unit: Unit,
    /// CSS custom property holding the series colour, e.g. `--ch-cpu`.
    pub color: &'a str,
    /// Window length in seconds and its right edge (unix seconds).
    pub range_secs: i64,
    pub now: i64,
    /// Never let the y axis exceed this (host CPU: 100).
    pub cap: Option<f64>,
    /// Start the y axis at zero (everything but temperature).
    pub zero_based: bool,
    /// Short chart for the overview grid.
    pub compact: bool,
}

pub const VB_W: f64 = 1000.0;
pub const VB_H: f64 = 100.0;
/// Do not embed hover data for more slots than this.
const MAX_SLOTS: i64 = 2000;

/// The geometry of one chart, separate from markup so it can be tested.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    /// Line segments (one `M` per gap-free run).
    pub line: String,
    /// Closed areas under the line segments.
    pub area: String,
    /// Isolated points (runs of length one), as zero-length segments.
    pub dots: String,
    pub y_lo: f64,
    pub y_hi: f64,
    pub last: f64,
    pub min: f64,
    pub max: f64,
    pub n: usize,
}

/// Pick a "nice" number at or above `x` from 1, 2, 4, 5, 8 times a power of ten
/// (so that halving gives tidy axis labels too).
fn nice_ceil(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let e = 10f64.powf(x.log10().floor());
    let m = x / e;
    let step = [1.0, 2.0, 4.0, 5.0, 8.0, 10.0]
        .into_iter()
        .find(|&s| m <= s + 1e-9)
        .unwrap_or(10.0);
    step * e
}

/// Round `x` up to a nice multiple of a power of 1024 (for byte axes).
fn nice_ceil_bytes(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let k = (x.log2() / 10.0).floor().max(0.0);
    let unit = 1024f64.powf(k);
    nice_ceil(x / unit) * unit
}

fn y_range(spec: &ChartSpec, min: f64, max: f64) -> (f64, f64) {
    let mut hi;
    let mut lo = 0.0;
    if spec.zero_based {
        let want = max * 1.1;
        hi = if spec.unit.is_bytes() {
            nice_ceil_bytes(want)
        } else {
            nice_ceil(want)
        };
        if spec.unit == Unit::Percent {
            hi = hi.max(5.0);
        }
    } else {
        // Temperature and the like: a window around the data, in steps of 5.
        lo = ((min - 3.0) / 5.0).floor() * 5.0;
        hi = ((max + 3.0) / 5.0).ceil() * 5.0;
        if hi - lo < 10.0 {
            hi = lo + 10.0;
        }
    }
    if let Some(cap) = spec.cap {
        hi = hi.min(cap);
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    (lo, hi)
}

fn n(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").unwrap_or(&s).to_string()
}

/// Compute the geometry. `None` when there is nothing finite to draw in the window.
#[allow(dead_code)] // public API; the pages use `plan_ex` through `line_chart_ex`
pub fn plan(spec: &ChartSpec, series: &Series) -> Option<Plot> {
    plan_ex(spec, series, &[])
}

/// Finite points of `series` inside the chart window.
fn window_points(spec: &ChartSpec, series: &Series) -> Vec<(f64, f64)> {
    let t0 = (spec.now - spec.range_secs) as f64;
    series
        .points
        .iter()
        .filter(|(t, v)| v.is_finite() && *t as f64 >= t0 && *t <= spec.now)
        .map(|&(t, v)| (t as f64, v))
        .collect()
}

/// Like `plan`, with the y axis also covering `others` (a max band, a
/// comparison line), so every layer shares one scale.
pub fn plan_ex(spec: &ChartSpec, series: &Series, others: &[&Series]) -> Option<Plot> {
    let pts = window_points(spec, series);
    if pts.is_empty() {
        return None;
    }
    let min = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (mut lo, mut hi) = (min, max);
    for o in others {
        for p in window_points(spec, o) {
            lo = lo.min(p.1);
            hi = hi.max(p.1);
        }
    }
    let (y_lo, y_hi) = y_range(spec, lo, hi);
    let (line, area, dots) = paths(spec, series.step, &pts, y_lo, y_hi);
    Some(Plot {
        line,
        area,
        dots,
        y_lo,
        y_hi,
        last: pts[pts.len() - 1].1,
        min,
        max,
        n: pts.len(),
    })
}

/// Line, area and lone-point paths for `pts` on the scale `y_lo..y_hi`.
fn paths(
    spec: &ChartSpec,
    series_step: i64,
    pts: &[(f64, f64)],
    y_lo: f64,
    y_hi: f64,
) -> (String, String, String) {
    let range = spec.range_secs.max(1) as f64;
    let t0 = (spec.now - spec.range_secs) as f64;
    // A gap is more than 2.5 steps between neighbours.
    let step = effective_step(series_step, pts);
    let gap = step * 2.5;
    let x = |t: f64| ((t - t0) / range * VB_W).clamp(0.0, VB_W);
    let y = |v: f64| (VB_H - (v - y_lo) / (y_hi - y_lo) * VB_H).clamp(0.0, VB_H);

    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut prev_t = f64::NEG_INFINITY;
    for &(t, v) in pts {
        if runs.is_empty() || t - prev_t > gap {
            runs.push(Vec::new());
        }
        if let Some(r) = runs.last_mut() {
            r.push((x(t), y(v)));
        }
        prev_t = t;
    }
    let (mut line, mut area, mut dots) = (String::new(), String::new(), String::new());
    for r in &runs {
        if r.len() == 1 {
            dots.push_str(&format!("M{} {}h0", n(r[0].0), n(r[0].1)));
            continue;
        }
        for (i, &(px, py)) in r.iter().enumerate() {
            line.push_str(&format!(
                "{}{} {}",
                if i == 0 { "M" } else { "L" },
                n(px),
                n(py)
            ));
        }
        area.push_str(&format!("M{} {}", n(r[0].0), n(VB_H)));
        for &(px, py) in r {
            area.push_str(&format!("L{} {}", n(px), n(py)));
        }
        area.push_str(&format!("L{} {}Z", n(r[r.len() - 1].0), n(VB_H)));
    }
    (line, area, dots)
}

/// A point in time worth marking on a chart (a crash, an alert).
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The event's id (its timeline row is `#ev-<id>` on History).
    pub id: i64,
    pub ts: i64,
    /// Tooltip, accessible name and the read-out's text under the
    /// crosshair ("web: die, exit code 1").
    pub label: String,
    /// Where a click goes: the event's row in the History timeline.
    pub href: Option<String>,
    /// `bad`, `warn` or empty.
    pub level: &'static str,
}

/// Markers for the events worth seeing on a time chart: crashes, OOM kills,
/// failing health checks and fired alerts. Clean exits and plain starts are
/// left out (they are routine). A marker opens its event (LG-44): the row
/// in the History timeline, highlighted, in an hour around it (History
/// itself points them at its own window, `pages::history`).
pub fn markers_from_events(evs: &[crate::db::Event]) -> Vec<Marker> {
    evs.iter()
        .filter_map(|e| {
            let level = match e.kind.as_str() {
                "container.die" if e.level != "info" => "bad",
                "container.oom" => "bad",
                "container.health" if e.detail == "unhealthy" => "warn",
                k if k.starts_with("alert.fired") => "bad",
                _ => return None,
            };
            let all = if e.level == "info" { "&events=all" } else { "" };
            let href = format!(
                "/history?from={}&to={}{all}&before={}#ev-{}",
                e.ts - 1800,
                e.ts + 1800,
                e.id + 1,
                e.id
            );
            let what = e.kind.rsplit('.').next().unwrap_or(&e.kind);
            let label = if e.detail.is_empty() {
                format!("{}: {what}", e.subject)
            } else {
                format!(
                    "{}: {what}, {}",
                    e.subject,
                    crate::ui::pages::alerts::without_echo(&e.detail)
                )
            };
            Some(Marker {
                id: e.id,
                ts: e.ts,
                label,
                href: Some(href),
                level,
            })
        })
        .collect()
}

/// Optional layers and behaviour for `line_chart_ex`.
#[derive(Debug, Clone, Default)]
pub struct Extras<'a> {
    /// Bucket maxima (rollups): drawn as a faint band above the average.
    pub max: Option<&'a Series>,
    /// The same window one day earlier, already shifted onto this window.
    pub compare: Option<&'a Series>,
    /// Event markers along the top edge (clickable).
    pub markers: &'a [Marker],
    /// Dragging across the plot zooms (the page reads `from`/`to`).
    pub zoom: bool,
}

/// Markers inside the window as `(left %, marker)`, with markers closer than
/// 1.5% of the width folded into one ("4 events: web: die, …"), worst level
/// first, so a crash loop is one mark, not a fence.
fn cluster_markers(spec: &ChartSpec, ms: &[Marker]) -> Vec<(f64, Marker)> {
    let t0 = spec.now - spec.range_secs;
    let mut v: Vec<(f64, &Marker)> = ms
        .iter()
        .filter(|m| m.ts >= t0 && m.ts <= spec.now)
        .map(|m| {
            (
                (m.ts - t0) as f64 / spec.range_secs.max(1) as f64 * 100.0,
                m,
            )
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, Vec<&Marker>)> = Vec::new();
    for (x, m) in v {
        match out.last_mut() {
            Some((x0, g)) if x - *x0 < 1.5 => g.push(m),
            _ => out.push((x, vec![m])),
        }
    }
    out.into_iter()
        .map(|(x, g)| {
            if g.len() == 1 {
                return (x, g[0].clone());
            }
            let worst = g.iter().find(|m| m.level == "bad").unwrap_or(&g[0]);
            let mut label = format!("{} events: ", g.len());
            label.push_str(
                &g.iter()
                    .take(4)
                    .map(|m| m.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            if g.len() > 4 {
                label.push_str("; …");
            }
            // The worst event's row; the others sit next to it in the
            // timeline (LG-44).
            (
                x,
                Marker {
                    id: worst.id,
                    ts: worst.ts,
                    label,
                    href: worst.href.clone(),
                    level: worst.level,
                },
            )
        })
        .collect()
}

/// Shift a series by `secs` (the "same time yesterday" line: query the
/// window ending a day earlier, then shift it by a day).
pub fn shifted(s: &Series, secs: i64) -> Series {
    Series {
        step: s.step,
        points: s.points.iter().map(|&(t, v)| (t + secs, v)).collect(),
    }
}

/// The area between the bucket maxima and the averages (same buckets).
fn band_path(spec: &ChartSpec, max: &Series, avg: &Series, y_lo: f64, y_hi: f64) -> Option<String> {
    let hi = window_points(spec, max);
    if hi.len() < 2 {
        return None;
    }
    let lo: std::collections::HashMap<i64, f64> = window_points(spec, avg)
        .into_iter()
        .map(|(t, v)| (t as i64, v))
        .collect();
    let range = spec.range_secs.max(1) as f64;
    let t0 = (spec.now - spec.range_secs) as f64;
    let x = |t: f64| ((t - t0) / range * VB_W).clamp(0.0, VB_W);
    let y = |v: f64| (VB_H - (v - y_lo) / (y_hi - y_lo) * VB_H).clamp(0.0, VB_H);
    let gap = effective_step(max.step, &hi) * 2.5;
    let mut out = String::new();
    let flush = |run: &mut Vec<(f64, f64)>, out: &mut String| {
        if run.len() >= 2 {
            for (i, &(t, v)) in run.iter().enumerate() {
                let c = if i == 0 { "M" } else { "L" };
                out.push_str(&format!("{c}{} {}", n(x(t)), n(y(v))));
            }
            for &(t, v) in run.iter().rev() {
                let a = lo.get(&(t as i64)).copied().unwrap_or(v).min(v);
                out.push_str(&format!("L{} {}", n(x(t)), n(y(a))));
            }
            out.push('Z');
        }
        run.clear();
    };
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut prev = f64::NEG_INFINITY;
    for &(t, v) in &hi {
        if t - prev > gap {
            flush(&mut run, &mut out);
        }
        run.push((t, v));
        prev = t;
    }
    flush(&mut run, &mut out);
    (!out.is_empty()).then_some(out)
}

/// The series' step, or the median spacing when it is missing.
fn effective_step(step: i64, pts: &[(f64, f64)]) -> f64 {
    if step > 0 {
        return step as f64;
    }
    let mut d: Vec<f64> = pts.windows(2).map(|w| w[1].0 - w[0].0).collect();
    if d.is_empty() {
        return 60.0;
    }
    d.sort_by(|a, b| a.total_cmp(b));
    d[d.len() / 2].max(1.0)
}

/// Values per time slot for the hover read-out; empty entries are gaps.
fn slot_values(spec: &ChartSpec, series: &Series, step: i64) -> Option<String> {
    let step = step.max(1);
    let t0 = spec.now - spec.range_secs;
    let slots = spec.range_secs / step + 1;
    if !(2..=MAX_SLOTS).contains(&slots) {
        return None;
    }
    let mut v: Vec<Option<f64>> = vec![None; slots as usize];
    for &(t, val) in &series.points {
        if !val.is_finite() || t < t0 || t > spec.now {
            continue;
        }
        let i = ((t - t0) as f64 / step as f64).round() as usize;
        if let Some(slot) = v.get_mut(i) {
            *slot = Some(val);
        }
    }
    Some(
        v.iter()
            .map(|o| o.map(|x| format!("{x:.3}")).unwrap_or_default())
            .map(|s| s.trim_end_matches('0').trim_end_matches('.').to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// X-axis tick times: `count` evenly spaced labels, formatted for the window.
fn x_ticks(spec: &ChartSpec, count: usize) -> Vec<(f64, i64, String)> {
    (0..count)
        .map(|i| {
            let f = i as f64 / (count - 1) as f64;
            let t = spec.now - spec.range_secs + (f * spec.range_secs as f64) as i64;
            let full = fmt_utc(t.max(0) as u64); // 2026-11-14T14:02:11Z
            let label = if spec.range_secs <= 86_400 {
                full.get(11..16).unwrap_or("").to_string()
            } else {
                format!(
                    "{} {}",
                    full.get(8..10).unwrap_or("").trim_start_matches('0'),
                    month_name(full.get(5..7).unwrap_or("01"))
                )
            };
            (f * 100.0, t, label)
        })
        .collect()
}

fn month_name(m: &str) -> &'static str {
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    M[m.parse::<usize>().unwrap_or(1).clamp(1, 12) - 1]
}

/// The empty state, shown when there are no points or the database is off.
pub fn empty(title: &str, db_enabled: bool, compact: bool) -> Markup {
    html! {
        figure.chart.is-empty.compact[compact] {
            figcaption { span.chart-title { (title) } }
            div.chart-empty {
                svg.ghost viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true" {
                    path d="M0 25H1000M0 50H1000M0 75H1000" {}
                }
                div.chart-msg {
                    @if db_enabled {
                        strong { "Collecting history" }
                        span { " — first points appear within a minute." }
                    } @else {
                        strong { "History is off" }
                        @if compact {
                            span { " — live values still update; charts fill in once the history database is enabled." }
                        }
                    }
                }
            }
        }
    }
}

/// A line-and-area chart with axes, min/max/last and hover data.
pub fn line_chart(spec: &ChartSpec, series: &Series, db_enabled: bool) -> Markup {
    line_chart_ex(spec, series, db_enabled, &Extras::default())
}

/// `line_chart` with extra layers: a max band, a comparison line, event
/// markers, and drag-to-zoom. A legend appears when there is more than one
/// layer; its buttons toggle layers (`static/app.js`).
pub fn line_chart_ex(spec: &ChartSpec, series: &Series, db_enabled: bool, x: &Extras) -> Markup {
    let others: Vec<&Series> = [x.max, x.compare].into_iter().flatten().collect();
    let Some(p) = plan_ex(spec, series, &others) else {
        return empty(spec.title, db_enabled, spec.compact);
    };
    let band = x
        .max
        .and_then(|m| band_path(spec, m, series, p.y_lo, p.y_hi));
    let cmp = x.compare.and_then(|c| {
        let pts = window_points(spec, c);
        (!pts.is_empty()).then(|| paths(spec, c.step, &pts, p.y_lo, p.y_hi))
    });
    let marks = cluster_markers(spec, x.markers);
    let legend = band.is_some() || cmp.is_some();
    let unit = spec.unit;
    let step = if series.step > 0 { series.step } else { 60 };
    let hover = slot_values(spec, series, step);
    let mid = (p.y_lo + p.y_hi) / 2.0;
    let ticks = x_ticks(spec, 5);
    let summary = format!(
        "{}: last {}, min {}, max {}",
        spec.title,
        unit.fmt(p.last),
        unit.fmt(p.min),
        unit.fmt(p.max)
    );
    let (y_hi, y_mid, y_lo) = (unit.axis(p.y_hi), unit.axis(mid), unit.axis(p.y_lo));
    // Data covering only the last part of the window (a young database, a
    // container started minutes ago): say from when (LG-40, OV-43).
    let first = window_points(spec, series).first().map(|q| q.0 as i64);
    let since = first.filter(|&t| t > spec.now - spec.range_secs + spec.range_secs / 5);
    // Y-axis column sized to its widest label (mono font: `ch` is exact), so
    // "20.0 KiB/s" is never clipped and "50%" does not waste space.
    let y_width = [&y_hi, &y_mid, &y_lo]
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(4)
        .max(3);
    html! {
        figure.chart.compact[spec.compact] style={"--s:var(" (spec.color) ");--yw:" (y_width) "ch"}
            data-t0=(spec.now - spec.range_secs) data-t1=(spec.now) data-step=(step)
            data-u=(unit.tag()) data-v=[hover.as_deref()] data-zoom=[x.zoom.then_some("1")] {
            figcaption {
                span.chart-title { (spec.title) }
                // The newest stored point, said as such: next to a live tile
                // it is not "now" (OV-41).
                span.chart-now title="The newest recorded value in this window" { span.chart-lw { "last " } (unit.fmt(p.last)) }
                span.chart-mm title="minimum and maximum in this window" {
                    "min " (unit.fmt(p.min)) " · max " (unit.fmt(p.max))
                }
                @if let Some(t) = since {
                    span.chart-since title="The history database has nothing earlier in this window" {
                        "data from " time datetime=(fmt_utc(t.max(0) as u64)) data-t=(t) data-abs="1" { (crate::ui::fmt::short_utc(t.max(0) as u64)) }
                    }
                }
                @if legend {
                    span.legend {
                        button.lg.lg-avg type="button" data-ser="avg" aria-pressed="true" { i {} "avg" }
                        @if band.is_some() { button.lg.lg-max type="button" data-ser="max" aria-pressed="true" { i {} "max" } }
                        @if cmp.is_some() { button.lg.lg-cmp type="button" data-ser="cmp" aria-pressed="true" { i {} "yesterday" } }
                    }
                }
            }
            div.plot {
                div.yaxis aria-hidden="true" {
                    span style="top:0" { (y_hi) }
                    span style="top:50%" { (y_mid) }
                    span style="top:100%" { (y_lo) }
                }
                div.canvas {
                    svg viewBox={"0 0 " (VB_W) " " (VB_H)} preserveAspectRatio="none" role="img"
                        aria-label=(summary) {
                        path.grid d="M0 0H1000M0 50H1000M0 100H1000" {}
                        @if let Some(b) = &band { path.band d=(b) {} }
                        @if let Some((l, _, d)) = &cmp {
                            @if !l.is_empty() { path.cmp d=(l) {} }
                            @if !d.is_empty() { path.cmp.dots d=(d) {} }
                        }
                        @if !p.area.is_empty() { path.area d=(p.area) {} }
                        @if !p.line.is_empty() { path.line d=(p.line) {} }
                        @if !p.dots.is_empty() { path.dots d=(p.dots) {} }
                    }
                    @if !marks.is_empty() {
                        div.evm {
                            // No `title`: the read-out names the marker under the
                            // crosshair (OV-17, touch too) and a click opens its
                            // row; the label is sent once, not twice per chart.
                            @for (left, m) in &marks {
                                @if let Some(h) = &m.href {
                                    a class={"mk " (m.level)} href=(h) style={"left:" (n(*left)) "%"} data-t=(m.ts) aria-label=(m.label) {}
                                } @else {
                                    span class={"mk " (m.level)} style={"left:" (n(*left)) "%"} data-t=(m.ts) aria-label=(m.label) {}
                                }
                            }
                        }
                    }
                    div.cursor hidden {}
                    div.sel hidden {}
                    div.tip hidden role="status" {}
                }
            }
            div.xaxis aria-hidden="true" {
                @for (i, (left, t, label)) in ticks.iter().enumerate() {
                    span.tick.mid[i % 2 == 1] style={"left:" (n(*left)) "%"} data-t=(t) { (label) }
                }
            }
        }
    }
}

/// The last `n` values of a series as `"1.5,2,3.25"`, for seeding the live
/// sparklines that the script keeps drawing. `None` when there are none.
pub fn seed(series: &Series, n: usize) -> Option<String> {
    let v: Vec<String> = series
        .points
        .iter()
        .rev()
        .filter(|p| p.1.is_finite())
        .take(n)
        .map(|p| trim(p.1))
        .collect();
    if v.is_empty() {
        return None;
    }
    Some(v.into_iter().rev().collect::<Vec<_>>().join(","))
}

/// A live sparkline (stat tiles, vitals chips, table cells). The script keeps
/// a short ring buffer per `key` and redraws it from each new `data-v`;
/// `hist` (comma-separated, oldest first) seeds it from the database, and is
/// drawn here too so the first paint already shows the line.
pub fn live_spark(key: &str, v: Option<f64>, color: &str, hist: Option<String>) -> Markup {
    let seeded: Vec<f64> = hist
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter_map(|x| x.parse::<f64>().ok())
        .filter(|x| x.is_finite())
        .collect();
    let d = sparkline_path(&seeded, 100.0, 24.0);
    html! {
        svg.spark viewBox="0 0 100 24" preserveAspectRatio="none" aria-hidden="true"
            style={"--s:var(" (color) ")"}
            data-k=(key) data-v=[v.filter(|v| v.is_finite()).map(|v| format!("{v:.2}"))] data-h=[hist] {
            path.base d="M0 23.5H100" {}
            @if let Some(d) = d {
                path.area d={(d) "L100 24L0 24Z"} {}
                path.line d=(d) {}
            } @else {
                path.area d="" {}
                path.line d="" {}
            }
        }
    }
}

/// Path data for a sparkline in a `w` x `h` box (1 px padding top and
/// bottom), scaled from zero to the maximum so a quiet series stays low
/// instead of filling the box with noise. `None` for fewer than two points.
/// `static/app.js` (`sparkPath`) draws the same way.
pub fn sparkline_path(values: &[f64], w: f64, h: f64) -> Option<String> {
    if values.len() < 2 {
        return None;
    }
    let min = values
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = if max - min < 1e-9 { 1.0 } else { max - min };
    let last = (values.len() - 1) as f64;
    let mut d = String::new();
    for (i, v) in values.iter().enumerate() {
        let px = i as f64 / last * w;
        let py = h - 1.0 - (v - min) / span * (h - 2.0);
        d.push_str(&format!(
            "{}{} {}",
            if i == 0 { "M" } else { "L" },
            n(px),
            n(py)
        ));
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn spec(unit: Unit) -> ChartSpec<'static> {
        ChartSpec {
            title: "CPU",
            unit,
            color: "--ch-cpu",
            range_secs: 3600,
            now: NOW,
            cap: None,
            zero_based: true,
            compact: false,
        }
    }

    fn series(step: i64, pts: &[(i64, f64)]) -> Series {
        Series {
            step,
            points: pts.to_vec(),
        }
    }

    #[test]
    fn scaling_maps_window_to_viewbox() {
        let pts: Vec<(i64, f64)> = (0..=60)
            .map(|i| (NOW - 3600 + i * 60, i as f64 * 100.0 / 60.0))
            .collect();
        let s = series(60, &pts);
        let mut sp = spec(Unit::Percent);
        sp.cap = Some(100.0);
        let p = plan(&sp, &s).unwrap();
        assert_eq!((p.y_lo, p.y_hi), (0.0, 100.0));
        // x spans the whole window; y 0 is the bottom, 100 the top.
        assert!(p.line.starts_with("M0 100"), "{}", p.line);
        assert!(p.line.contains("L500 50"), "{}", p.line);
        assert!(p.line.ends_with("L1000 0"), "{}", p.line);
        assert_eq!((p.min, p.max, p.last, p.n), (0.0, 100.0, 100.0, 61));
        assert!(p.dots.is_empty());
    }

    #[test]
    fn y_axis_is_nice_and_capped() {
        let s = series(60, &[(NOW - 60, 3.0), (NOW, 7.0)]);
        let p = plan(&spec(Unit::Percent), &s).unwrap();
        assert_eq!(p.y_hi, 8.0, "7 * 1.1 rounds up to 8");
        let s = series(60, &[(NOW - 60, 99.0), (NOW, 100.0)]);
        let mut sp = spec(Unit::Percent);
        sp.cap = Some(100.0);
        assert_eq!(plan(&sp, &s).unwrap().y_hi, 100.0);
        // Bytes use powers of 1024: 1.2 GiB tops out at 2 GiB, not 2e9.
        let gib = 1024.0 * 1024.0 * 1024.0;
        let s = series(60, &[(NOW - 60, 1.2 * gib), (NOW, 1.2 * gib)]);
        assert_eq!(plan(&spec(Unit::Bytes), &s).unwrap().y_hi, 2.0 * gib);
        // Temperature is not zero based.
        let s = series(60, &[(NOW - 60, 52.0), (NOW, 58.0)]);
        let mut sp = spec(Unit::Celsius);
        sp.zero_based = false;
        let p = plan(&sp, &s).unwrap();
        assert_eq!((p.y_lo, p.y_hi), (45.0, 65.0));
    }

    #[test]
    fn gaps_break_the_line() {
        // 60 s step; a 10-minute hole in the middle.
        let s = series(
            60,
            &[
                (NOW - 1800, 10.0),
                (NOW - 1740, 12.0),
                (NOW - 1680, 11.0),
                (NOW - 600, 20.0),
                (NOW - 540, 22.0),
            ],
        );
        let p = plan(&spec(Unit::Percent), &s).unwrap();
        assert_eq!(p.line.matches('M').count(), 2, "two runs: {}", p.line);
        assert_eq!(p.area.matches('M').count(), 2);
        // The segment between the runs is never drawn.
        let xs: Vec<f64> = p
            .line
            .split(['M', 'L'])
            .filter(|s| !s.is_empty())
            .map(|s| s.split(' ').next().unwrap().parse().unwrap())
            .collect();
        let before = (3600.0 - 1680.0) / 3600.0 * 1000.0;
        let after = (3600.0 - 600.0) / 3600.0 * 1000.0;
        assert!(
            !xs.iter().any(|&x| x > before + 1.0 && x < after - 1.0),
            "{xs:?}"
        );
    }

    #[test]
    fn a_lone_point_is_a_dot() {
        let s = series(60, &[(NOW - 100, 5.0)]);
        let p = plan(&spec(Unit::Percent), &s).unwrap();
        assert!(p.line.is_empty() && p.area.is_empty());
        assert!(
            p.dots.starts_with('M') && p.dots.ends_with("h0"),
            "{}",
            p.dots
        );
        let html = line_chart(&spec(Unit::Percent), &s, true).into_string();
        assert!(html.contains("path class=\"dots\""));
        assert!(html.contains("5.0%"));
    }

    #[test]
    fn two_isolated_points_are_two_dots() {
        let s = series(60, &[(NOW - 3000, 5.0), (NOW - 100, 6.0)]);
        let p = plan(&spec(Unit::Percent), &s).unwrap();
        assert_eq!(p.dots.matches('M').count(), 2);
        assert!(p.line.is_empty());
    }

    #[test]
    fn empty_and_disabled_show_the_message() {
        for db in [true, false] {
            let html = line_chart(&spec(Unit::Percent), &Series::default(), db).into_string();
            assert_eq!(html.contains("Collecting history"), db, "{html}");
            assert_eq!(html.contains("History is off"), !db, "{html}");
            assert!(!html.contains("class=\"line\""));
        }
        // Points outside the window do not count.
        let s = series(60, &[(NOW - 99_999, 1.0), (NOW + 500, 2.0)]);
        assert!(plan(&spec(Unit::Percent), &s).is_none());
    }

    #[test]
    fn non_finite_values_are_dropped() {
        let s = series(
            60,
            &[
                (NOW - 180, 1.0),
                (NOW - 120, f64::NAN),
                (NOW - 60, f64::INFINITY),
                (NOW, 3.0),
            ],
        );
        let p = plan(&spec(Unit::Percent), &s).unwrap();
        assert_eq!(p.n, 2);
        assert!(p.min.is_finite() && p.max.is_finite() && p.last == 3.0);
        assert!(!p.line.contains("NaN") && !p.line.contains("inf"));
        let all_bad = series(60, &[(NOW, f64::NAN), (NOW - 60, f64::NEG_INFINITY)]);
        assert!(plan(&spec(Unit::Percent), &all_bad).is_none());
        // Flat data must not divide by zero.
        let flat = series(60, &[(NOW - 60, 0.0), (NOW, 0.0)]);
        let p = plan(&spec(Unit::Bytes), &flat).unwrap();
        assert!(p.y_hi > p.y_lo);
        assert!(!p.line.contains("NaN"));
    }

    #[test]
    fn markup_has_axes_hover_data_and_summary() {
        let s = series(60, &[(NOW - 120, 10.0), (NOW - 60, 30.0), (NOW, 20.0)]);
        let html = line_chart(&spec(Unit::Percent), &s, true).into_string();
        assert!(html.contains("data-u=\"pct\""));
        assert!(html.contains("data-step=\"60\""));
        assert!(
            html.contains("aria-label=\"CPU: last 20%, min 10%, max 30%\""),
            "{html}"
        );
        // OV-41: the header value is the newest stored point, labelled so.
        assert!(
            html.contains(r#"<span class="chart-lw">last </span>20%</span>"#),
            "{html}"
        );
        // 61 slots, the last three filled.
        let v = html
            .split("data-v=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let slots: Vec<&str> = v.split(',').collect();
        assert_eq!(slots.len(), 61);
        assert_eq!(&slots[58..], &["10", "30", "20"]);
        assert!(slots[..58].iter().all(|s| s.is_empty()));
        assert!(html.contains("class=\"yaxis\""));
        assert!(html.contains("min 10% · max 30%"));
        // The y column's width is on the figure, for the x axis too (OV-15).
        assert!(
            html.contains(r#"style="--s:var(--ch-cpu);--yw:3ch""#),
            "{html}"
        );
        // Data from the last 2 minutes of an hour: says from when (LG-40).
        assert!(html.contains("data from <time"), "{html}");
        let full = series(60, &[(NOW - 3500, 1.0), (NOW, 2.0)]);
        assert!(!line_chart(&spec(Unit::Percent), &full, true)
            .into_string()
            .contains("data from"));
        assert_eq!(html.matches("class=\"tick").count(), 5);
        // Title text is escaped.
        let mut sp = spec(Unit::Percent);
        sp.title = "<script>x</script>";
        let html = line_chart(&sp, &s, true).into_string();
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn unit_formatting() {
        assert_eq!(Unit::Percent.fmt(0.44), "0.4%");
        assert_eq!(Unit::Percent.fmt(45.6), "46%");
        assert_eq!(Unit::Bytes.fmt(1536.0), "1.5 KiB");
        assert_eq!(Unit::BytesPerSec.fmt(2048.0), "2.0 KiB/s");
        assert_eq!(Unit::Celsius.fmt(61.2), "61 °C");
        assert_eq!(Unit::Plain.fmt(f64::NAN), "—");
        assert_eq!(Unit::Plain.fmt(3.0), "3");
        assert_eq!(Unit::Plain.fmt(1.25), "1.25");
        assert_eq!(Unit::Percent.axis(0.0), "0");
        assert_eq!(Unit::Percent.axis(2.5), "2.5%");
        assert_eq!(Unit::Percent.axis(50.0), "50%");
    }

    #[test]
    fn sparkline_paths() {
        assert_eq!(sparkline_path(&[], 100.0, 24.0), None);
        assert_eq!(sparkline_path(&[1.0], 100.0, 24.0), None);
        let d = sparkline_path(&[0.0, 10.0, 5.0], 100.0, 24.0).unwrap();
        assert_eq!(d, "M0 23L50 1L100 12");
        // Flat series: no division by zero.
        let d = sparkline_path(&[4.0, 4.0], 100.0, 24.0).unwrap();
        assert!(!d.contains("NaN"), "{d}");
        // Zero based: a flat 4 sits at the top, not in the middle.
        let d = sparkline_path(&[2.0, 4.0], 100.0, 24.0).unwrap();
        assert_eq!(d, "M0 12L100 1");
        let html = live_spark("s:cpu", Some(3.0), "--ch-cpu", Some("1,x,3,2".into())).into_string();
        assert!(
            html.contains("class=\"line\"") && !html.contains("NaN"),
            "{html}"
        );
        assert!(
            html.contains(r#"data-k="s:cpu" data-v="3.00" data-h="1,x,3,2""#),
            "{html}"
        );
        let html = live_spark("s:cpu", None, "--ch-cpu", None).into_string();
        assert!(
            html.contains(r#"class="base""#) && !html.contains("data-v"),
            "{html}"
        );
    }

    /// Not a check: prints sample charts for a visual review in a browser
    /// (`cargo test demo_charts -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn demo_charts() {
        let pts: Vec<(i64, f64)> = (0..=360)
            .filter(|i| !(150..175).contains(i))
            .map(|i| {
                let x = i as f64 / 20.0;
                (
                    NOW - 6 * 3600 + i * 60,
                    22.0 + 14.0 * x.sin() + 6.0 * (x * 3.1).cos().abs(),
                )
            })
            .collect();
        let s = series(60, &pts);
        let mut sp = spec(Unit::Percent);
        sp.range_secs = 6 * 3600;
        sp.cap = Some(100.0);
        println!("{}", line_chart(&sp, &s, true).into_string());
        let mem = Series {
            step: 60,
            points: pts
                .iter()
                .map(|&(t, v)| (t, v * 40.0 * 1024.0 * 1024.0))
                .collect(),
        };
        let mut sp = spec(Unit::Bytes);
        sp.title = "Memory";
        sp.color = "--ch-mem";
        sp.range_secs = 6 * 3600;
        println!("{}", line_chart(&sp, &mem, true).into_string());
        sp.compact = true;
        sp.title = "Memory · 1 h";
        println!("{}", line_chart(&sp, &mem, true).into_string());
    }

    #[test]
    fn extras_band_compare_markers_and_legend() {
        let avg = series(60, &[(NOW - 120, 10.0), (NOW - 60, 20.0), (NOW, 15.0)]);
        let max = series(60, &[(NOW - 120, 30.0), (NOW - 60, 60.0), (NOW, 25.0)]);
        let y = shifted(
            &series(60, &[(NOW - 86_460, 90.0), (NOW - 86_400, 80.0)]),
            86_400,
        );
        let marks = vec![Marker {
            id: 3,
            ts: NOW - 60,
            label: "web: die, exit code 1".into(),
            href: Some("/containers/web".into()),
            level: "bad",
        }];
        let x = Extras {
            max: Some(&max),
            compare: Some(&y),
            markers: &marks,
            zoom: true,
        };
        let html = line_chart_ex(&spec(Unit::Percent), &avg, true, &x).into_string();
        assert!(html.contains("class=\"band\""), "{html}");
        assert!(html.contains("class=\"cmp\""), "{html}");
        assert!(html.contains("data-ser=\"max\"") && html.contains("data-ser=\"cmp\""));
        assert!(
            html.contains(
                r#"class="mk bad" href="/containers/web" style="left:98.3%" data-t="1799999940""#
            ),
            "{html}"
        );
        assert!(html.contains("data-zoom=\"1\""));
        // The y axis covers the comparison line (90 -> 100).
        assert!(html.contains(">100%<"), "{html}");
        // The summary still describes the main series only.
        assert!(html.contains("max 20%"), "{html}");
        let plain = line_chart(&spec(Unit::Percent), &avg, true).into_string();
        assert!(!plain.contains("legend") && !plain.contains("data-zoom"));
        assert!(!plain.contains("class=\"evm\""));
        let ev = |kind: &str, level: &str, detail: &str| crate::db::Event {
            id: 1,
            ts: NOW,
            kind: kind.into(),
            level: level.into(),
            subject: "a<b".into(),
            detail: detail.into(),
        };
        let m = markers_from_events(&[
            ev("container.die", "warn", "exit code 1"),
            ev("container.die", "info", "exit code 0"),
            ev("container.start", "info", ""),
            ev("alert.fired", "error", "cpu"),
        ]);
        assert_eq!(m.len(), 2);
        // Each opens its row in the History timeline, an hour around it
        // (LG-44), not the subject's page or the top of a list.
        let row = format!(
            "/history?from={}&to={}&before=2#ev-1",
            NOW - 1800,
            NOW + 1800
        );
        assert_eq!(m[0].href.as_deref(), Some(row.as_str()));
        assert_eq!(m[1].href.as_deref(), Some(row.as_str()));
        // Folded markers open the worst event's row, never a bare list.
        let evs: Vec<crate::db::Event> = (0..3)
            .map(|i| crate::db::Event {
                id: 10 + i,
                ts: NOW - 100 + i,
                kind: if i == 1 {
                    "container.oom"
                } else {
                    "container.health"
                }
                .into(),
                level: "warn".into(),
                subject: format!("c{i}"),
                detail: "unhealthy".into(),
            })
            .collect();
        let folded = cluster_markers(&spec(Unit::Percent), &markers_from_events(&evs));
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].1.id, 11);
        assert!(folded[0].1.label.starts_with("3 events: "));
        assert!(folded[0].1.href.as_deref().unwrap().ends_with("#ev-11"));
    }

    #[test]
    fn huge_ranges_skip_hover_data() {
        let mut sp = spec(Unit::Percent);
        sp.range_secs = 30 * 86_400;
        let s = series(1, &[(NOW - 10, 1.0), (NOW, 2.0)]);
        let html = line_chart(&sp, &s, true).into_string();
        assert!(!html.contains("data-v="));
    }
}
