//! Tests for the container view model.
#![allow(clippy::field_reassign_with_default)]

use super::*;
use crate::config::{Bookmark, GroupConfig};
use crate::model::{ContainerStats, PortMapping};

const HOST: &str = "100.64.0.10";

fn port(ip: &str, host_port: u16, container_port: u16) -> PortMapping {
    PortMapping {
        host_ip: ip.into(),
        host_port,
        container_port,
        proto: "tcp".into(),
    }
}

fn ctr(name: &str, project: Option<&str>) -> Container {
    Container {
        id: format!(
            "{:0<64}",
            name.bytes().map(|b| format!("{b:x}")).collect::<String>()
        )
        .chars()
        .take(64)
        .collect(),
        name: name.into(),
        image: format!("img/{name}:latest"),
        state: "running".into(),
        status: "Up 3 hours".into(),
        project: project.map(str::to_string),
        service: project.map(|p| {
            name.trim_start_matches(&format!("{p}-"))
                .trim_end_matches("-1")
                .to_string()
        }),
        ..Default::default()
    }
}

fn stopped(mut c: Container, code: i64) -> Container {
    c.state = "exited".into();
    c.status = format!("Exited ({code}) 2 hours ago");
    c.exit_code = Some(code);
    c
}

fn snap(cs: Vec<Container>) -> Snapshot {
    Snapshot {
        containers: cs,
        fetched_at: 1,
        error: None,
    }
}

fn build(cfg: &Config, cs: Vec<Container>) -> Vec<GroupView> {
    build_groups(cfg, &snap(cs), &StatsMap::new(), HOST)
}

fn names(gs: &[GroupView]) -> Vec<&str> {
    gs.iter().map(|g| g.name.as_str()).collect()
}

fn card<'a>(gs: &'a [GroupView], title: &str) -> &'a CardView {
    gs.iter()
        .flat_map(|g| &g.cards)
        .find(|c| c.title == title)
        .unwrap_or_else(|| panic!("no card {title}"))
}

fn sample() -> Vec<Container> {
    vec![
        ctr("webshop-dev-web-1", Some("webshop-dev")),
        ctr("webshop-dev-db-1", Some("webshop-dev")),
        stopped(ctr("webshop-dev-job-1", Some("webshop-dev")), 137),
        ctr("beszel", Some("beszel")),
        ctr("homepage", None),
        ctr("demo-pg", None),
    ]
}

#[test]
fn empty_config_groups_by_project() {
    let gs = build(&Config::default(), sample());
    assert_eq!(names(&gs), vec!["beszel", "Standalone", "webshop-dev"]);
    let sa = &gs[2];
    assert_eq!((sa.running, sa.stopped), (2, 1));
    assert_eq!(sa.counts(), "2 running · 1 stopped");
    assert_eq!(gs[0].counts(), "1 running");
    // Display name falls back to the compose service, sorted by name.
    let titles: Vec<&str> = sa.cards.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["db", "job", "web"]);
    assert_eq!(
        sa.cards[0].sub.as_deref(),
        Some("webshop-dev-db-1 · img/webshop-dev-db-1:latest")
    );
    // Standalone: name is the container name, sub is only the image.
    let h = card(&gs, "homepage");
    assert_eq!(h.sub.as_deref(), Some("img/homepage:latest"));
    assert_eq!(gs[0].columns, 4);
    assert_eq!(gs[0].layout, Layout::Grid);
    assert!(!gs[0].collapsed);
    assert_eq!(gs[1].title, "Standalone");
}

#[test]
fn stopped_is_grey_not_red() {
    let gs = build(&Config::default(), sample());
    let j = card(&gs, "job");
    assert_eq!(j.dot, Some(Dot::Off));
    assert!(!j.running);
    assert_eq!(j.state_text.as_deref(), Some("stopped · 2 hours ago"));
    assert_eq!(j.exit_code, Some(137));
    let clean = stopped(ctr("x", None), 0);
    let gs = build(&Config::default(), vec![clean]);
    assert_eq!(card(&gs, "x").exit_code, None);
}

