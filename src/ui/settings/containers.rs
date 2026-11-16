//! Containers and groups: `[docker] exclude`, `[[groups]]` (order, title,
//! icon, collapsed, hide) and the per-container overrides table with search,
//! an inline editor and bulk actions.

use maud::{html, Markup};
use std::collections::BTreeSet;

use super::form::{foot, Form, In, MONO, NUM, TEXT};
use crate::ui::fmt::{parse_query, query_get};
use crate::ui::ic;
use crate::ui::shell::Ctx;

/// Group names the inputs can suggest: configured groups, compose projects
/// and the standalone group.
pub fn group_names(ctx: &Ctx) -> Markup {
    let snap = ctx.state.snapshot();
    let mut names: BTreeSet<String> = ctx.cfg.groups.iter().map(|g| g.name.clone()).collect();
    names.extend(snap.containers.iter().filter_map(|c| c.project.clone()));
    names.insert(ctx.cfg.docker.standalone_group.clone());
    names.extend(ctx.cfg.bookmarks.iter().filter_map(|b| b.group.clone()));
    html! {
        datalist #grp-names { @for n in &names { option value=(n) {} } }
    }
}

pub fn body(ctx: &Ctx) -> Markup {
    let q = parse_query(ctx.query);
    let focus = query_get(&q, "name").unwrap_or("");
    let ov = Form("ov");
    html! {
        (group_names(ctx))
        section.panel aria-labelledby="ex-h" {
            div.panel-h { h2 #ex-h { "Hidden containers" } }
            form.sf #f-exclude data-form="exclude" method="post" action="/settings/containers" novalidate {
                p.muted.sf-lead { "Containers whose name matches one of these patterns are not shown anywhere. " code { "*" } " matches any run of characters, " code { "?" } " one character." }
                ul.chips #ex-list aria-label="Exclude patterns" {}
                div.add-line {
                    label.sr for="ex-new" { "New pattern" }
                    input.in.mono #ex-new type="text" placeholder="e.g. buildx_* or *-tmp" autocomplete="off" spellcheck="false";
                    button.btn.sm type="button" data-act="ex-add" { (ic("plus")) "Add" }
                }
                (foot("Save", html! {}))
            }
        }
        section.panel aria-labelledby="grp-h" {
            div.panel-h { h2 #grp-h { "Groups" } }
            form.sf #f-groups data-form="groups" method="post" action="/settings/containers" novalidate {
                p.muted.sf-lead { "A group is a compose project (or a name used by overrides and bookmarks). Listed groups come first on the overview, in this order." }
                div.rows #grp-list { (super::form::loading()) }
                div.add-line {
                    label.sr for="grp-new" { "New group name" }
                    input.in.mono #grp-new type="text" list="grp-names" placeholder="Group name (e.g. a compose project)" autocomplete="off" spellcheck="false";
                    button.btn.sm type="button" data-act="grp-add" { (ic("plus")) "Add group" }
                }
                (foot("Save groups", html! {}))
            }
        }
        section.panel #ov-sec aria-labelledby="ov-h" data-focus=(focus) {
            div.panel-h { h2 #ov-h { "Containers" } span.count-pill #ov-count {} }
            div.sec-note hidden {}
            div.toolbar.ov-bar {
                div.search-in {
                    (ic("search"))
                    label.sr for="ov-q" { "Search containers" }
                    input #ov-q type="search" placeholder="Search name, project, image" autocomplete="off" spellcheck="false" value=(focus);
                }
                label.cb { input #ov-only type="checkbox"; span { "Only with overrides" } }
            }
            div.bulk #ov-bulk hidden role="region" aria-label="Bulk edit" {
                span.bulk-n #ov-bulk-n {}
                label.sr for="ov-bulk-group" { "Group for the selected containers" }
                input.in.mono #ov-bulk-group type="text" list="grp-names" placeholder="Group" autocomplete="off";
                button.btn.sm type="button" data-act="bulk-group" { "Set group" }
                button.btn.sm type="button" data-act="bulk-hide" { "Hide" }
                button.btn.sm type="button" data-act="bulk-show" { "Show" }
                button.btn.sm type="button" data-act="bulk-clear" { "Remove overrides" }
                button.btn.sm.ghost type="button" data-act="bulk-none" { "Clear selection" }
            }
            div.tbl-wrap {
                table.tbl.stack-sm #ov-table {
                    thead {
                        tr {
                            th.ck { input #ov-all type="checkbox" aria-label="Select all shown"; }
                            th { "Container" }
                            th { "Display name" }
                            th { "Group" }
                            th { "Icon" }
                            th { "URL" }
                            th { "Flags" }
                            th.act { span.sr { "Actions" } }
                        }
                    }
                    tbody #ov-body { tr { td colspan="8" { (super::form::loading()) } } }
                }
            }
            // The inline editor; the script moves it under the edited row.
            form.sf.ov-edit #f-override data-form="override" hidden method="post" action="/settings/containers" novalidate {
                h3.ov-edit-h { "Override for " code #ov-edit-name {} }
                div.sf-grid {
                    (ov.text("name", "Display name", "Empty: the compose service or container name."))
                    (ov.input("group", "Group", "Move it to another group.", In { list: Some("grp-names"), ..MONO }))
                    (ov.input("icon", "Icon", "A dashboard-icons name (e.g. grafana), an http(s) URL or a file in data/icons/.", MONO))
                    (ov.input("url", "URL", "Card link. Empty: the first published port.", In { mono: true, ..TEXT }))
                    (ov.input("description", "Description", "", In { wide: true, ..TEXT }))
                    (ov.tri("hide", "Hide", "", "shown"))
                    (ov.input("order", "Order", "Lower first within the group.", NUM))
                    (ov.select("size", "Card size", "", &[("", "Default"), ("small", "Small"), ("normal", "Normal"), ("wide", "Wide")], None))
                }
                (foot("Save override", html! {
                    button.btn type="button" data-act="ov-cancel" { "Cancel" }
                    button.btn.danger type="button" data-act="ov-remove" { (ic("x")) "Remove override" }
                }))
            }
        }
    }
}
