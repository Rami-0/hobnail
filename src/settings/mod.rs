//! Settings inside the app: lossless `toml_edit` patches, validation, the
//! `command`-widget guard, atomic writes, revisions, and the settings JSON
//! API (`/api/settings/*`). See docs/DESIGN.md sections 5 and 6.
//!
//! Modules: `doc` (patch engine, hashing, atomic write), `guard` (command
//! widgets, secret files, secret-value warnings), `store` (the save
//! pipeline), `ops` (per-tab operations and request shapes), `diff` (line
//! diff), `api` (HTTP), `notify` ("Send test").
//!
//! # Access
//!
//! Every endpoint needs a signed-in user. Writes already passed
//! `auth::require_write` (session + `X-RH-CSRF` + same Origin) in
//! `http::server::route`; `route` checks it again (defense in depth). GETs
//! are not covered by the central guard, so `route` answers 401 without a
//! session.
//!
//! # Conventions
//!
//! JSON in, JSON out. Request bodies are at most 256 KiB, unknown fields are
//! refused (400). Every config write carries `base_hash`, the `hash` the
//! client got from a GET; if the file changed since, the answer is 409
//! `{"error":"config changed on disk; reload","stale":true}` and nothing is
//! written. A successful write answers
//! `{"ok":true,"hash":"<new hash>","revision":<id|null>,"note":"…","warnings":[…]}`
//! (`"unchanged":true` when the result is byte-identical). Errors are
//! `{"error":"…"}` with 400 (shape), 401 (no session), 403 (`"locked":true`:
//! command-widget or secret-file rule), 404, 409 (stale), 413 (too large),
//! 422 (`{"error","field","line","col"}`: the result would not load;
//! `"file_invalid":true` when the file on disk is already broken).

mod api;
mod diff;
mod doc;
mod guard;
mod notify;
mod ops;
mod store;
#[cfg(test)]
mod tests;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{Method, Request, Response, StatusCode};
use std::sync::Arc;

use crate::http::server::Body;
use crate::state::AppState;

/// Everything under `/api/settings/`. Generic over the body so tests can
/// call it with a `Full<Bytes>`; the server passes `Incoming`.
pub async fn route<B>(state: &Arc<AppState>, req: Request<B>) -> Response<Body>
where
    B: hyper::body::Body<Data = Bytes>,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (parts, body) = req.into_parts();
    let read = parts.method == Method::GET || parts.method == Method::HEAD;
    let user = if read {
        match crate::auth::current_user(state, &parts.headers) {
            Some(u) => u,
            None => {
                return api::error(store::ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "sign in required",
                ))
            }
        }
    } else {
        match crate::auth::require_write(state, &parts.headers) {
            Ok(u) => u,
            Err(d) => return d.response(),
        }
    };
    let declared = parts
        .headers
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > api::MAX_BODY) {
        return api::error(store::ApiError::too_large(
            "request body is limited to 256 KiB",
        ));
    }
    let body = if read {
        Bytes::new()
    } else {
        match http_body_util::Limited::new(body, api::MAX_BODY)
            .collect()
            .await
        {
            Ok(c) => c.to_bytes(),
            Err(_) => {
                return api::error(store::ApiError::too_large(
                    "request body is limited to 256 KiB (or could not be read)",
                ))
            }
        }
    };
    let path = parts.uri.path();
    let rest = path.strip_prefix("/api/settings").unwrap_or(path);
    let segs: Vec<String> = rest
        .split('/')
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect();
    api::handle(
        state,
        api::Call {
            method: parts.method,
            segs,
            headers: parts.headers,
            body,
            user,
        },
    )
    .await
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
