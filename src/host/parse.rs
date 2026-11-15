//! Pure parsers for the host `/proc` and `/sys` files the /host page reads.
//! Every function takes the file text (or a directory) and never panics on
//! odd input: unknown lines are skipped, missing fields read as zero.

use std::path::Path;

/// One `cpu` / `cpuN` line of `/proc/stat`, in clock ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    /// Guest time is already inside user/nice, so it is not added again.
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }
}

fn cpu_line(rest: &str) -> CpuTimes {
    let mut f = rest
        .split_ascii_whitespace()
        .map(|x| x.parse::<u64>().unwrap_or(0));
    let mut n = || f.next().unwrap_or(0);
    CpuTimes {
        user: n(),
        nice: n(),
        system: n(),
        idle: n(),
        iowait: n(),
        irq: n(),
        softirq: n(),
        steal: n(),
    }
}

/// `/proc/stat`: the aggregate line and one entry per core, in core order
/// (`cpu0`, `cpu1`, ...; an offline core leaves no line and is skipped).
pub fn parse_stat(s: &str) -> (Option<CpuTimes>, Vec<(u32, CpuTimes)>) {
    let mut total = None;
    let mut cores = Vec::new();
    for line in s.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            continue;
        };
        if let Some(r) = rest.strip_prefix(' ') {
            total = Some(cpu_line(r));
        } else if let Some((n, r)) = rest.split_once(' ') {
            if let Ok(n) = n.parse::<u32>() {
                cores.push((n, cpu_line(r)));
            }
        }
    }
    cores.sort_by_key(|c| c.0);
    (total, cores)
}

/// One line of a `/proc/pressure/*` file.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PsiLine {
    /// avg10, avg60, avg300: percent of wall time.
    pub avg: [f64; 3],
    /// Cumulative stall time in microseconds.
    pub total: u64,
}

/// A `/proc/pressure/{cpu,memory,io}` file. `full` is missing on old kernels
/// for cpu (and is all zero there on newer ones).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Psi {
    pub some: PsiLine,
    pub full: Option<PsiLine>,
}

pub fn parse_pressure(s: &str) -> Option<Psi> {
    let mut some = None;
    let mut full = None;
    for line in s.lines() {
        let mut it = line.split_ascii_whitespace();
        let kind = it.next();
        let mut l = PsiLine::default();
        for kv in it {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            match k {
                "avg10" => l.avg[0] = v.parse().unwrap_or(0.0),
                "avg60" => l.avg[1] = v.parse().unwrap_or(0.0),
                "avg300" => l.avg[2] = v.parse().unwrap_or(0.0),
                "total" => l.total = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        match kind {
            Some("some") => some = Some(l),
            Some("full") => full = Some(l),
            _ => {}
        }
    }
    Some(Psi { some: some?, full })
}

/// `/proc/meminfo`, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mem {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub slab: u64,
    pub sreclaimable: u64,
    pub shmem: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_cached: u64,
    pub dirty: u64,
}

impl Mem {
    /// Page cache as `free` shows it: Cached + SReclaimable.
    pub fn cache(&self) -> u64 {
        self.cached + self.sreclaimable
    }
    /// `free`'s "used": total - free - buffers - cache.
    pub fn used(&self) -> u64 {
        self.total
            .saturating_sub(self.free)
            .saturating_sub(self.buffers)
            .saturating_sub(self.cache())
    }
    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }
}

pub fn parse_meminfo(s: &str) -> Mem {
    let mut m = Mem::default();
    for line in s.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let kb: u64 = v
            .split_ascii_whitespace()
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        let b = kb.saturating_mul(1024);
        match k {
            "MemTotal" => m.total = b,
            "MemFree" => m.free = b,
            "MemAvailable" => m.available = b,
            "Buffers" => m.buffers = b,
            "Cached" => m.cached = b,
            "Slab" => m.slab = b,
            "SReclaimable" => m.sreclaimable = b,
            "Shmem" => m.shmem = b,
            "SwapTotal" => m.swap_total = b,
            "SwapFree" => m.swap_free = b,
            "SwapCached" => m.swap_cached = b,
            "Dirty" => m.dirty = b,
            _ => {}
        }
    }
    m
}

/// The `/proc/vmstat` counters the page shows (pages, cumulative).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmStat {
    pub pswpin: u64,
    pub pswpout: u64,
    pub pgmajfault: u64,
}

pub fn parse_vmstat(s: &str) -> VmStat {
    let mut v = VmStat::default();
    for line in s.lines() {
        let Some((k, n)) = line.split_once(' ') else {
            continue;
        };
        let n = n.trim().parse().unwrap_or(0);
        match k {
            "pswpin" => v.pswpin = n,
            "pswpout" => v.pswpout = n,
            "pgmajfault" => v.pgmajfault = n,
            _ => {}
        }
    }
    v
}

