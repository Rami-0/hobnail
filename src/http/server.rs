//! HTTP server on hyper directly (no axum/tower): six routes do not need a
//! router framework, and skipping it saves binary size and allocations.
//!
//! Live updates use Server-Sent Events: each open page holds one `/api/stream`
//! connection. The connection counts as a viewer (which is what makes the
//! refresh loop run), receives "fragment changed" ids from the broadcast
//! channel, renders that fragment for its own Host (links depend on the
//! address the page was opened on) and pushes the HTML.

use bytes::Bytes;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::body::{Frame, Incoming};
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};

use crate::state::{AppState, ViewerGuard};

pub type Body = UnsyncBoxBody<Bytes, Infallible>;

/// Inlined into every page: tokens, themes and shell (`app.css`), then the
/// component layer (`components.css`), then the page parts.
pub const APP_CSS: &str = concat!(
    include_str!("../../static/app.css"),
    include_str!("../../static/components.css"),
    include_str!("../../static/cards.css"),
    include_str!("../../static/widgets.css"),
    include_str!("../../static/alerts.css"),
    include_str!("../../static/detail.css"),
    include_str!("../../static/host.css"),
);
/// `/static/app.js`: the component layer (`ui.js`, `window.HB`) and the app
/// script, one cached request.
pub const APP_JS: &str = concat!(
    include_str!("../../static/ui.js"),
    include_str!("../../static/app.js"),
);
/// Loaded only by the settings pages (kept out of every other page).
pub const SETTINGS_CSS: &str = include_str!("../../static/settings.css");
pub const SETTINGS_JS: &str = include_str!("../../static/settings.js");
/// Loaded only by the logs pages.
pub const LOGS_CSS: &str = include_str!("../../static/logs.css");
pub const LOGS_JS: &str = include_str!("../../static/logs.js");

/// 64-bit FNV-1a of an asset's bytes, at compile time: the `?v=` of its URL.
/// Keyed on the content, not the build id (`dev`, `nogit`, `<sha>-dirty` can
/// stay the same while the files change).
const fn fnv1a(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}
const fn hex16(h: u64) -> [u8; 16] {
    let d = b"0123456789abcdef";
    let mut o = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        o[i] = d[((h >> (60 - 4 * i)) & 0xf) as usize];
        i += 1;
    }
    o
}
macro_rules! version_of {
    ($name:ident, $hex:ident, $src:expr) => {
        const $hex: [u8; 16] = hex16(fnv1a($src));
        pub const $name: &str = match std::str::from_utf8(&$hex) {
            Ok(s) => s,
            Err(_) => panic!("hex is ASCII"),
        };
    };
}
version_of!(APP_CSS_V, APP_CSS_HEX, APP_CSS);
version_of!(APP_JS_V, APP_JS_HEX, APP_JS);
version_of!(SETTINGS_CSS_V, SETTINGS_CSS_HEX, SETTINGS_CSS);
version_of!(SETTINGS_JS_V, SETTINGS_JS_HEX, SETTINGS_JS);
version_of!(LOGS_CSS_V, LOGS_CSS_HEX, LOGS_CSS);
version_of!(LOGS_JS_V, LOGS_JS_HEX, LOGS_JS);

pub async fn serve(state: Arc<AppState>, addr: SocketAddr) -> Result<(), String> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    crate::info!("listening", addr = addr);
    serve_listener(state, listener).await
}

