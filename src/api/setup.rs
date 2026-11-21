//! Guided setup for owners created through the browser.
use std::collections::{HashMap, HashSet};

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    access::{Handler, Principal},
    boards::{self, BoardDocument, BoardItem, BoardPayload, BoardSection, BoardSuggestion},
    dashboard_settings,
    error::ApiError,
    integrations::{self, IntegrationInput, IntegrationKind},
};
use crate::{
    http::{client, server::Body},
    state::AppState,
};

const KEY: &str = crate::db::accounts::SETUP_DRAFT_KEY;

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename_all = "snake_case"))]
pub enum SetupStep {
    #[default]
    Appearance,
    Discover,
    Select,
    Connections,
    Review,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupDocument {
    step: SetupStep,
    theme: Option<String>,
    items: Vec<SetupItem>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupItem {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    source: Option<String>,
    included: bool,
    name: String,
    group: String,
    target: SetupTarget,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS), ts(tag = "kind", rename_all = "snake_case"))]
pub enum SetupTarget {
    Board { payload: BoardPayload },
    Http { base_url: String, enabled: bool },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    revision: u32,
    board_id: Option<String>,
    document: SetupDocument,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupDraft {
    revision: u32,
    board_id: Option<String>,
    document: SetupDocument,
    theme_default: String,
    credential_origins: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupUpdate {
    revision: u32,
    document: SetupDocument,
    credentials: Vec<SetupCredentialChange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupCredentialChange {
    item_id: String,
    base_url: String,
    secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupCommit {
    revision: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupTest {
    revision: u32,
    item_id: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupResult {
    board_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename_all = "snake_case"))]
pub enum SetupDocker {
    Reachable,
    Unavailable,
    Disabled,
    Waiting,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetupDiscovery {
    docker: SetupDocker,
    config_stale: bool,
    suggestions: Vec<BoardSuggestion>,
}

pub(super) fn pending(conn: &Connection, owner: i64) -> Result<bool, ApiError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM drafts WHERE user_id=?1 AND key=?2
         AND json_extract(value,'$.board_id') IS NULL)",
        params![owner, KEY],
        |row| row.get(0),
    )?)
}

fn read(conn: &Connection, owner: i64) -> Result<Saved, ApiError> {
    let raw: String = conn
        .query_row(
            "SELECT value FROM drafts WHERE user_id=?1 AND key=?2",
            params![owner, KEY],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(ApiError::not_found)?;
    boards::decode(&raw)
}

fn store(conn: &Connection, owner: i64, saved: &Saved, now: i64) -> Result<(), ApiError> {
    let raw = serde_json::to_string(saved).expect("serializable setup draft");
    conn.execute(
        "UPDATE drafts SET value=?3,updated=?4 WHERE user_id=?1 AND key=?2",
        params![owner, KEY, raw, now],
    )?;
    Ok(())
}

fn view(
    conn: &Connection,
    owner: i64,
    saved: Saved,
    config: &crate::config::Config,
) -> Result<SetupDraft, ApiError> {
    let mut stmt =
        conn.prepare("SELECT item_id, origin FROM setup_credentials WHERE user_id=?1")?;
    let origins = stmt.query_map([owner], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(SetupDraft {
        revision: saved.revision,
        board_id: saved.board_id,
        document: saved.document,
        theme_default: dashboard_settings::theme(conn, config)?,
        credential_origins: origins.collect::<rusqlite::Result<_>>()?,
    })
}

fn require_revision(saved: &Saved, revision: u32) -> Result<(), ApiError> {
    if saved.board_id.is_some() || saved.revision != revision {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "Setup changed in another request. Reload the saved draft.",
        ));
    }
    Ok(())
}

fn save(conn: &Connection, owner: i64, update: SetupUpdate, now: i64) -> Result<Saved, ApiError> {
    let mut saved = read(conn, owner)?;
    require_revision(&saved, update.revision)?;
    let items: HashMap<_, _> = update
        .document
        .items
        .iter()
        .map(|item| (item.id.as_str(), &item.target))
        .collect();
    if update.document.items.len() > 512
        || items.len() != update.document.items.len()
        || items.keys().any(|id| !super::valid_id(id))
        || update.credentials.len() > 128
    {
        return Err(ApiError::bad_request(
            "invalid or too many setup items",
            Some("items"),
        ));
    }
    dashboard_settings::validate_theme(update.document.theme.as_deref())?;
    // An omitted item or one converted to another kind cannot retain a token.
    let mut stmt = conn.prepare("SELECT item_id FROM setup_credentials WHERE user_id=?1")?;
    let keys = stmt
        .query_map([owner], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for key in keys {
        if !matches!(items.get(key.as_str()), Some(SetupTarget::Http { .. })) {
            conn.execute(
                "DELETE FROM setup_credentials WHERE user_id=?1 AND item_id=?2",
                params![owner, key],
            )?;
        }
    }
    for change in update.credentials {
        let Some(SetupTarget::Http { base_url, .. }) = items.get(change.item_id.as_str()) else {
            return Err(ApiError::bad_request(
                "select an HTTP connection for this credential",
                Some("secret"),
            ));
        };
        if let Some(secret) = change.secret {
            integrations::secret_value(&secret)?;
            let origin = integration_origin(&change.base_url)?;
            if client::origin(base_url).as_deref() != Some(&origin) {
                return Err(ApiError::bad_request(
                    "enter the credential for the current service origin",
                    Some("secret"),
                ));
            }
            conn.execute(
                "INSERT INTO setup_credentials (user_id,item_id,origin,secret) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(user_id,item_id) DO UPDATE SET origin=excluded.origin,secret=excluded.secret",
                params![owner, change.item_id, origin, secret],
            )?;
        } else {
            conn.execute(
                "DELETE FROM setup_credentials WHERE user_id=?1 AND item_id=?2",
                params![owner, change.item_id],
            )?;
        }
    }
    saved.document = update.document;
    saved.revision += 1;
    store(conn, owner, &saved, now)?;
    Ok(saved)
}

fn integration_origin(url: &str) -> Result<String, ApiError> {
    client::origin(url).ok_or_else(|| {
        ApiError::bad_request(
            "Enter an HTTP or HTTPS service URL without user information, a query or a fragment.",
            Some("base_url"),
        )
    })
}

fn endpoint_key(url: &str) -> Result<String, ApiError> {
    let origin = integration_origin(url)?;
    let uri: hyper::Uri = url.parse().expect("validated HTTP URL");
    Ok(format!("{origin}{}", uri.path()))
}

fn credential(
    conn: &Connection,
    owner: i64,
    item_id: &str,
    url: &str,
) -> Result<Option<client::BoundCredential>, ApiError> {
    let origin = integration_origin(url)?;
    let saved = conn
        .query_row(
            "SELECT origin,secret FROM setup_credentials WHERE user_id=?1 AND item_id=?2",
            params![owner, item_id],
            |row| {
                Ok(client::BoundCredential {
                    origin: row.get(0)?,
                    token: row.get(1)?,
                })
            },
        )
        .optional()?;
    if saved.as_ref().is_some_and(|value| value.origin != origin) {
        return Err(ApiError::bad_request(
            "Enter the credential again for the new origin or clear the saved credential.",
            Some("secret"),
        ));
    }
    Ok(saved)
}

fn target(
    conn: &Connection,
    user: &Principal,
    test: SetupTest,
) -> Result<(String, Option<client::BoundCredential>), ApiError> {
    let saved = read(conn, user.id)?;
    require_revision(&saved, test.revision)?;
    let item = saved
        .document
        .items
        .into_iter()
        .find(|item| item.id == test.item_id && item.included)
        .ok_or_else(ApiError::not_found)?;
    match item.target {
        SetupTarget::Http {
            base_url,
            enabled: true,
        } => {
            let secret = credential(conn, user.id, &item.id, &base_url)?;
            Ok((base_url, secret))
        }
        SetupTarget::Http { enabled: false, .. } => Err(ApiError::bad_request(
            "Enable this integration before testing its connection.",
            None,
        )),
        SetupTarget::Board {
            payload: BoardPayload::Integration { integration_id, .. },
        } => integrations::target(conn, &integration_id),
        SetupTarget::Board {
            payload:
                BoardPayload::Link { url, .. }
                | BoardPayload::Container {
                    link: Some(url), ..
                },
        } => {
            boards::check_test_link(&url)?;
            Ok((url, None))
        }
        _ => Err(ApiError::bad_request(
            "This item has no HTTP connection to test.",
            None,
        )),
    }
}

fn payload(conn: &Connection, owner: i64, item: SetupItem) -> Result<BoardPayload, ApiError> {
    let name = item.name.trim().to_owned();
    Ok(match item.target {
        SetupTarget::Board { mut payload } => {
            match &mut payload {
                BoardPayload::Link { title, .. } => *title = name,
                BoardPayload::Container { title, .. }
                | BoardPayload::Widget { title, .. }
                | BoardPayload::Integration { title, .. } => *title = Some(name),
            }
            payload
        }
        SetupTarget::Http { base_url, enabled } => {
            let secret = credential(conn, owner, &item.id, &base_url)?.map(|value| value.token);
            let integration = integrations::save(
                conn,
                None,
                IntegrationInput {
                    name,
                    kind: IntegrationKind::Http,
                    base_url,
                    icon: None,
                    enabled,
                    secret,
                },
            )?;
            BoardPayload::Integration {
                integration_id: integration.id,
                title: None,
            }
        }
    })
}

fn commit(
    conn: &Connection,
    user: &Principal,
    revision: u32,
    config: &crate::config::Config,
    now: i64,
) -> Result<(SetupResult, Option<u32>), ApiError> {
    let mut saved = read(conn, user.id)?;
    if let Some(board_id) = saved.board_id {
        return Ok((SetupResult { board_id }, None));
    }
    require_revision(&saved, revision)?;
    if saved.document.step != SetupStep::Review {
        return Err(ApiError::bad_request(
            "Review the setup before creating the board.",
            Some("step"),
        ));
    }
    let mut sections: Vec<BoardSection> = Vec::new();
    let mut identities = HashSet::new();
    let mut sources = HashSet::new();
    let mut new_endpoints = HashSet::new();
    let mut saved_endpoints = HashSet::new();
    for item in saved.document.items.drain(..).filter(|item| item.included) {
        let unique = match &item.target {
            SetupTarget::Http { base_url, .. } => {
                let key = endpoint_key(base_url)?;
                !saved_endpoints.contains(&key) && new_endpoints.insert(key)
            }
            SetupTarget::Board { payload } => {
                let new_identity = identities.insert(boards::identity(payload));
                if let BoardPayload::Integration { integration_id, .. } = payload {
                    let key = endpoint_key(&integrations::base_url(conn, integration_id)?)?;
                    let duplicates_new = new_endpoints.contains(&key);
                    saved_endpoints.insert(key);
                    new_identity && !duplicates_new
                } else {
                    new_identity
                }
            }
        };
        if !unique
            || item
                .source
                .as_ref()
                .is_some_and(|source| !sources.insert(source.clone()))
        {
            return Err(ApiError::bad_request("This service is selected more than once. Exclude a duplicate before creating the board.", Some("items")));
        }
        let group = item.group.trim();
        let title = if group.is_empty() { "Services" } else { group };
        let index = sections
            .iter()
            .position(|s| s.title == title)
            .unwrap_or_else(|| {
                let index = sections.len();
                sections.push(BoardSection {
                    id: None,
                    title: title.to_owned(),
                    position: index as u32,
                    items: Vec::new(),
                });
                index
            });
        let section = &mut sections[index];
        section.items.push(BoardItem {
            id: None,
            position: section.items.len() as u32,
            spans: Default::default(),
            payload: payload(conn, user.id, item)?,
        });
    }
    let doc = BoardDocument {
        version: 0,
        sections,
    };
    let empty = BoardDocument {
        version: 0,
        sections: Vec::new(),
    };
    boards::validate(&doc, &empty, user, conn, config)?;
    let board = boards::create(conn, "Home", None, now)?;
    boards::store(conn, &board.id, doc, now)?;
    dashboard_settings::set_default_board(conn, Some(&board.id))?;
    if let Some(theme) = saved.document.theme.as_deref() {
        dashboard_settings::set_theme(conn, Some(theme))?;
    }
    conn.execute("DELETE FROM setup_credentials WHERE user_id=?1", [user.id])?;
    saved.board_id = Some(board.id.clone());
    store(conn, user.id, &saved, now)?;
    Ok((SetupResult { board_id: board.id }, Some(board.version + 1)))
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    handler: Handler,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    let now = crate::auth::now_s();
    let config = state.config.current();
    let db = &state.db;
    Ok(match handler {
        Handler::SetupDraft => super::json(
            StatusCode::OK,
            &db.dashboard(|conn| view(conn, user.id, read(conn, user.id)?, &config))?,
        ),
        Handler::SetupSave => {
            let update = super::read_json(req, 2 * 1024 * 1024).await?;
            let draft = db.dashboard(|conn| {
                view(conn, user.id, save(conn, user.id, update, now)?, &config)
            })?;
            super::json(StatusCode::OK, &draft)
        }
        Handler::SetupDiscover => {
            let integrations = db.dashboard(|conn| {
                let saved = read(conn, user.id)?;
                require_revision(&saved, saved.revision)?;
                integrations::list(conn, user)
            })?;
            let snap = state.snapshot();
            let docker = if !config.docker.enabled {
                SetupDocker::Disabled
            } else if snap.error.is_some() {
                SetupDocker::Unavailable
            } else if snap.fetched_at == 0 {
                SetupDocker::Waiting
            } else {
                SetupDocker::Reachable
            };
            let doc = BoardDocument {
                version: 0,
                sections: Vec::new(),
            };
            let suggestions = boards::suggestions(
                state,
                now,
                &doc,
                integrations,
                &user.container_visibility(state),
            );
            super::json(
                StatusCode::OK,
                &SetupDiscovery {
                    docker,
                    config_stale: state.config.snapshot().error.is_some(),
                    suggestions,
                },
            )
        }
        Handler::SetupTest => {
            let test = super::read_json(req, 8192).await?;
            let (url, secret) = db.dashboard(|conn| target(conn, user, test))?;
            integrations::limit_test(state, user.id)?;
            super::json(
                StatusCode::OK,
                &integrations::test_connection(state, url, secret).await,
            )
        }
        Handler::SetupCommit => {
            let input: SetupCommit = super::read_json(req, 8192).await?;
            let (result, version) =
                db.dashboard(|conn| commit(conn, user, input.revision, &config, now))?;
            if let Some(version) = version {
                boards::publish(state, &result.board_id, version);
                state.changed("reload");
            }
            super::json(StatusCode::OK, &result)
        }
        _ => unreachable!("setup route"),
    })
}
