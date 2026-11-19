//! Route declarations and the shared authentication and role gate.

//!
//! Owners and unscoped accounts use the role gate alone. Scoped accounts
//! retain admin access only for editing granted boards and integrations.
//! Container reads use the union of container targets on granted boards.
//! Legacy pages and host-wide reads are unavailable to scoped accounts.

use std::collections::HashSet;

use hyper::{HeaderMap, Method};

use super::{boards::ContainerMatch, error::ApiError};
use crate::auth::{self, Session, WriteDenied};
use crate::db::accounts::Role;
use crate::state::AppState;

pub use crate::db::accounts::Account as Principal;

impl Principal {
    pub fn is_scoped(&self) -> bool {
        self.role != Role::Owner && self.scopes.is_some()
    }

    pub fn can_see_container(&self, visible: &ContainerVisibility, name: &str) -> bool {
        visible.allows(name)
    }

    fn scoped_targets<T: Eq + std::hash::Hash>(
        &self,
        conn: &rusqlite::Connection,
        kind: &str,
        project: impl Fn(super::boards::BoardPayload) -> Option<T>,
    ) -> Result<Option<HashSet<T>>, ApiError> {
        let Some(scopes) = self.scopes.as_ref().filter(|_| self.is_scoped()) else {
            return Ok(None);
        };
        let mut targets = HashSet::new();
        let mut stmt =
            conn.prepare("SELECT config FROM board_items WHERE board_id=?1 AND kind=?2")?;
        for id in &scopes.boards {
            let configs = stmt.query_map([id.as_str(), kind], |r| r.get::<_, String>(0))?;
            for config in configs {
                if let Some(target) = project(super::boards::decode(&config?)?) {
                    targets.insert(target);
                }
            }
        }
        Ok(Some(targets))
    }

