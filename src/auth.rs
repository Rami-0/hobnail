//! Login and sessions. See docs/DESIGN.md section 6.
//!
//! - Viewing is open. Every write goes through [`require_write`], which the
//!   central router calls for every non-GET request except `POST /api/login`.
//! - One password (user `admin`), set from the console with
//!   `hobnail set-password`; there is no web "claim" form. Stored in the
//!   `kv` table as `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>`
//!   (PBKDF2-HMAC-SHA256 via `ring`, 600 000 iterations, 16-byte random salt,
//!   constant-time verify).
//! - A session is 32 random bytes in the `rh_session` cookie (hex); only its
//!   SHA-256 is stored. 7 days, sliding server-side, revocable.
//! - CSRF: `SameSite=Strict`, plus an `X-RH-CSRF` header that must equal a
//!   token derived from the session (hex SHA-256 of `"csrf:" + token`, so no
//!   extra storage), plus `Origin` (or `Referer`) must name this `Host`.
//! - Failed logins back off exponentially (1 s, 2 s, 4 s ... 60 s) per client
//!   IP and globally; while backing off the answer is 429 and the password is
//!   not even checked.
//!
//! Nothing in this file logs, prints or stores a password, token or cookie.
//! Events record the client IP for failed logins only.

#![allow(dead_code)] // parts of the API are used by the UI and settings work

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use http_body_util::{BodyExt, Limited};
use hyper::header::{
    HeaderValue, CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, REFERER, RETRY_AFTER,
    SET_COOKIE,
};
use hyper::{HeaderMap, Method, Request, Response, StatusCode};
use ring::{digest, pbkdf2, rand::SecureRandom};

use crate::db::{Db, EventFilter};
use crate::http::server::{full, set, Body, Peer};
use crate::state::AppState;

/// PBKDF2 iterations for new hashes (OWASP 2023 figure for HMAC-SHA256).
pub const ITERATIONS: u32 = 600_000;
/// Stored hashes claiming more iterations are rejected (bounds the CPU a
/// corrupt row could cost).
const MAX_ITERATIONS: u32 = 10_000_000;
pub const COOKIE_NAME: &str = "rh_session";
pub const CSRF_HEADER: &str = "x-rh-csrf";
/// Sessions last this long after their last use.
pub const SESSION_TTL_SECS: i64 = 7 * 86_400;
const KV_PASSWORD: &str = "auth.password";
pub const USER: &str = "admin";
pub const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_BYTES: usize = 1024;
/// Login request bodies larger than this are refused.
const MAX_BODY: usize = 4096;
const MAX_NEXT: usize = 2048;

fn now_s() -> i64 {
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

/// The signed-in user's name (always `"admin"` for now), or `None` when the
/// request has no valid session. A valid session's expiry slides (written
/// at most once a minute).
pub fn current_user(state: &AppState, headers: &HeaderMap) -> Option<String> {
    session_of(&state.db, headers, now_s()).map(|_| USER.to_string())
}

/// True when a password has been set (`set-password` was run). Until then the
/// app is read-only and the settings pages explain how to set one.
pub fn password_is_set(state: &AppState) -> bool {
    password_set(&state.db)
}

/// The CSRF token for the current session (sent back by the page's JS in the
/// `X-RH-CSRF` header on every write). `None` when not signed in.
pub fn csrf_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    session_of(&state.db, headers, now_s()).map(|s| csrf_for(&s.token))
}

/// A `Set-Cookie` value that renews the browser cookie's `Max-Age` for the
/// current valid session. The cookie itself expires 7 days after it was
/// set, so a page that wants the sliding expiry to reach the browser can
/// attach this to its response. `None` when not signed in.
pub fn refresh_cookie(state: &AppState, headers: &HeaderMap) -> Option<String> {
    session_of(&state.db, headers, now_s()).map(|s| session_cookie(&s.token))
}

/// Why a write was refused. `response()` gives the HTTP answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDenied {
    /// No password set yet (`set-password` not run): nothing is writable.
    NoPassword,
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
            Self::NoPassword => text(
                StatusCode::FORBIDDEN,
                "read-only: no password set (run `hobnail set-password`)\n",
            ),
            Self::NoSession => text(StatusCode::UNAUTHORIZED, "sign in required\n"),
            Self::BadCsrf => text(StatusCode::FORBIDDEN, "missing or bad X-RH-CSRF header\n"),
            Self::BadOrigin => text(StatusCode::FORBIDDEN, "cross-origin write refused\n"),
        }
    }
}

