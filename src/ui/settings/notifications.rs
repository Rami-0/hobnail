//! Notifications: channels (Slack, Telegram, webhook) by environment
//! variable NAME with a set / not set badge, never a value; **Send test**;
//! and the alert rules editor covering every `RuleConfig` field.

use maud::{html, Markup};

use super::form::{foot, head, loading, Form, In, MONO, NUM, T, TEXT};
use crate::ui::ic;
use crate::ui::shell::Ctx;

const ENV_HINT: &str =
    "The NAME of an environment variable of this container (A-Z, 0-9, _), never the secret itself.";

fn channel_form() -> Markup {
    let f = Form("ch");
    html! {
        form.sf.editor #f-channel data-form="channel" hidden method="post" action="/settings/notifications" novalidate {
            h3.ed-h #ch-ed-h { "Add channel" }
            div.sf-grid {
                (f.input("name", "Name", "Rules refer to it; letters, digits, - and _.", MONO))
                (f.select("type", "Type", "", &[("slack", "Slack"), ("telegram", "Telegram"), ("webhook", "Webhook (ntfy, Gotify, …)")], None))
                (f.input("webhook_url_env", "Webhook URL variable", ENV_HINT, In { for_: Some("slack"), placeholder: Some("RH_SLACK_WEBHOOK_URL"), ..MONO }))
                (f.input("token_env", "Bot token variable", ENV_HINT, In { for_: Some("telegram"), placeholder: Some("RH_TELEGRAM_TOKEN"), ..MONO }))
                (f.input("chat_id", "Chat id", "The chat, group or channel id (not a secret).", In { for_: Some("telegram"), t: T::Json, ..MONO }))
                (f.input("url_env", "URL variable", "Preferred: such URLs often contain a token.", In { for_: Some("webhook"), placeholder: Some("RH_WEBHOOK_URL"), ..MONO }))
                (f.input("url", "URL (plain text)", "Stored in the config file and its revisions as plain text. Use the URL variable if it contains a token.", In { for_: Some("webhook"), ..MONO }))
                (f.input("timeout", "Timeout", "Default 10s.", In { placeholder: Some("10s"), ..MONO }))
            }
            div.ro-box #ch-ro hidden {
                p { (ic("lock")) " Set in the config file on the host (bound to the destination):" }
                ul.kv #ch-ro-list {}
            }
            (foot("Save channel", html! { button.btn type="button" data-act="ed-cancel" { "Cancel" } }))
        }
    }
}

