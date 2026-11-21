//! Saved dashboard connections and bounded connection tests.

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
    providers::{self, Class as VerdictClass, Target},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationInput {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[cfg_attr(test, ts(optional = nullable))]
    pub icon: Option<String>,
    pub enabled: bool,
    #[cfg_attr(test, ts(optional = nullable))]
    pub secret: Option<String>,
    #[serde(default)]
    pub clear_secret: bool,
    #[cfg_attr(test, ts(optional = nullable))]
    pub fingerprint: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional, type = "Record<string, unknown>"))]
    pub options: providers::Values,
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
    pub(super) kind: String,
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
    fingerprint: Option<String>,
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    options: providers::Values,
}

#[derive(Deserialize, Serialize)]
struct Config {
    base_url: String,
    icon: Option<String>,
    enabled: bool,
    fingerprint: Option<String>,
    #[serde(default)]
    options: providers::Values,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IntegrationVerdict {
    ok: bool,
    class: VerdictClass,
    message: &'static str,
    latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    fingerprint: Option<String>,
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

const SELECT: &str = "SELECT id, name, kind, origin, config, secret_origin FROM integrations";

fn read(row: &rusqlite::Row<'_>, admin: bool) -> rusqlite::Result<IntegrationView> {
    let origin: String = row.get(3)?;
    let config: String = row.get(4)?;
    let config = serde_json::from_str::<Config>(&config).unwrap_or(Config {
        base_url: origin.clone(),
        icon: None,
        enabled: true,
        fingerprint: None,
        options: Default::default(),
    });
    let secret_origin: Option<String> = row.get(5)?;
    Ok(IntegrationView {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        origin,
        has_secret: secret_origin.is_some(),
        enabled: config.enabled,
        check: None,
        admin: admin.then_some(IntegrationDetails {
            base_url: config.base_url,
            icon: config.icon,
            enabled: config.enabled,
            secret_origin,
            fingerprint: config.fingerprint,
            options: config.options,
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

pub(super) fn suggestion_targets(
    conn: &Connection,
    user: &Principal,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut stmt = conn.prepare("SELECT id, name FROM integrations ORDER BY name, id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))?;
    let mut visible = Vec::new();
    for row in rows {
        let (id, name) = row?;
        if user.can_see_integration(&id) {
            visible.push((id, name));
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

pub(super) fn provider(kind: &str) -> Result<&'static providers::Provider, ApiError> {
    providers::get(kind)
        .ok_or_else(|| ApiError::bad_request("Choose a supported provider.", Some("kind")))
}

pub(super) fn validate_input(input: &IntegrationInput) -> Result<(), ApiError> {
    let provider = provider(&input.kind)?;
    let mut values = input.options.clone();
    if values.keys().any(|key| {
        matches!(key.as_str(), "base_url" | "enabled" | "secret")
            || provider
                .fields
                .iter()
                .any(|field| field.id == key && field.kind == providers::Kind::Secret)
    }) {
        return Err(ApiError::bad_request(
            "Invalid provider options.",
            Some("options"),
        ));
    }
    values.insert("base_url".into(), input.base_url.clone().into());
    values.insert("enabled".into(), input.enabled.into());
    if input.clear_secret && input.secret.is_some() {
        return Err(ApiError::bad_request(
            "Choose either replacement or removal.",
            Some("secret"),
        ));
    }
    if let Some(secret) = &input.secret {
        secret_value(secret)?;
        values.insert("secret".into(), secret.clone().into());
    }
    providers::validate(provider.fields, &mut values).map_err(|field| {
        ApiError::bad_request("Enter a valid value for this field.", Some(&field))
    })?;
    providers::fingerprint(&input.base_url, input.fingerprint.as_deref())
        .map_err(|message| ApiError::bad_request(message, Some("fingerprint")))?;
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
    let kind = &input.kind;
    validate_input(&input)?;
    let config = serde_json::to_string(&Config {
        base_url: input.base_url,
        icon: input.icon,
        enabled: input.enabled,
        fingerprint: input.fingerprint,
        options: input.options,
    })
    .expect("serializable integration config");
    let binding = input.secret.as_ref().map(|_| &origin);
    let id = if let Some(id) = id {
        let previous = get(conn, id, true)?;
        if previous.kind != input.kind {
            return Err(ApiError::bad_request(
                "The provider of a saved integration is fixed.",
                Some("kind"),
            ));
        }
        if previous
            .admin
            .and_then(|a| a.secret_origin)
            .is_some_and(|bound| bound != origin)
            && input.secret.is_none()
            && !input.clear_secret
        {
            return Err(ApiError::bad_request(
                "enter a new secret for the changed origin, or clear the stored secret first",
                Some("secret"),
            ));
        }
        conn.execute(
            "UPDATE integrations SET name=?2, kind=?3, origin=?4, config=?5,
             secret=CASE WHEN ?8 THEN NULL ELSE coalesce(?6,secret) END,
             secret_origin=CASE WHEN ?8 THEN NULL ELSE coalesce(?7,secret_origin) END WHERE id=?1",
            params![
                id,
                input.name,
                kind,
                origin,
                config,
                input.secret,
                binding,
                input.clear_secret
            ],
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
) -> Result<(&'static providers::Provider, Target), ApiError> {
    let view = get(conn, id, true)?;
    let provider = provider(&view.kind)?;
    let details = view.admin.expect("admin details");
    if !details.enabled {
        return Err(ApiError::bad_request(
            "Enable this integration before testing its connection.",
            None,
        ));
    }
    let credential = read_credential(conn, id)?;
    let target = Target {
        fingerprint: details
            .fingerprint
            .as_deref()
            .and_then(crate::http::pin::parse),
        url: details.base_url,
        credential,
        internet_enabled: super::dashboard_settings::internet(conn)?,
        options: details.options,
    };
    Ok((provider, target))
}

fn read_credential(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<Option<client::BoundCredential>> {
    conn.query_row(
        "SELECT secret_origin,secret FROM integrations WHERE id=?1 AND secret IS NOT NULL",
        [id],
        |row| {
            Ok(client::BoundCredential {
                origin: row.get(0)?,
                token: row.get(1)?,
            })
        },
    )
    .optional()
}

pub(super) fn draft_target(
    conn: &Connection,
    id: Option<&str>,
    input: IntegrationInput,
) -> Result<(&'static providers::Provider, Target), ApiError> {
    validate_input(&input)?;
    if !input.enabled {
        return Err(ApiError::bad_request(
            "Enable this integration before testing its connection.",
            Some("enabled"),
        ));
    }
    let origin = client::origin(&input.base_url).expect("validated origin");
    let credential = match (input.secret, id) {
        (Some(token), _) => Some(client::BoundCredential { origin, token }),
        (None, Some(id)) if !input.clear_secret => {
            let saved = get(conn, id, true)?;
            if saved.kind != input.kind {
                return Err(ApiError::bad_request(
                    "Choose the saved provider.",
                    Some("kind"),
                ));
            }
            let credential = read_credential(conn, id)?;
            if credential
                .as_ref()
                .is_some_and(|value| value.origin != origin)
            {
                return Err(ApiError::bad_request(
                    "Enter the credential again for the new origin.",
                    Some("secret"),
                ));
            }
            credential
        }
        _ => None,
    };
    Ok((
        provider(&input.kind)?,
        Target {
            fingerprint: input
                .fingerprint
                .as_deref()
                .and_then(crate::http::pin::parse),
            url: input.base_url,
            credential,
            internet_enabled: super::dashboard_settings::internet(conn)?,
            options: input.options,
        },
    ))
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
    provider: Option<&providers::Provider>,
    target: Target,
) -> IntegrationVerdict {
    let started = Instant::now();
    let permit = state.provider_requests.try_acquire();
    let policy = state.config.current();
    let result = match provider {
        _ if permit.is_err() => Err(VerdictClass::Busy.into()),
        Some(provider) if provider.public_internet && !target.internet_enabled => {
            Err(VerdictClass::InternetOff.into())
        }
        Some(provider) => providers::bounded((provider.test)(&target, &policy.http)).await,
        None => target.response(&policy.http, None).await.map(drop),
    };
    let (class, fingerprint) = match result {
        Ok(()) => (VerdictClass::Success, None),
        Err(error) => (error.class, error.fingerprint),
    };
    IntegrationVerdict {
        ok: matches!(class, VerdictClass::Success),
        class,
        message: class.message(),
        latency_ms: started.elapsed().as_millis() as u64,
        fingerprint,
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
                        || input.clear_secret
                        || get(c, id, true)?.admin.is_some_and(|old| {
                            old.base_url != input.base_url
                                || old.enabled != input.enabled
                                || old.fingerprint != input.fingerprint
                                || old.options != input.options
                        }));
                let mut view = save(c, (!create).then_some(id), input)?;
                if connection_changed {
                    super::provider_widgets::invalidate(state, c, Some(id))?;
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
                super::provider_widgets::invalidate(state, c, Some(id))?;
                state.integration_checks.lock().unwrap().remove(id);
                get(c, id, true)
            })?
        }
        Handler::IntegrationDelete => {
            db.dashboard(|c| -> Result<_, ApiError> {
                super::provider_widgets::invalidate(state, c, Some(id))?;
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
            let ((provider, target), started) = db.dashboard(|c| {
                let target = target(c, id)?;
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
                Ok::<_, ApiError>((target, started))
            })?;
            let verdict = test_connection(state, Some(provider), target).await;
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
            fingerprint: None,
        };
        slot.record(failed.clone(), 1);
        assert!(slot.check.as_ref().unwrap().last_good_at.is_none());
        let success = IntegrationVerdict {
            ok: true,
            class: VerdictClass::Success,
            message: "Connection succeeded",
            latency_ms: 5,
            fingerprint: None,
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
