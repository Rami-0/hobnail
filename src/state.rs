//! Process-wide state shared by the server, the refresh loop and background
//! tasks. Locks are std (short critical sections, never held across .await).

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch, Notify};

use crate::config::ConfigStore;
use crate::model::{Snapshot, StatsMap, SystemStats};
use crate::widgets::{WidgetCache, WidgetOutput};

#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub version: &'static str,
    /// Git short SHA (with `-dirty` when built from a modified tree).
    pub sha: &'static str,
    /// UTC build time.
    pub time: &'static str,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            sha: option_env!("RH_GIT_SHA").unwrap_or("dev"),
            time: option_env!("RH_BUILD_TIME").unwrap_or("unknown"),
        }
    }
}

/// Settings from environment variables (things that need a restart).
#[derive(Debug, Clone)]
pub struct Env {
    pub listen: Vec<SocketAddr>,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub docker_socket: String,
    pub host_proc: String,
    pub host_sys: String,
    /// Host name shown by default (from RH_HOSTNAME, else /proc hostname).
    pub hostname: String,
    /// Extra Host header values (globs) accepted besides IPs, localhost and
    /// `hostname` (RH_ALLOWED_HOSTS, comma-separated). DNS-rebinding guard.
    pub allowed_hosts: Vec<String>,
    /// Exact socket peer IPs allowed to supply forwarding headers.
    pub trusted_proxies: Vec<IpAddr>,
    /// Open connections at once, event streams included
    /// (RH_MAX_CONNECTIONS).
    pub max_connections: usize,
    /// Open connections one peer address (IPv6: /64) may hold; 0 is no
    /// per-peer limit (RH_MAX_CONNECTIONS_PER_PEER).
    pub max_connections_per_peer: usize,
}

/// Highest `RH_MAX_CONNECTIONS`. Every open connection can pin about
/// 200 KiB of socket and process memory (docs/configuration.md).
pub(crate) const MAX_CONNECTIONS_CEILING: usize = 4096;
/// Lowest `RH_MAX_CONNECTIONS`: a few devices with an event stream each.
pub(crate) const MAX_CONNECTIONS_FLOOR: usize = 8;

/// `RH_MAX_CONNECTIONS` (8 to 4096, default from
/// [`crate::http::server::LIMITS`]) and `RH_MAX_CONNECTIONS_PER_PEER`
/// (0, meaning no per-peer limit, up to the global cap; default from the
/// same place).
pub(crate) fn parse_connection_limits(
    connections: Option<&str>,
    per_peer: Option<&str>,
) -> Result<(usize, usize), String> {
    let set = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let defaults = crate::http::server::LIMITS;
    let connections = match set(connections) {
        None => defaults.connections,
        Some(value) => match value.parse::<usize>() {
            Ok(n) if (MAX_CONNECTIONS_FLOOR..=MAX_CONNECTIONS_CEILING).contains(&n) => n,
            _ => {
                return Err(format!(
                    "RH_MAX_CONNECTIONS: expected {MAX_CONNECTIONS_FLOOR} to {MAX_CONNECTIONS_CEILING}, got {value:?}"
                ))
            }
        },
    };
    let per_peer = match set(per_peer) {
        None => defaults.per_peer.min(connections),
        Some(value) => match value.parse::<usize>() {
            Ok(n) if n <= connections => n,
            _ => {
                return Err(format!(
                    "RH_MAX_CONNECTIONS_PER_PEER: expected 0 (no limit) to RH_MAX_CONNECTIONS ({connections}), got {value:?}"
                ))
            }
        },
    };
    Ok((connections, per_peer))
}

