//! The logs pages. The server renders the frame (toolbar, panes, states);
//! `static/logs.js` connects each pane to `/api/logs/stream` and renders
//! the lines. Without JavaScript the page says so and offers the download.

use hyper::StatusCode;
use maud::{html, Markup};

use super::client::valid_name;
use super::{logs_url, split_url, stack_logs_url, stack_members};
use crate::db::Range;
use crate::model::Container;
use crate::state::AppState;
use crate::ui::fmt::{parse_query, url_encode};
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::{empty_state, ic};

/// Stable hue (0..360) for a container name (FNV-1a, so names that only
/// differ at the end still get distinct colors); `static/logs.js` has the same.
pub fn hue(name: &str) -> u32 {
    name.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ b as u32).wrapping_mul(16_777_619)
    }) % 360
}

/// A stack member's name without the `<stack>-` prefix (compose names are
/// `<project>-<service>-<n>`), for compact chips.
pub fn short_name<'a>(name: &'a str, stack: Option<&str>) -> &'a str {
    stack
        .and_then(|s| name.strip_prefix(s))
        .and_then(|r| r.strip_prefix(['-', '_']))
        .filter(|r| !r.is_empty())
        .unwrap_or(name)
}

struct Pane {
    /// Query for `/api/logs/stream` and the download (`c=…` or `stack=…`).
    query: String,
    title: String,
    names: Vec<String>,
    /// Compose project, for the merged view's short chip names.
    stack: Option<String>,
    /// Link for the pane title.
    href: Option<String>,
}

fn assets(ctx: &Ctx) -> Markup {
    let sha = ctx.state.build.sha;
    html! {
        link rel="stylesheet" href={"/static/logs.css?v=" (sha)};
        script src={"/static/logs.js?v=" (sha)} defer {}
    }
}

fn login_needed(ctx: &Ctx, title: &str) -> (StatusCode, String) {
    let next = url_encode(&if ctx.query.is_empty() {
        ctx.path.to_string()
    } else {
        format!("{}?{}", ctx.path, ctx.query)
    });
    let body = html! {
        div.page {
            (empty_state("lock", "Sign in to read logs", html! {
                "Container logs often contain secrets, so they are shown only after signing in ("
                code { "[logs] require_login" } ", on by default). "
                @if crate::auth::password_is_set(ctx.state) {
                    a href={"/login?next=" (next)} { "Sign in" }
                } @else {
                    "No password is set yet: run "
                    code { "docker exec -it hobnail /hobnail set-password" }
                    " on the host, or set " code { "require_login = false" } " under "
                    code { "[logs] " } " to make logs open like the rest of the page."
                }
            }))
        }
    };
    (
        StatusCode::UNAUTHORIZED,
        layout(
            ctx,
            Nav::Containers,
            title,
            crate::ui::pages::crumb_title(title),
            body,
        ),
    )
}

fn docker_banner(ctx: &Ctx) -> Markup {
    let snap = ctx.state.snapshot();
    html! {
        @if let Some(e) = &snap.error {
            div.banner.bad role="alert" {
                (ic("alert"))
                div { strong { "Docker is unreachable." } " Logs cannot be read right now. " code.wrap { (e) } }
            }
        }
    }
}

fn watch_banner() -> Markup {
    let over = super::watch::shared().lock().unwrap().over_cap.clone();
    html! {
        @if !over.is_empty() {
            div.banner.warn {
                (ic("alert"))
                div {
                    strong { "Log alerts are not watching " (over.len()) " container" @if over.len() != 1 { "s" } "." }
                    " More containers match log_match rules than " code { "[logs] max_watched" } " allows: "
                    span.mono { (over.join(", ")) }
                }
            }
        }
    }
}

fn find<'a>(snap: &'a crate::model::Snapshot, name: &str) -> Option<&'a Container> {
    snap.containers.iter().find(|c| c.name == name)
}

