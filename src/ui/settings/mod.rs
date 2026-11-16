//! The settings pages: `/settings` (overview of the tabs) and
//! `/settings/<tab>` (DESIGN.md section 5), in the shell.
//!
//! Forms that moved onto the dialog (DESIGN.md §11.6; Bookmarks, Widgets and
//! Notifications so far, see the recipe in `dlg.rs`) are rendered with their
//! values by the server, from the file on disk, for a session only; the add
//! / edit dialog and the
//! `?add=` / `?edit=` page form come from `dlg.rs`, and `?frag=list` serves
//! a list again for in-place updates ([`fragment`]). The other tabs still
//! render their structure only: `static/settings.js` loads their values
//! from the settings JSON API (`/api/settings/*`, see `crate::settings`).
//! Every save is a `fetch` with `X-RH-CSRF` and `base_hash`; every form is
//! `method="post"` with its submit button disabled until the script runs,
//! so no value can ever end up in a URL (ROB-15). `static/settings.css`
//! and `settings.js` are linked from these pages only.
//!
//! Signed out, a tab shows only the lock panel (how to set the first
//! password, or a sign-in link) and no config data at all: no list, no
//! dialog, no `data-item`, no page form, and `?frag=` answers 401.
//!
//! Controls carry `data-t` (how the script converts a value: `list`, `num`,
//! `json`, `bool`, `tri`, `set`) and sit in `.fld` wrappers; a wrapper with
//! `data-for="a b"` is only shown (and only sent) for those entity types.

mod appearance;
mod bookmarks;
mod config;
mod containers;
mod disk;
mod dlg;
mod form;
mod general;
mod history;
mod notifications;
mod security;
mod widgets;

#[cfg(test)]
mod tests;

use hyper::StatusCode;
use maud::{html, Markup};

use crate::ui::fmt::{parse_query, query_get, url_encode};
use crate::ui::ic;
use crate::ui::pages::login::SET_PASSWORD_CMD;
use crate::ui::routes::SETTINGS;
use crate::ui::shell::{layout, Ctx, Nav};

/// Icon per tab slug (same order as `routes::SETTINGS`).
fn tab_icon(slug: &str) -> &'static str {
    match slug {
        "general" => "sliders",
        "containers" => "box",
        "bookmarks" => "bookmark",
        "widgets" => "zap",
        "notifications" => "bell",
        "appearance" => "sun",
        "history" => "history",
        "config" => "terminal",
        "security" => "lock",
        _ => "sliders",
    }
}

