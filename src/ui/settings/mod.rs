//! The settings pages: `/settings/<tab>` (DESIGN.md section 5), in the
//! shell. `/settings` has no page of its own (SET-22): it is the General
//! tab, and the script points the Settings links at the tab this browser
//! used last (Y17, `rh-set-tab`).
//!
//! Every tab is rendered with its values by the server, from the file on
//! disk, for a session only (DESIGN.md §11.14, Y18). Lists use the dialog
//! pattern (the recipe in `dlg.rs`; `bookmarks.rs` is the reference): add /
//! edit in a dialog opened from the row, the `?add=` / `?edit=` page form
//! that stands in for it without script, and `?frag=list` serving the
//! lists again for in-place updates ([`fragment`]). Single settings with
//! several fields (General, Sample interval, custom.css, the raw editor,
//! Change password) are page-section forms on the same submit path
//! (`data-hb`); a single control (a switch, the default theme) saves when
//! changed. Every save is a `fetch` with `X-RH-CSRF` and the `base_hash` the
//! form, dialog, confirm or switch started from; every form is
//! `method="post"` with its submit button disabled until the script runs,
//! so no value can ever end up in a URL (ROB-15). `static/settings.css`
//! and `settings.js` are linked from these pages only.
//!
//! Signed out, a tab shows only the lock panel (how to set the first
//! password, or a sign-in link) and no config data at all: no tab strip
//! (Y11, SET-21), no list, no dialog, no `data-item`, no page form, and
//! `?frag=` answers 401.

mod appearance;
mod bookmarks;
mod config;
mod containers;
mod disk;
mod dlg;
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

/// The page for `/settings` (`slug = None`: the General tab) or
/// `/settings/<slug>`; `None` when there is no such tab (the caller answers
/// 404).
pub fn page(ctx: &Ctx, slug: Option<&str>) -> Option<String> {
    let t: &Tab = match slug {
        None | Some("") => &SETTINGS[0],
        Some(s) => SETTINGS.iter().find(|t| t.1 == s)?,
    };
    let signed = ctx.user.is_some();
    let none = html! {};
    let (content, dialogs, acts) = match signed {
        false => (lock_panel(ctx, slug.filter(|s| !s.is_empty())), none, None),
        true => match t.1 {
            "general" => (general::body(ctx), none, None),
            "containers" => (containers::body(ctx), containers::dialogs(ctx), None),
            "bookmarks" => (
                bookmarks::body(ctx),
                bookmarks::dialogs(),
                Some(bookmarks::head_actions()),
            ),
            "widgets" => (widgets::body(ctx), widgets::dialogs(), None),
            "notifications" => (notifications::body(ctx), notifications::dialogs(ctx), None),
            "appearance" => (appearance::body(ctx), none, None),
            "history" => (history::body(ctx), none, None),
            "config" => (config::body(ctx), config::dialogs(), None),
            "security" => (security::body(ctx), none, None),
            _ => return None,
        },
    };
    Some(shell(ctx, t, content, dialogs, acts))
}

/// A Settings tab's lists and dialogs carried by another page (the Alerts
/// page: owner decision Y15), **for a session only** (the caller checks
/// `ctx.user` first; signed out nothing of this is rendered). The markup,
/// the `?add=` / `?edit=` page forms, the dialogs and the script are the
/// tab's own: `settings.js` runs on the page as on the tab (`data-tab`),
/// reads the lists from the tab's `?frag=list` (`data-frag`) and saves
/// through the same routes with the same base-hash and patch rules.
pub struct Embed {
    /// `<link>` to settings.css (the lists' and forms' layout).
    pub head: Markup,
    /// The tab's body inside `#set-main` (with its banners region).
    pub body: Markup,
    /// The confirm, the tab's dialogs and settings.js (outside `#set-main`).
    pub tail: Markup,
    /// The config file's version the page was rendered from: a dialog
    /// opened from a trigger inside `[data-hash]` saves on this base.
    pub hash: String,
}

