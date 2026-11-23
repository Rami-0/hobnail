//! Notifications: channels (generic webhook, Slack, Telegram), rules
//! (container, http, host, widget), dedupe, `for`, cooldown, recovery
//! messages, retries and a small persisted state file.
//!
//! Nothing is sent anywhere unless a channel *and* a rule naming it are
//! configured. Config (in the main TOML file):
//!
//! ```toml
//! # ---- channels: named destinations ------------------------------------
//! [channels.ops-webhook]
//! type = "webhook"
//! url_env = "RH_WEBHOOK_URL"        # or url_file = "/data/secrets/webhook",
//!                                   # or url = "http://..." (only for URLs that are not secret)
//! # headers = { "X-Source" = "hobnail" }
//! # headers_env = { Authorization = "RH_WEBHOOK_AUTH" }   # header value from an env var
//! # body = '{"title":"{{title}}","text":"{{message}}"}'  # JSON template, values JSON-escaped.
//! #   Default body: {"kind","title","message","rule","subject","state","value","time","host"}
//! # timeout = "10s"
//!
//! [channels.slack]
//! type = "slack"                    # incoming webhook, sends {"text": message}
//! webhook_url_env = "RH_SLACK_WEBHOOK_URL"   # or webhook_url_file
//!
//! [channels.telegram]
//! type = "telegram"                 # plain text, no parse_mode
//! token_env = "RH_TELEGRAM_TOKEN"   # or token_file
//! chat_id = "123456"                # number or "@channel"; or chat_id_env
//! # api_base = "https://api.telegram.org"
//!
//! # ---- rules -------------------------------------------------------------
//! [[alerts]]
//! name = "container-down"
//! type = "container"
//! containers = ["*"]                # globs on the container name (default all)
//! exclude = ["*-e2e-*"]
//! on = ["stopped", "exit_nonzero", "unhealthy", "restarting"]  # default: stopped, unhealthy, restarting
//! restarts = 3                      # "restarting" = at least this many dies ...
//! window = "10m"                    # ... within this window (1m..1h)
//! for = "30s"                       # must hold this long before firing (default 30s for containers, else 0)
//! cooldown = "15m"                  # min time between two notified firings of rule+subject (default 15m)
//! channels = ["ops-webhook", "slack", "telegram"]   # empty: shown on the page only
//! # message = "🔴 {{subject}}: {{state}} ({{rule}} on {{host}}, {{time}})"
//! # recovery = "✅ {{subject}}: {{state}} after {{duration}} ({{rule}} on {{host}}, {{time}})"
//! # recovery = ""                   # no recovery messages
//! # enabled = false
//!
//! [[alerts]]
//! name = "odoo-up"
//! type = "http"
//! url = "http://100.64.0.10:8070/web/health"
//! interval = "60s"
//! timeout = "5s"
//! expect_status = 200               # default: any 2xx/3xx
//! # contains = "pass"               # body must contain this text
//! failures = 2                      # consecutive failed checks before it counts as down (default 1)
//! channels = ["slack"]
//!
//! [[alerts]]
//! name = "disk"
//! type = "host"
//! metric = "disk"                   # disk | memory | cpu (percent) | temp (°C)
//! # disk = "/"                      # disk label glob (default: every disk)
//! above = 90                        # or below =
//! for = "5m"
//! # interval = "30s"
//! channels = ["telegram"]
//!
//! [[alerts]]
//! name = "mail-backlog"
//! type = "widget"
//! widget = "mailpit"                # a [[widgets]] id
//! field = "unread"                  # key in the widget's extracted values
//! above = 1000                      # or below =, equals =, not_equals =
//! # on_error = true                 # also alert when the widget fails
//! channels = ["ops-webhook"]
//!
//! [[alerts]]
//! name = "app-errors"
//! type = "log_match"                # lines in container logs (src/logs/watch.rs)
//! pattern = '(?i)\b(panic|fatal)\b' # regex subset: see src/logs/matcher.rs
//! containers = ["shop-*"]           # name globs, and/or
//! # stack = "shop"                  # every container of a compose project
//! # exclude = ["shop-test"]
//! count = 3                         # matching lines ...
//! window = "5m"                     # ... within this window (10s..1d); default 1 in 5m
//! channels = ["telegram"]
//! ```
//!
//! `log_match` rules tail only the containers they watch, at most
//! `[logs] max_watched` (default 20) at once; the rest show "not watched".
//! Messages carry the count, never the log text.
//!
//! Template variables: `{{rule}}`, `{{subject}}` (container name, URL,
//! "disk /", "memory", "cpu", "temperature", widget id), `{{state}}`
//! ("stopped (exit 137)", "unhealthy", "down: HTTP 502", "92.1% > 90%",
//! "recovered", "removed"), `{{value}}`, `{{time}}` (UTC ISO 8601),
//! `{{host}}`, `{{duration}}` (how long it was bad), `{{kind}}`
//! (firing / recovery / test), `{{title}}` and, in webhook bodies,
//! `{{message}}`.
//!
//! Secrets come only from environment variables or files and are never
//! logged or rendered. Channels obey `[http] allow_hosts`. State lives in
//! `{data_dir}/alerts-state.json`. Test a channel with
//! `hobnail notify-test <channel>`.

