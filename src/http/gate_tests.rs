//! End-to-end tests of the write gate through the real router and a real
//! socket: no write route is reachable without a session, the CSRF header
//! and a same-origin `Origin`; settings reads need a session too.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::state::AppState;

const PW: &str = "correct horse battery";

/// Every write route (method, path). Bodies are valid-ish JSON; what matters
/// is that the gate answers before the handler.
const WRITES: &[(&str, &str)] = &[
    ("POST", "/api/logout"),
    ("POST", "/api/settings/general"),
    ("POST", "/api/settings/docker"),
    ("POST", "/api/settings/groups"),
    ("POST", "/api/settings/containers/override"),
    ("POST", "/api/settings/bookmarks"),
    ("POST", "/api/settings/widgets"),
    ("POST", "/api/settings/widgets/preview"),
    ("POST", "/api/settings/system"),
    ("POST", "/api/settings/channels"),
    ("POST", "/api/settings/channels/test"),
    ("POST", "/api/settings/alerts"),
    ("POST", "/api/settings/appearance"),
    ("POST", "/api/settings/history"),
    ("POST", "/api/settings/history/clear"),
    ("POST", "/api/settings/config"),
    ("POST", "/api/settings/config/validate"),
    ("POST", "/api/settings/revision/1/restore"),
    ("POST", "/api/settings/security/password"),
    ("POST", "/api/settings/security/sign-out-others"),
    ("PUT", "/api/settings/general"),
    ("DELETE", "/api/settings/bookmarks"),
    ("PATCH", "/api/settings/config"),
    ("POST", "/"),
    ("POST", "/containers"),
    // Where the Settings forms point (`method="post"`): refused like any write.
    ("POST", "/settings/security"),
    ("POST", "/settings/bookmarks"),
    ("POST", "/api/events"),
    ("DELETE", "/api/history/system"),
];

/// Every settings read (GET) endpoint.
const SETTINGS_READS: &[&str] = &[
    "meta",
    "env",
    "config",
    "revisions",
    "revision/1",
    "revision/1/diff",
    "general",
    "containers",
    "bookmarks",
    "widgets",
    "notifications",
    "appearance",
    "history",
    "history/export",
    "security",
];

struct Resp {
    status: u16,
    head: String,
    body: String,
}

async fn start(state: Arc<AppState>) -> std::net::SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(super::server::serve_listener(state, l));
    addr
}

