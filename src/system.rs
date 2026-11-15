//! Host stats read straight from `/proc` and `/sys` (mounted read-only from
//! the host at `RH_HOST_PROC` / `RH_HOST_SYS`, default `/proc` and `/sys`).
//!
//! What is read, per sample (a handful of small files into one reused buffer):
//! - CPU: `{proc}/stat` (first `cpu` line; busy = total − idle − iowait;
//!   percent = Δbusy / Δtotal between two samples), cores from `cpuN` lines.
//! - Load: `{proc}/loadavg`. Uptime: `{proc}/uptime`.
//! - Memory: `{proc}/meminfo`; used = MemTotal − MemAvailable (like `free`).
//! - Network: `{proc}/1/net/dev`, i.e. host PID 1's network namespace (the
//!   container's own `{proc}/net/dev` would only show its bridge interface).
//!   The interface is `net_iface`, else the default-route interface from
//!   `{proc}/1/net/route`, else the busiest non-virtual one.
//! - Temperature: hwmon (`coretemp` "Package id 0" by default), else a
//!   thermal zone (`x86_pkg_temp`, else the first readable one).
//! - Disks: `statvfs(2)`, reported like `df` (see [`DiskStats::used_percent`]).
//!
//! Missing or unreadable files never fail a sample: the value becomes
//! `None`/0 and the tile shows "—".

use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::model::{DiskStats, SystemStats};

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    /// Show the system widgets row (default true).
    pub enabled: bool,
    /// Refresh interval while someone is viewing.
    pub refresh: crate::util::Dur,
    /// Paths to report disk usage for, as seen inside the container.
    /// Default: `["/data"]` shown as "/", which is the host root filesystem
    /// when `./data` lives on it.
    pub disks: Vec<DiskConfig>,
    /// Network interface for the network widget. Default: the interface
    /// carrying the default route, else the busiest non-virtual one.
    pub net_iface: Option<String>,
    /// hwmon sensor name to read the temperature from (default "coretemp",
    /// then thermal_zone fallbacks).
    pub temp_sensor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskConfig {
    pub path: String,
    pub label: Option<String>,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh: crate::util::Dur::secs(3),
            disks: vec![],
            net_iface: None,
            temp_sensor: None,
        }
    }
}

impl DiskStats {
    /// Use% like `df`: used / (used + available to unprivileged users), so
    /// the blocks reserved for root count as neither.
    pub fn used_percent(&self) -> Option<f64> {
        let denom = self.used.saturating_add(self.avail);
        (denom > 0).then(|| self.used as f64 * 100.0 / denom as f64)
    }
}

/// Rates over a longer gap than this are not shown (the first sample after
/// nobody watched for a while would otherwise be a long average).
const MAX_RATE_GAP: Duration = Duration::from_secs(120);
/// Samples closer together than this reuse the previous rate instead of
/// computing one over a few milliseconds (a page render and the first live
/// refresh can land back to back, which used to show "0 B/s").
#[cfg(not(test))]
const MIN_RATE_GAP: Duration = Duration::from_secs(1);
#[cfg(test)]
const MIN_RATE_GAP: Duration = Duration::from_millis(20);
/// How often to rescan for a temperature sensor when none was found.
const TEMP_RESCAN: Duration = Duration::from_secs(60);

/// hwmon driver names tried in order when `temp_sensor` is not set.
const DEFAULT_SENSORS: &[&str] = &["coretemp", "k10temp", "zenpower", "cpu_thermal"];
/// Labels that mean "whole CPU package" in the drivers above.
const PACKAGE_LABELS: &[&str] = &["Package id 0", "Tctl", "Tdie"];
/// Interface name prefixes that are never the "real" uplink.
const VIRTUAL_IFACES: &[&str] = &[
    "lo",
    "docker",
    "br-",
    "veth",
    "virbr",
    "vnet",
    "tailscale",
    "tun",
    "tap",
    "wg",
    "zt",
    "cni",
    "flannel",
    "cali",
    "kube",
];

#[derive(Debug, Clone, Copy)]
struct CpuPrev {
    busy: u64,
    total: u64,
    at: Instant,
}

#[derive(Debug, Clone)]
struct NetPrev {
    iface: String,
    rx: u64,
    tx: u64,
    at: Instant,
}

