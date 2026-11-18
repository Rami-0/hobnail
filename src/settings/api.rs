//! HTTP handlers for `/api/settings/*` (JSON in, JSON out). The full list,
//! with request and response shapes, is in the module docs of `settings`.

use bytes::Bytes;
use hyper::{HeaderMap, Method, Response, StatusCode};
use serde_json::{json, Map, Value as J};
use std::sync::Arc;

use super::doc;
use super::ops::{self, CollectionReq};
use super::store::{self, ApiError, Current};
use crate::http::server::{full, set, Body};
use crate::state::AppState;

pub const MAX_BODY: usize = 256 * 1024;
pub const MAX_CSS: usize = 64 * 1024;
const CSS_FILE: &str = "custom.css";

/// One request, already authenticated (see `settings::route`).
pub struct Call {
    pub method: Method,
    /// Path segments after `/api/settings/`, percent-decoded.
    pub segs: Vec<String>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub user: String,
}

pub fn respond(status: StatusCode, v: &J) -> Response<Body> {
    let mut r = full(status, serde_json::to_vec(v).unwrap_or_default());
    set(&mut r, hyper::header::CONTENT_TYPE, "application/json");
    set(&mut r, hyper::header::CACHE_CONTROL, "no-store");
    r
}

pub fn error(e: ApiError) -> Response<Body> {
    respond(e.status, &e.body)
}

fn body<T: serde::de::DeserializeOwned>(call: &Call) -> Result<T, ApiError> {
    if call.body.is_empty() {
        return Err(ApiError::bad_request("expected a JSON body"));
    }
    serde_json::from_slice(&call.body).map_err(|e| ApiError::bad_request(format!("bad JSON: {e}")))
}

pub async fn handle(state: &Arc<AppState>, call: Call) -> Response<Body> {
    match dispatch(state, &call).await {
        Ok(Reply::Json(v)) => respond(StatusCode::OK, &v),
        Ok(Reply::Download(v, name)) => {
            let mut r = respond(StatusCode::OK, &v);
            set(
                &mut r,
                hyper::header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{name}\""),
            );
            r
        }
        Err(e) => error(e),
    }
}

enum Reply {
    Json(J),
    Download(J, String),
}

