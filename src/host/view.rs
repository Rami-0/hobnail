//! Rendering for `/host`. The live region (`#hx-live`) is also served alone
//! for the 3 s poll; history charts sit below it and only change with the
//! range picker.

use std::collections::{HashMap, HashSet};

use maud::{html, Markup, PreEscaped};

use super::collect::{CpuSplit, Host, NetView, ProcReport, PSI_RES};
use super::parse::SensorKind;
use crate::db::{Agg, Range};
use crate::ui::chart::{line_chart, ChartSpec, Unit};
use crate::ui::fmt::{pct, query_get, url_encode};
use crate::ui::ic;
use crate::ui::shell::{layout, Ctx, Nav};
use crate::util::human_bytes;

pub struct LiveView<'a> {
    pub h: &'a Host,
    pub p: &'a ProcReport,
    /// Container id -> name.
    pub names: &'a HashMap<String, String>,
    /// Names whose logs may be read (`logs::tailable`): only these get a
    /// logs icon (an excluded container gets none).
    pub logs: &'a HashSet<String>,
    /// Full command lines (they can hold secrets) only for a signed-in user.
    pub signed_in: bool,
}

/// Polls the live fragment every 3 s while the tab is visible; updates the
/// region only when the response is a fragment (a login redirect is not),
/// in place (`HB.morph`): an open "Show N bridges", a sideways scroll, a
/// text selection and focus survive the update (LG-34, A11Y-02).
const POLL_JS: &str = "(function(){var el=document.getElementById('hx-live');if(!el||!window.fetch)return;var busy=0;function go(){if(document.hidden||busy)return;busy=1;fetch('/host?frag=live',{credentials:'same-origin'}).then(function(r){return r.ok?r.text():''}).then(function(t){busy=0;if(t.indexOf('hx-grid')>0){if(window.HB&&HB.morph)HB.morph(el,t);else el.innerHTML=t}}).catch(function(){busy=0})}setInterval(go,3000);document.addEventListener('visibilitychange',go)})();";

pub fn page(ctx: &Ctx, lv: &LiveView, q: &[(String, String)]) -> String {
    let range = query_get(q, "range")
        .and_then(Range::parse)
        .unwrap_or(Range::H1);
    let body = html! {
        div.page.hx {
            (crate::ui::pages::page_sub(html! {
                "Everything the kernel reports about this machine. Live numbers refresh every 3 s while this tab is visible; rates are averages since the previous reading."
            }))
            div #hx-live aria-live="off" { (live_region(lv)) }
            (history(ctx, lv.h, range))
            script { (PreEscaped(POLL_JS)) }
        }
    };
    layout(
        ctx,
        Nav::Host,
        "Host",
        crate::ui::pages::crumb_title("Host"),
        body,
    )
}

fn lvl(v: Option<f64>, warn: f64, bad: f64) -> &'static str {
    match v {
        Some(v) if v >= bad => "bad",
        Some(v) if v >= warn => "warn",
        _ => "",
    }
}

fn opt_pct(v: Option<f64>) -> String {
    v.map(pct).unwrap_or_else(|| "—".into())
}

fn rate(v: Option<f64>) -> String {
    v.map(|v| Unit::BytesPerSec.fmt(v))
        .unwrap_or_else(|| "—".into())
}

/// A rate as table text: the number, then its unit in a span that a phone
/// puts on a line of its own, on every row alike (LG-36: "144 KiB/s" in
/// five columns does not fit 390 px; one height for every row keeps the
/// 3 s poll from moving anything).
fn rate_cell(v: Option<f64>) -> Markup {
    let s = rate(v);
    match s.split_once(' ') {
        Some((n, u)) => html! { (n) span.hx-u { " " (u) } },
        None => html! { (s) },
    }
}

fn meter(v: Option<f64>, warn: f64, bad: f64) -> Markup {
    let w = v.unwrap_or(0.0).clamp(0.0, 100.0);
    html! {
        span.meter.(lvl(v, warn, bad)) role="presentation" {
            span style={"width:" (format!("{w:.1}")) "%"} {}
        }
    }
}

