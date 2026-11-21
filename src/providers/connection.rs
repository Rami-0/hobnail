use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    sync::Mutex,
};

use hyper::{header::HeaderName, Method};
use tokio::sync::{watch, OwnedSemaphorePermit};

use super::{Class, Fault, Values, MAX_RESPONSE, REQUEST_TIMEOUT};
use crate::{
    config::HttpPolicy,
    http::client::{self, BoundCredential, BoundPolicy, Response},
};

const MAX_REQUEST: usize = 16 * 1024;
const MAX_SESSION: usize = 1024;
const MAX_SESSIONS: usize = 16;

pub type Credentials = HashMap<String, BoundCredential>;

pub(crate) struct Target {
    pub url: String,
    pub credentials: Credentials,
    pub fingerprint: Option<[u8; 32]>,
    pub internet_enabled: bool,
    pub options: Values,
    pub revision: Revision,
    session_key: Option<SessionKey>,
}

impl Target {
    pub fn new(url: String, credentials: Credentials, internet_enabled: bool) -> Self {
        Self {
            url,
            credentials,
            fingerprint: None,
            internet_enabled,
            options: Values::new(),
            revision: Revision::default(),
            session_key: None,
        }
    }
}

type Signal = (u64, watch::Sender<()>);

#[derive(Default)]
pub(crate) struct Connections {
    revisions: Mutex<Revisions>,
    sessions: Mutex<VecDeque<(SessionKey, Session)>>,
    creating: [tokio::sync::Mutex<()>; 4],
}

#[derive(Default)]
struct Revisions {
    policy: Option<Signal>,
    integrations: HashMap<String, Signal>,
    next: u64,
}

fn signal(next: &mut u64) -> Signal {
    *next += 1;
    (*next, watch::channel(()).0)
}

#[derive(Clone, Default)]
pub(crate) struct Revision {
    http: Option<watch::Receiver<()>>,
    policy: Option<watch::Receiver<()>>,
    integration: Option<watch::Receiver<()>>,
}

impl Revision {
    pub fn check(&self) -> Result<(), String> {
        if [&self.http, &self.policy, &self.integration]
            .into_iter()
            .flatten()
            .any(|rx| rx.has_changed().unwrap_or(true))
        {
            return Err("connection changed".into());
        }
        Ok(())
    }

    async fn cancelled(mut self) {
        async fn wait(rx: Option<&mut watch::Receiver<()>>) {
            match rx {
                Some(rx) => {
                    let _ = rx.changed().await;
                }
                None => std::future::pending().await,
            }
        }
        tokio::select! {
            _ = wait(self.http.as_mut()) => {},
            _ = wait(self.policy.as_mut()) => {},
            _ = wait(self.integration.as_mut()) => {},
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SessionKey {
    integration: String,
    origin: String,
    // A single revision rotates for credential, pin, URL and option changes.
    revision: u64,
}

impl Connections {
    /// Capture with the database snapshot, under the same lock as invalidation.
    pub(crate) fn bind(&self, target: &mut Target, id: Option<&str>, cache: bool) {
        let mut revisions = self.revisions.lock().unwrap();
        let Revisions {
            policy,
            integrations,
            next,
        } = &mut *revisions;
        target.revision.policy = Some(policy.get_or_insert_with(|| signal(next)).1.subscribe());
        if let Some(id) = id {
            let (version, tx) = integrations
                .entry(id.into())
                .or_insert_with(|| signal(next));
            target.revision.integration = Some(tx.subscribe());
            if cache {
                target.session_key = client::origin(&target.url).map(|origin| SessionKey {
                    integration: id.into(),
                    origin,
                    revision: *version,
                });
            }
        }
    }

    pub(crate) fn invalidate(&self, id: Option<&str>) {
        let mut revisions = self.revisions.lock().unwrap();
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(id) = id {
            revisions.integrations.remove(id);
            sessions.retain(|(key, _)| key.integration != id);
        } else {
            revisions.policy = None;
            sessions.clear();
        }
    }
}

/// Opaque provider session material. Store only the part needed to authenticate
/// the next request (for example a SID, cookie header value or bearer token).
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    scope: u64,
    value: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Session([redacted])")
    }
}

/// A request input. Credentials and session material have no string getter.
pub enum Input<'a> {
    Text(&'a str),
    Credential(&'a str),
    Session(&'a Session),
}

impl fmt::Debug for Input<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Input([redacted])")
    }
}

pub enum Auth<'a> {
    Header(&'a str, Input<'a>),
    Bearer(Input<'a>),
    #[allow(dead_code)] // Available to providers that use HTTP Basic.
    Basic(Input<'a>, Input<'a>),
    #[allow(dead_code)] // Available to providers with custom authorization schemes.
    Authorization(&'a str, Input<'a>),
}

pub enum Body<'a> {
    Json(Values, &'a [(&'a str, Input<'a>)]),
    #[allow(dead_code)] // Available to providers with form-based login.
    Form(&'a [(&'a str, Input<'a>)]),
}

#[derive(Clone, Copy)]
pub enum Verb {
    Get,
    Post,
    #[allow(dead_code)] // Available for provider APIs using PUT.
    Put,
    Delete,
}

pub struct Step<'a> {
    pub verb: Verb,
    /// Empty uses the exact saved endpoint. Otherwise this is relative to
    /// the saved path prefix, even when it begins with a single slash.
    pub path: &'a str,
    pub auth: &'a [Auth<'a>],
    pub query: &'a [(&'a str, Input<'a>)],
    pub body: Option<Body<'a>>,
    pub response_headers: &'a [&'a str],
    pub status_only: bool,
}

