//! Notifications: channels and alert rules, rendered on the server from the
//! file on disk, each with its add / edit dialog (the recipe in `dlg.rs`;
//! Bookmarks is the reference). Signed in only (the caller checks).
//!
//! Channels name their secret by environment variable, by a file in
//! `{data_dir}/secrets/` or (webhook only) as a plain URL; the page shows a
//! variable's NAME with set / not set and a file's path with found / not
//! found, never a value. Header values and body templates are never put in
//! the page: the script reads them from the settings API when a save has
//! to keep them. "Save and send test" saves, then sends the test with the
//! existing `channels/test` (owner decision Y14). A rule's on / off switch
//! in its row saves on change (Y21).

use maud::{html, Markup};
use serde_json::{json, Map, Value as J};

use super::disk::Disk;
use super::dlg::{self, sval, Field, Spec, D, F, N};
use crate::ui::fmt::parse_query;
use crate::ui::ic;
use crate::ui::shell::Ctx;

const ACTION: &str = "/settings/notifications";

pub const CH: Spec = Spec {
    id: "dlg-channel",
    kind: "channel",
    prefix: "ch",
    add: "Add channel",
    edit: "Edit channel “{}”",
    title_key: "name",
    fields: &[],
    delete: Some("Delete…"),
    back: ("Notifications", ACTION),
    list: "ch-list",
    wide: false,
};

pub const RU: Spec = Spec {
    id: "dlg-rule",
    kind: "rule",
    prefix: "ru",
    add: "Add rule",
    edit: "Edit rule “{}”",
    title_key: "name",
    fields: &[],
    delete: Some("Delete…"),
    back: ("Notifications", ACTION),
    list: "ru-list",
    wide: true,
};

const NAME_PATTERN: &str = r"[A-Za-z0-9_\-]{1,64}";
const NAME_MSG: &str = "Letters, digits, - and _ only, at most 64.";
const ENV_HINT: &str =
    "The NAME of an environment variable of this container, never the secret itself.";

// ------------------------------------------------------------------ helpers

/// Which choice a `data-for` belongs to: `"a b"` is the type, `"k:a b"` the
/// choice named `k`. Hidden when the current value of that choice is not
/// listed (the page form without script shows only what applies).
fn hidden(sel: &[(&str, &str)], f: Option<&str>) -> bool {
    let Some(f) = f else { return false };
    let (k, list) = f.split_once(':').unwrap_or(("type", f));
    let cur = sel.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| v);
    !list.split(' ').any(|x| x == cur)
}

fn str_of<'a>(t: &'a Map<String, J>, k: &str) -> Option<&'a str> {
    t.get(k).and_then(J::as_str)
}

fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

fn file_found(p: &str) -> bool {
    std::path::Path::new(p).is_file()
}

/// A regular expression matching `s` literally (for a `pattern`).
fn re_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The `{data_dir}/secrets` folder as the form shows it.
fn secrets_dir(ctx: &Ctx) -> String {
    ctx.state
        .env
        .data_dir
        .join("secrets")
        .display()
        .to_string()
        .trim_end_matches('/')
        .to_string()
}

/// Rules (names) that send to the channel `name`.
fn rules_using(disk: &Disk, name: &str) -> Vec<String> {
    disk.list("alerts")
        .iter()
        .filter(|r| {
            r.get("channels")
                .and_then(J::as_array)
                .is_some_and(|a| a.iter().any(|c| c.as_str() == Some(name)))
        })
        .map(|r| str_of(r, "name").unwrap_or("?").to_string())
        .collect()
}

// ------------------------------------------------------------------ channels

const KINDS: &[(&str, &str, Option<&str>)] = &[
    ("webhook", "Webhook", None),
    ("slack", "Slack", None),
    ("telegram", "Telegram", None),
];

/// The secret's source a channel uses today.
fn source(t: &Map<String, J>) -> &'static str {
    let has = |k: &str| t.get(k).is_some_and(|v| !v.is_null());
    match str_of(t, "type").unwrap_or("webhook") {
        "slack" if has("webhook_url_file") => "file",
        "telegram" if has("token_file") => "file",
        "webhook" if has("url_file") => "file",
        "webhook" if has("url") && !has("url_env") => "plain",
        _ => "env",
    }
}

/// What is set in the file and cannot be changed here, as (what, value)
/// lines: never a header value or a body.
fn read_only(t: &Map<String, J>) -> Vec<(String, String)> {
    let mut ro = Vec::new();
    if let Some(n) = str_of(t, "chat_id_env") {
        ro.push(("Chat id from variable".into(), n.to_string()));
    }
    if let Some(b) = str_of(t, "api_base") {
        ro.push(("API base".into(), b.to_string()));
    }
    for (h, n) in t
        .get("headers_env")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
    {
        ro.push((
            format!("Header {h}"),
            format!("from variable {}", n.as_str().unwrap_or("?")),
        ));
    }
    for (h, p) in t
        .get("headers_file")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
    {
        ro.push((
            format!("Header {h}"),
            format!("from file {}", p.as_str().unwrap_or("?")),
        ));
    }
    for h in t
        .get("headers")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
        .map(|(h, _)| h)
    {
        ro.push((format!("Header {h}"), "plain text in the file".into()));
    }
    if t.contains_key("body") {
        ro.push(("Body".into(), "a template in the file".into()));
    }
    ro
}

