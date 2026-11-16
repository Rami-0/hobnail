//! Bookmarks: the `[[bookmarks]]` list, rendered on the server, with add
//! and edit in the dialog, delete (confirm, then Undo) and one-click up /
//! down (one revision per step). The reference for every list form: see
//! the recipe in `dlg.rs`. Signed in only (the caller checks).

use maud::{html, Markup};
use serde_json::{json, Map, Value as J};

use super::disk::Disk;
use super::dlg::{self, Field, Spec, F};
use crate::config::Bookmark;
use crate::ui::fmt::{parse_query, query_get};
use crate::ui::ic;
use crate::ui::shell::Ctx;

const ACTION: &str = "/settings/bookmarks";

/// Today's fields, in the order of DESIGN.md §11.6.
const FIELDS: &[Field] = &[
    Field {
        name: "name",
        label: "Name",
        required: true,
        max: 80,
        ..F
    },
    Field {
        name: "group",
        label: "Group",
        hint: "Empty: the Bookmarks group.",
        list: Some("grp-names"),
        placeholder: Some("Bookmarks"),
        ..F
    },
    Field {
        name: "url",
        label: "URL",
        hint: "An http:// or https:// link.",
        required: true,
        mono: true,
        wide: true,
        v: Some("http"),
        placeholder: Some("https://"),
        inputmode: Some("url"),
        ..F
    },
    Field {
        name: "icon",
        label: "Icon",
        hint: "A dashboard-icons name, an http(s) URL or a file in data/icons/. Empty: the first letter.",
        mono: true,
        wide: true,
        ..F
    },
    Field {
        name: "description",
        label: "Description",
        wide: true,
        max: 200,
        ..F
    },
];

pub const SPEC: Spec = Spec {
    id: "dlg-bookmark",
    kind: "bookmark",
    prefix: "bm",
    add: "Add bookmark",
    edit: "Edit bookmark “{}”",
    title_key: "name",
    fields: FIELDS,
    delete: Some("Delete…"),
    back: ("Bookmarks", ACTION),
    list: "bm-list",
};

/// The values the dialog is filled with (`data-item`): every field, "" for
/// an unset one.
fn item(b: &Bookmark) -> Map<String, J> {
    let s = |o: &Option<String>| o.clone().unwrap_or_default();
    match json!({
        "name": b.name, "url": b.url, "group": s(&b.group),
        "icon": s(&b.icon), "description": s(&b.description),
    }) {
        J::Object(m) => m,
        _ => Map::new(),
    }
}

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let q = parse_query(ctx.query);
    let list = disk.cfg.as_ref().map(|c| c.bookmarks.as_slice()).ok();
    // `?add=bookmark` / `?edit=bookmark:<index>`: the page form (no script).
    // `edit` wins over `add` when a URL carries both (as in ui.js).
    let add = query_get(&q, "add") == Some(SPEC.kind);
    let edit = query_get(&q, "edit").and_then(|e| e.strip_prefix("bookmark:"));
    let landed = match (edit, add) {
        (None, true) => Some(Ok(dlg::page(&SPEC, ACTION, None))),
        (Some(i), _) => Some(
            i.parse::<usize>()
                .ok()
                .and_then(|i| list.and_then(|l| l.get(i)))
                .map(|b| dlg::page(&SPEC, ACTION, Some(&item(b))))
                .ok_or(()),
        ),
        _ => None,
    };
    html! {
        (super::containers::group_names(ctx))
        @match landed {
            Some(Ok(p)) => (p),
            Some(Err(())) => p.note.warn { (ic("alert")) span { "That bookmark does not exist (any more). The list below is the current file." } },
            None => {},
        }
        section.panel #bm-sec aria-label="Bookmarks" {
            (list_markup(&disk))
        }
    }
}

/// The tab's header action (next to the one "Bookmarks" heading).
pub fn head_actions() -> Markup {
    html! {
        a.btn.primary #bm-add href="?add=bookmark" data-dlg=(SPEC.id) { (ic("plus")) "Add bookmark" }
    }
}

/// The dialogs of this tab (rendered outside `#set-main`).
pub fn dialogs() -> Markup {
    html! {
        (dlg::dialog(&SPEC, ACTION))
        (dlg::row_menu("m-bm"))
    }
}

/// The list: also the `?frag=list` answer the script swaps in after a save.
pub fn list_markup(disk: &Disk) -> Markup {
    html! {
        div #bm-list data-hash=(disk.hash) {
            @match &disk.cfg {
                Err(e) => p.note.bad {
                    (ic("alert"))
                    span { "The config file does not load, so the list cannot be shown: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } }
                },
                Ok(cfg) if cfg.bookmarks.is_empty() => div.empty {
                    span.empty-ic { (ic("bookmark")) }
                    h3 { "No bookmarks yet" }
                    p { "Bookmarks are links to things that are not containers, like the router or a NAS." }
                    div.acts { a.btn.primary href="?add=bookmark" data-dlg=(SPEC.id) { (ic("plus")) "Add bookmark" } }
                },
                Ok(cfg) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th { "Name" } th { "URL" } th { "Group" } th.act { span.sr { "Actions" } } } }
                        tbody {
                            @for (i, b) in cfg.bookmarks.iter().enumerate() {
                                (row(i, cfg.bookmarks.len(), b))
                            }
                        }
                    }
                },
            }
        }
    }
}

fn row(i: usize, n: usize, b: &Bookmark) -> Markup {
    let key = format!("{}:{i}", SPEC.kind);
    let href = dlg::edit_href(SPEC.kind, &i.to_string());
    let data = J::Object(item(b)).to_string();
    let name = if b.name.is_empty() {
        "(no name)"
    } else {
        &b.name
    };
    // Up / down are rendered disabled; the script enables them (not the
    // first row's up, nor the last row's down).
    let mv = |dir: &str, label: &str, icon: &str, edge: bool| {
        html! {
            button.ia type="button" data-move=(dir) aria-label={(label) " " (name)} data-tip=(label)
                disabled data-ready=[(!edge).then_some("")] { (ic(icon)) }
        }
    };
    html! {
        tr data-key=(key) {
            td data-l="Name" {
                a.linkbtn href=(href) data-dlg=(SPEC.id) data-key=(key) data-item=(data) { (name) }
                @if let Some(d) = b.description.as_deref().filter(|d| !d.is_empty()) { span.sub.muted { (d) } }
            }
            td.url-c data-l="URL" { code.wrap { (b.url) } }
            td data-l="Group" {
                @match b.group.as_deref().filter(|g| !g.is_empty()) {
                    Some(g) => code { (g) },
                    None => span.muted { "Bookmarks" },
                }
            }
            td.act {
                div.ra {
                    (mv("up", "Move up", "arrow-up", i == 0))
                    (mv("down", "Move down", "arrow-down", i + 1 == n))
                    a.ia href=(href) data-dlg=(SPEC.id) data-key=(key) data-item=(data) aria-label={"Edit " (name)} data-tip="Edit" { (ic("edit")) }
                    a.ia href=(href) data-menu="m-bm" role="button" aria-haspopup="menu" aria-expanded="false"
                        aria-label={"More actions for " (name)} data-tip="More" { (ic("more")) }
                }
            }
        }
    }
}
