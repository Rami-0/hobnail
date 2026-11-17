//! Containers and groups: three server-rendered lists on the dialog
//! pattern (the recipe in `dlg.rs`), signed in only (the caller checks).
//!
//! - **Hidden containers** (`[docker] exclude`): one row per pattern with how
//!   many containers it hides now; add and edit in a dialog with a live
//!   "matches N containers" line, delete with Undo. Each one saves at once
//!   (owner decision Y6: no staged lists).
//! - **Groups** (`[[groups]]`): Name, Title, Icon, Collapsed, Hidden in a
//!   dialog; one-click up / down, one revision per step (Y13). `columns` and
//!   `layout` are no longer offered (§11.12) and are kept as they are in the
//!   file by the patch.
//! - **Containers** (`[containers."<name>"]` overrides): every container
//!   (running or overridden) with search, "Only with overrides", the bulk
//!   bar, and the override dialog ("Edit"), also opened from a container
//!   page (`?edit=override:<name>`, or the older `?name=<name>`).
//!
//! The three lists share one file version, so `?frag=list` serves all three
//! (and the group suggestions) and the script refreshes them together.

use maud::{html, Markup};
use serde_json::{json, Map, Value as J};
use std::collections::BTreeSet;

use super::disk::Disk;
use super::dlg::{self, Field, Spec, F, N};
use crate::ui::fmt::{parse_query, query_get};
use crate::ui::ic;
use crate::ui::shell::Ctx;
use crate::util::glob_match;

const ACTION: &str = "/settings/containers";
const BACK: (&str, &str) = ("Containers and groups", ACTION);

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

// ------------------------------------------------------------ hidden containers

pub const EX_SPEC: Spec = Spec {
    id: "dlg-exclude",
    kind: "exclude",
    prefix: "ex",
    add: "Hide containers",
    edit: "Edit pattern “{}”",
    title_key: "pattern",
    fields: &[Field {
        name: "pattern",
        label: "Pattern",
        hint: "Containers whose name matches are not shown anywhere. * matches any run of characters, ? one character.",
        required: true,
        mono: true,
        wide: true,
        max: 256,
        placeholder: Some("e.g. buildx_* or *-tmp"),
        ..F
    }],
    delete: Some("Delete…"),
    back: BACK,
    list: "ex-list",
    wide: false,
};

/// "hides 2 containers now" for a pattern.
fn hides(pattern: &str, names: &[String]) -> String {
    match names.iter().filter(|n| glob_match(pattern, n)).count() {
        0 => "matches no container now".into(),
        1 => "hides 1 container now".into(),
        n => format!("hides {n} containers now"),
    }
}

