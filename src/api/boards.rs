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
use crate::{board_grid, http::server::Body, model::Container, state::AppState};

// A full document with the largest supported set of links must round-trip
// after the server adds section and item IDs.
const MAX_DOCUMENT: usize = 2 * 1024 * 1024;

// Reserve most of the response for the editable document and resolved metadata.
const MAX_BOARD_RESPONSE: usize = 4 * 1024 * 1024;
const BOARD_WIDGET_BYTES: usize = 512 * 1024;

#[derive(Default)]
pub(super) struct JsonSize(pub usize);

impl std::io::Write for JsonSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        if self.0 > MAX_BOARD_RESPONSE {
            return Err(std::io::Error::other(
                "board response exceeded its size limit",
            ));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn board_response(mut view: BoardView) -> Result<Response<Body>, ApiError> {
    let mut size = JsonSize::default();
    if serde_json::to_writer(&mut size, &view).is_err() {
        for item in &mut view.resolved {
            if let Some(widget) = &mut item.widget {
                widget.omit_board_body();
            }
        }
        size = JsonSize::default();
        serde_json::to_writer(&mut size, &view).map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "board_too_large",
                "Board metadata exceeds the 4 MiB response limit.",
            )
        })?;
    }
    let mut bytes = Vec::with_capacity(size.0);
    serde_json::to_writer(&mut bytes, &view).expect("board view serialized during sizing");
    Ok(super::json_bytes(StatusCode::OK, bytes))
}

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

/// A board's whole editable content. Save writes all of it as one revision:
/// the name and icon, the appearance and every section and item. Reads
/// always carry `name`; a saved document without one keeps the board's name
/// and icon, and one with a `name` sets both (no `icon` clears it).
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardDocument {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub icon: Option<String>,
    #[serde(default)]
    pub appearance: BoardAppearance,
    pub sections: Vec<BoardSection>,
}

/// Board-level look. Every field is a closed preset, so the stored values
/// can be rendered without any author-supplied CSS or URL.
#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardAppearance {
    #[serde(default)]
    pub background: BoardBackground,
    #[serde(default)]
    pub density: BoardDensity,
    #[serde(default)]
    pub surface: TileSurface,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub accent: Option<Accent>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum BoardBackground {
    #[default]
    None,
    Dawn,
    Dusk,
    Aurora,
    Ocean,
    Forest,
    Graphite,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum BoardDensity {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum TileSurface {
    #[default]
    Solid,
    Glass,
    Outline,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum Accent {
    Blue,
    Violet,
    Green,
    Amber,
    Rose,
    Teal,
    Slate,
}

/// A titled group of items with its own 12-column canvas.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardSection {
    #[cfg_attr(test, ts(optional = nullable))]
    pub id: Option<String>,
    pub title: String,
    pub position: u32,
    /// Shown folded until a reader opens it.
    #[serde(default)]
    pub collapsed: bool,
    pub items: Vec<BoardItem>,
}

impl BoardSection {
    pub fn new(title: impl Into<String>, position: u32) -> Self {
        Self {
            id: None,
            title: title.into(),
            position,
            collapsed: false,
            items: Vec::new(),
        }
    }

    fn rects(&self) -> Vec<board_grid::Rect> {
        self.items.iter().map(BoardItem::rect).collect()
    }

    /// Add an item of its kind's default size in the first free slot.
    pub fn append(&mut self, payload: BoardPayload) {
        let (w, h) = board_grid::default_size(payload.kind());
        self.append_sized(payload, w, h);
    }

    /// Add an item of `w` × `h` cells in the first free slot.
    pub fn append_sized(&mut self, payload: BoardPayload, w: u16, h: u16) {
        let rect = board_grid::first_free(&self.rects(), w, h);
        self.items.push(BoardItem {
            id: None,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            look: None,
            payload,
        });
    }
}

/// One tile: a rectangle on its section's canvas (`x` 0–11, `w` 1–12,
/// `h` 1–16 rows) and what it shows.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardItem {
    #[cfg_attr(test, ts(optional = nullable))]
    pub id: Option<String>,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub look: Option<TileLook>,
    pub payload: BoardPayload,
}

impl BoardItem {
    pub fn rect(&self) -> board_grid::Rect {
        board_grid::Rect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        }
    }
}