/// One `/proc/diskstats` line (sectors are always 512 bytes there).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskIo {
    pub name: String,
    pub rd_ios: u64,
    pub rd_sectors: u64,
    pub wr_ios: u64,
    pub wr_sectors: u64,
    /// Milliseconds the device had I/O in flight (`%util` source).
    pub io_ticks: u64,
}

pub fn parse_diskstats(s: &str) -> Vec<DiskIo> {
    s.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_ascii_whitespace().collect();
            if f.len() < 14 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            Some(DiskIo {
                name: f[2].to_string(),
                rd_ios: n(3),
                rd_sectors: n(5),
                wr_ios: n(7),
                wr_sectors: n(9),
                io_ticks: n(12),
            })
        })
        .collect()
}

/// Whether a block device is worth its own I/O row: whole physical disks
/// (and md arrays), not partitions, loop, ram, zram, optical or
/// device-mapper volumes (whose I/O is already counted on the disk below).
/// `sys_block` lists `/sys/block` (whole devices only); when it is empty the
/// name alone decides.
pub fn keep_disk(name: &str, sys_block: &[String]) -> bool {
    const SKIP: [&str; 7] = ["loop", "ram", "zram", "sr", "fd", "dm-", "nbd"];
    if SKIP.iter().any(|p| name.starts_with(p)) {
        return false;
    }
    if !sys_block.is_empty() {
        return sys_block.iter().any(|b| b == name);
    }
    // No /sys: sda1 / nvme0n1p1 / mmcblk0p1 are partitions.
    let last_digit = name.ends_with(|c: char| c.is_ascii_digit());
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        return !name.contains('p') || !last_digit;
    }
    !(last_digit && (name.starts_with("sd") || name.starts_with("vd") || name.starts_with("xvd")))
}

/// A real file system from `/proc/1/mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub dev: String,
    pub mnt: String,
    pub fstype: String,
}

/// Mount table text -> real file systems: backed by a `/dev/` node, not a
/// pseudo, overlay, tmpfs or squashfs (snap) mount, one entry per device
/// (the shortest mount point wins, so bind mounts of a sub-directory and
/// Docker's per-container binds are dropped).
pub fn parse_mounts(s: &str) -> Vec<Mount> {
    const SKIP_FS: [&str; 9] = [
        "squashfs", "overlay", "tmpfs", "devtmpfs", "iso9660", "udf", "nsfs", "ramfs", "autofs",
    ];
    let mut out: Vec<Mount> = Vec::new();
    for line in s.lines() {
        let mut f = line.split_ascii_whitespace();
        let (Some(dev), Some(mnt), Some(fstype)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if !dev.starts_with("/dev/") || dev.starts_with("/dev/loop") {
            continue;
        }
        if SKIP_FS.contains(&fstype) || fstype.starts_with("fuse.") {
            continue;
        }
        let mnt = unescape_mount(mnt);
        if mnt.starts_with("/var/lib/docker/") || mnt.starts_with("/snap/") {
            continue;
        }
        match out.iter_mut().find(|m| m.dev == dev) {
            Some(m) => {
                if mnt.len() < m.mnt.len() {
                    m.mnt = mnt;
                }
            }
            None => out.push(Mount {
                dev: dev.to_string(),
                mnt,
                fstype: fstype.to_string(),
            }),
        }
    }
    out.sort_by(|a, b| a.mnt.cmp(&b.mnt));
    out
}

/// Mount tables escape space, tab, newline and backslash as `\ooo`.
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 4])
                .ok()
                .and_then(|o| u8::from_str_radix(o, 8).ok())
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One interface line of `/proc/net/dev`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetDev {
    pub name: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errs: u64,
    pub rx_drop: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errs: u64,
    pub tx_drop: u64,
}

