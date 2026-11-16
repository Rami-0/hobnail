//! General: every `[page]` key. The script shows the effective value or the
//! default as each placeholder and sends only what changed (an emptied
//! field removes the key, so the default applies).

use maud::{html, Markup};

use super::form::{foot, Form, In, NUM, TEXT};

pub fn body() -> Markup {
    let f = Form("gen");
    html! {
        section.panel aria-labelledby="gen-h" {
            div.panel-h { h2 #gen-h { "Page" } }
            form.sf #f-general data-form="general" method="post" action="/settings/general" novalidate {
                div.sf-grid {
                    (f.text("title", "Title", "Shown in the sidebar and the browser tab. Empty: the host name."))
                    (f.text("greeting", "Greeting", "Line under the title on the overview. Empty: a time-of-day greeting."))
                    (f.input("columns", "Columns", "Cards per row in a group, 1 to 6.", NUM))
                    (f.select("layout", "Layout", "Cards or compact rows.", &[("", "Default"), ("grid", "Grid (cards)"), ("list", "List (rows)")], None))
                    (f.select("theme", "Theme", "Each browser's theme button overrides this for that browser.", &[("", "Default"), ("auto", "Auto (follow the OS)"), ("dark", "Dark"), ("light", "Light"), ("ember", "Ember (warm dark)"), ("contrast", "High contrast")], None))
                    (f.input("search_url", "Search URL", "Used by the command palette for web searches; {q} is replaced by the query.", In { mono: true, wide: true, ..TEXT }))
                    (f.toggle("show_stopped", "Show stopped containers", "Off: only running containers appear on the overview.", None))
                    (f.toggle("show_stats", "Show live CPU and memory on cards", "", None))
                }
                (foot("Save", html! {}))
            }
        }
    }
}
