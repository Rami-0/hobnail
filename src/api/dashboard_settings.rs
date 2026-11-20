//! Dashboard product settings stored independently of config.toml.

use hyper::{body::Incoming, Request, Response, StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{access::Principal, error::ApiError};
use crate::{http::server::Body, state::AppState};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DashboardSettings {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub default_board: Option<String>,
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
    Ok(DashboardSettings { default_board })
}

pub(super) async fn route(
    state: &AppState,
    req: Request<Incoming>,
    user: &Principal,
) -> Result<Response<Body>, ApiError> {
    let settings = if req.method() == hyper::Method::PATCH {
        let input: DashboardSettings = super::read_json(req, 8192).await?;
        let saved = state.db.dashboard(|conn| -> Result<_, ApiError> {
            if let Some(id) = input.default_board {
                if !user.can_see_board(&id) || !board_exists(conn, &id)? {
                    return Err(ApiError::bad_request(
                        "select a visible board",
                        Some("default_board"),
                    ));
                }
                conn.execute(
                    "INSERT INTO settings (key,value) VALUES ('default_board',json_quote(?1))
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [id],
                )?;
            } else {
                conn.execute("DELETE FROM settings WHERE key='default_board'", [])?;
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
