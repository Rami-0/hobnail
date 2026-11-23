//! Detected services: running local containers that look like a provider
//! Hobnail can connect to, each with a proposed API URL the server could
//! reach. Reads only the Docker snapshot Hobnail already keeps; nothing is
//! contacted and no credential is used until the user tests a connection.
//!
//! Matching, strongest first: a `hobnail.integration=<provider id>` label,
//! then the image repository (registry host and tag removed) containing a
//! provider's `detect.images` needle, then the container or compose service
//! name containing one. The longest needle wins, so `jellyseerr` is not
//! taken for `jellyfin`. The provider's `detect.port` picks which published
//! port is the API.
//!
//! URL proposal, first usable wins (the rest are offered as alternatives):
//! 1. the published host port: on the address it is bound to, or on the
//!    host the user reached Hobnail by when bound to all interfaces
//!    (loopback-only bindings are skipped: unreachable from another
//!    container and refused by the connection policy);
//! 2. the container's IP on a Docker network Hobnail is also attached to;
//! 3. `http://<container name>:<port>` on a shared user-defined network.
//!
//! Every candidate passes [`client::proposal_allowed`] (no restricted
//! address, no public address while provider internet access is off).

use std::collections::HashSet;

use rusqlite::Connection;
use serde::Serialize;

use crate::{
    http::client,
    model::{Container, PortMapping},
    providers::{self, Provider},
};

use super::error::ApiError;