/// `/containers/<name>/logs`.
pub fn container(ctx: &Ctx, name: &str) -> (StatusCode, String) {
    let title = format!("Logs · {name}");
    if ctx.cfg.logs.require_login && ctx.user.is_none() {
        return login_needed(ctx, &title);
    }
    let snap = ctx.state.snapshot();
    let shown: Vec<crate::model::Container> = snap
        .containers
        .iter()
        .filter(|c| !crate::ui::view::is_excluded(&ctx.cfg, c))
        .cloned()
        .collect();
    let c = find(&snap, name).filter(|c| !crate::ui::view::is_excluded(&ctx.cfg, c));
    let hidden = find(&snap, name).is_some() && c.is_none();
    if hidden || c.is_none() && snap.error.is_none() && snap.fetched_at != 0 || !valid_name(name) {
        return not_found(ctx, name, &shown);
    }
    let heading = html! {
        nav.crumb aria-label="Breadcrumb" {
            a href="/containers" { "Containers" }
            (ic("chev-r"))
            a.mono href={"/containers/" (url_encode(name))} { (name) }
            (ic("chev-r"))
            h1.title { "Logs" }
        }
    };
    let stopped = c.filter(|c| !c.is_running());
    let pane = Pane {
        query: format!("c={}", url_encode(name)),
        title: name.to_string(),
        names: vec![name.to_string()],
        stack: None,
        href: Some(format!("/containers/{}", url_encode(name))),
    };
    let body = html! {
        (assets(ctx))
        div.page.lpage {
            (docker_banner(ctx))
            @if let Some(c) = stopped {
                div.banner.warn.lstopped {
                    (ic("alert"))
                    div {
                        strong { "This container is stopped." }
                        " Showing its last lines. " span.muted { (c.status) }
                    }
                }
            }
            (watch_banner())
            (viewer(ctx, &[pane], false))
        }
    };
    (
        StatusCode::OK,
        layout(ctx, Nav::Containers, &title, heading, body),
    )
}

/// `/stacks/<name>/logs`.
pub fn stack(ctx: &Ctx, stack: &str) -> (StatusCode, String) {
    let title = format!("Logs · {stack}");
    if ctx.cfg.logs.require_login && ctx.user.is_none() {
        return login_needed(ctx, &title);
    }
    let names = stack_members(ctx.state, stack);
    let snap = ctx.state.snapshot();
    if names.is_empty() && snap.error.is_none() {
        let body = html! {
            div.page {
                (empty_state("search", "No such stack", html! {
                    "No compose project named " code { (stack) } " has containers. "
                    a href="/containers" { "See all containers" } "."
                }))
            }
        };
        return (
            StatusCode::NOT_FOUND,
            layout(
                ctx,
                Nav::Containers,
                &title,
                crate::ui::pages::crumb_title("Logs"),
                body,
            ),
        );
    }
    let heading = html! {
        nav.crumb aria-label="Breadcrumb" {
            a href={"/containers?stack=" (url_encode(stack))} { "Stack " span.mono { (stack) } }
            (ic("chev-r"))
            h1.title { "Logs" }
        }
    };
    let pane = Pane {
        query: format!("stack={}", url_encode(stack)),
        title: format!("{} containers, merged by time", names.len()),
        names,
        stack: Some(stack.to_string()),
        href: None,
    };
    let body = html! {
        (assets(ctx))
        div.page.lpage {
            (docker_banner(ctx))
            (watch_banner())
            (viewer(ctx, &[pane], true))
        }
    };
    (
        StatusCode::OK,
        layout(ctx, Nav::Containers, &title, heading, body),
    )
}

