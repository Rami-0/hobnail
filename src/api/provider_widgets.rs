//! Registry install API, board widget instances, and viewer-gated refreshes.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
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
    /// Saved placement IDs of this integration on `board_id` to take off the
    /// board, in the same board revision as the selection.
    #[serde(default)]
    remove: Vec<String>,
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

fn check_integration(user: &Principal, id: Option<&str>) -> Result<(), ApiError> {
    if user.is_scoped() && !id.is_some_and(|id| user.can_see_integration(id)) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "insufficient role",
        ));
    }
    Ok(())
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
        check_integration(user, Some(id))?;
        if !user.can_see_board(board) {
            return Err(ApiError::not_found());
        }
        let options = state.db.dashboard(|conn| {
            let mut stmt = conn.prepare(
                "SELECT pw.widget, pw.id, pw.options, i.kind FROM provider_widgets pw
                 JOIN integrations i ON i.id=pw.integration_id
                 WHERE pw.integration_id=?1 AND pw.board_id=?2 AND EXISTS (
                   SELECT 1 FROM board_items item WHERE item.board_id=?2 AND item.kind='widget'
                   AND json_extract(item.config,'$.widget_id')=pw.id
                 ) ORDER BY pw.id",
            )?;
            let rows = stmt.query_map([id, board], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            let mut options = HashMap::<_, Vec<_>>::new();
            for row in rows {
                let (widget, id, value, kind) = row?;
                if let Ok(mut value) = serde_json::from_str::<Values>(&value) {
                    if let Some((provider, spec)) = providers::get(&kind)
                        .and_then(|provider| {
                            provider
                                .widgets
                                .iter()
                                .find(|spec| spec.id == widget)
                                .map(|spec| (provider, spec))
                        })
                        .filter(|(provider, _)| provider.public_internet)
                    {
                        let seconds = spec.refresh(&value, provider.public_internet).as_secs();
                        value.insert("refresh".into(), seconds.into());
                    }
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
        check_integration(user, input.integration_id.as_deref())?;
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
    check_integration(user, input.integration_id.as_deref())?;
    let (integration, board) = state
        .db
        .dashboard(|conn| install(state, conn, input, user))?;
    if let Some((id, version)) = board {
        boards::publish(state, &id, version);
    }
    placements_changed(state);
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
    let placing = !input.widgets.is_empty() || !input.remove.is_empty();
    if input.widgets.len() > 512 || input.remove.len() > 512 || placing && input.board_id.is_none()
    {
        return Err(ApiError::bad_request(
            "Choose a board and supported widgets.",
            Some("widgets"),
        ));
    }
    if placing
        && input
            .board_id
            .as_ref()
            .is_some_and(|id| !user.can_see_board(id))
    {
        return Err(ApiError::not_found());
    }
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for (index, mut selection) in input.widgets.into_iter().enumerate() {
        let widget = provider
            .widgets
            .iter()
            .find(|w| w.id == selection.id)
            .filter(|widget| seen.insert((widget.id, selection.placement.clone())))
            .ok_or_else(|| {
                ApiError::bad_request("Choose a supported widget once.", Some("widgets"))
            })?;
        validate_options(widget, &mut selection.options).map_err(|mut error| {
            error.field = error.field.map(|field| format!("widgets.{index}.{field}"));
            error
        })?;
        options.push((widget.id, selection.placement, selection.options));
    }
    let changed = if let Some(id) = &input.integration_id {
        integrations::connection_changed(conn, id, &input.integration)?
    } else {
        false
    };
    let integration = integrations::save(conn, input.integration_id.as_deref(), input.integration)?;
    if let Some(index) = options.iter().position(|(widget, _, values)| {
        providers::requires_paths(provider.id, widget, integration.has_secret, values)
    }) {
        return Err(ApiError::bad_request(
            "Choose specific JSON paths for a connection with saved credentials.",
            Some(&format!("widgets.{index}.paths")),
        ));
    }
    let remove: HashSet<String> = input.remove.into_iter().collect();
    if options
        .iter()
        .any(|(_, placement, _)| placement.as_ref().is_some_and(|id| remove.contains(id)))
    {
        return Err(ApiError::bad_request(
            "Choose a widget placement once.",
            Some("widgets"),
        ));
    }
    let board = if let Some(id) = input.board_id.filter(|_| placing) {
        let board = boards::get(conn, &id)?;
        let previous = boards::document(conn, &board)?;
        let mut doc = previous.clone();
        for widget_id in &remove {
            let owned = conn
                .query_row(
                    "SELECT 1 FROM provider_widgets WHERE id=?1 AND integration_id=?2 AND board_id=?3",
                    (widget_id, &integration.id, &id),
                    |_| Ok(()),
                )
                .optional()?;
            if owned.is_none() {
                return Err(ApiError::bad_request(
                    "The widget placement has changed. Reload the board and try again.",
                    Some("widgets"),
                ));
            }
        }
        let mut removed = false;
        for section in &mut doc.sections {
            section.items.retain(|item| {
                let gone = matches!(&item.payload, boards::BoardPayload::Widget { widget_id, .. } if remove.contains(widget_id));
                removed |= gone;
                !gone
            });
        }
        let mut section = boards::BoardSection {
            id: None,
            title: "Widgets".into(),
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
        if section.items.is_empty() && !removed {
            None
        } else {
            let added = !section.items.is_empty();
            if added {
                doc.sections.push(section);
            }
            boards::validate(&doc, &previous, user, conn, &state.config.current())?;
            if added {
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

pub(super) struct PlacementSource {
    integration: String,
    widget: String,
    options: String,
}

pub(super) fn placement_source(
    conn: &Connection,
    board: &str,
    widget_id: &str,
) -> rusqlite::Result<Option<PlacementSource>> {
    conn.query_row(
        "SELECT integration_id,widget,options FROM provider_widgets WHERE board_id=?1 AND id=?2",
        (board, widget_id),
        |row| {
            Ok(PlacementSource {
                integration: row.get(0)?,
                widget: row.get(1)?,
                options: row.get(2)?,
            })
        },
    )
    .optional()
}

/// Copy from the pre-save snapshot. Another item may already have overwritten
/// the original placement while saving this document.
pub(super) fn place(
    conn: &Connection,
    board: &str,
    item: &str,
    widget_id: &mut String,
    source: &PlacementSource,
) -> rusqlite::Result<()> {
    let id = format!("provider:{item}");
    if *widget_id != id {
        conn.execute(
            "INSERT INTO provider_widgets (id,integration_id,board_id,widget,options)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET integration_id=excluded.integration_id,widget=excluded.widget,options=excluded.options",
            (&id, &source.integration, board, &source.widget, &source.options),
        )?;
        *widget_id = id;
    }
    Ok(())
}

pub(super) fn placements_changed(state: &AppState) {
    let _ = state.db.dashboard(|conn| -> rusqlite::Result<()> {
        let mut cache = state.widgets.lock().unwrap();
        let attempts = cache.provider_attempts();
        if attempts.is_empty() {
            return Ok(());
        }
        let mut stmt = conn.prepare(
            "SELECT i.kind, pw.integration_id, pw.widget, pw.options
             FROM board_items item JOIN provider_widgets pw
               ON item.board_id=pw.board_id AND json_extract(item.config,'$.widget_id')=pw.id
             JOIN integrations i ON pw.integration_id=i.id WHERE item.kind='widget'",
        )?;
        let mut rows = stmt.query([])?;
        let mut live = HashSet::new();
        while let Some(row) = rows.next()? {
            let raw: String = row.get(3)?;
            if let Ok(options) = serde_json::from_str::<Values>(&raw) {
                let key = cache_key(
                    &row.get::<_, String>(0)?,
                    &row.get::<_, String>(1)?,
                    &row.get::<_, String>(2)?,
                    &options,
                );
                if attempts.contains_key(&key) {
                    live.insert(key);
                }
            }
        }
        attempts.retain(|key, _| live.contains(key));
        attempts.shrink_to_fit();
        Ok(())
    });
    state.provider_viewers.reschedule();
    state.provider_viewers.changed.send_replace(());
    state.wake.notify_one();
}

pub(super) fn cache_key(
    provider: &str,
    integration: &str,
    widget: &str,
    options: &Values,
) -> String {
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

fn fetch_prefix(integration: Option<&str>) -> String {
    integration.map_or_else(
        || "provider-fetch:".into(),
        |id| format!("provider-fetch:{id}:"),
    )
}

/// An integration's connection inputs changed (or, with `None`, a setting
/// every provider reads). Its widgets keep their last data, marked stale,
/// and non-public ones refetch on the next tick.
pub(crate) fn invalidate(state: &AppState, integration: Option<&str>) {
    state.provider_connections.invalidate(integration);
    state.provider_viewers.reschedule();
    state
        .widgets
        .lock()
        .unwrap()
        .invalidate_providers(&fetch_prefix(integration));
}

/// The integration was deleted: drop its widgets' data.
pub(super) fn forget(state: &AppState, integration: &str) {
    state.provider_connections.invalidate(Some(integration));
    state.provider_viewers.reschedule();
    state
        .widgets
        .lock()
        .unwrap()
        .remove_providers(&fetch_prefix(Some(integration)));
}

pub(crate) struct Viewers {
    ids: Mutex<HashMap<String, usize>>,
    changed: watch::Sender<()>,
    /// Per viewed placement, what the last [`due`] found, so a placement
    /// that is not yet due costs no database read on the per-second tick.
    /// Cleared whenever placements, integrations or the policy change.
    schedule: Mutex<HashMap<String, Scheduled>>,
    /// Where the next tick starts in the viewed placements, so placements
    /// late in the order are not starved when request slots run out.
    cursor: AtomicUsize,
}

#[derive(Clone)]
enum Scheduled {
    Fetch { key: String, refresh: Duration },
    InternetOff,
}

impl Default for Viewers {
    fn default() -> Self {
        Self {
            ids: Mutex::default(),
            changed: watch::channel(()).0,
            schedule: Mutex::default(),
            cursor: AtomicUsize::new(0),
        }
    }
}

impl Viewers {
    /// The viewed placements in a stable order, rotated by one each call.
    fn active(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.ids.lock().unwrap().keys().cloned().collect();
        ids.sort_unstable();
        self.schedule
            .lock()
            .unwrap()
            .retain(|id, _| ids.binary_search(id).is_ok());
        if !ids.is_empty() {
            let start = self.cursor.fetch_add(1, Ordering::Relaxed) % ids.len();
            ids.rotate_left(start);
        }
        ids
    }

    fn reschedule(&self) {
        self.schedule.lock().unwrap().clear();
    }

    fn remember(&self, id: &str, scheduled: Scheduled) {
        self.schedule.lock().unwrap().insert(id.into(), scheduled);
    }

    /// Whether the last check of `id` says it is not due yet.
    fn waiting(&self, state: &AppState, id: &str, now: Instant) -> bool {
        let scheduled = self.schedule.lock().unwrap().get(id).cloned();
        match scheduled {
            Some(Scheduled::InternetOff) => true,
            Some(Scheduled::Fetch { key, refresh }) => state
                .widgets
                .lock()
                .unwrap()
                .deadline(&key, refresh)
                .is_some_and(|at| now < at),
            None => false,
        }
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
    public: bool,
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
                state.provider_viewers.remember(id, Scheduled::InternetOff);
                return Ok(None);
            }
            let Some(widget) = provider.widgets.iter().find(|w| w.id == widget) else {
                return Ok(None);
            };
            let Ok(mut options) = serde_json::from_str::<Values>(&options) else {
                return Ok(None);
            };
            let key = cache_key(provider.id, &integration, widget.id, &options);
            // A user-URL provider (feed, HTTP check) aimed at a public host
            // gets the public floor once a fetch has resolved it publicly.
            let public =
                provider.public_internet || state.widgets.lock().unwrap().attempt_public(&key);
            let refresh = widget.refresh(&options, public);
            state.provider_viewers.remember(
                id,
                Scheduled::Fetch {
                    key: key.clone(),
                    refresh,
                },
            );
            if state
                .widgets
                .lock()
                .unwrap()
                .deadline(&key, refresh)
                .is_some_and(|at| now < at)
            {
                return Ok(None);
            }
            options.remove("refresh");
            Ok(Some(Poll {
                key,
                provider: provider.id,
                public: provider.public_internet,
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
    let mut stmt = conn.prepare(
        "SELECT id,options FROM provider_widgets pw WHERE integration_id=?1 AND widget=?2
             AND EXISTS (SELECT 1 FROM board_items item
                 WHERE item.board_id=pw.board_id AND item.kind='widget'
                 AND json_extract(item.config,'$.widget_id')=pw.id)",
    )?;
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
        if state.provider_viewers.waiting(state, &id, now) {
            continue;
        }
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
        let attempt = state.db.dashboard(|conn| -> rusqlite::Result<_> {
            if poll.target.revision.check().is_err() || matching_placements(conn, &poll)?.is_empty()
            {
                return Ok(None);
            }
            let mut cache = state.widgets.lock().unwrap();
            Ok(Arc::ptr_eq(config, &state.config.current())
                .then(|| cache.begin_provider(&poll.key, now, poll.public)))
        });
        let Ok(Some(attempt)) = attempt else {
            continue;
        };
        running_ids.insert(poll.key.clone());
        let (state, config, running) = (state.clone(), config.clone(), running.clone());
        let revision = revision.clone();
        let visible = visible.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            let reached_public = std::sync::atomic::AtomicBool::new(false);
            let result = tokio::select! {
                biased;
                _ = unviewed(&state, &poll, visible) => None,
                result = async {
                    let conn = providers::Conn::new(&poll.target, &config.http, &state.provider_connections, permit)?
                        .with_http_revision(revision);
                    let result = providers::fetch(&conn, poll.widget, &poll.options).await;
                    if conn.reached_public() {
                        reached_public.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    result
                } => Some(result),
            };
            if reached_public.into_inner() {
                state.widgets.lock().unwrap().mark_public(&poll.key);
            }
            let cancelled = result.is_none();
            if let Some(result) = result.filter(|_| poll.target.revision.check().is_ok()) {
                let mut output = WidgetOutput {
                    fetched_at: crate::util::unix_now(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                match result {
                    Ok(body) => output.structured = Some(Arc::new(body)),
                    Err(error) => output.error = Some(error.class.message().into()),
                }
                let _ = state.db.dashboard(|conn| -> rusqlite::Result<()> {
                    if poll.target.revision.check().is_err() {
                        return Ok(());
                    }
                    let ids = matching_placements(conn, &poll)?;
                    let active = state.provider_viewers.ids.lock().unwrap();
                    let mut viewed = ids
                        .into_iter()
                        .filter(|id| active.contains_key(id))
                        .peekable();
                    if viewed.peek().is_some()
                        && state.record_widget(&config, &poll.key, attempt, Instant::now(), output)
                    {
                        for id in viewed {
                            state.changed(&format!("w-{id}"));
                        }
                    }
                    Ok(())
                });
            }
            running.lock().unwrap().remove(&poll.key);
            if cancelled {
                state.wake.notify_one();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tick_starts_one_placement_later() {
        let viewers = Viewers::default();
        viewers
            .ids
            .lock()
            .unwrap()
            .extend(["c", "a", "b"].map(|id| (id.to_string(), 1)));
        let firsts: Vec<_> = (0..4).map(|_| viewers.active()[0].clone()).collect();
        assert_eq!(firsts, ["a", "b", "c", "a"]);
        assert_eq!(viewers.active(), ["b", "c", "a"]);
    }

    #[test]
    fn a_placement_that_is_not_due_skips_the_database() {
        let state = crate::testutil::state_with(vec![], "");
        let viewers = &state.provider_viewers;
        let now = Instant::now();
        let key = "provider-fetch:fixture:key";
        let refresh = Duration::from_secs(60);
        assert!(
            !viewers.waiting(&state, "provider:one", now),
            "unknown: check it"
        );
        viewers.remember(
            "provider:one",
            Scheduled::Fetch {
                key: key.into(),
                refresh,
            },
        );
        assert!(
            !viewers.waiting(&state, "provider:one", now),
            "never fetched"
        );
        state
            .widgets
            .lock()
            .unwrap()
            .begin_provider(key, now, false);
        assert!(viewers.waiting(&state, "provider:one", now));
        assert!(!viewers.waiting(&state, "provider:one", now + refresh));
        viewers.remember("provider:two", Scheduled::InternetOff);
        assert!(viewers.waiting(&state, "provider:two", now));
        // Placement, integration or policy changes force a fresh check.
        viewers.reschedule();
        assert!(!viewers.waiting(&state, "provider:one", now));
        assert!(!viewers.waiting(&state, "provider:two", now));
        // Placements nobody views any more are forgotten.
        viewers.remember("provider:gone", Scheduled::InternetOff);
        viewers.active();
        assert!(viewers.schedule.lock().unwrap().is_empty());
    }
}
