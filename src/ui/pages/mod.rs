//! Full pages. `render` maps a path to a page; `bundle` re-renders the live
//! regions an SSE event touches.

pub mod alerts;
pub mod container;
pub mod containers;
pub mod history;
pub mod login;
pub mod overview;

use hyper::StatusCode;
use maud::{html, Markup};

use crate::ui::empty_state;
use crate::ui::fmt::url_decode;
use crate::ui::shell::{layout, Ctx, Nav};

/// The page for `ctx.path`, or the 404 page in the shell.
pub fn render(ctx: &Ctx) -> (StatusCode, String) {
    let ok = |s: String| (StatusCode::OK, s);
    match ctx.path {
        "/" => ok(overview::page(ctx)),
        "/containers" | "/containers/" => ok(containers::page(ctx)),
        "/history" => ok(history::page(ctx)),
        "/alerts" => ok(alerts::page(ctx)),
        "/login" => ok(login::page(ctx)),
        "/host" => crate::host::page(ctx),
        "/logs" => crate::logs::split_page(ctx),
        "/more" => ok(crate::ui::shell::more_page(ctx)),
        "/settings" | "/settings/" => ok(crate::ui::settings::page(ctx, None).unwrap_or_default()),
        p => {
            if let Some(slug) = p.strip_prefix("/settings/") {
                if let Some(page) = crate::ui::settings::page(ctx, Some(slug)) {
                    return ok(page);
                }
            }
            if let Some(stack) = p
                .strip_prefix("/stacks/")
                .and_then(|r| r.strip_suffix("/logs"))
                .filter(|r| !r.is_empty() && !r.contains('/'))
            {
                return crate::logs::stack_page(ctx, &url_decode(stack));
            }
            if let Some(name) = p
                .strip_prefix("/containers/")
                .and_then(|r| r.strip_suffix("/logs"))
                .filter(|r| !r.is_empty() && !r.contains('/'))
            {
                return crate::logs::container_page(ctx, &url_decode(name));
            }
            match p.strip_prefix("/containers/") {
                // Docker names never contain '/', so a decoded name with one
                // cannot match anything: it is a plain 404.
                Some(raw) if !raw.is_empty() && !raw.contains('/') => {
                    container::page(ctx, &url_decode(raw))
                }
                _ => not_found(ctx),
            }
        }
    }
}

/// A proper 404 inside the shell.
pub fn not_found(ctx: &Ctx) -> (StatusCode, String) {
    let body = html! {
        div.page {
            (empty_state(
                "search",
                "Page not found",
                html! {
                    "There is no page at " code { (ctx.path) } ". "
                    a href="/" { "Go to the overview" } " or press " kbd { "Ctrl K" } " to search."
                },
            ))
        }
    };
    (
        StatusCode::NOT_FOUND,
        layout(ctx, Nav::None, "Not found", crumb_title("Not found"), body),
    )
}

/// The top-bar heading for a page without a breadcrumb.
pub fn crumb_title(t: &str) -> Markup {
    html! { h1.title { (t) } }
}

/// Live regions to re-render when the server announces that `event` changed.
/// Regions that are not on the viewer's page are ignored by the script, so a
/// bundle holds every region an event can touch (a few KB).
pub fn bundle(ctx: &Ctx, event: &str) -> Option<Markup> {
    use crate::ui::shell;
    Some(match event {
        "groups" => {
            let v = overview::Live::new(ctx).with_since(ctx);
            html! {
                (shell::tree(&ctx.cfg, &v.snap, &v.groups))
                (shell::docker_banner(&ctx.cfg, &v.snap))
                (overview::health(&v))
                (overview::attention(&ctx.cfg, ctx.now, &v))
                (overview::top(&v, ctx.cfg.page.show_stats))
                (container::live_json(&v.groups))
            }
        }
        "sys" => {
            let sys = ctx.state.system.lock().unwrap().1.clone();
            html! {
                (shell::vitals(&ctx.cfg, sys.as_ref(), None))
                @if ctx.cfg.system.enabled { (overview::sys_strip(sys.as_ref(), None)) }
            }
        }
        "alerts" => {
            let v = overview::Live::new(ctx).with_since(ctx);
            html! {
                (shell::nav_alerts(v.status.incidents.len()))
                (overview::health(&v))
                (overview::attention(&ctx.cfg, ctx.now, &v))
                (alerts::fragment(ctx))
            }
        }
        other => {
            let wid = other.strip_prefix("w-")?;
            let w = ctx.cfg.widgets.iter().find(|w| w.id == wid)?;
            let out = ctx.state.widgets.lock().unwrap().get(wid).cloned();
            crate::ui::widgets::render(w, out.as_ref())
        }
    })
}

/// The last part of an event kind ("container.die" -> "die").
pub fn kind_label(kind: &str) -> &str {
    kind.rsplit('.').next().unwrap_or(kind)
}

/// Said once per page when the history database is off (the charts then
/// only say "History is off").
pub fn history_off_banner() -> Markup {
    html! {
        div.banner.info {
            (crate::ui::ic("info"))
            div {
                strong { "History is off." }
                " Nothing is being recorded yet, so these charts and timelines are empty. "
                "Live numbers on the overview and the containers page still update."
            }
        }
    }
}

/// The one-line description under the top bar (the title is in the top bar).
pub fn page_sub(sub: Markup) -> Markup {
    html! { p.page-sub { (sub) } }
}

#[cfg(test)]
mod tests;
