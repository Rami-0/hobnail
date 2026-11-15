//! The page shell: sidebar (navigation and stack tree), top bar (title,
//! command palette button, vitals chips, theme, account), the palette dialog
//! and the icon sprite. Every page is `layout(ctx, nav, title, heading, main)`.
//!
//! Live parts (`#tree`, `#vitals`, `#nav-al`) are re-rendered by
//! `pages::bundle` and swapped in by the SSE client.

use maud::{html, Markup, PreEscaped, DOCTYPE};
use std::sync::Arc;

use crate::config::Config;
use crate::db::strips::Strips;
use crate::db::{Db, Range, SysMetric};
use crate::model::{Snapshot, SystemStats};
use crate::state::AppState;
use crate::ui::chart::seed;
use crate::ui::fmt::{url_decode, url_encode};
use crate::ui::sprite::{ic, sheet};
use crate::ui::status_dot;
use crate::ui::view::{health_strip, stack_strip, CardView, GroupView, StripStyle};
use crate::util::human_bytes;

/// Runs before first paint: theme, collapsed sidebar, and a `js` class for
/// styles that only make sense with scripting.
const BOOT_JS: &str = "var d=document.documentElement;d.classList.add('js');try{var t=localStorage.getItem('rh-theme');if(t)d.dataset.theme=t;if(localStorage.getItem('rh-side')==='rail')d.dataset.side='rail'}catch(e){}";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Overview,
    Containers,
    History,
    Alerts,
    Settings,
    None,
    /// `/host` (host detail).
    Host,
    /// `/logs` and the per-container / per-stack log pages.
    Logs,
}

/// Everything a page needs to know about the request.
pub struct Ctx<'a> {
    pub state: &'a AppState,
    pub cfg: Arc<Config>,
    /// Host the page was opened on (for links to published ports).
    pub host: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub user: Option<String>,
    pub csrf: Option<String>,
    pub now: u64,
}

impl<'a> Ctx<'a> {
    pub fn new(
        state: &'a AppState,
        host: &'a str,
        path: &'a str,
        query: &'a str,
        headers: &hyper::HeaderMap,
    ) -> Self {
        let user = crate::auth::current_user(state, headers);
        let csrf = user
            .as_ref()
            .and_then(|_| crate::auth::csrf_token(state, headers));
        Self {
            state,
            cfg: state.config.current(),
            host,
            path,
            query,
            user,
            csrf,
            now: crate::util::unix_now(),
        }
    }

    /// A context for live fragments: no request, so no page and no account.
    pub fn live(state: &'a AppState, host: &'a str) -> Self {
        Self {
            state,
            cfg: state.config.current(),
            host,
            path: "",
            query: "",
            user: None,
            csrf: None,
            now: crate::util::unix_now(),
        }
    }

    pub fn site_title(&self) -> String {
        self.cfg
            .page
            .title
            .clone()
            .unwrap_or_else(|| self.state.env.hostname.clone())
    }
}

pub const NAV: &[(Nav, &str, &str, &str)] = &[
    (Nav::Overview, "/", "Overview", "grid"),
    (Nav::Containers, "/containers", "Containers", "box"),
    (Nav::Host, "/host", "Host", "server"),
    (Nav::Logs, "/logs", "Logs", "logs"),
    (Nav::History, "/history", "History", "history"),
    (Nav::Alerts, "/alerts", "Alerts", "bell"),
    (Nav::Settings, "/settings", "Settings", "sliders"),
];

