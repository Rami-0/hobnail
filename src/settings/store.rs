//! Load the config file with its hash, and the one save pipeline every
//! config write goes through:
//!
//! stale check → `config::parse` (the real validator) → guards (command
//! widgets, secret files) → atomic write → revisions → event → reload.
//!
//! An invalid or refused result is never written. Saves are serialized by a
//! process-wide lock (held only across synchronous code, never an await).

use hyper::StatusCode;
use serde_json::{json, Value as J};
use std::sync::Mutex;

use super::{doc, guard};
use crate::state::AppState;

/// File label used in validation messages (`config.toml:3:7: …`).
pub const LABEL: &str = "config.toml";

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Hold this across "read, check, write".
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ------------------------------------------------------------ errors

/// An HTTP error with a JSON body `{"error": …, …}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: J,
}

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": msg.into() }),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, msg)
    }
    pub fn too_large(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, msg)
    }
    /// 403 for a guard refusal (`"locked": true` so the UI can show the lock).
    pub fn denied(d: guard::Denied) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: json!({ "error": d.0, "locked": true }),
        }
    }
    pub fn stale() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: json!({ "error": "config changed on disk; reload", "stale": true }),
        }
    }
    /// 422 with the failing form field (when known) and line/column.
    pub fn invalid(msg: &str, field: Option<&str>, pos: Option<(usize, usize)>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: json!({
                "error": msg,
                "field": field,
                "line": pos.map(|p| p.0),
                "col": pos.map(|p| p.1),
            }),
        }
    }
}

/// Split `config.toml:3:7: message` into the message and the position.
pub fn split_error(e: &str) -> (String, Option<(usize, usize)>) {
    let rest = e
        .strip_prefix(LABEL)
        .map(|r| r.strip_prefix(':').unwrap_or(r))
        .unwrap_or(e);
    let mut parts = rest.splitn(3, ':');
    if let (Some(l), Some(c), Some(msg)) = (parts.next(), parts.next(), parts.next()) {
        if let (Ok(l), Ok(c)) = (l.trim().parse(), c.trim().parse()) {
            return (msg.trim().to_string(), Some((l, c)));
        }
    }
    (rest.trim().to_string(), None)
}

/// The form field an error message is about: the first of `fields` that
/// appears in it as a whole word.
pub fn field_in(msg: &str, fields: &[&str]) -> Option<String> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut best: Option<(usize, &str)> = None;
    for f in fields {
        let mut from = 0;
        while let Some(i) = msg[from..].find(f) {
            let s = from + i;
            let e = s + f.len();
            let before_ok = s == 0 || !msg[..s].chars().next_back().is_some_and(is_word);
            let after_ok = e == msg.len() || !msg[e..].chars().next().is_some_and(is_word);
            if before_ok && after_ok {
                if best.is_none_or(|(bs, bf)| s < bs || (s == bs && f.len() > bf.len())) {
                    best = Some((s, f));
                }
                break;
            }
            from = e;
        }
    }
    best.map(|(_, f)| f.to_string())
}

/// Validate config text like a load would. Err is a 422.
pub fn validate(text: &str, fields: &[&str]) -> Result<crate::config::Config, ApiError> {
    crate::config::parse(text, LABEL).map_err(|e| {
        let (msg, pos) = split_error(&e);
        let field = field_in(&msg, fields);
        ApiError::invalid(&msg, field.as_deref(), pos)
    })
}

// ------------------------------------------------------------ current file

/// The config file as it is on disk now.
#[derive(Debug, Clone)]
pub struct Current {
    pub text: String,
    pub hash: String,
    pub exists: bool,
}

pub fn read_current(state: &AppState) -> Result<Current, ApiError> {
    let path = state.config.path();
    let (text, exists) = match std::fs::read(path) {
        Ok(b) => (
            String::from_utf8(b).map_err(|_| ApiError::internal("config file is not UTF-8"))?,
            true,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => {
            return Err(ApiError::internal(format!(
                "cannot read config: {}",
                e.kind()
            )))
        }
    };
    Ok(Current {
        hash: doc::sha256_hex(text.as_bytes()),
        text,
        exists,
    })
}

pub fn check_base(cur: &Current, base_hash: &str) -> Result<(), ApiError> {
    if cur.hash != base_hash.trim().to_ascii_lowercase() {
        return Err(ApiError::stale());
    }
    Ok(())
}

/// The parsed, valid config on disk (form tabs need a valid file).
pub fn current_config(cur: &Current) -> Result<crate::config::Config, ApiError> {
    crate::config::parse(&cur.text, LABEL).map_err(|e| {
        let (msg, pos) = split_error(&e);
        let mut err = ApiError::invalid(
            &format!("the config file is invalid ({msg}); fix it in the Config file tab"),
            None,
            pos,
        );
        err.body["file_invalid"] = J::Bool(true);
        err
    })
}

// ------------------------------------------------------------ save

/// One proposed change of the config file.
#[derive(Debug)]
pub struct Draft {
    pub text: String,
    /// Event and revision note, e.g. "widgets: added mailpit".
    pub note: String,
    /// `config.saved` or `config.restored`.
    pub kind: &'static str,
    /// Form fields, to name the one an error is about.
    pub fields: &'static [&'static str],
    /// Warnings from the operation itself (e.g. plain webhook URL).
    pub warnings: Vec<String>,
}

