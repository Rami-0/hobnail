//! Labelled form controls shared by the tabs. Every control gets an id
//! `<form>-<name>`, a `<label for>`, an optional hint and an error slot
//! (`-h`, `-e`) referenced by `aria-describedby`; the script fills the error
//! slot from a 422 answer's `field`.

use maud::{html, Markup};

use crate::ui::ic;

/// Controls for one form; `.0` is the id prefix.
pub struct Form(pub &'static str);

/// How the script converts a control's value (`data-t`).
#[derive(Clone, Copy)]
pub enum T {
    /// Trimmed string; empty means "not set".
    Str,
    /// A number.
    Num,
}

impl T {
    fn attr(self) -> Option<&'static str> {
        match self {
            T::Str => None,
            T::Num => Some("num"),
        }
    }
}

/// Options for one input.
#[derive(Clone, Copy)]
pub struct In<'a> {
    pub kind: &'a str,
    pub t: T,
    pub mono: bool,
    pub placeholder: Option<&'a str>,
    pub list: Option<&'a str>,
    pub for_: Option<&'a str>,
    pub wide: bool,
}

pub const TEXT: In = In {
    kind: "text",
    t: T::Str,
    mono: false,
    placeholder: None,
    list: None,
    for_: None,
    wide: false,
};
pub const MONO: In = In { mono: true, ..TEXT };
pub const NUM: In = In {
    kind: "number",
    t: T::Num,
    mono: true,
    ..TEXT
};

impl Form {
    pub fn id(&self, name: &str) -> String {
        format!("{}-{}", self.0, name.replace(['.', '_'], "-"))
    }

    fn wrap(
        &self,
        name: &str,
        label: &str,
        hint: &str,
        for_: Option<&str>,
        wide: bool,
        control: Markup,
    ) -> Markup {
        let id = self.id(name);
        html! {
            div.fld.wide[wide] data-for=[for_] {
                label for=(id) { (label) }
                (control)
                @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
                p.fld-err id={(id) "-e"} hidden {}
            }
        }
    }

    fn described(&self, name: &str) -> String {
        let id = self.id(name);
        format!("{id}-h {id}-e")
    }

    pub fn input(&self, name: &str, label: &str, hint: &str, o: In) -> Markup {
        let id = self.id(name);
        let control = html! {
            input.in.mono[o.mono] type=(o.kind) id=(id) name=(name) data-t=[o.t.attr()]
                placeholder=[o.placeholder] list=[o.list] aria-describedby=(self.described(name))
                autocomplete="off" spellcheck="false";
        };
        self.wrap(name, label, hint, o.for_, o.wide, control)
    }

    pub fn text(&self, name: &str, label: &str, hint: &str) -> Markup {
        self.input(name, label, hint, TEXT)
    }

    pub fn select(
        &self,
        name: &str,
        label: &str,
        hint: &str,
        opts: &[(&str, &str)],
        for_: Option<&str>,
    ) -> Markup {
        let id = self.id(name);
        let control = html! {
            select.in id=(id) name=(name) aria-describedby=(self.described(name)) {
                @for (v, l) in opts { option value=(v) { (l) } }
            }
        };
        self.wrap(name, label, hint, for_, false, control)
    }

    /// A yes / no / default select (`data-t="tri"`): "" means not set.
    pub fn tri(&self, name: &str, label: &str, hint: &str, default: &str) -> Markup {
        let id = self.id(name);
        let control = html! {
            select.in id=(id) name=(name) data-t="tri" aria-describedby=(self.described(name)) {
                option value="" { "Default (" (default) ")" }
                option value="true" { "Yes" }
                option value="false" { "No" }
            }
        };
        self.wrap(name, label, hint, None, false, control)
    }

    /// An on/off switch (`data-t="bool"`).
    pub fn toggle(&self, name: &str, label: &str, hint: &str, for_: Option<&str>) -> Markup {
        let id = self.id(name);
        html! {
            div.fld.tg data-for=[for_] {
                input.sw type="checkbox" role="switch" id=(id) name=(name) data-t="bool"
                    aria-describedby=(self.described(name));
                label for=(id) { (label) }
                @if !hint.is_empty() { p.hint id={(id) "-h"} { (hint) } }
                p.fld-err id={(id) "-e"} hidden {}
            }
        }
    }
}

/// Form footer: the submit button (disabled until the script runs, see
/// `dlg.rs`), optional extra buttons, and the status line the script
/// writes to.
pub fn foot(save: &str, extra: Markup) -> Markup {
    html! {
        div.sf-note hidden {}
        (super::dlg::needs_js(super::dlg::Values::LoadWithJs))
        div.sf-foot {
            button.btn.primary type="submit" disabled data-ready { (ic("check")) (save) }
            (extra)
            span.sf-msg role="status" aria-live="polite" {}
        }
    }
}

/// A "loading" placeholder the script replaces.
pub fn loading() -> Markup {
    html! { p.muted.loading { "Loading…" } }
}
