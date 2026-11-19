//! End-to-end tests of the write gate through the real router and a real
//! socket: no write route is reachable without a session, the CSRF header
//! and a same-origin `Origin`; settings reads need a session too.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::state::AppState;

#[path = "dashboard_tests.rs"]
mod dashboard_tests;

const PW: &str = "correct horse battery";

/// Every write route (method, path). Bodies are valid-ish JSON; what matters
/// is that the gate answers before the handler.
const PUBLIC_WRITES: &[(&str, &str)] = &[
    ("POST", "/api/login"),
    ("POST", "/api/v1/onboarding/owner"),
    ("POST", "/api/v1/invites/accept"),
];

const WRITES: &[(&str, &str)] = &[
    ("PATCH", "/api/v1/settings"),
    ("POST", "/api/v1/boards"),
    ("PUT", "/api/v1/boards/sample-1"),
    ("PATCH", "/api/v1/boards/sample-1"),
    ("DELETE", "/api/v1/boards/sample-1"),
    ("PUT", "/api/v1/boards/sample-1/draft"),
    ("DELETE", "/api/v1/boards/sample-1/draft"),
    ("POST", "/api/v1/integrations"),
    ("PUT", "/api/v1/integrations/sample-1"),
    ("DELETE", "/api/v1/integrations/sample-1"),
    ("PUT", "/api/v1/integrations/sample-1/secret"),
    ("DELETE", "/api/v1/integrations/sample-1/secret"),
    ("POST", "/api/v1/integrations/sample-1/test"),
    ("POST", "/api/login"),
    ("POST", "/api/v1/onboarding/owner"),
    ("POST", "/api/v1/invites/accept"),
    ("POST", "/api/v1/invites"),
    ("DELETE", "/api/v1/invites/sample-1"),
    ("PATCH", "/api/v1/users/sample-1/role"),
    ("PATCH", "/api/v1/users/sample-1/disabled"),
    ("PUT", "/api/v1/users/sample-1/scopes"),
    ("POST", "/api/v1/users/sample-1/sign-out"),
    ("DELETE", "/api/v1/users/sample-1"),
    ("POST", "/api/v1/account/password"),
    ("DELETE", "/api/v1/account/sessions/sample-1"),
    ("DELETE", "/api/v1/account/sessions"),
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
    // The React client's API and documents (docs/plans/react-port.md 4.2):
    // every /api/v1 write route is listed here; non-GET on /app and
    // /assets/* is refused by the gate like any other write.
    ("POST", "/api/v1/session"),
    ("DELETE", "/api/v1/session"),
    ("POST", "/api/v1/containers"),
    ("PUT", "/api/v1/containers/x"),
    ("POST", "/app"),
    ("POST", "/assets/x.js"),
];

