//! Widgets: the "System tiles" switch (saves on change, owner decision
//! Y21), the `custom-api` widgets rendered on the server with their add /
//! edit dialog (tabs Request / Fields / Display and a **Fetch now**
//! preview; the recipe in `dlg.rs`), and, read-only, the widgets that can
//! run a program (`command`, or anything not `custom-api`). Those have no
//! controls at all: they are managed in the file on the host (see
//! `settings::guard`). Signed in only (the caller checks).
//!
//! Header values and request bodies are never put in the page: a save that
//! has to keep them reads them from the settings API (as for channels).

use maud::{html, Markup};
use serde_json::{json, Map, Value as J};

use super::disk::Disk;
use super::dlg::{self, sval, Field, Spec, D, F};
use crate::ui::fmt::parse_query;
use crate::ui::ic;
use crate::ui::shell::Ctx;
use crate::widgets::WidgetConfig;

const ACTION: &str = "/settings/widgets";

pub const LOCKED_TEXT: &str = "Defined in the config file on the host — command widgets can't be created, changed or enabled from the app.";

pub const SPEC: Spec = Spec {
    id: "dlg-widget",
    kind: "widget",
    prefix: "wf",
    add: "Add widget",
    edit: "Edit widget “{}”",
    title_key: "id",
    fields: &[],
    delete: Some("Delete…"),
    back: ("Widgets", ACTION),
    list: "w-list",
    wide: true,
};

/// Mirrors `settings::guard::is_command_capable` (fail closed).
pub fn is_locked(w: &WidgetConfig) -> bool {
    w.kind != "custom-api" || !w.command.is_empty() || !w.env.is_empty()
}

/// The same test on the file's JSON.
fn locked_json(t: &Map<String, J>) -> bool {
    t.get("type").and_then(J::as_str) != Some("custom-api")
        || t.contains_key("command")
        || t.contains_key("env")
}

fn str_of<'a>(t: &'a Map<String, J>, k: &str) -> Option<&'a str> {
    t.get(k).and_then(J::as_str)
}

