//! Owner account administration and self-service session management.
use std::sync::Arc;

use http_body_util::{BodyExt, Limited};
use hyper::{body::Incoming, Request, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

use super::access::{Handler, Matched};
use super::error::ApiError;
use crate::auth::{self, now_s, sha256_hex, Session};
use crate::db::accounts::{AccountChange, AccountError, Invite, Role, Scopes};
use crate::http::server::Body;
use crate::state::AppState;

const INVITE_TTL: i64 = 7 * 86_400;
const MAX_BODY: usize = 8192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleRequest {
    role: Role,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteRequest {
    role: Role,
    scopes: Option<Scopes>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisabledRequest {
    disabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeRequest {
    scopes: Option<Scopes>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    current: String,
    new: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IssuedInvite {
    pub invite: Invite,
    pub token: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InviteValidation {
    pub valid: bool,
    pub role: Option<Role>,
    pub expires: Option<i64>,
}

pub fn validate_invite(state: &AppState, query: &str) -> Response<Body> {
    let invite = crate::db::api::parse_query(query)
        .and_then(|pairs| {
            pairs
                .into_iter()
                .find_map(|(key, value)| (key == "token").then_some(value))
        })
        .filter(|token| token.len() == 64 && token.bytes().all(|c| c.is_ascii_hexdigit()))
        .and_then(|token| {
            state
                .db
                .validate_invite(&sha256_hex(token.as_bytes()), now_s())
        });
    super::json(
        StatusCode::OK,
        &InviteValidation {
            valid: invite.is_some(),
            role: invite.as_ref().map(|i| i.role),
            expires: invite.map(|i| i.expires),
        },
    )
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Conflict => {
                Self::new(StatusCode::CONFLICT, "conflict", "account changed")
            }
            AccountError::Missing => Self::not_found(),
            AccountError::Protected => Self::new(
                StatusCode::FORBIDDEN,
                "protected_owner",
                "owner account is protected",
            ),
            AccountError::Invalid => Self::bad_request("invalid account operation", None),
            AccountError::Storage => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "account storage unavailable",
            ),
        }
    }
}

async fn body<T: DeserializeOwned>(req: Request<Incoming>) -> Result<T, ApiError> {
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
    let bytes = Limited::new(req.into_body(), MAX_BODY)
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

fn validate_scopes(scopes: Option<&Scopes>) -> Result<(), ApiError> {
    if let Some(scopes) = scopes {
        for id in scopes.boards.iter().chain(&scopes.integrations) {
            if id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(ApiError::bad_request(
                    "scope IDs must be generated board or integration IDs",
                    Some("scopes"),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) async fn route(
    state: &Arc<AppState>,
    req: Request<Incoming>,
    m: Matched,
    session: Session,
) -> Response<Body> {
    handle(state, req, m, session)
        .await
        .unwrap_or_else(ApiError::response)
}

async fn handle(
    state: &Arc<AppState>,
    req: Request<Incoming>,
    m: Matched,
    session: Session,
) -> Result<Response<Body>, ApiError> {
    let now = now_s();
    let db = &state.db;
    let user = &session.account;
    let id = m.param("id").unwrap_or_default();
    match m.route.handler {
        Handler::Invites => return Ok(super::json(StatusCode::OK, &db.invites()?)),
        Handler::InviteCreate => {
            let request: InviteRequest = body(req).await?;
            if request.role == Role::Owner {
                return Err(ApiError::bad_request(
                    "invite role must be admin or viewer",
                    Some("role"),
                ));
            }
            validate_scopes(request.scopes.as_ref())?;
            let token = auth::random_token().map_err(|_| AccountError::Storage)?;
            let invite = db.create_invite(
                &sha256_hex(token.as_bytes()),
                request.role,
                request.scopes.as_ref(),
                now,
                now + INVITE_TTL,
            )?;
            return Ok(super::json(
                StatusCode::CREATED,
                &IssuedInvite { invite, token },
            ));
        }
        Handler::InviteRevoke => db.revoke_invite(id, now)?,
        Handler::Users => return Ok(super::json(StatusCode::OK, &db.accounts()?)),
        Handler::Password => {
            let PasswordRequest { current, new } = body(req).await?;
            auth::check_password_policy(&new).map_err(|e| ApiError::bad_request(e, Some("new")))?;
            let state = state.clone();
            tokio::task::spawn_blocking(move || {
                auth::change_password_for(&state, &session, &current, &new)
            })
            .await
            .map_err(|_| AccountError::Storage)?
            .map_err(|message| ApiError::bad_request(message, None))?;
        }
        Handler::Sessions => {
            return Ok(super::json(
                StatusCode::OK,
                &db.other_sessions(user.id, &session.hash, now)?,
            ))
        }
        Handler::SessionsRevoke => db.sign_out_other_sessions(user.id, &session.hash, None)?,
        Handler::SessionRevoke => db.sign_out_other_sessions(user.id, &session.hash, Some(id))?,
        handler => {
            let id = id.parse::<i64>().map_err(|_| ApiError::not_found())?;
            let change = match handler {
                Handler::UserRole => AccountChange::Role(body::<RoleRequest>(req).await?.role),
                Handler::UserDisabled => {
                    AccountChange::Disabled(body::<DisabledRequest>(req).await?.disabled)
                }
                Handler::UserScopes => {
                    let scopes = body::<ScopeRequest>(req).await?.scopes;
                    validate_scopes(scopes.as_ref())?;
                    AccountChange::Scopes(scopes)
                }
                Handler::UserSignOut => AccountChange::SignOut,
                Handler::UserDelete => AccountChange::Delete,
                _ => unreachable!("account route"),
            };
            db.change_account(id, change, now)?;
        }
    }
    Ok(super::json(StatusCode::OK, &json!({"ok": true})))
}
