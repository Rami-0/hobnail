//! Local history database (SQLite). See docs/DESIGN.md section 4.
//!
//! The public API in this file is the contract the UI and settings work build
//! against. While `is_enabled()` is false (no database could be opened) every
//! method records nothing and returns empty results.
//!
//! The handle is synchronous (one connection behind a mutex inside). Async
//! callers should use `tokio::task::spawn_blocking` for reads.
//!
//! Layout: `schema` (pragmas, migrations), `series` (range -> table and
//! bucket, downsampling), `maintain` (rollups, retention, vacuum), `sampler`
//! (the 15 s task), `api` (JSON read API).

#![allow(dead_code)] // parts of the API are only used by the UI/settings work

pub(crate) mod accounts;
pub mod api;
pub mod ctrx;
mod hostx;
mod logcounts;
pub mod sampler;
pub mod strips;

use std::path::Path;

use crate::model::{ContainerStats, SystemStats};

/// Time range for a chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    H1,
    H6,
    H24,
    D7,
    D30,
}

impl Range {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "1h" => Self::H1,
            "6h" => Self::H6,
            "24h" => Self::H24,
            "7d" => Self::D7,
            "30d" => Self::D30,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H6 => "6h",
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
        }
    }

    pub fn secs(self) -> i64 {
        match self {
            Self::H1 => 3600,
            Self::H6 => 6 * 3600,
            Self::H24 => 24 * 3600,
            Self::D7 => 7 * 86400,
            Self::D30 => 30 * 86400,
        }
    }

    pub const ALL: [Range; 5] = [Self::H1, Self::H6, Self::H24, Self::D7, Self::D30];
}

/// Host metrics that are recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysMetric {
    /// Percent, 0..=100.
    Cpu,
    Load1,
    /// Bytes used.
    Mem,
    /// Bytes used.
    Swap,
    /// Percent used (df-style) of the first disk.
    Disk,
    /// Bytes per second.
    NetRx,
    NetTx,
    /// Degrees C.
    Temp,
}

impl SysMetric {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cpu" => Self::Cpu,
            "load1" => Self::Load1,
            "mem" => Self::Mem,
            "swap" => Self::Swap,
            "disk" => Self::Disk,
            "net_rx" => Self::NetRx,
            "net_tx" => Self::NetTx,
            "temp" => Self::Temp,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Load1 => "load1",
            Self::Mem => "mem",
            Self::Swap => "swap",
            Self::Disk => "disk",
            Self::NetRx => "net_rx",
            Self::NetTx => "net_tx",
            Self::Temp => "temp",
        }
    }
}

/// Per-container metrics that are recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrMetric {
    /// Percent of one core (100 = one full core), like `docker stats`.
    Cpu,
    /// Bytes.
    Mem,
}

impl CtrMetric {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "mem" => Some(Self::Mem),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
        }
    }
}

/// A chart series: at most ~400 points, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    /// Seconds between points (the rollup step that was used).
    pub step: i64,
    /// `(unix seconds, value)`; gaps (app down) are simply missing points.
    pub points: Vec<(i64, f64)>,
}

/// Event kinds are dotted strings. Recorded today:
/// `app.start`, `container.start`, `container.stop`, `container.die`,
/// `container.oom`, `container.health`, `alert.fired`, `alert.resolved`,
/// `alert.delivery_failed`, `config.saved`, `config.restored`,
/// `auth.login`, `auth.login_failed`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    /// Unix seconds.
    pub ts: i64,
    pub kind: String,
    /// `info`, `warn` or `error`.
    pub level: String,
    /// Usually a container name, an alert rule name, or empty.
    pub subject: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub subject: Option<String>,
    /// Matches `kind` that starts with this (e.g. `container.`).
    pub kind_prefix: Option<String>,
    /// Only events with `id < before_id` (for paging).
    pub before_id: Option<i64>,
    /// 0 means the default of 50. Capped at 500.
    pub limit: usize,
    /// Leave out `kind`s starting with this. Anonymous viewers get
    /// `auth.` excluded: login history (with client IPs) is not dashboard
    /// data. See [`EventFilter::for_viewer`].
    pub exclude_kind_prefix: Option<String>,
    /// Only events with `ts >= since` / `ts <= until` (unix seconds).
    /// Served by the `events_ts` index (schema v6), so a window reads only
    /// its own rows however long the table is.
    pub since: Option<i64>,
    pub until: Option<i64>,
}

