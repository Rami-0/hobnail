//! `/containers`: a dense, sortable, filterable table. All state lives in the
//! query string (`q`, `state`, `stack`, `sort`, `dir`, `group`), so the page
//! works and can be bookmarked without JavaScript; the script adds instant
//! text filtering, collapsible groups and remembers the view.

use maud::{html, Markup};
use std::cmp::Ordering;

use crate::config::Layout;
use crate::db::strips::Strips;
use crate::model::Snapshot;
use crate::ui::fmt::{parse_query, pct, query_get, url_encode};
use crate::ui::pages::crumb_title;
use crate::ui::pages::overview::live_spark;
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::view::{
    build_groups, health_strip, stack_strip, window_label, CardView, GroupView, StripStyle,
};
use crate::ui::{empty, empty_state, ic, pill, status_dot, Empty};
use crate::util::human_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StateFilter {
    #[default]
    All,
    Running,
    Stopped,
    Problem,
}

impl StateFilter {
    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "stopped" => Self::Stopped,
            "problem" => Self::Problem,
            _ => Self::All,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Problem => "problem",
        }
    }
    fn matches(self, c: &CardView) -> bool {
        match self {
            Self::All => true,
            Self::Running => c.running,
            Self::Stopped => !c.running,
            Self::Problem => c.attention().is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Stack,
    Image,
    Status,
    Cpu,
    Mem,
}

impl Sort {
    const ALL: [(Sort, &'static str); 6] = [
        (Sort::Name, "name"),
        (Sort::Stack, "stack"),
        (Sort::Image, "image"),
        (Sort::Status, "status"),
        (Sort::Cpu, "cpu"),
        (Sort::Mem, "mem"),
    ];
    fn parse(s: &str) -> Self {
        Self::ALL
            .iter()
            .find(|(_, n)| *n == s)
            .map(|(k, _)| *k)
            .unwrap_or_default()
    }
    fn as_str(self) -> &'static str {
        Self::ALL.iter().find(|(k, _)| *k == self).unwrap().1
    }
    /// Numeric columns read best biggest first.
    fn default_desc(self) -> bool {
        matches!(self, Sort::Cpu | Sort::Mem)
    }
}

/// The table's view settings, parsed from the query string.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListQuery {
    pub q: String,
    pub state: StateFilter,
    pub stack: Option<String>,
    pub sort: Sort,
    /// `None`: the column's natural direction.
    pub desc: Option<bool>,
    /// `None`: follow `page.layout` (grid = grouped, list = flat).
    pub group: Option<bool>,
}

impl ListQuery {
    pub fn parse(qs: &str) -> Self {
        let p = parse_query(qs);
        let get = |k| query_get(&p, k).unwrap_or("");
        ListQuery {
            q: get("q").trim().chars().take(100).collect(),
            state: StateFilter::parse(get("state")),
            stack: Some(get("stack"))
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            sort: Sort::parse(get("sort")),
            desc: match get("dir") {
                "desc" => Some(true),
                "asc" => Some(false),
                _ => None,
            },
            group: match get("group") {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            },
        }
    }

    /// Grouped by stack unless the query says otherwise (`page.layout = "list"`
    /// makes flat the default).
    pub fn grouped(&self, cfg: &crate::config::Config) -> bool {
        self.group.unwrap_or(cfg.page.layout == Layout::Grid)
    }

    pub fn is_desc(&self) -> bool {
        self.desc.unwrap_or_else(|| self.sort.default_desc())
    }

    /// Canonical query string (non-default values only), without the `?`.
    pub fn to_query(&self) -> String {
        let mut v: Vec<String> = Vec::new();
        if !self.q.is_empty() {
            v.push(format!("q={}", url_encode(&self.q)));
        }
        if self.state != StateFilter::All {
            v.push(format!("state={}", self.state.as_str()));
        }
        if let Some(s) = &self.stack {
            v.push(format!("stack={}", url_encode(s)));
        }
        if self.sort != Sort::Name {
            v.push(format!("sort={}", self.sort.as_str()));
        }
        if let Some(d) = self.desc {
            if d != self.sort.default_desc() {
                v.push(format!("dir={}", if d { "desc" } else { "asc" }));
            }
        }
        if let Some(g) = self.group {
            v.push(format!("group={}", u8::from(g)));
        }
        v.join("&")
    }

