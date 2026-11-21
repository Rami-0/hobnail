//! Registry install API, board widget instances, and viewer-gated refreshes.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Instant,
};

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use tokio::sync::watch;

use super::{
    access::{Handler, Principal},
    boards, dashboard_settings,
    error::ApiError,
    integrations::{self, IntegrationInput, IntegrationView},
    widgets::{self, WidgetView},
};
use crate::{
    http::server::Body,
    providers::{self, Target, Values},
    state::AppState,
    widgets::WidgetOutput,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    integration: IntegrationInput,
    integration_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    integration: IntegrationInput,
    integration_id: Option<String>,
    board_id: Option<String>,
    #[serde(default)]
    widgets: Vec<Selection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    id: String,
    #[serde(default)]
    options: Values,
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    handler: Handler,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    if handler == Handler::ProviderWidgetOptions {
        let id = crate::http::query::parse_query(req.uri().query().unwrap_or_default());
        let id =
            crate::http::query::query_get(&id, "integration").ok_or_else(ApiError::not_found)?;
        let options = state.db.dashboard(|conn| {
            let mut stmt = conn
                .prepare("SELECT widget, options FROM provider_widgets WHERE integration_id=?1")?;
            let rows = stmt.query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut options = HashMap::new();
            for row in rows {
                let (widget, value) = row?;
                if let Ok(value) = serde_json::from_str::<Values>(&value) {
                    options.insert(widget, value);
                }
            }
            Ok::<_, rusqlite::Error>(options)
        })?;
        return Ok(super::json(StatusCode::OK, &options));
    }
    if handler == Handler::ProviderTest {
        let input: Draft = super::read_json(req, 16384).await?;
        let (provider, target) = state.db.dashboard(|conn| {
            integrations::draft_target(conn, input.integration_id.as_deref(), input.integration)
        })?;
        integrations::limit_test(state, user.id)?;
        return Ok(super::json(
            StatusCode::OK,
            &integrations::test_connection(state, Some(provider), target).await,
        ));
    }
    let input: Install = super::read_json(req, 32768).await?;
    let (integration, board) = state
        .db
        .dashboard(|conn| install(state, conn, input, user))?;
    if let Some((id, version)) = board {
        boards::publish(state, &id, version);
    }
    state.changed("reload");
    Ok(super::json(StatusCode::CREATED, &integration))
}

fn install(
    state: &AppState,
    conn: &Connection,
    input: Install,
    user: &Principal,
) -> Result<(IntegrationView, Option<(String, u32)>), ApiError> {
    let provider = integrations::provider(&input.integration.kind)?;
    if input.widgets.len() > provider.widgets.len()
        || !input.widgets.is_empty() && input.board_id.is_none()
    {
        return Err(ApiError::bad_request(
            "Choose a board and supported widgets.",
            Some("widgets"),
        ));
    }
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for mut selection in input.widgets {
        let widget = provider
            .widgets
            .iter()
            .find(|w| w.id == selection.id)
            .filter(|widget| seen.insert(widget.id))
            .ok_or_else(|| {
                ApiError::bad_request("Choose a supported widget once.", Some("widgets"))
            })?;
        validate_options(widget, &mut selection.options)?;
        options.push((widget.id, selection.options));
    }
    let name = input.integration.name.clone();
    let integration = integrations::save(conn, input.integration_id.as_deref(), input.integration)?;
    if input.integration_id.is_some() {
        invalidate(state, conn, Some(&integration.id))?;
        state
            .integration_checks
            .lock()
            .unwrap()
            .remove(&integration.id);
    }
    let board = if let Some(id) = input.board_id.filter(|_| !options.is_empty()) {
        if !user.can_see_board(&id) {
            return Err(ApiError::not_found());
        }
        let board = boards::get(conn, &id)?;
        let previous = boards::document(conn, &board)?;
        let mut doc = previous.clone();
        let mut section = boards::BoardSection {
            id: None,
            title: name,
            position: doc.sections.len() as u32,
            items: Vec::new(),
        };
        for (widget, options) in options {
            let widget_id = conn.query_row(
                "INSERT INTO provider_widgets (integration_id,widget,options) VALUES (?1,?2,?3)
                 ON CONFLICT(integration_id,widget) DO UPDATE SET options=excluded.options RETURNING id",
                (&integration.id, widget, options_to_json(&options)),
                |row| row.get(0),
            )?;
            if doc.sections.iter().flat_map(|section| &section.items).any(|item| matches!(
                &item.payload, boards::BoardPayload::Widget { widget_id: old, .. } if old == &widget_id
            )) {
                continue;
            }
            section.items.push(boards::BoardItem {
                id: None,
                position: section.items.len() as u32,
                spans: Default::default(),
                payload: boards::BoardPayload::Widget {
                    widget_id,
                    title: None,
                },
            });
        }
        if section.items.is_empty() {
            return Ok((integration, None));
        }
        doc.sections.push(section);
        boards::validate(&doc, &previous, user, conn, &state.config.current())?;
        boards::store(conn, &id, doc, crate::auth::now_s())?;
        Some((id, board.version.saturating_add(1)))
    } else {
        None
    };
    Ok((integration, board))
}

