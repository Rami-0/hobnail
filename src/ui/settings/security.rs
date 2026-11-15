//! Security: change the password, sign out other sessions, recent logins.

use maud::{html, Markup};

use super::form::loading;
use crate::ui::ic;

fn pw(id: &str, name: &str, label: &str, auto: &str, hint: &str) -> Markup {
    html! {
        div.fld {
            label for=(id) { (label) }
            input.in #(id) type="password" name=(name) autocomplete=(auto) aria-describedby={(id) "-h " (id) "-e"};
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
            p.fld-err id={(id) "-e"} hidden {}
        }
    }
}

pub fn body() -> Markup {
    html! {
        section.panel aria-labelledby="pw-h" {
            div.panel-h { h2 #pw-h { "Change password" } }
            form.sf #f-password data-form="password" novalidate {
                div.sf-grid {
                    (pw("pw-current", "current", "Current password", "current-password", ""))
                    (pw("pw-new", "new", "New password", "new-password", "At least 8 characters."))
                    (pw("pw-repeat", "repeat", "Repeat the new password", "new-password", ""))
                }
                div.sf-note hidden {}
                div.sf-foot {
                    button.btn.primary type="submit" { (ic("check")) "Change password" }
                    span.sf-msg role="status" aria-live="polite" {}
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
