//! `[[alerts]]` rules: config, validation, and turning the latest data
//! (container snapshot, HTTP check results, host stats, widget values) into
//! engine observations. Everything here except [`http_check`] is pure.

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

use super::engine::{Obs, Params};
use super::template;
use crate::config::{Config, HttpPolicy};
use crate::model::{Snapshot, SystemStats};
use crate::util::Dur;
use crate::widgets::WidgetOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    Container,
    Http,
    Host,
    Widget,
    /// Lines matching a pattern in container logs (`src/logs/watch.rs`).
    #[serde(rename = "log_match")]
    LogMatch,
}

impl RuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleKind::Container => "container",
            RuleKind::Http => "http",
            RuleKind::Host => "host",
            RuleKind::Widget => "widget",
            RuleKind::LogMatch => "log_match",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    Disk,
    Memory,
    Temp,
    Cpu,
}

/// One `[[alerts]]` entry. Fields that do not apply to the rule's `type`
/// are rejected by [`validate`], so a misplaced key is an error, not a no-op.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: RuleKind,
    /// Channel names to notify. Empty: the incident is only shown on the page.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Set to false to disable the rule without deleting it.
    pub enabled: Option<bool>,
    /// Must be bad continuously this long before firing.
    #[serde(rename = "for")]
    pub for_: Option<Dur>,
    /// Minimum time between two notified firings of the same rule+subject.
    pub cooldown: Option<Dur>,
    /// Firing message template.
    pub message: Option<String>,
    /// Recovery message template; `""` disables recovery messages.
    pub recovery: Option<String>,

    // container
    pub containers: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub on: Option<Vec<String>>,
    pub restarts: Option<u32>,
    pub window: Option<Dur>,

    // http
    pub url: Option<String>,
    /// http and host: how often to check.
    pub interval: Option<Dur>,
    pub timeout: Option<Dur>,
    pub expect_status: Option<u16>,
    pub contains: Option<String>,
    pub failures: Option<u32>,

    // host
    pub metric: Option<Metric>,
    /// Disk label glob (metric = "disk"), default all disks.
    pub disk: Option<String>,

    // host and widget thresholds
    pub above: Option<f64>,
    pub below: Option<f64>,

    // widget
    pub widget: Option<String>,
    pub field: Option<String>,
    pub equals: Option<Value>,
    pub not_equals: Option<Value>,
    pub on_error: Option<bool>,

    // log_match (also uses containers, exclude, window)
    /// Regular expression (see `logs::matcher` for the supported syntax).
    pub pattern: Option<String>,
    /// Compose project whose containers are watched.
    pub stack: Option<String>,
    /// Matching lines within `window` needed to fire (default 1).
    pub count: Option<u32>,
}

pub const CONDITIONS: &[&str] = &["stopped", "exit_nonzero", "unhealthy", "restarting"];
const DEFAULT_ON: &[&str] = &["stopped", "unhealthy", "restarting"];
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(15 * 60);

impl RuleConfig {
    pub fn enabled(&self) -> bool {
        self.enabled != Some(false)
    }

    pub fn params(&self) -> Params {
        let default_for = match self.kind {
            // Rides out `docker compose up` re-creating a container.
            RuleKind::Container => 30,
            _ => 0,
        };
        Params {
            for_secs: self.for_.map(|d| d.0.as_secs()).unwrap_or(default_for),
            cooldown_secs: self
                .cooldown
                .map(|d| d.0)
                .unwrap_or(DEFAULT_COOLDOWN)
                .as_secs(),
        }
    }

    pub fn interval(&self) -> Duration {
        let d = match self.kind {
            RuleKind::Host => 30,
            _ => 60,
        };
        self.interval.map(|d| d.0).unwrap_or(Duration::from_secs(d))
    }

    pub fn timeout(&self) -> Duration {
        self.timeout.map(|d| d.0).unwrap_or(Duration::from_secs(5))
    }

    fn on_has(&self, cond: &str) -> bool {
        match &self.on {
            Some(v) => v.iter().any(|c| c == cond),
            None => DEFAULT_ON.contains(&cond),
        }
    }

