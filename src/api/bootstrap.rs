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
    pub scoped: bool,
    pub boards: Vec<BoardNavigation>,
    pub default_board: Option<String>,
    pub remote_icons: bool,
    /// Icons built into the server, available whatever `remote_icons` says.
    pub brand_icons: Vec<BrandIcon>,
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

/// A vendored icon: its name, brand colour and whether it is one dark colour
/// (inverted on dark themes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BrandIcon {
    pub name: &'static str,
    pub color: &'static str,
    pub mono: bool,
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
    let (boards, settings) = state.db.dashboard(|c| -> Result<_, ApiError> {
        let boards = super::boards::list(c, user)?
            .into_iter()
            .map(|b| BoardNavigation {
                id: b.id,
                name: b.name,
                icon: b.icon,
            })
            .collect();
        Ok((boards, super::dashboard_settings::get(c, user)?))
    })?;
    let loaded = state.config.snapshot();
    let cfg = &loaded.config;
    let snap = state.snapshot();
    Ok(Bootstrap {
        role,
        scoped: user.is_scoped(),
        boards,
        default_board: settings.default_board,
        remote_icons: cfg.icons.remote,
        brand_icons: crate::http::icons::BRANDS
            .iter()
            .map(|brand| BrandIcon {
                name: brand.name,
                color: brand.color,
                mono: brand.mono,
            })
            .collect(),
        title: cfg
            .page
            .title
            .clone()
            .unwrap_or_else(|| state.env.hostname.clone()),
        hostname: state.env.hostname.clone(),
        theme_default: settings
            .default_theme
            .unwrap_or_else(|| super::dashboard_settings::config_theme(cfg).into()),
        search_url: Some(cfg.page.search_url.clone()).filter(|s| !s.trim().is_empty()),
        features: Features {
            stats: cfg.page.show_stats,
            docker: cfg.docker.enabled,
            history: state.db.is_enabled(),
            system: !user.is_scoped() && cfg.system.enabled,
        },
        docker_error: super::containers::docker_error(cfg, &snap).filter(|_| !user.is_scoped()),
        config_error: loaded
            .error
            .clone()
            .filter(|_| role >= Role::Admin && !user.is_scoped()),
        alerts_firing: if user.is_scoped() {
            0
        } else {
            super::alerts::firing(state).len() as u32
        },
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