    pub(super) fn container_targets(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Option<HashSet<ContainerMatch>>, ApiError> {
        self.scoped_targets(conn, "container", |item| match item {
            super::boards::BoardPayload::Container { target, .. } => Some(target),
            _ => None,
        })
    }

    pub(super) fn widget_targets(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Option<HashSet<String>>, ApiError> {
        self.scoped_targets(conn, "widget", |item| match item {
            super::boards::BoardPayload::Widget { widget_id } => Some(widget_id),
            _ => None,
        })
    }

    pub fn container_visibility(&self, state: &AppState) -> ContainerVisibility {
        let config = state.config.current();
        let snapshot = state.snapshot();
        let excluded = snapshot
            .containers
            .iter()
            .filter(|container| crate::read_model::is_excluded(&config, container))
            .map(|container| container.name.clone())
            .collect();
        if !self.is_scoped() {
            return ContainerVisibility {
                scoped: None,
                excluded,
                config,
            };
        }
        let targets = state
            .db
            .dashboard(|conn| self.container_targets(conn))
            .ok()
            .flatten()
            .unwrap_or_default();
        ContainerVisibility {
            excluded,
            config,
            scoped: Some(
                snapshot
                    .containers
                    .iter()
                    .filter(|c| targets.contains(&ContainerMatch::from(*c)))
                    .map(|c| c.name.clone())
                    .collect(),
            ),
        }
    }

    pub fn can_see_board(&self, id: &str) -> bool {
        self.role == Role::Owner
            || self
                .scopes
                .as_ref()
                .is_none_or(|s| s.boards.iter().any(|b| b == id))
    }

    pub fn can_see_integration(&self, id: &str) -> bool {
        self.role == Role::Owner
            || self
                .scopes
                .as_ref()
                .is_none_or(|s| s.integrations.iter().any(|i| i == id))
    }
}

/// An unrestricted view, or the names visible in the request's snapshot.
pub struct ContainerVisibility {
    scoped: Option<HashSet<String>>,
    excluded: HashSet<String>,
    config: std::sync::Arc<crate::config::Config>,
}

impl ContainerVisibility {
    pub fn allows(&self, name: &str) -> bool {
        self.scoped
            .as_ref()
            .is_none_or(|names| names.contains(name))
            && !self.excluded.contains(name)
            && !self
                .config
                .containers
                .get(name)
                .and_then(|entry| entry.hide)
                .unwrap_or_else(|| {
                    self.config
                        .docker
                        .exclude
                        .iter()
                        .any(|pattern| crate::util::glob_match(pattern, name))
                })
    }

    pub fn is_scoped(&self) -> bool {
        self.scoped.is_some()
    }

    pub(super) fn filter_view(
        &self,
        mut view: super::containers::ContainersView,
    ) -> super::containers::ContainersView {
        if self.is_scoped() {
            view.docker_error = None;
            view.groups.retain_mut(|group| {
                group.containers.retain(|row| self.allows(&row.name));
                group.running = group
                    .containers
                    .iter()
                    .filter(|row| row.state == "running")
                    .count() as u32;
                group.stopped = group.containers.len() as u32 - group.running;
                !group.containers.is_empty()
            });
        }
        view
    }
}

/// Apply scoped restrictions before dispatch. New admin routes require full
/// admin access unless explicitly listed here.
pub(crate) fn check_scope(
    principal: &Principal,
    access: Access,
    lookup: Option<&Lookup>,
    path: &str,
    query: &str,
) -> Result<(), Denied> {
    if !principal.is_scoped() {
        return Ok(());
    }
    if let Some(lookup) = lookup {
        if let Lookup::Found(m) = lookup {
            if access == Access::Admin {
                let id = m.param("id").unwrap_or_default();
                let granted = match m.route.handler {
                    Handler::BoardSave
                    | Handler::BoardUpdate
                    | Handler::BoardSuggestions
                    | Handler::BoardDraft
                    | Handler::BoardDraftSave
                    | Handler::BoardDraftDelete => principal.can_see_board(id),
                    Handler::IntegrationUpdate
                    | Handler::IntegrationSetSecret
                    | Handler::IntegrationClearSecret
                    | Handler::IntegrationTest => principal.can_see_integration(id),
                    _ => false,
                };
                if !granted {
                    return Err(Denied::Role);
                }
            }
            if matches!(
                m.route.handler,
                Handler::History
                    | Handler::Bookmarks
                    | Handler::Widgets
                    | Handler::Alerts
                    | Handler::Overview
                    | Handler::Host
                    | Handler::HostHistory
            ) {
                return Err(Denied::Role);
            }
            if m.route.handler == Handler::Stream && {
                let topics = super::stream::Topics::requested(query);
                topics.system || topics.alerts || topics.host
            } {
                return Err(Denied::Role);
            }
        }
        return Ok(());
    }
    if access == Access::Admin {
        return Err(Denied::Role);
    }
    if matches!(path, "/api/logout" | "/custom.css")
        || path.starts_with("/api/history/container/")
        || matches!(
            path,
            "/api/logs/stream" | "/api/logs/download" | "/api/logs/levels"
        )
        || path.starts_with("/icons/")
    {
        return Ok(());
    }
    Err(Denied::Role)
}

/// Who may call a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anyone (still behind the Host guard).
    Public,
    Viewer,
    Admin,
    Owner,
}

/// Which handler answers a matched route (dispatched in `api::route`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handler {
    Alarm,
    Integrations,
    IntegrationCreate,
    Integration,
    IntegrationUpdate,
    IntegrationDelete,
    IntegrationSetSecret,
    IntegrationClearSecret,
    IntegrationTest,
    Boards,
    BoardCreate,
    Board,
    BoardSave,
    BoardUpdate,
    BoardDelete,
    BoardSuggestions,
    BoardDraft,
    BoardDraftSave,
    BoardDraftDelete,
    DashboardSettings,
    DashboardIcon,
    Session,
    Bootstrap,
    LogsPicker,
    Containers,
    Container,
    ContainerInspect,
    ContainerMetrics,
    ContainerEvents,
    ContainerEnv,
    ContainerTop,
    History,
    Bookmarks,
    Widgets,
    Alerts,
    Overview,
    Host,
    HostHistory,
    Strips,
    Stream,
    Onboard,
    InviteAccept,
    InviteValidate,
    Invites,
    InviteCreate,
    InviteRevoke,
    Users,
    UserRole,
    UserDisabled,
    UserScopes,
    UserSignOut,
    UserDelete,
    Password,
    Sessions,
    SessionRevoke,
    SessionsRevoke,
}

#[derive(Debug)]
pub struct Route {
    /// `GET` routes also answer `HEAD`.
    pub method: &'static str,
    /// `/api/v1/...`, segments separated by `/`; `<name>` captures one.
    pub pattern: &'static str,
    pub access: Access,
    pub handler: Handler,
}

