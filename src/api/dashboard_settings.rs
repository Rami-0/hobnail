//! Dashboard product settings stored independently of config.toml.

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{access::Principal, error::ApiError};
use crate::{config::Config, http::server::Body, state::AppState};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DashboardSettings {
    pub default_board: Option<String>,
    pub default_theme: Option<String>,
    pub provider_internet: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsPatch {
    #[serde(default, deserialize_with = "present")]
    default_board: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    default_theme: Option<Option<String>>,
    provider_internet: Option<bool>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

pub(super) fn get(conn: &Connection, user: &Principal) -> Result<DashboardSettings, ApiError> {
    let default_board = conn
        .query_row(
            "SELECT b.id FROM settings s JOIN boards b ON b.id=json_extract(s.value,'$')
             WHERE s.key='default_board'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .filter(|id| user.can_see_board(id));
    Ok(DashboardSettings {
        default_board,
        default_theme: stored_theme(conn)?,
        provider_internet: internet(conn)?,
    })
}

pub(super) fn internet(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT value = 'true' FROM settings WHERE key='provider_internet'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(Option::unwrap_or_default)
}

fn stored_theme(conn: &Connection) -> Result<Option<String>, ApiError> {
    Ok(conn
        .query_row(
            "SELECT json_extract(value,'$') FROM settings WHERE key='default_theme'",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn theme(conn: &Connection, config: &Config) -> Result<String, ApiError> {
    Ok(stored_theme(conn)?.unwrap_or_else(|| config_theme(config).into()))
}

pub(super) fn config_theme(config: &Config) -> &str {
    match config.page.theme.as_str() {
        t @ ("dark" | "light" | "ember" | "contrast") => t,
        _ => "auto",
    }
}

pub(super) fn validate_theme(theme: Option<&str>) -> Result<(), ApiError> {
    if theme.is_some_and(|theme| !matches!(theme, "auto" | "dark" | "light" | "ember" | "contrast"))
    {
        return Err(ApiError::bad_request(
            "select a supported theme",
            Some("default_theme"),
        ));
    }
    Ok(())
}

pub(super) fn set_theme(conn: &Connection, theme: Option<&str>) -> Result<(), ApiError> {
    validate_theme(theme)?;
    set(conn, "default_theme", theme)
}

pub(super) fn set_default_board(conn: &Connection, id: Option<&str>) -> Result<(), ApiError> {
    set(conn, "default_board", id)
}

fn set(conn: &Connection, key: &str, value: Option<&str>) -> Result<(), ApiError> {
    if let Some(value) = value {
        conn.execute(
            "INSERT INTO settings (key,value) VALUES (?1,json_quote(?2))
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            (key, value),
        )?;
    } else {
        conn.execute("DELETE FROM settings WHERE key=?1", [key])?;
    }
    Ok(())
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    let settings = if req.method() == hyper::Method::PATCH {
        let input: SettingsPatch = super::read_json(req, 8192).await?;
        let saved = state.db.dashboard(|conn| -> Result<_, ApiError> {
            if input.default_board.is_none()
                && input.default_theme.is_none()
                && input.provider_internet.is_none()
            {
                return Err(ApiError::bad_request("supply a dashboard setting", None));
            }
            if let Some(id) = input.default_board {
                if let Some(id) = id.as_deref() {
                    if !user.can_see_board(id) || !board_exists(conn, id)? {
                        return Err(ApiError::bad_request(
                            "select a visible board",
                            Some("default_board"),
                        ));
                    }
                }
                set_default_board(conn, id.as_deref())?;
            }
            if let Some(theme) = input.default_theme {
                set_theme(conn, theme.as_deref())?;
            }
            if let Some(enabled) = input.provider_internet {
                let changed = internet(conn)? != enabled;
                conn.execute(
                    "INSERT INTO settings (key,value) VALUES ('provider_internet',?1)
                    ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [if enabled { "true" } else { "false" }],
                )?;
                if changed {
                    super::provider_widgets::invalidate(state, None);
                    // Stored test verdicts were made under the other setting.
                    state.integration_checks.lock().unwrap().clear();
                }
            }
            get(conn, user)
        })?;
        let _ = state.events.send("reload".into());
        saved
    } else {
        state.db.dashboard(|conn| get(conn, user))?
    };
    Ok(super::json(StatusCode::OK, &settings))
}

fn board_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM boards WHERE id=?1)",
        [id],
        |r| r.get(0),
    )
}