async fn dispatch(state: &Arc<AppState>, call: &Call) -> Result<Reply, ApiError> {
    let segs: Vec<&str> = call.segs.iter().map(String::as_str).collect();
    let get = call.method == Method::GET || call.method == Method::HEAD;
    let post = call.method == Method::POST;
    let user = call.user.as_str();
    let json = Reply::Json;
    Ok(match (get, post, segs.as_slice()) {
        // ---- overview
        (true, _, ["meta"]) => json(meta(state, user)?),
        (true, _, ["env"]) => json(env_vars(state)?),

        // ---- config file, revisions
        (true, _, ["config"]) => json(raw_get(state)?),
        (_, true, ["config"]) => {
            let r: ops::RawReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |_| {
                Ok(store::Draft {
                    text: r.text.clone(),
                    note: "raw edit".into(),
                    kind: "config.saved",
                    fields: ops::RAW_FIELDS,
                    warnings: Vec::new(),
                })
            })?)
        }
        (_, true, ["config", "validate"]) => {
            let r: ops::TextReq = body(call)?;
            json(validate_text(state, &r.text)?)
        }
        (true, _, ["revisions"]) => json(revisions(state)),
        (true, _, ["revision", id]) => json(revision(state, id)?),
        (true, _, ["revision", id, "diff"]) => json(revision_diff(state, id)?),
        (_, true, ["revision", id, "restore"]) => {
            let r: ops::BaseReq = body(call)?;
            let id = rev_id(id)?;
            let text = state
                .db
                .revision_text(id)
                .ok_or_else(|| ApiError::not_found(format!("no revision #{id}")))?;
            json(store::edit(state, &r.base_hash, user, |_| {
                Ok(store::Draft {
                    text,
                    note: format!("restored #{id}"),
                    kind: "config.restored",
                    fields: ops::RAW_FIELDS,
                    warnings: Vec::new(),
                })
            })?)
        }

        // ---- general
        (true, _, ["general"]) => json(general_get(state)?),
        (_, true, ["general"]) => {
            let r: ops::PatchReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::general(c, &r).and_then(ops::precheck)
            })?)
        }

        // ---- containers and groups
        (true, _, ["containers"]) => json(containers_get(state)?),
        // Both take one entry at a time (`{base_hash, action, id?, item?,
        // order?}`, as every list). The old whole-list bodies are gone: a
        // whole list rewrote keys the caller never showed (`columns`).
        (_, true, ["docker"]) => {
            let r: CollectionReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::exclude_action(c, &r).and_then(ops::precheck)
            })?)
        }
        (_, true, ["groups"]) => json(list_post(state, call, &ops::GROUPS)?),
        (_, true, ["containers", "override"]) => {
            let r: ops::OverrideReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::container_override(c, &r).and_then(ops::precheck)
            })?)
        }

        // ---- bookmarks
        (true, _, ["bookmarks"]) => json(list_get(state, "bookmarks")?),
        (_, true, ["bookmarks"]) => json(list_post(state, call, &ops::BOOKMARKS)?),

        // ---- widgets
        (true, _, ["widgets"]) => json(widgets_get(state)?),
        (_, true, ["widgets"]) => json(list_post(state, call, &ops::WIDGETS)?),
        (_, true, ["widgets", "preview"]) => {
            let r: ops::PreviewReq = body(call)?;
            let w = ops::preview_widget(&r.item)?;
            // Secret binding (guard.rs): a preview may only send header
            // secrets to the destination a saved widget already sends them to.
            if !w.headers_env.is_empty() || !w.headers_file.is_empty() {
                let bound = state.config.current().widgets.iter().any(|s| {
                    s.url == w.url
                        && s.headers_env == w.headers_env
                        && s.headers_file == w.headers_file
                });
                if !bound {
                    return Err(ApiError::denied(super::guard::Denied(format!(
                        "refused: preview would send header secrets to an unsaved destination; {}",
                        super::guard::SECRET_BINDING_RULE
                    ))));
                }
            }
            let policy = state.config.current().http.clone();
            let out = crate::widgets::run(&w, &policy).await;
            json(json!({
                "ok": out.error.is_none(),
                "html": out.html,
                "values": out.values,
                "error": out.error,
                "duration_ms": out.duration_ms,
            }))
        }
        (_, true, ["system"]) => {
            let r: ops::SystemReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::system(c, &r).and_then(ops::precheck)
            })?)
        }

        // ---- notifications
        (true, _, ["notifications"]) => json(notifications_get(state)?),
        (_, true, ["channels"]) => {
            let r: CollectionReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::channel_action(c, &r).and_then(ops::precheck)
            })?)
        }
        (_, true, ["channels", "test"]) => {
            let r: ops::NameReq = body(call)?;
            json(channel_test(state, &r.name).await?)
        }
        (_, true, ["alerts"]) => json(list_post(state, call, &ops::ALERTS)?),

        // ---- appearance
        (true, _, ["appearance"]) => json(css_get(state)?),
        (_, true, ["appearance"]) => {
            let r: ops::CssReq = body(call)?;
            json(css_post(state, user, &r)?)
        }

        // ---- history and data
        (true, _, ["history"]) => json(history_get(state)?),
        (_, true, ["history"]) => {
            let r: ops::PatchReq = body(call)?;
            json(store::edit(state, &r.base_hash, user, |c| {
                ops::history(c, &r).and_then(ops::precheck)
            })?)
        }
        (_, true, ["history", "clear"]) => {
            state.db.clear_history();
            let now = crate::util::unix_now() as i64;
            state
                .db
                .record_event(now, "history.cleared", "warn", user, "history cleared");
            crate::info!("history cleared from the app", user = user);
            json(json!({"ok": true}))
        }
        (true, _, ["history", "export"]) => {
            let now = crate::util::unix_now();
            Reply::Download(export(state, now), format!("hobnail-export-{now}.json"))
        }

        // ---- security
        (true, _, ["security"]) => json(security_get(state)),
        (_, true, ["security", "password"]) => {
            let r: ops::PasswordReq = body(call)?;
            if r.new.is_empty() || r.new.len() > 1024 {
                return Err(ApiError::invalid("Use 8 to 1024 bytes.", Some("new"), None));
            }
            crate::auth::change_password(state, &call.headers, &r.current, &r.new)
                .map_err(password_error)?;
            json(json!({"ok": true}))
        }
        (_, true, ["security", "sign-out-others"]) => {
            if !crate::auth::sign_out_others(state, &call.headers) {
                return Err(ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "could not sign out sessions",
                ));
            }
            json(json!({"ok": true}))
        }

        (false, false, _) => {
            return Err(ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "use GET or POST",
            ))
        }
        _ => return Err(ApiError::not_found("no such settings endpoint")),
    })
}

