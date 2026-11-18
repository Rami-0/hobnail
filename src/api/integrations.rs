//! Generic HTTP integrations. Hobnail trusts public certificate authorities only.
//! Private CAs and self-signed certificates fail verification. There is no bypass.

use std::time::Instant;

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    access::{Handler, Matched, Principal},
    error::ApiError,
};
use crate::{
    db::accounts::Role,
    http::{client, server::Body},
    state::AppState,
};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum IntegrationKind {
    Http,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationInput {
    name: String,
    kind: IntegrationKind,
    base_url: String,
    #[cfg_attr(test, ts(optional))]
    icon: Option<String>,
    enabled: bool,
    #[cfg_attr(test, ts(optional))]
    secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationSecretInput {
    secret: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationView {
    pub id: String,
    pub name: String,
    kind: IntegrationKind,
    base_url: String,
    has_secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    admin: Option<IntegrationDetails>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationDetails {
    icon: Option<String>,
    enabled: bool,
    secret_origin: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Config {
    base_url: String,
    icon: Option<String>,
    enabled: bool,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationVerdict {
    ok: bool,
    class: VerdictClass,
    message: &'static str,
    latency_ms: u64,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) enum VerdictClass {
    Success,
    Dns,
    Connect,
    Tls,
    Timeout,
    HttpStatus,
    Auth,
    TooLarge,
    Blocked,
    Protocol,
}

const SELECT: &str = "SELECT id, name, kind, origin, config, secret_origin FROM integrations";

fn read(row: &rusqlite::Row<'_>, admin: bool) -> rusqlite::Result<IntegrationView> {
    let origin: String = row.get(3)?;
    let config: String = row.get(4)?;
    let config = serde_json::from_str::<Config>(&config).unwrap_or(Config {
        base_url: origin,
        icon: None,
        enabled: true,
    });
    let secret_origin: Option<String> = row.get(5)?;
    let kind = match row.get::<_, String>(2)?.as_str() {
        "http" => IntegrationKind::Http,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(IntegrationView {
        id: row.get(0)?,
        name: row.get(1)?,
        kind,
        base_url: config.base_url,
        has_secret: secret_origin.is_some(),
        admin: admin.then_some(IntegrationDetails {
            icon: config.icon,
            enabled: config.enabled,
            secret_origin,
        }),
    })
}

pub(super) fn list(conn: &Connection, user: &Principal) -> Result<Vec<IntegrationView>, ApiError> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY name, id"))?;
    let mut rows = stmt.query([])?;
    let mut visible = Vec::new();
    while let Some(row) = rows.next()? {
        if user.can_see_integration(
            row.get_ref(0)?
                .as_str()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        ) {
            visible.push(read(row, user.role >= Role::Admin)?);
        }
    }
    Ok(visible)
}

fn get(conn: &Connection, id: &str, admin: bool) -> Result<IntegrationView, ApiError> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], |r| read(r, admin))
        .optional()?
        .ok_or_else(ApiError::not_found)
}

fn secret_value(value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.len() > 4096 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ApiError::bad_request("enter a non-empty bearer token of up to 4096 printable ASCII characters without spaces", Some("secret")));
    }
    Ok(())
}

fn save(
    conn: &Connection,
    id: Option<&str>,
    input: IntegrationInput,
) -> Result<IntegrationView, ApiError> {
    super::validate_label(&input.name, "name")?;
    super::validate_icon(input.icon.as_deref())?;
    let origin = client::origin(&input.base_url).ok_or_else(|| {
        ApiError::bad_request(
            "enter an absolute HTTP or HTTPS URL without user information, query, or fragment",
            Some("base_url"),
        )
    })?;
    let kind = match input.kind {
        IntegrationKind::Http => "http",
    };
    if let Some(secret) = &input.secret {
        secret_value(secret)?;
    }
    let config = serde_json::to_string(&Config {
        base_url: input.base_url,
        icon: input.icon,
        enabled: input.enabled,
    })
    .expect("serializable integration config");
    let binding = input.secret.as_ref().map(|_| &origin);
    let id = if let Some(id) = id {
        let previous = get(conn, id, true)?;
        if previous
            .admin
            .and_then(|a| a.secret_origin)
            .is_some_and(|bound| bound != origin)
            && input.secret.is_none()
        {
            return Err(ApiError::bad_request(
                "enter a new secret for the changed origin, or clear the stored secret first",
                Some("secret"),
            ));
        }
        conn.execute(
            "UPDATE integrations SET name=?2, kind=?3, origin=?4, config=?5,
             secret=coalesce(?6,secret), secret_origin=coalesce(?7,secret_origin) WHERE id=?1",
            params![id, input.name, kind, origin, config, input.secret, binding],
        )?;
        id.to_string()
    } else {
        conn.query_row(
            "INSERT INTO integrations (name,kind,origin,config,secret,secret_origin)
             VALUES (?1,?2,?3,?4,?5,?6) RETURNING id",
            params![input.name, kind, origin, config, input.secret, binding],
            |r| r.get(0),
        )?
    };
    get(conn, &id, true)
}

fn target(conn: &Connection, id: &str) -> Result<(String, Option<(String, String)>), ApiError> {
    let view = get(conn, id, true)?;
    let credential = conn
        .query_row(
            "SELECT secret_origin,secret FROM integrations WHERE id=?1 AND secret IS NOT NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok((view.base_url, credential))
}

async fn test_connection(
    state: &AppState,
    url: String,
    credential: Option<(String, String)>,
) -> IntegrationVerdict {
    let started = Instant::now();
    let policy = state.config.current();
    let result = client::send_bound(url, credential, &policy.http).await;
    use VerdictClass::*;
    let (class, message) = match result {
        Ok(r) if r.is_success() => (Success, "Connection succeeded"),
        Ok(r) if matches!(r.status, 401 | 403) => (Auth, "The service refused authentication"),
        Ok(_) => (HttpStatus, "The service returned a non-success status"),
        Err(e) if e.starts_with("TLS") || e.starts_with("invalid TLS") => (
            Tls,
            "TLS verification failed. Hobnail trusts public certificate authorities only.",
        ),
        Err(e) if e.starts_with("timed out") => (Timeout, "The connection test timed out"),
        Err(e) if e.starts_with("resolve") => (Dns, "The hostname could not be resolved"),
        Err(e) if e.starts_with("connect") => (Connect, "Could not connect to the service"),
        Err(e) if e.starts_with("response too large") => {
            (TooLarge, "The response exceeds the size limit")
        }
        Err(e) if e.starts_with("blocked") || e.contains("http.allow_hosts") => {
            (Blocked, "The destination is blocked by the outbound policy")
        }
        Err(_) => (Protocol, "The service did not return a valid HTTP response"),
    };
    IntegrationVerdict {
        ok: matches!(class, Success),
        class,
        message,
        latency_ms: started.elapsed().as_millis() as u64,
    }
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    m: Matched,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    let id = m.param("id");
    if id.is_some_and(|id| !user.can_see_integration(id)) {
        return Err(ApiError::not_found());
    }
    let db = &state.db;
    let id = id.unwrap_or_default();
    let view = match m.route.handler {
        Handler::Integrations => {
            return Ok(super::json(
                StatusCode::OK,
                &db.dashboard(|c| list(c, user))?,
            ))
        }
        Handler::Integration => db.dashboard(|c| get(c, id, user.role >= Role::Admin))?,
        Handler::IntegrationCreate | Handler::IntegrationUpdate => {
            let create = m.route.handler == Handler::IntegrationCreate;
            if create && user.role != Role::Owner && user.scopes.is_some() {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "scoped accounts cannot create integrations",
                ));
            }
            let input = super::read_json(req, 16384).await?;
            let saved = db.dashboard(|c| save(c, (!create).then_some(id), input))?;
            return Ok(super::json(
                if create {
                    StatusCode::CREATED
                } else {
                    StatusCode::OK
                },
                &saved,
            ));
        }
        Handler::IntegrationSetSecret | Handler::IntegrationClearSecret => {
            let secret = if m.route.handler == Handler::IntegrationSetSecret {
                let IntegrationSecretInput { secret } = super::read_json(req, 8192).await?;
                secret_value(&secret)?;
                Some(secret)
            } else {
                None
            };
            db.dashboard(|c| {
                c.execute("UPDATE integrations SET secret=?2, secret_origin=CASE WHEN ?2 IS NULL THEN NULL ELSE origin END WHERE id=?1", params![id, secret])?;
                get(c,id,true)
            })?
        }
        Handler::IntegrationDelete => {
            db.dashboard(|c| -> Result<_, ApiError> {
                if c.execute("DELETE FROM integrations WHERE id=?1", [id])? == 0 {
                    return Err(ApiError::not_found());
                }
                c.execute(
                    "DELETE FROM access_grants WHERE kind='integration' AND target_id=?1",
                    [id],
                )?;
                Ok(())
            })?;
            return Ok(super::no_content());
        }
        Handler::IntegrationTest => {
            let (url, credential) = db.dashboard(|c| target(c, id))?;
            return Ok(super::json(
                StatusCode::OK,
                &test_connection(state, url, credential).await,
            ));
        }
        _ => unreachable!("integration route"),
    };
    Ok(super::json(StatusCode::OK, &view))
}
