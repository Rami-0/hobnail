//! The add / edit dialog on the server (DESIGN.md §11.6), the no-JavaScript
//! page form that stands in for it, the confirm dialog and the row menu.
//! The script side is `static/ui.js` (`window.HB`); the styles are the
//! component layer (`static/components.css`).
//!
//! # Making a form a dialog (the pattern; `bookmarks.rs` is the reference)
//!
//! 1. Describe the fields once as a `&[Field]` (start from [`F`]) and the
//!    dialog as a [`Spec`]. The field `name`s are the keys the save handler
//!    reads and the keys a 422 answer's `field` names. A form with choices,
//!    sections or tabs builds its body with [`field`], [`choice`],
//!    [`select`], [`switch`] and [`checks`] (fields of one choice carry
//!    `data-for`) and is rendered with [`dialog_with`] / [`page_with`]
//!    (`notifications.rs`, `widgets.rs`).
//! 2. Render the list on the server from the file on disk ([`super::disk`]),
//!    signed in only. Every trigger is a real link: `href=(edit_link(..))`
//!    (or `?add=<kind>`) with `data-dlg=<spec.id>`, `data-key=<kind>:<id>`
//!    and, for edits, `data-item` = the row's values as JSON (keys = field
//!    names). Put the list in one element with an `id` and `data-hash`, and
//!    serve the same markup at `?frag=list` (see `super::fragment`).
//! 3. Render [`dialog`] once in the tab's `dialogs` (outside `#set-main`),
//!    and [`page`] when the URL asks for `?add=` / `?edit=` ([`landed`]; the
//!    no-JavaScript form; with the script the same URL opens the dialog).
//!    Never put a secret value in a page or in `data-item`: name the
//!    variable or file instead, and let the script read a value the save
//!    must keep from the settings API when it saves.
//! 4. In `static/settings.js` give the dialog its save handler with
//!    `listDialog(...)` and the row actions with `listActions(...)` (see the
//!    bookmarks tab there).
//! 5. The base-hash rule (stated in full above `post` in settings.js): every
//!    write sends the file version the user started from. A dialog takes it
//!    when it opens (the `data-hash` of the list around its trigger, or
//!    `Spec::list`), a confirm or arrow click before anything is awaited.
//!    Nothing adopts a newer hash written elsewhere, so a moved file is
//!    always a 409; after a 409 an edit is only retargeted to an entry found
//!    again with all its original values, otherwise it cannot be saved. The
//!    page's own save is not an outside change: every control based on the
//!    file it started from moves to the file it wrote (`adopt` in
//!    settings.js; not for a whole-file write, nor across custom.css).
//!    An untouched edit dialog sends nothing ("No changes").
//! 6. Show [`needs_js`] with `Values::Shown` (the values are on the page).
//!
//! A setting with several fields that stays on the page (General, Sample
//! interval, custom.css, the raw editor) is a page-section form with
//! `data-hb` and `data-hash` (the file version its values came from), a
//! `.dlg-note` and a body marked `data-hb-body`; `pageForm(...)` in
//! settings.js gives it the same submit path, errors, busy state, 409
//! handling and "No changes" as a dialog. A single control (a switch, the
//! default theme) saves when changed, based on the `data-hash` around it
//! at the moment it is changed.
//!
//! Without JavaScript nothing can be saved: the guards need the `X-RH-CSRF`
//! header, which a plain form cannot send. So every form is `method="post"`
//! (its values never reach a URL), its submit button is rendered `disabled`
//! (`data-ready`: the script enables it) and [`needs_js`] says why.

use maud::{html, Markup};
use serde_json::{Map, Value as J};

use crate::ui::ic;

