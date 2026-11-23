//! The one-off upgrade from responsive spans to canvas rectangles.
//!
//! Before the canvas, an item stored `{section, position, spans}` and the
//! client laid sections out with a dense auto-fill grid. This keeps every
//! section and item, and gives each item the rectangle that flow produced:
//! items in saved order, each in the first free slot of its old size.
//! Private drafts get the same treatment, plus the board's name and icon,
//! which documents now carry.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};

use crate::board_grid::{self, Rect};

fn span(value: &Value, key: &str, default: u16) -> u16 {
    value
        .get("spans")
        .and_then(|s| s.get(key))
        .and_then(Value::as_u64)
        .map_or(default, |n| n.min(u64::from(u16::MAX)) as u16)
}

fn size(kind: &str, item: &Value) -> (u16, u16) {
    board_grid::legacy_size(kind, span(item, "desktop", 4), span(item, "rows", 1))
}

fn set_rect(object: &mut Map<String, Value>, rect: Rect) {
    object.remove("position");
    object.remove("spans");
    object.insert("x".into(), json!(rect.x));
    object.insert("y".into(), json!(rect.y));
    object.insert("w".into(), json!(rect.w));
    object.insert("h".into(), json!(rect.h));
}

/// A saved item: its old position, ID, kind and layout object.
type Saved = (u64, String, String, Value);

pub(super) fn upgrade(conn: &Connection) -> rusqlite::Result<()> {
    let bad = |_| rusqlite::Error::InvalidQuery;
    // Saved items, per board and section, in their saved order.
    let mut sections: BTreeMap<(String, String), Vec<Saved>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare("SELECT id,board_id,kind,layout FROM board_items")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let (id, board, kind, layout): (String, String, String, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
            let layout: Value = serde_json::from_str(&layout).map_err(bad)?;
            let Some(section) = layout.get("section").and_then(Value::as_str) else {
                continue;
            };
            let position = layout.get("position").and_then(Value::as_u64).unwrap_or(0);
            sections
                .entry((board, section.to_owned()))
                .or_default()
                .push((position, id, kind, layout));
        }
    }
    for mut items in sections.into_values() {
        items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let sizes: Vec<_> = items
            .iter()
            .map(|(_, _, kind, layout)| size(kind, layout))
            .collect();
        for ((_, id, _, mut layout), rect) in items.into_iter().zip(board_grid::flow(&sizes)) {
            if let Some(object) = layout.as_object_mut() {
                set_rect(object, rect);
            }
            conn.execute(
                "UPDATE board_items SET layout=?2 WHERE id=?1",
                params![id, layout.to_string()],
            )?;
        }
    }

    let drafts: Vec<(i64, String, String)> = conn
        .prepare("SELECT user_id,key,value FROM drafts WHERE key LIKE 'board:%'")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (user, key, value) in drafts {
        let board = key.trim_start_matches("board:");
        let named: Option<(String, Option<String>)> = conn
            .query_row("SELECT name,icon FROM boards WHERE id=?1", [board], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok();
        let mut doc: Value = serde_json::from_str(&value).map_err(bad)?;
        let (Some((name, icon)), Some(object)) = (named, doc.as_object_mut()) else {
            conn.execute(
                "DELETE FROM drafts WHERE user_id=?1 AND key=?2",
                params![user, key],
            )?;
            continue;
        };
        object.insert("name".into(), json!(name));
        object.insert("icon".into(), json!(icon));
        for section in object
            .get_mut("sections")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            let Some(items) = section.get_mut("items").and_then(Value::as_array_mut) else {
                continue;
            };
            let sizes: Vec<_> = items
                .iter()
                .map(|item| {
                    let kind = item
                        .pointer("/payload/kind")
                        .and_then(Value::as_str)
                        .unwrap_or("link");
                    size(kind, item)
                })
                .collect();
            for (item, rect) in items.iter_mut().zip(board_grid::flow(&sizes)) {
                if let Some(object) = item.as_object_mut() {
                    set_rect(object, rect);
                }
            }
        }
        conn.execute(
            "UPDATE drafts SET value=?3 WHERE user_id=?1 AND key=?2",
            params![user, key, doc.to_string()],
        )?;
    }
    Ok(())
}
