//! Account credentials, sessions, CSRF and login throttling.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, OnceLock};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use http_body_util::{BodyExt, Limited};
use hyper::header::{
    HeaderValue, CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, ORIGIN, REFERER, RETRY_AFTER,
    SET_COOKIE,
};
use hyper::{HeaderMap, Method, Request, Response, StatusCode};
use ring::{digest, rand::SecureRandom};

use crate::api::error::ApiError;
use crate::db::accounts::AccountError;

use crate::db::{Db, EventFilter};
use crate::http::server::{full, set, Body, Peer};
use crate::state::AppState;

pub const COOKIE_NAME: &str = "rh_session";
pub const CSRF_HEADER: &str = "x-rh-csrf";
/// Sessions last this long after their last use.
pub const SESSION_TTL_SECS: i64 = 7 * 86_400;
pub const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_BYTES: usize = 1024;
/// Login request bodies larger than this are refused.
const MAX_BODY: usize = 4096;

pub(crate) fn now_s() -> i64 {
    crate::util::unix_now() as i64
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Contract used by the UI and the router

pub fn current_user(state: &AppState, headers: &HeaderMap) -> Option<String> {
    request_session(state, headers).map(|s| s.account.name)
}

pub fn password_is_set(state: &AppState) -> bool {
    state.db.owner_exists()
}

pub fn csrf_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    request_session(state, headers).map(|s| csrf_for(&s.token))
}

pub(crate) fn request_session(state: &AppState, headers: &HeaderMap) -> Option<Session> {
    session_of(&state.db, headers, now_s())
}

/// Why a write was refused. `response()` gives the HTTP answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDenied {
    /// No valid session cookie → 401.
    NoSession,
    /// Missing or wrong `X-RH-CSRF` header → 403.
    BadCsrf,
    /// `Origin` (or `Referer`) missing or not this host → 403.
    BadOrigin,
}

impl WriteDenied {
    pub fn response(self) -> hyper::Response<crate::http::server::Body> {
        use crate::http::server::text;
        match self {
            Self::NoSession => text(StatusCode::UNAUTHORIZED, "sign in required\n"),
            Self::BadCsrf => text(StatusCode::FORBIDDEN, "missing or bad X-RH-CSRF header\n"),
            Self::BadOrigin => text(StatusCode::FORBIDDEN, "cross-origin write refused\n"),
        }
    }
}

pub(crate) fn check_write(session: &Session, headers: &HeaderMap) -> Result<(), WriteDenied> {
    let got = headers
        .get(CSRF_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or(b"");
    if !ct_eq(got, csrf_for(&session.token).as_bytes()) {
        return Err(WriteDenied::BadCsrf);
    }
    if !same_origin(headers) {
        return Err(WriteDenied::BadOrigin);
    }
    Ok(())
}

/// `/api/login` (POST JSON credentials; checks Origin and rate limits)
/// and `/api/logout` (POST, already passed `require_write`).
/// The peer address is in `req.extensions().get::<http::server::Peer>()`.
pub(crate) async fn route(
    state: &Arc<AppState>,
    req: Request<hyper::body::Incoming>,
    session: Option<&Session>,
) -> Response<Body> {
    if req.method() != Method::POST {
        let mut r = json_error(StatusCode::METHOD_NOT_ALLOWED, "POST only");
        r.headers_mut()
            .insert(hyper::header::ALLOW, HeaderValue::from_static("POST"));
        return r;
    }
    if req.uri().path() == "/api/logout" {
        return logout(state, session.expect("logout gate"));
    }
    credential_route(state, req, CredentialAction::Login).await
}

/// `hobnail set-password`: reads the new password from the TTY without
/// echo (twice), or one line from stdin when piped; stores the hash in the
/// database and signs every session out.
pub fn cli_set_password(args: &[String]) -> i32 {
    if args.len() > 1 || args.first().is_some_and(|name| !valid_name(name)) {
        eprintln!("usage: hobnail set-password [username] (1–64 letters, digits, dots, hyphens or underscores)");
        return 1;
    }
    let path = crate::db::path_from_env();
    let db = match Db::open(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("set-password: cannot open the database: {e}");
            return 1;
        }
    };
    let had = db.owner_exists();
    let password = if unsafe { libc::isatty(0) } == 1 {
        let a = match tty::read_secret("New password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("set-password: {e}; nothing changed");
                return 1;
            }
        };
        if let Err(e) = check_password_policy(&a) {
            eprintln!("set-password: {e}; nothing changed");
            return 1;
        }
        let b = match tty::read_secret("Repeat password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("set-password: {e}; nothing changed");
                return 1;
            }
        };
        if a != b {
            eprintln!("set-password: the passwords do not match; nothing changed");
            return 1;
        }
        a
    } else {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => {
                eprintln!("set-password: no password on stdin; nothing changed");
                return 1;
            }
            Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            Err(_) => {
                eprintln!("set-password: could not read stdin; nothing changed");
                return 1;
            }
        }
    };
    let result = check_password_policy(&password)
        .and_then(|()| hash_password(&password))
        .and_then(|hash| {
            db.recover_owner(args.first().map(String::as_str), &hash, now_s())
                .map_err(|e| format!("{e:?}"))
        });
    match result {
        Ok(()) => {
            db.record_event(
                now_s(),
                "auth.password_changed",
                "info",
                "owner",
                "set from the console",
            );
            println!(
                "Password {} in {}. The owner's sessions were signed out.",
                if had { "changed" } else { "set" },
                path.display()
            );
            0
        }
        Err(e) => {
            eprintln!("set-password: {e}; nothing changed");
            1
        }
    }
}

