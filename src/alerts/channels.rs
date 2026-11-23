//! `[channels.<name>]`: where notifications go (generic webhook, Slack
//! incoming webhook, Telegram bot).
//!
//! Secrets (webhook URLs, bot tokens, auth headers) are read from
//! environment variables or files at send time, never from the config for
//! Slack and Telegram. They are never logged: every error string produced
//! here goes through [`Target::scrub`], which replaces any secret it
//! contains, and the HTTP client never puts URL paths in its errors.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

use super::template::{self, Vars};
use crate::config::HttpPolicy;
use crate::http::client::{self, Request};
use crate::util::Dur;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Webhook,
    Slack,
    Telegram,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Webhook => "webhook",
            ChannelKind::Slack => "slack",
            ChannelKind::Telegram => "telegram",
        }
    }
}

/// One `[channels.<name>]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelConfig {
    #[serde(rename = "type")]
    pub kind: ChannelKind,

    // webhook
    /// Plain URL (only for URLs that are not secret; webhook only).
    pub url: Option<String>,
    pub url_env: Option<String>,
    pub url_file: Option<String>,
    /// Extra request headers (not secret).
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Header name -> environment variable holding its (secret) value.
    #[serde(default)]
    pub headers_env: BTreeMap<String, String>,
    /// JSON body template; `{{var}}` values are JSON-escaped when inserted.
    pub body: Option<String>,

    // slack
    pub webhook_url_env: Option<String>,
    pub webhook_url_file: Option<String>,

    // telegram
    pub token_env: Option<String>,
    pub token_file: Option<String>,
    /// Chat id: a number or "@channelname".
    pub chat_id: Option<Value>,
    pub chat_id_env: Option<String>,
    /// Default `https://api.telegram.org` (override for tests/proxies).
    pub api_base: Option<String>,

    /// Per-request timeout (default 10s).
    pub timeout: Option<Dur>,
}

const TELEGRAM_API: &str = "https://api.telegram.org";
/// Telegram rejects messages over 4096 characters.
const TELEGRAM_MAX: usize = 4000;

impl ChannelConfig {
    fn set_fields(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut f = |name, set: bool| {
            if set {
                v.push(name)
            }
        };
        f("url", self.url.is_some());
        f("url_env", self.url_env.is_some());
        f("url_file", self.url_file.is_some());
        f("headers", !self.headers.is_empty());
        f("headers_env", !self.headers_env.is_empty());
        f("body", self.body.is_some());
        f("webhook_url_env", self.webhook_url_env.is_some());
        f("webhook_url_file", self.webhook_url_file.is_some());
        f("token_env", self.token_env.is_some());
        f("token_file", self.token_file.is_some());
        f("chat_id", self.chat_id.is_some());
        f("chat_id_env", self.chat_id_env.is_some());
        f("api_base", self.api_base.is_some());
        v
    }

    pub fn timeout(&self) -> Duration {
        self.timeout.map(|d| d.0).unwrap_or(Duration::from_secs(10))
    }
}

/// Variables used to check body templates at config load.
fn sample_vars() -> Vars {
    template::VARS
        .iter()
        .map(|k| (k.to_string(), format!("sample \"{k}\" \\ value")))
        .collect()
}