/// One text field of a dialog form.
#[derive(Clone, Copy)]
pub struct Field<'a> {
    pub name: &'a str,
    pub label: &'a str,
    pub hint: &'a str,
    pub required: bool,
    pub mono: bool,
    /// Spans both columns.
    pub wide: bool,
    /// `maxlength`; 0 is none.
    pub max: u32,
    /// A browser check from `ui.js` (`http`, `duration`, `env`, `id`).
    pub v: Option<&'a str>,
    /// A `<datalist>` id.
    pub list: Option<&'a str>,
    pub placeholder: Option<&'a str>,
    pub inputmode: Option<&'a str>,
    /// `text`, `number` or `textarea`.
    pub ty: &'a str,
    /// `min`, `max` and `step` of a number field.
    pub min: Option<&'a str>,
    pub maxv: Option<&'a str>,
    pub step: Option<&'a str>,
    /// A `pattern` and the message `ui.js` shows when it does not match.
    pub pattern: Option<&'a str>,
    pub msg: Option<&'a str>,
    /// `data-for`: the choice this field belongs to (see `ui.js` byType).
    pub for_: Option<&'a str>,
}

/// The defaults: an optional plain text field.
pub const F: Field<'static> = Field {
    name: "",
    label: "",
    hint: "",
    required: false,
    mono: false,
    wide: false,
    max: 0,
    v: None,
    list: None,
    placeholder: None,
    inputmode: None,
    ty: "text",
    min: None,
    maxv: None,
    step: None,
    pattern: None,
    msg: None,
    for_: None,
};

/// A whole number field (`type=number`, `step=1`).
pub const N: Field<'static> = Field {
    ty: "number",
    mono: true,
    inputmode: Some("numeric"),
    ..F
};

/// A duration such as `30s` (`data-v="duration"`).
pub const D: Field<'static> = Field {
    mono: true,
    v: Some("duration"),
    ..F
};

/// One dialog: what it edits and how it is titled.
pub struct Spec {
    /// The `<dialog>` id, e.g. `dlg-bookmark`.
    pub id: &'static str,
    /// `?add=<kind>`, `?edit=<kind>:<id>`, `data-key="<kind>:<id>"`.
    pub kind: &'static str,
    /// Id prefix of the fields (`<prefix>-<name>`; the page form adds `p`).
    pub prefix: &'static str,
    /// Title and primary button when adding ("Add bookmark").
    pub add: &'static str,
    /// Title when editing; `{}` is replaced by the value of `title_key`.
    pub edit: &'static str,
    pub title_key: &'static str,
    pub fields: &'static [Field<'static>],
    /// Label of the destructive action shown when editing ("Delete…").
    pub delete: Option<&'static str>,
    /// The plain page this dialog belongs to ("Bookmarks", its path).
    pub back: (&'static str, &'static str),
    /// Id of the list element (with `data-hash`) whose file version an add
    /// opened from outside the list is based on.
    pub list: &'static str,
    /// The wide dialog (760 px).
    pub wide: bool,
}

/// What a form shows when the script did not run (for [`needs_js`]).
/// Every Settings form is rendered with its values now; nothing loads them
/// after the page.
#[derive(Clone, Copy, PartialEq)]
pub enum Values {
    /// The server rendered the values.
    Shown,
    /// There is nothing to show (Change password).
    None,
}

/// One field: label, control, error, hint. `value` fills it (the page form);
/// `hidden` hides a `data-for` field that belongs to another choice.
pub fn field(prefix: &str, f: &Field, value: &str, hidden: bool) -> Markup {
    let id = format!("{prefix}-{}", f.name.replace('_', "-"));
    let desc = if f.hint.is_empty() {
        format!("{id}-e")
    } else {
        format!("{id}-e {id}-h")
    };
    let (step, inputmode) = match (f.ty, f.step) {
        ("number", None) => (Some("1"), f.inputmode),
        ("number", Some("any")) => (Some("any"), Some("decimal")),
        (_, s) => (s, f.inputmode),
    };
    html! {
        div.fld.wide[f.wide] data-for=[f.for_] hidden[hidden] {
            label for=(id) {
                (f.label)
                @if f.required { " " abbr.req title="required" { "*" } }
            }
            @if f.ty == "textarea" {
                textarea.in.mono[f.mono] id=(id) name=(f.name) rows="3"
                    required[f.required] maxlength=[(f.max > 0).then_some(f.max)]
                    placeholder=[f.placeholder] autocomplete="off" spellcheck="false"
                    aria-describedby=(desc) { (value) }
            } @else {
                input.in.mono[f.mono] type=(f.ty) id=(id) name=(f.name) value=(value)
                    required[f.required] maxlength=[(f.max > 0).then_some(f.max)]
                    min=[f.min] max=[f.maxv] step=[step] pattern=[f.pattern] data-msg=[f.msg]
                    data-v=[f.v] list=[f.list] placeholder=[f.placeholder] inputmode=[inputmode]
                    autocomplete="off" spellcheck="false" aria-describedby=(desc);
            }
            p.fld-err id={(id) "-e"} hidden {}
            @if !f.hint.is_empty() { p.hint id={(id) "-h"} { (f.hint) } }
        }
    }
}