pub(crate) fn change_password_for(
    state: &AppState,
    session: &Session,
    current: &str,
    new: &str,
) -> Result<(), String> {
    check_password_policy(new)?;
    let _permit = state
        .login_verifying
        .try_lock()
        .map_err(|_| "too many attempts; try again in 1 s")?;
    let key = session.account.id;
    let lock = || {
        state
            .password_limiter
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    };
    if let Err(secs) = lock().check(key, now_ms()) {
        return Err(format!("too many attempts; try again in {secs} s"));
    }
    let (id, stored, _) = state
        .db
        .credential(&session.account.name)
        .ok_or("sign in required")?;
    if current.len() > MAX_PASSWORD_BYTES || !verify_password(&stored, current) {
        lock().failure(key, now_ms());
        state.db.record_event(
            now_s(),
            "auth.login_failed",
            "warn",
            &session.account.name,
            "wrong current password when changing it",
        );
        return Err("the current password is wrong".into());
    }
    let hash = hash_password(new)?;
    state
        .db
        .change_account_password(id, &stored, &hash, &session.hash, now_s())
        .map_err(|_| "could not change password; reload and retry")?;
    lock().success(key);
    state.db.record_event(
        now_s(),
        "auth.password_changed",
        "info",
        &session.account.name,
        "changed in the app",
    );
    Ok(())
}

/// Recent `auth.login` / `auth.login_failed` events, newest first.
pub fn recent_logins(state: &AppState, limit: usize) -> Vec<crate::db::Event> {
    login_events(&state.db, limit)
}

fn login_events(db: &Db, limit: usize) -> Vec<crate::db::Event> {
    db.events(&EventFilter {
        kind_prefix: Some("auth.login".into()),
        limit,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Password storage

pub fn check_password_policy(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!(
            "the password must have at least {MIN_PASSWORD_CHARS} characters"
        ));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(format!(
            "the password must be at most {MAX_PASSWORD_BYTES} bytes"
        ));
    }
    if password.chars().any(|c| c.is_control()) {
        return Err("the password must not contain control characters".into());
    }
    Ok(())
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "the system random number generator failed".to_string())?;
    Ok(b)
}

fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

pub(crate) fn random_token() -> Result<String, String> {
    random_bytes::<32>().map(|bytes| hex(&bytes))
}

fn argon2(memory: u32, passes: u32) -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(memory, passes, 1, Some(32)).expect("fixed Argon2id parameters"),
    )
}

fn hash_password(password: &str) -> Result<String, String> {
    hash_with(&argon2(19_456, 2), password)
}

fn hash_with(algorithm: &Argon2<'_>, password: &str) -> Result<String, String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()?).map_err(|e| e.to_string())?;
    algorithm
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| e.to_string())
}

