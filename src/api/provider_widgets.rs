//! Registry install API, board widget instances, and viewer-gated refreshes.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Instant,
};

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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
    placement: Option<String>,
    #[serde(default)]
    options: Values,
}

#[derive(Serialize)]
struct PlacementOptions {
    id: String,
    options: Values,
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    handler: Handler,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    if handler == Handler::ProviderWidgetOptions {
        let query = crate::http::query::parse_query(req.uri().query().unwrap_or_default());
        let id =
            crate::http::query::query_get(&query, "integration").ok_or_else(ApiError::not_found)?;
        let board =
            crate::http::query::query_get(&query, "board").ok_or_else(ApiError::not_found)?;
        let options = state.db.dashboard(|conn| {
            let mut stmt = conn.prepare(
                "SELECT widget, id, options FROM provider_widgets pw
                 WHERE integration_id=?1 AND board_id=?2 AND EXISTS (
                   SELECT 1 FROM board_items item WHERE item.board_id=?2 AND item.kind='widget'
                   AND json_extract(item.config,'$.widget_id')=pw.id
                 ) ORDER BY id",
            )?;
            let rows = stmt.query_map([id, board], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            let mut options = HashMap::<_, Vec<_>>::new();
            for row in rows {
                let (widget, id, value) = row?;
                if let Ok(value) = serde_json::from_str::<Values>(&value) {
                    options
                        .entry(widget)
                        .or_default()
                        .push(PlacementOptions { id, options: value });
                }
            }
            Ok::<_, rusqlite::Error>(options)
        })?;
        return Ok(super::json(StatusCode::OK, &options));
    }
    if handler == Handler::ProviderTest {
        let input: Draft = super::read_json(req, 16384).await?;
        let (provider, target) = state.db.dashboard(|conn| {
            integrations::draft_target(
                state,
                conn,
                input.integration_id.as_deref(),
                input.integration,
            )
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
    if input.widgets.len() > 512 || !input.widgets.is_empty() && input.board_id.is_none() {
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
            .filter(|widget| seen.insert((widget.id, selection.placement.clone())))
            .ok_or_else(|| {
                ApiError::bad_request("Choose a supported widget once.", Some("widgets"))
            })?;
        validate_options(widget, &mut selection.options)?;
        options.push((widget.id, selection.placement, selection.options));
    }
    let name = input.integration.name.clone();
    let changed = if let Some(id) = &input.integration_id {
        integrations::connection_changed(conn, id, &input.integration)?
    } else {
        false
    };
    let integration = integrations::save(conn, input.integration_id.as_deref(), input.integration)?;
    if options.iter().any(|(widget, _, values)| {
        providers::requires_paths(provider.id, widget, integration.has_secret, values)
    }) {
        return Err(ApiError::bad_request(
            "Choose specific JSON paths for a connection with saved credentials.",
            Some("paths"),
        ));
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
        let mut placements = Vec::new();
        let mut updated = HashSet::new();
        for (widget, placement, options) in options {
            let existing = conn
                .query_row(
                    "SELECT id FROM provider_widgets pw
                 WHERE integration_id=?1 AND board_id=?2 AND widget=?3 AND (?4 IS NULL OR id=?4)
                 AND EXISTS (
                   SELECT 1 FROM board_items item WHERE item.board_id=?2 AND item.kind='widget'
                   AND json_extract(item.config,'$.widget_id')=pw.id
                 ) ORDER BY id LIMIT 1",
                    (&integration.id, &id, widget, &placement),
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(widget_id) = existing {
                if !updated.insert(widget_id.clone()) {
                    return Err(ApiError::bad_request(
                        "Choose a widget placement once.",
                        Some("widgets"),
                    ));
                }
                conn.execute(
                    "UPDATE provider_widgets SET options=?2 WHERE id=?1",
                    (&widget_id, options_to_json(&options)),
                )?;
                continue;
            }
            if placement.is_some() {
                return Err(ApiError::bad_request(
                    "The widget placement has changed. Reload the board and try again.",
                    Some("widgets"),
                ));
            }
            let item_id = boards::next_id(conn, None)?;
            let widget_id = format!("provider:{item_id}");
            conn.execute(
                "INSERT INTO provider_widgets (id,integration_id,board_id,widget,options) VALUES (?1,?2,?3,?4,?5)",
                (&widget_id, &integration.id, &id, widget, options_to_json(&options)),
            )?;
            placements.push(item_id);
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
            None
        } else {
            doc.sections.push(section);
            boards::validate(&doc, &previous, user, conn, &state.config.current())?;
            for (item, placement) in doc
                .sections
                .last_mut()
                .expect("new widget section")
                .items
                .iter_mut()
                .zip(placements)
            {
                item.id = Some(placement);
            }
            boards::store(conn, &id, doc, crate::auth::now_s())?;
            Some((id, board.version.saturating_add(1)))
        }
    } else {
        None
    };
    if changed {
        invalidate(state, Some(&integration.id));
        state
            .integration_checks
            .lock()
            .unwrap()
            .remove(&integration.id);
    }
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

pub(super) fn placement_scope(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT board_id, integration_id FROM provider_widgets WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

/// Copies a provider widget's options when an existing widget is used in a new
/// board item. The board item ID makes the placement stable across reorders.
pub(super) fn place(
    conn: &Connection,
    board: &str,
    item: &str,
    widget_id: &mut String,
) -> rusqlite::Result<()> {
    if !widget_id.starts_with("provider:") {
        return Ok(());
    }
    let id = format!("provider:{item}");
    if *widget_id != id && conn.execute(
        "INSERT INTO provider_widgets (id,integration_id,board_id,widget,options)
         SELECT ?1,integration_id,?2,widget,options FROM provider_widgets WHERE id=?3
         ON CONFLICT(id) DO UPDATE SET integration_id=excluded.integration_id,widget=excluded.widget,options=excluded.options",
        (&id, board, &*widget_id),
    )? > 0 {
        *widget_id = id;
    }
    Ok(())
}

fn cache_key(provider: &str, integration: &str, widget: &str, options: &Values) -> String {
    let mut inputs = options.clone();
    inputs.remove("refresh");
    let material =
        serde_json::to_vec(&(provider, integration, widget, inputs)).expect("fetch inputs");
    format!(
        "provider-fetch:{integration}:{}",
        crate::auth::sha256_hex(&material)
    )
}

pub(super) fn view(
    state: &AppState,
    user: &Principal,
    board: &str,
    id: &str,
) -> Option<WidgetView> {
    if !id.starts_with("provider:") || !user.can_see_board(board) {
        return None;
    }
    state
        .db
        .dashboard(|conn| {
            let entry = conn
                .query_row(
                    "SELECT i.name, i.kind, json_extract(i.config,'$.icon'), pw.widget,
                    coalesce(json_extract(i.config,'$.enabled'),1), pw.integration_id, pw.options
             FROM provider_widgets pw JOIN integrations i ON i.id=pw.integration_id
             WHERE pw.id=?1 AND pw.board_id=?2 AND EXISTS (
                SELECT 1 FROM board_items item WHERE item.board_id=?2 AND item.kind='widget'
                AND json_extract(item.config,'$.widget_id')=pw.id
             )",
                    [id, board],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, bool>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )
                .optional()?;
            let Some((name, provider, icon, widget, enabled, integration, options)) = entry else {
                return Ok(None);
            };
            if !user.can_see_integration(&integration) {
                return Ok(None);
            }
            let Some(provider) = providers::get(&provider) else {
                return Ok(None);
            };
            let Some(spec) = provider.widgets.iter().find(|w| w.id == widget) else {
                return Ok(None);
            };
            let enabled =
                enabled && (!provider.public_internet || dashboard_settings::internet(conn)?);
            let options = serde_json::from_str::<Values>(&options)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let key = cache_key(provider.id, &integration, spec.id, &options);
            Ok::<_, rusqlite::Error>(Some(widgets::provider_view(
                state,
                id,
                &key,
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
                "SELECT DISTINCT pw.id, pw.integration_id FROM board_items item
             JOIN provider_widgets pw ON pw.id=json_extract(item.config,'$.widget_id')
             WHERE item.board_id=?1 AND pw.board_id=?1 AND item.kind='widget'",
            )?;
            let rows = stmt.query_map([board], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut targets = HashSet::new();
            for row in rows {
                let (id, integration) = row?;
                if user.can_see_integration(&integration) {
                    targets.insert(id);
                }
            }
            Ok::<_, rusqlite::Error>(targets)
        })
        .unwrap_or_default()
}

pub(super) fn invalidate(state: &AppState, integration: Option<&str>) {
    state.provider_connections.invalidate(integration);
    let prefix = integration.map_or_else(
        || "provider-fetch:".into(),
        |id| format!("provider-fetch:{id}:"),
    );
    state.widgets.lock().unwrap().remove_prefix(&prefix);
}

pub(crate) struct Viewers {
    ids: Mutex<HashMap<String, usize>>,
    changed: watch::Sender<()>,
}

impl Default for Viewers {
    fn default() -> Self {
        Self {
            ids: Mutex::default(),
            changed: watch::channel(()).0,
        }
    }
}

impl Viewers {
    fn active(&self) -> Vec<String> {
        self.ids.lock().unwrap().keys().cloned().collect()
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
        if ids == self.ids {
            return;
        }
        let viewers = &self.state.provider_viewers;
        let mut counts = viewers.ids.lock().unwrap();
        for id in ids.difference(&self.ids) {
            *counts.entry(id.clone()).or_default() += 1;
        }
        for id in self.ids.difference(&ids) {
            let count = counts.get_mut(id).expect("subscribed placement");
            *count -= 1;
            if *count == 0 {
                counts.remove(id);
            }
        }
        viewers.changed.send_replace(());
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
    key: String,
    provider: &'static str,
    integration: String,
    target: Target,
    widget: &'static providers::Widget,
    options: Values,
}

fn due(state: &AppState, id: &str, now: Instant) -> Option<Poll> {
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
            let Ok((provider, target)) = integrations::target(state, conn, &integration) else {
                return Ok(None);
            };
            if provider.public_internet && !target.internet_enabled {
                return Ok(None);
            }
            let Some(widget) = provider.widgets.iter().find(|w| w.id == widget) else {
                return Ok(None);
            };
            let Ok(mut options) = serde_json::from_str::<Values>(&options) else {
                return Ok(None);
            };
            let key = cache_key(provider.id, &integration, widget.id, &options);
            if state
                .widgets
                .lock()
                .unwrap()
                .deadline(&key, providers::refresh(&options))
                .is_some_and(|at| now < at)
            {
                return Ok(None);
            }
            options.remove("refresh");
            Ok(Some(Poll {
                key,
                provider: provider.id,
                integration,
                target,
                widget,
                options,
            }))
        })
        .ok()
        .flatten()
}

fn matching_placements(conn: &Connection, poll: &Poll) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT id,options FROM provider_widgets WHERE integration_id=?1 AND widget=?2")?;
    let rows = stmt.query_map((&poll.integration, poll.widget.id), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, options) = row?;
        if serde_json::from_str(&options).is_ok_and(|options| {
            cache_key(poll.provider, &poll.integration, poll.widget.id, &options) == poll.key
        }) {
            ids.push(id);
        }
    }
    Ok(ids)
}

async fn unviewed(state: &AppState, poll: &Poll, mut changed: watch::Receiver<()>) {
    loop {
        let placements = state
            .db
            .dashboard(|conn| matching_placements(conn, poll))
            .unwrap_or_default();
        let viewed = {
            let active = state.provider_viewers.ids.lock().unwrap();
            placements.iter().any(|id| active.contains_key(id))
        };
        if !viewed {
            return;
        }
        if changed.changed().await.is_err() {
            return;
        }
    }
}

/// Runs from the existing live tick. A dropped last subscription cancels the
/// request, and idle boards never enter the scheduler.
pub(crate) fn poll(
    state: &Arc<AppState>,
    config: &Arc<crate::config::Config>,
    running: &Arc<Mutex<HashSet<String>>>,
    now: Instant,
) {
    let (current, revision) = state.config.provider_http();
    if !Arc::ptr_eq(config, &current) {
        return;
    }
    let visible = state.provider_viewers.changed.subscribe();
    for id in state.provider_viewers.active() {
        let Ok(permit) = state.provider_requests.clone().try_acquire_owned() else {
            break;
        };
        let Some(poll) = due(state, &id, now) else {
            continue;
        };
        let mut running_ids = running.lock().unwrap();
        if running_ids.contains(&poll.key) {
            continue;
        }
        let Some(attempt) = state.begin_widget(config, &poll.key) else {
            continue;
        };
        running_ids.insert(poll.key.clone());
        let (state, config, running) = (state.clone(), config.clone(), running.clone());
        let revision = revision.clone();
        let visible = visible.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            let result = tokio::select! {
                biased;
                _ = unviewed(&state, &poll, visible) => None,
                result = async {
                    let conn = providers::Conn::new(&poll.target, &config.http, &state.provider_connections, permit)?
                        .with_http_revision(revision);
                    providers::fetch(&conn, poll.widget, &poll.options).await
                } => Some(result),
            };
            if let Some(result) = result.filter(|_| poll.target.revision.check().is_ok()) {
                let mut output = WidgetOutput {
                    fetched_at: crate::util::unix_now(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                match result {
                    Ok(body) => output.structured = Some(body),
                    Err(error) => output.error = Some(error.class.message().into()),
                }
                let _ = state.db.dashboard(|conn| -> rusqlite::Result<()> {
                    if poll.target.revision.check().is_err() {
                        return Ok(());
                    }
                    if state.record_widget(&config, &poll.key, attempt, Instant::now(), output) {
                        let ids = matching_placements(conn, &poll)?;
                        let active = state.provider_viewers.ids.lock().unwrap();
                        for id in ids.into_iter().filter(|id| active.contains_key(id)) {
                            state.changed(&format!("w-{id}"));
                        }
                    }
                    Ok(())
                });
            }
            running.lock().unwrap().remove(&poll.key);
        });
    }
}
