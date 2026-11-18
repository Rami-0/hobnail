//! `GET /api/v1/bootstrap` (Public, `no-store`): what the shell needs before
//! its first render (title, theme, which features are on, the refresh
//! cadence). `config_error` is for a session only (null for a visitor).

use serde::Serialize;

use crate::state::AppState;

/// How often the live loop samples container stats (`live::STATS_EVERY`).
pub const STATS_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Bootstrap {
    /// `[page] title`, else the host name.
    pub title: String,
    pub hostname: String,
    #[cfg_attr(test, ts(type = r#""auto" | "dark" | "light" | "ember" | "contrast""#))]
    pub theme_default: String,
    /// Web search URL (`{q}` is the query); null when not configured.
    pub search_url: Option<String>,
    pub features: Features,
    /// Docker could not be read (the lists show the last known state).
    pub docker_error: Option<String>,
    /// The config file failed to load (the last good one is in use).
    /// Session only: always null for a visitor.
    pub config_error: Option<String>,
    /// Alert incidents firing now.
    pub alerts_firing: u32,
    pub refresh: Refresh,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Features {
    /// Live CPU and memory per container (`[page] show_stats`).
    pub stats: bool,
    /// Container discovery (`[docker] enabled`).
    pub docker: bool,
    /// The history database is recording.
    pub history: bool,
    /// Host vitals (`[system] enabled`).
    pub system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Refresh {
    /// Seconds between `system` stream events.
    pub system_secs: u64,
    /// Seconds between `stats` stream events.
    pub stats_secs: u64,
}

pub fn bootstrap(state: &AppState, signed: bool, now: i64) -> Bootstrap {
    let loaded = state.config.snapshot();
    let cfg = &loaded.config;
    let theme = match cfg.page.theme.as_str() {
        t @ ("dark" | "light" | "ember" | "contrast") => t,
        _ => "auto",
    };
    let snap = state.snapshot();
    Bootstrap {
        title: cfg
            .page
            .title
            .clone()
            .unwrap_or_else(|| state.env.hostname.clone()),
        hostname: state.env.hostname.clone(),
        theme_default: theme.into(),
        search_url: Some(cfg.page.search_url.clone()).filter(|s| !s.trim().is_empty()),
        features: Features {
            stats: cfg.page.show_stats,
            docker: cfg.docker.enabled,
            history: state.db.is_enabled(),
            system: cfg.system.enabled,
        },
        docker_error: super::containers::docker_error(cfg, &snap),
        config_error: loaded.error.clone().filter(|_| signed),
        alerts_firing: state.alerts.lock().unwrap().incidents.len() as u32,
        refresh: Refresh {
            system_secs: cfg.system.refresh.0.as_secs().max(1),
            stats_secs: STATS_SECS,
        },
        now,
    }
}

pub fn get(
    state: &AppState,
    headers: &hyper::HeaderMap,
) -> hyper::Response<crate::http::server::Body> {
    let signed = crate::auth::current_user(state, headers).is_some();
    let now = crate::util::unix_now() as i64;
    super::json(hyper::StatusCode::OK, &bootstrap(state, signed, now))
}