/// Semantic checks for one channel. Messages name the channel.
pub fn validate(name: &str, c: &ChannelConfig) -> Result<(), String> {
    let who = format!("channel {name:?} ({})", c.kind.as_str());
    let err = |m: String| Err(format!("{who}: {m}"));
    if name.is_empty() {
        return Err("[channels] entry with an empty name".into());
    }
    let allowed: &[&str] = match c.kind {
        ChannelKind::Webhook => &[
            "url",
            "url_env",
            "url_file",
            "headers",
            "headers_env",
            "body",
        ],
        ChannelKind::Slack => &["webhook_url_env", "webhook_url_file"],
        ChannelKind::Telegram => &[
            "token_env",
            "token_file",
            "chat_id",
            "chat_id_env",
            "api_base",
        ],
    };
    if let Some(f) = c.set_fields().into_iter().find(|f| !allowed.contains(f)) {
        let hint = match (c.kind, f) {
            (ChannelKind::Slack, "url") => {
                " (the webhook URL is a secret: use webhook_url_env or webhook_url_file)"
            }
            _ => "",
        };
        return err(format!(
            "`{f}` does not apply to {} channels{hint}",
            c.kind.as_str()
        ));
    }
    let exactly_one = |names: &[(&str, bool)]| -> Result<(), String> {
        if names.iter().filter(|(_, s)| *s).count() != 1 {
            let all: Vec<&str> = names.iter().map(|(n, _)| *n).collect();
            return Err(format!("{who}: set exactly one of {}", all.join(", ")));
        }
        Ok(())
    };
    let t = c.timeout();
    if t < Duration::from_secs(1) || t > Duration::from_secs(60) {
        return err("timeout must be between 1s and 60s".into());
    }
    match c.kind {
        ChannelKind::Webhook => {
            exactly_one(&[
                ("url", c.url.is_some()),
                ("url_env", c.url_env.is_some()),
                ("url_file", c.url_file.is_some()),
            ])?;
            if let Some(u) = &c.url {
                check_url(u).or_else(|e| err(format!("url: {e}")))?;
            }
            for h in c.headers.keys().chain(c.headers_env.keys()) {
                if hyper::header::HeaderName::from_bytes(h.as_bytes()).is_err() {
                    return err(format!("invalid header name {h:?}"));
                }
            }
            for (h, v) in &c.headers {
                if hyper::header::HeaderValue::from_str(v).is_err() {
                    return err(format!("invalid value for header {h:?}"));
                }
            }
            if let Some(b) = &c.body {
                template::check(b, template::VARS).or_else(|e| err(format!("body: {e}")))?;
                let rendered = template::render(b, &sample_vars(), true);
                if let Err(e) = serde_json::from_slice::<Value>(rendered.as_bytes()) {
                    return err(format!("body is not valid JSON once filled in: {e}"));
                }
            }
        }
        ChannelKind::Slack => {
            exactly_one(&[
                ("webhook_url_env", c.webhook_url_env.is_some()),
                ("webhook_url_file", c.webhook_url_file.is_some()),
            ])?;
        }
        ChannelKind::Telegram => {
            exactly_one(&[
                ("token_env", c.token_env.is_some()),
                ("token_file", c.token_file.is_some()),
            ])?;
            exactly_one(&[
                ("chat_id", c.chat_id.is_some()),
                ("chat_id_env", c.chat_id_env.is_some()),
            ])?;
            if let Some(id) = &c.chat_id {
                if !(id.is_i64() || id.as_str().is_some_and(|s| !s.is_empty())) {
                    return err("chat_id must be a number or a non-empty string".into());
                }
            }
            if let Some(b) = &c.api_base {
                check_url(b).or_else(|e| err(format!("api_base: {e}")))?;
            }
        }
    }
    Ok(())
}

fn check_url(u: &str) -> Result<(), String> {
    if !(u.starts_with("http://") || u.starts_with("https://")) || u.parse::<hyper::Uri>().is_err()
    {
        return Err("must be an http:// or https:// URL".into());
    }
    Ok(())
}

/// A channel with its secrets resolved, ready to send.
#[derive(Debug, Clone)]
pub struct Target {
    kind: ChannelKind,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    chat_id: Option<Value>,
    timeout: Duration,
    secrets: Vec<String>,
}

impl Target {
    /// Replace every secret in `s` by `<redacted>`.
    pub fn scrub(&self, s: &str) -> String {
        let mut out = s.to_string();
        for sec in &self.secrets {
            if sec.len() >= 4 {
                out = out.replace(sec.as_str(), "<redacted>");
            }
        }
        out
    }
}