/// A refused password change on the field it is about (422), in plain
/// words; anything else (no password set, too many attempts) stays a 400.
fn password_error(e: String) -> ApiError {
    let field = if e.contains("current password") {
        Some("current")
    } else if e.starts_with("the password must") {
        Some("new")
    } else {
        None
    };
    let mut c = e.chars();
    let msg = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect::<String>() + ".")
        .unwrap_or_default();
    match field {
        Some(f) => ApiError::invalid(&msg, Some(f), None),
        None => ApiError::bad_request(msg),
    }
}

fn list_post(state: &AppState, call: &Call, spec: &ops::ListSpec) -> Result<J, ApiError> {
    let r: CollectionReq = body(call)?;
    store::edit(state, &r.base_hash, &call.user, |c| {
        ops::list_action(c, spec, &r).and_then(ops::precheck)
    })
}

fn rev_id(s: &str) -> Result<i64, ApiError> {
    s.parse()
        .map_err(|_| ApiError::bad_request("revision id must be a number"))
}

// ------------------------------------------------------------ GET builders

fn load(state: &AppState) -> Result<(Current, toml_edit::DocumentMut), ApiError> {
    let cur = store::read_current(state)?;
    let d = ops::parse_doc(&cur)?;
    Ok((cur, d))
}

fn get_json(d: &toml_edit::DocumentMut, key: &str) -> J {
    d.get(key).map(doc::item_json).unwrap_or(J::Null)
}

fn meta(state: &AppState, user: &str) -> Result<J, ApiError> {
    let cur = store::read_current(state)?;
    let snap = state.config.snapshot();
    Ok(json!({
        "user": user,
        "hash": cur.hash,
        "path": state.config.path().display().to_string(),
        "exists": cur.exists,
        "bytes": cur.text.len(),
        "load_error": snap.error,
        "db_enabled": state.db.is_enabled(),
        "password_set": crate::auth::password_is_set(state),
        "limits": {"body_bytes": MAX_BODY, "css_bytes": MAX_CSS},
    }))
}

fn raw_get(state: &AppState) -> Result<J, ApiError> {
    let cur = store::read_current(state)?;
    let check = validate_report(&cur.text, None, &store::secrets_dir(state));
    Ok(json!({
        "hash": cur.hash,
        "text": cur.text,
        "exists": cur.exists,
        "bytes": cur.text.len(),
        "path": state.config.path().display().to_string(),
        "valid": check,
    }))
}

/// `{ok: true, warnings}` or `{ok: false, error, line, col, locked?}`.
fn validate_report(text: &str, old: Option<&str>, secrets_dir: &std::path::Path) -> J {
    if let Err(e) = store::validate(text, &[]) {
        let mut b = e.body;
        b["ok"] = J::Bool(false);
        return b;
    }
    if let Some(old) = old {
        if let Err(d) = super::guard::check(old, text, secrets_dir) {
            return json!({"ok": false, "error": d.0, "locked": true, "line": null, "col": null});
        }
        return json!({"ok": true, "warnings": super::guard::secret_warnings(old, text)});
    }
    json!({"ok": true, "warnings": []})
}

fn validate_text(state: &AppState, text: &str) -> Result<J, ApiError> {
    let cur = store::read_current(state)?;
    let mut v = validate_report(text, Some(&cur.text), &store::secrets_dir(state));
    v["hash"] = J::String(cur.hash);
    Ok(v)
}

fn revision_json(r: &crate::db::Revision) -> J {
    json!({"id": r.id, "ts": r.ts, "author": r.author, "note": r.note, "bytes": r.bytes})
}

fn revisions(state: &AppState) -> J {
    let list: Vec<J> = state.db.revisions(200).iter().map(revision_json).collect();
    json!({"enabled": state.db.is_enabled(), "revisions": list})
}

