//! Alerts panel: open incidents and channel health. Empty when no channels
//! or rules are configured. Never shows secrets: channel errors are
//! scrubbed before they reach `Status`.

use maud::{html, Markup};
use std::collections::HashSet;

use crate::alerts::{ChannelStatus, Health, Status};
use crate::config::Config;
use crate::util::{fmt_utc, human_duration, unix_now};

/// Must return the element `<section id="alerts">…</section>` (may be empty
/// when no channels or rules are configured).
#[cfg(test)]
pub fn render(cfg: &Config, status: &Status) -> Markup {
    render_with(cfg, status, &HashSet::new())
}

/// As `render`; an incident whose subject is one of `logs` (containers whose
/// logs may be read) ends with a link to that container's logs.
pub fn render_with(cfg: &Config, status: &Status, logs: &HashSet<String>) -> Markup {
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
                    span.muted.alerts-counts title="rules · notifications sent · failed attempts" {
                        (status.rules) @if status.rules == 1 { " rule · " } @else { " rules · " } (sent) " sent"
                        @if failed > 0 { " · " (failed) " failed" }
                        @if status.pending > 0 { " · " (status.pending) " queued" }
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
                            span class={"badge chan " (hc)} title=(title) {
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
                        li.incident title={(i.rule) " · since " (fmt_utc(i.since))} {
                            span.dot.bad {}
                            strong.inc-subject { (i.subject) }
                            span.inc-state { (i.state) }
                            span.muted.inc-since { (human_duration(now.saturating_sub(i.since))) }
                            @if !i.notified {
                                span.badge.warn title="suppressed by the cooldown; will be sent if it is still open when the cooldown ends" { "cooldown" }
                            }
                            @if logs.contains(&i.subject) {
                                a.ia.logs href=(crate::logs::logs_url(&i.subject)) title="Logs"
                                    aria-label={"Logs of " (i.subject)} { (crate::ui::ic("logs")) }
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
        "{} channel {name}: {label} · {} sent · {} failed",
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
