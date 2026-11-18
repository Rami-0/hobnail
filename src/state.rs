//! Process-wide state shared by the server, the refresh loop and background
//! tasks. Locks are std (short critical sections, never held across .await).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch, Notify};

use crate::config::ConfigStore;
use crate::model::{Snapshot, StatsMap, SystemStats};
use crate::widgets::WidgetOutput;

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
        })
    }
}

pub struct AppState {
    pub build: BuildInfo,
    pub env: Env,
    pub started_unix: u64,
    pub config: ConfigStore,
    /// Latest container list. The docker task publishes, everyone else reads
    /// (`borrow()`) or waits for changes (`subscribe()`).
    pub containers: watch::Sender<Arc<Snapshot>>,
    pub container_stats: Mutex<(crate::docker::StatsSampler, StatsMap)>,
    pub system: Mutex<(crate::system::Sampler, Option<SystemStats>)>,
    pub widgets: Mutex<HashMap<String, WidgetOutput>>,
    /// Local history database (docs/DESIGN.md section 4). May be disabled.
    pub db: Arc<crate::db::Db>,
    pub alerts: Mutex<crate::alerts::Status>,
    /// Login back-off state (per instance; see `auth::RateLimiter`).
    pub password_limiter: Mutex<crate::auth::PasswordLimiter>,
    pub login_limiter: Mutex<crate::auth::RateLimiter>,
    /// Serializes password hashing and verification to bound peak memory.
    pub login_verifying: Mutex<()>,
    /// Fragment ids that changed ("groups", "sys", "w-<id>", "alerts") or
    /// "reload". Each SSE connection re-renders what it receives.
    pub events: broadcast::Sender<Arc<str>>,
    /// Woken when the first viewer connects.
    pub wake: Notify,
    viewers: AtomicUsize,
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
            started_unix: crate::util::unix_now(),
            config,
            containers,
            container_stats: Mutex::new((
                crate::docker::StatsSampler::new(&env.host_sys),
                StatsMap::new(),
            )),
            system: Mutex::new((
                crate::system::Sampler::new(&env.host_proc, &env.host_sys),
                None,
            )),
            widgets: Mutex::new(HashMap::new()),
            db,
            alerts: Mutex::new(Default::default()),
            password_limiter: Mutex::new(Default::default()),
            login_limiter: Mutex::new(Default::default()),
            login_verifying: Mutex::new(()),
            events,
            wake: Notify::new(),
            viewers: AtomicUsize::new(0),
            env,
        })
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.containers.borrow().clone()
    }

    /// Publish a new container snapshot and tell viewers.
    pub fn publish_containers(&self, snap: Snapshot) {
        self.containers.send_replace(Arc::new(snap));
        self.changed("groups");
    }

    /// Tell connected viewers that a fragment changed.
    pub fn changed(&self, id: &str) {
        let _ = self.events.send(Arc::from(id));
    }

    pub fn viewers(&self) -> usize {
        self.viewers.load(Ordering::Relaxed)
    }

    pub fn viewer_guard(self: &Arc<Self>) -> ViewerGuard {
        if self.viewers.fetch_add(1, Ordering::Relaxed) == 0 {
            self.wake.notify_one();
        }
        ViewerGuard(self.clone())
    }

    pub fn uptime_secs(&self) -> u64 {
        crate::util::unix_now().saturating_sub(self.started_unix)
    }
}

/// Counts a connected live viewer for as long as it is alive.
pub struct ViewerGuard(Arc<AppState>);

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}
