//! Custom widgets: `custom-api` (fetch JSON, pick fields, render a template)
//! and `command` (run a program, show its JSON output; off unless enabled).
//!
//! # Config reference
//!
//! ```toml
//! [[widgets]]
//! id = "mailpit"                 # unique, [A-Za-z0-9_-]; DOM id and alert-rule key
//! type = "custom-api"            # or "command"
//! title = "Mailpit"              # default: the id
//! link = "http://100.64.0.10:8025"   # optional, makes the title a link
//! icon = "mailpit.svg"           # optional, a file in data/icons/
//! refresh = "30s"                # default 30s, minimum 1s
//! timeout = "5s"                 # default 5s
//! # enabled = false              # turn a widget off without deleting it
//!
//! # --- custom-api ---
//! url = "http://100.64.0.10:8025/api/v1/info"
//! # method = "GET"               # any method; default GET
//! # body = '{"query": "..."}'    # request body (content-type defaults to JSON)
//! # headers = { Accept = "application/json" }
//! # Secrets never go in the config. Header values can come from an
//! # environment variable (give its NAME) or a file (first line, trimmed):
//! # headers_env = { Authorization = "MAILPIT_AUTH" }
//! # headers_file = { Authorization = "/data/secrets/mailpit" }
//!
//! # --- command (disabled unless `enabled = true`) ---
//! # command = ["sh", "-c", "echo '{\"a\": 1}'"]   # argv, no implicit shell
//! # env = { FOO = "bar" }        # the environment is cleared except PATH
//! # Runs in /data (if it exists); stdout is capped at 256 KiB; the timeout
//! # kills the whole process group; a non-zero exit is an error.
//!
//! # --- both ---
//! # output = "json"              # or "text": show stdout/body as-is (`{{_text}}`)
//!
//! [widgets.fields]               # name = path into the JSON
//! messages = "Messages"
//! unread = "Unread"
//! size = "DatabaseSize"
//! uptime = "RuntimeStats.Uptime"
//!
//! # How to show it. All optional:
//! # template = "{{unread}} unread"   # one big headline value
//! [[widgets.rows]]                   # label / value rows under it
//! label = "Unread"
//! value = "{{unread | number}} / {{messages | number}}"
//! [[widgets.rows]]
//! label = "DB"
//! value = "{{size | bytes}}"
//! # With neither template nor rows: one row per field, sorted by name
//! # (no fields either: the raw JSON, shortened).
//!
//! # Status dot: which field decides, and the values that mean "ok".
//! # Without `ok`, any truthy value (true, non-zero, non-empty) is ok.
//! # status = { field = "status", ok = ["pass"] }
//! ```
//!
//! **Paths** (see `path.rs`): `a.b.c`, `a[0]`, `a[-1]`, `a.0`, `a.#` (length),
//! `a."x.y"` / `a["x.y"]` for odd keys, `""` or `$` for the whole document.
//! A path that matches nothing gives `null`, shown as "—".
//!
//! **Templates** (see `template.rs`): `{{name}}` or `{{name | filter | …}}` with
//! filters `bytes`, `duration` (seconds), `round:N`, `percent`, `number`,
//! `upper`, `lower`, `default:"x"`. Besides the fields, templates and the
//! status rule can use `_status` (HTTP status or exit code), `_ms` (latency)
//! and, with `output = "text"`, `_text`.
//!
//! Unknown fields, filters, bad paths and duplicate ids are config errors that
//! name the widget, so a typo keeps the last good config instead of silently
//! showing nothing. At runtime a failing widget shows a compact error; it
//! never breaks the page or the other widgets.

mod path;
pub(crate) mod template;

#[cfg(test)]
mod tests;

pub use path::Path;
pub use template::Template;

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::config::HttpPolicy;

