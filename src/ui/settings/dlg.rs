//! The add / edit dialog on the server (DESIGN.md §11.6), the no-JavaScript
//! page form that stands in for it, the confirm dialog and the row menu.
//! The script side is `static/ui.js` (`window.HB`); the styles are the
//! component layer (`static/components.css`).
//!
//! # Making a form a dialog (the pattern; `bookmarks.rs` is the reference)
//!
//! 1. Describe the fields once as a `&[Field]` (start from [`F`]) and the
//!    dialog as a [`Spec`]. The field `name`s are the keys the save handler
//!    reads and the keys a 422 answer's `field` names.
//! 2. Render the list on the server from the file on disk ([`super::disk`]),
//!    signed in only. Every trigger is a real link: `href=(edit_href(..))`
//!    (or `?add=<kind>`) with `data-dlg=<spec.id>`, `data-key=<kind>:<id>`
//!    and, for edits, `data-item` = the row's values as JSON (keys = field
//!    names). Put the list in one element with an `id` and `data-hash`, and
//!    serve the same markup at `?frag=list` (see `super::fragment`).
//! 3. Render [`dialog`] once in the tab's `dialogs` (outside `#set-main`),
//!    and [`page`] when the URL asks for `?add=` / `?edit=` (the
//!    no-JavaScript form; with the script the same URL opens the dialog).
//! 4. In `static/settings.js` give the dialog its save handler with
//!    `listDialog(...)` and the row actions with `listActions(...)` (see the
//!    bookmarks tab there).
//! 5. The base-hash rule (stated in full above `post` in settings.js): every
//!    write sends the file version the user started from. A dialog takes it
//!    when it opens (the `data-hash` of the list around its trigger, or
//!    `Spec::list`), a confirm or arrow click before anything is awaited.
//!    Nothing adopts a newer hash for it, so a moved file is always a 409;
//!    after a 409 an edit is only retargeted to an entry found again with all
//!    its original values, otherwise it cannot be saved.
//! 6. Flip the form's [`needs_js`] to `Values::Shown` once its values are
//!    rendered by the server.
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
pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub required: bool,
    pub mono: bool,
    /// Spans both columns.
    pub wide: bool,
    /// `maxlength`; 0 is none.
    pub max: u32,
    /// A browser check from `ui.js` (`http`, `duration`, `env`, `id`).
    pub v: Option<&'static str>,
    /// A `<datalist>` id.
    pub list: Option<&'static str>,
    pub placeholder: Option<&'static str>,
    pub inputmode: Option<&'static str>,
}

/// The defaults: an optional plain text field.
pub const F: Field = Field {
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
    pub fields: &'static [Field],
    /// Label of the destructive action shown when editing ("Delete…").
    pub delete: Option<&'static str>,
    /// The plain page this dialog belongs to ("Bookmarks", its path).
    pub back: (&'static str, &'static str),
    /// Id of the list element (with `data-hash`) whose file version an add
    /// opened from outside the list is based on.
    pub list: &'static str,
}

/// What a form shows when the script did not run (for [`needs_js`]). The
/// next forms switch from `LoadWithJs` to `Shown` as they are converted.
#[derive(Clone, Copy, PartialEq)]
pub enum Values {
    /// The server rendered the values (converted forms).
    Shown,
    /// The values load with the script (forms not converted yet).
    LoadWithJs,
    /// There is nothing to show (Change password).
    None,
}

/// `?edit=<kind>:<id>`.
pub fn edit_href(kind: &str, id: &str) -> String {
    format!("?edit={kind}:{id}")
}

fn field(prefix: &str, f: &Field, value: &str) -> Markup {
    let id = format!("{prefix}-{}", f.name);
    let desc = if f.hint.is_empty() {
        format!("{id}-e")
    } else {
        format!("{id}-e {id}-h")
    };
    html! {
        div.fld.wide[f.wide] {
            label for=(id) {
                (f.label)
                @if f.required { " " abbr.req title="required" { "*" } }
            }
            input.in.mono[f.mono] type="text" id=(id) name=(f.name) value=(value)
                required[f.required] maxlength=[(f.max > 0).then_some(f.max)]
                data-v=[f.v] list=[f.list] placeholder=[f.placeholder] inputmode=[f.inputmode]
                autocomplete="off" spellcheck="false" aria-describedby=(desc);
            p.fld-err id={(id) "-e"} hidden {}
            @if !f.hint.is_empty() { p.hint id={(id) "-h"} { (f.hint) } }
        }
    }
}

fn fields(prefix: &str, list: &[Field], values: Option<&Map<String, J>>) -> Markup {
    let val = |k: &str| {
        values
            .and_then(|m| m.get(k))
            .and_then(J::as_str)
            .unwrap_or("")
            .to_string()
    };
    html! {
        div.grid-2 { @for f in list { (field(prefix, f, &val(f.name))) } }
    }
}

/// "Saving needs JavaScript", in words true for this form. Visible until
/// the component script has run and enabled the save buttons (the root gets
/// `hb-ready`), so a slow or failed `/static/app.js` still shows it.
pub fn needs_js(v: Values) -> Markup {
    let rest = match v {
        Values::Shown => " You can read the values here.",
        Values::LoadWithJs => " The current values load with the script too.",
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
    html! {
        dialog.dlg id=(s.id) aria-labelledby={(s.id) "-t"} data-mode="add" data-list=(s.list) {
            form method="post" action=(action) novalidate data-hb {
                div.dlg-h {
                    h2 id={(s.id) "-t"} data-add=(s.add) data-edit=(s.edit) data-key=(s.title_key) { (s.add) }
                    button.btn.icon.ghost type="button" data-dlg-close aria-label="Close" { (ic("x")) }
                }
                div.dlg-b {
                    div.note.dlg-note hidden tabindex="-1" {}
                    (fields(s.prefix, s.fields, None))
                }
                div.dlg-ft {
                    div.dlg-main {
                        @if let Some(d) = s.delete {
                            button.btn.ghost.danger.edit-only type="button" data-act="dlg-delete" { (d) }
                        }
                        span.grow.dlg-msg role="status" {}
                        button.btn type="button" data-dlg-close { "Cancel" }
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
    let edit = values.is_some();
    let title = if edit {
        let k = values
            .and_then(|m| m.get(s.title_key))
            .and_then(J::as_str)
            .unwrap_or("");
        s.edit.replace("{}", k)
    } else {
        s.add.to_string()
    };
    let pid = format!("{}p", s.prefix);
    html! {
        section.dlg-page.needs-js aria-labelledby={(pid) "-t"} {
            form method="post" action=(action) novalidate {
                div.dlg-h { h2 id={(pid) "-t"} { (title) } }
                div.dlg-b {
                    (needs_js(Values::Shown))
                    (fields(&pid, s.fields, values))
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
