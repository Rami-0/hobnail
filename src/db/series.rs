//! Chart series: which tables a range reads, and downsampling to at most
//! `MAX_POINTS` buckets.
//!
//! Ranges up to 6 h read raw samples, 24 h and 7 d the 1-minute rollup, 30 d
//! the 10-minute rollup. Rollups run hourly, so the newest part of a range is
//! not rolled up yet: it is stitched on from the finer table, after the
//! coarser table's watermark, so no time is counted twice. Each bucket is one
//! point (average, or maximum); a bucket with no data gives no point, so a
//! gap (the app was down) stays a gap.

use rusqlite::Connection;

use super::Range;

/// Nominal raw sampling step; bucket sizes are multiples of it.
pub(crate) const RAW_STEP: i64 = 15;
/// A series has at most about this many points.
pub(crate) const MAX_POINTS: i64 = 400;

/// Which aggregate of a bucket a series shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agg {
    Avg,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Src {
    Raw,
    /// A rollup at this resolution (seconds).
    Roll(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seg {
    pub src: Src,
    pub lo: i64,
    pub hi: i64,
}

/// `(base resolution, bucket)`: the resolution the range reads from and the
/// bucket size in seconds, a multiple of it giving at most `MAX_POINTS`.
pub(crate) fn bucket_for(r: Range) -> (i64, i64) {
    let base = match r {
        Range::H1 | Range::H6 => RAW_STEP,
        Range::H24 | Range::D7 => 60,
        Range::D30 => 600,
    };
    let want = (r.secs() + MAX_POINTS - 1) / MAX_POINTS;
    (base, ((want + base - 1) / base).max(1) * base)
}

/// The table segments to read for `[now - range, now]`, given the rollup
/// watermarks (everything before `w60` is in the 1-minute rollup, before
/// `w600` in the 10-minute one).
pub(crate) fn segments(r: Range, now: i64, w60: i64, w600: i64) -> Vec<Seg> {
    let from = now - r.secs();
    let end = i64::MAX;
    let (base, _) = bucket_for(r);
    let mut segs = Vec::with_capacity(3);
    let mut push = |src, lo: i64, hi: i64| {
        if lo < hi {
            segs.push(Seg { src, lo, hi });
        }
    };
    match base {
        600 => {
            push(Src::Roll(600), from, w600);
            push(Src::Roll(60), from.max(w600), w60);
            push(Src::Raw, from.max(w60), end);
        }
        60 => {
            push(Src::Roll(60), from, w60);
            push(Src::Raw, from.max(w60), end);
        }
        _ => push(Src::Raw, from, end),
    }
    segs
}

/// Run the bucketed query. `col` is a metric column name from the metric
/// enums (never user text); `cid` selects a container (else host tables).
/// Every value is a bound parameter.
pub(crate) fn query(
    conn: &Connection,
    segs: &[Seg],
    bucket: i64,
    agg: Agg,
    col: &str,
    cid: Option<i64>,
    scale: f64,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    debug_assert!(col
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
    let mut args: Vec<i64> = vec![bucket];
    let mut p = |v: i64| {
        args.push(v);
        format!("?{}", args.len())
    };
    let roll_col = if agg == Agg::Max {
        format!("{col}_x")
    } else {
        col.to_string()
    };
    let mut parts = Vec::with_capacity(segs.len());
    for s in segs {
        let part = match (s.src, cid) {
            (Src::Raw, None) => format!(
                "SELECT ts, {col} AS v, 1 AS w FROM sys_samples WHERE ts >= {} AND ts < {}",
                p(s.lo),
                p(s.hi)
            ),
            (Src::Roll(res), None) => format!(
                "SELECT ts, {roll_col} AS v, n AS w FROM sys_roll WHERE res = {} AND ts >= {} AND ts < {}",
                p(res),
                p(s.lo),
                p(s.hi)
            ),
            (Src::Raw, Some(c)) => format!(
                "SELECT ts, {col} AS v, 1 AS w FROM ctr_samples WHERE cid = {} AND ts >= {} AND ts < {}",
                p(c),
                p(s.lo),
                p(s.hi)
            ),
            (Src::Roll(res), Some(c)) => format!(
                "SELECT ts, {roll_col} AS v, n AS w FROM ctr_roll
                 WHERE cid = {} AND res = {} AND ts >= {} AND ts < {}",
                p(c),
                p(res),
                p(s.lo),
                p(s.hi)
            ),
        };
        parts.push(part);
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    // A bucket can mix rows of different resolution (raw 15 s rows next to
    // 1-minute rollups at a watermark): weight each by its sample count.
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
