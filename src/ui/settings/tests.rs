//! Settings pages: every tab renders in the shell; signed out they show only
//! the lock panel (no config data); signed in, command widgets are listed
//! read-only with no controls.

use hyper::header::HeaderValue;
use hyper::{HeaderMap, StatusCode};
use std::sync::Arc;

use crate::state::AppState;
use crate::ui::pages::render;
use crate::ui::routes::SETTINGS;
use crate::ui::shell::Ctx;
use crate::ui::testutil::{sample, state_with};

const FIXTURE: &str = include_str!("../../settings/fixtures/config.toml");

fn get(st: &AppState, path: &str, h: &HeaderMap) -> (StatusCode, String) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let ctx = Ctx::new(st, "100.1.2.3", path, query, h);
    render(&ctx)
}

/// A valid session cookie for `st` (stored like `auth` does: its SHA-256).
fn signed_in(st: &Arc<AppState>) -> HeaderMap {
    let token = "ab".repeat(32);
    let hash: String = ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let now = crate::util::unix_now() as i64;
    assert!(
        st.db.create_session(&hash, now, now + 3600),
        "history database needed for sessions"
    );
    let mut h = HeaderMap::new();
    h.insert(
        "cookie",
        HeaderValue::from_str(&format!("rh_session={token}")).unwrap(),
    );
    h
}

fn paths() -> Vec<String> {
    let mut v = vec!["/settings".to_string()];
    v.extend(SETTINGS.iter().map(|s| format!("/settings/{}", s.1)));
    v
}