/// [`Embed`] of a tab: "notifications" (channels and rules), or a list of
/// dialog-only tabs ("bookmarks widgets": their dialogs, no list).
pub fn embed(ctx: &Ctx, tabs: &str) -> Option<Embed> {
    ctx.user.as_ref()?;
    let first = tabs.split(' ').next().unwrap_or("");
    let frag = format!("/settings/{first}");
    let hash = disk::Disk::read(ctx.state).hash;
    let (body, dialogs) = match tabs {
        "notifications" => (notifications::body(ctx), notifications::dialogs(ctx)),
        "bookmarks widgets" => (
            html! {},
            html! { (bookmarks::dialogs()) (widgets::dialogs()) },
        ),
        _ => return None,
    };
    Some(Embed {
        head: html! { link rel="stylesheet" href={"/static/settings.css?v=" (crate::http::server::SETTINGS_CSS_V)}; },
        body: html! {
            div.set-main.set-embed.bare[body.0.is_empty()] #set-main data-tab=(tabs) data-signed="1" data-frag=(frag) {
                div.set-banners #set-banners aria-live="assertive" {}
                (body)
            }
        },
        tail: html! {
            (dlg::confirm())
            (dialogs)
            script src={"/static/settings.js?v=" (crate::http::server::SETTINGS_JS_V)} defer {}
        },
        hash,
    })
}

/// `?edit=channel:<name>`: the link that opens a channel's dialog on a page
/// that carries it (the Notifications tab, or the Alerts page signed in).
pub fn channel_edit_link(name: &str) -> String {
    dlg::edit_link("channel", name)
}

/// `?edit=rule:<name>`: the link that opens a rule's dialog (the
/// Notifications tab, or the Alerts page signed in).
pub fn rule_edit_link(name: &str) -> String {
    dlg::edit_link("rule", name)
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
        ("containers", "list") => (StatusCode::OK, containers::fragment(ctx).into_string()),
        ("history", "list") => (StatusCode::OK, history::stats(ctx).into_string()),
        ("config", "list") => (StatusCode::OK, config::revisions(ctx).into_string()),
        _ => (StatusCode::NOT_FOUND, String::new()),
    })
}

fn shell(ctx: &Ctx, tab: &Tab, content: Markup, dialogs: Markup, acts: Option<Markup>) -> String {
    let (name, slug, desc) = *tab;
    let signed = ctx.user.is_some();
    // Signed out every tab is the same lock panel: no tab strip of links
    // that all lead to it (Y11), and the top bar just says "Settings".
    let heading = if signed {
        html! {
            nav.crumb aria-label="Breadcrumb" {
                a href="/settings" { "Settings" }
                (ic("chev-r"))
                h1.title { (name) }
            }
        }
    } else {
        html! { h1.title { "Settings" } }
    };
    let body = html! {
        link rel="stylesheet" href={"/static/settings.css?v=" (crate::http::server::SETTINGS_CSS_V)};
        div.page.set.locked[!signed] {
            @if signed {
                nav.set-nav #set-nav aria-label="Settings sections" {
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
            }
            div.set-main #set-main data-tab=(slug) data-signed=(if signed { "1" } else { "0" }) {
                @if signed {
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
        script src={"/static/settings.js?v=" (crate::http::server::SETTINGS_JS_V)} defer {}
    };
    let title = if signed { name } else { "Settings" };
    layout(ctx, Nav::Settings, title, heading, body)
}

/// Signed out: how to unlock. Never shows any config data.
/// `slug`: the tab asked for (`None` for `/settings`), where signing in
/// returns to.
fn lock_panel(ctx: &Ctx, slug: Option<&str>) -> Markup {
    let next = match slug {
        None => "/settings".to_string(),
        Some(s) => format!("/settings/{s}"),
    };
    let why = "Changing settings needs a login because a config that can be edited \
               from a browser could run commands in a container that has the Docker socket.";
    html! {
        section.panel.set-lock aria-labelledby="lock-h" {
            div.lock-ic { (ic("lock")) }
            div.lock-body {
                @if !crate::auth::password_is_set(ctx.state) {
                    h2 #lock-h { "Settings are locked until a password is set on the host" }
                    p { (why) }
                    p { "Run this on the host, then sign in here:" }
                    div.copy-line {
                        pre.cmdline { code #set-pw-cmd { (SET_PASSWORD_CMD) } }
                        button.btn.sm type="button" data-copy="set-pw-cmd" { (ic("copy")) "Copy" }
                    }
                    p.muted { "There is no web form for the first password, so nobody on the network can claim it first. Viewing the dashboard stays open." }
                } @else {
                    h2 #lock-h { "Sign in to change settings" }
                    p { (why) " Viewing stays open." }
                    p {
                        a.btn.primary href={"/login?next=" (url_encode(&next))} { (ic("log-in")) "Sign in" }
                    }
                }
            }
        }
    }
}