pub fn live_region(lv: &LiveView) -> Markup {
    let h = lv.h;
    html! {
        div.hx-grid {
            (cpu(h))
            (pressure(h))
            (memory(h))
            (sensors(h))
            (procs(lv))
            (disks(h))
            (network(h))
            (extras(h))
        }
        p.foot-line {
            // One statement of the averaging window, in the intro (LG-42).
            (ic("clock"))
            @if h.window <= 0.0 { "Rates appear on the next refresh · " }
            "Process scan " (format!("{:.1}", lv.p.scan_ms)) " ms over " (lv.p.total) " processes"
            " · up " (crate::util::human_duration(h.uptime))
        }
    }
}

fn stack(parts: &[(&str, f64)]) -> Markup {
    html! {
        div.hx-stack role="presentation" {
            @for (cls, v) in parts {
                @if *v > 0.05 {
                    span.(cls) style={"width:" (format!("{:.2}", v.clamp(0.0, 100.0))) "%"} {}
                }
            }
        }
    }
}

fn cpu(h: &Host) -> Markup {
    let c = h.cpu.unwrap_or_default();
    let have = h.cpu.is_some();
    let per_core = if h.ncores > 0 {
        h.load[0] / h.ncores as f64
    } else {
        0.0
    };
    let legend = |cls: &str, name: &str, v: f64| {
        html! { span.hx-key { i.(cls) {} (name) " " b { @if have { (pct(v)) } @else { "—" } } } }
    };
    html! {
        section.panel.hx-p aria-label="CPU" {
            div.panel-h {
                h2 { (ic("cpu")) "CPU" }
                span.muted.hx-h { (h.ncores) " cores" }
            }
            div.hx-big {
                b.(lvl(h.cpu.map(|c| c.busy), 70.0, 90.0)) { (opt_pct(h.cpu.map(|c| c.busy))) }
                span.muted { "busy" }
                span.hx-load title="Load average 1, 5, 15 minutes" {
                    "load " b { (format!("{:.2}", h.load[0])) } " · " (format!("{:.2}", h.load[1])) " · " (format!("{:.2}", h.load[2]))
                    span.muted { " (" (format!("{per_core:.2}")) " per core)" }
                }
            }
            (stack(&[("u", c.user), ("s", c.system), ("w", c.iowait), ("q", c.irq), ("t", c.steal)]))
            div.hx-legend {
                (legend("u", "user", c.user))
                (legend("s", "system", c.system))
                (legend("w", "iowait", c.iowait))
                (legend("q", "irq", c.irq))
                (legend("t", "steal", c.steal))
            }
            @if h.cores.is_empty() {
                p.muted { "No per-core figures in /proc/stat." }
            } @else {
                ul.hx-cores {
                    @for (n, s) in &h.cores {
                        @let b = s.map(|s: CpuSplit| s.busy);
                        li data-key={"cpu" (n)} { span.hx-l { "cpu" (n) } (meter(b, 70.0, 90.0)) span.hx-v { (opt_pct(b)) } }
                    }
                }
            }
        }
    }
}

fn pressure(h: &Host) -> Markup {
    let names = ["CPU", "Memory", "I/O"];
    html! {
        section.panel.hx-p aria-label="Pressure" {
            div.panel-h { h2 { (ic("activity")) "Pressure stall" } }
            @match &h.psi {
                None => p.muted { "PSI not available on this kernel (no /proc/pressure)." },
                Some(psi) => {
                    div.scroll-x {
                        table.tbl.small.hx-t {
                            thead { tr {
                                th { "" } th.num { "Now" } th.num { "10 s" } th.num.hx-o { "1 min" } th.num.hx-o { "5 min" } th.num { "Full 10 s" }
                            } }
                            tbody {
                                @for (i, name) in names.iter().enumerate() {
                                    @if let Some(p) = &psi[i] {
                                        tr {
                                            th scope="row" { (name) }
                                            td.num.(lvl(h.psi_now[i].0, 10.0, 40.0)) { (opt_pct(h.psi_now[i].0)) }
                                            td.num.(lvl(Some(p.some.avg[0]), 10.0, 40.0)) { (pct(p.some.avg[0])) }
                                            td.num.hx-o { (pct(p.some.avg[1])) }
                                            td.num.hx-o { (pct(p.some.avg[2])) }
                                            td.num.(lvl(p.full.map(|f| f.avg[0]), 5.0, 20.0)) {
                                                @if PSI_RES[i] == "cpu" { span.muted title="Not meaningful system-wide for CPU" { "—" } }
                                                @else { (p.full.map(|f| pct(f.avg[0])).unwrap_or_else(|| "—".into())) }
                                            }
                                        }
                                    } @else {
                                        tr { th scope="row" { (name) } td.muted colspan="5" { "not readable" } }
                                    }
                                }
                            }
                        }
                    }
                    p.hx-note {
                        "Share of time tasks were stalled waiting for this resource. "
                        b { "Some" } ": at least one task waited; " b { "full" } ": all non-idle tasks waited at once (time lost entirely). "
                        "0% means nobody waited; sustained double digits mean the machine is short of it."
                    }
                }
            }
        }
    }
}