/// Render a full page. `heading` goes in the top bar (a title or breadcrumb);
/// `doc_title` is the `<title>` prefix.
pub fn layout(ctx: &Ctx, nav: Nav, doc_title: &str, heading: Markup, main: Markup) -> String {
    let loaded = ctx.state.config.snapshot();
    if ctx.cfg.system.enabled && ctx.state.system.lock().unwrap().1.is_none() {
        crate::live::refresh_system(ctx.state);
    }
    let site = ctx.site_title();
    let sha = ctx.state.build.sha;
    let theme = match ctx.cfg.page.theme.as_str() {
        "dark" | "light" | "ember" | "contrast" => ctx.cfg.page.theme.as_str(),
        _ => "auto",
    };
    let has_custom_css =
        std::fs::metadata(ctx.state.env.data_dir.join("custom.css")).is_ok_and(|m| m.len() > 0);
    let snap = ctx.state.snapshot();
    let stats = crate::ui::view::stats_with_history(ctx.state, &ctx.state.snapshot());
    let groups = crate::ui::view::build_groups(&ctx.cfg, &snap, &stats, ctx.host);
    let mut sys = ctx.state.system.lock().unwrap().1.clone();
    crate::ui::view::seed_cpu(ctx.state, &mut sys, ctx.now as i64);
    let strips = crate::ui::view::strips(ctx.state, ctx.now as i64);
    let refresh = ctx.cfg.system.refresh.0.as_secs().max(1);
    let active_ctr = ctx.path.strip_prefix("/containers/").map(url_decode);
    let firing = ctx.state.alerts.lock().unwrap().incidents.len();
    let now = ctx.now as i64;
    let markup = html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="dark light";
                meta name="theme-color" content="#0b0e13";
                link rel="manifest" href="/manifest.webmanifest";
                link rel="apple-touch-icon" href="/apple-touch-icon.png";
                @if let Some(t) = &ctx.csrf { meta name="rh-csrf" content=(t); }
                title { (doc_title) " · " (site) " · Hobnail" }
                link rel="icon" href=(crate::ui::FAVICON);
                // Inlined: one render-blocking request fewer.
                style { (PreEscaped(crate::http::server::APP_CSS)) }
                @if has_custom_css { link rel="stylesheet" href="/custom.css"; }
                script { (PreEscaped(BOOT_JS)) }
            }
            body data-started=(ctx.state.started_unix) data-now=(ctx.now) data-sha=(sha)
                 data-search-url=(ctx.cfg.page.search_url) data-path=(ctx.path) data-refresh=(refresh) {
                a.skip href="#main" { "Skip to content" }
                div.app {
                    aside #side.side aria-label="Sidebar" {
                        div.side-head {
                            a.brand href="/" title={"Hobnail · " (site)} {
                                (logo())
                                span.brand-name { "Hobnail" span.brand-site { (site) } }
                            }
                            button.icon-btn.rail-toggle type="button" #rail-toggle
                                aria-label="Collapse sidebar" title="Collapse sidebar" aria-pressed="false" { (ic("panel")) }
                        }
                        nav.nav aria-label="Main" {
                            @for (n, href, label, icon) in NAV {
                                a.nav-item href=(href) title=(label)
                                    aria-current=[(*n == nav).then_some("page")] {
                                    (ic(icon))
                                    span.nav-label { (label) }
                                    @if *n == Nav::Alerts { (nav_alerts(firing)) }
                                }
                            }
                        }
                        div.tree-wrap {
                            div.tree-head {
                                span.label { "Stacks" }
                                (tree_counts(&groups))
                            }
                            div.tree-filter.js-only {
                                (ic("filter"))
                                input #tree-filter type="search" placeholder="Filter containers"
                                    aria-label="Filter stacks and containers" autocomplete="off" spellcheck="false";
                            }
                            (tree_inner(&ctx.cfg, &snap, &groups, active_ctr.as_deref(), Some(&strips)))
                        }
                        div.side-foot {
                            span.live #live title="Live updates" { span.dot.off {} span.live-t { "connecting" } }
                            button.linklike.keys-btn type="button" data-act="keys" title="Keyboard shortcuts (?)" aria-label="Keyboard shortcuts" { (ic("keyboard")) }
                            span.ver title={"built " (ctx.state.build.time)} {
                                "v" (ctx.state.build.version) " · " code { (sha) }
                            }
                        }
                    }
                    div.scrim #scrim {}
                    div.col {
                        header.top {
                            button.icon-btn.menu-btn type="button" #menu-btn aria-label="Open sidebar"
                                aria-controls="side" aria-expanded="false" { (ic("menu")) }
                            div.crumbs { (heading) }
                            button.cmd type="button" #cmd aria-haspopup="dialog" aria-controls="palette" aria-label="Search and commands (Ctrl K)" {
                                (ic("search"))
                                span.cmd-t { "Search or jump to…" }
                                kbd.cmd-k { "Ctrl K" }
                            }
                            (vitals(&ctx.cfg, sys.as_ref(), Some((&ctx.state.db, now))))
                            button.icon-btn #theme type="button" aria-label="Theme: auto. Click to change" title="Theme" {
                                span.th.th-auto { (ic("monitor")) }
                                span.th.th-dark { (ic("moon")) }
                                span.th.th-light { (ic("sun")) }
                                span.th.th-ember { (ic("flame")) }
                                span.th.th-contrast { (ic("contrast")) }
                            }
                            (account(ctx))
                        }
                        main #main tabindex="-1" {
                            @if let Some(e) = &loaded.error {
                                div.banner.bad role="alert" {
                                    (ic("alert"))
                                    div {
                                        strong { "Config error. Still using the last good config." }
                                        " " code.wrap { (e) }
                                    }
                                }
                            }
                            (main)
                        }
                    }
                }
                (bottom_bar(nav, firing))
                (palette_shell())
                (keys_dialog())
                (sheet())
                script src={"/static/app.js?v=" (sha)} defer {}
            }
        }
    };
    markup.into_string()
}