    fn restarts(&self) -> u32 {
        self.restarts.unwrap_or(3)
    }

    fn window(&self) -> Duration {
        self.window.map(|d| d.0).unwrap_or(Duration::from_secs(600))
    }

    /// Fields set on this rule, by name (for "does not apply" checks).
    fn set_fields(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut f = |name, set: bool| {
            if set {
                v.push(name)
            }
        };
        f("containers", self.containers.is_some());
        f("exclude", self.exclude.is_some());
        f("on", self.on.is_some());
        f("restarts", self.restarts.is_some());
        f("window", self.window.is_some());
        f("url", self.url.is_some());
        f("interval", self.interval.is_some());
        f("timeout", self.timeout.is_some());
        f("expect_status", self.expect_status.is_some());
        f("contains", self.contains.is_some());
        f("failures", self.failures.is_some());
        f("metric", self.metric.is_some());
        f("disk", self.disk.is_some());
        f("above", self.above.is_some());
        f("below", self.below.is_some());
        f("widget", self.widget.is_some());
        f("field", self.field.is_some());
        f("equals", self.equals.is_some());
        f("not_equals", self.not_equals.is_some());
        f("on_error", self.on_error.is_some());
        f("pattern", self.pattern.is_some());
        f("stack", self.stack.is_some());
        f("count", self.count.is_some());
        v
    }
}