#[derive(Debug)]
struct TempCache {
    sensor: Option<String>,
    path: Option<PathBuf>,
    at: Instant,
}

/// Keeps the previous CPU/network counters so rates can be computed.
#[derive(Debug)]
pub struct Sampler {
    proc: PathBuf,
    sys: PathBuf,
    buf: String,
    cpu: Option<CpuPrev>,
    net: Option<NetPrev>,
    temp: Option<TempCache>,
    /// Rates from the last real computation (reused for back-to-back samples).
    last_cpu: Option<f64>,
    last_net: (Option<f64>, Option<f64>),
}

impl Sampler {
    pub fn new(host_proc: &str, host_sys: &str) -> Self {
        Self {
            proc: PathBuf::from(host_proc),
            sys: PathBuf::from(host_sys),
            buf: String::with_capacity(4096),
            cpu: None,
            net: None,
            temp: None,
            last_cpu: None,
            last_net: (None, None),
        }
    }

    /// Take a sample. Must be cheap (a handful of small file reads) and never
    /// panic on missing files: missing data becomes `None`/0.
    pub fn sample(&mut self, cfg: &SystemConfig) -> SystemStats {
        let now = Instant::now();
        let mut s = SystemStats {
            sampled_at: crate::util::unix_now(),
            ..Default::default()
        };

        if read_into(&self.proc.join("stat"), &mut self.buf) {
            if let Some((busy, total, cores)) = parse_cpu(&self.buf) {
                s.cpu_cores = cores;
                if self.cpu.is_some_and(|p| now - p.at < MIN_RATE_GAP) {
                    s.cpu_percent = self.last_cpu;
                } else {
                    if let Some(p) = self.cpu.filter(|p| now - p.at <= MAX_RATE_GAP) {
                        let dt = total.saturating_sub(p.total);
                        if dt > 0 && total >= p.total && busy >= p.busy {
                            let pct = (busy - p.busy) as f64 * 100.0 / dt as f64;
                            s.cpu_percent = Some(pct.clamp(0.0, 100.0));
                        }
                    }
                    self.last_cpu = s.cpu_percent;
                    self.cpu = Some(CpuPrev {
                        busy,
                        total,
                        at: now,
                    });
                }
            }
        }

        if read_into(&self.proc.join("loadavg"), &mut self.buf) {
            s.load = parse_loadavg(&self.buf);
        }
        if read_into(&self.proc.join("uptime"), &mut self.buf) {
            s.uptime_secs = parse_uptime(&self.buf);
        }
        if read_into(&self.proc.join("meminfo"), &mut self.buf) {
            let m = parse_meminfo(&self.buf);
            s.mem_total = m.total;
            s.mem_used = m.total.saturating_sub(m.available);
            s.swap_total = m.swap_total;
            s.swap_used = m.swap_total.saturating_sub(m.swap_free);
        }

        self.sample_net(cfg, now, &mut s);
        s.temp_c = self.sample_temp(cfg, now);
        s.disks = sample_disks(cfg);
        s
    }

    fn sample_net(&mut self, cfg: &SystemConfig, now: Instant, s: &mut SystemStats) {
        let iface = match cfg.net_iface.clone() {
            Some(i) => Some(i),
            None => {
                let route = if read_into(&self.proc.join("1/net/route"), &mut self.buf) {
                    default_route_iface(&self.buf)
                } else {
                    None
                };
                if !read_into(&self.proc.join("1/net/dev"), &mut self.buf) {
                    self.net = None;
                    return;
                }
                route.or_else(|| busiest_iface(&self.buf))
            }
        };
        let Some(iface) = iface else {
            self.net = None;
            return;
        };
        if cfg.net_iface.is_some() && !read_into(&self.proc.join("1/net/dev"), &mut self.buf) {
            self.net = None;
            s.net_iface = Some(iface);
            return;
        }
        let Some((rx, tx)) = net_counters(&self.buf, &iface) else {
            self.net = None;
            s.net_iface = Some(iface);
            return;
        };
        if let Some(p) = &self.net {
            if p.iface == iface && now - p.at < MIN_RATE_GAP {
                (s.net_rx_bps, s.net_tx_bps) = self.last_net;
                s.net_iface = Some(iface);
                return;
            }
            let dt = (now - p.at).as_secs_f64();
            if p.iface == iface
                && dt > 0.0
                && now - p.at <= MAX_RATE_GAP
                && rx >= p.rx
                && tx >= p.tx
            {
                s.net_rx_bps = Some((rx - p.rx) as f64 / dt);
                s.net_tx_bps = Some((tx - p.tx) as f64 / dt);
            }
        }
        self.last_net = (s.net_rx_bps, s.net_tx_bps);
        self.net = Some(NetPrev {
            iface: iface.clone(),
            rx,
            tx,
            at: now,
        });
        s.net_iface = Some(iface);
    }

