//! Background log tails for `log_match` alert rules and per-minute level
//! counts (every tailed container is counted). A container is tailed when an enabled `log_match` rule watches it
//! or it matches `[logs] count`; at most `[logs] max_watched` at once,
//! alert-watched containers first. With no rule and an empty `count` this
//! task opens nothing.
//!
//! Each tail starts at "now" (`tail=0&follow=1`), so nothing is counted
//! twice across restarts of the tail; lines are counted and matched, then
//! dropped. Nothing about a line except its level and whether it matched is
//! kept: counts per minute go to the database, match times (no text) stay
//! in memory for the rule's window.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::client::{self, Line, Query};
use super::frame::strip_ansi;
use super::level::{self, Level};
use super::matcher::Regex;
use crate::alerts::{RuleConfig, RuleKind};
use crate::config::Config;
use crate::model::{Container, Snapshot};
use crate::state::AppState;
use crate::util::glob_match;

const RECONCILE: Duration = Duration::from_secs(10);
/// Most match times remembered per rule and container.
pub const MAX_COUNT: u32 = 10_000;

/// A compiled `log_match` rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub re: Regex,
    pub count: u32,
    containers: Vec<String>,
    exclude: Vec<String>,
    stack: Option<String>,
}

impl Rule {
    pub fn from_config(r: &RuleConfig) -> Result<Self, String> {
        let pattern = r.pattern.as_deref().unwrap_or("");
        let re = Regex::new(pattern).map_err(|e| format!("pattern {pattern:?}: {e}"))?;
        Ok(Rule {
            name: r.name.clone(),
            re,
            count: r.count.unwrap_or(1).clamp(1, MAX_COUNT),
            containers: r.containers.clone().unwrap_or_default(),
            exclude: r.exclude.clone().unwrap_or_default(),
            stack: r.stack.clone(),
        })
    }

    pub fn watches(&self, c: &Container) -> bool {
        if let Some(s) = &self.stack {
            if c.project.as_deref() != Some(s.as_str()) {
                return false;
            }
        } else if self.containers.is_empty() {
            return false; // validation requires containers or stack
        }
        let incl =
            self.containers.is_empty() || self.containers.iter().any(|p| glob_match(p, &c.name));
        incl && !self.exclude.iter().any(|p| glob_match(p, &c.name))
    }
}

/// Shared between the tails, the alert engine and the pages.
#[derive(Debug, Default)]
pub struct Shared {
    /// rule -> container -> match times (unix s), newest last, at most `count`.
    matches: HashMap<String, HashMap<String, VecDeque<u64>>>,
    /// Containers tailed now, with the rules that watch them.
    pub watched: Vec<String>,
    /// Containers some rule wants but the cap left out.
    pub over_cap: Vec<String>,
    pub cap: usize,
    /// Rules that failed to compile: (rule, error).
    pub bad_rules: Vec<(String, String)>,
    /// Counts for the current minute: container -> (minute, counts).
    minute: HashMap<String, (i64, [u64; 5])>,
    /// Matched since the alert engine last looked.
    pub dirty: bool,
    /// Lines seen since start (all tails), for measurements.
    pub lines: u64,
    /// The task has reconciled at least once.
    pub ready: bool,
    /// rule -> containers it watches (tailed or not).
    pub rule_targets: HashMap<String, Vec<String>>,
}

pub fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// What a `log_match` rule sees now: (container, matches in window,
/// "tailed" | "over_cap" | "not_running").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleView {
    pub subjects: Vec<(String, usize, &'static str)>,
    pub cap: usize,
}

impl Shared {
    /// Record one match of `rule` in `container` at `now`.
    pub fn record_match(&mut self, rule: &Rule, container: &str, now: u64) {
        let q = self
            .matches
            .entry(rule.name.clone())
            .or_default()
            .entry(container.to_string())
            .or_default();
        q.push_back(now);
        while q.len() > rule.count as usize {
            q.pop_front();
        }
        self.dirty = true;
    }