/// Every settings read (GET) endpoint.
const SETTINGS_READS: &[&str] = &[
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
    let st = crate::testutil::state_with(crate::testutil::sample(), "");
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
        &format!("{{\"username\":\"admin\",\"password\":\"{PW}\"}}"),
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
    for (m, p) in WRITES.iter().filter(|pair| !PUBLIC_WRITES.contains(pair)) {
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

    for (m, p) in WRITES.iter().filter(|pair| !PUBLIC_WRITES.contains(pair)) {
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
async fn login_refuses_foreign_origin_and_form_credentials() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let body = format!("{{\"username\":\"admin\",\"password\":\"{PW}\"}}");
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
    // The former form login cannot create a session or redirect.
    let origin = format!("http://{addr}");
    let form = format!(
        "username=admin&password={}&next=%2F%09%2Fevil.example",
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
    assert_eq!(r.status, 415, "{}", r.head);
    assert!(header(&r.head, "location").is_none());
    assert!(header(&r.head, "set-cookie").is_none());
}

/// ROB-15: a Settings form that is submitted without the script (a plain
/// form post, no `X-RH-CSRF`) is refused by the gate exactly as before,
/// changes nothing and echoes nothing of what was sent.
#[tokio::test]
async fn a_plain_form_post_is_refused_and_leaks_nothing() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let stored = st.db.credential("admin").map(|(_, hash, _)| hash);
    let cfg = std::fs::read_to_string(st.config.path()).unwrap_or_default();
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    let origin = format!("http://{addr}");
    let form = "current=correct+horse+battery&new=Sekrit-New-Value-1&repeat=Sekrit-New-Value-1";
    for path in [
        "/api/settings/general",
        "/api/settings/config",
        "/api/settings/bookmarks",
        "/api/settings/alerts",
        "/api/settings/widgets",
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
        st.db.credential("admin").map(|(_, hash, _)| hash),
        stored,
        "the password is unchanged"
    );
    assert_eq!(
        std::fs::read_to_string(st.config.path()).unwrap_or_default(),
        cfg,
        "the config is unchanged"
    );
}

fn header(head: &str, name: &str) -> Option<String> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

/// A JSON response that no cache may store.
fn assert_json_no_store(r: &Resp, what: &str) {
    assert_eq!(
        header(&r.head, "content-type").as_deref(),
        Some("application/json"),
        "{what}: {}",
        r.head
    );
    assert_eq!(
        header(&r.head, "cache-control").as_deref(),
        Some("no-store"),
        "{what}: {}",
        r.head
    );
}

/// The `/api/v1` error shape: `{"error":{"code","message"[,"field"]}}` and
/// nothing else.
fn assert_api_error(r: &Resp, status: u16, code: &str, what: &str) {
    assert_eq!(r.status, status, "{what}: {}\n{}", r.head, r.body);
    assert_json_no_store(r, what);
    let v: serde_json::Value = serde_json::from_str(&r.body).expect("JSON error body");
    let top = v.as_object().expect("object");
    assert_eq!(top.len(), 1, "{what}: only `error`: {}", r.body);
    let e = top["error"].as_object().expect("error object");
    assert_eq!(e["code"], code, "{what}: {}", r.body);
    assert!(
        e["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{what}: {}",
        r.body
    );
    assert!(
        e.keys()
            .all(|k| k == "code" || k == "message" || k == "field"),
        "{what}: {}",
        r.body
    );
}

/// True when `s` contains 64 hex digits in a row (a CSRF or session token).
fn has_token(s: &str) -> bool {
    let mut run = 0;
    for b in s.bytes() {
        run = if b.is_ascii_hexdigit() { run + 1 } else { 0 };
        if run >= 64 {
            return true;
        }
    }
    false
}

async fn get_session(addr: std::net::SocketAddr, headers: &[(&str, &str)]) -> serde_json::Value {
    let r = req(addr, "GET", "/api/v1/session", headers, "").await;
    assert_eq!(r.status, 200, "{}\n{}", r.head, r.body);
    assert_json_no_store(&r, "GET /api/v1/session");
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    if v["csrf"].is_null() {
        // The build's content hash may itself be a sha256; nothing else in
        // the body may look like a token.
        let body = r.body.replace(crate::api::CONTENT_HASH, "");
        assert!(!has_token(&body), "token-like value without csrf: {body}");
    }
    v
}

/// `GET /api/v1/session`: public, `no-store`; `user` and `csrf` only with a
/// valid session cookie (never for a forged, expired or revoked one); the
/// csrf is the one the login returned.
#[tokio::test]
async fn api_v1_session_reports_the_session_and_never_a_token_without_one() {
    // No password yet.
    let st = state();
    let addr = start(st.clone()).await;
    let v = get_session(addr, &[]).await;
    assert_eq!(v["user"], serde_json::Value::Null, "{v}");
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");
    assert_eq!(v["owner_exists"], false, "{v}");
    assert_eq!(v["api"], 1, "{v}");
    let b = &v["build"];
    assert_eq!(b["version"], env!("CARGO_PKG_VERSION"), "{v}");
    assert_eq!(b["sha"], st.build.sha, "{v}");
    assert_eq!(b["time"], st.build.time, "{v}");
    assert_eq!(b["content"], crate::api::CONTENT_HASH, "{v}");

    // Password set, still signed out.
    crate::auth::set_password_for_tests(&st.db, PW);
    let v = get_session(addr, &[]).await;
    assert_eq!(v["owner_exists"], true, "{v}");
    assert_eq!(v["user"], serde_json::Value::Null, "{v}");
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");

    // Signed in.
    let (cookie, csrf) = login(addr).await;
    let v = get_session(addr, &[("Cookie", &cookie)]).await;
    assert_eq!(
        v["user"],
        serde_json::json!({"name": "admin", "role": "owner"}),
        "{v}"
    );
    assert_eq!(v["csrf"], csrf.as_str(), "{v}");

    // A forged cookie (well-formed, never issued).
    let forged = format!("rh_session={}", "ab".repeat(32));
    let v = get_session(addr, &[("Cookie", &forged)]).await;
    assert_eq!(v["user"], serde_json::Value::Null, "{v}");
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");
    // A malformed one.
    let v = get_session(addr, &[("Cookie", "rh_session=not-a-token")]).await;
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");

    // An expired session (issued, then past its expiry).
    let token = "cd".repeat(32);
    let hash: String = ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let now = crate::util::unix_now() as i64;
    assert!(st.db.create_session(&hash, now - 100, now - 10));
    let expired = format!("rh_session={token}");
    let v = get_session(addr, &[("Cookie", &expired)]).await;
    assert_eq!(v["user"], serde_json::Value::Null, "{v}");
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");

    // Revoked by logout.
    let origin = format!("http://{addr}");
    let r = req(
        addr,
        "POST",
        "/api/logout",
        &[
            ("Origin", &origin),
            ("Cookie", &cookie),
            ("X-RH-CSRF", &csrf),
        ],
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let v = get_session(addr, &[("Cookie", &cookie)]).await;
    assert_eq!(v["user"], serde_json::Value::Null, "{v}");
    assert_eq!(v["csrf"], serde_json::Value::Null, "{v}");
}

/// Unknown `/api/v1` paths are a 404 in the JSON error shape; a known path
/// with the wrong method is a 405 (with `Allow`) once a write has passed the
/// gate; a write that fails the gate keeps the plain-text refusal.
#[tokio::test]
async fn api_v1_errors_are_json_and_no_store() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    for p in [
        "/api/v1/",
        "/api/v1/nope",
        "/api/v1/session/",
        "/api/v1/session/x",
        "/api/v1//session",
        "/api/v1/./session",
        "/api/v1/x/../session",
    ] {
        let r = req(addr, "GET", p, &[], "").await;
        assert_api_error(&r, 404, "not_found", &format!("GET {p}"));
    }
    let (cookie, csrf) = login(addr).await;
    let origin = format!("http://{addr}");
    let ok = [
        ("Content-Type", "application/json"),
        ("Origin", origin.as_str()),
        ("Cookie", cookie.as_str()),
        ("X-RH-CSRF", csrf.as_str()),
    ];
    for m in ["POST", "PUT", "DELETE", "PATCH"] {
        let r = req(addr, m, "/api/v1/session", &ok, "{}").await;
        assert_api_error(&r, 405, "method_not_allowed", &format!("{m} session"));
        assert_eq!(header(&r.head, "allow").as_deref(), Some("GET, HEAD"));
        let r = req(addr, m, "/api/v1/nope", &ok, "{}").await;
        assert_api_error(&r, 404, "not_found", &format!("{m} nope"));
    }
    // Gate failures stay plain text.
    let r = req(addr, "POST", "/api/v1/session", &[("Origin", &origin)], "").await;
    assert_eq!(r.status, 401);
    assert_eq!(r.body, "sign in required\n");
}

/// The DNS-rebinding guard covers `/api/v1` like every other route.
#[tokio::test]
async fn api_v1_refuses_a_foreign_host() {
    let st = state();
    let addr = start(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(
        b"GET /api/v1/session HTTP/1.1\r\nHost: rebind.example\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.starts_with("HTTP/1.1 421"), "{text}");
    assert!(!text.contains("password_set"), "{text}");
}

/// A v1 write's same-origin check: a foreign `Origin` is refused; with no
/// `Origin` a same-origin `Referer` passes the gate (the handler answers);
/// neither header is refused.
#[tokio::test]
async fn api_v1_write_origin_and_referer() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let (cookie, csrf) = login(addr).await;
    let base = [("Cookie", cookie.as_str()), ("X-RH-CSRF", csrf.as_str())];
    let with = |extra: &[(&'static str, String)]| {
        let mut h: Vec<(&str, String)> = base.iter().map(|(k, v)| (*k, v.to_string())).collect();
        h.extend(extra.iter().cloned());
        h
    };
    let send = |h: Vec<(&'static str, String)>| async move {
        let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
        req(addr, "POST", "/api/v1/session", &h, "").await
    };
    let r = send(with(&[("Origin", "http://evil.example".into())])).await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body, "cross-origin write refused\n");
    // A foreign Origin wins over a same-origin Referer.
    let r = send(with(&[
        ("Origin", "http://evil.example".into()),
        ("Referer", format!("http://{addr}/app")),
    ]))
    .await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body, "cross-origin write refused\n");
    let r = send(with(&[("Referer", "http://evil.example/app".into())])).await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body, "cross-origin write refused\n");
    let r = send(with(&[])).await;
    assert_eq!(r.status, 403);
    assert_eq!(r.body, "cross-origin write refused\n");
    // Same-origin Referer, no Origin: through the gate to the API (405).
    let r = send(with(&[("Referer", format!("http://{addr}/app/settings"))])).await;
    assert_api_error(&r, 405, "method_not_allowed", "Referer only");
}

/// Walk the v1 declarations as a visitor with no cookie or a forged cookie.
#[tokio::test]
async fn api_v1_access_table_walked_as_a_visitor() {
    use crate::api::access::{Access, ROUTES};
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let forged = format!("rh_session={}", "ab".repeat(32));
    for r in ROUTES {
        let path: String = r
            .pattern
            .split('/')
            .map(|s| if s.starts_with('<') { "sample-1" } else { s })
            .collect::<Vec<_>>()
            .join("/");
        let what = format!("{} {path} ({:?})", r.method, r.access);
        if r.method != "GET" {
            assert!(
                WRITES.contains(&(r.method, path.as_str())),
                "{what}: add it to WRITES"
            );
            continue;
        }
        assert_eq!(
            r.access == Access::Public,
            matches!(
                path.as_str(),
                "/api/v1/session" | "/api/v1/invites/validate"
            ),
            "{what}: public reads are explicitly listed",
        );
        for cookie in [None, Some(forged.as_str())] {
            let h: Vec<(&str, &str)> = cookie.map(|c| ("Cookie", c)).into_iter().collect();
            for m in ["GET", "HEAD"] {
                // Bounded: the event stream answers and stays open.
                let resp = req_bounded(addr, m, &path, &h).await;
                match r.access {
                    Access::Public => assert_ne!(resp.status, 401, "{m} {what}"),
                    _ if m == "HEAD" => {
                        assert_eq!(resp.status, 401, "{m} {what}");
                        assert!(resp.body.is_empty(), "{m} {what}: {}", resp.body);
                    }
                    _ => assert_api_error(&resp, 401, "sign_in_required", &what),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// /api/v1 phase 1 reads (docs/plans/react-port.md 5, phase 1): bootstrap,
// containers, one container, strips, the stream. All Public; what a visitor
// must never get: probe output and checks, `auth.*` events, the config
// error, and any container the owner hid (override, label, `[docker]
// exclude`).

/// Like `req`, but gives up reading after a second (the event stream never
/// ends); what arrived until then is the response.
async fn req_bounded(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Resp {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: 0\r\n"
    );
    for (k, v) in headers {
        r.push_str(&format!("{k}: {v}\r\n"));
    }
    r.push_str("\r\n");
    s.write_all(r.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut buf = [0u8; 8192];
    loop {
        match tokio::time::timeout_at(deadline, s.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => out.extend_from_slice(&buf[..n]),
        }
    }
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

/// What no visitor response may contain.
const CONFIG_SECRET: &str = "unknown field";
/// Every owner-hidden container's name starts with this.
const HIDDEN_PREFIX: &str = "secret-";

const PRIVACY_TOML: &str = r#"
[docker]
exclude = ["secret-exc*"]

[containers.secret-ovr]
hide = true

[containers.web]
url = "http://{host}:9000/{name}"

[[widgets]]
id = "probe-widget"
type = "custom-api"
url = "https://example.com/private"
headers = { Authorization = "widget-secret" }

[[bookmarks]]
name = "Example"
url = "https://example.com"

[channels.hook]
type = "webhook"
url = "https://example.com/channel-secret"

[[alerts]]
name = "shop-down"
type = "container"
containers = ["web", "db"]
on = ["unhealthy", "stopped"]
"#;

/// A fake Docker socket answering `GET /containers/<id>/json` for `web`
/// with a failing health check whose output is a secret.
async fn fake_docker_inspect(tag: &str) -> String {
    use tokio::net::UnixListener;
    let path = std::env::temp_dir().join(format!("rh-api-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let web_id = crate::testutil::ctr("web", None, "running").id;
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let web_id = web_id.clone();
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&req).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let (status, body) = if path == format!("/containers/{web_id}/json") {
                    let body = serde_json::json!({
                        "Id": web_id, "Name": "/web",
                        "Config": {"Cmd": ["--token=SECRET-CMD"], "Env": ["PASSWORD=hunter2"], "Healthcheck": {"Test": ["CMD", "SECRET-HEALTH-TEST"]}},
                        "Mounts": [{"Type": "bind", "Source": "/private/SECRET-MOUNT", "Destination": "/app/data", "RW": false}],
                        "State": {
                            "Status": "running", "Running": true,
                            "StartedAt": "2026-11-14T21:00:00Z",
                            "Health": {"Status": "unhealthy", "FailingStreak": 3, "Log": [
                                {"Start": "2026-11-14T21:01:00Z", "End": "2026-11-14T21:01:01Z",
                                 "ExitCode": 1, "Output": "SECRET-PROBE-OUTPUT token=abc\nsecond line"},
                                {"Start": "2026-11-14T21:01:30Z", "End": "2026-11-14T21:01:31Z",
                                 "ExitCode": 1, "Output": "SECRET-PROBE-OUTPUT again"}
                            ]}
                        }
                    });
                    ("200 OK", body.to_string())
                } else {
                    (
                        "404 Not Found",
                        r#"{"message":"No such container"}"#.to_string(),
                    )
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    path.to_string_lossy().into_owned()
}

/// The privacy fixture: `web` (unhealthy, link override), `db` (exited 1),
/// `a.b-c_d` (a name that needs encoding care) and three hidden ones, with
/// history and events for all of them (so a filter that fails shows).
async fn privacy_state(tag: &str) -> Arc<AppState> {
    use crate::testutil::ctr;
    let socket = fake_docker_inspect(tag).await;
    let mut web = ctr("web", Some("shop"), "running");
    web.health = Some("unhealthy".into());
    web.status = "Up 3 hours (unhealthy)".into();
    web.created = 1_700_000_000;
    let mut lbl = ctr("secret-lbl", Some("shop"), "running");
    lbl.labels.insert("hobnail.hide".into(), "true".into());
    let containers = vec![
        web,
        ctr("db", Some("shop"), "exited"),
        ctr("a.b-c_d", None, "running"),
        lbl,
        ctr("secret-ovr", Some("shop"), "running"),
        ctr("secret-exc", Some("vault"), "running"),
    ];
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rh-api-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), PRIVACY_TOML).unwrap();
    let st = AppState::new(crate::state::Env {
        listen: vec![],
        data_dir: dir.clone(),
        config_path: dir.join("config.toml"),
        docker_socket: socket,
        host_proc: "/nonexistent".into(),
        host_sys: "/nonexistent".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    });
    assert!(
        st.config.snapshot().error.is_none(),
        "{:?}",
        st.config.snapshot().error
    );
    let now = crate::util::unix_now() as i64;
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        containers: containers.clone(),
        fetched_at: now as u64,
        error: None,
    }));
    assert!(st.db.is_enabled());
    let s = crate::model::ContainerStats {
        cpu_percent: 12.5,
        mem_bytes: 64 << 20,
    };
    for c in &containers {
        for k in 0..20 {
            st.db
                .record_containers(now - 3000 + k * 15, &[(c.name.as_str(), &s)]);
        }
        st.db
            .record_event(now - 100, "container.start", "info", &c.name, "started");
        // Login history under a container's name: never for a visitor.
        st.db.record_event(
            now - 50,
            "auth.login_failed",
            "warn",
            &c.name,
            "from 192.0.2.7",
        );
    }
    crate::auth::set_password_for_tests(&st.db, PW);
    st
}

fn json_of(r: &Resp, what: &str) -> serde_json::Value {
    assert_eq!(r.status, 200, "{what}: {}\n{}", r.head, r.body);
    assert_json_no_store(r, what);
    serde_json::from_str(&r.body).unwrap_or_else(|e| panic!("{what}: {e}: {}", r.body))
}

/// Break the config file (the last good one stays in use, with an error).
fn break_config(st: &AppState) {
    std::fs::write(
        st.config.path(),
        format!("{PRIVACY_TOML}\n[page]\nbogus_key = 1\n"),
    )
    .unwrap();
    st.config.reload();
    let e = st.config.snapshot().error.expect("config error");
    assert!(e.contains(CONFIG_SECRET), "{e}");
}

#[tokio::test]
async fn protected_views_still_hide_excluded_containers() {
    let st = privacy_state("privacy").await;
    let addr = start(st).await;
    let (cookie, _) = login(addr).await;
    let headers = [("Cookie", cookie.as_str())];
    let r = req(addr, "GET", "/api/v1/containers", &headers, "").await;
    let v = json_of(&r, "containers");
    let names: Vec<_> = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["containers"].as_array().unwrap())
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["db", "web", "a.b-c_d"]);
    for hidden in ["secret-ovr", "secret-lbl", "secret-exc", "nope"] {
        let r = req(
            addr,
            "GET",
            &format!("/api/v1/containers/{hidden}"),
            &headers,
            "",
        )
        .await;
        assert_api_error(&r, 404, "not_found", hidden);
        assert!(!r.body.contains(HIDDEN_PREFIX));
    }
    for hidden in ["secret-ovr", "secret-lbl", "secret-exc"] {
        for tab in ["inspect", "metrics", "events"] {
            let path = format!("/api/v1/containers/{hidden}/{tab}");
            let response = req(addr, "GET", &path, &headers, "").await;
            assert_api_error(&response, 404, "not_found", &path);
        }
        let path = format!("/api/history/container/{hidden}?metric=cpu");
        assert_eq!(req(addr, "GET", &path, &headers, "").await.status, 404);
    }
    let r = req(addr, "GET", "/api/v1/strips", &headers, "").await;
    let v = json_of(&r, "strips");
    let keys: Vec<_> = v["containers"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["a.b-c_d", "db", "web"]);
    assert_eq!(v["buckets"], 96);
}

/// Signed in: the same reads add the probe's first line, Docker's last
/// checks and the config error; login history shows in the timeline.
#[tokio::test]
async fn api_v1_reads_with_a_session_add_probe_checks_and_config_error() {
    let st = privacy_state("signed").await;
    break_config(&st);
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    let h = [("Cookie", cookie.as_str())];
    let r = req(addr, "GET", "/api/v1/bootstrap", &h, "").await;
    let v = json_of(&r, "bootstrap");
    assert!(
        v["config_error"]
            .as_str()
            .is_some_and(|e| e.contains(CONFIG_SECRET)),
        "{v}"
    );
    let r = req(addr, "GET", "/api/v1/containers/web", &h, "").await;
    let v = json_of(&r, "detail web");
    let why = &v["why"];
    assert_eq!(
        why["probe"], "SECRET-PROBE-OUTPUT again",
        "newest failing probe, first line: {v}"
    );
    let checks = why["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 2, "{v}");
    assert_eq!(checks[0]["output"], "SECRET-PROBE-OUTPUT again");
    assert_eq!(
        checks[1]["output"],
        "SECRET-PROBE-OUTPUT token=abc\nsecond line"
    );
    assert_eq!(checks[0]["exit_code"], 1);
    assert_eq!(checks[0]["at"], 1_794_690_090, "{v}");
    assert_eq!(why["title"], "Health check failing, 3 in a row", "{v}");
    // The exact start from the inspect.
    assert_eq!(v["row"]["started_at"], 1_794_690_000, "{v}");
    assert!(
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "auth.login_failed"),
        "{v}"
    );
    // Hidden stays hidden with a session too (as today's lists).
    let r = req(addr, "GET", "/api/v1/containers/secret-ovr", &h, "").await;
    assert_api_error(&r, 404, "not_found", "hidden, signed in");
    let r = req(addr, "GET", "/api/v1/containers", &h, "").await;
    assert!(!r.body.contains(HIDDEN_PREFIX), "{}", r.body);
}

/// The chart window: `to` past now is clamped to now, `from` before 30 days
/// to 30 days; `from >= to`, half a window, a bad number or range are a 400
/// naming the field; names are percent-decoded, a bad encoding is a 404.
#[tokio::test]
async fn api_v1_container_window_and_names() {
    let st = privacy_state("window").await;
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    let h = [("Cookie", cookie.as_str())];
    let now = crate::util::unix_now() as i64;
    let get = |q: String| async move {
        req(addr, "GET", &format!("/api/v1/containers/web{q}"), &h, "").await
    };

    let v = json_of(&get(String::new()).await, "default");
    let s = &v["series"];
    assert_eq!(s["range"], "6h", "{s}");
    assert_eq!(
        s["to"].as_i64().unwrap() - s["from"].as_i64().unwrap(),
        6 * 3600
    );
    assert!(s["cpu_max"].is_null() && s["mem_max"].is_null(), "{s}");
    assert_eq!(s["step"], 60, "{s}");
    let n = s["cpu"].as_array().unwrap().len();
    assert!(
        (5..=6).contains(&n),
        "20 samples of 15 s in 60 s buckets: {s}"
    );
    assert_eq!(s["cpu"][0][1], 12.5, "{s}");

    let v = json_of(&get("?range=7d".into()).await, "7d");
    assert_eq!(v["series"]["range"], "7d");
    assert!(v["series"]["cpu_max"].is_array(), "band from 24 h up");

    // to > now: clamped.
    let v = json_of(
        &get(format!("?from={}&to={}", now - 3600, now + 7200)).await,
        "future",
    );
    let s = &v["series"];
    assert!(
        s["to"].as_i64().unwrap() <= crate::util::unix_now() as i64,
        "{s}"
    );
    assert!(s["to"].as_i64().unwrap() >= now, "{s}");
    // The server may cross the next second while handling the request.
    assert_eq!(s["from"], now - 3600, "{s}");
    let span = s["to"].as_i64().unwrap() - s["from"].as_i64().unwrap();
    assert_eq!(s["range"], if span <= 3600 { "1h" } else { "6h" }, "{s}");
    // More than 30 days: from clamped to 30 days back.
    let v = json_of(
        &get(format!("?from={}&to={now}", now - 90 * 86_400)).await,
        "90d",
    );
    let s = &v["series"];
    assert_eq!(s["range"], "30d", "{s}");
    assert!(s["from"].as_i64().unwrap() >= now - 30 * 86_400, "{s}");
    assert!(
        s["cpu"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p[0].as_i64().unwrap() >= s["from"].as_i64().unwrap()),
        "points before from are dropped: {s}"
    );
    // A zoomed hour reads at the 1 h preset; points < from dropped.
    let v = json_of(
        &get(format!("?from={}&to={}", now - 2000, now - 1000)).await,
        "zoom",
    );
    let s = &v["series"];
    assert_eq!(s["range"], "1h", "{s}");
    assert_eq!(s["from"], now - 2000, "{s}");
    assert!(
        s["cpu"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p[0].as_i64().unwrap() >= now - 2000),
        "{s}"
    );

    for (q, field) in [
        (format!("?from={now}&to={now}"), "from"),
        (format!("?from={}&to={}", now, now - 10), "from"),
        (format!("?from={}&to={}", now + 100, now + 200), "from"),
        (
            format!("?from={}&to={}", now - 60 * 86_400, now - 40 * 86_400),
            "from",
        ),
        (format!("?from={now}"), "to"),
        (format!("?to={now}"), "from"),
        ("?from=abc&to=1".to_string(), "from"),
        ("?from=1&to=1.5".to_string(), "to"),
        ("?range=2h".to_string(), "range"),
    ] {
        let r = get(q.clone()).await;
        assert_api_error(&r, 400, "bad_request", &q);
        let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(v["error"]["field"], field, "{q}: {v}");
    }
    // Unknown name (with a bad window too): 404 first.
    let r = req(addr, "GET", "/api/v1/containers/nope?range=2h", &h, "").await;
    assert_api_error(&r, 404, "not_found", "unknown");

    // Percent-encoded names.
    for p in ["a.b-c_d", "a%2Eb%2Dc_d", "%61.b-c_d"] {
        let r = req(addr, "GET", &format!("/api/v1/containers/{p}"), &h, "").await;
        let v = json_of(&r, p);
        assert_eq!(v["row"]["name"], "a.b-c_d", "{p}");
    }
    for p in ["a%2Fb", "%2e%2e", "%zz", "web%00", "secret%2Dovr"] {
        let r = req(addr, "GET", &format!("/api/v1/containers/{p}"), &h, "").await;
        assert_api_error(&r, 404, "not_found", p);
    }
}

/// An open `/api/v1/stream` connection (HTTP/1.0: the body is the raw
/// event stream, no chunk framing).
struct Stream {
    s: TcpStream,
    buf: Vec<u8>,
}

impl Stream {
    async fn open(addr: std::net::SocketAddr, path: &str, cookie: &str) -> (Stream, String) {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(
            format!("GET {path} HTTP/1.0\r\nHost: {addr}\r\nCookie: {cookie}\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut st = Stream { s, buf: Vec::new() };
        let head = st.until(b"\r\n\r\n").await;
        (st, head)
    }

    /// Bytes up to and including `sep` (5 s at most).
    async fn until(&mut self, sep: &[u8]) -> String {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(i) = self.buf.windows(sep.len()).position(|w| w == sep) {
                let out: Vec<u8> = self.buf.drain(..i + sep.len()).collect();
                return String::from_utf8_lossy(&out).into_owned();
            }
            let mut b = [0u8; 8192];
            match tokio::time::timeout_at(deadline, self.s.read(&mut b)).await {
                Ok(Ok(n)) if n > 0 => self.buf.extend_from_slice(&b[..n]),
                other => panic!(
                    "stream ended or timed out: {other:?}; have {:?}",
                    String::from_utf8_lossy(&self.buf)
                ),
            }
        }
    }

    /// The next event (comments skipped): (name, JSON data).
    async fn next(&mut self) -> (String, serde_json::Value) {
        loop {
            let block = self.until(b"\n\n").await;
            let mut name = None;
            let mut data = String::new();
            for line in block.lines() {
                if let Some(n) = line.strip_prefix("event: ") {
                    name = Some(n.to_string());
                } else if let Some(d) = line.strip_prefix("data: ") {
                    assert!(data.is_empty(), "one data line per event: {block}");
                    data.push_str(d);
                }
            }
            if let Some(n) = name {
                let v = serde_json::from_str(&data).unwrap_or_else(|e| panic!("{n}: {e}: {data}"));
                return (n, v);
            }
        }
    }

    /// True when nothing arrives within `ms` (no event pending).
    async fn quiet(&mut self, ms: u64) -> bool {
        if !self.buf.is_empty() {
            return false;
        }
        let mut b = [0u8; 1024];
        match tokio::time::timeout(std::time::Duration::from_millis(ms), self.s.read(&mut b)).await
        {
            Err(_) => true,
            Ok(Ok(n)) => {
                self.buf.extend_from_slice(&b[..n]);
                false
            }
            Ok(Err(_)) => false,
        }
    }
}

async fn viewers_reach(st: &AppState, n: usize, poke: bool) -> bool {
    for _ in 0..100 {
        if st.viewers() == n {
            return true;
        }
        if poke {
            // Only a write notices a closed socket.
            st.changed("reload");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    st.viewers() == n
}

/// The stream: `hello` first; a publish that changes the list is a
/// `containers` event (no hidden container in it); a stats refresh that
/// changes nothing else is a `stats` event only; a config reload is
/// `config`; falling behind is `resync`; the viewer count follows the
/// connection.
#[tokio::test]
async fn api_v1_stream_events_and_viewer_count() {
    let st = privacy_state("stream").await;
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    assert_eq!(st.viewers(), 0);
    let (mut s, head) = Stream::open(
        addr,
        "/api/v1/stream?topics=containers,stats,system,bogus",
        &cookie,
    )
    .await;
    assert!(
        head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200"),
        "{head}"
    );
    assert_eq!(
        header(&head, "content-type").as_deref(),
        Some("text/event-stream")
    );
    assert_eq!(header(&head, "cache-control").as_deref(), Some("no-store"));
    let (name, v) = s.next().await;
    assert_eq!(name, "hello");
    assert_eq!(v["sha"], st.build.sha);
    assert_eq!(v["content"], crate::api::CONTENT_HASH);
    assert_eq!(v["api"], 1);
    assert!(
        viewers_reach(&st, 1, false).await,
        "connected viewer counted"
    );

    // A stats refresh with nothing else changed: `stats` only.
    crate::live::refresh_container_stats(&st);
    let (name, v) = s.next().await;
    assert_eq!(name, "stats", "{v}");
    let keys: Vec<&String> = v["containers"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["a.b-c_d", "db", "web"], "visible only: {v}");
    assert!(
        s.quiet(300).await,
        "no containers event after a stats refresh"
    );

    // A container comes: `containers` with the whole view.
    let mut snap = (*st.snapshot()).clone();
    snap.containers
        .push(crate::testutil::ctr("new-one", Some("shop"), "running"));
    st.publish_containers(snap.clone());
    let (name, v) = s.next().await;
    assert_eq!(name, "containers", "{v}");
    let text = v.to_string();
    assert!(text.contains("\"new-one\""), "{text}");
    assert!(!text.contains(HIDDEN_PREFIX), "{text}");
    assert!(v["groups"].is_array() && v["now"].is_i64(), "{v}");
    // The same list again: only numbers.
    st.publish_containers(snap);
    let (name, _) = s.next().await;
    assert_eq!(name, "stats");

    // System: a host sample.
    crate::live::refresh_system(&st);
    let (name, v) = s.next().await;
    assert_eq!(name, "system", "{v}");
    assert!(v["load"].is_array() && v["disks"].is_array(), "{v}");

    // Ignored ids.
    st.changed("w-x");
    st.changed("alerts");
    assert!(s.quiet(300).await, "w-* and alerts are not sent");

    // Config reload.
    st.changed("reload");
    let (name, v) = s.next().await;
    assert_eq!(name, "config");
    assert_eq!(v, serde_json::json!({}));

    // Falling behind (more than the 64 the broadcast holds, with no await
    // in between): `resync`.
    for _ in 0..200 {
        st.changed("w-x");
    }
    let (name, v) = s.next().await;
    assert_eq!(name, "resync");
    assert_eq!(v, serde_json::json!({}));

    // Disconnect: the viewer goes away (within the next writes).
    drop(s);
    assert!(
        viewers_reach(&st, 0, true).await,
        "viewers: {}",
        st.viewers()
    );
}

/// Topics: only what was asked for (`system` alone gets no list events);
/// HEAD answers the headers without counting a viewer; a visitor's stream
/// never carries a hidden container.
#[tokio::test]
async fn api_v1_stream_topics_and_head() {
    let st = privacy_state("topics").await;
    let addr = start(st.clone()).await;
    let (cookie, _) = login(addr).await;
    let r = req(addr, "HEAD", "/api/v1/stream", &[("Cookie", &cookie)], "").await;
    assert_eq!(r.status, 200, "{}", r.head);
    assert_eq!(
        header(&r.head, "content-type").as_deref(),
        Some("text/event-stream")
    );
    assert_eq!(st.viewers(), 0);

    let (mut s, _) = Stream::open(addr, "/api/v1/stream?topics=system", &cookie).await;
    assert_eq!(s.next().await.0, "hello");
    crate::live::refresh_container_stats(&st);
    let mut snap = (*st.snapshot()).clone();
    snap.containers
        .push(crate::testutil::ctr("other", None, "running"));
    st.publish_containers(snap);
    assert!(s.quiet(300).await, "no list events without the topics");
    crate::live::refresh_system(&st);
    assert_eq!(s.next().await.0, "system");
    drop(s);

    // Only `containers`: a stats refresh sends nothing.
    let (mut s, _) = Stream::open(addr, "/api/v1/stream?topics=containers", &cookie).await;
    assert_eq!(s.next().await.0, "hello");
    crate::live::refresh_container_stats(&st);
    assert!(s.quiet(300).await, "stats not asked for");
    drop(s);
    assert!(viewers_reach(&st, 0, true).await);
}

#[tokio::test]
async fn legacy_reads_require_a_session_and_public_assets_do_not() {
    let addr = start(state()).await;
    for path in [
        "/api/history/system?metric=cpu",
        "/api/history/container/web?metric=cpu",
        "/api/events",
        "/api/logs/stream?c=web",
        "/api/logs/download?c=web",
        "/api/logs/levels?c=web",
        "/api/logs/status",
        "/custom.css",
        "/icons/di/nginx.svg",
        "/icons/web.svg",
    ] {
        for method in ["GET", "HEAD", "OPTIONS"] {
            let r = req_bounded(addr, method, path, &[]).await;
            assert_eq!(r.status, 401, "{method} {path}");
            assert_eq!(
                r.body,
                if method == "HEAD" {
                    ""
                } else {
                    "sign in required\n"
                }
            );
        }
    }

    for path in [
        "/healthz",
        "/api/v1/session",
        "/api/v1/invites/validate?token=example",
        "/app",
        "/app/containers/web",
        "/assets/missing.js",
        "/manifest.webmanifest",
        "/favicon.ico",
        "/icon.svg",
        "/icon-192.png",
        "/icon-512.png",
        "/apple-touch-icon.png",
    ] {
        for method in ["GET", "HEAD", "OPTIONS"] {
            let r = req_bounded(addr, method, path, &[]).await;
            if method == "OPTIONS" {
                assert_eq!(r.status, 401, "{method} {path}");
            } else {
                assert!(
                    !matches!(r.status, 303 | 401 | 403),
                    "{method} {path}: {}",
                    r.status
                );
            }
        }
    }
}

#[tokio::test]
async fn retired_routes_are_gone_with_or_without_a_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let (cookie, _) = login(addr).await;
    for headers in [&[][..], &[("Cookie", cookie.as_str())][..]] {
        for path in [
            "/",
            "/containers",
            "/containers/",
            "/containers/web",
            "/containers/web/logs",
            "/containers?frag=table",
            "/logs",
            "/logs/split",
            "/stacks/shop",
            "/stacks/shop/logs",
            "/host",
            "/host?frag=live",
            "/history",
            "/alerts",
            "/login",
            "/more",
            "/settings",
            "/settings/",
            "/settings/general",
            "/settings/containers",
            "/settings/bookmarks",
            "/settings/widgets",
            "/settings/notifications",
            "/settings/appearance",
            "/settings/history",
            "/settings/config",
            "/settings/security",
            "/settings/containers?frag=list",
            "/missing-page",
            "/static/app.js",
            "/alarm.json",
            "/api/state",
            "/api/stream",
            "/api/palette",
            "/api/containers/web/top",
            "/api/missing",
            "/api/settings/meta",
            "/api/settings/env",
            "/api/settings/missing",
        ] {
            let r = req_bounded(addr, "GET", path, headers).await;
            assert_eq!(r.status, 404, "{path}: {}", r.body);
            assert!(header(&r.head, "location").is_none(), "{path}");
            if path == "/" {
                assert!(r.body.contains("Hobnail") && r.body.contains("href=\"/app\""));
                assert!(header(&r.head, "content-security-policy")
                    .unwrap()
                    .contains("default-src 'none'"));
            } else if path.starts_with("/api/") {
                assert_eq!(
                    header(&r.head, "content-type").as_deref(),
                    Some("application/json")
                );
            } else {
                assert_eq!(r.body, "not found\n");
            }
        }
    }
}

#[tokio::test]
async fn public_credential_writes_require_same_origin_and_known_host() {
    let addr = start(state()).await;
    for &(method, path) in PUBLIC_WRITES {
        assert!(WRITES.contains(&(method, path)));
        let r = req(
            addr,
            method,
            path,
            &[
                ("Origin", "https://foreign.example"),
                ("Content-Type", "application/json"),
            ],
            "{}",
        )
        .await;
        assert_eq!(r.status, 403, "{path}");
        assert_eq!(r.body, "cross-origin write refused\n", "{path}");
        let mut connection = TcpStream::connect(addr).await.unwrap();
        connection.write_all(format!(
            "{method} {path} HTTP/1.1\r\nHost: rebind.example\r\nOrigin: http://rebind.example\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        ).as_bytes()).await.unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 421"), "{path}: {response}");
    }
}

fn credentials_of(response: &Resp) -> (String, String) {
    let cookie = header(&response.head, "set-cookie").unwrap();
    assert_json_no_store(response, "signed in");
    let data: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let csrf = data["csrf"].as_str().unwrap().to_owned();
    (cookie.split(';').next().unwrap().to_owned(), csrf)
}

#[tokio::test]
async fn concurrent_owner_onboarding_issues_one_session() {
    let st = state();
    let addr = start(st.clone()).await;
    let origin = format!("http://{addr}");
    let headers = [
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
    ];
    let path = "/api/v1/onboarding/owner";
    let short = req(
        addr,
        "POST",
        path,
        &headers,
        r#"{"username":"admin","password":"short"}"#,
    )
    .await;
    assert_api_error(&short, 400, "bad_request", "password policy");
    let oversized = req(addr, "POST", path, &headers, &"x".repeat(4097)).await;
    assert_api_error(&oversized, 413, "body_too_large", "body limit");
    let a = r#"{"username":"alice","password":"correct horse battery"}"#;
    let b = r#"{"username":"bob","password":"correct horse battery"}"#;
    let (a, b) = tokio::join!(
        req(addr, "POST", path, &headers, a),
        req(addr, "POST", path, &headers, b)
    );
    let (first, second) = if a.status == 201 { (a, b) } else { (b, a) };
    assert_eq!(first.status, 201, "{}", first.body);
    assert_api_error(&second, 409, "conflict", "second owner attempt");
    assert_eq!(st.db.accounts().unwrap().len(), 1);
    assert_eq!(st.db.session_count(crate::auth::now_s()), 1);
    let (cookie, csrf) = credentials_of(&first);
    let session = get_session(addr, &[("Cookie", &cookie)]).await;
    assert_eq!(session["owner_exists"], true);
    assert_eq!(session["user"]["role"], "owner");
    assert_eq!(session["csrf"], csrf);
    let r = req(
        addr,
        "POST",
        path,
        &headers,
        r#"{"username":"third","password":"correct horse battery"}"#,
    )
    .await;
    assert_api_error(&r, 409, "conflict", "owner already exists");
}

#[tokio::test]
async fn invite_roles_and_account_management() {
    let st = privacy_state("invite-roles-inspect").await;
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st.clone()).await;
    let owner = login(addr).await;
    let origin = format!("http://{addr}");
    let origin_headers = [
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
    ];
    let owner_headers = [
        origin_headers[0],
        origin_headers[1],
        ("Cookie", owner.0.as_str()),
        ("X-RH-CSRF", owner.1.as_str()),
    ];

    let created = req(
        addr,
        "POST",
        "/api/v1/boards",
        &owner_headers,
        r#"{"name":"Scoped board"}"#,
    )
    .await;
    assert_eq!(created.status, 201);
    let created: serde_json::Value = serde_json::from_str(&created.body).unwrap();
    let scope = created["id"].as_str().unwrap().to_owned();
    st.db
        .dashboard(|conn| {
            conn.execute(
            "INSERT INTO board_items (board_id,kind,config,layout) VALUES (?1,'container',?2,'{}')",
            rusqlite::params![scope, r#"{"kind":"container","target":{"by":"compose","project":"shop","service":"web"}}"#],
        )
        })
        .unwrap();
    for role in ["viewer", "admin"] {
        let data =
            serde_json::json!({"role": role, "scopes": {"boards": [scope], "integrations": [] }});
        let r = req(
            addr,
            "POST",
            "/api/v1/invites",
            &owner_headers,
            &data.to_string(),
        )
        .await;
        assert_eq!(r.status, 201, "{}", r.body);
        assert_json_no_store(&r, "created invite");
        let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        let token = v["token"].as_str().unwrap();
        let r = req(addr, "GET", "/api/v1/invites", &owner_headers, "").await;
        assert!(!r.body.contains(token));
        let validate = format!("/api/v1/invites/validate?token={token}");
        let r = req(addr, "GET", &validate, &[], "").await;
        assert_eq!(
            json_of(&r, "invite validation"),
            serde_json::json!({
                "valid": true, "role": role, "expires": v["invite"]["expires"]
            })
        );
        let account =
            serde_json::json!({"username":format!("guest-{role}"), "password":PW, "token":token});
        let r = req(
            addr,
            "POST",
            "/api/v1/invites/accept",
            &origin_headers,
            &account.to_string(),
        )
        .await;
        assert_eq!(r.status, 201, "{}", r.body);
        let (cookie, csrf) = credentials_of(&r);
        let h = [
            origin_headers[0],
            origin_headers[1],
            ("Cookie", cookie.as_str()),
            ("X-RH-CSRF", csrf.as_str()),
        ];
        let r = req(addr, "GET", &validate, &[], "").await;
        assert_eq!(
            json_of(&r, "used invite"),
            serde_json::json!({"valid": false, "role": null, "expires": null})
        );
        let r = req(addr, "GET", "/api/v1/bootstrap", &h, "").await;
        let bootstrap = json_of(&r, "role in bootstrap");
        assert_eq!(bootstrap["role"], role);
        assert_eq!(bootstrap["scoped"], true);
        let picker = req(addr, "GET", "/api/v1/logs/picker", &h, "").await;
        assert_eq!(picker.status, 200);
        let picker = json_of(&picker, "scoped logs picker");
        let names: Vec<_> = picker["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["containers"].as_array().unwrap())
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["web"]);
        for (path, expected) in [
            ("/api/logs/levels?c=db", 404),
            ("/api/logs/levels?c=web", 200),
            ("/api/logs/download?c=db", 400),
            ("/api/logs/stream?c=db", 400),
            ("/api/logs/status", 403),
        ] {
            assert_eq!(
                req(addr, "GET", path, &h, "").await.status,
                expected,
                "{path}"
            );
        }
        for resource in ["inspect", "metrics", "events"] {
            let path = format!("/api/v1/containers/no-grant/{resource}");
            assert_eq!(req(addr, "GET", &path, &h, "").await.status, 404, "{path}");
        }
        for resource in ["inspect", "metrics", "events"] {
            let path = format!("/api/v1/containers/web/{resource}");
            let response = req(addr, "GET", &path, &h, "").await;
            assert_eq!(response.status, 200, "{path}");
            if resource == "inspect" {
                let view = json_of(&response, "scoped inspect");
                assert!(view["command"].is_null());
                assert!(view["health_test"].is_null());
                assert!(view["mounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|mount| mount["source"].is_null()));
            }
        }
        assert_eq!(
            req(addr, "GET", "/api/settings/config", &h, "")
                .await
                .status,
            403
        );
        for resource in ["top", "env"] {
            let path = format!("/api/v1/containers/no-grant/{resource}");
            assert_eq!(req(addr, "GET", &path, &h, "").await.status, 403, "{path}");
        }
        assert_eq!(
            req(addr, "GET", "/api/v1/history", &h, "").await.status,
            403
        );

        assert_eq!(
            req(addr, "GET", "/api/v1/bookmarks", &h, "").await.status,
            403
        );
        for path in [
            "/api/v1/widgets",
            "/api/v1/alerts",
            "/api/v1/host",
            "/api/v1/host/history",
            "/api/v1/stream?topics=host",
            "/api/v1/overview",
            "/api/v1/stream?topics=alerts",
        ] {
            assert_eq!(req(addr, "GET", path, &h, "").await.status, 403, "{path}");
        }
        let (mut widget_stream, _) =
            Stream::open(addr, "/api/v1/stream?topics=widget", &cookie).await;
        assert_eq!(widget_stream.next().await.0, "hello");
        assert!(widget_stream.quiet(100).await);
        drop(widget_stream);
        let r = req(
            addr,
            "POST",
            "/api/settings/config/validate",
            &h,
            r#"{"text":"[page]"}"#,
        )
        .await;
        assert_eq!(r.status, 403, "{role}: {}", r.body);
        let r = req(addr, "GET", "/api/v1/users", &h, "").await;
        assert_api_error(&r, 403, "forbidden", role);
        let r = req(
            addr,
            "PATCH",
            "/api/v1/users/1/disabled",
            &h,
            r#"{"disabled":true}"#,
        )
        .await;
        assert_api_error(&r, 403, "forbidden", role);

        let account = st
            .db
            .accounts()
            .unwrap()
            .into_iter()
            .find(|u| u.name == format!("guest-{role}"))
            .unwrap();
        assert!(account.can_see_board(&scope));
        assert!(!account.can_see_board("other"));
        let base = format!("/api/v1/users/{}", account.id);
        let r = req(
            addr,
            "PATCH",
            &format!("{base}/disabled"),
            &owner_headers,
            r#"{"disabled":true}"#,
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(get_session(addr, &h).await["user"].is_null());
        let r = req(
            addr,
            "PATCH",
            &format!("{base}/disabled"),
            &owner_headers,
            r#"{"disabled":false}"#,
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(get_session(addr, &h).await["user"].is_null());
        let r = req(addr, "PUT", &format!("{base}/scopes"), &owner_headers, "{}").await;
        assert_api_error(
            &r,
            400,
            "bad_request",
            "omitting scopes cannot broaden access",
        );
        let r = req(
            addr,
            "PUT",
            &format!("{base}/scopes"),
            &owner_headers,
            r#"{"scopes":null}"#,
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.body);
        let r = req(addr, "DELETE", &base, &owner_headers, "").await;
        assert_eq!(r.status, 200, "{}", r.body);
    }
    for (method, path, body) in [
        ("PATCH", "/api/v1/users/1/role", r#"{"role":"viewer"}"#),
        ("PATCH", "/api/v1/users/1/disabled", r#"{"disabled":true}"#),
        ("DELETE", "/api/v1/users/1", ""),
    ] {
        let r = req(addr, method, path, &owner_headers, body).await;
        assert_api_error(&r, 403, "protected_owner", path);
    }
}

#[tokio::test]
async fn password_change_preserves_only_the_callers_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let (cookie, csrf) = login(addr).await;
    let (other, _) = login(addr).await;
    let (third, _) = login(addr).await;
    let origin = format!("http://{addr}");
    let h = [
        ("Origin", origin.as_str()),
        ("Cookie", cookie.as_str()),
        ("X-RH-CSRF", csrf.as_str()),
        ("Content-Type", "application/json"),
    ];
    let r = req(addr, "GET", "/api/v1/account/sessions", &h, "").await;
    let sessions = json_of(&r, "other sessions");
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    let id = sessions[0]["id"].as_str().unwrap();
    let r = req(
        addr,
        "DELETE",
        &format!("/api/v1/account/sessions/{id}"),
        &h,
        "",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_ne!(
        get_session(addr, &[("Cookie", &other)]).await["user"].is_null(),
        get_session(addr, &[("Cookie", &third)]).await["user"].is_null()
    );
    let r = req(
        addr,
        "POST",
        "/api/v1/account/password",
        &h,
        &serde_json::json!({"current": PW, "new":"new password"}).to_string(),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(get_session(addr, &h).await["csrf"], csrf);
    for revoked in [other, third] {
        assert!(get_session(addr, &[("Cookie", &revoked)]).await["user"].is_null());
    }
}

#[tokio::test]
async fn locked_onboarding_requires_console_recovery() {
    let st = state();
    assert!(st.db.kv_set("auth.onboarding_locked", "1"));
    let addr = start(st.clone()).await;
    assert_eq!(get_session(addr, &[]).await["onboarding_locked"], true);
    let origin = format!("http://{addr}");
    let response = req(
        addr,
        "POST",
        "/api/v1/onboarding/owner",
        &[("Origin", &origin), ("Content-Type", "application/json")],
        r#"{"username":"alice","password":"correct horse battery"}"#,
    )
    .await;
    assert_api_error(&response, 403, "onboarding_locked", "locked onboarding");
    assert!(response.body.contains("hobnail set-password"));
    assert!(!st.db.owner_exists());
    crate::auth::set_password_for_tests(&st.db, PW);
    let session = get_session(addr, &[]).await;
    assert_eq!(session["onboarding_locked"], false);
    assert_eq!(session["owner_exists"], true);
}

#[tokio::test]
async fn viewer_cannot_read_administrative_details() {
    use crate::db::accounts::Role;
    let st = privacy_state("viewer").await;
    break_config(&st);
    let now = crate::auth::now_s();
    st.db
        .create_invite("privacy-invite", Role::Viewer, None, now, now + 60)
        .unwrap();
    let token = "ab".repeat(32);
    st.db
        .accept_invite(
            "privacy-invite",
            "guest",
            "unused",
            &crate::auth::sha256_hex(token.as_bytes()),
            now,
            now + 60,
        )
        .unwrap();
    let addr = start(st).await;
    let cookie = format!("rh_session={token}");
    let h = [("Cookie", cookie.as_str())];
    for path in [
        "/api/settings/config",
        "/api/settings/revisions",
        "/api/settings/history/export",
        "/api/settings/security",
    ] {
        assert_eq!(req(addr, "GET", path, &h, "").await.status, 403, "{path}");
    }
    let bootstrap = json_of(
        &req(addr, "GET", "/api/v1/bootstrap", &h, "").await,
        "viewer bootstrap",
    );
    assert!(bootstrap["config_error"].is_null());
    for path in ["/api/v1/containers/web/env", "/api/v1/containers/web/top"] {
        assert_eq!(req(addr, "GET", path, &h, "").await.status, 403, "{path}");
    }
    let inspect = req(addr, "GET", "/api/v1/containers/web/inspect", &h, "").await;
    assert_eq!(
        json_of(&inspect, "viewer inspect")["env_keys"],
        serde_json::json!(["PASSWORD"])
    );
    let view = json_of(&inspect, "viewer inspect");
    assert!(view["health_test"].is_null());
    assert!(view["mounts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|mount| mount["source"].is_null()));
    assert_eq!(
        req(addr, "GET", "/api/settings/config", &h, "")
            .await
            .status,
        403
    );
    assert!(!inspect.body.contains("SECRET-CMD"));
    assert!(!inspect.body.contains("hunter2"));
    let metrics = req(addr, "GET", "/api/v1/containers/web/metrics", &h, "").await;
    assert_eq!(
        json_of(&metrics, "viewer metrics")["series"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    let events = req(addr, "GET", "/api/v1/containers/web/events", &h, "").await;
    assert_eq!(
        json_of(&events, "viewer events")["events"][0]["kind"],
        "container.start"
    );
    assert!(!events.body.contains("auth.login_failed"));
    let detail = json_of(
        &req(addr, "GET", "/api/v1/containers/web", &h, "").await,
        "viewer detail",
    );
    assert!(detail["why"]["probe"].is_null());
    assert_eq!(detail["why"]["checks"], serde_json::json!([]));
    assert!(!detail.to_string().contains("auth.login_failed"));
    for path in ["/api/events", "/api/events?kind=auth."] {
        let response = req(addr, "GET", path, &h, "").await;
        assert_eq!(response.status, 200, "{path}");
        assert!(!response.body.contains("192.0.2.7"), "{path}");
        assert!(!response.body.contains(CONFIG_SECRET), "{path}");
        assert!(!response.body.contains("SECRET-PROBE-OUTPUT"), "{path}");
    }
    let facts = req(addr, "GET", "/api/v1/host", &h, "").await;
    let facts = json_of(&facts, "viewer host");
    assert_eq!(facts["hostname"], "testhost");
    assert!(facts["processes"].is_null());
    let series = req(addr, "GET", "/api/v1/host/history", &h, "").await;
    assert!(json_of(&series, "viewer host history")["metrics"].is_array());
    let (mut stream, _) = Stream::open(addr, "/api/v1/stream?topics=host", &cookie).await;
    assert_eq!(stream.next().await.0, "hello");
    let (topic, live) = stream.next().await;
    assert_eq!(topic, "host");
    assert!(live["processes"].is_null());
    assert!(live["process_groups"].is_null());
    let overview = req(addr, "GET", "/api/v1/overview?probes=1", &h, "").await;
    assert_eq!(
        json_of(&overview, "viewer overview")["attention"][0]["reason"],
        "unhealthy"
    );
    assert!(!overview.body.contains("SECRET-PROBE-OUTPUT"));
    assert!(!overview.body.contains(HIDDEN_PREFIX));
    let alerts = req(addr, "GET", "/api/v1/alerts", &h, "").await;
    assert_eq!(
        json_of(&alerts, "viewer alerts")["rules"][0]["name"],
        "shop-down"
    );
    assert!(!alerts.body.contains("channel-secret"));
    let widgets = req(addr, "GET", "/api/v1/widgets", &h, "").await;
    assert_eq!(json_of(&widgets, "viewer widgets")[0]["id"], "probe-widget");
    assert!(!widgets.body.contains("widget-secret"));
    assert!(!widgets.body.contains("/private"));
    let bookmarks = req(addr, "GET", "/api/v1/bookmarks", &h, "").await;
    assert_eq!(
        json_of(&bookmarks, "viewer bookmarks")[0]["name"],
        "Example"
    );
    let path = "/api/v1/history?range=24h&cmp=1&events=all";
    let history = req(addr, "GET", path, &h, "").await;
    assert!(history
        .head
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let history = json_of(&history, "viewer history");
    assert_eq!(history["metrics"].as_array().unwrap().len(), 8);
    assert!(history["metrics"][0]["yesterday"].is_array());
    assert!(!history.to_string().contains("auth.login_failed"));
    let bad = req(addr, "GET", "/api/v1/history?from=1", &h, "").await;
    assert_eq!(bad.status, 400);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&bad.body).unwrap()["error"]["field"],
        "to"
    );
    let (owner, _) = login(addr).await;
    let admin_headers = [("Cookie", owner.as_str())];
    let initial = req(addr, "GET", "/api/v1/host", &admin_headers, "").await;
    assert!(json_of(&initial, "owner initial host")["processes"].is_null());
    let (mut admin_stream, _) = Stream::open(addr, "/api/v1/stream?topics=host", &owner).await;
    assert_eq!(admin_stream.next().await.0, "hello");
    let (topic, sample) = admin_stream.next().await;
    assert_eq!(topic, "host");
    assert!(sample["processes"].is_array());
    assert!(sample["process_groups"].is_array());
    drop(admin_stream);
    let inspect = req(
        addr,
        "GET",
        "/api/v1/containers/web/inspect",
        &admin_headers,
        "",
    )
    .await;
    assert!(inspect.body.contains("SECRET-CMD"));
    assert!(inspect.body.contains("SECRET-MOUNT"));
    assert!(inspect.body.contains("SECRET-HEALTH-TEST"));
    assert!(!inspect.body.contains("hunter2"));
    let env = req(
        addr,
        "GET",
        "/api/v1/containers/web/env",
        &admin_headers,
        "",
    )
    .await;
    assert_eq!(json_of(&env, "revealed env")["env"][0][1], "hunter2");
    let owner_overview = req(
        addr,
        "GET",
        "/api/v1/overview?probes=1",
        &[("Cookie", &owner)],
        "",
    )
    .await;
    assert!(owner_overview.body.contains("SECRET-PROBE-OUTPUT"));
    let owner_history = req(addr, "GET", path, &[("Cookie", &owner)], "").await;
    assert_eq!(owner_history.status, 200);
    assert!(owner_history.body.contains("auth.login_failed"));
}

#[tokio::test]
async fn session_renewal_sets_a_cookie_once_per_slide() {
    let st = state();
    let addr = start(st.clone()).await;
    for (index, path) in ["/api/v1/session", "/api/v1/bootstrap", "/api/events"]
        .into_iter()
        .enumerate()
    {
        let token = format!("{index:064x}");
        let hash = crate::auth::sha256_hex(token.as_bytes());
        let now = crate::auth::now_s();
        let ttl = crate::auth::SESSION_TTL_SECS;
        assert!(st.db.create_session(&hash, now - 7200, now + ttl - 7200));
        let cookie = format!("rh_session={token}");
        let h = [("Cookie", cookie.as_str())];
        let first = req(addr, "GET", path, &h, "").await;
        assert_eq!(first.status, 200, "{path}");
        let renewed = header(&first.head, "set-cookie").expect("renewed cookie");
        assert!(renewed.starts_with(&cookie));
        assert!(renewed.ends_with(&format!("Max-Age={ttl}")));
        let second = req(addr, "GET", path, &h, "").await;
        assert_eq!(second.status, 200, "{path}");
        assert!(header(&second.head, "set-cookie").is_none());
    }
}

#[tokio::test]
async fn session_distinguishes_unavailable_database_from_fresh_installation() {
    let mut st = state();
    Arc::get_mut(&mut st).unwrap().db = crate::db::Db::disabled().into();
    let addr = start(st).await;
    let session = get_session(addr, &[]).await;
    assert_eq!(session["database"], false);
    assert_eq!(session["owner_exists"], false);
    assert!(session["user"].is_null());
}

/// JSON equivalents of the classic Settings fragment, edit and cache checks.
#[tokio::test]
async fn settings_json_values_and_hashes_require_an_admin_session() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let config = r#"# raw-marker
[page]
greeting = "greeting-marker"

[docker]
exclude = ["exclude-marker"]

[[groups]]
name = "group-marker"

[containers.override-marker]
name = "Override marker"

[[bookmarks]]
name = "Wiki marker"
url = "https://wiki.example/bookmark-marker"

[[widgets]]
id = "widget-marker"
type = "custom-api"
url = "http://widget.example/marker"

[channels.channel-marker]
type = "webhook"
url_env = "RH_GATE_MARKER"

[[alerts]]
name = "rule-marker"
type = "container"
channels = ["channel-marker"]

[history]
sample_interval = "45s"
"#;
    std::fs::write(st.config.path(), config).unwrap();
    std::fs::write(st.env.data_dir.join("custom.css"), "/* css-marker */").unwrap();
    let addr = start(st).await;
    let (cookie, _) = login(addr).await;
    let hash = crate::auth::sha256_hex(config.as_bytes());
    for (path, values) in [
        ("bookmarks", &["Wiki marker", "bookmark-marker"][..]),
        ("widgets", &["widget-marker", "widget.example"]),
        (
            "notifications",
            &["RH_GATE_MARKER", "channel-marker", "rule-marker"],
        ),
        (
            "containers",
            &["exclude-marker", "group-marker", "Override marker"],
        ),
        ("general", &["greeting-marker"]),
        ("history", &["45s"]),
        ("config", &["raw-marker"]),
        ("appearance", &["css-marker"]),
        ("revisions", &[]),
        ("security", &[]),
    ] {
        let url = format!("/api/settings/{path}");
        let anon = req(addr, "GET", &url, &[], "").await;
        assert_eq!(anon.status, 401, "{url}");
        assert!(!anon.body.contains(&hash), "{url}");
        let signed = req(addr, "GET", &url, &[("Cookie", &cookie)], "").await;
        assert_eq!(signed.status, 200, "{url}: {}", signed.body);
        assert_json_no_store(&signed, &url);
        for value in values {
            assert!(!anon.body.contains(value), "{url} exposed {value}");
            assert!(signed.body.contains(value), "{url} omitted {value}");
        }
        let data = json_of(&signed, &url);
        match path {
            "security" | "revisions" => {}
            "appearance" => assert_eq!(data["hash"], crate::auth::sha256_hex(b"/* css-marker */")),
            _ => assert_eq!(data["hash"], hash, "{url}"),
        }
    }
}