/// One settings tab: (name, slug, description).
pub type Tab = (&'static str, &'static str, &'static str);

/// The page for `/settings` (`tab = None`) or `/settings/<slug>`; `None`
/// when there is no such tab (the caller answers 404).
pub fn page(ctx: &Ctx, slug: Option<&str>) -> Option<String> {
    let tab: Option<&Tab> = match slug {
        None | Some("") => None,
        Some(s) => Some(SETTINGS.iter().find(|t| t.1 == s)?),
    };
    let signed = ctx.user.is_some();
    let none = html! {};
    let (content, dialogs, acts) = match (tab, signed) {
        (None, _) => (index(ctx), none, None),
        (Some(t), false) => (lock_panel(ctx, t.1), none, None),
        (Some(t), true) => match t.1 {
            "general" => (general::body(), none, None),
            "containers" => (containers::body(ctx), none, None),
            "bookmarks" => (
                bookmarks::body(ctx),
                bookmarks::dialogs(),
                Some(bookmarks::head_actions()),
            ),
            "widgets" => (widgets::body(ctx), widgets::dialogs(), None),
            "notifications" => (notifications::body(ctx), notifications::dialogs(ctx), None),
            "appearance" => (appearance::body(), none, None),
            "history" => (history::body(), none, None),
            "config" => (config::body(), none, None),
            "security" => (security::body(), none, None),
            _ => return None,
        },
    };
    Some(shell(ctx, tab, content, dialogs, acts))
}

/// `GET /settings/<tab>?frag=<name>`: one part of a tab, for the script to
/// swap in after a save. `None` when the request is not a fragment request.
/// Reads the session from the request (unlike the containers-table
/// fragment, which is public): without one the answer is 401 with an empty
/// body, whatever was asked for.
pub fn fragment(ctx: &Ctx) -> Option<(StatusCode, String)> {
    let slug = ctx.path.strip_prefix("/settings/")?;
    let q = parse_query(ctx.query);
    let frag = query_get(&q, "frag")?;
    if ctx.user.is_none() {
        return Some((StatusCode::UNAUTHORIZED, String::new()));
    }
    Some(match (slug, frag) {
        ("bookmarks", "list") => (
            StatusCode::OK,
            bookmarks::list_markup(&disk::Disk::read(ctx.state)).into_string(),
        ),
        ("widgets", "list") => (
            StatusCode::OK,
            widgets::list_markup(&disk::Disk::read(ctx.state)).into_string(),
        ),
        ("notifications", "list") => (StatusCode::OK, notifications::fragment(ctx).into_string()),
        _ => (StatusCode::NOT_FOUND, String::new()),
    })
}

fn shell(
    ctx: &Ctx,
    tab: Option<&Tab>,
    content: Markup,
    dialogs: Markup,
    acts: Option<Markup>,
) -> String {
    let sha = ctx.state.build.sha;
    let heading = match tab {
        None => html! { h1.title { "Settings" } },
        Some((name, _, _)) => html! {
            nav.crumb aria-label="Breadcrumb" {
                a href="/settings" { "Settings" }
                (ic("chev-r"))
                h1.title { (name) }
            }
        },
    };
    let slug = tab.map(|t| t.1).unwrap_or("");
    let signed = ctx.user.is_some();
    let body = html! {
        link rel="stylesheet" href={"/static/settings.css?v=" (sha)};
        div.page.set {
            nav.set-nav aria-label="Settings sections" {
                ul {
                    @for (name, s, _) in SETTINGS {
                        li {
                            a href={"/settings/" (s)} aria-current=[(*s == slug).then_some("page")] {
                                (ic(tab_icon(s))) span { (name) }
                            }
                        }
                    }
                }
            }
            div.set-main #set-main data-tab=(slug) data-signed=(if signed { "1" } else { "0" }) {
                @if let Some((name, _, desc)) = tab {
                    div.set-head {
                        div.set-head-t {
                            h2 { (name) }
                            p.page-sub { (desc) }
                        }
                        @if let Some(a) = acts { div.set-acts { (a) } }
                    }
                }
                div.set-banners #set-banners aria-live="assertive" {}
                (content)
            }
        }
        // Dialogs sit outside `#set-main`, whose forms the page-level
        // unsaved-edits check watches; the dialog has its own guard.
        @if signed {
            (dlg::confirm())
            (dialogs)
        }
        script src={"/static/settings.js?v=" (sha)} defer {}
    };
    let title = tab.map(|t| t.0).unwrap_or("Settings");
    layout(ctx, Nav::Settings, title, heading, body)
}

/// `/settings`: the tabs as cards, and the lock panel when signed out.
fn index(ctx: &Ctx) -> Markup {
    html! {
        div.set-head {
            div.set-head-t {
            h2 { "Settings" }
            p.page-sub { "Change the config from the browser. Every save is validated, keeps the file's comments and is kept as a revision." }
            }
        }
        @if ctx.user.is_none() { (lock_panel(ctx, "")) }
        ul.set-cards {
            @for (name, s, desc) in SETTINGS {
                li {
                    a.set-card href={"/settings/" (s)} {
                        span.set-card-ic { (ic(tab_icon(s))) }
                        span.set-card-t { (name) }
                        span.set-card-d { (desc) }
                    }
                }
            }
        }
    }
}

/// Signed out: how to unlock. Never shows any config data.
fn lock_panel(ctx: &Ctx, slug: &str) -> Markup {
    let next = if slug.is_empty() {
        "/settings".to_string()
    } else {
        format!("/settings/{slug}")
    };
    let why = "Changing settings needs a login because a config that can be edited \
               from a browser could run commands in a container that has the Docker socket.";
    html! {
        section.panel.set-lock aria-labelledby="lock-h" {
            div.lock-ic { (ic("lock")) }
            div.lock-body {
                @if !crate::auth::password_is_set(ctx.state) {
                    h3 #lock-h { "Settings are locked until a password is set on the host" }
                    p { (why) }
                    p { "Run this on the host, then sign in here:" }
                    div.copy-line {
                        pre.cmdline { code #set-pw-cmd { (SET_PASSWORD_CMD) } }
                        button.btn.sm type="button" data-copy="set-pw-cmd" { (ic("copy")) "Copy" }
                    }
                    p.muted { "There is no web form for the first password, so nobody on the network can claim it first. Viewing the dashboard stays open." }
                } @else {
                    h3 #lock-h { "Sign in to change settings" }
                    p { (why) " Viewing stays open." }
                    p {
                        a.btn.primary href={"/login?next=" (url_encode(&next))} { (ic("log-in")) "Sign in" }
                    }
                }
            }
        }
    }
}
