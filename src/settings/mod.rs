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
//! The central HTTP router requires an Admin session for reads and writes.
//! Writes also require a CSRF token and a same-origin Origin or Referer.
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
pub(crate) mod store;
#[cfg(test)]
mod tests;

use bytes::Bytes;
use hyper::{Method, Request, Response};
use std::sync::Arc;

use crate::http::server::Body;

use crate::state::AppState;

/// Recognize settings reads before applying the Admin gate.
pub(crate) fn handles_read(path: &str) -> bool {
    let Some(segs) = segments(path) else {
        return false;
    };
    let segs: Vec<_> = segs.iter().map(String::as_str).collect();
    matches!(
        segs.as_slice(),
        ["general"
            | "containers"
            | "bookmarks"
            | "widgets"
            | "notifications"
            | "appearance"
            | "history"
            | "config"
            | "revisions"
            | "security"]
            | ["revision", _]
            | ["revision", _, "diff"]
            | ["history", "export"]
    )
}

fn segments(path: &str) -> Option<Vec<String>> {
    Some(
        path.strip_prefix("/api/settings/")?
            .split('/')
            .filter(|s| !s.is_empty())
            .map(percent_decode)
            .collect(),
    )
}

/// Dispatch a settings request after the shared access check.
pub(crate) async fn route(
    state: &Arc<AppState>,
    req: Request<hyper::body::Incoming>,
    session: crate::auth::Session,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let read = parts.method == Method::GET || parts.method == Method::HEAD;
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
        use crate::http::conn::{collect_body, BodyDeadline, BodyError};
        let deadline = parts.extensions.get::<BodyDeadline>();
        match collect_body(deadline, body, api::MAX_BODY).await {
            Ok(bytes) => bytes,
            Err(BodyError::TimedOut) => {
                return api::error(store::ApiError::new(
                    hyper::StatusCode::REQUEST_TIMEOUT,
                    "the request body did not arrive in time",
                ))
            }
            Err(BodyError::TooLarge) => {
                return api::error(store::ApiError::too_large(
                    "request body is limited to 256 KiB (or could not be read)",
                ))
            }
        }
    };
    let segs = segments(parts.uri.path()).expect("settings path");
    api::handle(
        state,
        api::Call {
            method: parts.method,
            segs,
            session,
            body,
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