fn revision(state: &AppState, id: &str) -> Result<J, ApiError> {
    let id = rev_id(id)?;
    let text = state
        .db
        .revision_text(id)
        .ok_or_else(|| ApiError::not_found(format!("no revision #{id}")))?;
    let meta = state
        .db
        .revisions(200)
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| revision_json(&r))
        .unwrap_or(J::Null);
    Ok(json!({
        "id": id,
        "meta": meta,
        "hash": doc::sha256_hex(text.as_bytes()),
        "text": text,
    }))
}

fn revision_diff(state: &AppState, id: &str) -> Result<J, ApiError> {
    let id = rev_id(id)?;
    let text = state
        .db
        .revision_text(id)
        .ok_or_else(|| ApiError::not_found(format!("no revision #{id}")))?;
    let cur = store::read_current(state)?;
    diff_json(&cur.text, &text, id)
}

/// Diff from the current file to revision `id` ("what restoring changes").
pub fn diff_json(current: &str, revision: &str, id: i64) -> Result<J, ApiError> {
    use super::diff;
    for t in [current, revision] {
        if t.len() > diff::MAX_BYTES || t.lines().count() > diff::MAX_LINES {
            return Err(ApiError::too_large("too large to diff"));
        }
    }
    let mut v = diff::hunks_json(current, revision);
    v["from"] = J::String("current".into());
    v["to"] = J::String(format!("revision #{id}"));
    v["id"] = J::from(id);
    Ok(v)
}

fn general_get(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    let cfg = store::current_config(&cur)?;
    let p = &cfg.page;
    let def = crate::config::PageConfig::default();
    let layout = |l: crate::config::Layout| match l {
        crate::config::Layout::Grid => "grid",
        crate::config::Layout::List => "list",
    };
    Ok(json!({
        "hash": cur.hash,
        "page": get_json(&d, "page"),
        "effective": {
            "title": p.title.clone().unwrap_or_else(|| state.env.hostname.clone()),
            "greeting": p.greeting, "columns": p.columns, "layout": layout(p.layout),
            "theme": p.theme, "search_url": p.search_url,
            "show_stopped": p.show_stopped, "show_stats": p.show_stats,
        },
        "defaults": {
            "title": state.env.hostname, "greeting": null, "columns": def.columns,
            "layout": layout(def.layout), "theme": def.theme, "search_url": def.search_url,
            "show_stopped": def.show_stopped, "show_stats": def.show_stats,
        },
    }))
}

fn containers_get(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    let cfg = store::current_config(&cur)?;
    let snap = state.snapshot();
    let known: Vec<J> = snap
        .containers
        .iter()
        .map(|c| {
            json!({
                "name": c.name, "project": c.project, "image": c.image,
                "state": c.state, "overridden": cfg.containers.contains_key(&c.name),
            })
        })
        .collect();
    Ok(json!({
        "hash": cur.hash,
        "docker": {"exclude": cfg.docker.exclude, "standalone_group": cfg.docker.standalone_group},
        "groups": list_items(&d, "groups"),
        "overrides": get_json(&d, "containers"),
        "known": known,
    }))
}

fn list_items(d: &toml_edit::DocumentMut, key: &str) -> Vec<J> {
    match get_json(d, key) {
        J::Array(a) => a,
        _ => Vec::new(),
    }
}

fn list_get(state: &AppState, key: &str) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    let items: Vec<J> = list_items(&d, key)
        .into_iter()
        .enumerate()
        .map(|(i, item)| json!({"id": i, "item": item}))
        .collect();
    let mut m = Map::new();
    m.insert("hash".into(), J::String(cur.hash));
    m.insert(key.into(), J::Array(items));
    Ok(J::Object(m))
}