fn options_to_json(options: &Values) -> String {
    serde_json::to_string(options).expect("serializable widget options")
}

fn validate_options(widget: &providers::Widget, options: &mut Values) -> Result<(), ApiError> {
    providers::validate(widget.fields, options)
        .map_err(|field| ApiError::bad_request("Enter a valid widget option.", Some(&field)))?;
    (widget.validate)(options)
        .map_err(|field| ApiError::bad_request("Enter a valid widget option.", Some(field)))
}

pub(super) fn exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_widgets WHERE id=?1)",
        [id],
        |row| row.get(0),
    )
}

pub(super) fn view(state: &AppState, id: &str) -> Option<WidgetView> {
    if !id.starts_with("provider:") {
        return None;
    }
    state
        .db
        .dashboard(|conn| {
            let entry = conn
                .query_row(
                    "SELECT i.name, i.kind, json_extract(i.config,'$.icon'), pw.widget,
                    coalesce(json_extract(i.config,'$.enabled'),1)
             FROM provider_widgets pw JOIN integrations i ON i.id=pw.integration_id WHERE pw.id=?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, bool>(4)?,
                        ))
                    },
                )
                .optional()?;
            let Some((name, provider, icon, widget, enabled)) = entry else {
                return Ok(None);
            };
            let Some(provider) = providers::get(&provider) else {
                return Ok(None);
            };
            let Some(spec) = provider.widgets.iter().find(|w| w.id == widget) else {
                return Ok(None);
            };
            let enabled =
                enabled && (!provider.public_internet || dashboard_settings::internet(conn)?);
            Ok::<_, rusqlite::Error>(Some(widgets::provider_view(
                state,
                id,
                format!("{name} · {}", spec.name),
                icon.or_else(|| Some(provider.icon.into())),
                enabled,
            )))
        })
        .ok()
        .flatten()
}

pub(super) fn board_targets(
    state: &AppState,
    user: &Principal,
    board: Option<&str>,
) -> HashSet<String> {
    let Some(board) = board.filter(|id| user.can_see_board(id)) else {
        return HashSet::new();
    };
    state
        .db
        .dashboard(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT pw.id FROM board_items item
             JOIN provider_widgets pw ON pw.id=json_extract(item.config,'$.widget_id')
             WHERE item.board_id=?1 AND item.kind='widget'",
            )?;
            let rows = stmt.query_map([board], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn invalidate(
    state: &AppState,
    conn: &Connection,
    integration: Option<&str>,
) -> rusqlite::Result<()> {
    let mut stmt =
        conn.prepare("SELECT id FROM provider_widgets WHERE ?1 IS NULL OR integration_id=?1")?;
    let mut cache = state.widgets.lock().unwrap();
    for id in stmt.query_map([integration], |row| row.get::<_, String>(0))? {
        cache.remove(&id?);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct Viewers(Mutex<HashMap<String, (usize, watch::Sender<bool>)>>);

impl Viewers {
    fn active(&self) -> Vec<(String, watch::Receiver<bool>)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(id, (_, tx))| (id.clone(), tx.subscribe()))
            .collect()
    }
}

pub(super) struct Subscription {
    state: Arc<AppState>,
    ids: HashSet<String>,
}

impl Subscription {
    pub(super) fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: state.clone(),
            ids: HashSet::new(),
        }
    }

    pub(super) fn set(&mut self, ids: HashSet<String>) {
        let mut viewers = self.state.provider_viewers.0.lock().unwrap();
        for id in ids.difference(&self.ids) {
            viewers
                .entry(id.clone())
                .or_insert_with(|| (0, watch::channel(true).0))
                .0 += 1;
        }
        for id in self.ids.difference(&ids) {
            if let Some((count, tx)) = viewers.get_mut(id) {
                *count -= 1;
                if *count == 0 {
                    tx.send_replace(false);
                    viewers.remove(id);
                }
            }
        }
        self.ids = ids;
        self.state.wake.notify_one();
    }

    pub(super) fn ids(&self) -> impl Iterator<Item = &String> {
        self.ids.iter()
    }

    pub(super) fn includes(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.set(HashSet::new());
    }
}

