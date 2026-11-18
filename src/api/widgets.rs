use hyper::{Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;
use crate::state::AppState;
use crate::widgets::{WidgetBody, WidgetConfig};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct WidgetView {
    id: String,
    title: String,
    icon: Option<String>,
    link: Option<String>,
    status: &'static str,
    fetched_at: Option<u64>,
    duration_ms: Option<u64>,
    body: Option<WidgetBody>,
    error: Option<String>,
}

pub(super) fn view(state: &AppState, config: &WidgetConfig) -> WidgetView {
    let cache = state.widgets.lock().unwrap();
    let enabled = config.is_enabled();
    let output = cache.get(&config.id).filter(|_| enabled);
    let error = output.and_then(|o| o.error.as_deref());
    let status = match (enabled, output) {
        (false, _) => "disabled",
        (_, None) => "waiting",
        (_, Some(_)) if error.is_some() => "error",
        (_, Some(o)) => match o.values.get("_ok").and_then(serde_json::Value::as_bool) {
            Some(true) => "ok",
            Some(false) => "bad",
            None => "ready",
        },
    };
    WidgetView {
        id: config.id.clone(),
        title: config.title.clone().unwrap_or_else(|| config.id.clone()),
        icon: config.icon.clone(),
        link: config.link.clone().filter(|l| crate::widgets::safe_link(l)),
        status,
        fetched_at: output.map(|o| o.fetched_at),
        duration_ms: output.map(|o| o.duration_ms),
        body: output
            .filter(|_| error.is_none())
            .map(|o| o.display.clone()),
        error: error.map(crate::ui::widgets::plain_error),
    }
}

pub(super) fn get(state: &AppState) -> Response<Body> {
    let config = state.config.current();
    let widgets: Vec<_> = config.widgets.iter().map(|w| view(state, w)).collect();
    super::json(StatusCode::OK, &widgets)
}