/// `/logs?c=a,b`: 2 to 4 containers side by side (one is fine too).
pub fn split(ctx: &Ctx) -> (StatusCode, String) {
    let title = "Logs";
    if ctx.cfg.logs.require_login && ctx.user.is_none() {
        return login_needed(ctx, title);
    }
    let q = parse_query(ctx.query);
    let mut names: Vec<String> = vec![];
    for (k, v) in &q {
        if k != "c" {
            continue;
        }
        for n in v.split(',').map(str::trim) {
            if super::may_tail(ctx.state, n) && !names.iter().any(|x| x == n) && names.len() < 4 {
                names.push(n.to_string());
            }
        }
    }
    let snap = ctx.state.snapshot();
    let heading = crate::ui::pages::crumb_title("Logs");
    let body = if names.is_empty() {
        picker(ctx, &snap)
    } else {
        let panes: Vec<Pane> = names
            .iter()
            .map(|n| Pane {
                query: format!("c={}", url_encode(n)),
                title: n.clone(),
                names: vec![n.clone()],
                stack: None,
                href: Some(logs_url(n)),
            })
            .collect();
        let missing: Vec<&String> = if snap.fetched_at != 0 && snap.error.is_none() {
            names.iter().filter(|n| find(&snap, n).is_none()).collect()
        } else {
            vec![]
        };
        html! {
            (assets(ctx))
            div.page.lpage.lsplit {
                (docker_banner(ctx))
                @if !missing.is_empty() {
                    div.banner.warn {
                        (ic("alert"))
                        div { "Not found: " span.mono { @for (i, m) in missing.iter().enumerate() { @if i > 0 { ", " } (m) } } }
                    }
                }
                (watch_banner())
                (viewer(ctx, &panes, false))
            }
        }
    };
    (
        StatusCode::OK,
        layout(ctx, Nav::Containers, title, heading, body),
    )
}

/// The split view with nothing chosen: pick up to four containers.
fn picker(ctx: &Ctx, snap: &crate::model::Snapshot) -> Markup {
    let mut cs: Vec<&Container> = snap.containers.iter().collect();
    cs.sort_by(|a, b| (!a.is_running(), &a.name).cmp(&(!b.is_running(), &b.name)));
    html! {
        (assets(ctx))
        div.page.lpage {
            (docker_banner(ctx))
            section.panel.lpick {
                div.panel-h { h2 { "Logs side by side" } }
                p.muted { "Choose up to four containers. On a phone they stack." }
                @if cs.is_empty() {
                    (empty_state("box", "No containers", html! { "Nothing to show yet." }))
                } @else {
                    form method="get" action="/logs" {
                        ul.lpick-list {
                            @for c in &cs {
                                li {
                                    label {
                                        input type="checkbox" name="c" value=(c.name);
                                        span.dot.(if c.is_running() { "ok" } else { "off" }) {}
                                        span.mono { (c.name) }
                                        @if !c.is_running() { span.muted { " (stopped)" } }
                                    }
                                }
                            }
                        }
                        button.btn.primary type="submit" { (ic("logs")) "Show logs" }
                    }
                }
            }
        }
    }
}

fn not_found(ctx: &Ctx, name: &str, all: &[Container]) -> (StatusCode, String) {
    let lower = name.to_lowercase();
    let mut near: Vec<&str> = all
        .iter()
        .map(|c| c.name.as_str())
        .filter(|n| {
            let l = n.to_lowercase();
            !lower.is_empty() && (l.contains(&lower) || lower.contains(&l))
        })
        .take(5)
        .collect();
    near.sort();
    let body = html! {
        div.page {
            (empty_state("search", "Container not found", html! {
                "There is no container named " code { (name) } ". "
                @if !near.is_empty() {
                    "Did you mean "
                    @for (i, n) in near.iter().enumerate() {
                        @if i > 0 { ", " }
                        a.mono href=(logs_url(n)) { (n) }
                    }
                    "? "
                }
                a href="/containers" { "See all containers" } "."
            }))
        }
    };
    (
        StatusCode::NOT_FOUND,
        layout(
            ctx,
            Nav::Containers,
            "Not found",
            crate::ui::pages::crumb_title("Not found"),
            body,
        ),
    )
}

const RANGES: &[(&str, &str)] = &[
    ("", "Live: last lines"),
    ("15m", "Last 15 minutes"),
    ("1h", "Last hour"),
    ("6h", "Last 6 hours"),
    ("24h", "Last 24 hours"),
    ("start", "Since container start"),
    ("custom", "Custom range…"),
];

fn toggle(id: &str, label: &str, icon: Option<&str>, pressed: bool, title: &str) -> Markup {
    html! {
        button.btn.sm.ltog type="button" id=(id) aria-pressed=(if pressed { "true" } else { "false" }) title=(title) {
            @if let Some(i) = icon { (ic(i)) }
            span { (label) }
        }
    }
}

