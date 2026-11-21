//! View model for containers and bookmarks, grouped by stack.
//!
//! `build_groups` projects config, a snapshot and stats into plain data
//! for the container and logs APIs.

mod ports;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use ports::binding_link;
pub use ports::{port_views, PortView};

use std::collections::{BTreeMap, HashSet};

use crate::config::{Config, ContainerOverride, Layout};
use crate::model::{Container, Snapshot, StatsMap};
use crate::util::glob_match;
use ports::url_host_part;

const LABEL_HIDE: &str = "hobnail.hide";
const LABEL_NAME: &str = "hobnail.name";
const LABEL_GROUP: &str = "hobnail.group";
const LABEL_ICON: &str = "hobnail.icon";
const LABEL_URL: &str = "hobnail.url";
const BOOKMARKS_GROUP: &str = "Bookmarks";

#[derive(Debug, Clone, PartialEq)]
pub struct GroupView {
    /// Group key (compose project, override or label value).
    pub name: String,
    pub title: String,
    pub icon: Option<String>,
    pub columns: u8,
    pub layout: Layout,
    pub collapsed: bool,
    pub running: usize,
    pub stopped: usize,
    pub links: usize,
    pub cards: Vec<CardView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Ok,
    Warn,
    Bad,
    Off,
}

impl Dot {
    pub fn class(self) -> &'static str {
        match self {
            Dot::Ok => "ok",
            Dot::Warn => "warn",
            Dot::Bad => "bad",
            Dot::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardView {
    /// Container name (empty for bookmarks).
    pub name: String,
    pub image: String,
    /// Group key this card sits in (see `GroupView::name`).
    pub stack: String,
    pub title: String,
    /// Second line: description, or "container · image".
    pub sub: Option<String>,
    pub icon: Option<String>,
    /// Main link of the card.
    pub url: Option<String>,
    /// None for bookmarks.
    pub dot: Option<Dot>,
    /// "Up 3 hours", "stopped · 2 hours ago".
    pub state_text: Option<String>,
    pub health: Option<String>,
    /// Non-zero exit code of a stopped container.
    pub exit_code: Option<i64>,
    /// `die` events in the last hour, when 2 or more.
    pub restarts: usize,
    pub ports: Vec<PortView>,
    /// Live usage; None when stats are off, the container is stopped or not sampled yet.
    pub cpu: Option<f64>,
    pub mem: Option<u64>,
    /// One word for the status column: running, healthy, unhealthy, starting,
    /// restarting, paused, stopped, created, dead.
    pub label: &'static str,
    /// Docker `State` (running, exited, ...).
    pub state: String,
    pub search: String,
    /// Containers only: false when not running.
    pub running: bool,
    pub container: bool,
    pub(crate) order: i32,
}

/// How bad a container's condition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sev {
    Bad,
    Warn,
}

impl CardView {
    /// Why this container needs attention, if it does. A cleanly stopped
    /// container does not: stopped is a state, not a problem.
    pub fn attention(&self) -> Option<(Sev, String)> {
        if !self.container {
            return None;
        }
        match self.label {
            "unhealthy" => return Some((Sev::Bad, "unhealthy".into())),
            "restarting" => return Some((Sev::Bad, "restarting".into())),
            "dead" => return Some((Sev::Bad, "dead".into())),
            "starting" => return Some((Sev::Warn, "starting".into())),
            "paused" => return Some((Sev::Warn, "paused".into())),
            _ => {}
        }
        if let Some(code) = self.exit_code {
            return Some((Sev::Warn, format!("stopped, exit {code}")));
        }
        if self.restarts > 0 {
            return Some((
                Sev::Warn,
                format!("{} exits in the last hour", self.restarts),
            ));
        }
        None
    }
}

/// The card for one container by name, even when the lists hide it (direct
/// links to a hidden container still work). The card's `stack` is its group key.
pub fn card_for(
    cfg: &Config,
    snap: &Snapshot,
    stats: &StatsMap,
    host: &str,
    name: &str,
) -> Option<CardView> {
    let c = snap.containers.iter().find(|c| c.name == name)?;
    let ov = cfg.containers.get(&c.name);
    let mut card = container_card(cfg, c, ov, stats, host);
    card.stack = group_of(cfg, c, ov).to_owned();
    Some(card)
}

/// Build the view model. Pure: no I/O, no clock.
pub fn build_groups(cfg: &Config, snap: &Snapshot, stats: &StatsMap, host: &str) -> Vec<GroupView> {
    let mut containers: BTreeMap<String, Vec<CardView>> = BTreeMap::new();
    if cfg.docker.enabled {
        for c in &snap.containers {
            let ov = cfg.containers.get(&c.name);
            if is_hidden(cfg, c) {
                continue;
            }
            let group = group_of(cfg, c, ov);
            let card = container_card(cfg, c, ov, stats, host);
            containers.entry(group.to_owned()).or_default().push(card);
        }
    }
    let mut bookmarks: BTreeMap<String, Vec<CardView>> = BTreeMap::new();
    for b in &cfg.bookmarks {
        let group = b
            .group
            .clone()
            .filter(|g| !g.is_empty())
            .unwrap_or_else(|| BOOKMARKS_GROUP.into());
        let url = fill(&b.url, host, "", "");
        let sub = b.description.clone().or_else(|| Some(url_host(&url)));
        bookmarks.entry(group).or_default().push(CardView {
            search: format!("{} {}", b.name, url_host(&url)),
            title: b.name.clone(),
            sub,
            icon: b.icon.clone(),
            url: Some(url),
            ..Default::default()
        });
    }

    // Order: configured groups first, then container groups, then
    // bookmark-only groups (each alphabetically).
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for g in &cfg.groups {
        if seen.insert(g.name.clone()) {
            order.push(g.name.clone());
        }
    }
    let mut rest: Vec<&String> = containers.keys().filter(|k| !seen.contains(*k)).collect();
    rest.sort_by_key(|k| k.to_lowercase());
    let rest: Vec<String> = rest.into_iter().cloned().collect();
    seen.extend(rest.iter().cloned());
    order.extend(rest);
    let mut rest_b: Vec<&String> = bookmarks.keys().filter(|k| !seen.contains(*k)).collect();
    rest_b.sort_by_key(|k| k.to_lowercase());
    let rest_b: Vec<String> = rest_b.into_iter().cloned().collect();
    order.extend(rest_b);

    let mut out = Vec::new();
    for name in order {
        let gc = cfg.groups.iter().find(|g| g.name == name);
        if gc.is_some_and(|g| g.hide) {
            continue;
        }
        let mut cards = containers.remove(&name).unwrap_or_default();
        cards.sort_by(|a, b| {
            (a.order, a.title.to_lowercase(), &a.title).cmp(&(
                b.order,
                b.title.to_lowercase(),
                &b.title,
            ))
        });
        let mut links = bookmarks.remove(&name).unwrap_or_default();
        for c in cards.iter_mut().chain(links.iter_mut()) {
            c.stack = name.clone();
        }
        if cards.is_empty() && links.is_empty() {
            continue;
        }
        let running = cards.iter().filter(|c| c.running).count();
        let stopped = cards.len() - running;
        let n_links = links.len();
        cards.extend(links);
        out.push(GroupView {
            title: gc
                .and_then(|g| g.title.clone())
                .unwrap_or_else(|| name.clone()),
            icon: gc.and_then(|g| g.icon.clone()),
            columns: gc.and_then(|g| g.columns).unwrap_or(cfg.page.columns),
            layout: gc.and_then(|g| g.layout).unwrap_or(cfg.page.layout),
            collapsed: gc.is_some_and(|g| g.collapsed),
            name,
            running,
            stopped,
            links: n_links,
            cards,
        });
    }
    out
}

fn label<'a>(c: &'a Container, key: &str) -> Option<&'a str> {
    c.labels
        .get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // Existing container labels keep working; Hobnail labels take priority.
            let suffix = key.strip_prefix("hobnail.")?;
            c.labels
                .get(&format!("rust-homepage.{suffix}"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        })
}

