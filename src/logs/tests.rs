//! Route tests and the end-to-end "stream stops after disconnect" test
//! against a fake Docker daemon on a unix socket.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener};

use crate::state::{AppState, Env};
use crate::ui::shell::Ctx;
use crate::ui::testutil::{ctr, state_with};

fn page(st: &AppState, path: &str, query: &str) -> (hyper::StatusCode, String) {
    let h = hyper::HeaderMap::new();
    let ctx = Ctx::new(st, "h", path, query, &h);
    crate::ui::pages::render(&ctx)
}

fn sample() -> Vec<crate::model::Container> {
    vec![
        ctr("web", Some("shop"), "running"),
        ctr("db", Some("shop"), "exited"),
        ctr("solo", None, "running"),
    ]
}

#[test]
fn container_page_renders_frame_and_assets() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains("/static/logs.js?v="), "logs.js linked");
    assert!(html.contains("/static/logs.css?v="));
    assert!(html.contains(r#"data-q="c=web""#), "{html}");
    assert!(html.contains("data-ring=\"5000\""));
    assert!(html.contains("/stacks/shop/logs"), "link to the stack view");
    assert!(!html.contains("container is stopped"));
    // The stopped one says so and still renders the viewer.
    let (s, html) = page(&st, "/containers/db/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains("This container is stopped."));
    assert!(html.contains(r#"data-q="c=db""#));
}

#[test]
fn not_found_and_escaping() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/wbe/logs", "");
    assert_eq!(s, 404);
    assert!(html.contains("Container not found"));
    let (s, html) = page(&st, "/containers/<b>x<%2Fb>/logs", "");
    assert_eq!(s, 404);
    assert!(!html.contains("<b>x"), "{html}");
    let (s, html) = page(&st, "/stacks/nope/logs", "");
    assert_eq!(s, 404);
    assert!(html.contains("No such stack"));
    let (s, _) = page(&st, "/stacks/%3Cscript%3E/logs", "");
    assert_eq!(s, 404);
}

#[test]
fn stack_and_split_pages() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/stacks/shop/logs", "");
    assert_eq!(s, 200);
    assert!(html.contains(r#"data-q="stack=shop""#));
    assert!(html.contains(r#"data-name="db""#) && html.contains(r#"data-name="web""#));
    assert!(html.contains("2 containers, merged by time"));
    let (s, html) = page(&st, "/logs", "c=web,solo&c=db&c=bad%2Fname");
    assert_eq!(s, 200);
    assert_eq!(html.matches("class=\"lpane\"").count(), 3, "{html}");
    // Names that are not shown containers are dropped (never sent to Docker).
    let (_, html) = page(&st, "/logs", "c=web,ghost");
    assert_eq!(html.matches("class=\"lpane\"").count(), 1, "{html}");
    let (s, html) = page(&st, "/logs", "");
    assert_eq!(s, 200);
    assert!(
        html.contains(r#"name="c" value="web""#),
        "side by side still offered"
    );
    // The nav item is current on every logs page (LG-01).
    for (p, q) in [
        ("/logs", ""),
        ("/logs", "c=web,solo"),
        ("/stacks/shop/logs", ""),
        ("/containers/web/logs", ""),
    ] {
        let (_, html) = page(&st, p, q);
        assert!(
            html.contains(r#"<a class="nav-item" href="/logs" title="Logs" aria-current="page">"#),
            "{p}?{q}"
        );
        assert!(
            html.contains(r#"<a class="bb-i" href="/logs" aria-current="page">"#),
            "{p}?{q}"
        );
    }
}

/// `/logs` is a list grouped by stack (LG-04): every container one click
/// from its logs, a filter that also works without the script, stack logs
/// only for compose projects (LG-09).
#[test]
fn logs_list_groups_by_stack_and_filters() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/logs", "");
    assert_eq!(s, 200);
    let list = &html[html.find(r#"id="lpick""#).expect("the list")..];
    for n in ["web", "db", "solo"] {
        assert!(
            list.contains(&format!(
                r#"<a class="row-link" href="/containers/{n}/logs""#
            )),
            "{n}: {list}"
        );
    }
    assert!(list.contains(r#"data-k="shop""#) && list.contains(r#"data-k="Standalone""#));
    assert!(
        list.contains(r#"href="/stacks/shop/logs""#),
        "a compose project has stack logs"
    );
    assert!(
        !list.contains("/stacks/Standalone/logs"),
        "a group that is not a project has none"
    );
    assert!(list.contains(r#"<input id="lpq" type="search" name="q""#));
    assert!(list.contains("3 containers"));
    // `q` filters on the server: no script needed.
    let (_, html) = page(&st, "/logs", "q=SOL");
    let list = &html[html.find(r#"id="lpick""#).unwrap()..];
    assert!(list.contains("1 of 3 containers"));
    assert!(
        list.contains(r#"<section class="lpick-g" data-k="shop" hidden>"#),
        "{list}"
    );
    assert!(list.contains(r#"value="SOL""#) || list.contains(r#"value="sol""#));
    let (_, html) = page(&st, "/logs", "q=nothing-matches");
    assert!(
        html.contains(r#"<div id="lpick-none"><div class="empty compact">"#),
        "no-match state shown"
    );
}

/// The container's Logs tab is the viewer (owner decision Y2): header and
/// tabs above it, the Logs tab current, no second "Logs" control, and the
/// error/warning chart that the old tab carried.
#[test]
fn container_logs_page_is_the_logs_tab() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200);
    assert!(
        html.contains(r#"<header class="ctr-head">"#),
        "container header"
    );
    assert!(html.contains(r#"<nav class="cd-tabs" aria-label="Container sections">"#));
    assert!(
        html.contains(r#"<a href="/containers/web/logs?range=6h" aria-current="page">Logs</a>"#),
        "{html}"
    );
    assert!(html.contains(r#"href="/containers/web?tab=metrics"#));
    assert!(html.matches(">Logs</a>").count() == 1, "one Logs control");
    assert!(html.contains("lvl-chart"), "level chart kept");
    assert!(html.contains(r#"id="lv""#));
    // The tree marks the container on its logs page too.
    let tree = &html[html.find(r#"id="tree""#).unwrap()..];
    assert!(
        tree.contains(r#"<a href="/containers/web" title="web" aria-current="page">"#),
        "{tree}"
    );
    // Old `?tab=logs` links land on the viewer.
    let (s, html) = page(&st, "/containers/web", "tab=logs");
    assert_eq!(s, 200);
    assert!(html.contains(r#"data-q="c=web""#));
}

/// Signed out with logs behind a sign-in: the header and tabs (public on the
/// detail page) and the sign-in state; no log viewer, no stream.
#[test]
fn container_logs_signed_out_keeps_the_frame() {
    let st = state_with(sample(), "");
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 401);
    assert!(html.contains(r#"<header class="ctr-head">"#));
    assert!(html.contains(r#"aria-current="page">Logs</a>"#));
    assert!(html.contains("Sign in to read logs"));
    assert!(
        !html.contains("logs.js") && !html.contains(r#"id="lv""#) && !html.contains("lvl-chart")
    );
}

#[test]
fn docker_unreachable_banner() {
    let st = state_with(vec![], OPEN);
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        containers: vec![],
        fetched_at: 1,
        error: Some("cannot connect".into()),
    }));
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200, "page renders; the stream reports the error");
    assert!(html.contains("Docker is unreachable."));
}

/// Feature tests opt out of the default login wall explicitly.
const OPEN: &str = "[logs]\nrequire_login = false\n";

#[test]
fn require_login_hides_logs() {
    let st = state_with(sample(), "[logs]\nrequire_login = true\n");
    assert!(st.config.snapshot().error.is_none());
    for p in ["/containers/web/logs", "/stacks/shop/logs", "/logs"] {
        let (s, html) = page(&st, p, "");
        assert_eq!(s, 401, "{p}");
        assert!(html.contains("Sign in to read logs"));
        assert!(!html.contains("logs.js"));
    }
}

#[test]
fn level_chart_states() {
    let st = state_with(sample(), OPEN);
    let html = super::level_chart(&st, "web", crate::db::Range::H1).into_string();
    // OV-51: plain words and a way to change it, no config syntax.
    assert!(
        html.contains("a log alert watches") && html.contains(r#"href="/alerts""#),
        "empty state explains: {html}"
    );
    assert!(!html.contains("log_match") && !html.contains("[logs]"));
    let now = crate::util::unix_now() as i64 / 60 * 60;
    st.db
        .record_log_counts(&[("web".into(), now - 120, [3, 2, 10, 0, 0])]);
    if st.db.is_enabled() {
        let html = super::level_chart(&st, "web", crate::db::Range::H1).into_string();
        assert!(html.contains("<svg"), "{html}");
        assert!(html.contains("3 error and 2 warning lines"));
    }
}

#[test]
fn helpers() {
    assert_eq!(super::logs_url("a b"), "/containers/a%20b/logs");
    assert_eq!(super::stack_logs_url("shop"), "/stacks/shop/logs");
    assert_eq!(super::split_url(&["a", "b"]), "/logs?c=a,b");
    assert_eq!(super::parse_since("start", 100.0), Some(-1.0));
    assert_eq!(super::parse_since("1m", 100.0), Some(40.0));
    assert_eq!(super::parse_since("12.5", 100.0), Some(12.5));
    assert_eq!(super::parse_since("junk", 100.0), None);
    assert_eq!(
        super::tail::iso_ms(1_000_123_456_789),
        "1970-01-01T00:16:40.123Z"
    );
}

#[test]
fn script_never_uses_inner_html() {
    let js = crate::http::server::LOGS_JS;
    assert!(
        !js.contains("innerHTML"),
        "log text must go through textContent"
    );
    assert!(!js.contains("insertAdjacentHTML"));
    assert!(!js.contains("outerHTML"));
}

/// LG-12: lines have their real height (no `content-visibility` placeholder,
/// so the bottom is the true bottom) and a ResizeObserver keeps a following
/// pane on its newest line. The browser behaviour is checked by
/// `scripts/logs-check.mjs`.
#[test]
fn following_uses_real_heights() {
    let css = crate::http::server::LOGS_CSS;
    let rules: String = css
        .lines()
        .filter(|l| !l.trim_start().starts_with("/*"))
        .collect();
    assert!(!rules.contains("content-visibility:auto"), "LG-12");
    // No scroll anchoring: following, the script sets the bottom; scrolled
    // up, it puts the reader's line back (`restore`).
    assert!(css.contains("overflow-anchor:none"));
    let js = crate::http::server::LOGS_JS;
    assert!(js.contains("new ResizeObserver"));
    assert!(
        !js.contains("autoscrolling"),
        "following is decided by position, not a flag"
    );
}

/// A config change reloads other pages but never a logs page (owner
/// decision Y12, LG-03), nor does coming back to a hidden logs tab. The
/// script decides by the ids it looks for (`const logsPage = …`): every
/// logs page shape must carry one of them, and other pages none. The browser
/// behaviour is checked by `scripts/logs-check.mjs`.
#[test]
fn config_change_never_reloads_a_logs_page() {
    let js = crate::http::server::APP_JS;
    let line = js
        .lines()
        .find(|l| l.trim_start().starts_with("const logsPage = "))
        .expect("app.js decides which pages are logs pages");
    let ids: Vec<&str> = line
        .split("$(\"")
        .skip(1)
        .filter_map(|p| p.split('"').next())
        .collect();
    assert!(!ids.is_empty(), "{line}");
    assert!(js.contains("if (!logsPage) location.reload();"));
    assert!(js.contains("> 60000 && !logsPage &&"));
    let st = state_with(sample(), OPEN);
    let marked = |html: &str| ids.iter().any(|i| html.contains(&format!("id=\"{i}\"")));
    for (p, q) in [
        ("/logs", ""),
        ("/logs", "q=web"),
        ("/logs", "c=web,solo"),
        ("/logs", "c=web"),
        ("/stacks/shop/logs", ""),
        ("/containers/web/logs", ""),
        ("/containers/db/logs", ""),
        ("/containers/web", "tab=logs"),
    ] {
        let (s, html) = page(&st, p, q);
        assert_eq!(s, 200, "{p}?{q}");
        assert!(
            marked(&html),
            "{p}?{q} is not seen as a logs page by {ids:?}"
        );
    }
    for p in ["/", "/containers", "/containers/web"] {
        let (_, html) = page(&st, p, "");
        assert!(!marked(&html), "{p} would never reload");
    }
}

/// A container the owner excluded is in no logs list or link: not on
/// `/logs`, not as a logs icon in the sidebar tree, not in `tailable`, and
/// its logs pages say "not found".
#[test]
fn excluded_container_is_nowhere_in_logs() {
    let st = state_with(
        sample(),
        "[logs]\nrequire_login = false\n[docker]\nexclude = [\"solo\"]\n",
    );
    assert!(st.config.snapshot().error.is_none());
    let names = super::tailable(&st);
    assert!(names.contains("web") && names.contains("db"), "{names:?}");
    assert!(!names.contains("solo"), "{names:?}");
    assert!(!super::may_tail(&st, "solo"));
    let (_, html) = page(&st, "/logs", "");
    let list = &html[html.find(r#"id="lpick""#).expect("the list")..];
    assert!(list.contains(r#"value="web""#), "{list}");
    assert!(!list.contains("solo"), "{list}");
    let tree = &html[html.find(r#"id="tree""#).expect("the tree")..];
    let tree = &tree[..tree.find("side-foot").expect("tree end")];
    assert!(tree.contains(r#"href="/containers/web/logs""#), "{tree}");
    assert!(!tree.contains("solo"), "{tree}");
    let (s, _) = page(&st, "/containers/solo/logs", "");
    assert_eq!(s, 404);
    let (_, html) = page(&st, "/logs", "c=solo,web");
    assert_eq!(html.matches("class=\"lpane\"").count(), 1);
    assert!(!html.contains(r#"data-q="c=solo""#));
}

/// `/logs` lists stopped containers even when the dashboard hides them
/// (`[page] show_stopped = false`): their logs are what one comes for.
#[test]
fn logs_list_keeps_stopped_containers() {
    let st = state_with(
        sample(),
        "[logs]\nrequire_login = false\n[page]\nshow_stopped = false\n",
    );
    assert!(st.config.snapshot().error.is_none());
    let (_, html) = page(&st, "/logs", "");
    let list = &html[html.find(r#"id="lpick""#).expect("the list")..];
    assert!(list.contains(r#"href="/containers/db/logs""#), "{list}");
    assert!(list.contains("3 containers"), "{list}");
    // The dashboard itself still hides it.
    let tree = &html[html.find(r#"id="tree""#).unwrap()..];
    let tree = &tree[..tree.find("side-foot").expect("tree end")];
    assert!(!tree.contains("/containers/db\""), "{tree}");
}

/// Without the script, Enter in the filter submits only `q` (it filters),
/// and "Side by side" submits only the ticks: two forms.
#[test]
fn logs_list_filter_and_ticks_are_separate_forms() {
    let st = state_with(sample(), OPEN);
    let (_, html) = page(&st, "/logs", "");
    let list = &html[html.find(r#"id="lpick""#).unwrap()..];
    let f = &list[list.find(r#"<form class="lpick-f""#).expect("filter form")..];
    let f = &f[..f.find("</form>").unwrap()];
    assert!(
        f.contains(r#"name="q""#) && !f.contains(r#"name="c""#),
        "{f}"
    );
    let c = &list[list
        .find(r#"<form class="lpick" id="lpick-c""#)
        .expect("ticks form")..];
    let c = &c[..c.find("</form>").unwrap()];
    assert!(
        c.contains(r#"name="c""#) && !c.contains(r#"name="q""#),
        "{c}"
    );
    assert!(
        list.contains(r#"id="lpick-go" type="submit" form="lpick-c""#),
        "{list}"
    );
}

/// The split view names every pane (LG-06), links back to the list
/// (NAV-44), and each pane can be closed or another added (LG-07): plain
/// links, so it works without the script. A pane of a compose project links
/// to its stack (LG-10).
#[test]
fn split_view_names_closes_and_adds_panes() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/logs", "c=web");
    assert_eq!(s, 200);
    let crumb = &html[html.find(r#"<nav class="crumb""#).expect("crumb")..];
    assert!(crumb.contains(r#"<a href="/logs">Logs</a>"#), "{crumb}");
    let pane = &html[html.find(r#"<section class="lpane""#).expect("pane")..];
    let head = &pane[..pane.find("</header>").expect("a pane header for one pane")];
    assert!(head.contains(r#"href="/containers/web/logs""#), "{head}");
    assert!(head.contains(">web</span>"), "{head}");
    assert!(head.contains(r#"href="/stacks/shop/logs""#), "{head}");
    assert!(
        head.contains(r#"class="ia lclose" href="/logs""#),
        "closing the last pane goes back to the list: {head}"
    );
    assert!(html.contains(r#"id="ladd" href="/logs?sel=web""#), "{html}");
    let (_, html) = page(&st, "/logs", "c=web,solo");
    assert!(html.contains(r#"<h1 class="title">Side by side</h1>"#));
    assert!(html.contains(r#"class="ia lclose" href="/logs?c=solo" title="Close this pane" aria-label="Close web""#), "{html}");
    assert!(html.contains(
        r#"class="ia lclose" href="/logs?c=web" title="Close this pane" aria-label="Close solo""#
    ));
    assert!(html.contains(r#"href="/logs?sel=web,solo""#));
    assert_eq!(
        html.matches("/stacks/").count() - html.matches("/stacks/shop/logs").count(),
        0
    );
    let four = state_with(
        vec![
            ctr("a1", None, "running"),
            ctr("a2", None, "running"),
            ctr("a3", None, "running"),
            ctr("a4", None, "running"),
            ctr("a5", None, "running"),
        ],
        OPEN,
    );
    let (_, html) = page(&four, "/logs", "c=a1,a2,a3,a4,a5");
    assert_eq!(html.matches("class=\"lpane\"").count(), 4);
    assert!(
        html.contains(r#"id="ladd" href="/logs?sel=a1,a2,a3,a4" hidden"#),
        "no fifth"
    );
    assert!(html.contains("Four at a time: not shown"), "{html}");
    // The container's page offers the list with it ticked.
    let (_, html) = page(&st, "/containers/web/logs", "");
    assert!(html.contains(r#"href="/logs?sel=web""#), "{html}");
}

/// `sel=` ticks names on the list (the "Add" and "side by side" links);
/// unknown names are said (LG-08); "Side by side" with nothing ticked says
/// what to do (LG-05). Names that are not shown containers are never tailed
/// and an excluded one reads as "not found".
#[test]
fn list_preticks_and_says_what_is_missing() {
    let st = state_with(
        sample(),
        "[logs]\nrequire_login = false\n[docker]\nexclude = [\"solo\"]\n",
    );
    let (_, html) = page(&st, "/logs", "sel=web,ghost");
    assert!(html.contains(r#"name="c" value="web" checked"#), "{html}");
    assert!(!html.contains(r#"value="db" checked"#));
    assert!(html.contains(r#"<input type="hidden" name="go" value="1">"#));
    let (_, html) = page(&st, "/logs", "c=web,nope");
    assert_eq!(html.matches("class=\"lpane\"").count(), 1);
    assert!(
        html.contains("Not found: <span class=\"mono\">nope</span>"),
        "{html}"
    );
    let (s, html) = page(&st, "/logs", "c=nope,%3Cb%3E");
    assert_eq!(s, 200);
    assert!(html.contains(r#"id="lpick""#), "the list again");
    assert!(html.contains("nope, &lt;b&gt;"), "{html}");
    let (_, html) = page(&st, "/logs", "c=solo");
    assert!(html.contains("Not found: <span class=\"mono\">solo</span>"));
    assert!(!html.contains(r#"data-q="c=solo""#) && !html.contains(r#"value="solo""#));
    let (_, html) = page(&st, "/logs", "go=1");
    assert!(
        html.contains("Tick one to four containers first."),
        "{html}"
    );
    let (_, html) = page(&st, "/logs", "");
    assert!(!html.contains("Tick one to four") && !html.contains("Not found"));
    // Signed out, none of it is shown.
    let st = state_with(sample(), "");
    for q in ["c=web", "sel=web", "c=nope", "go=1"] {
        let (s, html) = page(&st, "/logs", q);
        assert_eq!(s, 401, "{q}");
        assert!(
            !html.contains("Not found") && !html.contains(r#"value="web""#),
            "{q}"
        );
    }
}

/// The toolbar is one row (LG-19, LG-31): search with the match stepper
/// (LG-13), "Filters" for phones, one "Show" control instead of "Only
/// matching" plus context; the custom range has visible labels and minute
/// precision (LG-17); a stack's chips are behind a summary (LG-24).
#[test]
fn toolbar_shape() {
    let st = state_with(sample(), OPEN);
    let (_, html) = page(&st, "/containers/web/logs", "");
    for id in [
        "lq", "lqn", "lprev", "lnext", "lfilt", "lmore", "lshow", "lrange", "lcustom", "lrerr",
    ] {
        assert!(html.contains(&format!(r#"id="{id}""#)), "{id}");
    }
    assert!(!html.contains(r#"id="lonly""#) && !html.contains(r#"id="lctx""#));
    assert!(html.contains(r#"aria-controls="lmore""#));
    assert!(html.contains("<span>From</span>") && html.contains("<span>To</span>"));
    assert_eq!(
        html.matches(r#"type="datetime-local" step="60""#).count(),
        2
    );
    assert!(
        html.contains(r#"class="btn sm lolder""#),
        "load older (LG-21)"
    );
    assert!(
        !html.contains(r#"class="lg""#),
        "LG-22: no chart-legend class"
    );
    let (_, html) = page(&st, "/stacks/shop/logs", "");
    assert!(html.contains(r#"<details class="lchipd"><summary><span class="lph-t">2 containers, merged by time</span></summary>"#), "{html}");
}

/// Browser-side rules the static files must keep; the behaviour itself is
/// checked by `scripts/logs-check.mjs`.
#[test]
fn viewer_script_and_styles() {
    let js = crate::http::server::LOGS_JS;
    let css = crate::http::server::LOGS_CSS;
    // LG-20: the line actions show on a picked line and while focused, also
    // on touch screens.
    assert!(css.contains(".ll.lsel>.lact") && css.contains(".lact:focus-within"));
    assert!(js.contains(r#"e.key !== "Enter" || e.target !== p.body"#));
    // LG-13: the search is kept in the address; Enter steps.
    assert!(js.contains("search=") && js.contains("step(e.shiftKey ? 1 : -1)"));
    // LG-22: the gutter has its own class; A11Y-08: and a name.
    assert!(js.contains(r#"el("span", "lgut")"#) && js.contains(r#""aria-label", lw"#));
    // A11Y-14: no faded milliseconds.
    assert!(css.contains(".lms{color:var(--text-3)}"));
    // LG-26: narrow panes put the time above the message.
    assert!(
        css.contains("container-type:inline-size") && css.contains("@container (max-width:340px)")
    );
}

/// The Logs tab keeps the chosen range: its chart and the other tabs use it.
#[test]
fn logs_tab_keeps_the_range() {
    let st = state_with(sample(), OPEN);
    let (_, html) = page(&st, "/containers/web", "tab=metrics&range=7d");
    assert!(
        html.contains(r#"href="/containers/web/logs?range=7d""#),
        "{html}"
    );
    let (_, html) = page(&st, "/containers/web/logs", "range=7d");
    assert!(html.contains("Errors and warnings, last 7d"), "{html}");
    assert!(
        html.contains(r#"href="/containers/web?tab=metrics&amp;range=7d""#),
        "{html}"
    );
}

// ---------------------------------------------------------------- fake docker

struct Fake {
    path: std::path::PathBuf,
    /// Log connections currently open.
    open: Arc<AtomicUsize>,
    /// Set when a log connection was closed by the client.
    closed: Arc<AtomicBool>,
}

fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![stream, 0, 0, 0];
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

async fn fake_docker(tag: &str) -> Fake {
    let path = std::env::temp_dir().join(format!("rh-logs-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let open = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let (o, c) = (open.clone(), closed.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (o, c) = (o.clone(), c.clone());
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&req).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                if path.ends_with("/json") {
                    let body = if path.contains("/web/") {
                        r#"{"Id":"abc123","Name":"/web","Config":{"Tty":false},"State":{"Status":"running","Running":true,"StartedAt":"2026-11-14T21:00:00Z"}}"#
                    } else {
                        r#"{"message":"No such container"}"#
                    };
                    let status = if path.contains("/web/") {
                        "200 OK"
                    } else {
                        "404 Not Found"
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                    return;
                }
                o.fetch_add(1, Ordering::SeqCst);
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nConnection: close\r\n\r\n")
                    .await;
                let (mut rd, mut wr) = s.into_split();
                let writer = async {
                    let mut n = 0u64;
                    loop {
                        n += 1;
                        let line = format!(
                            "2026-11-14T21:30:{:02}.000000000Z \x1b[31mERROR\x1b[0m line {n} <b>&\n",
                            n % 60
                        );
                        if wr
                            .write_all(&frame(1 + (n % 2) as u8, line.as_bytes()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                };
                let reader = async {
                    let mut b = [0u8; 64];
                    loop {
                        match rd.read(&mut b).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                    }
                };
                tokio::select! { _ = writer => {}, _ = reader => {} }
                o.fetch_sub(1, Ordering::SeqCst);
                c.store(true, Ordering::SeqCst);
            });
        }
    });
    Fake { path, open, closed }
}

fn state_on(socket: &std::path::Path, tag: &str) -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!("rh-logs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), OPEN).unwrap();
    let st = AppState::new(Env {
        listen: vec![],
        data_dir: dir.clone(),
        config_path: dir.join("config.toml"),
        docker_socket: socket.display().to_string(),
        host_proc: "/nonexistent".into(),
        host_sys: "/nonexistent".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    });
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        containers: vec![ctr("web", Some("shop"), "running")],
        fetched_at: 1,
        error: None,
    }));
    st
}

async fn serve(st: Arc<AppState>) -> (std::net::SocketAddr, String) {
    let token = "ab".repeat(32);
    let hash = crate::auth::sha256_hex(token.as_bytes());
    let now = crate::auth::now_s();
    assert!(st.db.create_session(&hash, now, now + 60));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(crate::http::server::serve_listener(st, l));
    (addr, format!("rh_session={token}"))
}

async fn read_until(s: &mut TcpStream, needle: &str, limit: Duration) -> String {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    let end = Instant::now() + limit;
    while !String::from_utf8_lossy(&got).contains(needle) {
        let left = end.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, s.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&got).to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn stream_tails_and_stops_after_disconnect() {
    let fake = fake_docker("sse").await;
    let st = state_on(&fake.path, "sse");
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=web&tail=10 HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "event: l", Duration::from_secs(5)).await;
    assert!(got.contains("text/event-stream"), "{got}");
    assert!(got.contains("event: meta"), "{got}");
    let got = got + &read_until(&mut s, "line 3", Duration::from_secs(5)).await;
    // ANSI is passed through (the browser renders it); HTML is plain JSON text.
    assert!(got.contains(r#"\u001b[31mERROR"#), "{got}");
    assert!(got.contains("<b>&"), "{got}");
    assert!(got.contains(r#""l":"e""#), "level detected: {got}");
    assert!(got.contains(r#""s":2"#), "stderr marked");
    assert_eq!(fake.open.load(Ordering::SeqCst), 1);
    let t = Instant::now();
    drop(s);
    while fake.open.load(Ordering::SeqCst) > 0 && t.elapsed() < Duration::from_secs(8) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let took = t.elapsed();
    assert!(
        fake.closed.load(Ordering::SeqCst),
        "docker stream was closed"
    );
    assert!(took < Duration::from_secs(5), "stopped after {took:?}");
    eprintln!("docker stream closed {took:?} after the viewer left");
}

/// A closed window is read from its start (`tail=all` toward Docker) and
/// cut by the server at the asked-for count; an open one keeps `tail`.
#[test]
fn closed_window_is_cut_by_the_server() {
    let q = crate::ui::fmt::parse_query("c=web&since=100&until=200&tail=3");
    let rq = super::stream_query(&q, 500);
    assert_eq!((rq.tail, rq.cap, rq.follow), (None, Some(3), false));
    assert!(
        rq.path("x").contains("&tail=all&since=100."),
        "{}",
        rq.path("x")
    );
    let rq = super::stream_query(
        &crate::ui::fmt::parse_query("c=web&until=200&tail=9000"),
        500,
    );
    assert_eq!((rq.tail, rq.cap), (Some(5000), None));
    let rq = super::stream_query(&crate::ui::fmt::parse_query("c=web"), 500);
    assert_eq!((rq.tail, rq.cap, rq.follow), (Some(500), None, true));
}

#[tokio::test(flavor = "current_thread")]
async fn closed_window_stops_at_the_cap() {
    let fake = fake_docker("cap").await;
    let st = state_on(&fake.path, "cap");
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=web&since=100&until=200&tail=3 HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "event: done", Duration::from_secs(5)).await;
    assert!(got.contains("event: done"), "{got}");
    assert!(got.contains("line 3 ") && !got.contains("line 4 "), "{got}");
    // More lines were in the window: the end event says the read was cut.
    assert!(
        got.contains(
            r#"event: end
data: {"c":0,"cut":true}"#
        ),
        "{got}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stream_reports_missing_container_and_bad_request() {
    let fake = fake_docker("missing").await;
    let st = state_on(&fake.path, "missing");
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    // A name that is not a shown container is refused before any Docker call.
    s.write_all(
        format!(
            "GET /api/logs/stream?c=ghost HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let got = read_until(&mut s, "\r\n\r\n", Duration::from_secs(5)).await;
    assert!(got.starts_with("HTTP/1.1 400"), "{got}");
    assert_eq!(fake.open.load(Ordering::SeqCst), 0, "Docker was not asked");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/stream?c=..%2Fetc HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "\r\n\r\n", Duration::from_secs(5)).await;
    assert!(got.starts_with("HTTP/1.1 400"), "{got}");
}

#[tokio::test(flavor = "current_thread")]
async fn download_streams_text_with_disposition() {
    let fake = fake_docker("dl").await;
    let st = state_on(&fake.path, "dl");
    // The fake never ends a log, so ask for a stopped-style read: the
    // download uses follow=0, and the fake closes nothing; read a bit.
    let (addr, cookie) = serve(st).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/logs/download?c=web&since=1h HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "line 2", Duration::from_secs(8)).await;
    assert!(
        got.contains("content-disposition: attachment; filename=\"web-"),
        "{got}"
    );
    assert!(
        got.contains("2026-11-14T21:30:01.000Z stderr ERROR line 1 <b>&"),
        "ANSI removed: {got}"
    );
}

#[test]
fn logs_need_a_login_by_default() {
    // No [logs] section at all: fail closed.
    let st = state_with(sample(), "");
    for p in ["/containers/web/logs", "/stacks/shop/logs", "/logs"] {
        let (s, html) = page(&st, p, "");
        assert_eq!(s, 401, "{p}");
        assert!(html.contains("set-password"), "explains how to unlock: {p}");
    }
}

#[test]
fn hidden_and_excluded_containers_have_no_logs() {
    let mut cs = sample();
    cs.push(ctr("secret-db", Some("shop"), "running"));
    let st = state_with(
        cs,
        "[logs]\nrequire_login = false\n[docker]\nexclude = [\"secret-*\"]\n[containers.web]\nhide = true\n",
    );
    assert!(!super::may_tail(&st, "secret-db"), "excluded by glob");
    assert!(!super::may_tail(&st, "web"), "hidden by override");
    assert!(!super::may_tail(&st, "ghost"), "not a container here");
    assert!(super::may_tail(&st, "db"));
    let (s, _) = page(&st, "/containers/secret-db/logs", "");
    assert_eq!(s, 404);
    let (s, _) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 404);
    let members = super::stack_members(&st, "shop");
    assert!(
        !members.iter().any(|m| m == "secret-db" || m == "web"),
        "{members:?}"
    );
}

/// The pane is served plain: no busy region and no skeleton (the script
/// sets both while it connects), and a sentence that works without the
/// script, also when it is blocked or fails to load (not a `<noscript>`,
/// which then shows nothing; §11.9).
#[test]
fn pane_is_served_plain_and_the_script_makes_it_busy() {
    let st = state_with(sample(), OPEN);
    let (s, html) = page(&st, "/containers/web/logs", "");
    assert_eq!(s, 200);
    assert!(
        html.contains(r#"<div class="lbody" tabindex="0" aria-label="Lines of web. Enter picks a line to copy or link."><button class="btn sm lolder" type="button" hidden>"#)
            && html.contains(r#"Load older lines</button><div class="lines""#),
        "{html}"
    );
    assert!(!html.contains(r#"aria-busy=""#) && !html.contains(r#"<span class="sk""#));
    assert!(html.contains(r#"<p class="js-late">The log viewer needs JavaScript. You can still <a href="/api/logs/download?"#));
    assert!(crate::http::server::LOGS_JS.contains(r#"this.body.setAttribute("aria-busy""#));
}
