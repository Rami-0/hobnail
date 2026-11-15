use std::path::{Path, PathBuf};

use super::collect::Collector;
use super::parse::*;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "rh-host-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn write(&self, rel: &str, s: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    fn path(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stat_per_core() {
    let s = "cpu  100 5 50 800 50 3 2 7 0 0\ncpu1 60 0 30 400 20 0 0 0 0 0\ncpu0 40 5 20 400 30 3 2 7 0 0\nintr 1 2\ncpufreq x\n";
    let (t, cores) = parse_stat(s);
    let t = t.unwrap();
    assert_eq!((t.user, t.nice, t.iowait, t.steal), (100, 5, 50, 7));
    assert_eq!(t.total(), 1017);
    assert_eq!(cores.iter().map(|c| c.0).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(cores[1].1.user, 60);
    assert_eq!(parse_stat("garbage"), (None, vec![]));
}

#[test]
fn pressure_lines() {
    let p = parse_pressure(
        "some avg10=12.29 avg60=11.78 avg300=11.63 total=113896981325\nfull avg10=0.18 avg60=0.16 avg300=0.26 total=11111086850\n",
    )
    .unwrap();
    assert_eq!(p.some.avg, [12.29, 11.78, 11.63]);
    assert_eq!(p.some.total, 113_896_981_325);
    assert_eq!(p.full.unwrap().avg[0], 0.18);
    let old = parse_pressure("some avg10=1.00 avg60=0.50 avg300=0.10 total=5\n").unwrap();
    assert!(old.full.is_none());
    assert!(parse_pressure("").is_none());
}

#[test]
fn meminfo_and_vmstat() {
    let m = parse_meminfo(
        "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 600 kB\nBuffers: 50 kB\nCached: 300 kB\nSReclaimable: 50 kB\nSlab: 80 kB\nShmem: 10 kB\nSwapTotal: 2000 kB\nSwapFree: 500 kB\nDirty: 4 kB\n",
    );
    assert_eq!(m.total, 1_024_000);
    assert_eq!(m.cache(), 350 * 1024);
    assert_eq!(m.used(), (1000 - 100 - 50 - 350) * 1024);
    assert_eq!(m.swap_used(), 1500 * 1024);
    let v = parse_vmstat("nr_free_pages 5\npswpin 123\npswpout 456\npgmajfault 9\n");
    assert_eq!((v.pswpin, v.pswpout, v.pgmajfault), (123, 456, 9));
}

#[test]
fn diskstats_and_filter() {
    let s = "   7       0 loop0 10 0 20 0 0 0 0 0 0 5 5 0 0 0 0\n   8       0 sda 1000 10 20000 500 2000 20 40000 900 0 1500 1400 0 0 0 0\n   8       1 sda1 5 0 10 0 0 0 0 0 0 1 1\n 253       0 dm-0 900 0 18000 400 1900 0 38000 800 0 1400 1200 0 0 0 0\n";
    let d = parse_diskstats(s);
    assert_eq!(d.len(), 4);
    assert!(
        parse_diskstats("8 0 sda 1 2").is_empty(),
        "short line skipped"
    );
    let sda = &d[1];
    assert_eq!(
        (
            sda.rd_ios,
            sda.rd_sectors,
            sda.wr_ios,
            sda.wr_sectors,
            sda.io_ticks
        ),
        (1000, 20000, 2000, 40000, 1500)
    );
    let block = vec!["sda".to_string(), "dm-0".to_string(), "loop0".to_string()];
    let kept: Vec<_> = d
        .iter()
        .filter(|x| keep_disk(&x.name, &block))
        .map(|x| x.name.as_str())
        .collect();
    assert_eq!(kept, vec!["sda"]);
    // Without /sys, by name.
    for (n, k) in [
        ("sda", true),
        ("sda1", false),
        ("nvme0n1", true),
        ("nvme0n1p2", false),
        ("md0", true),
        ("sr0", false),
        ("mmcblk0", true),
        ("mmcblk0p1", false),
    ] {
        assert_eq!(keep_disk(n, &[]), k, "{n}");
    }
}

#[test]
fn mounts_filtering() {
    let s = "/dev/mapper/vg-root / ext4 rw,relatime 0 0\n\
proc /proc proc rw 0 0\n\
tmpfs /run tmpfs rw 0 0\n\
/dev/sda2 /boot ext4 rw 0 0\n\
/dev/sda1 /boot/efi vfat rw 0 0\n\
overlay /var/lib/docker/overlay2/x/merged overlay rw 0 0\n\
/dev/mapper/vg-root /var/lib/docker/containers/abc/hosts ext4 rw 0 0\n\
/dev/mapper/vg-root /opt/stacks/x ext4 rw 0 0\n\
/dev/loop3 /snap/core/1 squashfs ro 0 0\n\
/dev/sdb1 /mnt/my\\040disk xfs rw 0 0\n";
    let m = parse_mounts(s);
    let mnts: Vec<_> = m.iter().map(|x| x.mnt.as_str()).collect();
    assert_eq!(mnts, vec!["/", "/boot", "/boot/efi", "/mnt/my disk"]);
    assert_eq!(m[0].dev, "/dev/mapper/vg-root");
    assert_eq!(m[3].fstype, "xfs");
}