fn fields(prefix: &str, list: &[Field], values: Option<&Map<String, J>>) -> Markup {
    html! {
        div.grid-2 { @for f in list { (field(prefix, f, &sval(values, f.name), false)) } }
    }
}

/// A value of `values` as the text a field shows: strings as they are,
/// numbers and booleans written out, a list joined with ", "; "" when unset.
pub fn sval(values: Option<&Map<String, J>>, k: &str) -> String {
    match values.and_then(|m| m.get(k)) {
        Some(J::String(s)) => s.clone(),
        Some(J::Number(n)) => n.to_string(),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Array(a)) => a
            .iter()
            .map(|x| match x {
                J::String(s) => s.clone(),
                o => o.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// A page form (ids `<prefix>p`, see [`page_with`]): nothing in it can be
/// saved, so its choices, switches and selects are disabled too (text
/// stays readable and selectable).
fn on_page(prefix: &str) -> bool {
    prefix.ends_with('p')
}

/// The ids of a choice-like control (`<prefix>-<name>`).
fn cid(prefix: &str, name: &str) -> String {
    format!("{prefix}-{}", name.replace('_', "-"))
}

/// A radio group drawn as a segmented control (two to four options). Each
/// option is `(value, label, data-for)`; `value` is the chosen one.
pub fn choice(
    prefix: &str,
    name: &str,
    label: &str,
    hint: &str,
    opts: &[(&str, &str, Option<&str>)],
    value: &str,
    hidden: impl Fn(Option<&str>) -> bool,
) -> Markup {
    let id = cid(prefix, name);
    html! {
        div.fld {
            span.fld-l id={(id) "-l"} { (label) }
            div.choice role="radiogroup" aria-labelledby={(id) "-l"} aria-describedby=[(!hint.is_empty()).then(|| format!("{id}-h"))] {
                @for (v, l, f) in opts {
                    label data-for=[*f] hidden[hidden(*f)] {
                        input type="radio" name=(name) value=(v) checked[*v == value] disabled[on_page(prefix)];
                        (l)
                    }
                }
            }
            p.fld-err id={(id) "-e"} hidden {}
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
        }
    }
}

/// A select (`opts`: value, label).
#[allow(clippy::too_many_arguments)]
pub fn select(
    prefix: &str,
    name: &str,
    label: &str,
    hint: &str,
    opts: &[(&str, &str)],
    value: &str,
    for_: Option<&str>,
    hidden: bool,
) -> Markup {
    let id = cid(prefix, name);
    html! {
        div.fld data-for=[for_] hidden[hidden] {
            label for=(id) { (label) }
            select.in id=(id) name=(name) disabled[on_page(prefix)] aria-describedby={(id) "-e" @if !hint.is_empty() { " " (id) "-h" }} {
                @for (v, l) in opts { option value=(v) selected[*v == value] { (l) } }
            }
            p.fld-err id={(id) "-e"} hidden {}
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
        }
    }
}

/// An on / off switch (a checkbox with `role=switch`).
pub fn switch(
    prefix: &str,
    name: &str,
    label: &str,
    hint: &str,
    on: bool,
    for_: Option<&str>,
    hidden: bool,
) -> Markup {
    switch_in(prefix, name, label, hint, on, for_, hidden, false)
}

/// A switch of a form that stays on the page (General): disabled until the
/// script is ready (`data-ready`), so without it nothing looks live.
pub fn page_switch(prefix: &str, name: &str, label: &str, hint: &str, on: bool) -> Markup {
    switch_in(prefix, name, label, hint, on, None, false, true)
}

#[allow(clippy::too_many_arguments)]
fn switch_in(
    prefix: &str,
    name: &str,
    label: &str,
    hint: &str,
    on: bool,
    for_: Option<&str>,
    hidden: bool,
    ready: bool,
) -> Markup {
    let id = cid(prefix, name);
    html! {
        div.fld.tg data-for=[for_] hidden[hidden] {
            input.sw type="checkbox" role="switch" id=(id) name=(name) checked[on] disabled[on_page(prefix) || ready] data-ready=[ready.then_some("")]
                aria-describedby={(id) "-e" @if !hint.is_empty() { " " (id) "-h" }};
            label for=(id) { (label) }
            p.fld-err id={(id) "-e"} hidden {}
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
        }
    }
}

/// A group of checkboxes sharing one name (`data-t="set"`: the checked
/// values as a list). `opts`: value, label; `on`: the checked values.
#[allow(clippy::too_many_arguments)]
pub fn checks(
    prefix: &str,
    name: &str,
    label: &str,
    hint: &str,
    opts: &[(String, String)],
    on: &[String],
    for_: Option<&str>,
    hidden: bool,
) -> Markup {
    let id = cid(prefix, name);
    html! {
        fieldset.fld.wide.cset data-for=[for_] hidden[hidden] id=(id) aria-describedby={(id) "-e" @if !hint.is_empty() { " " (id) "-h" }} {
            legend { (label) }
            div.cset-opts {
                @for (v, l) in opts {
                    label.cb { input type="checkbox" name=(name) value=(v) data-t="set" checked[on.contains(v)] disabled[on_page(prefix)]; span.mono { (l) } }
                }
            }
            p.fld-err id={(id) "-e"} hidden {}
            @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
        }
    }
}

/// "Saving needs JavaScript", in words true for this form. Visible until
/// the component script has run and enabled the save buttons (the root gets
/// `hb-ready`), so a slow or failed `/static/app.js` still shows it.
pub fn needs_js(v: Values) -> Markup {
    let rest = match v {
        Values::Shown => " You can read the values here.",
        Values::None => "",
    };
    html! {
        p.note.warn.needs-js {
            (ic("lock"))
            span {
                strong { "Saving needs JavaScript." }
                " Every change is sent with a security header that a plain form cannot carry."
                (rest)
            }
        }
    }
}

/// The dialog (closed). `action` is a safe target for the `post` form: the
/// write guards refuse a plain form post, so it never does anything.
pub fn dialog(s: &Spec, action: &str) -> Markup {
    dialog_with(s, action, fields(s.prefix, s.fields, None), html! {})
}

/// A dialog whose body is built by the caller (fields of several kinds,
/// sections, tabs). `extra` are secondary footer buttons, placed between
/// Cancel and the primary: a button that saves too is `type="button"
/// data-submit="<action>"`, so Enter still runs the primary only.
pub fn dialog_with(s: &Spec, action: &str, body: Markup, extra: Markup) -> Markup {
    html! {
        dialog.dlg.wide[s.wide] id=(s.id) aria-labelledby={(s.id) "-t"} data-mode="add" data-list=(s.list) {
            form method="post" action=(action) novalidate data-hb {
                div.dlg-h {
                    h2 id={(s.id) "-t"} data-add=(s.add) data-edit=(s.edit) data-key=(s.title_key) { (s.add) }
                    button.btn.icon.ghost type="button" data-dlg-close aria-label="Close" { (ic("x")) }
                }
                div.dlg-b {
                    div.note.dlg-note hidden tabindex="-1" {}
                    (body)
                }
                div.dlg-ft {
                    div.dlg-main {
                        @if let Some(d) = s.delete {
                            button.btn.ghost.danger.edit-only type="button" data-act="dlg-delete" { (d) }
                        }
                        span.grow.dlg-msg role="status" {}
                        button.btn type="button" data-dlg-close { "Cancel" }
                        (extra)
                        button.btn.primary type="submit" disabled data-ready data-add=(s.add) data-edit="Save changes" {
                            (ic("check")) (s.add)
                        }
                    }
                    div.dlg-guard hidden {
                        span { "Discard your changes?" }
                        button.btn type="button" data-keep { "Keep editing" }
                        button.btn.danger type="button" data-discard { "Discard" }
                    }
                }
            }
        }
    }
}

/// The same form as a page section, for `?add=` / `?edit=` without
/// JavaScript (hidden when the script runs, which opens the dialog
/// instead; it stays until the script is ready). `values` fills it; `None`
/// is an empty add form.
pub fn page(s: &Spec, action: &str, values: Option<&Map<String, J>>) -> Markup {
    let pid = format!("{}p", s.prefix);
    page_with(s, action, values, fields(&pid, s.fields, values))
}

/// [`page`] with a body built by the caller (with the ids prefix
/// `<prefix>p`, so they never clash with the dialog's).
pub fn page_with(s: &Spec, action: &str, values: Option<&Map<String, J>>, body: Markup) -> Markup {
    let edit = values.is_some();
    let title = if edit {
        s.edit.replace("{}", &sval(values, s.title_key))
    } else {
        s.add.to_string()
    };
    let pid = format!("{}p", s.prefix);
    html! {
        section.dlg-page.needs-js.wide[s.wide] aria-labelledby={(pid) "-t"} {
            form method="post" action=(action) novalidate {
                div.dlg-h { h2 id={(pid) "-t"} { (title) } }
                div.dlg-b {
                    (needs_js(Values::Shown))
                    (body)
                }
                div.dlg-ft {
                    @if let (true, Some(d)) = (edit, s.delete) {
                        button.btn.ghost.danger type="button" disabled { (d) }
                    }
                    span.grow {}
                    a.btn href=(s.back.1) { "Back to " (s.back.0) }
                    button.btn.primary type="submit" disabled {
                        (ic("check")) (if edit { "Save changes" } else { s.add })
                    }
                }
            }
        }
    }
}

/// "That … does not exist (any more)" for an `?edit=` whose entry is gone.
pub fn gone(noun: &str) -> Markup {
    html! {
        p.note.warn { (ic("alert")) span { "That " (noun) " does not exist (any more). The list below is the current file." } }
    }
}

/// `?add=<kind>` / `?edit=<kind>:<id>` of this request: `Some(None)` for an
/// add, `Some(Some(id))` for an edit (`edit` wins, as in `ui.js`).
pub fn landed<'a>(q: &'a [(String, String)], kind: &str) -> Option<Option<&'a str>> {
    let edit = crate::ui::fmt::query_get(q, "edit")
        .and_then(|e| e.strip_prefix(kind))
        .and_then(|e| e.strip_prefix(':'));
    match edit {
        Some(id) => Some(Some(id)),
        None => (crate::ui::fmt::query_get(q, "add") == Some(kind)).then_some(None),
    }
}

/// The `href` of an edit link, the id percent-encoded.
pub fn edit_link(kind: &str, id: &str) -> String {
    format!("?edit={kind}:{}", crate::ui::fmt::url_encode(id))
}

/// `HB.confirm`'s dialog: one per page (signed in only).
pub fn confirm() -> Markup {
    html! {
        dialog.dlg.confirm #dlg-confirm aria-labelledby="dlg-cf-t" aria-describedby="dlg-cf-b" {
            div.dlg-h { h2 #dlg-cf-t { (ic("alert")) span { "Delete?" } } }
            div.dlg-b {
                p #dlg-cf-b data-body {}
                div.fld data-typed hidden {
                    label for="cf-typed" { "Type " b {} " to confirm" }
                    input.in #cf-typed type="text" autocomplete="off" spellcheck="false";
                }
            }
            div.dlg-ft {
                span.grow {}
                button.btn type="button" data-cancel { "Cancel" }
                button.btn.primary.danger type="button" data-ok { "Delete" }
            }
        }
    }
}

/// The row ⋯ menu of a list (one per page; `HB.menuFor` is the trigger).
/// Without the script each trigger is a link to the row's edit page.
pub fn row_menu(id: &str) -> Markup {
    html! {
        div.menu id=(id) popover role="menu" aria-label="Row actions" {
            button.mi role="menuitem" type="button" data-row-act="edit" { (ic("edit")) span { "Edit" } }
            hr;
            button.mi.danger role="menuitem" type="button" data-row-act="delete" { (ic("x")) span { "Delete…" } }
        }
    }
}