    fn sample_temp(&mut self, cfg: &SystemConfig, now: Instant) -> Option<f64> {
        let stale = match &self.temp {
            None => true,
            Some(c) => {
                c.sensor != cfg.temp_sensor || (c.path.is_none() && now - c.at >= TEMP_RESCAN)
            }
        };
        if stale {
            self.temp = Some(TempCache {
                sensor: cfg.temp_sensor.clone(),
                path: find_temp_input(&self.sys, cfg.temp_sensor.as_deref()),
                at: now,
            });
        }
        let path = self.temp.as_ref()?.path.clone()?;
        let t = if read_into(&path, &mut self.buf) {
            parse_millidegrees(&self.buf)
        } else {
            None
        };
        if t.is_none() {
            // Sensor vanished or stopped answering: rescan next time.
            self.temp = None;
        }
        t
    }
}

/// Read a whole (small) file into `buf`. False if it cannot be read.
fn read_into(path: &Path, buf: &mut String) -> bool {
    buf.clear();
    match std::fs::File::open(path) {
        Ok(mut f) => f.read_to_string(buf).is_ok(),
        Err(_) => false,
    }
}

/// `(busy, total, cores)` from `/proc/stat`.
fn parse_cpu(stat: &str) -> Option<(u64, u64, u32)> {
    let mut agg = None;
    let mut cores = 0u32;
    for line in stat.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            // The cpu lines come first; stop at the first other line.
            if agg.is_some() {
                break;
            }
            continue;
        };
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            cores += 1;
        } else if rest.starts_with(' ') {
            // user nice system idle iowait irq softirq steal (guest* are
            // already included in user/nice, so they are not added again).
            let v: Vec<u64> = rest
                .split_ascii_whitespace()
                .take(8)
                .map(|x| x.parse().unwrap_or(0))
                .collect();
            if v.len() < 4 {
                return None;
            }
            let total: u64 = v.iter().sum();
            let idle = v[3] + v.get(4).copied().unwrap_or(0);
            agg = Some((total.saturating_sub(idle), total));
        }
    }
    agg.map(|(b, t)| (b, t, cores))
}

fn parse_loadavg(s: &str) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (o, v) in out.iter_mut().zip(s.split_ascii_whitespace()) {
        *o = v.parse().unwrap_or(0.0);
    }
    out
}

fn parse_uptime(s: &str) -> u64 {
    s.split_ascii_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v as u64)
        .unwrap_or(0)
}

#[derive(Debug, Default, PartialEq)]
struct MemInfo {
    total: u64,
    available: u64,
    swap_total: u64,
    swap_free: u64,
}

fn parse_meminfo(s: &str) -> MemInfo {
    let (mut m, mut have_avail) = (MemInfo::default(), false);
    let (mut free, mut buffers, mut cached) = (0, 0, 0);
    for line in s.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let kb: u64 = v
            .split_ascii_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let bytes = kb.saturating_mul(1024);
        match k {
            "MemTotal" => m.total = bytes,
            "MemAvailable" => {
                m.available = bytes;
                have_avail = true;
            }
            "MemFree" => free = bytes,
            "Buffers" => buffers = bytes,
            "Cached" => cached = bytes,
            "SwapTotal" => m.swap_total = bytes,
            "SwapFree" => m.swap_free = bytes,
            _ => {}
        }
    }
    if !have_avail {
        // Kernels before 3.14 have no MemAvailable.
        m.available = free + buffers + cached;
    }
    m
}

