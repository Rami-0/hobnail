//! `/alerts`: rules and their state, channels and delivery status, what is
//! firing now, and the alert log from the history database.

use maud::{html, Markup};
use std::collections::HashSet;

use crate::alerts::{ChannelStatus, Health, RuleConfig, RuleKind, Status};
use crate::config::Config;
use crate::db::EventFilter;
use crate::ui::fmt::time_tag;
use crate::ui::pages::overview::event_row;
use crate::ui::pages::page_sub;
use crate::ui::shell::{layout, Ctx, Nav};
use crate::ui::{empty_state, panel_head, pill};

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
    let body = html! {
        div.page {
            (page_sub(html! { "Rules, notification channels and what has fired" }))
            (fragment(ctx))
            section.sec.ghost aria-label="Alert log" {
                (panel_head("Alert log", None))
                @if log.is_empty() {
                    p.muted {
                        @if ctx.state.db.is_enabled() { "Nothing has fired yet." }
                        @else { "Fired, resolved and failed notifications are logged here once history is recording." }
                    }
                } @else {
                    ul.events { @for e in &log { (event_row(ctx.now, e, false, Some(&names))) } }
                }
            }
        }
    };
    layout(
        ctx,
        Nav::Alerts,
        "Alerts",
        crate::ui::pages::crumb_title("Alerts"),
        body,
    )
}

/// The live part: firing list, rules and channels.
pub fn fragment(ctx: &Ctx) -> Markup {
    let status = ctx.state.alerts.lock().unwrap().clone();
    live(
        &ctx.cfg,
        &status,
        ctx.now,
        &crate::logs::tailable(ctx.state),
    )
}

fn live(cfg: &Config, status: &Status, now: u64, names: &HashSet<String>) -> Markup {
    html! {
        div #al-live {
            @if cfg.alerts.is_empty() && cfg.channels.is_empty() {
                (empty_state("bell", "No alert rules", html! {
                    "Nothing is watched and nothing is sent. Add " code { "[[alerts]]" } " rules and "
                    code { "[channels.*]" } " to the config to get notified."
                }))
            } @else {
                (crate::ui::alerts::render_with(cfg, status, names))
                @if let Some(e) = &status.error { p.bad.wrap { "alert state: " (e) } }
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

    #[test]
    fn page_part_states() {
        let now = crate::util::unix_now();
        let none = HashSet::new();
        let empty = live(&Config::default(), &Status::default(), now, &none).into_string();
        assert!(empty.starts_with(r#"<div id="al-live">"#), "{empty}");
        assert!(empty.contains("No alert rules"));
        let c = cfg(r#"
[channels.hook]
type = "webhook"
url = "http://x/"
[[alerts]]
name = "<r>"
type = "container"
channels = ["hook"]
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
        let html = live(&c, &st, now, &none).into_string();
        assert!(html.contains("1 firing") && html.contains("2m"), "{html}");
        assert!(
            !html.contains("/logs"),
            "no logs link for an unknown subject"
        );
        // An incident about a container: one click to its logs.
        let names: HashSet<String> = ["web".to_string()].into();
        let html = live(&c, &st, now, &names).into_string();
        assert!(html.contains(r#"<a class="ia logs" href="/containers/web/logs" title="Logs" aria-label="Logs of web">"#), "{html}");
        assert!(html.contains("&lt;r&gt;") && !html.contains("<r>"));
        assert!(html.contains("failing") && html.contains("HTTP 500 &lt;b&gt;"));
        assert!(html.contains("cooldown"));
        assert!(html.contains("(1 queued)"));
    }
}