/// Opt-in label naming the provider a container runs.
pub const LABEL_PROVIDER: &str = "hobnail.integration";
/// Most suggestions returned.
const LIMIT: usize = 64;

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DetectedService {
    /// Provider id, e.g. `sonarr`.
    pub provider: &'static str,
    pub container: String,
    pub image: String,
    /// What identified it: `label`, `image` or `name`.
    pub matched: &'static str,
    /// The API port inside the container.
    pub port: u16,
    /// Proposed URLs, best first; empty when none is reachable from Hobnail.
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Candidate {
    pub url: String,
    /// `published` (host port), `network` (container IP) or `name`
    /// (container name on a shared network).
    pub via: &'static str,
}

/// Lower-case image repository without registry host, tag or digest:
/// `lscr.io/linuxserver/sonarr:4` → `linuxserver/sonarr`.
fn repository(image: &str) -> String {
    let image = image.to_ascii_lowercase();
    let image = image.split('@').next().unwrap_or_default();
    let (path, last) = image.rsplit_once('/').unwrap_or(("", image));
    let last = last.split(':').next().unwrap_or_default();
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts
        .first()
        .is_some_and(|first| first.contains(['.', ':']) || *first == "localhost")
    {
        parts.remove(0);
    }
    parts.push(last);
    parts.join("/")
}

/// The provider with the longest needle found in `text`.
fn best_match(text: &str) -> Option<&'static Provider> {
    providers::REGISTRY
        .iter()
        .filter(|p| p.has_endpoint())
        .filter_map(|p| {
            let needle = p
                .detect?
                .images
                .iter()
                .filter(|needle| text.contains(*needle))
                .max_by_key(|needle| needle.len())?;
            Some((needle.len(), p))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

/// Which provider a container runs, and what identified it.
pub fn identify(c: &Container) -> Option<(&'static Provider, &'static str)> {
    if let Some(p) = c
        .labels
        .get(LABEL_PROVIDER)
        .and_then(|id| providers::get(id.trim()))
        .filter(|p| p.has_endpoint() && p.detect.is_some())
    {
        return Some((p, "label"));
    }
    if let Some(p) = best_match(&repository(&c.image)) {
        return Some((p, "image"));
    }
    let names = [Some(c.name.as_str()), c.service.as_deref()];
    names
        .into_iter()
        .flatten()
        .find_map(|name| best_match(&name.to_ascii_lowercase()))
        .map(|p| (p, "name"))
}

fn unspecified(ip: &str) -> bool {
    ip.is_empty() || ip == "0.0.0.0" || ip == "::"
}

fn loopback(ip: &str) -> bool {
    ip == "localhost"
        || ip
            .parse::<std::net::IpAddr>()
            .is_ok_and(|a| a.is_loopback())
}

fn url(host: &str, port: u16) -> String {
    let scheme = if matches!(port, 443 | 8443) {
        "https"
    } else {
        "http"
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{scheme}://{host}:{port}")
}

/// The published TCP bindings of the API port: the bindings of `port`, or
/// of the only published TCP port when `port` itself is not published.
fn api_bindings(c: &Container, port: u16) -> Vec<&PortMapping> {
    let tcp: Vec<&PortMapping> = c.ports.iter().filter(|p| p.proto == "tcp").collect();
    let exact: Vec<&PortMapping> = tcp
        .iter()
        .copied()
        .filter(|p| p.container_port == port)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let distinct: HashSet<u16> = tcp.iter().map(|p| p.container_port).collect();
    if distinct.len() == 1 {
        tcp
    } else {
        Vec::new()
    }
}

/// Inputs that describe where Hobnail itself runs.
pub struct Origin<'a> {
    /// Host the user reached Hobnail by (the request's Host, no port).
    pub host: &'a str,
    /// Networks of Hobnail's own container, when it runs in one.
    pub networks: &'a [String],
    pub internet: bool,
}

/// Proposed URLs for a container's API, best first.
pub fn candidates(c: &Container, port: u16, at: &Origin<'_>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut bindings = api_bindings(c, port);
    // A binding on the address the user reached Hobnail by comes first.
    bindings.sort_by_key(|b| b.host_ip != at.host);
    for b in bindings {
        let host = if unspecified(&b.host_ip) {
            at.host
        } else {
            b.host_ip.as_str()
        };
        if host.is_empty() || loopback(host) {
            continue;
        }
        out.push(Candidate {
            url: url(host, b.host_port),
            via: "published",
        });
    }
    let shared: Vec<_> = c
        .networks
        .iter()
        .filter(|n| at.networks.contains(&n.name) && !matches!(n.name.as_str(), "host" | "none"))
        .collect();
    for n in &shared {
        if !n.ip.is_empty() {
            out.push(Candidate {
                url: url(&n.ip, port),
                via: "network",
            });
        }
    }
    // Docker resolves container names on user-defined networks only.
    if shared.iter().any(|n| n.name != "bridge") {
        out.push(Candidate {
            url: url(&c.name, port),
            via: "name",
        });
    }
    let mut seen = HashSet::new();
    out.retain(|c| client::proposal_allowed(&c.url, at.internet) && seen.insert(c.url.clone()));
    out
}

/// Hobnail's own container: the one whose id starts with this host name
/// (Docker's default container host name) or is named like it.
pub fn own_container<'a>(containers: &'a [Container], hostname: &str) -> Option<&'a Container> {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return None;
    }
    containers
        .iter()
        .find(|c| (hostname.len() >= 12 && c.id.starts_with(hostname)) || c.name == hostname)
}

/// The host and port of an origin such as `http://sonarr:8989`.
fn host_port(origin: &str) -> Option<(String, u16)> {
    let uri: hyper::Uri = origin.parse().ok()?;
    let default = if uri.scheme_str() == Some("https") {
        443
    } else {
        80
    };
    let host = uri.host()?.trim_matches(['[', ']']).to_string();
    Some((host, uri.port_u16().unwrap_or(default)))
}

/// Whether an integration of the same provider already points at this
/// container: at one of its proposed URLs, at its name, or at one of the
/// host ports its API is published on.
fn connected(
    c: &Container,
    provider: &Provider,
    port: u16,
    proposed: &[Candidate],
    existing: &[(String, String)],
) -> bool {
    let published: Vec<u16> = api_bindings(c, port).iter().map(|b| b.host_port).collect();
    existing
        .iter()
        .filter(|(kind, _)| kind == provider.id)
        .any(|(_, origin)| {
            proposed
                .iter()
                .any(|cand| client::origin(&cand.url).as_deref() == Some(origin))
                || host_port(origin).is_some_and(|(host, p)| {
                    host.eq_ignore_ascii_case(&c.name) || published.contains(&p)
                })
        })
}

