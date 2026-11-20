//! Shared display-format fixtures for the client.

use crate::util::human_duration;

pub fn ago(now: u64, ts: u64) -> String {
    let d = now.saturating_sub(ts);
    if d < 10 {
        "just now".into()
    } else {
        format!("{} ago", human_duration(d))
    }
}

pub fn pct(v: f64) -> String {
    if !v.is_finite() {
        "—".into()
    } else if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}