#[test]
fn status_dots() {
    let mut c = ctr("a", None);
    assert_eq!(dot_for(&c), Dot::Ok);
    c.health = Some("healthy".into());
    assert_eq!(dot_for(&c), Dot::Ok);
    c.health = Some("starting".into());
    assert_eq!(dot_for(&c), Dot::Warn);
    c.health = Some("unhealthy".into());
    assert_eq!(dot_for(&c), Dot::Bad);
    for (s, d) in [
        ("paused", Dot::Warn),
        ("restarting", Dot::Warn),
        ("exited", Dot::Off),
        ("created", Dot::Off),
        ("dead", Dot::Off),
    ] {
        c.state = s.into();
        assert_eq!(dot_for(&c), d, "{s}");
    }
}

#[test]
fn state_text_strips_health() {
    let mut c = ctr("a", None);
    c.status = "Up 5 days (healthy)".into();
    c.health = Some("healthy".into());
    let gs = build(&Config::default(), vec![c]);
    let a = card(&gs, "a");
    assert_eq!(a.state_text.as_deref(), Some("Up 5 days"));
    assert_eq!(a.health.as_deref(), Some("healthy"));
}

#[test]
fn group_precedence() {
    let mut cfg = Config::default();
    let mut a = ctr("a", Some("proj"));
    a.labels.insert(LABEL_GROUP.into(), "FromLabel".into());
    let mut b = ctr("b", Some("proj"));
    b.labels.insert(LABEL_GROUP.into(), "FromLabel".into());
    let c = ctr("c", Some("proj"));
    let d = ctr("d", None);
    cfg.containers.insert(
        "b".into(),
        ContainerOverride {
            group: Some("FromConfig".into()),
            ..Default::default()
        },
    );
    cfg.docker.standalone_group = "Loose".into();
    let gs = build(&cfg, vec![a, b, c, d]);
    let find = |t: &str| {
        gs.iter()
            .find(|g| g.cards.iter().any(|c| c.title == t))
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(find("a"), "FromLabel");
    assert_eq!(find("b"), "FromConfig");
    assert_eq!(find("c"), "proj");
    assert_eq!(find("d"), "Loose");
}

#[test]
fn configured_group_order_and_options() {
    let mut cfg = Config::default();
    cfg.page.columns = 3;
    cfg.groups = vec![
        GroupConfig {
            name: "webshop-dev".into(),
            title: Some("Webshop".into()),
            columns: Some(2),
            layout: Some(Layout::List),
            collapsed: true,
            icon: Some("sa.svg".into()),
            hide: false,
        },
        GroupConfig {
            name: "beszel".into(),
            hide: true,
            ..Default::default()
        },
        GroupConfig {
            name: "nothing-here".into(),
            ..Default::default()
        },
    ];
    let gs = build(&cfg, sample());
    assert_eq!(names(&gs), vec!["webshop-dev", "Standalone"]);
    let g = &gs[0];
    assert_eq!(g.title, "Webshop");
    assert_eq!(g.columns, 2);
    assert_eq!(g.layout, Layout::List);
    assert!(g.collapsed);
    assert_eq!(g.icon.as_deref(), Some("sa.svg"));
    assert_eq!(gs[1].columns, 3);
}

#[test]
fn alphabetical_is_case_insensitive() {
    let gs = build(
        &Config::default(),
        vec![
            ctr("x", Some("zeta")),
            ctr("y", Some("Alpha")),
            ctr("z", Some("beta")),
        ],
    );
    assert_eq!(names(&gs), vec!["Alpha", "beta", "zeta"]);
}

#[test]
fn hiding() {
    let mut cfg = Config::default();
    let mut by_label = ctr("by-label", None);
    by_label.labels.insert(LABEL_HIDE.into(), "true".into());
    let mut unhidden = ctr("unhidden", None);
    unhidden.labels.insert(LABEL_HIDE.into(), "true".into());
    cfg.containers.insert(
        "unhidden".into(),
        ContainerOverride {
            hide: Some(false),
            ..Default::default()
        },
    );
    cfg.containers.insert(
        "by-config".into(),
        ContainerOverride {
            hide: Some(true),
            ..Default::default()
        },
    );
    cfg.docker.exclude = vec!["ci-test-*".into()];
    let cs = vec![
        by_label,
        unhidden,
        ctr("by-config", None),
        ctr("ci-test-pg", None),
        ctr("keep", None),
        stopped(ctr("old", None), 1),
    ];
    let gs = build(&cfg, cs.clone());
    let titles: Vec<&str> = gs[0].cards.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["keep", "old", "unhidden"]);
    cfg.page.show_stopped = false;
    let gs = build(&cfg, cs);
    let titles: Vec<&str> = gs[0].cards.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["keep", "unhidden"]);
    assert_eq!(gs[0].counts(), "2 running");
}

