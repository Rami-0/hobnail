//! Settings pages: every tab renders in the shell; signed out they show only
//! the lock panel (no config data); signed in, command widgets are listed
//! read-only with no controls.

use hyper::header::HeaderValue;
use hyper::{HeaderMap, StatusCode};
use std::sync::Arc;

use crate::state::AppState;
use crate::ui::pages::render;
use crate::ui::routes::SETTINGS;
use crate::ui::shell::Ctx;
use crate::ui::testutil::{sample, state_with};

const FIXTURE: &str = include_str!("../../settings/fixtures/config.toml");

fn get(st: &AppState, path: &str, h: &HeaderMap) -> (StatusCode, String) {
    let ctx = Ctx::new(st, "100.1.2.3", path, "", h);
    render(&ctx)
}

/// A valid session cookie for `st` (stored like `auth` does: its SHA-256).
fn signed_in(st: &Arc<AppState>) -> HeaderMap {
    let token = "ab".repeat(32);
    let hash: String = ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let now = crate::util::unix_now() as i64;
    assert!(
        st.db.create_session(&hash, now, now + 3600),
        "history database needed for sessions"
    );
    let mut h = HeaderMap::new();
    h.insert(
        "cookie",
        HeaderValue::from_str(&format!("rh_session={token}")).unwrap(),
    );
    h
}

fn paths() -> Vec<String> {
    let mut v = vec!["/settings".to_string()];
    v.extend(SETTINGS.iter().map(|s| format!("/settings/{}", s.1)));
    v
}

#[test]
fn signed_out_every_tab_is_locked_and_leaks_nothing() {
    let st = state_with(sample(), FIXTURE);
    let h = HeaderMap::new();
    for p in paths() {
        let (status, html) = get(&st, &p, &h);
        assert_eq!(status, StatusCode::OK, "{p}");
        assert!(html.contains("set-lock"), "{p}: lock panel");
        assert!(
            html.contains("docker exec -it hobnail /hobnail set-password"),
            "{p}: first-password command"
        );
        assert!(html.contains("Docker socket"), "{p}: says why");
        assert!(!html.contains("rh-csrf"), "{p}: no csrf when signed out");
        // Nothing from the config file: no widget URLs, no editors.
        for leak in [
            "100.64.0.10:8090/api/health",
            "/host/proc",
            "broken-demo",
            "f-raw",
            "f-widget",
        ] {
            assert!(!html.contains(leak), "{p}: leaked {leak}");
        }
        assert!(html.contains("/static/settings.css?v="), "{p}");
        assert!(html.contains("/static/settings.js?v="), "{p}");
    }
    // Once a password exists, the panel links to the login with `next`.
    st.db.kv_set("auth.password", "pbkdf2-sha256$1$00$00");
    let (_, html) = get(&st, "/settings/widgets", &h);
    assert!(html.contains("Sign in to change settings"), "{html}");
    assert!(html.contains(r#"href="/login?next=%2Fsettings%2Fwidgets""#));
    assert!(!html.contains("set-password"));
}

#[test]
fn unknown_tab_is_404() {
    let st = state_with(sample(), FIXTURE);
    let (status, _) = get(&st, "/settings/nope", &HeaderMap::new());
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn signed_in_tabs_render_editors() {
    let st = state_with(sample(), FIXTURE);
    if !st.db.is_enabled() {
        eprintln!("skipped: no history database");
        return;
    }
    let h = signed_in(&st);
    for p in paths() {
        let (status, html) = get(&st, &p, &h);
        assert_eq!(status, StatusCode::OK, "{p}");
        assert!(!html.contains("set-lock"), "{p}: no lock panel");
        assert!(html.contains(r#"<meta name="rh-csrf""#), "{p}");
        assert!(html.contains(r#"data-signed="1""#), "{p}");
    }
    let (_, html) = get(&st, "/settings/config", &h);
    assert!(html.contains(r#"id="raw-text""#));
    let (_, html) = get(&st, "/settings/general", &h);
    for f in [
        "title",
        "greeting",
        "columns",
        "layout",
        "theme",
        "search_url",
        "show_stopped",
        "show_stats",
    ] {
        assert!(html.contains(&format!(r#"name="{f}""#)), "general: {f}");
    }
    // Every input has a label.
    for p in paths() {
        let (_, html) = get(&st, &p, &h);
        for id in html.split(r#"<input class="in"#).skip(1).filter_map(|s| {
            let i = s.find(r#"id=""#)? + 4;
            Some(s[i..i + s[i..].find('"')?].to_string())
        }) {
            assert!(
                html.contains(&format!(r#"for="{id}""#)),
                "{p}: no label for {id}"
            );
        }
    }
}

#[test]
fn command_widgets_are_read_only_with_no_controls() {
    let st = state_with(sample(), FIXTURE);
    if !st.db.is_enabled() {
        return;
    }
    let h = signed_in(&st);
    let (status, html) = get(&st, "/settings/widgets", &h);
    assert_eq!(status, StatusCode::OK);
    let start = html.find(r#"id="w-locked""#).expect("locked section");
    let end = start + html[start..].find("</section>").unwrap();
    let locked = &html[start..end];
    assert!(locked.contains(r#"data-id="procs""#), "{locked}");
    assert!(
        locked.contains("command widgets can&#39;t be created")
            || locked.contains("command widgets can't be created"),
        "{locked}"
    );
    assert!(locked.contains("i-lock"), "lock icon");
    for control in [
        "<button",
        "<input",
        "<select",
        "<textarea",
        "data-act",
        "<form",
    ] {
        assert!(!locked.contains(control), "locked section has {control}");
    }
    // custom-api widgets are not in the locked list.
    assert!(!locked.contains(r#"data-id="beszel""#));
    // The editor has no fields for command, env or secret headers.
    for f in [
        r#"name="command""#,
        r#"name="env""#,
        r#"name="headers_env""#,
        r#"name="headers_file""#,
    ] {
        assert!(!html.contains(f), "widget form offers {f}");
    }
}
