//! Per-tab operations: validate the request shape (serde, unknown fields
//! refused), patch the document losslessly, return a [`Draft`] for the save
//! pipeline. Nothing here touches the disk.
//!
//! Semantics:
//! - Singleton sections (`[page]`, `[history]`): a **patch**. A key that is
//!   present sets the value, `null` removes the key (the default applies),
//!   an absent key is left alone.
//! - Entities (a group, bookmark, widget, channel, rule) are sent **whole**
//!   when added: a managed field that is missing or `null` is not written.
//! - An **edit** of a list entry or channel is a patch of that entry: a key
//!   that is present sets the value, `null` removes it, and a key that is
//!   absent is left exactly as it is in the file. So a form never removes a
//!   key it does not show (header values, a body, `api_base`, …), and a
//!   field the user did not change can simply be left out. Keys the form
//!   does not know are always left untouched.

use serde::Deserialize;
use serde_json::{Map, Value as J};
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Key, Table, Value};

use super::doc::{self, Style};
use super::guard;
use super::store::{self, ApiError, Current, Draft};

pub const PAGE_KEYS: &[&str] = &[
    "title",
    "greeting",
    "columns",
    "layout",
    "theme",
    "search_url",
    "show_stopped",
    "show_stats",
];
pub const GROUP_KEYS: &[&str] = &[
    "name",
    "title",
    "columns",
    "layout",
    "collapsed",
    "icon",
    "hide",
];
pub const OVERRIDE_KEYS: &[&str] = &[
    "name",
    "group",
    "icon",
    "url",
    "description",
    "hide",
    "order",
    "size",
];
pub const BOOKMARK_KEYS: &[&str] = &["name", "url", "group", "icon", "description"];
pub const WIDGET_KEYS: &[&str] = &[
    "id",
    "type",
    "title",
    "icon",
    "link",
    "url",
    "method",
    "body",
    "headers",
    "headers_env",
    "headers_file",
    "refresh",
    "timeout",
    "enabled",
    "output",
    "template",
    "status",
    "fields",
    "rows",
];
pub const CHANNEL_KEYS: &[&str] = &[
    "type",
    "url",
    "url_env",
    "url_file",
    "webhook_url_env",
    "webhook_url_file",
    "token_env",
    "token_file",
    "chat_id",
    "chat_id_env",
    "api_base",
    "headers",
    "headers_env",
    "body",
    "timeout",
];
/// Channel form fields (the name is the table key, not a field in it).
pub const CHANNEL_FIELDS: &[&str] = &[
    "name",
    "type",
    "url",
    "url_env",
    "url_file",
    "webhook_url_env",
    "webhook_url_file",
    "token_env",
    "token_file",
    "chat_id",
    "chat_id_env",
    "api_base",
    "headers",
    "headers_env",
    "body",
    "timeout",
];
pub const RULE_KEYS: &[&str] = &[
    "name",
    "type",
    "containers",
    "exclude",
    "on",
    "restarts",
    "window",
    "url",
    "interval",
    "timeout",
    "expect_status",
    "contains",
    "failures",
    "pattern",
    "stack",
    "count",
    "metric",
    "disk",
    "widget",
    "field",
    "above",
    "below",
    "equals",
    "not_equals",
    "on_error",
    "for",
    "cooldown",
    "channels",
    "message",
    "recovery",
    "enabled",
];
pub const DOCKER_FIELDS: &[&str] = &["exclude"];
pub const SYSTEM_FIELDS: &[&str] = &["enabled"];
pub const RAW_FIELDS: &[&str] = &[];

// ------------------------------------------------------------ shapes

fn shape<T: serde::de::DeserializeOwned>(v: &J, what: &str) -> Result<T, ApiError> {
    serde_json::from_value(v.clone()).map_err(|e| shape_error::<T>(v, what, &e.to_string()))
}

/// A shape error as a 422 on the form field that caused it, in plain words
/// (SET-07): a missing required key is "Required."; a value of the wrong
/// type is found by checking each key on its own (a lone key with the right
/// type only misses the others). An unknown key stays a 400: the form never
/// sends one, so it is a client bug, not a field problem.
fn shape_error<T: serde::de::DeserializeOwned>(v: &J, what: &str, msg: &str) -> ApiError {
    let raw = || ApiError::bad_request(format!("{what}: {msg}"));
    if let Some(f) = msg
        .strip_prefix("missing field `")
        .and_then(|r| r.split('`').next())
    {
        return ApiError::invalid("Required.", Some(f), None);
    }
    if msg.starts_with("unknown field") {
        return raw();
    }
    for (k, x) in v.as_object().into_iter().flatten() {
        let one = J::Object(Map::from_iter([(k.clone(), x.clone())]));
        if let Err(e) = serde_json::from_value::<T>(one) {
            let e = e.to_string();
            if !e.starts_with("missing field") && !e.starts_with("unknown field") {
                return ApiError::invalid(&plain(&e), Some(k), None);
            }
        }
    }
    raw()
}

/// A validator or deserialiser message in the words a form uses (SET-06,
/// SET-07, SET-55). Messages it does not know are returned as they are.
pub fn plain(msg: &str) -> String {
    let m = msg.trim();
    let int = [
        "expected u8",
        "expected u16",
        "expected u32",
        "expected u64",
        "expected i32",
        "expected i64",
        "expected usize",
    ];
    if m.contains("untagged enum DurJ") || m.starts_with("invalid duration") {
        return "Use a duration like 30s or 5m.".into();
    }
    if m.starts_with("invalid value: integer") && int.iter().any(|t| m.contains(t)) {
        return "This number is out of range.".into();
    }
    if m.starts_with("invalid type") && int.iter().any(|t| m.contains(t)) {
        return "Use a whole number.".into();
    }
    if m.starts_with("invalid type") && m.contains("expected f64") {
        return "Use a number.".into();
    }
    if m.starts_with("invalid type") && m.contains("expected a boolean") {
        return "Use on or off.".into();
    }
    if m.starts_with("invalid type") && m.contains("expected a string") {
        return "Use text here.".into();
    }
    if m.starts_with("unknown variant") {
        return "Choose one of the offered options.".into();
    }
    // `page.columns must be 1..=6, got 9`, `… must be between 5s and 5m, got 1s`
    if let Some((_, r)) = m.split_once(" must be ") {
        let r = r.split(", got").next().unwrap_or(r);
        if let Some((a, b)) = r.split_once("..=") {
            return format!("Use a number from {a} to {b}.");
        }
        if let Some((a, b)) = r
            .strip_prefix("between ")
            .and_then(|r| r.split_once(" and "))
        {
            return format!("Use a value from {a} to {b}.");
        }
    }
    m.to_string()
}