#[test]
fn all_hidden_group_disappears() {
    let mut cfg = Config::default();
    cfg.docker.exclude = vec!["*".into()];
    assert!(build(&cfg, sample()).is_empty());
}

#[test]
fn names_icons_order_size() {
    let mut cfg = Config::default();
    let mut a = ctr("proj-a-1", Some("proj"));
    a.labels.insert(LABEL_NAME.into(), "Label Name".into());
    a.labels.insert(LABEL_ICON.into(), "label.svg".into());
    let mut b = ctr("proj-b-1", Some("proj"));
    b.labels.insert(LABEL_NAME.into(), "Label B".into());
    let c = ctr("proj-c-1", Some("proj"));
    cfg.containers.insert(
        "proj-b-1".into(),
        ContainerOverride {
            name: Some("Zed".into()),
            icon: Some("cfg.png".into()),
            description: Some("The B service".into()),
            order: Some(-1),
            ..Default::default()
        },
    );
    cfg.containers.insert(
        "proj-c-1".into(),
        ContainerOverride {
            ..Default::default()
        },
    );
    let gs = build(&cfg, vec![a, b, c]);
    let titles: Vec<&str> = gs[0].cards.iter().map(|c| c.title.as_str()).collect();
    // order -1 first, then by name.
    assert_eq!(titles, vec!["Zed", "c", "Label Name"]);
    let z = card(&gs, "Zed");
    assert_eq!(z.icon.as_deref(), Some("cfg.png"));
    assert_eq!(z.sub.as_deref(), Some("The B service"));
    assert_eq!(card(&gs, "Label Name").icon.as_deref(), Some("label.svg"));
    assert_eq!(z.search, "Zed proj-b-1 proj img/proj-b-1:latest");
}

