//! Integration tests: config parsing and validation, the delivery queue,
//! and the whole task against the mock receiver (webhook, Slack and
//! Telegram on one ephemeral port).

use super::*;
use crate::model::Container;
use crate::state::Env;
use serde_json::Value;

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-alerts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(dir: &std::path::Path, config: &str) -> Arc<AppState> {
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, config).unwrap();
    let st = AppState::new(Env {
        listen: vec![],
        data_dir: dir.to_path_buf(),
        config_path,
        docker_socket: "/nonexistent.sock".into(),
        host_proc: "/proc".into(),
        host_sys: "/sys".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    });
    assert!(
        st.config.snapshot().error.is_none(),
        "{:?}",
        st.config.snapshot().error
    );
    st
}

fn container(name: &str, state: &str, exit: Option<i64>) -> Container {
    Container {
        name: name.into(),
        state: state.into(),
        exit_code: exit,
        ..Default::default()
    }
}

fn publish(st: &AppState, cs: Vec<Container>) {
    st.publish_containers(Snapshot {
        containers: cs,
        fetched_at: unix_now(),
        error: None,
    });
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..400 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Give the task time to (not) do something.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn parse(src: &str) -> Result<Config, String> {
    crate::config::parse(src, "c.toml")
}

// ------------------------------------------------------------------ config

const EXAMPLE: &str = r#"
[channels.ops-webhook]
type = "webhook"
url_env = "RH_WEBHOOK_URL"
headers = { "X-Source" = "hobnail" }
headers_env = { Authorization = "RH_WEBHOOK_AUTH" }
body = '{"title":"{{title}}","text":"{{message}}"}'

[channels.plain]
type = "webhook"
url = "http://127.0.0.1:9999/hook"

[channels.slack]
type = "slack"
webhook_url_env = "RH_SLACK_WEBHOOK_URL"

[channels.telegram]
type = "telegram"
token_file = "/data/secrets/telegram"
chat_id = "123456"

[[alerts]]
name = "container-down"
type = "container"
containers = ["*"]
exclude = ["*-e2e-*"]
on = ["stopped", "exit_nonzero", "unhealthy", "restarting"]
restarts = 3
window = "10m"
for = "10s"
cooldown = "15m"
channels = ["ops-webhook", "slack", "telegram"]
message = "🔴 {{subject}}: {{state}} ({{value}}) at {{time}}"
recovery = "✅ {{subject}} recovered at {{time}}"

[[alerts]]
name = "odoo-up"
type = "http"
url = "http://100.64.0.10:8070/web/health"
interval = "60s"
timeout = "5s"
expect_status = 200
failures = 2
channels = ["slack"]

[[alerts]]
name = "disk"
type = "host"
metric = "disk"
above = 90
for = "5m"
channels = ["telegram"]
"#;

#[test]
fn example_config_parses() {
    let c = parse(EXAMPLE).unwrap();
    assert_eq!(c.channels.len(), 4);
    assert_eq!(c.alerts.len(), 3);
    let r = &c.alerts[0];
    assert_eq!(r.kind, RuleKind::Container);
    assert_eq!(r.params().for_secs, 10);
    assert_eq!(r.params().cooldown_secs, 900);
    assert_eq!(c.alerts[2].params().for_secs, 300);
    assert_eq!(c.channels["telegram"].kind, ChannelKind::Telegram);
    // Defaults.
    let c = parse("[[alerts]]\nname='a'\ntype='container'").unwrap();
    assert_eq!(c.alerts[0].params().for_secs, 30);
    assert!(c.alerts[0].channels.is_empty());
}

#[test]
fn validation_errors_name_the_rule_or_channel() {
    let cases: &[(&str, &[&str])] = &[
        (
            "[[alerts]]\nname='x'\ntype='container'\nchannels=['nope']",
            &["alert \"x\"", "unknown channel \"nope\""],
        ),
        (
            "[[alerts]]\nname='x'\ntype='container'\non=['stoped']",
            &["alert \"x\"", "stoped"],
        ),
        (
            "[[alerts]]\nname='x'\ntype='container'\nurl='http://a'",
            &["alert \"x\"", "`url` does not apply"],
        ),
        ("[[alerts]]\nname='x'\ntype='http'", &["url"]),
        (
            "[[alerts]]\nname='x'\ntype='http'\nurl='ftp://a'",
            &["invalid url"],
        ),
        (
            "[[alerts]]\nname='x'\ntype='http'\nurl='http://a'\ninterval='10s'\ntimeout='30s'",
            &["timeout"],
        ),
        ("[[alerts]]\nname='x'\ntype='host'\nmetric='disk'", &["exactly one of above, below"]),
        ("[[alerts]]\nname='x'\ntype='host'\nmetric='load'\nabove=1", &["load"]),
        (
            "[[alerts]]\nname='x'\ntype='widget'\nwidget='mailpit'\nfield='n'\nabove=1",
            &["unknown widget \"mailpit\""],
        ),
        (
            "[[alerts]]\nname='x'\ntype='container'\nmessage='{{subjekt}}'",
            &["subjekt"],
        ),
        ("[[alerts]]\nname='x'\ntype='container'\nfor='2d'", &["for"]),
        (
            "[[alerts]]\nname='x'\ntype='container'\nwindow='2h'",
            &["window"],
        ),
        (
            "[[alerts]]\nname='x'\ntype='container'\n[[alerts]]\nname='x'\ntype='host'\nmetric='cpu'\nabove=1",
            &["duplicate alert name"],
        ),
        ("[[alerts]]\nname='x'\ntype='container'\ncooldwn='1m'", &["cooldwn"]),
        ("[[alerts]]\nname='x'\ntype='cointainer'", &["cointainer"]),
        (
            "[channels.s]\ntype='slack'\nwebhook_url='https://hooks.slack.com/x'",
            &["webhook_url"],
        ),
        ("[channels.t]\ntype='telegram'\ntoken_env='T'", &["channel \"t\"", "chat_id"]),
    ];
    for (src, needles) in cases {
        let e = parse(src).expect_err(src);
        for n in *needles {
            assert!(e.contains(n), "{src}\n=> {e}\n(missing {n:?})");
        }
    }
}

#[test]
fn widget_rule_needs_an_existing_widget() {
    let base =
        "[[alerts]]\nname='mail'\ntype='widget'\nwidget='mailpit'\nfield='unread'\nabove=1000\n";
    // The widget config format belongs to the widgets module; accept either
    // a bare widget or one with a url.
    let ok = [
        "[[widgets]]\nid='mailpit'\ntype='custom-api'\n",
        "[[widgets]]\nid='mailpit'\ntype='custom-api'\nurl='http://127.0.0.1:1/api'\n",
    ]
    .iter()
    .any(|w| parse(&format!("{w}{base}")).is_ok());
    assert!(
        ok,
        "widget rule referencing an existing widget should be valid"
    );
}

// ------------------------------------------------------------------ queue

fn core_in(dir: &std::path::Path) -> Core {
    Core::load(dir.join(STATE_FILE))
}

fn ev(kind: EventKind) -> Event {
    Event {
        rule: "down".into(),
        subject: "odoo".into(),
        kind,
        state: "stopped (exit 137)".into(),
        value: "137".into(),
        since: 1000,
        at: 1300,
    }
}

#[test]
fn queue_retries_then_gives_up() {
    let dir = tmp_dir("queue");
    let cfg = parse("[channels.a]\ntype='webhook'\nurl='http://x/h'\n[channels.b]\ntype='webhook'\nurl='http://y/h'\n[[alerts]]\nname='down'\ntype='container'\nchannels=['a','b']").unwrap();
    let mut c = core_in(&dir);
    let rule = &cfg.alerts[0];
    let now = 10_000;
    c.enqueue(Some(rule), &ev(EventKind::Firing), "h", now);
    c.enqueue(Some(rule), &ev(EventKind::Recovery), "h", now);
    assert_eq!(c.outbox.len(), 4);
    let due = c.take_due(&cfg, now);
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].2.len(), 2, "both of a's notifications, in order");
    assert_eq!(due[0].2[0].vars["kind"], "firing");
    let (a0, a1) = (due[0].2[0].id, due[0].2[1].id);
    let b0 = due[1].2[0].id;
    // a fails on its first; b succeeds.
    c.apply("a", vec![(a0, Err("HTTP 500: down".into()))], now);
    c.apply("b", vec![(b0, Ok(()))], now);
    let st = c.status(&cfg);
    assert_eq!(st.channels["a"].health, Health::Failing);
    assert_eq!(st.channels["a"].error.as_deref(), Some("HTTP 500: down"));
    assert_eq!(st.channels["a"].pending, 2);
    assert_eq!(st.channels["b"].health, Health::Ok);
    assert_eq!(st.pending, 3);
    // a is blocked until its retry time (FIFO: the recovery waits too).
    let due = c.take_due(&cfg, now + 10);
    assert!(due.iter().all(|(n, _, _)| n != "a"));
    assert_eq!(c.next_try(), Some(0), "b's recovery is still due");
    let due = c.take_due(&cfg, now + RETRY_EVERY);
    let a = due.iter().find(|(n, _, _)| n == "a").unwrap();
    assert_eq!(a.2[0].id, a0);
    assert_eq!(a.2[0].attempts, 1);
    c.apply("a", vec![(a0, Ok(())), (a1, Ok(()))], now + RETRY_EVERY);
    assert_eq!(c.status(&cfg).channels["a"].health, Health::Ok);
    assert_eq!(c.status(&cfg).channels["a"].sent, 2);

    // Undeliverable for too long: dropped and counted.
    c.enqueue(Some(rule), &ev(EventKind::Firing), "h", now);
    c.take_due(&cfg, now + GIVE_UP_AFTER);
    assert!(c.outbox.is_empty());
    assert_eq!(c.status(&cfg).channels["a"].dropped, 1);
    // A removed channel's queue is discarded.
    c.enqueue(Some(rule), &ev(EventKind::Firing), "h", now);
    let cfg2 = parse("[channels.a]\ntype='webhook'\nurl='http://x/h'").unwrap();
    c.take_due(&cfg2, now);
    assert!(c.outbox.iter().all(|d| d.channel == "a"));
}

