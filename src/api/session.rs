//! `GET /api/v1/session` (Public, `no-store`): who is signed in, the CSRF
//! token for writes, and what the client needs before its first render.
//! `user` and `csrf` are null unless the request carries a valid session
//! cookie. Uses only the public contract of `auth`.

use hyper::{HeaderMap, Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;
use crate::state::AppState;

/// The `/api/v1` version the client is talking to.
pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Session {
    /// The signed-in user (`"admin"`), or null.
    pub user: Option<String>,
    /// The `X-RH-CSRF` token for writes; null when not signed in.
    pub csrf: Option<String>,
    /// False until `hobnail set-password` has run (everything is read-only).
    pub password_set: bool,
    /// `[logs] require_login`: logs need a session.
    pub logs_require_login: bool,
    pub build: BuildInfoView,
    pub api: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BuildInfoView {
    pub version: String,
    /// Git short SHA (`-dirty` when built from a modified tree).
    pub sha: String,
    /// UTC build time.
    pub time: String,
    /// Hash of the build inputs (`dev` outside the image build).
    pub content: String,
}

pub fn session(state: &AppState, headers: &HeaderMap) -> Session {
    let user = crate::auth::current_user(state, headers);
    let csrf = crate::auth::csrf_token(state, headers);
    // Both or neither (the session could end between the two reads).
    let (user, csrf) = match (user, csrf) {
        (Some(u), Some(c)) => (Some(u), Some(c)),
        _ => (None, None),
    };
    let b = &state.build;
    Session {
        user,
        csrf,
        password_set: crate::auth::password_is_set(state),
        logs_require_login: state.config.current().logs.require_login,
        build: BuildInfoView {
            version: b.version.to_string(),
            sha: b.sha.to_string(),
            time: b.time.to_string(),
            content: super::CONTENT_HASH.to_string(),
        },
        api: API_VERSION,
    }
}

pub fn get(state: &AppState, headers: &HeaderMap) -> Response<Body> {
    super::json(StatusCode::OK, &session(state, headers))
}
