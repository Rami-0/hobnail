//! Unit tests of the `/api/v1` view builders, and the formatting twins:
//! `web/src/format/fixtures.json` holds inputs and the outputs Rust gives
//! for them (bytes, durations, "ago", percentages, the status word and dot,
//! port links), so the client's formatters are checked against the same
//! values. Drift-checked like the generated types; `RH_UPDATE_TYPES=1`
//! rewrites it.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::containers::{parse_window, started_from_status, view};
use super::stream::{signature, stats_event, Topics};
use crate::config::Config;
use crate::model::{Container, PortMapping, Snapshot, StatsMap};
use crate::ui::view::{binding_link, card_for, dot_for, link_template, port_views, status_label};

fn port(ip: &str, host_port: u16, container_port: u16, proto: &str) -> PortMapping {
    PortMapping {
        host_ip: ip.into(),
        host_port,
        container_port,
        proto: proto.into(),
    }
}

fn binding_json(p: &PortMapping) -> Value {
    json!({
        "host_ip": p.host_ip, "host_port": p.host_port,
        "container_port": p.container_port, "proto": p.proto,
    })
}

/// Viewer hosts as the server sees them (`request_host`: no port, no
/// brackets): IPv4, IPv6, loopback, localhost, a host name.
const HOSTS: &[&str] = &[
    "192.0.2.10",
    "2001:db8::10",
    "127.0.0.1",
    "::1",
    "localhost",
    "homelab.example.com",
];

fn binding_sets() -> Vec<(&'static str, Vec<PortMapping>)> {
    vec![
        ("any v4", vec![port("0.0.0.0", 8080, 80, "tcp")]),
        (
            "any v4 and v6",
            vec![
                port("0.0.0.0", 8080, 80, "tcp"),
                port("::", 8080, 80, "tcp"),
            ],
        ),
        ("any v6, https", vec![port("::", 8443, 443, "tcp")]),
        ("empty ip", vec![port("", 9000, 9000, "tcp")]),
        ("loopback only", vec![port("127.0.0.1", 5000, 5000, "tcp")]),
        ("loopback v6 only", vec![port("::1", 7000, 7000, "tcp")]),
        ("specific ip", vec![port("192.0.2.10", 3000, 3000, "tcp")]),
        (
            "other specific ip",
            vec![port("192.0.2.20", 3001, 3000, "tcp")],
        ),
        ("specific v6", vec![port("2001:db8::10", 3002, 3000, "tcp")]),
        (
            "loopback and specific",
            vec![
                port("127.0.0.1", 8081, 80, "tcp"),
                port("192.0.2.10", 8082, 80, "tcp"),
            ],
        ),
        ("postgres", vec![port("0.0.0.0", 5432, 5432, "tcp")]),
        ("dns udp", vec![port("0.0.0.0", 53, 53, "udp")]),
        ("udp web port", vec![port("0.0.0.0", 8080, 80, "udp")]),
        (
            "two ports",
            vec![
                port("0.0.0.0", 80, 80, "tcp"),
                port("0.0.0.0", 443, 443, "tcp"),
                port("0.0.0.0", 8443, 8443, "tcp"),
            ],
        ),
    ]
}

fn ctr(name: &str, state: &str, health: Option<&str>) -> Container {
    Container {
        id: format!("{:0<64}", "ab12cd34ef56"),
        name: name.into(),
        image: format!("img/{name}:latest"),
        state: state.into(),
        status: if state == "running" {
            "Up 3 hours".into()
        } else {
            "Exited (1) 2 hours ago".into()
        },
        health: health.map(str::to_string),
        ..Default::default()
    }
}

fn cfg(toml: &str) -> Config {
    crate::config::parse(toml, "test.toml").unwrap()
}

