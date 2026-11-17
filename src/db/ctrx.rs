//! Extra per-container metrics for the container detail Metrics tab: CPU
//! throttling, page cache and anonymous memory, the memory limit, OOM kills,
//! network rx/tx, block I/O and PIDs.
//!
//! Sampled by the 15 s history sampler (`on_tick`) from cgroup v2 files and
//! `{host_proc}/<pid>/net/dev` (the container's network namespace, through its
//! main PID from `docker inspect`). Rates are computed from counters against
//! this module's own previous readings. Storage mirrors the core container
//! tables (DESIGN §4): raw rows for 6 h, 1-minute rollups for 7 d, 10-minute
//! rollups for 90 d, average and maximum. The rollups follow the core
//! watermarks (`hist.w60`, `hist.w600`), so `series::segments` picks the same
//! tables for these series as for CPU and memory; they run from `on_tick`
//! as soon as the core rollup moved, and retention runs hourly.
//!
//! Cost per running container and tick: seven small cgroup/proc reads plus
//! one `/proc/<pid>/cgroup` read to check the cached PID still belongs to the
//! container. `docker inspect` runs only when a container is new or was
//! restarted (at most `MAX_INSPECTS_PER_TICK` a tick).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use rusqlite::{params, OptionalExtension};

use super::series::{self, Agg, Seg, Src};
use super::{Db, Inner, Range, Series, RAW_KEEP_SECS, ROLL600_KEEP_SECS, ROLL60_KEEP_SECS};
use crate::state::AppState;

/// Schema migration v4. Never edit it once deployed.
pub(super) const MIGRATION: &str = "
CREATE TABLE ctrx_samples (
  cid INTEGER NOT NULL, ts INTEGER NOT NULL,
  thr INTEGER, cache INTEGER, anon INTEGER, lim INTEGER, oom INTEGER,
  rx INTEGER, tx INTEGER, rd INTEGER, wr INTEGER, pids INTEGER,
  PRIMARY KEY (cid, ts)
) WITHOUT ROWID;
CREATE TABLE ctrx_roll (
  cid INTEGER NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  n INTEGER NOT NULL DEFAULT 1,
  thr INTEGER, thr_x INTEGER, cache INTEGER, cache_x INTEGER,
  anon INTEGER, anon_x INTEGER, lim INTEGER, lim_x INTEGER,
  oom INTEGER, oom_x INTEGER, rx INTEGER, rx_x INTEGER, tx INTEGER, tx_x INTEGER,
  rd INTEGER, rd_x INTEGER, wr INTEGER, wr_x INTEGER, pids INTEGER, pids_x INTEGER,
  PRIMARY KEY (cid, res, ts)
) WITHOUT ROWID;
";

/// Columns, in table order.
const COLS: [&str; 10] = [
    "thr", "cache", "anon", "lim", "oom", "rx", "tx", "rd", "wr", "pids",
];

/// A series in the extra container tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XMetric {
    /// Percent of CFS periods in which the container was throttled.
    Throttled,
    /// Page cache (`memory.stat` file), bytes.
    Cache,
    /// Anonymous memory (`memory.stat` anon), bytes.
    Anon,
    /// `memory.max`, bytes (missing when unlimited).
    Limit,
    /// Cumulative `oom_kill` count of this container instance.
    OomKills,
    NetRx,
    NetTx,
    /// Block I/O, bytes per second.
    BlkRead,
    BlkWrite,
    Pids,
}

impl XMetric {
    fn col(self) -> &'static str {
        match self {
            Self::Throttled => "thr",
            Self::Cache => "cache",
            Self::Anon => "anon",
            Self::Limit => "lim",
            Self::OomKills => "oom",
            Self::NetRx => "rx",
            Self::NetTx => "tx",
            Self::BlkRead => "rd",
            Self::BlkWrite => "wr",
            Self::Pids => "pids",
        }
    }

    fn scale(self) -> f64 {
        match self {
            Self::Throttled => 100.0,
            _ => 1.0,
        }
    }
}

/// One stored sample. Rates are `None` on a container's first reading.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XSample {
    pub throttled_pct: Option<f64>,
    pub cache: Option<u64>,
    pub anon: Option<u64>,
    pub limit: Option<u64>,
    pub oom_kills: Option<u64>,
    pub net_rx: Option<f64>,
    pub net_tx: Option<f64>,
    pub blk_read: Option<f64>,
    pub blk_write: Option<f64>,
    pub pids: Option<u64>,
}

