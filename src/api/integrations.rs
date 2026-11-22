//! Saved dashboard connections and bounded connection tests.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

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
    providers::{self, Class as VerdictClass, Credentials, Target},
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
    #[cfg_attr(test, ts(skip))]
    pub clear_secret: bool,
    /// Omitted fields keep their saved value. Null explicitly removes a field.
    #[serde(default)]
    #[cfg_attr(test, ts(optional, type = "Record<string, string | null>"))]
    pub credentials: BTreeMap<String, Option<String>>,
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
    pub(super) has_secret: bool,
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
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    credentials: BTreeMap<String, String>,
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

const SELECT: &str = "SELECT id, name, kind, origin, config,
    (SELECT json_group_object(field, origin) FROM integration_credentials WHERE integration_id=integrations.id)
    FROM integrations";

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
    let bindings: String = row.get(5)?;
    let credentials: BTreeMap<String, String> =
        serde_json::from_str(&bindings).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let secret_origin = credentials.get("secret").cloned();
    Ok(IntegrationView {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        origin,
        has_secret: !credentials.is_empty(),
        enabled: config.enabled,
        check: None,
        admin: admin.then_some(IntegrationDetails {
            base_url: config.base_url,
            icon: config.icon,
            enabled: config.enabled,
            secret_origin,
            credentials,
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
    include_local: bool,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut stmt = conn
        .prepare("SELECT id, name FROM integrations WHERE ?1 OR origin <> '' ORDER BY name, id")?;
    let rows = stmt.query_map([include_local], |row| {
        Ok((row.get::<_, String>(0)?, row.get(1)?))
    })?;
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

pub(super) fn credential_field(
    provider: &'static providers::Provider,
    field: &str,
    value: Option<&str>,
) -> Result<&'static str, ApiError> {
    let spec = provider
        .fields
        .iter()
        .find(|spec| spec.id == field && spec.kind == providers::Kind::Secret)
        .ok_or_else(|| ApiError::bad_request("Choose a credential field.", Some("credentials")))?;
    if let Some(value) = value {
        if provider.id == "http" {
            secret_value(value)?;
        }
        if !spec.accepts(&serde_json::Value::from(value)) || value.len() > 4096 {
            return Err(ApiError::bad_request(
                "Enter a valid credential.",
                Some(spec.id),
            ));
        }
    }
    Ok(spec.id)
}

pub(super) fn provider_origin(
    provider: &providers::Provider,
    url: &str,
) -> Result<String, ApiError> {
    if !provider.has_endpoint() && url.is_empty() {
        return Ok(String::new());
    }
    if provider.has_endpoint() {
        if let Some(origin) = client::origin(url) {
            return Ok(origin);
        }
    }
    Err(ApiError::bad_request(
        "Enter an absolute HTTP or HTTPS URL without user information, query, or fragment.",
        Some("base_url"),
    ))
}

fn validated_options(input: &IntegrationInput) -> Result<providers::Values, ApiError> {
    let provider = provider(&input.kind)?;
    let mut values = input.options.clone();
    if values.keys().any(|key| {
        matches!(key.as_str(), "base_url" | "enabled")
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
    provider_origin(provider, &input.base_url)?;
    if provider.has_endpoint() {
        values.insert("base_url".into(), input.base_url.clone().into());
    }
    values.insert("enabled".into(), input.enabled.into());
    let fields: Vec<_> = provider
        .fields
        .iter()
        .copied()
        .filter(|field| field.kind != providers::Kind::Secret)
        .collect();
    providers::validate(&fields, &mut values).map_err(|field| {
        ApiError::bad_request("Enter a valid value for this field.", Some(&field))
    })?;
    providers::fingerprint(&input.base_url, input.fingerprint.as_deref())
        .map_err(|message| ApiError::bad_request(message, Some("fingerprint")))?;
    values.remove("base_url");
    values.remove("enabled");
    Ok(values)
}

fn effective_credentials(
    conn: &Connection,
    id: Option<&str>,
    input: &mut IntegrationInput,
    origin: &str,
) -> Result<Credentials, ApiError> {
    let provider = provider(&input.kind)?;
    let mut saved = if let Some(id) = id {
        if get(conn, id, true)?.kind != input.kind {
            return Err(ApiError::bad_request(
                "The provider of a saved integration is fixed.",
                Some("kind"),
            ));
        }
        read_credentials(conn, id)?
    } else {
        Credentials::new()
    };
    if input.secret.is_some() || input.clear_secret {
        if input.secret.is_some() && input.clear_secret || input.credentials.contains_key("secret")
        {
            return Err(ApiError::bad_request(
                "Choose either replacement or removal.",
                Some("secret"),
            ));
        }
        input
            .credentials
            .insert("secret".into(), input.secret.take());
    }
    for (field, change) in std::mem::take(&mut input.credentials) {
        credential_field(provider, &field, change.as_deref())?;
        if let Some(token) = change {
            saved.insert(
                field,
                client::BoundCredential {
                    origin: origin.into(),
                    token,
                },
            );
        } else {
            saved.remove(&field);
        }
    }
    for field in provider
        .fields
        .iter()
        .filter(|spec| spec.kind == providers::Kind::Secret)
    {
        match saved.get(field.id) {
            Some(value) if value.origin != origin => {
                return Err(ApiError::bad_request(
                    "Enter the credential again for the new origin or clear the saved credential.",
                    Some(field.id),
                ))
            }
            None if field.required => {
                return Err(ApiError::bad_request("Enter a credential.", Some(field.id)))
            }
            _ => {}
        }
    }
    Ok(saved)
}

pub(super) fn connection_changed(
    conn: &Connection,
    id: &str,
    input: &IntegrationInput,
) -> Result<bool, ApiError> {
    let options = validated_options(input)?;
    Ok(input.secret.is_some()
        || input.clear_secret
        || !input.credentials.is_empty()
        || get(conn, id, true)?.admin.is_some_and(|old| {
            old.base_url != input.base_url
                || old.enabled != input.enabled
                || old.fingerprint != input.fingerprint
                || old.options != options
        }))
}

pub(super) fn save(
    conn: &Connection,
    id: Option<&str>,
    mut input: IntegrationInput,
) -> Result<IntegrationView, ApiError> {
    super::validate_label(&input.name, "name")?;
    super::validate_icon(input.icon.as_deref())?;
    let origin = provider_origin(provider(&input.kind)?, &input.base_url)?;
    input.options = validated_options(&input)?;
    let credentials = effective_credentials(conn, id, &mut input, &origin)?;
    let kind = &input.kind;
    let config = serde_json::to_string(&Config {
        base_url: input.base_url,
        icon: input.icon,
        enabled: input.enabled,
        fingerprint: input.fingerprint,
        options: input.options,
    })
    .expect("serializable integration config");
    let id = if let Some(id) = id {
        conn.execute(
            "DELETE FROM integration_credentials WHERE integration_id=?1",
            [id],
        )?;
        conn.execute(
            "UPDATE integrations SET name=?2, origin=?3, config=?4 WHERE id=?1",
            params![id, input.name, origin, config],
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
            "INSERT INTO integrations (name,kind,origin,config) VALUES (?1,?2,?3,?4) RETURNING id",
            params![input.name, kind, origin, config],
            |r| r.get(0),
        )?
    };
    for (field, credential) in credentials {
        write_credential(conn, &id, &field, &credential.origin, &credential.token)?;
    }
    get(conn, &id, true)
}

fn write_credential(
    conn: &Connection,
    id: &str,
    field: &str,
    origin: &str,
    token: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO integration_credentials (integration_id,field,origin,secret) VALUES (?1,?2,?3,?4)
         ON CONFLICT(integration_id,field) DO UPDATE SET origin=excluded.origin,secret=excluded.secret",
        params![id, field, origin, token],
    )?;
    Ok(())
}

pub(super) fn endpoints(
    conn: &Connection,
) -> Result<Vec<(String, String, String, String)>, ApiError> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY name, id"))?;
    let rows = stmt.query_map([], |row| {
        let view = read(row, true)?;
        Ok((
            view.id,
            view.name,
            view.kind,
            view.admin.expect("admin details").base_url,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn target(
    state: &AppState,
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
    let mut target = Target::new(
        details.base_url,
        read_credentials(conn, id)?,
        super::dashboard_settings::internet(conn)?,
    );
    target.fingerprint = details
        .fingerprint
        .as_deref()
        .and_then(crate::http::pin::parse);
    target.options = details.options;
    state.provider_connections.bind(&mut target, Some(id), true);
    Ok((provider, target))
}

fn read_credentials(conn: &Connection, id: &str) -> rusqlite::Result<Credentials> {
    let mut stmt = conn.prepare(
        "SELECT field,origin,secret FROM integration_credentials WHERE integration_id=?1",
    )?;
    let rows = stmt.query_map([id], |row| {
        Ok((
            row.get(0)?,
            client::BoundCredential {
                origin: row.get(1)?,
                token: row.get(2)?,
            },
        ))
    })?;
    rows.collect()
}

pub(super) fn draft_target(
    state: &AppState,
    conn: &Connection,
    id: Option<&str>,
    mut input: IntegrationInput,
) -> Result<(&'static providers::Provider, Target), ApiError> {
    input.options = validated_options(&input)?;
    if !input.enabled {
        return Err(ApiError::bad_request(
            "Enable this integration before testing its connection.",
            Some("enabled"),
        ));
    }
    let origin = provider_origin(provider(&input.kind)?, &input.base_url)?;
    let credentials = effective_credentials(conn, id, &mut input, &origin)?;
    let mut target = Target::new(
        input.base_url,
        credentials,
        super::dashboard_settings::internet(conn)?,
    );
    target.fingerprint = input
        .fingerprint
        .as_deref()
        .and_then(crate::http::pin::parse);
    target.options = input.options;
    state.provider_connections.bind(&mut target, id, false);
    Ok((provider(&input.kind)?, target))
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
    let (policy, revision) = state.config.provider_http();
    let result = async {
        let permit = state
            .provider_requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| VerdictClass::Busy)?;
        if provider.is_some_and(|provider| provider.public_internet) && !target.internet_enabled {
            return Err(VerdictClass::InternetOff.into());
        }
        let conn =
            providers::Conn::new(&target, &policy.http, &state.provider_connections, permit)?
                .with_http_revision(revision);
        conn.run(async {
            match provider {
                Some(provider) => (provider.test)(&conn).await,
                None => providers::success_status(conn.http_status().await?.status),
            }
        })
        .await
    }
    .await;
    let (class, fingerprint) = match result {
        Ok(()) => (VerdictClass::Success, None),
        Err(error) => (error.class, error.fingerprint),
    };
    let message = match (class, provider.map(|provider| provider.id)) {
        (VerdictClass::Success, Some("nginx_proxy_manager")) => {
            "Signed in and read the account profile. Widget list permissions are unchecked."
        }
        (VerdictClass::Success, Some("immich")) => {
            "Read storage statistics. Library statistics and job permissions are unchecked."
        }
        (VerdictClass::Success, Some("technitium")) => {
            "Read the zone list. Dashboard permissions are unchecked."
        }
        _ => class.message(),
    };
    IntegrationVerdict {
        ok: matches!(class, VerdictClass::Success),
        class,
        message,
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
                let connection_changed = !create && connection_changed(c, id, &input)?;
                let mut view = save(c, (!create).then_some(id), input)?;
                if connection_changed {
                    super::provider_widgets::invalidate(state, Some(id));
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
                let saved = get(c, id, true)?;
                credential_field(provider(&saved.kind)?, "secret", None)?;
                if let Some((origin, token)) = replacement {
                    if saved.origin != origin {
                        return Err(ApiError::bad_request(
                            "the integration origin changed; reload it before entering a token",
                            Some("base_url"),
                        ));
                    }
                    write_credential(c, id, "secret", &origin, &token)?;
                } else {
                    c.execute("DELETE FROM integration_credentials WHERE integration_id=?1 AND field='secret'", [id])?;
                }
                super::provider_widgets::invalidate(state, Some(id));
                state.integration_checks.lock().unwrap().remove(id);
                get(c, id, true)
            })?
        }
        Handler::IntegrationDelete => {
            db.dashboard(|c| -> Result<_, ApiError> {
                super::provider_widgets::invalidate(state, Some(id));
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
                db.invalidate_provider_scope();
                Ok(())
            })?;
            super::provider_widgets::placements_changed(state);
            state.changed("reload");
            return Ok(super::no_content());
        }
        Handler::IntegrationTest => {
            let ((provider, target), started) = db.dashboard(|c| {
                let target = target(state, c, id)?;
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