/// The interface of the default IPv4 route with the lowest metric.
fn default_route_iface(route: &str) -> Option<String> {
    route
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_ascii_whitespace().collect();
            let flags = u32::from_str_radix(f.get(3)?, 16).ok()?;
            let up = flags & 1 != 0;
            let metric: u64 = f.get(6)?.parse().ok()?;
            (*f.get(1)? == "00000000" && *f.get(7)? == "00000000" && up)
                .then(|| (metric, f[0].to_string()))
        })
        .min()
        .map(|(_, i)| i)
}

fn is_virtual(iface: &str) -> bool {
    VIRTUAL_IFACES.iter().any(|p| iface.starts_with(p))
}

/// Iterate `(iface, rx_bytes, tx_bytes)` over `/proc/net/dev`.
fn net_dev_rows(dev: &str) -> impl Iterator<Item = (&str, u64, u64)> {
    dev.lines().skip(2).filter_map(|l| {
        let (name, rest) = l.split_once(':')?;
        let mut f = rest.split_ascii_whitespace();
        let rx = f.next()?.parse().ok()?;
        let tx = f.nth(7)?.parse().ok()?;
        Some((name.trim(), rx, tx))
    })
}

fn net_counters(dev: &str, iface: &str) -> Option<(u64, u64)> {
    net_dev_rows(dev)
        .find(|(n, _, _)| *n == iface)
        .map(|(_, rx, tx)| (rx, tx))
}

fn busiest_iface(dev: &str) -> Option<String> {
    net_dev_rows(dev)
        .filter(|(n, _, _)| !is_virtual(n))
        .max_by_key(|(_, rx, tx)| rx.saturating_add(*tx))
        .filter(|(_, rx, tx)| rx.saturating_add(*tx) > 0)
        .map(|(n, _, _)| n.to_string())
}

fn parse_millidegrees(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse::<i64>().ok()? as f64 / 1000.0;
    (-50.0..=200.0).contains(&v).then_some(v)
}

/// Sorted directory entries whose names start with `prefix`.
fn dir_entries(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut v: Vec<(u64, String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n = name.strip_prefix(prefix)?;
            // Numeric order (hwmon10 after hwmon2) where the suffix allows it.
            let num = n
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(u64::MAX);
            Some((num, name, e.path()))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, _, p)| p).collect()
}

fn read_trim(path: &Path) -> Option<String> {
    let mut s = String::new();
    read_into(path, &mut s).then(|| s.trim().to_string())
}

/// Find the `temp*_input` file to read. See the module docs for the order.
fn find_temp_input(sys: &Path, sensor: Option<&str>) -> Option<PathBuf> {
    let hwmons = dir_entries(&sys.join("class/hwmon"), "hwmon");
    let wanted: Vec<&str> = match sensor {
        Some(s) => vec![s],
        None => DEFAULT_SENSORS.to_vec(),
    };
    for want in &wanted {
        for dir in &hwmons {
            if read_trim(&dir.join("name")).as_deref() == Some(*want) {
                if let Some(p) = hwmon_input(dir) {
                    return Some(p);
                }
            }
        }
    }
    let zones = dir_entries(&sys.join("class/thermal"), "thermal_zone");
    let readable = |z: &PathBuf| {
        let p = z.join("temp");
        read_trim(&p)
            .and_then(|s| parse_millidegrees(&s))
            .map(|_| p)
    };
    let want_zone = sensor.unwrap_or("x86_pkg_temp");
    if let Some(p) = zones
        .iter()
        .filter(|z| read_trim(&z.join("type")).as_deref() == Some(want_zone))
        .find_map(readable)
    {
        return Some(p);
    }
    if sensor.is_some() {
        // An explicit sensor that does not exist shows "—" rather than
        // silently reading something else.
        return None;
    }
    zones.iter().find_map(readable)
}

/// In one hwmon dir: the package sensor if labelled, else temp1, else the first.
fn hwmon_input(dir: &Path) -> Option<PathBuf> {
    let inputs: Vec<PathBuf> = dir_entries(dir, "temp")
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_input"))
        })
        .collect();
    let label_of = |input: &PathBuf| {
        let name = input.file_name()?.to_str()?.strip_suffix("_input")?;
        read_trim(&dir.join(format!("{name}_label")))
    };
    inputs
        .iter()
        .find(|p| label_of(p).is_some_and(|l| PACKAGE_LABELS.contains(&l.as_str())))
        .or_else(|| inputs.iter().find(|p| p.ends_with("temp1_input")))
        .or_else(|| inputs.first())
        .cloned()
}

