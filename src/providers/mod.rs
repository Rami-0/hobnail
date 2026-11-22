//! Add a provider in its own module and register its static descriptor below.
//! Declare fields, destination policy, connection test and widgets together.
//! Test and Fetch receive a Conn with one deadline, permit and revision. Use
//! Step for provider-relative requests and select any response headers needed.
//!
//! - API key: Auth::Header with Input::Credential.
//! - Bearer: Auth::Bearer with Input::Credential.
//! - Basic: Auth::Basic with separate username and password credential fields.
//! - Query key: Step::query with Input::Credential.
//! - Form login: POST a Body::Form, select Set-Cookie, parse a named cookie with
//!   Conn::session_cookie and send it with Auth::Cookie. Use TargetHeader for
//!   Origin or Referer when required.
//! - 409 challenge: use retry_session with a login closure returning None.
//!   Send the actual RPC and return Fault::session_conflict with session_header.
//! - Token exchange: wrap the response token with Conn::session and use
//!   retry_session with a login closure returning Some(session).
//! - Large JSON: Conn::scan streams a body through json_scan::Scanner with a
//!   visitor holding bounded state, never the document.
//! - Credential-free probe: use dns_lookup for A/AAAA, tcp_probe for a timed
//!   TCP connect, tls_leaf for handshake metadata and a verification verdict,
//!   or http_status for a status without the response body. TCP connect
//!   substitutes for ICMP ping.
//!
//! Return typed faults and structured display data. Exercise the workflow
//! with a fake service. See docs/features/integrations.md for the contract.

use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    http::{client, pin},
    widgets,
};

mod adguard_home;
mod arr;
mod bookmarks;
mod clock;
mod connection;
mod duplicati;
mod feed;
mod generic;
mod gitea;
mod github;
mod glances;
mod home_assistant;
mod immich;
mod jellyfin;
mod jellyseerr;
mod json_scan;
mod markdown_note;
mod nextcloud;
mod nginx_proxy_manager;
mod paperless;
mod pihole;
mod plex;
mod prowlarr;
mod proxmox;
mod qbittorrent;
mod radarr;
mod reachability;
mod repositories;
mod sabnzbd;
mod sonarr;
mod ssl_expiry;
mod syncthing;
mod tailscale;
mod technitium;
mod transmission;
mod truenas;
mod uptime_kuma;
mod weather;
mod wg_easy;

#[cfg(test)]
mod batch_test;

#[allow(unused_imports)] // Available to credential-free probe providers.
pub use crate::http::probe::{HttpProbe, SubjectAltName, TcpProbe, TlsLeaf, TlsVerification};
#[allow(unused_imports)] // Available to providers that need CSRF headers.
pub use connection::TargetHeader;
pub use connection::{success, Auth, Body, Conn, Input, Session, Step, Verb};
pub(crate) use connection::{success_status, Connections, Credentials, Revision, Target};

pub const REGISTRY: &[Provider] = &[
    generic::HTTP,
    pihole::PI_HOLE,
    uptime_kuma::UPTIME_KUMA,
    jellyfin::JELLYFIN,
    qbittorrent::QBITTORRENT,
    sonarr::SONARR,
    radarr::RADARR,
    prowlarr::PROWLARR,
    jellyseerr::JELLYSEERR,
    adguard_home::ADGUARD_HOME,
    technitium::TECHNITIUM,
    tailscale::TAILSCALE,
    wg_easy::WG_EASY,
    nginx_proxy_manager::NGINX_PROXY_MANAGER,
    glances::GLANCES,
    plex::PLEX,
    immich::IMMICH,
    transmission::TRANSMISSION,
    sabnzbd::SABNZBD,
    home_assistant::HOME_ASSISTANT,
    nextcloud::NEXTCLOUD,
    syncthing::SYNCTHING,
    duplicati::DUPLICATI,
    proxmox::PROXMOX,
    truenas::TRUENAS,
    gitea::GITEA,
    github::GITHUB,
    paperless::PAPERLESS,
    ssl_expiry::SSL_EXPIRY,
    reachability::REACHABILITY,
    feed::FEED,
    weather::WEATHER,
    clock::CLOCK,
    bookmarks::BOOKMARKS,
    markdown_note::MARKDOWN_NOTE,
];
pub const MIN_REFRESH: u64 = 15;
pub const MIN_PUBLIC_REFRESH: u32 = 600;
pub const DEFAULT_REFRESH: u64 = 60;
pub const MAX_RESPONSE: usize = 128 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub fn get(id: &str) -> Option<&'static Provider> {
    REGISTRY.iter().find(|provider| provider.id == id)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub docs: &'static str,
    pub public_internet: bool,
    pub contacts: &'static str,
    pub fields: &'static [Field],
    pub widgets: &'static [Widget],
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub test: Test,
}