    fn href(&self) -> String {
        let q = self.to_query();
        if q.is_empty() {
            "/containers".into()
        } else {
            format!("/containers?{q}")
        }
    }

    fn with(&self, f: impl FnOnce(&mut ListQuery)) -> String {
        let mut c = self.clone();
        f(&mut c);
        c.href()
    }

    fn matches(&self, c: &CardView) -> bool {
        if !self.state.matches(c) {
            return false;
        }
        if self.stack.as_deref().is_some_and(|s| s != c.stack) {
            return false;
        }
        let hay = c.search.to_lowercase();
        self.q
            .to_lowercase()
            .split_whitespace()
            .all(|t| hay.contains(t))
    }
}

fn status_rank(c: &CardView) -> u8 {
    match c.label {
        "unhealthy" | "restarting" | "dead" => 0,
        "starting" | "paused" => 1,
        "healthy" | "running" => 2,
        _ => 3,
    }
}

fn cmp_card(a: &CardView, b: &CardView, sort: Sort, desc: bool) -> Ordering {
    let name = |c: &CardView| c.title.to_lowercase();
    let num = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) => {
            let o = x.total_cmp(&y);
            if desc {
                o.reverse()
            } else {
                o
            }
        }
        // Rows without data always sink to the bottom.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    let primary = match sort {
        Sort::Cpu => num(a.cpu, b.cpu),
        Sort::Mem => num(a.mem.map(|m| m as f64), b.mem.map(|m| m as f64)),
        Sort::Name => name(a).cmp(&name(b)),
        Sort::Stack => a.stack.to_lowercase().cmp(&b.stack.to_lowercase()),
        Sort::Image => a.image.cmp(&b.image),
        Sort::Status => status_rank(a).cmp(&status_rank(b)),
    };
    let primary = if matches!(sort, Sort::Cpu | Sort::Mem) || !desc {
        primary
    } else {
        primary.reverse()
    };
    primary.then_with(|| name(a).cmp(&name(b)))
}

/// Containers visible under the query, in table order, with the stack that
/// holds each (for grouped mode the stacks keep their configured order).
pub fn select<'a>(
    groups: &'a [GroupView],
    q: &ListQuery,
    grouped: bool,
) -> Vec<(&'a GroupView, Vec<&'a CardView>)> {
    let desc = q.is_desc();
    let mut out: Vec<(&GroupView, Vec<&CardView>)> = Vec::new();
    for g in groups {
        let mut v: Vec<&CardView> = g
            .cards
            .iter()
            .filter(|c| c.container && q.matches(c))
            .collect();
        v.sort_by(|a, b| cmp_card(a, b, q.sort, desc));
        if !v.is_empty() {
            out.push((g, v));
        }
    }
    if !grouped {
        // One flat list: re-sort everything together.
        let mut all: Vec<(&GroupView, &CardView)> = out
            .iter()
            .flat_map(|(g, v)| v.iter().map(move |c| (*g, *c)))
            .collect();
        all.sort_by(|a, b| cmp_card(a.1, b.1, q.sort, desc));
        return all.into_iter().map(|(g, c)| (g, vec![c])).collect();
    }
    out
}

