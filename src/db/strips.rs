//! Health strips: per container, N time buckets over a window, each with a
//! status (ok / warn / bad / off / unknown), derived from the samples (a
//! sample means "running") and the lifecycle events (die, oom, health).
//!
//! One statement reads sample presence for every container (an indexed seek
//! per container name on the `(cid, ts)` / `(cid, res, ts)` primary keys,
//! never a table scan), one walks the newest events backwards until the
//! window starts. The result is cached for `TTL` seconds per window, so pages
//! and the live sidebar share one computation.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// The status of one bucket. Ordered by severity (worst last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cell {
    /// No samples for anything: the app (or history) was not running.
    Unknown,
    /// History was recording, this container was not running.
    Off,
    Ok,
    /// Restarted or exited cleanly without a stop.
    Warn,
    /// Crashed (non-zero exit), killed for memory, or unhealthy.
    Bad,
}

impl Cell {
    pub fn class(self) -> &'static str {
        match self {
            Cell::Unknown => "u",
            Cell::Off => "f",
            Cell::Ok => "o",
            Cell::Warn => "w",
            Cell::Bad => "b",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Cell::Unknown => "no data",
            Cell::Off => "not running",
            Cell::Ok => "running",
            Cell::Warn => "restarted",
            Cell::Bad => "problem",
        }
    }
}

/// One container's strip.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Strip {
    pub cells: Vec<Cell>,
    /// Bucket index -> what happened (for warn / bad cells), newest wins.
    pub why: BTreeMap<usize, String>,
}

impl Strip {
    /// Worst-of merge for a stack: bad > warn > ok > off > unknown, except
    /// that one running member is enough for "ok" (a stopped sidecar does
    /// not make a whole stack look down).
    pub fn merge<'a>(n: usize, parts: impl Iterator<Item = &'a Strip>) -> Strip {
        let mut out = Strip {
            cells: vec![Cell::Unknown; n],
            why: BTreeMap::new(),
        };
        for p in parts {
            for (i, c) in p.cells.iter().enumerate().take(n) {
                if *c > out.cells[i] {
                    out.cells[i] = *c;
                }
            }
            for (i, w) in &p.why {
                if *i < n {
                    out.why.entry(*i).or_insert_with(|| w.clone());
                }
            }
        }
        out
    }
}

/// Strips for every container with history in the window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Strips {
    /// Window `[from, to)`, `to` aligned to `step`.
    pub from: i64,
    pub to: i64,
    pub step: i64,
    pub by_name: HashMap<String, Strip>,
    /// Buckets in which anything at all was sampled.
    pub sampled: Vec<bool>,
    /// How long the computation took (microseconds), for the measurements.
    pub cost_us: u64,
}

impl Strips {
    pub fn buckets(&self) -> usize {
        self.sampled.len()
    }
    /// The strip for `name`; a container with no history gets `Off` where
    /// the host was sampling and `Unknown` elsewhere.
    pub fn get(&self, name: &str) -> Strip {
        self.by_name.get(name).cloned().unwrap_or_else(|| Strip {
            cells: self
                .sampled
                .iter()
                .map(|&s| if s { Cell::Off } else { Cell::Unknown })
                .collect(),
            why: BTreeMap::new(),
        })
    }
}

const TTL: i64 = 30;

type Key = (usize, i64, usize, i64);
static CACHE: Mutex<Option<(Key, i64, Arc<Strips>)>> = Mutex::new(None);

/// The newest strips computed by any page (for live fragments that have no
/// database handle, such as the sidebar tree).
pub fn last() -> Option<Arc<Strips>> {
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|c| c.2.clone())
}

