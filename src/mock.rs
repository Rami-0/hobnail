//! `hobnail mock-receiver [addr]`: a local fake for webhook/Slack/Telegram
//! endpoints, used to test notifications without contacting real services.
//!
//! Accepts any method and path. Every request is printed to stdout as one
//! JSON line and the last 500 are kept in memory:
//!
//! - `GET /_received`: the recorded requests as a JSON array
//! - `GET /_clear`: forget them
//! - any path containing `/fail`: HTTP 500 (to test "channel down")
//! - Telegram paths (`/bot<token>/<method>`): `{"ok":true,"result":{"message_id":N}}`
//! - paths starting with `/slack`: `ok` (like Slack)
//! - anything else: `{"ok":true}`
//!
//! Bot tokens in paths are recorded as `/bot<redacted>/…`, and the values of
//! `Authorization` headers are not recorded.
//!
//! Point channels at it, e.g. `url = "http://127.0.0.1:9999/hook"`,
//! `webhook_url_env` = `http://127.0.0.1:9999/slack/T/B/x`, or
//! `api_base = "http://127.0.0.1:9999"` for Telegram.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const KEEP: usize = 500;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Default)]
pub struct Mock {
    received: Mutex<VecDeque<Value>>,
    next_message_id: AtomicU64,
    print: bool,
}

impl Mock {
    pub fn received(&self) -> Vec<Value> {
        self.received.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.received.lock().unwrap().clear();
    }
}

