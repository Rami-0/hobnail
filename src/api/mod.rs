//! The JSON API of the React client. Routes reach this dispatcher after the shared HTTP gate.
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Request, Response, StatusCode};
use serde::Serialize;

use crate::auth::{CredentialAction, Session};
use crate::http::server::{full, set, Body};
use crate::state::AppState;

pub mod access;
mod accounts;
pub mod bootstrap;
pub mod containers;
pub mod error;
pub mod session;
pub mod stream;
pub mod strips;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod types_tests;

use access::{Handler, Lookup};
use error::ApiError;

/// Hash of the build inputs (set by the image build; `dev` otherwise).
pub const CONTENT_HASH: &str = env!("RH_CONTENT_HASH");

pub(crate) async fn route(
    state: &Arc<AppState>,
    req: Request<Incoming>,
    lookup: Lookup,
    principal: Option<Session>,
) -> Response<Body> {
    let m = match lookup {
        Lookup::Found(m) => m,
        Lookup::NotFound => return ApiError::not_found().response(),
        Lookup::MethodNotAllowed(allow) => return ApiError::method_not_allowed_response(&allow),
    };
    let query = req.uri().query().unwrap_or("");
    match m.route.handler {
        Handler::Session => session::get(state, req.headers()),
        Handler::Bootstrap => bootstrap::get(state, principal.expect("viewer gate").account.role),
        Handler::Onboard => {
            crate::auth::credential_route(state, req, CredentialAction::Onboard).await
        }
        Handler::InviteAccept => {
            crate::auth::credential_route(state, req, CredentialAction::Accept).await
        }
        Handler::InviteValidate => accounts::validate_invite(state, query),
        Handler::Invites
        | Handler::InviteCreate
        | Handler::InviteRevoke
        | Handler::Users
        | Handler::UserRole
        | Handler::UserDisabled
        | Handler::UserScopes
        | Handler::UserSignOut
        | Handler::UserDelete
        | Handler::Password
        | Handler::Sessions
        | Handler::SessionRevoke
        | Handler::SessionsRevoke => {
            accounts::route(state, req, m, principal.expect("account gate")).await
        }
        Handler::Containers => containers::list(state).await,
        Handler::Container => {
            let name = m.param("name").unwrap_or_default();
            let admin =
                principal.expect("viewer gate").account.role >= crate::db::accounts::Role::Admin;
            containers::get(state, name, query, admin).await
        }
        Handler::Strips => strips::get(state).await,
        Handler::Stream => stream::get(
            state,
            query,
            req.method() == hyper::Method::HEAD,
            principal.expect("viewer gate").hash,
        ),
    }
}

/// A JSON response that no cache stores (every `/api/v1` answer).
pub fn json(status: StatusCode, v: &impl Serialize) -> Response<Body> {
    let mut r = full(status, serde_json::to_vec(v).unwrap_or_default());
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-store");
    r
}