/// Hidden on purpose by the owner (override `hide`, label
/// `hobnail.hide`, `[docker] exclude`), as opposed to merely stopped.
/// Such containers are not reachable through the logs pages or API either.
pub fn is_excluded(cfg: &Config, c: &Container) -> bool {
    let ov = cfg.containers.get(&c.name);
    if let Some(h) = ov.and_then(|o| o.hide) {
        return h;
    }
    if label(c, LABEL_HIDE)
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
    {
        return true;
    }
    cfg.docker.exclude.iter().any(|p| glob_match(p, &c.name))
}

fn is_hidden(cfg: &Config, c: &Container) -> bool {
    (!cfg.page.show_stopped && !c.is_running()) || is_excluded(cfg, c)
}

fn group_of<'a>(cfg: &'a Config, c: &'a Container, ov: Option<&'a ContainerOverride>) -> &'a str {
    ov.and_then(|o| o.group.as_deref())
        .filter(|g| !g.is_empty())
        .or_else(|| label(c, LABEL_GROUP))
        .or_else(|| c.project.as_deref().filter(|p| !p.is_empty()))
        .unwrap_or(&cfg.docker.standalone_group)
}

fn container_title<'a>(c: &'a Container, ov: Option<&'a ContainerOverride>) -> &'a str {
    ov.and_then(|o| o.name.as_deref())
        .filter(|s| !s.is_empty())
        .or_else(|| label(c, LABEL_NAME))
        .or_else(|| c.service.as_deref().filter(|s| !s.is_empty()))
        .unwrap_or(&c.name)
}

