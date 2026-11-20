//! Extra host series (per core, per device, per interface, sensors, PSI)
//! for the /host page history.
//!
//! One narrow table keyed by a series id instead of a column per metric:
//! the set of cores, disks, interfaces and sensors differs per machine.
//! Same resolutions and retention as the core tables (DESIGN.md section 4):
//! raw 15 s rows for 6 h, 1-minute avg + max for 7 days, 10-minute avg +
//! max for 90 days. The rollup runs from the sampler tick (at most hourly)
//! with its own watermarks, so it needs nothing from `maintain.rs`.
//!
//! Keys are short dotted names chosen by `crate::host` (`cpu.c0`,
//! `psi.io.some`, `disk.sda.rd`, `net.enp1s0.rx`, `temp.coretemp/Core 0`).
//! Values are integers scaled by [`scale`].

use rusqlite::{params, Connection, OptionalExtension};

use super::series::{bucket_for, segments, Src};
use super::{Agg, Db, Inner, Range, Series, RAW_KEEP_SECS, ROLL600_KEEP_SECS, ROLL60_KEEP_SECS};

/// Schema migration v5. Never edit it once deployed.
pub(super) const MIGRATION: &str = "
CREATE TABLE hostx_keys (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE
);
CREATE TABLE hostx_samples (
  kid INTEGER NOT NULL REFERENCES hostx_keys(id),
  ts INTEGER NOT NULL,
  v INTEGER,
  PRIMARY KEY (kid, ts)
) WITHOUT ROWID;
CREATE TABLE hostx_roll (
  kid INTEGER NOT NULL REFERENCES hostx_keys(id),
  res INTEGER NOT NULL, ts INTEGER NOT NULL,
  n INTEGER NOT NULL DEFAULT 1,
  v INTEGER, v_x INTEGER,
  PRIMARY KEY (kid, res, ts)
) WITHOUT ROWID;
";

const W60: &str = "hostx.w60";
const W600: &str = "hostx.w600";
const HOUR: i64 = 3600;

/// Stored integer = value * scale. Byte counts and byte rates are whole
/// numbers already; percentages, temperatures and page rates keep two
/// decimals.
pub fn scale(key: &str) -> f64 {
    let first = key.split('.').next().unwrap_or("");
    let last = key.rsplit('.').next().unwrap_or("");
    if matches!(first, "mem" | "mount") || matches!(last, "rd" | "wr" | "rx" | "tx") {
        1.0
    } else {
        100.0
    }
}

