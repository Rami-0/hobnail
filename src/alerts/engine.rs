//! The pure alert engine: no I/O, no clock. The caller feeds observations
//! ("rule R, subject S is bad/ok right now, at time T") and gets back the
//! notifications to send. All the policy lives here, so it is unit-tested
//! without sockets or sleeps.
//!
//! Per (rule, subject) the engine keeps one [`Entry`]:
//! - **`for`**: a subject must be bad continuously for `for` seconds before
//!   an incident opens (`bad_since` marks the start). Going ok earlier
//!   resets the timer silently (flap guard).
//! - **Dedupe**: an open incident (`firing`) produces exactly one firing
//!   notification, however many times it is observed.
//! - **Cooldown**: when an incident opens within `cooldown` of the last
//!   *notified* firing of the same rule+subject, it is suppressed (counted,
//!   not sent) and its recovery is not sent either. If a suppressed incident
//!   is still open once the cooldown has passed, it is notified then, so a
//!   service that went down again and stayed down is never silent.
//! - **Recovery**: an incident that was notified sends one recovery when it
//!   becomes ok, or when its subject disappears ("removed").
//!
//! The entries are persisted by the caller (see [`Engine::to_records`]) so
//! a restart neither re-sends open incidents nor forgets the cooldown.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Timing parameters of a rule, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    pub for_secs: u64,
    pub cooldown_secs: u64,
}

/// One observation of one subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Obs {
    pub bad: bool,
    /// Human description, e.g. "stopped (exit 137)" or "92.1% > 90".
    pub state: String,
    /// The raw value, e.g. "137" or "92.1".
    pub value: String,
}

impl Obs {
    pub fn bad(state: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            bad: true,
            state: state.into(),
            value: value.into(),
        }
    }
    pub fn ok(state: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            bad: false,
            state: state.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Firing,
    Recovery,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Firing => "firing",
            EventKind::Recovery => "recovery",
        }
    }
}

/// A notification to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub rule: String,
    pub subject: String,
    pub kind: EventKind,
    /// Firing: the bad state. Recovery: "recovered" or "removed".
    pub state: String,
    pub value: String,
    /// When the subject went bad (start of the incident).
    pub since: u64,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// Bad since this time (pending or firing); None while ok.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bad_since: Option<u64>,
    /// An incident is open (bad for at least `for`).
    pub firing: bool,
    /// The open incident's firing notification was sent (not suppressed).
    pub notified: bool,
    /// Last notified firing, for the cooldown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<u64>,
    pub state: String,
    pub value: String,
    /// Incidents suppressed by the cooldown (lifetime count for this subject).
    #[serde(skip_serializing_if = "is_zero")]
    pub suppressed: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Entry {
    fn idle(&self) -> bool {
        !self.firing && self.bad_since.is_none()
    }
}

/// One persisted entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub rule: String,
    pub subject: String,
    #[serde(flatten)]
    pub entry: Entry,
}

#[derive(Debug, Default)]
pub struct Engine {
    entries: BTreeMap<(String, String), Entry>,
    dirty: bool,
}

impl Engine {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_records(records: Vec<Record>) -> Self {
        Self {
            entries: records
                .into_iter()
                .map(|r| ((r.rule, r.subject), r.entry))
                .collect(),
            dirty: false,
        }
    }

    pub fn to_records(&self) -> Vec<Record> {
        self.entries
            .iter()
            .map(|((rule, subject), entry)| Record {
                rule: rule.clone(),
                subject: subject.clone(),
                entry: entry.clone(),
            })
            .collect()
    }

    /// True once after any change worth persisting.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    #[cfg(test)]
    pub fn get(&self, rule: &str, subject: &str) -> Option<&Entry> {
        self.entries.get(&(rule.to_string(), subject.to_string()))
    }

    /// Open incidents: (rule, subject, entry).
    pub fn open(&self) -> impl Iterator<Item = (&str, &str, &Entry)> {
        self.entries
            .iter()
            .filter(|(_, e)| e.firing)
            .map(|((r, s), e)| (r.as_str(), s.as_str(), e))
    }