mod channels;
mod engine;
mod rules;
mod template;
#[cfg(test)]
mod tests;

pub use channels::{ChannelConfig, ChannelKind};
pub use rules::{RuleConfig, RuleKind};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Notify};

use crate::config::Config;
use crate::model::{Snapshot, SystemStats};
use crate::state::AppState;
use crate::util::unix_now;
use engine::{Engine, Event, EventKind, Record};
use rules::{CheckResult, HttpState};
use template::Vars;

const STATE_FILE: &str = "alerts-state.json";
/// A failed notification is retried this often...
const RETRY_EVERY: u64 = 60;
/// ...until it is this old, then dropped (and counted).
const GIVE_UP_AFTER: u64 = 6 * 3600;
const MAX_OUTBOX: usize = 200;
/// Attempts per delivery round, with these pauses in between.
const ATTEMPTS: usize = 3;
#[cfg(not(test))]
const BACKOFF_MS: [u64; ATTEMPTS - 1] = [1000, 3000];
#[cfg(test)]
const BACKOFF_MS: [u64; ATTEMPTS - 1] = [10, 20];
/// Re-evaluation cadence while a `for` timer or cooldown is running...
const TICK_BUSY: u64 = 5;
/// ...and otherwise (data changes wake the loop anyway).
const TICK_IDLE: u64 = 60;
const CHANNEL_CHECK_EVERY: u64 = 60;

const DEFAULT_FIRING: &str = "🔴 {{subject}}: {{state}} ({{rule}} on {{host}}, {{time}})";
const DEFAULT_RECOVERY: &str =
    "✅ {{subject}}: {{state}} after {{duration}} ({{rule}} on {{host}}, {{time}})";

// ---------------------------------------------------------------- status

/// Current alert incidents and channel health.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Status {
    /// Enabled rules.
    pub rules: usize,
    pub channels: BTreeMap<String, ChannelStatus>,
    /// Open incidents, oldest first.
    pub incidents: Vec<Incident>,
    /// Notifications waiting to be delivered (retrying).
    pub pending: usize,
    /// Problem with the state file, if any.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChannelStatus {
    pub kind: ChannelKind,
    pub health: Health,
    /// Last error (failing) or the configuration problem (misconfigured).
    /// Never contains a secret.
    pub error: Option<String>,
    pub last_success: Option<u64>,
    pub last_failure: Option<u64>,
    pub sent: u64,
    pub failed: u64,
    /// Notifications given up on (too old, or the queue overflowed).
    pub dropped: u64,
    pub pending: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    /// The last delivery succeeded.
    Ok,
    /// The last delivery failed.
    Failing,
    /// A secret is missing or unreadable.
    Misconfigured,
    /// Nothing sent yet.
    Idle,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Incident {
    pub rule: String,
    pub subject: String,
    pub state: String,
    pub value: String,
    /// Unix seconds when it went bad.
    pub since: u64,
    /// False while suppressed by the cooldown.
    pub notified: bool,
}

// ---------------------------------------------------------------- validate