/// Validate a form's draft before the save pipeline does (it validates
/// again; parsing twice is cheap next to a write), to put a config error on
/// the form field that caused it (SET-06): when the validator names no field
/// the key at the reported line and column does. A field error loses its
/// line and column (the form never shows the file) and is put in plain
/// words. An error that belongs to no field is returned unchanged.
pub fn precheck(d: Draft) -> Result<Draft, ApiError> {
    let Err(mut e) = store::validate(&d.text, d.fields) else {
        return Ok(d);
    };
    // `alert "x": …`, `channel "x" (slack): …`, `widget "x": …`: a check of
    // one entry. The field is looked for after the entry's name (a rule
    // called "disk" is not about the Disk field) and the message is said in
    // the form's words (SET-55).
    let raw = e.body["error"].as_str().unwrap_or("").to_string();
    if let Some(rest) = strip_owner(&raw) {
        e.body["field"] = entry_field(rest, d.fields).map_or(J::Null, J::String);
        e.body["error"] = J::String(sentence(rest));
        e.body["line"] = J::Null;
        e.body["col"] = J::Null;
        return Err(e);
    }
    if e.body["field"].is_null() {
        let at = (e.body["line"].as_u64(), e.body["col"].as_u64());
        if let (Some(l), Some(c)) = at {
            let line = d
                .text
                .lines()
                .nth((l as usize).saturating_sub(1))
                .unwrap_or("");
            if let Some(k) = key_before(line, c as usize).filter(|k| d.fields.contains(k)) {
                e.body["field"] = J::String(k.to_string());
            }
        }
    }
    if !e.body["field"].is_null() {
        let msg = plain(e.body["error"].as_str().unwrap_or(""));
        e.body["error"] = J::String(msg);
        e.body["line"] = J::Null;
        e.body["col"] = J::Null;
    }
    Err(e)
}

/// The message after an entry's name (`alert "x": `, `channel "x" (slack): `,
/// `widget "x": `), or `None` when it names no entry.
pub fn strip_owner(msg: &str) -> Option<&str> {
    for p in ["alert \"", "channel \"", "widget \""] {
        if let Some(r) = msg.strip_prefix(p) {
            // The name is quoted with Rust's `{:?}`: skip escaped quotes.
            let mut esc = false;
            for (i, c) in r.char_indices() {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    let after = &r[i + 1..];
                    let after = match after.strip_prefix(" (") {
                        Some(a) => &a[a.find(')').map_or(0, |j| j + 1)..],
                        None => after,
                    };
                    return after.strip_prefix(": ");
                }
            }
        }
    }
    None
}

/// The form field one entry's check is about. A widget's field names and
/// paths, display rows and status are lists on the form, named by their
/// list (`fields`, `rows`, `status`).
pub fn entry_field(rest: &str, fields: &[&str]) -> Option<String> {
    let list = |k: &str| fields.contains(&k).then(|| k.to_string());
    if rest.starts_with("field ") {
        return list("fields");
    }
    if rest.starts_with("row ") {
        return list("rows");
    }
    if rest.starts_with("status") {
        return list("status");
    }
    store::field_in(rest, fields)
}

/// The label a form shows for a config key.
pub fn label_of(key: &str) -> &str {
    match key {
        "above" => "Above",
        "below" => "Below",
        "equals" => "Equals",
        "not_equals" => "Not equals",
        "url" => "URL",
        "url_env" => "URL variable",
        "url_file" => "URL file",
        "webhook_url_env" => "Webhook URL variable",
        "webhook_url_file" => "Webhook URL file",
        "token_env" => "Bot token variable",
        "token_file" => "Bot token file",
        "chat_id" => "Chat id",
        "chat_id_env" => "Chat id variable",
        "containers" => "Containers",
        "exclude" => "Except",
        "stack" => "Stack",
        "pattern" => "Pattern",
        "widget" => "Widget",
        "field" => "Field",
        "window" => "Window",
        "interval" => "Check every",
        "timeout" => "Timeout",
        "restarts" => "Restarts",
        "count" => "Lines",
        "failures" => "Failures in a row",
        "expect_status" => "Expected status",
        "refresh" => "Refresh",
        "cooldown" => "Cooldown",
        "for" => "For",
        "on" => "Fire on",
        "metric" => "Metric",
        "disk" => "Disk",
        "link" => "Link",
        "id" => "Id",
        "template" => "Template",
        "message" => "Message",
        "recovery" => "Recovery message",
        k => k,
    }
}

/// One entry's check message as a sentence in the form's words:
/// `set exactly one of above, below` → "Set exactly one of: Above or
/// Below."; a `key` in backticks becomes its label; durations and ranges as
/// [`plain`] says them.
pub fn sentence(msg: &str) -> String {
    let m = msg.trim();
    if let Some(list) = m.strip_prefix("set exactly one of ") {
        let names: Vec<&str> = list.split(", ").map(|k| label_of(k.trim())).collect();
        let joined = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
            _ => names.join(""),
        };
        return format!("Set exactly one of: {joined}.");
    }
    let p = plain(m);
    let mut out = if p != m { p } else { m.to_string() }.replace(
        "(add it under [widgets.fields])",
        "(add it on the Fields tab)",
    );
    while let Some(a) = out.find('`') {
        let Some(b) = out[a + 1..].find('`') else {
            break;
        };
        let k = out[a + 1..a + 1 + b].to_string();
        out.replace_range(a..a + b + 2, &format!("“{}”", label_of(&k)));
    }
    let mut c = out.chars();
    let mut out = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect::<String>())
        .unwrap_or_default();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// The key of the last `key =` before column `col` (1-based) of a TOML line:
/// `refresh = "abc"` and `{ name = "x", url = "bad" }` both work.
pub fn key_before(line: &str, col: usize) -> Option<&str> {
    let end = line
        .char_indices()
        .nth(col.saturating_sub(1))
        .map_or(line.len(), |(i, _)| i);
    let upto = &line[..end];
    let eq = upto.rfind('=')?;
    let k = upto[..eq].trim_end();
    let start = k
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map_or(0, |i| i + 1);
    let k = &k[start..];
    (!k.is_empty()).then_some(k)
}

fn object<'a>(v: &'a J, what: &str) -> Result<&'a Map<String, J>, ApiError> {
    v.as_object()
        .ok_or_else(|| ApiError::bad_request(format!("{what} must be a JSON object")))
}