/// Detected services among `containers`, without Hobnail itself and those
/// an integration of the same provider already covers.
pub fn detect(
    containers: &[Container],
    own: Option<&Container>,
    at: &Origin<'_>,
    existing: &[(String, String)],
    hidden: impl Fn(&Container) -> bool,
) -> Vec<DetectedService> {
    let mut out: Vec<DetectedService> = containers
        .iter()
        .filter(|c| c.is_running() && own.is_none_or(|own| own.id != c.id) && !hidden(c))
        .filter_map(|c| {
            let (provider, matched) = identify(c)?;
            let port = provider.detect?.port;
            let candidates = candidates(c, port, at);
            if connected(c, provider, port, &candidates, existing) {
                return None;
            }
            Some(DetectedService {
                provider: provider.id,
                container: c.name.clone(),
                image: c.image.clone(),
                matched,
                port,
                candidates,
            })
        })
        .collect();
    out.sort_by(|a, b| (a.provider, &a.container).cmp(&(b.provider, &b.container)));
    out.truncate(LIMIT);
    out
}

/// `(kind, origin)` of every saved integration.
pub fn existing(conn: &Connection) -> Result<Vec<(String, String)>, ApiError> {
    let mut stmt = conn.prepare("SELECT kind, origin FROM integrations")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// This process's host name: inside a container, Docker's short id.
pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default()
}

