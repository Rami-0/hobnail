//! Hourly housekeeping: roll raw samples up into 1-minute and 10-minute
//! buckets (average and maximum), prune what is past retention, `PRAGMA
//! optimize`, and a weekly `incremental_vacuum`. Each phase takes the
//! connection lock on its own, so a request never waits behind the whole job.

use std::sync::Arc;
use std::time::Duration;

use super::{
    params, Db, Inner, EVENTS_KEEP_SECS, RAW_KEEP_SECS, ROLL600_KEEP_SECS, ROLL60_KEEP_SECS,
    SYS_COLS,
};

pub(crate) const HOUR: i64 = 3600;
const WEEK: i64 = 7 * 86_400;

/// Background task: runs `maintain` a minute after start, then checks every
/// ten minutes (`maintain` itself does the work at most hourly). The work
/// runs on the blocking pool.
pub async fn run(state: Arc<crate::state::AppState>) {
    if !state.db.is_enabled() {
        return;
    }
    tokio::time::sleep(Duration::from_secs(60)).await;
    loop {
        let now = crate::util::unix_now() as i64;
        if state.db.maintain_due(now) {
            let db = state.db.clone();
            let _ = tokio::task::spawn_blocking(move || db.maintain(now)).await;
        }
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
}

fn sys_cols(f: impl Fn(&str) -> String) -> String {
    SYS_COLS.iter().map(|c| f(c)).collect::<Vec<_>>().join(", ")
}

/// Raw samples -> 1-minute rows in `[?1, ?2)`.
fn sql_sys_60() -> String {
    format!(
        "INSERT OR REPLACE INTO sys_roll (res, ts, n, {cols})
         SELECT 60, ts / 60 * 60, COUNT(*), {sel} FROM sys_samples
         WHERE ts >= ?1 AND ts < ?2 GROUP BY ts / 60 * 60",
        cols = sys_cols(|c| format!("{c}, {c}_x")),
        sel = sys_cols(|c| format!("CAST(ROUND(AVG({c})) AS INTEGER), MAX({c})")),
    )
}

/// 1-minute rows -> 10-minute rows in `[?1, ?2)`, averages weighted by the
/// number of samples behind each 1-minute row.
fn sql_sys_600() -> String {
    format!(
        "INSERT OR REPLACE INTO sys_roll (res, ts, n, {cols})
         SELECT 600, ts / 600 * 600, SUM(n), {sel} FROM sys_roll
         WHERE res = 60 AND ts >= ?1 AND ts < ?2 GROUP BY ts / 600 * 600",
        cols = sys_cols(|c| format!("{c}, {c}_x")),
        sel = sys_cols(|c| format!("{}, MAX({c}_x)", wavg(c))),
    )
}

/// Sample-weighted average of a rollup column (NULLs carry no weight).
fn wavg(c: &str) -> String {
    format!(
        "CAST(ROUND(SUM({c} * n) * 1.0 / SUM(CASE WHEN {c} IS NOT NULL THEN n END)) AS INTEGER)"
    )
}

const SQL_CTR_60: &str = "
INSERT OR REPLACE INTO ctr_roll (cid, res, ts, n, cpu, cpu_x, mem, mem_x)
SELECT cid, 60, ts / 60 * 60, COUNT(*),
       CAST(ROUND(AVG(cpu)) AS INTEGER), MAX(cpu),
       CAST(ROUND(AVG(mem)) AS INTEGER), MAX(mem)
FROM ctr_samples WHERE ts >= ?1 AND ts < ?2 GROUP BY cid, ts / 60 * 60";

fn sql_ctr_600() -> String {
    format!(
        "INSERT OR REPLACE INTO ctr_roll (cid, res, ts, n, cpu, cpu_x, mem, mem_x)
         SELECT cid, 600, ts / 600 * 600, SUM(n), {}, MAX(cpu_x), {}, MAX(mem_x)
         FROM ctr_roll WHERE res = 60 AND ts >= ?1 AND ts < ?2
         GROUP BY cid, ts / 600 * 600",
        wavg("cpu"),
        wavg("mem")
    )
}

pub(super) fn run_once(db: &Db, now: i64) {
    // At most once an hour, however often this is called.
    let go = db.with(false, |i| {
        if i.last_maintain.is_some_and(|l| (now - l).abs() < HOUR) {
            return Ok(false);
        }
        i.last_maintain = Some(now);
        i.maintain_runs += 1;
        Ok(true)
    });
    if !go {
        return;
    }
    let t0 = std::time::Instant::now();
    db.with((), |i| rollup_60(i, now));
    db.with((), |i| rollup_600(i, now));
    db.with((), |i| prune(i, now));
    db.with((), |i| optimize(i, now));
    let ms = t0.elapsed().as_millis();
    if ms >= 500 {
        crate::info!("history maintenance was slow", ms = ms);
    }
}

/// Raw -> 1 minute, for every complete minute not rolled up yet.
fn rollup_60(i: &mut Inner, now: i64) -> rusqlite::Result<()> {
    let end = now / 60 * 60;
    if end <= i.w60 {
        return Ok(());
    }
    let tx = i.conn.transaction()?;
    tx.execute(&sql_sys_60(), [i.w60, end])?;
    tx.execute(SQL_CTR_60, [i.w60, end])?;
    Inner::kv_set(&tx, "hist.w60", &end.to_string())?;
    tx.commit()?;
    i.w60 = end;
    Ok(())
}

/// 1 minute -> 10 minutes, for every complete 10 minutes that the 1-minute
/// rollup already covers.
fn rollup_600(i: &mut Inner, now: i64) -> rusqlite::Result<()> {
    let end = (now / 600 * 600).min(i.w60 / 600 * 600);
    if end <= i.w600 {
        return Ok(());
    }
    let tx = i.conn.transaction()?;
    tx.execute(&sql_sys_600(), [i.w600, end])?;
    tx.execute(&sql_ctr_600(), [i.w600, end])?;
    Inner::kv_set(&tx, "hist.w600", &end.to_string())?;
    tx.commit()?;
    i.w600 = end;
    Ok(())
}

pub(super) fn prune(i: &mut Inner, now: i64) -> rusqlite::Result<()> {
    // Never drop data that has not been rolled up yet.
    let raw_cut = (now - RAW_KEEP_SECS).min(i.w60);
    let r60_cut = (now - ROLL60_KEEP_SECS).min(i.w600);
    let r600_cut = now - ROLL600_KEEP_SECS;
    {
        let tx = i.conn.transaction()?;
        tx.execute("DELETE FROM sys_samples WHERE ts < ?1", [raw_cut])?;
        tx.execute("DELETE FROM sys_roll WHERE res = 60 AND ts < ?1", [r60_cut])?;
        tx.execute(
            "DELETE FROM sys_roll WHERE res = 600 AND ts < ?1",
            [r600_cut],
        )?;
        // Container tables are keyed by (cid, ...): one tight range each.
        let ids: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM ctr_names")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        {
            let mut raw = tx.prepare("DELETE FROM ctr_samples WHERE cid = ?1 AND ts < ?2")?;
            let mut roll =
                tx.prepare("DELETE FROM ctr_roll WHERE cid = ?1 AND res = ?2 AND ts < ?3")?;
            for cid in &ids {
                raw.execute(params![cid, raw_cut])?;
                roll.execute(params![cid, 60, r60_cut])?;
                roll.execute(params![cid, 600, r600_cut])?;
            }
        }
        // Containers that no longer have any data.
        tx.execute(
            "DELETE FROM ctr_names WHERE
               NOT EXISTS (SELECT 1 FROM ctr_samples s WHERE s.cid = ctr_names.id)
               AND NOT EXISTS (SELECT 1 FROM ctr_roll r WHERE r.cid = ctr_names.id)",
            [],
        )?;
        tx.execute("DELETE FROM events WHERE ts < ?1", [now - EVENTS_KEEP_SECS])?;
        tx.execute(
            "DELETE FROM drafts WHERE updated < ?1 AND key != ?2",
            params![now - WEEK, super::accounts::SETUP_DRAFT_KEY],
        )?;
        tx.execute("DELETE FROM sessions WHERE expires <= ?1", [now])?;
        tx.commit()?;
    }
    i.names.clear();
    i.cap_events()?;
    Ok(())
}