impl super::Db {
    /// Cached `health_strips_uncached`: at most one computation per window
    /// every `TTL` seconds.
    pub fn health_strips(&self, now: i64, buckets: usize, step: i64) -> Arc<Strips> {
        let to = (now.div_euclid(step) + 1) * step;
        let key = (self as *const _ as usize, to, buckets, step);
        {
            let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((k, at, s)) = g.as_ref() {
                if *k == key && now - at < TTL {
                    return s.clone();
                }
            }
        }
        let s = Arc::new(self.health_strips_uncached(now, buckets, step));
        *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, now, s.clone()));
        s
    }

    /// `buckets` cells of `step` seconds ending with the bucket that holds
    /// `now`.
    pub fn health_strips_uncached(&self, now: i64, buckets: usize, step: i64) -> Strips {
        let started = std::time::Instant::now();
        let step = step.max(1);
        let n = buckets.clamp(1, 2000);
        let to = (now.div_euclid(step) + 1) * step;
        let from = to - n as i64 * step;
        let mut out = Strips {
            from,
            to,
            step,
            by_name: HashMap::new(),
            sampled: vec![false; n],
            cost_us: 0,
        };
        self.with((), |i| {
            // 1. Presence: which (container, bucket) pairs have a sample.
            // Before the 10-minute watermark read the 10-minute rollup (one
            // row per 10 min), after it the raw samples (at most ~1-2 h).
            // CROSS JOIN keeps `ctr_names` as the outer loop, so each side is
            // a primary-key range seek per container, never a table scan.
            let mut st = i.conn.prepare(
                "SELECT DISTINCT n.name, (s.ts - ?1) / ?3 FROM ctr_names n
                   CROSS JOIN ctr_roll s ON s.cid = n.id AND s.res = 600 AND s.ts >= ?1 AND s.ts < ?4
                 UNION
                 SELECT DISTINCT n.name, (s.ts - ?1) / ?3 FROM ctr_names n
                   CROSS JOIN ctr_samples s ON s.cid = n.id AND s.ts >= max(?1, ?4) AND s.ts < ?2",
            )?;
            let w = i.w600.min(to);
            let rows = st.query_map([from, to, step, w], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (name, b) = row?;
                let Ok(b) = usize::try_from(b) else { continue };
                if b >= n {
                    continue;
                }
                out.sampled[b] = true;
                let s = out.by_name.entry(name).or_insert_with(|| Strip {
                    cells: vec![Cell::Off; n],
                    why: BTreeMap::new(),
                });
                s.cells[b] = Cell::Ok;
            }
            // 2. Events, newest first, until the window starts.
            let mut st = i.conn.prepare(
                "SELECT ts, kind, subject, detail FROM events
                 WHERE kind IN ('container.die', 'container.oom', 'container.health',
                                'container.stop', 'container.start')
                 ORDER BY id DESC",
            )?;
            let mut rows = st.query([])?;
            let mut evs: Vec<(i64, String, String, String)> = Vec::new();
            let mut older = 0;
            while let Some(r) = rows.next()? {
                let ts: i64 = r.get(0)?;
                if ts < from {
                    // Ids follow time closely; a few stragglers are fine.
                    older += 1;
                    if older > 50 {
                        break;
                    }
                    continue;
                }
                if ts < to {
                    evs.push((ts, r.get(1)?, r.get(2)?, r.get(3)?));
                }
            }
            evs.sort_by_key(|e| e.0);
            apply_events(&mut out, &evs);
            Ok(())
        });
        // Fill "off" where nothing was sampled at all with "unknown".
        let sampled = out.sampled.clone();
        for s in out.by_name.values_mut() {
            for (c, any) in s.cells.iter_mut().zip(&sampled) {
                if *c == Cell::Off && !any {
                    *c = Cell::Unknown;
                }
            }
        }
        out.cost_us = started.elapsed().as_micros() as u64;
        out
    }

    /// The newest host CPU sample at or after `since`: seeds the CPU tile and
    /// chip on first paint, before the live sampler has two readings.
    pub fn latest_cpu(&self, since: i64) -> Option<f64> {
        self.with(None, |i| {
            use rusqlite::OptionalExtension;
            i.conn
                .prepare(
                    "SELECT cpu FROM sys_samples WHERE ts >= ?1 AND cpu IS NOT NULL
                     ORDER BY ts DESC LIMIT 1",
                )?
                .query_row([since], |r| r.get::<_, i64>(0))
                .optional()
                .map(|v| v.map(|v| v as f64 / 100.0))
        })
    }
}