fn kv(conn: &Connection, k: &str) -> rusqlite::Result<i64> {
    Ok(conn
        .query_row("SELECT value FROM kv WHERE key = ?1", [k], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

fn key_id(conn: &Connection, key: &str) -> rusqlite::Result<i64> {
    if let Some(id) = conn
        .prepare("SELECT id FROM hostx_keys WHERE key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    conn.prepare("INSERT INTO hostx_keys (key) VALUES (?1)")?
        .execute([key])?;
    Ok(conn.last_insert_rowid())
}

impl Db {
    /// Write one tick of extra host series; `None` values are stored as
    /// missing (first tick, counter reset). Also runs the hourly rollup.
    pub fn record_hostx(&self, ts: i64, rows: &[(String, Option<f64>)]) {
        if rows.is_empty() {
            return;
        }
        self.with((), |i| {
            let tx = i.conn.transaction()?;
            {
                let mut ins = tx.prepare(
                    "INSERT OR REPLACE INTO hostx_samples (kid, ts, v) VALUES (?1, ?2, ?3)",
                )?;
                for (k, v) in rows {
                    let kid = key_id(&tx, k)?;
                    let v = v
                        .filter(|v| v.is_finite())
                        .map(|v| (v * scale(k)).round().clamp(-9e15, 9e15) as i64);
                    ins.execute(params![kid, ts, v])?;
                }
            }
            tx.commit()
        });
        self.with((), |i| hostx_maintain(i, ts, false));
    }

    /// A chart series for one key (see [`scale`]); empty for an unknown key.
    pub fn hostx_series(&self, key: &str, r: Range, now: i64, agg: Agg) -> Series {
        let bucket = bucket_for(r).1;
        let mut out = Series {
            step: bucket,
            points: Vec::new(),
        };
        self.with((), |i| {
            let Some(kid) = i
                .conn
                .prepare("SELECT id FROM hostx_keys WHERE key = ?1")?
                .query_row([key], |r| r.get::<_, i64>(0))
                .optional()?
            else {
                return Ok(());
            };
            let (w60, w600) = (kv(&i.conn, W60)?, kv(&i.conn, W600)?);
            out.points = query(&i.conn, kid, r, now, w60, w600, bucket, agg, scale(key))?;
            Ok(())
        });
        out
    }

    /// Every key starting with `prefix` (for the history of things that
    /// are not present right now), sorted.
    pub fn hostx_keys(&self, prefix: &str) -> Vec<String> {
        self.with(Vec::new(), |i| {
            let mut st = i
                .conn
                .prepare("SELECT key FROM hostx_keys WHERE substr(key, 1, ?2) = ?1 ORDER BY key")?;
            let rows = st.query_map(params![prefix, prefix.len() as i64], |r| r.get(0))?;
            rows.collect()
        })
    }

    #[cfg(test)]
    /// Row count of the extra host tables (raw + rollups).
    pub fn hostx_rows(&self) -> u64 {
        self.with(0, |i| {
            i.conn.query_row(
                "SELECT (SELECT COUNT(*) FROM hostx_samples) + (SELECT COUNT(*) FROM hostx_roll)",
                [],
                |r| r.get::<_, i64>(0).map(|n| n.max(0) as u64),
            )
        })
    }

    #[cfg(test)]
    /// Force the rollup and pruning now (tests, `clear`).
    pub fn hostx_maintain_now(&self, now: i64) {
        self.with((), |i| hostx_maintain(i, now, true));
    }

    #[cfg(test)]
    /// Drop every extra host series (part of "clear history").
    pub fn hostx_clear(&self) {
        self.with((), |i| {
            i.conn.execute_batch(
                "DELETE FROM hostx_roll; DELETE FROM hostx_samples; DELETE FROM hostx_keys;
                 DELETE FROM kv WHERE key IN ('hostx.w60', 'hostx.w600');",
            )
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn query(
    conn: &Connection,
    kid: i64,
    r: Range,
    now: i64,
    w60: i64,
    w600: i64,
    bucket: i64,
    agg: Agg,
    scale: f64,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    let segs = segments(r, now, w60, w600);
    let mut args: Vec<i64> = vec![bucket, kid];
    let mut parts = Vec::with_capacity(segs.len());
    let vcol = if agg == Agg::Max { "v_x" } else { "v" };
    for s in &segs {
        let n = args.len();
        match s.src {
            Src::Raw => {
                args.extend([s.lo, s.hi]);
                parts.push(format!(
                    "SELECT ts, v, 1 AS w FROM hostx_samples WHERE kid = ?2 AND ts >= ?{} AND ts < ?{}",
                    n + 1,
                    n + 2
                ));
            }
            Src::Roll(res) => {
                args.extend([res, s.lo, s.hi]);
                parts.push(format!(
                    "SELECT ts, {vcol} AS v, n AS w FROM hostx_roll
                     WHERE kid = ?2 AND res = ?{} AND ts >= ?{} AND ts < ?{}",
                    n + 1,
                    n + 2,
                    n + 3
                ));
            }
        }
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    let f = if agg == Agg::Max {
        "MAX(v)"
    } else {
        "SUM(v * w) * 1.0 / SUM(w)"
    };
    let sql = format!(
        "SELECT ts / ?1 * ?1 AS t, {f} FROM ({}) WHERE v IS NOT NULL GROUP BY t ORDER BY t",
        parts.join(" UNION ALL ")
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map(rusqlite::params_from_iter(args), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (t, v) = row?;
        let v = v / scale;
        if v.is_finite() {
            out.push((t, v));
        }
    }
    Ok(out)
}

/// Roll up and prune, at most once an hour unless `force`.
fn hostx_maintain(i: &mut Inner, now: i64, force: bool) -> rusqlite::Result<()> {
    let w60 = kv(&i.conn, W60)?;
    let end60 = now / 60 * 60;
    if !force && end60 - w60 < HOUR {
        return Ok(());
    }
    let tx = i.conn.transaction()?;
    if end60 > w60 {
        tx.execute(
            "INSERT OR REPLACE INTO hostx_roll (kid, res, ts, n, v, v_x)
             SELECT kid, 60, ts / 60 * 60, COUNT(*), CAST(ROUND(AVG(v)) AS INTEGER), MAX(v)
             FROM hostx_samples WHERE ts >= ?1 AND ts < ?2 GROUP BY kid, ts / 60 * 60",
            [w60, end60],
        )?;
        Inner::kv_set(&tx, W60, &end60.to_string())?;
    }
    let w60 = end60.max(w60);
    let w600 = kv(&tx, W600)?;
    let end600 = (now / 600 * 600).min(w60 / 600 * 600);
    if end600 > w600 {
        tx.execute(
            "INSERT OR REPLACE INTO hostx_roll (kid, res, ts, n, v, v_x)
             SELECT kid, 600, ts / 600 * 600, SUM(n),
               CAST(ROUND(SUM(v * n) * 1.0 / SUM(CASE WHEN v IS NOT NULL THEN n END)) AS INTEGER),
               MAX(v_x)
             FROM hostx_roll WHERE res = 60 AND ts >= ?1 AND ts < ?2 GROUP BY kid, ts / 600 * 600",
            [w600, end600],
        )?;
        Inner::kv_set(&tx, W600, &end600.to_string())?;
    }
    let w600 = end600.max(w600);
    // Never drop what is not rolled up yet.
    let raw_cut = (now - RAW_KEEP_SECS).min(w60);
    let r60_cut = (now - ROLL60_KEEP_SECS).min(w600);
    let r600_cut = now - ROLL600_KEEP_SECS;
    let ids: Vec<i64> = {
        let mut st = tx.prepare("SELECT id FROM hostx_keys")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    {
        let mut raw = tx.prepare("DELETE FROM hostx_samples WHERE kid = ?1 AND ts < ?2")?;
        let mut roll =
            tx.prepare("DELETE FROM hostx_roll WHERE kid = ?1 AND res = ?2 AND ts < ?3")?;
        for kid in &ids {
            raw.execute(params![kid, raw_cut])?;
            roll.execute(params![kid, 60, r60_cut])?;
            roll.execute(params![kid, 600, r600_cut])?;
        }
    }
    // Series of devices that are long gone.
    tx.execute(
        "DELETE FROM hostx_keys WHERE
           NOT EXISTS (SELECT 1 FROM hostx_samples s WHERE s.kid = hostx_keys.id)
           AND NOT EXISTS (SELECT 1 FROM hostx_roll r WHERE r.kid = hostx_keys.id)",
        [],
    )?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_query_roll_prune() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_800_000_000 / 3600 * 3600;
        // 2 hours of 15 s samples; one key ramps, one is bytes.
        for k in 0..480 {
            let ts = t0 + k * 15;
            db.record_hostx(
                ts,
                &[
                    ("cpu.c0".into(), Some((k % 4) as f64 * 10.0)),
                    ("net.eth0.rx".into(), Some(1000.0)),
                    ("psi.io.some".into(), None),
                ],
            );
        }
        let now = t0 + 480 * 15;
        let s = db.hostx_series("cpu.c0", Range::H24, now, Agg::Avg);
        assert!(!s.points.is_empty());
        assert!(
            s.points.iter().all(|p| (p.1 - 15.0).abs() < 0.01),
            "{:?}",
            &s.points[..3]
        );
        let m = db.hostx_series("cpu.c0", Range::H24, now, Agg::Max);
        assert!(m.points.iter().all(|p| (p.1 - 30.0).abs() < 0.01));
        assert_eq!(
            db.hostx_series("net.eth0.rx", Range::H6, now, Agg::Avg)
                .points[0]
                .1,
            1000.0
        );
        assert!(db
            .hostx_series("psi.io.some", Range::H1, now, Agg::Avg)
            .points
            .is_empty());
        assert!(db
            .hostx_series("nope", Range::H1, now, Agg::Avg)
            .points
            .is_empty());
        assert_eq!(db.hostx_keys("cpu."), vec!["cpu.c0".to_string()]);
        // A week later raw rows are gone, the rollups remain.
        db.hostx_maintain_now(now + 8 * 86400);
        let d30 = db.hostx_series("cpu.c0", Range::D30, now + 8 * 86400, Agg::Avg);
        assert!(!d30.points.is_empty());
        let rows = db.hostx_rows();
        // only 600 s rollups: 2 h * 6 per hour * 3 keys
        assert!(rows <= 3 * 13, "{rows}");
        db.hostx_clear();
        assert_eq!(db.hostx_rows(), 0);
    }

    #[test]
    fn scales() {
        assert_eq!(scale("mem.cache"), 1.0);
        assert_eq!(scale("disk.sda.rd"), 1.0);
        assert_eq!(scale("disk.sda.util"), 100.0);
        assert_eq!(scale("temp.coretemp/Core 0"), 100.0);
    }
}
