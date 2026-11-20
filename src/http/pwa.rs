//! Installable app: `/manifest.webmanifest` and the icons (`/icon.svg`,
//! `/icon-192.png`, `/icon-512.png`, `/apple-touch-icon.png`).
//!
//! There is deliberately **no service worker**: a monitor that shows cached
//! numbers while the server is unreachable would be lying (V3-PLAN §4). Chrome
//! and Safari install a site with a manifest and icons without one. With the
//! network gone the browser shows its own error page (the old `/offline`
//! page could never be reached without a worker, so it was removed, NAV-70).
//!
//! Every icon is built by `docs/brand/src/build.py` from the traced mark: the
//! SVG tile is the one-colour glyph, the PNGs are the faceted cut on a full-bleed
//! ground so they also serve as maskable icons (docs/BRANDING.md). No image crate
//! at runtime.

use hyper::{Response, StatusCode};

use crate::http::server::{full, set, Body};
use crate::state::AppState;

const ICON_192: &[u8] = include_bytes!("pwa/icon-192.png");
const ICON_512: &[u8] = include_bytes!("pwa/icon-512.png");
/// 180 px, opaque: iOS ignores the manifest and wants exactly this.
const ICON_APPLE: &[u8] = include_bytes!("pwa/apple-touch-icon.png");

/// The logo tile as a standalone SVG.
pub const ICON_SVG: &str = include_str!("pwa/hobnail.svg");

/// Returns `None` for paths it does not own.
pub fn route(state: &AppState, path: &str) -> Option<Response<Body>> {
    Some(match path {
        "/manifest.webmanifest" => {
            let cfg = state.config.current();
            let name = cfg
                .page
                .title
                .clone()
                .unwrap_or_else(|| state.env.hostname.clone());
            asset(manifest(&name).into_bytes(), "application/manifest+json")
        }
        "/favicon.ico" | "/icon.svg" => asset(ICON_SVG.as_bytes().to_vec(), "image/svg+xml"),
        "/icon-192.png" => asset(ICON_192.to_vec(), "image/png"),
        "/icon-512.png" => asset(ICON_512.to_vec(), "image/png"),
        "/apple-touch-icon.png" => asset(ICON_APPLE.to_vec(), "image/png"),
        _ => return None,
    })
}

fn asset(body: Vec<u8>, ctype: &str) -> Response<Body> {
    let mut r = full(StatusCode::OK, body);
    set(&mut r, hyper::header::CONTENT_TYPE, ctype);
    set(
        &mut r,
        hyper::header::CACHE_CONTROL,
        "public, max-age=86400",
    );
    r
}

/// The web app manifest. `name` is the page title (escaped as JSON).
pub fn manifest(name: &str) -> String {
    serde_json::json!({
        "id": "/",
        "name": format!("Hobnail · {name}"),
        "short_name": "Hobnail",
        "description": "A tiny home for your homelab. Containers, host and history at a glance.",
        "start_url": "/app",
        "scope": "/app",
        "display": "standalone",
        "background_color": "#1D1210",
        "theme_color": "#1D1210",
        "icons": [
            { "src": "/icon.svg", "sizes": "any", "type": "image/svg+xml", "purpose": "any" },
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" }
        ],
        "shortcuts": [
            { "name": "Containers", "url": "/app/containers" },
            { "name": "Logs", "url": "/app/logs" },
            { "name": "Alerts", "url": "/app/alerts" }
        ]
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json_and_escapes() {
        let m = manifest("my \"box\" <x>");
        let v: serde_json::Value = serde_json::from_str(&m).unwrap();
        assert_eq!(v["name"], "Hobnail · my \"box\" <x>");
        assert_eq!(v["short_name"], "Hobnail");
        assert_eq!(v["display"], "standalone");
        assert_eq!(v["start_url"], "/app");
        for shortcut in v["shortcuts"].as_array().unwrap() {
            assert!(shortcut["url"].as_str().unwrap().starts_with("/app/"));
        }
        assert_eq!(v["icons"].as_array().unwrap().len(), 3);
        assert!(m.chars().count() < 2000);
    }

    #[test]
    fn icons_are_pngs() {
        for b in [ICON_192, ICON_512, ICON_APPLE] {
            assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
        }
        // IHDR width
        assert_eq!(
            u32::from_be_bytes(ICON_192[16..20].try_into().unwrap()),
            192
        );
        assert_eq!(
            u32::from_be_bytes(ICON_512[16..20].try_into().unwrap()),
            512
        );
        assert_eq!(
            u32::from_be_bytes(ICON_APPLE[16..20].try_into().unwrap()),
            180
        );
        // Compiled into the binary: keep them small (docs/BRANDING.md).
        assert!(ICON_192.len() <= 8 * 1024 && ICON_APPLE.len() <= 8 * 1024);
        assert!(ICON_512.len() <= 24 * 1024);
    }
}