fn verify_password(stored: &str, password: &str) -> bool {
    let Ok(hash) = PasswordHash::new(stored) else {
        return false;
    };
    let production =
        hash.params.get_decimal("m") == Some(19_456) && hash.params.get_decimal("t") == Some(2);
    let test = cfg!(test)
        && hash.params.get_decimal("m") == Some(32)
        && hash.params.get_decimal("t") == Some(1);
    if hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(19)
        || hash.params.get_decimal("p") != Some(1)
        || !(production || test)
    {
        return false;
    }
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

// Public dummy record with the same work factors as a real password.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

// ---------------------------------------------------------------------------
// Sessions, CSRF, Origin

/// A valid session: the cookie token and its stored hash.
#[derive(Clone)]
pub(crate) struct Session {
    token: String,
    pub hash: String,
    pub account: crate::db::accounts::Account,
}

impl Session {
    pub fn csrf(&self) -> String {
        csrf_for(&self.token)
    }
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, data).as_ref())
}

/// The session token from the `Cookie` header, if one is well-formed.
fn cookie_token(headers: &HeaderMap) -> Option<String> {
    for v in headers.get_all(COOKIE) {
        let Ok(v) = v.to_str() else { continue };
        for part in v.split(';') {
            if let Some(t) = part.trim().strip_prefix("rh_session=") {
                if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Some(t.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

/// The request's session if it is valid.
fn session_of(db: &Db, headers: &HeaderMap, now: i64) -> Option<Session> {
    let token = cookie_token(headers)?;
    let hash = sha256_hex(token.as_bytes());
    let account = db.account_session(&hash, now)?;
    Some(Session {
        token,
        hash,
        account,
    })
}

pub(crate) fn csrf_for(token: &str) -> String {
    let mut m = Vec::with_capacity(5 + token.len());
    m.extend_from_slice(b"csrf:");
    m.extend_from_slice(token.as_bytes());
    sha256_hex(&m)
}

/// Constant-time byte comparison (the length is not secret: always 64).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let d = a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y));
    std::hint::black_box(d) == 0
}

pub(crate) fn refresh_cookie(state: &AppState, session: &Session, response: &mut Response<Body>) {
    if response.headers().contains_key(SET_COOKIE) {
        return;
    }
    let now = now_s();
    if let Some(expires) = state
        .db
        .slide_session(&session.hash, now, now + SESSION_TTL_SECS)
    {
        set(
            response,
            SET_COOKIE,
            &session_cookie(&session.token, expires - now),
        );
    }
}

fn session_cookie(token: &str, max_age: i64) -> String {
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}")
}

/// `host[:port]` of an `http(s)://` Origin or Referer value.
fn url_authority(s: &str) -> Option<&str> {
    let rest = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let a = &rest[..end];
    (!a.is_empty() && !a.contains('@')).then_some(a)
}

/// `Origin` (or `Referer` when there is no `Origin`) must name exactly the
/// request's `Host` (host and port). Neither header → refused.
pub(crate) fn same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let src = headers.get(ORIGIN).or_else(|| headers.get(REFERER));
    let Some(src) = src.and_then(|v| v.to_str().ok()) else {
        return false;
    };
    url_authority(src).is_some_and(|a| a.eq_ignore_ascii_case(host))
}

// ---------------------------------------------------------------------------
// Login rate limit

#[derive(Debug, Default, Clone, Copy)]
struct Slot {
    fails: u32,
    until_ms: u64,
    last_ms: u64,
}

/// Back-off after failed logins per client IP and across all clients.
#[derive(Debug, Default)]
pub struct RateLimiter {
    ips: HashMap<IpAddr, Slot>,
    global: Slot,
}

/// Password changes have independent back-off per account.
#[derive(Debug, Default)]
pub struct PasswordLimiter {
    accounts: HashMap<i64, Slot>,
}

impl PasswordLimiter {
    fn check(&mut self, account: i64, now_ms: u64) -> Result<(), u64> {
        self.accounts
            .retain(|_, slot| now_ms.saturating_sub(slot.last_ms) < FORGET_AFTER_MS);
        check_backoff(
            self.accounts.get(&account).map_or(0, |slot| slot.until_ms),
            now_ms,
        )
    }

    fn failure(&mut self, account: i64, now_ms: u64) {
        self.accounts.entry(account).or_default().fail(now_ms, 0);
    }

    fn success(&mut self, account: i64) {
        self.accounts.remove(&account);
    }
}

fn check_backoff(until: u64, now_ms: u64) -> Result<(), u64> {
    if now_ms < until {
        Err((until - now_ms).div_ceil(1000))
    } else {
        Ok(())
    }
}

impl Slot {
    fn fail(&mut self, now_ms: u64, free: u32) {
        self.fails = self.fails.saturating_add(1);
        self.until_ms = now_ms + delay_ms(self.fails, free);
        self.last_ms = now_ms;
    }
}

const GLOBAL_FREE: u32 = 5;
const FORGET_AFTER_MS: u64 = 15 * 60 * 1000;
const MAX_TRACKED_IPS: usize = 256;
const MAX_DELAY_MS: u64 = 60_000;

/// Delay after `fails` failures, the first `free` of which cost nothing.
fn delay_ms(fails: u32, free: u32) -> u64 {
    if fails <= free {
        return 0;
    }
    (1000u64 << (fails - free - 1).min(6)).min(MAX_DELAY_MS)
}

impl RateLimiter {
    /// `Err(seconds to wait)` while this client must wait.
    pub fn check(&mut self, ip: IpAddr, now_ms: u64) -> Result<(), u64> {
        self.forget_old(now_ms);
        let until = self
            .ips
            .get(&ip)
            .map_or(0, |s| s.until_ms)
            .max(self.global.until_ms);
        check_backoff(until, now_ms)
    }

    pub fn failure(&mut self, ip: IpAddr, now_ms: u64) {
        self.forget_old(now_ms);
        if !self.ips.contains_key(&ip) && self.ips.len() >= MAX_TRACKED_IPS {
            // Drop the client that failed longest ago rather than grow.
            if let Some(old) = self
                .ips
                .iter()
                .min_by_key(|(_, s)| s.last_ms)
                .map(|(k, _)| *k)
            {
                self.ips.remove(&old);
            }
        }
        self.ips.entry(ip).or_default().fail(now_ms, 0);
        self.global.fail(now_ms, GLOBAL_FREE);
    }

    pub fn success(&mut self, ip: IpAddr) {
        self.ips.remove(&ip);
    }

    fn forget_old(&mut self, now_ms: u64) {
        self.ips
            .retain(|_, s| now_ms.saturating_sub(s.last_ms) < FORGET_AFTER_MS);
        if now_ms.saturating_sub(self.global.last_ms) >= FORGET_AFTER_MS {
            self.global = Slot::default();
        }
    }
}

// ---------------------------------------------------------------------------
// Routes

fn json_error(status: StatusCode, msg: &str) -> Response<Body> {
    json_response(status, &serde_json::json!({ "error": msg }))
}

fn json_response(status: StatusCode, v: &serde_json::Value) -> Response<Body> {
    let mut r = full(status, serde_json::to_vec(v).unwrap_or_default());
    set(&mut r, CONTENT_TYPE, "application/json");
    set(&mut r, CACHE_CONTROL, "no-store");
    r
}

#[derive(serde::Deserialize)]
struct Credentials {
    username: String,
    password: String,
    token: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialAction {
    Login,
    Onboard,
    Accept,
}

#[derive(Debug)]
enum Attempt {
    OnboardingLocked,
    Limited(u64),
    Wrong,
    Conflict,
    InvalidInvite,
    InvalidCredentials(String),
    Failed,
}

/// The client address for the rate limit (IPv4-mapped IPv6 folded to IPv4).
fn peer_ip<B>(req: &Request<B>) -> IpAddr {
    req.extensions()
        .get::<Peer>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |p| {
            p.0.ip().to_canonical()
        })
}

pub(crate) async fn credential_route(
    state: &Arc<AppState>,
    req: Request<hyper::body::Incoming>,
    action: CredentialAction,
) -> Response<Body> {
    let legacy = action == CredentialAction::Login;
    let error = |status, code, message: &str| {
        if legacy {
            json_error(status, message)
        } else {
            ApiError::new(status, code, message).response()
        }
    };
    let limited = |secs: u64| {
        let mut response = if legacy {
            json_response(
                StatusCode::TOO_MANY_REQUESTS,
                &serde_json::json!({"error": "too many failed attempts", "retry_after": secs}),
            )
        } else {
            error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many attempts",
            )
        };
        set(&mut response, RETRY_AFTER, &secs.to_string());
        response
    };
    let ctype = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !ctype.eq_ignore_ascii_case("application/json") {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "send application/json",
        );
    }
    let ip = peer_ip(&req);
    if let Err(secs) = state
        .login_limiter
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .check(ip, now_ms())
    {
        return limited(secs);
    }
    let Ok(body) = Limited::new(req.into_body(), MAX_BODY).collect().await else {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            "request too large",
        );
    };
    let Ok(credentials) = serde_json::from_slice(&body.to_bytes()) else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "username and password required",
        );
    };
    let st = state.clone();
    let outcome = tokio::task::spawn_blocking(move || authenticate(&st, ip, action, credentials))
        .await
        .unwrap_or(Err(Attempt::Failed));
    let token = match outcome {
        Ok(token) => token,
        Err(Attempt::Limited(secs)) => return limited(secs),
        Err(Attempt::Wrong) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "invalid username or password",
            )
        }
        Err(Attempt::OnboardingLocked) => {
            return error(
                StatusCode::FORBIDDEN,
                "onboarding_locked",
                "Run hobnail set-password on the host to create the owner account.",
            );
        }
        Err(Attempt::Conflict) => {
            let message = if action == CredentialAction::Onboard {
                "owner already exists"
            } else {
                "username already in use"
            };
            return error(StatusCode::CONFLICT, "conflict", message);
        }
        Err(Attempt::InvalidInvite) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_invite",
                "invite is invalid or expired",
            )
        }
        Err(Attempt::InvalidCredentials(message)) => {
            return error(StatusCode::BAD_REQUEST, "bad_request", &message)
        }
        Err(Attempt::Failed) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not sign in",
            )
        }
    };
    let mut response = json_response(
        if legacy {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        },
        &serde_json::json!({"ok": true, "csrf": csrf_for(&token)}),
    );
    set(
        &mut response,
        SET_COOKIE,
        &session_cookie(&token, SESSION_TTL_SECS),
    );
    response
}

