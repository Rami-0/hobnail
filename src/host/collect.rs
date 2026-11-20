//! Readings and rates for the /host page. A `Collector` keeps the previous
//! cumulative counters, so every `collect` gives rates over the time since
//! the last call. The history sampler and the live page each own one (see
//! `db/sampler.rs` for why rates must not share a reader).
//!
//! `ProcScan` (top processes) is separate and only runs while somebody is
//! looking at /host: it walks every pid in the host `/proc`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::parse::{self, CpuTimes, DiskIo, IfKind, Mem, NetDev, Psi, Sensor, VmStat};

/// CPU time split in percent of the time window (of all cores for the
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuSplit {
    pub user: f64,
    pub system: f64,
    pub iowait: f64,
    pub irq: f64,
    pub steal: f64,
    /// Everything but idle and iowait.
    pub busy: f64,
}

fn split(a: &CpuTimes, b: &CpuTimes) -> Option<CpuSplit> {
    let dt = b.total().checked_sub(a.total())? as f64;
    if dt <= 0.0 {
        return None;
    }
    let d = |x: u64, y: u64| y.saturating_sub(x) as f64 * 100.0 / dt;
    let idle = d(a.idle, b.idle);
    let iowait = d(a.iowait, b.iowait);
    Some(CpuSplit {
        user: d(a.user + a.nice, b.user + b.nice),
        system: d(a.system, b.system),
        iowait,
        irq: d(a.irq + a.softirq, b.irq + b.softirq),
        steal: d(a.steal, b.steal),
        busy: (100.0 - idle - iowait).clamp(0.0, 100.0),
    })
}