fn int(v: Option<u64>) -> Option<i64> {
    v.map(|v| v.min(i64::MAX as u64) as i64)
}

fn rate(v: Option<f64>, scale: f64) -> Option<i64> {
    v.filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v * scale).round().min(i64::MAX as f64) as i64)
}

impl Db {
    /// `rows` are `(container name, sample)`. One transaction per batch.
    pub fn record_ctrx(&self, ts: i64, rows: &[(&str, &XSample)]) {
        if rows.is_empty() {
            return;
        }
        self.with((), |i| {
            let Inner { conn, names, .. } = i;
            let r = (|| {
                let tx = conn.transaction()?;
                {
                    let mut ins = tx.prepare(
                        "INSERT OR REPLACE INTO ctrx_samples
                         (cid, ts, thr, cache, anon, lim, oom, rx, tx, rd, wr, pids)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    )?;
                    for (name, s) in rows {
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
                            rate(s.throttled_pct, 100.0),
                            int(s.cache),
                            int(s.anon),
                            int(s.limit),
                            int(s.oom_kills),
                            rate(s.net_rx, 1.0),
                            rate(s.net_tx, 1.0),
                            rate(s.blk_read, 1.0),
                            rate(s.blk_write, 1.0),
                            int(s.pids),
                        ])?;
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

    /// A chart series of an extra container metric.
    pub fn ctrx_series(&self, name: &str, m: XMetric, r: Range, now: i64, agg: Agg) -> Series {
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
            out.points = query(i, &segs, bucket, agg, m.col(), cid, m.scale())?;
            Ok(())
        });
        out
    }

    /// Roll up to the core watermarks and, at most hourly, prune. Called
    /// from `on_tick`; cheap when there is nothing to do.
    pub fn ctrx_maintain(&self, now: i64) {
        self.with((), |i| {
            let get = |i: &Inner, k: &str| -> rusqlite::Result<i64> {
                Ok(i.kv_get(k)?.and_then(|v| v.parse().ok()).unwrap_or(0))
            };
            let (mut m60, mut m600) = (get(i, "ctrx.w60")?, get(i, "ctrx.w600")?);
            // History was cleared (core watermarks went back): start over.
            if m60 > i.w60 || m600 > i.w600 {
                m60 = i.w60.min(m60);
                m600 = i.w600.min(m600);
            }
            if i.w60 > m60 {
                let tx = i.conn.transaction()?;
                tx.execute(&sql_60(), [m60, i.w60])?;
                Inner::kv_set(&tx, "ctrx.w60", &i.w60.to_string())?;
                tx.commit()?;
            }
            if i.w600 > m600 {
                let tx = i.conn.transaction()?;
                tx.execute(&sql_600(), [m600, i.w600])?;
                Inner::kv_set(&tx, "ctrx.w600", &i.w600.to_string())?;
                tx.commit()?;
            }
            let last_prune = get(i, "ctrx.prune")?;
            if (now - last_prune).abs() >= super::maintain::HOUR {
                let raw_cut = (now - RAW_KEEP_SECS).min(i.w60);
                let r60_cut = (now - ROLL60_KEEP_SECS).min(i.w600);
                let r600_cut = now - ROLL600_KEEP_SECS;
                let tx = i.conn.transaction()?;
                let cids: Vec<i64> = {
                    let mut st = tx.prepare("SELECT DISTINCT cid FROM ctrx_roll")?;
                    let mut v: Vec<i64> =
                        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                    let mut st = tx.prepare("SELECT DISTINCT cid FROM ctrx_samples")?;
                    v.extend(st.query_map([], |r| r.get::<_, i64>(0))?.flatten());
                    v.sort_unstable();
                    v.dedup();
                    v
                };
                {
                    let mut raw =
                        tx.prepare("DELETE FROM ctrx_samples WHERE cid = ?1 AND ts < ?2")?;
                    let mut roll = tx
                        .prepare("DELETE FROM ctrx_roll WHERE cid = ?1 AND res = ?2 AND ts < ?3")?;
                    for c in &cids {
                        raw.execute(params![c, raw_cut])?;
                        roll.execute(params![c, 60, r60_cut])?;
                        roll.execute(params![c, 600, r600_cut])?;
                    }
                }
                Inner::kv_set(&tx, "ctrx.prune", &now.to_string())?;
                tx.commit()?;
            }
            Ok(())
        });
    }
}

fn cols(f: impl Fn(&str) -> String) -> String {
    COLS.iter().map(|c| f(c)).collect::<Vec<_>>().join(", ")
}

fn sql_60() -> String {
    format!(
        "INSERT OR REPLACE INTO ctrx_roll (cid, res, ts, n, {})
         SELECT cid, 60, ts / 60 * 60, COUNT(*), {} FROM ctrx_samples
         WHERE ts >= ?1 AND ts < ?2 GROUP BY cid, ts / 60 * 60",
        cols(|c| format!("{c}, {c}_x")),
        cols(|c| format!("CAST(ROUND(AVG({c})) AS INTEGER), MAX({c})")),
    )
}

fn sql_600() -> String {
    format!(
        "INSERT OR REPLACE INTO ctrx_roll (cid, res, ts, n, {})
         SELECT cid, 600, ts / 600 * 600, SUM(n), {} FROM ctrx_roll
         WHERE res = 60 AND ts >= ?1 AND ts < ?2 GROUP BY cid, ts / 600 * 600",
        cols(|c| format!("{c}, {c}_x")),
        cols(|c| {
            format!(
            "CAST(ROUND(SUM({c} * n) * 1.0 / SUM(CASE WHEN {c} IS NOT NULL THEN n END)) AS INTEGER), MAX({c}_x)"
        )
        }),
    )
}

/// Same bucketing as `series::query`, over the extra tables. `col` is
/// from `XMetric::col` (never user text); every value is bound.
fn query(
    i: &Inner,
    segs: &[Seg],
    bucket: i64,
    agg: Agg,
    col: &str,
    cid: i64,
    scale: f64,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    let mut args: Vec<i64> = vec![bucket, cid];
    let mut parts = Vec::new();
    let roll_col = if agg == Agg::Max {
        format!("{col}_x")
    } else {
        col.to_string()
    };
    for s in segs {
        args.push(s.lo);
        args.push(s.hi);
        let (lo, hi) = (args.len() - 1, args.len());
        parts.push(match s.src {
            Src::Raw => format!(
                "SELECT ts, {col} AS v, 1 AS w FROM ctrx_samples WHERE cid = ?2 AND ts >= ?{lo} AND ts < ?{hi}"
            ),
            Src::Roll(res) => format!(
                "SELECT ts, {roll_col} AS v, n AS w FROM ctrx_roll
                 WHERE cid = ?2 AND res = {res} AND ts >= ?{lo} AND ts < ?{hi}"
            ),
        });
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
    let mut st = i.conn.prepare(&sql)?;
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

// ---------------------------------------------------------------------------
// Reading cgroup and proc files (pure parsers, unit-tested)

/// Value of `key` in a flat-keyed file (`key value` per line).
pub fn flat(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(' ')?;
        (k == key).then(|| v.trim().parse().ok())?
    })
}

/// `memory.max`, `pids.max`: a number, or `None` for "max".
pub fn parse_max(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// `io.stat`: total `(rbytes, wbytes)` over all devices (what `docker stats`
/// shows as BLOCK I/O).
pub fn parse_io_stat(text: &str) -> (u64, u64) {
    let (mut r, mut w) = (0u64, 0u64);
    for l in text.lines() {
        for f in l.split_whitespace().skip(1) {
            if let Some(v) = f.strip_prefix("rbytes=") {
                r = r.saturating_add(v.parse().unwrap_or(0));
            } else if let Some(v) = f.strip_prefix("wbytes=") {
                w = w.saturating_add(v.parse().unwrap_or(0));
            }
        }
    }
    (r, w)
}

/// `/proc/<pid>/net/dev`: total `(rx bytes, tx bytes)` over every interface
/// but loopback.
pub fn parse_net_dev(text: &str) -> Option<(u64, u64)> {
    let mut any = false;
    let (mut rx, mut tx) = (0u64, 0u64);
    for l in text.lines().skip(2) {
        let Some((iface, rest)) = l.split_once(':') else {
            continue;
        };
        if iface.trim() == "lo" {
            any = true;
            continue;
        }
        let f: Vec<u64> = rest
            .split_whitespace()
            .map(|x| x.parse().unwrap_or(0))
            .collect();
        if f.len() >= 9 {
            rx = rx.saturating_add(f[0]);
            tx = tx.saturating_add(f[8]);
            any = true;
        }
    }
    any.then_some((rx, tx))
}

/// Everything readable right now from one container's cgroup directory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CgroupNow {
    pub nr_periods: Option<u64>,
    pub nr_throttled: Option<u64>,
    pub throttled_usec: Option<u64>,
    pub cache: Option<u64>,
    pub anon: Option<u64>,
    pub limit: Option<u64>,
    pub oom_kills: Option<u64>,
    pub io_read: Option<u64>,
    pub io_write: Option<u64>,
    pub pids: Option<u64>,
    pub pids_max: Option<u64>,
    /// `cpu.stat` `usage_usec`, `memory.current` and `memory.stat`
    /// `inactive_file`: what the core per-container sample needs, so the
    /// history tick reads each file once for both samplers.
    pub usage_usec: Option<u64>,
    pub mem_current: Option<u64>,
    pub inactive_file: Option<u64>,
}

impl CgroupNow {
    /// Memory as `docker stats` shows it: `memory.current` minus
    /// `inactive_file`.
    pub fn docker_mem(&self) -> Option<u64> {
        self.mem_current
            .map(|c| c.saturating_sub(self.inactive_file.unwrap_or(0)))
    }
}

/// One reading per running container that has a cgroup directory, keyed by
/// container id.
pub fn read_all(
    host_sys: &str,
    containers: &[crate::model::Container],
) -> HashMap<String, CgroupNow> {
    containers
        .iter()
        .filter(|c| c.is_running())
        .filter_map(|c| {
            let dir = crate::docker::container_cgroup_dir(host_sys, &c.id)?;
            Some((c.id.clone(), read_cgroup(&dir)))
        })
        .collect()
}

pub fn read_cgroup(dir: &Path) -> CgroupNow {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    let mut c = CgroupNow::default();
    if let Some(t) = read("cpu.stat") {
        c.nr_periods = flat(&t, "nr_periods");
        c.nr_throttled = flat(&t, "nr_throttled");
        c.throttled_usec = flat(&t, "throttled_usec");
        c.usage_usec = flat(&t, "usage_usec");
    }
    if let Some(t) = read("memory.stat") {
        c.cache = flat(&t, "file");
        c.anon = flat(&t, "anon");
        c.inactive_file = flat(&t, "inactive_file");
    }
    c.mem_current = read("memory.current").and_then(|t| t.trim().parse().ok());
    c.limit = read("memory.max").and_then(|t| parse_max(&t));
    c.oom_kills = read("memory.events").and_then(|t| flat(&t, "oom_kill"));
    if let Some(t) = read("io.stat") {
        let (r, w) = parse_io_stat(&t);
        c.io_read = Some(r);
        c.io_write = Some(w);
    }
    c.pids = read("pids.current").and_then(|t| parse_max(&t));
    c.pids_max = read("pids.max").and_then(|t| parse_max(&t));
    c
}

/// Network counters of a process's network namespace.
pub fn read_net(host_proc: &str, pid: i64) -> Option<(u64, u64)> {
    if pid <= 0 {
        return None;
    }
    parse_net_dev(&std::fs::read_to_string(format!("{host_proc}/{pid}/net/dev")).ok()?)
}

/// True when `{host_proc}/<pid>/cgroup` names the container id (the cached
/// PID is still the container's main process).
pub fn pid_belongs(host_proc: &str, pid: i64, id: &str) -> bool {
    pid > 0
        && std::fs::read_to_string(format!("{host_proc}/{pid}/cgroup"))
            .is_ok_and(|t| t.contains(id))
}

// ---------------------------------------------------------------------------
// The sampler hook

/// How the network of a container can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetSource {
    /// Through this host PID's `net/dev`.
    Pid(i64),
    /// `network_mode: host`: its traffic is the host's, not shown per container.
    Host,
    /// `none`, or `container:<other>` (counted on the other container).
    Shared,
}

/// How the network of an inspected container is read.
pub fn net_source_of(i: &crate::docker::Inspect) -> NetSource {
    match i.host_config.network_mode.as_str() {
        "host" => NetSource::Host,
        m if m == "none" || m.starts_with("container:") => NetSource::Shared,
        _ => NetSource::Pid(i.state.pid),
    }
}

#[derive(Debug, Clone)]
struct Prev {
    t: Instant,
    periods: Option<u64>,
    throttled: Option<u64>,
    io: Option<(u64, u64)>,
    net: Option<(u64, u64)>,
}

#[derive(Default)]
struct XSampler {
    prev: HashMap<String, Prev>,
    /// id -> how its network is read.
    net: HashMap<String, NetSource>,
    /// id -> start time (`/proc/<pid>/stat` field 22) of the PID an inspect
    /// gave. While it is unchanged the PID is still that process, even when
    /// its `cgroup` file does not name the container (a process in our own
    /// cgroup namespace reads `0::/`), so it is not inspected again.
    started: HashMap<String, u64>,
}

/// Start time of a process in clock ticks since boot (`stat` field 22).
fn pid_start(host_proc: &str, pid: i64) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    let t = std::fs::read_to_string(format!("{host_proc}/{pid}/stat")).ok()?;
    // The command name (field 2) may hold spaces and parentheses.
    t.get(t.rfind(')')? + 1..)?
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

/// Inspects per tick at most (the first tick after start would otherwise
/// inspect every container at once).
const MAX_INSPECTS_PER_TICK: usize = 8;

static SAMPLER: Mutex<Option<XSampler>> = Mutex::new(None);

/// The cached network source of a container id, if known.
pub fn net_source(id: &str) -> Option<NetSource> {
    SAMPLER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()?
        .net
        .get(id)
        .copied()
}

/// Called by the history sampler after the core container samples, with
/// the cgroup readings it took for them (`read_all`).
pub fn on_tick(state: &AppState, now: i64, cgroups: &HashMap<String, CgroupNow>) {
    let snap = state.snapshot();
    let env = &state.env;
    let t = Instant::now();
    let mut guard = SAMPLER.lock().unwrap_or_else(|e| e.into_inner());
    let s = guard.get_or_insert_with(XSampler::default);
    let mut inspects = 0;
    let mut rows: Vec<(String, XSample)> = Vec::new();
    let mut seen = HashMap::new();
    for c in snap.containers.iter().filter(|c| c.is_running()) {
        let Some(cg) = cgroups.get(&c.id) else {
            continue;
        };
        // Network source: cached, checked against /proc/<pid>/cgroup.
        let same_start = |p: i64| {
            s.started
                .get(&c.id)
                .is_some_and(|&st| pid_start(&env.host_proc, p) == Some(st))
        };
        let src = match s.net.get(&c.id) {
            Some(NetSource::Pid(p)) if same_start(*p) || pid_belongs(&env.host_proc, *p, &c.id) => {
                Some(NetSource::Pid(*p))
            }
            Some(NetSource::Pid(_)) | None => {
                if inspects < MAX_INSPECTS_PER_TICK && !env.docker_socket.is_empty() {
                    inspects += 1;
                    let src = crate::docker::inspect_blocking(&env.docker_socket, &c.id)
                        .ok()
                        .map(|i| net_source_of(&i));
                    match src.and_then(|x| match x {
                        NetSource::Pid(p) => pid_start(&env.host_proc, p),
                        _ => None,
                    }) {
                        Some(st) => s.started.insert(c.id.clone(), st),
                        None => s.started.remove(&c.id),
                    };
                    src
                } else {
                    None
                }
            }
            Some(other) => Some(*other),
        };
        if let Some(src) = src {
            s.net.insert(c.id.clone(), src);
        }
        let net = match src {
            Some(NetSource::Pid(p)) => read_net(&env.host_proc, p),
            _ => None,
        };
        let io = cg.io_read.zip(cg.io_write);
        let cur = Prev {
            t,
            periods: cg.nr_periods,
            throttled: cg.nr_throttled,
            io,
            net,
        };
        let prev = s.prev.get(&c.id);
        let sample = compute(prev, &cur, cg);
        seen.insert(c.id.clone(), cur);
        rows.push((c.name.clone(), sample));
    }
    s.prev = seen;
    // Forget network sources of containers that are gone.
    let live: std::collections::HashSet<&str> =
        snap.containers.iter().map(|c| c.id.as_str()).collect();
    s.net.retain(|id, _| live.contains(id.as_str()));
    s.started.retain(|id, _| live.contains(id.as_str()));
    drop(guard);
    let refs: Vec<(&str, &XSample)> = rows.iter().map(|(n, x)| (n.as_str(), x)).collect();
    state.db.record_ctrx(now, &refs);
    state.db.ctrx_maintain(now);
    let us = t.elapsed().as_micros() as u64;
    LAST_TICK_US.store(us, std::sync::atomic::Ordering::Relaxed);
    // The cost, logged on the first ticks and then hourly.
    let n = TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 4 || n.is_multiple_of(240) {
        crate::info!(
            "container extras sampled",
            containers = rows.len(),
            us = us,
            inspects = inspects
        );
    }
}

static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Duration of the last `on_tick`, microseconds (for the cost report).
pub static LAST_TICK_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The stored sample from a previous and a current reading.
fn compute(prev: Option<&Prev>, cur: &Prev, cg: &CgroupNow) -> XSample {
    let mut x = XSample {
        cache: cg.cache,
        anon: cg.anon,
        limit: cg.limit,
        oom_kills: cg.oom_kills,
        pids: cg.pids,
        ..Default::default()
    };
    let Some(p) = prev else { return x };
    let wall = cur.t.saturating_duration_since(p.t).as_secs_f64();
    if wall < 0.5 {
        return x;
    }
    let d = |a: Option<u64>, b: Option<u64>| -> Option<u64> { b?.checked_sub(a?) };
    if let (Some(dp), Some(dt)) = (d(p.periods, cur.periods), d(p.throttled, cur.throttled)) {
        x.throttled_pct = Some(if dp == 0 {
            0.0
        } else {
            (dt as f64 / dp as f64 * 100.0).min(100.0)
        });
    }
    if let (Some(a), Some(b)) = (p.io, cur.io) {
        x.blk_read = b.0.checked_sub(a.0).map(|v| v as f64 / wall);
        x.blk_write = b.1.checked_sub(a.1).map(|v| v as f64 / wall);
    }
    if let (Some(a), Some(b)) = (p.net, cur.net) {
        x.net_rx = b.0.checked_sub(a.0).map(|v| v as f64 / wall);
        x.net_tx = b.1.checked_sub(a.1).map(|v| v as f64 / wall);
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rh-ctrx-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_cgroup_files_from_a_directory() {
        let d = tmp("cg");
        let w = |f: &str, t: &str| std::fs::write(d.join(f), t).unwrap();
        w(
            "cpu.stat",
            "usage_usec 100\nnr_periods 50\nnr_throttled 5\nthrottled_usec 1234\n",
        );
        w(
            "memory.stat",
            "anon 30797824\nfile 24670208\ninactive_file 17424384\n",
        );
        w("memory.max", "max\n");
        w(
            "memory.events",
            "low 0\nhigh 0\nmax 3\noom 1\noom_kill 2\noom_group_kill 0\n",
        );
        w(
            "io.stat",
            "8:0 rbytes=100 wbytes=10 rios=1 wios=1 dbytes=0 dios=0\n252:0 rbytes=50 wbytes=5 rios=1 wios=1 dbytes=0 dios=0\n",
        );
        w("pids.current", "14\n");
        w("pids.max", "8706\n");
        let c = read_cgroup(&d);
        assert_eq!(c.nr_periods, Some(50));
        assert_eq!(c.nr_throttled, Some(5));
        assert_eq!(c.throttled_usec, Some(1234));
        assert_eq!(c.anon, Some(30797824));
        assert_eq!(c.cache, Some(24670208));
        assert_eq!(c.limit, None);
        assert_eq!(c.oom_kills, Some(2));
        assert_eq!((c.io_read, c.io_write), (Some(150), Some(15)));
        assert_eq!((c.pids, c.pids_max), (Some(14), Some(8706)));
        // The core sample's values come from the same reads.
        assert_eq!(c.usage_usec, Some(100));
        assert_eq!((c.mem_current, c.docker_mem()), (None, None));
        w("memory.current", "50000000\n");
        let c = read_cgroup(&d);
        assert_eq!(c.docker_mem(), Some(50000000 - 17424384));
        w("memory.max", "536870912\n");
        assert_eq!(read_cgroup(&d).limit, Some(536870912));
        // A missing directory reads as nothing, not an error.
        assert_eq!(read_cgroup(&d.join("gone")), CgroupNow::default());
    }

    #[test]
    fn parses_net_dev_of_a_pid_without_loopback() {
        let p = tmp("proc");
        std::fs::create_dir_all(p.join("42/net")).unwrap();
        std::fs::write(
            p.join("42/net/dev"),
            "Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:   36660     268    0    0    0     0          0         0    36660     268    0    0    0     0       0          0
  eth0: 23961094  338106    0    0    0     0          0         0 52318412  360606    0    0    0     0       0          0
  eth1: 6 1 0 0 0 0 0 0 4 1 0 0 0 0 0 0
",
        )
        .unwrap();
        std::fs::write(
            p.join("42/cgroup"),
            "0::/system.slice/docker-abc123.scope\n",
        )
        .unwrap();
        let root = p.to_str().unwrap();
        assert_eq!(read_net(root, 42), Some((23961100, 52318416)));
        assert_eq!(read_net(root, 43), None);
        assert_eq!(read_net(root, 0), None);
        assert!(pid_belongs(root, 42, "abc123"));
        assert!(!pid_belongs(root, 42, "def456"));
        // Only loopback: a namespace with no traffic is still a reading.
        assert_eq!(
            parse_net_dev("h\nh\n    lo: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\n"),
            Some((0, 0))
        );
        assert_eq!(parse_net_dev(""), None);
    }

    #[test]
    fn rates_come_from_previous_counters() {
        let t0 = Instant::now();
        let a = Prev {
            t: t0,
            periods: Some(100),
            throttled: Some(10),
            io: Some((1000, 0)),
            net: Some((0, 0)),
        };
        let b = Prev {
            t: t0 + Duration::from_secs(10),
            periods: Some(200),
            throttled: Some(60),
            io: Some((11_000, 500)),
            net: Some((20_000, 1_000)),
        };
        let cg = CgroupNow {
            pids: Some(3),
            ..Default::default()
        };
        let first = compute(None, &a, &cg);
        assert_eq!(first.net_rx, None);
        assert_eq!(first.pids, Some(3));
        let x = compute(Some(&a), &b, &cg);
        assert_eq!(x.throttled_pct, Some(50.0));
        assert_eq!(x.blk_read, Some(1000.0));
        assert_eq!(x.blk_write, Some(50.0));
        assert_eq!(x.net_rx, Some(2000.0));
        assert_eq!(x.net_tx, Some(100.0));
        // A counter that went backwards (restart) gives no rate.
        let c = Prev {
            io: Some((5, 5)),
            ..b.clone()
        };
        let y = compute(
            Some(&b),
            &Prev {
                t: b.t + Duration::from_secs(10),
                ..c
            },
            &cg,
        );
        assert_eq!(y.blk_read, None);
    }

    #[test]
    fn records_queries_and_rolls_up() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_800_000_000 / 600 * 600;
        let s = |rx: f64| XSample {
            net_rx: Some(rx),
            pids: Some(4),
            throttled_pct: Some(12.5),
            ..Default::default()
        };
        for k in 0..8 {
            // The sampler always writes the core row too (it keeps the name).
            let core = crate::model::ContainerStats::default();
            db.record_containers(now - 3600 + k * 15, &[("web", &core)]);
            db.record_ctrx(now - 3600 + k * 15, &[("web", &s(100.0 * k as f64))]);
        }
        let r = db.ctrx_series("web", XMetric::NetRx, Range::H1, now, Agg::Avg);
        assert!(!r.points.is_empty());
        let thr = db.ctrx_series("web", XMetric::Throttled, Range::H1, now, Agg::Max);
        assert_eq!(thr.points[0].1, 12.5);
        assert!(db
            .ctrx_series("nope", XMetric::NetRx, Range::H1, now, Agg::Avg)
            .points
            .is_empty());
        // Core maintenance moves the watermarks; ours follows on the next tick.
        db.maintain(now);
        db.ctrx_maintain(now);
        let d = db.ctrx_series("web", XMetric::Pids, Range::D7, now, Agg::Avg);
        assert!(!d.points.is_empty());
        assert!(d.points.iter().all(|p| p.1 == 4.0));
        let n: i64 = db.with(0, |i| {
            i.conn
                .query_row("SELECT COUNT(*) FROM ctrx_roll WHERE res = 60", [], |r| {
                    r.get(0)
                })
        });
        assert_eq!(n, 2);
    }
}
