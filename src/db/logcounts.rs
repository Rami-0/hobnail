//! Per-minute log line counts by level per container (no log text, ever).
//!
//! Written by the logs background task (`logs::watch`) once a minute, for
//! the containers it tails. Each write adds to the 1-minute row *and* the
//! 10-minute row of that minute, so there is no separate rollup pass;
//! retention follows DESIGN.md section 4 (1 min kept 7 d, 10 min kept 90 d)
//! and is applied by [`Db::prune_log_counts`] (called hourly by the same
//! task).

use super::{params, Db, Range};

/// Schema migration v3 (applied after v1, v2). Never edit it once deployed.
pub(super) const MIGRATION: &str = "
CREATE TABLE log_counts (
  cid INTEGER NOT NULL REFERENCES ctr_names(id),
  res INTEGER NOT NULL,
  ts INTEGER NOT NULL,
  err INTEGER NOT NULL DEFAULT 0,
  warn INTEGER NOT NULL DEFAULT 0,
  info INTEGER NOT NULL DEFAULT 0,
  debug INTEGER NOT NULL DEFAULT 0,
  other INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (cid, res, ts)
) WITHOUT ROWID;
";

/// Lines per level: error, warn, info, debug, other.
pub type LevelCounts = [u64; 5];

/// A level-count series: `(bucket start, counts)`, oldest first, only
/// buckets that have lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelSeries {
    pub step: i64,
    pub points: Vec<(i64, LevelCounts)>,
}

const KEEP_60: i64 = super::ROLL60_KEEP_SECS;
const KEEP_600: i64 = super::ROLL600_KEEP_SECS;
/// Most bars a level chart asks for.
pub const MAX_BUCKETS: i64 = 120;

impl Db {
    /// Add counts for `(container name, minute start, counts)` rows.
    pub fn record_log_counts(&self, rows: &[(String, i64, LevelCounts)]) {
        if rows.is_empty() {
            return;
        }
        self.with((), |i| {
            let super::Inner { conn, names, .. } = i;
            let r = (|| {
                let tx = conn.transaction()?;
                {
                    let mut ins = tx.prepare(
                        "INSERT INTO log_counts (cid, res, ts, err, warn, info, debug, other)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT (cid, res, ts) DO UPDATE SET
                           err = err + excluded.err, warn = warn + excluded.warn,
                           info = info + excluded.info, debug = debug + excluded.debug,
                           other = other + excluded.other",
                    )?;
                    for (name, ts, c) in rows {
                        if name.is_empty() || name.len() > 256 || c.iter().all(|&n| n == 0) {
                            continue;
                        }
                        let cid = match names.get(name.as_str()) {
                            Some(&id) => id,
                            None => {
                                tx.execute(
                                    "INSERT OR IGNORE INTO ctr_names (name) VALUES (?1)",
                                    [name],
                                )?;
                                let id: i64 = tx.query_row(
                                    "SELECT id FROM ctr_names WHERE name = ?1",
                                    [name],
                                    |r| r.get(0),
                                )?;
                                names.insert(name.to_string(), id);
                                id
                            }
                        };
                        let n = |k: usize| c[k].min(i64::MAX as u64) as i64;
                        for res in [60i64, 600] {
                            ins.execute(params![
                                cid,
                                res,
                                ts / res * res,
                                n(0),
                                n(1),
                                n(2),
                                n(3),
                                n(4)
                            ])?;
                        }
                    }
                }
                tx.commit()
            })();
            if r.is_err() {
                names.clear();
            }
            r
        });
    }

