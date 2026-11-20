//! History range selection and restart counts.

use std::collections::BTreeMap;

use crate::db::Range;
use crate::http::query::query_get;

const DAYS: usize = 7;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::query::parse_query;
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
}