/// Bound to its destination (`guard.rs` secret binding): the destination
/// and the secret can only be changed in the file on the host.
fn bound(t: &Map<String, J>) -> bool {
    let nonempty = |k: &str| {
        t.get(k)
            .and_then(J::as_object)
            .is_some_and(|m| !m.is_empty())
    };
    match str_of(t, "type") {
        Some("telegram") => t.contains_key("api_base"),
        Some("webhook") => nonempty("headers_env") || nonempty("headers_file"),
        _ => false,
    }
}

/// The dialog's values for one channel (`data-item`): the form fields, plus
/// `_ro` (the read-only lines), `_keep` (the file has keys the form does
/// not show, which a save must keep), `_bound`, `_chat_env` and `_st` (set /
/// found per secret field). Never a header value or a body.
fn channel_item(name: &str, t: &Map<String, J>) -> Map<String, J> {
    let mut m = Map::new();
    m.insert("name".into(), J::String(name.to_string()));
    m.insert(
        "type".into(),
        J::String(str_of(t, "type").unwrap_or("webhook").to_string()),
    );
    m.insert("src".into(), J::String(source(t).to_string()));
    for k in [
        "webhook_url_env",
        "webhook_url_file",
        "token_env",
        "token_file",
        "chat_id",
        "url_env",
        "url_file",
        "url",
        "timeout",
    ] {
        m.insert(k.into(), J::String(sval(Some(t), k)));
    }
    let ro: Vec<J> = read_only(t)
        .into_iter()
        .map(|(a, b)| json!([a, b]))
        .collect();
    m.insert("_ro".into(), J::Array(ro));
    let keep = [
        "chat_id_env",
        "api_base",
        "headers",
        "headers_env",
        "headers_file",
        "body",
    ]
    .iter()
    .any(|k| t.contains_key(*k));
    m.insert("_keep".into(), J::Bool(keep));
    m.insert("_bound".into(), J::Bool(bound(t)));
    m.insert("_chat_env".into(), J::Bool(t.contains_key("chat_id_env")));
    let mut st = Map::new();
    for (k, v) in t {
        if let Some(s) = v.as_str() {
            if k.ends_with("_env") {
                st.insert(k.clone(), J::Bool(env_is_set(s)));
            } else if k.ends_with("_file") {
                st.insert(k.clone(), J::Bool(file_found(s)));
            }
        }
    }
    m.insert("_st".into(), J::Object(st));
    m
}

fn src_hint(env: bool) -> &'static str {
    if env {
        ENV_HINT
    } else {
        "A file inside data/secrets/ on the host; its first line is the secret."
    }
}