impl Provider {
    pub(crate) fn has_endpoint(&self) -> bool {
        self.fields.iter().any(|field| field.id == "base_url")
    }
}

type Job<'a, T> = Pin<Box<dyn Future<Output = Result<T, Fault>> + Send + 'a>>;
pub type Test = for<'a> fn(&'a Conn<'_>) -> Job<'a, ()>;
pub type Fetch = for<'a> fn(&'a Conn<'_>, &'a Values) -> Job<'a, widgets::StructuredBody>;
pub type Values = Map<String, Value>;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Widget {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub recommended: bool,
    pub fields: &'static [Field],
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub fetch: Fetch,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub validate: fn(&Values) -> Result<(), &'static str>,
}

impl Widget {
    pub(crate) fn refresh(&self, options: &Values, public_internet: bool) -> Duration {
        let floor = self
            .fields
            .iter()
            .find(|field| field.id == "refresh")
            .and_then(|field| field.min)
            .unwrap_or(MIN_REFRESH as u32);
        refresh(options, public_internet).max(Duration::from_secs(u64::from(floor)))
    }
}

#[derive(Clone, Copy, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub help: &'static str,
    pub max_length: Option<usize>,
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub choices: &'static [&'static str],
    pub default: Option<&'static str>,
}

impl Field {
    pub const fn new(
        id: &'static str,
        label: &'static str,
        kind: Kind,
        help: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            kind,
            help,
            required: false,
            max_length: None,
            min: None,
            max: None,
            choices: &[],
            default: None,
        }
    }

    pub(crate) fn accepts(&self, value: &Value) -> bool {
        let text = || {
            value.as_str().filter(|s| {
                !s.trim().is_empty() && self.max_length.is_none_or(|max| s.len() <= max)
            })
        };
        match self.kind {
            Kind::Url => text().is_some_and(|url| {
                client::origin(url).is_some()
                    && (self.choices.is_empty() || self.choices.contains(&url))
            }),
            Kind::Secret => text().is_some_and(|s| !s.chars().any(char::is_control)),
            Kind::Text | Kind::Multiline => text().is_some(),
            Kind::Decimal => text()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .is_some_and(|n| {
                    n.is_finite() && self.max.is_none_or(|max| n.abs() <= f64::from(max))
                }),
            Kind::Bool => value.is_boolean(),
            Kind::Number => value.as_u64().is_some_and(|n| {
                self.min.is_none_or(|min| n >= u64::from(min))
                    && self.max.is_none_or(|max| n <= u64::from(max))
            }),
            Kind::Select => text().is_some_and(|s| self.choices.contains(&s)),
            Kind::List => value.as_array().is_some_and(|items| {
                items.len() <= self.max.unwrap_or(16).min(16) as usize
                    && (!self.required || !items.is_empty())
                    && items.iter().all(|item| {
                        item.as_str().is_some_and(|s| {
                            !s.trim().is_empty() && self.max_length.is_none_or(|max| s.len() <= max)
                        })
                    })
            }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum Kind {
    Url,
    Text,
    Multiline,
    Secret,
    Number,
    Decimal,
    Bool,
    Select,
    List,
}

pub fn validate(schema: &[Field], values: &mut Values) -> Result<(), String> {
    if values
        .keys()
        .any(|id| !schema.iter().any(|f| f.id == id.as_str()))
    {
        // Input keys are untrusted. Never return them as error details.
        return Err("configuration".into());
    }
    for field in schema {
        if let Some(value) = values.get(field.id) {
            if !field.accepts(value) {
                return Err(field.id.into());
            }
        } else if let Some(default) = field.default {
            let value = match field.kind {
                Kind::Bool | Kind::Number | Kind::List => {
                    serde_json::from_str(default).expect("valid schema default")
                }
                _ => Value::from(default),
            };
            values.insert(field.id.into(), value);
        } else if field.required {
            return Err(field.id.into());
        }
    }
    Ok(())
}

const SERVICE_URL: Field = Field {
    required: true,
    max_length: Some(2048),
    ..Field::new(
        "base_url",
        "Service URL",
        Kind::Url,
        "Include any reverse-proxy prefix. Use an address reachable from Hobnail.",
    )
};

const ENABLED: Field = Field {
    default: Some("true"),
    ..Field::new("enabled", "Enabled", Kind::Bool, "")
};

const OPTIONAL_BASIC_USER: Field = Field {
    max_length: Some(256),
    ..Field::new(
        "username",
        "Username",
        Kind::Secret,
        "Leave both credential fields blank if the service has no HTTP Basic authentication.",
    )
};

const OPTIONAL_BASIC_PASSWORD: Field = Field {
    max_length: Some(4096),
    ..Field::new("password", "Password", Kind::Secret, "")
};

fn optional_basic(conn: &Conn<'_>) -> Result<Option<Auth<'static>>, Fault> {
    match (
        conn.has_credential("username"),
        conn.has_credential("password"),
    ) {
        (false, false) => Ok(None),
        (true, true) => Ok(Some(Auth::Basic(
            Input::Credential("username"),
            Input::Credential("password"),
        ))),
        _ => Err(Class::Auth.into()),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(
    response: crate::http::client::Response,
) -> Result<T, Fault> {
    serde_json::from_slice(&success(response)?.body).map_err(|_| Class::Protocol.into())
}

/// Keep independent metrics when one upstream response exceeds the step budget.
fn within_response_limit<T>(result: Result<T, Fault>) -> Result<Option<T>, Fault> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(fault) if matches!(fault.class, Class::TooLarge) => Ok(None),
        Err(fault) => Err(fault),
    }
}

fn display_text(conn: &Conn<'_>, mut text: String) -> String {
    conn.output_redactor().redact(&mut text);
    if let Some((end, _)) = text.char_indices().nth(160) {
        text.truncate(end);
        text.push('…');
    }
    text.shrink_to_fit();
    text
}

fn navigation_link(value: &str) -> bool {
    if value.contains(['\\', '"', '<', '>'])
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    let Ok(uri) = value.parse::<hyper::Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https"))
        && uri.authority().is_some_and(|authority| {
            !authority.host().is_empty() && !authority.as_str().contains(['@', '%'])
        })
}

fn local_test<'a>(_: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async { Ok(()) })
}

fn no_widget_options(_: &Values) -> Result<(), &'static str> {
    Ok(())
}

pub const REFRESH: Field = Field {
    min: Some(MIN_REFRESH as u32),
    max: Some(3600),
    default: Some("60"),
    ..Field::new(
        "refresh",
        "Refresh interval (seconds)",
        Kind::Number,
        "Runs only while a board showing this widget is open.",
    )
};

const PUBLIC_REFRESH: Field = Field {
    min: Some(MIN_PUBLIC_REFRESH),
    default: Some("600"),
    ..REFRESH
};

pub struct Fault {
    pub class: Class,
    pub fingerprint: Option<String>,
    session: Option<Session>,
}

impl std::fmt::Debug for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fault")
            .field("message", &self.class.message())
            .finish_non_exhaustive()
    }
}