pub fn page(ctx: &Ctx) -> String {
    let q = ListQuery::parse(ctx.query);
    let snap = ctx.state.snapshot();
    let stats = crate::ui::view::stats_with_history(ctx.state, &ctx.state.snapshot());
    let groups = build_groups(&ctx.cfg, &snap, &stats, ctx.host);
    let all: Vec<&CardView> = groups
        .iter()
        .flat_map(|g| g.cards.iter())
        .filter(|c| c.container)
        .collect();
    let stacks = groups
        .iter()
        .filter(|g| g.cards.iter().any(|c| c.container))
        .count();
    let count = |f: StateFilter| all.iter().filter(|c| f.matches(c)).count();
    let seg = |label: &str, f: StateFilter, n: usize| {
        let on = q.state == f;
        html! {
            a href=(q.with(|x| x.state = f)) data-state=(f.as_str()) aria-current=[on.then_some("true")] {
                (label) " " span.n.num data-n=(f.as_str()) { (n) }
            }
        }
    };
    let grouped = q.grouped(&ctx.cfg);
    let body = html! {
        div.page.wide {
            form.bar #cbar method="get" action="/containers" role="search"
                data-default-group=(u8::from(ctx.cfg.page.layout == Layout::Grid)) {
                div.search-in {
                    (ic("search"))
                    input #cq type="search" name="q" value=(q.q) placeholder="Filter by name, stack or image"
                        autocomplete="off" spellcheck="false" aria-label="Filter containers";
                }
                @if q.state != StateFilter::All { input type="hidden" name="state" value=(q.state.as_str()); }
                @if q.sort != Sort::Name { input type="hidden" name="sort" value=(q.sort.as_str()); }
                @if let Some(d) = q.desc { input type="hidden" name="dir" value=(if d { "desc" } else { "asc" }); }
                @if let Some(g) = q.group { input type="hidden" name="group" value=(u8::from(g)); }
                nav.seg #cstate aria-label="State" {
                    (seg("All", StateFilter::All, count(StateFilter::All)))
                    (seg("Running", StateFilter::Running, count(StateFilter::Running)))
                    (seg("Stopped", StateFilter::Stopped, count(StateFilter::Stopped)))
                    (seg("Problems", StateFilter::Problem, count(StateFilter::Problem)))
                }
                select #cstack name="stack" aria-label="Stack" {
                    option value="" selected[q.stack.is_none()] { "All stacks (" (stacks) ")" }
                    @for g in groups.iter().filter(|g| g.cards.iter().any(|c| c.container)) {
                        option value=(g.name) selected[q.stack.as_deref() == Some(g.name.as_str())] { (g.title) }
                    }
                }
                nav.seg #cgroup aria-label="Layout" {
                    a href=(q.with(|x| x.group = Some(true))) data-group="1" aria-current=[grouped.then_some("true")] { (ic("layers")) span { "Stacks" } }
                    a href=(q.with(|x| x.group = Some(false))) data-group="0" aria-current=[(!grouped).then_some("true")] { (ic("menu")) span { "Flat" } }
                }
                button.btn.nojs type="submit" { "Apply" }
            }
            (table(ctx, &snap, &groups, &q))
            (crate::ui::pages::container::live_json(&groups))
        }
    };
    layout(
        ctx,
        Nav::Containers,
        "Containers",
        crumb_title("Containers"),
        body,
    )
}

fn th(q: &ListQuery, label: &str, key: Sort, cls: &str) -> Markup {
    let on = q.sort == key;
    let desc = q.is_desc();
    let next_desc = if on { !desc } else { key.default_desc() };
    let href = q.with(|x| {
        x.sort = key;
        x.desc = Some(next_desc);
    });
    html! {
        th class=(cls) scope="col" data-sort=(key.as_str())
            aria-sort=(if on { if desc { "descending" } else { "ascending" } } else { "none" }) {
            a href=(href) {
                (label)
                svg.i.sort-i aria-hidden="true" { use href=(if on && desc { "#i-arrow-down" } else { "#i-arrow-up" }) {} }
            }
        }
    }
}

const COLS: usize = 9;