#[test]
fn messages_use_templates() {
    let cfg = parse("[[alerts]]\nname='down'\ntype='container'\n[[alerts]]\nname='custom'\ntype='container'\nmessage='{{host}}/{{subject}} is {{state}} ({{value}})'\nrecovery=''").unwrap();
    let v = event_vars(&cfg.alerts[0], &ev(EventKind::Firing), "homelab").unwrap();
    assert_eq!(
        v["message"],
        "🔴 odoo: stopped (exit 137) (down on homelab, 1970-01-01T00:21:40Z)"
    );
    assert_eq!(v["title"], "🔴 odoo: stopped (exit 137)");
    let mut rec = ev(EventKind::Recovery);
    rec.state = "recovered".into();
    let v = event_vars(&cfg.alerts[0], &rec, "homelab").unwrap();
    assert_eq!(
        v["message"],
        "✅ odoo: recovered after 5m (down on homelab, 1970-01-01T00:21:40Z)"
    );
    let v = event_vars(&cfg.alerts[1], &ev(EventKind::Firing), "homelab").unwrap();
    assert_eq!(v["message"], "homelab/odoo is stopped (exit 137) (137)");
    assert!(
        event_vars(&cfg.alerts[1], &rec, "homelab").is_none(),
        "recovery = '' disables it"
    );
}

