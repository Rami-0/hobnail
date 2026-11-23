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
    /// What connecting needs beyond the URL, for services that are not a
    /// local container (Tailscale: an API key and internet access).
    #[cfg_attr(test, ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
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
            let detect = p.detect?;
            if detect.not.iter().any(|not| text.contains(*not)) {
                return None;
            }
            let needle = detect
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
                note: None,
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
    let mut found = detect(&snap.containers, own, &at, &existing, |c| {
        crate::read_model::is_excluded(&cfg, c)
    });
    found.extend(tailscale(
        &at,
        &snap.containers,
        &state.env.allowed_hosts,
        &existing,
    ));
    Ok(super::json(hyper::StatusCode::OK, &found))
}

/// Whether `host` is a Tailscale address (100.64.0.0/10, CGNAT space that
/// Tailscale assigns) or a MagicDNS name (`*.ts.net`).
pub fn tailnet_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.ends_with(".ts.net") {
        return true;
    }
    matches!(
        host.parse::<std::net::IpAddr>(),
        Ok(std::net::IpAddr::V4(v4)) if v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])
    )
}

const TAILSCALE_NOTE: &str = "This host is on a tailnet. Create an API access token in the Tailscale admin console (Settings → Keys) and turn on Provider internet access: the widgets read Tailscale's public API.";

/// The Tailscale suggestion: when Hobnail is reached over Tailscale, a
/// container publishes a port on a Tailscale address, or a configured host
/// name is a MagicDNS name. Nothing is contacted.
pub fn tailscale(
    at: &Origin<'_>,
    containers: &[Container],
    allowed_hosts: &[String],
    existing: &[(String, String)],
) -> Option<DetectedService> {
    let provider = providers::get("tailscale")?;
    if existing.iter().any(|(kind, _)| kind == provider.id) {
        return None;
    }
    let published = containers
        .iter()
        .flat_map(|c| &c.ports)
        .any(|p| tailnet_host(&p.host_ip));
    let configured = allowed_hosts
        .iter()
        .any(|h| tailnet_host(h.trim_start_matches("*.")));
    if !(tailnet_host(at.host) || published || configured) {
        return None;
    }
    let url = provider
        .fields
        .iter()
        .find(|f| f.id == "base_url")
        .and_then(|f| f.default)
        .unwrap_or("https://api.tailscale.com");
    Some(DetectedService {
        provider: provider.id,
        container: "tailnet".into(),
        image: String::new(),
        matched: "tailnet",
        port: 443,
        candidates: vec![Candidate {
            url: url.into(),
            via: "public",
        }],
        note: Some(TAILSCALE_NOTE),
    })
}

/// A running container with a web UI the user can open in a browser.
#[derive(Debug, PartialEq, Eq)]
pub struct DetectedApp {
    pub container: String,
    pub title: String,
    /// A vendored or dashboard-icons name, never `builtin:…`.
    pub icon: Option<String>,
    pub url: String,
}

/// Known web apps: an image needle, the display name and the web UI port
/// inside the container. `None` marks images without a web UI.
const APPS: &[(&str, Option<(&str, u16)>)] = &[
    ("beszel-agent", None),
    ("beszel", Some(("Beszel", 8090))),
    ("dozzle", Some(("Dozzle", 8080))),
    ("dockge", Some(("Dockge", 5001))),
    ("gethomepage/homepage", Some(("Homepage", 3000))),
    ("homarr", Some(("Homarr", 7575))),
    ("mailpit", Some(("Mailpit", 8025))),
    ("mailhog", Some(("MailHog", 8025))),
    ("minio", Some(("MinIO", 9001))),
    ("odoo", Some(("Odoo", 8069))),
    ("portainer", Some(("Portainer", 9443))),
    ("grafana", Some(("Grafana", 3000))),
    ("prometheus", Some(("Prometheus", 9090))),
    ("uptime-kuma", Some(("Uptime Kuma", 3001))),
    ("buildkit", None),
];

