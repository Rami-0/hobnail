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
    assert!(
        html.contains(r#"<link rel="stylesheet" href="/static/app.css?v="#)
            && !html.contains("<style>"),
        "the stylesheet is one cached file"
    );
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

/// Wave 5 on the detail page: the title is not repeated in the sub-line
/// (ROB-08), every tab keeps the range (OV-37), the strip has an id for the
/// scroll cue (OV-36), ports stack on phones (OV-40), a container without a
/// health check has four tiles and no Health tile (OV-42, OV-46), the
/// Metrics link line is gone (OV-48), and a visitor sees "Sign in to view"
/// links instead of quoted placeholders (OV-56).
#[test]
fn detail_page_wave5() {
    let (_, html) = get("/containers/web", "range=24h");
    let sub = &html[html.find("<p class=\"mono muted\">").unwrap()..];
    let sub = &sub[..sub.find("</p>").unwrap()];
    assert!(!sub.contains("web ·"), "name not repeated: {sub}");
    assert!(
        sub.contains("img/web:latest") && sub.contains(" · id "),
        "{sub}"
    );
    assert!(html.contains(r#"<span class="ctr-t">web</span>"#));
    for t in ["overview", "metrics", "inspect", "processes", "events"] {
        assert!(
            html.contains(&format!(r#"href="/containers/web?tab={t}&amp;range=24h""#)),
            "{t} keeps the range"
        );
    }
    assert!(html.contains(r#"class="tbl small cd-stack""#) && html.contains(r#"data-l="Link""#));
    assert!(html.contains(r#"class="strip facts n4""#), "four tiles");
    assert!(!html.contains(r#"data-cd="h""#) && html.contains("Restarts (1 h)"));
    assert!(!html.contains("on the Metrics tab"));
    let (_, ins) = get("/containers/web", "tab=inspect");
    assert!(
        !ins.contains(crate::ui::pages::container::HIDDEN),
        "no quoted placeholder"
    );
    let (_, ev) = get("/containers/web", "tab=events");
    assert!(
        !ev.contains(r#"aria-label="Kind""#),
        "no filters over nothing"
    );
    // A container with a health check: State says running, Health has the word.
    let mut c = ctr("api", Some("shop"), "running");
    c.health = Some("unhealthy".into());
    let st = state_with(vec![c], "");
    let h = page_of(&st, "/containers/api", "");
    assert!(h.contains(r#"data-cd="l" data-run>running<"#), "state tile");
    assert!(h.contains(r#"data-cd="h">unhealthy<"#), "health tile");
    assert!(h.contains(r#"class="strip facts""#));
}

/// ROB-07: a long stack name is one ellipsised line with the full name in
/// its title; OV-19: phones get a sort control in the toolbar.
#[test]
fn containers_table_long_stack_and_sort_control() {
    let long = format!("a-very-long-compose-project-{}", "y".repeat(80));
    let st = state_with(vec![ctr("web", Some(&long), "running")], "");
    let html = page_of(&st, "/containers", "");
    assert!(
        html.contains(&format!(r#"<span class="grp-n" title="{long}">"#)),
        "title"
    );
    assert!(html.contains(r#"id="csort" name="sort""#) && html.contains(r#"id="cdir""#));
    assert_eq!(
        html.matches(r#"name="sort""#).count(),
        1,
        "one sort field in the form"
    );
    assert!(html.contains(r#"<span class="coll"></span>"#));
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

fn page_of(st: &crate::state::AppState, path: &str, query: &str) -> String {
    let h = HeaderMap::new();
    render(&Ctx::new(st, "h", path, query, &h)).1
}

/// Wave 4 (DESIGN.md §11.9): every empty and error state is the one
/// component and is rendered by the server, so it shows with the script
/// blocked; nothing of it waits for JavaScript.
#[test]
fn empty_states_are_server_rendered() {
    // An empty host: first use, with what Hobnail lists (ROB-01).
    let st = state_with(vec![], "");
    let ov = page_of(&st, "/", "");
    assert!(ov.contains("No containers yet"), "overview: no containers");
    assert!(!ov.contains("All clear"), "no green tick for 0 containers");
    assert!(
        !ov.contains("Waiting for the first sample"),
        "nothing to wait for"
    );
    let ct = page_of(&st, "/containers", "");
    assert!(
        ct.contains(r#"<div class="empty"><span class="empty-ic">"#),
        "{ct}"
    );
    assert!(ct.contains("No containers yet"));
    // Alerts without rules: plain words and one real action (ROB-02).
    let al = page_of(&st, "/alerts", "");
    assert!(
        al.contains("No alert rules") && !al.contains("[[alerts]]"),
        "no TOML for a visitor"
    );
    // No password yet: no sign-in button that cannot work, the way to set one.
    assert!(!al.contains("Sign in to add a rule") && al.contains("hobnail set-password"));
    // With one: one real action that ends in the "Add rule" dialog.
    let html = super::alerts::tests_public(true);
    assert!(
        html.contains(r#"href="/login?next=%2Falerts%3Fadd%3Drule""#),
        "{html}"
    );

    // A state filter that empties the table is good news (OV-24).
    let st = state_with(sample(), CFG);
    let ct = page_of(&st, "/containers", "state=stopped");
    for w in [
        "Nothing is stopped",
        "No problems",
        "No containers match",
        r#"data-clear"#,
    ] {
        assert!(ct.contains(w), "{w}");
    }
    assert!(
        ct.contains(r#"data-why="stopped""#) || ct.contains(r#"hidden data-why="#),
        "{ct}"
    );
    let ct = page_of(&st, "/containers", "q=zzz-none");
    assert!(ct.contains(r#"data-why="filter""#));
}

/// Docker cannot be reached: one plain banner on every page, the raw error
/// behind "Details", in a live region (ROB-09).
#[test]
fn docker_unreachable_is_said_on_every_page() {
    let st = state_with(vec![], "");
    st.containers
        .send_replace(std::sync::Arc::new(crate::model::Snapshot {
            containers: vec![],
            fetched_at: 1,
            error: Some("cannot connect to /var/run/docker.sock".into()),
        }));
    for p in [
        "/",
        "/containers",
        "/host",
        "/history",
        "/alerts",
        "/settings",
    ] {
        let html = page_of(&st, p, "");
        let at = html
            .find(r#"<div id="dk-err">"#)
            .unwrap_or_else(|| panic!("{p}: no banner region"));
        let b = &html[at..at + 600.min(html.len() - at)];
        assert!(b.contains("Hobnail cannot reach Docker."), "{p}: {b}");
        assert!(
            b.contains("<details") && b.contains("docker.sock"),
            "{p}: raw error behind Details"
        );
    }
    // While Docker is down, /alerts gives no good news in green: container
    // rules cannot be evaluated.
    let al = page_of(&st, "/alerts", "");
    assert!(
        al.contains("Alerts cannot be evaluated for containers") && !al.contains("empty good"),
        "{al}"
    );
    let ov = page_of(&st, "/", "");
    assert!(!ov.contains("All clear") && ov.contains("Unknown while Docker cannot be reached"));
    assert_eq!(
        page_of(&st, "/containers", "")
            .matches("Hobnail cannot reach Docker")
            .count(),
        1,
        "said once"
    );
    // It is a live region: the "groups" bundle carries it, empty when fine.
    let h = HeaderMap::new();
    let b = super::bundle(&Ctx::live(&st, "h"), "groups")
        .unwrap()
        .into_string();
    assert!(
        b.contains(r#"<div id="dk-err"><div class="banner bad""#),
        "{b}"
    );
    let ok = state_with(vec![], "");
    let b = super::bundle(&Ctx::new(&ok, "h", "", "", &h), "groups")
        .unwrap()
        .into_string();
    assert!(b.contains(r#"<div id="dk-err"></div>"#), "{b}");
}

/// The "alerts" bundle reads nothing from the database for a config without
/// rules and channels: `#al-pub` is then static and left out (it changes
/// with the config, which reloads the page).
#[test]
fn alerts_bundle_leaves_out_a_static_empty_state() {
    let st = state_with(sample(), "");
    let b = super::bundle(&Ctx::live(&st, "h"), "alerts")
        .unwrap()
        .into_string();
    assert!(
        b.contains(r#"id="al-live""#) && !b.contains(r#"id="al-pub""#),
        "{b}"
    );
    let st = state_with(sample(), CFG);
    let b = super::bundle(&Ctx::live(&st, "h"), "alerts")
        .unwrap()
        .into_string();
    assert!(
        b.contains(r#"<div id="al-pub">"#) && b.contains(r#" data-key="ctr">"#),
        "{b}"
    );
    let p = page_of(&state_with(sample(), ""), "/alerts", "");
    assert!(
        p.contains(r#"<div id="al-pub">"#) && p.contains("No alert rules"),
        "{p}"
    );
}

/// Content that arrives after the page waits as a skeleton on a region the
/// script makes busy; the server never marks anything busy, so without
/// the script it is a plain word and nothing is busy (§11.9, ROB-14).
#[test]
fn loading_states_are_skeletons_on_busy_regions() {
    let st = state_with(sample(), CFG);
    let ov = page_of(&st, "/", "");
    let w = &ov[ov.find(r#"id="w-w1""#).expect("widget tile")..];
    let w = &w[..w.find("</div></div>").unwrap_or(w.len())];
    assert!(
        w.contains(r#"data-wait="""#) && w.contains(r#"<span class="sk"></span>"#),
        "{w}"
    );
    assert!(
        !ov.contains(r#"aria-busy=""#),
        "nothing is busy from the server"
    );
    // The skeleton is switched on by the class the loaded script sets, not
    // by the boot script's `.js` (a blocked /static/app.js keeps the word).
    let css = crate::http::server::APP_CSS;
    assert!(css.contains("html:not(.hb-ready) .sk{display:none}"));
    assert!(css.contains(".hb-ready .sk-t{position:absolute"));
    assert!(!css.contains(".js .sk-t") && !css.contains("html:not(.js) .sk"));
    let js = crate::http::server::APP_JS;
    assert!(js.contains(r#"root.classList.add("hb-ready");"#) && js.contains("HB.busy(doc.body)"));
    // Its own marker: `data-busy` already carries a button's busy label in
    // Settings, which must not make that button busy on load.
    assert!(
        js.contains(r#"querySelectorAll("[data-wait]")"#)
            && !js.contains(r#"querySelectorAll("[data-busy]")"#)
    );
    assert!(
        w.contains(r#"<span class="sk-t">Waiting for the first answer…</span>"#),
        "{w}"
    );
    // The log pane before its first lines (signed in only, so the page
    // here is the sign-in state; the viewer markup is checked in logs).
    let lg = page_of(&st, "/containers/web/logs", "");
    // (No password is set here: the owner's setup sentence; with one it is a
    // "Sign in" button, checked in the browser by motion-check.)
    assert!(
        lg.contains("Sign in to read logs") && lg.contains("Logs often hold passwords and tokens"),
        "LG-11: plain words"
    );
    assert!(
        !lg.contains("[logs] require_login"),
        "no config syntax quoted as a reason (LG-11)"
    );
}

/// Page changes are one swap (ux-pass, "flicker"): on every route, signed
/// in and out, error pages included, the icon sprite comes before the first
/// icon that uses it; the head asks Chromium to hold the first paint until
/// `#hb-end`, the document's last element, is parsed; and the live
/// indicator starts as "live" (no flip on every load).
#[test]
fn every_route_paints_once_with_its_icons() {
    let st = state_with(sample(), CFG);
    let h = HeaderMap::new();
    for signed in [false, true] {
        for (path, query) in [
            ("/", ""),
            ("/containers", ""),
            ("/containers/web", ""),
            ("/containers/web", "tab=metrics&range=24h"),
            ("/containers/web/logs", ""),
            ("/stacks/shop/logs", ""),
            ("/logs", ""),
            ("/host", ""),
            ("/history", ""),
            ("/alerts", ""),
            ("/login", ""),
            ("/more", ""),
            ("/settings", ""),
            ("/settings/notifications", ""),
            ("/settings/nope", ""),
            ("/containers/missing", ""),
            ("/nope", ""),
        ] {
            let mut ctx = Ctx::new(&st, "100.1.2.3", path, query, &h);
            if signed {
                ctx.user = Some("admin".into());
                ctx.csrf = Some("t".into());
            }
            let (status, html) = render(&ctx);
            let at = format!("{path}?{query} signed={signed} ({status})");
            let head_end = html.find("</head>").expect("head");
            let head = &html[..head_end];
            assert!(
                head.contains(r##"<link rel="expect" href="#hb-end" blocking="render">"##),
                "{at}"
            );
            // The app script is deferred and never holds the paint (a slow
            // or failing script must not keep a page blank); it comes after
            // the pages' own scripts, which settings.js relies on (it wraps
            // EventSource before app.js opens the stream).
            assert_eq!(html.matches(r#"blocking="render""#).count(), 1, "{at}");
            let app = html.find("/static/app.js?v=").expect("app script");
            assert!(
                app > head_end && html[app..].contains(r#"" defer></script><div id="hb-end""#),
                "{at}"
            );
            for own in ["/static/settings.js", "/static/logs.js"] {
                if let Some(i) = html.find(own) {
                    assert!(i < app, "{at}: {own} runs before app.js");
                }
            }
            assert_eq!(html.matches(r#"id="hb-end""#).count(), 1, "{at}");
            assert!(
                html.ends_with(r#"<div id="hb-end" hidden></div></body></html>"#),
                "{at}: #hb-end is the last element"
            );
            let body = html.find("<body ").expect("body");
            let sprite = html.find("<svg xmlns=").expect("sprite");
            let first_use = html.find("<use href=").expect("an icon");
            assert!(body < sprite && sprite < first_use, "{at}: sprite first");
            assert!(
                html[body..sprite].matches('<').count() == 1,
                "{at}: the sprite is the body's first child"
            );
            assert!(
                html.contains(r#"<span class="dot ok"></span><span class="live-t">live</span>"#),
                "{at}"
            );
            assert!(!html.contains(">connecting<"), "{at}");
        }
    }
}