/// The single write guard. Called by `http::server::route` for **every**
/// non-GET request except `POST /api/login`. Returns the user name.
///
/// Checks, in this order: a password is set, the session cookie is valid,
/// `X-RH-CSRF` matches the session's token, `Origin` (or `Referer` when
/// there is no `Origin`) names the request's `Host`.
pub fn require_write(state: &AppState, headers: &HeaderMap) -> Result<String, WriteDenied> {
    check_write(&state.db, headers, now_s())
}

pub(crate) fn check_write(db: &Db, headers: &HeaderMap, now: i64) -> Result<String, WriteDenied> {
    if !password_set(db) {
        return Err(WriteDenied::NoPassword);
    }
    let s = session_of(db, headers, now).ok_or(WriteDenied::NoSession)?;
    let got = headers
        .get(CSRF_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or(b"");
    if !ct_eq(got, csrf_for(&s.token).as_bytes()) {
        return Err(WriteDenied::BadCsrf);
    }
    if !same_origin(headers) {
        return Err(WriteDenied::BadOrigin);
    }
    Ok(USER.to_string())
}

/// `/api/login` (POST: form or JSON `{password}`; checks Origin, rate limit)
/// and `/api/logout` (POST, already passed `require_write`).
/// The peer address is in `req.extensions().get::<http::server::Peer>()`.
pub async fn route(state: &Arc<AppState>, req: Request<hyper::body::Incoming>) -> Response<Body> {
    if req.method() != Method::POST {
        let mut r = json_error(StatusCode::METHOD_NOT_ALLOWED, "POST only");
        r.headers_mut()
            .insert(hyper::header::ALLOW, HeaderValue::from_static("POST"));
        return r;
    }
    if req.uri().path() == "/api/logout" {
        return logout(state, req.headers());
    }
    login_route(state, req).await
}

/// `hobnail set-password`: reads the new password from the TTY without
/// echo (twice), or one line from stdin when piped; stores the hash in the
/// database and signs every session out.
pub fn cli_set_password() -> i32 {
    let path = crate::db::path_from_env();
    let db = match Db::open(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("set-password: cannot open the database: {e}");
            return 1;
        }
    };
    let had = password_set(&db);
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
    match set_password(&db, &password, now_s(), "set from the console") {
        Ok(()) => {
            println!(
                "Password {} in {}. All sessions were signed out.",
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

/// Server-side check of a post-login redirect target. Returns the path when it
/// is a same-origin relative path, else `None` (caller falls back to `/`).
/// Rules (security review): reject every control character (incl. TAB, which
/// browsers strip, turning `/\t/evil` into `//evil`) and space; require a
/// leading `/`; no leading `//`; no backslash anywhere; percent-decoded form
/// must pass the same checks.
pub fn safe_next(next: &str) -> Option<&str> {
    // The raw form also has to be plain ASCII so it is a valid Location
    // header that every browser reads the same way.
    if next.len() > MAX_NEXT || !next.is_ascii() || !local_path_ok(next) {
        return None;
    }
    let decoded = crate::db::api::percent_decode(next)?;
    local_path_ok(&decoded).then_some(next)
}

fn local_path_ok(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || c == ' ')
        && s.starts_with('/')
        && !s.starts_with("//")
        && !s.contains('\\')
}

/// Change the password (Security tab). Requires the current password.
/// Blocking (two PBKDF2 runs, about a second): call it from
/// `spawn_blocking`. Wrong current passwords go through the same back-off
/// as logins. On success **every** session is signed out, including the
/// caller's (sign in again with the new password).
pub fn change_password(state: &AppState, current: &str, new: &str) -> Result<(), String> {
    change_password_db(
        &state.db,
        &state.login_limiter,
        current,
        new,
        now_s(),
        &now_ms,
    )
}

/// Sign out every other session (keeps the caller's).
pub fn sign_out_others(state: &AppState, headers: &HeaderMap) {
    if let Some(s) = session_of(&state.db, headers, now_s()) {
        state.db.delete_sessions_except(Some(&s.hash));
    }
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

fn password_set(db: &Db) -> bool {
    db.kv_get(KV_PASSWORD).is_some()
}

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

/// Store a new password hash and sign every session out.
fn set_password(db: &Db, password: &str, now: i64, how: &str) -> Result<(), String> {
    set_password_with(db, password, now, how, ITERATIONS)
}

fn set_password_with(
    db: &Db,
    password: &str,
    now: i64,
    how: &str,
    iterations: u32,
) -> Result<(), String> {
    check_password_policy(password)?;
    if !db.is_enabled() {
        return Err("the database is not available".into());
    }
    let stored = hash_with(password, iterations)?;
    // One transaction: never the new password with the old sessions.
    if !db.replace_password(KV_PASSWORD, &stored) {
        return Err("could not write to the database".into());
    }
    db.record_event(now, "auth.password_changed", "info", USER, how);
    Ok(())
}

fn change_password_db(
    db: &Db,
    limiter: &Mutex<RateLimiter>,
    current: &str,
    new: &str,
    now: i64,
    clock: &dyn Fn() -> u64,
) -> Result<(), String> {
    check_password_policy(new)?;
    let stored = db.kv_get(KV_PASSWORD).ok_or("no password is set")?;
    // Same back-off as logins, under a key of its own.
    let key = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let lock = || limiter.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(secs) = lock().check(key, clock()) {
        return Err(format!("too many attempts; try again in {secs} s"));
    }
    if current.len() > MAX_PASSWORD_BYTES || !verify_password(&stored, current) {
        lock().failure(key, clock());
        db.record_event(
            now,
            "auth.login_failed",
            "warn",
            USER,
            "wrong current password when changing it",
        );
        return Err("the current password is wrong".into());
    }
    lock().success(key);
    set_password(db, new, now, "changed in the app")
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

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some((nib(p[0])? << 4) | nib(p[1])?))
        .collect()
}

/// `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>` with a fresh 16-byte
/// salt and `ITERATIONS`.
pub fn hash_password(password: &str) -> Result<String, String> {
    hash_with(password, ITERATIONS)
}

fn hash_with(password: &str, iterations: u32) -> Result<String, String> {
    let salt: [u8; 16] = random_bytes()?;
    let mut out = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).ok_or("zero iterations")?,
        &salt,
        password.as_bytes(),
        &mut out,
    );
    Ok(format!(
        "pbkdf2-sha256${iterations}${}${}",
        hex(&salt),
        hex(&out)
    ))
}

/// Constant-time check (`ring::pbkdf2::verify`). False for anything that is
/// not a well-formed record.
pub fn verify_password(stored: &str, password: &str) -> bool {
    let mut it = stored.split('$');
    let (Some("pbkdf2-sha256"), Some(iters), Some(salt), Some(hash), None) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    else {
        return false;
    };
    let Some(iters) = iters
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=MAX_ITERATIONS).contains(n))
        .and_then(NonZeroU32::new)
    else {
        return false;
    };
    let (Some(salt), Some(hash)) = (unhex(salt), unhex(hash)) else {
        return false;
    };
    if salt.is_empty() || hash.len() != 32 {
        return false;
    }
    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iters,
        &salt,
        password.as_bytes(),
        &hash,
    )
    .is_ok()
}