pub fn parse_net_dev(s: &str) -> Vec<NetDev> {
    s.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let f: Vec<u64> = rest
                .split_ascii_whitespace()
                .map(|x| x.parse().unwrap_or(0))
                .collect();
            if f.len() < 12 {
                return None;
            }
            Some(NetDev {
                name: name.trim().to_string(),
                rx_bytes: f[0],
                rx_packets: f[1],
                rx_errs: f[2],
                rx_drop: f[3],
                tx_bytes: f[8],
                tx_packets: f[9],
                tx_errs: f[10],
                tx_drop: f[11],
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfKind {
    /// A physical or VPN interface: own row and history.
    Real,
    /// A Docker or Linux bridge: grouped into one row.
    Bridge,
    /// Loopback and container veth ends: not shown.
    Skip,
}

pub fn iface_kind(name: &str) -> IfKind {
    if name == "lo" || name.starts_with("veth") {
        IfKind::Skip
    } else if name == "docker0" || name.starts_with("br-") || name.starts_with("virbr") {
        IfKind::Bridge
    } else {
        IfKind::Real
    }
}

/// The fields of `/proc/<pid>/stat` the process table needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcStat {
    pub comm: String,
    pub state: char,
    /// utime + stime, clock ticks.
    pub ticks: u64,
    /// Clock ticks after boot.
    pub starttime: u64,
    pub threads: u64,
    /// Resident pages.
    pub rss_pages: u64,
}

/// `comm` may contain spaces and parentheses, so split at the last `)`.
pub fn parse_proc_stat(s: &str) -> Option<ProcStat> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close < open {
        return None;
    }
    let comm = s[open + 1..close].to_string();
    let f: Vec<&str> = s[close + 1..].split_ascii_whitespace().collect();
    // f[0] is field 3 (state); utime is field 14 -> f[11].
    if f.len() < 22 {
        return None;
    }
    let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
    Some(ProcStat {
        comm,
        state: f[0].chars().next().unwrap_or('?'),
        ticks: n(11) + n(12),
        threads: n(17),
        starttime: n(19),
        rss_pages: n(21),
    })
}

/// The Docker container id in a `/proc/<pid>/cgroup` file, for the systemd
/// (`docker-<id>.scope`) and cgroupfs (`/docker/<id>`) drivers.
pub fn container_id_from_cgroup(s: &str) -> Option<&str> {
    for line in s.lines() {
        let path = line.rsplit(':').next().unwrap_or("");
        for seg in path.split('/').rev() {
            let id = seg
                .strip_prefix("docker-")
                .and_then(|r| r.strip_suffix(".scope"))
                .or_else(|| (seg.len() == 64 && path.contains("/docker/")).then_some(seg));
            if let Some(id) = id {
                if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// `/proc/loadavg` -> 1, 5, 15 minute averages.
pub fn parse_loadavg(s: &str) -> [f64; 3] {
    let mut f = s.split_ascii_whitespace().map(|x| x.parse().unwrap_or(0.0));
    [
        f.next().unwrap_or(0.0),
        f.next().unwrap_or(0.0),
        f.next().unwrap_or(0.0),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    Temp,
    Fan,
}

/// One hwmon reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Sensor {
    /// The chip's `name` (coretemp, nvme, k10temp, acpitz...).
    pub chip: String,
    /// `tempN_label`, else "tempN".
    pub label: String,
    pub kind: SensorKind,
    /// °C or RPM.
    pub value: f64,
    pub max: Option<f64>,
    pub crit: Option<f64>,
    /// Stable id for history (`chip/label`; hwmonN numbers change on boot).
    pub key: String,
}

fn read_num(p: &Path) -> Option<f64> {
    std::fs::read_to_string(p).ok()?.trim().parse::<f64>().ok()
}

/// Every temperature and fan under `<sys>/class/hwmon`, sorted by chip then
/// label. Temperatures are in millidegrees in sysfs; impossible readings
/// (below -40 or above 150 °C, typically a disconnected probe) are dropped.
pub fn read_hwmon(sys: &Path) -> Vec<Sensor> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(sys.join("class/hwmon")) else {
        return out;
    };
    let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let chip = std::fs::read_to_string(dir.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "hwmon".into());
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut inputs: Vec<String> = files
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| (n.starts_with("temp") || n.starts_with("fan")) && n.ends_with("_input"))
            .collect();
        inputs.sort_by_key(|n| {
            let base = n.trim_end_matches("_input");
            let num: u32 = base
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .parse()
                .unwrap_or(0);
            (n.starts_with("fan"), num)
        });
        for name in inputs {
            let base = name.trim_end_matches("_input");
            let kind = if base.starts_with("temp") {
                SensorKind::Temp
            } else {
                SensorKind::Fan
            };
            let Some(raw) = read_num(&dir.join(&name)) else {
                continue;
            };
            let div = if kind == SensorKind::Temp {
                1000.0
            } else {
                1.0
            };
            let value = raw / div;
            if kind == SensorKind::Temp && !(-40.0..=150.0).contains(&value) {
                continue;
            }
            let label = std::fs::read_to_string(dir.join(format!("{base}_label")))
                .map(|s| s.trim().to_string())
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| base.to_string());
            let lim = |suffix: &str| {
                read_num(&dir.join(format!("{base}_{suffix}")))
                    .map(|v| v / div)
                    .filter(|v| *v > 0.0 && (kind == SensorKind::Fan || *v < 200.0))
            };
            let (max, crit) = match kind {
                SensorKind::Temp => (lim("max"), lim("crit")),
                SensorKind::Fan => (lim("max"), None),
            };
            let mut key = format!("{chip}/{label}");
            let mut i = 2;
            while out.iter().any(|s: &Sensor| s.key == key) {
                key = format!("{chip}/{label}#{i}");
                i += 1;
            }
            out.push(Sensor {
                chip: chip.clone(),
                label,
                kind,
                value,
                max,
                crit,
                key,
            });
        }
    }
    out
}