async fn req(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Resp {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        r.push_str(&format!("{k}: {v}\r\n"));
    }
    r.push_str("\r\n");
    r.push_str(body);
    s.write_all(r.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Resp {
        status,
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn state() -> Arc<AppState> {
    let st = crate::ui::testutil::state_with(crate::ui::testutil::sample(), "");
    assert!(st.db.is_enabled(), "test state needs a real database");
    st
}

/// Log in for real; returns (cookie header value, csrf token).
async fn login(addr: std::net::SocketAddr) -> (String, String) {
    let origin = format!("http://{addr}");
    let r = req(
        addr,
        "POST",
        "/api/login",
        &[("Content-Type", "application/json"), ("Origin", &origin)],
        &format!("{{\"password\":\"{PW}\"}}"),
    )
    .await;
    assert_eq!(r.status, 200, "login: {}\n{}", r.head, r.body);
    let cookie = r
        .head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .starts_with("set-cookie:")
                .then(|| l[11..].trim().split(';').next().unwrap().to_string())
        })
        .expect("session cookie");
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    (cookie, v["csrf"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn nothing_is_writable_before_a_password_is_set() {
    let st = state();
    let addr = start(st).await;
    for (m, p) in WRITES {
        let r = req(addr, m, p, &[("Content-Type", "application/json")], "{}").await;
        assert!(
            r.status == 403 || r.status == 401,
            "{m} {p} -> {} (must be refused without a password)",
            r.status
        );
    }
}

#[tokio::test]
async fn every_write_needs_session_csrf_and_same_origin() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let (cookie, csrf) = login(addr).await;
    let origin = format!("http://{addr}");
    let json = ("Content-Type", "application/json");

    for (m, p) in WRITES {
        // No session.
        let r = req(
            addr,
            m,
            p,
            &[json, ("Origin", &origin), ("X-RH-CSRF", &csrf)],
            "{}",
        )
        .await;
        assert_eq!(r.status, 401, "{m} {p} without a session");
        // Session, no CSRF header.
        let r = req(
            addr,
            m,
            p,
            &[json, ("Origin", &origin), ("Cookie", &cookie)],
            "{}",
        )
        .await;
        assert_eq!(r.status, 403, "{m} {p} without X-RH-CSRF");
        // Session, wrong CSRF.
        let r = req(
            addr,
            m,
            p,
            &[
                json,
                ("Origin", &origin),
                ("Cookie", &cookie),
                ("X-RH-CSRF", "00"),
            ],
            "{}",
        )
        .await;
        assert_eq!(r.status, 403, "{m} {p} with a wrong X-RH-CSRF");
        // Session + CSRF, foreign Origin.
        let r = req(
            addr,
            m,
            p,
            &[
                json,
                ("Origin", "http://evil.example"),
                ("Cookie", &cookie),
                ("X-RH-CSRF", &csrf),
            ],
            "{}",
        )
        .await;
        assert_eq!(r.status, 403, "{m} {p} from a foreign Origin");
        // Session + CSRF, no Origin and no Referer.
        let r = req(
            addr,
            m,
            p,
            &[json, ("Cookie", &cookie), ("X-RH-CSRF", &csrf)],
            "{}",
        )
        .await;
        assert_eq!(r.status, 403, "{m} {p} without Origin");
    }

    // With everything right, the gate lets a write through to its handler
    // (which then refuses the stale/empty base_hash on its own terms).
    let ok = [
        json,
        ("Origin", origin.as_str()),
        ("Cookie", cookie.as_str()),
        ("X-RH-CSRF", csrf.as_str()),
    ];
    let r = req(
        addr,
        "POST",
        "/api/settings/system",
        &ok,
        r#"{"base_hash":"stale","enabled":true}"#,
    )
    .await;
    assert_eq!(
        r.status, 409,
        "gate passed, handler refused the stale hash: {}",
        r.body
    );
    let r = req(
        addr,
        "POST",
        "/api/settings/config/validate",
        &ok,
        r#"{"text":"[page]\ntitle = \"x\"\n"}"#,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);

    // Logout revokes the session.
    let r = req(addr, "POST", "/api/logout", &ok, "").await;
    assert!(
        r.status == 200 || r.status == 204 || r.status == 303,
        "logout: {}",
        r.status
    );
    let r = req(
        addr,
        "POST",
        "/api/settings/system",
        &ok,
        r#"{"base_hash":"x","enabled":true}"#,
    )
    .await;
    assert_eq!(r.status, 401, "old cookie after logout");
}

#[tokio::test]
async fn settings_reads_need_a_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    for p in SETTINGS_READS {
        let r = req(addr, "GET", &format!("/api/settings/{p}"), &[], "").await;
        assert_eq!(r.status, 401, "GET /api/settings/{p} without a session");
    }
    let (cookie, _) = login(addr).await;
    let r = req(
        addr,
        "GET",
        "/api/settings/general",
        &[("Cookie", &cookie)],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn login_refuses_foreign_origin_and_bad_redirects() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let body = format!("{{\"password\":\"{PW}\"}}");
    let r = req(
        addr,
        "POST",
        "/api/login",
        &[
            ("Content-Type", "application/json"),
            ("Origin", "http://evil.example"),
        ],
        &body,
    )
    .await;
    assert_eq!(r.status, 403);
    // Form login with a tab-smuggled next goes home, not off-site.
    let origin = format!("http://{addr}");
    let form = format!(
        "password={}&next=%2F%09%2Fevil.example",
        PW.replace(' ', "+")
    );
    let r = req(
        addr,
        "POST",
        "/api/login",
        &[
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Origin", &origin),
        ],
        &form,
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.head);
    let loc = r
        .head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("location:"))
        .unwrap();
    assert_eq!(loc[9..].trim(), "/", "{loc}");
}

#[tokio::test]
async fn anonymous_events_hide_login_history() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st.clone()).await;
    // A failed login records an auth.login_failed event with the IP.
    let origin = format!("http://{addr}");
    let _ = req(
        addr,
        "POST",
        "/api/login",
        &[("Content-Type", "application/json"), ("Origin", &origin)],
        r#"{"password":"wrong password"}"#,
    )
    .await;
    let r = req(addr, "GET", "/api/events?limit=500", &[], "").await;
    assert_eq!(r.status, 200);
    assert!(
        !r.body.contains("auth."),
        "anonymous /api/events: {}",
        r.body
    );
    for page in ["/", "/history", "/alerts"] {
        let r = req(addr, "GET", page, &[], "").await;
        assert!(
            !r.body.contains("127.0.0.1") || !r.body.contains("login_failed"),
            "{page}"
        );
        assert!(
            !r.body.contains("auth.login_failed"),
            "{page} shows login history"
        );
    }
}

/// ROB-15: a Settings form that is submitted without the script (a plain
/// form post, no `X-RH-CSRF`) is refused by the gate exactly as before,
/// changes nothing and echoes nothing of what was sent.
#[tokio::test]
async fn a_plain_form_post_is_refused_and_leaks_nothing() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let stored = st.db.kv_get("auth.password");
    let cfg = std::fs::read_to_string(st.config.path()).unwrap_or_default();
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    let origin = format!("http://{addr}");
    let form = "current=correct+horse+battery&new=Sekrit-New-Value-1&repeat=Sekrit-New-Value-1";
    for path in [
        "/settings/security",
        "/api/settings/security/password",
        "/settings/bookmarks",
        "/settings/notifications",
        "/settings/widgets",
    ] {
        let r = req(
            addr,
            "POST",
            path,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Origin", &origin),
                ("Cookie", &cookie),
            ],
            form,
        )
        .await;
        assert_eq!(r.status, 403, "{path}: {}", r.body);
        assert_eq!(r.body, "missing or bad X-RH-CSRF header\n", "{path}");
        assert!(
            !r.head.contains("Sekrit") && !r.body.contains("Sekrit"),
            "{path}"
        );
        // Signed out: 401, same refusal as today.
        let r = req(
            addr,
            "POST",
            path,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Origin", &origin),
            ],
            form,
        )
        .await;
        assert_eq!(r.status, 401, "{path}");
        assert!(!r.body.contains("Sekrit"), "{path}");
    }
    assert_eq!(
        st.db.kv_get("auth.password"),
        stored,
        "the password is unchanged"
    );
    assert_eq!(
        std::fs::read_to_string(st.config.path()).unwrap_or_default(),
        cfg,
        "the config is unchanged"
    );
}