/// Images that are data stores or plumbing, never a page to open.
const NOT_WEB_IMAGES: &[&str] = &[
    "postgres",
    "pgvector",
    "pgbouncer",
    "postgis",
    "timescale",
    "mysql",
    "mariadb",
    "redis",
    "valkey",
    "keydb",
    "mongo",
    "memcached",
    "cassandra",
    "clickhouse",
    "etcd",
    "zookeeper",
    "rabbitmq",
    "nats",
    "mosquitto",
    "kafka",
];

/// Container ports that speak something other than HTTP.
const NOT_WEB_PORTS: &[u16] = &[
    21, 22, 25, 53, 110, 143, 465, 587, 993, 995, 1025, 1883, 3306, 4222, 5432, 5672, 6379, 8883,
    9042, 11211, 27017,
];

/// Common web UI ports, preferred when a container publishes several.
const WEB_PORTS: &[u16] = &[
    80, 443, 8080, 3000, 8000, 8443, 9443, 5000, 8081, 8888, 3001, 9000,
];

fn title_case(name: &str) -> String {
    name.split(['-', '_', '.', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Another Hobnail (or its predecessor), by image.
fn hobnail_image(repo: &str) -> bool {
    let last = repo.rsplit('/').next().unwrap_or(repo);
    last.starts_with("hobnail") || last == "rust-homepage"
}

/// The URL a browser on `host` can open for `port` inside the container:
/// a binding on that host, then one on all interfaces (on that host), then
/// any other non-loopback binding. Loopback bindings only when the user
/// browses from the same machine.
fn browser_url(c: &Container, port: u16, host: &str) -> Option<String> {
    let bindings: Vec<&PortMapping> = c
        .ports
        .iter()
        .filter(|p| p.proto == "tcp" && p.container_port == port)
        .collect();
    let on_host = bindings.iter().find(|b| b.host_ip == host);
    let any = bindings.iter().find(|b| unspecified(&b.host_ip));
    let other = bindings
        .iter()
        .find(|b| !unspecified(&b.host_ip) && !loopback(&b.host_ip));
    let local = bindings
        .iter()
        .find(|b| loopback(&b.host_ip))
        .filter(|_| loopback(host));
    let (ip, binding) = if let Some(b) = on_host {
        (b.host_ip.as_str(), b)
    } else if let Some(b) = any.filter(|_| !host.is_empty()) {
        (host, b)
    } else if let Some(b) = other {
        (b.host_ip.as_str(), b)
    } else {
        (host, local?)
    };
    let scheme = if matches!(port, 443 | 8443 | 9443) {
        "https"
    } else {
        "http"
    };
    let ip = if ip.contains(':') && !ip.starts_with('[') {
        format!("[{ip}]")
    } else {
        ip.to_string()
    };
    Some(format!("{scheme}://{ip}:{}", binding.host_port))
}

/// The web UI port: the known app's, else a provider's API port, else the
/// most web-like published TCP port.
fn web_port(c: &Container, known: Option<u16>) -> Option<u16> {
    let published: Vec<u16> = c
        .ports
        .iter()
        .filter(|p| p.proto == "tcp" && !NOT_WEB_PORTS.contains(&p.container_port))
        .map(|p| p.container_port)
        .collect();
    if let Some(port) = known.filter(|port| published.contains(port)) {
        return Some(port);
    }
    WEB_PORTS
        .iter()
        .copied()
        .find(|port| published.contains(port))
        .or_else(|| published.iter().copied().min())
}

/// Running containers with a published web UI, as link tiles the browser
/// on `host` can open. Data stores, Hobnail itself and hidden containers
/// are left out; nothing is contacted.
pub fn apps(
    cfg: &crate::config::Config,
    containers: &[Container],
    own: Option<&Container>,
    host: &str,
) -> Vec<DetectedApp> {
    let mut out: Vec<DetectedApp> = containers
        .iter()
        .filter(|c| c.is_running() && own.is_none_or(|own| own.id != c.id))
        .filter_map(|c| {
            let title = crate::read_model::suggestion_title(cfg, c)?;
            let repo = repository(&c.image);
            if hobnail_image(&repo) || NOT_WEB_IMAGES.iter().any(|n| repo.contains(n)) {
                return None;
            }
            let known = APPS
                .iter()
                .find(|(needle, _)| repo.contains(needle))
                .map(|(_, app)| *app);
            if known == Some(None) {
                return None;
            }
            let known = known.flatten();
            let provider = identify(c).map(|(p, _)| p);
            let port = web_port(
                c,
                known
                    .map(|(_, port)| port)
                    .or_else(|| provider.and_then(|p| p.detect).map(|d| d.port)),
            )?;
            let overridden = cfg
                .containers
                .get(&c.name)
                .and_then(|o| o.url.clone())
                .filter(|u| !u.is_empty());
            let url = overridden.or_else(|| browser_url(c, port, host))?;
            let custom = title != c.name && Some(title) != c.service.as_deref();
            let title = if custom {
                title.to_string()
            } else if let Some((name, _)) = known {
                name.to_string()
            } else if let Some(p) = provider {
                p.name.to_string()
            } else {
                // `app`, `web` … say little; the compose project says more.
                let generic =
                    matches!(title, "app" | "web" | "server" | "frontend" | "ui" | "main");
                title_case(match (&c.project, generic) {
                    (Some(project), true) => project,
                    _ => title,
                })
            };
            let icon = crate::read_model::container_icon(cfg, c)
                .filter(|icon| crate::http::icons::valid_slug(icon));
            Some(DetectedApp {
                container: c.name.clone(),
                title,
                icon,
                url,
            })
        })
        .collect();
    out.sort_by_key(|a| a.title.to_lowercase());
    out.truncate(LIMIT);
    out
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

    /// The owner's real homelab, as Docker reports it.
    fn homelab() -> Vec<Container> {
        const TS: &str = "100.125.149.51";
        const LAN_IP: &str = "192.168.8.200";
        let mut all = Vec::new();
        let mut add =
            |name: &str, image: &str, project: Option<(&str, &str)>, ports: Vec<PortMapping>| {
                let mut c = ctr(name, image);
                c.id = format!("{name:0<64}");
                c.ports = ports;
                if let Some((project, service)) = project {
                    c.project = Some(project.into());
                    c.service = Some(service.into());
                }
                all.push(c);
            };
        add(
            "beszel",
            "henrygd/beszel:latest",
            Some(("beszel", "beszel")),
            vec![
                port(TS, 8090, 8090),
                port("127.0.0.1", 8090, 8090),
                port(LAN_IP, 8090, 8090),
            ],
        );
        add(
            "beszel-agent",
            "henrygd/beszel-agent:latest",
            Some(("beszel", "beszel-agent")),
            vec![],
        );
        add(
            "dozzle",
            "amir20/dozzle:latest",
            Some(("dozzle", "dozzle")),
            vec![port(TS, 8888, 8080), port(LAN_IP, 8888, 8080)],
        );
        add(
            "dockge-dockge-1",
            "louislam/dockge:1",
            Some(("dockge", "dockge")),
            vec![port("0.0.0.0", 5001, 5001)],
        );
        add(
            "homepage",
            "ghcr.io/gethomepage/homepage:latest",
            None,
            vec![port("0.0.0.0", 3000, 3000)],
        );
        add(
            "mail",
            "axllent/mailpit:v1.31.6",
            Some(("rmli-hotel-portal", "mailpit")),
            vec![
                port(TS, 8125, 8025),
                port("127.0.0.1", 8125, 8025),
                port(TS, 1025, 1025),
            ],
        );
        add(
            "cafe-minio",
            "quay.io/minio/minio",
            None,
            vec![port("127.0.0.1", 59000, 9000)],
        );
        add(
            "odoo",
            "odoo:19.0",
            Some(("rmli-hotel-portal", "odoo")),
            vec![port(TS, 8179, 8069)],
        );
        add(
            "cafe-platform-app-1",
            "cafe-platform:local",
            Some(("cafe-platform", "app")),
            vec![port(TS, 5000, 5000), port("127.0.0.1", 5000, 5000)],
        );
        add(
            "pg",
            "postgres:16-alpine",
            Some(("cafe-platform", "postgres")),
            vec![port(TS, 55433, 5432)],
        );
        add(
            "hobnail",
            "hobnail:latest",
            None,
            vec![port(TS, 3200, 3200)],
        );
        add(
            "hobnail-react-3300",
            "hobnail-react:ab8a92e",
            None,
            vec![port(TS, 3300, 3300)],
        );
        add("buildx_buildkit_x", "moby/buildkit:v0.26.2", None, vec![]);
        all
    }

    #[test]
    fn suggests_every_web_ui_on_the_address_the_user_browses_from() {
        let all = homelab();
        let own = all.iter().find(|c| c.name == "hobnail-react-3300");
        let cfg = crate::config::Config::default();
        let got: Vec<(String, String, Option<String>)> = apps(&cfg, &all, own, "100.125.149.51")
            .into_iter()
            .map(|a| (a.title, a.url, a.icon))
            .collect();
        let s = |x: &str| x.to_string();
        assert_eq!(
            got,
            [
                (
                    s("Beszel"),
                    s("http://100.125.149.51:8090"),
                    Some(s("beszel"))
                ),
                (s("Cafe Platform"), s("http://100.125.149.51:5000"), None),
                (
                    s("Dockge"),
                    s("http://100.125.149.51:5001"),
                    Some(s("dockge"))
                ),
                (
                    s("Dozzle"),
                    s("http://100.125.149.51:8888"),
                    Some(s("dozzle"))
                ),
                (s("Homepage"), s("http://100.125.149.51:3000"), None),
                // The web UI, not the SMTP port.
                (
                    s("Mailpit"),
                    s("http://100.125.149.51:8125"),
                    Some(s("mailpit"))
                ),
                (s("Odoo"), s("http://100.125.149.51:8179"), Some(s("odoo"))),
            ]
        );
        // From the LAN: the LAN binding, else the published address.
        let lan: Vec<String> = apps(&cfg, &all, own, "192.168.8.200")
            .into_iter()
            .filter(|a| ["Beszel", "Odoo", "Dockge"].contains(&a.title.as_str()))
            .map(|a| a.url)
            .collect();
        assert_eq!(
            lan,
            [
                "http://192.168.8.200:8090",
                "http://192.168.8.200:5001",
                "http://100.125.149.51:8179"
            ]
        );
        // On the host itself, loopback-only MinIO opens too.
        assert!(apps(&cfg, &all, own, "127.0.0.1")
            .iter()
            .any(|a| a.title == "MinIO" && a.url == "http://127.0.0.1:59000"));
    }

    #[test]
    fn beszel_hub_is_an_integration_and_its_agent_is_not() {
        let all = homelab();
        let home = Origin {
            host: "192.168.8.200",
            ..LAN
        };
        let found = detect(&all, None, &home, &[], |_| false);
        let names: Vec<_> = found
            .iter()
            .map(|d| (d.provider, d.container.as_str()))
            .collect();
        assert_eq!(names, [("beszel", "beszel")]);
        assert_eq!(found[0].candidates[0].url, "http://192.168.8.200:8090");
    }

    #[test]
    fn suggests_tailscale_on_a_tailnet() {
        assert!(tailnet_host("100.125.149.51") && tailnet_host("100.64.0.1"));
        assert!(tailnet_host("box.tail642c98.ts.net"));
        assert!(!tailnet_host("100.128.0.1") && !tailnet_host("192.168.8.200"));
        let ts = Origin {
            host: "100.125.149.51",
            ..LAN
        };
        let found = tailscale(&ts, &[], &[], &[]).expect("tailnet request");
        assert_eq!(found.candidates[0].url, "https://api.tailscale.com");
        assert!(found.note.is_some_and(|n| n.contains("API access token")));
        // A Tailscale binding or a MagicDNS host name says so too.
        assert!(tailscale(&LAN, &homelab(), &[], &[]).is_some());
        assert!(tailscale(&LAN, &[], &["*.tail1234.ts.net".into()], &[]).is_some());
        assert!(tailscale(&LAN, &[], &[], &[]).is_none());
        // Already connected: nothing to suggest.
        let existing = [(
            "tailscale".to_string(),
            "https://api.tailscale.com".to_string(),
        )];
        assert!(tailscale(&ts, &[], &[], &existing).is_none());
    }
}