impl EventFilter {
    /// Apply what a viewer may see: without a session, `auth.*` events are
    /// hidden (failed logins carry the client IP).
    pub fn for_viewer(mut self, signed_in: bool) -> Self {
        if !signed_in {
            self.exclude_kind_prefix = Some("auth.".into());
        }
        self
    }
}

/// One saved version of the config file.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    pub id: i64,
    pub ts: i64,
    pub author: String,
    pub note: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbStats {
    pub size_bytes: u64,
    pub sys_rows: u64,
    pub ctr_rows: u64,
    pub event_rows: u64,
    pub revisions: u64,
    pub oldest_ts: Option<i64>,
}

// ---------------------------------------------------------------------------
// Implementation

pub mod maintain;
mod schema;
mod series;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

pub use series::Agg;

/// Raw samples are kept this long (then only the rollups remain).
pub const RAW_KEEP_SECS: i64 = 6 * 3600;
/// 1-minute rollups (avg + max) are kept this long.
pub const ROLL60_KEEP_SECS: i64 = 7 * 86_400;
/// 10-minute rollups (avg + max) are kept this long.
pub const ROLL600_KEEP_SECS: i64 = 90 * 86_400;
/// Events are kept this long, and at most `EVENTS_MAX` rows.
pub const EVENTS_KEEP_SECS: i64 = 180 * 86_400;
pub const EVENTS_MAX: i64 = 100_000;
/// Config revisions kept (newest).
pub const REVISIONS_KEEP: i64 = 200;
/// Concurrent sessions kept (the oldest are dropped past this).
const MAX_SESSIONS: i64 = 20;
/// A sliding session expiry is only written when it moves by at least this
/// much, so a busy page does not turn every request into a write.
const SLIDE_MIN_SECS: i64 = 60;
/// The event cap is enforced on insert every this many events (and hourly).
const CAP_EVERY: u32 = 1000;
const MAX_TEXT: usize = 2 * 1024 * 1024;

/// Host metric columns, in table order. Values are stored as integers scaled
/// by `SysMetric::scale` (varints are compact on disk).
pub(crate) const SYS_COLS: [&str; 8] = [
    "cpu", "load1", "mem", "swap", "disk", "net_rx", "net_tx", "temp",
];

impl SysMetric {
    pub const ALL: [SysMetric; 8] = [
        Self::Cpu,
        Self::Load1,
        Self::Mem,
        Self::Swap,
        Self::Disk,
        Self::NetRx,
        Self::NetTx,
        Self::Temp,
    ];

    /// Stored integer = value * scale.
    fn scale(self) -> f64 {
        match self {
            Self::Cpu | Self::Load1 | Self::Disk => 100.0,
            Self::Temp => 10.0,
            Self::Mem | Self::Swap | Self::NetRx | Self::NetTx => 1.0,
        }
    }
}

impl CtrMetric {
    fn scale(self) -> f64 {
        match self {
            Self::Cpu => 100.0,
            Self::Mem => 1.0,
        }
    }
}

fn enc(v: f64, scale: f64) -> Option<i64> {
    if !v.is_finite() {
        return None;
    }
    Some((v * scale).round().clamp(-9e15, 9e15) as i64)
}

fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut n = max;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

/// The history database handle.
pub struct Db {
    inner: Option<Mutex<Inner>>,
    path: Option<PathBuf>,
}

pub(crate) struct Inner {
    pub(crate) conn: Connection,
    /// Container name -> `ctr_names.id`.
    names: HashMap<String, i64>,
    /// Everything before this (unix s) is in the 1-minute rollup.
    pub(crate) w60: i64,
    /// Everything before this is in the 10-minute rollup.
    pub(crate) w600: i64,
    pub(crate) last_maintain: Option<i64>,
    pub(crate) maintain_runs: u64,
    pub(crate) events_max: i64,
    events_since_cap: u32,
    last_warn: i64,
}

