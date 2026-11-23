//! `GET /api/v1/session` (Public, `no-store`): who is signed in, the CSRF
//! token for writes, and what the client needs before its first render.
//! `user` and `csrf` are null unless the request carries a valid session
//! cookie. Uses only the public contract of `auth`.

use hyper::{Request, Response, StatusCode};
use serde::Serialize;

use super::error::ApiError;
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
    pub database: bool,
    pub owner_exists: bool,
    pub onboarding_locked: bool,
    pub setup_pending: bool,
    pub board_tour_pending: bool,
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

fn session(state: &AppState, principal: Option<&crate::auth::Session>) -> Session {
    let (user, csrf) = principal.map_or((None, None), |s| {
        (
            Some(SessionUser {
                name: s.account.name.clone(),
                role: s.account.role,
            }),
            Some(s.csrf()),
        )
    });
    let (setup_pending, board_tour_pending) = principal
        .and_then(|session| {
            let account = &session.account;
            state
                .db
                .dashboard(|conn| -> Result<_, ApiError> {
                    let seen: bool = conn.query_row(
                        "SELECT board_tour_seen FROM users WHERE id=?1",
                        [account.id],
                        |row| row.get(0),
                    )?;
                    let setup =
                        account.role == Role::Owner && super::setup::pending(conn, account.id)?;
                    Ok((setup, !seen))
                })
                .ok()
        })
        .unwrap_or_default();
    let b = &state.build;
    Session {
        user,
        csrf,
        database: state.db.is_enabled(),
        owner_exists: state.db.owner_exists(),
        onboarding_locked: state.db.onboarding_locked(),
        setup_pending,
        board_tour_pending,
        build: BuildInfoView {
            version: b.version.to_string(),
            sha: b.sha.to_string(),
            time: b.time.to_string(),
            content: super::CONTENT_HASH.to_string(),
        },
        api: API_VERSION,
    }
}

pub fn get<B>(state: &AppState, req: &Request<B>) -> Response<Body> {
    let principal = crate::auth::request_session(state, req.headers());
    let mut response = super::json(StatusCode::OK, &session(state, principal.as_ref()));
    if let Some(principal) = principal {
        let secure = crate::http::proxy::is_https(req, &state.env);
        crate::auth::refresh_cookie(state, &principal, secure, &mut response);
    }
    response
}

pub(super) fn dismiss_board_tour(state: &AppState, id: i64) -> Result<Response<Body>, ApiError> {
    state.db.dashboard::<_, rusqlite::Error>(|conn| {
        conn.execute(
            "UPDATE users SET board_tour_seen=1 WHERE id=?1 AND board_tour_seen=0",
            [id],
        )
    })?;
    Ok(super::no_content())
}
