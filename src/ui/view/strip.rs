//! The health strip: a thin bar of the last N hours, one colour per time
//! bucket (green running, amber restarted, red problem, grey not running,
//! hatched no data). The signature element of the v3 design (DESIGN §10).
//!
//! Inline SVG, stretched to its box (`preserveAspectRatio="none"`). Runs of
//! equal cells are one `<rect>` with one `<title>`, so a steady container is
//! a single element, not 96; `cells` mode draws every bucket with a hairline
//! gap (Overview and stack headers, where the strip is wide). Accessible: the
//! SVG has `role="img"` and an `aria-label` that sums the window up in words.

use maud::{html, Markup};

use crate::db::strips::{Cell, Strip, Strips};
use crate::util::human_duration;

/// How to draw a strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StripStyle {
    /// Runs merged (table rows, sidebar): smallest markup.
    #[default]
    Runs,
    /// One rect per bucket with gaps (wide strips).
    Cells,
}

/// The window in words: "24 h", "7 d".
pub fn window_label(st: &Strips) -> String {
    let secs = st.to - st.from;
    if secs % 86_400 == 0 && secs >= 2 * 86_400 {
        format!("{} d", secs / 86_400)
    } else {
        format!("{} h", (secs + 1800) / 3600)
    }
}

/// A one-line summary: "running 98% of the last 24 h · 2 problems".
pub fn summary(s: &Strip, st: &Strips) -> String {
    let known = s.cells.iter().filter(|c| **c != Cell::Unknown).count();
    if known == 0 {
        return format!("no history in the last {}", window_label(st));
    }
    let up = s
        .cells
        .iter()
        .filter(|c| matches!(c, Cell::Ok | Cell::Warn | Cell::Bad))
        .count();
    let runs = |want: Cell| {
        let mut n = 0;
        let mut prev = None;
        for c in &s.cells {
            if *c == want && prev != Some(want) {
                n += 1;
            }
            prev = Some(*c);
        }
        n
    };
    let (bad, warn) = (runs(Cell::Bad), runs(Cell::Warn));
    let p = (up * 100 + known / 2) / known;
    // Mostly no data yet (a new instance): the percentage is of the time
    // with data, and says so (OV-03).
    let n = s.cells.len().max(1);
    let mut out = if known * 2 < n {
        let secs = (st.to - st.from).max(0) as u64 * known as u64 / n as u64;
        format!("running {p}% of {} with data", human_duration(secs))
    } else {
        format!("running {p}% of the last {}", window_label(st))
    };
    if bad > 0 {
        out.push_str(&format!(
            " · {bad} problem{}",
            if bad == 1 { "" } else { "s" }
        ));
    }
    if warn > 0 {
        out.push_str(&format!(
            " · {warn} restart{}",
            if warn == 1 { "" } else { "s" }
        ));
    }
    out
}

/// "5h 30m ago", "now".
fn ago(secs: i64) -> String {
    if secs <= 0 {
        "now".into()
    } else {
        format!("{} ago", human_duration(secs as u64))
    }
}

/// Render `s` (one container or a merged stack) on the window of `st`.
/// `label` names the subject in the accessible summary.
pub fn health_strip(s: &Strip, st: &Strips, label: &str, style: StripStyle) -> Markup {
    let n = st.buckets().max(1);
    let aria = format!("{label}: {}", summary(s, st));
    let end_of = |i: usize| st.to - st.from - (i as i64 + 1) * st.step; // seconds before `to`
    let title = |a: usize, b: usize, c: Cell| {
        let why = (a..=b).rev().find_map(|i| s.why.get(&i));
        let span = format!("{} – {}", ago(end_of(a) + st.step), ago(end_of(b)));
        match why {
            Some(w) => format!("{span}: {w}"),
            None => format!("{span}: {}", c.word()),
        }
    };
    html! {
        svg.hstrip viewBox={"0 0 " (n) " 8"} preserveAspectRatio="none" role="img" aria-label=(aria) {
            @if style == StripStyle::Cells {
                @for (i, c) in s.cells.iter().enumerate() {
                    rect class=(c.class()) x=(i) y="0" width="0.82" height="8" {
                        title { (title(i, i, *c)) }
                    }
                }
            } @else {
                @for (a, b, c) in runs(&s.cells) {
                    rect class=(c.class()) x=(a) y="0" width=(b + 1 - a) height="8" {
                        title { (title(a, b, c)) }
                    }
                }
            }
        }
    }
}

/// Runs of equal cells as `(first, last, cell)`.
fn runs(cells: &[Cell]) -> Vec<(usize, usize, Cell)> {
    let mut out: Vec<(usize, usize, Cell)> = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        match out.last_mut() {
            Some(r) if r.2 == *c => r.1 = i,
            _ => out.push((i, i, *c)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn strips(n: usize) -> Strips {
        Strips {
            from: 0,
            to: n as i64 * 900,
            step: 900,
            sampled: vec![true; n],
            ..Default::default()
        }
    }

    #[test]
    fn runs_merge_and_summary() {
        let st = strips(96);
        let mut s = Strip {
            cells: vec![Cell::Ok; 96],
            why: BTreeMap::new(),
        };
        s.cells[10] = Cell::Bad;
        s.cells[11] = Cell::Bad;
        s.cells[50] = Cell::Warn;
        s.why.insert(11, "crashed (exit code 1)".into());
        let html = health_strip(&s, &st, "web<1>", StripStyle::Runs).into_string();
        assert_eq!(html.matches("<rect").count(), 5, "{html}");
        assert!(html.contains("crashed (exit code 1)"));
        assert!(
            html.contains("aria-label=\"web&lt;1&gt;: running 100% of the last 24 h · 1 problem · 1 restart\""),
            "{html}"
        );
        let cells = health_strip(&s, &st, "x", StripStyle::Cells).into_string();
        assert_eq!(cells.matches("<rect").count(), 96);
    }

    /// OV-03: two cells of data in 96 is "of 30m with data", not "of 24 h".
    #[test]
    fn mostly_empty_history_says_how_much() {
        let st = strips(96);
        let mut s = Strip {
            cells: vec![Cell::Unknown; 96],
            why: BTreeMap::new(),
        };
        s.cells[94] = Cell::Ok;
        s.cells[95] = Cell::Ok;
        assert_eq!(summary(&s, &st), "running 100% of 30m with data");
    }

    #[test]
    fn empty_history_says_so() {
        let st = Strips {
            sampled: vec![false; 4],
            ..strips(4)
        };
        let s = st.get("nobody");
        assert_eq!(summary(&s, &st), "no history in the last 1 h");
        let html = health_strip(&s, &st, "n", StripStyle::Runs).into_string();
        assert!(html.contains("class=\"u\""));
        assert_eq!(window_label(&strips(7 * 12)), "21 h");
        let week = Strips {
            to: 7 * 86_400,
            ..strips(84)
        };
        assert_eq!(window_label(&week), "7 d");
    }
}