    /// Whether a timer is running (a `for` countdown, or a suppressed
    /// incident waiting for its cooldown), so the caller should re-evaluate
    /// soon even if no new data arrives.
    pub fn needs_tick(&self) -> bool {
        self.entries
            .values()
            .any(|e| (!e.firing && e.bad_since.is_some()) || (e.firing && !e.notified))
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Feed one observation. Returns the notification to send, if any.
    pub fn observe(
        &mut self,
        rule: &str,
        subject: &str,
        obs: Obs,
        p: Params,
        now: u64,
    ) -> Option<Event> {
        let key = (rule.to_string(), subject.to_string());
        if !obs.bad && !self.entries.contains_key(&key) {
            // Never seen bad: nothing to remember.
            return None;
        }
        let e = self.entries.entry(key).or_default();
        if obs.bad {
            e.state = obs.state;
            e.value = obs.value;
            if !e.firing {
                let since = match e.bad_since {
                    Some(t) => t,
                    None => {
                        e.bad_since = Some(now);
                        self.dirty = true;
                        now
                    }
                };
                if now.saturating_sub(since) < p.for_secs {
                    return None;
                }
                e.firing = true;
                self.dirty = true;
                if in_cooldown(e.last_fired, p, now) {
                    e.notified = false;
                    e.suppressed = e.suppressed.saturating_add(1);
                    return None;
                }
            } else if e.notified || in_cooldown(e.last_fired, p, now) {
                return None;
            }
            // Fire: a new incident, or a suppressed one whose cooldown ended.
            e.notified = true;
            e.last_fired = Some(now);
            self.dirty = true;
            Some(Event {
                rule: rule.to_string(),
                subject: subject.to_string(),
                kind: EventKind::Firing,
                state: e.state.clone(),
                value: e.value.clone(),
                since: e.bad_since.unwrap_or(now),
                at: now,
            })
        } else if e.idle() {
            None
        } else {
            self.dirty = true;
            close(rule, subject, e, "recovered", obs.value, now)
        }
    }

    /// Close the incidents of `rule` whose subject is not in `present` (a
    /// removed container, a disk that is gone). Their recovery says "removed".
    pub fn resolve_missing(&mut self, rule: &str, present: &HashSet<&str>, now: u64) -> Vec<Event> {
        let mut out = Vec::new();
        for ((r, s), e) in self.entries.iter_mut() {
            if r != rule || present.contains(s.as_str()) || e.idle() {
                continue;
            }
            self.dirty = true;
            let value = e.value.clone();
            out.extend(close(r, s, e, "removed", value, now));
        }
        out
    }

    /// Forget entries of rules that no longer exist (`params` returns None),
    /// and idle entries whose cooldown is over.
    pub fn prune(&mut self, now: u64, params: impl Fn(&str) -> Option<Params>) {
        let before = self.entries.len();
        self.entries.retain(|(rule, _), e| match params(rule) {
            None => false,
            Some(p) => !e.idle() || in_cooldown(e.last_fired, p, now),
        });
        if self.entries.len() != before {
            self.dirty = true;
        }
    }
}

fn in_cooldown(last_fired: Option<u64>, p: Params, now: u64) -> bool {
    last_fired.is_some_and(|t| now < t.saturating_add(p.cooldown_secs))
}

/// Reset an entry to ok; return the recovery if the incident was notified.
fn close(
    rule: &str,
    subject: &str,
    e: &mut Entry,
    state: &str,
    value: String,
    now: u64,
) -> Option<Event> {
    let since = e.bad_since.take().unwrap_or(now);
    let was_notified = e.firing && e.notified;
    e.firing = false;
    e.notified = false;
    e.state = state.to_string();
    e.value = value.clone();
    was_notified.then(|| Event {
        rule: rule.to_string(),
        subject: subject.to_string(),
        kind: EventKind::Recovery,
        state: state.to_string(),
        value,
        since,
        at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Params = Params {
        for_secs: 10,
        cooldown_secs: 900,
    };
    const NOW: Params = Params {
        for_secs: 0,
        cooldown_secs: 900,
    };

    fn bad() -> Obs {
        Obs::bad("stopped (exit 137)", "137")
    }
    fn ok() -> Obs {
        Obs::ok("running", "")
    }

    /// Feed `obs` for `rule`/`s` at each time; collect events.
    fn run(e: &mut Engine, p: Params, steps: &[(u64, bool)]) -> Vec<Event> {
        steps
            .iter()
            .filter_map(|&(t, b)| e.observe("r", "s", if b { bad() } else { ok() }, p, t))
            .collect()
    }

    fn kinds(ev: &[Event]) -> Vec<(EventKind, u64)> {
        ev.iter().map(|e| (e.kind, e.at)).collect()
    }

    #[test]
    fn dedupe_one_alert_per_incident() {
        let mut e = Engine::new();
        let steps: Vec<_> = (0..500).map(|i| (1000 + i * 5, true)).collect();
        let ev = run(&mut e, NOW, &steps);
        assert_eq!(kinds(&ev), vec![(EventKind::Firing, 1000)]);
        assert_eq!(ev[0].state, "stopped (exit 137)");
        assert_eq!(ev[0].value, "137");
        assert_eq!(e.open().count(), 1);
    }

    #[test]
    fn ok_subjects_leave_no_state() {
        let mut e = Engine::new();
        for t in 0..100 {
            assert!(e.observe("r", &format!("c{t}"), ok(), P, t).is_none());
        }
        assert_eq!(e.len(), 0);
        assert!(!e.take_dirty());
    }

    #[test]
    fn for_must_hold_continuously() {
        let mut e = Engine::new();
        // Bad for 9s only, then ok: nothing.
        assert!(run(
            &mut e,
            P,
            &[(100, true), (105, true), (109, true), (110, false)]
        )
        .is_empty());
        // Timer restarts at 120; fires at 130, not before.
        let ev = run(
            &mut e,
            P,
            &[
                (120, true),
                (125, true),
                (129, true),
                (130, true),
                (131, true),
            ],
        );
        assert_eq!(kinds(&ev), vec![(EventKind::Firing, 130)]);
        assert_eq!(ev[0].since, 120);
        assert!(!e.needs_tick());
    }

    #[test]
    fn pending_timer_requests_ticks() {
        let mut e = Engine::new();
        run(&mut e, P, &[(100, true)]);
        assert!(e.needs_tick());
        assert!(e.open().next().is_none());
    }

    #[test]
    fn recovery_sent_once() {
        let mut e = Engine::new();
        let ev = run(
            &mut e,
            NOW,
            &[
                (100, true),
                (110, true),
                (120, false),
                (130, false),
                (140, false),
            ],
        );
        assert_eq!(
            kinds(&ev),
            vec![(EventKind::Firing, 100), (EventKind::Recovery, 120)]
        );
        assert_eq!(ev[1].state, "recovered");
        assert_eq!(ev[1].since, 100);
    }

    #[test]
    fn cooldown_suppresses_then_fires_after_it() {
        let mut e = Engine::new();
        let ev = run(
            &mut e,
            NOW,
            &[
                (1000, true),  // fires
                (1060, false), // recovery
                (1120, true),  // new incident within 15m: suppressed
                (1180, false), // its recovery is not sent either
                (2000, true),  // 1000s after the last firing: fires
                (2010, false),
            ],
        );
        assert_eq!(
            kinds(&ev),
            vec![
                (EventKind::Firing, 1000),
                (EventKind::Recovery, 1060),
                (EventKind::Firing, 2000),
                (EventKind::Recovery, 2010)
            ]
        );
        assert_eq!(e.get("r", "s").unwrap().suppressed, 1);
    }

    #[test]
    fn suppressed_incident_still_open_after_cooldown_is_notified() {
        let mut e = Engine::new();
        let mut steps = vec![(1000, true), (1060, false)];
        // Down again at 1120 and stays down.
        steps.extend((0..200).map(|i| (1120 + i * 5, true)));
        let ev = run(&mut e, NOW, &steps);
        assert_eq!(
            kinds(&ev),
            vec![
                (EventKind::Firing, 1000),
                (EventKind::Recovery, 1060),
                (EventKind::Firing, 1900)
            ]
        );
        assert_eq!(ev[2].since, 1120);
        // ...and then it recovers normally, once.
        let ev = run(&mut e, NOW, &[(2200, false), (2205, false)]);
        assert_eq!(kinds(&ev), vec![(EventKind::Recovery, 2200)]);
    }

    #[test]
    fn suppressed_incident_requests_ticks() {
        let mut e = Engine::new();
        run(&mut e, NOW, &[(1000, true), (1060, false), (1120, true)]);
        assert!(e.needs_tick());
    }

    #[test]
    fn flapping_sends_one_pair_per_cooldown() {
        let mut e = Engine::new();
        // Up/down every 20s for 10 minutes, `for` = 0.
        let steps: Vec<_> = (0..30).map(|i| (1000 + i * 20, i % 2 == 0)).collect();
        let ev = run(&mut e, NOW, &steps);
        assert_eq!(
            kinds(&ev),
            vec![(EventKind::Firing, 1000), (EventKind::Recovery, 1020)]
        );
        assert_eq!(e.get("r", "s").unwrap().suppressed, 14);
    }

    #[test]
    fn flapping_faster_than_for_sends_nothing() {
        let mut e = Engine::new();
        let steps: Vec<_> = (0..100).map(|i| (1000 + i * 5, i % 2 == 0)).collect();
        assert!(run(&mut e, P, &steps).is_empty());
    }

    #[test]
    fn removed_subject_resolves_with_removed() {
        let mut e = Engine::new();
        e.observe("r", "a", bad(), NOW, 100).unwrap();
        e.observe("r", "b", bad(), P, 100); // pending only
        e.observe("other", "a", bad(), NOW, 100).unwrap();
        let present: HashSet<&str> = HashSet::new();
        let ev = e.resolve_missing("r", &present, 200);
        assert_eq!(ev.len(), 1);
        assert_eq!(
            (ev[0].subject.as_str(), ev[0].state.as_str()),
            ("a", "removed")
        );
        assert_eq!(ev[0].kind, EventKind::Recovery);
        assert!(e.resolve_missing("r", &present, 300).is_empty());
        // The other rule is untouched.
        assert_eq!(e.open().count(), 1);
        // A re-created container that dies again within the cooldown is suppressed.
        assert!(e.observe("r", "a", bad(), NOW, 400).is_none());
    }

    #[test]
    fn removed_while_suppressed_sends_nothing() {
        let mut e = Engine::new();
        run(&mut e, NOW, &[(1000, true), (1010, false), (1020, true)]);
        assert!(e.resolve_missing("r", &HashSet::new(), 1030).is_empty());
    }

    #[test]
    fn persistence_round_trip() {
        let mut e = Engine::new();
        e.observe("r", "s", bad(), NOW, 1000).unwrap();
        e.observe("r", "pending", bad(), P, 1000);
        assert!(e.take_dirty());
        let json = serde_json::to_string(&e.to_records()).unwrap();

        // "Restart": a new engine from the file does not re-send the open alert.
        let recs: Vec<Record> = serde_json::from_str(&json).unwrap();
        let mut e2 = Engine::from_records(recs);
        assert_eq!(e2.to_records(), e.to_records());
        assert!(e2.observe("r", "s", bad(), NOW, 1100).is_none());
        // The pending timer kept its start: fires at 1010 + ... (already past `for`).
        let ev = e2.observe("r", "pending", bad(), P, 1100).unwrap();
        assert_eq!(ev.since, 1000);
        // It still sends the recovery, once.
        let ev = e2.observe("r", "s", ok(), NOW, 1200).unwrap();
        assert_eq!(ev.kind, EventKind::Recovery);
        // And it remembers the cooldown across another restart.
        let json = serde_json::to_string(&e2.to_records()).unwrap();
        let mut e3 = Engine::from_records(serde_json::from_str(&json).unwrap());
        assert!(e3.observe("r", "s", bad(), NOW, 1300).is_none());
        assert!(e3.get("r", "s").unwrap().firing);
        assert!(!e3.get("r", "s").unwrap().notified);
    }

    #[test]
    fn prune_drops_removed_rules_and_idle_entries() {
        let mut e = Engine::new();
        run(&mut e, NOW, &[(1000, true), (1010, false)]); // idle, in cooldown
        e.observe("gone", "x", bad(), NOW, 1000);
        e.observe("r", "open", bad(), NOW, 1000);
        e.take_dirty();
        let params = |r: &str| (r == "r").then_some(NOW);
        e.prune(1100, params);
        assert!(e.take_dirty());
        assert!(e.get("gone", "x").is_none());
        assert!(e.get("r", "s").is_some(), "kept while in cooldown");
        e.prune(1000 + 900, params);
        assert!(e.get("r", "s").is_none(), "dropped after cooldown");
        assert!(e.get("r", "open").is_some(), "open incidents are kept");
    }
}
