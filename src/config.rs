//! TOML config: overrides and additions only. An empty or missing file is a
//! valid config (everything is auto-discovered).
//!
//! Hot reload: `ConfigStore::poll` checks the file's mtime and length every
//! couple of seconds (a single `stat`). We do not use `notify`/inotify: it
//! needs an extra thread, and inotify on bind mounts misses editors that
//! replace the file by rename. A broken edit keeps the last good config and
//! records the error (with file, line and column) for the page to show.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub page: PageConfig,
    pub docker: DockerConfig,
    pub http: HttpPolicy,
    pub icons: IconsConfig,
    pub system: crate::system::SystemConfig,
    /// `[history]`: the local history database (docs/DESIGN.md section 4).
    pub history: HistoryConfig,
    /// `[logs]`: the in-app log viewer and background log tails.
    pub logs: crate::logs::LogsConfig,
    /// `[[groups]]`: order, columns, layout and collapse state of groups.
    pub groups: Vec<GroupConfig>,
    /// `[containers.<container-name>]`: per-container overrides.
    pub containers: BTreeMap<String, ContainerOverride>,
    /// `[[bookmarks]]`: extra links that are not containers.
    pub bookmarks: Vec<Bookmark>,
    /// `[[widgets]]`: custom API and command widgets.
    pub widgets: Vec<crate::widgets::WidgetConfig>,
    /// `[channels.<name>]`: notification destinations. Nothing is sent by default.
    pub channels: BTreeMap<String, crate::alerts::ChannelConfig>,
    /// `[[alerts]]`: rules that send to channels.
    pub alerts: Vec<crate::alerts::RuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PageConfig {
    /// Page title. Default: the host name.
    pub title: Option<String>,
    /// Greeting line under the title. Default: time-of-day greeting.
    pub greeting: Option<String>,
    /// Default columns per group (1..=6).
    pub columns: u8,
    /// "grid" (cards) or "list" (compact rows).
    pub layout: Layout,
    /// "auto" (follow OS), "dark" or "light".
    pub theme: String,
    /// Web search URL for the quick-launch box; `{q}` is replaced.
    pub search_url: String,
    /// Show stopped containers (default true).
    pub show_stopped: bool,
    /// Show live CPU/memory on container cards (default true).
    pub show_stats: bool,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            title: None,
            greeting: None,
            columns: 4,
            layout: Layout::Grid,
            theme: "auto".into(),
            search_url: "https://duckduckgo.com/?q={q}".into(),
            show_stopped: true,
            show_stats: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerConfig {
    /// Discover containers at all (default true).
    pub enabled: bool,
    /// Hide containers matching these globs (by container name).
    pub exclude: Vec<String>,
    /// Logs link template. `{host}` = host the page was opened on, `{id}` =
    /// 12-char container id, `{name}` = container name. Default: auto-detected
    /// from a running Dozzle container.
    pub logs_url: Option<String>,
    /// Metrics link template (same placeholders). Default: auto-detected Beszel.
    pub metrics_url: Option<String>,
    /// Name of the group for containers without a compose project.
    pub standalone_group: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryConfig {
    /// Record host and container samples (default true). Events are always
    /// recorded while the database is available.
    pub enabled: bool,
    /// How often the host and every running container are sampled, with or
    /// without a viewer (5s to 5m, default 15s).
    pub sample_interval: crate::util::Dur,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_interval: crate::util::Dur::secs(15),
        }
    }
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exclude: vec![],
            logs_url: None,
            metrics_url: None,
            standalone_group: "Standalone".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconsConfig {
    /// Fetch dashboard-icons by name from `cdn` on first use (then cached in
    /// `data/icons/di/`). Off by default: no internet calls unless you opt in.
    pub remote: bool,
    /// Base URL of the dashboard-icons repository (`{cdn}/svg/{name}.svg`).
    pub cdn: String,
    /// Guess icons for containers from their image name (default true).
    pub auto: bool,
}

impl Default for IconsConfig {
    fn default() -> Self {
        Self {
            remote: false,
            cdn: "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons".into(),
            auto: true,
        }
    }
}

/// Outgoing HTTP policy for widgets, checks and notifications.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpPolicy {
    /// If non-empty, only these hosts (exact host names/IPs, or `*.example.com`
    /// globs) may be contacted. Empty = any host.
    pub allow_hosts: Vec<String>,
}

