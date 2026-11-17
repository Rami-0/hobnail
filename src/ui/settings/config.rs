//! Config file (signed in only; the caller checks): the raw TOML editor, a
//! page form on the dialog's submit path rendered with the file as it is
//! (validated as you type, the error line highlighted, Ctrl+S saves), and
//! the revisions, rendered on the server. View and Diff open in a dialog
//! (Esc closes it); Restore asks with the one confirm component.
//!
//! The file text and the revision list are settings data: never in a
//! signed-out page, and served `no-store` with the rest of Settings.

use maud::{html, Markup};

use super::disk::Disk;
use crate::ui::fmt::time_tag;
use crate::ui::ic;
use crate::ui::shell::Ctx;
use crate::util::human_bytes;

pub fn body(ctx: &Ctx) -> Markup {
    let cur = crate::settings::read_current(ctx.state);
    let path = ctx.state.config.path().display().to_string();
    html! {
        section.panel aria-labelledby="raw-h" {
            div.panel-h {
                h2 #raw-h { "config.toml" }
                code.muted.raw-path #raw-path title=(path) {
                    (path)
                    @if cur.as_ref().is_ok_and(|c| !c.exists) { " (does not exist yet)" }
                }
            }
            @match &cur {
                Err(e) => p.note.bad { (ic("alert")) span { (e.body["error"].as_str().unwrap_or("cannot read the config file")) } },
                Ok(c) => form.sf #f-raw method="post" action="/settings/config" novalidate data-hb data-hash=(c.hash) {
                    div.note.dlg-note hidden tabindex="-1" {}
                    div.editor-wrap data-hb-body {
                        pre.gutter #raw-gutter aria-hidden="true" {}
                        div.ed-hl #raw-hl hidden {}
                        label.sr for="raw-text" { "config.toml" }
                        textarea.in.mono.code #raw-text name="text" rows="28" spellcheck="false" wrap="off"
                            autocomplete="off" autocapitalize="off" aria-describedby="raw-status" {
                            // The parser drops a newline right after
                            // <textarea>: this one, so the file keeps its own.
                            "\n" (c.text)
                        }
                    }
                    div.raw-status #raw-status role="status" aria-live="polite" {}
                    (super::dlg::needs_js(super::dlg::Values::Shown))
                    div.sf-foot {
                        button.btn.primary type="submit" disabled data-ready { (ic("check")) "Save" }
                        button.btn type="button" data-act="raw-revert" disabled data-ready { (ic("refresh")) "Revert" }
                        button.btn type="button" data-act="raw-goto" hidden { "Go to error" }
                        span.dlg-msg role="status" {}
                        span.muted.kbd-hint { kbd { "Ctrl" } " " kbd { "S" } " saves" }
                    }
                },
            }
        }
        section.panel #rev-sec aria-labelledby="rev-h" {
            div.panel-h { h2 #rev-h { "Revisions" } }
            (revisions(ctx))
        }
    }
}

/// The revision list (also the `?frag=list` answer after a save or a
/// restore). `data-hash` is the file the list was rendered against: what a
/// Restore started from this list is based on.
pub fn revisions(ctx: &Ctx) -> Markup {
    let db = &ctx.state.db;
    let now = crate::util::unix_now();
    let hash = Disk::read(ctx.state).hash;
    let all = db.revisions(200);
    // Paged, newest first, `SHOWN` a page (SET-64): `?rp=2` is the second
    // page; a page past the end shows the last one.
    let (page, pages) = rev_page(ctx.query, all.len());
    let first = (page - 1) * SHOWN;
    let list = &all[first.min(all.len())..(first + SHOWN).min(all.len())];
    let total = all.len();
    html! {
        div #rev-list data-hash=(hash) {
            @if !db.is_enabled() {
                p.muted.none { "Revisions need the history database, which is not available." }
            } @else if list.is_empty() {
                p.muted.none { "No revisions yet. The first save from the app keeps the file as it was, too." }
            } @else {
                div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th.num scope="col" { "#" } th scope="col" { "Time" } th scope="col" { "By" } th scope="col" { "Note" } th.num scope="col" { "Size" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody {
                            @for (i, r) in list.iter().enumerate() {
                                tr data-key={"rev:" (r.id)} data-rev=(r.id) data-ts=(r.ts) data-note=(r.note) {
                                    td.num data-l="#" { code { "#" (r.id) } }
                                    td data-l="Time" { (time_tag(now, r.ts.max(0) as u64)) }
                                    td data-l="By" { @if r.author == "file" { span.pill { "on disk" } } @else { span { (r.author) } } }
                                    td.wrap data-l="Note" { span { (r.note) } @if i == 0 && page == 1 { " " span.pill.ok { "latest" } } }
                                    td.num data-l="Size" { (human_bytes(r.bytes as u64)) }
                                    td.act {
                                        div.ra {
                                            button.btn.sm type="button" data-rev-act="view" disabled data-ready aria-label={"View revision " (r.id)} { "View" }
                                            button.btn.sm type="button" data-rev-act="diff" disabled data-ready aria-label={"Diff revision " (r.id) " with the current file"} { "Diff" }
                                            button.btn.sm type="button" data-rev-act="restore" disabled data-ready aria-label={"Restore revision " (r.id)} { (ic("refresh")) "Restore…" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                @if pages > 1 {
                    nav.rev-pager aria-label="Revision pages" {
                        @if page > 1 {
                            a.btn.sm href={"?rp=" (page - 1)} data-rp=(page - 1) rel="prev" { span aria-hidden="true" { "←" } "Newer" }
                        } @else {
                            span.btn.sm aria-disabled="true" { span aria-hidden="true" { "←" } "Newer" }
                        }
                        span.muted aria-live="polite" { "Revisions " (first + 1) "–" (first + list.len()) " of " (total) }
                        @if page < pages {
                            a.btn.sm href={"?rp=" (page + 1)} data-rp=(page + 1) rel="next" { "Older" span aria-hidden="true" { "→" } }
                        } @else {
                            span.btn.sm aria-disabled="true" { "Older" span aria-hidden="true" { "→" } }
                        }
                    }
                }
            }
        }
    }
}

/// Revisions per page.
const SHOWN: usize = 25;

/// The page asked for by `?rp=` (1-based, clamped to the pages there are)
/// and the number of pages for `total` revisions.
fn rev_page(query: &str, total: usize) -> (usize, usize) {
    let pages = total.div_ceil(SHOWN).max(1);
    let q = crate::ui::fmt::parse_query(query);
    let want = crate::ui::fmt::query_get(&q, "rp")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1);
    (want.clamp(1, pages), pages)
}

/// The revision viewer: one dialog for View and Diff, filled by the script.
pub fn dialogs() -> Markup {
    html! {
        dialog.dlg.wide #dlg-rev aria-labelledby="dlg-rev-t" data-mode="edit" {
            form method="post" action="/settings/config" novalidate data-hb {
                div.dlg-h {
                    h2 #dlg-rev-t { "Revision" }
                    button.btn.icon.ghost type="button" data-dlg-close aria-label="Close" { (ic("x")) }
                }
                div.dlg-b {
                    div.note.dlg-note hidden tabindex="-1" {}
                    div #dlg-rev-b tabindex="-1" {}
                }
                div.dlg-ft {
                    div.dlg-main {
                        button.btn type="button" data-act="rev-open" hidden { (ic("edit")) "Open in the editor" }
                        span.grow.dlg-msg role="status" {}
                        button.btn type="button" data-dlg-close { "Close" }
                        button.btn.primary type="button" data-act="rev-restore" { (ic("refresh")) "Restore…" }
                    }
                }
            }
        }
    }
}