/// Read a secret from an environment variable or a file. Errors name the
/// variable or file, never the value.
fn secret(env: Option<&str>, file: Option<&str>) -> Result<String, String> {
    let v = if let Some(var) = env {
        std::env::var(var).map_err(|_| format!("environment variable {var} is not set"))?
    } else if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {}", e.kind()))?
    } else {
        return Err("no secret source configured".into());
    };
    let v = v.trim().to_string();
    if v.is_empty() {
        return Err(match (env, file) {
            (Some(var), _) => format!("environment variable {var} is empty"),
            (_, Some(path)) => format!("{path} is empty"),
            _ => "empty secret".into(),
        });
    }
    Ok(v)
}

/// Add a URL and its path to the secrets to scrub.
fn url_secrets(url: &str, out: &mut Vec<String>) {
    out.push(url.to_string());
    if let Ok(u) = url.parse::<hyper::Uri>() {
        if let Some(pq) = u.path_and_query() {
            if pq.as_str().len() > 1 {
                out.push(pq.as_str().to_string());
            }
        }
    }
}

/// Resolve secrets. An error here means the channel is misconfigured.
pub fn resolve(c: &ChannelConfig) -> Result<Target, String> {
    let mut secrets = Vec::new();
    let mut headers = Vec::new();
    let mut chat_id = None;
    let url = match c.kind {
        ChannelKind::Webhook => {
            let url = match &c.url {
                Some(u) => u.clone(),
                None => secret(c.url_env.as_deref(), c.url_file.as_deref())?,
            };
            url_secrets(&url, &mut secrets);
            for (k, v) in &c.headers {
                secrets.push(v.clone());
                headers.push((k.clone(), v.clone()));
            }
            secrets.extend(c.body.iter().cloned());
            for (k, var) in &c.headers_env {
                let v = secret(Some(var), None)?;
                if hyper::header::HeaderValue::from_str(&v).is_err() {
                    return Err(format!(
                        "environment variable {var} is not a valid header value"
                    ));
                }
                secrets.push(v.clone());
                headers.push((k.clone(), v));
            }
            url
        }
        ChannelKind::Slack => {
            let u = secret(c.webhook_url_env.as_deref(), c.webhook_url_file.as_deref())?;
            url_secrets(&u, &mut secrets);
            u
        }
        ChannelKind::Telegram => {
            let token = secret(c.token_env.as_deref(), c.token_file.as_deref())?;
            if token.contains(['/', '?', '#', ' ']) {
                return Err("the bot token contains invalid characters".into());
            }
            secrets.push(token.clone());
            chat_id = Some(match (&c.chat_id, &c.chat_id_env) {
                (Some(id), _) => id.clone(),
                (None, Some(var)) => Value::String(secret(Some(var), None)?),
                (None, None) => return Err("no chat_id configured".into()),
            });
            let base = c.api_base.as_deref().unwrap_or(TELEGRAM_API);
            format!("{}/bot{token}/sendMessage", base.trim_end_matches('/'))
        }
    };
    if !(url.starts_with("http://") || url.starts_with("https://"))
        || url.parse::<hyper::Uri>().is_err()
    {
        return Err("the configured URL is not a valid http(s) URL".into());
    }
    Ok(Target {
        kind: c.kind,
        url,
        headers,
        body: c.body.clone(),
        chat_id,
        timeout: c.timeout(),
        secrets,
    })
}

