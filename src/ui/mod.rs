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

/// An empty state: icon, headline and a sentence of help.
pub fn empty_state(icon: &str, title: &str, body: Markup) -> Markup {
    html! {
        div.empty {
            (ic(icon))
            h3 { (title) }
            p { (body) }
        }
    }
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