/// The Settings list fragment reads the session from the request: 401 and
/// an empty body without one, the list with one.
#[tokio::test]
async fn settings_fragments_need_a_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    std::fs::write(
        st.config.path(),
        "[[bookmarks]]\nname = \"Wiki\"\nurl = \"https://wiki.example/frag-mark\"\n",
    )
    .unwrap();
    let addr = start(st).await;
    let r = req(addr, "GET", "/settings/bookmarks?frag=list", &[], "").await;
    assert_eq!(r.status, 401);
    assert!(r.body.is_empty(), "{}", r.body);
    let r = req(addr, "GET", "/settings/bookmarks?edit=bookmark:0", &[], "").await;
    assert_eq!(r.status, 200);
    assert!(!r.body.contains("data-item") && !r.body.contains("value=\"Wiki\""));
    let (cookie, _) = login(addr).await;
    let r = req(
        addr,
        "GET",
        "/settings/bookmarks?frag=list",
        &[("Cookie", &cookie)],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("frag-mark") && r.body.contains("data-hash="));
    assert!(!r.body.contains("<html"), "a fragment, not a page");
    // Form values and the CSRF token are never stored by a cache.
    let no_store = |h: &str| {
        h.lines()
            .any(|l| l.eq_ignore_ascii_case("cache-control: no-store"))
    };
    assert!(no_store(&r.head), "{}", r.head);
    let r = req(
        addr,
        "GET",
        "/settings/bookmarks",
        &[("Cookie", &cookie)],
        "",
    )
    .await;
    assert!(
        r.body.contains("rh-csrf") && no_store(&r.head),
        "{}",
        r.head
    );
}

