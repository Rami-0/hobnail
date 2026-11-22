use std::sync::Arc;

use hyper::{Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;
use crate::state::AppState;
use crate::widgets::{shorten, StructuredBody, WidgetBody, WidgetConfig};
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
    last_good_at: Option<u64>,
    duration_ms: Option<u64>,
    body: Option<WidgetBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    structured: Option<Arc<StructuredBody>>,
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    values: std::collections::BTreeMap<String, serde_json::Value>,
    error: Option<String>,
}

impl WidgetView {
    pub(super) fn fit_board_budget(&mut self, remaining: &mut usize) {
        if self.body.is_none() && self.structured.is_none() && self.values.is_empty() {
            return;
        }
        let mut size = super::boards::JsonSize::default();
        if serde_json::to_writer(&mut size, &self).is_ok() && size.0 <= *remaining {
            *remaining -= size.0;
        } else {
            self.omit_board_body();
        }
    }

    pub(super) fn omit_board_body(&mut self) {
        if self.body.is_none() && self.structured.is_none() && self.values.is_empty() {
            return;
        }
        self.body = None;
        self.structured = None;
        self.values.clear();
        self.status = "error";
        self.error = Some("Widget data unavailable within the board response limit.".into());
    }
}

pub(super) fn view(state: &AppState, config: &WidgetConfig) -> WidgetView {
    let cache = state.widgets.lock().unwrap();
    let enabled = config.is_enabled();
    let output = cache.get(&config.id).filter(|_| enabled);
    let error = output.and_then(|o| o.error.as_deref());
    let last_good_at = output.and_then(|o| o.stale_since);
    let status = match (enabled, output) {
        (false, _) => "disabled",
        (_, None) => "waiting",
        (_, Some(_)) if last_good_at.is_some() => "stale",
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
        last_good_at,
        duration_ms: output.map(|o| o.duration_ms),
        body: output
            .filter(|_| error.is_none() || last_good_at.is_some())
            .map(|o| o.display.clone()),
        structured: None,
        values: output
            .filter(|_| error.is_none() || last_good_at.is_some())
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

pub(super) fn provider_view(
    state: &AppState,
    id: &str,
    cache_key: &str,
    title: String,
    icon: Option<String>,
    enabled: bool,
) -> WidgetView {
    let cache = state.widgets.lock().unwrap();
    let output = cache.get(cache_key).filter(|_| enabled);
    let error = output.and_then(|out| out.error.clone());
    let last_good_at = output.and_then(|out| out.stale_since);
    let status = if !enabled {
        "disabled"
    } else if last_good_at.is_some() {
        "stale"
    } else if error.is_some() {
        "error"
    } else if output.is_some() {
        "ready"
    } else {
        "waiting"
    };
    WidgetView {
        id: id.into(),
        title,
        icon,
        link: None,
        status,
        last_good_at,
        fetched_at: output.map(|out| out.fetched_at),
        duration_ms: output.map(|out| out.duration_ms),
        structured: output.and_then(|out| out.structured.clone()),
        body: None,
        values: Default::default(),
        error,
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
