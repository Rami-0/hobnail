//! Route tests and the end-to-end "stream stops after disconnect" test
//! against a fake Docker daemon on a unix socket.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener};

use crate::state::{AppState, Env};
use crate::testutil::{ctr, sample, state_with};

const OPEN: &str = "[logs]\nrequire_login = false\n";

#[test]
fn legacy_require_login_setting_still_parses() {
    for config in [OPEN, "[logs]\nrequire_login = true\n"] {
        let st = state_with(sample(), config);
        assert!(st.config.snapshot().error.is_none());
    }
}

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

async fn serve(st: Arc<AppState>) -> (std::net::SocketAddr, String) {
    let token = "ab".repeat(32);
    let hash = crate::auth::sha256_hex(token.as_bytes());
    let now = crate::auth::now_s();
    assert!(st.db.create_session(&hash, now, now + 60));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(crate::http::server::serve_listener(st, l));
    (addr, format!("rh_session={token}"))
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
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=web&tail=10 HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
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

/// A closed window is read from its start (`tail=all` toward Docker) and
/// cut by the server at the asked-for count; an open one keeps `tail`.
#[test]
fn closed_window_is_cut_by_the_server() {
    let q = crate::http::query::parse_query("c=web&since=100&until=200&tail=3");
    let rq = super::stream_query(&q, 500);
    assert_eq!((rq.tail, rq.cap, rq.follow), (None, Some(3), false));
    assert!(
        rq.path("x").contains("&tail=all&since=100."),
        "{}",
        rq.path("x")
    );
    let rq = super::stream_query(
        &crate::http::query::parse_query("c=web&until=200&tail=9000"),
        500,
    );
    assert_eq!((rq.tail, rq.cap), (Some(5000), None));
    let rq = super::stream_query(&crate::http::query::parse_query("c=web"), 500);
    assert_eq!((rq.tail, rq.cap, rq.follow), (Some(500), None, true));
}

#[tokio::test(flavor = "current_thread")]
async fn closed_window_stops_at_the_cap() {
    let fake = fake_docker("cap").await;
    let st = state_on(&fake.path, "cap");
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=web&since=100&until=200&tail=3 HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "event: done", Duration::from_secs(5)).await;
    assert!(got.contains("event: done"), "{got}");
    assert!(got.contains("line 3 ") && !got.contains("line 4 "), "{got}");
    // More lines were in the window: the end event says the read was cut.
    assert!(
        got.contains(
            r#"event: end
data: {"c":0,"cut":true}"#
        ),
        "{got}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stream_reports_missing_container_and_bad_request() {
    let fake = fake_docker("missing").await;
    let st = state_on(&fake.path, "missing");
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    // A name that is not a shown container is refused before any Docker call.
    s.write_all(
        format!(
            "GET /api/logs/stream?c=ghost HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let got = read_until(&mut s, "\r\n\r\n", Duration::from_secs(5)).await;
    assert!(got.starts_with("HTTP/1.1 400"), "{got}");
    assert_eq!(fake.open.load(Ordering::SeqCst), 0, "Docker was not asked");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=..%2Fetc HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
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
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/download?c=web&since=1h HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\nAccept-Encoding: gzip\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "line 2", Duration::from_secs(8)).await;
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(!got.contains("content-encoding:"));
    assert!(!got.contains("vary:"));
    assert!(
        got.contains("content-type: text/plain; charset=utf-8"),
        "{got}"
    );
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
    let viewer = crate::api::access::Principal {
        id: 1,
        name: "viewer".into(),
        role: crate::db::accounts::Role::Viewer,
        disabled: false,
        created: 0,
        updated: 0,
        scopes: None,
    };
    let visible = viewer.container_visibility(&st);
    let query = crate::http::query::parse_query("stack=shop");
    let members = super::targets(&st, &query, 4, &visible);
    assert_eq!(members, ["db"]);
}

#[test]
fn scoped_stack_limit_applies_after_visibility() {
    use crate::api::access::Principal;
    use crate::db::accounts::{Role, Scopes};
    let mut containers: Vec<_> = (0..30)
        .map(|n| crate::testutil::ctr(&format!("a{n:02}"), Some("shop"), "running"))
        .collect();
    containers.push(crate::testutil::ctr("z-last", Some("shop"), "running"));
    let state = crate::testutil::state_with(containers, "");
    let board = "11111111111111111111111111111111";
    state.db.dashboard(|conn| -> rusqlite::Result<()> {
        conn.execute("INSERT INTO boards (id,name,created,updated) VALUES (?1,'Logs grant',0,0)", [board])?;
        conn.execute("INSERT INTO board_items (board_id,kind,config,layout) VALUES (?1,'container',?2,'{}')", rusqlite::params![board, r#"{"kind":"container","target":{"by":"compose","project":"shop","service":"z-last"}}"#])?;
        Ok(())
    }).unwrap();
    let viewer = Principal {
        id: 1,
        name: "viewer".into(),
        role: Role::Viewer,
        disabled: false,
        created: 0,
        updated: 0,
        scopes: Some(Scopes {
            boards: vec![board.into()],
            integrations: vec![],
        }),
    };
    let visible = viewer.container_visibility(&state);
    assert_eq!(
        super::targets(&state, &[("stack".into(), "shop".into())], 4, &visible),
        vec!["z-last"]
    );
}
