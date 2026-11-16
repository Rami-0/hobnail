//! `/alerts`: rules and their state, channels and delivery status, what is
//! firing now, and the alert log from the history database.

use maud::{html, Markup};
use std::collections::HashSet;

use crate::alerts::{ChannelStatus, Health, RuleConfig, RuleKind, Status};
use crate::config::Config;
use crate::db::EventFilter;
use crate::ui::alerts::{subject_href, subject_link};
use crate::ui::fmt::{clock_tag, time_tag};
use crate::ui::pages::{kind_label, page_sub};
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::{empty_state, ic, panel_head, pill};

pub fn page(ctx: &Ctx) -> String {
    let log = ctx.state.db.events(
        &EventFilter {
            kind_prefix: Some("alert.".into()),
            limit: 50,
            ..Default::default()
        }
        .for_viewer(ctx.user.is_some()),
    );
    let names = crate::logs::tailable(ctx.state);
    let status = ctx.state.alerts.lock().unwrap().clone();
    // Signed in, channels and rules are the Notifications tab's own lists,
    // dialogs and script (Y15: "Add channel", "Add rule", "Send test", the
    // rule switches; the same routes and rules as Settings). Signed out, the
    // read-only tables, exactly what this page always showed.
    let embed = crate::ui::settings::embed(ctx, "notifications");
    let body = html! {
        @if let Some(e) = &embed { (e.head) }
        div.page {
            (page_sub(html! { "Rules, notification channels and what has fired" }))
            (live(&ctx.cfg, &status, &names, embed.is_some()))
            @match &embed {
                Some(e) => (e.body),
                None => (public(&ctx.cfg, &status, ctx.now)),
            }
            section.sec.ghost aria-label="Alert log" {
                (panel_head("Alert log", None))
                @if log.is_empty() {
                    p.muted {
                        @if ctx.state.db.is_enabled() { "Nothing has fired yet." }
                        @else { "Fired, resolved and failed notifications are logged here once history is recording." }
                    }
                } @else {
                    ul.events.al-log { @for e in &log { (log_row(&ctx.cfg, e, &names, ctx.user.is_some())) } }
                }
            }
        }
        @if let Some(e) = &embed { (e.tail) }
    };
    layout(
        ctx,
        Nav::Alerts,
        "Alerts",
        crate::ui::pages::crumb_title("Alerts"),
        body,
    )
}

/// The live parts (SSE "alerts"): the firing list, and the read-only rules
/// and channels tables. Rendered without a session, so a signed-in page,
/// which shows the Settings lists instead, has no `#al-pub` to swap.
pub fn fragment(ctx: &Ctx) -> Markup {
    let status = ctx.state.alerts.lock().unwrap().clone();
    html! {
        (live(&ctx.cfg, &status, &crate::logs::tailable(ctx.state), ctx.user.is_some()))
        (public(&ctx.cfg, &status, ctx.now))
    }
}

/// What is firing now (live region `al-live`).
fn live(cfg: &Config, status: &Status, names: &HashSet<String>, signed: bool) -> Markup {
    html! {
        div #al-live {
            (crate::ui::alerts::render_with(cfg, status, names, signed))
            @if let Some(e) = &status.error { p.bad.wrap { "alert state: " (e) } }
        }
    }
}

/// Signed out: rules and channels, read only (live region `al-pub`).
fn public(cfg: &Config, status: &Status, now: u64) -> Markup {
    html! {
        div #al-pub {
            @if cfg.alerts.is_empty() && cfg.channels.is_empty() {
                (empty_state("bell", "No alert rules", html! {
                    "Nothing is watched and nothing is sent. Add " code { "[[alerts]]" } " rules and "
                    code { "[channels.*]" } " to the config to get notified."
                }))
            } @else {
                section.panel aria-label="Rules" {
                    (panel_head("Rules", None))
                    (rules_table(cfg, status))
                }
                section.panel aria-label="Channels" {
                    (panel_head("Channels", None))
                    (channels_table(cfg, status, now))
                }
            }
        }
    }
}