fn fixtures() -> Value {
    use crate::ui::fmt::{ago, pct};
    use crate::util::{human_bytes, human_duration};

    let bytes: Vec<Value> = [
        0u64,
        1,
        1023,
        1024,
        1536,
        10_240,
        102_400,
        1_048_575,
        1_048_576,
        104_857_600,
        1_610_612_736,
        1_099_511_627_776,
        3 << 50,
        9_007_199_254_740_991,
    ]
    .iter()
    .map(|&n| json!({ "in": n, "out": human_bytes(n) }))
    .collect();
    let durations: Vec<Value> = [
        0u64, 1, 9, 59, 60, 61, 119, 3599, 3600, 3661, 7199, 86_399, 86_400, 90_061, 604_800,
        31_536_000,
    ]
    .iter()
    .map(|&n| json!({ "in": n, "out": human_duration(n) }))
    .collect();
    let now = 1_800_000_000u64;
    let agos: Vec<Value> = [0i64, 1, 9, 10, 59, 60, 3599, 3600, 86_400, 90_061, -30]
        .iter()
        .map(|&d| {
            let at = (now as i64 - d) as u64;
            json!({ "now": now, "at": at, "out": ago(now, at) })
        })
        .collect();
    let pcts: Vec<Value> = [
        0.0f64, 0.04, 0.05, 0.4, 1.25, 9.94, 9.95, 9.96, 10.0, 12.5, 99.5, 100.0, 250.4, -1.0,
    ]
    .iter()
    .map(|&v| json!({ "in": v, "out": pct(v) }))
    .collect();

    // Status word, dot and attention of a row, as the list computes them.
    let empty = Config::default();
    let mut status = Vec::new();
    for (state, health, exit, dies) in [
        ("running", None, None, 0usize),
        ("running", Some("healthy"), None, 0),
        ("running", Some("unhealthy"), None, 0),
        ("running", Some("starting"), None, 0),
        ("running", Some("none"), None, 0),
        ("running", None, None, 1),
        ("running", None, None, 3),
        ("restarting", None, None, 0),
        ("paused", None, None, 0),
        ("exited", None, Some(0i64), 0),
        ("exited", None, Some(137), 0),
        ("exited", Some("unhealthy"), Some(1), 0),
        ("created", None, None, 0),
        ("dead", None, None, 0),
        ("removing", None, None, 0),
    ] {
        let mut c = ctr("x", state, health);
        c.exit_code = exit;
        c.die_times = (0..dies as u64).map(|i| now - 60 * i).collect();
        let snap = Snapshot {
            containers: vec![c.clone()],
            fetched_at: now,
            error: None,
        };
        let card = card_for(&empty, &snap, &StatsMap::new(), "", "x").unwrap();
        status.push(json!({
            "state": state, "health": health, "exit_code": exit, "die_events_last_hour": dies,
            "label": status_label(&c), "dot": dot_for(&c).class(),
            "row_exit_code": card.exit_code, "row_restarts": card.restarts,
            "attention": card.attention().map(|(s, r)| json!({
                "severity": match s { crate::ui::view::Sev::Bad => "bad", crate::ui::view::Sev::Warn => "warn" },
                "reason": r,
            })),
        }));
    }

    let mut views = Vec::new();
    let mut links = Vec::new();
    for (what, ports) in binding_sets() {
        for host in HOSTS {
            let out: Vec<Value> = port_views(&ports, host)
                .into_iter()
                .map(|p| {
                    json!({ "label": p.label, "url": p.url, "local": p.local, "detail": p.detail, "proto": p.proto })
                })
                .collect();
            views.push(json!({
                "case": what, "host": host,
                "ports": ports.iter().map(binding_json).collect::<Vec<_>>(),
                "out": out,
            }));
            for p in &ports {
                links.push(json!({
                    "case": what, "host": host, "binding": binding_json(p),
                    "out": match binding_link(p, host) {
                        Ok(u) => json!({ "url": u }),
                        Err(why) => json!({ "none": why }),
                    },
                }));
            }
        }
    }

    // The row's main link: the configured template with `{host}` filled,
    // else the first port link (`CardView::url`, what the pages open).
    let mut row_links = Vec::new();
    for tpl in [
        None,
        Some("http://{host}:9000/{name}"),
        Some("https://{host}/app?id={id}"),
        Some("https://media.example.com/"),
    ] {
        for (what, ports) in [
            ("any v4", vec![port("0.0.0.0", 8080, 80, "tcp")]),
            ("loopback only", vec![port("127.0.0.1", 5000, 5000, "tcp")]),
            (
                "postgres then web",
                vec![
                    port("0.0.0.0", 5432, 5432, "tcp"),
                    port("0.0.0.0", 8443, 443, "tcp"),
                ],
            ),
            ("none", vec![]),
        ] {
            let toml = match tpl {
                Some(t) => format!("[containers.web]\nurl = \"{t}\"\n"),
                None => String::new(),
            };
            let cfg = cfg(&toml);
            let mut c = ctr("web", "running", None);
            c.ports = ports.clone();
            let snap = Snapshot {
                containers: vec![c.clone()],
                fetched_at: now,
                error: None,
            };
            for host in HOSTS {
                let card = card_for(&cfg, &snap, &StatsMap::new(), host, "web").unwrap();
                row_links.push(json!({
                    "case": what, "host": host,
                    "link": link_template(&cfg, &c),
                    "ports": ports.iter().map(binding_json).collect::<Vec<_>>(),
                    "out": card.url,
                }));
            }
        }
    }

    json!({
        "_comment": "Generated by `RH_UPDATE_TYPES=1 scripts/cargo.sh test` (src/api/tests.rs); do not edit. Expected outputs are what the Rust helpers return.",
        "human_bytes": bytes,
        "human_duration": durations,
        "ago": agos,
        "pct": pcts,
        "status": status,
        "port_views": views,
        "binding_link": links,
        "row_link": row_links,
    })
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/src/format/fixtures.json")
}