/// OV-25: a chip that is not a link (not HTTP) carries its protocol, and the
/// containers table shows it, so grey chips say what they are.
#[test]
fn plain_ports_say_their_protocol() {
    let mut u = port("0.0.0.0", 5353, 53);
    u.proto = "udp".into();
    let v = port_views(&[port("0.0.0.0", 5432, 5432), u], "homelab");
    assert_eq!((v[0].url.as_deref(), v[0].proto.as_str()), (None, "tcp"));
    assert_eq!((v[1].url.as_deref(), v[1].proto.as_str()), (None, "udp"));
    let mut c = ctr("db", None);
    c.ports = vec![port("0.0.0.0", 5432, 5432)];
    let st = crate::ui::testutil::state_with(vec![c], "");
    let h = hyper::HeaderMap::new();
    let ctx = crate::ui::shell::Ctx::new(&st, "h", "/containers", "", &h);
    let html = crate::ui::pages::render(&ctx).1;
    assert!(
        html.contains(r#"<span class="port plain" title="5432 → 5432/tcp on 0.0.0.0 (not a web address)">5432 <small>tcp</small></span>"#),
        "{}",
        &html[html.rfind("c-ports").unwrap_or(0)..][..600]
    );
}

#[test]
fn links_any_address_uses_page_host() {
    let ps = vec![port("0.0.0.0", 3000, 3000)];
    let v = port_views(&ps, "homelab");
    assert_eq!(v[0].url.as_deref(), Some("http://homelab:3000"));
    let v = port_views(&ps, "::1");
    assert_eq!(v[0].url.as_deref(), Some("http://[::1]:3000"));
    let v = port_views(&[port("::", 8080, 80)], "192.168.1.10");
    assert_eq!(v[0].url.as_deref(), Some("http://192.168.1.10:8080"));
}

#[test]
fn links_prefer_page_host_then_non_loopback() {
    // Tailscale + loopback, as most containers on the dev host.
    let ps = vec![
        port("100.64.0.10", 8125, 8025),
        port("127.0.0.1", 8125, 8025),
    ];
    let v = port_views(&ps, "192.168.1.10");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].url.as_deref(), Some("http://100.64.0.10:8125"));
    assert!(!v[0].local);
    assert_eq!(v[0].label, "8125");
    // Opened on localhost: the loopback binding is reachable.
    let v = port_views(&ps, "127.0.0.1");
    assert_eq!(v[0].url.as_deref(), Some("http://127.0.0.1:8125"));
    let v = port_views(&ps, "localhost");
    assert_eq!(v[0].url.as_deref(), Some("http://localhost:8125"));
    // Dozzle: Tailscale + LAN; match the one the page was opened on.
    let ps = vec![
        port("100.64.0.10", 8888, 8080),
        port("192.168.1.10", 8888, 8080),
    ];
    assert_eq!(
        port_views(&ps, "192.168.1.10")[0].url.as_deref(),
        Some("http://192.168.1.10:8888")
    );
    assert_eq!(
        port_views(&ps, "homelab")[0].url.as_deref(),
        Some("http://100.64.0.10:8888")
    );
}

#[test]
fn loopback_only_is_local_badge() {
    let v = port_views(&[port("127.0.0.1", 8011, 8000)], HOST);
    assert_eq!(v[0].url, None);
    assert!(v[0].local);
    assert_eq!(v[0].label, "8011");
    assert!(v[0].detail.contains("127.0.0.1"), "{}", v[0].detail);
}

#[test]
fn non_http_and_https_ports() {
    let mut udp = port("0.0.0.0", 53, 53);
    udp.proto = "udp".into();
    let ps = vec![
        port("100.64.0.10", 55432, 5432),
        port("0.0.0.0", 8443, 8443),
        port("0.0.0.0", 9000, 443),
        port("0.0.0.0", 1025, 1025),
        udp,
        port("0.0.0.0", 8080, 8080),
    ];
    let v = port_views(&ps, HOST);
    let urls: Vec<Option<&str>> = v.iter().map(|p| p.url.as_deref()).collect();
    assert_eq!(
        urls,
        vec![
            None,
            Some("https://100.64.0.10:8443"),
            Some("https://100.64.0.10:9000"),
            None,
            None,
            Some("http://100.64.0.10:8080"),
        ]
    );
    assert!(v.iter().all(|p| !p.local));
}