/// One alert log row (LG-55, LG-61): a clock time, what happened in a word,
/// the subject as a link to it, the rule that fired, and the logs of a
/// container at that moment. `signed`: a failed delivery opens its channel.
fn log_row(cfg: &Config, e: &crate::db::Event, names: &HashSet<String>, signed: bool) -> Markup {
    let (cls, word) = match e.kind.as_str() {
        "alert.fired" => ("bad", "Fired"),
        "alert.resolved" => ("ok", "Resolved"),
        "alert.delivery_failed" => ("warn", "Not delivered"),
        k => ("", kind_label(k)),
    };
    // "rule: state (value)": the rule is the part before the first ": "
    // when such a rule exists.
    let (rule, rest) = match e.detail.split_once(": ") {
        Some((r, rest)) if cfg.alerts.iter().any(|x| x.name == r) => (Some(r), rest),
        _ => (None, e.detail.as_str()),
    };
    let href = if e.kind == "alert.delivery_failed" {
        signed.then(|| {
            (
                format!(
                    "/alerts{}",
                    crate::ui::settings::channel_edit_link(&e.subject)
                ),
                false,
            )
        })
    } else {
        rule.and_then(|r| subject_href(cfg, r, &e.subject))
    };
    let ts = e.ts.max(0) as u64;
    html! {
        li.ev.rl[href.is_some()] {
            span.when { (clock_tag(ts)) }
            (pill(cls, word))
            @if !e.subject.is_empty() { (subject_link(href, &e.subject, "nm")) }
            @if let Some(r) = rule { span.ev-rule { "rule " b { (r) } } }
            @if !rest.is_empty() { span.muted.detail { (rest) } }
            @if names.contains(&e.subject) {
                a.ia.logs href=(crate::logs::logs_at(&e.subject, e.ts)) title="Logs at that moment"
                    aria-label={"Logs of " (e.subject) " at that moment"} { (ic("logs")) }
            }
        }
    }
}

/// One-line description of what a rule watches.
pub fn describe(r: &RuleConfig) -> String {
    match r.kind {
        RuleKind::Container => {
            let on =
                r.on.as_ref()
                    .map(|v| v.join(", "))
                    .unwrap_or_else(|| "stopped, unhealthy, restarting".into());
            let which = r
                .containers
                .as_ref()
                .filter(|v| !v.is_empty())
                .map(|v| v.join(", "))
                .unwrap_or_else(|| "all containers".into());
            format!("{which}: {on}")
        }
        RuleKind::Http => format!(
            "GET {} expects {}",
            r.url.as_deref().unwrap_or("?"),
            r.expect_status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "2xx".into())
        ),
        RuleKind::Host => {
            let m = r
                .metric
                .map(|m| format!("{m:?}").to_lowercase())
                .unwrap_or_else(|| "?".into());
            match (r.above, r.below) {
                (Some(a), _) => format!("{m} above {a}"),
                (_, Some(b)) => format!("{m} below {b}"),
                _ => m,
            }
        }
        RuleKind::LogMatch => crate::logs::rule_summary(r),
        RuleKind::Widget => format!(
            "widget {}{}",
            r.widget.as_deref().unwrap_or("?"),
            r.field
                .as_deref()
                .map(|f| format!(" / {f}"))
                .unwrap_or_default()
        ),
    }
}

