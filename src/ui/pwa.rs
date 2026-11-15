//! Installable app: `/manifest.webmanifest`, the icons (`/icon.svg`,
//! `/icon-192.png`, `/icon-512.png`, `/apple-touch-icon.png`) and `/offline`.
//!
//! There is deliberately **no service worker**: a monitor that shows cached
//! numbers while the server is unreachable would be lying (V3-PLAN §4). Chrome
//! and Safari install a site with a manifest and icons without one. `/offline`
//! is a plain page a user (or a future worker) can open; it explains that the
//! server is unreachable and offers a retry.
//!
//! Every icon is built by `docs/brand/src/build.py` from the traced mark: the
//! SVG tile is the one-colour glyph, the PNGs are the faceted cut on a full-bleed
//! ground so they also serve as maskable icons (docs/BRANDING.md). No image crate
//! at runtime.

use hyper::{Response, StatusCode};

use crate::http::server::{full, set, Body};
use crate::state::AppState;

const ICON_192: &[u8] = include_bytes!("../../static/pwa/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../../static/pwa/icon-512.png");
/// 180 px, opaque: iOS ignores the manifest and wants exactly this.
const ICON_APPLE: &[u8] = include_bytes!("../../static/pwa/apple-touch-icon.png");

/// The logo tile as a standalone SVG; the sidebar inlines the same markup and
/// recolours its `rect` and `path` from the theme.
pub const ICON_SVG: &str = include_str!("../../static/brand/hobnail.svg");

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
        "/icon.svg" => asset(ICON_SVG.as_bytes().to_vec(), "image/svg+xml"),
        "/icon-192.png" => asset(ICON_192.to_vec(), "image/png"),
        "/icon-512.png" => asset(ICON_512.to_vec(), "image/png"),
        "/apple-touch-icon.png" => asset(ICON_APPLE.to_vec(), "image/png"),
        // The alarm badge of a hidden tab (its live stream is closed): one
        // tiny poll a minute instead of a stream.
        "/alarm.json" => {
            let cfg = state.config.current();
            let snap = state.snapshot();
            let groups = crate::ui::view::build_groups(&cfg, &snap, &Default::default(), "");
            let firing = state.alerts.lock().unwrap().incidents.len();
            let n = crate::ui::view::problem_count(&groups).max(firing);
            let mut r = full(StatusCode::OK, format!("{{\"problems\":{n}}}").into_bytes());
            set(&mut r, hyper::header::CONTENT_TYPE, "application/json");
            set(&mut r, hyper::header::CACHE_CONTROL, "no-store");
            r
        }
        "/offline" => {
            let mut r = full(StatusCode::OK, OFFLINE.as_bytes().to_vec());
            set(
                &mut r,
                hyper::header::CONTENT_TYPE,
                "text/html; charset=utf-8",
            );
            set(&mut r, hyper::header::CACHE_CONTROL, "no-cache");
            r
        }
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
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": "#0b0e13",
        "theme_color": "#0b0e13",
        "icons": [
            { "src": "/icon.svg", "sizes": "any", "type": "image/svg+xml", "purpose": "any" },
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" }
        ],
        "shortcuts": [
            { "name": "Containers", "url": "/containers" },
            { "name": "Logs", "url": "/logs" },
            { "name": "Alerts", "url": "/alerts" }
        ]
    })
    .to_string()
}

const OFFLINE: &str = r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="dark light"><title>Offline · Hobnail</title><link rel="icon" href="/icon.svg"><style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b0e13;color:#e8edf4;font:15px/1.5 ui-sans-serif,system-ui,sans-serif}@media (prefers-color-scheme:light){body{background:#f4f5f7;color:#121720}}main{max-width:34ch;padding:24px;text-align:center}h1{font-size:22px;margin:16px 0 6px}p{margin:0 0 18px;opacity:.8}a{display:inline-block;padding:8px 16px;border-radius:8px;background:#0f766e;color:#fff;text-decoration:none;font-weight:600}</style></head><body><main><img src="/icon.svg" width="48" height="48" alt="Hobnail"><h1>Can't reach the server</h1><p>Hobnail shows only live data, so there is nothing to show offline. Check the connection (Tailscale?) and try again.</p><a href="/">Try again</a></main></body></html>"##;

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
        assert!(OFFLINE.contains("Try again") && !OFFLINE.contains("serviceWorker"));
    }
}