    pub fn view(&mut self, rule: &str, window: u64, now: u64) -> Option<RuleView> {
        if !self.ready {
            return None;
        }
        let targets = self.rule_targets.get(rule)?.clone();
        let per = self.matches.entry(rule.to_string()).or_default();
        per.retain(|c, _| targets.contains(c));
        let mut subjects = vec![];
        for c in targets {
            let n = per.get_mut(&c).map_or(0, |q| {
                while q.front().is_some_and(|&t| t + window < now) {
                    q.pop_front();
                }
                q.len()
            });
            let st = if self.watched.contains(&c) {
                "tailed"
            } else if self.over_cap.contains(&c) {
                "over_cap"
            } else {
                "not_running"
            };
            subjects.push((c, n, st));
        }
        Some(RuleView {
            subjects,
            cap: self.cap,
        })
    }

    fn count(&mut self, container: &str, lv: Level, now: u64) -> Option<(String, i64, [u64; 5])> {
        let minute = (now as i64) / 60 * 60;
        self.lines += 1;
        let e = self
            .minute
            .entry(container.to_string())
            .or_insert((minute, [0; 5]));
        let mut flushed = None;
        if e.0 != minute {
            flushed = Some((container.to_string(), e.0, e.1));
            *e = (minute, [0; 5]);
        }
        e.1[lv as usize] += 1;
        flushed
    }

    /// Finished minutes (all of them when `all`).
    fn take_minutes(&mut self, now: u64, all: bool) -> Vec<(String, i64, [u64; 5])> {
        let minute = (now as i64) / 60 * 60;
        let mut out = vec![];
        self.minute.retain(|c, (m, counts)| {
            if all || *m < minute {
                out.push((c.clone(), *m, *counts));
                false
            } else {
                true
            }
        });
        out
    }
}

/// The alert engine's view of a rule (None until the task has run once).
pub fn rule_view(rule: &str, window: u64, now: u64) -> Option<RuleView> {
    shared().lock().unwrap().view(rule, window, now)
}

/// Compile the enabled `log_match` rules.
pub fn rules(cfg: &Config) -> (Vec<Rule>, Vec<(String, String)>) {
    let mut ok = vec![];
    let mut bad = vec![];
    for r in cfg
        .alerts
        .iter()
        .filter(|r| r.enabled() && r.kind == RuleKind::LogMatch)
    {
        match Rule::from_config(r) {
            Ok(x) => ok.push(x),
            Err(e) => bad.push((r.name.clone(), e)),
        }
    }
    (ok, bad)
}

/// Which running containers to tail: alert-watched first, then counted,
/// capped. Returns (tail set, over-cap alert containers, rule targets).
pub fn plan(
    cfg: &Config,
    rules: &[Rule],
    snap: &Snapshot,
) -> (Vec<String>, Vec<String>, HashMap<String, Vec<String>>) {
    let cap = cfg.logs.max_watched;
    let mut want: Vec<String> = vec![];
    let mut targets: HashMap<String, Vec<String>> = HashMap::new();
    for r in rules {
        let t: Vec<String> = snap
            .containers
            .iter()
            .filter(|c| r.watches(c))
            .map(|c| c.name.clone())
            .collect();
        for c in &t {
            let running = snap
                .containers
                .iter()
                .any(|x| &x.name == c && x.is_running());
            if running && !want.contains(c) {
                want.push(c.clone());
            }
        }
        targets.insert(r.name.clone(), t);
    }
    let over: Vec<String> = want.iter().skip(cap).cloned().collect();
    want.truncate(cap);
    for c in snap.containers.iter().filter(|c| c.is_running()) {
        if want.len() >= cap {
            break;
        }
        if cfg.logs.count.iter().any(|p| glob_match(p, &c.name)) && !want.contains(&c.name) {
            want.push(c.name.clone());
        }
    }
    (want, over, targets)
}

struct Tail {
    id: String,
    task: tokio::task::JoinHandle<()>,
}