fn authenticate(
    state: &AppState,
    ip: IpAddr,
    action: CredentialAction,
    credentials: Credentials,
) -> Result<String, Attempt> {
    // Onboarding queues behind a raced onboarding request so the loser
    // sees the committed owner and receives 409. Other operations can retry.
    let _permit = if action == CredentialAction::Onboard {
        state
            .login_verifying
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    } else {
        state
            .login_verifying
            .try_lock()
            .map_err(|_| Attempt::Limited(1))?
    };
    let lock = || {
        state
            .login_limiter
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    };
    let now = now_s();
    let db = &state.db;
    if action == CredentialAction::Onboard && db.owner_exists() {
        return Err(Attempt::Conflict);
    }
    if action == CredentialAction::Onboard && db.onboarding_locked() {
        return Err(Attempt::OnboardingLocked);
    }
    lock().check(ip, now_ms()).map_err(Attempt::Limited)?;
    let Credentials {
        username,
        password,
        token: invite,
    } = credentials;
    let token = random_token().map_err(|_| Attempt::Failed)?;
    let hash = sha256_hex(token.as_bytes());
    let expires = now + SESSION_TTL_SECS;
    let result = if action == CredentialAction::Login {
        let credential = db.credential(&username);
        let stored = credential.as_ref().map_or(DUMMY_HASH, |(_, hash, _)| hash);
        let verified = password.len() <= MAX_PASSWORD_BYTES && verify_password(stored, &password);
        match credential {
            Some((id, stored, false)) if verified => db.sign_in(id, &stored, &hash, now, expires),
            _ => Err(AccountError::Invalid),
        }
    } else {
        if !valid_name(&username) {
            return Err(Attempt::InvalidCredentials(
                "username must be 1–64 letters, digits, dots, hyphens or underscores".into(),
            ));
        }
        check_password_policy(&password).map_err(Attempt::InvalidCredentials)?;
        let invite_hash = invite.map(|t| sha256_hex(t.as_bytes()));
        if action == CredentialAction::Accept
            && invite_hash
                .as_deref()
                .and_then(|hash| db.validate_invite(hash, now))
                .is_none()
        {
            lock().failure(ip, now_ms());
            return Err(Attempt::InvalidInvite);
        }
        let stored = hash_password(&password).map_err(|_| Attempt::Failed)?;
        match invite_hash.filter(|_| action == CredentialAction::Accept) {
            Some(invite) => {
                let now = now_s();
                db.accept_invite(
                    &invite,
                    &username,
                    &stored,
                    &hash,
                    now,
                    now + SESSION_TTL_SECS,
                )
            }
            None => db.create_owner(&username, &stored, &hash, now, expires),
        }
    };
    match result {
        Ok(()) => {
            lock().success(ip);
            db.record_event(now_s(), "auth.login", "info", &username, "");
            Ok(token)
        }
        Err(AccountError::OnboardingLocked) => Err(Attempt::OnboardingLocked),
        Err(AccountError::Conflict) => Err(Attempt::Conflict),
        Err(AccountError::Invalid) => {
            lock().failure(ip, now_ms());
            if action == CredentialAction::Login {
                db.record_event(
                    now_s(),
                    "auth.login_failed",
                    "warn",
                    "",
                    &format!("from {ip}"),
                );
                Err(Attempt::Wrong)
            } else {
                Err(Attempt::InvalidInvite)
            }
        }
        Err(_) => Err(Attempt::Failed),
    }
}