impl Fault {
    #[allow(dead_code)] // A provider may answer a conflict with a replacement session.
    pub fn session_conflict(session: Session) -> Self {
        Self {
            session: Some(session),
            ..Class::Conflict.into()
        }
    }
}

impl From<Class> for Fault {
    fn from(class: Class) -> Self {
        Self {
            class,
            fingerprint: None,
            session: None,
        }
    }
}

impl From<client::BoundError> for Fault {
    fn from(error: client::BoundError) -> Self {
        let e = error.message.as_str();
        let class = if e.starts_with("TLS") || e.starts_with("invalid TLS") {
            Class::Tls
        } else if e == "connection changed" {
            Class::Changed
        } else if e.starts_with("timed out") {
            Class::Timeout
        } else if e.starts_with("resolve") {
            Class::Dns
        } else if e.starts_with("connect") {
            Class::Connect
        } else if e.starts_with("response too large") {
            Class::TooLarge
        } else if e == "blocked by public internet setting" {
            Class::InternetOff
        } else if e.starts_with("blocked") || e.contains("http.allow_hosts") {
            Class::Blocked
        } else {
            Class::Protocol
        };
        Self {
            class,
            fingerprint: error.fingerprint,
            session: None,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename_all = "snake_case"))]
pub enum Class {
    Success,
    Dns,
    Connect,
    Tls,
    Timeout,
    HttpStatus,
    Auth,
    TooLarge,
    Blocked,
    Protocol,
    InternetOff,
    Busy,
    Changed,
    Conflict,
    RateLimited,
}