/// `GET /api/v1/providers/detected`: unscoped admins only, since only they
/// can connect a new integration.
pub(super) fn route(
    state: &crate::state::AppState,
    req: &hyper::Request<hyper::body::Incoming>,
    user: &super::access::Principal,
) -> Result<hyper::Response<crate::http::server::Body>, ApiError> {
    if user.is_scoped() {
        return Err(ApiError::not_found());
    }
    let cfg = state.config.current();
    if !cfg.docker.enabled {
        return Ok(super::json(
            hyper::StatusCode::OK,
            &Vec::<DetectedService>::new(),
        ));
    }
    let (internet, existing) = state.db.dashboard(|c| -> Result<_, ApiError> {
        Ok((super::dashboard_settings::internet(c)?, existing(c)?))
    })?;
    let snap = state.snapshot();
    let own = own_container(&snap.containers, &hostname());
    let networks: Vec<String> = own
        .map(|c| c.networks.iter().map(|n| n.name.clone()).collect())
        .unwrap_or_default();
    let host = crate::http::server::request_host(req);
    let at = Origin {
        host: &host,
        networks: &networks,
        internet,
    };
    let found = detect(&snap.containers, own, &at, &existing, |c| {
        crate::read_model::is_excluded(&cfg, c)
    });
    Ok(super::json(hyper::StatusCode::OK, &found))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NetworkAddr;

    fn ctr(name: &str, image: &str) -> Container {
        Container {
            id: format!("{:0<64}", name.len()),
            name: name.into(),
            image: image.into(),
            state: "running".into(),
            ..Default::default()
        }
    }

    fn port(host_ip: &str, host_port: u16, container_port: u16) -> PortMapping {
        PortMapping {
            host_ip: host_ip.into(),
            host_port,
            container_port,
            proto: "tcp".into(),
        }
    }

    fn net(name: &str, ip: &str) -> NetworkAddr {
        NetworkAddr {
            name: name.into(),
            ip: ip.into(),
        }
    }

    const LAN: Origin<'static> = Origin {
        host: "192.168.1.10",
        networks: &[],
        internet: false,
    };

    fn id(c: &Container) -> Option<(&'static str, &'static str)> {
        identify(c).map(|(p, how)| (p.id, how))
    }

    #[test]
    fn repository_drops_registry_and_tag() {
        assert_eq!(
            repository("lscr.io/linuxserver/sonarr:4.0"),
            "linuxserver/sonarr"
        );
        assert_eq!(repository("pihole/pihole:latest"), "pihole/pihole");
        assert_eq!(repository("postgres:16-alpine"), "postgres");
        assert_eq!(
            repository("localhost:5000/x/jellyfin@sha256:ab"),
            "x/jellyfin"
        );
        assert_eq!(
            repository("ghcr.io/home-assistant/home-assistant:stable"),
            "home-assistant/home-assistant"
        );
    }

    #[test]
    fn matches_images_names_and_labels() {
        assert_eq!(
            id(&ctr("tv", "lscr.io/linuxserver/sonarr:latest")),
            Some(("sonarr", "image"))
        );
        assert_eq!(
            id(&ctr("dns", "pihole/pihole:2024.07.0")),
            Some(("pihole", "image"))
        );
        assert_eq!(
            id(&ctr("media", "jellyfin/jellyfin:10.10")),
            Some(("jellyfin", "image"))
        );
        assert_eq!(
            id(&ctr("ha", "ghcr.io/home-assistant/home-assistant:stable")),
            Some(("home-assistant", "image"))
        );
        // The longest needle wins: jellyseerr is not jellyfin, NPM is not plain nginx.
        assert_eq!(
            id(&ctr("req", "fallenbagel/jellyseerr")),
            Some(("jellyseerr", "image"))
        );
        assert_eq!(
            id(&ctr("npm", "jc21/nginx-proxy-manager:2")),
            Some(("nginx_proxy_manager", "image"))
        );
        // A custom image is recognised by its container name.
        assert_eq!(
            id(&ctr("my-sonarr", "registry.example.com/tv:1")),
            Some(("sonarr", "name"))
        );
        // A label names the provider outright.
        let mut labelled = ctr("box", "example/thing");
        labelled
            .labels
            .insert(LABEL_PROVIDER.into(), "radarr".into());
        assert_eq!(id(&labelled), Some(("radarr", "label")));
        // Plain services are not integrations.
        assert_eq!(id(&ctr("postgres", "postgres:16-alpine")), None);
        assert_eq!(id(&ctr("cache", "redis:7")), None);
        // A label naming a provider without an endpoint is ignored.
        let mut clock = ctr("box", "example/thing");
        clock.labels.insert(LABEL_PROVIDER.into(), "clock".into());
        assert_eq!(id(&clock), None);
    }

    #[test]
    fn proposes_the_published_port_first() {
        let mut c = ctr("sonarr", "linuxserver/sonarr");
        c.ports = vec![port("0.0.0.0", 18989, 8989), port("0.0.0.0", 9999, 9000)];
        let got = candidates(&c, 8989, &LAN);
        assert_eq!(
            got,
            vec![Candidate {
                url: "http://192.168.1.10:18989".into(),
                via: "published"
            }]
        );
        // Bound to one address: that address, whatever the request host.
        c.ports = vec![port("192.168.1.20", 8989, 8989)];
        assert_eq!(
            candidates(&c, 8989, &LAN)[0].url,
            "http://192.168.1.20:8989"
        );
        // Loopback only: unreachable from Hobnail's container, so no URL.
        c.ports = vec![port("127.0.0.1", 8989, 8989)];
        assert!(candidates(&c, 8989, &LAN).is_empty());
        // The only published TCP port stands in for a remapped API port.
        c.ports = vec![port("0.0.0.0", 8080, 80)];
        assert_eq!(
            candidates(&c, 8989, &LAN)[0].url,
            "http://192.168.1.10:8080"
        );
    }

    #[test]
    fn falls_back_to_shared_network_ip_then_name() {
        let mut c = ctr("jellyfin", "jellyfin/jellyfin");
        c.networks = vec![net("media", "172.20.0.5"), net("other", "172.21.0.5")];
        let shared = ["media".to_string()];
        let at = Origin {
            networks: &shared,
            ..LAN
        };
        let got = candidates(&c, 8096, &at);
        assert_eq!(
            got,
            vec![
                Candidate {
                    url: "http://172.20.0.5:8096".into(),
                    via: "network"
                },
                Candidate {
                    url: "http://jellyfin:8096".into(),
                    via: "name"
                },
            ]
        );
        // No shared network and nothing published: no proposal.
        assert!(candidates(&c, 8096, &LAN).is_empty());
        // The default bridge has no name resolution.
        c.networks = vec![net("bridge", "172.17.0.4")];
        let bridge = ["bridge".to_string()];
        let at = Origin {
            networks: &bridge,
            ..LAN
        };
        assert_eq!(candidates(&c, 8096, &at).len(), 1);
    }

    #[test]
    fn respects_the_connection_policy() {
        let mut c = ctr("sonarr", "linuxserver/sonarr");
        c.ports = vec![port("0.0.0.0", 8989, 8989)];
        let public = Origin {
            host: "203.0.113.7",
            ..LAN
        };
        assert!(
            candidates(&c, 8989, &public).is_empty(),
            "public while internet is off"
        );
        let open = Origin {
            internet: true,
            ..public
        };
        assert_eq!(candidates(&c, 8989, &open).len(), 1);
        let local = Origin {
            host: "localhost",
            ..LAN
        };
        assert!(
            candidates(&c, 8989, &local).is_empty(),
            "loopback is restricted"
        );
        // Link-local (cloud metadata and friends) is never proposed.
        c.ports = vec![port("169.254.169.254", 80, 8989)];
        assert!(candidates(&c, 8989, &LAN).is_empty());
    }

    #[test]
    fn hides_connected_services_and_itself() {
        let mut sonarr = ctr("sonarr", "linuxserver/sonarr");
        sonarr.ports = vec![port("0.0.0.0", 8989, 8989)];
        let mut pihole = ctr("pihole", "pihole/pihole");
        pihole.ports = vec![port("0.0.0.0", 8053, 80)];
        let mut stopped = ctr("radarr", "linuxserver/radarr");
        stopped.state = "exited".into();
        let mut own = ctr("hobnail", "hobnail");
        own.id = format!("{:0<64}", "abcdef123456");
        own.labels.insert(LABEL_PROVIDER.into(), "glances".into());
        let all = vec![
            sonarr,
            pihole,
            stopped,
            own.clone(),
            ctr("db", "postgres:16"),
        ];
        let names = |found: Vec<DetectedService>| {
            found
                .into_iter()
                .map(|d| format!("{}:{}", d.provider, d.container))
                .collect::<Vec<_>>()
        };
        let mine = own_container(&all, "abcdef123456");
        assert_eq!(mine.map(|c| c.name.as_str()), Some("hobnail"));
        assert_eq!(
            names(detect(&all, mine, &LAN, &[], |_| false)),
            ["pihole:pihole", "sonarr:sonarr"]
        );
        // Connected at the proposed URL, by another host name on the same
        // published port, or by container name: hidden.
        for origin in [
            "http://192.168.1.10:8989",
            "http://nas.lan:8989",
            "http://sonarr:8989",
        ] {
            let existing = [("sonarr".to_string(), origin.to_string())];
            assert_eq!(
                names(detect(&all, mine, &LAN, &existing, |_| false)),
                ["pihole:pihole"],
                "{origin}"
            );
        }
        // Another provider at that origin does not hide it.
        let other = [("radarr".to_string(), "http://192.168.1.10:8989".to_string())];
        assert_eq!(names(detect(&all, mine, &LAN, &other, |_| false)).len(), 2);
        // Containers the owner hid stay hidden.
        assert_eq!(
            names(detect(&all, mine, &LAN, &[], |c| c.name == "pihole")),
            ["sonarr:sonarr"]
        );
    }
}