#[test]
fn format_fixtures_match_the_committed_file() {
    let want = format!("{}\n", serde_json::to_string_pretty(&fixtures()).unwrap());
    let path = fixtures_path();
    if std::env::var("RH_UPDATE_TYPES").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let got = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        got == want,
        "web/src/format/fixtures.json is out of date with the Rust formatters; run \
         `RH_UPDATE_TYPES=1 scripts/cargo.sh test` and commit the result"
    );
}

#[test]
fn fixtures_cover_the_hard_cases() {
    let f = fixtures();
    let find = |case: &str, host: &str| {
        f["port_views"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["case"] == case && v["host"] == host)
            .unwrap()["out"][0]
            .clone()
    };
    assert_eq!(
        find("any v4", "2001:db8::10")["url"],
        "http://[2001:db8::10]:8080"
    );
    assert_eq!(find("loopback only", "192.0.2.10")["url"], Value::Null);
    assert_eq!(find("loopback only", "192.0.2.10")["local"], true);
    assert_eq!(
        find("loopback only", "localhost")["url"],
        "http://localhost:5000"
    );
    assert_eq!(find("postgres", "192.0.2.10")["url"], Value::Null);
    assert_eq!(
        find("any v6, https", "homelab.example.com")["url"],
        "https://homelab.example.com:8443"
    );
    assert_eq!(
        find("other specific ip", "192.0.2.10")["url"],
        "http://192.0.2.20:3001"
    );
}

// ---------------------------------------------------------------- views

#[test]
fn started_at_from_docker_status() {
    let t = 1_800_000_000;
    let at = |s| started_from_status(s, t);
    assert_eq!(at("Up 3 hours"), Some(t as i64 - 3 * 3600));
    assert_eq!(at("Up 3 hours (healthy)"), Some(t as i64 - 3 * 3600));
    assert_eq!(at("Up About an hour"), Some(t as i64 - 3600));
    assert_eq!(at("Up About a minute"), Some(t as i64 - 60));
    assert_eq!(at("Up Less than a second"), Some(t as i64));
    assert_eq!(at("Up 1 second"), Some(t as i64 - 1));
    assert_eq!(at("Up 2 weeks (Paused)"), Some(t as i64 - 14 * 86_400));
    assert_eq!(at("Up 5 days"), Some(t as i64 - 5 * 86_400));
    assert_eq!(at("Exited (1) 2 hours ago"), None);
    assert_eq!(at("Up forever"), None);
    assert_eq!(at(""), None);
    assert_eq!(started_from_status("Up 3 hours", 0), None);
}

