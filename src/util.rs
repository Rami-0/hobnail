//! Small helpers shared by every module: time, durations, globs, formatting.

use serde::{Deserialize, Deserializer};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format a unix timestamp as `YYYY-MM-DDTHH:MM:SSZ` (UTC). No chrono needed.
pub fn fmt_utc(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Howard Hinnant's days-from-civil inverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parse `500ms`, `30s`, `10m`, `1h`, `2h30m`, `1d`, or a bare number of seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(Duration::from_secs(n));
    }
    let mut total = Duration::ZERO;
    let mut num = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let mut unit = c.to_string();
        if c == 'm' && chars.peek() == Some(&'s') {
            unit.push(chars.next().unwrap());
        }
        let n: f64 = num.parse().map_err(|_| format!("invalid duration {s:?}"))?;
        num.clear();
        let secs = match unit.as_str() {
            "ms" => n / 1000.0,
            "s" => n,
            "m" => n * 60.0,
            "h" => n * 3600.0,
            "d" => n * 86_400.0,
            _ => return Err(format!("invalid duration unit {unit:?} in {s:?}")),
        };
        total += Duration::from_secs_f64(secs);
    }
    if !num.is_empty() {
        return Err(format!("duration {s:?} is missing a unit (s, m, h)"));
    }
    Ok(total)
}

/// A duration in the config: `"30s"`, `"10m"` or an integer number of seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dur(pub Duration);

impl Dur {
    pub const fn secs(n: u64) -> Self {
        Dur(Duration::from_secs(n))
    }
}

impl<'de> Deserialize<'de> for Dur {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            N(u64),
            S(String),
        }
        match Repr::deserialize(d)? {
            Repr::N(n) => Ok(Dur(Duration::from_secs(n))),
            Repr::S(s) => parse_duration(&s)
                .map(Dur)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Glob match with `*` (any run) and `?` (one char). Case-sensitive.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// `3d 4h`, `5h 2m`, `7m`, `12s`.
pub fn human_duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs / 3600) % 24, (secs / 60) % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_format() {
        assert_eq!(fmt_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(fmt_utc(1_794_660_300), "2026-11-14T12:45:00Z");
        assert_eq!(fmt_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("2h30m").unwrap(), Duration::from_secs(9000));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
        assert!(parse_duration("10x").is_err());
        assert!(parse_duration("10m5").is_err());
    }

    #[test]
    fn globs() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("webshop-*-1", "webshop-dev-odoo-1"));
        assert!(!glob_match("webshop-*-1", "webshop-dev-odoo-2"));
        assert!(glob_match("odoo1?", "odoo19"));
        assert!(!glob_match("odoo", "odoo19"));
    }

    #[test]
    fn humans() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(112 * 1024 * 1024), "112 MiB");
        assert_eq!(human_duration(90_000), "1d 1h");
        assert_eq!(human_duration(42), "42s");
    }
}