/// A command widget as shown read-only: argv yes, env values no.
fn locked_view(item: &J) -> J {
    let mut out = Map::new();
    if let J::Object(m) = item {
        for (k, v) in m {
            match k.as_str() {
                "env" => {
                    let names: Vec<&String> = v
                        .as_object()
                        .map(|o| o.keys().collect())
                        .unwrap_or_default();
                    out.insert("env_names".into(), json!(names));
                }
                "headers" | "headers_env" | "headers_file" | "body" => {}
                _ => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
    }
    J::Object(out)
}

fn widgets_get(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    let cfg = store::current_config(&cur)?;
    let widgets: Vec<J> = list_items(&d, "widgets")
        .into_iter()
        .zip(cfg.widgets.iter())
        .map(|(item, w)| {
            let locked = ops::is_command_json(&item);
            json!({
                "id": w.id,
                "type": w.kind,
                "locked": locked,
                "locked_reason": locked.then_some("command widget: defined in the config file on the host; read-only here"),
                "enabled": w.is_enabled(),
                "item": if locked { locked_view(&item) } else { item },
            })
        })
        .collect();
    Ok(json!({
        "hash": cur.hash,
        "system": {"enabled": cfg.system.enabled},
        "widgets": widgets,
    }))
}

/// Secret references of a channel: env var names with set/not-set, files
/// with exists. Never a value.
fn secret_refs(item: &J) -> Vec<J> {
    let mut out = Vec::new();
    let Some(m) = item.as_object() else {
        return out;
    };
    for (k, v) in m {
        if k == "headers_env" {
            for (h, n) in v.as_object().into_iter().flatten() {
                if let Some(n) = n.as_str() {
                    out.push(json!({"field": format!("headers_env.{h}"), "kind": "env", "name": n, "set": env_set(n)}));
                }
            }
        } else if k == "headers_file" {
            for (h, p) in v.as_object().into_iter().flatten() {
                if let Some(p) = p.as_str() {
                    out.push(json!({"field": format!("headers_file.{h}"), "kind": "file", "path": p, "exists": std::path::Path::new(p).is_file()}));
                }
            }
        } else if k.ends_with("_env") {
            if let Some(n) = v.as_str() {
                out.push(json!({"field": k, "kind": "env", "name": n, "set": env_set(n)}));
            }
        } else if k.ends_with("_file") {
            if let Some(p) = v.as_str() {
                out.push(json!({"field": k, "kind": "file", "path": p, "exists": std::path::Path::new(p).is_file()}));
            }
        }
    }
    out
}

fn env_set(name: &str) -> bool {
    std::env::var(name).is_ok()
}

fn notifications_get(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    store::current_config(&cur)?;
    let status = state.alerts.lock().unwrap().channels.clone();
    let mut channels = Vec::new();
    if let J::Object(m) = get_json(&d, "channels") {
        for (name, item) in m {
            let mut warnings = Vec::new();
            if item.get("url").is_some() {
                warnings.push("url is stored in the config file as plain text; use url_env if it contains a token");
            }
            channels.push(json!({
                "id": name,
                "type": item.get("type"),
                "item": item,
                "secrets": secret_refs(&item),
                "warnings": warnings,
                "status": status.get(&name).and_then(|s| serde_json::to_value(s).ok()),
            }));
        }
    }
    let rules: Vec<J> = list_items(&d, "alerts")
        .into_iter()
        .map(|item| json!({"id": item.get("name"), "item": item}))
        .collect();
    Ok(json!({"hash": cur.hash, "channels": channels, "rules": rules}))
}

/// Env vars referenced by channels and widget headers: names and set/not set.
fn env_vars(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    let mut vars: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    if let J::Object(m) = get_json(&d, "channels") {
        for (name, item) in m {
            for r in secret_refs(&item) {
                if let (Some(n), Some(f)) = (r["name"].as_str(), r["field"].as_str()) {
                    vars.entry(n.into())
                        .or_default()
                        .push(format!("channel {name}: {f}"));
                }
            }
        }
    }
    for item in list_items(&d, "widgets") {
        let id = item
            .get("id")
            .and_then(J::as_str)
            .unwrap_or("?")
            .to_string();
        for r in secret_refs(&json!({"headers_env": item.get("headers_env")})) {
            if let (Some(n), Some(f)) = (r["name"].as_str(), r["field"].as_str()) {
                vars.entry(n.into())
                    .or_default()
                    .push(format!("widget {id}: {f}"));
            }
        }
    }
    let list: Vec<J> = vars
        .into_iter()
        .map(|(name, used_by)| json!({"name": name, "set": env_set(&name), "used_by": used_by}))
        .collect();
    Ok(json!({"hash": cur.hash, "vars": list}))
}

async fn channel_test(state: &AppState, name: &str) -> Result<J, ApiError> {
    let cfg = state.config.current();
    if !cfg.channels.contains_key(name) {
        return Err(ApiError::not_found(format!(
            "no saved channel {name:?} (save it first)"
        )));
    }
    let res = super::notify::send_test(state, &cfg, name).await;
    let now = crate::util::unix_now() as i64;
    match res {
        Ok(()) => {
            state
                .db
                .record_event(now, "alert.test", "info", name, "test notification sent");
            Ok(json!({"ok": true}))
        }
        Err(e) => {
            state
                .db
                .record_event(now, "alert.test", "warn", name, "test notification failed");
            Ok(json!({"ok": false, "error": e}))
        }
    }
}

fn css_path(state: &AppState) -> std::path::PathBuf {
    state.env.data_dir.join(CSS_FILE)
}

/// The saved custom.css and its hash (the `base_hash` of a save), for the
/// Appearance page.
pub fn css_current(state: &AppState) -> Result<(String, String), ApiError> {
    let v = css_get(state)?;
    Ok((
        v["css"].as_str().unwrap_or("").to_string(),
        v["hash"].as_str().unwrap_or("").to_string(),
    ))
}

fn css_get(state: &AppState) -> Result<J, ApiError> {
    let css = match std::fs::read(css_path(state)) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(ApiError::internal(format!(
                "cannot read custom.css: {}",
                e.kind()
            )))
        }
    };
    Ok(json!({
        "hash": doc::sha256_hex(css.as_bytes()),
        "css": css,
        "bytes": css.len(),
        "max_bytes": MAX_CSS,
    }))
}