/// Per-tile presentation. `icon` and `link` override what the target
/// supplies (container and link payloads keep their own fields too).
#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TileLook {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub accent: Option<Accent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub link: Option<String>,
    #[serde(default)]
    pub hide_status: bool,
    #[serde(default)]
    pub hide_metrics: bool,
}

impl TileLook {
    fn is_plain(&self) -> bool {
        *self == Self::default()
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

impl BoardPayload {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Container { .. } => "container",
            Self::Link { .. } => "link",
            Self::Widget { .. } => "widget",
            Self::Integration { .. } => "integration",
        }
    }
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
    container: Option<Box<ContainerRow>>,
    integration: Option<Box<IntegrationView>>,
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
    // Empty means the migration identified a candidate with ambiguous origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_provider_title: Option<String>,
    title: String,
    position: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    collapsed: bool,
}

// Stored in `board_items.layout`; the payload is in `config`.
#[derive(Deserialize, Serialize)]
struct Placement {
    section: String,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    look: Option<TileLook>,
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
    serde_json::from_slice(s.as_bytes()).map_err(|_| rusqlite::Error::InvalidQuery.into())
}

fn encode(value: &impl Serialize) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|_| rusqlite::Error::InvalidQuery.into())
}

fn sections(conn: &Connection, board: &str) -> Result<Vec<Section>, ApiError> {
    let raw: String = conn.query_row("SELECT sections FROM boards WHERE id=?1", [board], |r| {
        r.get(0)
    })?;
    decode(&raw)
}

pub(super) fn document(conn: &Connection, board: &BoardSummary) -> Result<BoardDocument, ApiError> {
    let raw: String = conn.query_row(
        "SELECT appearance FROM boards WHERE id=?1",
        [&board.id],
        |r| r.get(0),
    )?;
    let appearance = decode(&raw)?;
    let mut sections: Vec<_> = sections(conn, &board.id)?
        .into_iter()
        .map(|s| BoardSection {
            id: Some(s.id),
            title: s.title,
            position: s.position,
            collapsed: s.collapsed,
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
            x: placement.x,
            y: placement.y,
            w: placement.w,
            h: placement.h,
            look: placement.look,
            payload: decode(&row.get::<_, String>(1)?)?,
        });
    }
    sections.sort_by_key(|s| s.position);
    for section in &mut sections {
        section.items.sort_by_key(|i| (i.y, i.x));
    }
    Ok(BoardDocument {
        version: board.version,
        name: Some(board.name.clone()),
        icon: board.icon.clone(),
        appearance,
        sections,
    })
}

fn link_key(link: &str) -> Option<String> {
    if link.len() > 2048 || link.contains('\\') {
        return None;
    }
    let (scheme, rest) = link.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(split);
    let parsed: hyper::Uri = format!("{scheme}://{rest}").parse().ok()?;
    let host = parsed.host()?;
    if host.is_empty() || authority.contains('@') {
        return None;
    }
    let port = parsed.port_u16();
    let authority = match port {
        Some(port) if port == default => host.to_ascii_lowercase(),
        Some(port) => format!("{}:{port}", host.to_ascii_lowercase()),
        None => authority.to_ascii_lowercase(),
    };
    let slash = if suffix.starts_with('/') { "" } else { "/" };
    // The HTTP URI parser drops the fragment. Preserve the original suffix,
    // including its path spelling, query order and fragment.
    Some(format!("{scheme}://{authority}{slash}{suffix}"))
}

