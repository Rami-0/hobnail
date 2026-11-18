//! `GET /api/v1/session` (Public, `no-store`): who is signed in, the CSRF
//! token for writes, and what the client needs before its first render.
//! `user` and `csrf` are null unless the request carries a valid session
//! cookie. Uses only the public contract of `auth`.

use hyper::{HeaderMap, Response, StatusCode};
use serde::Serialize;

use crate::db::accounts::Role;
use crate::http::server::Body;
use crate::state::AppState;

/// The `/api/v1` version the client is talking to.
pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Session {
    pub user: Option<SessionUser>,
    /// The `X-RH-CSRF` token for writes; null when not signed in.
    pub csrf: Option<String>,
    pub owner_exists: bool,
    pub onboarding_locked: bool,
    pub build: BuildInfoView,
    pub api: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionUser {
    pub name: String,
    pub role: Role,
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
    let (user, csrf) = crate::auth::request_session(state, headers).map_or((None, None), |s| {
        (
            Some(SessionUser {
                name: s.account.name.clone(),
                role: s.account.role,
            }),
            Some(s.csrf()),
        )
    });
    let b = &state.build;
    Session {
        user,
        csrf,
        owner_exists: state.db.owner_exists(),
        onboarding_locked: state.db.onboarding_locked(),
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
