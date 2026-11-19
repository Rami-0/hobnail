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
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::http::server::{full, set, text, Body};
use crate::state::AppState;

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

fn cached_path(data_dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    data_dir
        .join("icons")
        .join("di")
        .join(format!("{name}.svg"))
}

/// Recently failed fetches, so a missing icon is not re-fetched per request.
static MISSES: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
const MAX_MISSES: usize = 512;
const MISS_TTL: Duration = Duration::from_secs(3600);

fn cache_miss(misses: &mut HashMap<String, Instant>, name: &str) {
    misses.retain(|_, time| time.elapsed() < MISS_TTL);
    if misses.len() >= MAX_MISSES && !misses.contains_key(name) {
        if let Some(oldest) = misses
            .iter()
            .min_by_key(|(_, time)| *time)
            .map(|(name, _)| name.clone())
        {
            misses.remove(&oldest);
        }
    }
    misses.insert(name.to_string(), Instant::now());
}

/// `GET /icons/di/<name>.svg?l=X`
pub async fn dashboard_icon(state: &AppState, file: &str, letter: Option<char>) -> Response<Body> {
    let Some(name) = file.strip_suffix(".svg") else {
        return text(StatusCode::NOT_FOUND, "not found\n");
    };
    if !is_safe_name(name) || name.contains('.') {
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
        cache_miss(
            MISSES.lock().unwrap().get_or_insert_with(HashMap::new),
            name,
        );
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
    fn miss_cache_expires_entries_and_limits_size() {
        let mut misses = HashMap::from([("expired".into(), Instant::now() - MISS_TTL)]);
        for i in 0..MAX_MISSES + 1 {
            cache_miss(&mut misses, &i.to_string());
        }
        assert_eq!(misses.len(), MAX_MISSES);
        assert!(!misses.contains_key("expired"));
        assert!(misses.contains_key(&MAX_MISSES.to_string()));
    }

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
}

fn is_safe_name(n: &str) -> bool {
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