/// Mark warn / bad buckets from lifecycle events (sorted by time).
fn apply_events(out: &mut Strips, evs: &[(i64, String, String, String)]) {
    let n = out.sampled.len();
    let (from, step) = (out.from, out.step);
    let idx = |ts: i64| ((ts - from) / step) as usize;
    // Last `container.stop` per name: a non-zero exit right after a stop
    // request (SIGTERM 143, SIGKILL 137) is a deliberate stop, not a crash.
    let mut stopped_at: HashMap<&str, i64> = HashMap::new();
    let mut unhealthy_since: HashMap<&str, usize> = HashMap::new();
    let set = |out: &mut Strips, name: &str, b: usize, c: Cell, why: String| {
        if b >= n {
            return;
        }
        let s = out
            .by_name
            .entry(name.to_string())
            .or_insert_with(|| Strip {
                cells: vec![Cell::Off; n],
                why: BTreeMap::new(),
            });
        if c >= s.cells[b] || s.cells[b] == Cell::Ok {
            s.cells[b] = s.cells[b].max(c);
            s.why.insert(b, why);
        }
    };
    for (ts, kind, name, detail) in evs {
        let b = idx(*ts);
        match kind.as_str() {
            "container.stop" => {
                stopped_at.insert(name, *ts);
            }
            "container.die" => {
                let code = detail.strip_prefix("exit code ").unwrap_or("?");
                let asked = stopped_at.get(name.as_str()).is_some_and(|t| ts - t <= 30);
                if code == "0" || asked {
                    if !asked {
                        set(out, name, b, Cell::Warn, format!("exited ({detail})"));
                    }
                } else {
                    set(out, name, b, Cell::Bad, format!("crashed ({detail})"));
                }
            }
            "container.oom" => set(out, name, b, Cell::Bad, "out of memory".into()),
            "container.start" => {
                // A start while the container ran in the same bucket is a
                // restart; the first start after being off is just a start.
                let ran_before = b > 0
                    && out
                        .by_name
                        .get(name.as_str())
                        .is_some_and(|s| s.cells.get(b - 1).is_some_and(|c| *c != Cell::Off));
                if ran_before && !stopped_at.get(name.as_str()).is_some_and(|t| ts - t <= 120) {
                    set(out, name, b, Cell::Warn, "restarted".into());
                }
            }
            "container.health" => {
                if detail == "unhealthy" {
                    unhealthy_since.insert(name, b);
                } else if let Some(s) = unhealthy_since.remove(name.as_str()) {
                    for k in s..=b.min(n.saturating_sub(1)) {
                        set(out, name, k, Cell::Bad, "unhealthy".into());
                    }
                }
            }
            _ => {}
        }
    }
    // Still unhealthy at the end of the window: bad up to the last sample.
    for (name, s) in unhealthy_since {
        let last = out
            .by_name
            .get(name)
            .and_then(|st| {
                st.cells
                    .iter()
                    .rposition(|c| *c != Cell::Off && *c != Cell::Unknown)
            })
            .unwrap_or(s);
        for k in s..=last.max(s).min(n.saturating_sub(1)) {
            set(out, name, k, Cell::Bad, "unhealthy".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::model::ContainerStats;

    fn stats() -> ContainerStats {
        ContainerStats {
            cpu_percent: 1.0,
            mem_bytes: 1 << 20,
        }
    }

    #[test]
    fn presence_events_and_unknown() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_800_000_899; // last second of a 15-min bucket
        let s = stats();
        // "web" runs in the last 4 buckets; "db" only in the newest.
        for k in 0..4 {
            let ts = now - k * 900;
            db.record_containers(ts, &[("web", &s)]);
        }
        db.record_containers(now, &[("db", &s)]);
        db.record_event(now - 900, "container.die", "warn", "web", "exit code 1");
        db.record_event(now - 1800, "container.oom", "error", "web", "killed");
        db.record_event(now - 10, "container.stop", "info", "db", "");
        db.record_event(now - 5, "container.die", "info", "db", "exit code 143");
        let st = db.health_strips_uncached(now, 8, 900);
        assert_eq!(st.buckets(), 8);
        let web = st.get("web");
        assert_eq!(
            web.cells,
            vec![
                Cell::Unknown,
                Cell::Unknown,
                Cell::Unknown,
                Cell::Unknown,
                Cell::Ok,
                Cell::Bad,
                Cell::Bad,
                Cell::Ok
            ]
        );
        assert_eq!(
            web.why.get(&6).map(String::as_str),
            Some("crashed (exit code 1)")
        );
        // A stop followed by exit 143 is not a crash.
        let db_strip = st.get("db");
        assert_eq!(db_strip.cells[7], Cell::Ok);
        assert_eq!(db_strip.cells[6], Cell::Off);
        // Unknown names: off where sampled, unknown elsewhere.
        let x = st.get("nope");
        assert_eq!(x.cells[0], Cell::Unknown);
        assert_eq!(x.cells[7], Cell::Off);
        // Stack merge: worst of, but one running member is ok.
        let m = Strip::merge(8, [&web, &db_strip].into_iter());
        assert_eq!(m.cells[5], Cell::Bad);
        assert_eq!(m.cells[7], Cell::Ok);
    }

    #[test]
    fn disabled_db_is_empty_and_cache_is_shared() {
        let db = Db::disabled();
        let s = db.health_strips(1_800_000_000, 96, 900);
        assert_eq!(s.buckets(), 96);
        assert!(s.by_name.is_empty());
        assert!(last().is_some());
        assert_eq!(db.latest_cpu(0), None);
    }

    #[test]
    fn unhealthy_spans_until_healthy() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_800_000_899; // 1_800_000_000 is a bucket start
        let s = stats();
        for k in 0..6 {
            db.record_containers(now - k * 900, &[("a", &s)]);
        }
        db.record_event(now - 4 * 900, "container.health", "warn", "a", "unhealthy");
        db.record_event(now - 2 * 900, "container.health", "info", "a", "healthy");
        let st = db.health_strips_uncached(now, 6, 900);
        let a = st.get("a");
        assert_eq!(
            a.cells,
            vec![
                Cell::Ok,
                Cell::Bad,
                Cell::Bad,
                Cell::Bad,
                Cell::Ok,
                Cell::Ok
            ]
        );
    }
}
