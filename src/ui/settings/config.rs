//! Config file: the raw TOML editor (validated as you type, the error line
//! highlighted) and the revisions (view, diff against the current file,
//! restore).

use maud::{html, Markup};

use super::form::loading;
use crate::ui::ic;

pub fn body() -> Markup {
    html! {
        section.panel aria-labelledby="raw-h" {
            div.panel-h {
                h2 #raw-h { "config.toml" }
                code.muted.raw-path #raw-path {}
            }
            form.sf #f-raw data-form="raw" novalidate {
                div.editor-wrap {
                    pre.gutter #raw-gutter aria-hidden="true" {}
                    div.ed-hl #raw-hl hidden {}
                    label.sr for="raw-text" { "config.toml" }
                    textarea.in.mono.code #raw-text name="text" rows="28" spellcheck="false" wrap="off"
                        autocomplete="off" autocapitalize="off" aria-describedby="raw-status" {}
                }
                div.raw-status #raw-status role="status" aria-live="polite" { span.muted { "Loading…" } }
                div.sf-note hidden {}
                div.sf-foot {
                    button.btn.primary type="submit" { (ic("check")) "Save" }
                    button.btn type="button" data-act="raw-revert" { (ic("refresh")) "Revert" }
                    button.btn type="button" data-act="raw-goto" hidden { "Go to error" }
                    span.sf-msg role="status" aria-live="polite" {}
                }
            }
        }
        section.panel aria-labelledby="rev-h" {
            div.panel-h { h2 #rev-h { "Revisions" } }
            div.sec-note hidden {}
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th.num { "#" } th { "Time" } th { "By" } th { "Note" } th.num { "Size" } th.act { span.sr { "Actions" } } } }
                    tbody #rev-list { tr { td colspan="6" { (loading()) } } }
                }
            }
            div.rev-view #rev-view hidden {
                div.rev-view-h {
                    h3 #rev-view-t {}
                    button.btn.sm type="button" data-act="rev-close" { (ic("x")) "Close" }
                }
                div.rev-view-b #rev-view-b {}
            }
        }
    }
}
