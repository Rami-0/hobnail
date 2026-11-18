//! Signed-in application bootstrap data.

use serde::Serialize;

use super::{access::Principal, error::ApiError};
use crate::db::accounts::Role;
use crate::state::AppState;

/// How often the live loop samples container stats (`live::STATS_EVERY`).
pub const STATS_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Bootstrap {
    pub role: Role,
    pub boards: Vec<BoardNavigation>,
    pub default_board: Option<String>,
    pub remote_icons: bool,
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
    pub config_error: Option<String>,
    /// Alert incidents firing now.
    pub alerts_firing: u32,
    pub refresh: Refresh,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoardNavigation {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
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

fn bootstrap(state: &AppState, user: &Principal, now: i64) -> Result<Bootstrap, ApiError> {
    let role = user.role;
    let (boards, default_board) = state.db.dashboard(|c| -> Result<_, ApiError> {
        let boards = super::boards::list(c, user)?
            .into_iter()
            .map(|b| BoardNavigation {
                id: b.id,
                name: b.name,
                icon: b.icon,
            })
            .collect();
        Ok((
            boards,
            super::dashboard_settings::get(c, user)?.default_board,
        ))
    })?;
    let loaded = state.config.snapshot();
    let cfg = &loaded.config;
    let theme = match cfg.page.theme.as_str() {
        t @ ("dark" | "light" | "ember" | "contrast") => t,
        _ => "auto",
    };
    let snap = state.snapshot();
    Ok(Bootstrap {
        role,
        boards,
        default_board,
        remote_icons: cfg.icons.remote,
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
        config_error: loaded.error.clone().filter(|_| role >= Role::Admin),
        alerts_firing: state.alerts.lock().unwrap().incidents.len() as u32,
        refresh: Refresh {
            system_secs: cfg.system.refresh.0.as_secs().max(1),
            stats_secs: STATS_SECS,
        },
        now,
    })
}

pub fn get(state: &AppState, user: &Principal) -> hyper::Response<crate::http::server::Body> {
    let now = crate::util::unix_now() as i64;
    match bootstrap(state, user, now) {
        Ok(view) => super::json(hyper::StatusCode::OK, &view),
        Err(error) => error.response(),
    }
}
