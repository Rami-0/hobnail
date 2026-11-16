//! Security: change the password, sign out other sessions, recent logins.

use maud::{html, Markup};

use super::form::loading;
use crate::ui::ic;

fn pw(
    id: &str,
    name: &str,
    label: &str,
    auto: &str,
    hint: &str,
    rule: Option<(&str, &str)>,
) -> Markup {
    let desc = if hint.is_empty() {
        format!("{id}-e")
    } else {
        format!("{id}-e {id}-h")
    };
    html! {
        div.fld {
            label for=(id) { (label) " " abbr.req title="required" { "*" } }
            input.in #(id) type="password" name=(name) autocomplete=(auto) required maxlength="1024"
                pattern=[rule.map(|r| r.0)] data-msg=[rule.map(|r| r.1)] aria-describedby=(desc);
            p.fld-err id={(id) "-e"} hidden {}
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
        }
    }
}

/// Change password is a page section, not a dialog, but it goes through the
/// same submit path (`data-hb`, `form._submit` in settings.js): every
/// problem at once on its field, a busy button, passwords never trimmed,
/// and `method="post"` with the button disabled until the script runs, so
/// a password can never end up in a URL (ROB-15).
pub fn body() -> Markup {
    html! {
        section.panel aria-labelledby="pw-h" {
            div.panel-h { h2 #pw-h { "Change password" } }
            form.sf.pw-form #f-password method="post" action="/settings/security" novalidate data-hb {
                div.note.dlg-note hidden tabindex="-1" {}
                div.sf-grid data-hb-body {
                    (pw("pw-current", "current", "Current password", "current-password", "", None))
                    (pw("pw-new", "new", "New password", "new-password", "At least 8 characters.", Some((".{8,}", "Use at least 8 characters."))))
                    (pw("pw-repeat", "repeat", "Repeat the new password", "new-password", "", None))
                }
                (super::dlg::needs_js(super::dlg::Values::None))
                div.sf-foot {
                    button.btn.primary type="submit" disabled data-ready data-busy="Changing…" { (ic("check")) "Change password" }
                    span.dlg-msg role="status" {}
                }
            }
        }
        section.panel aria-labelledby="ses-h" {
            div.panel-h { h2 #ses-h { "Sessions" } }
            div.sec-note hidden {}
            div.data-acts {
                button.btn type="button" data-act="sign-out-others" { (ic("log-out")) "Sign out all other sessions" }
                span.muted { "Every other browser has to sign in again. This one stays signed in." }
            }
        }
        section.panel aria-labelledby="lg-h" {
            div.panel-h { h2 #lg-h { "Recent logins" } }
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th { "Time" } th { "Result" } th { "Detail" } } }
                    tbody #lg-list { tr { td colspan="3" { (loading()) } } }
                }
            }
        }
    }
}