/// Semantic checks for one rule. Messages name the rule.
pub fn validate(r: &RuleConfig, cfg: &Config) -> Result<(), String> {
    let who = format!("alert {:?}", r.name);
    let err = |m: String| Err(format!("{who}: {m}"));
    let allowed: &[&str] = match r.kind {
        RuleKind::Container => &["containers", "exclude", "on", "restarts", "window"],
        RuleKind::Http => &[
            "url",
            "interval",
            "timeout",
            "expect_status",
            "contains",
            "failures",
        ],
        RuleKind::Host => &["metric", "disk", "above", "below", "interval"],
        RuleKind::Widget => &[
            "widget",
            "field",
            "above",
            "below",
            "equals",
            "not_equals",
            "on_error",
        ],
        RuleKind::LogMatch => &[
            "pattern",
            "containers",
            "exclude",
            "stack",
            "count",
            "window",
        ],
    };
    if let Some(f) = r.set_fields().into_iter().find(|f| !allowed.contains(f)) {
        return err(format!(
            "`{f}` does not apply to type = \"{}\" rules",
            r.kind.as_str()
        ));
    }
    for c in &r.channels {
        if !cfg.channels.contains_key(c) {
            let known: Vec<&str> = cfg.channels.keys().map(String::as_str).collect();
            return err(format!(
                "unknown channel {c:?} (configured: {})",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        }
    }
    let p = r.params();
    if p.for_secs > 86_400 {
        return err("`for` must be at most 1d".into());
    }
    if p.cooldown_secs > 7 * 86_400 {
        return err("cooldown must be at most 7d".into());
    }
    for (what, t) in [("message", &r.message), ("recovery", &r.recovery)] {
        if let Some(t) = t {
            template::check(t, template::VARS).or_else(|e| err(format!("{what}: {e}")))?;
        }
    }
    let one_threshold = |names: &[(&str, bool)]| -> Result<(), String> {
        let set: Vec<&str> = names.iter().filter(|(_, s)| *s).map(|(n, _)| *n).collect();
        if set.len() != 1 {
            let all: Vec<&str> = names.iter().map(|(n, _)| *n).collect();
            return Err(format!("{who}: set exactly one of {}", all.join(", ")));
        }
        Ok(())
    };
    match r.kind {
        RuleKind::Container => {
            if let Some(on) = &r.on {
                if on.is_empty() {
                    return err("`on` is empty".into());
                }
                if let Some(c) = on.iter().find(|c| !CONDITIONS.contains(&c.as_str())) {
                    return err(format!(
                        "unknown condition {c:?} in `on` (known: {})",
                        CONDITIONS.join(", ")
                    ));
                }
            }
            if r.restarts == Some(0) {
                return err("restarts must be at least 1".into());
            }
            let w = r.window().as_secs();
            if !(60..=3600).contains(&w) {
                return err(
                    "window must be between 1m and 1h (die events are kept for an hour)".into(),
                );
            }
        }
        RuleKind::Http => {
            let Some(url) = &r.url else {
                return err("`url` is required for http rules".into());
            };
            if !(url.starts_with("http://") || url.starts_with("https://"))
                || url.parse::<hyper::Uri>().is_err()
            {
                return err(format!("invalid url {url:?} (http:// or https://)"));
            }
            if r.interval().as_secs() < 5 {
                return err("interval must be at least 5s".into());
            }
            if r.timeout() < Duration::from_secs(1) || r.timeout() > r.interval() {
                return err("timeout must be at least 1s and not longer than interval".into());
            }
            if let Some(s) = r.expect_status {
                if !(100..=599).contains(&s) {
                    return err(format!("expect_status {s} is not an HTTP status"));
                }
            }
            if r.failures == Some(0) {
                return err("failures must be at least 1".into());
            }
        }
        RuleKind::Host => {
            let Some(m) = r.metric else {
                return err("`metric` is required for host rules (disk, memory, temp, cpu)".into());
            };
            if r.disk.is_some() && m != Metric::Disk {
                return err("`disk` only applies to metric = \"disk\"".into());
            }
            one_threshold(&[("above", r.above.is_some()), ("below", r.below.is_some())])?;
            if r.interval().as_secs() < 5 {
                return err("interval must be at least 5s".into());
            }
        }
        RuleKind::Widget => {
            let Some(w) = &r.widget else {
                return err("`widget` (a [[widgets]] id) is required for widget rules".into());
            };
            if !cfg.widgets.iter().any(|x| &x.id == w) {
                return err(format!("unknown widget {w:?}"));
            }
            if r.field.as_deref().is_none_or(str::is_empty) {
                return err("`field` is required for widget rules".into());
            }
            one_threshold(&[
                ("above", r.above.is_some()),
                ("below", r.below.is_some()),
                ("equals", r.equals.is_some()),
                ("not_equals", r.not_equals.is_some()),
            ])?;
        }
        RuleKind::LogMatch => {
            let Some(p) = r.pattern.as_deref().filter(|p| !p.is_empty()) else {
                return err("`pattern` is required for log_match rules".into());
            };
            if let Err(e) = crate::logs::matcher::Regex::new(p) {
                return err(format!("pattern {p:?}: {e}"));
            }
            if r.containers.as_ref().is_none_or(|c| c.is_empty()) && r.stack.is_none() {
                return err("set `containers` (globs) and/or `stack` for log_match rules".into());
            }
            if let Some(c) = r.count {
                if !(1..=crate::logs::watch::MAX_COUNT).contains(&c) {
                    return err(format!(
                        "count must be between 1 and {}",
                        crate::logs::watch::MAX_COUNT
                    ));
                }
            }
            let w = r.window.map(|d| d.0.as_secs()).unwrap_or(300);
            if !(10..=86_400).contains(&w) {
                return err("window must be between 10s and 1d".into());
            }
        }
    }
    Ok(())
}

/// Observations for one rule from the current data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleObs {
    pub subjects: Vec<(String, Obs)>,
    /// The subject list is complete: subjects not in it are gone ("removed").
    pub complete: bool,
}

/// Container rules. None while the snapshot is unusable (never fetched or
/// Docker errored), so a Docker outage neither fires nor resolves anything.
pub fn eval_containers(r: &RuleConfig, snap: &Snapshot, now: u64) -> Option<RuleObs> {
    if snap.fetched_at == 0 || snap.error.is_some() {
        return None;
    }
    let include = r.containers.as_deref().unwrap_or(&[]);
    let exclude = r.exclude.as_deref().unwrap_or(&[]);
    let window = r.window().as_secs();
    let mut subjects = Vec::new();
    for c in &snap.containers {
        let incl =
            include.is_empty() || include.iter().any(|p| crate::util::glob_match(p, &c.name));
        if !incl || exclude.iter().any(|p| crate::util::glob_match(p, &c.name)) {
            continue;
        }
        let dies = c
            .die_times
            .iter()
            .filter(|&&t| t.saturating_add(window) >= now)
            .count();
        let stopped = matches!(c.state.as_str(), "exited" | "dead");
        let nonzero = stopped && c.exit_code != Some(0);
        let obs = if r.on_has("restarting") && dies >= r.restarts() as usize {
            Obs::bad(
                format!(
                    "restarting ({dies} restarts in {})",
                    crate::util::human_duration(window)
                ),
                dies.to_string(),
            )
        } else if (stopped && r.on_has("stopped")) || (nonzero && r.on_has("exit_nonzero")) {
            let state = match (c.state.as_str(), c.exit_code) {
                ("dead", _) => "dead".to_string(),
                (_, Some(code)) => format!("stopped (exit {code})"),
                _ => "stopped".to_string(),
            };
            let value = c.exit_code.map(|c| c.to_string()).unwrap_or_default();
            Obs::bad(state, value)
        } else if c.health.as_deref() == Some("unhealthy") && r.on_has("unhealthy") {
            Obs::bad("unhealthy", "unhealthy")
        } else {
            Obs::ok(c.state.clone(), c.health.clone().unwrap_or_default())
        };
        subjects.push((c.name.clone(), obs));
    }
    Some(RuleObs {
        subjects,
        complete: true,
    })
}

/// `log_match` rules, from what the logs background task counted. None
/// until it has looked at the containers once.
pub fn eval_log_match(r: &RuleConfig, v: &crate::logs::watch::RuleView) -> RuleObs {
    let count = r.count.unwrap_or(1).max(1) as usize;
    let window = r.window.map(|d| d.0.as_secs()).unwrap_or(300);
    let win = crate::util::human_duration(window);
    let subjects = v
        .subjects
        .iter()
        .map(|(c, n, st)| {
            let obs = match *st {
                "over_cap" => Obs::ok(
                    format!("not watched: more than {} containers to tail", v.cap),
                    "",
                ),
                "not_running" => Obs::ok("not running", ""),
                _ if *n >= count => Obs::bad(
                    format!(
                        "{n}{} matching line{} in {win}",
                        if *n >= crate::logs::watch::MAX_COUNT as usize {
                            "+"
                        } else {
                            ""
                        },
                        if *n == 1 { "" } else { "s" }
                    ),
                    n.to_string(),
                ),
                _ => Obs::ok(
                    format!(
                        "{n} matching line{} in {win}",
                        if *n == 1 { "" } else { "s" }
                    ),
                    n.to_string(),
                ),
            };
            (c.clone(), obs)
        })
        .collect();
    RuleObs {
        subjects,
        complete: true,
    }
}

/// Result of one HTTP check.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckResult {
    pub ok: bool,
    /// "HTTP 200", "HTTP 502", "timed out after 5000ms", ...
    pub detail: String,
    pub value: String,
}