/// `"30s"` or a number of seconds (checked by the config validator).
#[derive(Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum DurJ {
    N(u64),
    S(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum LayoutJ {
    Grid,
    List,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ThemeJ {
    Auto,
    Dark,
    Light,
    Ember,
    Contrast,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct PageShape {
    title: Option<String>,
    greeting: Option<String>,
    columns: Option<u8>,
    layout: Option<LayoutJ>,
    theme: Option<ThemeJ>,
    search_url: Option<String>,
    show_stopped: Option<bool>,
    show_stats: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct GroupShape {
    name: String,
    title: Option<String>,
    columns: Option<u8>,
    layout: Option<LayoutJ>,
    collapsed: Option<bool>,
    icon: Option<String>,
    hide: Option<bool>,
}

/// One `[docker] exclude` pattern as the Hidden containers dialog sends it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatternShape {
    pattern: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum SizeJ {
    Small,
    Normal,
    Wide,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct OverrideShape {
    name: Option<String>,
    group: Option<String>,
    icon: Option<String>,
    url: Option<String>,
    description: Option<String>,
    hide: Option<bool>,
    order: Option<i32>,
    size: Option<SizeJ>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct BookmarkShape {
    name: String,
    url: String,
    group: Option<String>,
    icon: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum OutputJ {
    Json,
    Text,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RowShape {
    label: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct StatusShape {
    field: String,
    ok: Option<Vec<J>>,
}

/// The `custom-api` widget form. There are deliberately no `command` or
/// `env` fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WidgetShape {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    title: Option<String>,
    icon: Option<String>,
    link: Option<String>,
    url: String,
    method: Option<String>,
    body: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    headers_env: Option<BTreeMap<String, String>>,
    headers_file: Option<BTreeMap<String, String>>,
    refresh: Option<DurJ>,
    timeout: Option<DurJ>,
    enabled: Option<bool>,
    output: Option<OutputJ>,
    template: Option<String>,
    status: Option<StatusShape>,
    fields: Option<BTreeMap<String, String>>,
    rows: Option<Vec<RowShape>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ChannelKindJ {
    Webhook,
    Slack,
    Telegram,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ChannelShape {
    name: String,
    #[serde(rename = "type")]
    kind: ChannelKindJ,
    url: Option<String>,
    url_env: Option<String>,
    url_file: Option<String>,
    webhook_url_env: Option<String>,
    webhook_url_file: Option<String>,
    token_env: Option<String>,
    token_file: Option<String>,
    chat_id: Option<J>,
    chat_id_env: Option<String>,
    api_base: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    headers_env: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout: Option<DurJ>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum RuleKindJ {
    Container,
    Http,
    Host,
    Widget,
    #[serde(rename = "log_match")]
    LogMatch,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum MetricJ {
    Disk,
    Memory,
    Temp,
    Cpu,
}

/// Every field of `alerts::RuleConfig`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RuleShape {
    name: String,
    #[serde(rename = "type")]
    kind: RuleKindJ,
    channels: Option<Vec<String>>,
    enabled: Option<bool>,
    #[serde(rename = "for")]
    for_: Option<DurJ>,
    cooldown: Option<DurJ>,
    message: Option<String>,
    recovery: Option<String>,
    containers: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    on: Option<Vec<String>>,
    restarts: Option<u32>,
    window: Option<DurJ>,
    url: Option<String>,
    interval: Option<DurJ>,
    timeout: Option<DurJ>,
    expect_status: Option<u16>,
    contains: Option<String>,
    failures: Option<u32>,
    pattern: Option<String>,
    stack: Option<String>,
    count: Option<u32>,
    metric: Option<MetricJ>,
    disk: Option<String>,
    above: Option<f64>,
    below: Option<f64>,
    widget: Option<String>,
    field: Option<String>,
    equals: Option<J>,
    not_equals: Option<J>,
    on_error: Option<bool>,
}

// ------------------------------------------------------------ requests

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchReq {
    pub base_hash: String,
    #[serde(alias = "page", alias = "history")]
    pub values: J,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludeReq {
    pub base_hash: String,
    pub exclude: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupsReq {
    pub base_hash: String,
    pub groups: Vec<J>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideReq {
    pub base_hash: String,
    pub names: Vec<String>,
    #[serde(default)]
    pub set: Option<J>,
    #[serde(default)]
    pub clear: Vec<String>,
    /// Delete the whole `[containers."<name>"]` table.
    #[serde(default)]
    pub remove: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Add,
    Edit,
    Delete,
    Reorder,
}

/// One request shape for every list: `{base_hash, action, id?, item?, order?}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionReq {
    pub base_hash: String,
    pub action: Action,
    /// The entry to edit or delete: a widget id, rule or channel name, or a
    /// bookmark's 0-based index.
    #[serde(default)]
    pub id: Option<J>,
    #[serde(default)]
    pub item: Option<J>,
    /// For `reorder`: every id (or index) in the new order.
    #[serde(default)]
    pub order: Option<Vec<J>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemReq {
    pub base_hash: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawReq {
    pub base_hash: String,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextReq {
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseReq {
    pub base_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewReq {
    pub item: J,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameReq {
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CssReq {
    pub base_hash: String,
    pub css: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordReq {
    pub current: String,
    pub new: String,
}

// ------------------------------------------------------------ helpers

pub fn parse_doc(cur: &Current) -> Result<DocumentMut, ApiError> {
    doc::parse(&cur.text).map_err(|(msg, pos)| {
        let mut e = ApiError::invalid(
            &format!("the config file has a syntax error ({msg}); fix it in the Config file tab"),
            None,
            pos,
        );
        e.body["file_invalid"] = J::Bool(true);
        e
    })
}

fn engine(e: String) -> ApiError {
    ApiError::bad_request(e)
}

fn draft(doc: &DocumentMut, note: String, fields: &'static [&'static str]) -> Draft {
    Draft {
        text: doc.to_string(),
        note,
        kind: "config.saved",
        fields,
        warnings: Vec::new(),
    }
}

/// Docker container names: `[A-Za-z0-9][A-Za-z0-9_.-]*`, at most 128.
pub fn valid_container_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphanumeric())
        && c.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
        && s.len() <= 128
}

/// Channel names (TOML keys, also shown in messages).
fn valid_channel_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The managed keys an edit touches: those present in the item (`null`
/// included, which removes). An absent key is kept as it is in the file.
fn present(keys: &[&'static str], m: &Map<String, J>) -> Vec<&'static str> {
    keys.iter()
        .copied()
        .filter(|k| m.contains_key(*k))
        .collect()
}

/// Drop `null` members (they mean "not set").
fn strip_nulls(m: &Map<String, J>) -> Map<String, J> {
    m.iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The widget JSON could run a program (see `guard::is_command_capable`).
pub fn is_command_json(w: &J) -> bool {
    w.get("type").and_then(J::as_str) != Some("custom-api")
        || w.get("command").is_some()
        || w.get("env").is_some()
}

// ------------------------------------------------------------ general

/// An `http://` or `https://` link with a host and no spaces (SET-24,
/// SET-35, SET-40).
fn web_link(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    let host = u
        .strip_prefix("http://")
        .or_else(|| u.strip_prefix("https://"))
        .unwrap_or("");
    !host.is_empty() && !host.starts_with('/') && !u.contains(char::is_whitespace)
}

/// A text the form limits (`maxlength`): checked again here, in the words
/// the browser uses.
fn at_most(m: &Map<String, J>, key: &str, max: usize) -> Result<(), ApiError> {
    match m.get(key).and_then(J::as_str) {
        Some(s) if s.chars().count() > max => Err(ApiError::invalid(
            &format!("At most {max} characters."),
            Some(key),
            None,
        )),
        _ => Ok(()),
    }
}

pub fn general(cur: &Current, req: &PatchReq) -> Result<Draft, ApiError> {
    let _: PageShape = shape(&req.values, "page")?;
    let patch = object(&req.values, "page")?;
    // Only what the form sends is checked: a hand-written value left as it
    // is in the file stays (the patch does not touch it).
    at_most(patch, "title", 80)?;
    at_most(patch, "greeting", 200)?;
    if let Some(u) = patch.get("search_url").and_then(J::as_str) {
        if !web_link(u) || !u.contains("{q}") {
            return Err(ApiError::invalid(
                "Use an http:// or https:// link with {q} where the search words go.",
                Some("search_url"),
                None,
            ));
        }
    }
    let mut doc = parse_doc(cur)?;
    let root = doc.as_table_mut();
    let existed = root.contains_key("page");
    let t = doc::table_mut(root, "page", false).map_err(engine)?;
    for (k, v) in patch {
        doc::set_json(t, k, Some(v), Style::Inline).map_err(engine)?;
    }
    if !existed && t.is_empty() {
        root.remove("page");
    }
    Ok(draft(&doc, "general".into(), PAGE_KEYS))
}

/// Retired theme token names that a `custom.css` sets (DESIGN.md §11.2,
/// `docs/features/ui.md`), one sentence each naming the token to set
/// instead. Reading an old name still works (they are aliases); setting one
/// no longer recolours anything. Comments are ignored.
pub fn css_token_warnings(css: &str) -> Vec<String> {
    let mut text = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        text.push_str(&rest[..i]);
        rest = rest[i + 2..]
            .find("*/")
            .map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    text.push_str(rest);
    // `--name:` as a declaration, not a longer name (`--ctl-border:`) and
    // not a read (`var(--ctl)`).
    let sets = |name: &str| {
        text.match_indices(name).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + name.len()..].trim_start();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && after.starts_with(':')
        })
    };
    let mut out = Vec::new();
    for (old, new) in [
        ("--ctl", "--ctl-border"),
        ("--border-2", "--ctl-border"),
        ("--accent-bg", "--accent"),
        ("--pop", "--shadow-2"),
        ("--fs-label", "--fs-micro"),
    ] {
        if sets(old) {
            out.push(format!(
                "custom.css sets {old}, which no longer changes anything: set {new} instead."
            ));
        }
    }
    if sets("--glow") {
        out.push("custom.css sets --glow, which is no longer used: remove it.".into());
    }
    if sets("--border") && !sets("--line") {
        out.push(
            "custom.css sets --border, which now only outlines dialogs, menus and the main panel: \
             set --line as well to recolour panel outlines, table rows and dividers."
                .into(),
        );
    }
    out
}

/// `[history]`: a patch of scalar keys. Which keys exist is decided by the
/// config validator (unknown keys are a 422).
pub fn history(cur: &Current, req: &PatchReq) -> Result<Draft, ApiError> {
    let patch = object(&req.values, "history")?;
    for (k, v) in patch {
        let key_ok = k.len() <= 64
            && k.starts_with(|c: char| c.is_ascii_lowercase())
            && k.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !key_ok {
            return Err(ApiError::bad_request(format!("history: bad key {k:?}")));
        }
        if v.is_array() || v.is_object() {
            return Err(ApiError::bad_request(format!(
                "history.{k}: only strings, numbers, booleans or null"
            )));
        }
    }
    let mut doc = parse_doc(cur)?;
    let root = doc.as_table_mut();
    let existed = root.contains_key("history");
    let t = doc::table_mut(root, "history", false).map_err(engine)?;
    for (k, v) in patch {
        doc::set_json(t, k, Some(v), Style::Inline).map_err(engine)?;
    }
    if !existed && t.is_empty() {
        root.remove("history");
    }
    Ok(draft(
        &doc,
        "history".into(),
        &["enabled", "sample_interval"],
    ))
}

// ------------------------------------------------------------ containers

pub fn docker_exclude(cur: &Current, req: &ExcludeReq) -> Result<Draft, ApiError> {
    if req.exclude.len() > 500 {
        return Err(ApiError::bad_request("exclude: at most 500 patterns"));
    }
    if req
        .exclude
        .iter()
        .any(|p| p.trim().is_empty() || p.len() > 256)
    {
        return Err(ApiError::bad_request(
            "exclude: patterns must be 1-256 characters",
        ));
    }
    let mut doc = parse_doc(cur)?;
    let t = doc::table_mut(doc.as_table_mut(), "docker", false).map_err(engine)?;
    let v = (!req.exclude.is_empty()).then(|| J::from(req.exclude.clone()));
    doc::set_json(t, "exclude", v.as_ref(), Style::Inline).map_err(engine)?;
    if t.is_empty() {
        doc.as_table_mut().remove("docker");
    }
    Ok(draft(&doc, "containers: exclude".into(), DOCKER_FIELDS))
}

/// One pattern of `[docker] exclude` added, changed or removed (the Hidden
/// containers list: `{action, id: index, item: {pattern}}`). The array is
/// edited in place, so the other patterns keep their formatting.
pub fn exclude_action(cur: &Current, req: &CollectionReq) -> Result<Draft, ApiError> {
    let pattern = || -> Result<String, ApiError> {
        let item = req
            .item
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("needs item"))?;
        let p: PatternShape = shape(item, "pattern")?;
        let p = p.pattern.trim().to_string();
        if p.is_empty() {
            return Err(ApiError::invalid("Required.", Some("pattern"), None));
        }
        if p.chars().count() > 256 {
            return Err(ApiError::invalid(
                "At most 256 characters.",
                Some("pattern"),
                None,
            ));
        }
        Ok(p)
    };
    let mut doc = parse_doc(cur)?;
    let t = doc::table_mut(doc.as_table_mut(), "docker", false).map_err(engine)?;
    let not_list = || {
        ApiError::bad_request(
            "`exclude` is not written as a list of patterns in the file; edit it in the Config file tab",
        )
    };
    let list: Vec<String> = match t.get("exclude") {
        None => Vec::new(),
        Some(it) => it
            .as_array()
            .ok_or_else(not_list)?
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(not_list))
            .collect::<Result<_, _>>()?,
    };
    let index = || -> Result<usize, ApiError> {
        req.id
            .as_ref()
            .and_then(J::as_u64)
            .map(|i| i as usize)
            .filter(|i| *i < list.len())
            .ok_or_else(|| ApiError::not_found("no such pattern (any more)"))
    };
    let dup = || {
        ApiError::invalid(
            "This pattern is already in the list.",
            Some("pattern"),
            None,
        )
    };
    let note = match req.action {
        Action::Add => {
            let p = pattern()?;
            if list.contains(&p) {
                return Err(dup());
            }
            match t.get_mut("exclude").and_then(Item::as_array_mut) {
                Some(a) => a.push(p.as_str()),
                None => doc::set_json(t, "exclude", Some(&J::from(vec![p.clone()])), Style::Inline)
                    .map_err(engine)?,
            }
            format!("containers: hide {p}")
        }
        Action::Edit => {
            let (i, p) = (index()?, pattern()?);
            if list.iter().enumerate().any(|(j, x)| j != i && *x == p) {
                return Err(dup());
            }
            if list[i] != p {
                let a = t
                    .get_mut("exclude")
                    .and_then(Item::as_array_mut)
                    .ok_or_else(not_list)?;
                a.replace(i, p.as_str());
            }
            format!("containers: hide {p} (was {})", list[i])
        }
        Action::Delete => {
            let i = index()?;
            let a = t
                .get_mut("exclude")
                .and_then(Item::as_array_mut)
                .ok_or_else(not_list)?;
            a.remove(i);
            // `["a", "b"]` without "a" is `["b"]`, not `[ "b"]`.
            if let Some(first) = a.get_mut(0) {
                if first.decor().prefix().and_then(|p| p.as_str()) == Some(" ") {
                    first.decor_mut().set_prefix("");
                }
            }
            if a.is_empty() {
                t.remove("exclude");
            }
            format!("containers: show {} again", list[i])
        }
        Action::Reorder => return Err(ApiError::bad_request("patterns have no order")),
    };
    // As `docker_exclude`: a `[docker]` left empty goes.
    if t.is_empty() {
        doc.as_table_mut().remove("docker");
    }
    Ok(draft(&doc, note, &["pattern"]))
}

fn str_field<'a>(t: &'a Table, k: &str) -> Option<&'a str> {
    t.get(k).and_then(Item::as_str)
}

fn names_of(aot: &toml_edit::ArrayOfTables, key: &str) -> Vec<String> {
    aot.iter()
        .map(|t| str_field(t, key).unwrap_or("").to_string())
        .collect()
}

/// Note for a whole-list replace: what was added, removed, changed, moved.
fn list_note(label: &str, old: &[String], new: &[String], edited: bool) -> String {
    let added: Vec<&String> = new.iter().filter(|n| !old.contains(n)).collect();
    let removed: Vec<&String> = old.iter().filter(|n| !new.contains(n)).collect();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!(
            "added {}",
            added
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !removed.is_empty() {
        parts.push(format!(
            "removed {}",
            removed
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let kept_old: Vec<&String> = old.iter().filter(|n| new.contains(n)).collect();
    let kept_new: Vec<&String> = new.iter().filter(|n| old.contains(n)).collect();
    if kept_old != kept_new {
        parts.push("reordered".into());
    }
    if parts.is_empty() || edited {
        parts.push("edited".into());
    }
    format!("{label}: {}", parts.join(", "))
}

pub fn groups(cur: &Current, req: &GroupsReq) -> Result<Draft, ApiError> {
    let mut want = Vec::new();
    for (i, g) in req.groups.iter().enumerate() {
        let _: GroupShape = shape(g, &format!("groups[{i}]"))?;
        let m = object(g, "group")?;
        let name = m.get("name").and_then(J::as_str).unwrap_or("").to_string();
        if name.trim().is_empty() {
            return Err(ApiError::bad_request(format!("groups[{i}]: name is empty")));
        }
        if want.iter().any(|(n, _): &(String, _)| *n == name) {
            return Err(ApiError::bad_request(format!(
                "group {name:?} is listed twice"
            )));
        }
        want.push((name, m));
    }
    let mut doc = parse_doc(cur)?;
    let aot = doc::aot_mut(&mut doc, "groups").map_err(engine)?;
    let old = names_of(aot, "name");
    let mut used = vec![false; old.len()];
    let plan: Vec<Option<usize>> = want
        .iter()
        .map(|(n, _)| {
            let i = old.iter().enumerate().position(|(i, o)| o == n && !used[i]);
            if let Some(i) = i {
                used[i] = true;
            }
            i
        })
        .collect();
    let before: Vec<J> = aot.iter().map(|t| doc::table_json(t)).collect();
    let orphan = doc::rebuild_aot(aot, &plan);
    let mut edited = false;
    for (i, (_, m)) in want.iter().enumerate() {
        let t = aot.get_mut(i).expect("rebuilt slot");
        doc::patch_managed(t, m, GROUP_KEYS, Style::Inline).map_err(engine)?;
        if let Some(Some(o)) = plan.get(i) {
            edited |= before[*o] != doc::table_json(t);
        }
    }
    let empty = aot.is_empty();
    if empty {
        doc.as_table_mut().remove("groups");
        doc::keep_orphan_comment(&mut doc, orphan);
    }
    let new: Vec<String> = want.iter().map(|(n, _)| n.clone()).collect();
    Ok(draft(
        &doc,
        list_note("groups", &old, &new, edited),
        GROUP_KEYS,
    ))
}

pub fn container_override(cur: &Current, req: &OverrideReq) -> Result<Draft, ApiError> {
    if req.names.is_empty() || req.names.len() > 500 {
        return Err(ApiError::bad_request("names: 1 to 500 container names"));
    }
    if let Some(n) = req.names.iter().find(|n| !valid_container_name(n)) {
        return Err(ApiError::bad_request(format!(
            "{n:?} is not a valid container name"
        )));
    }
    let empty = Map::new();
    let set = match &req.set {
        Some(v) => {
            let _: OverrideShape = shape(v, "set")?;
            object(v, "set")?
        }
        None => &empty,
    };
    // Checked when the form sends them (only changed fields are sent, so a
    // hand-written value left alone stays editable).
    if set
        .get("url")
        .and_then(J::as_str)
        .is_some_and(|u| !web_link(u))
    {
        return Err(ApiError::invalid(
            "Use a link that starts with http:// or https://.",
            Some("url"),
            None,
        ));
    }
    at_most(set, "name", 80)?;
    at_most(set, "description", 200)?;
    if let Some(k) = req
        .clear
        .iter()
        .find(|k| !OVERRIDE_KEYS.contains(&k.as_str()))
    {
        return Err(ApiError::bad_request(format!("clear: unknown field {k:?}")));
    }
    if let Some(k) = req.clear.iter().find(|k| set.contains_key(k.as_str())) {
        return Err(ApiError::bad_request(format!(
            "{k:?} is both set and cleared"
        )));
    }
    let mut doc = parse_doc(cur)?;
    let root = doc.as_table_mut();
    let ctrs = doc::table_mut(root, "containers", true).map_err(engine)?;
    for name in &req.names {
        if req.remove {
            ctrs.remove(name);
            continue;
        }
        let t = doc::table_mut(ctrs, name, false).map_err(engine)?;
        for (k, v) in set {
            doc::set_json(t, k, Some(v), Style::Inline).map_err(engine)?;
        }
        for k in &req.clear {
            t.remove(k);
        }
        if t.is_empty() {
            ctrs.remove(name);
        }
    }
    if ctrs.is_empty() && ctrs.is_implicit() {
        root.remove("containers");
    }
    let note = match req.names.as_slice() {
        [one] => format!("containers: {one}"),
        many => format!("containers: {} overrides", many.len()),
    };
    Ok(draft(&doc, note, OVERRIDE_KEYS))
}

// ------------------------------------------------------------ lists

/// How one `[[array]]` is edited.
pub struct ListSpec {
    pub key: &'static str,
    /// Note prefix ("widgets", "bookmarks", "alerts").
    pub label: &'static str,
    pub keys: &'static [&'static str],
    pub style: Style,
    /// The field that identifies an entry; `None`: the 0-based index.
    pub id_field: Option<&'static str>,
    /// The field shown in notes.
    pub name_field: &'static str,
}

pub const BOOKMARKS: ListSpec = ListSpec {
    key: "bookmarks",
    label: "bookmarks",
    keys: BOOKMARK_KEYS,
    style: Style::Inline,
    id_field: None,
    name_field: "name",
};
pub const WIDGETS: ListSpec = ListSpec {
    key: "widgets",
    label: "widgets",
    keys: WIDGET_KEYS,
    style: Style::Tables,
    id_field: Some("id"),
    name_field: "id",
};
/// `[[groups]]` one entry at a time (the Groups list on Containers and
/// groups). An edit is a patch, so `columns` and `layout`, which the form
/// no longer shows, stay as they are in the file.
pub const GROUPS: ListSpec = ListSpec {
    key: "groups",
    label: "groups",
    keys: GROUP_KEYS,
    style: Style::Inline,
    id_field: Some("name"),
    name_field: "name",
};
pub const ALERTS: ListSpec = ListSpec {
    key: "alerts",
    label: "alerts",
    keys: RULE_KEYS,
    style: Style::Inline,
    id_field: Some("name"),
    name_field: "name",
};

fn find_entry(aot: &toml_edit::ArrayOfTables, spec: &ListSpec, id: &J) -> Result<usize, ApiError> {
    match spec.id_field {
        None => {
            let i = id
                .as_u64()
                .ok_or_else(|| ApiError::bad_request("id must be the entry's index"))?
                as usize;
            if i < aot.len() {
                Ok(i)
            } else {
                Err(ApiError::not_found(format!("no {} entry #{i}", spec.label)))
            }
        }
        Some(f) => {
            let s = id
                .as_str()
                .ok_or_else(|| ApiError::bad_request(format!("id must be the entry's {f}")))?;
            aot.iter()
                .position(|t| str_field(t, f) == Some(s))
                .ok_or_else(|| ApiError::not_found(format!("no {} entry {s:?}", spec.label)))
        }
    }
}

fn check_widget_item(item: &J) -> Result<Vec<String>, ApiError> {
    let m = object(item, "item")?;
    if m.contains_key("command") || m.contains_key("env") {
        return Err(ApiError::denied(guard::Denied(guard::COMMAND_RULE.into())));
    }
    if let Some(t) = m.get("type").and_then(J::as_str) {
        if t != "custom-api" {
            return Err(ApiError::denied(guard::Denied(format!(
                "only type = \"custom-api\" widgets can be edited here; {}",
                guard::COMMAND_RULE
            ))));
        }
    }
    let _: WidgetShape = shape(item, "widget")?;
    let mut warn = Vec::new();
    secret_refs_ok(m, &mut warn)?;
    Ok(warn)
}

/// Shape check for one list entry (per list). Returns warnings.
fn check_item(spec: &ListSpec, item: &J) -> Result<Vec<String>, ApiError> {
    match spec.key {
        "bookmarks" => {
            let b: BookmarkShape = shape(item, "bookmark")?;
            if b.name.trim().is_empty() {
                return Err(ApiError::invalid("Required.", Some("name"), None));
            }
        }
        "widgets" => return check_widget_item(item),
        "alerts" => {
            let _: RuleShape = shape(item, "rule")?;
        }
        "groups" => {
            let g: GroupShape = shape(item, "group")?;
            if g.name.trim().is_empty() {
                return Err(ApiError::invalid("Required.", Some("name"), None));
            }
            at_most(object(item, "group")?, "title", 80)?;
        }
        _ => {}
    }
    Ok(Vec::new())
}

/// SET-40: a bookmark URL from the form must be a web link (no
/// `javascript:`, no bare words). Checked on add, and on edit only when the
/// URL changed: a hand-written entry such as `url = "/files"` stays editable
/// (the config validator itself still accepts it).
fn bookmark_url(spec: &ListSpec, item: &J, old: Option<&str>) -> Result<(), ApiError> {
    if spec.key != "bookmarks" {
        return Ok(());
    }
    let url = item.get("url").and_then(J::as_str).unwrap_or("");
    if old == Some(url) {
        return Ok(());
    }
    if !web_link(url) {
        return Err(ApiError::invalid(
            "Use a link that starts with http:// or https://.",
            Some("url"),
            None,
        ));
    }
    Ok(())
}

/// "That name is taken", on the field that names the entry.
fn taken(id: &str, field: &str) -> ApiError {
    ApiError::invalid(
        &format!(
            "“{id}” is already in the list; use another {}.",
            label_of(field).to_lowercase()
        ),
        Some(field),
        None,
    )
}

fn locked_entry(spec: &ListSpec, t: &Table) -> bool {
    spec.key == "widgets" && is_command_json(&doc::table_json(t))
}

fn locked_err(t: &Table) -> ApiError {
    let id = str_field(t, "id").unwrap_or("?");
    ApiError::denied(guard::Denied(format!(
        "widget {id:?} is a command widget (locked); {}",
        guard::COMMAND_RULE
    )))
}

pub fn list_action(cur: &Current, spec: &ListSpec, req: &CollectionReq) -> Result<Draft, ApiError> {
    let mut doc = parse_doc(cur)?;
    let aot = doc::aot_mut(&mut doc, spec.key).map_err(engine)?;
    let entry_name = |t: &Table| str_field(t, spec.name_field).unwrap_or("?").to_string();
    let item_id = |item: &J| {
        spec.id_field
            .and_then(|f| item.get(f))
            .and_then(J::as_str)
            .map(str::to_string)
    };
    let mut warnings = Vec::new();
    let note = match req.action {
        Action::Add => {
            let item = req
                .item
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("add needs item"))?;
            warnings.extend(check_item(spec, item)?);
            bookmark_url(spec, item, None)?;
            if let (Some(id), Some(f)) = (item_id(item), spec.id_field) {
                if let Some(t) = aot.iter().find(|t| str_field(t, f) == Some(id.as_str())) {
                    if locked_entry(spec, t) {
                        return Err(locked_err(t));
                    }
                    return Err(taken(&id, f));
                }
            }
            let mut t = Table::new();
            doc::patch_managed(&mut t, object(item, "item")?, spec.keys, spec.style)
                .map_err(engine)?;
            let name = entry_name(&t);
            aot.push(t);
            format!("{}: added {name}", spec.label)
        }
        Action::Edit => {
            let id = req
                .id
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("edit needs id"))?;
            let item = req
                .item
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("edit needs item"))?;
            let i = find_entry(aot, spec, id)?;
            if locked_entry(spec, aot.get(i).unwrap()) {
                return Err(locked_err(aot.get(i).unwrap()));
            }
            warnings.extend(check_item(spec, item)?);
            bookmark_url(spec, item, str_field(aot.get(i).unwrap(), "url"))?;
            if let (Some(new_id), Some(f)) = (item_id(item), spec.id_field) {
                if let Some(t) = aot
                    .iter()
                    .enumerate()
                    .find(|(j, t)| *j != i && str_field(t, f) == Some(new_id.as_str()))
                {
                    if locked_entry(spec, t.1) {
                        return Err(locked_err(t.1));
                    }
                    // A rename onto another entry's name would make two.
                    return Err(taken(&new_id, f));
                }
            }
            let t = aot.get_mut(i).unwrap();
            let old = entry_name(t);
            let m = object(item, "item")?;
            doc::patch_managed(t, m, &present(spec.keys, m), spec.style).map_err(engine)?;
            let new = entry_name(t);
            if old == new {
                format!("{}: edited {new}", spec.label)
            } else if spec.key == "widgets" {
                // Rules that watch the widget follow the new id (the dialog
                // names them and asks before saving).
                let moved = rename_widget_refs(&mut doc, &old, &new);
                if moved.is_empty() {
                    format!("{}: renamed {old} to {new}", spec.label)
                } else {
                    format!(
                        "{}: renamed {old} to {new} (rules {})",
                        spec.label,
                        moved.join(", ")
                    )
                }
            } else {
                format!("{}: renamed {old} to {new}", spec.label)
            }
        }
        Action::Delete => {
            let id = req
                .id
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("delete needs id"))?;
            let i = find_entry(aot, spec, id)?;
            let t = aot.get(i).unwrap();
            if locked_entry(spec, t) {
                // The guard would allow removing it (that only reduces
                // capability), but locked entries are file-only in the form.
                return Err(locked_err(t));
            }
            let name = entry_name(t);
            if spec.key == "widgets" {
                let users = widget_rules(&doc, &name);
                if !users.is_empty() {
                    return Err(in_use("widget", &name, &users));
                }
            }
            let aot = doc::aot_mut(&mut doc, spec.key).map_err(engine)?;
            let plan: Vec<Option<usize>> = (0..aot.len()).filter(|j| *j != i).map(Some).collect();
            let orphan = doc::rebuild_aot(aot, &plan);
            if aot.is_empty() {
                doc.as_table_mut().remove(spec.key);
                doc::keep_orphan_comment(&mut doc, orphan);
            }
            format!("{}: deleted {name}", spec.label)
        }
        Action::Reorder => {
            let order = req
                .order
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("reorder needs order"))?;
            let mut plan = Vec::with_capacity(order.len());
            for id in order {
                let i = find_entry(aot, spec, id)?;
                if plan.contains(&Some(i)) {
                    return Err(ApiError::bad_request("order lists an entry twice"));
                }
                plan.push(Some(i));
            }
            if plan.len() != aot.len() {
                return Err(ApiError::bad_request(format!(
                    "order must list all {} entries",
                    aot.len()
                )));
            }
            doc::rebuild_aot(aot, &plan);
            format!("{}: reordered", spec.label)
        }
    };
    let mut d = draft(&doc, note, spec.keys);
    d.warnings = warnings;
    Ok(d)
}

pub fn system(cur: &Current, req: &SystemReq) -> Result<Draft, ApiError> {
    let mut doc = parse_doc(cur)?;
    let t = doc::table_mut(doc.as_table_mut(), "system", false).map_err(engine)?;
    doc::set_json(t, "enabled", Some(&J::Bool(req.enabled)), Style::Inline).map_err(engine)?;
    let note = format!(
        "widgets: system tiles {}",
        if req.enabled { "on" } else { "off" }
    );
    Ok(draft(&doc, note, SYSTEM_FIELDS))
}

// ------------------------------------------------------------ channels

/// `*_env` must be variable NAMES, `*_file` safe paths; never secret values.
fn secret_refs_ok(m: &Map<String, J>, warnings: &mut Vec<String>) -> Result<(), ApiError> {
    for (k, v) in m {
        if k.ends_with("_env") && k != "headers_env" {
            if let Some(s) = v.as_str() {
                if !guard::valid_env_name(s) {
                    return Err(ApiError::invalid(
                        &format!(
                            "{k} must be the NAME of an environment variable (A-Z, 0-9, _), \
                             not the secret itself"
                        ),
                        Some(k),
                        None,
                    ));
                }
            }
        }
        if k == "headers_env" {
            for (h, n) in v.as_object().into_iter().flatten() {
                if !n.as_str().is_some_and(guard::valid_env_name) {
                    return Err(ApiError::invalid(
                        &format!(
                            "headers_env.{h} must be the NAME of an environment variable \
                             (A-Z, 0-9, _), not the secret itself"
                        ),
                        Some("headers_env"),
                        None,
                    ));
                }
            }
        }
        if k.ends_with("_file") && k != "headers_file" {
            if let Some(p) = v.as_str() {
                guard::check_secret_path(p).map_err(|e| ApiError::invalid(&e, Some(k), None))?;
            }
        }
        if k == "headers_file" {
            for p in v
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, p)| p.as_str())
            {
                guard::check_secret_path(p)
                    .map_err(|e| ApiError::invalid(&e, Some("headers_file"), None))?;
            }
        }
        if k == "headers" {
            for h in v.as_object().into_iter().flatten().map(|(h, _)| h) {
                let l = h.to_ascii_lowercase();
                if ["authorization", "token", "key", "secret", "cookie"]
                    .iter()
                    .any(|s| l.contains(s))
                {
                    warnings.push(format!(
                        "header {h} is stored in the config file as plain text; if it is a \
                         secret, use headers_env (an environment variable name) instead"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// A checked channel form: name, table fields (no name), warnings.
type CheckedChannel = (String, Map<String, J>, Vec<String>);

fn check_channel(item: &J) -> Result<CheckedChannel, ApiError> {
    let _: ChannelShape = shape(item, "channel")?;
    let m = object(item, "channel")?;
    let name = m.get("name").and_then(J::as_str).unwrap_or("").to_string();
    if !valid_channel_name(&name) {
        return Err(ApiError::invalid(
            "channel name: 1-64 characters of A-Z, a-z, 0-9, '-', '_'",
            Some("name"),
            None,
        ));
    }
    let mut warnings = Vec::new();
    secret_refs_ok(m, &mut warnings)?;
    if m.get("url").is_some_and(|u| !u.is_null()) {
        warnings.push(
            "The URL is stored in the config file and its revisions as plain text; if it contains \
             a token, use a variable or a file instead."
                .into(),
        );
    }
    let mut fields = strip_nulls(m);
    fields.remove("name");
    Ok((name, fields, warnings))
}

pub fn channel_action(cur: &Current, req: &CollectionReq) -> Result<Draft, ApiError> {
    let mut doc = parse_doc(cur)?;
    let root = doc.as_table_mut();
    let existed = root.contains_key("channels");
    let chans = doc::table_mut(root, "channels", true).map_err(engine)?;
    let id = || -> Result<String, ApiError> {
        req.id
            .as_ref()
            .and_then(J::as_str)
            .map(str::to_string)
            .ok_or_else(|| ApiError::bad_request("id must be the channel name"))
    };
    let mut warnings = Vec::new();
    let note = match req.action {
        Action::Add => {
            let item = req
                .item
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("add needs item"))?;
            let (name, fields, w) = check_channel(item)?;
            warnings = w;
            if chans.contains_key(&name) {
                return Err(ApiError::invalid(
                    &format!("channel {name:?} already exists"),
                    Some("name"),
                    None,
                ));
            }
            let mut t = Table::new();
            doc::patch_managed(&mut t, &fields, CHANNEL_KEYS, Style::Inline).map_err(engine)?;
            chans.insert(&name, Item::Table(t));
            format!("notifications: added channel {name}")
        }
        Action::Edit => {
            let old = id()?;
            let item = req
                .item
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("edit needs item"))?;
            let (name, fields, w) = check_channel(item)?;
            let sent = present(CHANNEL_KEYS, object(item, "channel")?);
            warnings = w;
            if !chans.contains_key(&old) {
                return Err(ApiError::not_found(format!("no channel {old:?}")));
            }
            if name != old && chans.contains_key(&name) {
                return Err(ApiError::invalid(
                    &format!("channel {name:?} already exists"),
                    Some("name"),
                    None,
                ));
            }
            let t = doc::table_mut(chans, &old, false).map_err(engine)?;
            doc::patch_managed(t, &fields, &sent, Style::Inline).map_err(engine)?;
            if name != old {
                // Move the table (decor and position go with it) and keep
                // the rules that send to it pointing at it.
                let (_, item) = chans.remove_entry(&old).unwrap();
                chans.insert_formatted(&Key::new(name.as_str()), item);
                rename_channel_refs(&mut doc, &old, &name);
                format!("notifications: renamed channel {old} to {name}")
            } else {
                format!("notifications: edited channel {name}")
            }
        }
        Action::Delete => {
            let name = id()?;
            if !chans.contains_key(&name) {
                return Err(ApiError::not_found(format!("no channel {name:?}")));
            }
            let users = rules_naming(&doc, "channels", &name);
            if !users.is_empty() {
                return Err(in_use("channel", &name, &users));
            }
            let chans = doc::table_mut(doc.as_table_mut(), "channels", true).map_err(engine)?;
            chans.remove(&name);
            format!("notifications: deleted channel {name}")
        }
        Action::Reorder => {
            return Err(ApiError::bad_request("channels have no order"));
        }
    };
    let root = doc.as_table_mut();
    if let Some(Item::Table(c)) = root.get("channels") {
        if c.is_empty() && (c.is_implicit() || !existed) {
            root.remove("channels");
        }
    }
    let mut d = draft(&doc, note, CHANNEL_FIELDS);
    d.warnings = warnings;
    Ok(d)
}

/// Names of the rules whose `key` (a list, `channels`) contains `name`.
fn rules_naming(doc: &DocumentMut, key: &str, name: &str) -> Vec<String> {
    let Some(Item::ArrayOfTables(rules)) = doc.as_table().get("alerts") else {
        return Vec::new();
    };
    rules
        .iter()
        .filter(|r| {
            r.get(key)
                .and_then(Item::as_array)
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)))
        })
        .map(|r| str_field(r, "name").unwrap_or("?").to_string())
        .collect()
}

/// Names of the widget rules that watch the widget `id`.
fn widget_rules(doc: &DocumentMut, id: &str) -> Vec<String> {
    let Some(Item::ArrayOfTables(rules)) = doc.as_table().get("alerts") else {
        return Vec::new();
    };
    rules
        .iter()
        .filter(|r| str_field(r, "widget") == Some(id))
        .map(|r| str_field(r, "name").unwrap_or("?").to_string())
        .collect()
}

/// A delete refused because rules still refer to the entry: says which, in
/// plain words (422, no field: it is not about a form field).
fn in_use(what: &str, name: &str, rules: &[String]) -> ApiError {
    let (noun, list) = match rules {
        [one] => ("the rule", format!("“{one}”")),
        many => (
            "the rules",
            many.iter()
                .map(|r| format!("“{r}”"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    };
    let fix = if what == "channel" {
        "Take it out of those rules first (or delete them), then delete the channel."
    } else {
        "Change or delete those rules first, then delete the widget."
    };
    let mut e = ApiError::invalid(
        &format!("The {what} “{name}” is used by {noun} {list}. {fix}"),
        None,
        None,
    );
    e.body["used_by"] = J::from(rules.to_vec());
    e
}

/// `widget = "old"` in every rule becomes `new` (formatting kept). Returns
/// the names of the rules changed.
fn rename_widget_refs(doc: &mut DocumentMut, old: &str, new: &str) -> Vec<String> {
    let Some(Item::ArrayOfTables(rules)) = doc.as_table_mut().get_mut("alerts") else {
        return Vec::new();
    };
    let mut moved = Vec::new();
    for r in rules.iter_mut() {
        if str_field(r, "widget") != Some(old) {
            continue;
        }
        if let Some(v) = r.get_mut("widget").and_then(Item::as_value_mut) {
            let decor = v.decor().clone();
            let mut nv = Value::from(new);
            *nv.decor_mut() = decor;
            *v = nv;
        }
        moved.push(str_field(r, "name").unwrap_or("?").to_string());
    }
    moved
}

/// `channels = [...]` in every rule: `old` becomes `new` (formatting kept).
fn rename_channel_refs(doc: &mut DocumentMut, old: &str, new: &str) {
    let Some(Item::ArrayOfTables(rules)) = doc.as_table_mut().get_mut("alerts") else {
        return;
    };
    for r in rules.iter_mut() {
        if let Some(arr) = r.get_mut("channels").and_then(Item::as_array_mut) {
            for i in 0..arr.len() {
                if arr.get(i).and_then(Value::as_str) == Some(old) {
                    let decor = arr.get(i).unwrap().decor().clone();
                    let mut v = Value::from(new);
                    *v.decor_mut() = decor;
                    arr.replace_formatted(i, v);
                }
            }
        }
    }
}

// ------------------------------------------------------------ preview

/// The (unsaved) widget form as a `WidgetConfig`, for "Fetch now".
pub fn preview_widget(item: &J) -> Result<crate::widgets::WidgetConfig, ApiError> {
    check_widget_item(item)?;
    let m = strip_nulls(object(item, "item")?);
    let mut w: crate::widgets::WidgetConfig = serde_json::from_value(J::Object(m))
        .map_err(|e| ApiError::invalid(&format!("widget: {e}"), None, None))?;
    // The same field and words as a save's check (`precheck`).
    crate::widgets::validate(std::slice::from_ref(&w)).map_err(|e| match strip_owner(&e) {
        Some(rest) => ApiError::invalid(
            &sentence(rest),
            entry_field(rest, WIDGET_KEYS).as_deref(),
            None,
        ),
        None => {
            let field = super::store::field_in(&e, WIDGET_KEYS);
            ApiError::invalid(&e, field.as_deref(), None)
        }
    })?;
    // Preview even a disabled widget, and never wait long.
    w.enabled = None;
    let cap = std::time::Duration::from_secs(15);
    if w.timeout.0 > cap {
        w.timeout = crate::util::Dur(cap);
    }
    Ok(w)
}