fn kv(label: &str, value: String) -> Markup {
    html! { div { dt { (label) } dd { (value) } } }
}

fn memory(h: &Host) -> Markup {
    let m = &h.mem;
    let t = m.total.max(1) as f64;
    let p = |v: u64| v as f64 * 100.0 / t;
    let swap_pct = (m.swap_total > 0).then(|| m.swap_used() as f64 * 100.0 / m.swap_total as f64);
    let page_rate = |v: Option<f64>| {
        v.map(|v| Unit::BytesPerSec.fmt(v * 4096.0))
            .unwrap_or_else(|| "—".into())
    };
    html! {
        section.panel.hx-p aria-label="Memory" {
            div.panel-h { h2 { (ic("memory")) "Memory" } span.muted.hx-h { (human_bytes(m.total)) } }
            @if !h.mem_ok {
                p.muted { "/proc/meminfo is not readable." }
            } @else {
                div.hx-big {
                    b.(lvl(Some(100.0 - p(m.available)), 80.0, 92.0)) { (human_bytes(m.total.saturating_sub(m.available))) }
                    span.muted { "in use · " (human_bytes(m.available)) " available" }
                }
                (stack(&[("u", p(m.used())), ("s", p(m.buffers)), ("c", p(m.cache())), ]))
                div.hx-legend {
                    span.hx-key { i.u {} "used " b { (human_bytes(m.used())) } }
                    span.hx-key { i.s {} "buffers " b { (human_bytes(m.buffers)) } }
                    span.hx-key { i.c {} "cache " b { (human_bytes(m.cache())) } }
                    span.hx-key { i {} "free " b { (human_bytes(m.free)) } }
                }
                dl.hx-kv {
                    (kv("Slab", human_bytes(m.slab)))
                    (kv("Shared", human_bytes(m.shmem)))
                    (kv("Dirty", human_bytes(m.dirty)))
                    (kv("Major faults", h.majflt.map(|v| format!("{v:.0}/s")).unwrap_or_else(|| "—".into())))
                }
                div.hx-swap {
                    div.hx-row {
                        span.hx-l { "Swap" }
                        (meter(swap_pct, 50.0, 85.0))
                        span.hx-v {
                            @if m.swap_total == 0 { "none" }
                            @else { (human_bytes(m.swap_used())) " / " (human_bytes(m.swap_total)) }
                        }
                    }
                    p.hx-note {
                        "Swap activity: in " b.(lvl(h.swap_in, 1.0, 100.0)) { (page_rate(h.swap_in)) }
                        " · out " b.(lvl(h.swap_out, 1.0, 100.0)) { (page_rate(h.swap_out)) }
                        ". Used swap alone is harmless; steady swapping in means memory is short."
                    }
                }
            }
        }
    }
}

