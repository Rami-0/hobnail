//! `/api/v1`: the JSON API of the React client (docs/plans/react-port.md
//! 4.2). Every route is a row of [`access::ROUTES`], whose access is checked
//! before the handler runs; errors use the shape in [`error`]. Writes reach
//! [`route`] only after `auth::require_write` (in `http::server::route`).
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Request, Response, StatusCode};
use serde::Serialize;

use crate::http::server::{full, set, Body};
use crate::state::AppState;

pub mod access;
pub mod error;
pub mod session;
#[cfg(test)]
mod types_tests;

use access::{Handler, Lookup};
use error::ApiError;

/// Hash of the build inputs (set by the image build; `dev` otherwise).
pub const CONTENT_HASH: &str = env!("RH_CONTENT_HASH");

pub async fn route(state: &Arc<AppState>, req: Request<Incoming>) -> Response<Body> {
    let m = match access::lookup(req.method(), req.uri().path()) {
        Lookup::Found(m) => m,
        Lookup::NotFound => return ApiError::not_found().response(),
        Lookup::MethodNotAllowed(allow) => return ApiError::method_not_allowed_response(&allow),
    };
    if let Err(e) = access::check(state, req.headers(), m.route.access) {
        return e.response();
    }
    match m.route.handler {
        Handler::Session => session::get(state, req.headers()),
    }
}

/// A JSON response that no cache stores (every `/api/v1` answer so far).
pub fn json(status: StatusCode, v: &impl Serialize) -> Response<Body> {
    let mut r = full(status, serde_json::to_vec(v).unwrap_or_default());
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-store");
    r
}
