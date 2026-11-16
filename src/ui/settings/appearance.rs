//! Appearance (signed in only; the caller checks): the default theme
//! (`[page] theme`, a single choice that saves when changed, owner decision
//! Y21; Ember stays a theme, Y1) and the `custom.css` editor, a page form
//! on the dialog's submit path with a live preview. Both are rendered with
//! what is saved. When the saved custom.css sets a retired token name, a
//! note names the token to set instead (`docs/features/ui.md`).

use maud::{html, Markup};
use serde_json::Value as J;

use super::disk::Disk;
use crate::ui::ic;
use crate::ui::shell::Ctx;

const CSS_PLACEHOLDER: &str = "/* Any token can be overridden, e.g. */\n:root {\n  --accent: #f59e0b;\n  --accent-hover: #d97706;\n}\n";

pub const THEMES: &[(&str, &str)] = &[
    ("", "Default (auto)"),
    ("auto", "Auto (follow the OS)"),
    ("dark", "Dark"),
    ("light", "Light"),
    ("ember", "Ember (warm dark)"),
    ("contrast", "High contrast"),
];

/// The retired token names a custom.css sets, as a note (empty: nothing).
pub fn css_warnings(css: &str) -> Markup {
    let w = crate::settings::css_token_warnings(css);
    html! {
        div #css-warn aria-live="polite" {
            @if !w.is_empty() {
                div.note.warn {
                    (ic("alert"))
                    div {
                        strong { "Old token names: " }
                        @if w.len() == 1 { (w[0]) } @else { ul { @for m in &w { li { (m) } } } }
                    }
                }
            }
        }
    }
}

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let theme = disk
        .doc
        .get("page")
        .and_then(|p| p.get("theme"))
        .and_then(J::as_str)
        .unwrap_or("")
        .to_string();
    let css = crate::settings::css_current(ctx.state);
    let max = crate::settings::MAX_CSS;
    html! {
        (super::dlg::needs_js(super::dlg::Values::Shown))
        section.panel #th-sec aria-labelledby="th-h" {
            div.panel-h { h2 #th-h { "Theme" } }
            @match &disk.cfg {
                Err(e) => p.note.bad { (ic("alert")) span { "The config file does not load: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } } },
                Ok(_) => div.fld.th-fld #th-box data-hash=(disk.hash) {
                    label for="th-theme" { "Default theme" }
                    select.in #th-theme name="theme" disabled data-ready aria-describedby="th-theme-h" data-was=(theme) {
                        @for (v, l) in THEMES { option value=(v) selected[*v == theme.as_str()] { (l) } }
                    }
                    p.hint #th-theme-h { "For browsers that have not picked one with the theme button in the top bar. Saves when changed." }
                    p.hint #th-local hidden {
                        "This browser uses its own choice from the theme button. "
                        button.linklike type="button" data-act="th-forget" { "Use the default here" }
                    }
                },
            }
        }
        section.panel aria-labelledby="css-h" {
            div.panel-h {
                h2 #css-h { "Custom CSS" }
                span.muted.css-size #css-size data-max=(max) {}
            }
            @match &css {
                Err(e) => p.note.bad { (ic("alert")) span { (e.body["error"].as_str().unwrap_or("custom.css cannot be read")) } },
                Ok((text, hash)) => form.sf #f-css method="post" action="/settings/appearance" novalidate data-hb data-hash=(hash) {
                    p.muted.sf-lead { "Saved as " code { "custom.css" } " in the data directory and loaded after the app's styles on every page." }
                    div.note.dlg-note hidden tabindex="-1" {}
                    (css_warnings(text))
                    label.cb.css-live { input #css-live type="checkbox" checked; span { "Live preview on this page" } }
                    div.fld.wide data-hb-body {
                        label for="css-text" { "custom.css" }
                        textarea.in.mono.code #css-text name="css" rows="16" spellcheck="false" wrap="off"
                            placeholder=(CSS_PLACEHOLDER) aria-describedby="css-text-e" {
                            // A newline right after <textarea> is dropped by
                            // the parser: this one, so the text keeps its own.
                            "\n" (text)
                        }
                        p.fld-err #css-text-e hidden {}
                    }
                    (super::dlg::needs_js(super::dlg::Values::Shown))
                    div.sf-foot {
                        button.btn.primary type="submit" disabled data-ready { (ic("check")) "Save" }
                        button.btn type="button" data-act="css-revert" disabled data-ready { (ic("refresh")) "Revert" }
                        span.dlg-msg role="status" {}
                    }
                },
            }
        }
    }
}