/// Rolling state of one HTTP rule.
#[derive(Debug, Clone, Default)]
pub struct HttpState {
    pub url: String,
    pub consecutive_failures: u32,
    pub last: Option<CheckResult>,
}

impl HttpState {
    pub fn record(&mut self, res: CheckResult) {
        if res.ok {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        }
        self.last = Some(res);
    }
}

pub fn eval_http(r: &RuleConfig, st: &HttpState) -> Option<RuleObs> {
    let last = st.last.as_ref()?;
    let subject = r.url.clone().unwrap_or_default();
    let threshold = r.failures.unwrap_or(1);
    let obs = if !last.ok && st.consecutive_failures >= threshold {
        let mut state = format!("down: {}", last.detail);
        if threshold > 1 {
            state.push_str(&format!(" ({} checks in a row)", st.consecutive_failures));
        }
        Obs::bad(state, last.value.clone())
    } else {
        Obs::ok(format!("up: {}", last.detail), last.value.clone())
    };
    Some(RuleObs {
        subjects: vec![(subject, obs)],
        complete: true,
    })
}

/// Run one HTTP check (GET, no redirects followed). Errors never contain
/// the URL path (see `http::client::send`).
pub async fn http_check(r: &RuleConfig, policy: &HttpPolicy) -> CheckResult {
    let url = r.url.clone().unwrap_or_default();
    let req = crate::http::client::Request::get(url).timeout(r.timeout());
    match crate::http::client::send(req, policy).await {
        Ok(resp) => {
            let status_ok = match r.expect_status {
                Some(s) => resp.status == s,
                None => (200..400).contains(&resp.status),
            };
            let detail = format!("HTTP {}", resp.status);
            if !status_ok {
                return CheckResult {
                    ok: false,
                    detail,
                    value: resp.status.to_string(),
                };
            }
            if let Some(needle) = &r.contains {
                if !resp.text().contains(needle.as_str()) {
                    return CheckResult {
                        ok: false,
                        detail: format!("{detail}, body does not contain {needle:?}"),
                        value: resp.status.to_string(),
                    };
                }
            }
            CheckResult {
                ok: true,
                detail,
                value: resp.status.to_string(),
            }
        }
        Err(e) => CheckResult {
            ok: false,
            detail: e,
            value: "error".into(),
        },
    }
}