impl Env {
    pub fn from_env() -> Result<Self, String> {
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let listen = var("RH_LISTEN", "127.0.0.1:3200")
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<SocketAddr>()
                    .map_err(|_| format!("RH_LISTEN: invalid address {s:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if listen.is_empty() {
            return Err("RH_LISTEN is empty".into());
        }
        let data_dir = PathBuf::from(var("RH_DATA_DIR", "./data"));
        let config_path = std::env::var("RH_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("config.toml"));
        let host_proc = var("RH_HOST_PROC", "/proc");
        let hostname = std::env::var("RH_HOSTNAME").unwrap_or_else(|_| {
            std::fs::read_to_string(format!("{host_proc}/sys/kernel/hostname"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "home".into())
        });
        let (max_connections, max_connections_per_peer) = parse_connection_limits(
            std::env::var("RH_MAX_CONNECTIONS").ok().as_deref(),
            std::env::var("RH_MAX_CONNECTIONS_PER_PEER").ok().as_deref(),
        )?;
        Ok(Self {
            listen,
            data_dir,
            config_path,
            docker_socket: var("RH_DOCKER_SOCKET", "/var/run/docker.sock"),
            host_proc,
            host_sys: var("RH_HOST_SYS", "/sys"),
            hostname,
            allowed_hosts: var("RH_ALLOWED_HOSTS", "")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            trusted_proxies: var("RH_TRUSTED_PROXIES", "")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.parse::<IpAddr>()
                        .map(|ip| ip.to_canonical())
                        .map_err(|_| "RH_TRUSTED_PROXIES: expected comma-separated IP addresses")
                })
                .collect::<Result<Vec<_>, _>>()?,
            max_connections,
            max_connections_per_peer,
        })
    }
}

pub struct AppState {
    pub build: BuildInfo,
    pub env: Env,
    pub config: ConfigStore,
    pub(crate) settings_undo: Mutex<Option<crate::settings::store::Undo>>,
    /// Latest container list. The docker task publishes, everyone else reads
    /// (`borrow()`) or waits for changes (`subscribe()`).
    pub containers: watch::Sender<Arc<Snapshot>>,
    pub container_stats: Mutex<(crate::docker::StatsSampler, StatsMap)>,
    pub system: Mutex<(crate::system::Sampler, Option<SystemStats>)>,
    pub(crate) widgets: Mutex<WidgetCache>,
    /// Local history database (docs/DESIGN.md section 4). May be disabled.
    pub db: Arc<crate::db::Db>,
    pub alerts: Mutex<crate::alerts::Status>,
    /// Login back-off state (per instance; see `auth::RateLimiter`).
    pub password_limiter: Mutex<crate::auth::PasswordLimiter>,
    pub(crate) integration_checks: Mutex<HashMap<String, crate::api::integrations::CheckSlot>>,
    pub integration_tests: Mutex<HashMap<i64, VecDeque<std::time::Instant>>>,
    pub login_limiter: Mutex<crate::auth::RateLimiter>,
    /// Serializes password hashing and verification to bound peak memory.
    pub(crate) credential_queue: crate::auth::CredentialQueue,
    /// Internal notifications for JSON stream topics and config changes.
    pub events: broadcast::Sender<Arc<str>>,
    /// Woken when the first viewer connects.
    pub wake: Notify,
    viewers: AtomicUsize,
    widget_viewers: AtomicUsize,
    pub(crate) provider_viewers: crate::api::provider_widgets::Viewers,
    pub(crate) provider_connections: crate::providers::Connections,
    pub(crate) provider_requests: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    pub fn new(env: Env) -> Arc<Self> {
        let config = ConfigStore::open(env.config_path.clone());
        let (containers, _) = watch::channel(Arc::new(Snapshot::default()));
        let (events, _) = broadcast::channel(64);
        // Never fatal: without a database the dashboard works, minus history.
        let db = Arc::new(crate::db::open_or_disabled(&crate::db::path_for(&env)));
        Arc::new(Self {
            build: BuildInfo::current(),
            config,
            settings_undo: Mutex::new(None),
            containers,
            container_stats: Mutex::new((
                crate::docker::StatsSampler::new(&env.host_sys),
                StatsMap::new(),
            )),
            system: Mutex::new((
                crate::system::Sampler::new(&env.host_proc, &env.host_sys),
                None,
            )),
            widgets: Mutex::default(),
            db,
            alerts: Mutex::new(Default::default()),
            password_limiter: Mutex::new(Default::default()),
            integration_checks: Mutex::new(HashMap::new()),
            integration_tests: Mutex::new(HashMap::new()),
            login_limiter: Mutex::new(Default::default()),
            credential_queue: Default::default(),
            events,
            wake: Notify::new(),
            viewers: AtomicUsize::new(0),
            widget_viewers: AtomicUsize::new(0),
            provider_viewers: Default::default(),
            provider_connections: Default::default(),
            provider_requests: Arc::new(tokio::sync::Semaphore::new(4)),
            env,
        })
    }

