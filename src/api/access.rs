//! The `/api/v1` route table: every route declares who may call it, and the
//! check runs before its handler (docs/plans/react-port.md 4.2, "Reads are
//! declared, not open by default"). A path that is not in [`ROUTES`] is a
//! 404; a known path with another method is a 405.
//!
//! Writes (any method but GET/HEAD) have already passed
//! `auth::require_write` in `http::server::route` before they get here; a
//! write row still declares its access (normally `Session`).
//!
//! Adding a route = adding a row (and a [`Handler`] variant). Pattern
//! segments are literals or `<name>` captures. A capture is percent-decoded
//! and must not be empty, `.`, `..`, or contain `/`, `\` or a control
//! character (so `%2F` cannot smuggle a slash into a name).

use hyper::{HeaderMap, Method};

use super::error::ApiError;
use crate::state::AppState;

/// Who may call a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anyone (still behind the Host guard).
    Public,
    /// A valid session cookie (`auth::current_user`).
    #[cfg_attr(not(test), allow(dead_code))] // first used by phase 1 routes
    Session,
    /// Readable logs: `[logs] require_login = false`, or a session
    /// (`logs::allowed`).
    #[cfg_attr(not(test), allow(dead_code))] // first used by phase 3 (logs)
    LogsGate,
}

/// Which handler answers a matched route (dispatched in `api::route`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handler {
    Session,
    Bootstrap,
    Containers,
    Container,
    Strips,
    Stream,
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
///
/// The phase 1 reads are all `Public`, with the same visitor/session split
/// as today's pages: session-only fields are left out for a visitor.
pub const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        pattern: "/api/v1/session",
        access: Access::Public,
        handler: Handler::Session,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/bootstrap",
        access: Access::Public,
        handler: Handler::Bootstrap,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers",
        access: Access::Public,
        handler: Handler::Containers,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/containers/<name>",
        access: Access::Public,
        handler: Handler::Container,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/strips",
        access: Access::Public,
        handler: Handler::Strips,
    },
    Route {
        method: "GET",
        pattern: "/api/v1/stream",
        access: Access::Public,
        handler: Handler::Stream,
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

/// The access check, run before every handler. `Err` is the 401 to send.
pub fn check(state: &AppState, headers: &HeaderMap, access: Access) -> Result<(), ApiError> {
    match access {
        Access::Public => Ok(()),
        Access::Session => crate::auth::current_user(state, headers)
            .map(|_| ())
            .ok_or_else(ApiError::sign_in_required),
        Access::LogsGate => {
            if crate::logs::allowed(state, headers) {
                Ok(())
            } else {
                Err(ApiError::logs_sign_in_required())
            }
        }
    }
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
    fn check_each_access() {
        let st = crate::ui::testutil::state_with(vec![], "");
        let none = HeaderMap::new();
        assert_eq!(check(&st, &none, Access::Public), Ok(()));
        assert_eq!(
            check(&st, &none, Access::Session),
            Err(ApiError::sign_in_required())
        );
        // `[logs] require_login` defaults to true.
        assert_eq!(
            check(&st, &none, Access::LogsGate),
            Err(ApiError::logs_sign_in_required())
        );
        let mut forged = HeaderMap::new();
        forged.insert(
            hyper::header::COOKIE,
            format!("rh_session={}", "ab".repeat(32)).parse().unwrap(),
        );
        assert!(check(&st, &forged, Access::Session).is_err());
        assert!(check(&st, &forged, Access::LogsGate).is_err());

        // A valid session passes both.
        let token = "cd".repeat(32);
        let hash: String = ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let now = crate::util::unix_now() as i64;
        assert!(st.db.create_session(&hash, now, now + 3600));
        let mut signed_in = HeaderMap::new();
        signed_in.insert(
            hyper::header::COOKIE,
            format!("rh_session={token}").parse().unwrap(),
        );
        assert_eq!(check(&st, &signed_in, Access::Session), Ok(()));
        assert_eq!(check(&st, &signed_in, Access::LogsGate), Ok(()));

        let open = crate::ui::testutil::state_with(vec![], "[logs]\nrequire_login = false\n");
        assert_eq!(check(&open, &none, Access::LogsGate), Ok(()));
        assert!(check(&open, &none, Access::Session).is_err());
    }
}
