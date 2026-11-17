//! History and data (signed in only; the caller checks): `[history]` as a
//! switch that saves when changed (Recording, owner decision Y21) and a
//! one-field page form (Sample interval) on the dialog's submit path, the
//! database statistics, the export link, and Clear history behind the one
//! confirm component with its typed phrase. All rendered on the server.

use maud::{html, Markup};
use serde_json::Value as J;

use super::disk::Disk;
use super::dlg::{self, D};
use crate::ui::fmt::time_tag;
use crate::ui::ic;
use crate::ui::shell::Ctx;
use crate::util::human_bytes;

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let h = disk
        .doc
        .get("history")
        .and_then(J::as_object)
        .cloned()
        .unwrap_or_default();
    let enabled = h.get("enabled").and_then(J::as_bool).unwrap_or(true);
    let interval = dlg::sval(Some(&h), "sample_interval");
    let f = dlg::Field {
        name: "sample_interval",
        label: "Sample interval",
        hint: "How often host and container samples are taken: 5s to 5m. Default 15s. Use a duration like 30s or 5m.",
        placeholder: Some("15s"),
        ..D
    };
    html! {
        section.panel #hi-sec aria-labelledby="hi-h" {
            div.panel-h { h2 #hi-h { "Recording" } }
            @match &disk.cfg {
                Err(e) => p.note.bad { (ic("alert")) span { "The config file does not load: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } } },
                Ok(_) => {
                    div.fld.tg #hi-rec data-hash=(disk.hash) {
                        input.sw #hi-enabled type="checkbox" role="switch" checked[enabled] disabled data-ready aria-describedby="hi-enabled-h";
                        label for="hi-enabled" { "Record host and container samples" }
                        p.hint #hi-enabled-h { "Saves when switched. Events are recorded either way while the database is available." }
                    }
                    form.sf #f-history method="post" action="/settings/history" novalidate data-hb data-hash=(disk.hash) {
                        div.note.dlg-note hidden tabindex="-1" {}
                        div.grid-2 data-hb-body { (dlg::field("hi", &f, &interval, false)) }
                        (dlg::needs_js(dlg::Values::Shown))
                        (super::general::foot("Save"))
                    }
                },
            }
        }
        section.panel aria-labelledby="db-h" {
            div.panel-h { h2 #db-h { "Database" } }
            (stats(ctx))
            div.data-acts {
                a.btn #db-export href="/api/settings/history/export" download { (ic("arrow-down")) "Export snapshot (JSON)" }
                span.muted { "Events, revision list and database statistics." }
            }
        }
        section.panel.danger-zone aria-labelledby="clr-h" {
            div.panel-h { h2 #clr-h { "Clear history" } }
            p.muted.sf-lead { "Deletes all recorded samples and events. Config revisions and the login are kept. This cannot be undone." }
            div.data-acts {
                button.btn.danger #clr-btn type="button" disabled data-ready { (ic("x")) "Clear history…" }
                span.muted { "Asks you to type " code { "clear history" } " first." }
            }
        }
    }
}

/// The database figures (also the `?frag=list` answer after Clear history).
pub fn stats(ctx: &Ctx) -> Markup {
    let db = &ctx.state.db;
    let now = crate::util::unix_now();
    let s = db.stats();
    let n = |v: u64| {
        // 12345 → "12,345"
        let d = v.to_string();
        let mut out = String::new();
        for (i, c) in d.chars().enumerate() {
            if i > 0 && (d.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    html! {
        div #db-stats {
            @if !db.is_enabled() {
                p.muted { "The history database is not available (see the server log), so nothing is recorded." }
            } @else {
                dl.stats {
                    div { dt { "Size on disk" } dd.num { (human_bytes(s.size_bytes)) } }
                    div { dt { "Host samples" } dd.num { (n(s.sys_rows)) } }
                    div { dt { "Container samples" } dd.num { (n(s.ctr_rows)) } }
                    div { dt { "Events" } dd.num { (n(s.event_rows)) } }
                    div { dt { "Config revisions" } dd.num { (n(s.revisions)) } }
                    div { dt { "Oldest sample" } dd.num { @match s.oldest_ts { Some(t) => (time_tag(now, t.max(0) as u64)), None => "—" } } }
                }
            }
        }
    }
}