#[test]
fn state_file_round_trip_and_corruption() {
    let dir = tmp_dir("statefile");
    let cfg = parse("[channels.a]\ntype='webhook'\nurl='http://x/h'\n[[alerts]]\nname='down'\ntype='container'\nfor='0s'\nchannels=['a']").unwrap();
    let mut c = core_in(&dir);
    let snap = Snapshot {
        containers: vec![container("odoo", "exited", Some(1))],
        fetched_at: 1,
        error: None,
    };
    assert_eq!(
        evaluate(&mut c, &cfg, &Probes::default(), &snap, "h", 5000),
        1
    );
    c.save_if_dirty();
    let text = std::fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    assert!(text.contains("\"odoo\""), "{text}");
    assert!(!dir.join("alerts-state.json.tmp").exists());

    // Reload: the incident is remembered as notified, the queue survives.
    let mut c2 = core_in(&dir);
    assert_eq!(c2.outbox.len(), 1);
    assert_eq!(
        evaluate(&mut c2, &cfg, &Probes::default(), &snap, "h", 5100),
        0
    );
    assert!(c2.next_id > c2.outbox[0].id);
    // Nothing changed, so nothing is written.
    let mtime = std::fs::metadata(dir.join(STATE_FILE))
        .unwrap()
        .modified()
        .unwrap();
    c2.save_if_dirty();
    assert_eq!(
        std::fs::metadata(dir.join(STATE_FILE))
            .unwrap()
            .modified()
            .unwrap(),
        mtime
    );

    // Rules that no longer exist are pruned.
    let cfg_none = parse("").unwrap();
    evaluate(&mut c2, &cfg_none, &Probes::default(), &snap, "h", 5200);
    assert_eq!(c2.engine.len(), 0);

    // A corrupt file means a fresh start, not a crash.
    std::fs::write(dir.join(STATE_FILE), "{not json").unwrap();
    let c3 = core_in(&dir);
    assert_eq!(c3.engine.len(), 0);
    assert!(c3.outbox.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn docker_outage_neither_fires_nor_resolves() {
    let dir = tmp_dir("outage");
    let cfg = parse("[[alerts]]\nname='down'\ntype='container'\nfor='0s'").unwrap();
    let mut c = core_in(&dir);
    let p = Probes::default();
    let snap = Snapshot {
        containers: vec![container("odoo", "exited", Some(1))],
        fetched_at: 1,
        error: None,
    };
    evaluate(&mut c, &cfg, &p, &snap, "h", 100);
    let broken = Snapshot {
        error: Some("docker unreachable".into()),
        fetched_at: 2,
        ..Default::default()
    };
    assert_eq!(evaluate(&mut c, &cfg, &p, &broken, "h", 200), 0);
    assert_eq!(c.engine.open().count(), 1);
    // Back, and the container is gone: resolved as removed.
    let gone = Snapshot {
        fetched_at: 3,
        ..Default::default()
    };
    assert_eq!(evaluate(&mut c, &cfg, &p, &gone, "h", 300), 1);
    assert_eq!(c.outbox.len(), 0, "rule has no channels: page only");
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ end to end

fn by_kind<'a>(recs: &'a [Value], kind: &str) -> Vec<&'a Value> {
    recs.iter().filter(|r| r["kind"] == kind).collect()
}

#[tokio::test]
async fn container_down_and_recovery_reach_every_channel_once() {
    let (addr, mock) = crate::mock::start("127.0.0.1:0".parse().unwrap(), false)
        .await
        .unwrap();
    let pid = std::process::id();
    let tg_var = format!("RH_TEST_E2E_TG_{pid}");
    let slack_var = format!("RH_TEST_E2E_SLACK_{pid}");
    let auth_var = format!("RH_TEST_E2E_AUTH_{pid}");
    std::env::set_var(&tg_var, "4242:E2E-TOKEN");
    std::env::set_var(
        &slack_var,
        format!("http://{addr}/slack/T000/B000/XXSECRETXX"),
    );
    std::env::set_var(&auth_var, "Bearer e2e");
    let config = format!(
        r#"
[channels.hook]
type = "webhook"
url = "http://{addr}/hook"
headers = {{ "X-Source" = "rh" }}
headers_env = {{ Authorization = "{auth_var}" }}

[channels.slack]
type = "slack"
webhook_url_env = "{slack_var}"

[channels.tg]
type = "telegram"
token_env = "{tg_var}"
chat_id = 42
api_base = "http://{addr}"

[[alerts]]
name = "down"
type = "container"
exclude = ["ignored-*"]
for = "0s"
channels = ["hook", "slack", "tg"]
"#
    );
    let dir = tmp_dir("e2e");
    let st = app(&dir, &config);
    let task = tokio::spawn(run(st.clone()));

    // Stopped: one message per channel.
    publish(
        &st,
        vec![
            container("odoo", "exited", Some(137)),
            container("web", "running", None),
            container("ignored-job", "exited", Some(1)),
        ],
    );
    wait_for("3 firing messages", || mock.received().len() >= 3).await;
    wait_for("delivery recorded", || {
        st.alerts
            .lock()
            .unwrap()
            .channels
            .values()
            .all(|c| c.sent == 1)
    })
    .await;
    let recs = mock.received();
    assert_eq!(recs.len(), 3, "{recs:#?}");
    let hook = by_kind(&recs, "webhook")[0];
    assert_eq!(hook["path"], "/hook");
    assert_eq!(hook["body"]["kind"], "firing");
    assert_eq!(hook["body"]["subject"], "odoo");
    assert_eq!(hook["body"]["state"], "stopped (exit 137)");
    assert_eq!(hook["body"]["value"], "137");
    assert_eq!(hook["body"]["host"], "testhost");
    assert_eq!(hook["headers"]["x-source"], "rh");
    assert_eq!(hook["headers"]["authorization"], "<redacted>");
    let slack = by_kind(&recs, "slack")[0];
    assert_eq!(slack["path"], "/slack/T000/B000/XXSECRETXX");
    let text = slack["body"]["text"].as_str().unwrap();
    assert!(
        text.starts_with("🔴 odoo: stopped (exit 137) (down on testhost, "),
        "{text}"
    );
    let tg = by_kind(&recs, "telegram")[0];
    assert_eq!(tg["path"], "/bot<redacted>/sendMessage");
    assert_eq!(tg["body"]["chat_id"], 42);
    assert_eq!(tg["body"]["text"], text);
    {
        let s = st.alerts.lock().unwrap().clone();
        assert_eq!(s.incidents.len(), 1);
        assert_eq!(s.incidents[0].subject, "odoo");
        assert!(s.channels.values().all(|c| c.health == Health::Ok));
        let html = crate::ui::alerts::render(&st.config.current(), &s).into_string();
        assert!(html.contains("odoo") && html.contains("1 open"), "{html}");
    }

    // Still stopped, many times over: deduped.
    for _ in 0..5 {
        publish(&st, vec![container("odoo", "exited", Some(137))]);
        tokio::task::yield_now().await;
    }
    settle().await;
    assert_eq!(mock.received().len(), 3);

    // Process restart: a fresh task with the same data dir does not re-send.
    task.abort();
    let st = app(&dir, &config);
    let task = tokio::spawn(run(st.clone()));
    publish(&st, vec![container("odoo", "exited", Some(137))]);
    wait_for("incident loaded from the state file", || {
        st.alerts.lock().unwrap().incidents.len() == 1
    })
    .await;
    settle().await;
    assert_eq!(mock.received().len(), 3);
    assert!(st.alerts.lock().unwrap().incidents[0].notified);

    // Recovered: one recovery per channel.
    publish(&st, vec![container("odoo", "running", None)]);
    wait_for("3 recovery messages", || mock.received().len() >= 6).await;
    settle().await;
    let recs = mock.received();
    assert_eq!(recs.len(), 6);
    let recovery = &recs[3..];
    assert!(recovery
        .iter()
        .any(|r| r["body"]["kind"] == "recovery" && r["body"]["state"] == "recovered"));
    assert!(recovery.iter().any(|r| r["body"]["text"]
        .as_str()
        .is_some_and(|t| t.starts_with("✅ odoo: recovered after "))));

    // Down again within the cooldown: suppressed, shown on the page only.
    publish(&st, vec![container("odoo", "exited", Some(1))]);
    wait_for("suppressed incident", || {
        st.alerts
            .lock()
            .unwrap()
            .incidents
            .first()
            .is_some_and(|i| !i.notified)
    })
    .await;
    // ...and removed: its recovery is not sent either.
    publish(&st, vec![]);
    wait_for("incident closed", || {
        st.alerts.lock().unwrap().incidents.is_empty()
    })
    .await;
    settle().await;
    assert_eq!(mock.received().len(), 6);

    // Nothing secret anywhere we expose.
    let api = crate::render::state_json(&st, true).to_string();
    let page =
        crate::ui::alerts::render(&st.config.current(), &st.alerts.lock().unwrap()).into_string();
    let file = std::fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    for s in ["E2E-TOKEN", "XXSECRETXX", "Bearer e2e"] {
        assert!(
            !api.contains(s) && !page.contains(s) && !file.contains(s),
            "{s} leaked"
        );
    }
    task.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn failing_and_misconfigured_channels_are_visible_and_secret_free() {
    let (addr, mock) = crate::mock::start("127.0.0.1:0".parse().unwrap(), false)
        .await
        .unwrap();
    let pid = std::process::id();
    let tg_var = format!("RH_TEST_FAIL_TG_{pid}");
    std::env::set_var(&tg_var, "999:TOPSECRET-TOKEN");
    let config = format!(
        r#"
[channels.tg]
type = "telegram"
token_env = "{tg_var}"
chat_id = 1
api_base = "http://{addr}/fail"

[channels.missing]
type = "slack"
webhook_url_env = "RH_TEST_SURELY_UNSET_{pid}"

[[alerts]]
name = "down"
type = "container"
for = "0s"
channels = ["tg", "missing"]
"#
    );
    let dir = tmp_dir("failing");
    let st = app(&dir, &config);
    let task = tokio::spawn(run(st.clone()));
    publish(&st, vec![container("odoo", "exited", Some(137))]);
    wait_for("tg failing", || {
        let s = st.alerts.lock().unwrap();
        s.channels
            .get("tg")
            .is_some_and(|c| c.health == Health::Failing)
            && s.channels.get("missing").is_some_and(|c| c.failed > 0)
    })
    .await;
    let s = st.alerts.lock().unwrap().clone();
    let tg = &s.channels["tg"];
    let err = tg.error.clone().unwrap();
    assert!(
        err.contains("HTTP 500") && err.contains("mock failure"),
        "{err}"
    );
    assert!(!err.contains("TOPSECRET"), "{err}");
    assert_eq!(tg.pending, 1, "kept for a later retry");
    assert_eq!(mock.received().len(), ATTEMPTS, "retried within the round");
    assert_eq!(
        mock.received()[0]["path"],
        "/fail/bot<redacted>/sendMessage"
    );
    let m = &s.channels["missing"];
    assert_eq!(m.health, Health::Misconfigured);
    assert!(m
        .error
        .as_deref()
        .unwrap()
        .contains(&format!("RH_TEST_SURELY_UNSET_{pid}")));
    assert_eq!(s.pending, 2);

    let html = crate::ui::alerts::render(&st.config.current(), &s).into_string();
    assert!(
        html.contains("failing") && html.contains("misconfigured"),
        "{html}"
    );
    let api = crate::render::state_json(&st, true).to_string();
    let file = std::fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    for text in [&html, &api, &file] {
        assert!(!text.contains("TOPSECRET"));
    }
    task.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn telegram_error_never_contains_the_token() {
    let (addr, _mock) = crate::mock::start("127.0.0.1:0".parse().unwrap(), false)
        .await
        .unwrap();
    let var = format!("RH_TEST_TG_DIRECT_{}", std::process::id());
    std::env::set_var(&var, "123456:VERY-SECRET-TOKEN");
    let policy = crate::config::HttpPolicy::default();
    let vars = test_vars("tg", "h", 0);
    // An HTTP error, a connection error, and a timeout.
    for base in [
        format!("http://{addr}/fail"),
        "http://127.0.0.1:1".to_string(),
    ] {
        let c: ChannelConfig = toml::from_str(&format!(
            "type='telegram'\ntoken_env='{var}'\nchat_id=1\napi_base='{base}'\ntimeout='1s'"
        ))
        .unwrap();
        let t = channels::resolve(&c).unwrap();
        let e = channels::send(&t, &vars, &policy).await.unwrap_err();
        assert!(
            !e.contains("VERY-SECRET-TOKEN") && !e.contains("123456"),
            "{e}"
        );
    }
    // Disallowed host: the policy error names the host only.
    let c: ChannelConfig =
        toml::from_str(&format!("type='telegram'\ntoken_env='{var}'\nchat_id=1")).unwrap();
    let t = channels::resolve(&c).unwrap();
    let strict = crate::config::HttpPolicy {
        allow_hosts: vec!["example.com".into()],
    };
    let e = channels::send(&t, &vars, &strict).await.unwrap_err();
    assert!(
        e.contains("api.telegram.org") && !e.contains("SECRET"),
        "{e}"
    );
}

#[tokio::test]
async fn http_rule_fires_and_recovers() {
    let (addr, mock) = crate::mock::start("127.0.0.1:0".parse().unwrap(), false)
        .await
        .unwrap();
    let dir = tmp_dir("http");
    let config = |path: &str| {
        format!(
            r#"
[channels.hook]
type = "webhook"
url = "http://{addr}/hook"

[[alerts]]
name = "site"
type = "http"
url = "http://{addr}{path}"
interval = "5s"
timeout = "2s"
channels = ["hook"]
"#
        )
    };
    let st = app(&dir, &config("/fail/health"));
    let task = tokio::spawn(run(st.clone()));
    wait_for("down notification", || {
        mock.received().iter().any(|r| r["path"] == "/hook")
    })
    .await;
    let recs = mock.received();
    let hook = recs.iter().find(|r| r["path"] == "/hook").unwrap();
    assert_eq!(hook["body"]["state"], "down: HTTP 500");
    assert_eq!(
        hook["body"]["subject"],
        format!("http://{addr}/fail/health")
    );
    task.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn notify_test_cli() {
    let (addr, mock) = crate::mock::start("127.0.0.1:0".parse().unwrap(), false)
        .await
        .unwrap();
    let dir = tmp_dir("cli");
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        format!(
            "[channels.hook]\ntype='webhook'\nurl='http://{addr}/hook'\n[channels.down]\ntype='webhook'\nurl='http://{addr}/fail'\n"
        ),
    )
    .unwrap();
    // Only this test sets RH_CONFIG.
    std::env::set_var("RH_CONFIG", &path);
    assert_eq!(cli_notify_test(&["hook".into()]).await, 0);
    let recs = mock.received();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["body"]["kind"], "test");
    assert!(recs[0]["body"]["message"]
        .as_str()
        .unwrap()
        .starts_with("hobnail test message from "));
    assert_eq!(cli_notify_test(&["down".into()]).await, 1);
    assert_eq!(cli_notify_test(&["nope".into()]).await, 2);
    assert_eq!(cli_notify_test(&[]).await, 2);
    std::env::remove_var("RH_CONFIG");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn panel_render() {
    let empty = parse("").unwrap();
    assert_eq!(
        crate::ui::alerts::render(&empty, &Status::default()).into_string(),
        r#"<section id="alerts"></section>"#
    );
    let cfg = parse("[channels.a]\ntype='webhook'\nurl='http://x/h'\n[[alerts]]\nname='down'\ntype='container'\nchannels=['a']").unwrap();
    let mut c = core_in(&tmp_dir("panel"));
    let html = crate::ui::alerts::render(&cfg, &c.status(&cfg)).into_string();
    assert!(
        html.contains("all clear") && html.contains("not used yet"),
        "{html}"
    );
    let snap = Snapshot {
        containers: vec![container("<odoo>", "exited", Some(1))],
        fetched_at: 1,
        error: None,
    };
    let cfg0 = parse("[[alerts]]\nname='down'\ntype='container'\nfor='0s'").unwrap();
    evaluate(&mut c, &cfg0, &Probes::default(), &snap, "h", unix_now());
    let html = crate::ui::alerts::render(&cfg0, &c.status(&cfg0)).into_string();
    assert!(
        html.contains("&lt;odoo&gt;") && html.contains("stopped (exit 1)"),
        "{html}"
    );
    assert!(html.contains("1 open"));
}
