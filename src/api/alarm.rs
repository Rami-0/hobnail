//! A tiny hidden-tab poll: count only visible containers needing attention.
use hyper::{Response, StatusCode};
use serde::Serialize;

use super::access::Principal;
use crate::http::server::Body;
use crate::state::AppState;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct Alarm {
    problems: usize,
}

pub(super) fn get(state: &AppState, principal: &Principal) -> Response<Body> {
    let visible = principal.container_visibility(state);
    let groups = crate::read_model::build_groups(
        &state.config.current(),
        &state.snapshot(),
        &Default::default(),
        "",
    );
    let problems = groups
        .iter()
        .flat_map(|group| &group.cards)
        .filter(|card| {
            card.container
                && principal.can_see_container(&visible, &card.name)
                && matches!(card.label, "unhealthy" | "restarting" | "dead")
        })
        .count();
    super::json(StatusCode::OK, &Alarm { problems })
}