// ---------------------------------------------------------------------------
// Sessions, CSRF, Origin

/// A valid session: the cookie token and its stored hash.
struct Session {
    token: String,
    hash: String,
}

fn sha256_hex(data: &[u8]) -> String {
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

/// The request's session if it is valid; slides its expiry.
fn session_of(db: &Db, headers: &HeaderMap, now: i64) -> Option<Session> {
    let token = cookie_token(headers)?;
    let hash = sha256_hex(token.as_bytes());
    db.touch_session(&hash, now, now + SESSION_TTL_SECS)
        .then_some(Session { token, hash })
}

fn csrf_for(token: &str) -> String {
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

fn session_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_TTL_SECS}")
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
fn same_origin(headers: &HeaderMap) -> bool {
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

/// Back-off after failed logins: 1 s, 2 s, 4 s ... capped at 60 s, per
/// client IP. A global slot backs it up (many IPs cannot dodge the per-IP
/// limit); it only starts after `GLOBAL_FREE` failures, so one stranger's
/// bad guess does not lock the owner out, and it does not apply to clients
/// that have signed in before (since start). A success clears the IP and the
/// global slot; 15 minutes without failures forget them.
#[derive(Debug, Default)]
pub struct RateLimiter {
    ips: HashMap<IpAddr, Slot>,
    global: Slot,
    /// Clients that signed in successfully (since start), newest last. They
    /// skip the global slot (not their own), so a stranger who keeps the
    /// global back-off at 60 s cannot lock the owner out.
    trusted: Vec<IpAddr>,
}

const GLOBAL_FREE: u32 = 5;
const FORGET_AFTER_MS: u64 = 15 * 60 * 1000;
const MAX_TRACKED_IPS: usize = 256;
const MAX_DELAY_MS: u64 = 60_000;
const MAX_TRUSTED: usize = 16;

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
        let global = if self.trusted.contains(&ip) {
            0
        } else {
            self.global.until_ms
        };
        let until = self.ips.get(&ip).map_or(0, |s| s.until_ms).max(global);
        if now_ms < until {
            Err((until - now_ms).div_ceil(1000))
        } else {
            Ok(())
        }
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
        let s = self.ips.entry(ip).or_default();
        s.fails += 1;
        s.until_ms = now_ms + delay_ms(s.fails, 0);
        s.last_ms = now_ms;
        let g = &mut self.global;
        g.fails += 1;
        g.until_ms = now_ms + delay_ms(g.fails, GLOBAL_FREE);
        g.last_ms = now_ms;
    }

    pub fn success(&mut self, ip: IpAddr) {
        self.ips.remove(&ip);
        self.global = Slot::default();
        self.trusted.retain(|t| *t != ip);
        if self.trusted.len() >= MAX_TRUSTED {
            self.trusted.remove(0);
        }
        self.trusted.push(ip);
    }

    fn forget_old(&mut self, now_ms: u64) {
        self.ips
            .retain(|_, s| now_ms.saturating_sub(s.last_ms) < FORGET_AFTER_MS);
        if now_ms.saturating_sub(self.global.last_ms) >= FORGET_AFTER_MS {
            self.global = Slot::default();
        }
    }
}