#[test]
fn net_dev_errors_drops() {
    let s = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0\nenp1s0: 5000 50 2 3 0 0 0 0 7000 70 4 5 0 0 0 0\nveth1: 1 1 0 0 0 0 0 0 1 1 0 0 0 0 0 0\n";
    let d = parse_net_dev(s);
    assert_eq!(d.len(), 3);
    let e = &d[1];
    assert_eq!(e.name, "enp1s0");
    assert_eq!(
        (e.rx_bytes, e.rx_errs, e.rx_drop, e.tx_bytes, e.tx_errs, e.tx_drop),
        (5000, 2, 3, 7000, 4, 5)
    );
    assert_eq!(iface_kind("veth1"), IfKind::Skip);
    assert_eq!(iface_kind("lo"), IfKind::Skip);
    assert_eq!(iface_kind("br-abc"), IfKind::Bridge);
    assert_eq!(iface_kind("docker0"), IfKind::Bridge);
    assert_eq!(iface_kind("tailscale0"), IfKind::Real);
}

#[test]
fn proc_stat_and_cgroup() {
    let s = "1234 (my (odd) proc) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 7 0 98765 123456789 2048 18446744073709551615";
    let p = parse_proc_stat(s).unwrap();
    assert_eq!(p.comm, "my (odd) proc");
    assert_eq!(p.state, 'S');
    assert_eq!(p.ticks, 300);
    assert_eq!(p.threads, 7);
    assert_eq!(p.starttime, 98765);
    assert_eq!(p.rss_pages, 2048);
    assert!(parse_proc_stat("12 (x) S 1 2").is_none());

    let id = "5b39b52d8f5cc87523305419044aaec10fc29a6c592cfcbbb8a50738b041b278";
    assert_eq!(
        container_id_from_cgroup(&format!("0::/system.slice/docker-{id}.scope\n")),
        Some(id)
    );
    assert_eq!(
        container_id_from_cgroup(&format!("12:memory:/docker/{id}\n")),
        Some(id)
    );
    assert_eq!(container_id_from_cgroup("0::/init.scope\n"), None);
    assert_eq!(
        container_id_from_cgroup("0::/system.slice/docker.service\n"),
        None
    );
    assert_eq!(
        container_id_from_cgroup("0::/system.slice/docker-abc.scope\n"),
        None
    );
}

#[test]
fn hwmon_discovery() {
    let d = Dir::new("hwmon");
    d.write("class/hwmon/hwmon0/name", "coretemp\n");
    d.write("class/hwmon/hwmon0/temp1_input", "55000\n");
    d.write("class/hwmon/hwmon0/temp1_label", "Package id 0\n");
    d.write("class/hwmon/hwmon0/temp1_max", "84000\n");
    d.write("class/hwmon/hwmon0/temp1_crit", "100000\n");
    d.write("class/hwmon/hwmon0/temp2_input", "51000\n");
    d.write("class/hwmon/hwmon0/temp10_input", "50000\n");
    d.write("class/hwmon/hwmon1/name", "nvme\n");
    d.write("class/hwmon/hwmon1/temp1_input", "41850\n");
    d.write("class/hwmon/hwmon1/temp1_label", "Composite\n");
    d.write("class/hwmon/hwmon1/fan1_input", "1200\n");
    d.write("class/hwmon/hwmon2/name", "bogus\n");
    d.write("class/hwmon/hwmon2/temp1_input", "-273000\n");
    let s = read_hwmon(&d.0);
    let keys: Vec<_> = s.iter().map(|x| x.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "coretemp/Package id 0",
            "coretemp/temp2",
            "coretemp/temp10",
            "nvme/Composite",
            "nvme/fan1"
        ]
    );
    assert_eq!(s[0].value, 55.0);
    assert_eq!((s[0].max, s[0].crit), (Some(84.0), Some(100.0)));
    assert_eq!(s[3].value, 41.85);
    assert_eq!(s[4].kind, SensorKind::Fan);
    assert_eq!(s[4].value, 1200.0);
    assert!(read_hwmon(Path::new("/nonexistent")).is_empty());
}