struct Poll {
    target: Target,
    widget: &'static providers::Widget,
    options: Values,
    attempt: u64,
}

fn due(
    state: &AppState,
    id: &str,
    config: &Arc<crate::config::Config>,
    now: Instant,
) -> Option<Poll> {
    state
        .db
        .dashboard(|conn| -> Result<_, ApiError> {
            let Some((integration, widget, options)) = conn
                .query_row(
                    "SELECT integration_id,widget,options FROM provider_widgets WHERE id=?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
            else {
                return Ok(None);
            };
            let Ok((provider, target)) = integrations::target(conn, &integration) else {
                return Ok(None);
            };
            if provider.public_internet && !target.internet_enabled {
                return Ok(None);
            }
            let Some(widget) = provider.widgets.iter().find(|w| w.id == widget) else {
                return Ok(None);
            };
            let Ok(options) = serde_json::from_str::<Values>(&options) else {
                return Ok(None);
            };
            if state
                .widgets
                .lock()
                .unwrap()
                .deadline(id, providers::refresh(&options))
                .is_some_and(|at| now < at)
            {
                return Ok(None);
            }
            Ok(state.begin_widget(config, id).map(|attempt| Poll {
                target,
                widget,
                options,
                attempt,
            }))
        })
        .ok()
        .flatten()
}

/// Runs from the existing live tick. A dropped last subscription cancels the
/// request, and idle boards never enter the scheduler.
pub(crate) fn poll(
    state: &Arc<AppState>,
    config: &Arc<crate::config::Config>,
    running: &Arc<Mutex<HashSet<String>>>,
    now: Instant,
) {
    let mut active = state.provider_viewers.active();
    // IDs with no completion are served first, then the oldest completion.
    {
        let cache = state.widgets.lock().unwrap();
        active.sort_unstable_by_key(|(id, _)| cache.deadline(id, std::time::Duration::ZERO));
    }
    for (id, mut visible) in active {
        let Ok(permit) = state.provider_requests.clone().try_acquire_owned() else {
            break;
        };
        let mut running_ids = running.lock().unwrap();
        if running_ids.contains(&id) || !*visible.borrow() {
            continue;
        }
        let Some(poll) = due(state, &id, config, now) else {
            continue;
        };
        running_ids.insert(id.clone());
        let (state, config, running) = (state.clone(), config.clone(), running.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let start = Instant::now();
            let result = tokio::select! {
                biased;
                _ = visible.changed() => None,
                result = (poll.widget.fetch)(&poll.target, &config.http, &poll.options) => Some(result),
            };
            if let Some(result) = result {
                let mut output = WidgetOutput {
                    fetched_at: crate::util::unix_now(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                match result {
                    Ok(body) => output.structured = Some(body),
                    Err(error) => output.error = Some(error.class.message().into()),
                }
                state.record_widget(&config, &id, poll.attempt, Instant::now(), output);
            }
            running.lock().unwrap().remove(&id);
        });
    }
}