/// Validate, guard, write, record and reload. The caller holds [`lock`] and
/// has checked `base_hash` against `cur`. Returns the JSON for the client.
pub fn commit(state: &AppState, cur: &Current, user: &str, d: Draft) -> Result<J, ApiError> {
    commit_checked(state, cur, user, d, &cur.text)
}

fn commit_checked(
    state: &AppState,
    cur: &Current,
    user: &str,
    mut d: Draft,
    guard_base: &str,
) -> Result<J, ApiError> {
    // A CRLF file stays CRLF (the editor and new lines bring LF). A restore
    // writes the revision exactly as it was.
    if d.kind != "config.restored" {
        d.text = doc::keep_line_ends(&cur.text, d.text);
    }
    if d.text == cur.text {
        return Ok(json!({
            "ok": true, "unchanged": true, "hash": cur.hash,
            "revision": J::Null, "note": d.note, "warnings": d.warnings,
        }));
    }
    validate(&d.text, d.fields)?;
    guard::check(guard_base, &d.text, &secrets_dir(state)).map_err(ApiError::denied)?;
    let mut warnings = d.warnings;
    warnings.extend(guard::secret_warnings(&cur.text, &d.text));

    doc::write_atomic(state.config.path(), d.text.as_bytes(), 0o644).map_err(|e| {
        crate::warn!("config save failed", error = e);
        ApiError::internal(e)
    })?;

    *state.settings_undo.lock().unwrap() = None;
    let now = crate::util::unix_now() as i64;
    let db = &state.db;
    // Keep what was on disk before this save when no revision has it (the
    // first in-app edit, or a hand edit since the last one).
    if cur.exists {
        match db.revisions(1).first() {
            None => {
                db.add_revision(now, "file", "before first in-app edit", &cur.text);
            }
            Some(last) => {
                if db.revision_text(last.id).as_deref() != Some(cur.text.as_str()) {
                    db.add_revision(now, "file", "edited outside the app", &cur.text);
                }
            }
        }
    }
    let revision = db.add_revision(now, user, &d.note, &d.text);
    db.record_event(now, d.kind, "info", user, &d.note);
    crate::info!("config saved from the app", note = d.note, user = user);
    state.config.reload();
    state.changed("reload");
    Ok(json!({
        "ok": true,
        "hash": doc::sha256_hex(d.text.as_bytes()),
        "revision": revision,
        "note": d.note,
        "warnings": warnings,
    }))
}

/// Lock, read, check the base hash, build the new text with `f`, commit.
pub fn edit(
    state: &AppState,
    base_hash: &str,
    user: &str,
    f: impl FnOnce(&Current) -> Result<Draft, ApiError>,
) -> Result<J, ApiError> {
    let _g = lock();
    let cur = read_current(state)?;
    check_base(&cur, base_hash)?;
    let draft = f(&cur)?;
    commit(state, &cur, user, draft)
}

pub(crate) struct Undo {
    token: String,
    actor: String,
    after: String,
    before: String,
    expires: std::time::Instant,
}

/// Keep a single short-lived receipt for the exact pre-delete document.
pub fn edit_with_undo(
    state: &AppState,
    base_hash: &str,
    user: &str,
    deletion: bool,
    f: impl FnOnce(&Current) -> Result<Draft, ApiError>,
) -> Result<J, ApiError> {
    if !deletion {
        return edit(state, base_hash, user, f);
    }
    let _g = lock();
    let cur = read_current(state)?;
    check_base(&cur, base_hash)?;
    let draft = f(&cur)?;
    let token = crate::auth::random_token().map_err(ApiError::internal)?;
    let after = doc::sha256_hex(doc::keep_line_ends(&cur.text, draft.text.clone()).as_bytes());
    let mut reply = commit(state, &cur, user, draft)?;
    *state.settings_undo.lock().unwrap() = Some(Undo {
        token: token.clone(),
        actor: user.into(),
        after,
        before: cur.text,
        expires: std::time::Instant::now() + std::time::Duration::from_secs(60),
    });
    reply["undo_token"] = J::String(token);
    Ok(reply)
}

pub fn undo(state: &AppState, user: &str, token: &str) -> Result<J, ApiError> {
    let _g = lock();
    let cur = read_current(state)?;
    let before = {
        let undo = state.settings_undo.lock().unwrap();
        let receipt = undo
            .as_ref()
            .filter(|entry| {
                entry.token == token
                    && entry.actor == user
                    && entry.after == cur.hash
                    && entry.expires > std::time::Instant::now()
            })
            .ok_or_else(ApiError::stale)?;
        receipt.before.clone()
    };
    // The receipt authenticates the exact pre-delete config and destination
    // bindings. The client supplies no config, path, or destination.
    commit_checked(
        state,
        &cur,
        user,
        Draft {
            text: before.clone(),
            note: "undid settings deletion".into(),
            kind: "config.restored",
            fields: &[],
            warnings: Vec::new(),
        },
        &before,
    )
}

/// `{data_dir}/secrets`: the only place the app may point a new `*_file` at.
pub fn secrets_dir(state: &AppState) -> std::path::PathBuf {
    state.env.data_dir.join("secrets")
}