/// The request body for this channel.
fn payload(t: &Target, vars: &Vars) -> Vec<u8> {
    let get = |k: &str| vars.get(k).cloned().unwrap_or_default();
    let v = match t.kind {
        ChannelKind::Webhook => {
            if let Some(b) = &t.body {
                return template::render(b, vars, true).into_bytes();
            }
            json!({
                "kind": get("kind"),
                "title": get("title"),
                "message": get("message"),
                "rule": get("rule"),
                "subject": get("subject"),
                "state": get("state"),
                "value": get("value"),
                "time": get("time"),
                "host": get("host"),
            })
        }
        // Slack treats `<...>` as links/mentions (`<!channel>`) and `&` as an
        // entity start; container names and error texts must not inject those.
        ChannelKind::Slack => json!({ "text": slack_escape(&get("message")) }),
        ChannelKind::Telegram => {
            let mut text = get("message");
            if text.chars().count() > TELEGRAM_MAX {
                text = text.chars().take(TELEGRAM_MAX).collect::<String>() + "…";
            }
            json!({
                "chat_id": t.chat_id.clone().unwrap_or(Value::Null),
                "text": text,
                "disable_web_page_preview": true,
            })
        }
    };
    serde_json::to_vec(&v).unwrap_or_default()
}

/// Send one notification (one attempt). The error is safe to log and show.
pub async fn send(t: &Target, vars: &Vars, policy: &HttpPolicy) -> Result<(), String> {
    send_inner(t, vars, policy).await.map_err(|e| t.scrub(&e))
}

async fn send_inner(t: &Target, vars: &Vars, policy: &HttpPolicy) -> Result<(), String> {
    let payload = payload(t, vars);
    let mut req = Request::post_json(t.url.clone(), payload.clone()).timeout(t.timeout);
    for (k, v) in &t.headers {
        req.headers.retain(|(h, _)| !h.eq_ignore_ascii_case(k));
        req = req.header(k.clone(), v.clone());
    }
    let resp = client::send(req, policy).await?;
    if t.kind == ChannelKind::Telegram {
        let body: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
        if resp.is_success() && body["ok"] == Value::Bool(true) {
            return Ok(());
        }
        let desc = body["description"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| excerpt(&resp.text()));
        return Err(format!("HTTP {}: {desc}", resp.status));
    }
    if !resp.is_success() {
        let mut text = resp.text();
        if let Some(body) = std::str::from_utf8(&payload)
            .ok()
            .filter(|body| !body.is_empty())
        {
            text = text.replace(body, "<redacted>");
        }
        return Err(format!(
            "HTTP {}: {}",
            resp.status,
            excerpt(&t.scrub(&text))
        ));
    }
    Ok(())
}