/// A visible container's display name without constructing a card or reading stats.
pub(crate) fn suggestion_title<'a>(cfg: &'a Config, c: &'a Container) -> Option<&'a str> {
    if !cfg.docker.enabled || is_hidden(cfg, c) {
        return None;
    }
    let ov = cfg.containers.get(&c.name);
    let group = group_of(cfg, c, ov);
    if cfg
        .groups
        .iter()
        .find(|g| g.name == group)
        .is_some_and(|g| g.hide)
    {
        return None;
    }
    Some(container_title(c, ov))
}

fn container_card(
    cfg: &Config,
    c: &Container,
    ov: Option<&ContainerOverride>,
    stats: &StatsMap,
    host: &str,
) -> CardView {
    let id12: String = c.id.chars().take(12).collect();
    let title = container_title(c, ov).to_owned();
    let sub = ov
        .and_then(|o| o.description.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Some(if title != c.name {
                format!("{} · {}", c.name, c.image)
            } else {
                c.image.clone()
            })
        })
        .filter(|s| !s.is_empty());
    let icon = ov
        .and_then(|o| o.icon.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| label(c, LABEL_ICON).map(str::to_string))
        .or_else(|| {
            cfg.icons
                .auto
                .then(|| crate::http::icons::auto_icon(&c.image, cfg.icons.remote))
                .flatten()
        });
    let ports = port_views(&c.ports, host);
    let url = ov
        .and_then(|o| o.url.clone())
        .filter(|s| !s.is_empty())
        // Labels can come from any image, so only web URLs are accepted there.
        .or_else(|| {
            label(c, LABEL_URL)
                .filter(|u| is_web_url(u))
                .map(str::to_string)
        })
        .map(|u| fill(&u, host, &id12, &c.name))
        .or_else(|| ports.iter().find_map(|p| p.url.clone()));
    let running = c.is_running();
    let live = if cfg.page.show_stats && running {
        stats.get(&c.id)
    } else {
        None
    };
    let mut terms: Vec<&str> = Vec::with_capacity(4);
    for t in [
        title.as_str(),
        c.name.as_str(),
        c.project.as_deref().unwrap_or(""),
        c.image.as_str(),
    ] {
        if !t.is_empty() && !terms.contains(&t) {
            terms.push(t);
        }
    }
    let search = terms.join(" ");
    CardView {
        sub,
        icon,
        url,
        dot: Some(dot_for(c)),
        state_text: Some(state_text(c)),
        health: c.health.clone(),
        exit_code: c.exit_code.filter(|&n| n != 0 && c.state == "exited"),
        restarts: if c.die_times.len() >= 2 {
            c.die_times.len()
        } else {
            0
        },
        ports,
        cpu: live.map(|s| s.cpu_percent.max(0.0)),
        mem: live.map(|s| s.mem_bytes),
        label: status_label(c),
        state: c.state.clone(),
        name: c.name.clone(),
        image: c.image.clone(),
        stack: String::new(),
        search,
        running,
        container: true,
        order: ov.and_then(|o| o.order).unwrap_or(0),
        title,
    }
}

/// Honest status: green only when running and not failing a healthcheck;
/// a stopped container is grey, not red.
pub fn dot_for(c: &Container) -> Dot {
    match c.state.as_str() {
        "running" => match c.health.as_deref() {
            Some("unhealthy") => Dot::Bad,
            Some("starting") => Dot::Warn,
            _ => Dot::Ok,
        },
        "paused" | "restarting" => Dot::Warn,
        _ => Dot::Off,
    }
}

/// One word for the status column (never colour only).
pub fn status_label(c: &Container) -> &'static str {
    match (c.state.as_str(), c.health.as_deref()) {
        ("running", Some("unhealthy")) => "unhealthy",
        ("running", Some("starting")) => "starting",
        ("running", Some("healthy")) => "healthy",
        ("running", _) => "running",
        ("restarting", _) => "restarting",
        ("paused", _) => "paused",
        ("exited", _) => "stopped",
        ("created", _) => "created",
        _ => "dead",
    }
}