pub const DISABLED: &str = "disabled — set enabled = true";
/// Largest stdout a command widget may produce.
pub const MAX_STDOUT: usize = 256 * 1024;
/// How much stderr is kept for the error message.
const MAX_STDERR: usize = 4096;
/// `_text` is cut to this length before presentation and rule evaluation.
const MAX_TEXT_VALUE: usize = 8192;
/// Raw JSON shown when a widget has no fields.
const MAX_RAW_JSON: usize = 400;
/// Largest secret file read for `headers_file`.
const MAX_SECRET_FILE: u64 = 16 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetConfig {
    /// Unique id (used in the DOM and by alert rules).
    pub id: String,
    /// "custom-api" or "command".
    #[serde(rename = "type")]
    pub kind: String,
    pub title: Option<String>,
    pub icon: Option<String>,
    /// Link for the widget title.
    pub link: Option<String>,
    #[serde(default = "default_refresh")]
    pub refresh: crate::util::Dur,
    #[serde(default = "default_timeout")]
    pub timeout: crate::util::Dur,
    /// custom-api: default on. command: default off, must be `true`.
    pub enabled: Option<bool>,

    // custom-api
    pub url: Option<String>,
    pub method: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Header name → environment variable NAME holding the value.
    #[serde(default)]
    pub headers_env: BTreeMap<String, String>,
    /// Header name → file holding the value.
    #[serde(default)]
    pub headers_file: BTreeMap<String, String>,

    // command
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    // both
    #[serde(default)]
    pub output: Output,
    /// `[widgets.fields]`: name → path into the JSON.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    pub template: Option<String>,
    #[serde(default)]
    pub rows: Vec<RowConfig>,
    pub status: Option<StatusConfig>,
}

fn default_refresh() -> crate::util::Dur {
    crate::util::Dur::secs(30)
}
fn default_timeout() -> crate::util::Dur {
    crate::util::Dur::secs(5)
}

impl WidgetConfig {
    /// Command widgets need `enabled = true`; others can be switched off.
    pub fn is_enabled(&self) -> bool {
        match self.kind.as_str() {
            "command" => self.enabled == Some(true),
            _ => self.enabled != Some(false),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    #[default]
    Json,
    Text,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowConfig {
    pub label: String,
    /// A template, e.g. `"{{size | bytes}}"`.
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusConfig {
    /// Field (or `_status`) that decides ok / bad.
    pub field: String,
    /// Values that mean ok (compared as text, so `200` matches `"200"`).
    #[serde(default)]
    pub ok: Vec<Value>,
}

/// The structured display body, with all values as text.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WidgetBody {
    pub headline: Option<String>,
    pub rows: Vec<(String, String)>,
    pub text: Option<String>,
}

/// Result of one widget run.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WidgetOutput {
    #[serde(skip)]
    pub display: WidgetBody,
    /// Extracted values by name, for alert rules (`widget` rules read these).
    /// Every field, plus `_status` (HTTP status / exit code), `_ms`
    /// (latency), `_ok` (status rule result, when one is set) and `_text`
    /// (text output).
    pub values: BTreeMap<String, Value>,
    /// Error message on failure (shown on the widget, never breaks the page).
    pub error: Option<String>,
    /// Unix seconds.
    pub fetched_at: u64,
    pub duration_ms: u64,
    /// Timestamp of retained display data, present only after a refresh fails.
    #[serde(skip)]
    pub stale_since: Option<u64>,
}

// Retained display and alert values share one cache. Reserve space for a
// later error beside the last good data. The aggregate is 1.25% of 20 MiB.
const WIDGET_OUTPUT_BYTES: usize = 32 * 1024;
const WIDGET_CACHE_BYTES: usize = 256 * 1024;
const WIDGET_ERROR_BYTES: usize = 512;

fn map_bytes<'a>(len: usize, entries: impl Iterator<Item = (&'a String, &'a Value)>) -> usize {
    // Allow for the tree root, node slack and links as well as the entries.
    let overhead = if len == 0 { 0 } else { 1024 + 256 * len };
    overhead
        + entries
            .map(|(key, value)| key.capacity() + value_bytes(value))
            .sum::<usize>()
}

fn value_bytes(value: &Value) -> usize {
    match value {
        Value::String(s) => s.capacity(),
        Value::Array(values) => {
            values.capacity() * size_of::<Value>() + values.iter().map(value_bytes).sum::<usize>()
        }
        Value::Object(values) => map_bytes(values.len(), values.iter()),
        _ => 0,
    }
}

impl WidgetOutput {
    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + self.display.rows.capacity() * size_of::<(String, String)>()
            + self
                .display
                .rows
                .iter()
                .map(|(label, value)| label.capacity() + value.capacity())
                .sum::<usize>()
            + [&self.display.headline, &self.display.text, &self.error]
                .into_iter()
                .flatten()
                .map(String::capacity)
                .sum::<usize>()
            + map_bytes(self.values.len(), self.values.iter())
    }

