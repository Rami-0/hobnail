//! General: the `[page]` keys this form offers, rendered with the values in
//! the file (signed in only; the caller checks). One page-section form on
//! the dialog's submit path (`data-hb`): all errors at once on their
//! fields, a busy Save, and only the fields whose text changed are sent (an
//! emptied field removes its key, so the default applies).
//!
//! Theme lives in Appearance; Columns and Layout are no longer offered
//! (DESIGN.md §11.12). Their keys stay in the file as they are: the form
//! never sends them.

use maud::{html, Markup};
use serde_json::Value as J;

use super::disk::Disk;
use super::dlg::{self, Field, F};
use crate::ui::shell::Ctx;

/// The page form's id prefix (`gen-title`, …).
const P: &str = "gen";

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let page = disk
        .doc
        .get("page")
        .and_then(J::as_object)
        .cloned()
        .unwrap_or_default();
    let p = &ctx.cfg.page;
    let host = ctx.state.env.hostname.as_str();
    let text = |name, label, hint, max, ph: &'static str, wide| Field {
        name,
        label,
        hint,
        max,
        wide,
        placeholder: (!ph.is_empty()).then_some(ph),
        ..F
    };
    let title = Field {
        placeholder: Some(host),
        ..text(
            "title",
            "Title",
            "Shown in the sidebar and the browser tab. Empty: the host name.",
            80,
            "",
            false,
        )
    };
    let greeting = text(
        "greeting",
        "Greeting",
        "Line under the title on the overview. Empty: a time-of-day greeting.",
        200,
        "Good morning / afternoon / evening",
        false,
    );
    let search = Field {
        mono: true,
        v: Some("http"),
        pattern: Some(r".*\{q\}.*"),
        msg: Some("Put {q} where the search words go."),
        inputmode: Some("url"),
        ..text(
            "search_url",
            "Search URL",
            "Used by the command palette for web searches; {q} is replaced by the query.",
            0,
            "https://duckduckgo.com/?q={q}",
            true,
        )
    };
    let on = |k: &str, effective: bool| page.get(k).and_then(J::as_bool).unwrap_or(effective);
    let v = |k: &str| dlg::sval(Some(&page), k);
    html! {
        section.panel aria-labelledby="gen-h" {
            div.panel-h { h2 #gen-h { "Page" } }
            @match &disk.cfg {
                Err(e) => p.note.bad { (crate::ui::ic("alert")) span { "The config file does not load, so these settings cannot be shown: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } } },
                Ok(_) => form.sf #f-general method="post" action="/settings/general" novalidate data-hb data-hash=(disk.hash) {
                    div.note.dlg-note hidden tabindex="-1" {}
                    div.grid-2 data-hb-body {
                        (dlg::field(P, &title, &v("title"), false))
                        (dlg::field(P, &greeting, &v("greeting"), false))
                        (dlg::field(P, &search, &v("search_url"), false))
                        (dlg::page_switch(P, "show_stopped", "Show stopped containers", "Off: only running containers appear on the overview.", on("show_stopped", p.show_stopped)))
                        (dlg::page_switch(P, "show_stats", "Show live CPU and memory", "On the overview and in the containers table.", on("show_stats", p.show_stats)))
                    }
                    (dlg::needs_js(dlg::Values::Shown))
                    (foot("Save"))
                }
            }
        }
    }
}

/// A page form's footer: the one primary (disabled until the script runs)
/// and the status line the submit path writes to.
pub fn foot(save: &str) -> Markup {
    html! {
        div.sf-foot {
            button.btn.primary type="submit" disabled data-ready { (crate::ui::ic("check")) (save) }
            span.dlg-msg role="status" {}
        }
    }
}