/// The table (live region `#ctable`). Rows the query hides sit in a hidden
/// pool so the script can re-filter without a round trip; the script also
/// re-applies the viewer's filter, sort and grouping after every live swap.
pub fn table(ctx: &Ctx, snap: &Snapshot, groups: &[GroupView], q: &ListQuery) -> Markup {
    let grouped = q.grouped(&ctx.cfg);
    let rows = select(groups, q, grouped);
    let shown: usize = rows.iter().map(|(_, v)| v.len()).sum();
    let all: Vec<(usize, &GroupView, &CardView)> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| {
            g.cards
                .iter()
                .filter(|c| c.container)
                .map(move |c| (i, g, c))
        })
        .collect();
    let total = all.len();
    let gi = |g: &GroupView| groups.iter().position(|x| x.name == g.name).unwrap_or(0);
    let visible = |c: &CardView| {
        rows.iter()
            .any(|(_, v)| v.iter().any(|x| std::ptr::eq(*x, c)))
    };
    let show_stats = ctx.cfg.page.show_stats;
    let strips = crate::ui::view::strips(ctx.state, ctx.now as i64);
    html! {
        div #ctable.tablewrap data-total=(total) data-sig=(crate::ui::pages::container::live_sig(groups)) {
            // Docker unreachable: the page's banner says it (shell::docker_banner).
            @if !ctx.cfg.docker.enabled {
                (empty_state("box", "Docker discovery is off", html! { "Set " code { "docker.enabled = true" } " in the config to list containers." }))
            } @else if snap.fetched_at == 0 && snap.error.is_none() {
                (empty_state("refresh", "Connecting to Docker…", html! { "The list appears as soon as the first snapshot arrives." }))
            } @else if total == 0 && snap.error.is_some() {
                (empty(Empty::Error, "", "alert", "No containers to show", html! { "Docker cannot be reached and no earlier list is known." }, None))
            } @else if total == 0 {
                (empty_state("box", "No containers yet", html! { "Hobnail lists the containers Docker runs on this host. Hidden containers and excluded names are not listed." }))
            } @else {
                table.tbl.ctbl.grouped[grouped] {
                    caption.count aria-live="polite" {
                        "Showing " b.shown.num { (shown) } " of " span.num { (total) } " containers"
                    }
                    thead {
                        tr {
                            (th(q, "Name", Sort::Name, "c-name"))
                            (th(q, "Stack", Sort::Stack, "c-stack"))
                            (th(q, "Image", Sort::Image, "c-image"))
                            (th(q, "State", Sort::Status, "c-state"))
                            th.c-hist scope="col" title={"Health over the last " (window_label(&strips))} { (window_label(&strips)) }
                            (th(q, "CPU", Sort::Cpu, "c-num c-cpu"))
                            (th(q, "Memory", Sort::Mem, "c-num c-mem"))
                            th.c-ports scope="col" { "Ports" }
                            th.c-act scope="col" { span.sr { "Actions" } }
                        }
                    }
                    @if grouped {
                        @for (g, cs) in &rows {
                            @let all_in = g.cards.iter().filter(|c| c.container).count();
                            tbody.grp data-k=(g.name) data-gc[g.collapsed] {
                                tr.grp-h {
                                    th colspan=(COLS) scope="rowgroup" {
                                        button.grp-t type="button" aria-expanded="true" title=(g.counts()) {
                                            (ic("chev-d"))
                                            span.grp-n { (g.title) }
                                            span.cnt.num { (g.running) "/" (all_in) }
                                            span.grp-strip { (health_strip(&stack_strip(&strips, g), &strips, &g.title, StripStyle::Runs)) }
                                        }
                                    }
                                }
                                @for c in cs { (row(c, gi(g), g, show_stats, &strips)) }
                            }
                        }
                    } @else {
                        tbody.flat {
                            @for (g, cs) in &rows { @for c in cs { (row(c, gi(g), g, show_stats, &strips)) } }
                        }
                    }
                    // Nothing shown: good news for a state filter on its own,
                    // else no match (OV-24); the script keeps `data-why` true.
                    tbody.none hidden[shown > 0] data-why=(none_why(q)) {
                        tr { td colspan=(COLS) {
                            div.why-stopped { (empty(Empty::Good, "compact", "check", "Nothing is stopped", html! { "Every container is running." }, None)) }
                            div.why-problem { (empty(Empty::Good, "compact", "check", "No problems", html! { "Nothing is unhealthy, restarting or dead." }, None)) }
                            div.why-filter { (empty(Empty::NoMatch, "compact", "search", "No containers match", html! { "Try another name or state." },
                                Some(html! { a.btn.sm href="/containers" data-clear { "Clear filters" } }))) }
                        } }
                    }
                    tbody.pool hidden {
                        @for (i, g, c) in &all {
                            @if !visible(c) { (row(c, *i, g, show_stats, &strips)) }
                        }
                    }
                }
                // Stack strips for the headers the script rebuilds.
                div.gstrips hidden {
                    @for g in groups.iter().filter(|g| g.cards.iter().any(|c| c.container)) {
                        span data-k=(g.name) { (health_strip(&stack_strip(&strips, g), &strips, &g.title, StripStyle::Runs)) }
                    }
                }
            }
        }
    }
}

