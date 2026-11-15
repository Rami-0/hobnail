//! Route tests and the end-to-end "stream stops after disconnect" test
//! against a fake Docker daemon on a unix socket.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener};

use crate::state::{AppState, Env};
use crate::ui::shell::Ctx;
use crate::ui::testutil::{ctr, state_with};

fn page(st: &AppState, path: &str, query: &str) -> (hyper::StatusCode, String) {
    let h = hyper::HeaderMap::new();
    let ctx = Ctx::new(st, "h", path, query, &h);
    crate::ui::pages::render(&ctx)
}

fn sample() -> Vec<crate::model::Container> {
    vec![
        ctr("web", Some("shop"), "running"),
        ctr("db", Some("shop"), "exited"),
        ctr("solo", None, "running"),
    ]
}

#[test]
fn container_page_renders_frame_and_assets() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains("/static/logs.js?v="), "logs.js linked");
    assert!(html.contains("/static/logs.css?v="));
    assert!(html.contains(r#"data-q="c=web""#), "{html}");
    assert!(html.contains("data-ring=\"5000\""));
    assert!(html.contains("/stacks/shop/logs"), "link to the stack view");
    assert!(!html.contains("container is stopped"));
    // The stopped one says so and still renders the viewer.
    let (s, html) = page(&st, "/containers/db/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains("This container is stopped."));
    assert!(html.contains(r#"data-q="c=db""#));
}

#[test]
fn not_found_and_escaping() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/wbe/logs", "");
    assert_eq!(s, 404);
    assert!(html.contains("Container not found"));
    let (s, html) = page(&st, "/containers/<b>x<%2Fb>/logs", "");
    assert_eq!(s, 404);
    assert!(!html.contains("<b>x"), "{html}");
    let (s, html) = page(&st, "/stacks/nope/logs", "");
    assert_eq!(s, 404);
    assert!(html.contains("No such stack"));
    let (s, _) = page(&st, "/stacks/%3Cscript%3E/logs", "");
    assert_eq!(s, 404);
}

#[test]
fn stack_and_split_pages() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/stacks/shop/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains(r#"data-q="stack=shop""#));
    assert!(html.contains(r#"data-name="db""#) && html.contains(r#"data-name="web""#));
    assert!(html.contains("2 containers, merged by time"));
    let (s, html) = page(&st, "/logs", "c=web,solo&c=db&c=bad%2Fname");
    assert_eq!(s, 200);
    assert_eq!(html.matches("class=\"lpane\"").count(), 3, "{html}");
    // Names that are not shown containers are dropped (never sent to Docker).
    let (_, html) = page(&st, "/logs", "c=web,ghost");
    assert_eq!(html.matches("class=\"lpane\"").count(), 1, "{html}");
    let (s, html) = page(&st, "/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains("Logs side by side"));
    assert!(html.contains(r#"name="c" value="web""#));
}

#[test]
fn docker_unreachable_banner() {
    let st = state_with(vec![], OPEN);
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        containers: vec![],
        fetched_at: 1,
        error: Some("cannot connect".into()),
    }));
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200, "page renders; the stream reports the error");
    assert!(html.contains("Docker is unreachable."));
}

/// Feature tests opt out of the default login wall explicitly.
const OPEN: &str = "[logs]\nrequire_login = false\n";

#[test]
fn require_login_hides_logs() {
    let st = state_with(sample(), "[logs]\nrequire_login = true\n");
    assert!(st.config.snapshot().error.is_none());
    for p in ["/containers/web/logs", "/stacks/shop/logs", "/logs"] {
        let (s, html) = page(&st, p, "");
        assert_eq!(s, 401, "{p}");
        assert!(html.contains("Sign in to read logs"));
        assert!(!html.contains("logs.js"));
    }
}

#[test]
fn level_chart_states() {
    let st = state_with(sample(), OPEN);
    let html = super::level_chart(&st, "web", crate::db::Range::H1).into_string();
    assert!(html.contains("log_match"), "empty state explains: {html}");
    let now = crate::util::unix_now() as i64 / 60 * 60;
    st.db
        .record_log_counts(&[("web".into(), now - 120, [3, 2, 10, 0, 0])]);
    if st.db.is_enabled() {
        let html = super::level_chart(&st, "web", crate::db::Range::H1).into_string();
        assert!(html.contains("<svg"), "{html}");
        assert!(html.contains("3 error and 2 warning lines"));
    }
}

#[test]
fn helpers() {
    assert_eq!(super::logs_url("a b"), "/containers/a%20b/logs");
    assert_eq!(super::stack_logs_url("shop"), "/stacks/shop/logs");
    assert_eq!(super::split_url(&["a", "b"]), "/logs?c=a,b");
    assert_eq!(super::parse_since("start", 100.0), Some(-1.0));
    assert_eq!(super::parse_since("1m", 100.0), Some(40.0));
    assert_eq!(super::parse_since("12.5", 100.0), Some(12.5));
    assert_eq!(super::parse_since("junk", 100.0), None);
    assert_eq!(
        super::tail::iso_ms(1_000_123_456_789),
        "1970-01-01T00:16:40.123Z"
    );
}

#[test]
fn script_never_uses_inner_html() {
    let js = crate::http::server::LOGS_JS;
    assert!(
        !js.contains("innerHTML"),
        "log text must go through textContent"
    );
    assert!(!js.contains("insertAdjacentHTML"));
    assert!(!js.contains("outerHTML"));
}

// ---------------------------------------------------------------- fake docker

struct Fake {
    path: std::path::PathBuf,
    /// Log connections currently open.
    open: Arc<AtomicUsize>,
    /// Set when a log connection was closed by the client.
    closed: Arc<AtomicBool>,
}

fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![stream, 0, 0, 0];
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

async fn fake_docker(tag: &str) -> Fake {
    let path = std::env::temp_dir().join(format!("rh-logs-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let open = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let (o, c) = (open.clone(), closed.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (o, c) = (o.clone(), c.clone());
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
                if path.ends_with("/json") {
                    let body = if path.contains("/web/") {
                        r#"{"Id":"abc123","Name":"/web","Config":{"Tty":false},"State":{"Status":"running","Running":true,"StartedAt":"2026-11-14T21:00:00Z"}}"#
                    } else {
                        r#"{"message":"No such container"}"#
                    };
                    let status = if path.contains("/web/") {
                        "200 OK"
                    } else {
                        "404 Not Found"
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                    return;
                }
                o.fetch_add(1, Ordering::SeqCst);
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nConnection: close\r\n\r\n")
                    .await;
                let (mut rd, mut wr) = s.into_split();
                let writer = async {
                    let mut n = 0u64;
                    loop {
                        n += 1;
                        let line = format!(
                            "2026-11-14T21:30:{:02}.000000000Z \x1b[31mERROR\x1b[0m line {n} <b>&\n",
                            n % 60
                        );
                        if wr
                            .write_all(&frame(1 + (n % 2) as u8, line.as_bytes()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                };
                let reader = async {
                    let mut b = [0u8; 64];
                    loop {
                        match rd.read(&mut b).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                    }
                };
                tokio::select! { _ = writer => {}, _ = reader => {} }
                o.fetch_sub(1, Ordering::SeqCst);
                c.store(true, Ordering::SeqCst);
            });
        }
    });
    Fake { path, open, closed }
}

fn state_on(socket: &std::path::Path, tag: &str) -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!("rh-logs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), OPEN).unwrap();
    let st = AppState::new(Env {
        listen: vec![],
        data_dir: dir.clone(),
        config_path: dir.join("config.toml"),
        docker_socket: socket.display().to_string(),
        host_proc: "/nonexistent".into(),
        host_sys: "/nonexistent".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    });
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        containers: vec![ctr("web", Some("shop"), "running")],
        fetched_at: 1,
        error: None,
    }));
    st
}

async fn serve(st: Arc<AppState>) -> std::net::SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(crate::http::server::serve_listener(st, l));
    addr
}

async fn read_until(s: &mut TcpStream, needle: &str, limit: Duration) -> String {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    let end = Instant::now() + limit;
    while !String::from_utf8_lossy(&got).contains(needle) {
        let left = end.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, s.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&got).to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn stream_tails_and_stops_after_disconnect() {
    let fake = fake_docker("sse").await;
    let st = state_on(&fake.path, "sse");
    let addr = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET /api/logs/stream?c=web&tail=10 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let got = read_until(&mut s, "event: l", Duration::from_secs(5)).await;
    assert!(got.contains("text/event-stream"), "{got}");
    assert!(got.contains("event: meta"), "{got}");
    let got = got + &read_until(&mut s, "line 3", Duration::from_secs(5)).await;
    // ANSI is passed through (the browser renders it); HTML is plain JSON text.
    assert!(got.contains(r#"\u001b[31mERROR"#), "{got}");
    assert!(got.contains("<b>&"), "{got}");
    assert!(got.contains(r#""l":"e""#), "level detected: {got}");
    assert!(got.contains(r#""s":2"#), "stderr marked");
    assert_eq!(fake.open.load(Ordering::SeqCst), 1);
    let t = Instant::now();
    drop(s);
    while fake.open.load(Ordering::SeqCst) > 0 && t.elapsed() < Duration::from_secs(8) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let took = t.elapsed();
    assert!(
        fake.closed.load(Ordering::SeqCst),
        "docker stream was closed"
    );
    assert!(took < Duration::from_secs(5), "stopped after {took:?}");
    eprintln!("docker stream closed {took:?} after the viewer left");
}

#[tokio::test(flavor = "current_thread")]
async fn stream_reports_missing_container_and_bad_request() {
    let fake = fake_docker("missing").await;
    let st = state_on(&fake.path, "missing");
    let addr = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    // A name that is not a shown container is refused before any Docker call.
    s.write_all(b"GET /api/logs/stream?c=ghost HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let got = read_until(&mut s, "\r\n\r\n", Duration::from_secs(5)).await;
    assert!(got.starts_with("HTTP/1.1 400"), "{got}");
    assert_eq!(fake.open.load(Ordering::SeqCst), 0, "Docker was not asked");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET /api/logs/stream?c=..%2Fetc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let got = read_until(&mut s, "\r\n\r\n", Duration::from_secs(5)).await;
    assert!(got.starts_with("HTTP/1.1 400"), "{got}");
}

#[tokio::test(flavor = "current_thread")]
async fn download_streams_text_with_disposition() {
    let fake = fake_docker("dl").await;
    let st = state_on(&fake.path, "dl");
    // The fake never ends a log, so ask for a stopped-style read: the
    // download uses follow=0, and the fake closes nothing; read a bit.
    let addr = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET /api/logs/download?c=web&since=1h HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let got = read_until(&mut s, "line 2", Duration::from_secs(8)).await;
    assert!(
        got.contains("content-disposition: attachment; filename=\"web-"),
        "{got}"
    );
    assert!(
        got.contains("2026-11-14T21:30:01.000Z stderr ERROR line 1 <b>&"),
        "ANSI removed: {got}"
    );
}

#[test]
fn logs_need_a_login_by_default() {
    // No [logs] section at all: fail closed.
    let st = state_with(sample(), "");
    for p in ["/containers/web/logs", "/stacks/shop/logs", "/logs"] {
        let (s, html) = page(&st, p, "");
        assert_eq!(s, 401, "{p}");
        assert!(html.contains("set-password"), "explains how to unlock: {p}");
    }
    assert!(!super::allowed(&st, &hyper::HeaderMap::new()));
}

#[test]
fn hidden_and_excluded_containers_have_no_logs() {
    let mut cs = sample();
    cs.push(ctr("secret-db", Some("shop"), "running"));
    let st = state_with(
        cs,
        "[logs]\nrequire_login = false\n[docker]\nexclude = [\"secret-*\"]\n[containers.web]\nhide = true\n",
    );
    assert!(!super::may_tail(&st, "secret-db"), "excluded by glob");
    assert!(!super::may_tail(&st, "web"), "hidden by override");
    assert!(!super::may_tail(&st, "ghost"), "not a container here");
    assert!(super::may_tail(&st, "db"));
    let (s, _) = page(&st, "/containers/secret-db/logs", "");
    assert_eq!(s, 404);
    let (s, _) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 404);
    let members = super::stack_members(&st, "shop");
    assert!(
        !members.iter().any(|m| m == "secret-db" || m == "web"),
        "{members:?}"
    );
}