/// Phone navigation (under 700 px): the four places you go most, and
/// "More", which opens the sidebar drawer.
fn bottom_bar(nav: Nav, firing: usize) -> Markup {
    let item = |n: Nav, href: &str, label: &str, icon: &str| {
        html! {
            a.bb-i href=(href) aria-current=[(n == nav).then_some("page")] {
                (ic(icon)) span { (label) }
                @if n == Nav::Alerts && firing > 0 { b.bb-n { (firing) } }
            }
        }
    };
    html! {
        nav.bbar #bbar aria-label="Quick navigation" {
            (item(Nav::Overview, "/", "Overview", "grid"))
            (item(Nav::Containers, "/containers", "Containers", "box"))
            (item(Nav::Logs, "/logs", "Logs", "logs"))
            (item(Nav::Alerts, "/alerts", "Alerts", "bell"))
            button.bb-i type="button" data-act="more" aria-controls="side" { (ic("more")) span { "More" } }
        }
    }
}

/// The `?` cheat sheet (a native `<dialog>`; the script opens it).
fn keys_dialog() -> Markup {
    let row = |k: &[&str], what: &str| {
        html! {
            div.kr { dt { @for (i, x) in k.iter().enumerate() { @if i > 0 { " " } kbd { (x) } } } dd { (what) } }
        }
    };
    html! {
        dialog.keys #keys aria-labelledby="keys-h" {
            div.keys-h {
                h2 #keys-h { "Keyboard shortcuts" }
                button.icon-btn type="button" data-act="keys-close" aria-label="Close" { (ic("x")) }
            }
            dl.keys-l {
                (row(&["g", "o"], "Overview"))
                (row(&["g", "c"], "Containers"))
                (row(&["g", "h"], "Host"))
                (row(&["g", "l"], "Logs"))
                (row(&["g", "a"], "Alerts"))
                (row(&["g", "s"], "Settings"))
                (row(&["j"], "Next row"))
                (row(&["k"], "Previous row"))
                (row(&["↵"], "Open the row"))
                (row(&["l"], "Logs of the focused or open container"))
                (row(&["/"], "Filter this page"))
                (row(&["Ctrl", "K"], "Search and commands"))
                (row(&["?"], "This list"))
                (row(&["Esc"], "Close"))
            }
        }
    }
}

fn logo() -> Markup {
    PreEscaped(crate::ui::pwa::ICON_SVG.to_string())
}

/// The count badge next to "Alerts" in the nav (live region `nav-al`).
pub fn nav_alerts(firing: usize) -> Markup {
    html! {
        span #nav-al.nav-badge hidden[firing == 0] title="Alerts firing" {
            @if firing > 0 { (firing) span.sr { " firing" } }
        }
    }
}