fn logout(state: &AppState, session: &Session) -> Response<Body> {
    state.db.delete_session(&session.hash);
    let mut r = json_response(StatusCode::OK, &serde_json::json!({ "ok": true }));
    set(
        &mut r,
        SET_COOKIE,
        &format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    r
}

mod tty {
    use super::OnceLock;
    use std::io::Write;

    /// The terminal settings before echo was turned off, for the signal
    /// handler (set once, before echo is changed).
    static SAVED: OnceLock<libc::termios> = OnceLock::new();

    extern "C" fn restore_and_exit(sig: libc::c_int) {
        // Only async-signal-safe calls here.
        if let Some(t) = SAVED.get() {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, t);
                libc::write(2, b"\n".as_ptr().cast(), 1);
            }
        }
        unsafe { libc::_exit(128 + sig) };
    }

    /// Restores the terminal when dropped (every return path).
    struct Restore(libc::termios);

    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &self.0);
            }
        }
    }

    /// Prompt on stderr and read one line from the terminal with echo off.
    /// Ctrl-C (or a hang-up) restores the terminal before the process exits.
    pub(super) fn read_secret(prompt: &str) -> Result<String, String> {
        let mut old: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(0, &mut old) } != 0 {
            return Err("cannot read the terminal settings".into());
        }
        let _ = SAVED.set(old);
        let handler = restore_and_exit as extern "C" fn(libc::c_int) as libc::sighandler_t;
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            unsafe { libc::signal(sig, handler) };
        }
        let mut quiet = old;
        quiet.c_lflag &= !libc::ECHO;
        quiet.c_lflag |= libc::ECHONL; // still show the Enter
        if unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &quiet) } != 0 {
            return Err("cannot turn off echo".into());
        }
        let _restore = Restore(old);
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => Err("no input".into()),
            Ok(_) => Ok(line.trim_end_matches(['\n', '\r']).to_string()),
            Err(_) => Err("could not read the terminal".into()),
        }
    }
}