/// Accept loop on an already-bound listener (tests bind 127.0.0.1:0).
pub async fn serve_listener(state: Arc<AppState>, listener: TcpListener) -> Result<(), String> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                crate::warn!("accept failed", error = e);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let st = state.clone();
        tokio::spawn(async move {
            let svc = hyper::service::service_fn(move |mut req: Request<Incoming>| {
                let st = st.clone();
                req.extensions_mut().insert(Peer(peer));
                async move { Ok::<_, Infallible>(route(st, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

/// Host part of the request's Host header (no port), for building links.
pub fn request_host(req: &Request<Incoming>) -> String {
    let h = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let host = if let Some(rest) = h.strip_prefix('[') {
        // [v6]:port
        rest.split(']').next().unwrap_or(rest).to_string()
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h).to_string()
    };
    // Only allow characters that can be in a host name or IP.
    if host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
    {
        host
    } else {
        String::new() // fails the Host allowlist
    }
}

/// IP literals, localhost, the configured host name and `RH_ALLOWED_HOSTS`
/// globs are allowed.
pub fn host_allowed(host: &str, env: &crate::state::Env) -> bool {
    let h = host.to_ascii_lowercase();
    h.parse::<std::net::IpAddr>().is_ok()
        || h == "localhost"
        || h == env.hostname.to_ascii_lowercase()
        || env
            .allowed_hosts
            .iter()
            .any(|p| crate::util::glob_match(&p.to_ascii_lowercase(), &h))
}

/// The peer address of the connection, stored in request extensions.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // read by auth::route (login rate limit)
pub struct Peer(pub SocketAddr);

/// Central router. Ownership of route groups (so parallel work does not
/// collide in this file):
/// - `/api/login`, `/api/logout`            → `auth::route`
/// - `/api/history/*`, `/api/events`        → `db::api::route`
/// - `/api/settings/*`                      → `settings::route`
/// - every other page (`/`, `/containers`…) → `ui::routes::route`
///
async fn route(state: Arc<AppState>, req: Request<Incoming>) -> Response<Body> {
    let host = request_host(&req);
    let path = req.uri().path().to_string();
    // DNS-rebinding guard: refuse requests whose Host header names a domain we
    // don't know (a rebinding attack always carries the attacker's domain).
    // /healthz stays open for the healthcheck.
    if path != "/healthz" && !host_allowed(&host, &state.env) {
        return text(
            StatusCode::MISDIRECTED_REQUEST,
            "unknown Host; add it to RH_ALLOWED_HOSTS\n",
        );
    }
    let read = req.method() == Method::GET || req.method() == Method::HEAD;
    use crate::api::access::{self, Access, Lookup};
    let v1 = path.starts_with("/api/v1/");
    let lookup = v1.then(|| access::lookup(req.method(), &path));
    let public_asset = read
        && (path == "/app"
            || path.starts_with("/app/")
            || path.starts_with("/assets/")
            || matches!(
                path.as_str(),
                "/manifest.webmanifest"
                    | "/favicon.ico"
                    | "/icon.svg"
                    | "/icon-192.png"
                    | "/icon-512.png"
                    | "/apple-touch-icon.png"
            ));
    let page = read
        && !(path.starts_with("/api/")
            || path.starts_with("/static/")
            || path.starts_with("/icons/")
            || matches!(path.as_str(), "/custom.css" | "/alarm.json"));
    let access = match &lookup {
        Some(Lookup::Found(m)) => m.route.access,
        // Unknown paths have no handler or data. Keep v1 404/405 for reads.
        Some(_) if read => Access::Public,
        _ if public_asset
            || (read && path == "/healthz")
            || (req.method() == Method::POST && path == "/api/login") =>
        {
            Access::Public
        }
        _ if !read && path.starts_with("/api/settings/") => Access::Admin,
        _ => Access::Viewer,
    };
    let session = match access::check(&state, req.headers(), access, !read) {
        Ok(session) => session,
        Err(denied) => return denied.response(v1, page),
    };
    if let Some(lookup) = lookup {
        return crate::api::route(&state, req, lookup, session).await;
    }
    if !read {
        return match path.as_str() {
            "/api/login" | "/api/logout" => crate::auth::route(&state, req).await,
            p if p.starts_with("/api/settings/") => crate::settings::route(&state, req).await,
            _ => text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n"),
        };
    }
    let mut response = match path.as_str() {
        "/healthz" => text(StatusCode::OK, "ok\n"),
        "/favicon.ico" => crate::ui::pwa::route(&state, "/icon.svg").expect("favicon asset"),
        "/login" => {
            let mut response = full(StatusCode::SEE_OTHER, "");
            set(&mut response, hyper::header::LOCATION, "/app/login");
            set(&mut response, hyper::header::CACHE_CONTROL, "no-store");
            response
        }
        "/api/stream" => sse(
            state,
            host,
            session.as_ref().expect("viewer gate").hash.clone(),
        ),
        "/api/state" => json(&crate::render::state_json(&state)),
        "/api/login" | "/api/logout" => crate::auth::route(&state, req).await,
        "/static/app.css" => asset(req.uri().query(), APP_CSS, CSS, APP_CSS_V),
        "/static/app.js" => asset(req.uri().query(), APP_JS, JS, APP_JS_V),
        "/static/logs.css" => asset(req.uri().query(), LOGS_CSS, CSS, LOGS_CSS_V),
        "/static/logs.js" => asset(req.uri().query(), LOGS_JS, JS, LOGS_JS_V),
        "/static/settings.css" => asset(req.uri().query(), SETTINGS_CSS, CSS, SETTINGS_CSS_V),
        "/static/settings.js" => asset(req.uri().query(), SETTINGS_JS, JS, SETTINGS_JS_V),
        "/custom.css" => {
            let css =
                std::fs::read_to_string(state.env.data_dir.join("custom.css")).unwrap_or_default();
            let mut r = full(StatusCode::OK, css);
            set(&mut r, CONTENT_TYPE, "text/css; charset=utf-8");
            set(&mut r, CACHE_CONTROL, "no-cache");
            r
        }
        p if p.starts_with("/icons/di/") => {
            let letter = req
                .uri()
                .query()
                .and_then(|q| q.strip_prefix("l="))
                .and_then(|l| l.chars().next());
            crate::ui::icons::dashboard_icon(&state, &p[10..], letter).await
        }
        p if p.starts_with("/icons/") => crate::ui::icon_response(&state, &p[7..]),
        p if p.starts_with("/api/logs/") => {
            crate::logs::route(&state, &req, &session.as_ref().expect("viewer gate").hash).await
        }
        p if p.starts_with("/api/containers/") => {
            crate::ui::pages::container::api(&state, &req).await
        }
        p if crate::ui::pwa::route(&state, p).is_some() => {
            crate::ui::pwa::route(&state, p).unwrap()
        }
        p if p.starts_with("/api/history/") || p == "/api/events" => {
            crate::db::api::route(&state, &req).await
        }
        p if p.starts_with("/api/settings/") => crate::settings::route(&state, req).await,
        // The React client (docs/plans/react-port.md): its JSON API, the
        // embedded build under /assets/ and the app document under /app.
        p if p.starts_with("/assets/") || p == "/app" || p.starts_with("/app/") => {
            crate::http::assets::route(&req)
        }
        _ => crate::ui::routes::route(&state, &req, &host).await,
    };
    if session.is_some() {
        set(&mut response, CACHE_CONTROL, "no-store");
    }
    response
}

pub fn full(status: StatusCode, body: impl Into<Bytes>) -> Response<Body> {
    let mut r = Response::new(Full::new(body.into()).boxed_unsync());
    *r.status_mut() = status;
    r
}

pub fn set(r: &mut Response<Body>, k: hyper::header::HeaderName, v: &str) {
    if let Ok(v) = HeaderValue::from_str(v) {
        r.headers_mut().insert(k, v);
    }
}

pub fn text(status: StatusCode, s: &'static str) -> Response<Body> {
    let mut r = full(status, s);
    set(&mut r, CONTENT_TYPE, "text/plain; charset=utf-8");
    r
}

pub fn html(s: String) -> Response<Body> {
    let mut r = full(StatusCode::OK, s);
    set(&mut r, CONTENT_TYPE, "text/html; charset=utf-8");
    set(&mut r, CACHE_CONTROL, "no-cache");
    r
}

pub fn json(v: &serde_json::Value) -> Response<Body> {
    let mut r = full(
        StatusCode::OK,
        serde_json::to_vec_pretty(v).unwrap_or_default(),
    );
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-cache");
    r
}

const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";

/// A compiled-in asset. Cached for a year (`immutable`) only when the URL
/// asks for exactly this content (`?v=<hash of the bytes>`); any other or no
/// `v` is `no-cache`, so a stale page never pins a wrong file.
fn asset(query: Option<&str>, body: &'static str, ctype: &str, v: &str) -> Response<Body> {
    let mut r = full(StatusCode::OK, body);
    set(&mut r, CONTENT_TYPE, ctype);
    set(&mut r, CACHE_CONTROL, asset_cache(query, v));
    r
}

fn asset_cache(query: Option<&str>, v: &str) -> &'static str {
    let asked = query
        .unwrap_or("")
        .split('&')
        .find_map(|kv| kv.strip_prefix("v="));
    if asked == Some(v) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// SSE body backed by a channel; holding the viewer guard for its lifetime.
struct SseBody {
    rx: mpsc::Receiver<Bytes>,
    _guard: ViewerGuard,
}

impl hyper::body::Body for SseBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.rx.poll_recv(cx).map(|o| o.map(|b| Ok(Frame::data(b))))
    }
}

/// Encode one SSE event; multi-line data becomes several `data:` lines.
pub fn sse_event(event: &str, data: &str) -> Bytes {
    let mut s = String::with_capacity(data.len() + 32);
    s.push_str("event: ");
    s.push_str(event);
    s.push('\n');
    for line in data.split('\n') {
        s.push_str("data: ");
        s.push_str(line);
        s.push('\n');
    }
    s.push('\n');
    Bytes::from(s)
}

fn sse(state: Arc<AppState>, host: String, session_hash: String) -> Response<Body> {
    let (tx, rx) = mpsc::channel::<Bytes>(16);
    let mut sub = state.events.subscribe();
    let guard = state.viewer_guard();
    let st = state.clone();
    tokio::spawn(async move {
        if tx.send(sse_event("hello", st.build.sha)).await.is_err() {
            return;
        }
        let mut ping = tokio::time::interval(Duration::from_secs(25));
        ping.tick().await;
        loop {
            tokio::select! {
                m = sub.recv() => {
                    if !st.db.session_is_valid(&session_hash, crate::auth::now_s()) { break; }
                    let id = match m {
                        Ok(id) => id,
                        Err(broadcast::error::RecvError::Lagged(_)) => Arc::from("reload"),
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    let msg = if &*id == "reload" {
                        sse_event("reload", "")
                    } else if let Some(html) = crate::render::fragment(&st, &id, &host) {
                        sse_event("frag", &format!("{id}\n{html}"))
                    } else {
                        continue;
                    };
                    if tx.send(msg).await.is_err() { break; }
                }
                _ = ping.tick() => {
                    if !st.db.session_is_valid(&session_hash, crate::auth::now_s()) { break; }
                    if tx.send(Bytes::from_static(b": ping\n\n")).await.is_err() { break; }
                }
                _ = tx.closed() => break,
            }
        }
    });
    let mut r = Response::new(SseBody { rx, _guard: guard }.boxed_unsync());
    set(&mut r, CONTENT_TYPE, "text/event-stream");
    set(&mut r, CACHE_CONTROL, "no-cache");
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> crate::state::Env {
        crate::state::Env {
            listen: vec![],
            data_dir: "/data".into(),
            config_path: "/data/config.toml".into(),
            docker_socket: String::new(),
            host_proc: "/proc".into(),
            host_sys: "/sys".into(),
            hostname: "homelab".into(),
            allowed_hosts: vec!["*.tail1234.ts.net".into()],
        }
    }

    #[test]
    fn host_allowlist() {
        let e = env();
        assert!(host_allowed("100.64.0.10", &e));
        assert!(host_allowed("::1", &e));
        assert!(host_allowed("localhost", &e));
        assert!(host_allowed("Homelab", &e));
        assert!(host_allowed("homelab.tail1234.ts.net", &e));
        assert!(!host_allowed("evil.example.com", &e));
        assert!(!host_allowed("homelab.evil.com", &e));
    }

    #[test]
    fn sse_multiline() {
        let b = sse_event("frag", "a\nb");
        assert_eq!(&b[..], b"event: frag\ndata: a\ndata: b\n\n");
    }

    /// GET `path` from a server on a real socket: (status, head, body).
    async fn get(path: &str) -> (u16, String, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let st = crate::ui::testutil::state_with(crate::ui::testutil::sample(), "");
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(serve_listener(st, l));
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let r = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        s.write_all(r.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8_lossy(&out).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, head.to_ascii_lowercase(), body.to_string())
    }

    /// The component script is the first part of `/static/app.js` (one
    /// cached request, no new one), before the app script.
    #[tokio::test]
    async fn app_js_serves_the_component_layer_then_the_app() {
        let (status, head, body) = get(&format!("/static/app.js?v={APP_JS_V}")).await;
        assert_eq!(status, 200);
        assert!(head.contains("content-type: text/javascript"), "{head}");
        assert!(head.contains("immutable"), "{head}");
        assert!(body.contains(APP_JS), "the whole bundle, unchunked");
        let hb = body
            .find("const HB = (window.HB = {});")
            .expect("window.HB");
        let app = body
            .find("// hobnail client: theme, sidebar")
            .expect("app.js");
        assert!(hb < app, "ui.js comes first so app.js can use HB");
        for api in [
            "HB.open = ",
            "HB.close = ",
            "HB.validate = ",
            "HB.toast = ",
            "HB.confirm = ",
            "HB.flash = ",
            "HB.remove = ",
        ] {
            assert!(body.contains(api), "{api}");
        }
        // Not built (owner decision Y13): no reorder dialog code is shipped.
        assert!(!body.contains("data-move"), "reorder script shipped");
    }

    /// The offline page is gone (NAV-70): no route, no worker, and nothing
    /// (manifest, page head) points at it.
    #[tokio::test]
    async fn no_offline_page_and_no_service_worker() {
        let (status, _, body) = get("/offline").await;
        assert_eq!(status, 404);
        assert!(body.contains("Page not found"), "the shell's 404");
        let (status, head, manifest) = get("/manifest.webmanifest").await;
        assert_eq!(status, 200);
        assert!(head.contains("application/manifest+json"), "{head}");
        assert!(!manifest.contains("offline"), "{manifest}");
        assert!(!manifest.contains("serviceworker"));
        let (_, _, page) = get("/").await;
        assert!(page.contains("rel=\"manifest\""));
        assert!(!page.contains("/offline") && !page.contains("serviceWorker"));
        assert!(!APP_JS.contains("serviceWorker"));
    }

    /// The page CSS (tokens, then components, then the page parts) is one
    /// cached file, `/static/app.css?v=<hash of its bytes>`, linked in every page's head
    /// before `custom.css` (which loads last); nothing is inlined.
    #[tokio::test]
    async fn every_page_links_tokens_and_components() {
        let (status, head, css) = get(&format!("/static/app.css?v={APP_CSS_V}")).await;
        assert_eq!(status, 200);
        assert!(head.contains("content-type: text/css"), "{head}");
        assert!(head.contains("max-age=31536000, immutable"), "{head}");
        assert_eq!(css, APP_CSS);
        let tokens = css.find("\n:root,[data-theme=dark]{").expect("dark tokens");
        let comp = css.find("dialog.dlg{").expect("dialog");
        let cards = css.find(".tbl{").expect("cards.css");
        assert!(tokens < comp && comp < cards, "tokens, components, pages");
        for sel in [
            ".btn.primary{",
            ".fld-err{",
            ".in{",
            ".note{",
            "dialog.dlg::backdrop{",
            ".toasts{",
            ".toast{",
            ".menu{",
            ".mi{",
            ".tab{",
            ".empty-ic{",
            ".navbar{",
            ".sw{",
            ".choice{",
        ] {
            assert!(css.contains(sel), "{sel}");
        }
        assert!(
            !css.contains(".reorder"),
            "reorder CSS shipped (Y13: not built)"
        );
        let (status, _, html) = get("/").await;
        assert_eq!(status, 200);
        let head = &html[..html.find("</head>").unwrap()];
        assert!(!html.contains("<style>"), "nothing inlined");
        assert_eq!(
            head.matches(r#"<link rel="stylesheet" href="/static/app.css?v="#)
                .count(),
            1
        );
        assert_eq!(
            html.matches("<script src=").count(),
            1,
            "one script request"
        );
    }

    /// Asset URLs carry a hash of the bytes; only that exact `v` is cached
    /// for good. A wrong or missing `v` must be revalidated (`no-cache`).
    #[tokio::test]
    async fn assets_are_immutable_only_for_their_own_hash() {
        assert_eq!(APP_CSS_V.len(), 16);
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(APP_CSS_V, APP_JS_V);
        for (path, v) in [
            ("/static/app.css", APP_CSS_V),
            ("/static/app.js", APP_JS_V),
            ("/static/logs.css", LOGS_CSS_V),
            ("/static/logs.js", LOGS_JS_V),
            ("/static/settings.css", SETTINGS_CSS_V),
            ("/static/settings.js", SETTINGS_JS_V),
        ] {
            let (st, head, _) = get(&format!("{path}?v={v}")).await;
            assert_eq!(st, 200);
            assert!(
                head.contains("cache-control: public, max-age=31536000, immutable"),
                "{path}: {head}"
            );
            for q in ["", "?v=dev", "?v=0000000000000000", &format!("?x=1&v={v}x")] {
                let (_, head, _) = get(&format!("{path}{q}")).await;
                assert!(
                    head.contains("cache-control: no-cache"),
                    "{path}{q}: {head}"
                );
                assert!(!head.contains("immutable"), "{path}{q}");
            }
        }
        let (_, _, html) = get("/").await;
        assert!(html.contains(&format!("/static/app.css?v={APP_CSS_V}\"")));
        assert!(html.contains(&format!("/static/app.js?v={APP_JS_V}\"")));
    }
}