pub const PSI_RES: [&str; 3] = ["cpu", "memory", "io"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskView {
    pub name: String,
    pub model: Option<String>,
    pub size: Option<u64>,
    pub rotational: Option<bool>,
    /// bytes/s read, written; I/O operations per second; % busy.
    pub rd_bps: Option<f64>,
    pub wr_bps: Option<f64>,
    pub iops: Option<f64>,
    pub util: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MountView {
    pub dev: String,
    pub mnt: String,
    pub fstype: String,
    /// `(total, used, avail)` when the file system is visible here.
    pub usage: Option<(u64, u64, u64)>,
    /// Block device size when usage is not readable.
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetView {
    pub name: String,
    pub bridge: bool,
    pub speed_mbps: Option<u64>,
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    pub cur: NetDev,
    /// Errors + drops in this window (both directions).
    pub new_errs: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Host {
    /// Seconds the rates cover (0 = first reading, no rates yet).
    pub window: f64,
    pub load: [f64; 3],
    pub uptime: u64,
    pub ncores: usize,
    pub cpu: Option<CpuSplit>,
    pub cores: Vec<(u32, Option<CpuSplit>)>,
    /// `None`: no /proc/pressure on this kernel.
    pub psi: Option<[Option<Psi>; 3]>,
    /// Stall share in this window from the `total` counters: (some, full) %.
    pub psi_now: [(Option<f64>, Option<f64>); 3],
    pub mem: Mem,
    pub mem_ok: bool,
    /// Pages/s swapped in, out; major faults/s.
    pub swap_in: Option<f64>,
    pub swap_out: Option<f64>,
    pub majflt: Option<f64>,
    pub disks: Vec<DiskView>,
    pub mounts: Vec<MountView>,
    pub nets: Vec<NetView>,
    /// veth interfaces hidden from the table.
    pub veths: usize,
    pub sensors: Vec<Sensor>,
    pub gpus: Vec<Gpu>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub card: String,
    pub driver: String,
    pub busy: Option<f64>,
}

struct Reading {
    at: Instant,
    cpu: Option<CpuTimes>,
    cores: Vec<(u32, CpuTimes)>,
    psi: [Option<Psi>; 3],
    vm: VmStat,
    disks: Vec<DiskIo>,
    nets: Vec<NetDev>,
}

/// Model, size in bytes, rotational.
type DiskFacts = (Option<String>, Option<u64>, Option<bool>);

pub struct Collector {
    proc_: PathBuf,
    sys: PathBuf,
    prev: Option<Reading>,
    /// Device facts that do not change (model, size): read once.
    disk_facts: HashMap<String, DiskFacts>,
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn read_trim(p: &Path) -> Option<String> {
    let s = std::fs::read_to_string(p).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

impl Collector {
    pub fn new(host_proc: &str, host_sys: &str) -> Self {
        Self {
            proc_: PathBuf::from(host_proc),
            sys: PathBuf::from(host_sys),
            prev: None,
            disk_facts: HashMap::new(),
        }
    }

    pub fn collect(&mut self) -> Host {
        let p = &self.proc_;
        let now = Instant::now();
        let (cpu, cores) = parse::parse_stat(&read(&p.join("stat")));
        let psi_dir = p.join("pressure");
        let psi = PSI_RES.map(|r| parse::parse_pressure(&read(&psi_dir.join(r))));
        let vm = parse::parse_vmstat(&read(&p.join("vmstat")));
        let disks = parse::parse_diskstats(&read(&p.join("diskstats")));
        let nets = parse::parse_net_dev(&read(&p.join("1/net/dev")));
        let mem_s = read(&p.join("meminfo"));
        let mem = parse::parse_meminfo(&mem_s);
        let cur = Reading {
            at: now,
            cpu,
            cores,
            psi,
            vm,
            disks,
            nets,
        };
        let prev = self.prev.take();
        let dt = prev
            .as_ref()
            .map(|r| now.duration_since(r.at).as_secs_f64())
            .filter(|d| *d >= 0.2);
        let pr = prev.as_ref().filter(|_| dt.is_some());
        let rate = |a: u64, b: u64| dt.and_then(|d| b.checked_sub(a).map(|x| x as f64 / d));

        let mut h = Host {
            window: dt.unwrap_or(0.0),
            load: parse::parse_loadavg(&read(&p.join("loadavg"))),
            uptime: read(&p.join("uptime"))
                .split('.')
                .next()
                .and_then(|x| x.trim().parse().ok())
                .unwrap_or(0),
            ncores: cur.cores.len(),
            mem,
            mem_ok: mem.total > 0,
            ..Default::default()
        };
        h.cpu = pr
            .and_then(|r| r.cpu.as_ref().zip(cur.cpu.as_ref()))
            .and_then(|(a, b)| split(a, b));
        h.cores = cur
            .cores
            .iter()
            .map(|(n, b)| {
                let s = pr
                    .and_then(|r| r.cores.iter().find(|c| c.0 == *n))
                    .and_then(|(_, a)| split(a, b));
                (*n, s)
            })
            .collect();
        h.psi = psi_dir.is_dir().then_some(cur.psi);
        for (i, now_psi) in cur.psi.iter().enumerate() {
            let Some(b) = now_psi else { continue };
            let Some(a) = pr.and_then(|r| r.psi[i]) else {
                continue;
            };
            // total is in microseconds: us per second / 1e4 = percent.
            let some = rate(a.some.total, b.some.total).map(|v| (v / 1e4).clamp(0.0, 100.0));
            let full = match (a.full, b.full) {
                (Some(x), Some(y)) => rate(x.total, y.total).map(|v| (v / 1e4).clamp(0.0, 100.0)),
                _ => None,
            };
            h.psi_now[i] = (some, full);
        }
        if let Some(r) = pr {
            h.swap_in = rate(r.vm.pswpin, cur.vm.pswpin);
            h.swap_out = rate(r.vm.pswpout, cur.vm.pswpout);
            h.majflt = rate(r.vm.pgmajfault, cur.vm.pgmajfault);
        }

        // Disks
        let sys_block: Vec<String> = std::fs::read_dir(self.sys.join("block"))
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        for d in cur
            .disks
            .iter()
            .filter(|d| parse::keep_disk(&d.name, &sys_block))
        {
            let sys = &self.sys;
            let facts = self.disk_facts.entry(d.name.clone()).or_insert_with(|| {
                let b = sys.join("block").join(&d.name);
                (
                    read_trim(&b.join("device/model")),
                    read_trim(&b.join("size"))
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|s| s * 512),
                    read_trim(&b.join("queue/rotational")).map(|s| s == "1"),
                )
            });
            let mut v = DiskView {
                name: d.name.clone(),
                model: facts.0.clone(),
                size: facts.1,
                rotational: facts.2,
                ..Default::default()
            };
            if let Some(a) = pr.and_then(|r| r.disks.iter().find(|x| x.name == d.name)) {
                v.rd_bps = rate(a.rd_sectors, d.rd_sectors).map(|x| x * 512.0);
                v.wr_bps = rate(a.wr_sectors, d.wr_sectors).map(|x| x * 512.0);
                v.iops = rate(a.rd_ios + a.wr_ios, d.rd_ios + d.wr_ios);
                // io_ticks ms per second / 10 = percent.
                v.util = rate(a.io_ticks, d.io_ticks).map(|x| (x / 10.0).clamp(0.0, 100.0));
            }
            h.disks.push(v);
        }
        h.mounts = self.mounts();

        // Network
        for n in &cur.nets {
            match parse::iface_kind(&n.name) {
                IfKind::Skip => {
                    if n.name.starts_with("veth") {
                        h.veths += 1;
                    }
                    continue;
                }
                kind => {
                    let mut v = NetView {
                        name: n.name.clone(),
                        bridge: kind == IfKind::Bridge,
                        speed_mbps: read_trim(
                            &self.sys.join("class/net").join(&n.name).join("speed"),
                        )
                        .and_then(|s| s.parse::<i64>().ok())
                        .filter(|s| *s > 0)
                        .map(|s| s as u64),
                        cur: n.clone(),
                        ..Default::default()
                    };
                    if let Some(a) = pr.and_then(|r| r.nets.iter().find(|x| x.name == n.name)) {
                        v.rx_bps = rate(a.rx_bytes, n.rx_bytes);
                        v.tx_bps = rate(a.tx_bytes, n.tx_bytes);
                        let e = |x: &NetDev| x.rx_errs + x.tx_errs + x.rx_drop + x.tx_drop;
                        v.new_errs = e(n).saturating_sub(e(a));
                    }
                    h.nets.push(v);
                }
            }
        }
        h.sensors = parse::read_hwmon(&self.sys);
        h.gpus = gpus(&self.sys);
        self.prev = Some(cur);
        h
    }

    /// Host mounts with usage where this container can `statvfs` them: via
    /// `/proc/1/root/<mnt>` (needs privileges, usually denied), else through
    /// a mount of the same device inside this container (`/data`).
    fn mounts(&self) -> Vec<MountView> {
        let host = parse::parse_mounts(&read(&self.proc_.join("1/mounts")));
        let mine = parse::parse_mounts(&read(Path::new("/proc/self/mounts")));
        host.into_iter()
            .map(|m| {
                let via_root = self
                    .proc_
                    .join("1/root")
                    .join(m.mnt.trim_start_matches('/'));
                let usage = crate::host::statvfs(&via_root).or_else(|| {
                    mine.iter()
                        .find(|x| x.dev == m.dev)
                        .and_then(|x| crate::host::statvfs(Path::new(&x.mnt)))
                });
                let size = if usage.is_none() {
                    self.dev_size(&m.dev)
                } else {
                    None
                };
                MountView {
                    dev: m.dev,
                    mnt: m.mnt,
                    fstype: m.fstype,
                    usage,
                    size,
                }
            })
            .collect()
    }

    /// `/dev/sda2` or `/dev/mapper/vg-lv` -> size from /sys/class/block.
    fn dev_size(&self, dev: &str) -> Option<u64> {
        let name = dev.strip_prefix("/dev/")?;
        let name = if let Some(dm) = name.strip_prefix("mapper/") {
            std::fs::read_dir(self.sys.join("block"))
                .ok()?
                .flatten()
                .find(|e| read_trim(&e.path().join("dm/name")).as_deref() == Some(dm))?
                .file_name()
                .into_string()
                .ok()?
        } else {
            name.to_string()
        };
        read_trim(&self.sys.join("class/block").join(name).join("size"))?
            .parse::<u64>()
            .ok()
            .map(|s| s * 512)
    }
}

/// DRM cards with their driver; busy % where the driver exports it
/// (`gpu_busy_percent`: amdgpu). Intel and NVIDIA need perf or /dev access.
fn gpus(sys: &Path) -> Vec<Gpu> {
    let Ok(rd) = std::fs::read_dir(sys.join("class/drm")) else {
        return Vec::new();
    };
    let mut out: Vec<Gpu> = rd
        .flatten()
        .filter_map(|e| {
            let card = e.file_name().into_string().ok()?;
            if !card.starts_with("card") || card.contains('-') {
                return None;
            }
            let dev = e.path().join("device");
            let driver = std::fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "unknown".into());
            let busy = read_trim(&dev.join("gpu_busy_percent")).and_then(|s| s.parse().ok());
            Some(Gpu { card, driver, busy })
        })
        .collect();
    out.sort_by(|a, b| a.card.cmp(&b.card));
    out
}

// ---------------------------------------------------------------------------
// Processes

#[derive(Debug, Clone, PartialEq)]
pub struct ProcRow {
    pub pid: u32,
    pub comm: String,
    /// Full command line (only filled for the rows shown).
    pub cmd: String,
    pub uid: Option<u32>,
    /// Percent of one core (top's convention).
    pub cpu: f64,
    pub rss: u64,
    pub threads: u64,
    /// Owning container id, when the cgroup says so.
    pub cid: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcReport {
    pub top: Vec<ProcRow>,
    /// `(container id or None for the host, cpu %, rss, processes)`, by CPU.
    pub groups: Vec<(Option<String>, f64, u64, usize)>,
    /// CPU % is an average since each process started (first scan).
    pub lifetime: bool,
}

pub struct ProcScan {
    proc_: PathBuf,
    prev: HashMap<u32, (u64, u64)>,
    prev_at: Option<Instant>,
    /// `(pid, starttime)` -> container id: the cgroup of a process never
    /// changes in practice, so it is read once per process.
    cg: HashMap<(u32, u64), Option<String>>,
    hz: f64,
    page: u64,
}

impl ProcScan {
    pub fn new(host_proc: &str) -> Self {
        // SAFETY: sysconf has no preconditions.
        let (hz, page) = unsafe {
            (
                libc::sysconf(libc::_SC_CLK_TCK),
                libc::sysconf(libc::_SC_PAGESIZE),
            )
        };
        Self {
            proc_: PathBuf::from(host_proc),
            prev: HashMap::new(),
            prev_at: None,
            cg: HashMap::new(),
            hz: if hz > 0 { hz as f64 } else { 100.0 },
            page: if page > 0 { page as u64 } else { 4096 },
        }
    }

    /// Walk every pid. `uptime` (s) gives lifetime averages on the first
    /// scan, or when the previous one is older than a minute.
    pub fn scan(&mut self, uptime: u64, top_n: usize) -> ProcReport {
        let t0 = Instant::now();
        let dt = self
            .prev_at
            .map(|t| t0.duration_since(t).as_secs_f64())
            .filter(|d| *d >= 0.2 && *d <= 60.0);
        let mut rows = Vec::with_capacity(512);
        let mut next = HashMap::with_capacity(self.prev.len().max(256));
        let mut seen_cg = HashMap::with_capacity(self.cg.len().max(256));
        let mut buf = String::with_capacity(512);
        if let Ok(rd) = std::fs::read_dir(&self.proc_) {
            for e in rd.flatten() {
                let name = e.file_name();
                let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                    continue;
                };
                let dir = e.path();
                buf.clear();
                if !read_into(&dir.join("stat"), &mut buf) {
                    continue;
                }
                let Some(st) = parse::parse_proc_stat(&buf) else {
                    continue;
                };
                // Kernel threads have no memory and no container.
                if st.rss_pages == 0 && st.ticks == 0 {
                    continue;
                }
                let cpu = match (dt, self.prev.get(&pid)) {
                    (Some(d), Some(&(start, ticks))) if start == st.starttime => {
                        st.ticks.saturating_sub(ticks) as f64 / self.hz / d * 100.0
                    }
                    (Some(_), _) => {
                        // Started during the window: count it all.
                        let age = (uptime as f64 - st.starttime as f64 / self.hz).max(0.0);
                        if age < 60.0 && age > 0.0 {
                            st.ticks as f64 / self.hz / age * 100.0
                        } else {
                            0.0
                        }
                    }
                    (None, _) => {
                        let age = uptime as f64 - st.starttime as f64 / self.hz;
                        if age > 1.0 {
                            st.ticks as f64 / self.hz / age * 100.0
                        } else {
                            0.0
                        }
                    }
                };
                next.insert(pid, (st.starttime, st.ticks));
                let key = (pid, st.starttime);
                let cid = match self.cg.remove(&key) {
                    Some(c) => c,
                    None => {
                        buf.clear();
                        read_into(&dir.join("cgroup"), &mut buf);
                        parse::container_id_from_cgroup(&buf).map(str::to_string)
                    }
                };
                seen_cg.insert(key, cid.clone());
                use std::os::unix::fs::MetadataExt;
                let uid = std::fs::metadata(&dir).ok().map(|m| m.uid());
                rows.push(ProcRow {
                    pid,
                    comm: st.comm,
                    cmd: String::new(),
                    uid,
                    cpu,
                    rss: st.rss_pages * self.page,
                    threads: st.threads,
                    cid,
                });
            }
        }
        self.prev = next;
        self.cg = seen_cg;
        self.prev_at = Some(t0);

        let mut groups: Vec<(Option<String>, f64, u64, usize)> = Vec::new();
        for r in &rows {
            match groups.iter_mut().find(|g| g.0 == r.cid) {
                Some(g) => {
                    g.1 += r.cpu;
                    g.2 += r.rss;
                    g.3 += 1;
                }
                None => groups.push((r.cid.clone(), r.cpu, r.rss, 1)),
            }
        }
        groups.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.cmp(&a.2)));
        rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss.cmp(&a.rss)));
        rows.truncate(top_n);
        for r in &mut rows {
            if let Ok(b) = std::fs::read(self.proc_.join(r.pid.to_string()).join("cmdline")) {
                let s: String = String::from_utf8_lossy(&b)
                    .trim_end_matches('\0')
                    .replace('\0', " ");
                r.cmd = s.chars().take(160).collect();
            }
        }
        ProcReport {
            top: rows,
            groups,
            lifetime: dt.is_none(),
        }
    }
}

fn read_into(p: &Path, buf: &mut String) -> bool {
    use std::io::Read;
    std::fs::File::open(p)
        .and_then(|mut f| f.read_to_string(buf))
        .is_ok()
}