    pub(crate) fn begin_widget(
        &self,
        config: &Arc<crate::config::Config>,
        id: &str,
    ) -> Option<u64> {
        let mut cache = self.widgets.lock().unwrap();
        Arc::ptr_eq(config, &self.config.current()).then(|| cache.begin(id))
    }

    pub(crate) fn record_widget(
        &self,
        config: &Arc<crate::config::Config>,
        id: &str,
        attempt: u64,
        completed: std::time::Instant,
        out: WidgetOutput,
    ) -> bool {
        let mut cache = self.widgets.lock().unwrap();
        if !Arc::ptr_eq(config, &self.config.current())
            || !cache.record(id, attempt, completed, out)
        {
            return false;
        }
        self.changed(&format!("w-{id}"));
        true
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.containers.borrow().clone()
    }

    /// Publish a new container snapshot and tell viewers.
    pub fn publish_containers(&self, snap: Snapshot) {
        self.containers.send_replace(Arc::new(snap));
        self.changed("groups");
    }

    /// Notify live subscribers that data changed.
    pub fn changed(&self, id: &str) {
        let _ = self.events.send(Arc::from(id));
    }

    pub fn viewers(&self) -> usize {
        self.viewers.load(Ordering::Relaxed)
    }

    pub fn widget_viewers(&self) -> usize {
        self.widget_viewers.load(Ordering::Relaxed)
    }

    pub fn viewer_guard(self: &Arc<Self>, widgets: bool) -> ViewerGuard {
        if widgets {
            self.widget_viewers.fetch_add(1, Ordering::Relaxed);
        }
        if self.viewers.fetch_add(1, Ordering::Relaxed) == 0 {
            self.wake.notify_one();
        }
        ViewerGuard(self.clone(), widgets)
    }
}

/// Counts a connected live viewer for as long as it is alive.
pub struct ViewerGuard(Arc<AppState>, bool);

impl ViewerGuard {
    pub fn set_widgets(&mut self, widgets: bool) {
        if widgets == self.1 {
            return;
        }
        if widgets {
            self.0.widget_viewers.fetch_add(1, Ordering::Relaxed);
        } else {
            self.0.widget_viewers.fetch_sub(1, Ordering::Relaxed);
        }
        self.1 = widgets;
    }
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        if self.1 {
            self.0.widget_viewers.fetch_sub(1, Ordering::Relaxed);
        }
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn connection_limits_default_to_48_and_12_and_stay_in_range() {
        use super::parse_connection_limits as parse;
        assert_eq!(parse(None, None), Ok((48, 12)));
        assert_eq!(parse(Some(" "), Some("")), Ok((48, 12)));
        assert_eq!(parse(Some("256"), Some("64")), Ok((256, 64)));
        assert_eq!(parse(None, Some("0")), Ok((48, 0)), "0: no per-peer limit");
        assert_eq!(parse(None, Some("48")), Ok((48, 48)));
        assert_eq!(
            parse(Some("8"), None),
            Ok((8, 8)),
            "the default share fits the cap"
        );
        for bad in ["7", "4097", "-1", "many"] {
            assert!(parse(Some(bad), None).is_err(), "{bad}");
        }
        for bad in ["49", "-1", "1.5"] {
            assert!(parse(None, Some(bad)).is_err(), "{bad}");
        }
        assert!(
            parse(Some("32"), Some("33")).is_err(),
            "share above the cap"
        );
    }
}