pub fn validate(cfg: &Config) -> Result<(), String> {
    for (name, c) in &cfg.channels {
        channels::validate(name, c)?;
    }
    let mut seen = HashSet::new();
    for r in &cfg.alerts {
        if r.name.trim().is_empty() {
            return Err("[[alerts]] entry without a name".into());
        }
        if !seen.insert(r.name.as_str()) {
            return Err(format!("duplicate alert name {:?}", r.name));
        }
        rules::validate(r, cfg)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- core state

/// A notification waiting to be delivered to one channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Delivery {
    id: u64,
    channel: String,
    vars: Vars,
    created: u64,
    #[serde(default)]
    attempts: u32,
    #[serde(default)]
    next_try: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ChanStats {
    sent: u64,
    failed: u64,
    dropped: u64,
    last_success: Option<u64>,
    last_failure: Option<u64>,
    last_error: Option<String>,
}

/// The state file.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    entries: Vec<Record>,
    outbox: Vec<Delivery>,
    channels: BTreeMap<String, ChanStats>,
    next_id: u64,
}

struct Core {
    engine: Engine,
    outbox: VecDeque<Delivery>,
    chans: BTreeMap<String, ChanStats>,
    next_id: u64,
    dirty: bool,
    /// Channel name -> why its secrets cannot be resolved.
    misconfig: BTreeMap<String, String>,
    file_error: Option<String>,
    path: PathBuf,
    /// History events (alert.fired / resolved / delivery_failed) waiting to be
    /// written to the database; see `flush_history`.
    history: Vec<(&'static str, &'static str, String, String)>,
}

impl Core {
    /// Write the queued history events to the database.
    fn flush_history(&mut self, db: &crate::db::Db, now: u64) {
        for (kind, level, subject, detail) in self.history.drain(..) {
            db.record_event(now as i64, kind, level, &subject, &detail);
        }
    }

    fn load(path: PathBuf) -> Self {
        let saved = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_slice::<Saved>(text.as_bytes()) {
                Ok(s) => s,
                Err(e) => {
                    crate::warn!(
                        "alerts state file is corrupt, starting fresh",
                        path = path.display(),
                        error = e
                    );
                    Saved::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => {
                crate::warn!(
                    "cannot read alerts state file, starting fresh",
                    path = path.display(),
                    error = e
                );
                Saved::default()
            }
        };
        let next_id = saved
            .outbox
            .iter()
            .map(|d| d.id + 1)
            .max()
            .unwrap_or(0)
            .max(saved.next_id);
        Self {
            engine: Engine::from_records(saved.entries),
            outbox: saved.outbox.into(),
            chans: saved.channels,
            next_id,
            dirty: false,
            misconfig: BTreeMap::new(),
            file_error: None,
            path,
            history: Vec::new(),
        }
    }

    fn save_if_dirty(&mut self) {
        let engine_dirty = self.engine.take_dirty();
        if !(self.dirty || engine_dirty) {
            return;
        }
        self.dirty = false;
        let saved = Saved {
            version: 1,
            entries: self.engine.to_records(),
            outbox: self.outbox.iter().cloned().collect(),
            channels: self.chans.clone(),
            next_id: self.next_id,
        };
        let text = serde_json::to_string(&saved).unwrap_or_default();
        match write_atomic(&self.path, &text) {
            Ok(()) => self.file_error = None,
            Err(e) => {
                if self.file_error.as_deref() != Some(e.as_str()) {
                    crate::warn!("cannot write alerts state file", error = e);
                }
                self.file_error = Some(e);
                self.dirty = true; // retry next cycle
            }
        }
    }

    fn enqueue(&mut self, rule: Option<&RuleConfig>, ev: &Event, host: &str, now: u64) {
        crate::info!(
            if ev.kind == EventKind::Firing {
                "alert firing"
            } else {
                "alert recovered"
            },
            rule = ev.rule,
            subject = ev.subject,
            state = ev.state
        );
        let (hk, hl) = if ev.kind == EventKind::Firing {
            ("alert.fired", "warn")
        } else {
            ("alert.resolved", "info")
        };
        // Subject is what the alert is about (usually a container name), so
        // it shows on that container's timeline; the rule is in the detail.
        let mut detail = format!("{}: {}", ev.rule, ev.state);
        if !ev.value.is_empty() {
            detail.push_str(&format!(" ({})", ev.value));
        }
        self.history.push((hk, hl, ev.subject.clone(), detail));
        let Some(rule) = rule else { return };
        let Some(vars) = event_vars(rule, ev, host) else {
            return;
        };
        for ch in &rule.channels {
            while self.outbox.len() >= MAX_OUTBOX {
                if let Some(old) = self.outbox.pop_front() {
                    self.chans.entry(old.channel).or_default().dropped += 1;
                }
            }
            self.outbox.push_back(Delivery {
                id: self.next_id,
                channel: ch.clone(),
                vars: vars.clone(),
                created: now,
                attempts: 0,
                next_try: 0,
            });
            self.next_id += 1;
            self.dirty = true;
        }
    }

    /// Drop deliveries that cannot or should not be sent any more, then
    /// return the due ones, per channel, in order. A channel whose oldest
    /// delivery is waiting for its retry time is skipped entirely (FIFO).
    fn take_due(&mut self, cfg: &Config, now: u64) -> Vec<(String, ChannelConfig, Vec<Delivery>)> {
        let chans = &mut self.chans;
        let before = self.outbox.len();
        self.outbox.retain(|d| {
            if !cfg.channels.contains_key(&d.channel) {
                return false;
            }
            if now >= d.created + GIVE_UP_AFTER {
                crate::warn!(
                    "giving up on a notification",
                    channel = d.channel,
                    attempts = d.attempts
                );
                chans.entry(d.channel.clone()).or_default().dropped += 1;
                return false;
            }
            true
        });
        if self.outbox.len() != before {
            self.dirty = true;
        }
        let mut blocked = HashSet::new();
        let mut due: BTreeMap<&str, Vec<Delivery>> = BTreeMap::new();
        for d in &self.outbox {
            if blocked.contains(d.channel.as_str()) {
                continue;
            }
            if d.next_try > now {
                blocked.insert(d.channel.as_str());
                continue;
            }
            let v = due.entry(d.channel.as_str()).or_default();
            if v.len() < 20 {
                v.push(d.clone());
            }
        }
        due.into_iter()
            .filter_map(|(name, items)| {
                let c = cfg.channels.get(name)?.clone();
                Some((name.to_string(), c, items))
            })
            .collect()
    }

    /// Earliest retry time of a channel's oldest delivery.
    fn next_try(&self) -> Option<u64> {
        let mut seen = HashSet::new();
        self.outbox
            .iter()
            .filter(|d| seen.insert(d.channel.as_str()))
            .map(|d| d.next_try)
            .min()
    }

    fn apply(&mut self, channel: &str, results: Vec<(u64, Result<(), String>)>, now: u64) {
        let st = self.chans.entry(channel.to_string()).or_default();
        for (id, r) in results {
            match r {
                Ok(()) => {
                    self.outbox.retain(|d| d.id != id);
                    st.sent += 1;
                    st.last_success = Some(now);
                }
                Err(e) => {
                    if let Some(d) = self.outbox.iter_mut().find(|d| d.id == id) {
                        if d.attempts == 0 {
                            // Once per notification, not once per retry.
                            self.history.push((
                                "alert.delivery_failed",
                                "warn",
                                channel.to_string(),
                                e.clone(),
                            ));
                        }
                        d.attempts += 1;
                        d.next_try = now + RETRY_EVERY;
                    }
                    crate::warn!(
                        "notification failed, will retry",
                        channel = channel,
                        error = e
                    );
                    st.failed += 1;
                    st.last_failure = Some(now);
                    st.last_error = Some(e);
                }
            }
        }
        self.dirty = true;
    }

    fn status(&self, cfg: &Config) -> Status {
        let mut channels = BTreeMap::new();
        for (name, c) in &cfg.channels {
            let st = self.chans.get(name).cloned().unwrap_or_default();
            let mis = self.misconfig.get(name);
            let failing = match (st.last_failure, st.last_success) {
                (Some(f), Some(s)) => f >= s,
                (Some(_), None) => true,
                _ => false,
            };
            let (health, error) = if let Some(m) = mis {
                (Health::Misconfigured, Some(m.clone()))
            } else if failing {
                (Health::Failing, st.last_error.clone())
            } else if st.last_success.is_some() {
                (Health::Ok, None)
            } else {
                (Health::Idle, None)
            };
            channels.insert(
                name.clone(),
                ChannelStatus {
                    kind: c.kind,
                    health,
                    error,
                    last_success: st.last_success,
                    last_failure: st.last_failure,
                    sent: st.sent,
                    failed: st.failed,
                    dropped: st.dropped,
                    pending: self.outbox.iter().filter(|d| &d.channel == name).count(),
                },
            );
        }
        let mut incidents: Vec<Incident> = self
            .engine
            .open()
            .map(|(rule, subject, e)| Incident {
                rule: rule.to_string(),
                subject: subject.to_string(),
                state: e.state.clone(),
                value: e.value.clone(),
                since: e.bad_since.unwrap_or(0),
                notified: e.notified,
            })
            .collect();
        incidents
            .sort_by(|a, b| (a.since, &a.rule, &a.subject).cmp(&(b.since, &b.rule, &b.subject)));
        Status {
            rules: cfg.alerts.iter().filter(|r| r.enabled()).count(),
            channels,
            incidents,
            pending: self.outbox.len(),
            error: self.file_error.clone(),
        }
    }
}

fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Template variables for an event; None when its template is empty
/// (e.g. `recovery = ""`).
fn event_vars(rule: &RuleConfig, ev: &Event, host: &str) -> Option<Vars> {
    let (emoji, tpl) = match ev.kind {
        EventKind::Firing => ("🔴", rule.message.as_deref().unwrap_or(DEFAULT_FIRING)),
        EventKind::Recovery => ("✅", rule.recovery.as_deref().unwrap_or(DEFAULT_RECOVERY)),
    };
    if tpl.trim().is_empty() {
        return None;
    }
    let mut v = Vars::new();
    v.insert("rule".into(), ev.rule.clone());
    v.insert("subject".into(), ev.subject.clone());
    v.insert("state".into(), ev.state.clone());
    v.insert("value".into(), ev.value.clone());
    v.insert("time".into(), crate::util::fmt_utc(ev.at));
    v.insert("host".into(), host.to_string());
    v.insert("kind".into(), ev.kind.as_str().into());
    v.insert(
        "duration".into(),
        crate::util::human_duration(ev.at.saturating_sub(ev.since)),
    );
    v.insert(
        "title".into(),
        format!("{emoji} {}: {}", ev.subject, ev.state),
    );
    let message = template::render(tpl, &v, false);
    v.insert("message".into(), message);
    Some(v)
}

fn publish(state: &AppState, core: &Core, cfg: &Config) {
    let st = core.status(cfg);
    let changed = {
        let mut g = state.alerts.lock().unwrap();
        if *g != st {
            *g = st;
            true
        } else {
            false
        }
    };
    if changed {
        state.changed("alerts");
    }
}

// ---------------------------------------------------------------- probes

/// The data sources rules need, refreshed only while such rules exist.
#[derive(Default)]
struct Probes {
    http: HashMap<String, HttpProbe>,
    widgets: HashMap<String, WidgetProbe>,
    host: Option<HostProbe>,
}

struct HttpProbe {
    st: HttpState,
    next_due: u64,
    in_flight: bool,
}

#[derive(Default)]
struct WidgetProbe {
    next_due: Option<Instant>,
    in_flight: bool,
}

struct HostProbe {
    sampler: crate::system::Sampler,
    stats: Option<SystemStats>,
    next_due: u64,
}

enum ProbeResult {
    Http {
        rule: String,
        url: String,
        res: CheckResult,
    },
    Widget {
        id: String,
        accepted: bool,
    },
}

impl Probes {
    /// Start every check that is due. HTTP checks and widget fetches run as
    /// tasks and report back on `tx`; host sampling is a few file reads.
    fn start_due(
        &mut self,
        state: &Arc<AppState>,
        cfg: &Arc<Config>,
        tx: &mpsc::Sender<ProbeResult>,
        now: u64,
    ) {
        let active: Vec<&RuleConfig> = cfg.alerts.iter().filter(|r| r.enabled()).collect();

        // HTTP checks, one per rule at its interval.
        self.http.retain(|name, _| {
            active
                .iter()
                .any(|r| r.kind == RuleKind::Http && &r.name == name)
        });
        for r in active.iter().filter(|r| r.kind == RuleKind::Http) {
            let url = r.url.clone().unwrap_or_default();
            let p = self
                .http
                .entry(r.name.clone())
                .or_insert_with(|| HttpProbe {
                    st: HttpState::default(),
                    next_due: 0,
                    in_flight: false,
                });
            if p.st.url != url {
                p.st = HttpState {
                    url: url.clone(),
                    ..Default::default()
                };
                p.next_due = 0;
            }
            if p.in_flight || p.next_due > now {
                continue;
            }
            p.in_flight = true;
            p.next_due = now + r.interval().as_secs();
            let (rule, policy, tx) = ((*r).clone(), cfg.http.clone(), tx.clone());
            tokio::spawn(async move {
                let res = rules::http_check(&rule, &policy).await;
                let _ = tx
                    .send(ProbeResult::Http {
                        rule: rule.name,
                        url,
                        res,
                    })
                    .await;
            });
        }

        // Widgets referenced by widget rules, at the widget's refresh.
        let wanted: HashSet<&str> = active
            .iter()
            .filter(|r| r.kind == RuleKind::Widget)
            .filter_map(|r| r.widget.as_deref())
            .collect();
        self.widgets.retain(|id, _| wanted.contains(id.as_str()));
        let tick = Instant::now();
        for w in cfg
            .widgets
            .iter()
            .filter(|w| wanted.contains(w.id.as_str()))
        {
            let refresh = w.refresh.0.max(Duration::from_secs(5));
            let p = self.widgets.entry(w.id.clone()).or_default();
            if p.in_flight || p.next_due.is_some_and(|due| due > tick) {
                continue;
            }
            // Reuse a fresh result from the viewer refresh loop if there is one.
            let cached = {
                let cache = state.widgets.lock().unwrap();
                cache
                    .deadline(&w.id, refresh)
                    .filter(|due| *due > tick && cache.get(&w.id).is_some())
            };
            if let Some(due) = cached {
                p.next_due = Some(due);
                continue;
            }
            let Some(attempt) = state.begin_widget(cfg, &w.id) else {
                continue;
            };
            p.in_flight = true;
            p.next_due = Some(crate::widgets::refresh_deadline(tick, refresh));
            let (state, w, config, tx) = (state.clone(), w.clone(), cfg.clone(), tx.clone());
            tokio::spawn(async move {
                let out = crate::widgets::run(&w, &config.http).await;
                let accepted = state.record_widget(&config, &w.id, attempt, Instant::now(), out);
                let _ = tx.send(ProbeResult::Widget { id: w.id, accepted }).await;
            });
        }

        // Host stats, with our own sampler, only while host rules exist.
        let host_interval = active
            .iter()
            .filter(|r| r.kind == RuleKind::Host)
            .map(|r| r.interval().as_secs())
            .min();
        match host_interval {
            None => self.host = None,
            Some(iv) => {
                let h = self.host.get_or_insert_with(|| HostProbe {
                    sampler: crate::system::Sampler::new(&state.env.host_proc, &state.env.host_sys),
                    stats: None,
                    next_due: 0,
                });
                if h.next_due <= now {
                    h.stats = Some(h.sampler.sample(&cfg.system));
                    h.next_due = now + iv.max(5);
                }
            }
        }
    }

    fn apply(&mut self, r: ProbeResult) {
        match r {
            ProbeResult::Http { rule, url, res } => {
                if let Some(p) = self.http.get_mut(&rule) {
                    p.in_flight = false;
                    if p.st.url == url {
                        p.st.record(res);
                    }
                }
            }
            ProbeResult::Widget { id, accepted } => {
                if let Some(p) = self.widgets.get_mut(&id) {
                    p.in_flight = false;
                    if !accepted {
                        p.next_due = None;
                    }
                }
            }
        }
    }

    /// Earliest time a probe that is not running is due.
    fn next_due(&self) -> Option<u64> {
        let http = self
            .http
            .values()
            .filter(|p| !p.in_flight)
            .map(|p| p.next_due);
        let host = self.host.iter().map(|h| h.next_due);
        http.chain(host).min()
    }

    fn next_widget_due(&self) -> Option<Instant> {
        self.widgets
            .values()
            .filter(|p| !p.in_flight)
            .filter_map(|p| p.next_due)
            .min()
    }
}

/// Evaluate every enabled rule against the latest data; queue the
/// resulting notifications. Returns how many events were produced.
fn evaluate(
    core: &mut Core,
    cfg: &Config,
    probes: &Probes,
    snap: &Snapshot,
    host: &str,
    now: u64,
    widgets: &Mutex<crate::widgets::WidgetCache>,
) -> usize {
    let mut events = Vec::new();
    for r in cfg.alerts.iter().filter(|r| r.enabled()) {
        let ro = match r.kind {
            RuleKind::Container => rules::eval_containers(r, snap, now),
            RuleKind::Http => probes
                .http
                .get(&r.name)
                .and_then(|p| rules::eval_http(r, &p.st)),
            RuleKind::Host => probes
                .host
                .as_ref()
                .and_then(|h| h.stats.as_ref())
                .and_then(|s| rules::eval_host(r, s)),
            RuleKind::Widget => r.widget.as_deref().and_then(|id| {
                let cache = widgets.lock().unwrap();
                cache.get(id).and_then(|out| rules::eval_widget(r, out))
            }),
            RuleKind::LogMatch => {
                let window = r.window.map(|d| d.0.as_secs()).unwrap_or(300);
                crate::logs::watch::rule_view(&r.name, window, now)
                    .map(|v| rules::eval_log_match(r, &v))
            }
        };
        let Some(ro) = ro else { continue };
        let p = r.params();
        let mut present = HashSet::new();
        for (subject, obs) in &ro.subjects {
            present.insert(subject.as_str());
            events.extend(core.engine.observe(&r.name, subject, obs.clone(), p, now));
        }
        if ro.complete {
            events.extend(core.engine.resolve_missing(&r.name, &present, now));
        }
    }
    core.engine.prune(now, |name| {
        cfg.alerts
            .iter()
            .find(|r| r.name == name && r.enabled())
            .map(|r| r.params())
    });
    for ev in &events {
        let rule = cfg.alerts.iter().find(|r| r.name == ev.rule);
        core.enqueue(rule, ev, host, now);
    }
    events.len()
}

// ---------------------------------------------------------------- tasks

/// Background task: runs forever, evaluates rules (also with no viewers) and
/// sends notifications. Only does work for rules that exist: container rules
/// react to `state.containers` changes, HTTP checks run at their interval,
/// host stats and widgets are sampled only when a rule uses them.
pub async fn run(state: Arc<AppState>) {
    let path = state.env.data_dir.join(STATE_FILE);
    let core = Arc::new(Mutex::new(Core::load(path)));
    let wake = Arc::new(Notify::new());
    tokio::spawn(delivery_loop(state.clone(), core.clone(), wake.clone()));
    // Deliver whatever was still queued before a restart.
    wake.notify_one();

    let (tx, mut rx) = mpsc::channel::<ProbeResult>(32);
    let mut probes = Probes::default();
    let mut containers = state.containers.subscribe();
    let mut containers_open = true;
    let mut events = state.events.subscribe();
    let mut chan_check: Option<(u64, u64)> = None;
    loop {
        let generation = state.config.snapshot().generation;
        let cfg = state.config.current();
        let now = unix_now();
        probes.start_due(&state, &cfg, &tx, now);
        let busy = {
            let mut c = core.lock().unwrap();
            if chan_check.is_none_or(|(g, t)| g != generation || now >= t + CHANNEL_CHECK_EVERY) {
                c.misconfig = cfg
                    .channels
                    .iter()
                    .filter_map(|(n, ch)| channels::resolve(ch).err().map(|e| (n.clone(), e)))
                    .collect();
                chan_check = Some((generation, now));
            }
            let snap = state.snapshot();
            if evaluate(
                &mut c,
                &cfg,
                &probes,
                &snap,
                &state.env.hostname,
                now,
                &state.widgets,
            ) > 0
            {
                wake.notify_one();
            }
            c.flush_history(&state.db, now);
            c.save_if_dirty();
            publish(&state, &c, &cfg);
            c.engine.needs_tick()
        };

        let container_rules = cfg
            .alerts
            .iter()
            .any(|r| r.enabled() && r.kind == RuleKind::Container);
        let mut wait = if busy { TICK_BUSY } else { TICK_IDLE };
        if let Some(t) = probes.next_due() {
            wait = wait.min(t.saturating_sub(now).max(1));
        }
        let mut deadline = Instant::now() + Duration::from_secs(wait);
        if let Some(widget_due) = probes.next_widget_due() {
            deadline = deadline.min(widget_due);
        }
        let deadline = tokio::time::Instant::from_std(deadline);
        loop {
            tokio::select! {
                r = containers.changed(), if container_rules && containers_open => {
                    if r.is_err() {
                        containers_open = false;
                        continue;
                    }
                    break;
                }
                Some(p) = rx.recv() => {
                    probes.apply(p);
                    break;
                }
                m = events.recv() => match m {
                    // New `log_match` matches (src/logs/watch.rs).
                    Ok(id) if &*id == "log_match" => break,
                    Ok(id) if &*id != "reload" => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        tokio::time::sleep_until(deadline).await;
                        break;
                    }
                    _ => break,
                },
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }
    }
}

/// Sends queued notifications, retrying failed ones every minute. Runs
/// separately so a slow channel never delays rule evaluation.
async fn delivery_loop(state: Arc<AppState>, core: Arc<Mutex<Core>>, wake: Arc<Notify>) {
    loop {
        let cfg = state.config.current();
        let now = unix_now();
        let (batches, next_try) = {
            let mut c = core.lock().unwrap();
            let b = c.take_due(&cfg, now);
            c.save_if_dirty();
            publish(&state, &c, &cfg);
            (b, c.next_try())
        };
        if batches.is_empty() {
            let wait = next_try
                .map(|t| t.saturating_sub(now).clamp(1, 3600))
                .unwrap_or(3600);
            let _ = tokio::time::timeout(Duration::from_secs(wait), wake.notified()).await;
            continue;
        }
        let mut set = tokio::task::JoinSet::new();
        for (name, chan, items) in batches {
            let policy = cfg.http.clone();
            set.spawn(async move {
                let r = deliver_channel(&chan, items, &policy).await;
                (name, r)
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok((name, results)) = res {
                core.lock().unwrap().apply(&name, results, unix_now());
            }
        }
        let mut c = core.lock().unwrap();
        c.flush_history(&state.db, unix_now());
        c.save_if_dirty();
        publish(&state, &c, &cfg);
    }
}

/// Send a channel's due deliveries in order; stop at the first failure.
async fn deliver_channel(
    chan: &ChannelConfig,
    items: Vec<Delivery>,
    policy: &crate::config::HttpPolicy,
) -> Vec<(u64, Result<(), String>)> {
    let target = match channels::resolve(chan) {
        Ok(t) => t,
        Err(e) => {
            return items
                .first()
                .map(|d| vec![(d.id, Err(format!("misconfigured: {e}")))])
                .unwrap_or_default()
        }
    };
    let mut out = Vec::new();
    for d in items {
        let r = send_with_retries(&target, &d.vars, policy).await;
        let failed = r.is_err();
        out.push((d.id, r));
        if failed {
            break;
        }
    }
    out
}

async fn send_with_retries(
    t: &channels::Target,
    vars: &Vars,
    policy: &crate::config::HttpPolicy,
) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        match channels::send(t, vars, policy).await {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        if let Some(ms) = BACKOFF_MS.get(attempt) {
            tokio::time::sleep(Duration::from_millis(*ms)).await;
        }
    }
    Err(last)
}

// ---------------------------------------------------------------- CLI

/// `hobnail notify-test <channel>`: send a test message, print result.
pub async fn cli_notify_test(args: &[String]) -> i32 {
    let env = crate::state::Env::from_env();
    let path = match &env {
        Ok(e) => e.config_path.clone(),
        Err(_) => std::env::var("RH_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("RH_DATA_DIR").unwrap_or_else(|_| "./data".into()))
                    .join("config.toml")
            }),
    };
    let display = path.display().to_string();
    let cfg = match std::fs::read_to_string(&path) {
        Ok(text) => match crate::config::parse(&text, &display) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        },
        Err(e) => {
            eprintln!("{display}: {e}");
            return 1;
        }
    };
    let names: Vec<&str> = cfg.channels.keys().map(String::as_str).collect();
    let chan = match args {
        [name] => cfg.channels.get(name).map(|c| (name, c)),
        _ => None,
    };
    let Some((name, chan)) = chan else {
        eprintln!("usage: hobnail notify-test <channel>");
        eprintln!(
            "configured channels in {display}: {}",
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        );
        return 2;
    };
    let host = env
        .map(|e| e.hostname)
        .unwrap_or_else(|_| "home".to_string());
    let vars = test_vars(name, &host, unix_now());
    let target = match channels::resolve(chan) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{name}: misconfigured: {e}");
            return 1;
        }
    };
    match channels::send(&target, &vars, &cfg.http).await {
        Ok(()) => {
            println!("ok");
            0
        }
        Err(e) => {
            eprintln!("{name}: {e}");
            1
        }
    }
}

fn test_vars(channel: &str, host: &str, now: u64) -> Vars {
    let time = crate::util::fmt_utc(now);
    let message = format!("hobnail test message from {host} at {time}");
    [
        ("rule", "notify-test".to_string()),
        ("subject", channel.to_string()),
        ("state", "test".to_string()),
        ("value", String::new()),
        ("time", time),
        ("host", host.to_string()),
        ("kind", "test".to_string()),
        ("duration", "0s".to_string()),
        ("title", "hobnail test".to_string()),
        ("message", message),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Send the test message to one configured channel (CLI and Settings →
/// Send test). The error never contains a secret.
pub async fn send_test(cfg: &Config, name: &str, host: &str) -> Result<(), String> {
    let chan = cfg
        .channels
        .get(name)
        .ok_or_else(|| format!("no channel {name:?}"))?;
    let target = channels::resolve(chan).map_err(|e| format!("misconfigured: {e}"))?;
    channels::send(
        &target,
        &test_vars(name, host, crate::util::unix_now()),
        &cfg.http,
    )
    .await
}