fn fixture() -> Dir {
    let d = Dir::new("host");
    d.write("proc/stat", "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\ncpu1 50 0 25 400 25 0 0 0 0 0\n");
    d.write("proc/loadavg", "0.50 0.40 0.30 1/200 999\n");
    d.write("proc/uptime", "1000.5 2000.1\n");
    d.write("proc/meminfo", "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 600 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
    d.write("proc/vmstat", "pswpin 0\npswpout 0\n");
    d.write("proc/pressure/cpu", "some avg10=1.00 avg60=1.00 avg300=1.00 total=1000000\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    d.write("proc/pressure/io", "some avg10=1.00 avg60=1.00 avg300=1.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    d.write("proc/diskstats", "8 0 sda 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
    d.write("proc/1/net/dev", "h\nh\neth0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\nveth9: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\ndocker0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
    d.write("proc/1/mounts", "/dev/sda1 / ext4 rw 0 0\n");
    d.write("sys/block/sda/size", "2000\n");
    d.write("sys/block/sda/device/model", "DISK 1\n");
    d.write("sys/class/block/sda1/size", "1000\n");
    d.write("sys/class/net/eth0/speed", "1000\n");
    d
}

#[test]
fn collector_rates() {
    let d = fixture();
    let mut c = Collector::new(&d.path("proc"), &d.path("sys"));
    let h = c.collect();
    assert_eq!(h.window, 0.0);
    assert!(h.cpu.is_none());
    assert_eq!(h.ncores, 2);
    assert!(h.psi.is_some());
    assert!(h.psi.unwrap()[1].is_none(), "memory file missing");
    assert_eq!(h.disks.len(), 1);
    assert_eq!(h.disks[0].model.as_deref(), Some("DISK 1"));
    assert_eq!(h.disks[0].size, Some(2000 * 512));
    assert_eq!(h.nets.len(), 2);
    assert_eq!(h.veths, 1);
    assert_eq!(h.nets[0].speed_mbps, Some(1000));
    assert_eq!(h.mounts.len(), 1);
    // Not visible here: size from /sys.
    assert!(h.mounts[0].usage.is_none());
    assert_eq!(h.mounts[0].size, Some(1000 * 512));

    std::thread::sleep(std::time::Duration::from_millis(250));
    d.write("proc/stat", "cpu  160 0 70 900 70 0 0 0 0 0\ncpu0 80 0 35 450 35 0 0 0 0 0\ncpu1 80 0 35 450 35 0 0 0 0 0\n");
    d.write(
        "proc/diskstats",
        "8 0 sda 10 0 1000 0 10 0 2000 0 0 100 0 0 0 0 0\n",
    );
    d.write(
        "proc/1/net/dev",
        "h\nh\neth0: 5000 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
    );
    let h = c.collect();
    assert!(h.window >= 0.2);
    let cpu = h.cpu.unwrap();
    // delta: user 60, system 20, idle 100, iowait 20 of 200.
    assert!((cpu.user - 30.0).abs() < 1e-9);
    assert!((cpu.iowait - 10.0).abs() < 1e-9);
    assert!((cpu.busy - 40.0).abs() < 1e-9);
    assert!(h.cores[0].1.is_some());
    assert!(h.disks[0].rd_bps.unwrap() > 0.0);
    assert!(h.disks[0].util.unwrap() > 0.0);
    assert!(h.nets[0].rx_bps.unwrap() > 0.0);
    assert_eq!(h.nets[0].new_errs, 1);
    let rows = super::series_rows(&h);
    let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1);
    assert_eq!(get("cpu.c0"), Some(Some(40.0)));
    assert!(get("net.eth0.rx").is_some());
    assert!(get("net.docker0.rx").is_none(), "bridges have no history");
    assert!(get("psi.cpu.full").is_none());
}

#[test]
fn page_renders() {
    let d = fixture();
    std::fs::write(d.0.join("config.toml"), "").unwrap();
    let st = crate::state::AppState::new(crate::state::Env {
        listen: vec![],
        data_dir: d.0.clone(),
        config_path: d.0.join("config.toml"),
        docker_socket: String::new(),
        host_proc: d.path("proc"),
        host_sys: d.path("sys"),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    });
    super::on_tick(&st, 1_800_000_000);
    let headers = hyper::HeaderMap::new();
    let ctx = crate::ui::shell::Ctx::new(&st, "localhost", "/host", "", &headers);
    let (code, html) = super::page(&ctx);
    assert_eq!(code, hyper::StatusCode::OK);
    for want in [
        "Pressure stall",
        "cpu0",
        "Who is using the host",
        "sda",
        "eth0",
        "Bridges",
        "History",
        "hx-live",
    ] {
        assert!(html.contains(want), "{want}");
    }
    assert!(html.contains("No processes readable"));
    let ctx = crate::ui::shell::Ctx::new(&st, "localhost", "/host", "frag=live", &headers);
    let (code, frag) = super::page(&ctx);
    assert_eq!(code, hyper::StatusCode::OK);
    assert!(
        frag.starts_with("<div class=\"hx-grid\">"),
        "{}",
        &frag[..60]
    );
    assert!(!frag.contains("<html"));
}

#[test]
fn page_without_host_files() {
    let st = crate::ui::testutil::state_with(vec![], "");
    let headers = hyper::HeaderMap::new();
    let ctx = crate::ui::shell::Ctx::new(&st, "localhost", "/host", "range=7d", &headers);
    let (code, html) = super::page(&ctx);
    assert_eq!(code, hyper::StatusCode::OK);
    assert!(html.contains("PSI not available on this kernel"));
    assert!(html.contains("No hwmon temperatures"));
}