/// The background task (see the module docs).
pub async fn run(state: Arc<AppState>) {
    let mut tails: HashMap<String, Tail> = HashMap::new();
    let mut containers = state.containers.subscribe();
    let mut gen = u64::MAX;
    let mut rules_now: Arc<Vec<Rule>> = Arc::new(vec![]);
    let mut warned_cap: HashSet<String> = HashSet::new();
    let mut last_prune = 0u64;
    loop {
        let now = crate::util::unix_now();
        let cfg = state.config.current();
        let g = state.config.snapshot().generation;
        if g != gen {
            gen = g;
            let (ok, bad) = rules(&cfg);
            for (r, e) in &bad {
                crate::warn!("log_match rule not used", rule = r, error = e);
            }
            rules_now = Arc::new(ok);
            shared().lock().unwrap().bad_rules = bad;
            // Rules changed: restart tails so they use the new rules.
            for (_, t) in tails.drain() {
                t.task.abort();
            }
        }
        let snap = state.snapshot();
        let usable = snap.fetched_at != 0 && snap.error.is_none() && cfg.docker.enabled;
        let (want, over, targets) = if usable {
            plan(&cfg, &rules_now, &snap)
        } else {
            (vec![], vec![], HashMap::new())
        };
        for c in &over {
            if warned_cap.insert(c.clone()) {
                crate::warn!(
                    "log_match: too many containers to watch, not tailing",
                    container = c,
                    cap = cfg.logs.max_watched
                );
            }
        }
        warned_cap.retain(|c| over.contains(c));
        // Stop tails no longer wanted, finished, or of a re-created container.
        tails.retain(|name, t| {
            let id = snap
                .containers
                .iter()
                .find(|c| &c.name == name)
                .map(|c| &c.id);
            let keep = want.contains(name) && id == Some(&t.id) && !t.task.is_finished();
            if !keep {
                t.task.abort();
            }
            keep
        });
        for name in &want {
            if tails.contains_key(name) {
                continue;
            }
            let Some(c) = snap.containers.iter().find(|c| &c.name == name) else {
                continue;
            };
            let task = tokio::spawn(tail_one(state.clone(), c.name.clone(), rules_now.clone()));
            tails.insert(
                name.clone(),
                Tail {
                    id: c.id.clone(),
                    task,
                },
            );
        }
        let flushed = {
            let mut s = shared().lock().unwrap();
            s.watched = want.clone();
            s.over_cap = over;
            s.cap = cfg.logs.max_watched;
            s.rule_targets = targets;
            if usable {
                s.ready = true;
            }
            s.take_minutes(now, false)
        };
        write_counts(&state, flushed).await;
        let dirty = std::mem::take(&mut shared().lock().unwrap().dirty);
        if dirty {
            state.changed("log_match");
        }
        if now >= last_prune + 3600 && state.db.is_enabled() {
            last_prune = now;
            let db = state.db.clone();
            let _ = tokio::task::spawn_blocking(move || db.prune_log_counts(now as i64)).await;
        }
        tokio::select! {
            _ = containers.changed() => {
                // Let a burst of changes settle.
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            _ = tokio::time::sleep(RECONCILE) => {}
        }
    }
}

async fn write_counts(state: &AppState, rows: Vec<(String, i64, [u64; 5])>) {
    if rows.is_empty() || !state.db.is_enabled() || !state.config.current().history.enabled {
        return;
    }
    let db = state.db.clone();
    let _ = tokio::task::spawn_blocking(move || db.record_log_counts(&rows)).await;
}

/// Tail one container from now on: count levels, match rules.
async fn tail_one(state: Arc<AppState>, name: String, rules: Arc<Vec<Rule>>) {
    let socket = state.env.docker_socket.clone();
    let Ok(info) = client::inspect(&socket, &name).await else {
        return;
    };
    if !info.running {
        return;
    }
    let snap = state.snapshot();
    let mine: Vec<&Rule> = match snap.containers.iter().find(|c| c.name == name) {
        Some(c) => rules.iter().filter(|r| r.watches(c)).collect(),
        None => return,
    };
    let q = Query {
        follow: true,
        tail: Some(0),
        since: Some(crate::util::unix_now() as f64),
        until: None,
        cap: None,
    };
    let Ok(mut stream) = client::open(&socket, &info.id, info.tty, &q).await else {
        return;
    };
    let mut buf = Vec::new();
    loop {
        let more = stream.next(&mut buf).await;
        if buf.is_empty() && !more {
            return;
        }
        let now = crate::util::unix_now();
        let mut flushed = vec![];
        {
            let mut s = shared().lock().unwrap();
            for raw in buf.drain(..) {
                let line = Line::from_raw(raw);
                let plain = strip_ansi(&line.text);
                if let Some(f) = s.count(&name, level::detect(&plain), now) {
                    flushed.push(f);
                }
                for r in &mine {
                    if r.re.is_match(&plain) {
                        s.record_match(r, &name, now);
                    }
                }
            }
        }
        write_counts(&state, flushed).await;
        if !more {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(src: &str) -> Rule {
        let rc: RuleConfig = toml::from_str(src).unwrap();
        Rule::from_config(&rc).unwrap()
    }

    fn ctr(name: &str, project: Option<&str>, running: bool) -> Container {
        Container {
            id: format!("id-{name}"),
            name: name.into(),
            state: if running { "running" } else { "exited" }.into(),
            project: project.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn watch_by_glob_and_stack() {
        let r = rule(
            "name='a'\ntype='log_match'\npattern='x'\ncontainers=['web-*']\nexclude=['web-test']",
        );
        assert!(r.watches(&ctr("web-1", None, true)));
        assert!(!r.watches(&ctr("web-test", None, true)));
        assert!(!r.watches(&ctr("db", None, true)));
        let s = rule("name='b'\ntype='log_match'\npattern='x'\nstack='shop'");
        assert!(s.watches(&ctr("db", Some("shop"), true)));
        assert!(!s.watches(&ctr("db", Some("blog"), true)));
    }

    #[test]
    fn plan_caps_and_prioritises_alerts() {
        let mut cfg = Config::default();
        cfg.logs.max_watched = 2;
        cfg.logs.count = vec!["*".into()];
        let snap = Snapshot {
            containers: vec![
                ctr("a", None, true),
                ctr("w1", None, true),
                ctr("w2", None, true),
                ctr("w3", None, true),
                ctr("w4", None, false),
            ],
            fetched_at: 1,
            error: None,
        };
        let r = rule("name='r'\ntype='log_match'\npattern='x'\ncontainers=['w*']");
        let (want, over, targets) = plan(&cfg, &[r], &snap);
        assert_eq!(want, vec!["w1", "w2"]);
        assert_eq!(over, vec!["w3"], "stopped w4 is not over the cap");
        assert_eq!(targets["r"], vec!["w1", "w2", "w3", "w4"]);
        cfg.logs.max_watched = 10;
        let (want, _, _) = plan(&cfg, &[], &snap);
        assert_eq!(
            want,
            vec!["a", "w1", "w2", "w3"],
            "count globs fill the rest"
        );
        cfg.logs.count = vec![];
        assert!(
            plan(&cfg, &[], &snap).0.is_empty(),
            "nothing to do: no tails"
        );
    }

    #[test]
    fn matches_in_window() {
        let r = rule("name='m'\ntype='log_match'\npattern='(?i)error'\ncontainers=['*']\ncount=3\nwindow='1m'");
        let mut s = Shared {
            ready: true,
            watched: vec!["web".into()],
            ..Default::default()
        };
        s.rule_targets
            .insert("m".into(), vec!["web".into(), "far".into()]);
        for t in [100, 110, 120, 130] {
            s.record_match(&r, "web", t);
        }
        // Only `count` times are kept.
        let v = s.view("m", 60, 130).unwrap();
        assert_eq!(
            v.subjects,
            vec![
                ("web".into(), 3, "tailed"),
                ("far".into(), 0, "not_running")
            ]
        );
        let v = s.view("m", 60, 175).unwrap();
        assert_eq!(v.subjects[0].1, 2, "110 aged out");
        assert!(s.view("unknown", 60, 1).is_none());
        let mut fresh = Shared::default();
        assert!(fresh.view("m", 60, 1).is_none(), "not ready yet");
    }

    #[test]
    fn minute_counts_flush_on_rollover() {
        let mut s = Shared::default();
        assert!(s.count("web", Level::Error, 60).is_none());
        assert!(s.count("web", Level::Warn, 119).is_none());
        let f = s.count("web", Level::Info, 120).unwrap();
        assert_eq!(f, ("web".into(), 60, [1, 1, 0, 0, 0]));
        assert!(s.take_minutes(150, false).is_empty());
        assert_eq!(
            s.take_minutes(180, false),
            vec![("web".into(), 120, [0, 0, 1, 0, 0])]
        );
    }
}