/// The channel form's fields (dialog and page form). `values` fills them.
fn channel_body(prefix: &str, values: Option<&Map<String, J>>, dir: &str) -> Markup {
    let ty = values
        .map(|v| sval(Some(v), "type"))
        .unwrap_or_else(|| "webhook".into());
    let src = values
        .map(|v| sval(Some(v), "src"))
        .unwrap_or_else(|| "env".into());
    let sel = [("type", ty.as_str()), ("src", src.as_str())];
    let h = |f: Option<&str>| hidden(&sel, f);
    let file_pat = format!(r"{}/\S+", re_escape(dir));
    let file_msg = format!("A file inside {dir}/, like {dir}/hook-url.");
    // The patterns are built per request (the data folder), so the fields
    // are built here rather than as constants.
    let fld = |f: Field| dlg::field(prefix, &f, &sval(values, f.name), h(f.for_));
    let file = |name: &'static str, label: &'static str, ph: &'static str| {
        let ph = format!("{dir}/{ph}");
        html! {
            (dlg::field(prefix, &Field {
                name, label, hint: src_hint(false), required: true, mono: true, wide: true,
                pattern: Some(&file_pat), msg: Some(&file_msg), placeholder: Some(&ph), for_: Some("src:file"), ..F
            }, &sval(values, name), h(Some("src:file"))))
        }
    };
    let ro: Vec<(String, String)> = values
        .and_then(|v| v.get("_ro"))
        .and_then(J::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| {
            Some((
                x.get(0)?.as_str()?.to_string(),
                x.get(1)?.as_str()?.to_string(),
            ))
        })
        .collect();
    html! {
        (fld(Field {
            name: "name", label: "Name", required: true, mono: true, wide: true, max: 64,
            pattern: Some(NAME_PATTERN), msg: Some(NAME_MSG), placeholder: Some("ops-hook"),
            hint: "Alert rules refer to this name. If you rename it, you are asked before the rules are changed with it.",
            ..F
        }))
        (dlg::choice(prefix, "type", "Type", "Webhook covers ntfy, Gotify, Discord and anything that takes a POST.", KINDS, &ty, |_| false))
        (dlg::choice(prefix, "src", "Secret comes from", "", &[
            ("env", "Environment variable", None),
            ("file", "File in data/secrets/", None),
            ("plain", "Plain URL", Some("webhook")),
        ], &src, h))
        div.fc data-for="slack" hidden[h(Some("slack"))] {
            (fld(Field { name: "webhook_url_env", label: "Webhook URL variable", hint: ENV_HINT, required: true, mono: true, wide: true, v: Some("env"), placeholder: Some("RH_SLACK_WEBHOOK_URL"), for_: Some("src:env"), ..F }))
            (file("webhook_url_file", "Webhook URL file", "slack-url"))
        }
        div.fc data-for="telegram" hidden[h(Some("telegram"))] {
            (fld(Field { name: "token_env", label: "Bot token variable", hint: ENV_HINT, required: true, mono: true, wide: true, v: Some("env"), placeholder: Some("RH_TELEGRAM_TOKEN"), for_: Some("src:env"), ..F }))
            (file("token_file", "Bot token file", "telegram-token"))
            @if values.and_then(|v| v.get("_chat_env")).and_then(J::as_bool) == Some(true) {
                p.hint.wide { "Chat id: read from the variable named in the file (listed below)." }
            } @else {
                (fld(Field { name: "chat_id", label: "Chat id", hint: "The chat, group or channel id (not a secret).", required: true, mono: true, wide: true, pattern: Some(r"-?[0-9]+|@[A-Za-z0-9_]+"), msg: Some("A number like -1001234567890, or @channelname."), ..F }))
            }
        }
        div.fc data-for="webhook" hidden[h(Some("webhook"))] {
            (fld(Field { name: "url_env", label: "URL variable", hint: ENV_HINT, required: true, mono: true, wide: true, v: Some("env"), placeholder: Some("RH_WEBHOOK_URL"), for_: Some("src:env"), ..F }))
            (file("url_file", "URL file", "hook-url"))
            (fld(Field { name: "url", label: "URL (plain text)", hint: "Stored in the config file and its revisions as plain text. Use a variable or a file if the URL contains a token.", required: true, mono: true, wide: true, v: Some("http"), placeholder: Some("https://"), inputmode: Some("url"), for_: Some("src:plain"), ..F }))
        }
        div.ro-box data-ro hidden[ro.is_empty()] {
            p { (ic("lock")) span { "Set in the config file on the host; kept as it is when you save:" } }
            ul.kv { @for (a, b) in &ro { li { (a) ": " code.wrap { (b) } } } }
        }
        details.dlg-sec {
            summary { "More options" }
            div.grid-2 {
                (fld(Field { name: "timeout", label: "Timeout", hint: "From 1s to 60s. Default 10s.", placeholder: Some("10s"), ..D }))
            }
        }
    }
}

fn secret_cell(t: &Map<String, J>) -> Markup {
    let mut env: Vec<(&str, bool)> = Vec::new();
    let mut files: Vec<(&str, bool)> = Vec::new();
    for (k, v) in t {
        if let Some(s) = v.as_str() {
            if k.ends_with("_env") {
                env.push((s, env_is_set(s)));
            } else if k.ends_with("_file") {
                files.push((s, file_found(s)));
            }
        }
    }
    for n in t
        .get("headers_env")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(_, n)| n.as_str())
    {
        env.push((n, env_is_set(n)));
    }
    let plain = t.contains_key("url");
    let chat = t.contains_key("chat_id").then(|| sval(Some(t), "chat_id"));
    html! {
        @if env.is_empty() && files.is_empty() && !plain && chat.is_none() { span.muted { "—" } }
        @for (n, set) in &env {
            div.sec-ref { code { (n) } " " @if *set { span.pill.ok { "set" } } @else { span.pill.bad { "not set" } } }
        }
        @for (p, found) in &files {
            div.sec-ref { code.wrap { (p) } " " @if *found { span.pill.ok { "file found" } } @else { span.pill.bad { "no file" } } }
        }
        @if plain { div.sec-ref { span.pill.warn { "plain URL" } span.muted { " in the file" } } }
        @if let Some(c) = chat { div.sec-ref.muted { "chat " code { (c) } } }
    }
}