fn compare(r: &RuleConfig, v: f64) -> Option<(bool, String)> {
    if let Some(a) = r.above {
        Some((v > a, format!("> {}", fmt_num(a))))
    } else {
        r.below.map(|b| (v < b, format!("< {}", fmt_num(b))))
    }
}

pub fn eval_host(r: &RuleConfig, s: &SystemStats) -> Option<RuleObs> {
    let metric = r.metric?;
    let mut subjects = Vec::new();
    let mut push = |subject: String, v: f64, unit: &str| {
        if let Some((bad, op)) = compare(r, v) {
            let cur = format!("{v:.1}{unit}");
            let state = if bad {
                format!("{cur} {op}{unit}")
            } else {
                cur.clone()
            };
            subjects.push((
                subject,
                Obs {
                    bad,
                    state,
                    value: format!("{v:.1}"),
                },
            ));
        }
    };
    let complete = match metric {
        Metric::Disk => {
            let pat = r.disk.as_deref().unwrap_or("*");
            // Same Use% as the disk tile and `df`.
            for d in s.disks.iter() {
                if let Some(pct) = d.used_percent() {
                    if crate::util::glob_match(pat, &d.label) {
                        push(format!("disk {}", d.label), pct, "%");
                    }
                }
            }
            true
        }
        Metric::Memory => {
            if s.mem_total > 0 {
                push(
                    "memory".into(),
                    s.mem_used as f64 * 100.0 / s.mem_total as f64,
                    "%",
                );
            }
            false
        }
        Metric::Temp => {
            if let Some(t) = s.temp_c {
                push("temperature".into(), t, "°C");
            }
            false
        }
        Metric::Cpu => {
            if let Some(c) = s.cpu_percent {
                push("cpu".into(), c, "%");
            }
            false
        }
    };
    Some(RuleObs { subjects, complete })
}

