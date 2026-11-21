//! Dashboard board documents and per-account edit drafts.
use std::collections::{HashMap, HashSet};

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{
    access::{Handler, Matched, Principal},
    containers::{self, ContainerRow},
    error::ApiError,
    integrations::{self, IntegrationView},
};
use crate::{http::server::Body, model::Container, state::AppState};

// A full document with the largest supported set of links must round-trip
// after the server adds section and item IDs.
const MAX_DOCUMENT: usize = 2 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub position: u32,
    pub version: u32,
    pub created: i64,
    pub updated: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardCreate {
    name: String,
    #[cfg_attr(test, ts(optional = nullable))]
    icon: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardDocument {
    pub version: u32,
    pub sections: Vec<BoardSection>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardSection {
    #[cfg_attr(test, ts(optional = nullable))]
    pub id: Option<String>,
    pub title: String,
    pub position: u32,
    pub items: Vec<BoardItem>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardItem {
    #[cfg_attr(test, ts(optional = nullable))]
    pub id: Option<String>,
    pub position: u32,
    pub spans: GridSpans,
    pub payload: BoardPayload,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GridSpans {
    mobile: u8,
    tablet: u8,
    desktop: u8,
    rows: u8,
}

impl Default for GridSpans {
    fn default() -> Self {
        Self {
            mobile: 4,
            tablet: 4,
            desktop: 4,
            rows: 1,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case", tag = "kind"))]
pub enum BoardPayload {
    Container {
        target: ContainerMatch,
        #[cfg_attr(test, ts(optional = nullable))]
        title: Option<String>,
        #[cfg_attr(test, ts(optional = nullable))]
        icon: Option<String>,
        #[cfg_attr(test, ts(optional = nullable))]
        link: Option<String>,
    },
    Link {
        title: String,
        url: String,
        #[cfg_attr(test, ts(optional = nullable))]
        icon: Option<String>,
    },
    Widget {
        widget_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        title: Option<String>,
    },
    Integration {
        integration_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        title: Option<String>,
    },
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case", tag = "by", deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case", tag = "by"))]
pub enum ContainerMatch {
    Compose { project: String, service: String },
    Name { name: String },
}

impl From<&Container> for ContainerMatch {
    fn from(c: &Container) -> Self {
        match (&c.project, &c.service) {
            (Some(project), Some(service)) if !project.is_empty() && !service.is_empty() => {
                Self::Compose {
                    project: project.clone(),
                    service: service.clone(),
                }
            }
            _ => Self::Name {
                name: c.name.clone(),
            },
        }
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardView {
    board: BoardSummary,
    document: BoardDocument,
    resolved: Vec<ResolvedItem>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedItem {
    id: String,
    missing: bool,
    container: Option<ContainerRow>,
    integration: Option<IntegrationView>,
    widget: Option<super::widgets::WidgetView>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardSuggestion {
    pub title: String,
    pub payload: BoardPayload,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardDraft {
    document: BoardDocument,
    updated: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardUpdate {
    version: u32,
    name: String,
    #[cfg_attr(test, ts(optional = nullable))]
    icon: Option<String>,
    position: u32,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardConflict {
    error: ApiError,
    current_version: u32,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardEvent {
    pub id: String,
    pub version: u32,
}

#[derive(Deserialize, Serialize)]
struct Section {
    id: String,
    title: String,
    position: u32,
}

#[derive(Deserialize, Serialize)]
struct Placement {
    section: String,
    position: u32,
    spans: GridSpans,
}

const SUMMARY: &str = "id,name,icon,position,version,created,updated";

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<BoardSummary> {
    Ok(BoardSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        icon: row.get(2)?,
        position: row.get(3)?,
        version: row.get(4)?,
        created: row.get(5)?,
        updated: row.get(6)?,
    })
}

pub(super) fn list(conn: &Connection, user: &Principal) -> Result<Vec<BoardSummary>, ApiError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUMMARY} FROM boards ORDER BY position,id"
    ))?;
    let boards = stmt
        .query_map([], summary)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(boards
        .into_iter()
        .filter(|board| user.can_see_board(&board.id))
        .collect())
}

pub(super) fn create(
    conn: &Connection,
    name: &str,
    icon: Option<&str>,
    now: i64,
) -> Result<BoardSummary, ApiError> {
    super::validate_label(name, "name")?;
    super::validate_icon(icon)?;
    let count: u32 = conn.query_row("SELECT count(*) FROM boards", [], |row| row.get(0))?;
    if count >= 64 {
        return Err(ApiError::bad_request(
            "at most 64 boards are supported",
            None,
        ));
    }
    let id: String = conn.query_row(
        "INSERT INTO boards (name,icon,position,created,updated)
         VALUES (?1,?2,(SELECT min(coalesce(max(position)+1,0),4294967295) FROM boards),?3,?3)
         RETURNING id",
        params![name, icon, now],
        |row| row.get(0),
    )?;
    get(conn, &id)
}

pub(super) fn get(conn: &Connection, id: &str) -> Result<BoardSummary, ApiError> {
    conn.query_row(
        &format!("SELECT {SUMMARY} FROM boards WHERE id=?1"),
        [id],
        summary,
    )
    .optional()?
    .ok_or_else(ApiError::not_found)
}

pub(super) fn decode<T: DeserializeOwned>(s: &str) -> Result<T, ApiError> {
    serde_json::from_str(s).map_err(|_| rusqlite::Error::InvalidQuery.into())
}

fn encode(value: &impl Serialize) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|_| rusqlite::Error::InvalidQuery.into())
}

fn document(conn: &Connection, board: &BoardSummary) -> Result<BoardDocument, ApiError> {
    let raw: String = conn.query_row(
        "SELECT sections FROM boards WHERE id=?1",
        [&board.id],
        |r| r.get(0),
    )?;
    let mut sections: Vec<_> = decode::<Vec<Section>>(&raw)?
        .into_iter()
        .map(|s| BoardSection {
            id: Some(s.id),
            title: s.title,
            position: s.position,
            items: Vec::new(),
        })
        .collect();
    let mut stmt =
        conn.prepare("SELECT id,config,layout FROM board_items WHERE board_id=?1 ORDER BY id")?;
    let mut rows = stmt.query([&board.id])?;
    while let Some(row) = rows.next()? {
        let placement: Placement = decode(&row.get::<_, String>(2)?)?;
        let section = sections
            .iter_mut()
            .find(|s| s.id.as_deref() == Some(&placement.section))
            .ok_or(rusqlite::Error::InvalidQuery)?;
        section.items.push(BoardItem {
            id: Some(row.get(0)?),
            position: placement.position,
            spans: placement.spans,
            payload: decode(&row.get::<_, String>(1)?)?,
        });
    }
    sections.sort_by_key(|s| s.position);
    for section in &mut sections {
        section.items.sort_by_key(|i| i.position);
    }
    Ok(BoardDocument {
        version: board.version,
        sections,
    })
}

fn safe_link(link: &str, field: &str) -> Result<(), ApiError> {
    let parsed = link.parse::<hyper::Uri>().ok();
    if link.len() > 2048
        || link.contains('\\')
        || !parsed.is_some_and(|uri| {
            matches!(uri.scheme_str(), Some("http" | "https"))
                && uri
                    .authority()
                    .is_some_and(|a| !a.host().is_empty() && !a.as_str().contains('@'))
        })
    {
        return Err(ApiError::bad_request(
            "enter an absolute HTTP or HTTPS URL without user information",
            Some(field),
        ));
    }
    Ok(())
}

pub(super) fn check_test_link(url: &str) -> Result<(), ApiError> {
    if crate::http::client::origin(url).is_none() {
        return Err(ApiError::bad_request(
            "Enter an HTTP or HTTPS test URL without user information, a query or a fragment.",
            Some("url"),
        ));
    }
    Ok(())
}

pub(super) fn validate(
    doc: &BoardDocument,
    existing: &BoardDocument,
    user: &Principal,
    conn: &Connection,
    config: &crate::config::Config,
) -> Result<(), ApiError> {
    if doc.sections.len() > 64 || doc.sections.iter().map(|s| s.items.len()).sum::<usize>() > 512 {
        return Err(ApiError::bad_request(
            "a board supports up to 64 sections and 512 items",
            Some("sections"),
        ));
    }
    let mut sections: HashSet<_> = existing
        .sections
        .iter()
        .filter_map(|s| s.id.as_deref())
        .collect();
    let mut items: HashMap<_, _> = existing
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|i| Some((i.id.as_deref()?, &i.payload)))
        .collect();
    let validator = PayloadValidator::new(user, conn, config)?;
    for section in &doc.sections {
        if section.id.as_deref().is_some_and(|id| !sections.remove(id)) {
            return Err(ApiError::bad_request(
                "unknown or duplicate section ID",
                Some("sections"),
            ));
        }
        super::validate_label(&section.title, "sections.title")?;
        for item in &section.items {
            let previous = match item.id.as_deref() {
                Some(id) => Some(items.remove(id).ok_or_else(|| {
                    ApiError::bad_request("unknown or duplicate item ID", Some("sections.items"))
                })?),
                None => None,
            };
            let GridSpans {
                mobile,
                tablet,
                desktop,
                rows,
            } = item.spans;
            if !(1..=4).contains(&mobile)
                || !(1..=8).contains(&tablet)
                || !(1..=12).contains(&desktop)
                || !(1..=12).contains(&rows)
            {
                return Err(ApiError::bad_request(
                    "invalid responsive grid spans",
                    Some("spans"),
                ));
            }
            validator.validate(&item.payload, previous)?;
        }
    }
    Ok(())
}

pub(super) struct PayloadValidator<'a> {
    user: &'a Principal,
    conn: &'a Connection,
    config: &'a crate::config::Config,
    visible_targets: Option<HashSet<ContainerMatch>>,
    visible_widgets: Option<HashSet<String>>,
}

impl<'a> PayloadValidator<'a> {
    pub(super) fn new(
        user: &'a Principal,
        conn: &'a Connection,
        config: &'a crate::config::Config,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            user,
            conn,
            config,
            visible_targets: user.container_targets(conn)?,
            visible_widgets: user.widget_targets(conn)?,
        })
    }

    pub(super) fn validate(
        &self,
        payload: &BoardPayload,
        previous: Option<&BoardPayload>,
    ) -> Result<(), ApiError> {
        match payload {
            BoardPayload::Container {
                target,
                title,
                icon,
                link,
            } => {
                let unchanged = matches!(previous, Some(BoardPayload::Container { target: old, .. }) if old == target);
                if !unchanged
                    && self
                        .visible_targets
                        .as_ref()
                        .is_some_and(|targets| !targets.contains(target))
                {
                    return Err(ApiError::bad_request(
                        "invalid container reference",
                        Some("target"),
                    ));
                }
                match target {
                    ContainerMatch::Compose { project, service } => {
                        super::validate_label(project, "project")?;
                        super::validate_label(service, "service")?;
                    }
                    ContainerMatch::Name { name } => super::validate_label(name, "name")?,
                }
                if let Some(title) = title {
                    super::validate_label(title, "title")?;
                }
                super::validate_icon(icon.as_deref())?;
                if let Some(link) = link {
                    safe_link(link, "link")?;
                }
            }
            BoardPayload::Link { title, url, icon } => {
                super::validate_label(title, "title")?;
                safe_link(url, "url")?;
                super::validate_icon(icon.as_deref())?;
            }
            BoardPayload::Widget { widget_id, title } => {
                if let Some(title) = title {
                    super::validate_label(title, "title")?;
                }
                super::validate_label(widget_id, "widget_id")?;
                let unchanged = matches!(previous, Some(BoardPayload::Widget { widget_id: old, .. }) if old == widget_id);
                if !unchanged
                    && (self
                        .visible_widgets
                        .as_ref()
                        .is_some_and(|targets| !targets.contains(widget_id))
                        || !self
                            .config
                            .widgets
                            .iter()
                            .any(|widget| widget.id == *widget_id))
                {
                    return Err(ApiError::bad_request(
                        "invalid widget reference",
                        Some("widget_id"),
                    ));
                }
            }
            BoardPayload::Integration {
                integration_id,
                title,
            } => {
                if let Some(title) = title {
                    super::validate_label(title, "title")?;
                }
                let unchanged = matches!(previous, Some(BoardPayload::Integration { integration_id: old, .. }) if old == integration_id);
                if !super::valid_id(integration_id)
                    || (!unchanged
                        && (!self.user.can_see_integration(integration_id)
                            || !self.conn.query_row(
                                "SELECT EXISTS (SELECT 1 FROM integrations WHERE id=?1)",
                                [integration_id],
                                |r| r.get::<_, bool>(0),
                            )?))
                {
                    return Err(ApiError::bad_request(
                        "invalid integration reference",
                        Some("integration_id"),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn next_id(conn: &Connection, existing: Option<String>) -> Result<String, ApiError> {
    match existing {
        Some(id) => Ok(id),
        None => Ok(conn.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?),
    }
}

pub(super) fn store(
    conn: &Connection,
    id: &str,
    doc: BoardDocument,
    now: i64,
) -> Result<(), ApiError> {
    conn.execute("DELETE FROM board_items WHERE board_id=?1", [id])?;
    let mut sections = Vec::with_capacity(doc.sections.len());
    for section in doc.sections {
        let section_id = next_id(conn, section.id)?;
        for item in section.items {
            let item_id = next_id(conn, item.id)?;
            let (kind, target) = match &item.payload {
                BoardPayload::Container { .. } => ("container", None),
                BoardPayload::Link { .. } => ("link", None),
                BoardPayload::Widget { .. } => ("widget", None),
                BoardPayload::Integration { integration_id, .. } => {
                    ("integration", Some(integration_id))
                }
            };
            let placement = Placement {
                section: section_id.clone(),
                position: item.position,
                spans: item.spans,
            };
            conn.execute(
                "INSERT INTO board_items (id,board_id,integration_id,kind,config,layout)
                VALUES (?1,?2,(SELECT id FROM integrations WHERE id=?3),?4,?5,?6)",
                params![
                    item_id,
                    id,
                    target,
                    kind,
                    encode(&item.payload)?,
                    encode(&placement)?
                ],
            )?;
        }
        sections.push(Section {
            id: section_id,
            title: section.title,
            position: section.position,
        });
    }
    conn.execute(
        "UPDATE boards SET sections=?2,version=version+1,updated=?3 WHERE id=?1",
        params![id, encode(&sections)?, now],
    )?;
    Ok(())
}

fn live_containers(state: &AppState, now: i64) -> Vec<(ContainerMatch, ContainerRow)> {
    let cfg = state.config.current();
    let snap = state.snapshot();
    let stats = crate::read_model::stats_with_history(state, &snap);
    let targets: HashMap<_, _> = snap
        .containers
        .iter()
        .map(|c| (c.name.as_str(), ContainerMatch::from(c)))
        .collect();
    let mut rows: Vec<_> = containers::view(&cfg, &snap, &stats, now)
        .groups
        .into_iter()
        .flat_map(|g| g.containers)
        .filter_map(|row| Some((targets.get(row.name.as_str())?.clone(), row)))
        .collect();
    rows.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    rows
}

fn resolve(
    state: &AppState,
    now: i64,
    board: BoardSummary,
    document: BoardDocument,
    integrations: Vec<IntegrationView>,
) -> BoardView {
    let config = state.config.current();
    let mut containers = HashMap::new();
    if document
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .any(|i| matches!(i.payload, BoardPayload::Container { .. }))
    {
        for (target, row) in live_containers(state, now) {
            containers.entry(target).or_insert(row);
        }
    }
    let mut resolved = Vec::new();
    for item in document.sections.iter().flat_map(|s| &s.items) {
        let mut container = None;
        let mut integration = None;
        let mut widget = None;
        let missing = match &item.payload {
            BoardPayload::Container {
                target,
                title,
                icon,
                link,
            } => {
                container = containers.get(target).cloned().map(|mut row| {
                    if let Some(title) = title {
                        row.title.clone_from(title);
                    }
                    if icon.is_some() {
                        row.icon.clone_from(icon);
                    }
                    if link.is_some() {
                        row.link.clone_from(link);
                    }
                    row
                });
                container.is_none()
            }
            BoardPayload::Widget { widget_id, .. } => {
                widget = config
                    .widgets
                    .iter()
                    .find(|w| w.id == *widget_id)
                    .map(|config| super::widgets::view(state, config));
                widget.is_none()
            }
            BoardPayload::Integration { integration_id, .. } => {
                integration = integrations
                    .iter()
                    .find(|i| i.id == *integration_id)
                    .cloned();
                integration.is_none()
            }
            BoardPayload::Link { .. } => false,
        };
        resolved.push(ResolvedItem {
            id: item.id.clone().expect("stored item ID"),
            missing,
            container,
            integration,
            widget,
        });
    }
    BoardView {
        board,
        document,
        resolved,
    }
}

pub(super) fn identity(payload: &BoardPayload) -> (u8, String) {
    match payload {
        BoardPayload::Container { target, .. } => (
            0,
            serde_json::to_string(target).expect("container identity"),
        ),
        BoardPayload::Link { url, .. } => (1, url.clone()),
        BoardPayload::Widget { widget_id, .. } => (2, widget_id.clone()),
        BoardPayload::Integration { integration_id, .. } => (3, integration_id.clone()),
    }
}

pub(super) fn suggestions(
    state: &AppState,
    now: i64,
    doc: &BoardDocument,
    integrations: Vec<IntegrationView>,
    visible: &super::access::ContainerVisibility,
) -> Vec<BoardSuggestion> {
    let cfg = state.config.current();
    let mut existing: HashSet<_> = doc
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .map(|item| identity(&item.payload))
        .collect();
    let mut out = Vec::new();
    let mut add = |title, payload| {
        if existing.insert(identity(&payload)) {
            out.push(BoardSuggestion { title, payload });
        }
    };
    for (target, row) in live_containers(state, now) {
        if visible.allows(&row.name) {
            add(
                row.title,
                BoardPayload::Container {
                    target,
                    title: None,
                    icon: None,
                    link: None,
                },
            );
        }
    }
    for b in cfg.bookmarks.iter().filter(|_| !visible.is_scoped()) {
        if safe_link(&b.url, "url").is_ok() && super::validate_label(&b.name, "title").is_ok() {
            let icon = b
                .icon
                .clone()
                .filter(|i| super::validate_icon(Some(i)).is_ok());
            add(
                b.name.clone(),
                BoardPayload::Link {
                    title: b.name.clone(),
                    url: b.url.clone(),
                    icon,
                },
            );
        }
    }
    for widget in cfg.widgets.iter().filter(|_| !visible.is_scoped()) {
        add(
            widget.title.clone().unwrap_or_else(|| widget.id.clone()),
            BoardPayload::Widget {
                widget_id: widget.id.clone(),
                title: None,
            },
        );
    }
    for integration in integrations {
        add(
            integration.name,
            BoardPayload::Integration {
                integration_id: integration.id,
                title: None,
            },
        );
    }
    out
}

fn conflict(version: u32) -> Response<Body> {
    super::json(
        StatusCode::CONFLICT,
        &BoardConflict {
            error: ApiError::new(StatusCode::CONFLICT, "conflict", "board version changed"),
            current_version: version,
        },
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LinkTestInput {
    url: String,
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    m: Matched,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    let now = crate::auth::now_s();
    let id = m.param("id").unwrap_or_default();
    let db = &state.db;
    if !id.is_empty() && !user.can_see_board(id) {
        return Err(ApiError::not_found());
    }
    let draft_key = format!("board:{id}");
    let handler = m.route.handler;
    let response = match handler {
        Handler::BoardLinkTest => {
            let LinkTestInput { url } = super::read_json(req, 8192).await?;
            check_test_link(&url)?;
            integrations::limit_test(state, user.id)?;
            super::json(
                StatusCode::OK,
                &integrations::test_connection(state, url, None).await,
            )
        }
        Handler::Boards => super::json(StatusCode::OK, &db.dashboard(|c| list(c, user))?),
        Handler::Board | Handler::BoardSuggestions => {
            let (board, doc, visible) = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                Ok((
                    board.clone(),
                    document(c, &board)?,
                    integrations::list(state, c, user)?,
                ))
            })?;
            if handler == Handler::Board {
                super::json(StatusCode::OK, &resolve(state, now, board, doc, visible))
            } else {
                super::json(
                    StatusCode::OK,
                    &suggestions(state, now, &doc, visible, &user.container_visibility(state)),
                )
            }
        }
        Handler::BoardCreate => {
            let BoardCreate { name, icon } = super::read_json(req, 8192).await?;
            let board = db.dashboard(|c| create(c, &name, icon.as_deref(), now))?;
            publish(state, &board.id, board.version);
            super::json(StatusCode::CREATED, &board)
        }
        Handler::BoardSave => {
            let input: BoardDocument = super::read_json(req, MAX_DOCUMENT).await?;
            let result = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                if board.version != input.version {
                    return Ok(Err(board.version));
                }
                validate(
                    &input,
                    &document(c, &board)?,
                    user,
                    c,
                    &state.config.current(),
                )?;
                store(c, id, input, now)?;
                c.execute(
                    "DELETE FROM drafts WHERE user_id=?1 AND key=?2",
                    params![user.id, draft_key],
                )?;
                let board = get(c, id)?;
                Ok(Ok((
                    document(c, &board)?,
                    integrations::list(state, c, user)?,
                    board,
                )))
            })?;
            match result {
                Ok((doc, visible, board)) => {
                    publish(state, id, board.version);
                    state.changed("groups");
                    super::json(StatusCode::OK, &resolve(state, now, board, doc, visible))
                }
                Err(version) => conflict(version),
            }
        }
        Handler::BoardUpdate => {
            let input: BoardUpdate = super::read_json(req, 8192).await?;
            super::validate_label(&input.name, "name")?;
            super::validate_icon(input.icon.as_deref())?;
            let result = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c,id)?;
                if board.version != input.version { return Ok(Err(board.version)); }
                c.execute("UPDATE boards SET name=?2,icon=?3,position=?4,version=version+1,updated=?5 WHERE id=?1",
                    params![id,input.name,input.icon,input.position,now])?;
                Ok(Ok(get(c,id)?))
            })?;
            match result {
                Ok(board) => {
                    publish(state, id, board.version);
                    super::json(StatusCode::OK, &board)
                }
                Err(version) => conflict(version),
            }
        }
        Handler::BoardDelete => {
            let version = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                c.execute("DELETE FROM boards WHERE id=?1", [id])?;
                crate::db::accounts::revoke_target_invites(c, "$.boards", id, now)?;
                c.execute(
                    "DELETE FROM access_grants WHERE kind='board' AND target_id=?1",
                    [id],
                )?;
                c.execute(
                    "DELETE FROM settings WHERE key='default_board' AND json_extract(value,'$')=?1",
                    [id],
                )?;
                c.execute("DELETE FROM drafts WHERE key=?1", [draft_key])?;
                Ok(board.version.saturating_add(1))
            })?;
            publish(state, id, version);
            // The removed grants prevent scoped subscribers from receiving the
            // board ID. A generic invalidation lets them refresh navigation.
            let _ = state.events.send("reload".into());
            super::no_content()
        }
        Handler::BoardDraft => {
            let draft = db.dashboard(|c| -> Result<_, ApiError> {
                get(c, id)?;
                let row: Option<(String,i64)> = c.query_row(
                    "SELECT value,updated FROM drafts WHERE user_id=?1 AND key=?2 AND updated>=?3",
                    params![user.id,draft_key,now-7*86400], |r| Ok((r.get(0)?,r.get(1)?)),
                ).optional()?;
                row.map(|(value, updated)| {
                    Ok(BoardDraft {
                        document: decode(&value)?,
                        updated,
                    })
                })
                .transpose()
            })?;
            super::json(StatusCode::OK, &draft)
        }
        Handler::BoardDraftSave => {
            let input: BoardDocument = super::read_json(req, MAX_DOCUMENT).await?;
            let result = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c,id)?;
                if board.version != input.version { return Ok(Err(board.version)); }
                validate(&input,&document(c,&board)?,user,c,&state.config.current())?;
                c.execute("INSERT INTO drafts (user_id,key,value,updated) VALUES (?1,?2,?3,?4)
                    ON CONFLICT(user_id,key) DO UPDATE SET value=excluded.value,updated=excluded.updated",
                    params![user.id,draft_key,encode(&input)?,now])?;
                Ok(Ok(BoardDraft { document:input,updated:now }))
            })?;
            match result {
                Ok(draft) => super::json(StatusCode::OK, &draft),
                Err(version) => conflict(version),
            }
        }
        Handler::BoardDraftDelete => {
            db.dashboard(|c| -> Result<_, ApiError> {
                get(c, id)?;
                c.execute(
                    "DELETE FROM drafts WHERE user_id=?1 AND key=?2",
                    params![user.id, draft_key],
                )?;
                Ok(())
            })?;
            super::no_content()
        }
        _ => unreachable!("board route"),
    };
    Ok(response)
}