// ---------------------------------------------------------------------------

/// Tests elsewhere in the crate (e.g. the HTTP write-gate tests): set a
/// password with few iterations so login stays fast.
#[cfg(test)]
pub(crate) fn set_password_for_tests(db: &Db, password: &str) {
    let hash = hash_with(&argon2(32, 1), password).unwrap();
    db.recover_owner(None, &hash, 0).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2id_hashes_are_salted_and_verifiable() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password(&hash, "correct horse"));
        assert!(!verify_password(&hash, "wrong horse"));
        assert!(!verify_password("invalid", "correct horse"));
        assert_ne!(hash, hash_password("correct horse").unwrap());
    }

    #[test]
    fn login_rejects_unknown_accounts_and_throttles_failures() {
        let state = crate::ui::testutil::state_with(vec![], "");
        set_password_for_tests(&state.db, "correct horse");
        let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let credentials = |username: &str| Credentials {
            username: username.into(),
            password: "correct horse".into(),
            token: None,
        };
        assert!(matches!(
            authenticate(
                &state,
                local,
                CredentialAction::Login,
                credentials("unknown")
            ),
            Err(Attempt::Wrong)
        ));
        assert!(matches!(
            authenticate(&state, local, CredentialAction::Login, credentials("admin")),
            Err(Attempt::Limited(_))
        ));
        let other = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
        let token =
            authenticate(&state, other, CredentialAction::Login, credentials("ADMIN")).unwrap();
        let hash = sha256_hex(token.as_bytes());
        assert_eq!(
            state.db.account_session(&hash, now_s()).unwrap().name,
            "admin"
        );
        assert!(state.db.account_session(&token, now_s()).is_none());
    }

    #[test]
    fn rate_limit_backoff_sequence_and_cap() {
        let mut l = RateLimiter::default();
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut t = 1_000_000u64;
        let mut waits = Vec::new();
        for _ in 0..9 {
            assert!(l.check(ip, t).is_ok());
            l.failure(ip, t);
            let w = l.check(ip, t).unwrap_err();
            waits.push(w);
            // One ms before the end it still waits; at the end it is free.
            assert!(l.check(ip, t + w * 1000 - 1).is_err());
            t += w * 1000;
        }
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        // A success clears it.
        l.success(ip);
        assert!(l.check(ip, t).is_ok());
        // 15 minutes without failures forget the counter.
        t += FORGET_AFTER_MS;
        l.failure(ip, t);
        l.failure(ip, t + 1000);
        assert_eq!(l.check(ip, t + 1000), Err(2));
        let later = t + FORGET_AFTER_MS + 2000;
        l.failure(ip, later);
        assert_eq!(l.check(ip, later), Err(1), "counter restarted");
    }

    #[test]
    fn rate_limit_global() {
        let mut l = RateLimiter::default();
        let t = 5_000_000u64;
        // Five different clients fail once each: no global back-off yet.
        for n in 1..=GLOBAL_FREE {
            l.failure(IpAddr::V4(Ipv4Addr::new(10, 1, 0, n as u8)), t);
        }
        let fresh = IpAddr::V4(Ipv4Addr::new(10, 2, 0, 1));
        assert!(l.check(fresh, t).is_ok());
        // The next failure anywhere starts the global back-off.
        l.failure(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 99)), t);
        assert_eq!(l.check(fresh, t), Err(1));
        for _ in 0..20 {
            l.failure(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 99)), t);
        }
        assert_eq!(l.check(fresh, t), Err(60), "capped at 60 s");
        let owner = IpAddr::V4(Ipv4Addr::new(10, 3, 0, 1));
        l.success(owner);
        assert_eq!(l.check(fresh, t), Err(60));
        assert_eq!(l.check(owner, t), Err(60));
        // Bounded memory under many clients.
        for n in 0..1000u32 {
            l.failure(IpAddr::V4(Ipv4Addr::from(0x0b00_0000 + n)), t);
        }
        assert!(l.ips.len() <= MAX_TRACKED_IPS);
    }

    #[test]
    fn password_changes_back_off_independently_per_account() {
        let mut passwords = PasswordLimiter::default();
        let t = 1_000_000;
        passwords.failure(1, t);
        assert_eq!(passwords.check(1, t), Err(1));
        assert_eq!(passwords.check(2, t), Ok(()));
        passwords.success(2);
        assert_eq!(passwords.check(1, t), Err(1));
        passwords.success(1);
        assert_eq!(passwords.check(1, t), Ok(()));
    }

    #[test]
    fn csrf_is_derived_from_the_session() {
        let t = "0f".repeat(32);
        assert_eq!(csrf_for(&t), csrf_for(&t));
        assert_eq!(csrf_for(&t).len(), 64);
        assert_ne!(csrf_for(&t), csrf_for(&"0e".repeat(32)));
        assert_ne!(
            csrf_for(&t),
            sha256_hex(t.as_bytes()),
            "not the stored hash"
        );
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn cookie_format() {
        let c = session_cookie(&"aa".repeat(32), SESSION_TTL_SECS);
        assert!(c.starts_with("rh_session="));
        for part in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=604800"] {
            assert!(c.contains(part), "{c}");
        }
    }

    #[test]
    fn origin_parsing() {
        assert_eq!(url_authority("http://a:1"), Some("a:1"));
        assert_eq!(url_authority("https://a:1/x?y"), Some("a:1"));
        assert_eq!(url_authority("http://[::1]:3200/"), Some("[::1]:3200"));
        assert_eq!(url_authority("ftp://a"), None);
        assert_eq!(url_authority("http://"), None);
        assert_eq!(url_authority("http://u@a"), None);
    }
}
