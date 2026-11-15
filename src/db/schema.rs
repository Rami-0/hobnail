//! Connection settings and the forward-only schema migrations
//! (`PRAGMA user_version`).

use rusqlite::{Connection, TransactionBehavior};

/// Entry `n` upgrades `user_version` n to n + 1. Never edit an entry that has
/// shipped; append a new one.
///
/// Metric values are stored as scaled integers (see `SysMetric::scale`);
/// `*_x` columns hold the bucket maximum, `n` the number of raw samples. Containers are keyed by name
/// (`ctr_names`), because ids change on every recreate.
pub(super) const MIGRATIONS: &[&str] = &[
    // v1
    "
CREATE TABLE sys_samples (
  ts INTEGER PRIMARY KEY,
  cpu INTEGER, load1 INTEGER, mem INTEGER, swap INTEGER,
  disk INTEGER, net_rx INTEGER, net_tx INTEGER, temp INTEGER
);
CREATE TABLE sys_roll (
  res INTEGER NOT NULL, ts INTEGER NOT NULL,
  cpu INTEGER, cpu_x INTEGER, load1 INTEGER, load1_x INTEGER,
  mem INTEGER, mem_x INTEGER, swap INTEGER, swap_x INTEGER,
  disk INTEGER, disk_x INTEGER, net_rx INTEGER, net_rx_x INTEGER,
  net_tx INTEGER, net_tx_x INTEGER, temp INTEGER, temp_x INTEGER,
  PRIMARY KEY (res, ts)
) WITHOUT ROWID;
CREATE TABLE ctr_names (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);
CREATE TABLE ctr_samples (
  cid INTEGER NOT NULL REFERENCES ctr_names(id),
  ts INTEGER NOT NULL,
  cpu INTEGER, mem INTEGER,
  PRIMARY KEY (cid, ts)
) WITHOUT ROWID;
CREATE TABLE ctr_roll (
  cid INTEGER NOT NULL REFERENCES ctr_names(id),
  res INTEGER NOT NULL, ts INTEGER NOT NULL,
  cpu INTEGER, cpu_x INTEGER, mem INTEGER, mem_x INTEGER,
  PRIMARY KEY (cid, res, ts)
) WITHOUT ROWID;
CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL, level TEXT NOT NULL,
  subject TEXT NOT NULL, detail TEXT NOT NULL
);
CREATE INDEX events_subject ON events (subject, id);
CREATE TABLE config_revisions (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  author TEXT NOT NULL, note TEXT NOT NULL, text TEXT NOT NULL
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  created INTEGER NOT NULL, expires INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE kv (
  key TEXT PRIMARY KEY, value TEXT NOT NULL
) WITHOUT ROWID;
",
    // v2: how many raw samples are behind a rollup row, so averages over
    // rows of different resolutions (and 1 min -> 10 min) are weighted.
    "
ALTER TABLE sys_roll ADD COLUMN n INTEGER NOT NULL DEFAULT 1;
ALTER TABLE ctr_roll ADD COLUMN n INTEGER NOT NULL DEFAULT 1;
",
    // v3: log level counts (src/db/logcounts.rs)
    super::logcounts::MIGRATION,
    // v4: extra container metrics (src/db/ctrx.rs)
    super::ctrx::MIGRATION,
    // v5: extra host series (src/db/hostx.rs)
    super::hostx::MIGRATION,
];

/// Pragmas for every connection. `file` is false for `:memory:`.
pub(super) fn configure(conn: &Connection, file: bool) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // Only takes effect while the database has no tables yet (a new file);
    // it lets the weekly `incremental_vacuum` actually shrink the file.
    conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
    if file {
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "journal_mode is {mode}, not wal"
            )));
        }
    }
    // 2 MiB page cache, no mmap (RSS budget), temp tables in memory (the
    // container's root file system is read-only; only /data is writable).
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -2048;
         PRAGMA mmap_size = 0;
         PRAGMA journal_size_limit = 4194304;
         PRAGMA wal_autocheckpoint = 500;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;",
    )
}

pub(super) fn version(conn: &Connection) -> Result<usize, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v.max(0) as usize)
        .map_err(|e| e.to_string())
}

/// Apply every migration this build knows and the file does not have yet.
/// Idempotent; refuses a file written by a newer build.
pub(super) fn migrate(conn: &mut Connection) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let v = version(conn)?;
    if v > MIGRATIONS.len() {
        return Err(format!(
            "database schema version {v} is newer than this build understands ({})",
            MIGRATIONS.len()
        ));
    }
    if v == MIGRATIONS.len() {
        return Ok(());
    }
    // Immediate: another process (`set-password`) may be opening the same
    // file at the same moment; re-read the version inside the lock.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    let v = version(&tx)?;
    for (n, sql) in MIGRATIONS.iter().enumerate().skip(v) {
        tx.execute_batch(sql).map_err(e)?;
        tx.pragma_update(None, "user_version", (n + 1) as i64)
            .map_err(e)?;
    }
    tx.commit().map_err(e)
}