fn account(ctx: &Ctx) -> Markup {
    html! {
        div.menu #account {
            button.icon-btn.menu-btn-acct type="button" aria-haspopup="menu" aria-expanded="false" aria-controls="account-menu"
                aria-label={"Account" @if let Some(u) = &ctx.user { ": " (u) }}
                title=(ctx.user.as_deref().unwrap_or("Account")) {
                (ic("user"))
                @if ctx.user.is_some() { span.signed {} }
            }
            div.menu-body #account-menu role="menu" hidden {
                @if let Some(u) = &ctx.user {
                    div.menu-who { "Signed in as " strong { (u) } }
                    a.menu-item role="menuitem" href="/settings" { (ic("sliders")) "Settings" }
                    button.menu-item role="menuitem" type="button" data-act="logout" { (ic("log-out")) "Sign out" }
                } @else {
                    div.menu-who { "Viewing is open. Changing settings needs a login." }
                    a.menu-item role="menuitem" href={"/login?next=" (url_encode(&next_of(ctx)))} { (ic("log-in")) "Sign in" }
                }
            }
        }
    }
}

fn next_of(ctx: &Ctx) -> String {
    if ctx.path == "/login" || ctx.path.is_empty() {
        return "/".into();
    }
    if ctx.query.is_empty() {
        ctx.path.to_string()
    } else {
        format!("{}?{}", ctx.path, ctx.query)
    }
}

