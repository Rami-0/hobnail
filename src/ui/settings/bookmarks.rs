//! Bookmarks: `[[bookmarks]]` list with add, edit, delete and up/down.

use maud::{html, Markup};

use super::form::{foot, head, loading, Form, In, MONO, TEXT};
use crate::ui::shell::Ctx;

pub fn body(ctx: &Ctx) -> Markup {
    let f = Form("bm");
    html! {
        (super::containers::group_names(ctx))
        section.panel aria-labelledby="bm-h" {
            (head("Bookmarks", Some(("bm-new", "plus", "Add bookmark"))))
            div.sec-note hidden {}
            div.tbl-wrap {
                table.tbl.stack-sm {
                    thead { tr { th { "Name" } th { "URL" } th { "Group" } th.act { span.sr { "Actions" } } } }
                    tbody #bm-list { tr { td colspan="4" { (loading()) } } }
                }
            }
            form.sf.editor #f-bookmark data-form="bookmark" hidden novalidate {
                h3.ed-h #bm-ed-h { "Add bookmark" }
                div.sf-grid {
                    (f.text("name", "Name", ""))
                    (f.input("url", "URL", "http(s) link.", In { mono: true, ..TEXT }))
                    (f.input("group", "Group", "Empty: the Bookmarks group.", In { list: Some("grp-names"), ..MONO }))
                    (f.input("icon", "Icon", "A dashboard-icons name, an http(s) URL or a file in data/icons/.", MONO))
                    (f.input("description", "Description", "", In { wide: true, ..TEXT }))
                }
                (foot("Save bookmark", html! { button.btn type="button" data-act="ed-cancel" { "Cancel" } }))
            }
        }
    }
}