fn channel_row(ctx: &Ctx, disk: &Disk, name: &str, t: &Map<String, J>) -> Markup {
    let key = format!("{}:{name}", CH.kind);
    let href = dlg::edit_link(CH.kind, name);
    let data = J::Object(channel_item(name, t)).to_string();
    let rules = J::from(rules_using(disk, name)).to_string();
    let ty = str_of(t, "type").unwrap_or("?");
    let status = ctx.state.alerts.lock().ok().and_then(|a| {
        a.channels
            .get(name)
            .and_then(|s| serde_json::to_value(s).ok())
    });
    let st = |k: &str| status.as_ref().and_then(|s| s.get(k)).cloned();
    let health = st("health").and_then(|h| h.as_str().map(str::to_string));
    let (cls, word) = match health.as_deref() {
        Some("ok") => ("ok", "ok"),
        Some("failing") => ("bad", "failing"),
        Some("misconfigured") => ("warn", "misconfigured"),
        Some("idle") => ("off", "idle"),
        Some(o) => ("off", o),
        None => ("off", "not loaded"),
    };
    let n = |k: &str| st(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let (sent, failed) = (n("sent"), n("failed"));
    html! {
        tr data-key=(key) data-rules=(rules) {
            td data-l="Channel" {
                a.linkbtn.mono href=(href) data-dlg=(CH.id) data-key=(key) data-item=(data) { (name) }
            }
            td data-l="Type" { span.pill { (ty) } }
            td data-l="Secret" { (secret_cell(t)) }
            td.ch-st data-l="Status" {
                span.pill.(cls) { (word) }
                @if let Some(e) = st("error").and_then(|e| e.as_str().map(str::to_string)) { div.sub.err-line { (e) } }
                @if sent + failed > 0 { div.sub.muted { (sent) " sent, " (failed) " failed" } }
                div.sub.test-res role="status" {}
            }
            td.act {
                div.ra {
                    button.btn.sm type="button" data-test=(name) disabled data-ready { (ic("zap")) "Send test" }
                    a.ia href=(href) data-dlg=(CH.id) data-key=(key) data-item=(data) aria-label={"Edit " (name)} data-tip="Edit" { (ic("edit")) }
                    a.ia href=(href) data-menu="m-ch" role="button" aria-haspopup="menu" aria-expanded="false"
                        aria-label={"More actions for " (name)} data-tip="More" { (ic("more")) }
                }
            }
        }
    }
}

fn load_error(e: &str) -> Markup {
    html! {
        p.note.bad {
            (ic("alert"))
            span { "The config file does not load, so the list cannot be shown: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } }
        }
    }
}

/// The channel list; also part of the `?frag=list` answer.
pub fn channel_list(ctx: &Ctx, disk: &Disk) -> Markup {
    let chans = disk.channels();
    html! {
        div #ch-list data-hash=(disk.hash) {
            @match &disk.cfg {
                Err(e) => (load_error(e)),
                Ok(_) if chans.is_empty() => div.empty {
                    span.empty-ic { (ic("bell")) }
                    h3 { "No channels yet" }
                    p { "Alerts are shown on the page only until you add a channel and a rule sends to it." }
                    div.acts { a.btn.primary href="?add=channel" data-dlg=(CH.id) { (ic("plus")) "Add channel" } }
                },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th scope="col" { "Channel" } th scope="col" { "Type" } th scope="col" { "Secret" } th scope="col" { "Status" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody {
                            @for (name, t) in &chans {
                                @if let Some(t) = t.as_object() { (channel_row(ctx, disk, name, t)) }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ------------------------------------------------------------------ rules

const RULE_TYPES: &[(&str, &str)] = &[
    ("container", "Container state"),
    ("http", "HTTP check"),
    ("host", "Host metric"),
    ("widget", "Widget value"),
    ("log_match", "Log match"),
];
/// `alerts::rules::CONDITIONS`, with labels.
const CONDS: &[(&str, &str)] = &[
    ("stopped", "stopped"),
    ("exit_nonzero", "exit non-zero"),
    ("unhealthy", "unhealthy"),
    ("restarting", "restarting"),
];
/// What a container rule fires on when `on` is not set.
pub const DEFAULT_ON: &[&str] = &["stopped", "unhealthy", "restarting"];
const METRICS: &[(&str, &str)] = &[
    ("disk", "Disk used %"),
    ("memory", "Memory used %"),
    ("cpu", "CPU %"),
    ("temp", "Temperature °C"),
];

/// Every text field of a rule (the others: type, enabled, on, on_error,
/// metric, channels).
const RULE_TEXT: &[&str] = &[
    "name",
    "containers",
    "exclude",
    "restarts",
    "window",
    "pattern",
    "stack",
    "count",
    "url",
    "expect_status",
    "contains",
    "failures",
    "timeout",
    "disk",
    "interval",
    "widget",
    "field",
    "equals",
    "not_equals",
    "above",
    "below",
    "for",
    "cooldown",
    "message",
    "recovery",
];

fn str_list(t: &Map<String, J>, k: &str) -> Option<Vec<String>> {
    t.get(k).and_then(J::as_array).map(|a| {
        a.iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
            .collect()
    })
}

/// The dialog's values for one rule (`data-item`; rules hold no secrets).
pub fn rule_item(t: &Map<String, J>) -> Map<String, J> {
    let mut m = Map::new();
    for k in RULE_TEXT {
        m.insert((*k).into(), J::String(sval(Some(t), k)));
    }
    m.insert(
        "type".into(),
        J::String(str_of(t, "type").unwrap_or("container").into()),
    );
    m.insert(
        "enabled".into(),
        J::Bool(t.get("enabled").and_then(J::as_bool) != Some(false)),
    );
    m.insert(
        "on_error".into(),
        J::Bool(t.get("on_error").and_then(J::as_bool) == Some(true)),
    );
    m.insert(
        "metric".into(),
        J::String(str_of(t, "metric").unwrap_or("disk").into()),
    );
    let on =
        str_list(t, "on").unwrap_or_else(|| DEFAULT_ON.iter().map(|s| s.to_string()).collect());
    m.insert("on".into(), json!(on));
    m.insert(
        "channels".into(),
        json!(str_list(t, "channels").unwrap_or_default()),
    );
    m
}

/// One line saying what a rule watches.
fn summary(t: &Map<String, J>) -> String {
    let s = |k: &str| sval(Some(t), k);
    let has = |k: &str| t.get(k).is_some_and(|v| !v.is_null());
    let mut out = String::new();
    match str_of(t, "type").unwrap_or("") {
        "container" => {
            let c = s("containers");
            out.push_str(if c.is_empty() { "*" } else { &c });
            if has("exclude") {
                out += &format!(" except {}", s("exclude"));
            }
            let on = s("on");
            out += &format!(
                " on {}",
                if on.is_empty() {
                    DEFAULT_ON.join(", ")
                } else {
                    on
                }
            );
            if has("restarts") {
                out += &format!(", {} restarts", s("restarts"));
            }
        }
        "http" => {
            out += &s("url");
            if has("expect_status") {
                out += &format!(" → {}", s("expect_status"));
            }
            if has("contains") {
                out += &format!(" contains “{}”", s("contains"));
            }
        }
        "host" | "widget" => {
            if str_of(t, "type") == Some("host") {
                out += &s("metric");
                if has("disk") {
                    out += &format!(" {}", s("disk"));
                }
            } else {
                out += &format!("{}.{}", s("widget"), s("field"));
            }
            // Host thresholds are percentages, temperatures degrees (LG-60).
            let unit = match (str_of(t, "type"), str_of(t, "metric")) {
                (Some("host"), Some("temp")) => " °C",
                (Some("host"), _) => "%",
                _ => "",
            };
            for (k, op, u) in [
                ("above", ">", unit),
                ("below", "<", unit),
                ("equals", "=", ""),
                ("not_equals", "≠", ""),
            ] {
                if has(k) {
                    out += &format!(" {op} {}{u}", s(k));
                }
            }
            if t.get("on_error").and_then(J::as_bool) == Some(true) {
                out += ", on error";
            }
        }
        "log_match" => {
            out += &format!("/{}/ in ", s("pattern"));
            out += &if has("stack") {
                format!("stack {}", s("stack"))
            } else {
                s("containers")
            };
            if has("exclude") {
                out += &format!(" except {}", s("exclude"));
            }
            if has("count") {
                out += &format!(", {}×", s("count"));
            }
            if has("window") {
                out += &format!(" in {}", s("window"));
            }
        }
        _ => {}
    }
    out
}

fn rule_row(t: &Map<String, J>) -> Markup {
    let name = str_of(t, "name").unwrap_or("");
    let key = format!("{}:{name}", RU.kind);
    let href = dlg::edit_link(RU.kind, name);
    let item = rule_item(t);
    let data = J::Object(item.clone()).to_string();
    let on = item["enabled"].as_bool() == Some(true);
    let ty = RULE_TYPES
        .iter()
        .find(|(v, _)| Some(*v) == str_of(t, "type"))
        .map_or(str_of(t, "type").unwrap_or("?"), |(_, l)| l);
    let chans = str_list(t, "channels").unwrap_or_default();
    let default_for = if str_of(t, "type") == Some("container") {
        "30s"
    } else {
        "0s"
    };
    // When it fires and how often it repeats, in the words the Alerts page
    // uses (LG-60); the config's own words if the rule does not parse.
    let timing = serde_json::from_value::<crate::alerts::RuleConfig>(J::Object(t.clone()))
        .map(|r| crate::ui::pages::alerts::timing(&r));
    let raw = format!(
        "for {}, cooldown {}",
        if t.contains_key("for") {
            sval(Some(t), "for")
        } else {
            default_for.into()
        },
        if t.contains_key("cooldown") {
            sval(Some(t), "cooldown")
        } else {
            "15m".into()
        }
    );
    html! {
        tr.off[!on] data-key=(key) {
            td data-l="Rule" {
                a.linkbtn.mono href=(href) data-dlg=(RU.id) data-key=(key) data-item=(data) { (name) }
                span.sub.muted { (ty) @if timing.is_err() { " · " (raw) } }
            }
            td.wrap data-l="Condition" {
                span.mono-s { (summary(t)) }
                @if let Ok(tm) = &timing { br; span.muted.k { (tm) } }
            }
            td data-l="Channels" {
                @if chans.is_empty() { span.muted { "page only" } }
                @for c in &chans { span.chip-s.mono { (c) } " " }
            }
            td data-l="Enabled" {
                label.rsw {
                    input.sw type="checkbox" role="switch" data-toggle checked[on] disabled data-ready aria-label={"Enabled: " (name)};
                    span { @if on { "On" } @else { "Off" } }
                }
            }
            td.act {
                div.ra {
                    a.ia href=(href) data-dlg=(RU.id) data-key=(key) data-item=(data) aria-label={"Edit " (name)} data-tip="Edit" { (ic("edit")) }
                    a.ia href=(href) data-menu="m-ru" role="button" aria-haspopup="menu" aria-expanded="false"
                        aria-label={"More actions for " (name)} data-tip="More" { (ic("more")) }
                }
            }
        }
    }
}

/// The rule list; also part of the `?frag=list` answer.
pub fn rule_list(disk: &Disk) -> Markup {
    let rules = disk.list("alerts");
    html! {
        div #ru-list data-hash=(disk.hash) {
            @match &disk.cfg {
                Err(e) => (load_error(e)),
                Ok(_) if rules.is_empty() => div.empty {
                    span.empty-ic { (ic("alert")) }
                    h3 { "No alert rules yet" }
                    p { "A rule watches containers, a URL, a host metric, a widget or log lines, and notifies its channels." }
                    div.acts { a.btn.primary href="?add=rule" data-dlg=(RU.id) { (ic("plus")) "Add rule" } }
                },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th scope="col" { "Rule" } th scope="col" { "Condition" } th scope="col" { "Channels" } th scope="col" { "Enabled" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody { @for t in &rules { (rule_row(t)) } }
                    }
                },
            }
        }
    }
}

/// The channel choice of the rule form: every channel, ticked for a new
/// rule (a new rule notifies every channel; untick to exclude).
fn rule_channels(prefix: &str, disk: &Disk, values: Option<&Map<String, J>>) -> Markup {
    let names: Vec<String> = disk.channels().keys().cloned().collect();
    let mut opts: Vec<(String, String)> = names.iter().map(|n| (n.clone(), n.clone())).collect();
    let on: Vec<String> = match values {
        Some(v) => v
            .get("channels")
            .and_then(J::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        None => names.clone(),
    };
    // A rule may name a channel that is gone (a hand edit): keep it shown.
    for c in &on {
        if !names.contains(c) {
            opts.push((c.clone(), c.clone()));
        }
    }
    if opts.is_empty() {
        return html! {
            div.fld.wide id={(prefix) "-channels"} {
                span.fld-l { "Channels" }
                p.hint { "No channels yet: the incident is shown on the page only. Add a channel above first to be notified." }
            }
        };
    }
    dlg::checks(
        prefix,
        "channels",
        "Channels",
        "A new rule notifies every channel; untick the ones it should not. None: the incident is only shown on the page.",
        &opts,
        &on,
        None,
        false,
    )
}

const R_CONTAINERS: Field = Field {
    name: "containers",
    label: "Containers",
    hint: "Names or globs, separated by commas. Empty: all (*).",
    mono: true,
    placeholder: Some("*"),
    for_: Some("container log_match"),
    ..F
};

fn rule_body(prefix: &str, values: Option<&Map<String, J>>, disk: &Disk) -> Markup {
    let ty = values
        .map(|v| sval(Some(v), "type"))
        .unwrap_or_else(|| "container".into());
    let metric = values
        .map(|v| sval(Some(v), "metric"))
        .unwrap_or_else(|| "disk".into());
    let sel = [("type", ty.as_str()), ("metric", metric.as_str())];
    let h = |f: Option<&str>| hidden(&sel, f);
    let fld = |f: Field| dlg::field(prefix, &f, &sval(values, f.name), h(f.for_));
    let on: Vec<String> = match values.and_then(|v| v.get("on")).and_then(J::as_array) {
        Some(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        None => DEFAULT_ON.iter().map(|s| s.to_string()).collect(),
    };
    let conds: Vec<(String, String)> = CONDS
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect();
    let flag = |k: &str, d: bool| {
        values
            .and_then(|v| v.get(k))
            .and_then(J::as_bool)
            .unwrap_or(d)
    };
    let max_count = crate::logs::watch::MAX_COUNT.to_string();
    let ids: Vec<String> = disk
        .list("widgets")
        .iter()
        .filter_map(|w| str_of(w, "id").map(str::to_string))
        .collect();
    html! {
        datalist id={(prefix) "-widget-ids"} { @for w in &ids { option value=(w) {} } }
        div.grid-2 {
            (fld(Field { name: "name", label: "Name", required: true, mono: true, max: 80, hint: "Unique; shown with every incident.", ..F }))
            (dlg::select(prefix, "type", "Type", "", RULE_TYPES, &ty, None, false))
            (dlg::switch(prefix, "enabled", "Enabled", "Off: the rule is kept but never fires.", flag("enabled", true), None, false))
        }
        section.dlg-sec aria-label="Condition" {
            h3 { "Condition" }
            div.grid-2 {
                (fld(R_CONTAINERS))
                (fld(Field { name: "exclude", label: "Except", hint: "Names or globs, separated by commas.", mono: true, for_: Some("container log_match"), ..F }))
                (dlg::checks(prefix, "on", "Fire on", "Empty: stopped, unhealthy and restarting.", &conds, &on, Some("container"), h(Some("container"))))
                (fld(Field { name: "restarts", label: "Restarts", hint: "Also fire after this many restarts within the window.", min: Some("1"), for_: Some("container"), ..N }))
                (fld(Field { name: "window", label: "Window", hint: "Container: 1m to 1h (default 10m). Log match: 10s to 1d (default 5m).", placeholder: Some("10m"), for_: Some("container log_match"), ..D }))
                (fld(Field { name: "pattern", label: "Pattern", required: true, mono: true, wide: true, hint: "A regular expression matched against each log line (no backreferences or lookaround).", for_: Some("log_match"), ..F }))
                (fld(Field { name: "stack", label: "Stack", hint: "Watch every container of this compose project. Set Containers, Stack or both.", mono: true, for_: Some("log_match"), ..F }))
                (fld(Field { name: "count", label: "Lines", hint: "Matching lines within the window needed to fire. Default 1.", min: Some("1"), maxv: Some(&max_count), for_: Some("log_match"), ..N }))
                (fld(Field { name: "url", label: "URL", required: true, mono: true, wide: true, v: Some("http"), placeholder: Some("https://"), inputmode: Some("url"), for_: Some("http"), ..F }))
                (fld(Field { name: "expect_status", label: "Expected status", hint: "Default: any 2xx.", min: Some("100"), maxv: Some("599"), for_: Some("http"), ..N }))
                (fld(Field { name: "contains", label: "Body contains", for_: Some("http"), ..F }))
                (fld(Field { name: "failures", label: "Failures in a row", hint: "Default 1.", min: Some("1"), for_: Some("http"), ..N }))
                (fld(Field { name: "timeout", label: "Timeout", hint: "At least 1s, not longer than Check every. Default 10s.", placeholder: Some("10s"), for_: Some("http"), ..D }))
                (dlg::select(prefix, "metric", "Metric", "", METRICS, &metric, Some("host"), h(Some("host"))))
                div.fc data-for="host" hidden[h(Some("host"))] {
                    (fld(Field { name: "disk", label: "Disk", hint: "A disk label or glob. Empty: every disk.", mono: true, for_: Some("metric:disk"), ..F }))
                }
                (fld(Field { name: "interval", label: "Check every", hint: "At least 5s. Default 60s (host 30s).", placeholder: Some("60s"), for_: Some("http host"), ..D }))
                (fld(Field { name: "widget", label: "Widget", required: true, mono: true, hint: "The id of a widget.", list: Some(if prefix == "ru" { "ru-widget-ids" } else { "rup-widget-ids" }), for_: Some("widget"), ..F }))
                (fld(Field { name: "field", label: "Field", required: true, mono: true, hint: "A field of that widget.", for_: Some("widget"), ..F }))
                (fld(Field { name: "above", label: "Above", hint: "Fire when the value is above this.", step: Some("any"), for_: Some("host widget"), ..N }))
                (fld(Field { name: "below", label: "Below", hint: "Fire when the value is below this.", step: Some("any"), for_: Some("host widget"), ..N }))
                (fld(Field { name: "equals", label: "Equals", hint: "Fire when the field equals this.", mono: true, for_: Some("widget"), ..F }))
                (fld(Field { name: "not_equals", label: "Not equals", hint: "Fire when the field is not this.", mono: true, for_: Some("widget"), ..F }))
                (dlg::switch(prefix, "on_error", "Fire when the widget fails", "", flag("on_error", false), Some("widget"), h(Some("widget"))))
            }
        }
        section.dlg-sec aria-label="Notify" {
            h3 { "Notify" }
            (rule_channels(prefix, disk, values))
            details {
                summary { "More options" }
                div.grid-2 {
                    (fld(Field { name: "for", label: "For", hint: "Must be bad this long before firing. Default 30s for containers, else 0s.", placeholder: Some("0s"), ..D }))
                    (fld(Field { name: "cooldown", label: "Cooldown", hint: "Minimum time between two notifications. Default 15m.", placeholder: Some("15m"), ..D }))
                    (fld(Field { name: "message", label: "Message", hint: "A template; empty: the default message.", wide: true, ..F }))
                    (fld(Field { name: "recovery", label: "Recovery message", hint: "A template; empty: the default.", wide: true, ..F }))
                }
            }
        }
    }
}

// ------------------------------------------------------------------ the tab

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let q = parse_query(ctx.query);
    let dir = secrets_dir(ctx);
    // `?add=` / `?edit=` without the script: the page form. An edit wins
    // over an add, whichever list it is for (as in ui.js).
    let ch_edit = |id: &str| match disk.channels().get(id).and_then(J::as_object) {
        Some(t) => {
            let v = channel_item(id, t);
            dlg::page_with(&CH, ACTION, Some(&v), channel_body("chp", Some(&v), &dir))
        }
        None => dlg::gone("channel"),
    };
    let ru_edit = |id: &str| match disk
        .list("alerts")
        .iter()
        .find(|r| str_of(r, "name") == Some(id))
    {
        Some(t) => {
            let v = rule_item(t);
            dlg::page_with(&RU, ACTION, Some(&v), rule_body("rup", Some(&v), &disk))
        }
        None => dlg::gone("rule"),
    };
    let page = match (dlg::landed(&q, CH.kind), dlg::landed(&q, RU.kind)) {
        (Some(Some(id)), _) => Some(ch_edit(id)),
        (_, Some(Some(id))) => Some(ru_edit(id)),
        (Some(None), _) => Some(dlg::page_with(
            &CH,
            ACTION,
            None,
            channel_body("chp", None, &dir),
        )),
        (_, Some(None)) => Some(dlg::page_with(
            &RU,
            ACTION,
            None,
            rule_body("rup", None, &disk),
        )),
        _ => None,
    };
    html! {
        @if let Some(p) = page { (p) } @else { (dlg::needs_js(dlg::Values::Shown)) }
        section.panel #ch-sec aria-labelledby="ch-h" {
            div.panel-h {
                h2 #ch-h { "Channels" }
                a.btn.sm.head-act #ch-add href="?add=channel" data-dlg=(CH.id) { (ic("plus")) "Add channel" }
            }
            p.muted.sf-lead { "Secrets stay on the host: in the container's environment (e.g. a " code { "secrets.env" } " file in compose) or in a file in " code { "data/secrets/" } ". Here you only name them; the page shows whether they are there." }
            (channel_list(ctx, &disk))
        }
        section.panel #ru-sec aria-labelledby="ru-h" {
            div.panel-h {
                h2 #ru-h { "Alert rules" }
                a.btn.sm.head-act #ru-add href="?add=rule" data-dlg=(RU.id) { (ic("plus")) "Add rule" }
            }
            (rule_list(&disk))
        }
    }
}

/// The dialogs of this tab (rendered outside `#set-main`).
pub fn dialogs(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let dir = secrets_dir(ctx);
    html! {
        (dlg::dialog_with(&CH, ACTION, channel_body("ch", None, &dir), html! {
            button.btn type="button" data-submit="test" data-busy="Sending test…" disabled data-ready { (ic("zap")) "Save and send test" }
        }))
        (dlg::dialog_with(&RU, ACTION, rule_body("ru", None, &disk), html! {}))
        (dlg::row_menu("m-ch"))
        (dlg::row_menu("m-ru"))
    }
}

/// `?frag=list`: both lists (a channel rename changes rules too) and the
/// rule form's channel choice, as the page has them.
pub fn fragment(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    html! {
        (channel_list(ctx, &disk))
        (rule_list(&disk))
        (rule_channels("ru", &disk, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(toml: &str) -> String {
        let v: toml::Value = toml::from_str(toml).unwrap();
        let J::Object(t) = serde_json::to_value(v).unwrap() else {
            panic!()
        };
        rule_row(&t).into_string()
    }

    /// LG-60: host thresholds carry their unit and the rule says when it
    /// fires and how often it repeats, in the Alerts page's words.
    #[test]
    fn rule_rows_say_units_and_timing() {
        let disk = row("name='d'\ntype='host'\nmetric='disk'\nabove=90\nfor='5m'");
        assert!(disk.contains("disk &gt; 90%"), "{disk}");
        assert!(
            disk.contains("fires after 5 min · repeats after 15 min"),
            "{disk}"
        );
        assert!(
            disk.contains(r#"<span class="sub muted">Host metric</span>"#),
            "{disk}"
        );
        let temp = row("name='t'\ntype='host'\nmetric='temp'\nbelow=20");
        assert!(temp.contains("temp &lt; 20 °C"), "{temp}");
        assert!(temp.contains("fires at once"), "{temp}");
        let w = row("name='w'\ntype='widget'\nwidget='x'\nfield='f'\nabove=3");
        assert!(w.contains("x.f &gt; 3<"), "widgets have no unit: {w}");
    }
}