    fn bound(&mut self) {
        if let Some(error) = &mut self.error {
            if error.capacity() > WIDGET_ERROR_BYTES {
                *error = error[..error.floor_char_boundary(WIDGET_ERROR_BYTES)].to_owned();
            }
        }
        if self.retained_bytes() > WIDGET_OUTPUT_BYTES - WIDGET_ERROR_BYTES {
            self.display = WidgetBody::default();
            self.values.clear();
            self.error = Some("Widget output exceeded the 32 KiB retained data limit.".into());
        }
    }

    pub(crate) fn record(&mut self, mut next: Self) {
        if next.error.is_some() {
            let last_good = self
                .stale_since
                .or_else(|| self.error.is_none().then_some(self.fetched_at));
            if let Some(at) = last_good {
                next.display = std::mem::take(&mut self.display);
                next.values = std::mem::take(&mut self.values);
                next.duration_ms = self.duration_ms;
                next.stale_since = Some(at);
            }
        }
        *self = next;
    }
}

#[derive(Default)]
pub(crate) struct WidgetCache {
    slots: std::collections::HashMap<String, WidgetSlot>,
    retained_bytes: usize,
}

#[derive(Default)]
struct WidgetSlot {
    started: u64,
    completed: Option<(u64, Instant)>,
    output: Option<Box<WidgetOutput>>,
    bytes: usize,
}

impl WidgetCache {
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.retained_bytes = 0;
    }

    pub(crate) fn get(&self, id: &str) -> Option<&WidgetOutput> {
        self.slots.get(id)?.output.as_deref()
    }

    pub(crate) fn deadline(&self, id: &str, refresh: Duration) -> Option<Instant> {
        let (_, completed) = self.slots.get(id)?.completed?;
        Some(refresh_deadline(completed, refresh))
    }

    pub(crate) fn begin(&mut self, id: &str) -> u64 {
        let slot = self.slots.entry(id.to_owned()).or_default();
        slot.started += 1;
        slot.started
    }

    pub(crate) fn record(
        &mut self,
        id: &str,
        attempt: u64,
        completed: Instant,
        mut out: WidgetOutput,
    ) -> bool {
        let Some(slot) = self.slots.get_mut(id) else {
            return false;
        };
        if slot.completed.is_some_and(|(latest, _)| latest >= attempt) {
            return false;
        }
        slot.completed = Some((attempt, completed));
        out.bound();
        self.retained_bytes -= slot.bytes;
        match &mut slot.output {
            Some(current) => current.record(out),
            None => slot.output = Some(Box::new(out)),
        }
        slot.bytes = slot
            .output
            .as_deref()
            .map_or(0, WidgetOutput::retained_bytes);
        self.retained_bytes += slot.bytes;
        while self.retained_bytes > WIDGET_CACHE_BYTES {
            let oldest = self
                .slots
                .iter_mut()
                .filter(|(other, slot)| other.as_str() != id && slot.output.is_some())
                .min_by_key(|(_, slot)| slot.completed.map(|(_, at)| at))
                .expect("aggregate budget exceeds the single-widget budget")
                .1;
            self.retained_bytes -= std::mem::take(&mut oldest.bytes);
            oldest.output = None;
        }
        true
    }
}

pub(crate) fn refresh_deadline(now: Instant, refresh: Duration) -> Instant {
    // Durations too large to represent as a deadline are effectively never due.
    now.checked_add(refresh)
        .unwrap_or_else(|| now + Duration::from_secs(100 * 365 * 24 * 3600))
}

/// Parsed paths and templates of one widget.
struct Compiled {
    fields: Vec<(String, Path)>,
    template: Option<Template>,
    rows: Vec<(String, Template)>,
}

