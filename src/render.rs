//! Live fragment dispatch and the JSON state dump. Pages live in `ui::pages`.
//!
//! Each live-updatable part of a page is an element with a stable id. The SSE
//! stream announces which data changed (`groups`, `sys`, `alerts`, `w-<id>`);
//! `fragment` renders every region that data feeds, concatenated, and the
//! script swaps in the ones its page has (by id, `outerHTML`).

use serde_json::json;

use crate::state::AppState;
use crate::ui::pages;
use crate::ui::shell::Ctx;

/// Render the live regions for `id` for a viewer on `host`. `None` for
/// unknown ids.
pub fn fragment(state: &AppState, id: &str, host: &str) -> Option<String> {
    let ctx = Ctx::live(state, host);
    pages::bundle(&ctx, id).map(|m| m.into_string())
}

/// `/api/state`: everything we know, as JSON, for debugging and scripts.
pub fn state_json(state: &AppState) -> serde_json::Value {
    let loaded = state.config.snapshot();
    let snap = state.snapshot();
    json!({
        "build": { "version": state.build.version, "sha": state.build.sha, "time": state.build.time },
        "uptime_secs": state.uptime_secs(),
        "viewers": state.viewers(),
        "config": {
            "path": state.config.path().display().to_string(),
            "exists": loaded.file_exists,
            "generation": loaded.generation,
            "loaded_at": loaded.loaded_at,
            "error": loaded.error,
        },
        "containers": &*snap,
        "container_stats": state.container_stats.lock().unwrap().1,
        "system": state.system.lock().unwrap().1,
        "widgets": *state.widgets.lock().unwrap(),
        "alerts": *state.alerts.lock().unwrap(),
    })
}