impl Class {
    pub fn message(self) -> &'static str {
        match self {
            Self::RateLimited => "The service rate limited requests. Hobnail will retry after the cooldown.",
            Self::Changed => "Connection settings changed. Try again.",
            Self::Conflict => "The service returned a session conflict.",
            Self::Busy => "Other connections are in progress. Try again shortly.",
            Self::Success => "Connection succeeded",
            Self::Connect => "Could not connect. Check that the service is running and its port is reachable from Hobnail.",
            Self::Dns => "The hostname could not be resolved. Check the hostname and Hobnail’s DNS settings.",
            Self::Tls => "TLS verification failed. Hobnail trusts public certificate authorities only unless you confirm a verified leaf fingerprint.",
            Self::Timeout => "The connection timed out. Check connectivity and try again.",
            Self::HttpStatus => "The service returned a non-success status. Check the endpoint URL.",
            Self::Auth => "The service refused authentication. Check your credential and its access.",
            Self::TooLarge => "The response exceeds the size limit. Use a smaller endpoint.",
            Self::Blocked => "The destination is blocked by the outbound policy. Check http.allow_hosts.",
            Self::Protocol => "The service returned an unexpected response. Check the endpoint and response format.",
            Self::InternetOff => "Public internet access is off. Enable it in Settings → Integrations to use public destinations.",
        }
    }
}

pub fn fingerprint(url: &str, value: Option<&str>) -> Result<Option<[u8; 32]>, &'static str> {
    value
        .map(|text| {
            if !url.starts_with("https://") {
                return Err("Use an HTTPS URL for certificate pinning.");
            }
            pin::parse(text).ok_or(
                "Enter the 64 hexadecimal digits of the leaf certificate’s SHA-256 fingerprint.",
            )
        })
        .transpose()
}

pub fn refresh(values: &Values, public_internet: bool) -> Duration {
    let min = if public_internet {
        u64::from(MIN_PUBLIC_REFRESH)
    } else {
        MIN_REFRESH
    };
    Duration::from_secs(
        values
            .get("refresh")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_REFRESH)
            .max(min),
    )
}

mod redaction;
use redaction::OutputRedactor;

impl OutputRedactor {
    fn json(&self, value: &mut Value) {
        if self.is_empty() {
            return;
        }
        match value {
            Value::String(text) => {
                self.redact(text);
            }
            Value::Array(items) => {
                for item in items {
                    self.json(item);
                }
            }
            Value::Object(fields) => {
                for (mut key, mut item) in std::mem::take(fields) {
                    self.redact(&mut key);
                    self.json(&mut item);
                    fields.insert(key, item);
                }
            }
            _ if self.matches(&value.to_string()) => {
                *value = Value::String("<redacted>".into());
            }
            _ => {}
        }
    }

    fn body(&self, body: &mut widgets::StructuredBody) {
        use widgets::StructuredBody::*;
        match body {
            Stat { value } | Unavailable { message: value } => {
                self.redact(value);
            }
            StatGrid { rows } => {
                for (key, value) in rows {
                    self.redact(key);
                    self.redact(value);
                }
            }
            StatusList { rows } => {
                for (key, _) in rows {
                    self.redact(key);
                }
            }
            ItemList { items } => {
                for item in items {
                    self.redact(&mut item.title);
                    if item.icon.as_ref().is_some_and(|icon| self.matches(icon)) {
                        item.icon = None;
                    }
                    if let Some(text) = &mut item.secondary {
                        self.redact(text);
                    }
                    if item.link.as_ref().is_some_and(|link| self.matches(link)) {
                        item.link = None;
                    }
                    if item
                        .time
                        .is_some_and(|time| self.matches(&time.to_string()))
                    {
                        item.time = None;
                    }
                }
            }
            RichText { blocks } => {
                let redact = |span: &mut widgets::RichSpan| {
                    self.redact(&mut span.text);
                    if span.link.as_ref().is_some_and(|link| self.matches(link)) {
                        span.link = None;
                    }
                };
                for block in blocks {
                    use widgets::RichBlock::*;
                    match block {
                        Heading { spans, .. } | Paragraph { spans } => {
                            spans.iter_mut().for_each(redact)
                        }
                        List { items, .. } => items.iter_mut().flatten().for_each(redact),
                        Code { text } => {
                            self.redact(text);
                        }
                    }
                }
            }
            Clock { zones, .. } => {
                for (zone, label) in zones {
                    self.redact(zone);
                    self.redact(label);
                }
            }
            Meter { label, percent } => {
                self.redact(label);
                if self.matches(&percent.to_string()) {
                    *body = Unavailable {
                        message: "Value withheld".into(),
                    };
                }
            }
            Table { headings, rows } => {
                for text in headings.iter_mut().chain(rows.iter_mut().flatten()) {
                    self.redact(text);
                }
            }
        }
    }
}

