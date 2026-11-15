//! Widgets: system tiles on/off, the `custom-api` widgets (list, editor with
//! a **Fetch now** preview) and, read-only, the widgets that can run a
//! program (`command`, or anything not `custom-api`). Those are rendered
//! here from the loaded config, with no controls at all: they are managed
//! in the file on the host (see `settings::guard`).

use maud::{html, Markup};

use super::form::{foot, head, loading, Form, In, MONO, TEXT};
use crate::ui::ic;
use crate::ui::shell::Ctx;
use crate::widgets::WidgetConfig;

pub const LOCKED_TEXT: &str = "Defined in the config file on the host — command widgets can't be created, changed or enabled from the app.";

/// Mirrors `settings::guard::is_command_capable` (fail closed).
pub fn is_locked(w: &WidgetConfig) -> bool {
    w.kind != "custom-api" || !w.command.is_empty() || !w.env.is_empty()
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

pub fn body(ctx: &Ctx) -> Markup {
    let locked_ws: Vec<&WidgetConfig> = ctx.cfg.widgets.iter().filter(|w| is_locked(w)).collect();
    let sys = Form("sys");
    let f = Form("wf");
    html! {
        section.panel aria-labelledby="sys-h" {
            div.panel-h { h2 #sys-h { "System tiles" } }
            form.sf #f-system data-form="system" novalidate {
                div.sf-grid {
                    (sys.toggle("enabled", "Show host tiles and vitals", "CPU, memory, disk, network and temperature on the overview and in the top bar.", None))
                }
                (foot("Save", html! {}))
            }
        }
        section.panel aria-labelledby="w-h" {
            (head("Custom API widgets", Some(("w-new", "plus", "Add widget"))))
            div.sec-note hidden {}
            p.muted.sf-lead { "A custom-api widget fetches a JSON (or text) URL on a schedule and shows fields from it. Allowed hosts are set by " code { "[http] allow_hosts" } " in the config file." }
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th { "Widget" } th { "URL" } th.num { "Refresh" } th { "State" } th.act { span.sr { "Actions" } } } }
                    tbody #w-list { tr { td colspan="5" { (loading()) } } }
                }
            }
            form.sf.editor #f-widget data-form="widget" hidden novalidate {
                h3.ed-h #w-ed-h { "Add widget" }
                div.sf-grid {
                    (f.input("id", "Id", "Unique; letters, digits, - and _. Used by alert rules.", MONO))
                    (f.text("title", "Title", "Empty: the id."))
                    (f.input("url", "URL", "What to fetch (http or https).", In { mono: true, wide: true, ..TEXT }))
                    (f.select("method", "Method", "", &[("", "GET"), ("POST", "POST")], None))
                    (f.input("link", "Link", "Where the title links to.", In { mono: true, ..TEXT }))
                    (f.input("icon", "Icon", "A dashboard-icons name, an http(s) URL or a file in data/icons/.", MONO))
                    (f.input("refresh", "Refresh", "e.g. 30s, 5m. Default 30s.", In { placeholder: Some("30s"), ..MONO }))
                    (f.input("timeout", "Timeout", "Default 5s.", In { placeholder: Some("5s"), ..MONO }))
                    (f.toggle("enabled", "Enabled", "", None))
                }
                div.ro-box #wf-secrets hidden {
                    p { (ic("lock")) " Secret headers are set in the config file on the host (they are bound to their URL, so the URL can't be changed here either)." }
                    ul.kv #wf-secrets-list {}
                }
                fieldset.sub-f {
                    legend { "Fields" }
                    p.hint { "Name a value and give its path in the response, e.g. " code { "RuntimeStats.Uptime" } " or " code { "items.0.name" } "." }
                    div.rows #wf-fields {}
                    button.btn.sm type="button" data-act="wf-field-add" { (ic("plus")) "Add field" }
                }
                fieldset.sub-f {
                    legend { "Display" }
                    div.seg-r role="radiogroup" aria-label="Display as" {
                        label.cb { input type="radio" name="wf-mode" value="template" checked; span { "One line (template)" } }
                        label.cb { input type="radio" name="wf-mode" value="rows"; span { "Rows (label and value)" } }
                    }
                    div #wf-mode-template {
                        (f.input("template", "Template", "Fields in {{double braces}}, with filters: {{size | bytes}}, {{n | number}}, {{s | duration}}, {{x | upper}}.", In { mono: true, wide: true, ..TEXT }))
                    }
                    div #wf-mode-rows hidden {
                        div.rows #wf-rows {}
                        button.btn.sm type="button" data-act="wf-row-add" { (ic("plus")) "Add row" }
                    }
                }
                fieldset.sub-f {
                    legend { "Status dot" }
                    div.sf-grid {
                        (f.input("status.field", "Status field", "A field name from above. Empty: no status dot.", MONO))
                        (f.input("status.ok", "OK values", "Comma-separated values that mean OK, e.g. 200 or pass.", In { t: super::form::T::List, ..MONO }))
                    }
                }
                div.preview {
                    div.preview-bar {
                        button.btn type="button" data-act="wf-preview" { (ic("refresh")) "Fetch now" }
                        span.muted { "Runs the unsaved form once and shows the result." }
                    }
                    div.preview-out #wf-preview aria-live="polite" hidden {}
                }
                (foot("Save widget", html! { button.btn type="button" data-act="ed-cancel" { "Cancel" } }))
            }
        }
        @if !locked_ws.is_empty() { (locked(&locked_ws)) }
    }
}
