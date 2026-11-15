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