pub(crate) async fn fetch(
    conn: &Conn<'_>,
    widget: &Widget,
    options: &Values,
) -> Result<widgets::StructuredBody, Fault> {
    conn.run(async {
        let mut body = (widget.fetch)(conn, options).await?;
        conn.output_redactor().body(&mut body);
        Ok(body)
    })
    .await
}

pub(crate) fn requires_paths(
    provider: &str,
    widget: &str,
    credentials: bool,
    options: &Values,
) -> bool {
    provider == generic::HTTP.id
        && widget == "json"
        && credentials
        && !generic::explicit_paths(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_contracts_have_unique_ids_and_safe_schemas() {
        let mut ids = HashSet::new();
        for provider in REGISTRY {
            assert!(ids.insert(provider.id));
            assert!(!provider.name.is_empty() && !provider.category.is_empty());
            assert!(
                !provider.icon.is_empty()
                    && !provider.docs.is_empty()
                    && !provider.contacts.is_empty()
            );
            assert!(!provider.fields.is_empty() && !provider.widgets.is_empty());
            if provider.has_endpoint() {
                assert!(provider.fields.iter().any(|field| {
                    field.id == "base_url" && field.kind == Kind::Url && field.required
                }));
            } else {
                assert!(!provider.public_internet);
                assert!(provider
                    .fields
                    .iter()
                    .all(|field| field.kind != Kind::Secret));
            }
            assert!(provider
                .fields
                .iter()
                .filter(|field| field.id == "secret")
                .all(|field| field.kind == Kind::Secret));
            let mut widgets = HashSet::new();
            for widget in provider.widgets {
                assert!(widgets.insert(widget.id));
                assert!(widget.fields.iter().all(|field| field.kind != Kind::Secret));
            }
            for fields in std::iter::once(provider.fields)
                .chain(provider.widgets.iter().map(|widget| widget.fields))
            {
                let mut keys = HashSet::new();
                for field in fields {
                    assert!(keys.insert(field.id));
                    if field.kind == Kind::Secret {
                        assert!(field.default.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn validates_schema_types_and_applies_defaults() {
        let descriptor = get("http").unwrap();
        assert_eq!(
            validate(descriptor.fields, &mut Values::new()),
            Err("base_url".into())
        );
        let mut values =
            serde_json::json!({"base_url":"http://example.test", "secret":"control\nrejected"})
                .as_object()
                .unwrap()
                .clone();
        assert_eq!(
            validate(descriptor.fields, &mut values),
            Err("secret".into())
        );
        values.remove("secret");
        validate(descriptor.fields, &mut values).unwrap();
        assert_eq!(values["enabled"], true);

        let spec = &descriptor.widgets[0];
        let mut values = serde_json::json!({"refresh":14})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(validate(spec.fields, &mut values), Err("refresh".into()));
        values.remove("refresh");
        validate(spec.fields, &mut values).unwrap();
        assert_eq!(refresh(&values, false), Duration::from_secs(60));
        values.insert("paths".into(), serde_json::json!(["foo[nope]"]));
        validate(spec.fields, &mut values).unwrap();
        assert_eq!((spec.validate)(&values), Err("paths"));
    }
}

#[cfg(test)]
#[tokio::test]
async fn compact_display_text_fits_widget_cache() {
    batch_test::exercise(
        &jellyfin::JELLYFIN,
        &[],
        Values::new(),
        &[],
        |_, _| "",
        async |conn| {
            let oversized = "é".repeat(30_000);
            let title = display_text(conn, oversized);
            assert_eq!(title.chars().count(), 161);
            let short = display_text(conn, {
                let mut text = String::with_capacity(48_000);
                text.push_str("short");
                text
            });
            let mut cache = widgets::WidgetCache::default();
            let id = "provider-fetch:fixture";
            let attempt = cache.begin(id);
            let output = widgets::WidgetOutput {
                structured: Some(
                    widgets::StructuredBody::StatGrid {
                        rows: vec![(short, title)],
                    }
                    .into(),
                ),
                ..Default::default()
            };
            assert!(cache.record(id, attempt, std::time::Instant::now(), output));
            let output = cache.get(id).unwrap();
            assert!(output.error.is_none());
            assert!(output.structured.is_some());
        },
    )
    .await;
}
