//! History and data: `[history]`, database statistics, export, and clearing
//! the history (typed confirmation).

use maud::{html, Markup};

use super::form::{foot, loading, Form, In, MONO};
use crate::ui::ic;

pub fn body() -> Markup {
    let f = Form("hi");
    html! {
        section.panel aria-labelledby="hi-h" {
            div.panel-h { h2 #hi-h { "Recording" } }
            form.sf #f-history data-form="history" method="post" action="/settings/history" novalidate {
                div.sf-grid {
                    (f.toggle("enabled", "Record host and container samples", "Events are recorded either way while the database is available.", None))
                    (f.input("sample_interval", "Sample interval", "5s to 5m. Default 15s.", In { placeholder: Some("15s"), ..MONO }))
                }
                (foot("Save", html! {}))
            }
        }
        section.panel aria-labelledby="db-h" {
            div.panel-h { h2 #db-h { "Database" } }
            dl.stats #db-stats { (loading()) }
            div.data-acts {
                a.btn #db-export href="/api/settings/history/export" download { (ic("arrow-down")) "Export snapshot (JSON)" }
                span.muted { "Events, revision list and database statistics." }
            }
        }
        section.panel.danger-zone aria-labelledby="clr-h" {
            div.panel-h { h2 #clr-h { "Clear history" } }
            form.sf #f-clear data-form="clear" method="post" action="/settings/history" novalidate {
                p.muted.sf-lead { "Deletes all recorded samples and events. Config revisions and the login are kept. This cannot be undone." }
                div.sf-grid {
                    div.fld {
                        label for="clr-confirm" { "Type " code { "clear history" } " to confirm" }
                        input.in.mono #clr-confirm type="text" autocomplete="off" spellcheck="false" aria-describedby="clr-confirm-e";
                        p.fld-err #clr-confirm-e hidden {}
                    }
                }
                div.sf-note hidden {}
                (super::dlg::needs_js(super::dlg::Values::None))
                div.sf-foot {
                    button.btn.danger type="submit" disabled { (ic("x")) "Clear history" }
                    span.sf-msg role="status" aria-live="polite" {}
                }
            }
        }
    }
}
