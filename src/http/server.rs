//! Hyper server for the embedded client and HTTP APIs.

use bytes::Bytes;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

use crate::state::AppState;

pub type Body = UnsyncBoxBody<Bytes, Infallible>;

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
                async move { Ok::<_, Infallible>(respond(st, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

/// Host part of the request's Host header (no port), for the allowlist.
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
pub struct Peer(pub SocketAddr);

async fn respond(state: Arc<AppState>, req: Request<Incoming>) -> Response<Body> {
    // Keep stream and download responses out of content negotiation entirely.
    if matches!(
        req.uri().path(),
        "/api/v1/stream" | "/api/logs/stream" | "/api/logs/download"
    ) {
        return route(state, req).await;
    }
    let accepts_gzip = super::assets::accepts_gzip(
        req.headers()
            .get_all(hyper::header::ACCEPT_ENCODING)
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    super::compression::negotiate(route(state, req).await, accepts_gzip).await
}

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
    let legacy_read = matches!(
        path.as_str(),
        "/api/history/system"
            | "/api/events"
            | "/api/logs/stream"
            | "/api/logs/download"
            | "/api/logs/levels"
            | "/api/logs/status"
            | "/custom.css"
    ) || path.starts_with("/api/history/container/")
        || path.starts_with("/icons/");
    // Only paths without a read handler may return before the session gate.
    // Keep handler recognition in sync with dispatch: retired URLs (including
    // `/`) stay 404 instead of exposing data or redirecting to sign-in.
    if read
        && !(v1
            || public_asset
            || legacy_read
            || path == "/healthz"
            || crate::settings::handles_read(&path))
    {
        return not_found(&path);
    }
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
        _ if path.starts_with("/api/settings/") => Access::Admin,
        _ => Access::Viewer,
    };
    let session = match access::check(&state, req.headers(), access, !read) {
        Ok(session) => session,
        Err(denied) => return denied.response(v1),
    };
    if let Some(session) = &session {
        if let Err(denied) = access::check_scope(
            &session.account,
            access,
            lookup.as_ref(),
            &path,
            req.uri().query().unwrap_or_default(),
        ) {
            return denied.response(v1);
        }
        if let Some(name) = path.strip_prefix("/api/history/container/") {
            let visible = session.account.container_visibility(&state);
            if visible.is_scoped()
                && !crate::db::api::percent_decode(name)
                    .is_some_and(|name| session.account.can_see_container(&visible, &name))
            {
                return crate::db::api::json_error(StatusCode::NOT_FOUND, "container not found");
            }
        }
    }
    let dashboard_icon = matches!(&lookup, Some(Lookup::Found(m))
        if m.route.handler == access::Handler::DashboardIcon);
    let mut response = dispatch(&state, req, &path, read, lookup, session.as_ref()).await;
    if let Some(session) = session {
        crate::auth::refresh_cookie(&state, &session, &mut response);
        if dashboard_icon && response.status().is_success() {
            set(&mut response, CACHE_CONTROL, "private, max-age=86400");
            set(&mut response, hyper::header::VARY, "Cookie");
        } else {
            set(&mut response, CACHE_CONTROL, "no-store");
        }
    }
    response
}

async fn dispatch(
    state: &Arc<AppState>,
    req: Request<Incoming>,
    path: &str,
    read: bool,
    lookup: Option<crate::api::access::Lookup>,
    session: Option<&crate::auth::Session>,
) -> Response<Body> {
    if let Some(lookup) = lookup {
        return crate::api::route(state, req, lookup, session.cloned()).await;
    }
    if path.starts_with("/api/settings/") {
        return crate::settings::route(state, req, session.expect("settings gate").clone()).await;
    }
    if !read {
        return match path {
            "/api/login" | "/api/logout" => crate::auth::route(state, req, session).await,
            _ => not_found(path),
        };
    }
    let admin = session.is_some_and(|s| s.account.role >= crate::db::accounts::Role::Admin);
    match path {
        "/healthz" => text(StatusCode::OK, "ok\n"),
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
            crate::http::icons::dashboard_icon(state, &p[10..], letter).await
        }
        p if p.starts_with("/icons/") => crate::http::icons::icon_response(state, &p[7..]),
        p if p.starts_with("/api/logs/") => {
            let session = session.expect("viewer gate");
            let visible = session.account.container_visibility(state);
            crate::logs::route(state, &req, &session.hash, &visible).await
        }
        p if p.starts_with("/api/history/") || p == "/api/events" => {
            crate::db::api::route(state, &req, admin).await
        }
        // The React client (docs/plans/react-port.md): its JSON API, the
        // embedded build under /assets/ and the app document under /app.
        p if p.starts_with("/assets/") || p == "/app" || p.starts_with("/app/") => {
            crate::http::assets::route(&req)
        }
        p => super::pwa::route(state, p).unwrap_or_else(|| not_found(p)),
    }
}

fn not_found(path: &str) -> Response<Body> {
    if path == "/" {
        let mut response = full(
            StatusCode::NOT_FOUND,
            r#"<!doctype html><html lang="en"><meta charset="utf-8"><title>Hobnail</title><h1>Hobnail</h1><p><a href="/app">Open Hobnail</a></p></html>"#,
        );
        set(&mut response, CONTENT_TYPE, "text/html; charset=utf-8");
        set(
            &mut response,
            hyper::header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        );
        return response;
    }
    if path == "/api" || path.starts_with("/api/") {
        return crate::db::api::json_error(StatusCode::NOT_FOUND, "not found");
    }
    text(StatusCode::NOT_FOUND, "not found\n")
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

pub fn json(v: &serde_json::Value) -> Response<Body> {
    let mut r = full(
        StatusCode::OK,
        serde_json::to_vec_pretty(v).unwrap_or_default(),
    );
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-cache");
    r
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
}