/// The palette dialog; the script fills the list from `/api/palette`.
fn palette_shell() -> Markup {
    html! {
        div.pal #palette hidden {
            div.pal-box role="dialog" aria-modal="true" aria-label="Search and commands" {
                div.pal-in {
                    (ic("search"))
                    input #pal-q type="text" autocomplete="off" spellcheck="false"
                        placeholder="Containers, pages, bookmarks, actions… or search the web"
                        role="combobox" aria-expanded="true" aria-controls="pal-list" aria-autocomplete="list";
                    kbd { "Esc" }
                }
                ul.pal-list #pal-list role="listbox" aria-label="Results" {}
                div.pal-foot {
                    span { kbd { "↑" } kbd { "↓" } " move" }
                    span { kbd { "↵" } " open" }
                    span { kbd { "⇧↵" } " details" }
                    span { kbd { "Esc" } " close" }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- vitals

/// Fraction used, 0..=100, of the first disk (what `df` reports for it).
pub fn disk_percent(s: &SystemStats) -> Option<f64> {
    s.disks.first().and_then(|d| d.used_percent())
}

pub fn mem_percent(s: &SystemStats) -> Option<f64> {
    (s.mem_total > 0).then(|| s.mem_used as f64 * 100.0 / s.mem_total as f64)
}

/// Threshold class for a percentage: `warn` from 80, `bad` from 90.
pub fn level(p: Option<f64>) -> &'static str {
    match p {
        Some(p) if p >= 90.0 => "bad",
        Some(p) if p >= 80.0 => "warn",
        _ => "",
    }
}

/// The three chips in the top bar. `data-k`/`data-v` feed the client-side
/// sparklines; `db` seeds them with the last hour when history is recording.
/// Must return the element `<div id="vitals">`.
pub fn vitals(cfg: &Config, s: Option<&SystemStats>, db: Option<(&Db, i64)>) -> Markup {
    if !cfg.system.enabled {
        return html! { div.vitals #vitals {} };
    }
    let hist =
        |m: SysMetric| db.and_then(|(db, now)| seed(&db.system_series(m, Range::H1, now), 40));
    let chip =
        |key: &str, label: &str, v: Option<f64>, tip: String, color: &str, h: Option<String>| {
            let lv = level(v);
            html! {
                a.chip.(lv) href="/history" title=(tip) {
                    span.chip-l { (label) }
                    b.num { @match v { Some(v) => { (format!("{v:.0}")) "%" } None => { "—" } } }
                    @if !lv.is_empty() { span.sr { " (high)" } }
                    (crate::ui::pages::overview::live_spark(key, v, color, h))
                }
            }
        };
    let (cpu, mem, disk) = match s {
        Some(s) => (s.cpu_percent, mem_percent(s), disk_percent(s)),
        None => (None, None, None),
    };
    let mem_tip = s
        .filter(|s| s.mem_total > 0)
        .map(|s| {
            format!(
                "Memory: {} of {}",
                human_bytes(s.mem_used),
                human_bytes(s.mem_total)
            )
        })
        .unwrap_or_else(|| "Memory".into());
    let disk_tip = s
        .and_then(|s| s.disks.first())
        .filter(|d| d.total > 0)
        .map(|d| {
            format!(
                "Disk {}: {} of {}",
                d.label,
                human_bytes(d.used),
                human_bytes(d.total)
            )
        })
        .unwrap_or_else(|| "Disk".into());
    // History stores memory in bytes; the chip shows percent of total.
    let mem_hist = s.filter(|s| s.mem_total > 0).and_then(|s| {
        let (db, now) = db?;
        let mut series = db.system_series(SysMetric::Mem, Range::H1, now);
        for p in &mut series.points {
            p.1 = p.1 * 100.0 / s.mem_total as f64;
        }
        seed(&series, 40)
    });
    let cpu_tip = s
        .map(|s| {
            format!(
                "CPU · load {:.2} {:.2} {:.2}",
                s.load[0], s.load[1], s.load[2]
            )
        })
        .unwrap_or_else(|| "CPU".into());
    html! {
        div.vitals #vitals role="group" aria-label="Host vitals" {
            (chip("v:cpu", "CPU", cpu, cpu_tip, "--ch-cpu", hist(SysMetric::Cpu)))
            (chip("v:mem", "MEM", mem, mem_tip, "--ch-mem", mem_hist))
            (chip("v:disk", "DISK", disk, disk_tip, "--ch-disk", hist(SysMetric::Disk)))
        }
    }
}

// ---------------------------------------------------------------- tree

fn tree_counts(groups: &[GroupView]) -> Markup {
    let (run, all) = groups.iter().fold((0, 0), |(r, a), g| {
        (r + g.running, a + g.running + g.stopped)
    });
    html! {
        span.tree-cnt.num #tree-cnt title={(run) " of " (all) " containers running"} {
            b { (run) } "/" (all)
        }
    }
}

/// The stack tree (live region). Must return the element `<div id="tree">`.
pub fn tree(cfg: &Config, snap: &Snapshot, groups: &[GroupView]) -> Markup {
    let strips = crate::db::strips::last();
    html! {
        (tree_counts(groups))
        (tree_inner(cfg, snap, groups, None, strips.as_deref()))
    }
}

fn tree_inner(
    cfg: &Config,
    snap: &Snapshot,
    groups: &[GroupView],
    active: Option<&str>,
    strips: Option<&Strips>,
) -> Markup {
    if !cfg.docker.enabled {
        return html! { div.tree #tree { p.tree-note { "Docker discovery is off." } } };
    }
    let stacks: Vec<&GroupView> = groups
        .iter()
        .filter(|g| g.cards.iter().any(|c| c.container))
        .collect();
    html! {
        div.tree #tree data-p=(crate::ui::view::problem_count(groups)) {
            @if let Some(e) = &snap.error {
                p.tree-note.bad title=(e) { (ic("alert")) "Docker unreachable" }
            } @else if snap.fetched_at == 0 {
                p.tree-note { "Connecting to Docker…" }
            }
            @for g in &stacks {
                @let ctrs: Vec<&CardView> = g.cards.iter().filter(|c| c.container).collect();
                @let has_active = active.is_some_and(|a| ctrs.iter().any(|c| c.name == a));
                @let trouble = ctrs.iter().any(|c| matches!(c.label, "unhealthy" | "restarting" | "dead"));
                details.stack data-k=(g.name) open {
                    summary.cur[has_active] {
                        (ic("chev-r"))
                        span.stack-name title=(g.title) { (g.title) }
                        @if trouble { span.dot.bad role="img" aria-label="problem in this stack" {} }
                        span.cnt.num.all-up[g.running == ctrs.len()] title={(g.running) " of " (ctrs.len()) " running"} {
                            (g.running) "/" (ctrs.len())
                        }
                        a.icon-btn.sm.stack-logs href=(crate::logs::stack_logs_url(&g.name))
                            title={"Logs of " (g.title)} aria-label={"Logs of " (g.title)} { (ic("logs")) }
                        @if let Some(st) = strips.filter(|st| !st.by_name.is_empty()) {
                            (health_strip(&stack_strip(st, g), st, &g.title, StripStyle::Runs))
                        }
                    }
                    ul {
                        @for c in ctrs {
                            li.ctr.stopped[!c.running] data-s=(c.search.to_lowercase()) {
                                a href={"/containers/" (url_encode(&c.name))} title=(c.name)
                                    aria-current=[(active == Some(c.name.as_str())).then_some("page")] {
                                    (status_dot(c))
                                    span.nm { (c.title) }
                                    @if !c.running { span.sr { " (" (c.label) ")" } }
                                }
                            }
                        }
                    }
                }
            }
            @if stacks.is_empty() && snap.fetched_at != 0 && snap.error.is_none() {
                p.tree-note { "No containers." }
            }
            p.tree-note.tree-none hidden { "No match." }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Container, StatsMap};
    use crate::ui::view::build_groups;

    fn c(name: &str, project: &str, state: &str) -> Container {
        Container {
            id: format!("{name:0<64}"),
            name: name.into(),
            image: "img".into(),
            state: state.into(),
            project: Some(project.into()),
            service: Some(name.into()),
            ..Default::default()
        }
    }

    fn render_tree(snap: &Snapshot, active: Option<&str>) -> String {
        let cfg = Config::default();
        let groups = build_groups(&cfg, snap, &StatsMap::new(), "h");
        tree_inner(&cfg, snap, &groups, active, None).into_string()
    }

    #[test]
    fn tree_lists_stacks_with_counts_and_escapes() {
        let snap = Snapshot {
            containers: vec![
                c("web", "shop", "running"),
                c("db", "shop", "exited"),
                c("<b>x</b>", "a&b", "running"),
            ],
            fetched_at: 1,
            error: None,
        };
        let html = render_tree(&snap, Some("web"));
        assert!(
            html.starts_with("<div class=\"tree\" id=\"tree\" data-p=\"0\">"),
            "{html}"
        );
        assert!(
            html.contains(r#"<details class="stack" data-k="shop" open>"#),
            "{html}"
        );
        assert!(html.contains(r#"<summary class="cur">"#), "{html}");
        assert!(html.contains(r#"aria-current="page""#));
        assert!(html.contains(">1/2<"), "{html}");
        // Stopped containers are dimmed and say so in text.
        assert!(html.contains(r#"class="ctr stopped""#));
        assert!(html.contains("(stopped)"));
        // Names are escaped; links are percent-encoded.
        assert!(!html.contains("<b>x</b>"));
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(html.contains("/containers/%3Cb%3Ex%3C%2Fb%3E"), "{html}");
    }

    #[test]
    fn tree_states() {
        let html = render_tree(&Snapshot::default(), None);
        assert!(html.contains("Connecting to Docker"));
        let snap = Snapshot {
            error: Some("no socket".into()),
            ..Default::default()
        };
        assert!(render_tree(&snap, None).contains("Docker unreachable"));
        let mut off = Config::default();
        off.docker.enabled = false;
        assert!(tree_inner(&off, &snap, &[], None, None)
            .into_string()
            .contains("discovery is off"));
    }

    #[test]
    fn vitals_levels_and_placeholders() {
        let cfg = Config::default();
        let html = vitals(&cfg, None, None).into_string();
        assert!(html.contains("id=\"vitals\""));
        assert_eq!(html.matches("—").count(), 3, "{html}");
        let s = SystemStats {
            cpu_percent: Some(95.0),
            mem_total: 8 << 30,
            mem_used: 4 << 30,
            ..Default::default()
        };
        let html = vitals(&cfg, Some(&s), None).into_string();
        assert!(html.contains("chip bad"), "{html}");
        assert!(html.contains("95%"));
        assert!(html.contains("(high)"), "never colour only");
        assert!(html.contains("4.0 GiB of 8.0 GiB"));
        let mut off = Config::default();
        off.system.enabled = false;
        assert_eq!(
            vitals(&off, Some(&s), None).into_string(),
            r#"<div class="vitals" id="vitals"></div>"#
        );
    }

    /// Theme token sets parsed from `static/app.css`: `name -> var -> rgb`.
    fn themes() -> Vec<(String, std::collections::HashMap<String, [f64; 3]>)> {
        let css = include_str!("../../static/app.css");
        let mut out = Vec::new();
        for (sel, name) in [
            (":root,:root[data-theme=dark]{", "dark"),
            (":root[data-theme=light]{", "light"),
            (":root[data-theme=ember]{", "ember"),
            (":root[data-theme=contrast]{", "contrast"),
            (
                ":root[data-theme=auto],:root:not([data-theme]){",
                "auto-light",
            ),
        ] {
            let start = css.find(sel).unwrap_or_else(|| panic!("no block {sel}")) + sel.len();
            let body = &css[start..start + css[start..].find('}').unwrap()];
            let mut vars = std::collections::HashMap::new();
            for decl in body.split(';') {
                let Some((k, v)) = decl.trim().split_once(':') else {
                    continue;
                };
                let v = v.trim();
                if let Some(hex) = v.strip_prefix('#').filter(|h| h.len() == 6) {
                    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64;
                    vars.insert(k.trim().to_string(), [c(0), c(2), c(4)]);
                }
            }
            out.push((name.to_string(), vars));
        }
        out
    }

    fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
        let lum = |c: [f64; 3]| {
            let f = |v: f64| {
                let v = v / 255.0;
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// WCAG contrast for every theme: body text 4.5:1 (7:1 in high
    /// contrast), status text and links 4.5:1, UI parts (status dots, input
    /// borders, chart lines) 3:1. Run with `--nocapture` to print the table.
    #[test]
    fn theme_contrast() {
        let ts = themes();
        let light = &ts.iter().find(|t| t.0 == "light").unwrap().1;
        let auto = &ts.iter().find(|t| t.0 == "auto-light").unwrap().1;
        assert_eq!(light, auto, "the OS-light block must equal the light theme");
        for (name, v) in &ts {
            let get = |k: &str| *v.get(k).unwrap_or_else(|| panic!("{name}: no {k}"));
            let body_min = if name == "contrast" { 7.0 } else { 4.5 };
            let mut worst = f64::INFINITY;
            for bg in ["--bg", "--bg-side", "--surface", "--surface-2", "--raised"] {
                for fg in ["--text", "--text-2", "--text-3"] {
                    let r = contrast(get(fg), get(bg));
                    worst = worst.min(r);
                    assert!(
                        r >= body_min,
                        "{name}: {fg} on {bg} is {r:.2}:1, needs {body_min}"
                    );
                }
                for fg in ["--accent-fg", "--ok-fg", "--warn-fg", "--bad-fg"] {
                    if bg == "--surface-2" {
                        continue; // status text never sits on hover surfaces alone
                    }
                    let r = contrast(get(fg), get(bg));
                    assert!(r >= 4.5, "{name}: {fg} on {bg} is {r:.2}:1");
                }
                for ui in [
                    "--ok", "--warn", "--bad", "--accent", "--ctl", "--ch-cpu", "--ch-mem",
                ] {
                    if bg == "--surface-2" {
                        continue;
                    }
                    let r = contrast(get(ui), get(bg));
                    assert!(r >= 3.0, "{name}: {ui} on {bg} is {r:.2}:1 (UI needs 3)");
                }
            }
            let on = contrast(get("--on-accent"), get("--accent-bg"));
            assert!(on >= 4.5, "{name}: button text {on:.2}:1");
            println!(
                "{name:>10}: text {:.1}, text-2 {:.1}, text-3 {:.1} on surface; worst body {worst:.2}; button {on:.1}",
                contrast(get("--text"), get("--surface")),
                contrast(get("--text-2"), get("--surface")),
                contrast(get("--text-3"), get("--surface")),
            );
        }
    }

    #[test]
    fn sprite_names_used_by_the_shell_exist() {
        let sheet = crate::ui::sprite::sheet().into_string();
        for (_, _, _, icon) in NAV {
            assert!(sheet.contains(&format!("id=\"i-{icon}\"")), "{icon}");
        }
        for icon in [
            "panel", "menu", "search", "monitor", "moon", "sun", "user", "log-in", "log-out",
            "sliders", "chev-r", "alert", "cpu", "memory", "disk", "filter", "flame", "contrast",
            "more", "keyboard", "x", "server", "logs",
        ] {
            assert!(sheet.contains(&format!("id=\"i-{icon}\"")), "{icon}");
        }
    }
}