fn locked(ws: &[&WidgetConfig]) -> Markup {
    html! {
        section.panel #w-locked aria-labelledby="wl-h" {
            div.panel-h { h2 #wl-h { "Defined on the host" } span.count-pill { (ws.len()) } }
            p.lock-note { (ic("lock")) span { (LOCKED_TEXT) } }
            ul.locked-list {
                @for w in ws {
                    li.locked-w data-id=(w.id) {
                        span.lk { (ic("lock")) span.sr { "Locked: " } }
                        div.lw-main {
                            div.lw-t {
                                code.nm { (w.id) }
                                @if let Some(t) = &w.title { span.lw-title { (t) } }
                                span.pill { (w.kind) }
                                @if w.is_enabled() { span.pill.ok { "enabled" } } @else { span.pill.off { "disabled" } }
                            }
                            @if !w.command.is_empty() {
                                code.lw-cmd.wrap { (w.command.join(" ")) }
                            }
                            div.lw-meta.muted {
                                "refresh " (crate::util::human_duration(w.refresh.0.as_secs()))
                                " · timeout " (crate::util::human_duration(w.timeout.0.as_secs()))
                                @if !w.env.is_empty() {
                                    " · env " @for (i, k) in w.env.keys().enumerate() { @if i > 0 { ", " } code { (k) } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Header secrets and plain headers as read-only lines: the header name and
/// where its value comes from, never the value.
fn header_lines(t: &Map<String, J>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (h, n) in t
        .get("headers_env")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
    {
        out.push((
            h.clone(),
            format!("from variable {}", n.as_str().unwrap_or("?")),
        ));
    }
    for (h, p) in t
        .get("headers_file")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
    {
        out.push((
            h.clone(),
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
        out.push((h.clone(), "plain text in the file".into()));
    }
    if t.contains_key("body") {
        out.push(("Request body".into(), "a template in the file".into()));
    }
    out
}

/// Bound to its URL (`guard.rs` secret binding).
fn bound(t: &Map<String, J>) -> bool {
    let nonempty = |k: &str| {
        t.get(k)
            .and_then(J::as_object)
            .is_some_and(|m| !m.is_empty())
    };
    nonempty("headers_env") || nonempty("headers_file")
}

/// The dialog's values for one widget (`data-item`): the form fields (the
/// fields and rows as lists of pairs, in file order), plus `_ro` (header
/// lines), `_bound` and `_keep` (keys the form does not show that a save
/// must keep). Never a header value or a body.
pub fn widget_item(t: &Map<String, J>) -> Map<String, J> {
    let mut m = Map::new();
    for k in [
        "id", "title", "url", "link", "icon", "refresh", "timeout", "template",
    ] {
        m.insert(k.into(), J::String(sval(Some(t), k)));
    }
    let method = str_of(t, "method").unwrap_or("GET").to_ascii_uppercase();
    let other = method != "GET" && method != "POST";
    m.insert(
        "method".into(),
        J::String(if method == "POST" { "POST" } else { "GET" }.into()),
    );
    m.insert(
        "enabled".into(),
        J::Bool(t.get("enabled").and_then(J::as_bool) != Some(false)),
    );
    let fields: Vec<J> = t
        .get("fields")
        .and_then(J::as_object)
        .into_iter()
        .flatten()
        .map(|(k, v)| json!([k, v.as_str().unwrap_or("")]))
        .collect();
    m.insert("fields".into(), J::Array(fields));
    let rows: Vec<J> = t
        .get("rows")
        .and_then(J::as_array)
        .into_iter()
        .flatten()
        .map(|r| {
            json!([
                r.get("label").and_then(J::as_str).unwrap_or(""),
                r.get("value").and_then(J::as_str).unwrap_or("")
            ])
        })
        .collect();
    let mode = if !rows.is_empty() && !t.contains_key("template") {
        "rows"
    } else {
        "template"
    };
    m.insert("mode".into(), J::String(mode.into()));
    m.insert("rows".into(), J::Array(rows));
    let st = t.get("status").and_then(J::as_object);
    m.insert(
        "status_field".into(),
        J::String(
            st.and_then(|s| s.get("field"))
                .and_then(J::as_str)
                .unwrap_or("")
                .into(),
        ),
    );
    let ok = st
        .and_then(|s| s.get("ok"))
        .and_then(J::as_array)
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    m.insert("status_ok".into(), J::String(ok));
    // The OK values as typed in the file ("200" and 200 are not the same):
    // kept when the user does not change the text.
    let raw = st.and_then(|s| s.get("ok")).cloned().unwrap_or(J::Null);
    m.insert("_status_ok".into(), raw);
    let ro: Vec<J> = header_lines(t)
        .into_iter()
        .map(|(a, b)| json!([a, b]))
        .collect();
    m.insert("_ro".into(), J::Array(ro));
    m.insert("_bound".into(), J::Bool(bound(t)));
    let mut keep: Vec<&str> = ["body", "headers", "headers_env", "headers_file", "output"]
        .into_iter()
        .filter(|k| t.contains_key(*k))
        .collect();
    if other {
        keep.push("method");
    }
    m.insert("_keep".into(), json!(keep));
    m
}

fn pairs(values: Option<&Map<String, J>>, k: &str) -> Vec<(String, String)> {
    values
        .and_then(|v| v.get(k))
        .and_then(J::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p.get(0)?.as_str()?.to_string(),
                p.get(1)?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// One editable row of name / value pairs (a field or a display row).
fn kv_row(prefix: &str, kind: &str, i: usize, a: &str, b: &str) -> Markup {
    let (la, lb, pa, pb, ka, kb) = if kind == "field" {
        (
            "Field name",
            "Path in the response",
            "name",
            "path.to.value",
            "fk",
            "fv",
        )
    } else {
        (
            "Label",
            "Value (template)",
            "Label",
            "{{field | number}}",
            "rl",
            "rv",
        )
    };
    html! {
        div.row.kv-row data-kind=(kind) {
            input.in.mono type="text" name={(ka) "." (i)} value=(a) placeholder=(pa) aria-label=(la) autocomplete="off" spellcheck="false";
            input.in.mono type="text" name={(kb) "." (i)} value=(b) placeholder=(pb) aria-label=(lb) autocomplete="off" spellcheck="false";
            @if prefix == SPEC.prefix {
                button.ia type="button" data-kv-del aria-label={"Remove this " (if kind == "field" { "field" } else { "row" })} data-tip="Remove" { (ic("x")) }
            }
            span.kv-got.muted {}
        }
    }
}

/// The widget form (dialog: three tabs; page form: three sections).
fn widget_body(prefix: &str, values: Option<&Map<String, J>>, tabs: bool) -> Markup {
    let mode = values
        .map(|v| sval(Some(v), "mode"))
        .unwrap_or_else(|| "template".into());
    let method = values
        .map(|v| sval(Some(v), "method"))
        .unwrap_or_else(|| "GET".into());
    let h = |f: Option<&str>| f.is_some_and(|f| f.strip_prefix("mode:") != Some(mode.as_str()));
    let fld = |f: Field| dlg::field(prefix, &f, &sval(values, f.name), h(f.for_));
    let mut fields = pairs(values, "fields");
    if fields.is_empty() {
        fields.push((String::new(), String::new())); // one empty row to start
    }
    let rows = pairs(values, "rows");
    let ro: Vec<(String, String)> = pairs(values, "_ro");
    let enabled = values
        .and_then(|v| v.get("enabled"))
        .and_then(J::as_bool)
        .unwrap_or(true);
    let panel = |id: &str, title: &str, first: bool, inner: Markup| {
        html! {
            @if tabs {
                div.tabpanel role="tabpanel" id={(prefix) "-p-" (id)} aria-labelledby={(prefix) "-t-" (id)} hidden[!first] tabindex="-1" { (inner) }
            } @else {
                section.dlg-sec aria-label=(title) { h3 { (title) } (inner) }
            }
        }
    };
    html! {
        @if tabs {
            div.tabs.dlg-tabs role="tablist" aria-label="Parts of the widget" {
                @for (i, (id, t)) in [("req", "Request"), ("fields", "Fields"), ("display", "Display")].iter().enumerate() {
                    button.tab type="button" role="tab" id={(prefix) "-t-" (id)} aria-controls={(prefix) "-p-" (id)}
                        aria-selected=(if i == 0 { "true" } else { "false" }) tabindex=(if i == 0 { "0" } else { "-1" }) { (t) }
                }
            }
        }
        (panel("req", "Request", true, html! {
            div.grid-2 {
                (fld(Field { name: "id", label: "Id", required: true, mono: true, max: 64, pattern: Some(r"[A-Za-z0-9_\-]{1,64}"), msg: Some("Letters, digits, - and _ only, at most 64."), hint: "Unique. Alert rules refer to it; if you rename it, you are asked before they change with it.", ..F }))
                (fld(Field { name: "title", label: "Title", max: 80, hint: "Empty: the id.", ..F }))
                (fld(Field { name: "url", label: "URL", required: true, mono: true, wide: true, v: Some("http"), placeholder: Some("https://"), inputmode: Some("url"), hint: "What to fetch (http or https). Allowed hosts are set by [http] allow_hosts in the config file.", ..F }))
                (dlg::choice(prefix, "method", "Method", "", &[("GET", "GET", None), ("POST", "POST", None)], &method, |_| false))
                (fld(Field { name: "link", label: "Link", mono: true, hint: "Where the title links to (http, https or a path).", ..F }))
                (fld(Field { name: "icon", label: "Icon", mono: true, hint: "A dashboard-icons name, an http(s) URL or a file in data/icons/.", ..F }))
                (fld(Field { name: "refresh", label: "Refresh", hint: "At least 1s. Default 30s.", placeholder: Some("30s"), ..D }))
                (fld(Field { name: "timeout", label: "Timeout", hint: "From 100ms to 5m. Default 5s.", placeholder: Some("5s"), v: Some("duration"), ..D }))
                (dlg::switch(prefix, "enabled", "Enabled", "Off: kept in the file, not shown or fetched.", enabled, None, false))
            }
            div.ro-box data-ro hidden[ro.is_empty()] {
                p { (ic("lock")) span { "Set in the config file on the host and kept as they are. Secret headers are bound to the URL, so then the URL can only be changed there too." } }
                ul.kv { @for (a, b) in &ro { li { code { (a) } ": " (b) } } }
            }
        }))
        (panel("fields", "Fields", false, html! {
            p.hint id={(prefix) "-fields-h"} { "Name a value and give its path in the response, e.g. " code { "RuntimeStats.Uptime" } " or " code { "items.0.name" } ". Fetch now shows what each path finds." }
            // A `.fld` so that an error about the fields lands under them.
            div.fld.wide {
                div.kv-head aria-hidden="true" { span { "Field name" } span { "Path in the response" } }
                div.rows data-rows="field" id={(prefix) "-fields"} {
                    @for (i, (a, b)) in fields.iter().enumerate() { (kv_row(prefix, "field", i, a, b)) }
                }
                p.fld-err id={(prefix) "-fields-e"} hidden {}
            }
            @if tabs { button.btn.sm type="button" data-kv-add="field" { (ic("plus")) "Add field" } }
        }))
        (panel("display", "Display", false, html! {
            (dlg::choice(prefix, "mode", "Show as", "", &[("template", "One line (template)", None), ("rows", "Rows (label and value)", None)], &mode, |_| false))
            (fld(Field { name: "template", label: "Template", mono: true, wide: true, for_: Some("mode:template"), hint: "Fields in {{double braces}}, with filters: {{size | bytes}}, {{n | number}}, {{s | duration}}, {{x | upper}}.", ..F }))
            div.fld.wide data-for="mode:rows" hidden[mode != "rows"] {
                span.fld-l { "Rows" }
                div.kv-head aria-hidden="true" { span { "Label" } span { "Value (template)" } }
                div.rows data-rows="row" id={(prefix) "-rows"} {
                    @for (i, (a, b)) in rows.iter().enumerate() { (kv_row(prefix, "row", i, a, b)) }
                }
                p.fld-err id={(prefix) "-rows-e"} hidden {}
                @if tabs { button.btn.sm type="button" data-kv-add="row" { (ic("plus")) "Add row" } }
            }
            div.grid-2 {
                (fld(Field { name: "status_field", label: "Status field", mono: true, hint: "A field name from the Fields tab. Empty: no status dot.", ..F }))
                (fld(Field { name: "status_ok", label: "OK values", mono: true, hint: "Values that mean OK, separated by commas, e.g. 200 or pass.", ..F }))
            }
        }))
        @if tabs {
            div.preview-out #wf-preview aria-live="polite" hidden {}
        }
    }
}

/// Rules (names) that watch the widget `id`.
fn rules_using(disk: &Disk, id: &str) -> Vec<String> {
    disk.list("alerts")
        .iter()
        .filter(|r| str_of(r, "widget") == Some(id))
        .map(|r| str_of(r, "name").unwrap_or("?").to_string())
        .collect()
}

fn row(disk: &Disk, i: usize, n: usize, t: &Map<String, J>) -> Markup {
    let id = str_of(t, "id").unwrap_or("");
    let key = format!("{}:{id}", SPEC.kind);
    let href = dlg::edit_link(SPEC.kind, id);
    let data = J::Object(widget_item(t)).to_string();
    let title = str_of(t, "title").filter(|s| !s.is_empty()).unwrap_or(id);
    let enabled = t.get("enabled").and_then(J::as_bool) != Some(false);
    let mv = |dir: &str, label: &str, icon: &str, edge: bool| {
        html! {
            button.ia type="button" data-move=(dir) aria-label={(label) " " (id)} data-tip=(label)
                disabled data-ready=[(!edge).then_some("")] { (ic(icon)) }
        }
    };
    html! {
        tr data-key=(key) data-rules=(J::from(rules_using(disk, id)).to_string()) {
            td data-l="Widget" {
                a.linkbtn href=(href) data-dlg=(SPEC.id) data-key=(key) data-item=(data) { (title) }
                code.sub.sub-c { (id) }
            }
            td.url-c data-l="URL" { code.wrap { (sval(Some(t), "url")) } }
            td.num data-l="Refresh" { code { @if t.contains_key("refresh") { (sval(Some(t), "refresh")) } @else { "30s" } } }
            td data-l="State" {
                @if enabled { span.pill.ok { "enabled" } } @else { span.pill.off { "disabled" } }
                @if bound(t) { " " span.pill title="Has secret headers (set on the host)" { "secret headers" } }
            }
            td.act {
                div.ra {
                    (mv("up", "Move up", "arrow-up", i == 0))
                    (mv("down", "Move down", "arrow-down", i + 1 == n))
                    a.ia href=(href) data-dlg=(SPEC.id) data-key=(key) data-item=(data) aria-label={"Edit " (id)} data-tip="Edit" { (ic("edit")) }
                    a.ia href=(href) data-menu="m-w" role="button" aria-haspopup="menu" aria-expanded="false"
                        aria-label={"More actions for " (id)} data-tip="More" { (ic("more")) }
                }
            }
        }
    }
}

/// The custom-api list: also the `?frag=list` answer. `data-order` is every
/// widget id in file order (the command ones too), for the move arrows.
pub fn list_markup(disk: &Disk) -> Markup {
    let all = disk.list("widgets");
    let ws: Vec<&Map<String, J>> = all.iter().filter(|w| !locked_json(w)).collect();
    let order: Vec<&str> = all.iter().map(|w| str_of(w, "id").unwrap_or("")).collect();
    html! {
        div #w-list data-hash=(disk.hash) data-order=(json!(order).to_string())
            data-sys=(if disk.cfg.as_ref().map(|c| c.system.enabled).unwrap_or(true) { "1" } else { "0" }) {
            @match &disk.cfg {
                Err(e) => p.note.bad {
                    (ic("alert"))
                    span { "The config file does not load, so the list cannot be shown: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } }
                },
                Ok(_) if ws.is_empty() => div.empty {
                    span.empty-ic { (ic("zap")) }
                    h3 { "No custom API widgets yet" }
                    p { "A widget fetches a JSON (or text) URL on a schedule and shows fields from it on the Overview." }
                    div.acts { a.btn.primary href="?add=widget" data-dlg=(SPEC.id) { (ic("plus")) "Add widget" } }
                },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th scope="col" { "Widget" } th scope="col" { "URL" } th.num scope="col" { "Refresh" } th scope="col" { "State" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody { @for (i, t) in ws.iter().enumerate() { (row(disk, i, ws.len(), t)) } }
                    }
                },
            }
        }
    }
}

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let locked_ws: Vec<&WidgetConfig> = ctx.cfg.widgets.iter().filter(|w| is_locked(w)).collect();
    let q = parse_query(ctx.query);
    let page = dlg::landed(&q, SPEC.kind).map(|e| match e {
        None => dlg::page_with(&SPEC, ACTION, None, widget_body("wfp", None, false)),
        Some(id) => match disk
            .list("widgets")
            .iter()
            .find(|w| str_of(w, "id") == Some(id) && !locked_json(w))
        {
            Some(t) => {
                let v = widget_item(t);
                dlg::page_with(&SPEC, ACTION, Some(&v), widget_body("wfp", Some(&v), false))
            }
            None => dlg::gone("widget"),
        },
    });
    let sys = disk.cfg.as_ref().map(|c| c.system.enabled).unwrap_or(true);
    html! {
        @if let Some(p) = page { (p) } @else { (dlg::needs_js(dlg::Values::Shown)) }
        section.panel #sys-sec aria-labelledby="sys-h" {
            div.panel-h { h2 #sys-h { "System tiles" } }
            div.fld.tg.sys-tg {
                input.sw #sys-tiles type="checkbox" role="switch" checked[sys] disabled data-ready aria-describedby="sys-tiles-h";
                label for="sys-tiles" { "Show host tiles and vitals" }
                p.hint #sys-tiles-h { "CPU, memory, disk, network and temperature on the overview and in the top bar. Saves when switched." }
            }
        }
        section.panel #w-sec aria-labelledby="w-h" {
            div.panel-h {
                h2 #w-h { "Custom API widgets" }
                a.btn.sm.head-act #w-add href="?add=widget" data-dlg=(SPEC.id) { (ic("plus")) "Add widget" }
            }
            p.muted.sf-lead { "A custom-api widget fetches a JSON (or text) URL on a schedule and shows fields from it. Allowed hosts are set by " code { "[http] allow_hosts" } " in the config file." }
            (list_markup(&disk))
        }
        @if !locked_ws.is_empty() { (locked(&locked_ws)) }
    }
}

/// The dialog of this tab (rendered outside `#set-main`).
pub fn dialogs() -> Markup {
    html! {
        (dlg::dialog_with(&SPEC, ACTION, widget_body(SPEC.prefix, None, true), html! {
            button.btn type="button" data-act="wf-preview" data-busy="Fetching…" { (ic("refresh")) "Fetch now" }
        }))
        (dlg::row_menu("m-w"))
    }
}