    /// Counts for one container over `range`, in at most [`MAX_BUCKETS`]
    /// buckets (1-minute rows up to 7 days back, 10-minute rows beyond).
    pub fn log_level_series(&self, name: &str, range: Range, now: i64) -> LevelSeries {
        let from = now - range.secs();
        let res: i64 = if range.secs() <= KEEP_60 && range.secs() <= 24 * 3600 {
            60
        } else {
            600
        };
        let step = {
            let raw = (range.secs() / MAX_BUCKETS).max(res);
            (raw + res - 1) / res * res
        };
        self.with(LevelSeries::default(), |i| {
            let mut st = i.conn.prepare(
                "SELECT l.ts / ?1 * ?1 AS b, SUM(err), SUM(warn), SUM(info), SUM(debug), SUM(other)
                 FROM log_counts l JOIN ctr_names n ON n.id = l.cid
                 WHERE n.name = ?2 AND l.res = ?3 AND l.ts >= ?4
                 GROUP BY b ORDER BY b",
            )?;
            let points = st
                .query_map(params![step, name, res, from / step * step], |r| {
                    let g = |k: usize| r.get::<_, i64>(k).map(|v| v.max(0) as u64);
                    Ok((r.get::<_, i64>(0)?, [g(1)?, g(2)?, g(3)?, g(4)?, g(5)?]))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(LevelSeries { step, points })
        })
    }

    /// Delete rows past retention. Returns rows deleted.
    pub fn prune_log_counts(&self, now: i64) -> usize {
        self.with(0, |i| {
            let a = i.conn.execute(
                "DELETE FROM log_counts WHERE res = 60 AND ts < ?1",
                [now - KEEP_60],
            )?;
            let b = i.conn.execute(
                "DELETE FROM log_counts WHERE res = 600 AND ts < ?1",
                [now - KEEP_600],
            )?;
            Ok(a + b)
        })
    }

    /// Number of rows (for tests and the data page).
    pub fn log_count_rows(&self) -> u64 {
        self.with(0, |i| {
            i.conn
                .query_row("SELECT COUNT(*) FROM log_counts", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map(|n| n.max(0) as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_rollup_query_prune() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_800_000_000 / 600 * 600;
        db.record_log_counts(&[
            ("web".into(), now - 120, [2, 1, 10, 0, 3]),
            ("web".into(), now - 60, [1, 0, 0, 0, 0]),
            ("db".into(), now - 60, [0, 0, 5, 0, 0]),
            ("empty".into(), now - 60, [0, 0, 0, 0, 0]),
        ]);
        // Same minute again adds up.
        db.record_log_counts(&[("web".into(), now - 60, [1, 1, 0, 0, 0])]);
        let s = db.log_level_series("web", Range::H1, now);
        assert_eq!(s.step, 60);
        assert_eq!(
            s.points,
            vec![(now - 120, [2, 1, 10, 0, 3]), (now - 60, [2, 1, 0, 0, 0])]
        );
        // 7 days uses the 10-minute rows; both minutes fall in one bucket.
        let s = db.log_level_series("web", Range::D7, now);
        assert_eq!(s.step % 600, 0);
        let total: u64 = s.points.iter().map(|(_, c)| c[0]).sum();
        assert_eq!(total, 4);
        assert!(db
            .log_level_series("nope", Range::H1, now)
            .points
            .is_empty());
        // 2 web minutes + 1 db minute, each at two resolutions (web's share one 10-min row).
        assert_eq!(db.log_count_rows(), 2 + 1 + 1 + 1);
        assert_eq!(db.prune_log_counts(now + KEEP_60 + 600), 3);
        assert_eq!(db.prune_log_counts(now + KEEP_600 + 600), 2);
        assert_eq!(db.log_count_rows(), 0);
    }

    #[test]
    fn clear_history_also_clears_counts() {
        let db = Db::open_in_memory().unwrap();
        db.record_log_counts(&[("web".into(), 600, [1, 0, 0, 0, 0])]);
        db.clear_history();
        assert_eq!(db.log_count_rows(), 0);
        // And recording works again afterwards (name cache reset).
        db.record_log_counts(&[("web".into(), 600, [1, 0, 0, 0, 0])]);
        assert_eq!(db.log_count_rows(), 2);
    }

    #[test]
    fn disabled_db_is_a_no_op() {
        let db = Db::disabled();
        db.record_log_counts(&[("web".into(), 60, [1, 0, 0, 0, 0])]);
        assert!(db.log_level_series("web", Range::H1, 120).points.is_empty());
        assert_eq!(db.prune_log_counts(0), 0);
    }
}