fn css_post(state: &AppState, user: &str, r: &ops::CssReq) -> Result<J, ApiError> {
    if r.css.len() > MAX_CSS {
        return Err(ApiError::too_large(format!(
            "custom.css is limited to {} KiB",
            MAX_CSS / 1024
        )));
    }
    if r.css.contains('\0') {
        return Err(ApiError::bad_request(
            "custom.css must not contain NUL bytes",
        ));
    }
    let _g = store::lock();
    let cur = css_get(state)?;
    if cur["hash"].as_str() != Some(r.base_hash.trim()) {
        return Err(ApiError::stale());
    }
    // A CRLF file stays CRLF (the textarea gives LF).
    let css = doc::keep_line_ends(cur["css"].as_str().unwrap_or(""), r.css.clone());
    let hash = doc::sha256_hex(css.as_bytes());
    // Saved or not, the retired token names it sets (shown on the form).
    let warnings = ops::css_token_warnings(&css);
    if cur["css"].as_str() == Some(css.as_str()) {
        return Ok(json!({"ok": true, "unchanged": true, "hash": hash, "warnings": warnings}));
    }
    doc::write_atomic(&css_path(state), css.as_bytes(), 0o644).map_err(ApiError::internal)?;
    let now = crate::util::unix_now() as i64;
    state
        .db
        .record_event(now, "config.saved", "info", user, "appearance: custom.css");
    crate::info!("custom.css saved from the app", user = user);
    state.changed("reload");
    Ok(json!({"ok": true, "hash": hash, "warnings": warnings}))
}

fn stats_json(state: &AppState) -> J {
    let s = state.db.stats();
    json!({
        "enabled": state.db.is_enabled(),
        "size_bytes": s.size_bytes, "sys_rows": s.sys_rows, "ctr_rows": s.ctr_rows,
        "event_rows": s.event_rows, "revisions": s.revisions, "oldest_ts": s.oldest_ts,
    })
}

fn history_get(state: &AppState) -> Result<J, ApiError> {
    let (cur, d) = load(state)?;
    Ok(json!({"hash": cur.hash, "history": get_json(&d, "history"), "db": stats_json(state)}))
}

fn event_json(e: &crate::db::Event) -> J {
    json!({"id": e.id, "ts": e.ts, "kind": e.kind, "level": e.level, "subject": e.subject, "detail": e.detail})
}

fn export(state: &AppState, now: u64) -> J {
    let events: Vec<J> = state
        .db
        .events(&crate::db::EventFilter {
            limit: 500,
            ..Default::default()
        })
        .iter()
        .map(event_json)
        .collect();
    let revs: Vec<J> = state.db.revisions(200).iter().map(revision_json).collect();
    json!({
        "exported_at": now,
        "host": state.env.hostname,
        "version": state.build.version,
        "build": state.build.sha,
        "db": stats_json(state),
        "events": events,
        "revisions": revs,
    })
}

fn security_get(state: &AppState) -> J {
    let logins: Vec<J> = crate::auth::recent_logins(state, 20)
        .iter()
        .map(event_json)
        .collect();
    json!({
        "password_set": crate::auth::password_is_set(state),
        "recent_logins": logins,
    })
}
