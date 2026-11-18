use hyper::{Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;
use crate::state::AppState;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct BookmarkView {
    name: String,
    url: String,
    group: Option<String>,
    icon: Option<String>,
    description: Option<String>,
}

pub(super) fn get(state: &AppState) -> Response<Body> {
    let config = state.config.current();
    let bookmarks: Vec<_> = config
        .bookmarks
        .iter()
        .filter(|b| crate::widgets::safe_link(&b.url))
        .map(|b| BookmarkView {
            name: b.name.clone(),
            url: b.url.clone(),
            group: b.group.clone(),
            icon: b.icon.clone(),
            description: b.description.clone(),
        })
        .collect();
    super::json(StatusCode::OK, &bookmarks)
}