/// Notifications and Widgets (wave 2b) behave like Bookmarks: their list
/// fragments need a session (401, empty, without one); the `?edit=` page
/// forms show nothing to a visitor; with a session both are `no-store`.
/// Their writes still go through the unchanged guards: a channel a rule
/// uses is refused in plain words and nothing is written.
#[tokio::test]
async fn channel_rule_widget_lists_need_a_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let cfg = "[[widgets]]\nid = \"wmark\"\ntype = \"custom-api\"\nurl = \"http://w.example/frag-mark\"\n\n\
               [channels.chmark]\ntype = \"webhook\"\nurl_env = \"RH_FRAG_MARK\"\n\n\
               [[alerts]]\nname = \"rulemark\"\ntype = \"container\"\nchannels = [\"chmark\"]\n";
    std::fs::write(st.config.path(), cfg).unwrap();
    let addr = start(st.clone()).await;
    let marks = [
        "frag-mark",
        "RH_FRAG_MARK",
        "rulemark",
        "data-item",
        "data-hash",
    ];
    for path in [
        "/settings/notifications?frag=list",
        "/settings/widgets?frag=list",
    ] {
        let r = req(addr, "GET", path, &[], "").await;
        assert_eq!(r.status, 401, "{path}");
        assert!(r.body.is_empty(), "{path}: {}", r.body);
    }
    for path in [
        "/settings/notifications?edit=channel:chmark",
        "/settings/notifications?edit=rule:rulemark",
        "/settings/widgets?edit=widget:wmark",
    ] {
        let r = req(addr, "GET", path, &[], "").await;
        assert_eq!(r.status, 200, "{path}");
        // The sign-in link echoes the visitor's own URL (`next=`): not data.
        let body: String = r
            .body
            .split("/login?next=")
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    p
                } else {
                    &p[p.find('"').unwrap_or(0)..]
                }
            })
            .collect();
        for m in marks {
            assert!(!body.contains(m), "{path}: leaked {m}");
        }
    }
    let (cookie, csrf) = login(addr).await;
    let no_store = |h: &str| {
        h.lines()
            .any(|l| l.eq_ignore_ascii_case("cache-control: no-store"))
    };
    for (path, mark) in [
        ("/settings/notifications?frag=list", "RH_FRAG_MARK"),
        ("/settings/widgets?frag=list", "frag-mark"),
        (
            "/settings/notifications?edit=channel:chmark",
            "RH_FRAG_MARK",
        ),
        ("/settings/widgets?edit=widget:wmark", "frag-mark"),
    ] {
        let r = req(addr, "GET", path, &[("Cookie", &cookie)], "").await;
        assert_eq!(r.status, 200, "{path}: {}", r.body);
        assert!(r.body.contains(mark), "{path}");
        assert!(no_store(&r.head), "{path}: {}", r.head);
    }
    let origin = format!("http://{addr}");
    let hash = crate::settings::read_current(&st).unwrap().hash;
    let r = req(
        addr,
        "POST",
        "/api/settings/channels",
        &[
            ("Content-Type", "application/json"),
            ("Origin", &origin),
            ("Cookie", &cookie),
            ("X-RH-CSRF", &csrf),
        ],
        &format!(r#"{{"base_hash":"{hash}","action":"delete","id":"chmark"}}"#),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("used by the rule"), "{}", r.body);
    assert_eq!(std::fs::read_to_string(st.config.path()).unwrap(), cfg);
    // Without the CSRF header the same delete is refused by the gate.
    let r = req(
        addr,
        "POST",
        "/api/settings/channels",
        &[
            ("Content-Type", "application/json"),
            ("Origin", &origin),
            ("Cookie", &cookie),
        ],
        &format!(r#"{{"base_hash":"{hash}","action":"delete","id":"chmark"}}"#),
    )
    .await;
    assert_eq!(r.status, 403);
}
