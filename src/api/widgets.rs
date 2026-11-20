use hyper::{Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;
use crate::state::AppState;
use crate::widgets::{shorten, WidgetBody, WidgetConfig};
use serde_json::Value;

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
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    values: std::collections::BTreeMap<String, serde_json::Value>,
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
        values: output
            .into_iter()
            .flat_map(|o| {
                config.fields.keys().filter_map(|key| {
                    o.values
                        .get(key)
                        .map(|value| (key.clone(), bounded_value(value)))
                })
            })
            .collect(),
        error: error.map(crate::widgets::plain_error),
    }
}

pub(super) fn get(state: &AppState) -> Response<Body> {
    let config = state.config.current();
    let widgets: Vec<_> = config.widgets.iter().map(|w| view(state, w)).collect();
    super::json(StatusCode::OK, &widgets)
}

/// Preserve the JSON type for small values and return a preview for larger ones.
fn bounded_value(value: &Value) -> Value {
    let limit = crate::widgets::template::MAX_TEXT;
    let encoded = value.to_string();
    if encoded.chars().nth(limit).is_none() {
        value.clone()
    } else {
        Value::String(shorten(value.as_str().unwrap_or(&encoded), limit - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_widget_values_without_changing_small_types() {
        let small = serde_json::json!({"ok": true});
        assert_eq!(bounded_value(&small), small);
        for value in [
            serde_json::json!("🚀".repeat(1024)),
            serde_json::json!(vec![0; 512]),
        ] {
            let bounded = bounded_value(&value);
            assert!(bounded.as_str().unwrap().chars().count() <= 300);
        }
    }
}
