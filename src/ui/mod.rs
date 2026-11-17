//! UI building blocks: the shell, pages, charts and the shared markup helpers
//! (icons, status dots, pills). CSS lives in `static/` (tokens, components).
//!
//! Class names used across pages (static/app.css): `.panel`, `.tile`,
//! `.pill` + `.ok|.warn|.bad|.off`, `.dot` + `.ok|.warn|.bad|.off`,
//! `.tbl`, `.seg`, `.btn`, `.empty`, `.muted`, `.mono`, `.sr`.

pub mod alerts;
pub mod chart;
pub mod fmt;
pub mod icons;
pub mod pages;
pub mod pwa;
pub mod routes;
pub mod settings;
pub mod shell;
pub mod sprite;
pub mod view;
pub mod widgets;

#[cfg(test)]
pub mod testutil;

pub use sprite::ic;

use hyper::{Response, StatusCode};
use maud::{html, Markup};

use crate::ui::view::{CardView, Dot};

use crate::http::server::{full, set, text, Body};
use crate::state::AppState;

pub const FAVICON: &str = "/icon.svg";

/// A status dot with its state as text for assistive tech (and a tooltip), so
/// the state is never conveyed by colour alone. Pulses while starting or
/// restarting.
pub fn status_dot(c: &CardView) -> Markup {
    let dot = c.dot.unwrap_or(Dot::Off);
    let pulse = matches!(c.label, "starting" | "restarting");
    let class = if pulse {
        format!("dot {} pulse", dot.class())
    } else {
        format!("dot {}", dot.class())
    };
    html! {
        span class=(class) role="img" aria-label=(c.label) title=(c.label) {}
    }
}

/// A pill badge. `kind` is `ok`, `warn`, `bad`, `off` or empty (neutral).
pub fn pill(kind: &str, text: &str) -> Markup {
    html! { span class={"pill " (kind)} { (text) } }
}

/// What an empty state is about (DESIGN.md §11.9); it sets the icon's tone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Empty {
    /// Nothing here yet (first use, no history yet).
    First,
    /// A filter hides everything.
    NoMatch,
    /// Nothing is wrong: nothing stopped, nothing firing.
    Good,
    /// Could not load: Docker unreachable, a failing source.
    Error,
}

/// An empty state: icon, headline and a sentence of help.
pub fn empty_state(icon: &str, title: &str, body: Markup) -> Markup {
    empty(Empty::First, "", icon, title, body, None)
}

/// The one empty-state component (§11.9): icon, a one-line title, one
/// sentence, at most one action. `size` is "" (a page or panel),
/// `compact` (inside a panel) or `inline` (a list row's height). Always
/// rendered by the server, so it shows with the script blocked.
pub fn empty(
    kind: Empty,
    size: &str,
    icon: &str,
    title: &str,
    body: Markup,
    action: Option<Markup>,
) -> Markup {
    let tone = match kind {
        Empty::Good => " good",
        Empty::Error => " bad",
        Empty::First | Empty::NoMatch => "",
    };
    let sp = if size.is_empty() { "" } else { " " };
    html! {
        div class={"empty" (tone) (sp) (size)} {
            span.empty-ic { (ic(icon)) }
            h3 { (title) }
            p { (body) }
            @if let Some(a) = action { div.acts { (a) } }
        }
    }
}

/// A value that has not arrived yet (a first sample): a skeleton bar once
/// the script has run (`html.hb-ready`; it shows after 150 ms, §11.9), "—"
/// without it. The region around it carries `data-wait`, which the script
/// turns into `aria-busy` (never the server: with the script off, blocked
/// or failed, nothing is busy).
pub fn skel_word() -> Markup {
    html! { span.sk aria-hidden="true" {} span.sk-t { "—" } }
}

/// A panel heading with an optional link on the right.
pub fn panel_head(title: &str, link: Option<(&str, &str)>) -> Markup {
    html! {
        div.panel-h {
            h2 { (title) }
            @if let Some((href, label)) = link {
                a.more href=(href) { (label) (ic("arrow-right")) }
            }
        }
    }
}

/// Render an icon by name (see `icons` for the resolution rules), falling
/// back to a letter avatar colored from `label`.
pub fn icon(name: Option<&str>, label: &str) -> Markup {
    icons::render(name, label)
}

pub fn letter_avatar(label: &str) -> Markup {
    let letter = label
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?');
    let hue = label
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 360;
    html! { span.icon.avatar style={"--h:" (hue)} aria-hidden="true" { (letter) } }
}

pub fn is_safe_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() < 100
        && !n.starts_with('.')
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `GET /icons/<name>`: serve from `data/icons/`.
pub fn icon_response(state: &AppState, name: &str) -> Response<Body> {
    if !is_safe_name(name) {
        return text(StatusCode::BAD_REQUEST, "bad icon name\n");
    }
    let ctype = if name.ends_with(".svg") {
        "image/svg+xml"
    } else if name.ends_with(".png") {
        "image/png"
    } else {
        return text(StatusCode::NOT_FOUND, "not found\n");
    };
    match std::fs::read(state.env.data_dir.join("icons").join(name)) {
        Ok(b) => {
            let mut r = full(StatusCode::OK, b);
            set(&mut r, hyper::header::CONTENT_TYPE, ctype);
            set(
                &mut r,
                hyper::header::CACHE_CONTROL,
                "public, max-age=86400",
            );
            r
        }
        Err(_) => text(StatusCode::NOT_FOUND, "not found\n"),
    }
}
