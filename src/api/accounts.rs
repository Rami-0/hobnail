//! Owner account administration and self-service session management.
use std::sync::Arc;

use hyper::{body::Incoming, Request, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

use super::access::{Handler, Matched};
use super::error::ApiError;
use crate::auth::{self, now_s, sha256_hex, Session};
use crate::db::accounts::{
    legacy_hash_users, read_accounts, Account, AccountChange, AccountError, Invite, Role, Scopes,
};
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
    #[serde(deserialize_with = "Deserialize::deserialize")]
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

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ManagedUser {
    #[serde(flatten)]
    #[cfg_attr(test, ts(flatten))]
    account: Account,
    legacy_hash: bool,
    scope_targets: Option<ScopeTargets>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ScopeTargets {
    boards: Vec<ScopeTarget>,
    integrations: Vec<ScopeTarget>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ScopeTarget {
    id: String,
    name: String,
}

fn managed_users(conn: &rusqlite::Connection) -> Result<Vec<ManagedUser>, ApiError> {
    let mut boards = conn.prepare(
        "SELECT b.id,b.name FROM access_grants g JOIN boards b ON b.id=g.target_id
         WHERE g.kind='board' AND g.user_id=?1 ORDER BY b.name,b.id",
    )?;
    let mut integrations = conn.prepare(
        "SELECT i.id,i.name FROM access_grants g JOIN integrations i ON i.id=g.target_id
         WHERE g.kind='integration' AND g.user_id=?1 ORDER BY i.name,i.id",
    )?;
    let read = |row: &rusqlite::Row<'_>| {
        Ok(ScopeTarget {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    };
    let legacy = legacy_hash_users(conn)?;
    read_accounts(conn)?
        .into_iter()
        .map(|account| {
            let scope_targets = if account.scopes.is_some() {
                Some(ScopeTargets {
                    boards: boards
                        .query_map([account.id], read)?
                        .collect::<rusqlite::Result<_>>()?,
                    integrations: integrations
                        .query_map([account.id], read)?
                        .collect::<rusqlite::Result<_>>()?,
                })
            } else {
                None
            };
            Ok(ManagedUser {
                legacy_hash: legacy.contains(&account.name),
                account,
                scope_targets,
            })
        })
        .collect()
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
            AccountError::Protected | AccountError::OnboardingLocked => Self::new(
                StatusCode::FORBIDDEN,
                "protected_owner",
                "owner account is protected",
            ),
            AccountError::InvalidScopes => {
                Self::bad_request("select existing board and integration IDs", Some("scopes"))
            }
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
    super::read_json(req, MAX_BODY).await
}

fn validate_scopes(scopes: Option<&Scopes>) -> Result<(), ApiError> {
    if let Some(scopes) = scopes {
        for id in scopes.boards.iter().chain(&scopes.integrations) {
            if !super::valid_id(id) {
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
        Handler::Users => {
            return Ok(super::json(
                StatusCode::OK,
                &db.dashboard(|c| managed_users(c))?,
            ))
        }
        Handler::Password => {
            let PasswordRequest { current, new } = body(req).await?;
            auth::check_password_policy(&new).map_err(|e| ApiError::bad_request(e, Some("new")))?;
            let Some(permit) = state.credential_queue.acquire().await else {
                return Ok(auth::credential_busy(false));
            };
            let state = state.clone();
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
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
            if handler == Handler::UserScopes {
                state.changed("reload");
            }
        }
    }
    Ok(super::json(StatusCode::OK, &json!({"ok": true})))
}