fn sample_disks(cfg: &SystemConfig) -> Vec<DiskStats> {
    let list: Vec<(String, String)> = if cfg.disks.is_empty() {
        let path = if Path::new("/data").exists() {
            "/data"
        } else {
            "/"
        };
        vec![(path.to_string(), "/".to_string())]
    } else {
        cfg.disks
            .iter()
            .map(|d| {
                (
                    d.path.clone(),
                    d.label.clone().unwrap_or_else(|| d.path.clone()),
                )
            })
            .collect()
    };
    list.into_iter()
        .map(|(path, label)| {
            let (total, used, avail) = statvfs(&path).unwrap_or((0, 0, 0));
            DiskStats {
                label,
                total,
                used,
                avail,
            }
        })
        .collect()
}

/// `(total, used, avail)` bytes like `df -B1`.
#[allow(clippy::unnecessary_cast)] // field widths differ between targets
fn statvfs(path: &str) -> Option<(u64, u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: `c` is a valid NUL-terminated string and `st` is a plain C
    // struct that statvfs fills in; we only read it after success.
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
    let bfree = st.f_bfree as u64;
    let bavail = st.f_bavail as u64;
    Some((
        blocks.saturating_mul(frsize),
        blocks.saturating_sub(bfree).saturating_mul(frsize),
        bavail.saturating_mul(frsize),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT1: &str = "cpu  100 0 50 800 50 0 0 0 0 0\n\
cpu0 50 0 25 400 25 0 0 0 0 0\n\
cpu1 50 0 25 400 25 0 0 0 0 0\n\
intr 12345 0 0\nctxt 999\n";
    const STAT2: &str = "cpu  160 0 70 900 70 0 0 0 0 0\n\
cpu0 80 0 35 450 35 0 0 0 0 0\n\
cpu1 80 0 35 450 35 0 0 0 0 0\n";

    const MEMINFO: &str = "MemTotal:        7499412 kB\n\
MemFree:         1127188 kB\n\
MemAvailable:    3729644 kB\n\
Buffers:          138104 kB\n\
Cached:          2080000 kB\n\
SwapTotal:      20971516 kB\n\
SwapFree:       14905404 kB\n";

    const ROUTE: &str =
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n\
wlp2s0\t00000000\t0108A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
enp1s0\t00000000\t0108A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n";

    fn net_dev(rx: u64, tx: u64) -> String {
        format!(
            "Inter-|   Receive                                                |  Transmit\n \
face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
lo: 999999999 1 0 0 0 0 0 0 999999999 1 0 0 0 0 0 0\n\
enp1s0: {rx} 44030545 0 70 0 0 0 934184 {tx} 30786311 250 16 0 0 0 0\n\
docker0: 88888888888 1 0 0 0 0 0 0 1 1 0 0 0 0 0 0\n\
veth1f5102b: 77777777777 1 0 0 0 0 0 0 1 1 0 0 0 0 0 0\n"
        )
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rh-sys-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn write(&self, rel: &str, content: &str) {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        fn proc(&self) -> String {
            self.root.join("proc").display().to_string()
        }
        fn sys(&self) -> String {
            self.root.join("sys").display().to_string()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn host_like(name: &str) -> Fixture {
        let f = Fixture::new(name);
        f.write("proc/stat", STAT1);
        f.write("proc/loadavg", "3.67 3.65 3.39 10/1652 3740590\n");
        f.write("proc/uptime", "1820698.99 5144058.93\n");
        f.write("proc/meminfo", MEMINFO);
        f.write("proc/1/net/route", ROUTE);
        f.write("proc/1/net/dev", &net_dev(1000, 500));
        // The container's own netns must not be used.
        f.write("proc/net/dev", &net_dev(1, 1));
        f.write("sys/class/hwmon/hwmon0/name", "ADP0\n");
        f.write("sys/class/hwmon/hwmon2/name", "pch_skylake_0\n");
        f.write("sys/class/hwmon/hwmon2/temp1_input", "46000\n");
        f.write("sys/class/hwmon/hwmon3/name", "coretemp\n");
        f.write("sys/class/hwmon/hwmon3/temp1_input", "59000\n");
        f.write("sys/class/hwmon/hwmon3/temp1_label", "Package id 0\n");
        f.write("sys/class/hwmon/hwmon3/temp2_input", "55000\n");
        f.write("sys/class/hwmon/hwmon3/temp2_label", "Core 0\n");
        f.write("sys/class/thermal/thermal_zone0/type", "x86_pkg_temp\n");
        f.write("sys/class/thermal/thermal_zone0/temp", "68000\n");
        f
    }

    #[test]
    fn parses_cpu() {
        assert_eq!(parse_cpu(STAT1), Some((150, 1000, 2)));
        assert_eq!(parse_cpu("garbage"), None);
        assert_eq!(parse_cpu(""), None);
    }

    #[test]
    fn parses_meminfo() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.total, 7_679_397_888);
        assert_eq!(m.available, 3_819_155_456);
        assert_eq!(m.swap_total - m.swap_free, 6_211_698_688);
        // No MemAvailable: free + buffers + cached.
        let old = parse_meminfo("MemTotal: 100 kB\nMemFree: 10 kB\nBuffers: 5 kB\nCached: 20 kB\n");
        assert_eq!(old.available, 35 * 1024);
    }

    #[test]
    fn parses_route_and_dev() {
        assert_eq!(default_route_iface(ROUTE).as_deref(), Some("enp1s0"));
        assert_eq!(default_route_iface("Iface\tDestination\n"), None);
        let dev = net_dev(36_599_129_781, 17_075_613_354);
        assert_eq!(
            net_counters(&dev, "enp1s0"),
            Some((36_599_129_781, 17_075_613_354))
        );
        assert_eq!(net_counters(&dev, "eth9"), None);
        // lo, docker0 and veth* are skipped even though they are busier.
        assert_eq!(busiest_iface(&dev).as_deref(), Some("enp1s0"));
    }

    #[test]
    fn small_parsers() {
        assert_eq!(
            parse_loadavg("3.67 3.65 3.39 10/1652 1"),
            [3.67, 3.65, 3.39]
        );
        assert_eq!(parse_loadavg(""), [0.0; 3]);
        assert_eq!(parse_uptime("1820698.99 5144058.93\n"), 1_820_698);
        assert_eq!(parse_uptime("x"), 0);
        assert_eq!(parse_millidegrees("59000\n"), Some(59.0));
        assert_eq!(parse_millidegrees("-1"), Some(-0.001));
        assert_eq!(parse_millidegrees("999999"), None);
    }

    #[test]
    fn first_sample_then_rates() {
        let f = host_like("rates");
        let mut s = Sampler::new(&f.proc(), &f.sys());
        let cfg = SystemConfig::default();

        let a = s.sample(&cfg);
        assert_eq!(a.cpu_percent, None, "no CPU rate on the first sample");
        assert_eq!(a.net_rx_bps, None);
        assert_eq!(a.net_tx_bps, None);
        assert_eq!(a.cpu_cores, 2);
        assert_eq!(a.load, [3.67, 3.65, 3.39]);
        assert_eq!(a.uptime_secs, 1_820_698);
        assert_eq!(a.mem_total, 7_679_397_888);
        assert_eq!(a.mem_used, 7_679_397_888 - 3_819_155_456);
        assert_eq!(a.swap_used, 6_211_698_688);
        assert_eq!(a.net_iface.as_deref(), Some("enp1s0"));
        assert_eq!(a.temp_c, Some(59.0));
        assert_eq!(a.disks.len(), 1);
        assert_eq!(a.disks[0].label, "/");
        assert!(a.disks[0].total > 0);

        f.write("proc/stat", STAT2);
        f.write("proc/1/net/dev", &net_dev(1000 + 4000, 500 + 2000));
        std::thread::sleep(Duration::from_millis(50));
        let b = s.sample(&cfg);
        // Δbusy = (160+70) - (100+50) = 80, Δtotal = 1200 - 1000 = 200.
        assert_eq!(b.cpu_percent, Some(40.0));
        let rx = b.net_rx_bps.unwrap();
        let tx = b.net_tx_bps.unwrap();
        assert!(rx > 0.0 && rx < 4000.0 / 0.05 * 1.01, "{rx}");
        assert!((rx / tx - 2.0).abs() < 1e-9);

        // A sample right after another reuses the last rates (no "0 B/s").
        let c = s.sample(&cfg);
        assert_eq!(c.cpu_percent, Some(40.0));
        assert_eq!(c.net_rx_bps, Some(rx));
        assert_eq!(c.net_tx_bps, Some(tx));
    }

    #[test]
    fn counter_reset_gives_no_rate() {
        let f = host_like("reset");
        let mut s = Sampler::new(&f.proc(), &f.sys());
        let cfg = SystemConfig::default();
        s.sample(&cfg);
        f.write("proc/1/net/dev", &net_dev(10, 10));
        let b = s.sample(&cfg);
        assert_eq!(b.net_rx_bps, None);
    }

    #[test]
    fn missing_everything_is_not_a_panic() {
        let mut s = Sampler::new("/nonexistent/proc", "/nonexistent/sys");
        let cfg = SystemConfig {
            disks: vec![DiskConfig {
                path: "/nonexistent/disk".into(),
                label: None,
            }],
            ..Default::default()
        };
        let a = s.sample(&cfg);
        let b = s.sample(&cfg);
        for x in [a, b] {
            assert_eq!(x.cpu_percent, None);
            assert_eq!(x.mem_total, 0);
            assert_eq!(x.temp_c, None);
            assert_eq!(x.net_iface, None);
            assert_eq!(x.disks[0].label, "/nonexistent/disk");
            assert_eq!(x.disks[0].total, 0);
        }
    }

    #[test]
    fn explicit_iface_and_busiest_fallback() {
        let f = host_like("iface");
        let mut s = Sampler::new(&f.proc(), &f.sys());
        let cfg = SystemConfig {
            net_iface: Some("docker0".into()),
            ..Default::default()
        };
        assert_eq!(s.sample(&cfg).net_iface.as_deref(), Some("docker0"));
        // No default route: busiest non-virtual interface.
        std::fs::remove_file(f.root.join("proc/1/net/route")).unwrap();
        let a = s.sample(&SystemConfig::default());
        assert_eq!(a.net_iface.as_deref(), Some("enp1s0"));
        // Unknown interface: name kept, no rates.
        let cfg = SystemConfig {
            net_iface: Some("eth7".into()),
            ..Default::default()
        };
        let x = s.sample(&cfg);
        assert_eq!(x.net_iface.as_deref(), Some("eth7"));
        assert_eq!(x.net_rx_bps, None);
    }

    #[test]
    fn temperature_fallbacks() {
        let f = host_like("temp");
        // Explicit hwmon sensor without labels: temp1.
        let cfg = SystemConfig {
            temp_sensor: Some("pch_skylake_0".into()),
            ..Default::default()
        };
        let mut s = Sampler::new(&f.proc(), &f.sys());
        assert_eq!(s.sample(&cfg).temp_c, Some(46.0));
        // Explicit sensor that does not exist: none.
        let cfg = SystemConfig {
            temp_sensor: Some("nope".into()),
            ..Default::default()
        };
        assert_eq!(s.sample(&cfg).temp_c, None);
        // No coretemp: thermal zone x86_pkg_temp.
        std::fs::remove_dir_all(f.root.join("sys/class/hwmon/hwmon3")).unwrap();
        let mut s = Sampler::new(&f.proc(), &f.sys());
        assert_eq!(s.sample(&SystemConfig::default()).temp_c, Some(68.0));
        // Other zone types only: the first readable zone.
        f.write("sys/class/thermal/thermal_zone0/type", "acpitz\n");
        f.write("sys/class/thermal/thermal_zone1/type", "iwlwifi_1\n");
        let mut s = Sampler::new(&f.proc(), &f.sys());
        assert_eq!(s.sample(&SystemConfig::default()).temp_c, Some(68.0));
    }

    #[test]
    fn disk_percent_like_df() {
        let d = DiskStats {
            label: "/".into(),
            total: 468_191_526_912,
            used: 84_863_246_336,
            avail: 359_470_243_840,
        };
        let p = d.used_percent().unwrap();
        assert!((p - 19.1).abs() < 0.05, "{p}");
        assert_eq!(DiskStats::default().used_percent(), None);
        let (total, used, avail) = statvfs("/").unwrap();
        assert!(total > 0 && used <= total && avail <= total);
    }
}
