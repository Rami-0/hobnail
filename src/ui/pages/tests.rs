//! Route tests: every page renders in the shell with a sample `AppState`,
//! unknown things are 404s in the shell, and Docker-supplied text is escaped.

use hyper::{HeaderMap, StatusCode};

use super::render;
use crate::ui::shell::Ctx;
use crate::ui::testutil::{ctr, sample, state_with};

const CFG: &str = r#"
[page]
title = "lab & co"

[[bookmarks]]
name = "GitHub"
url = "https://github.com"

[[widgets]]
id = "w1"
type = "custom-api"
title = "<b>W</b>"
url = "http://127.0.0.1:9/x"
template = "{{x}}"
[widgets.fields]
x = "x"

[[alerts]]
name = "ctr"
type = "container"
containers = ["*"]
"#;

fn get(path: &str, query: &str) -> (StatusCode, String) {
    let st = state_with(sample(), CFG);
    let h = HeaderMap::new();
    let ctx = Ctx::new(&st, "100.1.2.3", path, query, &h);
    render(&ctx)
}

fn assert_shell(html: &str) {
    assert!(
        html.starts_with("<!DOCTYPE html>"),
        "{}",
        &html[..80.min(html.len())]
    );
    assert!(html.contains(r#"<aside class="side" id="side""#));
    assert!(html.contains(r#"id="palette""#));
    assert!(html.contains(r#"<symbol id="i-box""#), "sprite present");
    assert!(html.contains("/static/app.js?v="));
    assert!(html.contains("<style>"), "CSS is inlined");
    assert!(html.contains(" · Hobnail</title>"), "product in tab title");
    assert!(
        html.contains("Hobnail<span class=\"brand-site\">"),
        "brand retains site identity"
    );
    assert!(!html.contains("rh-csrf"), "no csrf meta when signed out");
}

#[test]
fn every_page_renders_200_in_the_shell() {
    for (path, query) in [
        ("/", ""),
        ("/containers", ""),
        ("/containers", "state=running&sort=cpu&group=0"),
        ("/containers/web", ""),
        ("/containers/web", "range=24h"),
        ("/history", ""),
        ("/history", "range=30d&events=all"),
        ("/alerts", ""),
        ("/login", "next=%2Fcontainers"),
        ("/settings", ""),
        ("/settings/security", ""),
    ] {
        let (status, html) = get(path, query);
        assert_eq!(status, StatusCode::OK, "{path}?{query}");
        assert_shell(&html);
        assert!(html.contains("lab &amp; co"), "{path}: site title escaped");
    }
}

#[test]
fn unknown_paths_are_404_in_the_shell() {
    for path in [
        "/nope",
        "/containers/missing",
        "/containers/a/b",
        "/settings/nope",
    ] {
        let (status, html) = get(path, "");
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_shell(&html);
    }
    let (_, html) = get("/containers/missing", "");
    assert!(html.contains("No such container"));
    let (_, html) = get("/x%3Cscript%3E", "");
    assert!(!html.contains("<script>x"), "404 path escaped");
}

#[test]
fn container_names_are_escaped_everywhere() {
    for (path, query) in [
        ("/", ""),
        ("/containers", ""),
        ("/containers/%3Cscript%3Ealert(1)%3C%2Fscript%3E", ""),
    ] {
        let (status, html) = get(path, query);
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(
            !html.contains("<script>alert(1)"),
            "{path}: raw name leaked"
        );
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{path}"
        );
    }
    // Links to it are percent-encoded.
    let (_, html) = get("/containers", "");
    assert!(html.contains(r#"href="/containers/%3Cscript%3Ealert%281%29%3C%2Fscript%3E""#));
}

#[test]
fn detail_page_has_header_charts_and_rules() {
    let (_, html) = get("/containers/web", "");
    assert!(html.contains(r#"data-cd-name="web""#));
    assert!(html.contains(r#"id="ctr-live""#));
    assert!(
        html.contains("History is off") || html.contains("Collecting history"),
        "empty chart state"
    );
    assert!(html.contains(">ctr<"), "matching rule listed");
    assert!(
        !html.contains("Customize") && !html.contains(">Edit<"),
        "only when signed in"
    );
    // One view of logs: the Logs tab is the viewer; no header "Logs" button,
    // no Beszel button (OV-33, OV-34, OV-35).
    assert!(
        html.contains(r#"<a href="/containers/web/logs?range=6h">Logs</a>"#),
        "Logs tab (keeps the range)"
    );
    assert!(!html.contains("tab=logs"));
    assert!(!html.contains("Beszel") && !html.contains("Open logs of"));
    let head = &html[html.find("class=\"ctr-actions\"").unwrap()..];
    let head = &head[..head.find("</header>").unwrap()];
    assert!(
        !head.contains("/logs") && !head.contains("btn primary"),
        "{head}"
    );
    assert!(
        html.contains("http://100.1.2.3:8080"),
        "port link uses page host"
    );
}

#[test]
fn login_page_keeps_only_safe_next() {
    let (_, html) = get("/login", "next=%2F%09%2Fevil.example");
    assert!(
        html.contains(r#"name="next" value="/""#),
        "bad next dropped"
    );
    let (_, html) = get("/login", "next=%2Fcontainers%3Fstate%3Drunning");
    assert!(html.contains(r#"name="next" value="/containers?state=running""#));
    assert!(html.contains(r#"action="/api/login""#));
    assert!(html.contains("set-password"), "explains the first password");
}

#[test]
fn settings_signed_out_explains_set_password() {
    let (_, html) = get("/settings", "");
    assert!(html.contains("docker exec -it hobnail /hobnail set-password"));
    // Signed out: the lock panel only, no tab strip (Y11).
    for s in crate::ui::routes::SETTINGS {
        assert!(
            !html.contains(&format!(r#"href="/settings/{}""#, s.1)),
            "{}",
            s.0
        );
    }
}

#[test]
fn bundles_cover_live_regions() {
    let st = state_with(sample(), CFG);
    let ctx = Ctx::live(&st, "h");
    let g = super::bundle(&ctx, "groups").unwrap().into_string();
    for id in [
        "tree",
        "tree-cnt",
        "ov-health",
        "ov-attn",
        "ov-top",
        "ctr-live",
    ] {
        assert!(
            g.contains(&format!("id=\"{id}\"")),
            "groups bundle has {id}"
        );
    }
    assert!(!g.contains("<script>alert"));
    assert!(
        !g.contains("id=\"ctable\""),
        "tables are fetched on structural change only"
    );
    assert!(g.contains("&quot;sig&quot;"));
    let s = super::bundle(&ctx, "sys").unwrap().into_string();
    assert!(s.contains("id=\"vitals\"") && s.contains("id=\"ov-sys\""));
    let a = super::bundle(&ctx, "alerts").unwrap().into_string();
    assert!(a.contains("id=\"nav-al\"") && a.contains("id=\"al-live\""));
    let w = super::bundle(&ctx, "w-w1").unwrap().into_string();
    assert!(w.contains("id=\"w-w1\"") && w.contains("&lt;b&gt;W&lt;/b&gt;"));
    assert!(super::bundle(&ctx, "w-nope").is_none());
    assert!(super::bundle(&ctx, "bogus").is_none());
}

#[test]
fn stopped_containers_and_filters() {
    let st = state_with(
        vec![ctr("a", None, "running"), ctr("b", None, "exited")],
        "",
    );
    let h = HeaderMap::new();
    let ctx = Ctx::new(&st, "h", "/containers", "state=stopped", &h);
    let (_, html) = render(&ctx);
    // The running one is kept in the hidden pool for the script.
    let pool = html.split("<tbody class=\"pool\" hidden>").nth(1).unwrap();
    assert!(pool.contains(r#"data-n="a""#), "running row pooled");
    assert!(html.contains(r#"<tr class="row rl stopped""#));
}