impl Inner {
    fn warn(&mut self, what: &str, e: &rusqlite::Error) {
        // Never flood the log: at most one line a minute.
        let now = crate::util::unix_now() as i64;
        if now - self.last_warn >= 60 {
            self.last_warn = now;
            crate::warn!("database error", op = what, error = e);
        }
    }

    pub(crate) fn kv_get(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .prepare("SELECT value FROM kv WHERE key = ?1")?
            .query_row([key], |r| r.get(0))
            .optional()
    }

    pub(crate) fn kv_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
        conn.prepare("INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)")?
            .execute([key, value])?;
        Ok(())
    }

    /// Delete the oldest events past the row cap.
    pub(crate) fn cap_events(&mut self) -> rusqlite::Result<usize> {
        self.events_since_cap = 0;
        self.conn.execute(
            "DELETE FROM events WHERE id <=
             (SELECT id FROM events ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            [self.events_max],
        )
    }
}

/// Where the server keeps the database: `RH_DB`, else
/// `<data_dir>/hobnail.db`, reusing an existing legacy database in place.
pub fn path_for(env: &crate::state::Env) -> PathBuf {
    match std::env::var("RH_DB") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => default_path(&env.data_dir),
    }
}

/// The same path from the environment alone (`RH_DB`, else `RH_DATA_DIR`,
/// default `./data`), for CLI commands such as `set-password`.
pub fn path_from_env() -> PathBuf {
    match std::env::var("RH_DB") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => default_path(Path::new(
            &std::env::var("RH_DATA_DIR").unwrap_or_else(|_| "./data".into()),
        )),
    }
}