/// Every `/api/v1` route. `gate_tests` walks this table as a visitor.
pub const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        pattern: "/api/v1/logs/picker",
        access: Access::Viewer,
        handler: Handler::LogsPicker,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/settings",
        access: Access::Viewer,
        handler: Handler::DashboardSettings,
    },
    Route {
        method: "PATCH",
        pattern: "/api/v1/settings",
        access: Access::Admin,
        handler: Handler::DashboardSettings,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/icons/<name>",
        access: Access::Viewer,
        handler: Handler::DashboardIcon,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/boards",
        access: Access::Viewer,
        handler: Handler::Boards,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/boards",
        access: Access::Admin,
        handler: Handler::BoardCreate,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/boards/<id>",
        access: Access::Viewer,
        handler: Handler::Board,
    },
    Route {
        method: "PUT",
        pattern: "/api/v1/boards/<id>",
        access: Access::Admin,
        handler: Handler::BoardSave,
    },
    Route {
        method: "PATCH",
        pattern: "/api/v1/boards/<id>",
        access: Access::Admin,
        handler: Handler::BoardUpdate,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/boards/<id>",
        access: Access::Admin,
        handler: Handler::BoardDelete,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/boards/<id>/suggestions",
        access: Access::Admin,
        handler: Handler::BoardSuggestions,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/boards/<id>/draft",
        access: Access::Admin,
        handler: Handler::BoardDraft,
    },
    Route {
        method: "PUT",
        pattern: "/api/v1/boards/<id>/draft",
        access: Access::Admin,
        handler: Handler::BoardDraftSave,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/boards/<id>/draft",
        access: Access::Admin,
        handler: Handler::BoardDraftDelete,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/integrations",
        access: Access::Viewer,
        handler: Handler::Integrations,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/integrations",
        access: Access::Admin,
        handler: Handler::IntegrationCreate,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/integrations/<id>",
        access: Access::Viewer,
        handler: Handler::Integration,
    },
    Route {
        method: "PUT",
        pattern: "/api/v1/integrations/<id>",
        access: Access::Admin,
        handler: Handler::IntegrationUpdate,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/integrations/<id>",
        access: Access::Admin,
        handler: Handler::IntegrationDelete,
    },
    Route {
        method: "PUT",
        pattern: "/api/v1/integrations/<id>/secret",
        access: Access::Admin,
        handler: Handler::IntegrationSetSecret,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/integrations/<id>/secret",
        access: Access::Admin,
        handler: Handler::IntegrationClearSecret,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/integrations/<id>/test",
        access: Access::Admin,
        handler: Handler::IntegrationTest,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/session",
        access: Access::Public,
        handler: Handler::Session,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/bootstrap",
        access: Access::Viewer,
        handler: Handler::Bootstrap,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers",
        access: Access::Viewer,
        handler: Handler::Containers,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>",
        access: Access::Viewer,
        handler: Handler::Container,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>/metrics",
        access: Access::Viewer,
        handler: Handler::ContainerMetrics,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>/inspect",
        access: Access::Viewer,
        handler: Handler::ContainerInspect,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>/events",
        access: Access::Viewer,
        handler: Handler::ContainerEvents,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>/env",
        access: Access::Admin,
        handler: Handler::ContainerEnv,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>/top",
        access: Access::Admin,
        handler: Handler::ContainerTop,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/alarm",
        access: Access::Viewer,
        handler: Handler::Alarm,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/strips",
        access: Access::Viewer,
        handler: Handler::Strips,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/history",
        access: Access::Viewer,
        handler: Handler::History,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/bookmarks",
        access: Access::Viewer,
        handler: Handler::Bookmarks,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/widgets",
        access: Access::Viewer,
        handler: Handler::Widgets,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/alerts",
        access: Access::Viewer,
        handler: Handler::Alerts,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/overview",
        access: Access::Viewer,
        handler: Handler::Overview,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/host",
        access: Access::Viewer,
        handler: Handler::Host,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/host/history",
        access: Access::Viewer,
        handler: Handler::HostHistory,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/stream",
        access: Access::Viewer,
        handler: Handler::Stream,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/onboarding/owner",
        access: Access::Public,
        handler: Handler::Onboard,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/invites/accept",
        access: Access::Public,
        handler: Handler::InviteAccept,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/invites/validate",
        access: Access::Public,
        handler: Handler::InviteValidate,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/invites",
        access: Access::Owner,
        handler: Handler::Invites,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/invites",
        access: Access::Owner,
        handler: Handler::InviteCreate,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/invites/<id>",
        access: Access::Owner,
        handler: Handler::InviteRevoke,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/users",
        access: Access::Owner,
        handler: Handler::Users,
    },
    Route {
        method: "PATCH",
        pattern: "/api/v1/users/<id>/role",
        access: Access::Owner,
        handler: Handler::UserRole,
    },
    Route {
        method: "PATCH",
        pattern: "/api/v1/users/<id>/disabled",
        access: Access::Owner,
        handler: Handler::UserDisabled,
    },
    Route {
        method: "PUT",
        pattern: "/api/v1/users/<id>/scopes",
        access: Access::Owner,
        handler: Handler::UserScopes,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/users/<id>/sign-out",
        access: Access::Owner,
        handler: Handler::UserSignOut,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/users/<id>",
        access: Access::Owner,
        handler: Handler::UserDelete,
    },
    Route {
        method: "POST",
        pattern: "/api/v1/account/password",
        access: Access::Viewer,
        handler: Handler::Password,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/account/sessions",
        access: Access::Viewer,
        handler: Handler::Sessions,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/account/sessions/<id>",
        access: Access::Viewer,
        handler: Handler::SessionRevoke,
    },
    Route {
        method: "DELETE",
        pattern: "/api/v1/account/sessions",
        access: Access::Viewer,
        handler: Handler::SessionsRevoke,
    },
];

