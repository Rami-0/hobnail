//! Host collectors (Wave 1 item 3, docs/V3-PLAN.md):
//! per-core CPU, PSI, memory detail, per-mount and per-device I/O, per
//! interface, all hwmon sensors, top processes mapped to containers.
//!
//! Two readers, as for the core history (see `db/sampler.rs`):
//! - the history sampler calls [`on_tick`] every 15 s; it keeps its own
//!   `Collector`, records the extra series and remembers the last result so
//!   a fresh page view has rates at once;
//! - the Host API uses a second `Collector` and can request a process scan.
//!   The scan walks every pid and runs only for an active administrator.
//!
//! Results of the live reader are reused for 2 s, so many viewers do not
//! multiply the work.

pub mod collect;
pub mod parse;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::state::AppState;
use collect::{Collector, Host, ProcReport, ProcScan};

/// Processes listed in the table.
const TOP_N: usize = 15;
/// A live result younger than this is reused.
const REUSE_SECS: f64 = 2.0;

struct Live {
    paths: (String, String),
    host: Collector,
    procs: ProcScan,
    last: Option<(Instant, Arc<Host>)>,
    report: Option<(Instant, Arc<ProcReport>)>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static TICK: Mutex<Option<(String, Collector)>> = Mutex::new(None);
static LAST_TICK: Mutex<Option<(String, Arc<Host>)>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A current reading with rates, and the process table.
pub(crate) fn live_sample(state: &AppState, scan: bool) -> (Arc<Host>, Option<Arc<ProcReport>>) {
    let paths = (state.env.host_proc.clone(), state.env.host_sys.clone());
    let mut g = lock(&LIVE);
    if g.as_ref().is_none_or(|l| l.paths != paths) {
        *g = Some(Live {
            host: Collector::new(&paths.0, &paths.1),
            procs: ProcScan::new(&paths.0),
            paths,
            last: None,
            report: None,
        });
    }
    let l = g.as_mut().expect("set above");
    let fresh = |at: &Instant| at.elapsed().as_secs_f64() < REUSE_SECS;
    let h = if let Some((_, cached)) = l.last.as_ref().filter(|(at, _)| fresh(at)) {
        cached.clone()
    } else {
        let mut h = l.host.collect();
        if h.window == 0.0 {
            // First view: borrow the sampler's rates (15 s window), keep the
            // fresh levels.
            let key = format!("{}\0{}", l.paths.0, l.paths.1);
            if let Some((_, t)) = lock(&LAST_TICK).as_ref().filter(|t| t.0 == key) {
                let mut t = (**t).clone();
                t.mem = h.mem;
                t.mem_ok = h.mem_ok;
                t.load = h.load;
                t.uptime = h.uptime;
                t.sensors = std::mem::take(&mut h.sensors);
                t.mounts = std::mem::take(&mut h.mounts);
                h = t;
            }
        }
        let h = Arc::new(h);
        l.last = Some((Instant::now(), h.clone()));
        h
    };
    if scan && l.report.as_ref().is_none_or(|(at, _)| !fresh(at)) {
        l.report = Some((Instant::now(), Arc::new(l.procs.scan(h.uptime, TOP_N))));
    }
    let report = scan
        .then(|| l.report.as_ref().map(|(_, p)| p.clone()))
        .flatten();
    (h, report)
}

/// Called by the history sampler every `[history] sample_interval`, after
/// the core samples are written. Records the extra host series.
pub fn on_tick(state: &AppState, now: i64) {
    let key = format!("{}\0{}", state.env.host_proc, state.env.host_sys);
    let h = {
        let mut g = lock(&TICK);
        if g.as_ref().is_none_or(|(k, _)| *k != key) {
            *g = Some((
                key.clone(),
                Collector::new(&state.env.host_proc, &state.env.host_sys),
            ));
        }
        g.as_mut().expect("set above").1.collect()
    };
    let rows = series_rows(&h);
    state.db.record_hostx(now, &rows);
    *lock(&LAST_TICK) = Some((key, Arc::new(h)));
}

/// The history keys and values for one reading (see `db/hostx.rs`).
pub fn series_rows(h: &Host) -> Vec<(String, Option<f64>)> {
    let mut r: Vec<(String, Option<f64>)> = Vec::with_capacity(64);
    let mut push = |k: String, v: Option<f64>| r.push((k, v));
    for (n, c) in &h.cores {
        push(format!("cpu.c{n}"), c.map(|c| c.busy));
    }
    push("cpu.user".into(), h.cpu.map(|c| c.user));
    push("cpu.system".into(), h.cpu.map(|c| c.system));
    push("cpu.iowait".into(), h.cpu.map(|c| c.iowait));
    push("cpu.steal".into(), h.cpu.map(|c| c.steal));
    if h.psi.is_some() {
        for (i, res) in collect::PSI_RES.iter().enumerate() {
            push(format!("psi.{res}.some"), h.psi_now[i].0);
            if *res != "cpu" {
                push(format!("psi.{res}.full"), h.psi_now[i].1);
            }
        }
    }
    if h.mem_ok {
        let m = &h.mem;
        push("mem.used".into(), Some(m.used() as f64));
        push("mem.cache".into(), Some(m.cache() as f64));
        push("mem.avail".into(), Some(m.available as f64));
        push("mem.swap".into(), Some(m.swap_used() as f64));
    }
    push("swap.in".into(), h.swap_in);
    push("swap.out".into(), h.swap_out);
    for d in &h.disks {
        push(format!("disk.{}.rd", d.name), d.rd_bps);
        push(format!("disk.{}.wr", d.name), d.wr_bps);
        push(format!("disk.{}.util", d.name), d.util);
    }
    for m in &h.mounts {
        if let Some((_, used, _)) = m.usage {
            push(format!("mount.{}", m.mnt), Some(used as f64));
        }
    }
    for n in h.nets.iter().filter(|n| !n.bridge) {
        push(format!("net.{}.rx", n.name), n.rx_bps);
        push(format!("net.{}.tx", n.name), n.tx_bps);
    }
    for s in &h.sensors {
        if s.kind == parse::SensorKind::Temp {
            push(format!("temp.{}", s.key), Some(s.value));
        }
    }
    r
}

/// `(total, used, avail)` bytes like `df -B1`, or `None` when the path is
/// not reachable (permission denied, not mounted here).
#[allow(clippy::unnecessary_cast)] // field widths differ between targets
pub(crate) fn statvfs(path: &Path) -> Option<(u64, u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` is a valid NUL-terminated string and `st` is a plain C
    // struct that statvfs fills in; it is only read after success.
    let st = unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        st
    };
    let frsize = if st.f_frsize > 0 {
        st.f_frsize as u64
    } else {
        st.f_bsize as u64
    };
    let blocks = st.f_blocks as u64;
    if blocks == 0 {
        return None;
    }
    Some((
        blocks.saturating_mul(frsize),
        blocks
            .saturating_sub(st.f_bfree as u64)
            .saturating_mul(frsize),
        (st.f_bavail as u64).saturating_mul(frsize),
    ))
}