fn sensors(h: &Host) -> Markup {
    html! {
        section.panel.hx-p aria-label="Sensors" {
            div.panel-h { h2 { (ic("thermo")) "Sensors" } }
            @if h.sensors.is_empty() {
                p.muted { "No hwmon temperatures or fans are exposed under /sys/class/hwmon." }
            } @else {
                div.scroll-x {
                    table.tbl.small.hx-t {
                        thead { tr { th { "Sensor" } th.num { "Now" } th.num { "High" } th.num { "Critical" } } }
                        tbody {
                            @for s in &h.sensors {
                                @let (now, hi, cr) = match s.kind {
                                    SensorKind::Temp => (format!("{:.0} °C", s.value), s.max.map(|v| format!("{v:.0} °C")), s.crit.map(|v| format!("{v:.0} °C"))),
                                    SensorKind::Fan => (format!("{:.0} rpm", s.value), s.max.map(|v| format!("{v:.0} rpm")), None),
                                };
                                @let cls = match (s.kind, s.max, s.crit) {
                                    (SensorKind::Temp, _, Some(c)) if s.value >= c - 5.0 => "bad",
                                    (SensorKind::Temp, Some(m), _) if s.value >= m => "warn",
                                    (SensorKind::Temp, None, None) if s.value >= 85.0 => "warn",
                                    _ => "",
                                };
                                tr data-key={(s.chip) "/" (s.label)} {
                                    th scope="row" { span.hx-chip { (s.chip) } " " (s.label) }
                                    td.num.(cls) { b { (now) } }
                                    td.num.muted { (hi.unwrap_or_else(|| "—".into())) }
                                    td.num.muted { (cr.unwrap_or_else(|| "—".into())) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn ctr_link(
    cid: &Option<String>,
    names: &HashMap<String, String>,
    logs: &HashSet<String>,
) -> Markup {
    match cid {
        None => html! { span.muted { "host" } },
        Some(id) => match names.get(id) {
            // The name opens the container; the icon, a sibling, its logs
            // (only when they may be read).
            Some(n) => html! {
                span.nm-l {
                    a href={"/containers/" (url_encode(n))} { (n) }
                    @if logs.contains(n) {
                        a.ia.logs href=(crate::logs::logs_url(n)) title="Logs" aria-label={"Logs of " (n)} { (ic("logs")) }
                    }
                }
            },
            None => html! { span.muted title=(id) { (id.get(..12).unwrap_or(id)) } },
        },
    }
}

fn user(uid: Option<u32>) -> String {
    match uid {
        Some(0) => "root".into(),
        Some(u) => u.to_string(),
        None => "?".into(),
    }
}

fn procs(lv: &LiveView) -> Markup {
    let p = lv.p;
    html! {
        section.panel.hx-p.hx-wide aria-label="Processes" {
            div.panel-h {
                h2 { (ic("server")) "Who is using the host" }
                // The first render's "average since started" note keeps its
                // room once live figures replace it (no shift at 3 s).
                span.muted.hx-h {
                    "CPU: 100% = one core"
                    span.hx-life.off[!p.lifetime] aria-hidden=[(!p.lifetime).then_some("true")] {
                        " · average since each process started; live figures from the next refresh"
                    }
                }
            }
            @if p.total == 0 {
                p.muted { "No processes readable in the host /proc. Is it mounted (RH_HOST_PROC)?" }
            } @else {
                div.hx-two {
                    div.scroll-x {
                        table.tbl.small.hx-t.hx-fix.hx-byc {
                            caption { "By container" }
                            thead { tr { th { "Container" } th.num { "CPU" } th.num { "Memory" } th.num.hx-o { "Procs" } } }
                            // Ranked (HB.morph, data-rank): re-sorted by every
                            // sample, so rows are re-used by position (none
                            // moves); the keyed row holding focus keeps it.
                            tbody data-rank {
                                @for (cid, cpu, rss, n) in p.groups.iter().take(10) {
                                    tr data-key=(cid.as_deref().unwrap_or("+host")) {
                                        th scope="row" { (ctr_link(cid, lv.names, lv.logs)) }
                                        td.num.(lvl(Some(*cpu), 80.0, 200.0)) { (pct(*cpu)) }
                                        td.num { (human_bytes(*rss)) }
                                        td.num.hx-o.muted { (n) }
                                    }
                                }
                            }
                        }
                        p.hx-note { "Memory is the sum of resident sets; shared pages count once per process." }
                    }
                    div.scroll-x {
                        table.tbl.small.hx-t.hx-fix.hx-procs {
                            caption { "Top processes" }
                            thead { tr { th.hx-o { "PID" } th { "Process" } th.hx-o { "User" } th.num { "CPU" } th.num { "RSS" } th { "Container" } } }
                            tbody data-rank {
                                @for r in &p.top {
                                    tr data-key=(r.pid) {
                                        td.hx-o.mono.muted { (r.pid) }
                                        td.hx-cmd {
                                            b.mono { (r.comm) }
                                            @if lv.signed_in && !r.cmd.is_empty() { span.muted.mono title=(r.cmd) { " " (r.cmd) } }
                                        }
                                        td.hx-o.muted { (user(r.uid)) }
                                        td.num.(lvl(Some(r.cpu), 80.0, 100.0)) { (pct(r.cpu)) }
                                        td.num { (human_bytes(r.rss)) }
                                        td { (ctr_link(&r.cid, lv.names, lv.logs)) }
                                    }
                                }
                            }
                        }
                        @if !lv.signed_in { p.hx-note { (ic("lock")) " Sign in to see full command lines (they can contain secrets)." } }
                    }
                }
            }
        }
    }
}

fn disks(h: &Host) -> Markup {
    let any_hidden = h.mounts.iter().any(|m| m.usage.is_none());
    html! {
        section.panel.hx-p.hx-wide aria-label="Disks" {
            div.panel-h { h2 { (ic("disk")) "Disks" } }
            div.hx-two {
                div.scroll-x {
                    table.tbl.small.hx-t {
                        caption { "File systems" }
                        thead { tr { th { "Mount" } th.hx-o { "Device" } th.num { "Used" } th.num.hx-o { "Free" } th.num { "Size" } th.hx-m { "" } } }
                        tbody {
                            @if h.mounts.is_empty() { tr { td.muted colspan="6" { "No real file systems in the host mount table." } } }
                            @for m in &h.mounts {
                                tr data-key=(m.mnt) {
                                    th scope="row" { span.mono { (m.mnt) } " " span.muted { (m.fstype) } }
                                    td.hx-o.hx-dev.mono.muted { (m.dev.trim_start_matches("/dev/")) }
                                    @if let Some((total, used, avail)) = m.usage {
                                        @let p = used as f64 * 100.0 / (used + avail).max(1) as f64;
                                        td.num { (human_bytes(used)) }
                                        td.num.hx-o { (human_bytes(avail)) }
                                        td.num { (human_bytes(total)) }
                                        td.hx-m { (meter(Some(p), 80.0, 92.0)) span.hx-v { (pct(p)) } }
                                    } @else {
                                        td.num.muted { "—" }
                                        td.num.hx-o.muted { "—" }
                                        td.num { (m.size.map(human_bytes).unwrap_or_else(|| "—".into())) }
                                        td.hx-m.muted { "size only" }
                                    }
                                }
                            }
                        }
                    }
                    @if any_hidden {
                        p.hx-note { "Used and free are only readable for file systems this container can see (the one holding /data). Others would need a bind mount or privileges." }
                    }
                }
                div.scroll-x {
                    table.tbl.small.hx-t {
                        caption { "Devices" }
                        thead { tr { th { "Device" } th.num { "Read" } th.num { "Write" } th.num.hx-o { "IOPS" } th.hx-m { "Busy" } } }
                        tbody {
                            @if h.disks.is_empty() { tr { td.muted colspan="5" { "No block devices in /proc/diskstats." } } }
                            @for d in &h.disks {
                                tr data-key=(d.name) {
                                    th scope="row" {
                                        span.mono { (d.name) }
                                        span.muted.hx-o {
                                            @if let Some(m) = &d.model { " " (m) }
                                            @if let Some(s) = d.size { " · " (human_bytes(s)) }
                                            @match d.rotational { Some(true) => " · HDD", Some(false) => " · SSD", None => "" }
                                        }
                                    }
                                    td.num { (rate_cell(d.rd_bps)) }
                                    td.num { (rate_cell(d.wr_bps)) }
                                    td.num.hx-o { (d.iops.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".into())) }
                                    td.hx-m { (meter(d.util, 60.0, 90.0)) span.hx-v { (opt_pct(d.util)) } }
                                }
                            }
                        }
                    }
                    p.hx-note { "Busy is the share of time the device had I/O in flight (" code { "%util" } "). Partitions, LVM and loop devices are counted on the disk below them." }
                }
            }
        }
    }
}

/// One interface row, keyed by `key` (an interface name, or "+bridges").
fn net_row(n: &NetView, key: &str, label: Markup) -> Markup {
    let c = &n.cur;
    let errs = c.rx_errs + c.tx_errs;
    let drops = c.rx_drop + c.tx_drop;
    html! {
        tr data-key=(key) {
            th scope="row" { (label) }
            td.num.hx-o.muted { (n.speed_mbps.map(|s| if s >= 1000 { format!("{} Gb/s", s / 1000) } else { format!("{s} Mb/s") }).unwrap_or_else(|| "—".into())) }
            td.num { (rate_cell(n.rx_bps)) }
            td.num { (rate_cell(n.tx_bps)) }
            td.num.(if n.new_errs > 0 { "warn" } else { "" }) title={"rx " (c.rx_errs) " / tx " (c.tx_errs)} { (errs) }
            td.num.(if n.new_errs > 0 { "warn" } else { "" }) title={"rx " (c.rx_drop) " / tx " (c.tx_drop)} { (drops) }
            td.num.hx-o.muted { (human_bytes(c.rx_bytes)) " / " (human_bytes(c.tx_bytes)) }
        }
    }
}

fn network(h: &Host) -> Markup {
    let real: Vec<&NetView> = h.nets.iter().filter(|n| !n.bridge).collect();
    let bridges: Vec<&NetView> = h.nets.iter().filter(|n| n.bridge).collect();
    let sum = |f: fn(&NetView) -> Option<f64>| -> Option<f64> {
        let v: Vec<f64> = bridges.iter().filter_map(|n| f(n)).collect();
        (!v.is_empty()).then(|| v.iter().sum())
    };
    let mut agg = NetView {
        bridge: true,
        rx_bps: sum(|n| n.rx_bps),
        tx_bps: sum(|n| n.tx_bps),
        ..Default::default()
    };
    for b in &bridges {
        let (a, c) = (&mut agg.cur, &b.cur);
        a.rx_bytes += c.rx_bytes;
        a.tx_bytes += c.tx_bytes;
        a.rx_errs += c.rx_errs;
        a.tx_errs += c.tx_errs;
        a.rx_drop += c.rx_drop;
        a.tx_drop += c.tx_drop;
        agg.new_errs += b.new_errs;
    }
    let head = html! { thead { tr { th { "Interface" } th.num.hx-o { "Link" } th.num { "In" } th.num { "Out" } th.num { "Errors" } th.num { "Drops" } th.num.hx-o { "Total in / out" } } } };
    html! {
        section.panel.hx-p.hx-wide aria-label="Network" {
            div.panel-h {
                h2 { (ic("globe")) "Network" }
                @if h.veths > 0 { span.muted.hx-h { (h.veths) " container veth links hidden" } }
            }
            div.scroll-x {
                table.tbl.small.hx-t {
                    (head)
                    tbody {
                        @if h.nets.is_empty() { tr { td.muted colspan="7" { "No interfaces in /proc/1/net/dev." } } }
                        @for n in &real { (net_row(n, &n.name, html! { span.mono { (n.name) } })) }
                        @if !bridges.is_empty() {
                            (net_row(&agg, "+bridges", html! { "Bridges " span.muted { "(" (bridges.len()) ")" } }))
                        }
                    }
                }
            }
            @if !bridges.is_empty() {
                details.hx-more {
                    summary { "Show " (bridges.len()) " bridges" }
                    div.scroll-x {
                        table.tbl.small.hx-t {
                            (head)
                            tbody { @for n in &bridges { (net_row(n, &n.name, html! { span.mono { (n.name) } })) } }
                        }
                    }
                }
            }
            p.hx-note { "Errors and drops are totals since boot; they turn amber when they grew during the last refresh." }
        }
    }
}

/// GPUs, only when the kernel lists one (LG-38): what is not shown and why
/// (S.M.A.R.T., systemd) is in `docs/features/host.md`, not on the page.
fn extras(h: &Host) -> Markup {
    if h.gpus.is_empty() {
        return html! {};
    }
    html! {
        section.panel.hx-p.hx-wide aria-label="GPU" {
            div.panel-h { h2 { (ic("monitor")) "GPU" } }
            p.hx-gpus {
                @for g in &h.gpus {
                    span.hx-gpu { span.mono { (g.card) } " " (g.driver) " "
                        @if let Some(b) = g.busy { b { (pct(b)) } " busy" } @else { span.muted { "(no busy % exported)" } }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// History

struct C {
    key: String,
    title: String,
    unit: Unit,
    color: &'static str,
    cap: Option<f64>,
}

fn c(key: impl Into<String>, title: impl Into<String>, unit: Unit, color: &'static str) -> C {
    C {
        key: key.into(),
        title: title.into(),
        unit,
        color,
        cap: (unit == Unit::Percent).then_some(100.0),
    }
}

fn groups(h: &Host) -> Vec<(&'static str, Vec<C>)> {
    let mut cpu: Vec<C> = h
        .cores
        .iter()
        .map(|(n, _)| {
            c(
                format!("cpu.c{n}"),
                format!("Core {n}"),
                Unit::Percent,
                "--ch-cpu",
            )
        })
        .collect();
    cpu.push(c("cpu.user", "User", Unit::Percent, "--ch-cpu"));
    cpu.push(c("cpu.system", "System", Unit::Percent, "--ch-load"));
    cpu.push(c("cpu.iowait", "I/O wait", Unit::Percent, "--ch-disk"));
    if h.cpu.is_some_and(|c| c.steal > 0.0) {
        cpu.push(c("cpu.steal", "Steal", Unit::Percent, "--ch-swap"));
    }
    let mut psi = Vec::new();
    if h.psi.is_some() {
        psi.push(c("psi.cpu.some", "CPU some", Unit::Percent, "--ch-cpu"));
        psi.push(c(
            "psi.memory.some",
            "Memory some",
            Unit::Percent,
            "--ch-mem",
        ));
        psi.push(c(
            "psi.memory.full",
            "Memory full",
            Unit::Percent,
            "--ch-mem",
        ));
        psi.push(c("psi.io.some", "I/O some", Unit::Percent, "--ch-disk"));
        psi.push(c("psi.io.full", "I/O full", Unit::Percent, "--ch-disk"));
    }
    let mem = vec![
        c("mem.used", "Used", Unit::Bytes, "--ch-mem"),
        c("mem.cache", "Cache", Unit::Bytes, "--ch-mem"),
        c("mem.avail", "Available", Unit::Bytes, "--ch-mem"),
        c("mem.swap", "Swap used", Unit::Bytes, "--ch-swap"),
        c("swap.in", "Swap in (pages/s)", Unit::Plain, "--ch-swap"),
        c("swap.out", "Swap out (pages/s)", Unit::Plain, "--ch-swap"),
    ];
    let mut disk = Vec::new();
    for d in &h.disks {
        disk.push(c(
            format!("disk.{}.rd", d.name),
            format!("{} read", d.name),
            Unit::BytesPerSec,
            "--ch-rx",
        ));
        disk.push(c(
            format!("disk.{}.wr", d.name),
            format!("{} write", d.name),
            Unit::BytesPerSec,
            "--ch-tx",
        ));
        disk.push(c(
            format!("disk.{}.util", d.name),
            format!("{} busy", d.name),
            Unit::Percent,
            "--ch-disk",
        ));
    }
    for m in h.mounts.iter().filter(|m| m.usage.is_some()) {
        disk.push(c(
            format!("mount.{}", m.mnt),
            format!("{} used", m.mnt),
            Unit::Bytes,
            "--ch-disk",
        ));
    }
    let mut net = Vec::new();
    for n in h.nets.iter().filter(|n| !n.bridge) {
        net.push(c(
            format!("net.{}.rx", n.name),
            format!("{} in", n.name),
            Unit::BytesPerSec,
            "--ch-rx",
        ));
        net.push(c(
            format!("net.{}.tx", n.name),
            format!("{} out", n.name),
            Unit::BytesPerSec,
            "--ch-tx",
        ));
    }
    let temp: Vec<C> = h
        .sensors
        .iter()
        .filter(|s| s.kind == SensorKind::Temp)
        .map(|s| C {
            cap: None,
            ..c(
                format!("temp.{}", s.key),
                format!("{} {}", s.chip, s.label),
                Unit::Celsius,
                "--ch-temp",
            )
        })
        .collect();
    vec![
        ("CPU", cpu),
        ("Pressure", psi),
        ("Memory", mem),
        ("Disks", disk),
        ("Network", net),
        ("Temperatures", temp),
    ]
}

fn history(ctx: &Ctx, h: &Host, range: Range) -> Markup {
    let db = &ctx.state.db;
    let en = db.is_enabled();
    let now = ctx.now as i64;
    html! {
        section.sec #history aria-label="History" {
            div.panel-h {
                h2 { "History" }
                nav.seg aria-label="Range" {
                    @for r in Range::ALL {
                        a href={"/host?range=" (r.as_str()) "#history"} aria-current=[(r == range).then_some("true")] { (r.as_str()) }
                    }
                }
            }
            @if !en { (crate::ui::pages::history_off_banner()) }
            @for (name, charts) in groups(h) {
                @if !charts.is_empty() {
                    h3.hx-gh { (name) }
                    div.charts.four {
                        @for ch in &charts {
                            @let spec = ChartSpec {
                                title: &ch.title,
                                unit: ch.unit,
                                color: ch.color,
                                range_secs: range.secs(),
                                now,
                                cap: ch.cap,
                                zero_based: ch.unit != Unit::Celsius,
                                compact: true,
                            };
                            (line_chart(&spec, &db.hostx_series(&ch.key, range, now, Agg::Avg), en))
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    /// A process row that belongs to a container: the name opens it and a
    /// sibling icon opens its logs (one click, never a link in a link).
    #[test]
    fn container_cells_link_the_container_and_its_logs() {
        let names: HashMap<String, String> = [
            ("abc".to_string(), "web<1>".to_string()),
            ("def".to_string(), "secret-db".to_string()),
        ]
        .into();
        let logs: HashSet<String> = ["web<1>".to_string()].into();
        let html = ctr_link(&Some("abc".into()), &names, &logs).into_string();
        assert!(
            html.contains(r#"<a href="/containers/web%3C1%3E">web&lt;1&gt;</a>"#),
            "{html}"
        );
        assert!(html.contains(r#"<a class="ia logs" href="/containers/web%3C1%3E/logs" title="Logs" aria-label="Logs of web&lt;1&gt;">"#), "{html}");
        let host = ctr_link(&None, &names, &logs).into_string();
        assert!(!host.contains("href"), "host processes have no logs");
        let unknown = ctr_link(&Some("ffffffffffffff".into()), &names, &logs).into_string();
        assert!(!unknown.contains("href"));
        // An excluded (or otherwise untailable) container: its name, no logs link.
        let excluded = ctr_link(&Some("def".into()), &names, &logs).into_string();
        assert!(excluded.contains("secret-db"), "{excluded}");
        assert!(!excluded.contains("/logs"), "{excluded}");
    }

    /// A rate's unit is its own span, so a phone can put it on a second
    /// line on every row (LG-36).
    #[test]
    fn rates_keep_their_unit_apart() {
        assert_eq!(
            rate_cell(Some(2048.0)).into_string(),
            r#"2.0<span class="hx-u"> KiB/s</span>"#
        );
        assert_eq!(rate_cell(None).into_string(), "—");
    }

    /// No GPU: no panel (LG-38); the "not shown" rows are gone for good.
    #[test]
    fn hardware_panel_only_for_a_gpu() {
        let mut h = Host::default();
        assert_eq!(extras(&h).into_string(), "");
        h.gpus.push(super::super::collect::Gpu {
            card: "card0".into(),
            driver: "i915".into(),
            busy: None,
        });
        let html = extras(&h).into_string();
        assert!(html.contains("card0") && html.contains("i915"), "{html}");
        assert!(!html.contains("S.M.A.R.T") && !html.contains("systemd"));
    }
}