/// One password check at a time: each costs a few hundred ms of CPU.
static VERIFYING: AtomicBool = AtomicBool::new(false);

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

fn too_many(secs: u64) -> Response<Body> {
    let mut r = json_response(
        StatusCode::TOO_MANY_REQUESTS,
        &serde_json::json!({ "error": "too many failed attempts", "retry_after": secs }),
    );
    set(&mut r, RETRY_AFTER, &secs.to_string());
    r
}

#[derive(serde::Deserialize)]
struct JsonLogin {
    password: String,
}

/// The client address for the rate limit (IPv4-mapped IPv6 folded to IPv4).
fn peer_ip<B>(req: &Request<B>) -> IpAddr {
    req.extensions()
        .get::<Peer>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |p| {
            p.0.ip().to_canonical()
        })
}

async fn login_route(state: &Arc<AppState>, req: Request<hyper::body::Incoming>) -> Response<Body> {
    if !same_origin(req.headers()) {
        return json_error(StatusCode::FORBIDDEN, "cross-origin login refused");
    }
    let ctype = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let form = match ctype.as_str() {
        "application/json" => false,
        "application/x-www-form-urlencoded" => true,
        _ => {
            return json_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "send application/json or a form",
            )
        }
    };
    let ip = peer_ip(&req);
    // Backing off: answer before reading or checking anything.
    if let Err(secs) = state
        .login_limiter
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .check(ip, now_ms())
    {
        return too_many(secs);
    }
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return json_error(StatusCode::PAYLOAD_TOO_LARGE, "request too large"),
    };
    let (password, next) = if form {
        let Some(pairs) = std::str::from_utf8(&body)
            .ok()
            .and_then(crate::db::api::parse_query)
        else {
            return json_error(StatusCode::BAD_REQUEST, "malformed form");
        };
        let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        (get("password"), get("next"))
    } else {
        match serde_json::from_slice::<JsonLogin>(&body) {
            Ok(j) => (Some(j.password), None),
            Err(_) => (None, None),
        }
    };
    let Some(password) = password.filter(|p| !p.is_empty()) else {
        return json_error(StatusCode::BAD_REQUEST, "password missing");
    };
    let st = state.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        // One password check at a time; a concurrent attempt is told to retry.
        if VERIFYING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Login::Limited(1);
        }
        // Cleared on every path, even if the check panicked.
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                VERIFYING.store(false, Ordering::Release);
            }
        }
        let _done = Done;
        login(&st.db, &st.login_limiter, ip, now_s(), &now_ms, &password)
    })
    .await
    .unwrap_or(Login::Failed);
    match outcome {
        Login::NoPassword => json_error(
            StatusCode::FORBIDDEN,
            "no password set (run `hobnail set-password`)",
        ),
        Login::Limited(secs) => too_many(secs),
        Login::Wrong => json_error(StatusCode::UNAUTHORIZED, "wrong password"),
        Login::Failed => json_error(StatusCode::INTERNAL_SERVER_ERROR, "login failed"),
        Login::Ok { token } => {
            let mut r = if form {
                let to = next.as_deref().and_then(safe_next).unwrap_or("/");
                let mut r = full(StatusCode::SEE_OTHER, "");
                set(&mut r, LOCATION, to);
                set(&mut r, CACHE_CONTROL, "no-store");
                r
            } else {
                json_response(
                    StatusCode::OK,
                    &serde_json::json!({ "ok": true, "csrf": csrf_for(&token) }),
                )
            };
            set(&mut r, SET_COOKIE, &session_cookie(&token));
            r
        }
    }
}

