//! A session-filtered picker, including stopped containers independently of page settings.
use hyper::{Response, StatusCode};
use serde::Serialize;

use super::access::Principal;
use crate::{http::server::Body, state::AppState};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct LogsPicker {
    groups: Vec<LogsGroup>,
    ring: usize,
    tail: u32,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct LogsGroup {
    name: String,
    title: String,
    project: bool,
    containers: Vec<LogsContainer>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(super) struct LogsContainer {
    name: String,
    title: String,
    state: String,
    search: String,
}

fn picker(state: &AppState, principal: &Principal) -> LogsPicker {
    let mut cfg = (*state.config.current()).clone();
    cfg.page.show_stopped = true;
    let snap = state.snapshot();
    let visible = principal.container_visibility(state);
    let tailable = crate::logs::tailable(state);
    let groups = crate::ui::view::build_groups(&cfg, &snap, &Default::default(), "localhost")
        .into_iter()
        .filter_map(|g| {
            let containers: Vec<_> = g
                .cards
                .into_iter()
                .filter(|c| {
                    c.container
                        && tailable.contains(&c.name)
                        && principal.can_see_container(&visible, &c.name)
                })
                .map(|c| LogsContainer {
                    state: snap
                        .containers
                        .iter()
                        .find(|v| v.name == c.name)
                        .map(|v| v.state.clone())
                        .unwrap_or_default(),
                    name: c.name,
                    title: c.title,
                    search: c.search,
                })
                .collect();
            (!containers.is_empty()).then(|| LogsGroup {
                project: crate::logs::is_project(&cfg, &snap, &g.name),
                name: g.name,
                title: g.title,
                containers,
            })
        })
        .collect();
    LogsPicker {
        groups,
        ring: cfg.logs.ring(),
        tail: cfg.logs.tail(),
    }
}

pub(super) async fn get(state: &std::sync::Arc<AppState>, principal: Principal) -> Response<Body> {
    let state = state.clone();
    match tokio::task::spawn_blocking(move || picker(&state, &principal)).await {
        Ok(picker) => super::json(StatusCode::OK, &picker),
        Err(_) => super::error::ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "log picker failed",
        )
        .response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::accounts::{Role, Scopes};
    use crate::ui::testutil::{ctr, state_with};

    #[test]
    fn includes_stopped_but_never_hidden_excluded_or_invalid_names() {
        let mut hidden = ctr("hidden", None, "running");
        hidden.labels.insert("hobnail.hide".into(), "true".into());
        let st = state_with(
            vec![
                hidden,
                ctr("web", Some("shop"), "running"),
                ctr("db", Some("shop"), "exited"),
                ctr("secret", None, "running"),
                ctr("bad/name", None, "running"),
            ],
            "[page]\nshow_stopped = false\n[docker]\nexclude = [\"secret\"]\n",
        );
        let mut account = Principal {
            id: 1,
            name: "viewer".into(),
            role: Role::Viewer,
            disabled: false,
            created: 0,
            updated: 0,
            scopes: None,
        };
        let view = picker(&st, &account);
        let names: Vec<_> = view
            .groups
            .iter()
            .flat_map(|g| &g.containers)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"db"));
        assert!(view.groups.iter().any(|g| g.name == "shop" && g.project));
        account.scopes = Some(Scopes::default());
        assert!(picker(&st, &account).groups.is_empty());
    }
}
