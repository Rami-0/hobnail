//! Add a provider in its own module and register its static descriptor below.
//! Declare fields, destination policy, connection test and widgets together.
//! Use the bound HTTP client through Target, return actionable errors without
//! upstream text, and shape successful data for the common widget renderer.

use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    config::HttpPolicy,
    http::{client, pin},
    widgets,
};

mod generic;

pub const REGISTRY: &[Provider] = &[generic::HTTP];
pub const MIN_REFRESH: u64 = 15;
pub const DEFAULT_REFRESH: u64 = 60;
pub const MAX_RESPONSE: usize = 128 * 1024;

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
pub type Test = for<'a> fn(&'a Target, &'a HttpPolicy) -> Job<'a, ()>;
pub type Fetch =
    for<'a> fn(&'a Target, &'a HttpPolicy, &'a Values) -> Job<'a, widgets::StructuredBody>;
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

    fn accepts(&self, value: &Value) -> bool {
        let text = || {
            value.as_str().filter(|s| {
                !s.trim().is_empty() && self.max_length.is_none_or(|max| s.len() <= max)
            })
        };
        match self.kind {
            Kind::Url => text().and_then(client::origin).is_some(),
            Kind::Secret => text().is_some_and(|s| s.bytes().all(|b| b.is_ascii_graphic())),
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

#[derive(Clone)]
pub struct Target {
    pub url: String,
    pub credential: Option<client::BoundCredential>,
    pub fingerprint: Option<[u8; 32]>,
    pub internet_enabled: bool,
    // Provider-defined non-secret connection options.
    #[allow(dead_code)]
    pub options: Values,
}

impl Target {
    pub fn link(url: String, credential: Option<client::BoundCredential>) -> Self {
        Self {
            url,
            credential,
            fingerprint: None,
            internet_enabled: true,
            options: Values::new(),
        }
    }

    pub async fn response(
        &self,
        policy: &HttpPolicy,
        max_body: Option<usize>,
    ) -> Result<client::Response, Fault> {
        let result = client::send_bound(
            self.url.clone(),
            self.credential.clone(),
            policy,
            client::BoundPolicy {
                pin: self.fingerprint,
                local_only: !self.internet_enabled,
                max_body,
            },
        )
        .await
        .map_err(Fault::from)?;
        match result.status {
            401 | 403 => Err(Class::Auth.into()),
            200..=299 => Ok(result),
            _ => Err(Class::HttpStatus.into()),
        }
    }

    pub async fn json(&self, policy: &HttpPolicy) -> Result<Value, Fault> {
        let result = self.response(policy, Some(MAX_RESPONSE)).await?;
        let mut json = serde_json::from_slice(&result.body).map_err(|_| Class::Protocol)?;
        if let Some(credential) = &self.credential {
            widgets::Scrub::new(std::slice::from_ref(&credential.token)).json(&mut json);
            // A token can also be reflected as a JSON number or boolean.
            if json.to_string().contains(&credential.token) {
                return Err(Class::Protocol.into());
            }
        }
        Ok(json)
    }
}

pub struct Fault {
    pub class: Class,
    pub fingerprint: Option<String>,
}

impl From<Class> for Fault {
    fn from(class: Class) -> Self {
        Self {
            class,
            fingerprint: None,
        }
    }
}

impl From<client::BoundError> for Fault {
    fn from(error: client::BoundError) -> Self {
        let e = error.message.as_str();
        let class = if e.starts_with("TLS") || e.starts_with("invalid TLS") {
            Class::Tls
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
}

impl Class {
    pub fn message(self) -> &'static str {
        match self {
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
            serde_json::json!({"base_url":"http://example.test", "secret":"spaces rejected"})
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