/// Why the "nothing shown" row is shown, for the query the page was opened with.
fn none_why(q: &ListQuery) -> &'static str {
    match q.state {
        StateFilter::Stopped if q.q.is_empty() && q.stack.is_none() => "stopped",
        StateFilter::Problem if q.q.is_empty() && q.stack.is_none() => "problem",
        _ => "filter",
    }
}

fn row(c: &CardView, gi: usize, g: &GroupView, show_stats: bool, st: &Strips) -> Markup {
    let href = format!("/containers/{}", url_encode(&c.name));
    let stats = show_stats && c.running;
    html! {
        tr.row.rl.stopped[!c.running] data-c=(c.name) data-n=(c.title.to_lowercase()) data-s=(c.search.to_lowercase())
            data-gi=(gi) data-k=(g.name) data-kt=(g.title) data-gc[g.collapsed]
            data-img=(c.image.to_lowercase()) data-st=(status_rank(c)) data-run=(u8::from(c.running))
            data-prob=(u8::from(c.attention().is_some()))
            data-cpu=[c.cpu.filter(|_| stats).map(|v| format!("{v:.3}"))]
            data-mem=[c.mem.filter(|_| stats)] {
            td.c-name {
                (status_dot(c))
                (crate::ui::icon(c.icon.as_deref(), &c.title))
                a.nm.row-link href=(href) title=(c.name) {
                    span.nm-t { (c.title) }
                    @if c.title != c.name { span.sub.mono { (c.name) } }
                }
                @if c.restarts > 0 { (pill("warn", &format!("{}× exit/1h", c.restarts))) }
            }
            td.c-stack data-l="Stack" { a href={"/containers?stack=" (url_encode(&c.stack))} { (g.title) } }
            td.c-image data-l="Image" { span.mono.img title=(c.image) { (c.image) } }
            td.c-state data-l="State" title=(c.state_text.as_deref().unwrap_or("")) {
                span class={"st-l " (c.dot.map(|d| d.class()).unwrap_or("off"))} { (c.label) }
                @if let Some(code) = c.exit_code { " " span.mono.exit title="exit code" { (code) } }
                " " span.st-t { (short_state(c)) }
            }
            td.c-hist data-l=(window_label(st)) { (health_strip(&st.get(&c.name), st, &c.title, StripStyle::Runs)) }

            td.c-num.c-cpu data-l="CPU" {
                @if stats {
                    @if let Some(v) = c.cpu {
                        span.num { (pct(v)) }
                        (live_spark(&format!("c:{}:cpu", c.name), Some(v), "--ch-cpu", None))
                    } @else { span.muted { "—" } }
                }
            }
            td.c-num.c-mem data-l="Memory" {
                @if stats {
                    @if let Some(m) = c.mem {
                        span.num { (human_bytes(m)) }
                        (live_spark(&format!("c:{}:mem", c.name), Some(m as f64), "--ch-mem", None))
                    } @else { span.muted { "—" } }
                }
            }
            td.c-ports data-l="Ports" {
                @for p in c.ports.iter().take(3) {
                    @if let Some(u) = &p.url {
                        a.port href=(u) target="_blank" rel="noopener noreferrer" title=(p.detail) { (p.label) }
                    } @else if p.local {
                        span.port.local title={(p.detail) " (loopback only)"} { (p.label) " " small { "local" } }
                    } @else {
                        span.port.plain title=(p.detail) { (p.label) }
                    }
                }
                @if c.ports.len() > 3 { a.port.more href=(href) title="More ports on the details page" { "+" (c.ports.len() - 3) } }
            }
            // "Logs" first, then "Open" when the service has a link (an empty
            // slot keeps the columns aligned); the name opens the detail page.
            td.c-act {
                div.ra {
                    a.btn.sm href=(crate::logs::logs_url(&c.name)) title="Logs (l)" aria-label={"Logs of " (c.title)} { (ic("logs")) "Logs" }
                    @if let Some(u) = &c.url {
                        a.ia href=(u) target="_blank" rel="noopener noreferrer" title="Open service" aria-label={"Open " (c.title)} { (ic("external")) }
                    } @else {
                        span.ia aria-hidden="true" {}
                    }
                }
            }
        }
    }
}