impl<'a> Step<'a> {
    pub fn get(path: &'a str) -> Self {
        Self {
            verb: Verb::Get,
            path,
            auth: &[],
            query: &[],
            body: None,
            response_headers: &[],
            status_only: false,
        }
    }
}

impl fmt::Debug for Step<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Step([redacted])")
    }
}

pub struct Conn<'a> {
    target: &'a Target,
    revision: Revision,
    origin: String,
    session_scope: u64,
    policy: &'a HttpPolicy,
    connections: &'a Connections,
    auth_output: Mutex<HashSet<String>>,
    _permit: OwnedSemaphorePermit,
}

impl<'a> Conn<'a> {
    pub(crate) fn new(
        target: &'a Target,
        policy: &'a HttpPolicy,
        connections: &'a Connections,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self, Fault> {
        let origin = client::origin(&target.url).ok_or(Class::Blocked)?;
        let session_scope = target.session_key.as_ref().map_or_else(
            || {
                let mut revisions = connections.revisions.lock().unwrap();
                revisions.next += 1;
                revisions.next
            },
            |key| key.revision,
        );
        Ok(Self {
            session_scope,
            revision: target.revision.clone(),
            target,
            origin,
            policy,
            connections,
            auth_output: Mutex::default(),
            _permit: permit,
        })
    }

    pub(crate) fn with_http_revision(mut self, revision: watch::Receiver<()>) -> Self {
        self.revision.http = Some(revision);
        self
    }

    pub(crate) async fn run<T>(
        &self,
        job: impl Future<Output = Result<T, Fault>>,
    ) -> Result<T, Fault> {
        tokio::select! {
            biased;
            _ = self.revision.clone().cancelled() => Err(Class::Changed.into()),
            result = tokio::time::timeout(REQUEST_TIMEOUT, job) => result.unwrap_or_else(|_| Err(Class::Timeout.into())),
        }
    }

    pub fn options(&self) -> &Values {
        &self.target.options
    }

    pub fn has_credential(&self, field: &str) -> bool {
        self.target.credentials.contains_key(field)
    }

    pub(super) fn has_credentials(&self) -> bool {
        !self.target.credentials.is_empty()
    }

    fn remember_auth(&self, value: &str) {
        if !value.is_empty() {
            let mut values = self.auth_output.lock().unwrap();
            if values.len() <= super::redaction::MAX_SECRETS && !values.contains(value) {
                values.insert(value.to_owned());
            }
        }
    }

    pub(super) fn output_redactor(&self) -> super::OutputRedactor {
        let emitted = self.auth_output.lock().unwrap();
        super::OutputRedactor::new(
            emitted.iter().map(String::as_str).chain(
                self.target
                    .credentials
                    .values()
                    .map(|value| value.token.as_str()),
            ),
        )
    }