// Reuse the old filename without moving an open SQLite database or its WAL.
// The server and every CLI command must resolve to the same file.
fn default_path(data_dir: &Path) -> PathBuf {
    let current = data_dir.join("hobnail.db");
    let legacy = data_dir.join("rust-homepage.db");
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

/// Open the database; on any failure log it and carry on without history, so
/// the dashboard itself keeps working (read-only file system, corrupt file).
pub fn open_or_disabled(path: &Path) -> Db {
    match Db::open(path) {
        Ok(db) => {
            crate::info!("history database open", path = path.display());
            db
        }
        Err(e) => {
            crate::error!(
                "history database unavailable, running without history",
                path = path.display(),
                error = e
            );
            Db::disabled()
        }
    }
}

impl Db {
    /// A handle that records nothing and returns empty results.
    pub fn disabled() -> Self {
        Self {
            inner: None,
            path: None,
        }
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        // It holds the password hash and session hashes: owner only. Create
        // the file 0600 up front; SQLite gives the -wal and -shm files the
        // database file's mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path);
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("open {}: {e}", path.display()))?;
        let db = Self::init(conn, Some(path.to_path_buf()))
            .map_err(|e| format!("initialise {}: {e}", path.display()))?;
        // Files from an older build may be more open: tighten them.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.as_os_str().to_owned();
                p.push(suffix);
                let p = PathBuf::from(p);
                if let Ok(m) = std::fs::metadata(&p) {
                    if m.permissions().mode() & 0o077 != 0 {
                        let _ =
                            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
                    }
                }
            }
        }
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(conn, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self, String> {
        schema::configure(&conn, path.is_some()).map_err(|e| e.to_string())?;
        schema::migrate(&mut conn)?;
        let get = |k: &str| -> i64 {
            conn.query_row("SELECT value FROM kv WHERE key = ?1", [k], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
        };
        let (w60, w600) = (get("hist.w60"), get("hist.w600"));
        Ok(Self {
            inner: Some(Mutex::new(Inner {
                conn,
                names: HashMap::new(),
                w60,
                w600,
                last_maintain: None,
                maintain_runs: 0,
                events_max: EVENTS_MAX,
                events_since_cap: 0,
                last_warn: 0,
            })),
            path,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Run `f` on the connection. Errors are logged (rate limited) and give
    /// `default`; nothing here panics, even if another thread panicked while
    /// holding the lock.
    pub(crate) fn with<R>(
        &self,
        default: R,
        f: impl FnOnce(&mut Inner) -> rusqlite::Result<R>,
    ) -> R {
        let Some(m) = &self.inner else {
            return default;
        };
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        match f(&mut g) {
            Ok(v) => v,
            Err(e) => {
                g.warn("query", &e);
                default
            }
        }
    }

    // ---- recording (called by the sampler and event hooks) ----

    pub fn record_system(&self, ts: i64, s: &SystemStats) {
        let have_mem = s.mem_total > 0;
        let row: [Option<i64>; 8] = [
            s.cpu_percent.and_then(|v| enc(v, 100.0)),
            enc(s.load[0], 100.0),
            have_mem.then_some(s.mem_used.min(i64::MAX as u64) as i64),
            have_mem.then_some(s.swap_used.min(i64::MAX as u64) as i64),
            s.disks
                .first()
                .and_then(|d| d.used_percent())
                .and_then(|v| enc(v, 100.0)),
            s.net_rx_bps.and_then(|v| enc(v, 1.0)),
            s.net_tx_bps.and_then(|v| enc(v, 1.0)),
            s.temp_c.and_then(|v| enc(v, 10.0)),
        ];
        self.with((), |i| {
            i.conn
                .prepare(
                    "INSERT OR REPLACE INTO sys_samples
                     (ts, cpu, load1, mem, swap, disk, net_rx, net_tx, temp)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )?
                .execute(params![
                    ts, row[0], row[1], row[2], row[3], row[4], row[5], row[6], row[7]
                ])?;
            Ok(())
        });
    }

    /// `rows` are `(container name, stats)`. One transaction per batch.
    pub fn record_containers(&self, ts: i64, rows: &[(&str, &ContainerStats)]) {
        if rows.is_empty() {
            return;
        }
        self.with((), |i| {
            let Inner { conn, names, .. } = i;
            let r = (|| {
                let tx = conn.transaction()?;
                {
                    let mut ins = tx.prepare(
                        "INSERT OR REPLACE INTO ctr_samples (cid, ts, cpu, mem)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?;
                    for (name, st) in rows {
                        if name.is_empty() || name.len() > 256 {
                            continue;
                        }
                        let cid = match names.get(*name) {
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
                        ins.execute(params![
                            cid,
                            ts,
                            enc(st.cpu_percent, 100.0),
                            st.mem_bytes.min(i64::MAX as u64) as i64
                        ])?;
                    }
                }
                tx.commit()
            })();
            if r.is_err() {
                // Ids cached during a rolled-back transaction would dangle.
                names.clear();
            }
            r
        });
    }

    pub fn record_event(&self, ts: i64, kind: &str, level: &str, subject: &str, detail: &str) {
        let level = match level {
            "warn" | "error" => level,
            _ => "info",
        };
        self.with((), |i| {
            i.conn
                .prepare(
                    "INSERT INTO events (ts, kind, level, subject, detail)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?
                .execute(params![
                    ts,
                    clip(kind, 64),
                    level,
                    clip(subject, 256),
                    clip(detail, 1024)
                ])?;
            i.events_since_cap += 1;
            if i.events_since_cap >= CAP_EVERY {
                i.cap_events()?;
            }
            Ok(())
        });
    }

    // ---- reading ----

    pub fn system_series(&self, m: SysMetric, r: Range, now: i64) -> Series {
        self.system_series_agg(m, r, now, Agg::Avg)
    }

    /// Like `system_series`, choosing the bucket average or maximum.
    pub fn system_series_agg(&self, m: SysMetric, r: Range, now: i64, agg: Agg) -> Series {
        let bucket = series::bucket_for(r).1;
        let mut out = Series {
            step: bucket,
            points: Vec::new(),
        };
        self.with((), |i| {
            let segs = series::segments(r, now, i.w60, i.w600);
            out.points = series::query(&i.conn, &segs, bucket, agg, m.as_str(), None, m.scale())?;
            Ok(())
        });
        out
    }

    pub fn container_series(&self, name: &str, m: CtrMetric, r: Range, now: i64) -> Series {
        self.container_series_agg(name, m, r, now, Agg::Avg)
    }

    /// Like `container_series`, choosing the bucket average or maximum.
    pub fn container_series_agg(
        &self,
        name: &str,
        m: CtrMetric,
        r: Range,
        now: i64,
        agg: Agg,
    ) -> Series {
        let bucket = series::bucket_for(r).1;
        let mut out = Series {
            step: bucket,
            points: Vec::new(),
        };
        self.with((), |i| {
            let cid: Option<i64> = match i.names.get(name) {
                Some(&id) => Some(id),
                None => i
                    .conn
                    .prepare("SELECT id FROM ctr_names WHERE name = ?1")?
                    .query_row([name], |r| r.get(0))
                    .optional()?,
            };
            let Some(cid) = cid else { return Ok(()) };
            let segs = series::segments(r, now, i.w60, i.w600);
            out.points = series::query(
                &i.conn,
                &segs,
                bucket,
                agg,
                m.as_str(),
                Some(cid),
                m.scale(),
            )?;
            Ok(())
        });
        out
    }

    /// Each container's newest sample at or after `since` (unix seconds):
    /// `name -> (cpu percent, memory bytes)`. Lets pages show numbers on
    /// first paint, before the live sampler has two readings. One indexed
    /// lookup per known name.
    pub fn latest_containers(&self, since: i64) -> std::collections::HashMap<String, (f64, u64)> {
        self.with(Default::default(), |i| {
            let mut st = i.conn.prepare(
                "SELECT n.name, s.cpu, s.mem FROM ctr_names n
                 JOIN ctr_samples s ON s.cid = n.id
                  AND s.ts = (SELECT MAX(ts) FROM ctr_samples WHERE cid = n.id)
                 WHERE s.ts >= ?1",
            )?;
            let rows = st.query_map([since], |r| {
                let cpu: Option<i64> = r.get(1)?;
                let mem: Option<i64> = r.get(2)?;
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        cpu.unwrap_or(0) as f64 / 100.0,
                        mem.unwrap_or(0).max(0) as u64,
                    ),
                ))
            })?;
            rows.collect()
        })
    }

    pub fn events(&self, f: &EventFilter) -> Vec<Event> {
        let limit = match f.limit {
            0 => 50,
            n => n.min(500),
        };
        self.events_ordered(f, limit, "id DESC")
    }

    /// Every event of `f` inside its `since..=until` window, newest first,
    /// at most `cap` (up to 10,000) rows, in one pass over `events_ts` in
    /// index order (`ts DESC, id DESC`): no sort and no repeated seek,
    /// however much of the table the window holds. Ids and timestamps both
    /// grow, so this is the order of `events` up to events of one second.
    pub fn window_events(&self, f: &EventFilter, cap: usize) -> Vec<Event> {
        self.events_ordered(f, cap.clamp(1, 10_000), "ts DESC, id DESC")
    }

    fn events_ordered(&self, f: &EventFilter, limit: usize, order: &str) -> Vec<Event> {
        let limit = limit as i64;
        self.with(Vec::new(), |i| {
            let mut sql =
                String::from("SELECT id, ts, kind, level, subject, detail FROM events WHERE 1 = 1");
            let mut args: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(s) = &f.subject {
                args.push(s.clone().into());
                sql.push_str(&format!(" AND subject = ?{}", args.len()));
            }
            if let Some(k) = f.kind_prefix.as_ref().filter(|k| !k.is_empty()) {
                args.push((k.chars().count() as i64).into());
                args.push(k.clone().into());
                sql.push_str(&format!(
                    " AND substr(kind, 1, ?{}) = ?{}",
                    args.len() - 1,
                    args.len()
                ));
            }
            if let Some(k) = f.exclude_kind_prefix.as_ref().filter(|k| !k.is_empty()) {
                args.push((k.chars().count() as i64).into());
                args.push(k.clone().into());
                sql.push_str(&format!(
                    " AND substr(kind, 1, ?{}) != ?{}",
                    args.len() - 1,
                    args.len()
                ));
            }
            if let Some(b) = f.before_id {
                args.push(b.into());
                sql.push_str(&format!(" AND id < ?{}", args.len()));
            }
            if let Some(t) = f.since {
                args.push(t.into());
                sql.push_str(&format!(" AND ts >= ?{}", args.len()));
            }
            if let Some(t) = f.until {
                args.push(t.into());
                sql.push_str(&format!(" AND ts <= ?{}", args.len()));
            }
            args.push(limit.into());
            sql.push_str(&format!(" ORDER BY {order} LIMIT ?{}", args.len()));
            let mut st = i.conn.prepare(&sql)?;
            let rows = st.query_map(rusqlite::params_from_iter(args), |r| {
                Ok(Event {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    kind: r.get(2)?,
                    level: r.get(3)?,
                    subject: r.get(4)?,
                    detail: r.get(5)?,
                })
            })?;
            rows.collect()
        })
    }

    /// The newest event id with `ts <= t`, or `None` when every event is
    /// newer: one seek on `events_ts`, so a History window far in the past
    /// starts its scan at the window. Ids and timestamps both grow; among
    /// events of the same second the highest id wins.
    pub fn event_id_at_or_before(&self, t: i64) -> Option<i64> {
        self.with(None, |i| {
            i.conn
                .prepare("SELECT id FROM events WHERE ts <= ?1 ORDER BY ts DESC, id DESC LIMIT 1")?
                .query_row([t], |r| r.get(0))
                .optional()
        })
    }

    /// Containers with their number of `container.start` events since `since`
    /// (unix seconds): restart counts.
    pub fn restart_counts(&self, since: i64) -> Vec<(String, u32)> {
        self.with(Vec::new(), |i| {
            let mut st = i.conn.prepare(
                "SELECT subject, COUNT(*) AS n FROM events
                 WHERE kind = 'container.start' AND ts >= ?1
                 GROUP BY subject ORDER BY n DESC, subject",
            )?;
            let rows = st.query_map([since], |r| Ok((r.get(0)?, r.get::<_, u32>(1)?)))?;
            rows.collect()
        })
    }

    // ---- config revisions ----

    /// Returns the new revision id. Keeps the newest `REVISIONS_KEEP`.
    pub fn add_revision(&self, ts: i64, author: &str, note: &str, text: &str) -> Option<i64> {
        if text.len() > MAX_TEXT {
            return None;
        }
        self.with(None, |i| {
            let tx = i.conn.transaction()?;
            tx.execute(
                "INSERT INTO config_revisions (ts, author, note, text) VALUES (?1, ?2, ?3, ?4)",
                params![ts, clip(author, 64), clip(note, 512), text],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "DELETE FROM config_revisions WHERE id <=
                 (SELECT id FROM config_revisions ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                [REVISIONS_KEEP],
            )?;
            tx.commit()?;
            Ok(Some(id))
        })
    }

    /// Newest first.
    pub fn revisions(&self, limit: usize) -> Vec<Revision> {
        let limit = limit.clamp(1, 1000) as i64;
        self.with(Vec::new(), |i| {
            let mut st = i.conn.prepare(
                "SELECT id, ts, author, note, length(CAST(text AS BLOB))
                 FROM config_revisions ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = st.query_map([limit], |r| {
                Ok(Revision {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    author: r.get(2)?,
                    note: r.get(3)?,
                    bytes: r.get::<_, i64>(4)?.max(0) as usize,
                })
            })?;
            rows.collect()
        })
    }

    pub fn revision_text(&self, id: i64) -> Option<String> {
        self.with(None, |i| {
            i.conn
                .prepare("SELECT text FROM config_revisions WHERE id = ?1")?
                .query_row([id], |r| r.get(0))
                .optional()
        })
    }

    // ---- sessions and small settings (used by auth) ----

    pub fn delete_session(&self, token_hash: &str) {
        self.with((), |i| {
            i.conn
                .prepare("DELETE FROM sessions WHERE token_hash = ?1")?
                .execute([token_hash])?;
            Ok(())
        });
    }

    /// Number of sessions that have not expired.
    pub fn session_count(&self, now: i64) -> u64 {
        self.with(0, |i| {
            i.conn
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE expires > ?1",
                    [now],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| n.max(0) as u64)
        })
    }

    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.with(None, |i| i.kv_get(key))
    }

    pub fn kv_set(&self, key: &str, value: &str) -> bool {
        self.with(false, |i| {
            Inner::kv_set(&i.conn, key, value)?;
            Ok(true)
        })
    }

    pub fn kv_delete(&self, key: &str) {
        self.with((), |i| {
            i.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
            Ok(())
        });
    }

    // ---- housekeeping ----

    pub fn stats(&self) -> DbStats {
        let file_bytes = self.path.as_ref().map(|p| {
            let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            let mut wal = p.as_os_str().to_owned();
            wal.push("-wal");
            len(p) + len(Path::new(&wal))
        });
        self.with(DbStats::default(), |i| {
            let count = |sql: &str| -> rusqlite::Result<u64> {
                i.conn
                    .query_row(sql, [], |r| r.get::<_, i64>(0))
                    .map(|n| n.max(0) as u64)
            };
            let sys_rows = count("SELECT COUNT(*) FROM sys_samples")?
                + count("SELECT COUNT(*) FROM sys_roll")?;
            let ctr_rows = count("SELECT COUNT(*) FROM ctr_samples")?
                + count("SELECT COUNT(*) FROM ctr_roll")?;
            let page_bytes = count("PRAGMA page_count")? * count("PRAGMA page_size")?;
            let mut oldest: Option<i64> = None;
            for sql in [
                "SELECT MIN(ts) FROM sys_roll WHERE res = 600",
                "SELECT MIN(ts) FROM sys_roll WHERE res = 60",
                "SELECT MIN(ts) FROM sys_samples",
            ] {
                let t: Option<i64> = i.conn.query_row(sql, [], |r| r.get(0))?;
                if let Some(t) = t {
                    oldest = Some(oldest.map_or(t, |o| o.min(t)));
                }
            }
            Ok(DbStats {
                size_bytes: file_bytes.unwrap_or(page_bytes),
                sys_rows,
                ctr_rows,
                event_rows: count("SELECT COUNT(*) FROM events")?,
                revisions: count("SELECT COUNT(*) FROM config_revisions")?,
                oldest_ts: oldest,
            })
        })
    }

    /// Roll up, prune, optimize. Safe to call often; does the work at most hourly.
    pub fn maintain(&self, now: i64) {
        maintain::run_once(self, now);
        self.release_memory();
    }

    /// Give the page cache back (`sqlite3_db_release_memory`). Called after
    /// each sampler tick and after maintenance: the cache is only worth
    /// something while a page is being read, and with nobody viewing it is
    /// most of the process's heap (about 2 MiB on a real database). The
    /// pages stay in the kernel's file cache, so the next read is a copy, not
    /// a disk read.
    pub fn release_memory(&self) {
        self.with((), |i| i.conn.release_memory());
    }

    /// True when `maintain(now)` would do its work (not run in the last hour).
    pub fn maintain_due(&self, now: i64) -> bool {
        self.with(false, |i| {
            Ok(i.last_maintain
                .is_none_or(|l| (now - l).abs() >= maintain::HOUR))
        })
    }

    /// Delete all samples and events (not config revisions, sessions or kv).
    pub fn clear_history(&self) {
        maintain::clear(self);
    }
}

// ---- Docker events ----

/// Record the Docker lifecycle events we keep: start, stop, die (with exit
/// code), oom and health changes. Everything else is ignored. A health event
/// is only stored when the state differs from the last one stored for that
/// container.
pub fn record_docker_event(db: &Db, ev: &crate::docker::Event) {
    let Some(name) = ev.name.as_deref() else {
        return;
    };
    let ts = if ev.time > 0 {
        ev.time
    } else {
        crate::util::unix_now()
    } as i64;
    let (kind, level, detail) = match ev.action.as_str() {
        "start" => ("container.start", "info", String::new()),
        "stop" => ("container.stop", "info", String::new()),
        "die" => {
            let code = ev.detail.as_deref().unwrap_or("?");
            (
                "container.die",
                if code == "0" { "info" } else { "warn" },
                format!("exit code {code}"),
            )
        }
        "oom" => (
            "container.oom",
            "error",
            "killed: out of memory".to_string(),
        ),
        "health_status" => {
            let st = ev.detail.as_deref().unwrap_or("unknown");
            let last: Option<String> = db.with(None, |i| {
                i.conn
                    .prepare(
                        "SELECT detail FROM events
                         WHERE subject = ?1 AND kind = 'container.health'
                         ORDER BY id DESC LIMIT 1",
                    )?
                    .query_row([name], |r| r.get(0))
                    .optional()
            });
            if last.as_deref() == Some(st) {
                return;
            }
            (
                "container.health",
                if st == "unhealthy" { "warn" } else { "info" },
                st.to_string(),
            )
        }
        _ => return,
    };
    db.record_event(ts, kind, level, name, &detail);
}