#[test]
fn main_link_precedence() {
    let mut cfg = Config::default();
    let mut a = ctr("a", None);
    a.ports = vec![
        port("127.0.0.1", 5000, 5000),
        port("100.64.0.10", 5000, 5000),
    ];
    let mut b = a.clone();
    b.name = "b".into();
    b.labels.insert(LABEL_URL.into(), "http://{host}/b".into());
    let mut c = b.clone();
    c.name = "c".into();
    cfg.containers.insert(
        "c".into(),
        ContainerOverride {
            url: Some("https://c.example/{name}/{id}".into()),
            ..Default::default()
        },
    );
    let mut d = ctr("d", None);
    d.ports = vec![port("127.0.0.1", 5432, 5432), port("0.0.0.0", 1, 5432)];
    let gs = build(&cfg, vec![a, b, c.clone(), d]);
    assert_eq!(
        card(&gs, "a").url.as_deref(),
        Some("http://100.64.0.10:5000")
    );
    assert_eq!(card(&gs, "b").url.as_deref(), Some("http://100.64.0.10/b"));
    let id12: String = c.id.chars().take(12).collect();
    assert_eq!(
        card(&gs, "c").url.clone().unwrap(),
        format!("https://c.example/c/{id12}")
    );
    assert_eq!(card(&gs, "d").url, None);
}

#[test]
fn stats_line() {
    let c = ctr("a", None);
    let mut stats = StatsMap::new();
    stats.insert(
        c.id.clone(),
        ContainerStats {
            cpu_percent: 1.234,
            mem_bytes: 51 * 1024 * 1024,
        },
    );
    let s = snap(vec![c.clone()]);
    let gs = build_groups(&Config::default(), &s, &stats, HOST);
    let a = card(&gs, "a");
    assert_eq!(a.cpu.map(|c| (c * 10.0).round()), Some(12.0));
    assert_eq!(a.mem, Some(51 * 1024 * 1024));
    let mut cfg = Config::default();
    cfg.page.show_stats = false;
    let gs = build_groups(&cfg, &s, &stats, HOST);
    assert_eq!(card(&gs, "a").cpu, None);
}

#[test]
fn restart_badge_from_die_times() {
    let mut c = ctr("a", None);
    c.die_times = vec![1];
    assert_eq!(
        card(&build(&Config::default(), vec![c.clone()]), "a").restarts,
        0
    );
    c.die_times = vec![1, 2, 3];
    assert_eq!(card(&build(&Config::default(), vec![c]), "a").restarts, 3);
}

#[test]
fn bookmarks() {
    let mut cfg = Config::default();
    cfg.bookmarks = vec![
        Bookmark {
            name: "GitHub".into(),
            url: "https://github.com/x".into(),
            ..Default::default()
        },
        Bookmark {
            name: "Beszel docs".into(),
            url: "https://beszel.dev".into(),
            group: Some("beszel".into()),
            description: Some("Docs".into()),
            ..Default::default()
        },
        Bookmark {
            name: "Router".into(),
            url: "http://{host}:81".into(),
            group: Some("Aaa links".into()),
            ..Default::default()
        },
    ];
    let gs = build(&cfg, sample());
    // Bookmark-only groups come after container groups.
    assert_eq!(
        names(&gs),
        vec![
            "beszel",
            "Standalone",
            "webshop-dev",
            "Aaa links",
            "Bookmarks"
        ]
    );
    let b = &gs[0];
    assert_eq!(b.counts(), "1 running · 1 link");
    assert_eq!(b.cards.last().unwrap().title, "Beszel docs");
    let gh = card(&gs, "GitHub");
    assert_eq!(gh.sub.as_deref(), Some("github.com"));
    assert_eq!(gh.dot, None);
    assert!(!gh.container);
    assert_eq!(
        card(&gs, "Router").url.as_deref(),
        Some("http://100.64.0.10:81")
    );
    assert_eq!(gs[4].counts(), "1 link");

    // Listed in cfg.groups: placed by config order.
    cfg.groups = vec![GroupConfig {
        name: "Bookmarks".into(),
        ..Default::default()
    }];
    assert_eq!(names(&build(&cfg, sample()))[0], "Bookmarks");
}

#[test]
fn docker_disabled_shows_only_bookmarks() {
    let mut cfg = Config::default();
    cfg.docker.enabled = false;
    assert!(build(&cfg, sample()).is_empty());
}