fn compile(w: &WidgetConfig) -> Result<Compiled, String> {
    let mut known: HashSet<&str> = ["_status", "_ms"].into_iter().collect();
    if w.output == Output::Text {
        known.insert("_text");
    }
    let mut fields = Vec::new();
    for (name, p) in &w.fields {
        if !template::valid_name(name) || name.starts_with('_') {
            return Err(format!(
                "field name {name:?}: use letters, digits, '-' and '_' (not starting with '_')"
            ));
        }
        let path = Path::parse(p).map_err(|e| format!("field {name}: path {p:?}: {e}"))?;
        fields.push((name.clone(), path));
        known.insert(name);
    }
    let check = |t: &Template, what: &str| -> Result<(), String> {
        match t.names().find(|n| !known.contains(n)) {
            Some(n) => Err(format!(
                "{what}: unknown field {n:?} (add it under [widgets.fields])"
            )),
            None => Ok(()),
        }
    };
    let template = match &w.template {
        Some(t) => {
            let t = Template::parse(t).map_err(|e| format!("template: {e}"))?;
            check(&t, "template")?;
            Some(t)
        }
        None => None,
    };
    let mut rows = Vec::new();
    for r in &w.rows {
        let what = format!("row {:?}", r.label);
        let t = Template::parse(&r.value).map_err(|e| format!("{what}: {e}"))?;
        check(&t, &what)?;
        rows.push((r.label.clone(), t));
    }
    if let Some(s) = &w.status {
        if !known.contains(s.field.as_str()) {
            return Err(format!(
                "status: unknown field {:?} (add it under [widgets.fields])",
                s.field
            ));
        }
    }
    Ok(Compiled {
        fields,
        template,
        rows,
    })
}

pub fn validate(widgets: &[WidgetConfig]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for w in widgets {
        if !seen.insert(w.id.as_str()) {
            return Err(format!("duplicate widget id {:?}", w.id));
        }
        validate_one(w).map_err(|e| format!("widget {:?}: {e}", w.id))?;
    }
    Ok(())
}

fn validate_one(w: &WidgetConfig) -> Result<(), String> {
    if w.id.is_empty()
        || w.id.len() > 64
        || !w
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("id must be 1-64 characters of A-Z, a-z, 0-9, '-', '_'".into());
    }
    if w.refresh.0 < Duration::from_secs(1) {
        return Err("refresh must be at least 1s".into());
    }
    if w.timeout.0 < Duration::from_millis(100) || w.timeout.0 > Duration::from_secs(300) {
        return Err("timeout must be between 100ms and 5m".into());
    }
    if let Some(l) = &w.link {
        if !safe_link(l) {
            return Err("link must start with http://, https:// or /".into());
        }
    }
    let http_only = w.url.is_some()
        || w.method.is_some()
        || w.body.is_some()
        || !w.headers.is_empty()
        || !w.headers_env.is_empty()
        || !w.headers_file.is_empty();
    match w.kind.as_str() {
        "custom-api" => {
            let url = w.url.as_deref().ok_or("custom-api needs url")?;
            let lower = url.to_ascii_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://"))
                || url.parse::<hyper::Uri>().is_err()
            {
                return Err("url must be a valid http:// or https:// URL".into());
            }
            if let Some(m) = &w.method {
                hyper::Method::from_bytes(m.to_ascii_uppercase().as_bytes())
                    .map_err(|_| format!("invalid method {m:?}"))?;
            }
            for k in w
                .headers
                .keys()
                .chain(w.headers_env.keys())
                .chain(w.headers_file.keys())
            {
                hyper::header::HeaderName::from_bytes(k.as_bytes())
                    .map_err(|_| format!("invalid header name {k:?}"))?;
            }
            for (k, v) in &w.headers {
                hyper::header::HeaderValue::from_str(v)
                    .map_err(|_| format!("header {k}: invalid value"))?;
            }
            if !w.command.is_empty() || !w.env.is_empty() {
                return Err("command and env are only for type = \"command\"".into());
            }
        }
        "command" => {
            if w.command.first().is_none_or(|c| c.is_empty()) {
                return Err("command widget needs command = [\"program\", \"arg\", …]".into());
            }
            if http_only {
                return Err(
                    "url, method, body and headers* are only for type = \"custom-api\"".into(),
                );
            }
        }
        other => {
            return Err(format!(
                "unknown type {other:?} (expected \"custom-api\" or \"command\")"
            ))
        }
    }
    if w.output == Output::Text && !w.fields.is_empty() {
        return Err("fields need output = \"json\" (with \"text\", use {{_text}})".into());
    }
    compile(w).map(|_| ())
}

pub fn safe_link(l: &str) -> bool {
    let lower = l.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || (lower.starts_with('/') && !matches!(lower.as_bytes().get(1), Some(b'/' | b'\\')))
}

/// What a fetch produced before fields are extracted.
enum Data {
    Json(Value),
    Text(String),
}

struct Fetched {
    code: i64,
    data: Data,
}

