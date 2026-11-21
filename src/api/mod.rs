//! The JSON API of the React client. Routes reach this dispatcher after the shared HTTP gate.
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Request, Response, StatusCode};
use serde::Serialize;

use crate::auth::{CredentialAction, Session};
use crate::http::server::{full, set, Body};
use crate::state::AppState;

pub(crate) mod access;
mod accounts;
mod alarm;
pub(crate) mod alerts;
mod boards;
mod bookmarks;
pub mod bootstrap;
pub mod container_facts;
mod container_tabs;
pub mod containers;
mod dashboard_settings;
pub mod error;
mod history;
mod history_window;
mod host;
pub(crate) mod integrations;
mod logs;
mod overview;
pub mod session;
mod setup;
pub mod stream;
pub mod strips;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod types_tests;
mod widgets;

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
        Handler::Integrations
        | Handler::IntegrationCreate
        | Handler::Integration
        | Handler::IntegrationUpdate
        | Handler::IntegrationDelete
        | Handler::IntegrationSetSecret
        | Handler::IntegrationClearSecret
        | Handler::IntegrationTest => {
            integrations::route(state, req, m, &principal.expect("integration gate").account)
                .await
                .unwrap_or_else(ApiError::response)
        }
        Handler::Boards
        | Handler::BoardCreate
        | Handler::Board
        | Handler::BoardSave
        | Handler::BoardUpdate
        | Handler::BoardDelete
        | Handler::BoardSuggestions
        | Handler::BoardLinkTest
        | Handler::BoardDraft
        | Handler::BoardDraftSave
        | Handler::BoardDraftDelete => {
            boards::route(state, req, m, &principal.expect("board gate").account)
                .await
                .unwrap_or_else(ApiError::response)
        }
        Handler::Alarm => alarm::get(state, &principal.expect("viewer gate").account),
        Handler::Session => session::get(state, &req),
        Handler::BoardTourDismiss => {
            session::dismiss_board_tour(state, principal.expect("viewer gate").account.id)
                .unwrap_or_else(ApiError::response)
        }
        Handler::Bootstrap => bootstrap::get(state, &principal.expect("viewer gate").account),
        Handler::DashboardSettings => {
            dashboard_settings::route(state, req, &principal.expect("settings gate").account)
                .await
                .unwrap_or_else(ApiError::response)
        }
        Handler::DashboardIcon => {
            let name = m.param("name").unwrap_or_default();
            if !state.config.current().icons.remote || validate_icon(Some(name)).is_err() {
                return ApiError::not_found().response();
            }
            crate::http::icons::dashboard_icon(state, &format!("{name}.svg"), None).await
        }
        handler @ (Handler::SetupDraft
        | Handler::SetupSave
        | Handler::SetupDiscover
        | Handler::SetupTest
        | Handler::SetupCommit) => {
            setup::route(state, req, handler, &principal.expect("owner gate").account)
                .await
                .unwrap_or_else(ApiError::response)
        }
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
        Handler::LogsPicker => logs::get(state, principal.expect("viewer gate").account).await,
        Handler::Containers => {
            containers::list(
                state,
                principal
                    .expect("viewer gate")
                    .account
                    .container_visibility(state),
            )
            .await
        }
        handler @ (Handler::ContainerInspect
        | Handler::ContainerMetrics
        | Handler::ContainerEvents
        | Handler::ContainerEnv
        | Handler::ContainerTop) => {
            container_tabs::get(
                state,
                principal.expect("container gate").account,
                m.param("name").unwrap_or_default(),
                query,
                handler,
            )
            .await
        }
        Handler::Container => {
            let name = m.param("name").unwrap_or_default();
            let account = principal.expect("viewer gate").account;
            let visible = account.container_visibility(state);
            if !account.can_see_container(&visible, name) {
                return ApiError::not_found().response();
            }
            let admin = !account.is_scoped() && account.role >= crate::db::accounts::Role::Admin;
            containers::get(state, name, query, admin).await
        }
        Handler::Host | Handler::HostHistory => {
            host::get(
                state,
                query,
                principal.expect("viewer gate").account.role,
                m.route.handler == Handler::HostHistory,
            )
            .await
        }
        Handler::Overview => {
            overview::get(state, query, principal.expect("viewer gate").account.role).await
        }
        Handler::Alerts => alerts::get(state, query).await,
        Handler::Widgets => widgets::get(state),
        Handler::Bookmarks => bookmarks::get(state),
        Handler::History => {
            history::get(state, query, principal.expect("viewer gate").account.role).await
        }
        Handler::Strips => {
            strips::get(
                state,
                principal
                    .expect("viewer gate")
                    .account
                    .container_visibility(state),
            )
            .await
        }
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

async fn read_json<T: serde::de::DeserializeOwned>(
    req: Request<Incoming>,
    limit: usize,
) -> Result<T, ApiError> {
    use http_body_util::{BodyExt, Limited};
    let json = req
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(';').next())
        .is_some_and(|h| h.trim().eq_ignore_ascii_case("application/json"));
    if !json {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "send application/json",
        ));
    }
    let bytes = Limited::new(req.into_body(), limit)
        .collect()
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
                "request too large",
            )
        })?;
    serde_json::from_slice(&bytes.to_bytes())
        .map_err(|_| ApiError::bad_request("invalid JSON body", None))
}

impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "dashboard storage unavailable",
        )
    }
}

fn no_content() -> Response<Body> {
    let mut response = full(StatusCode::NO_CONTENT, "");
    set(&mut response, CACHE_CONTROL, "no-store");
    response
}

fn validate_label(value: &str, field: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "must be non-empty, at most 256 bytes, and contain no control characters",
            Some(field),
        ));
    }
    Ok(())
}

fn validate_icon(icon: Option<&str>) -> Result<(), ApiError> {
    if icon.is_some_and(|name| {
        name.is_empty()
            || name.len() > 96
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    }) {
        return Err(ApiError::bad_request(
            "use a dashboard-icons name",
            Some("icon"),
        ));
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod format_tests;