/// A route that matched, with its decoded captures in pattern order.
#[derive(Debug)]
pub struct Matched {
    pub route: &'static Route,
    pub params: Vec<(&'static str, String)>,
}

impl Matched {
    /// The decoded value of capture `<name>`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub enum Lookup {
    Found(Matched),
    NotFound,
    /// The path exists for other methods (listed for `Allow`).
    MethodNotAllowed(Vec<&'static str>),
}

/// Find the route for `method` and `path` (the raw request path, no query).
pub fn lookup(method: &Method, path: &str) -> Lookup {
    let method = if method == Method::HEAD {
        "GET"
    } else {
        method.as_str()
    };
    let mut allow = Vec::new();
    for route in ROUTES {
        let Some(params) = match_path(route.pattern, path) else {
            continue;
        };
        if route.method == method {
            return Lookup::Found(Matched { route, params });
        }
        if !allow.contains(&route.method) {
            allow.push(route.method);
            if route.method == "GET" {
                allow.push("HEAD");
            }
        }
    }
    if allow.is_empty() {
        Lookup::NotFound
    } else {
        Lookup::MethodNotAllowed(allow)
    }
}

pub(crate) enum Denied {
    SignIn,
    Role,
    Write(WriteDenied),
}

impl Denied {
    pub fn response(self, v1: bool) -> hyper::Response<crate::http::server::Body> {
        use crate::http::server::text;
        use hyper::StatusCode;
        match self {
            Self::Write(error) => error.response(),
            Self::SignIn if v1 => ApiError::sign_in_required().response(),
            Self::SignIn => WriteDenied::NoSession.response(),
            Self::Role if v1 => {
                ApiError::new(StatusCode::FORBIDDEN, "forbidden", "insufficient role").response()
            }
            Self::Role => text(StatusCode::FORBIDDEN, "insufficient role\n"),
        }
    }
}

pub(crate) fn check(
    state: &AppState,
    headers: &HeaderMap,
    access: Access,
    write: bool,
) -> Result<Option<Session>, Denied> {
    let minimum = match access {
        Access::Public => {
            if write && !auth::same_origin(headers) {
                return Err(Denied::Write(WriteDenied::BadOrigin));
            }
            return Ok(None);
        }
        Access::Viewer => Role::Viewer,
        Access::Admin => Role::Admin,
        Access::Owner => Role::Owner,
    };
    let session = auth::request_session(state, headers).ok_or(if write {
        Denied::Write(WriteDenied::NoSession)
    } else {
        Denied::SignIn
    })?;
    if write {
        auth::check_write(&session, headers).map_err(Denied::Write)?;
    }
    if session.account.role < minimum {
        return Err(Denied::Role);
    }
    Ok(Some(session))
}

fn match_path(pattern: &'static str, path: &str) -> Option<Vec<(&'static str, String)>> {
    let mut pat = pattern.split('/');
    let mut got = path.split('/');
    let mut params = Vec::new();
    loop {
        match (pat.next(), got.next()) {
            (None, None) => return Some(params),
            (Some(p), Some(g)) => {
                if let Some(name) = p.strip_prefix('<').and_then(|p| p.strip_suffix('>')) {
                    params.push((name, capture(g)?));
                } else if p != g {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

/// Percent-decode one path segment for a `<name>` capture; `None` when it
/// is empty, a dot segment, or would contain a slash, backslash or control
/// character once decoded.
fn capture(raw: &str) -> Option<String> {
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3)?;
            let digit = |c: u8| (c as char).to_digit(16);
            out.push((digit(hex[0])? * 16 + digit(hex[1])?) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(out).ok()?;
    let bad = s.is_empty()
        || s == "."
        || s == ".."
        || s.chars().any(|c| c == '/' || c == '\\' || c.is_control());
    (!bad).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PATTERN: &str = "/api/v1/containers/<name>/logs";

    #[test]
    fn literal_routes() {
        let Lookup::Found(m) = lookup(&Method::GET, "/api/v1/session") else {
            panic!("session not found");
        };
        assert_eq!(m.route.handler, Handler::Session);
        assert!(m.params.is_empty());
        assert!(matches!(
            lookup(&Method::HEAD, "/api/v1/session"),
            Lookup::Found(_)
        ));
        for p in [
            "/api/v1/session/",
            "/api/v1//session",
            "/api/v1/Session",
            "/api/v1/session/x",
            "/api/v1",
            "/api/v1/",
            "",
        ] {
            assert!(matches!(lookup(&Method::GET, p), Lookup::NotFound), "{p}");
        }
        match lookup(&Method::POST, "/api/v1/session") {
            Lookup::MethodNotAllowed(a) => assert_eq!(a, ["GET", "HEAD"]),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            lookup(&Method::POST, "/api/v1/nope"),
            Lookup::NotFound
        ));
    }

    #[test]
    fn captures() {
        let ok = |p: &str| match_path(TEST_PATTERN, p).map(|v| v[0].1.clone());
        assert_eq!(ok("/api/v1/containers/web/logs").as_deref(), Some("web"));
        assert_eq!(
            ok("/api/v1/containers/my%20app/logs").as_deref(),
            Some("my app")
        );
        assert_eq!(
            ok("/api/v1/containers/a.b-c_d/logs").as_deref(),
            Some("a.b-c_d")
        );
        assert_eq!(ok("/api/v1/containers/...x/logs").as_deref(), Some("...x"));
        let m = Matched {
            route: &ROUTES[0],
            params: match_path(TEST_PATTERN, "/api/v1/containers/a%2Db/logs").unwrap(),
        };
        assert_eq!(m.param("name"), Some("a-b"));
        assert_eq!(m.param("other"), None);
        for bad in [
            "/api/v1/containers//logs",
            "/api/v1/containers/./logs",
            "/api/v1/containers/../logs",
            "/api/v1/containers/%2e%2e/logs",
            "/api/v1/containers/%2E/logs",
            "/api/v1/containers/a%2Fb/logs",
            "/api/v1/containers/a%2fb/logs",
            "/api/v1/containers/a%5Cb/logs",
            "/api/v1/containers/a%00b/logs",
            "/api/v1/containers/a%0ab/logs",
            "/api/v1/containers/%/logs",
            "/api/v1/containers/%2/logs",
            "/api/v1/containers/%zz/logs",
            "/api/v1/containers/%+f/logs",
            "/api/v1/containers/%ff/logs", // not UTF-8
            "/api/v1/containers/a/b/logs",
            "/api/v1/containers/web",
            "/api/v1/containers/web/logs/",
        ] {
            assert_eq!(ok(bad), None, "{bad}");
        }
    }

    #[test]
    fn table_is_well_formed() {
        for (i, r) in ROUTES.iter().enumerate() {
            assert!(r.pattern.starts_with("/api/v1/"), "{r:?}");
            assert!(
                ["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&r.method),
                "{r:?}"
            );
            for seg in r.pattern.split('/').skip(1) {
                assert!(!seg.is_empty(), "empty segment in {r:?}");
                let cap = seg.starts_with('<') || seg.ends_with('>');
                assert!(
                    !cap || (seg.len() > 2 && seg.starts_with('<') && seg.ends_with('>')),
                    "{r:?}"
                );
            }
            for other in &ROUTES[..i] {
                assert!(
                    (other.method, other.pattern) != (r.method, r.pattern),
                    "duplicate route {r:?}"
                );
            }
        }
    }

    #[test]
    fn scope_checks_distinguish_empty_scopes_and_unrestricted_access() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.recover_owner(None, "hash", 1).unwrap();
        let mut account = db.accounts().unwrap().pop().unwrap();
        use crate::db::accounts::Scopes;
        account.scopes = Some(Scopes::default());
        assert!(account.can_see_board("any"));
        assert!(account.can_see_integration("any"));
        account.role = Role::Admin;
        assert!(!account.can_see_board("any"));
        assert!(!account.can_see_integration("any"));
        account.scopes = Some(Scopes {
            boards: vec!["one".into()],
            integrations: vec!["two".into()],
        });
        assert!(account.can_see_board("one"));
        assert!(!account.can_see_board("two"));
        assert!(account.can_see_integration("two"));
        assert!(!account.can_see_integration("one"));
        account.scopes = None;
        assert!(account.can_see_board("any"));
        assert!(account.can_see_integration("any"));
    }
}
