//! The Home board: the board the Home page (the sidebar's first entry) shows.
//! Every install gets one, seeded once at startup from Hobnail's own widgets,
//! the `[[widgets]]` and the `[[bookmarks]]` of config.toml. After that it is
//! an ordinary board: people edit, restyle or delete it like any other.
//!
//! `settings.home_board` names it; `settings.home_seeded` records that the
//! seed ran, so a deleted Home board is not created again. The default board
//! (what `/app` opens) is a separate setting; the seed sets it to the Home
//! board only when no default is set yet.

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{
    boards::{self, BoardAppearance, BoardDocument, BoardItem, BoardPayload, BoardSection},
    dashboard_settings,
    error::ApiError,
    integrations::{self, IntegrationInput},
};
use crate::{config::Config, providers::Values, state::AppState};

/// The name of a seeded Home board; `FALLBACK_NAME` when a board is already
/// called that (an older guided setup made one), which is left as it is.
const NAME: &str = "Home";
const FALLBACK_NAME: &str = "Overview";

/// Hobnail widget, tile title, options and rectangle (x, y, w, h) of the
/// seeded layout.
type Spec = (&'static str, &'static str, fn() -> Values, [u16; 4]);

fn none() -> Values {
    Values::new()
}
fn top_cpu() -> Values {
    object(json!({"sort": "cpu", "count": 6}))
}
fn cpu_day() -> Values {
    object(json!({"metric": "cpu", "range": "24h"}))
}
fn memory_day() -> Values {
    object(json!({"metric": "mem", "range": "24h"}))
}
fn object(value: serde_json::Value) -> Values {
    value.as_object().cloned().unwrap_or_default()
}

/// Sections of the seeded board. Phones read each section top to bottom,
/// left to right, so the host comes first and then what needs attention.
const LAYOUT: &[(&str, &[Spec])] = &[
    (
        "This host",
        &[
            ("host", "Host", none, [0, 0, 8, 4]),
            ("attention", "Needs attention", none, [8, 0, 4, 4]),
        ],
    ),
    (
        "Containers",
        &[
            ("containers", "Top containers", top_cpu, [0, 0, 4, 4]),
            ("stacks", "Stacks", none, [4, 0, 4, 4]),
            ("events", "Recent events", none, [8, 0, 4, 4]),
        ],
    ),
    (
        "Last 24 hours",
        &[
            ("history", "CPU", cpu_day, [0, 0, 6, 3]),
            ("history", "Memory", memory_day, [6, 0, 6, 3]),
        ],
    ),
];

pub(super) const HOME_BOARD: &str = "home_board";
const SEEDED: &str = "home_seeded";

/// Seed the Home board once; logs instead of failing the start.
pub fn seed(state: &AppState) {
    let config = state.config.current();
    match state
        .db
        .dashboard(|conn| seed_in(conn, &config, crate::auth::now_s()))
    {
        Ok(Some(id)) => crate::info!("created the Home board", id = id),
        Ok(None) => {}
        Err(e) => crate::warn!("could not create the Home board", error = e.message),
    }
}

/// Create the Home board unless it was created before. Returns its ID.
pub(super) fn seed_in(
    conn: &Connection,
    config: &Config,
    now: i64,
) -> Result<Option<String>, ApiError> {
    let seeded = conn
        .query_row("SELECT 1 FROM settings WHERE key=?1", [SEEDED], |_| Ok(()))
        .optional()?
        .is_some();
    if seeded {
        return Ok(None);
    }
    let integration = hobnail_integration(conn)?;
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM boards WHERE lower(trim(name))=lower(?1))",
        [NAME],
        |r| r.get(0),
    )?;
    let board = boards::create(conn, if taken { FALLBACK_NAME } else { NAME }, None, now)?;
    let mut sections = Vec::new();
    for (title, specs) in LAYOUT {
        let mut section = BoardSection::new(*title, sections.len() as u32);
        for (widget, title, options, [x, y, w, h]) in *specs {
            let item = boards::next_id(conn, None)?;
            let widget_id = format!("provider:{item}");
            conn.execute(
                "INSERT INTO provider_widgets (id,integration_id,board_id,widget,options)
                 VALUES (?1,?2,?3,?4,?5)",
                (
                    &widget_id,
                    &integration,
                    &board.id,
                    widget,
                    serde_json::to_string(&options()).expect("widget options"),
                ),
            )?;
            section.items.push(BoardItem {
                id: Some(item),
                x: *x,
                y: *y,
                w: *w,
                h: *h,
                look: None,
                payload: BoardPayload::Widget {
                    widget_id,
                    title: Some((*title).to_owned()),
                },
            });
        }
        sections.push(section);
    }
    // What the old Overview page listed from config.toml.
    let mut widgets = BoardSection::new("Widgets", sections.len() as u32);
    for widget in config.widgets.iter().take(64) {
        if super::validate_label(&widget.id, "widget_id").is_ok() {
            widgets.append(BoardPayload::Widget {
                widget_id: widget.id.clone(),
                title: None,
            });
        }
    }
    if !widgets.items.is_empty() {
        sections.push(widgets);
    }
    let mut links = BoardSection::new("Bookmarks", sections.len() as u32);
    let mut seen = std::collections::HashSet::new();
    for bookmark in config.bookmarks.iter().take(128) {
        let payload = BoardPayload::Link {
            title: bookmark.name.clone(),
            url: bookmark.url.clone(),
            icon: bookmark
                .icon
                .clone()
                .filter(|icon| super::validate_icon(Some(icon)).is_ok()),
        };
        if boards::safe_link(&bookmark.url, "url").is_ok()
            && super::validate_label(&bookmark.name, "title").is_ok()
            && seen.insert(boards::identity(&payload))
        {
            links.append(payload);
        }
    }
    if !links.items.is_empty() {
        sections.push(links);
    }
    let doc = BoardDocument {
        version: board.version,
        name: None,
        icon: None,
        appearance: BoardAppearance::default(),
        sections,
    };
    boards::store(conn, &board.id, doc, now)?;
    dashboard_settings::set(conn, HOME_BOARD, Some(&board.id))?;
    conn.execute(
        "INSERT INTO settings (key,value) VALUES (?1,'true')",
        [SEEDED],
    )?;
    let default_set: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM settings s JOIN boards b ON b.id=json_extract(s.value,'$')
         WHERE s.key='default_board')",
        [],
        |r| r.get(0),
    )?;
    if !default_set {
        dashboard_settings::set_default_board(conn, Some(&board.id))?;
    }
    Ok(Some(board.id))
}

/// The Hobnail connection the widgets belong to: an existing one (enabled
/// first), else a new one.
fn hobnail_integration(conn: &Connection) -> Result<String, ApiError> {
    let existing = conn
        .query_row(
            "SELECT id FROM integrations WHERE kind='hobnail'
             ORDER BY json_extract(config,'$.enabled') DESC, rowid LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let input = IntegrationInput {
        name: "Hobnail".into(),
        kind: "hobnail".into(),
        base_url: String::new(),
        icon: None,
        enabled: true,
        secret: None,
        clear_secret: false,
        credentials: Default::default(),
        fingerprint: None,
        options: Values::new(),
    };
    Ok(integrations::save(conn, None, input)?.id)
}

/// The Home board's ID, when it exists and `visible` allows it.
pub(super) fn board(
    conn: &Connection,
    visible: impl Fn(&str) -> bool,
) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT b.id FROM settings s JOIN boards b ON b.id=json_extract(s.value,'$')
             WHERE s.key=?1",
            [HOME_BOARD],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .filter(|id| visible(id)))
}