pub fn eval_widget(r: &RuleConfig, out: &WidgetOutput) -> Option<RuleObs> {
    let subject = r.widget.clone()?;
    let field = r.field.as_deref()?;
    let one = |obs| {
        Some(RuleObs {
            subjects: vec![(subject.clone(), obs)],
            complete: false,
        })
    };
    if let Some(e) = &out.error {
        return if r.on_error == Some(true) {
            one(Obs::bad(format!("widget error: {e}"), "error"))
        } else {
            None
        };
    }
    let v = out.values.get(field)?;
    let shown = value_str(v);
    if let Some(x) = &r.equals {
        let bad = json_eq(v, x);
        let state = format!("{field} = {shown}");
        return one(Obs {
            bad,
            state,
            value: shown,
        });
    }
    if let Some(x) = &r.not_equals {
        let bad = !json_eq(v, x);
        let state = if bad {
            format!("{field} = {shown} (expected {})", value_str(x))
        } else {
            format!("{field} = {shown}")
        };
        return one(Obs {
            bad,
            state,
            value: shown,
        });
    }
    let n = as_num(v)?;
    let (bad, op) = compare(r, n)?;
    let state = if bad {
        format!("{field} {} {op}", fmt_num(n))
    } else {
        format!("{field} {}", fmt_num(n))
    };
    one(Obs {
        bad,
        state,
        value: fmt_num(n),
    })
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(_), _) | (_, Value::Bool(_)) => value_str(a) == value_str(b),
        _ => match (as_num(a), as_num(b)) {
            (Some(x), Some(y)) => x == y,
            _ => value_str(a) == value_str(b),
        },
    }
}

