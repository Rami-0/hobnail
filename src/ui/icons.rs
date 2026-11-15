//! Icons: local files, a small built-in set, and dashboard-icons names with an
//! on-disk cache.
//!
//! Resolution of an icon name (from config, a label, or auto-detection):
//! - `foo.svg` / `foo.png`  → `data/icons/foo.svg` (your own files)
//! - `builtin:database`     → inline SVG from the built-in set (no request)
//! - `beszel` (plain name)  → `/icons/di/beszel.svg`: served from
//!   `data/icons/di/` if cached; otherwise fetched once from the
//!   dashboard-icons CDN **only if** `[icons] remote = true`, then cached, so
//!   the page keeps working with no internet. A miss serves a generated
//!   letter-avatar SVG (never a broken image).
//!
//! Containers without an icon get one guessed from their image: a
//! dashboard-icons name when remote fetching is on, else a
//! built-in glyph by category (database, mail, logs, metrics …), else a
//! letter avatar.

use hyper::{Response, StatusCode};
use maud::{html, Markup, PreEscaped};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::http::server::{full, set, text, Body};
use crate::state::AppState;

/// Built-in glyphs: 24×24, stroke = currentColor. Static strings we wrote,
/// so they are safe to emit unescaped.
const BUILTIN: &[(&str, &str)] = &[
    (
        "database",
        r#"<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>"#,
    ),
    (
        "mail",
        r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 7 9 6 9-6"/>"#,
    ),
    (
        "globe",
        r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z"/>"#,
    ),
    (
        "terminal",
        r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#,
    ),
    ("chart", r#"<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>"#),
    (
        "box",
        r#"<path d="m12 3 8 4.5v9L12 21l-8-4.5v-9z"/><path d="m4 7.5 8 4.5 8-4.5M12 12v9"/>"#,
    ),
    (
        "server",
        r#"<rect x="3" y="4" width="18" height="7" rx="1.5"/><rect x="3" y="13" width="18" height="7" rx="1.5"/><path d="M7 7.5h.01M7 16.5h.01"/>"#,
    ),
    ("bookmark", r#"<path d="M6 3h12v18l-6-4-6 4z"/>"#),
    (
        "bot",
        r#"<rect x="4" y="8" width="16" height="12" rx="3"/><path d="M12 4v4M9 13h.01M15 13h.01M9 17h6"/>"#,
    ),
    (
        "storage",
        r#"<path d="M4 7h16l-1.5 12h-13z"/><path d="M8 7V5h8v2"/>"#,
    ),
];

/// Image substring → (dashboard-icons name, built-in fallback glyph).
const AUTO: &[(&str, &str, &str)] = &[
    ("pgvector", "postgres", "database"),
    ("postgres", "postgres", "database"),
    ("pgbouncer", "postgres", "database"),
    ("mysql", "mysql", "database"),
    ("mariadb", "mariadb", "database"),
    ("redis", "redis", "database"),
    ("mongo", "mongodb", "database"),
    ("minio", "minio", "storage"),
    ("mailpit", "mailpit", "mail"),
    ("dozzle", "dozzle", "terminal"),
    ("beszel", "beszel", "chart"),
    ("dockge", "dockge", "box"),
    ("homepage", "homepage", "globe"),
    ("odoo", "odoo", "globe"),
    ("nginx", "nginx", "globe"),
    ("caddy", "caddy", "globe"),
    ("traefik", "traefik", "globe"),
    ("grafana", "grafana", "chart"),
    ("prometheus", "prometheus", "chart"),
    ("node", "nodejs", "server"),
    ("llm", "", "bot"),
];

/// Guess an icon name for a container image, e.g. `postgres:16-alpine`.
/// Returns a dashboard-icons name or `builtin:<glyph>`.
pub fn auto_icon(image: &str, remote: bool) -> Option<String> {
    let img = image.to_ascii_lowercase();
    let repo = img.rsplit('/').next().unwrap_or(&img);
    let repo = repo.split([':', '@']).next().unwrap_or(repo);
    for (needle, di, glyph) in AUTO {
        if repo.contains(needle) || img.contains(needle) {
            if !di.is_empty() && remote {
                return Some(di.to_string());
            }
            return Some(format!("builtin:{glyph}"));
        }
    }
    None
}

fn builtin(name: &str) -> Option<&'static str> {
    BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

/// Markup for an icon name (see module docs); `label` feeds the avatar.
pub fn render(name: Option<&str>, label: &str) -> Markup {
    let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return super::letter_avatar(label);
    };
    if let Some(g) = n.strip_prefix("builtin:") {
        if let Some(paths) = builtin(g) {
            return html! {
                span.icon.glyph aria-hidden="true" {
                    svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                        stroke-linecap="round" stroke-linejoin="round" { (PreEscaped(paths)) }
                }
            };
        }
        return super::letter_avatar(label);
    }
    if !super::is_safe_name(n) {
        return super::letter_avatar(label);
    }
    if n.ends_with(".svg") || n.ends_with(".png") {
        return html! { img.icon src={"/icons/" (n)} alt="" loading="lazy"; };
    }
    html! { img.icon src={"/icons/di/" (n) ".svg?l=" (avatar_letter(label))} alt="" loading="lazy"; }
}

fn avatar_letter(label: &str) -> char {
    label
        .chars()
        .find(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?')
}

fn cached_path(data_dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    data_dir
        .join("icons")
        .join("di")
        .join(format!("{name}.svg"))
}

/// Recently failed fetches, so a missing icon is not re-fetched per request.
static MISSES: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
const MISS_TTL: Duration = Duration::from_secs(3600);

/// `GET /icons/di/<name>.svg?l=X`
pub async fn dashboard_icon(state: &AppState, file: &str, letter: Option<char>) -> Response<Body> {
    let Some(name) = file.strip_suffix(".svg") else {
        return text(StatusCode::NOT_FOUND, "not found\n");
    };
    if !super::is_safe_name(name) || name.contains('.') {
        return text(StatusCode::BAD_REQUEST, "bad icon name\n");
    }
    let path = cached_path(&state.env.data_dir, name);
    if let Ok(b) = std::fs::read(&path) {
        return svg_response(b, "public, max-age=86400");
    }
    let cfg = state.config.current();
    let recently_missed = MISSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(name))
        .is_some_and(|t| t.elapsed() < MISS_TTL);
    if cfg.icons.remote && !recently_missed {
        let url = format!("{}/svg/{name}.svg", cfg.icons.cdn.trim_end_matches('/'));
        let req = crate::http::client::Request::get(url).timeout(Duration::from_secs(5));
        match crate::http::client::send(req, &cfg.http).await {
            Ok(r) if r.is_success() && looks_like_svg(&r.body) && r.body.len() < 512 * 1024 => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let tmp = path.with_extension("svg.tmp");
                if std::fs::write(&tmp, &r.body).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
                crate::info!("icon cached", name = name);
                return svg_response(r.body.to_vec(), "public, max-age=86400");
            }
            Ok(r) => crate::info!("icon not found", name = name, status = r.status),
            Err(e) => crate::warn!("icon fetch failed", name = name, error = e),
        }
        MISSES
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), Instant::now());
    }
    let l = letter.filter(|c| c.is_ascii_alphanumeric()).unwrap_or('?');
    svg_response(avatar_svg(name, l).into_bytes(), "no-cache")
}

