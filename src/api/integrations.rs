//! Generic HTTP integrations. Hobnail trusts public certificate authorities only.
//! Private CAs and self-signed certificates fail verification. There is no bypass.

use std::time::{Duration, Instant};

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
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum IntegrationKind {
    Http,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationInput {
    pub name: String,
    pub kind: IntegrationKind,
    pub base_url: String,
    #[cfg_attr(test, ts(optional = nullable))]
    pub icon: Option<String>,
    pub enabled: bool,
    #[cfg_attr(test, ts(optional = nullable))]
    pub secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationSecretInput {
    /// The URL shown when the token was entered. Only its origin is compared.
    base_url: String,
    secret: String,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationView {
    pub id: String,
    pub name: String,
    kind: IntegrationKind,
    origin: String,
    has_secret: bool,
    enabled: bool,
    check: Option<IntegrationCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    admin: Option<IntegrationDetails>,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationDetails {
    base_url: String,
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

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationVerdict {
    ok: bool,
    class: VerdictClass,
    message: &'static str,
    latency_ms: u64,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationCheck {
    verdict: IntegrationVerdict,
    last_good_at: Option<u64>,
    last_good_latency_ms: Option<u64>,
}

pub(crate) struct CheckSlot {
    started: Instant,
    check: Option<IntegrationCheck>,
}

impl CheckSlot {
    fn record(&mut self, verdict: IntegrationVerdict, now: u64) {
        let previous = self.check.as_ref();
        self.check = Some(IntegrationCheck {
            last_good_at: if verdict.ok {
                Some(now)
            } else {
                previous.and_then(|check| check.last_good_at)
            },
            last_good_latency_ms: if verdict.ok {
                Some(verdict.latency_ms)
            } else {
                previous.and_then(|check| check.last_good_latency_ms)
            },
            verdict,
        });
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
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
        base_url: origin.clone(),
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
        origin,
        has_secret: secret_origin.is_some(),
        enabled: config.enabled,
        check: None,
        admin: admin.then_some(IntegrationDetails {
            base_url: config.base_url,
            icon: config.icon,
            enabled: config.enabled,
            secret_origin,
        }),
    })
}

pub(super) fn list(
    state: &AppState,
    conn: &Connection,
    user: &Principal,
) -> Result<Vec<IntegrationView>, ApiError> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY name, id"))?;
    let mut rows = stmt.query([])?;
    let mut visible = Vec::new();
    while let Some(row) = rows.next()? {
        if user.can_see_integration(
            row.get_ref(0)?
                .as_str()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        ) {
            let mut view = read(row, user.role >= Role::Admin)?;
            attach_check(state, &mut view);
            visible.push(view);
        }
    }
    Ok(visible)
}

fn attach_check(state: &AppState, view: &mut IntegrationView) {
    view.check = state
        .integration_checks
        .lock()
        .unwrap()
        .get(&view.id)
        .and_then(|slot| slot.check.clone());
}

fn get(conn: &Connection, id: &str, admin: bool) -> Result<IntegrationView, ApiError> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], |r| read(r, admin))
        .optional()?
        .ok_or_else(ApiError::not_found)
}

pub(super) fn secret_value(value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.len() > 4096 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ApiError::bad_request("enter a non-empty bearer token of up to 4096 printable ASCII characters without spaces", Some("secret")));
    }
    Ok(())
}

pub(super) fn save(
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
        let count: u32 =
            conn.query_row("SELECT count(*) FROM integrations", [], |row| row.get(0))?;
        if count >= 128 {
            return Err(ApiError::bad_request(
                "at most 128 integrations are supported",
                None,
            ));
        }
        conn.query_row(
            "INSERT INTO integrations (name,kind,origin,config,secret,secret_origin)
             VALUES (?1,?2,?3,?4,?5,?6) RETURNING id",
            params![input.name, kind, origin, config, input.secret, binding],
            |r| r.get(0),
        )?
    };
    get(conn, &id, true)
}

