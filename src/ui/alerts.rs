//! Alerts panel: open incidents and channel health. Empty when no channels
//! or rules are configured. Never shows secrets: channel errors are
//! scrubbed before they reach `Status`.

use maud::{html, Markup};
use std::collections::HashSet;

use crate::alerts::{ChannelStatus, Health, RuleKind, Status};
use crate::config::Config;
use crate::ui::fmt::{clock_tag, url_encode};
use crate::util::{fmt_utc, human_duration, unix_now};

/// Where an alert's subject lives (LG-55, OV-04): a container's page (state
/// and log-match rules), the Host page (host metrics), the widget on the
/// Overview, or the checked URL itself (`true`: outside Hobnail). The
/// subject is already public (it is shown on the page); `None` for a rule
/// that no longer exists or a subject that is not a link.
pub fn subject_href(cfg: &Config, rule: &str, subject: &str) -> Option<(String, bool)> {
    let r = cfg.alerts.iter().find(|r| r.name == rule)?;
    if subject.is_empty() {
        return None;
    }
    Some(match r.kind {
        RuleKind::Container | RuleKind::LogMatch => {
            (format!("/containers/{}", url_encode(subject)), false)
        }
        RuleKind::Host => ("/host".into(), false),
        RuleKind::Widget => (format!("/#w-{subject}"), false),
        RuleKind::Http => {
            let ok = subject.starts_with("http://") || subject.starts_with("https://");
            return ok.then(|| (subject.to_string(), true));
        }
    })
}

/// The subject of an incident or log row: a link to it when it has one,
/// stretched over the row (`row-link`; the row must be `.rl`).
pub fn subject_link(href: Option<(String, bool)>, subject: &str, cls: &str) -> Markup {
    html! {
        @match href {
            Some((u, true)) => a class={(cls) " row-link"} href=(u) target="_blank" rel="noopener noreferrer" { (subject) " ↗" },
            Some((u, false)) => a class={(cls) " row-link"} href=(u) { (subject) },
            None => strong class=(cls) { (subject) },
        }
    }
}

/// Must return the element `<section id="alerts">…</section>` (may be empty
/// when no channels or rules are configured).
#[cfg(test)]
pub fn render(cfg: &Config, status: &Status) -> Markup {
    render_with(cfg, status, &HashSet::new(), false)
}

/// As `render`; an incident whose subject is one of `logs` (containers whose
/// logs may be read) ends with a link to that container's logs.
///
/// `signed`: the page has a session, so a firing rule's name opens its
/// dialog (`/alerts?edit=rule:<name>`; the live fragment, rendered without
/// one, has the plain name and the script makes it that link again).
pub fn render_with(cfg: &Config, status: &Status, logs: &HashSet<String>, signed: bool) -> Markup {
    if cfg.channels.is_empty() && cfg.alerts.is_empty() {
        return html! { section #alerts {} };
    }
    let now = unix_now();
    let open = status.incidents.len();
    let cls = if open > 0 {
        "alerts tile bad"
    } else {
        "alerts tile"
    };
    let sent: u64 = status.channels.values().map(|c| c.sent).sum();
    let failed: u64 = status.channels.values().map(|c| c.failed).sum();
    // Notifications not delivered (waiting for a retry or given up), not
    // attempts (LG-59).
    let undelivered: u64 =
        status.pending as u64 + status.channels.values().map(|c| c.dropped).sum::<u64>();
    html! {
        section #alerts class=(cls) aria-label="Alerts" {
            div.alerts-head {
                span.tile-title { "Alerts" }
                span.alerts-sum {
                    @if open > 0 {
                        span.badge.bad { (open) " open" }
                    } @else {
                        span.badge.ok { "all clear" }
                    }
                    span.muted.alerts-counts title="rules · notifications sent · notifications not delivered (failed attempts)" {
                        (status.rules) @if status.rules == 1 { " rule · " } @else { " rules · " } (sent) " sent"
                        @if undelivered > 0 { " · " (undelivered) " undelivered" }
                        @if failed > 0 { " (" (failed) @if failed == 1 { " failed attempt)" } @else { " failed attempts)" } }
                    }
                }
                @if !status.channels.is_empty() {
                    span.alerts-chans {
                        @for (name, c) in &status.channels {
                            @let (hc, label) = match c.health {
                                Health::Ok => ("ok", "ok"),
                                Health::Failing => ("bad", "failing"),
                                Health::Misconfigured => ("warn", "misconfigured"),
                                Health::Idle => ("", "not used yet"),
                            };
                            @let title = chan_title(name, c, label);
                            span class={"badge chan " (hc)} data-key=(name) title=(title) {
                                span class={"dot " (hc)} {}
                                (name)
                                @if c.health != Health::Ok && c.health != Health::Idle {
                                    span.chan-state { (label) }
                                }
                            }
                        }
                    }
                }
            }
            @if open > 0 {
                ul.incidents {
                    @for i in &status.incidents {
                        // The whole row opens the subject; the rule that
                        // fired and the clock time are on the row (LG-55,
                        // LG-56); a container subject has its logs from the
                        // moment it went bad.
                        @let href = subject_href(cfg, &i.rule, &i.subject);
                        // Keyed by rule and subject (HB.morph): incidents
                        // come and go, and a focused row must stay its own.
                        li.incident.rl[href.is_some()] data-key={(i.rule) "/" (i.subject)} {
                            span.dot.bad {}
                            (subject_link(href, &i.subject, "inc-subject"))
                            span.inc-state { (i.state) }
                            span.inc-rule {
                                "rule "
                                @if signed {
                                    a.inc-rule-a href={"/alerts" (crate::ui::settings::rule_edit_link(&i.rule))} data-rule=(i.rule) { (i.rule) }
                                } @else {
                                    b data-rule=(i.rule) { (i.rule) }
                                }
                            }
                            @if !i.notified {
                                span.badge.warn title="suppressed by the cooldown; will be sent if it is still open when the cooldown ends" { "cooldown" }
                            }
                            // Right-aligned in one column on every row, wrapped
                            // or not (the logs slot is kept on rows without it).
                            span.muted.inc-since {
                                "since " (clock_tag(i.since)) " · " (human_duration(now.saturating_sub(i.since)))
                            }
                            @if logs.contains(&i.subject) {
                                a.ia.logs href=(crate::logs::logs_at(&i.subject, i.since as i64)) title="Logs from when it started"
                                    aria-label={"Logs of " (i.subject) " from when it started"} { (crate::ui::ic("logs")) }
                            }
                        }
                    }
                }
            }
            @if let Some(e) = &status.error {
                div.err.alerts-err { "alert state: " (e) }
            }
        }
    }
}

/// Hover text for a channel badge: health, counts, last success, error.
fn chan_title(name: &str, c: &ChannelStatus, label: &str) -> String {
    let mut t = format!(
        "{} channel {name}: {label} · {} sent · {} failed attempts",
        c.kind.as_str(),
        c.sent,
        c.failed
    );
    if let Some(ts) = c.last_success {
        t.push_str(&format!(" · last ok {}", fmt_utc(ts)));
    }
    if c.pending > 0 {
        t.push_str(&format!(" · {} queued", c.pending));
    }
    if c.dropped > 0 {
        t.push_str(&format!(" · {} dropped", c.dropped));
    }
    if let Some(e) = &c.error {
        t.push_str(&format!(" — {e}"));
    }
    t
}
