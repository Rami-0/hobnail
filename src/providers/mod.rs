//! Add a provider in its own module and register its static descriptor below.
//! Declare fields, destination policy, connection test and widgets together.
//! Test and Fetch receive a Conn for their full operation. Declare write-only
//! secret fields and refer to them by ID via Input::Credential. Use Step for
//! provider-relative requests, opt in to individual response headers, and
//! use the Conn session helpers for authentication handshakes. Return typed
//! faults and structured display data. Exercise the workflow with a fake HTTP
//! server. See docs/features/integrations.md for the complete contract.

use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    http::{client, pin},
    widgets,
};

mod connection;
mod generic;
mod pihole;

pub use connection::{success, Auth, Body, Conn, Input, Session, Step, Verb};
pub(crate) use connection::{Connections, Credentials, Revision, Target};

pub const REGISTRY: &[Provider] = &[generic::HTTP, pihole::PI_HOLE];
pub const MIN_REFRESH: u64 = 15;
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
            Kind::Url => text().and_then(client::origin).is_some(),
            Kind::Secret => text().is_some_and(|s| !s.chars().any(char::is_control)),
            Kind::Text => text().is_some(),
            Kind::Bool => value.is_boolean(),
            Kind::Number => value.as_u64().is_some_and(|n| {
                self.min.is_none_or(|min| n >= u64::from(min))
                    && self.max.is_none_or(|max| n <= u64::from(max))
            }),
            Kind::Select => text().is_some_and(|s| self.choices.contains(&s)),
            Kind::List => value.as_array().is_some_and(|items| {
                items.len() <= 16
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
    Secret,
    Number,
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
}

impl Class {
    pub fn message(self) -> &'static str {
        match self {
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

pub fn refresh(values: &Values) -> Duration {
    Duration::from_secs(
        values
            .get("refresh")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_REFRESH),
    )
}

/// Redact authentication material at the provider output boundary, including
/// credentials echoed as encoded strings inside JSON or as part of a URL.
pub(super) struct OutputRedactor(Vec<String>);

impl OutputRedactor {
    fn new(values: impl Iterator<Item = impl AsRef<str>>) -> Self {
        let mut patterns = std::collections::HashSet::new();
        for value in values {
            let value = value.as_ref();
            if value.is_empty() {
                continue;
            }
            let encoded = connection::base64(value);
            let url_base64 = encoded.replace('+', "-").replace('/', "_");
            for text in [
                value,
                &encoded,
                encoded.trim_end_matches('='),
                &url_base64,
                url_base64.trim_end_matches('='),
            ] {
                patterns.insert(text.to_owned());

                let escaped = Value::from(text).to_string();
                patterns.insert(escaped[1..escaped.len() - 1].to_owned());

                let mut url = String::new();
                connection::encode(&mut url, text);
                // Encoders may write lowercase hex or use '+' for spaces.
                let mut lower = url.clone().into_bytes();
                let mut index = 0;
                while index + 2 < lower.len() {
                    if lower[index] == b'%' {
                        lower[index + 1..index + 3].make_ascii_lowercase();
                        index += 2;
                    }
                    index += 1;
                }
                let lower = String::from_utf8(lower).expect("percent-encoded ASCII");
                for url in [url, lower] {
                    patterns.insert(url.replace("%20", "+"));
                    patterns.insert(url);
                }
            }
        }
        let mut patterns: Vec<_> = patterns.into_iter().collect();
        patterns.sort_unstable_by_key(|text| std::cmp::Reverse(text.len()));
        Self(patterns)
    }

    fn matches(&self, text: &str) -> bool {
        self.0.iter().any(|pattern| text.contains(pattern))
    }

    fn redact(&self, text: &mut String) -> bool {
        let mut changed = false;
        for pattern in &self.0 {
            if text.contains(pattern) {
                *text = text.replace(pattern, "<redacted>");
                changed = true;
            }
        }
        if changed && self.matches(text) {
            text.clear();
        }
        changed
    }

    fn json(&self, value: &mut Value) {
        if self.0.is_empty() {
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
            assert!(provider
                .fields
                .iter()
                .any(|field| field.id == "base_url" && field.kind == Kind::Url && field.required));
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
        assert_eq!(refresh(&values), Duration::from_secs(60));
        values.insert("paths".into(), serde_json::json!(["foo[nope]"]));
        validate(spec.fields, &mut values).unwrap();
        assert_eq!((spec.validate)(&values), Err("paths"));
    }
}