fn safe_link(link: &str, field: &str) -> Result<(), ApiError> {
    if link_key(link).is_none() {
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
    // Grandfather duplicate links already saved on a board, but reject
    // new duplicates regardless of which section comes first.
    let unchanged_link = |item: &BoardItem, previous: Option<&BoardPayload>| {
        matches!((&item.payload, previous), (BoardPayload::Link { url, .. }, Some(BoardPayload::Link { url: old, .. }))
            if link_key(url) == link_key(old))
    };
    let mut links: HashSet<_> = doc
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .filter(|item| {
            unchanged_link(
                item,
                item.id.as_deref().and_then(|id| items.get(id).copied()),
            )
        })
        .map(|item| identity(&item.payload))
        .collect();
    if let Some(name) = &doc.name {
        super::validate_label(name, "name")?;
        super::validate_icon(doc.icon.as_deref())?;
    } else if doc.icon.is_some() {
        return Err(ApiError::bad_request(
            "an icon needs the board name",
            Some("icon"),
        ));
    }
    let validator = PayloadValidator::new(user, conn, config)?;
    for section in &doc.sections {
        layout(section)?;
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
            if let Some(look) = &item.look {
                super::validate_icon(look.icon.as_deref())?;
                if let Some(link) = &look.link {
                    safe_link(link, "look.link")?;
                }
            }
            validator.validate(&item.payload, previous)?;
            if matches!(item.payload, BoardPayload::Link { .. })
                && !unchanged_link(item, previous)
                && !links.insert(identity(&item.payload))
            {
                return Err(ApiError::bad_request(
                    "This link is already on the board.",
                    Some("url"),
                ));
            }
        }
    }
    Ok(())
}