fn rule_form(ctx: &Ctx) -> Markup {
    let f = Form("ru");
    // `alerts::rules::CONDITIONS` (private there).
    let conds = [
        ("stopped", "stopped"),
        ("exit_nonzero", "exit non-zero"),
        ("unhealthy", "unhealthy"),
        ("restarting", "restarting"),
    ];
    html! {
        datalist #widget-ids { @for w in &ctx.cfg.widgets { option value=(w.id) {} } }
        form.sf.editor #f-rule data-form="rule" hidden method="post" action="/settings/notifications" novalidate {
            h3.ed-h #ru-ed-h { "Add rule" }
            div.sf-grid {
                (f.input("name", "Name", "Unique.", MONO))
                (f.select("type", "Type", "", &[("container", "Container state"), ("http", "HTTP check"), ("host", "Host metric"), ("widget", "Widget value"), ("log_match", "Log match")], None))
                (f.toggle("enabled", "Enabled", "", None))
            }
            fieldset.sub-f {
                legend { "Condition" }
                div.sf-grid {
                    // container
                    (f.input("containers", "Containers", "Comma-separated names or globs. Default: all (*).", In { for_: Some("container log_match"), t: T::List, placeholder: Some("*"), ..MONO }))
                    (f.input("exclude", "Except", "Comma-separated names or globs.", In { for_: Some("container"), t: T::List, ..MONO }))
                    (f.set("on", "Fire on", "Default: stopped, unhealthy, restarting.", &conds, Some("container")))
                    (f.input("restarts", "Restarts", "Also fire after this many restarts within the window.", In { for_: Some("container"), ..NUM }))
                    (f.input("window", "Window", "e.g. 10m.", In { for_: Some("container log_match"), ..MONO }))
                    (f.input("pattern", "Pattern", "Regex matched against each log line (no backreferences or lookaround).", In { for_: Some("log_match"), mono: true, wide: true, ..TEXT }))
                    (f.input("stack", "Stack", "Watch every container of this compose project.", In { for_: Some("log_match"), ..MONO }))
                    (f.input("count", "Lines", "Matching lines within the window needed to fire. Default 1.", In { for_: Some("log_match"), ..NUM }))
                    // http
                    (f.input("url", "URL", "", In { for_: Some("http"), mono: true, wide: true, ..TEXT }))
                    (f.input("expect_status", "Expected status", "Default: any 2xx.", In { for_: Some("http"), ..NUM }))
                    (f.input("contains", "Body contains", "", In { for_: Some("http"), ..TEXT }))
                    (f.input("failures", "Failures in a row", "Default 1.", In { for_: Some("http"), ..NUM }))
                    (f.input("timeout", "Timeout", "", In { for_: Some("http"), placeholder: Some("10s"), ..MONO }))
                    // host
                    (f.select("metric", "Metric", "", &[("disk", "Disk used %"), ("memory", "Memory used %"), ("cpu", "CPU %"), ("temp", "Temperature °C")], Some("host")))
                    (f.input("disk", "Disk", "Disk label glob (metric disk). Default: all disks.", In { for_: Some("host"), ..MONO }))
                    (f.input("interval", "Check every", "Default 60s (host 30s).", In { for_: Some("http host"), ..MONO }))
                    // widget
                    (f.input("widget", "Widget", "Widget id.", In { for_: Some("widget"), list: Some("widget-ids"), ..MONO }))
                    (f.input("field", "Field", "A field of that widget.", In { for_: Some("widget"), ..MONO }))
                    (f.input("equals", "Equals", "Fire when the field equals this.", In { for_: Some("widget"), t: T::Json, ..MONO }))
                    (f.input("not_equals", "Not equals", "Fire when the field is not this.", In { for_: Some("widget"), t: T::Json, ..MONO }))
                    (f.toggle("on_error", "Fire when the widget fails", "", Some("widget")))
                    // host and widget
                    (f.input("above", "Above", "", In { for_: Some("host widget"), ..NUM }))
                    (f.input("below", "Below", "", In { for_: Some("host widget"), ..NUM }))
                }
            }
            fieldset.sub-f {
                legend { "Notify" }
                div.sf-grid {
                    (f.set("channels", "Channels", "None: the incident is only shown on the page.", &[], None))
                    (f.input("for", "For", "Must be bad this long before firing. Default 30s (container), else 0.", In { placeholder: Some("0s"), ..MONO }))
                    (f.input("cooldown", "Cooldown", "Minimum time between two notifications. Default 15m.", In { placeholder: Some("15m"), ..MONO }))
                    (f.input("message", "Message", "Template; empty: the default message.", In { wide: true, ..TEXT }))
                    (f.input("recovery", "Recovery message", "Template; empty: the default.", In { wide: true, ..TEXT }))
                }
            }
            (foot("Save rule", html! { button.btn type="button" data-act="ed-cancel" { "Cancel" } }))
        }
    }
}

pub fn body(ctx: &Ctx) -> Markup {
    html! {
        // The channel and rule editors open with the script only.
        (super::dlg::needs_js(super::dlg::Values::LoadWithJs))
        section.panel aria-labelledby="ch-h" {
            (head("Channels", Some(("ch-new", "plus", "Add channel"))))
            div.sec-note hidden {}
            p.muted.sf-lead { "Secrets stay in the container's environment (e.g. a " code { "secrets.env" } " file in compose). Here you only name the variable; the page shows whether it is set." }
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th { "Channel" } th { "Type" } th { "Secrets" } th { "Status" } th.act { span.sr { "Actions" } } } }
                    tbody #ch-list { tr { td colspan="5" { (loading()) } } }
                }
            }
            (channel_form())
        }
        section.panel aria-labelledby="ru-h" {
            (head("Alert rules", Some(("ru-new", "plus", "Add rule"))))
            div.sec-note hidden {}
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th { "Rule" } th { "Type" } th { "Condition" } th { "Channels" } th { "Timing" } th.act { span.sr { "Actions" } } } }
                    tbody #ru-list { tr { td colspan="6" { (loading()) } } }
                }
            }
            (rule_form(ctx))
        }
    }
}