/// Bind and serve in the background. Returns the bound address (useful
/// with port 0) and a handle to the recorded requests.
pub async fn start(addr: SocketAddr, print: bool) -> Result<(SocketAddr, Arc<Mock>), String> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let mock = Arc::new(Mock {
        print,
        ..Default::default()
    });
    let m = mock.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            };
            let m = m.clone();
            tokio::spawn(async move {
                // The main server's socket type, exempt from its write-stall
                // limit (the mock never times out a client): one copy of
                // hyper's connection code serves both, which keeps the
                // release binary smaller.
                let rule = Arc::new(crate::http::conn::StallRule::default());
                rule.set(crate::http::conn::Stall::Exempt);
                let stream = crate::http::conn::Timed::new(
                    stream,
                    std::time::Duration::MAX,
                    std::time::Duration::MAX,
                    rule,
                );
                let svc = hyper::service::service_fn(move |req| {
                    let m = m.clone();
                    async move { Ok::<_, Infallible>(handle(&m, req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    Ok((local, mock))
}

pub async fn run(args: &[String]) -> i32 {
    let addr = args.first().map(String::as_str).unwrap_or("127.0.0.1:9999");
    let addr: SocketAddr = match addr.parse() {
        Ok(a) => a,
        Err(_) => {
            eprintln!("usage: hobnail mock-receiver [addr]   (default 127.0.0.1:9999)");
            return 2;
        }
    };
    match start(addr, true).await {
        Ok((local, _mock)) => {
            eprintln!(
                "mock-receiver listening on http://{local}  (GET /_received, GET /_clear; paths containing /fail return 500)"
            );
            let _ = tokio::signal::ctrl_c().await;
            0
        }
        Err(e) => {
            eprintln!("mock-receiver: {e}");
            1
        }
    }
}

/// `/bot123:abc/sendMessage` -> `/bot<redacted>/sendMessage`.
pub fn redact_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.len() > 3 && seg.starts_with("bot") {
                "bot<redacted>"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn guess_kind(path: &str) -> &'static str {
    if is_telegram(path) {
        "telegram"
    } else if path.starts_with("/slack") || path.contains("/services/") {
        "slack"
    } else {
        "webhook"
    }
}

fn is_telegram(path: &str) -> bool {
    path.split('/').any(|s| s.len() > 3 && s.starts_with("bot"))
}

fn reply(status: StatusCode, ctype: &str, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(body.into()));
    *r.status_mut() = status;
    if let Ok(v) = hyper::header::HeaderValue::from_str(ctype) {
        r.headers_mut().insert(hyper::header::CONTENT_TYPE, v);
    }
    r
}

async fn handle(m: &Mock, req: Request<Incoming>) -> Response<Full<Bytes>> {
    const JSON: &str = "application/json";
    let path = req.uri().path().to_string();
    if req.method() == hyper::Method::GET && path == "/_received" {
        let all = Value::Array(m.received());
        return reply(StatusCode::OK, JSON, all.to_string());
    }
    if req.method() == hyper::Method::GET && path == "/_clear" {
        m.clear();
        return reply(StatusCode::OK, JSON, r#"{"ok":true}"#);
    }
    let method = req.method().to_string();
    let mut headers = serde_json::Map::new();
    for (k, v) in req.headers() {
        let name = k.as_str();
        if matches!(name, "host" | "content-length" | "accept" | "user-agent") {
            continue;
        }
        let val = if matches!(name, "authorization" | "proxy-authorization" | "cookie") {
            "<redacted>".to_string()
        } else {
            String::from_utf8_lossy(v.as_bytes()).into_owned()
        };
        headers.insert(name.to_string(), Value::String(val));
    }
    let ctype = req
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = Limited::new(req.into_body(), MAX_BODY)
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();
    let body_v = serde_json::from_slice::<Value>(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    let now = crate::util::unix_now();
    let kind = guess_kind(&path);
    let rec = json!({
        "time": crate::util::fmt_utc(now),
        "unix": now,
        "method": method,
        "path": redact_path(&path),
        "kind": kind,
        "content_type": ctype,
        "headers": headers,
        "body": body_v,
    });
    if m.print {
        println!("{rec}");
    }
    {
        let mut g = m.received.lock().unwrap();
        if g.len() >= KEEP {
            g.pop_front();
        }
        g.push_back(rec);
    }
    if path.contains("/fail") {
        return reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            JSON,
            r#"{"ok":false,"description":"mock failure"}"#,
        );
    }
    match kind {
        "telegram" => {
            let id = m.next_message_id.fetch_add(1, Ordering::Relaxed) + 1;
            reply(
                StatusCode::OK,
                JSON,
                json!({"ok": true, "result": {"message_id": id}}).to_string(),
            )
        }
        "slack" => reply(StatusCode::OK, "text/plain", "ok"),
        _ => reply(StatusCode::OK, JSON, r#"{"ok":true}"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HttpPolicy;
    use crate::http::client::{send, Request as CReq};

    #[test]
    fn redacts_tokens() {
        assert_eq!(
            redact_path("/bot123:abc/sendMessage"),
            "/bot<redacted>/sendMessage"
        );
        assert_eq!(redact_path("/hook/bot"), "/hook/bot");
        assert_eq!(guess_kind("/x/bot1:a/sendMessage"), "telegram");
        assert_eq!(guess_kind("/slack/T/B/x"), "slack");
        assert_eq!(guess_kind("/hook"), "webhook");
    }

    #[tokio::test]
    async fn records_and_replies() {
        let (addr, mock) = start("127.0.0.1:0".parse().unwrap(), false).await.unwrap();
        let p = HttpPolicy::default();
        let base = format!("http://{addr}");
        let r = send(
            CReq::post_json(
                format!("{base}/bot42:SECRET/sendMessage"),
                br#"{"text":"hi"}"#.to_vec(),
            )
            .header("authorization", "Bearer s3cret"),
            &p,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"]["message_id"], 1);
        let r = send(
            CReq::post_json(format!("{base}/slack/x"), b"{}".to_vec()),
            &p,
        )
        .await
        .unwrap();
        assert_eq!(r.text(), "ok");
        let r = send(
            CReq::post_json(format!("{base}/fail/hook"), b"nope".to_vec()),
            &p,
        )
        .await
        .unwrap();
        assert_eq!(r.status, 500);

        let r = send(CReq::get(format!("{base}/_received")), &p)
            .await
            .unwrap();
        let all: Vec<Value> = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0]["path"], "/bot<redacted>/sendMessage");
        assert_eq!(all[0]["kind"], "telegram");
        assert_eq!(all[0]["body"]["text"], "hi");
        assert_eq!(all[0]["headers"]["authorization"], "<redacted>");
        assert!(!all[0].to_string().contains("SECRET"));
        assert_eq!(all[1]["kind"], "slack");
        assert_eq!(all[2]["body"], "nope");
        send(CReq::get(format!("{base}/_clear")), &p).await.unwrap();
        assert!(mock.received().is_empty());
    }
}