    fn resolve<'b>(&'b self, input: &'b Input<'_>) -> Result<&'b str, Fault> {
        match input {
            Input::Text(text) => Ok(text),
            Input::Session(session) if session.scope == self.session_scope => {
                self.remember_auth(&session.value);
                Ok(&session.value)
            }
            Input::Session(_) => Err(Class::Blocked.into()),
            Input::Credential(field) => {
                let credential = self.target.credentials.get(*field).ok_or(Class::Auth)?;
                if credential.origin != self.origin {
                    return Err(Class::Blocked.into());
                }
                Ok(&credential.token)
            }
        }
    }

    fn auth_value<'b>(&'b self, input: &'b Input<'_>) -> Result<&'b str, Fault> {
        if matches!(input, Input::Text(_)) {
            return Err(Class::Protocol.into());
        }
        self.resolve(input)
    }

    pub async fn request(&self, step: Step<'_>) -> Result<Response, Fault> {
        let mut url = join(&self.target.url, step.path)?;
        if client::origin(&url).as_deref() != Some(self.origin.as_str()) {
            return Err(Class::Blocked.into());
        }
        if !step.query.is_empty() {
            url.push('?');
            url.push_str(&self.pairs(step.query)?);
        }
        let mut request = client::Request::get(url);
        request.method = match step.verb {
            Verb::Get => Method::GET,
            Verb::Post => Method::POST,
            Verb::Put => Method::PUT,
            Verb::Delete => Method::DELETE,
        };
        for auth in step.auth {
            let (name, value) = match auth {
                Auth::Header(name, input) => (*name, self.auth_value(input)?.to_owned()),
                Auth::Bearer(input) => (
                    "authorization",
                    format!("Bearer {}", self.auth_value(input)?),
                ),
                Auth::Basic(user, password) => {
                    let user = self.auth_value(user)?;
                    let password = self.auth_value(password)?;
                    let raw = basic_input(user, password)?;
                    self.remember_auth(&raw);
                    ("authorization", format!("Basic {}", base64(&raw)))
                }
                Auth::Authorization(prefix, input) => (
                    "authorization",
                    format!("{prefix}{}", self.auth_value(input)?),
                ),
            };
            let header = HeaderName::try_from(name).map_err(|_| Class::Protocol)?;
            if matches!(
                header.as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "proxy-authorization"
                    | "proxy-connection"
                    | "upgrade"
                    | "te"
                    | "trailer"
                    | "expect"
                    | "content-type"
            ) {
                return Err(Class::Protocol.into());
            }
            self.remember_auth(&value);
            request.headers.push((header.as_str().into(), value));
        }
        if let Some(body) = step.body {
            let (content_type, bytes) = match body {
                Body::Json(mut fields, inputs) => {
                    for (name, input) in inputs {
                        fields.insert((*name).into(), self.resolve(input)?.into());
                    }
                    (
                        "application/json",
                        serde_json::to_vec(&fields).map_err(|_| Class::Protocol)?,
                    )
                }
                Body::Form(inputs) => (
                    "application/x-www-form-urlencoded",
                    self.pairs(inputs)?.into_bytes(),
                ),
            };
            if bytes.len() > MAX_REQUEST {
                return Err(Class::TooLarge.into());
            }
            request
                .headers
                .push(("content-type".into(), content_type.into()));
            request.body = Some(bytes);
        }
        let headers = step
            .response_headers
            .iter()
            .map(|name| HeaderName::try_from(*name))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Class::Protocol)?;
        client::send_step(
            request,
            &self.origin,
            self.policy,
            BoundPolicy {
                pin: self.target.fingerprint,
                local_only: !self.target.internet_enabled,
                max_body: Some(MAX_RESPONSE),
                status_only: step.status_only,
                headers,
                revision: self.revision.clone(),
            },
        )
        .await
        .map_err(Fault::from)
    }

    fn pairs(&self, fields: &[(&str, Input<'_>)]) -> Result<String, Fault> {
        let mut encoded = String::new();
        for (index, (name, value)) in fields.iter().enumerate() {
            if index != 0 {
                encoded.push('&');
            }
            encode(&mut encoded, name);
            encoded.push('=');
            encode(&mut encoded, self.resolve(value)?);
            if encoded.len() > MAX_REQUEST {
                return Err(Class::TooLarge.into());
            }
        }
        Ok(encoded)
    }

    pub fn session(&self, value: &str) -> Result<Session, Fault> {
        if value.is_empty() || value.len() > MAX_SESSION {
            return Err(Class::Protocol.into());
        }
        self.remember_auth(value);
        Ok(Session {
            scope: self.session_scope,
            value: value.into(),
        })
    }

    #[allow(dead_code)] // Available to challenge and cookie based providers.
    pub fn session_header(&self, response: &Response, name: &str) -> Result<Session, Fault> {
        self.session(
            response
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .ok_or(Class::Protocol)?,
        )
    }

    pub fn uses_session_cache(&self) -> bool {
        self.target.session_key.is_some()
    }

    pub async fn get_or_create_session(
        &self,
        create: impl Future<Output = Result<Session, Fault>>,
    ) -> Result<Session, Fault> {
        let Some(key) = &self.target.session_key else {
            return create.await;
        };
        // A fixed number of locks avoids duplicate logins without allocating
        // a lock per integration. Waiting consumes the operation's deadline.
        let _login = self.connections.creating
            [key.revision as usize % self.connections.creating.len()]
        .lock()
        .await;
        if let Some((_, session)) = self
            .connections
            .sessions
            .lock()
            .unwrap()
            .iter()
            .find(|(entry, _)| entry == key)
        {
            return Ok(session.clone());
        }
        let session = create.await?;
        self.store_session(key, &session)?;
        Ok(session)
    }

    fn store_session(&self, key: &SessionKey, session: &Session) -> Result<(), Fault> {
        if session.scope != self.session_scope {
            return Err(Class::Blocked.into());
        }
        let mut sessions = self.connections.sessions.lock().unwrap();
        self.revision.check().map_err(|_| Class::Changed)?;
        sessions.retain(|(entry, _)| entry != key);
        if sessions.len() == MAX_SESSIONS {
            sessions.pop_front();
        }
        sessions.push_back((key.clone(), session.clone()));
        Ok(())
    }

    fn discard_session(&self, session: &Session) {
        if let Some(key) = &self.target.session_key {
            self.connections
                .sessions
                .lock()
                .unwrap()
                .retain(|(entry, cached)| entry != key || cached != session);
        }
    }

    pub async fn retry_session<T, C, R>(
        &self,
        mut create: impl FnMut() -> C,
        mut request: impl FnMut(Session) -> R,
    ) -> Result<T, Fault>
    where
        C: Future<Output = Result<Session, Fault>>,
        R: Future<Output = Result<T, Fault>>,
    {
        let session = self.get_or_create_session(create()).await?;
        let error = match request(session.clone()).await {
            Err(error) if matches!(error.class, Class::Auth | Class::Conflict) => error,
            result => return result,
        };
        self.discard_session(&session);
        let next = if let Some(replacement) = error.session {
            if let Some(key) = &self.target.session_key {
                self.store_session(key, &replacement)?;
            }
            replacement
        } else {
            self.get_or_create_session(create()).await?
        };
        let result = request(next.clone()).await;
        if result
            .as_ref()
            .is_err_and(|error| matches!(error.class, Class::Auth | Class::Conflict))
        {
            self.discard_session(&next);
        }
        result
    }
}

pub fn success(response: Response) -> Result<Response, Fault> {
    match response.status {
        200..=299 => Ok(response),
        401 | 403 => Err(Class::Auth.into()),
        409 => Err(Class::Conflict.into()),
        _ => Err(Class::HttpStatus.into()),
    }
}

fn join(base: &str, path: &str) -> Result<String, Fault> {
    if path.is_empty() {
        return Ok(base.into());
    }
    // Provider paths are fixed endpoint names. Disallow escape sequences
    // entirely, including nested percent encoding and encoded dot segments.
    if path.contains("//")
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-._~".contains(&b))
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
    {
        return Err(Class::Blocked.into());
    }
    Ok(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}

pub(super) fn encode(out: &mut String, text: &str) {
    use std::fmt::Write;
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
}

fn basic_input(user: &str, password: &str) -> Result<String, Fault> {
    if user.contains(':') {
        return Err(Class::Protocol.into());
    }
    Ok(format!("{user}:{password}"))
}

pub(super) fn base64(raw: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(raw.len().div_ceil(3) * 4);
    for chunk in raw.as_bytes().chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        for (position, digit) in [
            a >> 2,
            (a << 4 | b >> 4) & 63,
            (b << 2 | c >> 6) & 63,
            c & 63,
        ]
        .into_iter()
        .enumerate()
        {
            out.push(if position > chunk.len() {
                '='
            } else {
                ALPHABET[usize::from(digit)] as char
            });
        }
    }
    out
}

#[cfg(test)]
pub(in crate::providers) mod tests;
