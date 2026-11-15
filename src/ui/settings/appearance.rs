//! Appearance: the theme (`[page] theme`, saved through the General API) and
//! the `custom.css` editor with a live preview (the script applies the
//! textarea to `<style id="rh-preview">` while editing).

use maud::{html, Markup};

use super::form::{foot, Form};
use crate::ui::ic;

const CSS_PLACEHOLDER: &str = "/* Any token can be overridden, e.g. */\n:root {\n  --accent: #f59e0b;\n  --accent-bg: #d97706;\n}\n";

pub fn body() -> Markup {
    let f = Form("ap");
    html! {
        section.panel aria-labelledby="th-h" {
            div.panel-h { h2 #th-h { "Theme" } }
            form.sf #f-theme data-form="theme" novalidate {
                div.sf-grid {
                    (f.select("theme", "Default theme", "For browsers that have not picked one with the theme button in the top bar.", &[("", "Default (auto)"), ("auto", "Auto (follow the OS)"), ("dark", "Dark"), ("light", "Light"), ("ember", "Ember (warm dark)"), ("contrast", "High contrast")], None))
                }
                p.hint #th-local hidden {
                    "This browser uses its own choice from the theme button. "
                    button.linklike type="button" data-act="th-forget" { "Use the default here" }
                }
                (foot("Save", html! {}))
            }
        }
        section.panel aria-labelledby="css-h" {
            div.panel-h {
                h2 #css-h { "Custom CSS" }
                span.muted.css-size #css-size {}
            }
            form.sf #f-css data-form="css" novalidate {
                p.muted.sf-lead { "Saved as " code { "custom.css" } " in the data directory and loaded after the app's styles on every page." }
                label.cb.css-live { input #css-live type="checkbox" checked; span { "Live preview on this page" } }
                div.fld.wide {
                    label for="css-text" { "custom.css" }
                    textarea.in.mono.code #css-text name="css" rows="16" spellcheck="false" wrap="off"
                        placeholder=(CSS_PLACEHOLDER) aria-describedby="css-text-e" {}
                    p.fld-err #css-text-e hidden {}
                }
                (foot("Save", html! {
                    button.btn type="button" data-act="css-revert" { (ic("refresh")) "Revert" }
                }))
            }
        }
    }
}