/// `1000`, `92.5`, `0.33`.
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Container, DiskStats};

    fn rule(toml_src: &str) -> RuleConfig {
        toml::from_str(toml_src).unwrap()
    }

    fn container(name: &str, state: &str, exit: Option<i64>, health: Option<&str>) -> Container {
        Container {
            name: name.into(),
            state: state.into(),
            exit_code: exit,
            health: health.map(Into::into),
            ..Default::default()
        }
    }

    fn snap(cs: Vec<Container>) -> Snapshot {
        Snapshot {
            containers: cs,
            fetched_at: 1,
            error: None,
        }
    }

    fn bad_subjects(o: &RuleObs) -> Vec<(&str, &str)> {
        o.subjects
            .iter()
            .filter(|(_, o)| o.bad)
            .map(|(s, o)| (s.as_str(), o.state.as_str()))
            .collect()
    }

    #[test]
    fn container_conditions() {
        let r = rule("name='c'\ntype='container'\non=['stopped','unhealthy']\nexclude=['*-e2e-*']");
        let s = snap(vec![
            container("odoo", "exited", Some(137), None),
            container("web", "running", None, Some("unhealthy")),
            container("ok", "running", None, Some("healthy")),
            container("sa-e2e-web", "exited", Some(1), None),
            container("dead", "dead", None, None),
            container("new", "created", None, None),
        ]);
        let o = eval_containers(&r, &s, 100).unwrap();
        assert!(o.complete);
        assert_eq!(
            bad_subjects(&o),
            vec![
                ("odoo", "stopped (exit 137)"),
                ("web", "unhealthy"),
                ("dead", "dead")
            ]
        );
        assert_eq!(o.subjects.len(), 5, "excluded containers are not subjects");
    }

    #[test]
    fn exit_nonzero_ignores_clean_exits() {
        let r = rule("name='c'\ntype='container'\non=['exit_nonzero']\ncontainers=['job-*']");
        let s = snap(vec![
            container("job-a", "exited", Some(0), None),
            container("job-b", "exited", Some(2), None),
            container("other", "exited", Some(2), None),
        ]);
        let o = eval_containers(&r, &s, 100).unwrap();
        assert_eq!(bad_subjects(&o), vec![("job-b", "stopped (exit 2)")]);
        assert_eq!(o.subjects[1].1.value, "2");
    }

    #[test]
    fn restart_loop_counts_dies_in_window() {
        let r = rule("name='c'\ntype='container'\non=['restarting']\nrestarts=3\nwindow='10m'");
        let now = 10_000;
        let mut c = container("loop", "running", None, None);
        c.die_times = vec![now - 700, now - 300, now - 100]; // only 2 within 10m
        let o = eval_containers(&r, &snap(vec![c.clone()]), now).unwrap();
        assert!(bad_subjects(&o).is_empty());
        c.die_times.push(now - 5);
        let o = eval_containers(&r, &snap(vec![c]), now).unwrap();
        assert_eq!(
            bad_subjects(&o),
            vec![("loop", "restarting (3 restarts in 10m)")]
        );
        // Later, once the dies age out of the window, it is ok again.
        let mut c = container("loop", "running", None, None);
        c.die_times = vec![now - 300, now - 100, now - 5];
        let o = eval_containers(&r, &snap(vec![c]), now + 600).unwrap();
        assert!(bad_subjects(&o).is_empty());
    }

    #[test]
    fn unusable_snapshot_is_no_data() {
        let r = rule("name='c'\ntype='container'");
        assert!(eval_containers(&r, &Snapshot::default(), 1).is_none());
        let mut s = snap(vec![]);
        s.error = Some("docker down".into());
        assert!(eval_containers(&r, &s, 1).is_none());
    }

    #[test]
    fn http_consecutive_failures() {
        let r = rule("name='h'\ntype='http'\nurl='http://x/health'\nfailures=2");
        let mut st = HttpState::default();
        assert!(eval_http(&r, &st).is_none());
        let fail = CheckResult {
            ok: false,
            detail: "HTTP 502".into(),
            value: "502".into(),
        };
        let up = CheckResult {
            ok: true,
            detail: "HTTP 200".into(),
            value: "200".into(),
        };
        st.record(fail.clone());
        assert!(!eval_http(&r, &st).unwrap().subjects[0].1.bad);
        st.record(fail.clone());
        let o = eval_http(&r, &st).unwrap();
        assert_eq!(o.subjects[0].0, "http://x/health");
        assert!(o.subjects[0].1.bad);
        assert_eq!(o.subjects[0].1.state, "down: HTTP 502 (2 checks in a row)");
        st.record(up);
        assert!(!eval_http(&r, &st).unwrap().subjects[0].1.bad);
        st.record(fail);
        assert!(!eval_http(&r, &st).unwrap().subjects[0].1.bad);
    }

    #[test]
    fn host_thresholds() {
        let s = SystemStats {
            mem_total: 1000,
            mem_used: 921,
            disks: vec![
                DiskStats {
                    label: "/".into(),
                    total: 100,
                    used: 95,
                    avail: 5,
                },
                DiskStats {
                    label: "/backup".into(),
                    total: 100,
                    used: 10,
                    avail: 90,
                },
            ],
            temp_c: Some(81.0),
            cpu_percent: None,
            ..Default::default()
        };
        let disk = rule("name='d'\ntype='host'\nmetric='disk'\nabove=90");
        let o = eval_host(&disk, &s).unwrap();
        assert!(o.complete);
        assert_eq!(bad_subjects(&o), vec![("disk /", "95.0% > 90%")]);
        assert_eq!(o.subjects.len(), 2);
        let only = rule("name='d'\ntype='host'\nmetric='disk'\ndisk='/backup'\nbelow=50");
        assert_eq!(
            bad_subjects(&eval_host(&only, &s).unwrap()),
            vec![("disk /backup", "10.0% < 50%")]
        );
        let mem = rule("name='m'\ntype='host'\nmetric='memory'\nabove=90");
        assert_eq!(
            bad_subjects(&eval_host(&mem, &s).unwrap()),
            vec![("memory", "92.1% > 90%")]
        );
        let temp = rule("name='t'\ntype='host'\nmetric='temp'\nabove=85");
        let o = eval_host(&temp, &s).unwrap();
        assert!(bad_subjects(&o).is_empty());
        assert_eq!(o.subjects[0].1.state, "81.0°C");
        let cpu = rule("name='c'\ntype='host'\nmetric='cpu'\nabove=90");
        assert!(
            eval_host(&cpu, &s).unwrap().subjects.is_empty(),
            "no cpu sample yet"
        );
    }

    #[test]
    fn widget_values() {
        let mut out = WidgetOutput::default();
        out.values.insert("unread".into(), serde_json::json!(1234));
        out.values
            .insert("status".into(), serde_json::json!("degraded"));
        out.values
            .insert("str_num".into(), serde_json::json!("12.5"));
        let above = rule("name='w'\ntype='widget'\nwidget='mailpit'\nfield='unread'\nabove=1000");
        assert_eq!(
            bad_subjects(&eval_widget(&above, &out).unwrap()),
            vec![("mailpit", "unread 1234 > 1000")]
        );
        let below = rule("name='w'\ntype='widget'\nwidget='m'\nfield='str_num'\nbelow=10");
        assert!(bad_subjects(&eval_widget(&below, &out).unwrap()).is_empty());
        let eq = rule("name='w'\ntype='widget'\nwidget='m'\nfield='status'\nequals='degraded'");
        assert_eq!(
            bad_subjects(&eval_widget(&eq, &out).unwrap()),
            vec![("m", "status = degraded")]
        );
        let ne = rule("name='w'\ntype='widget'\nwidget='m'\nfield='status'\nnot_equals='ok'");
        assert_eq!(
            bad_subjects(&eval_widget(&ne, &out).unwrap()),
            vec![("m", "status = degraded (expected ok)")]
        );
        let num_eq = rule("name='w'\ntype='widget'\nwidget='m'\nfield='str_num'\nequals=12.5");
        assert!(eval_widget(&num_eq, &out).unwrap().subjects[0].1.bad);
        let missing = rule("name='w'\ntype='widget'\nwidget='m'\nfield='nope'\nabove=1");
        assert!(eval_widget(&missing, &out).is_none());

        let err = WidgetOutput {
            error: Some("timed out".into()),
            ..Default::default()
        };
        assert!(eval_widget(&above, &err).is_none());
        let on_err = rule("name='w'\ntype='widget'\nwidget='m'\nfield='x'\nabove=1\non_error=true");
        assert_eq!(
            bad_subjects(&eval_widget(&on_err, &err).unwrap()),
            vec![("m", "widget error: timed out")]
        );
    }

    #[test]
    fn log_match_validation_and_eval() {
        let cfg = Config::default();
        let ok = rule("name='l'\ntype='log_match'\npattern='(?i)panic|oom'\ncontainers=['web*']\ncount=3\nwindow='5m'");
        assert_eq!(ok.kind, RuleKind::LogMatch);
        validate(&ok, &cfg).unwrap();
        for (src, msg) in [
            (
                "name='l'\ntype='log_match'\ncontainers=['x']",
                "`pattern` is required",
            ),
            (
                "name='l'\ntype='log_match'\npattern='('\ncontainers=['x']",
                "missing )",
            ),
            ("name='l'\ntype='log_match'\npattern='x'", "containers"),
            (
                "name='l'\ntype='log_match'\npattern='x'\nstack='s'\ncount=0",
                "count must be",
            ),
            (
                "name='l'\ntype='log_match'\npattern='x'\nstack='s'\nwindow='2d'",
                "window must be",
            ),
            (
                "name='l'\ntype='log_match'\npattern='x'\nstack='s'\nurl='http://x'",
                "does not apply",
            ),
            ("name='c'\ntype='container'\npattern='x'", "does not apply"),
        ] {
            let e = validate(&rule(src), &cfg).unwrap_err();
            assert!(e.contains(msg), "{src}: {e}");
        }
        let v = crate::logs::watch::RuleView {
            subjects: vec![
                ("web-1".into(), 3, "tailed"),
                ("web-2".into(), 1, "tailed"),
                ("web-3".into(), 0, "over_cap"),
                ("web-4".into(), 0, "not_running"),
            ],
            cap: 20,
        };
        let o = eval_log_match(&ok, &v);
        assert!(o.complete);
        assert_eq!(bad_subjects(&o), vec![("web-1", "3 matching lines in 5m")]);
        assert_eq!(o.subjects[1].1.state, "1 matching line in 5m");
        assert!(o.subjects[2].1.state.contains("more than 20"));
        assert_eq!(o.subjects[3].1.state, "not running");
    }

    #[test]
    fn numbers() {
        assert_eq!(fmt_num(1000.0), "1000");
        assert_eq!(fmt_num(92.5), "92.5");
        assert_eq!(fmt_num(0.333), "0.33");
    }
}