fn looks_like_svg(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase();
    head.contains("<svg")
}

fn avatar_svg(seed: &str, letter: char) -> String {
    let hue = seed
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 360;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36"><rect width="36" height="36" rx="8" fill="hsl({hue} 45% 42%)"/><text x="18" y="24" font-family="system-ui,sans-serif" font-size="17" font-weight="700" fill="#fff" text-anchor="middle">{letter}</text></svg>"##
    )
}

fn svg_response(body: Vec<u8>, cache: &str) -> Response<Body> {
    let mut r = full(StatusCode::OK, body);
    set(&mut r, hyper::header::CONTENT_TYPE, "image/svg+xml");
    set(&mut r, hyper::header::CACHE_CONTROL, cache);
    // Third-party SVG on our origin: never let it run script if opened directly.
    set(
        &mut r,
        hyper::header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; style-src 'unsafe-inline'; img-src data:",
    );
    set(&mut r, hyper::header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_icons() {
        assert_eq!(
            auto_icon("postgres:16-alpine", false).as_deref(),
            Some("builtin:database")
        );
        assert_eq!(
            auto_icon("postgres:16-alpine", true).as_deref(),
            Some("postgres")
        );
        assert_eq!(
            auto_icon("axllent/mailpit:v1.31.6", false).as_deref(),
            Some("builtin:mail")
        );
        assert_eq!(
            auto_icon("henrygd/beszel-agent:latest", true).as_deref(),
            Some("beszel")
        );
        assert_eq!(auto_icon("webshop-platform:dev", true), None);
    }

    #[test]
    fn render_kinds() {
        assert!(render(Some("builtin:database"), "x")
            .into_string()
            .contains("<svg"));
        assert!(render(Some("beszel"), "Beszel")
            .into_string()
            .contains("/icons/di/beszel.svg?l=B"));
        assert!(render(Some("me.png"), "x")
            .into_string()
            .contains("/icons/me.png"));
        assert!(render(Some("../etc"), "Zed")
            .into_string()
            .contains("avatar"));
        assert!(render(None, "Zed").into_string().contains(">Z<"));
        assert!(avatar_svg("a", 'Q').contains(">Q<"));
    }
}