pub(super) fn endpoints(conn: &Connection) -> Result<Vec<(String, String, String)>, ApiError> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY name, id"))?;
    let rows = stmt.query_map([], |row| {
        let view = read(row, true)?;
        Ok((
            view.id,
            view.name,
            view.admin.expect("admin details").base_url,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn target(
    conn: &Connection,
    id: &str,
) -> Result<(String, Option<client::BoundCredential>), ApiError> {
    let details = get(conn, id, true)?.admin.expect("admin details");
    if !details.enabled {
        return Err(ApiError::bad_request(
            "Enable this integration before testing its connection.",
            None,
        ));
    }
    let credential = conn
        .query_row(
            "SELECT secret_origin,secret FROM integrations WHERE id=?1 AND secret IS NOT NULL",
            [id],
            |r| {
                Ok(client::BoundCredential {
                    origin: r.get(0)?,
                    token: r.get(1)?,
                })
            },
        )
        .optional()?;
    Ok((details.base_url, credential))
}

pub(super) fn limit_test(state: &AppState, account: i64) -> Result<(), ApiError> {
    let mut attempts = state.integration_tests.lock().unwrap();
    let now = Instant::now();
    let recent = |time: &Instant| now.duration_since(*time) < Duration::from_secs(10);
    attempts.retain(|_, times| times.back().is_some_and(recent));
    let times = attempts.entry(account).or_default();
    times.retain(recent);
    if times.len() >= 3 {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Wait ten seconds before testing another integration.",
        ));
    }
    times.push_back(now);
    Ok(())
}

pub(super) async fn test_connection(
    state: &AppState,
    url: String,
    credential: Option<client::BoundCredential>,
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
                &db.dashboard(|c| list(state, c, user))?,
            ))
        }
        Handler::Integration => db.dashboard(|c| {
            let mut view = get(c, id, user.role >= Role::Admin)?;
            attach_check(state, &mut view);
            Ok::<_, ApiError>(view)
        })?,
        Handler::IntegrationCreate | Handler::IntegrationUpdate => {
            let create = m.route.handler == Handler::IntegrationCreate;
            let input: IntegrationInput = super::read_json(req, 16384).await?;
            let saved = db.dashboard(|c| {
                let connection_changed = !create
                    && (input.secret.is_some()
                        || get(c, id, true)?.admin.is_some_and(|old| {
                            old.base_url != input.base_url || old.enabled != input.enabled
                        }));
                let mut view = save(c, (!create).then_some(id), input)?;
                if connection_changed {
                    state.integration_checks.lock().unwrap().remove(id);
                } else {
                    attach_check(state, &mut view);
                }
                Ok::<_, ApiError>(view)
            })?;
            state.changed("reload");
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
            let replacement = if m.route.handler == Handler::IntegrationSetSecret {
                let IntegrationSecretInput { base_url, secret } =
                    super::read_json(req, 8192).await?;
                secret_value(&secret)?;
                let origin = client::origin(&base_url).ok_or_else(|| {
                    ApiError::bad_request(
                        "supply the integration URL shown when the token was entered",
                        Some("base_url"),
                    )
                })?;
                Some((origin, secret))
            } else {
                None
            };
            db.dashboard(|c| {
                let saved: String = c
                    .query_row("SELECT origin FROM integrations WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()?
                    .ok_or_else(ApiError::not_found)?;
                if let Some((origin, token)) = replacement {
                    if saved != origin {
                        return Err(ApiError::bad_request(
                            "the integration origin changed; reload it before entering a token",
                            Some("base_url"),
                        ));
                    }
                    c.execute(
                        "UPDATE integrations SET secret=?2, secret_origin=?3 WHERE id=?1",
                        params![id, token, origin],
                    )?;
                } else {
                    c.execute(
                        "UPDATE integrations SET secret=NULL, secret_origin=NULL WHERE id=?1",
                        [id],
                    )?;
                }
                state.integration_checks.lock().unwrap().remove(id);
                get(c, id, true)
            })?
        }
        Handler::IntegrationDelete => {
            db.dashboard(|c| -> Result<_, ApiError> {
                if c.execute("DELETE FROM integrations WHERE id=?1", [id])? == 0 {
                    return Err(ApiError::not_found());
                }
                crate::db::accounts::revoke_target_invites(
                    c,
                    "$.integrations",
                    id,
                    crate::auth::now_s(),
                )?;
                c.execute(
                    "DELETE FROM access_grants WHERE kind='integration' AND target_id=?1",
                    [id],
                )?;
                state.integration_checks.lock().unwrap().remove(id);
                Ok(())
            })?;
            state.changed("reload");
            return Ok(super::no_content());
        }
        Handler::IntegrationTest => {
            let (url, credential, started) = db.dashboard(|c| {
                let (url, credential) = target(c, id)?;
                limit_test(state, user.id)?;
                let started = Instant::now();
                let mut checks = state.integration_checks.lock().unwrap();
                checks
                    .entry(id.to_string())
                    .or_insert(CheckSlot {
                        started,
                        check: None,
                    })
                    .started = started;
                Ok::<_, ApiError>((url, credential, started))
            })?;
            let verdict = test_connection(state, url, credential).await;
            if let Some(slot) = state
                .integration_checks
                .lock()
                .unwrap()
                .get_mut(id)
                .filter(|slot| slot.started == started)
            {
                slot.record(verdict.clone(), crate::util::unix_now());
                state.changed("reload");
            }
            return Ok(super::json(StatusCode::OK, &verdict));
        }
        _ => unreachable!("integration route"),
    };
    Ok(super::json(StatusCode::OK, &view))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_keep_the_last_success_through_failures() {
        let mut slot = CheckSlot {
            started: Instant::now(),
            check: None,
        };
        let failed = IntegrationVerdict {
            ok: false,
            class: VerdictClass::Connect,
            message: "Could not connect to the service",
            latency_ms: 10,
        };
        slot.record(failed.clone(), 1);
        assert!(slot.check.as_ref().unwrap().last_good_at.is_none());
        let success = IntegrationVerdict {
            ok: true,
            class: VerdictClass::Success,
            message: "Connection succeeded",
            latency_ms: 5,
        };
        slot.record(success.clone(), 2);
        slot.record(failed.clone(), 3);
        slot.record(failed, 4);
        let check = slot.check.as_ref().unwrap();
        assert!(!check.verdict.ok);
        assert_eq!(check.last_good_at, Some(2));
        assert_eq!(check.last_good_latency_ms, Some(5));
        slot.record(success, 5);
        let check = slot.check.unwrap();
        assert!(check.verdict.ok);
        assert_eq!(check.last_good_at, Some(5));
    }
}