/// "Up 3 hours" -> "3 hours"; "stopped · 2 hours ago" -> "2 hours ago".
fn short_state(c: &CardView) -> String {
    let t = c.state_text.as_deref().unwrap_or("");
    let t = t.strip_prefix("Up ").unwrap_or(t);
    let t = t.strip_prefix("stopped · ").unwrap_or(t);
    if t == c.label || t == "stopped" {
        String::new()
    } else {
        // "About an hour" reads oddly mid-line.
        let mut s = t.to_string();
        if let Some(f) = s.get(..1) {
            s.replace_range(..1, &f.to_lowercase());
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::{Container, ContainerStats, Snapshot, StatsMap};

    fn ctr(name: &str, project: &str, state: &str) -> Container {
        Container {
            id: format!("{name:0<64}"),
            name: name.into(),
            image: format!("img/{name}"),
            state: state.into(),
            status: if state == "running" {
                "Up 3 hours".into()
            } else {
                "Exited (1) 2 hours ago".into()
            },
            exit_code: (state == "exited").then_some(1),
            project: Some(project.into()),
            ..Default::default()
        }
    }

    fn groups() -> Vec<GroupView> {
        let snap = Snapshot {
            containers: vec![
                ctr("zeta", "b", "running"),
                ctr("alpha", "a", "running"),
                ctr("mid", "a", "exited"),
            ],
            fetched_at: 1,
            error: None,
        };
        let mut stats = StatsMap::new();
        stats.insert(
            snap.containers[0].id.clone(),
            ContainerStats {
                cpu_percent: 50.0,
                mem_bytes: 100,
            },
        );
        stats.insert(
            snap.containers[1].id.clone(),
            ContainerStats {
                cpu_percent: 5.0,
                mem_bytes: 900,
            },
        );
        build_groups(&Config::default(), &snap, &stats, "h")
    }

    fn names(rows: &[(&GroupView, Vec<&CardView>)]) -> Vec<String> {
        rows.iter()
            .flat_map(|(_, v)| v.iter().map(|c| c.name.clone()))
            .collect()
    }

    /// Row actions (DESIGN §11.9): a labelled "Logs" first, "Open" only
    /// with a link (else an empty slot), no Dozzle and no Details (OV-20);
    /// the name is the stretched row link, never a link inside a link.
    #[test]
    fn row_actions_are_logs_then_open() {
        let gs = groups();
        let st = Strips::default();
        let (g, c) = (&gs[0], &gs[0].cards[0]);
        let html = row(c, 0, g, true, &st).into_string();
        assert!(html.contains("class=\"row rl"), "{html}");
        assert!(html.contains("class=\"nm row-link\""), "{html}");
        let acts = &html[html.find("class=\"c-act\"").unwrap()..];
        let logs = acts
            .find(&format!("href=\"/containers/{}/logs\"", c.name))
            .expect("logs");
        assert!(acts.contains(&format!("aria-label=\"Logs of {}\"", c.title)));
        assert!(acts[logs..].contains(">Logs</a>"), "labelled: {acts}");
        assert!(
            acts.contains("<span class=\"ia\" aria-hidden=\"true\"></span>"),
            "no link: empty slot"
        );
        assert_eq!(acts.matches("<a ").count(), 1, "{acts}");
        assert!(!html.contains("Dozzle") && !html.contains("Details of"));
        let mut c2 = c.clone();
        c2.url = Some("http://h:8080/".into());
        let html = row(&c2, 0, g, true, &st).into_string();
        let acts = &html[html.find("class=\"c-act\"").unwrap()..];
        assert!(
            acts.find(">Logs</a>").unwrap() < acts.find("aria-label=\"Open ").unwrap(),
            "Logs first"
        );
    }

    #[test]
    fn query_parsing_and_roundtrip() {
        let q = ListQuery::parse("q=my%20app&state=problem&stack=a%26b&sort=cpu&dir=asc&group=0");
        assert_eq!(q.q, "my app");
        assert_eq!(q.state, StateFilter::Problem);
        assert_eq!(q.stack.as_deref(), Some("a&b"));
        assert_eq!(q.sort, Sort::Cpu);
        assert_eq!(q.desc, Some(false));
        assert_eq!(q.group, Some(false));
        assert!(!q.is_desc());
        let again = ListQuery::parse(&q.to_query());
        assert_eq!(q, again);
        // Defaults are omitted; junk falls back to defaults.
        assert_eq!(ListQuery::parse("").to_query(), "");
        let junk = ListQuery::parse("state=zzz&sort=zzz&dir=zzz&group=7&q=%FF");
        assert_eq!(junk.state, StateFilter::All);
        assert_eq!(junk.sort, Sort::Name);
        assert_eq!(junk.desc, None);
        assert_eq!(junk.group, None);
        // Numeric columns default to descending.
        assert!(ListQuery::parse("sort=mem").is_desc());
        assert_eq!(ListQuery::parse("sort=mem&dir=desc").to_query(), "sort=mem");
        assert_eq!(
            ListQuery::parse("sort=mem&dir=asc").to_query(),
            "sort=mem&dir=asc"
        );
        let long = "x".repeat(500);
        assert_eq!(ListQuery::parse(&format!("q={long}")).q.len(), 100);
    }

    #[test]
    fn filter_and_sort() {
        let gs = groups();
        let q = ListQuery::default();
        assert_eq!(names(&select(&gs, &q, false)), ["alpha", "mid", "zeta"]);
        // Grouped keeps stacks together.
        let rows = select(&gs, &q, true);
        assert_eq!(rows.len(), 2);
        assert_eq!(names(&rows), ["alpha", "mid", "zeta"]);
        let q = ListQuery::parse("state=running");
        assert_eq!(names(&select(&gs, &q, false)), ["alpha", "zeta"]);
        let q = ListQuery::parse("state=stopped");
        assert_eq!(names(&select(&gs, &q, false)), ["mid"]);
        let q = ListQuery::parse("state=problem");
        assert_eq!(
            names(&select(&gs, &q, false)),
            ["mid"],
            "stopped with exit 1"
        );
        let q = ListQuery::parse("stack=a");
        assert_eq!(names(&select(&gs, &q, false)), ["alpha", "mid"]);
        let q = ListQuery::parse("q=ZET");
        assert_eq!(names(&select(&gs, &q, false)), ["zeta"]);
        let q = ListQuery::parse("q=img+mid");
        assert_eq!(
            names(&select(&gs, &q, false)),
            ["mid"],
            "all terms must match"
        );
        // CPU sorts biggest first, rows without data sink.
        let q = ListQuery::parse("sort=cpu");
        assert_eq!(names(&select(&gs, &q, false)), ["zeta", "alpha", "mid"]);
        let q = ListQuery::parse("sort=cpu&dir=asc");
        assert_eq!(names(&select(&gs, &q, false)), ["alpha", "zeta", "mid"]);
        let q = ListQuery::parse("sort=name&dir=desc");
        assert_eq!(names(&select(&gs, &q, false)), ["zeta", "mid", "alpha"]);
        let q = ListQuery::parse("sort=status");
        assert_eq!(names(&select(&gs, &q, false)), ["alpha", "zeta", "mid"]);
    }
}
