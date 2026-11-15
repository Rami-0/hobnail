//! One custom widget tile.

use maud::{html, Markup, PreEscaped};
use serde_json::Value;

use crate::widgets::{safe_link, WidgetConfig, WidgetOutput, DISABLED};

/// Must return the element `<div id="w-{id}" class="tile widget">…</div>`.
pub fn render(w: &WidgetConfig, out: Option<&WidgetOutput>) -> Markup {
    let title = w.title.as_deref().unwrap_or(&w.id);
    let enabled = w.is_enabled();
    let out = out.filter(|_| enabled);
    let error = out.and_then(|o| o.error.as_deref());
    let class = match (enabled, error) {
        (false, _) => "tile widget is-off",
        (true, Some(_)) => "tile widget is-err",
        (true, None) => "tile widget",
    };
    let dot = w.status.as_ref().map(|_| match out {
        None => ("dot", "waiting"),
        Some(o) if o.error.is_some() => ("dot bad", "error"),
        Some(o) => match o.values.get("_ok") {
            Some(Value::Bool(true)) => ("dot ok", "ok"),
            _ => ("dot bad", "not ok"),
        },
    });
    let link = w.link.as_deref().filter(|l| safe_link(l));
    html! {
        div id={"w-" (w.id)} class=(class) data-fetched=[out.map(|o| o.fetched_at)] {
            div.tile-title {
                @if w.icon.is_some() { (crate::ui::icon(w.icon.as_deref(), title)) }
                @if let Some(l) = link {
                    a.w-title href=(l) target="_blank" rel="noopener noreferrer" { (title) }
                } @else {
                    span.w-title { (title) }
                }
                @if let Some((c, t)) = dot { span class=(c) title=(t) {} }
            }
            @if !enabled {
                div.muted.w-off { (DISABLED) }
            } @else {
                @match out {
                    None => div.muted.w-loading { "loading…" },
                    Some(o) => {
                        @if let Some(e) = error {
                            div.err.w-err title=(e) { (e) }
                        } @else {
                            // Built by widgets::present with maud: every
                            // remote value in it is already escaped.
                            (PreEscaped(o.html.as_deref().unwrap_or("")))
                        }
                        div.w-meta.muted title={"updated " (crate::util::fmt_utc(o.fetched_at))} {
                            @if o.fetched_at > 0 { time data-t=(o.fetched_at) { "updated" } } @else { span {} }
                            span { (o.duration_ms) " ms" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(extra: &str) -> WidgetConfig {
        let text = format!("[[widgets]]\nid = \"x\"\ntitle = \"<X>\"\n{extra}\n");
        crate::config::parse(&text, "t.toml")
            .unwrap()
            .widgets
            .remove(0)
    }

    #[test]
    fn states() {
        let w = cfg("type = \"custom-api\"\nurl = \"http://h/\"\nlink = \"http://h/\"\nfields = { s = \"status\" }\nstatus = { field = \"s\", ok = [\"pass\"] }");
        let html = render(&w, None).into_string();
        assert!(html.contains(r#"id="w-x""#), "{html}");
        assert!(html.contains(r#"class="tile widget""#), "{html}");
        assert!(html.contains("loading…"));
        assert!(html.contains("&lt;X&gt;"), "title is escaped: {html}");
        assert!(html.contains(r#"href="http://h/""#));
        assert!(html.contains(r#"class="dot""#));

        let err = WidgetOutput {
            error: Some("HTTP 500 <b>".into()),
            duration_ms: 12,
            ..Default::default()
        };
        let html = render(&w, Some(&err)).into_string();
        assert!(
            html.contains("is-err") && html.contains("dot bad"),
            "{html}"
        );
        assert!(html.contains("HTTP 500 &lt;b&gt;"));
        assert!(html.contains("12 ms"));

        let mut ok = WidgetOutput {
            html: Some("<div class=\"tile-value w-value\">PASS</div>".into()),
            ..Default::default()
        };
        ok.values.insert("_ok".into(), Value::Bool(true));
        let html = render(&w, Some(&ok)).into_string();
        assert!(html.contains("dot ok") && html.contains(">PASS<"), "{html}");
    }

    #[test]
    fn disabled_command() {
        let w = cfg("type = \"command\"\ncommand = [\"true\"]");
        let html = render(&w, Some(&WidgetOutput::default())).into_string();
        assert!(html.contains("is-off") && html.contains(DISABLED), "{html}");
    }
}