fn viewer(ctx: &Ctx, panes: &[Pane], merged: bool) -> Markup {
    let cfg = &ctx.cfg.logs;
    let first = panes.first().map(|p| p.query.as_str()).unwrap_or("");
    let multi = panes.len() > 1;
    html! {
        div.lv #lv data-ring=(cfg.ring()) data-tail=(cfg.tail()) data-merged=[merged.then_some("1")]
            data-stack=[panes.first().and_then(|p| p.stack.as_deref())] {
            div.ltool role="toolbar" aria-label="Log tools" {
                div.lsearch {
                    (ic("search"))
                    input #lq type="search" placeholder="Search logs" aria-label="Search logs (press / to focus)"
                        autocomplete="off" spellcheck="false";
                    button.ltog.lre type="button" #lre aria-pressed="false" title="Regular expression" aria-label="Regular expression" { ".*" }
                    button.ltog.lre type="button" #lcase aria-pressed="false" title="Match case" aria-label="Match case" { "Aa" }
                }
                (toggle("lonly", "Only matching", Some("filter"), false, "Show only lines that match the search"))
                label.lctl title="Lines of context around each match" {
                    span.sr { "Context lines" }
                    select #lctx {
                        option value="0" { "No context" }
                        option value="1" { "±1 line" }
                        option value="3" { "±3 lines" }
                        option value="10" { "±10 lines" }
                    }
                }
                div.llevels role="group" aria-label="Levels" {
                    @for (code, name) in [("e", "Error"), ("w", "Warn"), ("i", "Info"), ("d", "Debug"), ("o", "Other")] {
                        button.lchip type="button" data-lv=(code) aria-pressed="true" title={"Show " (name.to_lowercase()) " lines"} {
                            span.lg {} (name) " " b.num data-n=(code) { "0" }
                        }
                    }
                }
                span.lsp {}
                label.lctl {
                    span.sr { "Time range" }
                    select #lrange {
                        @for (v, l) in RANGES { option value=(v) { (l) } }
                    }
                }
                span.lcustom #lcustom hidden {
                    label { span.sr { "From" } input #lfrom type="datetime-local" step="1"; }
                    "–"
                    label { span.sr { "To (empty: now, live)" } input #lto type="datetime-local" step="1"; }
                    button.btn.sm #lapply type="button" { "Apply" }
                }
                (toggle("lwrap", "Wrap", None, true, "Wrap long lines"))
                (toggle("ltime", "Time", Some("clock"), true, "Show timestamps (your local time)"))
                (toggle("lpause", "Pause", None, false, "Pause or resume the live tail"))
                a.btn.sm #ldl href={"/api/logs/download?" (first) "&since=1h"} title="Download the selected range as text" download {
                    (ic("arrow-down")) span { "Download" }
                }
                span.lcount.num #lcount aria-live="polite" { "0 lines" }
            }
            div.lpanes.(if multi { "multi" } else { "one" }) style={"--n:" (panes.len())} {
                @for (i, p) in panes.iter().enumerate() {
                    section.lpane data-q=(p.query) data-i=(i) aria-label={"Logs of " (p.title)} {
                        @if multi || merged {
                            header.lph {
                                @if merged {
                                    span.lph-t { (p.title) }
                                    span.lchips {
                                        @for n in &p.names {
                                            span.lcn style={"--h:" (hue(n))} data-name=(n) title=(n) { (short_name(n, p.stack.as_deref())) }
                                        }
                                    }
                                } @else if let Some(h) = &p.href {
                                    a.mono.lph-t href=(h) { span.lcn style={"--h:" (hue(&p.title))} { (p.title) } }
                                }
                                span.lph-s data-st="" {}
                            }
                        }
                        div.lbody tabindex="0" {
                            div.lines role="log" aria-live="off" {}
                            div.lmsg {
                                noscript { "The log viewer needs JavaScript. You can still " a href={"/api/logs/download?" (p.query) "&tail=5000"} { "download the last 5,000 lines" } "." }
                                span.js-only { "Connecting…" }
                            }
                        }
                        button.ljump type="button" hidden { (ic("arrow-down")) "Jump to latest" span.ljn.num {} }
                    }
                }
            }
            @if !multi && !merged {
                @if let Some(p) = panes.first() {
                    @if let Some(n) = p.names.first() {
                        p.lfoot.muted {
                            "Open side by side: "
                            a href=(split_url(&[n.as_str()])) { "split view" }
                            @if let Some(stack) = ctx.state.snapshot().containers.iter().find(|c| &c.name == n).and_then(|c| c.project.clone()) {
                                " · " a href=(stack_logs_url(&stack)) { "whole stack " span.mono { (stack) } }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Error and warning lines per bucket for one container, as a small inline
/// SVG bar chart (no external CSS needed, so any page can embed it). For the
/// container detail page and the design pass to embed.
#[allow(dead_code)]
pub fn level_chart(state: &AppState, name: &str, range: Range) -> Markup {
    let now = crate::util::unix_now() as i64;
    let s = state.db.log_level_series(name, range, now);
    let watched = super::watch::shared()
        .lock()
        .unwrap()
        .watched
        .iter()
        .any(|w| w == name);
    let total = |k: usize| s.points.iter().map(|(_, c)| c[k]).sum::<u64>();
    let (errs, warns) = (total(0), total(1));
    if s.points.is_empty() {
        return html! {
            div.lvchart.empty-sm {
                p.muted {
                    @if watched { "No log lines counted in this range yet." }
                    @else {
                        "Error and warning counts are recorded for containers watched by a "
                        code { "log_match" } " alert or listed in " code { "[logs] count" } "."
                    }
                }
            }
        };
    }
    let step = s.step.max(60);
    let from = (now - range.secs()) / step * step;
    let n = ((now - from) / step + 1).max(1) as usize;
    let mut buckets = vec![(0u64, 0u64); n];
    for (t, c) in &s.points {
        let i = ((t - from) / step).clamp(0, n as i64 - 1) as usize;
        buckets[i].0 += c[0];
        buckets[i].1 += c[1];
    }
    let max = buckets.iter().map(|(e, w)| e + w).max().unwrap_or(0).max(1) as f64;
    let h = 40.0;
    let label = format!(
        "{errs} error and {warns} warning lines in the last {}",
        range.as_str()
    );
    html! {
        figure.lvchart role="img" aria-label=(label) style="margin:0" {
            svg viewBox={"0 0 " (n) " " (h)} preserveAspectRatio="none" width="100%" height="40" aria-hidden="true" style="display:block" {
                @for (i, (e, w)) in buckets.iter().enumerate() {
                    @if e + w > 0 {
                        @let he = *e as f64 / max * h;
                        @let hw = *w as f64 / max * h;
                        rect x=(i) y=(format!("{:.2}", h - he)) width="0.8" height=(format!("{:.2}", he)) fill="var(--bad)" {
                            title { (crate::util::fmt_utc((from + i as i64 * step) as u64)) ": " (e) " errors, " (w) " warnings" }
                        }
                        rect x=(i) y=(format!("{:.2}", h - he - hw)) width="0.8" height=(format!("{:.2}", hw)) fill="var(--warn)" {}
                    }
                }
            }
            figcaption.muted style="font-size:12px;display:flex;gap:12px;margin-top:4px" {
                span { span style="display:inline-block;width:8px;height:8px;border-radius:2px;background:var(--bad);margin-right:4px" {} b.num { (errs) } " errors" }
                span { span style="display:inline-block;width:8px;height:8px;border-radius:2px;background:var(--warn);margin-right:4px" {} b.num { (warns) } " warnings" }
                span { "per " (crate::util::human_duration(step as u64)) }
                a href=(logs_url(name)) style="margin-left:auto" { "Open logs" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hues_and_short_names() {
        assert_eq!(hue("web"), hue("web"));
        let hs: Vec<u32> = ["shop-db-1", "shop-web-1", "shop-api-1", "shop-cache-1"]
            .iter()
            .map(|n| hue(n))
            .collect();
        let mut sorted = hs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "distinct hues: {hs:?}");
        assert_eq!(short_name("shop-db-1", Some("shop")), "db-1");
        assert_eq!(short_name("shop", Some("shop")), "shop");
        assert_eq!(short_name("other-1", Some("shop")), "other-1");
        assert_eq!(short_name("db", None), "db");
    }
}