/// First line-ish of a response body, for error messages.
fn excerpt(s: &str) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(160)
        .collect();
    let one = one.trim().to_string();
    if one.is_empty() {
        "(empty response)".into()
    } else {
        one
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chan(src: &str) -> ChannelConfig {
        toml_edit::de::from_str(src).unwrap()
    }

    fn vars() -> Vars {
        [
            ("kind", "firing"),
            ("title", "🔴 odoo: stopped"),
            ("message", "🔴 odoo: stopped (exit 137) \"quoted\""),
            ("subject", "odoo"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn inline_webhook_values_are_scrubbed_from_diagnostics() {
        let target = resolve(&chan(
            r#"
type = "webhook"
url = "https://example.test/hook-secret"
headers = { Authorization = "header-secret" }
body = '{"message":"body-secret"}'
"#,
        ))
        .unwrap();
        assert_eq!(
            target.scrub(
                r#"https://example.test/hook-secret header-secret {"message":"body-secret"}"#
            ),
            "<redacted> <redacted> <redacted>"
        );
    }

    #[test]
    fn validation() {
        assert!(validate("w", &chan("type='webhook'\nurl='http://x/hook'")).is_ok());
        let e = validate("w", &chan("type='webhook'")).unwrap_err();
        assert!(
            e.contains("channel \"w\"") && e.contains("exactly one of url"),
            "{e}"
        );
        let e = validate("s", &chan("type='slack'\nurl='https://hooks.slack.com/x'")).unwrap_err();
        assert!(e.contains("secret"), "{e}");
        let e = validate("t", &chan("type='telegram'\ntoken_env='T'")).unwrap_err();
        assert!(e.contains("chat_id"), "{e}");
        assert!(validate(
            "t",
            &chan("type='telegram'\ntoken_env='T'\nchat_id=-100123")
        )
        .is_ok());
        assert!(validate(
            "t",
            &chan("type='telegram'\ntoken_env='T'\nchat_id='@chan'")
        )
        .is_ok());
        let e = validate(
            "w",
            &chan("type='webhook'\nurl='http://x'\nbody='{\"t\":{{title}}}'"),
        )
        .unwrap_err();
        assert!(e.contains("not valid JSON"), "{e}");
        let e = validate(
            "w",
            &chan("type='webhook'\nurl='http://x'\nbody='{\"t\":\"{{titel}}\"}'"),
        )
        .unwrap_err();
        assert!(e.contains("titel"), "{e}");
        assert!(validate(
            "w",
            &chan("type='webhook'\nurl='http://x'\nbody='{\"t\":\"{{title}}\"}'")
        )
        .is_ok());
        let e = validate("w", &chan("type='webhook'\nurl='http://x'\nchat_id=1")).unwrap_err();
        assert!(e.contains("chat_id"), "{e}");
        // Secrets cannot even be written in the config for slack/telegram.
        assert!(
            toml_edit::de::from_str::<ChannelConfig>("type='telegram'\ntoken='123:abc'").is_err()
        );
        assert!(toml_edit::de::from_str::<ChannelConfig>("type='slak'").is_err());
    }

    #[test]
    fn missing_secret_is_misconfigured() {
        let c = chan("type='slack'\nwebhook_url_env='RH_TEST_SURELY_UNSET_VAR'");
        let e = resolve(&c).unwrap_err();
        assert_eq!(
            e,
            "environment variable RH_TEST_SURELY_UNSET_VAR is not set"
        );
        let c = chan("type='telegram'\ntoken_file='/nonexistent/rh-token'\nchat_id=1");
        assert!(resolve(&c).unwrap_err().contains("/nonexistent/rh-token"));
    }

    #[test]
    fn payloads() {
        let dir = std::env::temp_dir().join(format!("rh-chan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("token");
        std::fs::write(&f, "123456:SECRET-token\n").unwrap();
        let c = chan(&format!(
            "type='telegram'\ntoken_file='{}'\nchat_id=42\napi_base='http://127.0.0.1:1/'",
            f.display()
        ));
        let t = resolve(&c).unwrap();
        assert_eq!(
            t.url,
            "http://127.0.0.1:1/bot123456:SECRET-token/sendMessage"
        );
        let p: Value = serde_json::from_slice(&payload(&t, &vars())).unwrap();
        assert_eq!(p["chat_id"], 42);
        assert_eq!(p["text"], "🔴 odoo: stopped (exit 137) \"quoted\"");
        assert_eq!(p["disable_web_page_preview"], true);
        assert!(p.get("parse_mode").is_none());
        assert_eq!(t.scrub("x 123456:SECRET-token y"), "x <redacted> y");

        let w = resolve(&chan("type='webhook'\nurl='http://h/x'")).unwrap();
        let p: Value = serde_json::from_slice(&payload(&w, &vars())).unwrap();
        assert_eq!(p["kind"], "firing");
        assert_eq!(p["subject"], "odoo");
        let w = resolve(&chan(
            "type='webhook'\nurl='http://h/x'\nbody='{\"topic\":\"ops\",\"msg\":\"{{message}}\"}'",
        ))
        .unwrap();
        let p: Value = serde_json::from_slice(&payload(&w, &vars())).unwrap();
        assert_eq!(p["msg"], "🔴 odoo: stopped (exit 137) \"quoted\"");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Escape the three characters Slack's message formatting interprets.
fn slack_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod slack_tests {
    #[test]
    fn slack_text_is_escaped() {
        assert_eq!(
            super::slack_escape("a <!channel> & <http://x|y>"),
            "a &lt;!channel&gt; &amp; &lt;http://x|y&gt;"
        );
    }
}