#[test]
fn stack_and_labels_are_set() {
    let gs = build(&Config::default(), sample());
    let web = card(&gs, "web");
    assert_eq!(web.stack, "webshop-dev");
    assert_eq!(web.name, "webshop-dev-web-1");
    assert_eq!(web.image, "img/webshop-dev-web-1:latest");
    assert_eq!(web.label, "running");
    assert_eq!(card(&gs, "job").label, "stopped");
    let mut c = ctr("a", None);
    for (state, health, want) in [
        ("running", Some("healthy"), "healthy"),
        ("running", Some("starting"), "starting"),
        ("running", Some("unhealthy"), "unhealthy"),
        ("restarting", None, "restarting"),
        ("paused", None, "paused"),
        ("created", None, "created"),
    ] {
        c.state = state.into();
        c.health = health.map(str::to_string);
        assert_eq!(status_label(&c), want);
    }
}

#[test]
fn card_for_finds_hidden_containers() {
    let mut cfg = Config::default();
    cfg.containers.insert(
        "demo-pg".into(),
        ContainerOverride {
            hide: Some(true),
            ..Default::default()
        },
    );
    let s = snap(sample());
    assert!(!build_groups(&cfg, &s, &StatsMap::new(), HOST)
        .iter()
        .flat_map(|g| &g.cards)
        .any(|c| c.name == "demo-pg"));
    let c = card_for(&cfg, &s, &StatsMap::new(), HOST, "demo-pg").unwrap();
    assert_eq!(c.stack, "Standalone");
    assert!(card_for(&cfg, &s, &StatsMap::new(), HOST, "nope").is_none());
}

#[test]
fn attention_reasons() {
    let gs = build(&Config::default(), sample());
    // A stopped container with a non-zero exit needs a look; a running one does not.
    assert_eq!(
        card(&gs, "job").attention(),
        Some((Sev::Warn, "stopped, exit 137".into()))
    );
    assert_eq!(card(&gs, "web").attention(), None);
    let clean = stopped(ctr("x", None), 0);
    let gs = build(&Config::default(), vec![clean]);
    assert_eq!(
        card(&gs, "x").attention(),
        None,
        "clean stop is a state, not a problem"
    );
    let mut sick = ctr("sick", None);
    sick.health = Some("unhealthy".into());
    let mut loopy = ctr("loopy", None);
    loopy.die_times = vec![1, 2, 3];
    let gs = build(&Config::default(), vec![sick, loopy]);
    assert_eq!(card(&gs, "sick").attention().unwrap().0, Sev::Bad);
    assert_eq!(
        card(&gs, "loopy").attention(),
        Some((Sev::Warn, "3 exits in the last hour".into()))
    );
}

#[test]
fn legacy_labels_keep_working_and_hobnail_labels_take_priority() {
    let cfg = Config::default();
    let mut c = ctr("legacy", None);
    for (suffix, value) in [
        ("hide", "true"),
        ("name", "Old name"),
        ("group", "Old group"),
        ("icon", "server"),
        ("url", "https://old.example"),
    ] {
        c.labels
            .insert(format!("rust-homepage.{suffix}"), value.into());
        assert_eq!(label(&c, &format!("hobnail.{suffix}")), Some(value));
    }
    assert!(
        is_excluded(&cfg, &c),
        "legacy hidden containers stay inaccessible"
    );
    c.labels.insert("hobnail.hide".into(), "false".into());
    assert!(!is_excluded(&cfg, &c));
    for (suffix, value) in [
        ("name", "New name"),
        ("group", "New group"),
        ("icon", "box"),
        ("url", "https://new.example"),
    ] {
        c.labels.insert(format!("hobnail.{suffix}"), value.into());
        assert_eq!(label(&c, &format!("hobnail.{suffix}")), Some(value));
    }
    c.labels.insert("hobnail.name".into(), " ".into());
    assert_eq!(label(&c, LABEL_NAME), Some("Old name"));
}