fn rules_table(cfg: &Config, status: &Status) -> Markup {
    if cfg.alerts.is_empty() {
        return html! { p.muted { "No rules. Channels are configured but nothing uses them." } };
    }
    html! {
        div.scroll-x {
            table.tbl.small {
                thead { tr { th { "Rule" } th { "Watches" } th { "Notifies" } th { "State" } } }
                tbody {
                    @for r in &cfg.alerts {
                        @let n = status.incidents.iter().filter(|i| i.rule == r.name).count();
                        tr.stopped[!r.enabled()] {
                            td { span.nm { (r.name) } br; span.muted.k { (r.kind.as_str()) } }
                            td.wrap { (describe(r)) }
                            td {
                                @if r.channels.is_empty() { span.muted { "page only" } }
                                @else { @for c in &r.channels { span.chip-s { (c) } " " } }
                            }
                            td {
                                @if !r.enabled() { (pill("off", "disabled")) }
                                @else if n > 0 { (pill("bad", &format!("{n} firing"))) }
                                @else { (pill("ok", "ok")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn channels_table(cfg: &Config, status: &Status, now: u64) -> Markup {
    if cfg.channels.is_empty() {
        return html! { p.muted { "No channels: incidents show on this page only. Add " code { "[channels.<name>]" } " to send them somewhere." } };
    }
    html! {
        div.scroll-x {
            table.tbl.small {
                thead { tr { th { "Channel" } th { "Delivery" } th.num { "Sent" } th.num { "Failed" } th { "Last ok" } } }
                tbody {
                    @for (name, c) in &cfg.channels {
                        @let st: Option<&ChannelStatus> = status.channels.get(name);
                        tr {
                            td { span.nm { (name) } br; span.muted.k { (c.kind.as_str()) } }
                            td {
                                @match st.map(|s| s.health) {
                                    Some(Health::Ok) => (pill("ok", "ok")),
                                    Some(Health::Failing) => (pill("bad", "failing")),
                                    Some(Health::Misconfigured) => (pill("warn", "misconfigured")),
                                    Some(Health::Idle) | None => (pill("off", "not used yet")),
                                }
                                @if let Some(e) = st.and_then(|s| s.error.as_deref()) { div.muted.wrap.err-line { (e) } }
                            }
                            td.num { (st.map(|s| s.sent).unwrap_or(0)) }
                            td.num { (st.map(|s| s.failed).unwrap_or(0))
                                @if st.is_some_and(|s| s.pending > 0) { " (" (st.map(|s| s.pending).unwrap_or(0)) " queued)" } }
                            td { @if let Some(t) = st.and_then(|s| s.last_success) { (time_tag(now, t)) } @else { span.muted { "—" } } }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::Incident;

    fn cfg(toml: &str) -> Config {
        crate::config::parse(toml, "t.toml").unwrap()
    }

    #[test]
    fn rules_describe_themselves() {
        let c = cfg(r#"
[[alerts]]
name = "c"
type = "container"
containers = ["a-*"]
on = ["unhealthy"]
[[alerts]]
name = "d"
type = "host"
metric = "disk"
above = 90
[[alerts]]
name = "h"
type = "http"
url = "http://x/"
expect_status = 200
"#);
        assert_eq!(describe(&c.alerts[0]), "a-*: unhealthy");
        assert_eq!(describe(&c.alerts[1]), "disk above 90");
        assert_eq!(describe(&c.alerts[2]), "GET http://x/ expects 200");
    }

    /// Both live parts, as the SSE bundle and a signed-out page have them.
    fn parts(c: &Config, st: &Status, now: u64, names: &HashSet<String>) -> String {
        html! { (live(c, st, names, false)) (public(c, st, now)) }.into_string()
    }

    #[test]
    fn page_part_states() {
        let now = crate::util::unix_now();
        let none = HashSet::new();
        let empty = parts(&Config::default(), &Status::default(), now, &none);
        assert!(empty.starts_with(r#"<div id="al-live">"#), "{empty}");
        assert!(empty.contains(r#"<div id="al-pub">"#), "{empty}");
        assert!(empty.contains("No alert rules"));
        let c = cfg(r#"
[channels.hook]
type = "webhook"
url = "http://x/"
[[alerts]]
name = "<r>"
type = "container"
channels = ["hook"]
[[alerts]]
name = "mem"
type = "host"
metric = "memory"
above = 90
"#);
        let mut st = Status::default();
        st.incidents.push(Incident {
            rule: "<r>".into(),
            subject: "web".into(),
            state: "stopped (exit 1)".into(),
            value: "1".into(),
            since: now - 120,
            notified: false,
        });
        st.incidents.push(Incident {
            rule: "mem".into(),
            subject: "memory".into(),
            state: "92% > 90%".into(),
            value: "92".into(),
            since: now - 60,
            notified: true,
        });
        st.channels.insert(
            "hook".into(),
            ChannelStatus {
                kind: crate::alerts::ChannelKind::Webhook,
                health: Health::Failing,
                error: Some("HTTP 500 <b>".into()),
                last_success: None,
                last_failure: Some(now),
                sent: 3,
                failed: 2,
                dropped: 0,
                pending: 1,
            },
        );
        let html = parts(&c, &st, now, &none);
        assert!(html.contains("1 firing") && html.contains("2m"), "{html}");
        assert!(
            !html.contains("/logs"),
            "no logs link for an unknown subject"
        );
        // Each incident: a whole-row link to its subject, the rule's name
        // and a clock time on the row (LG-55, LG-56).
        assert!(html.contains(r#"<li class="incident rl">"#), "{html}");
        assert!(
            html.contains(r#"<a class="inc-subject row-link" href="/containers/web">web</a>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<a class="inc-subject row-link" href="/host">memory</a>"#),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<span class="inc-rule">rule <b data-rule="&lt;r&gt;">&lt;r&gt;</b></span>"#
            ),
            "{html}"
        );
        // Signed in, the rule's name opens its dialog on this page.
        let signed = html! { (live(&c, &st, &none, true)) }.into_string();
        assert!(
            signed.contains(r#"rule <a class="inc-rule-a" href="/alerts?edit=rule:%3Cr%3E" data-rule="&lt;r&gt;">&lt;r&gt;</a>"#),
            "{signed}"
        );
        assert!(
            html.contains(r#"since <time datetime="#) && html.contains(r#"data-abs="1""#),
            "{html}"
        );
        // An incident about a container: one click to its logs from the
        // moment it went bad.
        let names: HashSet<String> = ["web".to_string()].into();
        let html = parts(&c, &st, now, &names);
        let at = format!(
            r#"<a class="ia logs" href="/containers/web/logs#t={}" title="Logs from when it started" aria-label="Logs of web from when it started">"#,
            (now - 120) * 1000
        );
        assert!(html.contains(&at), "{html}");
        assert!(html.contains("&lt;r&gt;") && !html.contains("<r>"));
        assert!(html.contains("failing") && html.contains("HTTP 500 &lt;b&gt;"));
        assert!(html.contains("cooldown"));
        assert!(html.contains("(1 queued)"));
    }

    #[test]
    fn log_rows_link_their_subject_and_name_the_rule() {
        let c = cfg(r#"
[[alerts]]
name = "down"
type = "container"
[[alerts]]
name = "site"
type = "http"
url = "https://example.com/"
"#);
        let ev = |kind: &str, subject: &str, detail: &str| crate::db::Event {
            id: 0,
            ts: 1_700_000_000,
            kind: kind.into(),
            level: "warn".into(),
            subject: subject.into(),
            detail: detail.into(),
        };
        let names: HashSet<String> = ["web".to_string()].into();
        let h = log_row(
            &c,
            &ev("alert.fired", "web", "down: stopped (exit 1)"),
            &names,
            false,
        )
        .into_string();
        assert!(h.starts_with(r#"<li class="ev rl">"#), "{h}");
        assert!(
            h.contains(
                r#"<time datetime="2023-11-14T22:13:20Z" data-t="1700000000" data-abs="1">"#
            ),
            "{h}"
        );
        assert!(
            h.contains(">Fired<")
                && h.contains(r#"<a class="nm row-link" href="/containers/web">web</a>"#),
            "{h}"
        );
        assert!(
            h.contains(r#"rule <b>down</b>"#) && h.contains(">stopped (exit 1)<"),
            "{h}"
        );
        assert!(
            h.contains(r#"href="/containers/web/logs#t=1700000000000""#),
            "{h}"
        );
        let h = log_row(
            &c,
            &ev("alert.resolved", "https://example.com/", "site: recovered"),
            &names,
            false,
        )
        .into_string();
        assert!(
            h.contains(r#"href="https://example.com/" target="_blank""#) && !h.contains("/logs"),
            "{h}"
        );
        // A rule that no longer exists: plain text, no link, no rule.
        let h = log_row(&c, &ev("alert.fired", "x", "gone: bad"), &names, false).into_string();
        assert!(
            h.starts_with(r#"<li class="ev">"#) && !h.contains("href") && h.contains("gone: bad"),
            "{h}"
        );
        // A failed delivery opens its channel only for a session.
        let d = ev("alert.delivery_failed", "hook", "HTTP 500");
        assert!(!log_row(&c, &d, &names, false)
            .into_string()
            .contains("href"));
        assert!(log_row(&c, &d, &names, true)
            .into_string()
            .contains(r#"href="/alerts?edit=channel:hook""#));
    }
}