struct Failure {
    code: Option<i64>,
    msg: String,
}

impl Failure {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            code: None,
            msg: msg.into(),
        }
    }
}

/// Run one widget. Never panics; all failures become `WidgetOutput.error`.
pub async fn run(w: &WidgetConfig, policy: &HttpPolicy) -> WidgetOutput {
    let t0 = Instant::now();
    let mut out = WidgetOutput {
        fetched_at: crate::util::unix_now(),
        ..Default::default()
    };
    if !w.is_enabled() {
        out.error = Some(DISABLED.into());
        return out;
    }
    let compiled = match compile(w) {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    // The fetchers enforce `timeout` themselves; this is the safety net.
    let limit = w.timeout.0 + Duration::from_millis(250);
    let fut = async {
        match w.kind.as_str() {
            "custom-api" => fetch_api(w, policy).await,
            "command" => run_command(w).await,
            other => Err(Failure::new(format!("unknown widget type {other:?}"))),
        }
    };
    let res = tokio::time::timeout(limit, fut)
        .await
        .unwrap_or_else(|_| Err(Failure::new(timeout_msg(w.timeout.0))));
    let ms = t0.elapsed().as_millis() as u64;
    out.duration_ms = ms;
    out.values.insert("_ms".into(), Value::from(ms));
    match res {
        Ok(f) => {
            out.values.insert("_status".into(), Value::from(f.code));
            present(w, &compiled, f.data, &mut out);
        }
        Err(f) => {
            if let Some(c) = f.code {
                out.values.insert("_status".into(), Value::from(c));
            }
            if w.status.is_some() {
                out.values.insert("_ok".into(), Value::Bool(false));
            }
            out.error = Some(f.msg);
        }
    }
    out
}

fn timeout_msg(d: Duration) -> String {
    format!("timed out after {}ms", d.as_millis())
}

/// Extract the fields, evaluate the status rule and render the body.
fn present(w: &WidgetConfig, c: &Compiled, data: Data, out: &mut WidgetOutput) {
    let raw = match data {
        Data::Json(root) => {
            for (name, p) in &c.fields {
                out.values.insert(name.clone(), p.eval(&root));
            }
            if c.fields.is_empty() {
                Some(shorten(&root.to_string(), MAX_RAW_JSON))
            } else {
                None
            }
        }
        Data::Text(t) => {
            let t = shorten(t.trim_end(), MAX_TEXT_VALUE);
            out.values.insert("_text".into(), Value::String(t.clone()));
            Some(t)
        }
    };
    if let Some(s) = &w.status {
        let v = out.values.get(&s.field).unwrap_or(&Value::Null);
        let ok = if s.ok.is_empty() {
            truthy(v)
        } else {
            !v.is_null() && s.ok.iter().any(|o| template::text(o) == template::text(v))
        };
        out.values.insert("_ok".into(), Value::Bool(ok));
    }

    let headline = c.template.as_ref().map(|t| t.render(&out.values));
    let rows: Vec<(String, String)> = if !c.rows.is_empty() {
        c.rows
            .iter()
            .map(|(l, t)| (l.clone(), t.render(&out.values)))
            .collect()
    } else if headline.is_none() {
        c.fields
            .iter()
            .map(|(name, _)| {
                let v = out.values.get(name).unwrap_or(&Value::Null);
                (name.clone(), template::display(v))
            })
            .collect()
    } else {
        vec![]
    };
    let pre = raw.filter(|_| headline.is_none() && rows.is_empty());
    out.display = WidgetBody {
        headline,
        rows,
        text: pre,
    };
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Cut to `max` chars (on a char boundary), marking the cut with "…".
pub(crate) fn shorten(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Collapse whitespace and shorten, for stderr in error messages.
fn one_line(b: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(b);
    shorten(&s.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

async fn fetch_api(w: &WidgetConfig, policy: &HttpPolicy) -> Result<Fetched, Failure> {
    use crate::http::client;
    let mut req = client::Request::get(w.url.clone().unwrap_or_default()).timeout(w.timeout.0);
    if let Some(m) = &w.method {
        req.method = hyper::Method::from_bytes(m.to_ascii_uppercase().as_bytes())
            .map_err(|_| Failure::new(format!("invalid method {m:?}")))?;
    }
    let has = |name: &str| {
        w.headers
            .keys()
            .chain(w.headers_env.keys())
            .chain(w.headers_file.keys())
            .any(|k| k.eq_ignore_ascii_case(name))
    };
    if let Some(b) = &w.body {
        req.body = Some(b.clone().into_bytes());
        if !has("content-type") {
            req = req.header("content-type", "application/json");
        }
    }
    for (k, v) in &w.headers {
        req = req.header(k, v);
    }
    // Secret values: never put them in an error message or a log line, and
    // never let them come back in what the destination answers (one that
    // reflects request headers: TRACE, a debug or error page).
    let mut secrets = Vec::new();
    for (k, var) in &w.headers_env {
        let v = std::env::var(var).map_err(|_| {
            Failure::new(format!("header {k}: environment variable {var} is not set"))
        })?;
        let v = secret_value(k, v.trim())?;
        req = req.header(k, &v);
        secrets.push(v);
    }
    for (k, file) in &w.headers_file {
        let v = read_secret(file)
            .map_err(|e| Failure::new(format!("header {k}: cannot read {file}: {e}")))?;
        let v = secret_value(k, &v)?;
        req = req.header(k, &v);
        secrets.push(v);
    }
    let scrub = Scrub::new(&secrets);
    let resp = client::send(req, policy)
        .await
        .map_err(|e| Failure::new(scrub.text(&e)))?;
    let code = i64::from(resp.status);
    if !resp.is_success() {
        let reason = hyper::StatusCode::from_u16(resp.status)
            .ok()
            .and_then(|s| s.canonical_reason())
            .map(|r| format!(" {r}"))
            .unwrap_or_default();
        return Err(Failure {
            code: Some(code),
            msg: format!("HTTP {code}{reason}"),
        });
    }
    let data = match w.output {
        Output::Json => {
            let mut v: Value = serde_json::from_slice(&resp.body).map_err(|e| Failure {
                code: Some(code),
                msg: scrub.text(&format!("response is not JSON ({e})")),
            })?;
            scrub.json(&mut v);
            Data::Json(v)
        }
        Output::Text => Data::Text(scrub.text(&resp.text())),
    };
    Ok(Fetched { code, data })
}

/// Replaces the values of a widget's secret headers in what came back
/// (before anything is extracted, shown, stored or logged).
struct Scrub(Vec<String>);

const REDACTED: &str = "<redacted>";

impl Scrub {
    /// Each value, the credentials after its scheme (`Bearer x` → `x`) and
    /// its JSON-escaped form; the longest first.
    fn new(secrets: &[String]) -> Self {
        let mut all: Vec<String> = Vec::new();
        for v in secrets.iter().map(|v| v.trim()).filter(|v| !v.is_empty()) {
            all.push(v.to_string());
            if let Some((_, cred)) = v.split_once(' ') {
                let cred = cred.trim();
                if cred.chars().count() >= 4 {
                    all.push(cred.to_string());
                }
            }
            let esc = Value::from(v).to_string();
            let esc = &esc[1..esc.len() - 1];
            if esc != v {
                all.push(esc.to_string());
            }
        }
        all.sort_by_key(|s| std::cmp::Reverse(s.len()));
        all.dedup();
        Self(all)
    }

    fn text(&self, s: &str) -> String {
        let mut out = s.to_string();
        for v in &self.0 {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), REDACTED);
            }
        }
        out
    }

    fn json(&self, v: &mut Value) {
        if self.0.is_empty() {
            return;
        }
        match v {
            Value::String(s) => *s = self.text(s),
            Value::Array(a) => a.iter_mut().for_each(|x| self.json(x)),
            Value::Object(o) => {
                let old = std::mem::take(o);
                for (k, mut x) in old {
                    self.json(&mut x);
                    o.insert(self.text(&k), x);
                }
            }
            _ => {}
        }
    }
}

fn secret_value(name: &str, v: &str) -> Result<String, Failure> {
    hyper::header::HeaderValue::from_str(v)
        .map(|_| v.to_string())
        .map_err(|_| Failure::new(format!("header {name}: value is not a valid header value")))
}

/// First line of a small file, trimmed.
fn read_secret(path: &str) -> Result<String, String> {
    use std::io::Read;
    let mut s = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_SECRET_FILE).read_to_string(&mut s))
        .map_err(|e| e.kind().to_string())?;
    Ok(s.lines().next().unwrap_or("").trim().to_string())
}