fn optimize(i: &mut Inner, now: i64) -> rusqlite::Result<()> {
    i.conn.execute_batch("PRAGMA optimize;")?;
    let last: i64 = i
        .kv_get("hist.vacuum")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if now - last >= WEEK {
        // Gives the pages freed by pruning back to the file system.
        i.conn.execute_batch("PRAGMA incremental_vacuum;")?;
        Inner::kv_set(&i.conn, "hist.vacuum", &now.to_string())?;
    }
    Ok(())
}

pub(super) fn clear(db: &Db) {
    db.with((), |i| {
        let tx = i.conn.transaction()?;
        for t in [
            "hostx_roll",
            "hostx_samples",
            "hostx_keys",
            "ctrx_roll",
            "ctrx_samples",
            "ctr_roll",
            "ctr_samples",
            "log_counts", // references ctr_names
            "ctr_names",
            "sys_roll",
            "sys_samples",
            "events",
        ] {
            tx.execute(&format!("DELETE FROM {t}"), [])?;
        }
        tx.execute(
            "DELETE FROM kv WHERE key IN ('hist.w60', 'hist.w600', 'hostx.w60', 'hostx.w600', 'ctrx.w60', 'ctrx.w600')",
            [],
        )?;
        tx.commit()?;
        i.w60 = 0;
        i.w600 = 0;
        i.names.clear();
        // Shrink the file; best effort.
        let _ = i.conn.execute_batch("PRAGMA incremental_vacuum;");
        Ok(())
    });
}
