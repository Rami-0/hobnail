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
    let waiting = enabled && out.is_none();
    html! {
        div id={"w-" (w.id)} class=(class) data-fetched=[out.map(|o| o.fetched_at)] data-wait=[waiting.then_some("")] {
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
                    // A skeleton until the first answer (ROB-14); the size of a value.
                    None => div.w-loading { span.sk {} span.sk-t { "Waiting for the first answer…" } },
                    Some(o) => {
                        @if let Some(e) = error {
                            // A short plain cause; the raw error behind a disclosure.
                            div.err.w-err { (plain_error(e)) details.w-raw { summary { "Details" } code.wrap { (e) } } }
                        } @else {
                            // Built by widgets::present with maud: every
                            // remote value in it is already escaped.
                            (PreEscaped(o.html.as_deref().unwrap_or("")))
                        }
                        div.w-meta.muted title={"updated " (crate::util::fmt_utc(o.fetched_at))} {
                            @if o.fetched_at > 0 { time data-t=(o.fetched_at) { "updated" } } @else { span {} }
                            @if o.duration_ms > 0 { span { (o.duration_ms) " ms" } } @else { span {} }
                        }
                    }
                }
            }
        }
    }
}

/// A failed fetch in a few plain words; the raw message stays in "Details".
pub fn plain_error(e: &str) -> String {
    let l = e.to_ascii_lowercase();
    if l.contains("timed out") || l.contains("timeout") {
        "No answer in time".into()
    } else if l.contains("refused") {
        "Refused: nothing is listening there".into()
    } else if l.contains("lookup")
        || l.contains("resolve")
        || l.contains("dns")
        || l.contains("name or service")
    {
        "The name does not resolve".into()
    } else if let Some(code) = e
        .strip_prefix("HTTP ")
        .and_then(|r| r.get(..3))
        .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
    {
        format!("The service answered HTTP {code}")
    } else if l.contains("certificate") || l.contains("tls") {
        "The secure connection failed".into()
    } else {
        "Could not get an answer".into()
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
        // Waiting: marked for the script, which makes it a busy tile with a
        // skeleton; never busy from the server, so a plain word without
        // script (ROB-14, §11.9).
        assert!(
            html.contains(r#"data-wait="""#)
                && !html.contains("aria-busy")
                && html.contains(r#"class="sk""#),
            "{html}"
        );
        assert!(html.contains("Waiting for the first answer"));
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
        assert!(
            html.contains("HTTP 500 &lt;b&gt;"),
            "the raw error, in Details"
        );
        assert!(
            html.contains("The service answered HTTP 500") && html.contains("<details"),
            "{html}"
        );
        assert!(html.contains("12 ms") && !html.contains("data-wait"));
        assert_eq!(
            plain_error("connect 127.0.0.1:9: Connection refused (os error 111)"),
            "Refused: nothing is listening there"
        );
        assert_eq!(plain_error("timed out after 5000ms"), "No answer in time");
        let zero = WidgetOutput {
            error: Some("timed out after 5000ms".into()),
            ..Default::default()
        };
        assert!(
            !render(&w, Some(&zero)).into_string().contains("0 ms"),
            "no 0 ms timing on a failure"
        );

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