#[test]
fn signed_out_every_tab_is_locked_and_leaks_nothing() {
    let st = state_with(sample(), FIXTURE);
    let h = HeaderMap::new();
    for p in paths() {
        let (status, html) = get(&st, &p, &h);
        assert_eq!(status, StatusCode::OK, "{p}");
        assert!(html.contains("set-lock"), "{p}: lock panel");
        assert!(
            html.contains("docker exec -it hobnail /hobnail set-password"),
            "{p}: first-password command"
        );
        assert!(html.contains("Docker socket"), "{p}: says why");
        assert!(!html.contains("rh-csrf"), "{p}: no csrf when signed out");
        // Nothing from the config file: no widget URLs, no editors.
        for leak in [
            "100.64.0.10:8090/api/health",
            "/host/proc",
            "broken-demo",
            "f-raw",
            "f-widget",
        ] {
            assert!(!html.contains(leak), "{p}: leaked {leak}");
        }
        assert!(html.contains("/static/settings.css?v="), "{p}");
        assert!(html.contains("/static/settings.js?v="), "{p}");
    }
    // Once a password exists, the panel links to the login with `next`.
    st.db.kv_set("auth.password", "pbkdf2-sha256$1$00$00");
    let (_, html) = get(&st, "/settings/widgets", &h);
    assert!(html.contains("Sign in to change settings"), "{html}");
    assert!(html.contains(r#"href="/login?next=%2Fsettings%2Fwidgets""#));
    assert!(!html.contains("set-password"));
}

#[test]
fn unknown_tab_is_404() {
    let st = state_with(sample(), FIXTURE);
    let (status, _) = get(&st, "/settings/nope", &HeaderMap::new());
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn signed_in_tabs_render_editors() {
    let st = state_with(sample(), FIXTURE);
    if !st.db.is_enabled() {
        eprintln!("skipped: no history database");
        return;
    }
    let h = signed_in(&st);
    for p in paths() {
        let (status, html) = get(&st, &p, &h);
        assert_eq!(status, StatusCode::OK, "{p}");
        assert!(!html.contains("set-lock"), "{p}: no lock panel");
        assert!(html.contains(r#"<meta name="rh-csrf""#), "{p}");
        assert!(html.contains(r#"data-signed="1""#), "{p}");
    }
    let (_, html) = get(&st, "/settings/config", &h);
    assert!(html.contains(r#"id="raw-text""#));
    let (_, html) = get(&st, "/settings/general", &h);
    for f in [
        "title",
        "greeting",
        "columns",
        "layout",
        "theme",
        "search_url",
        "show_stopped",
        "show_stats",
    ] {
        assert!(html.contains(&format!(r#"name="{f}""#)), "general: {f}");
    }
    // Every input, select and textarea has a label (`<label for>`, or an
    // `aria-label` on the control, as the rows of a widget's fields have).
    for p in paths() {
        let (_, page) = get(&st, &p, &h);
        // The settings part of the page (the shell has its own checks).
        let html = &page[page.find(r#"class="page set""#).expect("settings page")
            ..page.find("/static/settings.js").expect("settings script")];
        for tag in ["input", "select", "textarea"] {
            for (at, a) in tags(html, tag) {
                let ty = attr(&a, "type").unwrap_or("text");
                if matches!(ty, "hidden" | "submit" | "button") {
                    continue;
                }
                let labelled = attr(&a, "aria-label").is_some()
                    || attr(&a, "id").is_some_and(|id| html.contains(&format!(r#"for="{id}""#)));
                // Or the <label> around it.
                let wrapped = html[..at].rfind("<label") > html[..at].rfind("</label>");
                assert!(labelled || wrapped, "{p}: no label for {a:?}");
            }
        }
    }
}

#[test]
fn command_widgets_are_read_only_with_no_controls() {
    let st = state_with(sample(), FIXTURE);
    if !st.db.is_enabled() {
        return;
    }
    let h = signed_in(&st);
    let (status, html) = get(&st, "/settings/widgets", &h);
    assert_eq!(status, StatusCode::OK);
    let start = html.find(r#"id="w-locked""#).expect("locked section");
    let end = start + html[start..].find("</section>").unwrap();
    let locked = &html[start..end];
    assert!(locked.contains(r#"data-id="procs""#), "{locked}");
    assert!(
        locked.contains("command widgets can&#39;t be created")
            || locked.contains("command widgets can't be created"),
        "{locked}"
    );
    assert!(locked.contains("i-lock"), "lock icon");
    for control in [
        "<button",
        "<input",
        "<select",
        "<textarea",
        "data-act",
        "<form",
    ] {
        assert!(!locked.contains(control), "locked section has {control}");
    }
    // custom-api widgets are not in the locked list.
    assert!(!locked.contains(r#"data-id="beszel""#));
    // The editor has no fields for command, env or secret headers.
    for f in [
        r#"name="command""#,
        r#"name="env""#,
        r#"name="headers_env""#,
        r#"name="headers_file""#,
    ] {
        assert!(!html.contains(f), "widget form offers {f}");
    }
}

/// A config with bookmarks whose values are easy to look for.
const MARKED: &str = "[[bookmarks]]\nname = \"Router-mark\"\nurl = \"http://router.example/mark\"\ngroup = \"Links\"\n\n[[bookmarks]]\nname = \"Docs-mark\"\nurl = \"https://docs.example/mark\"\ndescription = \"Desc-mark\"\n";

fn frag(st: &AppState, path: &str, query: &str, h: &HeaderMap) -> Option<(StatusCode, String)> {
    let ctx = Ctx::new(st, "100.1.2.3", path, query, h);
    crate::ui::settings::fragment(&ctx)
}

/// Signed out, no settings data reaches the HTML: not the list, not the
/// dialog, not `data-item`, not the `?add=` / `?edit=` page form, and the
/// list fragment answers 401 with an empty body.
#[test]
fn signed_out_bookmark_urls_and_fragments_leak_nothing() {
    let st = state_with(sample(), MARKED);
    let h = HeaderMap::new();
    for q in [
        "",
        "add=bookmark",
        "edit=bookmark:0",
        "edit=bookmark:1",
        "frag=list",
    ] {
        let ctx = Ctx::new(&st, "100.1.2.3", "/settings/bookmarks", q, &h);
        let (status, html) = render(&ctx);
        assert_eq!(status, StatusCode::OK, "{q}");
        // Bookmarks themselves are public (the Overview and the sidebar show
        // them); what must not appear is anything of the settings forms.
        for leak in [
            r#"value="Router-mark""#,
            r#"value="http://router.example/mark""#,
            "?edit=bookmark",
            "data-item",
            "data-hash",
            "dlg-bookmark",
            r#"class="dlg-page"#,
            "dlg-confirm",
            "rh-csrf",
        ] {
            assert!(!html.contains(leak), "?{q}: leaked {leak}");
        }
        assert!(html.contains("set-lock"), "?{q}: lock panel");
    }
    for (path, q) in [
        ("/settings/bookmarks", "frag=list"),
        ("/settings/general", "frag=list"),
        ("/settings/nope", "frag=x"),
    ] {
        let (status, body) = frag(&st, path, q, &h).expect("a fragment request");
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}?{q}");
        assert!(body.is_empty(), "{path}?{q}: {body}");
    }
    assert!(frag(&st, "/settings/bookmarks", "", &h).is_none());
    assert!(frag(&st, "/", "frag=list", &h).is_none());
}

#[test]
fn bookmarks_are_rendered_by_the_server_with_their_dialog() {
    let st = state_with(sample(), MARKED);
    assert!(
        st.db.is_enabled(),
        "this test needs the history database (sessions)"
    );
    let h = signed_in(&st);
    let (_, html) = get(&st, "/settings/bookmarks", &h);
    // Rows with links that open the dialog, filled from data-item.
    assert!(html.contains(r#"href="?edit=bookmark:1""#), "{html}");
    assert!(html.contains(r#"data-key="bookmark:1""#));
    assert!(html.contains("Docs-mark") && html.contains("Desc-mark"));
    assert!(html.contains("data-item=\"{&quot;description&quot;:&quot;Desc-mark&quot;"));
    let sum = crate::settings::read_current(&st).unwrap().hash;
    assert!(
        html.contains(&format!(r#"data-hash="{sum}""#)),
        "the list carries the file's hash"
    );
    // The dialog: exactly today's five fields, in the design's order.
    let d = &html[html
        .find(r#"<dialog class="dlg" id="dlg-bookmark""#)
        .expect("dialog")..];
    let d = &d[..d.find("</dialog>").unwrap()];
    let names: Vec<&str> = d
        .split(r#" name=""#)
        .skip(1)
        .map(|s| &s[..s.find('"').unwrap()])
        .collect();
    assert_eq!(names, ["name", "group", "url", "icon", "description"]);
    assert!(d.contains(r#"method="post""#) && d.contains("data-hb"));
    assert!(d.contains(r#"type="submit" disabled data-ready"#), "{d}");
    assert!(
        !d.contains("-mark"),
        "the dialog itself holds no row values"
    );
    assert!(html.contains(r#"id="dlg-confirm""#) && html.contains(r#"id="m-bm""#));
    // No page form unless asked for.
    assert!(!html.contains(r#"class="dlg-page"#));
    // The no-JS page form: the same fields, filled, Save disabled.
    let (_, html) = get(&st, "/settings/bookmarks?edit=bookmark:1", &h);
    let p = &html[html.find(r#"class="dlg-page"#).expect("page form")..];
    let p = &p[..p.find("</section>").unwrap()];
    assert!(
        p.contains(r#"value="Docs-mark""#) && p.contains(r#"value="https://docs.example/mark""#)
    );
    assert!(p.contains("Saving needs JavaScript"));
    assert!(p.contains(r#"method="post""#));
    assert!(
        !p.contains("data-ready"),
        "never enabled: there is no handler on the page form"
    );
    assert!(p.contains("Edit bookmark “Docs-mark”"));
    let (_, html) = get(&st, "/settings/bookmarks?add=bookmark", &h);
    let p = &html[html.find(r#"class="dlg-page"#).expect("page form")..];
    assert!(p.contains("Add bookmark") && p.contains(r#"name="url" value="""#));
    // `edit` wins over `add` (as in ui.js).
    let (_, html) = get(&st, "/settings/bookmarks?add=bookmark&edit=bookmark:1", &h);
    let p = &html[html.find(r#"class="dlg-page"#).expect("page form")..];
    assert!(p.contains("Edit bookmark “Docs-mark”"), "edit wins");
    // One heading: the tab's, with Add bookmark beside it.
    assert_eq!(html.matches(">Bookmarks</h2>").count(), 1);
    assert!(html.contains(r#"id="bm-add""#));
    let (_, html) = get(&st, "/settings/bookmarks?edit=bookmark:7", &h);
    assert!(!html.contains(r#"class="dlg-page"#) && html.contains("does not exist"));
    // The fragment is the same list.
    let (status, f) = frag(&st, "/settings/bookmarks", "frag=list", &h).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(
        f.starts_with(r#"<div id="bm-list" data-hash=""#) && f.contains("Router-mark"),
        "{f}"
    );
    let (status, _) = frag(&st, "/settings/general", "frag=list", &h).unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The attributes of one start tag (`<button class="a" disabled>`), names
/// lowercased; a bare attribute has the value "".
fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let mut it = b.char_indices().peekable();
    // skip the tag name
    while let Some(&(_, c)) = it.peek() {
        if c.is_whitespace() {
            break;
        }
        it.next();
    }
    loop {
        while matches!(it.peek(), Some(&(_, c)) if c.is_whitespace()) {
            it.next();
        }
        let Some(&(s, _)) = it.peek() else { break };
        while matches!(it.peek(), Some(&(_, c)) if !c.is_whitespace() && c != '=') {
            it.next();
        }
        let e = it.peek().map_or(b.len(), |&(i, _)| i);
        let name = b[s..e].to_ascii_lowercase();
        let mut val = String::new();
        if matches!(it.peek(), Some(&(_, '='))) {
            it.next();
            match it.peek() {
                Some(&(_, q)) if q == '"' || q == '\'' => {
                    it.next();
                    for (_, c) in it.by_ref() {
                        if c == q {
                            break;
                        }
                        val.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = it.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        val.push(c);
                        it.next();
                    }
                }
            }
        }
        if !name.is_empty() {
            out.push((name, val));
        }
    }
    out
}

/// Every start tag named `name` in `html`, with its offset and attributes.
fn tags(html: &str, name: &str) -> Vec<(usize, Vec<(String, String)>)> {
    let open = format!("<{name}");
    html.match_indices(&open)
        .filter(|(i, _)| {
            html[i + open.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace() || c == '>')
        })
        .map(|(i, _)| (i, attrs(&html[i..i + html[i..].find('>').unwrap() + 1])))
        .collect()
}

fn attr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[test]
fn the_tag_parser_reads_what_browsers_read() {
    let a = attrs(r#"<button disabled class="btn x" type=submit data-ready>"#);
    assert_eq!(attr(&a, "disabled"), Some(""));
    assert_eq!(attr(&a, "type"), Some("submit"));
    assert_eq!(attr(&a, "class"), Some("btn x"));
    assert_eq!(attr(&attrs("<button>"), "type"), None);
    let t = tags(
        r#"<form method="post"><buttonx><button type="button"></form>"#,
        "button",
    );
    assert_eq!(t.len(), 1);
}

/// ROB-15: no Settings form can put its values in a URL. Every form posts
/// to a Settings path; every button that would submit it (a `<button>`
/// without a type submits) and every submit input is disabled until the
/// script runs; no form can be submitted with Enter through a lone text
/// field; and outside dialogs the page says why.
#[test]
fn every_settings_form_posts_and_waits_for_the_script() {
    let st = state_with(sample(), FIXTURE);
    assert!(
        st.db.is_enabled(),
        "this test needs the history database (sessions)"
    );
    let h = signed_in(&st);
    let mut forms = 0;
    for p in paths() {
        let (_, html) = get(&st, &p, &h);
        for (i, fa) in tags(&html, "form") {
            forms += 1;
            let end = i + html[i..].find("</form>").unwrap();
            let body = &html[i..end];
            let in_dialog = html[..i].rfind("<dialog") > html[..i].rfind("</dialog>");
            let method = attr(&fa, "method").map(str::to_ascii_lowercase);
            assert_eq!(method.as_deref(), Some("post"), "{p}: {fa:?}");
            assert!(
                attr(&fa, "action").is_some_and(|a| a.starts_with("/settings/")),
                "{p}: {fa:?}"
            );
            let mut submits = 0;
            for (_, b) in tags(body, "button") {
                if attr(&b, "type").is_none_or(|t| t.eq_ignore_ascii_case("submit")) {
                    submits += 1;
                    assert!(
                        attr(&b, "disabled").is_some(),
                        "{p}: an enabled submit button {b:?}"
                    );
                }
            }
            let mut texts = 0;
            for (_, inp) in tags(body, "input") {
                let ty = attr(&inp, "type").unwrap_or("text").to_ascii_lowercase();
                if ty == "submit" || ty == "image" {
                    submits += 1;
                    assert!(
                        attr(&inp, "disabled").is_some(),
                        "{p}: an enabled submit input {inp:?}"
                    );
                }
                let field = !matches!(
                    ty.as_str(),
                    "checkbox"
                        | "radio"
                        | "hidden"
                        | "button"
                        | "submit"
                        | "image"
                        | "reset"
                        | "file"
                );
                if field {
                    texts += 1;
                }
            }
            // Without a submit button, a form with exactly one text field is
            // submitted by Enter (implicit submission).
            assert!(submits > 0 || texts != 1, "{p}: Enter would submit {fa:?}");
            if submits > 0 && !in_dialog {
                assert!(
                    body.contains("Saving needs JavaScript"),
                    "{p}: no reason shown"
                );
            }
        }
    }
    assert!(forms >= 14, "parser saw {forms} forms");
    let (_, html) = get(&st, "/settings/security", &h);
    let f = &html[html.find(r#"id="f-password""#).unwrap()..];
    let f = &f[..f.find("</form>").unwrap()];
    assert_eq!(f.matches(r#"type="password""#).count(), 3);
    assert!(f.contains("data-hb") && f.contains(r#"required"#));
    assert!(
        !html.contains(r#"id="toasts""#),
        "one toast system: HB.toast"
    );
}

/// The no-JavaScript note says what is true for each form: values shown
/// (converted), values that load with the script (not converted yet), or
/// nothing to show (passwords, the clear-history phrase). Notifications,
/// whose editors only open with the script, has the note too.
#[test]
fn the_no_js_note_tells_the_truth() {
    let st = state_with(sample(), MARKED);
    assert!(
        st.db.is_enabled(),
        "this test needs the history database (sessions)"
    );
    let h = signed_in(&st);
    let (_, html) = get(&st, "/settings/general", &h);
    assert!(html.contains("The current values load with the script too."));
    assert!(!html.contains("You can read the values here."));
    let (_, html) = get(&st, "/settings/notifications", &h);
    assert!(html.contains("needs-js") && html.contains("You can read the values here."));
    let (_, html) = get(&st, "/settings/widgets", &h);
    assert!(html.contains("You can read the values here."));
    let (_, html) = get(&st, "/settings/bookmarks?edit=bookmark:0", &h);
    assert!(html.contains("You can read the values here."));
    let (_, html) = get(&st, "/settings/security", &h);
    let f = &html[html.find(r#"id="f-password""#).unwrap()..];
    let f = &f[..f.find("</form>").unwrap()];
    assert!(
        f.contains("Saving needs JavaScript") && !f.contains("values"),
        "{f}"
    );
    // The note hides only once ui.js has enabled the buttons (hb-ready),
    // not merely because the inline boot script ran (`js`).
    assert!(crate::http::server::APP_CSS.contains(".hb-ready .needs-js{display:none!important}"));
    assert!(!html.contains("warn nojs"), "the old note class");
}

/// A config for the Channels, Rules and Widgets tabs whose values are easy
/// to look for. `Sekrit-…` stand for values that must never reach a page:
/// header values and bodies (the page shows header names only).
const LISTS: &str = r#"
[[widgets]]
id = "w-mark"
type = "custom-api"
title = "Widget-mark"
url = "http://wurl.example/mark"
refresh = "45s"
template = "{{v}}"
headers = { X-Plain = "Sekrit-Header-Value" }
body = "Sekrit-Body"
[widgets.fields]
v = "path.mark"

[[widgets]]
id = "w-bound"
type = "custom-api"
url = "http://bound.example/api"
headers_env = { Authorization = "RH_W_TOKEN" }

[[widgets]]
id = "procs"
type = "command"
command = ["sh", "-c", "echo 1"]

[channels.hook-mark]
type = "webhook"
url_env = "RH_HOOK_MARK"
headers = { X-Key = "Sekrit-Channel-Header" }
body = '{"text": "Sekrit-Channel-Body {{message}}"}'

[channels.tg-mark]
type = "telegram"
token_file = "/data/secrets/tg-mark"
chat_id = -1001234

[[alerts]]
name = "rule-mark"
type = "http"
url = "http://rule.example/mark"
channels = ["hook-mark"]

[[alerts]]
name = "w-rule"
type = "widget"
widget = "w-mark"
field = "v"
above = 1
enabled = false
"#;

/// Signed out, none of the three lists, their dialogs, `data-item`, the
/// `?add=` / `?edit=` page forms or the fragments carry settings data.
#[test]
fn signed_out_channels_rules_widgets_leak_nothing() {
    let st = state_with(sample(), LISTS);
    let h = HeaderMap::new();
    for (path, qs) in [
        (
            "/settings/notifications",
            &[
                "",
                "add=channel",
                "edit=channel:hook-mark",
                "add=rule",
                "edit=rule:rule-mark",
                "frag=list",
            ][..],
        ),
        (
            "/settings/widgets",
            &["", "add=widget", "edit=widget:w-mark", "frag=list"][..],
        ),
    ] {
        for q in qs {
            let ctx = Ctx::new(&st, "100.1.2.3", path, q, &h);
            let (status, html) = render(&ctx);
            assert_eq!(status, StatusCode::OK, "{path}?{q}");
            // The account menu's sign-in link echoes the visitor's own URL
            // (`next=`): that is the request, not the file.
            let html = html
                .split("/login?next=")
                .enumerate()
                .map(|(i, part)| {
                    if i == 0 {
                        part
                    } else {
                        &part[part.find('"').unwrap_or(0)..]
                    }
                })
                .collect::<String>();
            for leak in [
                "hook-mark",
                "RH_HOOK_MARK",
                "tg-mark",
                "rule-mark",
                "rule.example",
                "wurl.example",
                "path.mark",
                "RH_W_TOKEN",
                "Sekrit",
                "data-item",
                "data-hash",
                "dlg-channel",
                "dlg-rule",
                "dlg-widget",
                r#"class="dlg-page"#,
                "sys-tiles",
                "rh-csrf",
            ] {
                if let Some(i) = html.find(leak) {
                    panic!(
                        "{path}?{q}: leaked {leak}: …{}…",
                        &html[i.saturating_sub(200)..(i + 50).min(html.len())]
                    );
                }
            }
            assert!(html.contains("set-lock"), "{path}?{q}: lock panel");
        }
        let (status, body) = frag(&st, path, "frag=list", &h).expect("fragment");
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert!(body.is_empty());
    }
}

#[test]
fn channels_and_rules_are_rendered_by_the_server_with_their_dialogs() {
    let st = state_with(sample(), LISTS);
    assert!(st.db.is_enabled(), "needs the history database (sessions)");
    let h = signed_in(&st);
    let (_, html) = get(&st, "/settings/notifications", &h);
    let sum = crate::settings::read_current(&st).unwrap().hash;
    for id in ["ch-list", "ru-list"] {
        assert!(
            html.contains(&format!(r#"id="{id}" data-hash="{sum}""#)),
            "{id} carries the file's hash"
        );
    }
    // Rows: an edit link with the dialog's values, the rules using a channel.
    assert!(html.contains(r#"href="?edit=channel:hook-mark""#));
    assert!(html.contains(r#"data-key="channel:hook-mark" data-rules="[&quot;rule-mark&quot;]""#));
    assert!(html.contains(r#"href="?edit=rule:rule-mark""#));
    assert!(html.contains("RH_HOOK_MARK") && html.contains("/data/secrets/tg-mark"));
    // Never a header value or a body, also not in data-item.
    assert!(
        !html.contains("Sekrit"),
        "a secret-ish value reached the page"
    );
    assert!(
        html.contains("Header X-Key"),
        "the header is named, read-only"
    );
    // The channel dialog: the type and source choices, every secret field
    // of every type, the test as a secondary that does not submit.
    let d = &html[html.find(r#"id="dlg-channel""#).expect("channel dialog")..];
    let d = &d[..d.find("</dialog>").unwrap()];
    for f in [
        "name",
        "type",
        "src",
        "webhook_url_env",
        "webhook_url_file",
        "token_env",
        "token_file",
        "chat_id",
        "url_env",
        "url_file",
        "url",
        "timeout",
    ] {
        assert!(d.contains(&format!(r#"name="{f}""#)), "channel dialog: {f}");
    }
    assert_eq!(
        tags(d, "button")
            .iter()
            .filter(|(_, a)| attr(a, "type") == Some("submit"))
            .count(),
        1
    );
    assert!(d.contains(r#"type="button" data-submit="test""#), "{d}");
    assert!(
        !d.contains("-mark"),
        "the dialog itself holds no row values"
    );
    // The rule dialog: a new rule notifies every channel.
    let d = &html[html.find(r#"id="dlg-rule""#).expect("rule dialog")..];
    let d = &d[..d.find("</dialog>").unwrap()];
    for c in ["hook-mark", "tg-mark"] {
        assert!(
            d.contains(&format!(
                r#"name="channels" value="{c}" data-t="set" checked"#
            )),
            "{c} ticked for a new rule"
        );
    }
    assert!(d.contains(r#"<option value="w-mark">"#) || d.contains(r#"option value="w-mark""#));
    // A switched-off rule: its row switch is off and disabled until the script.
    let row = &html[html.find(r#"data-key="rule:w-rule""#).unwrap()..];
    let sw = tags(row, "input").into_iter().next().unwrap().1;
    assert!(
        attr(&sw, "data-toggle").is_some()
            && attr(&sw, "checked").is_none()
            && attr(&sw, "disabled").is_some()
    );
    // Delete is not offered from the page form; the fragment is both lists
    // and the rule form's channel choice.
    let (status, f) = frag(&st, "/settings/notifications", "frag=list", &h).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(
        f.contains(r#"id="ch-list""#)
            && f.contains(r#"id="ru-list""#)
            && f.contains(r#"id="ru-channels""#)
    );
    assert!(!f.contains("Sekrit"));
}

#[test]
fn channel_and_rule_page_forms_without_script() {
    let st = state_with(sample(), LISTS);
    assert!(st.db.is_enabled());
    let h = signed_in(&st);
    let page = |q: &str| {
        let (_, html) = get(&st, &format!("/settings/notifications?{q}"), &h);
        let p = html.find(r#"class="dlg-page"#).map(|i| {
            let p = &html[i..];
            p[..p.find("</form></section>").unwrap()].to_string()
        });
        (html, p)
    };
    // A Telegram channel with its token in a file: the choices are checked,
    // only Telegram's fields are shown, Save is disabled, the note is true.
    let (_, p) = page("edit=channel:tg-mark");
    let p = p.expect("page form");
    assert!(p.contains("Edit channel “tg-mark”"));
    assert!(p.contains(r#"value="telegram" checked"#) && p.contains(r#"value="file" checked"#));
    assert!(p.contains(r#"value="/data/secrets/tg-mark""#) && p.contains(r#"value="-1001234""#));
    assert!(p.contains(r#"<div class="fc" data-for="slack" hidden>"#));
    assert!(p.contains("You can read the values here."));
    assert!(tags(&p, "button")
        .iter()
        .any(|(_, a)| attr(a, "type") == Some("submit") && attr(a, "disabled").is_some()));
    // A rule.
    let (_, p) = page("edit=rule:rule-mark");
    let p = p.expect("page form");
    assert!(
        p.contains(r#"value="http://rule.example/mark""#)
            && p.contains(r#"<option value="http" selected>"#)
    );
    // Edit wins over add, also across the two lists; a gone entry says so.
    let (_, p) = page("add=channel&edit=rule:rule-mark");
    assert!(p.unwrap().contains("Edit rule “rule-mark”"));
    let (html, p) = page("edit=channel:nope");
    assert!(p.is_none() && html.contains("That channel does not exist"));
    let (_, p) = page("add=rule");
    assert!(p.unwrap().contains("Add rule"));
}

#[test]
fn widgets_are_rendered_by_the_server_with_their_dialog() {
    let st = state_with(sample(), LISTS);
    assert!(st.db.is_enabled());
    let h = signed_in(&st);
    let (_, html) = get(&st, "/settings/widgets", &h);
    // Custom-api rows only, every id in file order for the arrows.
    let l = &html[html.find(r#"id="w-list""#).unwrap()..];
    let l = &l[..l.find("</table>").unwrap()];
    assert!(l.contains(r#"data-key="widget:w-mark" data-rules="[&quot;w-rule&quot;]""#));
    assert!(l.contains(r#"data-key="widget:w-bound""#) && !l.contains("widget:procs"));
    assert!(
        l.contains(r#"data-order="[&quot;w-mark&quot;,&quot;w-bound&quot;,&quot;procs&quot;]""#),
        "{l}"
    );
    assert!(
        !html.contains("Sekrit"),
        "no header value or body in the page"
    );
    assert!(
        html.contains("RH_W_TOKEN"),
        "a header's variable name is shown"
    );
    // The dialog: three tabs, Fetch now as a plain button, one submit.
    let d = &html[html.find(r#"id="dlg-widget""#).expect("dialog")..];
    let d = &d[..d.find("</dialog>").unwrap()];
    assert_eq!(d.matches(r#"role="tab""#).count(), 3);
    assert_eq!(d.matches(r#"role="tabpanel""#).count(), 3);
    assert!(d.contains(r#"type="button" data-act="wf-preview""#));
    assert_eq!(
        tags(d, "button")
            .iter()
            .filter(|(_, a)| attr(a, "type") == Some("submit"))
            .count(),
        1
    );
    for f in [
        "name=\"command\"",
        "name=\"env\"",
        "name=\"headers_env\"",
        "name=\"headers\"",
        "name=\"body\"",
    ] {
        assert!(!d.contains(f), "widget dialog offers {f}");
    }
    // System tiles: one switch, disabled until the script, and the note.
    assert!(html.contains(r#"id="sys-tiles""#) && !html.contains(r#"id="f-system""#));
    assert!(html.contains("Saving needs JavaScript"));
    // The page form: filled, sections instead of tabs.
    let (_, html) = get(&st, "/settings/widgets?edit=widget:w-mark", &h);
    let p = &html[html.find(r#"class="dlg-page"#).expect("page form")..];
    let p = &p[..p.find("</form></section>").unwrap()];
    assert!(
        p.contains(r#"value="http://wurl.example/mark""#) && p.contains(r#"value="path.mark""#)
    );
    assert!(p.contains(r#"value="45s""#), "{p}");
    assert!(
        !p.contains(r#"role="tab""#),
        "the page form has sections, not tabs"
    );
    let (_, html) = get(&st, "/settings/widgets?edit=widget:procs", &h);
    assert!(
        !html.contains(r#"class="dlg-page"#) && html.contains("That widget does not exist"),
        "command widgets are never editable"
    );
    let (status, f) = frag(&st, "/settings/widgets", "frag=list", &h).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(f.starts_with(r#"<div id="w-list""#) && f.contains("Widget-mark"));
}

/// Review fixes: a log rule shows and keeps its Except; a Telegram channel
/// whose chat id comes from a variable shows no empty required field; the
/// no-JS page form's choices and switches cannot be changed; rule names
/// with a comma are kept whole in `data-rules`.
#[test]
fn page_forms_and_rows_say_what_is_in_the_file() {
    let cfg = r#"
[channels.tg-env]
type = "telegram"
token_env = "RH_TG"
chat_id_env = "RH_TG_CHAT"

[[alerts]]
name = "logs, odd"
type = "log_match"
pattern = "error"
containers = ["web-*"]
exclude = ["web-test"]
channels = ["tg-env"]
"#;
    let st = state_with(sample(), cfg);
    assert!(st.db.is_enabled());
    let h = signed_in(&st);
    let (_, html) = get(&st, "/settings/notifications", &h);
    assert!(
        html.contains("/error/ in web-* except web-test"),
        "the row says except"
    );
    assert!(html.contains(r#"data-rules="[&quot;logs, odd&quot;]""#));
    let page = |q: &str| {
        let (_, html) = get(&st, &format!("/settings/notifications?{q}"), &h);
        let p = &html[html.find(r#"class="dlg-page"#).expect("page form")..];
        p[..p.find("</form></section>").unwrap()].to_string()
    };
    let p = page("edit=rule:logs, odd");
    let ex = tags(&p, "input")
        .into_iter()
        .find(|(_, a)| attr(a, "name") == Some("exclude"))
        .expect("Except")
        .1;
    assert_eq!(attr(&ex, "value"), Some("web-test"));
    let fld = &p[..p.find(r#"name="exclude""#).unwrap()];
    let fld = &fld[fld.rfind("<div class=\"fld").unwrap()..];
    assert!(
        fld.contains(r#"data-for="container log_match""#) && !fld.contains(" hidden"),
        "{fld}"
    );
    for (_, a) in tags(&p, "input") {
        if matches!(attr(&a, "type"), Some("radio" | "checkbox")) {
            assert!(attr(&a, "disabled").is_some(), "page form control {a:?}");
        }
    }
    for (_, a) in tags(&p, "select") {
        assert!(attr(&a, "disabled").is_some());
    }
    let p = page("edit=channel:tg-env");
    assert!(
        !p.contains(r#"name="chat_id""#),
        "no empty required chat id"
    );
    assert!(p.contains("Chat id: read from the variable") && p.contains("RH_TG_CHAT"));
}