/// Every item inside the canvas and none on top of another.
fn layout(section: &BoardSection) -> Result<(), ApiError> {
    board_grid::compact(&mut section.rects()).map_err(|invalid| {
        let message = match invalid {
            board_grid::Invalid::Bounds(_) => {
                "Items must fit the 12-column grid (width 1-12, height 1-16)."
            }
            board_grid::Invalid::Overlap(_) => "Board items must not overlap.",
        };
        ApiError::bad_request(message, Some("layout"))
    })
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
                if !unchanged {
                    let allowed = if widget_id.starts_with("provider:") {
                        super::provider_widgets::placement_scope(self.conn, widget_id)?.is_some_and(
                            |(board, integration)| {
                                self.user.can_see_board(&board)
                                    && self.user.can_see_integration(&integration)
                            },
                        )
                    } else {
                        self.visible_widgets
                            .as_ref()
                            .is_none_or(|targets| targets.contains(widget_id))
                            && self
                                .config
                                .widgets
                                .iter()
                                .any(|widget| widget.id == *widget_id)
                    };
                    if !allowed {
                        return Err(ApiError::bad_request(
                            "invalid widget reference",
                            Some("widget_id"),
                        ));
                    }
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

pub(super) fn next_id(conn: &Connection, existing: Option<String>) -> Result<String, ApiError> {
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
    let legacy_titles: HashMap<_, _> = sections(conn, id)?
        .into_iter()
        .filter_map(|section| Some((section.id, (section.title, section.legacy_provider_title?))))
        .collect();
    let mut sources = HashMap::new();
    for item in doc.sections.iter().flat_map(|section| &section.items) {
        if let BoardPayload::Widget { widget_id, .. } = &item.payload {
            if widget_id.starts_with("provider:") && !sources.contains_key(widget_id) {
                if let Some(source) =
                    super::provider_widgets::placement_source(conn, id, widget_id)?
                {
                    sources.insert(widget_id.clone(), source);
                }
            }
        }
    }
    conn.execute("DELETE FROM board_items WHERE board_id=?1", [id])?;
    let mut sections = Vec::with_capacity(doc.sections.len());
    for section in doc.sections {
        let mut rects = section.rects();
        layout(&section)?;
        board_grid::compact(&mut rects).expect("validated layout");
        let section_id = next_id(conn, section.id)?;
        for (mut item, rect) in section.items.into_iter().zip(rects) {
            let item_id = next_id(conn, item.id)?;
            if let BoardPayload::Widget { widget_id, .. } = &mut item.payload {
                if let Some(source) = sources.get(widget_id) {
                    super::provider_widgets::place(conn, id, &item_id, widget_id, source)?;
                }
            }
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
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                look: item.look.filter(|look| !look.is_plain()),
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
            legacy_provider_title: legacy_titles
                .get(&section_id)
                .filter(|(title, _)| title == &section.title)
                .map(|(_, origin)| origin.clone()),
            id: section_id,
            title: section.title,
            position: section.position,
            collapsed: section.collapsed,
        });
    }
    crate::db::maintain::prune_provider_placements(conn, Some(id), now)?;
    conn.execute(
        "UPDATE boards SET sections=?2,name=coalesce(?4,name),
           icon=CASE WHEN ?4 IS NULL THEN icon ELSE ?5 END,appearance=?6,
           version=version+1,updated=?3
         WHERE id=?1",
        params![
            id,
            encode(&sections)?,
            now,
            doc.name,
            doc.icon,
            encode(&doc.appearance)?
        ],
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

// Early milestone builds migrated saved placements without updating drafts.
fn repair_legacy_draft(
    conn: &Connection,
    board: &BoardSummary,
    draft: &mut BoardDocument,
) -> Result<(), ApiError> {
    if board.version != draft.version {
        return Ok(());
    }
    let existing = document(conn, board)?;
    let mut saved_widgets = HashMap::new();
    for item in existing.sections.iter().flat_map(|section| &section.items) {
        if let BoardPayload::Widget { widget_id, .. } = &item.payload {
            if widget_id.starts_with("provider:") {
                saved_widgets.insert(&item.id, widget_id);
            }
        }
    }
    let mut legacy_ids = HashMap::new();
    for item in draft.sections.iter().flat_map(|section| &section.items) {
        if let (BoardPayload::Widget { widget_id, .. }, Some(saved)) =
            (&item.payload, saved_widgets.get(&item.id))
        {
            if widget_id.starts_with("provider:")
                && widget_id != *saved
                && super::provider_widgets::placement_scope(conn, widget_id)?.is_none()
            {
                legacy_ids.insert(widget_id.clone(), *saved);
            }
        }
    }
    for item in draft
        .sections
        .iter_mut()
        .flat_map(|section| &mut section.items)
    {
        if let BoardPayload::Widget { widget_id, .. } = &mut item.payload {
            if let Some(saved) = legacy_ids.get(widget_id) {
                widget_id.clone_from(saved_widgets.get(&item.id).unwrap_or(saved));
            }
        }
    }
    Ok(())
}

// Keep the placement IDs so a scoped editor can round-trip the board.
fn mask_provider_widgets(
    conn: &Connection,
    board: &str,
    user: &Principal,
    doc: &mut BoardDocument,
) -> Result<(), ApiError> {
    let mut invalid_titles = HashMap::new();
    let mut exists = conn.prepare("SELECT EXISTS (SELECT 1 FROM integrations WHERE id=?1)")?;
    for section in sections(conn, board)? {
        if let Some(origin) = section.legacy_provider_title {
            let hidden = if origin.is_empty() {
                user.is_scoped()
            } else {
                !user.can_see_integration(&origin)
                    || !exists.query_row([origin], |row| row.get::<_, bool>(0))?
            };
            if hidden {
                invalid_titles.insert(section.id, section.title);
            }
        }
    }
    let mut lookup = conn.prepare(
        "SELECT i.id,i.name FROM provider_widgets pw
         JOIN integrations i ON i.id=pw.integration_id WHERE pw.id=?1",
    )?;
    for section in &mut doc.sections {
        if section.id.as_ref().and_then(|id| invalid_titles.get(id)) == Some(&section.title) {
            section.title = "Widgets".into();
        }
        let provider_only = section.items.iter().all(|item| {
            matches!(&item.payload, BoardPayload::Widget { widget_id, .. } if widget_id.starts_with("provider:"))
        });
        for item in &mut section.items {
            if let BoardPayload::Integration {
                integration_id,
                title,
            } = &mut item.payload
            {
                // Hide which integration it is and its custom title. The
                // item's own ID stands in (unique, and never an integration
                // ID), so a scoped editor can round-trip the board.
                if let Some(stand_in) = item.id.as_ref() {
                    if !user.can_see_integration(integration_id) {
                        integration_id.clone_from(stand_in);
                        *title = Some(INTEGRATION_UNAVAILABLE.into());
                    }
                }
                continue;
            }
            let BoardPayload::Widget { widget_id, title } = &mut item.payload else {
                continue;
            };
            if !widget_id.starts_with("provider:") {
                continue;
            }
            let integration = lookup
                .query_row([widget_id.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .optional()?;
            if integration
                .as_ref()
                .is_some_and(|(id, _)| user.can_see_integration(id))
            {
                continue;
            }
            *title = Some("Widget unavailable".into());
            if integration
                .as_ref()
                .is_some_and(|(_, name)| section.title == *name)
                || (integration.is_none() && provider_only)
            {
                section.title = "Widgets".into();
            }
        }
    }
    Ok(())
}

const INTEGRATION_UNAVAILABLE: &str = "Integration unavailable";

/// Undo [`mask_provider_widgets`]' integration stand-ins in a scoped
/// editor's input, before validation: an item that still carries its own ID
/// as the integration gets the saved integration (and title, unless edited).
fn restore_masked_integrations(input: &mut BoardDocument, original: &BoardDocument) {
    let saved: HashMap<_, _> = original
        .sections
        .iter()
        .flat_map(|section| &section.items)
        .filter_map(|item| match &item.payload {
            BoardPayload::Integration {
                integration_id,
                title,
            } => Some((item.id.as_ref()?, (integration_id, title))),
            _ => None,
        })
        .collect();
    for item in input
        .sections
        .iter_mut()
        .flat_map(|section| &mut section.items)
    {
        let (
            Some(id),
            BoardPayload::Integration {
                integration_id,
                title,
            },
        ) = (item.id.as_ref(), &mut item.payload)
        else {
            continue;
        };
        if let Some((original, original_title)) = saved.get(id) {
            if integration_id == id {
                integration_id.clone_from(original);
                if title.as_deref() == Some(INTEGRATION_UNAVAILABLE) {
                    title.clone_from(original_title);
                }
            }
        }
    }
}

fn restore_masked_titles(
    input: &mut BoardDocument,
    original: &BoardDocument,
    masked: &BoardDocument,
) {
    let originals: HashMap<_, _> = original.sections.iter().map(|s| (&s.id, s)).collect();
    let projections: HashMap<_, _> = masked.sections.iter().map(|s| (&s.id, s)).collect();
    let mut titles = HashMap::new();
    for (original, masked) in original.sections.iter().zip(&masked.sections) {
        for (original, masked) in original.items.iter().zip(&masked.items) {
            if let (
                BoardPayload::Widget { widget_id, title },
                BoardPayload::Widget {
                    title: projected, ..
                },
            ) = (&original.payload, &masked.payload)
            {
                if title != projected {
                    titles.insert(&original.id, (widget_id, title, projected));
                }
            }
        }
    }
    for section in &mut input.sections {
        if let (Some(original), Some(masked)) =
            (originals.get(&section.id), projections.get(&section.id))
        {
            if section.title == masked.title {
                section.title.clone_from(&original.title);
            }
        }
        for item in &mut section.items {
            if let (BoardPayload::Widget { widget_id, title }, Some((old_id, original, masked))) =
                (&mut item.payload, titles.get(&item.id))
            {
                if widget_id == *old_id && title == *masked {
                    title.clone_from(original);
                }
            }
        }
    }
}

fn resolve(
    state: &AppState,
    now: i64,
    board: BoardSummary,
    document: BoardDocument,
    integrations: Vec<IntegrationView>,
    user: &Principal,
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
    let mut resolved = Vec::with_capacity(document.sections.iter().map(|s| s.items.len()).sum());
    let mut widget_budget = BOARD_WIDGET_BYTES;
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
                widget = if widget_id.starts_with("provider:") {
                    super::provider_widgets::view(state, user, &board.id, widget_id)
                } else {
                    config
                        .widgets
                        .iter()
                        .find(|w| w.id == *widget_id)
                        .map(|config| super::widgets::view(state, config))
                };
                if let Some(widget) = &mut widget {
                    widget.fit_board_budget(&mut widget_budget);
                }
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
            container: container.map(Box::new),
            integration: integration.map(Box::new),
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
        BoardPayload::Link { url, .. } => (1, link_key(url).unwrap_or_else(|| url.clone())),
        BoardPayload::Widget { widget_id, .. } => (2, widget_id.clone()),
        BoardPayload::Integration { integration_id, .. } => (3, integration_id.clone()),
    }
}

pub(super) const SUGGESTION_LIMIT: usize = 256;

pub(super) fn suggestions(
    state: &AppState,
    doc: Option<&BoardDocument>,
    integrations: Vec<(String, String)>,
    container_targets: Option<HashSet<ContainerMatch>>,
) -> Vec<BoardSuggestion> {
    let cfg = state.config.current();
    let snap = state.snapshot();
    let scoped = container_targets.is_some();
    let mut existing: HashSet<_> = doc
        .into_iter()
        .flat_map(|doc| &doc.sections)
        .flat_map(|section| &section.items)
        .map(|item| identity(&item.payload))
        .collect();
    let integrations = integrations.into_iter().map(|(id, name)| BoardSuggestion {
        title: name,
        payload: BoardPayload::Integration {
            integration_id: id,
            title: None,
        },
    });
    let widgets = cfg.widgets.iter().filter(|_| !scoped).filter_map(|widget| {
        let title = widget.title.as_deref().unwrap_or(&widget.id);
        super::validate_label(title, "title").ok()?;
        Some(BoardSuggestion {
            title: title.to_owned(),
            payload: BoardPayload::Widget {
                widget_id: widget.id.clone(),
                title: None,
            },
        })
    });
    let bookmarks = cfg
        .bookmarks
        .iter()
        .filter(|_| !scoped)
        .filter_map(|bookmark| {
            safe_link(&bookmark.url, "url").ok()?;
            super::validate_label(&bookmark.name, "title").ok()?;
            Some(BoardSuggestion {
                title: bookmark.name.clone(),
                payload: BoardPayload::Link {
                    title: bookmark.name.clone(),
                    url: bookmark.url.clone(),
                    icon: bookmark
                        .icon
                        .as_ref()
                        .filter(|icon| super::validate_icon(Some(icon)).is_ok())
                        .cloned(),
                },
            })
        });
    let containers = snap.containers.iter().filter_map(|container| {
        let title = crate::read_model::suggestion_title(&cfg, container)?;
        super::validate_label(title, "title").ok()?;
        let target = ContainerMatch::from(container);
        match &target {
            ContainerMatch::Compose { project, service } => {
                super::validate_label(project, "project").ok()?;
                super::validate_label(service, "service").ok()?;
            }
            ContainerMatch::Name { name } => super::validate_label(name, "name").ok()?,
        }
        if container_targets
            .as_ref()
            .is_some_and(|targets| !targets.contains(&target))
        {
            return None;
        }
        Some(BoardSuggestion {
            title: title.to_owned(),
            payload: BoardPayload::Container {
                target,
                title: None,
                icon: None,
                link: None,
            },
        })
    });
    integrations
        .chain(widgets)
        .chain(bookmarks)
        .chain(containers)
        .filter(|suggestion| existing.insert(identity(&suggestion.payload)))
        .take(SUGGESTION_LIMIT)
        .collect()
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
            let internet = db.dashboard(|c| super::dashboard_settings::internet(c))?;
            let target = crate::providers::Target::new(url, Default::default(), internet);
            super::json(
                StatusCode::OK,
                &integrations::test_connection(state, None, target).await,
            )
        }
        Handler::Boards => super::json(StatusCode::OK, &db.dashboard(|c| list(c, user))?),
        Handler::Board => {
            let (board, doc, visible) = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                let mut doc = document(c, &board)?;
                mask_provider_widgets(c, id, user, &mut doc)?;
                Ok((board, doc, integrations::list(state, c, user)?))
            })?;
            board_response(resolve(state, now, board, doc, visible, user))?
        }
        Handler::BoardSuggestions => {
            let (doc, integrations, targets) = db.dashboard(|c| -> Result<_, ApiError> {
                let doc = document(c, &get(c, id)?)?;
                Ok((
                    doc,
                    integrations::suggestion_targets(c, user, true)?,
                    user.container_targets(c)?,
                ))
            })?;
            super::json(
                StatusCode::OK,
                &suggestions(state, Some(&doc), integrations, targets),
            )
        }
        Handler::BoardCreate => {
            let BoardCreate { name, icon } = super::read_json(req, 8192).await?;
            let board = db.dashboard(|c| create(c, &name, icon.as_deref(), now))?;
            publish(state, &board.id, board.version);
            super::json(StatusCode::CREATED, &board)
        }
        Handler::BoardSave => {
            let mut input: BoardDocument = super::read_json(req, MAX_DOCUMENT).await?;
            let result = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                if board.version != input.version {
                    return Ok(Err(board.version));
                }
                let original = document(c, &board)?;
                if user.is_scoped() {
                    restore_masked_integrations(&mut input, &original);
                }
                validate(&input, &original, user, c, &state.config.current())?;
                if user.is_scoped() {
                    let mut masked = original.clone();
                    mask_provider_widgets(c, id, user, &mut masked)?;
                    restore_masked_titles(&mut input, &original, &masked);
                }
                store(c, id, input, now)?;
                c.execute(
                    "DELETE FROM drafts WHERE user_id=?1 AND key=?2",
                    params![user.id, draft_key],
                )?;
                let board = get(c, id)?;
                let mut doc = document(c, &board)?;
                mask_provider_widgets(c, id, user, &mut doc)?;
                db.invalidate_provider_scope();
                Ok(Ok((doc, integrations::list(state, c, user)?, board)))
            })?;
            match result {
                Ok((doc, visible, board)) => {
                    super::provider_widgets::placements_changed(state);
                    publish(state, id, board.version);
                    state.changed("groups");
                    board_response(resolve(state, now, board, doc, visible, user))?
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
                db.invalidate_provider_scope();
                Ok(board.version.saturating_add(1))
            })?;
            super::provider_widgets::placements_changed(state);
            publish(state, id, version);
            // The removed grants prevent scoped subscribers from receiving the
            // board ID. A generic invalidation lets them refresh navigation.
            let _ = state.events.send("reload".into());
            super::no_content()
        }
        Handler::BoardDraft => {
            let draft = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c, id)?;
                let row: Option<(String,i64)> = c.query_row(
                    "SELECT value,updated FROM drafts WHERE user_id=?1 AND key=?2 AND updated>=?3",
                    params![user.id,draft_key,now-7*86400], |r| Ok((r.get(0)?,r.get(1)?)),
                ).optional()?;
                row.map(|(value, updated)| {
                    let mut document = decode(&value)?;
                    repair_legacy_draft(c, &board, &mut document)?;
                    mask_provider_widgets(c, id, user, &mut document)?;
                    Ok(BoardDraft { document, updated })
                })
                .transpose()
            })?;
            super::json(StatusCode::OK, &draft)
        }
        Handler::BoardDraftSave => {
            let mut input: BoardDocument = super::read_json(req, MAX_DOCUMENT).await?;
            let result = db.dashboard(|c| -> Result<_, ApiError> {
                let board = get(c,id)?;
                if board.version != input.version { return Ok(Err(board.version)); }
                let original = document(c, &board)?;
                if user.is_scoped() {
                    restore_masked_integrations(&mut input, &original);
                }
                validate(&input,&original,user,c,&state.config.current())?;
                c.execute("INSERT INTO drafts (user_id,key,value,updated) VALUES (?1,?2,?3,?4)
                    ON CONFLICT(user_id,key) DO UPDATE SET value=excluded.value,updated=excluded.updated",
                    params![user.id,draft_key,encode(&input)?,now])?;
                mask_provider_widgets(c, id, user, &mut input)?;
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

    #[tokio::test]
    async fn full_board_shares_cached_widget_bodies_and_bounds_response_allocation() {
        use super::super::provider_widgets::cache_key;
        use crate::{
            providers::Values,
            testutil::{measure_allocations, state_with},
            widgets::{StructuredBody, WidgetOutput},
        };
        use http_body_util::BodyExt;

        let state = state_with(vec![], "");
        state.db.recover_owner(None, "fixture-owner", 1).unwrap();
        let owner = state.db.accounts().unwrap().pop().unwrap();
        let mut document = BoardDocument {
            version: 0,
            name: None,
            icon: None,
            appearance: Default::default(),
            sections: vec![BoardSection::new("Widgets", 0)],
        };
        let board = state.db.dashboard(|conn| -> rusqlite::Result<_> {
            conn.execute("INSERT INTO boards (id,name,created,updated) VALUES ('fixture','Board',0,0)", [])?;
            conn.execute(
                "INSERT INTO integrations (id,name,kind,origin) VALUES ('clock','Fixture','clock','')",
                [],
            )?;
            for position in 0..512usize {
                let id = format!("{position:032x}");
                let widget_id = format!("provider:{id}");
                conn.execute(
                    "INSERT INTO provider_widgets VALUES (?1,'clock','fixture','clock','{}')",
                    [&widget_id],
                )?;
                let payload = BoardPayload::Widget { widget_id, title: None };
                conn.execute(
                    "INSERT INTO board_items (id,board_id,kind,config) VALUES (?1,'fixture','widget',?2)",
                    params![id, serde_json::to_string(&payload).unwrap()],
                )?;
                document.sections[0].items.push(BoardItem {
                    id: Some(id),
                    x: (position % 3 * 4) as u16,
                    y: (position / 3 * 3) as u16,
                    w: 4,
                    h: 3,
                    look: None,
                    payload,
                });
            }
            Ok(get(conn, "fixture").unwrap())
        }).unwrap();
        let body = std::sync::Arc::new(StructuredBody::Stat {
            value: "x".repeat(31_000),
        });
        {
            let mut cache = state.widgets.lock().unwrap();
            let key = cache_key("clock", "clock", "clock", &Values::new());
            let attempt = cache.begin(&key);
            assert!(cache.record(
                &key,
                attempt,
                std::time::Instant::now(),
                WidgetOutput {
                    structured: Some(body.clone()),
                    ..Default::default()
                }
            ));
        }
        let ((response, shown), peak) = measure_allocations(|| {
            let view = resolve(&state, 1, board, document, vec![], &owner);
            let shown = std::sync::Arc::strong_count(&body) - 2;
            (board_response(view).unwrap(), shown)
        });
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        println!(
            "full board response: {} bytes, peak additional heap: {} bytes",
            bytes.len(),
            peak
        );
        assert!(
            (1..32).contains(&shown),
            "expected only budgeted Arc references"
        );
        assert!(bytes.len() < 768 * 1024);
        assert!(peak < 1024 * 1024);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let resolved = json["resolved"].as_array().unwrap();
        assert_eq!(resolved.len(), 512);
        assert_eq!(
            resolved
                .iter()
                .filter(|item| item["widget"].get("structured").is_some())
                .count(),
            shown
        );
        assert_eq!(
            resolved[511]["widget"]["error"],
            "Widget data unavailable within the board response limit."
        );
    }

    #[test]
    fn board_validation_checks_ids_links_layout_and_targets() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.recover_owner(None, "test", 1).unwrap();
        let owner = db.accounts().unwrap().pop().unwrap();
        let original = BoardDocument {
            version: 0,
            name: None,
            icon: None,
            appearance: Default::default(),
            sections: Vec::new(),
        };
        let mut board: BoardDocument = serde_json::from_value(serde_json::json!({
            "version": 0,
            "sections": [{
                "title": "Links",
                "position": 1,
                "items": [{
                    "x": 0, "y": 3, "w": 12, "h": 1,
                    "payload": {"kind": "link", "title": "Example", "url": "https://example.test"}
                }, {
                    "x": 0, "y": 0, "w": 4, "h": 2,
                    "look": {"accent": "teal", "icon": "jellyfin", "link": "https://media.example.test"},
                    "payload": {"kind": "link", "title": "Other", "url": "https://other.example.test"}
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
            item.x = 1;
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("layout"));
            let item = &mut board.sections[0].items[0];
            item.x = 0;
            item.y = 1;
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("layout"));
            let item = &mut board.sections[0].items[0];
            item.y = 3;
            item.h = 17;
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("layout"));
            board.sections[0].items[0].h = 1;
            board.sections[0].items[1].look.as_mut().unwrap().link = Some("javascript:x".into());
            assert_eq!(
                check(&board).unwrap_err().field.as_deref(),
                Some("look.link")
            );
            board.sections[0].items[1].look = None;
            board.name = Some(String::new());
            assert_eq!(check(&board).unwrap_err().field.as_deref(), Some("name"));
            board.name = None;
            let item = &mut board.sections[0].items[0];
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