fn logout(state: &AppState, headers: &HeaderMap) -> Response<Body> {
    // The router already ran `require_write` (session + CSRF + Origin).
    if let Some(s) = session_of(&state.db, headers, now_s()) {
        state.db.delete_session(&s.hash);
    }
    let mut r = json_response(StatusCode::OK, &serde_json::json!({ "ok": true }));
    set(
        &mut r,
        SET_COOKIE,
        &format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    r
}

#[derive(Debug, PartialEq, Eq)]
enum Login {
    NoPassword,
    /// Retry after this many seconds.
    Limited(u64),
    Wrong,
    /// Internal failure (database, random numbers).
    Failed,
    Ok {
        token: String,
    },
}

/// The login decision with all its side effects (limiter, events, session).
/// Blocking: one PBKDF2 run. `clock` gives milliseconds; it is read again
/// after the (slow) check, so the back-off starts when the answer is given,
/// not when the attempt arrived.
fn login(
    db: &Db,
    limiter: &Mutex<RateLimiter>,
    ip: IpAddr,
    now: i64,
    clock: &dyn Fn() -> u64,
    password: &str,
) -> Login {
    let Some(stored) = db.kv_get(KV_PASSWORD) else {
        return Login::NoPassword;
    };
    let lock = || limiter.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(secs) = lock().check(ip, clock()) {
        return Login::Limited(secs);
    }
    if password.len() > MAX_PASSWORD_BYTES || !verify_password(&stored, password) {
        lock().failure(ip, clock());
        db.record_event(
            now,
            "auth.login_failed",
            "warn",
            USER,
            &format!("from {ip}"),
        );
        return Login::Wrong;
    }
    lock().success(ip);
    let Ok(raw) = random_bytes::<32>() else {
        return Login::Failed;
    };
    let token = hex(&raw);
    if !db.create_session(&sha256_hex(token.as_bytes()), now, now + SESSION_TTL_SECS) {
        return Login::Failed;
    }
    db.record_event(now, "auth.login", "info", USER, "");
    Login::Ok { token }
}

// ---------------------------------------------------------------------------
// Reading a secret from the terminal

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
    set_password_with(db, password, 0, "test", 1_000).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast hashes for tests; production uses `ITERATIONS`.
    const TEST_ITERS: u32 = 1000;
    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));

    fn db_with_password(pw: &str) -> Db {
        let db = Db::open_in_memory().unwrap();
        set_password_with(&db, pw, 1_000, "test", TEST_ITERS).unwrap();
        db
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    /// Log in and return the session token.
    fn sign_in(db: &Db, now: i64) -> String {
        let lim = Mutex::new(RateLimiter::default());
        match login(db, &lim, IP, now, &|| now as u64 * 1000, "correct horse") {
            Login::Ok { token } => token,
            other => panic!("{other:?}"),
        }
    }

    fn write_headers(token: &str, csrf: &str, origin: Option<&str>) -> HeaderMap {
        let cookie = format!("theme=dark; rh_session={token}");
        let mut v = vec![
            ("host", "10.0.0.1:3200"),
            ("cookie", cookie.as_str()),
            ("x-rh-csrf", csrf),
        ];
        if let Some(o) = origin {
            v.push(("origin", o));
        }
        headers(&v)
    }

    #[test]
    fn hash_format_and_verify() {
        let rec = hash_with("correct horse", TEST_ITERS).unwrap();
        let parts: Vec<_> = rec.split('$').collect();
        assert_eq!(parts[0], "pbkdf2-sha256");
        assert_eq!(parts[1], TEST_ITERS.to_string());
        assert_eq!(parts[2].len(), 32, "16-byte salt in hex");
        assert_eq!(parts[3].len(), 64, "32-byte hash in hex");
        assert!(verify_password(&rec, "correct horse"));
        assert!(!verify_password(&rec, "correct horsf"));
        assert!(!verify_password(&rec, ""));
        // A fresh salt every time.
        assert_ne!(rec, hash_with("correct horse", TEST_ITERS).unwrap());
        // Malformed records never verify.
        for bad in [
            "",
            "plain",
            "pbkdf2-sha256$0$00$00",
            "pbkdf2-sha256$99999999999$00$00",
            "pbkdf2-sha1$1000$00$00",
            &format!("{rec}$extra"),
            &rec.replace("pbkdf2-sha256$1000$", "pbkdf2-sha256$1000$zz"),
        ] {
            assert!(!verify_password(bad, "correct horse"), "{bad}");
        }
    }

    #[test]
    fn production_iterations() {
        const { assert!(ITERATIONS >= 600_000) };
        let rec = hash_password("correct horse").unwrap();
        assert!(rec.starts_with("pbkdf2-sha256$600000$"), "{rec}");
        assert!(verify_password(&rec, "correct horse"));
        assert!(!verify_password(&rec, "wrong horse"));
    }

    #[test]
    fn password_policy() {
        assert!(check_password_policy("1234567").is_err());
        assert!(check_password_policy("12345678").is_ok());
        assert!(check_password_policy("ünïcödé!").is_ok()); // 8 chars
        assert!(check_password_policy(&"x".repeat(2000)).is_err());
        assert!(check_password_policy("tab\there!").is_err());
        let db = Db::open_in_memory().unwrap();
        assert!(set_password_with(&db, "short", 1, "t", TEST_ITERS).is_err());
        assert!(!password_set(&db));
        assert!(set_password_with(&Db::disabled(), "long enough", 1, "t", TEST_ITERS).is_err());
    }

    #[test]
    fn login_wrong_and_right() {
        let db = db_with_password("correct horse");
        let lim = Mutex::new(RateLimiter::default());
        assert_eq!(
            login(&db, &lim, IP, 2000, &|| 2_000_000, "wrong horse"),
            Login::Wrong
        );
        let failed = login_events(&db, 10);
        assert_eq!(failed[0].kind, "auth.login_failed");
        assert_eq!(failed[0].detail, "from 10.0.0.7");
        assert!(!failed[0].detail.contains("horse"));
        // Backing off: refused without checking, even the right password.
        assert_eq!(
            login(&db, &lim, IP, 2000, &|| 2_000_500, "correct horse"),
            Login::Limited(1)
        );
        let Login::Ok { token } = login(&db, &lim, IP, 2001, &|| 2_001_000, "correct horse") else {
            panic!()
        };
        assert_eq!(token.len(), 64);
        let ok = login_events(&db, 10);
        assert_eq!(ok[0].kind, "auth.login");
        assert_eq!(ok[0].detail, "", "no IP recorded for a good login");
        // Only the hash of the token is stored.
        assert!(db.touch_session(&sha256_hex(token.as_bytes()), 2002, 2003));
        assert!(!db.touch_session(&token, 2002, 2003));
        // No password set: nobody can log in.
        let empty = Db::open_in_memory().unwrap();
        assert_eq!(
            login(&empty, &lim, IP, 1, &|| 1, "anything at all"),
            Login::NoPassword
        );
    }

    #[test]
    fn backoff_starts_when_a_slow_check_ends() {
        let db = db_with_password("correct horse");
        let lim = Mutex::new(RateLimiter::default());
        // The attempt arrives at 1.000 s; the hash takes 6 s (debug build).
        let t = std::cell::Cell::new(1_000u64);
        let clock = || {
            let v = t.get();
            t.set(v + 6_000);
            v
        };
        assert_eq!(login(&db, &lim, IP, 1, &clock, "wrong horse"), Login::Wrong);
        // Failure stamped at 7.000 s: still backing off at 7.500 s.
        assert_eq!(
            login(&db, &lim, IP, 7, &|| 7_500, "correct horse"),
            Login::Limited(1)
        );
    }

    #[test]
    fn rate_limit_backoff_sequence_and_cap() {
        let mut l = RateLimiter::default();
        let ip = IP;
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
        // A client that signed in before is not held by the global slot
        // (only by its own), so the owner cannot be locked out this way.
        let owner = IpAddr::V4(Ipv4Addr::new(10, 3, 0, 1));
        l.success(owner);
        for _ in 0..20 {
            l.failure(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 99)), t);
        }
        assert_eq!(l.check(fresh, t), Err(60));
        assert!(l.check(owner, t).is_ok());
        l.failure(owner, t);
        assert_eq!(l.check(owner, t), Err(1), "its own back-off still applies");
        // Bounded memory under many clients.
        for n in 0..1000u32 {
            l.failure(IpAddr::V4(Ipv4Addr::from(0x0b00_0000 + n)), t);
        }
        assert!(l.ips.len() <= MAX_TRACKED_IPS);
        for n in 0..100u32 {
            l.success(IpAddr::V4(Ipv4Addr::from(0x0c00_0000 + n)));
        }
        assert_eq!(l.trusted.len(), MAX_TRUSTED);
    }

    #[test]
    fn session_expiry_sliding_and_revoke() {
        let db = db_with_password("correct horse");
        let t0 = 10_000;
        let token = sign_in(&db, t0);
        let h = write_headers(&token, "", None);
        assert!(session_of(&db, &h, t0 + 10).is_some());
        // Valid until 7 days after last use, sliding.
        let day = 86_400;
        assert!(session_of(&db, &h, t0 + 6 * day).is_some());
        assert!(
            session_of(&db, &h, t0 + 12 * day).is_some(),
            "slid at day 6"
        );
        assert!(session_of(&db, &h, t0 + 20 * day).is_none(), "expired");
        // Revoke.
        let token = sign_in(&db, t0);
        let h = write_headers(&token, "", None);
        assert!(session_of(&db, &h, t0 + 1).is_some());
        let s = session_of(&db, &h, t0 + 1).unwrap();
        db.delete_session(&s.hash);
        assert!(session_of(&db, &h, t0 + 2).is_none());
        // Garbage cookies are ignored.
        for c in ["rh_session=xyz", "rh_session=", "other=1"] {
            assert!(session_of(&db, &headers(&[("cookie", c)]), t0).is_none());
        }
    }

    #[test]
    fn sign_out_others_and_password_change() {
        let db = db_with_password("correct horse");
        let a = sign_in(&db, 100);
        let b = sign_in(&db, 100);
        let ha = write_headers(&a, "", None);
        let hb = write_headers(&b, "", None);
        let keep = session_of(&db, &ha, 101).unwrap();
        db.delete_sessions_except(Some(&keep.hash));
        assert!(session_of(&db, &ha, 102).is_some());
        assert!(session_of(&db, &hb, 102).is_none());

        let lim = Mutex::new(RateLimiter::default());
        let e = change_password_db(&db, &lim, "nope nope", "new password", 200, &|| 200_000);
        assert!(e.unwrap_err().contains("wrong"));
        assert!(session_of(&db, &ha, 201).is_some(), "nothing changed");
        assert!(change_password_db(&db, &lim, "correct horse", "short", 300, &|| 900_000).is_err());
        // Back-off applies (a failure 1 s ago).
        let e = change_password_db(&db, &lim, "correct horse", "new password", 200, &|| 200_500);
        assert!(e.unwrap_err().contains("too many"));
        change_password_db(&db, &lim, "correct horse", "new password", 300, &|| 900_000).unwrap();
        assert!(
            session_of(&db, &ha, 301).is_none(),
            "all sessions signed out"
        );
        let stored = db.kv_get(KV_PASSWORD).unwrap();
        assert!(stored.starts_with("pbkdf2-sha256$600000$"));
        assert!(verify_password(&stored, "new password"));
        assert!(!verify_password(&stored, "correct horse"));
        let kinds: Vec<_> = db
            .events(&EventFilter::default())
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert!(kinds.contains(&"auth.password_changed".to_string()));
    }

    #[test]
    fn require_write_each_denial() {
        // No password set yet.
        let db = Db::open_in_memory().unwrap();
        assert_eq!(
            check_write(&db, &HeaderMap::new(), 1),
            Err(WriteDenied::NoPassword)
        );
        let db = db_with_password("correct horse");
        let now = 50_000;
        // No session / bad session.
        assert_eq!(
            check_write(&db, &HeaderMap::new(), now),
            Err(WriteDenied::NoSession)
        );
        let fake = "ab".repeat(32);
        assert_eq!(
            check_write(
                &db,
                &write_headers(&fake, "x", Some("http://10.0.0.1:3200")),
                now
            ),
            Err(WriteDenied::NoSession)
        );
        let token = sign_in(&db, now);
        let csrf = csrf_for(&token);
        let origin = Some("http://10.0.0.1:3200");
        // CSRF missing, wrong, or another session's.
        let other = csrf_for(&sign_in(&db, now));
        for bad in ["", "0", &csrf[..63], &other, &csrf.to_uppercase()] {
            assert_eq!(
                check_write(&db, &write_headers(&token, bad, origin), now),
                Err(WriteDenied::BadCsrf),
                "{bad}"
            );
        }
        // Origin missing, foreign, other port, scheme-less, null.
        for o in [
            None,
            Some("http://evil.example"),
            Some("http://10.0.0.1:3201"),
            Some("http://10.0.0.1"),
            Some("10.0.0.1:3200"),
            Some("null"),
            Some("http://10.0.0.1:3200@evil.example"),
        ] {
            assert_eq!(
                check_write(&db, &write_headers(&token, &csrf, o), now),
                Err(WriteDenied::BadOrigin),
                "{o:?}"
            );
        }
        // All good.
        assert_eq!(
            check_write(&db, &write_headers(&token, &csrf, origin), now),
            Ok("admin".to_string())
        );
        assert_eq!(
            check_write(
                &db,
                &write_headers(&token, &csrf, Some("https://10.0.0.1:3200")),
                now
            ),
            Ok("admin".to_string())
        );
        // Referer is used when there is no Origin.
        let mut h = write_headers(&token, &csrf, None);
        h.insert(
            REFERER,
            HeaderValue::from_static("http://10.0.0.1:3200/settings?x=1"),
        );
        assert!(check_write(&db, &h, now).is_ok());
        h.insert(
            REFERER,
            HeaderValue::from_static("http://evil.example/10.0.0.1:3200"),
        );
        assert_eq!(check_write(&db, &h, now), Err(WriteDenied::BadOrigin));
        // A wrong Origin is not rescued by a good Referer.
        let mut h = write_headers(&token, &csrf, Some("http://evil.example"));
        h.insert(REFERER, HeaderValue::from_static("http://10.0.0.1:3200/"));
        assert_eq!(check_write(&db, &h, now), Err(WriteDenied::BadOrigin));
        // Statuses.
        assert_eq!(WriteDenied::NoPassword.response().status(), 403);
        assert_eq!(WriteDenied::NoSession.response().status(), 401);
        assert_eq!(WriteDenied::BadCsrf.response().status(), 403);
        assert_eq!(WriteDenied::BadOrigin.response().status(), 403);
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
        let c = session_cookie(&"aa".repeat(32));
        assert!(c.starts_with("rh_session="));
        for part in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=604800"] {
            assert!(c.contains(part), "{c}");
        }
    }

    #[test]
    fn safe_next_rules() {
        for bad in [
            "/\t/evil.example",
            "/\u{0}",
            "/ /evil",
            "/%09/evil.example",
            "//evil.example",
            "/\\evil.example",
            "https://evil.example",
            "javascript:alert(1)",
            "",
            "evil",
            "/\n/evil.example",
            "/\r/evil.example",
            "/\u{7f}x",
            "/%2F/evil.example",
            "/%5Cevil.example",
            "/%20/evil",
            "/a%20b", // decoded form has a space
            "/%00",
            "/%zz",
            "/%E2%80%",
            "/ü",
            "\\\\evil.example",
        ] {
            assert_eq!(safe_next(bad), None, "{bad:?}");
        }
        for good in [
            "/",
            "/containers?state=running",
            "/containers/web-1",
            "/history#cpu",
            "/a%2Db",
        ] {
            assert_eq!(safe_next(good), Some(good), "{good:?}");
        }
        assert_eq!(safe_next(&format!("/{}", "a".repeat(5000))), None);
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