async fn run_command(w: &WidgetConfig) -> Result<Fetched, Failure> {
    use std::process::Stdio;
    let Some((prog, args)) = w.command.split_first() else {
        return Err(Failure::new("command is empty"));
    };
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args)
        .env_clear()
        .env("PATH", path)
        .envs(&w.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Own process group, so a timeout can kill grandchildren too.
        .process_group(0);
    if std::path::Path::new("/data").is_dir() {
        cmd.current_dir("/data");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| Failure::new(format!("cannot start {prog}: {e}")))?;
    let pid = child.id();
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let work = async {
        tokio::join!(
            drain(stdout, MAX_STDOUT),
            drain(stderr, MAX_STDERR),
            child.wait()
        )
    };
    let ((out, out_over), (err, _), status) = match tokio::time::timeout(w.timeout.0, work).await {
        Ok(r) => r,
        Err(_) => {
            kill_group(pid);
            return Err(Failure::new(timeout_msg(w.timeout.0)));
        }
    };
    let status = status.map_err(|e| Failure::new(format!("wait: {e}")))?;
    let code = status.code();
    if !status.success() {
        let why = match code {
            Some(c) => format!("exit {c}"),
            None => {
                use std::os::unix::process::ExitStatusExt;
                format!("killed by signal {}", status.signal().unwrap_or(0))
            }
        };
        let detail = one_line(&err, 200);
        return Err(Failure {
            code: code.map(i64::from),
            msg: if detail.is_empty() {
                why
            } else {
                format!("{why}: {detail}")
            },
        });
    }
    let code = i64::from(code.unwrap_or(0));
    if out_over {
        return Err(Failure {
            code: Some(code),
            msg: format!("output larger than {} KiB", MAX_STDOUT / 1024),
        });
    }
    let data = match w.output {
        Output::Text => Data::Text(String::from_utf8_lossy(&out).into_owned()),
        Output::Json => {
            if out.iter().all(u8::is_ascii_whitespace) {
                let detail = one_line(&err, 200);
                return Err(Failure {
                    code: Some(code),
                    msg: if detail.is_empty() {
                        "no output".into()
                    } else {
                        format!("no output: {detail}")
                    },
                });
            }
            Data::Json(serde_json::from_slice(&out).map_err(|e| Failure {
                code: Some(code),
                msg: format!("output is not JSON ({e})"),
            })?)
        }
    };
    Ok(Fetched { code, data })
}

fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid.and_then(|p| libc::pid_t::try_from(p).ok()) {
        if pid > 0 {
            // SAFETY: plain syscall; the group was created by `process_group(0)`
            // for this child, so -pid only ever addresses our own processes.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}

/// Read a pipe to the end, keeping at most `keep` bytes. Returns the kept
/// bytes and whether more was written (and thrown away).
async fn drain<R: AsyncRead + Unpin>(r: Option<R>, keep: usize) -> (Vec<u8>, bool) {
    let Some(mut r) = r else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    let mut over = false;
    let mut buf = vec![0u8; 8192];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = keep.saturating_sub(out.len());
                if n > room {
                    over = true;
                }
                out.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    (out, over)
}

/// A failed fetch in a few plain words; the raw message stays in "Details".
pub fn plain_error(e: &str) -> String {
    let l = e.to_ascii_lowercase();
    if l.contains("timed out") || l.contains("timeout") {
        "No answer in time".into()
    } else if l.contains("refused") {
        "Refused: nothing is listening there".into()
    } else if l.contains("lookup")
        || l.contains("resolve")
        || l.contains("dns")
        || l.contains("name or service")
    {
        "The name does not resolve".into()
    } else if let Some(code) = e
        .strip_prefix("HTTP ")
        .and_then(|r| r.get(..3))
        .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
    {
        format!("The service answered HTTP {code}")
    } else if l.contains("certificate") || l.contains("tls") {
        "The secure connection failed".into()
    } else {
        "Could not get an answer".into()
    }
}

#[cfg(test)]
mod plain_error_tests {
    #[test]
    fn describes_connection_failures() {
        assert_eq!(
            super::plain_error("connect 127.0.0.1:9: Connection refused (os error 111)"),
            "Refused: nothing is listening there"
        );
        assert_eq!(
            super::plain_error("timed out after 5000ms"),
            "No answer in time"
        );
    }
}