impl HttpPolicy {
    pub fn allows(&self, host: &str) -> bool {
        self.allow_hosts.is_empty()
            || self
                .allow_hosts
                .iter()
                .any(|p| crate::util::glob_match(p, host))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroupConfig {
    /// Group name. Matches a compose project name or a `group` override.
    pub name: String,
    /// Display title (default: `name`).
    pub title: Option<String>,
    pub columns: Option<u8>,
    pub layout: Option<Layout>,
    pub collapsed: bool,
    /// Icon name for the group header.
    pub icon: Option<String>,
    /// Hide the whole group.
    pub hide: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContainerOverride {
    pub name: Option<String>,
    pub group: Option<String>,
    pub icon: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub hide: Option<bool>,
    /// Sort weight inside the group (lower first, default 0, then by name).
    pub order: Option<i32>,
    /// "small" | "normal" | "wide".
    pub size: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bookmark {
    pub name: String,
    pub url: String,
    /// Group to show it in (default "Bookmarks").
    pub group: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
}

impl Config {
    /// Semantic checks after parsing. Messages should name the offending item.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=6).contains(&self.page.columns) {
            return Err(format!(
                "page.columns must be 1..=6, got {}",
                self.page.columns
            ));
        }
        let iv = self.history.sample_interval.0;
        if iv < crate::db::sampler::MIN_INTERVAL || iv > crate::db::sampler::MAX_INTERVAL {
            return Err(format!(
                "history.sample_interval must be between 5s and 5m, got {:?}",
                iv
            ));
        }
        for g in &self.groups {
            if g.name.is_empty() {
                return Err("[[groups]] entry without a name".into());
            }
            if let Some(c) = g.columns {
                if !(1..=6).contains(&c) {
                    return Err(format!("group {:?}: columns must be 1..=6", g.name));
                }
            }
        }
        for b in &self.bookmarks {
            if b.name.is_empty() || b.url.is_empty() {
                return Err("[[bookmarks]] entries need name and url".into());
            }
        }
        crate::widgets::validate(&self.widgets)?;
        crate::alerts::validate(self)?;
        Ok(())
    }
}

/// Parse and validate. Errors are prefixed with the file path and, for syntax
/// errors, include line and column.
pub fn parse(text: &str, file: &str) -> Result<Config, String> {
    let cfg: Config = toml::from_str(text).map_err(|e| {
        let pos = e
            .span()
            .map(|s| {
                let (line, col) = line_col(text, s.start);
                format!(":{line}:{col}")
            })
            .unwrap_or_default();
        format!("{file}{pos}: {}", e.message())
    })?;
    cfg.validate().map_err(|e| format!("{file}: {e}"))?;
    Ok(cfg)
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
    (line, col)
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Arc<Config>,
    /// Error from the latest load attempt (the config above is the last good one).
    pub error: Option<String>,
    /// Bumped on every successful (re)load.
    pub generation: u64,
    /// Unix seconds of the last successful load.
    pub loaded_at: u64,
    /// Whether the file exists at all.
    pub file_exists: bool,
}

pub struct ConfigStore {
    path: PathBuf,
    inner: RwLock<Loaded>,
    stamp: RwLock<Option<(SystemTime, u64)>>,
}

impl ConfigStore {
    pub fn open(path: PathBuf) -> Self {
        let store = Self {
            path,
            inner: RwLock::new(Loaded {
                config: Arc::new(Config::default()),
                error: None,
                generation: 0,
                loaded_at: crate::util::unix_now(),
                file_exists: false,
            }),
            stamp: RwLock::new(None),
        };
        store.reload();
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn current(&self) -> Arc<Config> {
        self.inner.read().unwrap().config.clone()
    }

    pub fn snapshot(&self) -> Loaded {
        self.inner.read().unwrap().clone()
    }

    /// Check the file and reload if it changed. Returns true when a new config
    /// was applied *or* the error state changed (the page should refresh).
    pub fn poll(&self) -> bool {
        let stamp = file_stamp(&self.path);
        if *self.stamp.read().unwrap() == stamp {
            return false;
        }
        self.reload()
    }

    /// Re-read the file now (used after an in-app save; no 2 s wait).
    pub fn reload(&self) -> bool {
        let stamp = file_stamp(&self.path);
        *self.stamp.write().unwrap() = stamp;
        let display = self.path.display().to_string();
        let result = match std::fs::read_to_string(&self.path) {
            Ok(text) => parse(&text, &display).map(|c| (c, true)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Config::default(), false)),
            Err(e) => Err(format!("{display}: cannot read: {e}")),
        };
        let mut inner = self.inner.write().unwrap();
        match result {
            Ok((cfg, exists)) => {
                inner.config = Arc::new(cfg);
                inner.error = None;
                inner.generation += 1;
                inner.loaded_at = crate::util::unix_now();
                inner.file_exists = exists;
                crate::info!(
                    "config loaded",
                    path = display,
                    generation = inner.generation,
                    exists = exists
                );
                true
            }
            Err(e) => {
                crate::warn!("config invalid, keeping last good config", error = e);
                let changed = inner.error.as_deref() != Some(e.as_str());
                inner.error = Some(e);
                inner.file_exists = true;
                changed
            }
        }
    }
}

fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_valid() {
        let c = parse("", "config.toml").unwrap();
        assert_eq!(c.page.columns, 4);
        assert!(c.docker.enabled);
        assert!(c.widgets.is_empty());
    }

    #[test]
    fn overrides_parse() {
        let c = parse(
            r#"
[page]
title = "homelab"
columns = 3

[[groups]]
name = "beszel"
title = "Monitoring"
collapsed = true

[containers.dozzle]
name = "Logs"
group = "beszel"
icon = "dozzle"

[[bookmarks]]
name = "GitHub"
url = "https://github.com"
"#,
            "config.toml",
        )
        .unwrap();
        assert_eq!(c.page.title.as_deref(), Some("homelab"));
        assert_eq!(c.groups[0].title.as_deref(), Some("Monitoring"));
        assert_eq!(c.containers["dozzle"].name.as_deref(), Some("Logs"));
        assert_eq!(c.bookmarks.len(), 1);
    }

    #[test]
    fn syntax_error_has_line_and_column() {
        let e = parse(
            "[page]\ntitle = \"x\"\ncolumns = = 3\n",
            "/data/config.toml",
        )
        .unwrap_err();
        assert!(e.starts_with("/data/config.toml:3:"), "{e}");
    }

    #[test]
    fn unknown_field_is_an_error() {
        let e = parse("[page]\ntitel = \"x\"\n", "c.toml").unwrap_err();
        assert!(e.contains("titel"), "{e}");
    }

    #[test]
    fn history_section() {
        let c = parse("", "c.toml").unwrap();
        assert!(c.history.enabled);
        assert_eq!(c.history.sample_interval.0.as_secs(), 15);
        let c = parse(
            "[history]\nsample_interval = \"30s\"\nenabled = false\n",
            "c.toml",
        )
        .unwrap();
        assert_eq!(c.history.sample_interval.0.as_secs(), 30);
        assert!(!c.history.enabled);
        let e = parse("[history]\nsample_interval = \"2s\"\n", "c.toml").unwrap_err();
        assert!(e.contains("sample_interval"), "{e}");
        assert!(parse("[history]\nsample_interval = \"1h\"\n", "c.toml").is_err());
        assert!(parse("[history]\nsample_interval = \"5s\"\n", "c.toml").is_ok());
        assert!(parse("[history]\nkeep = 1\n", "c.toml").is_err());
    }

    #[test]
    fn semantic_error() {
        let e = parse("[page]\ncolumns = 9\n", "c.toml").unwrap_err();
        assert!(e.contains("columns"), "{e}");
    }

    #[test]
    fn hot_reload_keeps_last_good() {
        let dir = std::env::temp_dir().join(format!("rh-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);
        let store = ConfigStore::open(path.clone());
        assert!(store.snapshot().error.is_none());
        assert!(!store.snapshot().file_exists);

        std::fs::write(&path, "[page]\ntitle = \"one\"\n").unwrap();
        assert!(store.poll());
        assert_eq!(store.current().page.title.as_deref(), Some("one"));

        std::fs::write(&path, "[page]\ntitle = \n").unwrap();
        assert!(store.poll());
        let snap = store.snapshot();
        assert!(snap.error.is_some());
        assert_eq!(snap.config.page.title.as_deref(), Some("one"));

        std::fs::write(&path, "[page]\ntitle = \"two\"\n").unwrap();
        assert!(store.poll());
        assert!(store.snapshot().error.is_none());
        assert_eq!(store.current().page.title.as_deref(), Some("two"));
        assert!(!store.poll());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