fn state_text(c: &Container) -> String {
    match c.state.as_str() {
        "exited" => {
            // "Exited (137) 3 hours ago" → "stopped · 3 hours ago"
            let ago = c
                .status
                .split_once(") ")
                .map(|(_, rest)| rest.trim())
                .filter(|s| !s.is_empty());
            match ago {
                Some(a) => format!("stopped · {a}"),
                None => "stopped".into(),
            }
        }
        "created" => "created, never started".into(),
        "dead" => "dead".into(),
        _ => {
            let s = c
                .status
                .replace(" (healthy)", "")
                .replace(" (unhealthy)", "")
                .replace(" (health: starting)", "");
            if s.is_empty() {
                c.state.clone()
            } else {
                s
            }
        }
    }
}

/// The configured link of a container (override `url`, else a web-URL
/// `hobnail.url` label), with `{id}` and `{name}` filled and `{host}` kept
/// for the viewer to fill in; `None` when nothing is configured (the link is
/// then chosen from the published ports). The JSON API's `link`.
pub fn link_template(cfg: &Config, c: &Container) -> Option<String> {
    let id12: String = c.id.chars().take(12).collect();
    cfg.containers
        .get(&c.name)
        .and_then(|o| o.url.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            label(c, LABEL_URL)
                .filter(|u| is_web_url(u))
                .map(str::to_string)
        })
        .map(|u| u.replace("{id}", &id12).replace("{name}", &c.name))
}

/// Fill a link template: `{host}`, `{id}` (12 chars), `{name}`.
fn fill(tpl: &str, host: &str, id12: &str, name: &str) -> String {
    tpl.replace("{host}", &url_host_part(host))
        .replace("{id}", id12)
        .replace("{name}", name)
}

fn is_web_url(u: &str) -> bool {
    let l = u.get(..8).unwrap_or(u).to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// `https://github.com/x` → `github.com`.
fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

/// Live per-container stats, filled in from the history database for running
/// containers the live sampler has no reading for yet (it needs a viewer and
/// two samples). Keeps first paint from showing "—" everywhere. The DB is
/// only queried when something is missing.
pub fn stats_with_history(state: &crate::state::AppState, snap: &Snapshot) -> StatsMap {
    let mut stats = state.container_stats.lock().unwrap().1.clone();
    let missing = snap
        .containers
        .iter()
        .any(|c| c.is_running() && !stats.contains_key(&c.id));
    if !missing || !state.db.is_enabled() {
        return stats;
    }
    let since = crate::util::unix_now() as i64 - 90;
    let latest = state.db.latest_containers(since);
    for c in snap.containers.iter().filter(|c| c.is_running()) {
        if stats.contains_key(&c.id) {
            continue;
        }
        if let Some(&(cpu, mem)) = latest.get(&c.name) {
            stats.insert(
                c.id.clone(),
                crate::model::ContainerStats {
                    cpu_percent: cpu,
                    mem_bytes: mem,
                },
            );
        }
    }
    stats
}

/// Default health-strip window: 96 cells of 15 min (24 h).
pub const STRIP_BUCKETS: usize = 96;
pub const STRIP_STEP: i64 = 900;

/// The 24 h strips for every container (one cached query; see
/// `db::strips`). Shared across the JSON read endpoints.
pub fn strips(
    state: &crate::state::AppState,
    now: i64,
) -> std::sync::Arc<crate::db::strips::Strips> {
    state.db.health_strips(now, STRIP_BUCKETS, STRIP_STEP)
}

/// A stack's strip: its containers merged (worst of, one running is ok).
pub fn stack_strip(st: &crate::db::strips::Strips, g: &GroupView) -> crate::db::strips::Strip {
    let parts: Vec<crate::db::strips::Strip> = g
        .cards
        .iter()
        .filter(|c| c.container)
        .map(|c| st.get(&c.name))
        .collect();
    crate::db::strips::Strip::merge(st.buckets(), parts.iter())
}

/// The newest system sample's CPU when the live sampler has none yet (it
/// needs two readings): keeps first paint from showing "—".
pub fn seed_cpu(
    state: &crate::state::AppState,
    s: &mut Option<crate::model::SystemStats>,
    now: i64,
) {
    if let Some(s) = s.as_mut() {
        if s.cpu_percent.is_none() {
            s.cpu_percent = state.db.latest_cpu(now - 120);
        }
    }
}