pub(super) fn publish(state: &AppState, id: &str, version: u32) {
    let _ = state.events.send(format!("board:{id}:{version}").into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_validation_checks_ids_links_spans_and_targets() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.recover_owner(None, "test", 1).unwrap();
        let owner = db.accounts().unwrap().pop().unwrap();
        let original = BoardDocument {
            version: 0,
            sections: Vec::new(),
        };
        let mut board: BoardDocument = serde_json::from_value(serde_json::json!({
            "version": 0,
            "sections": [{
                "title": "Links",
                "position": 1,
                "items": [{
                    "position": 2,
                    "spans": {"mobile": 4, "tablet": 8, "desktop": 12, "rows": 1},
                    "payload": {"kind": "link", "title": "Example", "url": "https://example.test"}
                }]
            }]
        }))
        .unwrap();
        db.dashboard(|conn| -> Result<(), ApiError> {
            let config = crate::config::Config::default();
            let check = |doc: &BoardDocument| validate(doc, &original, &owner, conn, &config);
            assert!(check(&board).is_ok());
            board.sections[0].id = Some("arbitrary".into());
            assert_eq!(
                check(&board).unwrap_err().field.as_deref(),
                Some("sections")
            );
            board.sections[0].id = None;
            let item = &mut board.sections[0].items[0];
            item.spans.desktop = 13;
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("spans"));
            let item = &mut board.sections[0].items[0];
            item.spans.desktop = 12;
            item.payload = BoardPayload::Link {
                title: "Unsafe".into(),
                url: "javascript:alert(1)".into(),
                icon: None,
            };
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("url"));
            board.sections[0].items[0].payload = BoardPayload::Widget {
                widget_id: "unknown".into(),
                title: None,
            };
            assert_eq!(
                check(&board).unwrap_err().field.as_deref(),
                Some("widget_id")
            );
            board.sections[0].items[0].payload = BoardPayload::Integration {
                integration_id: "a".repeat(32),
                title: None,
            };
            assert_eq!(
                check(&board).unwrap_err().field.as_deref(),
                Some("integration_id")
            );
            Ok(())
        })
        .unwrap();
    }
}