#[test]
fn list_rows_say_what_the_cards_say() {
    let cfg = cfg(
        "[containers.web]\nurl = \"http://{host}:9000/{name}\"\n[containers.gone]\nhide = true\n",
    );
    let mut web = ctr("web", "running", Some("unhealthy"));
    web.project = Some("shop".into());
    web.ports = vec![port("0.0.0.0", 8080, 80, "tcp")];
    let mut db = ctr("db", "exited", None);
    db.project = Some("shop".into());
    db.exit_code = Some(137);
    let snap = Snapshot {
        containers: vec![web.clone(), db, ctr("gone", "running", None)],
        fetched_at: 1_800_000_000,
        error: Some("socket gone".into()),
    };
    let mut stats = StatsMap::new();
    stats.insert(
        web.id.clone(),
        crate::model::ContainerStats {
            cpu_percent: 7.5,
            mem_bytes: 1 << 20,
        },
    );
    let v = view(&cfg, &snap, &stats, 1_800_000_005);
    assert_eq!(v.docker_error.as_deref(), Some("socket gone"));
    assert_eq!(v.groups.len(), 1, "{v:?}");
    let g = &v.groups[0];
    assert_eq!((g.name.as_str(), g.running, g.stopped), ("shop", 1, 1));
    let names: Vec<_> = g.containers.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["db", "web"]);
    let w = &g.containers[1];
    assert_eq!(w.link.as_deref(), Some("http://{host}:9000/web"));
    assert_eq!((w.dot.as_str(), w.label.as_str()), ("bad", "unhealthy"));
    assert_eq!(w.cpu_pct, Some(7.5));
    assert_eq!(w.mem_bytes, Some(1 << 20));
    assert_eq!(w.started_at, Some(1_800_000_000 - 3 * 3600));
    assert_eq!(w.ports.len(), 1);
    let a = w.attention.as_ref().unwrap();
    assert_eq!(
        (a.severity.as_str(), a.reason.as_str()),
        ("bad", "unhealthy")
    );
    let d = &g.containers[0];
    assert_eq!((d.dot.as_str(), d.label.as_str()), ("off", "stopped"));
    assert_eq!(d.exit_code, Some(137));
    assert_eq!(d.started_at, None);
    assert_eq!(d.link, None);
    assert_eq!(d.attention.as_ref().unwrap().reason, "stopped, exit 137");

    // `show_stopped = false` drops the stopped one everywhere.
    let cfg2 = crate::config::Config {
        page: crate::config::PageConfig {
            show_stopped: false,
            ..cfg.page.clone()
        },
        ..cfg.clone()
    };
    let v2 = view(&cfg2, &snap, &stats, 0);
    let names: Vec<_> = v2
        .groups
        .iter()
        .flat_map(|g| &g.containers)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["web"]);

    // The stream's signature ignores the numbers and clocks…
    let s1 = signature(&v, &snap);
    let mut stats2 = stats.clone();
    stats2.get_mut(&web.id).unwrap().cpu_percent = 50.0;
    let snap_later = Snapshot {
        fetched_at: 1_800_000_060,
        ..snap.clone()
    };
    let v3 = view(&cfg, &snap_later, &stats2, 1_800_000_065);
    assert_eq!(signature(&v3, &snap_later), s1);
    let ev = stats_event(&v3, 1);
    assert_eq!(ev.containers["web"].cpu_pct, Some(50.0));
    assert_eq!(ev.containers["db"].cpu_pct, None);
    assert!(!ev.containers.contains_key("gone"));
    // …but not a state change or Docker's status text (a restart).
    let mut snap_restart = snap.clone();
    snap_restart.containers[0].status = "Up 2 seconds".into();
    let v4 = view(&cfg, &snap_restart, &stats, 0);
    assert_ne!(signature(&v4, &snap_restart), s1);
    assert_ne!(signature(&v2, &snap), s1);
}

#[test]
fn window_query() {
    let now = 1_800_000_000;
    let w = parse_window("", now).unwrap();
    assert_eq!((w.range.as_str(), w.to, w.secs), ("6h", now, 6 * 3600));
    let w = parse_window("range=30d", now).unwrap();
    assert_eq!(w.range.as_str(), "30d");
    let w = parse_window(&format!("range=1h&from={}&to={}", now - 7200, now), now).unwrap();
    assert_eq!((w.range.as_str(), w.from(), w.to), ("6h", now - 7200, now));
    // A short window snaps to five minutes (as History).
    let w = parse_window(&format!("from={}&to={}", now - 100, now - 40), now).unwrap();
    assert_eq!(w.secs, 300);
    let e = parse_window("from=5", now).unwrap_err();
    assert_eq!((e.code, e.field.as_deref()), ("bad_request", Some("to")));
}

#[test]
fn stream_topics() {
    let t = |q| Topics::parse(q);
    assert_eq!(t(""), Topics::ALL);
    assert_eq!(t("topics=nope"), Topics::ALL);
    assert_eq!(
        t("topics=system"),
        Topics {
            containers: false,
            stats: false,
            system: true,
            boards: false,
            widget: false,
            alerts: false,
        }
    );
    assert_eq!(
        t("topics=containers%2Cstats,bogus"),
        Topics {
            containers: true,
            stats: true,
            system: false,
            boards: false,
            widget: false,
            alerts: false,
        }
    );
    assert_eq!(
        t("topics=stats&topics=system"),
        Topics {
            containers: false,
            stats: true,
            system: true,
            boards: false,
            widget: false,
            alerts: false,
        }
    );
}
