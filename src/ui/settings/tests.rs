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
    // Every input has a label.
    for p in paths() {
        let (_, html) = get(&st, &p, &h);
        for id in html.split(r#"<input class="in"#).skip(1).filter_map(|s| {
            let i = s.find(r#"id=""#)? + 4;
            Some(s[i..i + s[i..].find('"')?].to_string())
        }) {
            assert!(
                html.contains(&format!(r#"for="{id}""#)),
                "{p}: no label for {id}"
            );
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
    assert!(html.contains("needs-js") && html.contains("Saving needs JavaScript"));
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