fn patterns(disk: &Disk) -> Vec<String> {
    match disk.doc.get("docker").and_then(|d| d.get("exclude")) {
        Some(J::Array(a)) => a
            .iter()
            .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn exclude_list(disk: &Disk, names: &[String]) -> Markup {
    let list = patterns(disk);
    html! {
        div #ex-list data-hash=(disk.hash) {
            @match &disk.cfg {
                Err(e) => (not_loading(e)),
                Ok(_) if list.is_empty() => p.muted.none { "No patterns: every container is shown." },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th scope="col" { "Pattern" } th scope="col" { "Now" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody {
                            @for (i, p) in list.iter().enumerate() {
                                @let key = format!("exclude:{i}");
                                @let href = dlg::edit_link("exclude", &i.to_string());
                                @let data = json!({"pattern": p}).to_string();
                                tr data-key=(key) {
                                    td data-l="Pattern" { a.linkbtn.mono href=(href) data-dlg=(EX_SPEC.id) data-key=(key) data-item=(data) { (p) } }
                                    td data-l="Now" { span.muted { (hides(p, names)) } }
                                    td.act {
                                        div.ra {
                                            a.ia href=(href) data-dlg=(EX_SPEC.id) data-key=(key) data-item=(data) aria-label={"Edit pattern " (p)} data-tip="Edit" { (ic("edit")) }
                                            a.ia href=(href) data-menu="m-ex" role="button" aria-haspopup="menu" aria-expanded="false" aria-label={"More actions for pattern " (p)} data-tip="More" { (ic("more")) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ------------------------------------------------------------ groups

pub const GRP_SPEC: Spec = Spec {
    id: "dlg-group",
    kind: "group",
    prefix: "gr",
    add: "Add group",
    edit: "Edit group “{}”",
    title_key: "name",
    fields: &[],
    delete: Some("Delete…"),
    back: BACK,
    list: "grp-list",
    wide: false,
};

const GRP_FIELDS: &[Field<'static>] = &[
    Field {
        name: "name",
        label: "Name",
        hint: "A compose project, or a group name used by overrides and bookmarks.",
        required: true,
        mono: true,
        max: 80,
        list: Some("grp-names"),
        ..F
    },
    Field {
        name: "title",
        label: "Title",
        hint: "Shown instead of the name. Empty: the name.",
        max: 80,
        ..F
    },
    Field {
        name: "icon",
        label: "Icon",
        hint: "A dashboard-icons name, an http(s) URL or a file in data/icons/.",
        mono: true,
        wide: true,
        ..F
    },
];

/// What the group dialog is filled with: the fields it shows, as text.
fn group_item(t: &Map<String, J>) -> Map<String, J> {
    let s = |k: &str| J::String(dlg::sval(Some(t), k));
    let b = |k: &str| J::Bool(t.get(k).and_then(J::as_bool).unwrap_or(false));
    match json!({"name": s("name"), "title": s("title"), "icon": s("icon"), "collapsed": b("collapsed"), "hide": b("hide")})
    {
        J::Object(m) => m,
        _ => Map::new(),
    }
}

fn group_body(prefix: &str, v: Option<&Map<String, J>>) -> Markup {
    let on = |k: &str| {
        v.and_then(|m| m.get(k))
            .and_then(J::as_bool)
            .unwrap_or(false)
    };
    html! {
        div.grid-2 {
            @for f in GRP_FIELDS { (dlg::field(prefix, f, &dlg::sval(v, f.name), false)) }
            (dlg::switch(prefix, "collapsed", "Collapsed", "Folded on the overview until opened.", on("collapsed"), None, false))
            (dlg::switch(prefix, "hide", "Hidden", "Not shown on the overview.", on("hide"), None, false))
        }
    }
}

fn group_list(disk: &Disk) -> Markup {
    let list = disk.list("groups");
    let n = list.len();
    html! {
        div #grp-list data-hash=(disk.hash) {
            @match &disk.cfg {
                Err(e) => (not_loading(e)),
                Ok(_) if list.is_empty() => p.muted.none { "No groups listed: groups appear in their default order (compose projects, then standalone, then bookmarks)." },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm {
                        thead { tr { th scope="col" { "Group" } th scope="col" { "Title" } th scope="col" { "Shown" } th.act scope="col" { span.sr { "Actions" } } } }
                        tbody {
                            @for (i, t) in list.iter().enumerate() {
                                @let name = dlg::sval(Some(t), "name");
                                @let key = format!("group:{name}");
                                @let href = dlg::edit_link("group", &name);
                                @let data = J::Object(group_item(t)).to_string();
                                @let flag = |k: &str| t.get(k).and_then(J::as_bool) == Some(true);
                                tr data-key=(key) {
                                    td data-l="Group" { a.linkbtn.mono href=(href) data-dlg=(GRP_SPEC.id) data-key=(key) data-item=(data) { (name) } }
                                    td data-l="Title" {
                                        @match t.get("title").and_then(J::as_str).filter(|s| !s.is_empty()) {
                                            Some(s) => span.wrap { (s) },
                                            None => span.muted { "—" },
                                        }
                                    }
                                    td data-l="Shown" {
                                        @if flag("hide") { span.pill.warn { "hidden" } }
                                        @else if flag("collapsed") { span.pill { "collapsed" } }
                                        @else { span.muted { "open" } }
                                    }
                                    td.act {
                                        div.ra {
                                            (move_btn("up", "Move up", "arrow-up", &name, i == 0))
                                            (move_btn("down", "Move down", "arrow-down", &name, i + 1 == n))
                                            a.ia href=(href) data-dlg=(GRP_SPEC.id) data-key=(key) data-item=(data) aria-label={"Edit group " (name)} data-tip="Edit" { (ic("edit")) }
                                            a.ia href=(href) data-menu="m-grp" role="button" aria-haspopup="menu" aria-expanded="false" aria-label={"More actions for group " (name)} data-tip="More" { (ic("more")) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Up / down, rendered disabled; the script enables them (not the first
/// row's up, nor the last row's down).
fn move_btn(dir: &str, label: &str, icon: &str, name: &str, edge: bool) -> Markup {
    html! {
        button.ia type="button" data-move=(dir) aria-label={(label) " " (name)} data-tip=(label)
            disabled data-ready=[(!edge).then_some("")] { (ic(icon)) }
    }
}

// ------------------------------------------------------------ overrides

pub const OV_SPEC: Spec = Spec {
    id: "dlg-override",
    kind: "override",
    prefix: "ov",
    add: "Edit container",
    edit: "Edit “{}”",
    title_key: "_c",
    fields: &[],
    delete: Some("Remove overrides…"),
    back: BACK,
    list: "ov-list",
    wide: false,
};

/// Today's eight fields (QA form inventory; DESIGN.md §11.6).
const OV_TEXT: &[Field<'static>] = &[
    Field {
        name: "name",
        label: "Display name",
        hint: "Empty: the compose service or container name.",
        max: 80,
        ..F
    },
    Field {
        name: "group",
        label: "Group",
        hint: "Move it to another group. Empty: its compose project.",
        mono: true,
        list: Some("grp-names"),
        ..F
    },
    Field {
        name: "icon",
        label: "Icon",
        hint: "A dashboard-icons name (e.g. grafana), an http(s) URL or a file in data/icons/.",
        mono: true,
        wide: true,
        ..F
    },
    Field {
        name: "url",
        label: "URL",
        hint: "Card link. Empty: the first published port.",
        mono: true,
        wide: true,
        v: Some("http"),
        placeholder: Some("https://"),
        inputmode: Some("url"),
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

const OV_ORDER: Field<'static> = Field {
    name: "order",
    label: "Order",
    hint: "Lower first within the group.",
    min: Some("-100000"),
    maxv: Some("100000"),
    ..N
};

/// The dialog's values for one container: every field as the text it
/// shows (`hide` as the choice's value), and `_c`, the container name.
fn ov_item(name: &str, t: Option<&Map<String, J>>) -> Map<String, J> {
    let s = |k: &str| J::String(dlg::sval(t, k));
    let hide = match t.and_then(|m| m.get("hide")).and_then(J::as_bool) {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    };
    match json!({
        "_c": name, "name": s("name"), "group": s("group"), "icon": s("icon"), "url": s("url"),
        "description": s("description"), "hide": hide, "order": s("order"), "size": s("size"),
    }) {
        J::Object(m) => m,
        _ => Map::new(),
    }
}

fn ov_body(prefix: &str, v: Option<&Map<String, J>>) -> Markup {
    let val = |k: &str| dlg::sval(v, k);
    html! {
        div.grid-2 {
            @for f in OV_TEXT { (dlg::field(prefix, f, &val(f.name), false)) }
            (dlg::choice(prefix, "hide", "Hide", "Default: shown, unless a hidden pattern matches.", &[("", "Default", None), ("true", "Hide", None), ("false", "Show", None)], &val("hide"), |_| false))
            (dlg::field(prefix, &OV_ORDER, &val("order"), false))
            (dlg::select(prefix, "size", "Card size", "", &[("", "Default"), ("small", "Small"), ("normal", "Normal"), ("wide", "Wide")], &val("size"), None, false))
        }
    }
}

/// One container of the table (running, or only in the file).
struct Row<'a> {
    name: String,
    known: Option<&'a crate::model::Container>,
    ov: Option<&'a Map<String, J>>,
}

fn ov_rows<'a>(known: &'a [crate::model::Container], ovs: &'a Map<String, J>) -> Vec<Row<'a>> {
    let mut names: BTreeSet<&str> = known.iter().map(|c| c.name.as_str()).collect();
    names.extend(ovs.keys().map(String::as_str));
    names
        .into_iter()
        .map(|n| Row {
            name: n.to_string(),
            known: known.iter().find(|c| c.name == n),
            ov: ovs.get(n).and_then(J::as_object),
        })
        .collect()
}

fn ov_list(ctx: &Ctx, disk: &Disk) -> Markup {
    let snap = ctx.state.snapshot();
    let ovs = match disk.doc.get("containers") {
        Some(J::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let rows = ov_rows(&snap.containers, &ovs);
    let standalone = ctx.cfg.docker.standalone_group.as_str();
    html! {
        div #ov-list data-hash=(disk.hash) data-n=(rows.len()) {
            @match &disk.cfg {
                Err(e) => (not_loading(e)),
                Ok(_) if rows.is_empty() => p.muted.none { "No containers found." },
                Ok(_) => div.tbl-wrap {
                    table.tbl.stack-sm #ov-table {
                        thead {
                            tr {
                                th.ck scope="col" { input #ov-all type="checkbox" aria-label="Select all shown" disabled data-ready; }
                                th scope="col" { "Container" }
                                th scope="col" { "Display name" }
                                th scope="col" { "Group" }
                                th scope="col" { "Icon" }
                                th scope="col" { "Flags" }
                                th.act scope="col" { span.sr { "Actions" } }
                            }
                        }
                        tbody {
                            @for r in &rows {
                                // Logs only for a container whose logs may be read.
                                @let logs = r.known.is_some_and(|c| !crate::ui::view::is_excluded(&ctx.cfg, c));
                                (ov_row(r, standalone, logs))
                            }
                        }
                    }
                },
            }
        }
    }
}

fn ov_row(r: &Row, standalone: &str, logs: bool) -> Markup {
    let key = format!("override:{}", r.name);
    let href = dlg::edit_link("override", &r.name);
    let data = J::Object(ov_item(&r.name, r.ov)).to_string();
    let get = |k: &str| r.ov.and_then(|m| m.get(k));
    let text = |k: &str| get(k).and_then(J::as_str).filter(|s| !s.is_empty());
    let state = r.known.map_or("unknown", |c| c.state.as_str());
    let dot = match state {
        "running" => "ok",
        "unknown" => "off",
        "exited" => "bad",
        _ => "warn",
    };
    let project = r.known.and_then(|c| c.project.as_deref());
    // What the search box matches (lower case).
    let q = [
        Some(r.name.as_str()),
        project,
        r.known.map(|c| c.image.as_str()),
        text("name"),
        text("group"),
    ]
    .iter()
    .flatten()
    .copied()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
    html! {
        tr data-key=(key) data-q=(q) data-ov=[r.ov.is_some().then_some("1")] class=[r.ov.is_some().then_some("has-ov")] {
            td.ck { input type="checkbox" data-sel=(r.name) aria-label={"Select " (r.name)} disabled data-ready; }
            td.ctr-c data-l="Container" {
                span.ctr-n {
                    span class={"dot " (dot)} role="img" aria-label=(state) title=(state) {}
                    a.linkbtn.mono href=(href) data-dlg=(OV_SPEC.id) data-key=(key) data-item=(data) { (r.name) }
                }
                @if let Some(c) = r.known {
                    span.sub.muted { @if let Some(p) = project { (p) " · " } (c.image) }
                }
            }
            td data-l="Display name" { @match text("name") { Some(s) => span.wrap { (s) }, None => span.muted { "—" } } }
            td data-l="Group" {
                @match text("group") {
                    Some(g) => code.wrap { (g) },
                    None => @if r.known.is_some() { span.muted { (project.unwrap_or(standalone)) } } @else { span.muted { "—" } },
                }
            }
            td data-l="Icon" { @match text("icon") { Some(s) => code.trunc title=(s) { (s) }, None => span.muted { "—" } } }
            td.flags data-l="Flags" {
                @match get("hide").and_then(J::as_bool) {
                    Some(true) => span.pill.warn { "hidden" },
                    Some(false) => span.pill { "shown" },
                    None => {},
                }
                @if let Some(o) = get("order") { span.pill { "order " (o) } }
                @if let Some(s) = text("size") { span.pill { (s) } }
                @if text("url").is_some() { span.pill { "link" } }
                @if r.known.is_none() { span.pill.off { "not running" } }
            }
            td.act {
                div.ra {
                    @if logs {
                        a.ia.logs href=(crate::logs::logs_url(&r.name)) title="Logs" aria-label={"Logs of " (r.name)} { (ic("logs")) }
                    }
                    a.btn.sm href=(href) data-dlg=(OV_SPEC.id) data-key=(key) data-item=(data) aria-label={"Edit " (r.name)} { (ic("edit")) "Edit" }
                }
            }
        }
    }
}

fn not_loading(e: &str) -> Markup {
    html! {
        p.note.bad {
            (ic("alert"))
            span { "The config file does not load, so the list cannot be shown: " code.wrap { (e) } " " a href="/settings/config" { "Open the Config file tab" } }
        }
    }
}

/// Container names the patterns are matched against (the live list).
fn names(ctx: &Ctx) -> Vec<String> {
    ctx.state
        .snapshot()
        .containers
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

// ------------------------------------------------------------ the tab

/// `?add=` / `?edit=` of one of the three dialogs: the page form that
/// stands in for it without JavaScript. `?name=<container>` is an older
/// link to an override and means `?edit=override:<container>`.
fn page_form(ctx: &Ctx, disk: &Disk, q: &[(String, String)]) -> Option<Markup> {
    if let Some(e) = dlg::landed(q, EX_SPEC.kind) {
        let list = patterns(disk);
        return Some(match e {
            None => dlg::page(&EX_SPEC, ACTION, None),
            Some(i) => match i.parse::<usize>().ok().and_then(|i| list.get(i)) {
                Some(p) => {
                    let v = json!({"pattern": p});
                    dlg::page(&EX_SPEC, ACTION, v.as_object())
                }
                None => dlg::gone("pattern"),
            },
        });
    }
    if let Some(e) = dlg::landed(q, GRP_SPEC.kind) {
        return Some(match e {
            None => dlg::page_with(&GRP_SPEC, ACTION, None, group_body("grp", None)),
            Some(n) => match disk
                .list("groups")
                .iter()
                .find(|t| t.get("name").and_then(J::as_str) == Some(n))
            {
                Some(t) => {
                    let v = group_item(t);
                    dlg::page_with(&GRP_SPEC, ACTION, Some(&v), group_body("grp", Some(&v)))
                }
                None => dlg::gone("group"),
            },
        });
    }
    let name = dlg::landed(q, OV_SPEC.kind)
        .flatten()
        .or_else(|| query_get(q, "name").filter(|n| !n.is_empty()))?;
    let ovs = disk.doc.get("containers").and_then(J::as_object);
    let ov = ovs.and_then(|m| m.get(name)).and_then(J::as_object);
    let running = ctx
        .state
        .snapshot()
        .containers
        .iter()
        .any(|c| c.name == name);
    Some(if ov.is_some() || running {
        let v = ov_item(name, ov);
        dlg::page_with(&OV_SPEC, ACTION, Some(&v), ov_body("ovp", Some(&v)))
    } else {
        dlg::gone("container")
    })
}

pub fn body(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    let q = parse_query(ctx.query);
    let page = page_form(ctx, &disk, &q);
    let n = names(ctx);
    html! {
        (group_names(ctx))
        @if let Some(p) = page { (p) } @else { (dlg::needs_js(dlg::Values::Shown)) }
        section.panel #ex-sec aria-labelledby="ex-h" {
            div.panel-h {
                h2 #ex-h { "Hidden containers" }
                a.btn.sm.head-act #ex-add href="?add=exclude" data-dlg=(EX_SPEC.id) { (ic("plus")) "Add pattern" }
            }
            p.muted.sf-lead { "Containers whose name matches one of these patterns are not shown anywhere. " code { "*" } " matches any run of characters, " code { "?" } " one character. Each change saves at once." }
            (exclude_list(&disk, &n))
        }
        section.panel #grp-sec aria-labelledby="grp-h" {
            div.panel-h {
                h2 #grp-h { "Groups" }
                a.btn.sm.head-act #grp-add href="?add=group" data-dlg=(GRP_SPEC.id) { (ic("plus")) "Add group" }
            }
            p.muted.sf-lead { "A group is a compose project (or a name used by overrides and bookmarks). Listed groups come first on the overview, in this order. Each change saves at once." }
            (group_list(&disk))
        }
        section.panel #ov-sec aria-labelledby="ov-h" {
            div.panel-h { h2 #ov-h { "Containers" } span.count-pill #ov-count {} }
            div.toolbar.ov-bar {
                div.search-in {
                    (ic("search"))
                    label.sr for="ov-q" { "Search containers" }
                    input #ov-q type="search" placeholder="Search name, project, image" autocomplete="off" spellcheck="false" disabled data-ready;
                }
                label.cb { input #ov-only type="checkbox" disabled data-ready; span { "Only with overrides" } }
            }
            div.bulk #ov-bulk hidden role="region" aria-label="Bulk edit" {
                span.bulk-n #ov-bulk-n role="status" {}
                label.sr for="ov-bulk-group" { "Group for the selected containers" }
                input.in.mono #ov-bulk-group type="text" list="grp-names" placeholder="Group" autocomplete="off";
                button.btn.sm type="button" data-bulk="group" { "Set group" }
                button.btn.sm type="button" data-bulk="hide" { "Hide" }
                button.btn.sm type="button" data-bulk="show" { "Show" }
                button.btn.sm.danger type="button" data-bulk="remove" { "Remove overrides…" }
                button.btn.sm.ghost type="button" data-bulk="none" { "Clear selection" }
            }
            (ov_list(ctx, &disk))
        }
    }
}

/// The dialogs of this tab (rendered outside `#set-main`). The pattern
/// dialog carries the live container names for its "matches N" line
/// (signed in only, like every dialog).
pub fn dialogs(ctx: &Ctx) -> Markup {
    let ex = html! {
        div.grid-2 { (dlg::field(EX_SPEC.prefix, &EX_SPEC.fields[0], "", false)) }
        p.muted.ex-match #ex-match role="status" aria-live="polite" data-names=(json!(names(ctx)).to_string()) {}
    };
    html! {
        (dlg::dialog_with(&EX_SPEC, ACTION, ex, html! {}))
        (dlg::dialog_with(&GRP_SPEC, ACTION, group_body(GRP_SPEC.prefix, None), html! {}))
        (dlg::dialog_with(&OV_SPEC, ACTION, ov_body(OV_SPEC.prefix, None), html! {}))
        (dlg::row_menu("m-ex"))
        (dlg::row_menu("m-grp"))
    }
}

/// `?frag=list`: the three lists and the group suggestions, as the page has
/// them (one file version for all).
pub fn fragment(ctx: &Ctx) -> Markup {
    let disk = Disk::read(ctx.state);
    html! {
        (group_names(ctx))
        (exclude_list(&disk, &names(ctx)))
        (group_list(&disk))
        (ov_list(ctx, &disk))
    }
}
