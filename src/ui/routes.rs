//! Page routes (GET): `/`, `/containers`, `/containers/<name>`, `/history`,
//! `/alerts`, `/login`, `/settings/*`, the command palette's data
//! (`/api/palette`) and the 404 page in the shell.
//!
//! `http::server::route` sends every GET that no other module claimed here.

use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::http::server::{full, set, text, Body};
use crate::state::AppState;
use crate::ui::fmt::url_encode;
use crate::ui::pages;
use crate::ui::shell::Ctx;
use crate::ui::view::build_groups;

fn html(status: StatusCode, body: String) -> Response<Body> {
    let mut r = crate::http::server::html(body);
    *r.status_mut() = status;
    r
}

pub async fn route(
    state: &Arc<AppState>,
    req: &hyper::Request<hyper::body::Incoming>,
    host: &str,
) -> Response<Body> {
    let path = req.uri().path();
    if path == "/api/palette" {
        return palette(state, host);
    }
    // Machine endpoints get a plain answer, people get a page.
    if path.starts_with("/api/") || path.starts_with("/static/") || path.starts_with("/icons/") {
        return text(StatusCode::NOT_FOUND, "not found\n");
    }
    // The containers table alone, for the script to refetch when the set of
    // containers or their state changed (see `pages::container::live_json`).
    if path == "/containers" && req.uri().query() == Some("frag=table") {
        let h = hyper::HeaderMap::new();
        let ctx = Ctx::new(state, host, "/containers", "", &h);
        let v = pages::overview::Live::new(&ctx);
        let q = pages::containers::ListQuery::default();
        let body = pages::containers::table(&ctx, &v.snap, &v.groups, &q).into_string();
        return html(StatusCode::OK, body);
    }
    // Settings pages carry form values and the CSRF token (signed in), and
    // their fragments the lists: never stored by a cache.
    let no_store = path == "/settings" || path.starts_with("/settings/");
    let (path, query) = (
        path.to_string(),
        req.uri().query().unwrap_or("").to_string(),
    );
    let (st, host, headers) = (state.clone(), host.to_string(), req.headers().clone());
    let job = move || {
        let ctx = Ctx::new(&st, &host, &path, &query, &headers);
        // `/settings/<tab>?frag=…`: built from this request's session (401
        // without one), never from an empty HeaderMap like the table above.
        if let Some(r) = crate::ui::settings::fragment(&ctx) {
            return r;
        }
        pages::render(&ctx)
    };
    // History queries are synchronous SQLite calls: keep them off the reactor.
    let (status, body) = if state.db.is_enabled() {
        match tokio::task::spawn_blocking(job).await {
            Ok(r) => r,
            Err(_) => return text(StatusCode::INTERNAL_SERVER_ERROR, "render failed\n"),
        }
    } else {
        job()
    };
    let mut r = html(status, body);
    if no_store {
        set(&mut r, CACHE_CONTROL, "no-store");
    }
    r
}

/// `GET /api/palette`: what the command palette can jump to.
pub fn palette(state: &AppState, host: &str) -> Response<Body> {
    let v = palette_json(state, host);
    let mut r = full(StatusCode::OK, serde_json::to_vec(&v).unwrap_or_default());
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-cache");
    r
}

const PAGES: &[(&str, &str, &str)] = &[
    ("Overview", "/", "grid"),
    ("Containers", "/containers", "box"),
    ("History", "/history", "history"),
    ("Host", "/host", "server"),
    ("Logs", "/logs", "logs"),
    ("Alerts", "/alerts", "bell"),
    ("Settings", "/settings", "sliders"),
];

/// Settings sections (DESIGN.md section 5; the pages arrive with the settings work).
pub const SETTINGS: &[(&str, &str, &str)] = &[
    (
        "General",
        "general",
        "Title, greeting, search, what the overview shows",
    ),
    (
        "Containers and groups",
        "containers",
        "Hidden containers, groups, per-container overrides",
    ),
    ("Bookmarks", "bookmarks", "Extra links"),
    ("Widgets", "widgets", "Custom API widgets, system tiles"),
    ("Notifications", "notifications", "Channels and alert rules"),
    ("Appearance", "appearance", "Theme and custom.css"),
    (
        "History and data",
        "history",
        "Recording, sample interval, database",
    ),
    ("Config file", "config", "Raw TOML editor and revisions"),
    ("Security", "security", "Password and sessions"),
];

/// Items are `{t, n, s?, u?, d?, l?, k?, r?}`: type (`p` page, `s` settings,
/// `c` container, `b` bookmark), name, secondary text, URL, details URL, logs
/// URL, extra search keywords, running.
pub fn palette_json(state: &AppState, host: &str) -> Value {
    let cfg = state.config.current();
    let snap = state.snapshot();
    let groups = build_groups(&cfg, &snap, &Default::default(), host);
    let mut items: Vec<Value> = Vec::new();
    for (n, u, _) in PAGES {
        items.push(json!({"t": "p", "n": n, "u": u}));
    }
    for (n, slug, _) in SETTINGS {
        items.push(json!({"t": "s", "n": n, "s": "Settings", "u": format!("/settings/{slug}")}));
    }
    for g in &groups {
        for c in &g.cards {
            if c.container {
                let details = format!("/containers/{}", url_encode(&c.name));
                items.push(json!({
                    "t": "c", "n": c.title, "s": g.title, "d": details, "u": c.url,
                    "l": c.logs_url, "k": c.search, "r": c.running,
                }));
            } else if let Some(u) = &c.url {
                items.push(json!({"t": "b", "n": c.title, "s": g.title, "u": u, "k": c.search}));
            }
        }
    }
    json!({ "items": items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Container;
    use crate::ui::testutil::state_with;

    #[test]
    fn palette_lists_pages_settings_containers_and_bookmarks() {
        let c = Container {
            id: "a".repeat(64),
            name: "web<1>".into(),
            image: "nginx".into(),
            state: "running".into(),
            ..Default::default()
        };
        let st = state_with(
            vec![c],
            "[[bookmarks]]\nname = \"GitHub\"\nurl = \"https://github.com\"\n",
        );
        let v = palette_json(&st, "h");
        let items = v["items"].as_array().unwrap();
        assert!(items.iter().any(|i| i["t"] == "p" && i["u"] == "/history"));
        assert!(items
            .iter()
            .any(|i| i["t"] == "s" && i["u"] == "/settings/security"));
        assert!(items
            .iter()
            .any(|i| i["t"] == "c" && i["d"] == "/containers/web%3C1%3E"));
        assert!(items.iter().any(|i| i["t"] == "b" && i["n"] == "GitHub"));
    }
}
