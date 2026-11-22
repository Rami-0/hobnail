//! Account credentials, sessions, CSRF and login throttling.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use http_body_util::{BodyExt, Limited};
use hyper::header::{
    HeaderValue, CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, ORIGIN, REFERER, RETRY_AFTER,
    SET_COOKIE,
};
use hyper::{HeaderMap, Method, Request, Response, StatusCode};
use ring::{digest, rand::SecureRandom};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::api::error::ApiError;
use crate::db::accounts::AccountError;

use crate::db::{Db, EventFilter};
use crate::http::server::{full, set, Body};
use crate::state::AppState;

pub const COOKIE_NAME: &str = "rh_session";
pub const CSRF_HEADER: &str = "x-rh-csrf";
/// Sessions last this long after their last use.
pub const SESSION_TTL_SECS: i64 = 7 * 86_400;
pub const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_BYTES: usize = 1024;
/// Login request bodies larger than this are refused.
const MAX_BODY: usize = 4096;

/// Eight waiting requests retain only parsed credentials and request state.
pub(crate) const CREDENTIAL_WAITERS: usize = 8;
const CREDENTIAL_WAIT: Duration = Duration::from_secs(5);
/// Sign-in responses while legacy hashes remain take at least this long
/// after the request obtains the credential permit.
pub(crate) const MIN_CREDENTIAL_FLOOR: Duration = Duration::from_millis(150);
/// The most the floor can grow to, and the fixed answer time (from obtaining
/// the permit) of any operation that outran its floor. With the factor and
/// margin below it covers the legacy estimate for current-profile verifies
/// up to 975 ms, over 20 times the 45 ms measured in M5.
pub(crate) const MAX_CREDENTIAL_FLOOR: Duration = Duration::from_secs(2);
/// Legacy verify cost relative to a current one. By work alone it is
/// 19,456 KiB x 2 / (7,168 KiB x 5) = 1.09; M5 measured 1.55 (68.85 / 44.52 ms
/// medians); 2 leaves room above that.
const LEGACY_COST_FACTOR: u32 = 2;
/// Added to the scaled estimate for scheduling jitter (the M5 legacy maximum
/// was 82.52 ms against a 44.52 ms current median: 2 x 44.52 + 50 covers it).
const LEGACY_MARGIN: Duration = Duration::from_millis(50);
/// Verify times are kept for one to two of these windows.
const SAMPLE_WINDOW: Duration = Duration::from_secs(150);

pub(crate) struct CredentialQueue {
    active: Arc<Semaphore>,
    waiters: Semaphore,
    samples: Mutex<VerifySamples>,
    /// The memory limit source (the cgroup in production, fixed in tests).
    pub(crate) memory_limit: fn() -> Option<u64>,
}

impl Default for CredentialQueue {
    fn default() -> Self {
        Self {
            active: Arc::new(Semaphore::new(1)),
            waiters: Semaphore::new(CREDENTIAL_WAITERS),
            samples: Mutex::new(VerifySamples::new(Instant::now())),
            memory_limit: read_memory_limit,
        }
    }
}

/// Current-profile verify times (real accounts and the dummy record used for
/// unknown and disabled ones): the maximum over the last 150 to 300 seconds,
/// falling back to the latest sample once both windows are empty. An outlier
/// (a loaded host) therefore stops widening the floor within five minutes.
/// Legacy verifications are never recorded: the bound must not move in a way
/// that depends on which account was tried.
#[derive(Debug)]
struct VerifySamples {
    started: Instant,
    current: Duration,
    previous: Duration,
    latest: Duration,
}

impl VerifySamples {
    fn new(now: Instant) -> Self {
        Self {
            started: now,
            current: Duration::ZERO,
            previous: Duration::ZERO,
            latest: Duration::ZERO,
        }
    }

    fn roll(&mut self, now: Instant) {
        let age = now.saturating_duration_since(self.started);
        if age >= SAMPLE_WINDOW {
            self.previous = if age >= SAMPLE_WINDOW * 2 {
                Duration::ZERO
            } else {
                self.current
            };
            self.current = Duration::ZERO;
            self.started = now;
        }
    }

    fn record(&mut self, verify: Duration, now: Instant) {
        self.roll(now);
        self.current = self.current.max(verify);
        self.latest = verify;
    }

    fn bound(&mut self, now: Instant) -> Duration {
        self.roll(now);
        let recent = self.current.max(self.previous);
        if recent.is_zero() {
            self.latest
        } else {
            recent
        }
    }
}

/// The smallest memory limit under which a legacy 19 MiB hash is verified.
/// The heaviest measured M5 workload peaked at 12.45 MiB without it; adding
/// the 19 MiB Argon2 block gives 31.45 MiB, and the rest is headroom for
/// allocator fragmentation, kernel and socket memory and cold file cache.
pub(crate) const LEGACY_VERIFY_MIN_LIMIT: u64 = 36 << 20;

/// This process's cgroup v2 memory limit; `max` or unreadable is no limit.
pub(crate) fn read_memory_limit() -> Option<u64> {
    parse_memory_max(&std::fs::read_to_string("/sys/fs/cgroup/memory.max").ok()?)
}

fn parse_memory_max(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

pub(crate) fn legacy_verify_fits(limit: Option<u64>) -> bool {
    limit.is_none_or(|limit| limit >= LEGACY_VERIFY_MIN_LIMIT)
}

/// Whether `stored` may be verified now. A legacy hash under a memory limit
/// too small for it is refused (and logged) instead of risking an OOM kill.
fn may_verify(state: &AppState, stored: &str) -> bool {
    if !is_legacy_hash(stored) {
        return true;
    }
    let limit = (state.credential_queue.memory_limit)();
    if legacy_verify_fits(limit) {
        return true;
    }
    crate::warn!(
        "legacy Argon2id hash not verified: the memory limit is too small; reset the account with hobnail reset-password <username>",
        limit = limit.unwrap_or_default(),
        required = LEGACY_VERIFY_MIN_LIMIT,
    );
    false
}

/// The floor for a current-profile bound: the legacy estimate (the bound
/// scaled by [`LEGACY_COST_FACTOR`] plus [`LEGACY_MARGIN`]), so every padded
/// attempt, legacy or not, answers at the same time; at least 150 ms and at
/// most [`MAX_CREDENTIAL_FLOOR`].
fn credential_floor(bound: Duration) -> Duration {
    (bound * LEGACY_COST_FACTOR + LEGACY_MARGIN).clamp(MIN_CREDENTIAL_FLOOR, MAX_CREDENTIAL_FLOOR)
}

/// When to answer: at the floor deadline, or, if the work ran past it, at the
/// fixed cap, so how far it overran is not measurable. Work that outlasts
/// even the cap is answered when it ends.
fn answer_at(
    acquired: tokio::time::Instant,
    deadline: tokio::time::Instant,
    finished: tokio::time::Instant,
) -> tokio::time::Instant {
    if finished <= deadline {
        deadline
    } else {
        finished.max(acquired + MAX_CREDENTIAL_FLOOR)
    }
}

impl CredentialQueue {
    pub(crate) async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        if let Ok(permit) = self.active.clone().try_acquire_owned() {
            return Some(permit);
        }
        let _slot = self.waiters.try_acquire().ok()?;
        tokio::time::timeout(CREDENTIAL_WAIT, self.active.clone().acquire_owned())
            .await
            .ok()?
            .ok()
    }

    /// The current padding for sign-in responses (see [`credential_floor`]).
    pub(crate) fn floor(&self) -> Duration {
        self.floor_at(Instant::now())
    }

    fn floor_at(&self, now: Instant) -> Duration {
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        credential_floor(samples.bound(now))
    }

    /// Records one current-profile verification (never a legacy one).
    fn observe_verify(&self, verify: Duration, now: Instant) {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(verify, now);
    }

    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        CREDENTIAL_WAITERS - self.waiters.available_permits()
    }
}

pub(crate) fn credential_busy(legacy: bool) -> Response<Body> {
    let message = "credential service busy; try again";
    let mut response = if legacy {
        json_response(
            StatusCode::TOO_MANY_REQUESTS,
            &serde_json::json!({"error": message, "retry_after": 1}),
        )
    } else {
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", message).response()
    };
    set(&mut response, RETRY_AFTER, "1");
    response
}

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

pub fn password_is_set(state: &AppState) -> bool {
    state.db.owner_exists()
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
        let secure = crate::http::proxy::is_https(&req, &state.env);
        return logout(state, session.expect("logout gate"), secure);
    }
    credential_route(state, req, CredentialAction::Login).await
}

pub fn cli_list_legacy_hashes(args: &[String]) -> i32 {
    if !args.is_empty() {
        eprintln!("usage: hobnail list-legacy-hashes");
        return 1;
    }
    let result = Db::open(&crate::db::path_from_env())
        .map_err(|e| e.to_string())
        .and_then(|db| db.legacy_hash_users().map_err(|e| format!("{e:?}")));
    match result {
        Ok(users) => {
            for name in users {
                println!("{name}");
            }
            0
        }
        Err(e) => {
            eprintln!("list-legacy-hashes: {e}");
            1
        }
    }
}

fn store_console_password(
    db: &Db,
    name: Option<&str>,
    password: &str,
    reset: bool,
) -> Result<(), String> {
    check_password_policy(password)?;
    let hash = hash_password(password)?;
    let now = now_s();
    if reset {
        db.reset_account_password(name.expect("reset requires a name"), &hash, now)
    } else {
        db.recover_owner(name, &hash, now)
    }
    .map_err(|e| format!("{e:?}"))
}

/// `hobnail set-password`: reads the new password from the TTY without
/// echo (twice), or one line from stdin when piped; stores the hash in the
/// database and signs out the affected account.
pub fn cli_set_password(args: &[String], reset: bool) -> i32 {
    let (command, operand) = if reset {
        ("reset-password", "<username>")
    } else {
        ("set-password", "[username]")
    };
    if args.len() > 1
        || (reset && args.is_empty())
        || args.first().is_some_and(|name| !valid_name(name))
    {
        eprintln!("usage: hobnail {command} {operand} (1–64 letters, digits, dots, hyphens or underscores)");
        return 1;
    }
    let path = crate::db::path_from_env();
    let db = match Db::open(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{command}: cannot open the database: {e}");
            return 1;
        }
    };
    let had = db.owner_exists();
    let password = if unsafe { libc::isatty(0) } == 1 {
        let a = match tty::read_secret("New password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{command}: {e}; nothing changed");
                return 1;
            }
        };
        if let Err(e) = check_password_policy(&a) {
            eprintln!("{command}: {e}; nothing changed");
            return 1;
        }
        let b = match tty::read_secret("Repeat password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{command}: {e}; nothing changed");
                return 1;
            }
        };
        if a != b {
            eprintln!("{command}: the passwords do not match; nothing changed");
            return 1;
        }
        a
    } else {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => {
                eprintln!("{command}: no password on stdin; nothing changed");
                return 1;
            }
            Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            Err(_) => {
                eprintln!("{command}: could not read stdin; nothing changed");
                return 1;
            }
        }
    };
    let name = args.first().map(String::as_str);
    match store_console_password(&db, name, &password, reset) {
        Ok(()) => {
            db.record_event(
                now_s(),
                "auth.password_changed",
                "info",
                if reset {
                    name.expect("reset requires a name")
                } else {
                    "owner"
                },
                "set from the console",
            );
            println!(
                "Password {} in {}. The account's sessions were signed out.",
                if reset || had { "changed" } else { "set" },
                path.display()
            );
            0
        }
        Err(e) => {
            eprintln!("{command}: {e}; nothing changed");
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
    if !may_verify(state, &stored) {
        return Err(
            "this account's old password hash needs more memory than the server may use; ask the owner to run hobnail reset-password".into(),
        );
    }
    if current.len() > MAX_PASSWORD_BYTES || verify_password(&stored, current).is_none() {
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
    hash_with(&argon2(7_168, 5), password)
}

fn hash_with(algorithm: &Argon2<'_>, password: &str) -> Result<String, String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()?).map_err(|e| e.to_string())?;
    algorithm
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PasswordProfile {
    Current,
    Legacy,
    #[cfg(test)]
    Test,
}

fn password_profile(hash: &PasswordHash<'_>) -> Option<PasswordProfile> {
    if hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(19)
        || hash.params.get_decimal("p") != Some(1)
    {
        return None;
    }
    Some(
        match (hash.params.get_decimal("m")?, hash.params.get_decimal("t")?) {
            (7_168, 5) => PasswordProfile::Current,
            (19_456, 2) => PasswordProfile::Legacy,
            #[cfg(test)]
            (32, 1) => PasswordProfile::Test,
            _ => return None,
        },
    )
}

pub(crate) fn is_legacy_hash(stored: &str) -> bool {
    PasswordHash::new(stored)
        .ok()
        .as_ref()
        .and_then(password_profile)
        == Some(PasswordProfile::Legacy)
}

fn verify_password(stored: &str, password: &str) -> Option<PasswordProfile> {
    let hash = PasswordHash::new(stored).ok()?;
    let profile = password_profile(&hash)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .ok()?;
    Some(profile)
}

// Public dummy record with the same work factors as a real password.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=7168,t=5,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

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

pub(crate) fn refresh_cookie(
    state: &AppState,
    session: &Session,
    secure: bool,
    response: &mut Response<Body>,
) {
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
            &session_cookie(&session.token, expires - now, secure),
        );
    }
}

fn session_cookie(token: &str, max_age: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}")
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

    #[cfg(test)]
    pub(crate) fn record_test_failure(&mut self, ip: IpAddr) {
        self.failure(ip, now_ms());
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
    let secure = crate::http::proxy::is_https(&req, &state.env);
    let ip = crate::http::proxy::client_ip(&req, &state.env);
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
    let Some(permit) = state.credential_queue.acquire().await else {
        return credential_busy(legacy);
    };
    let outcome = run_credential(state, permit, ip, action, credentials).await;
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
        &session_cookie(&token, SESSION_TTL_SECS, secure),
    );
    response
}

/// Runs one credential operation with the permit and answers no earlier than
/// its floor deadline while legacy hashes remain.
///
/// The deadline starts when this request holds the only permit, and the
/// permit is kept until then. A queued request therefore waits for the
/// previous holder's deadline, never for its hash, so neither its own
/// response time nor a later one reveals which hash profile was checked.
/// The work, the wait and the release run in a detached task that owns the
/// permit: a client that disconnects (dropping this future) cannot release
/// it early. The task only awaits; hashing stays on the blocking pool.
async fn run_credential(
    state: &Arc<AppState>,
    permit: OwnedSemaphorePermit,
    ip: IpAddr,
    action: CredentialAction,
    credentials: Credentials,
) -> Result<String, Attempt> {
    let acquired = tokio::time::Instant::now();
    let deadline = acquired + state.credential_queue.floor();
    let st = state.clone();
    let task = tokio::spawn(async move {
        let joined = tokio::task::spawn_blocking(move || {
            // Fail closed: a scan error pads like a remaining legacy hash.
            let padded = action == CredentialAction::Login
                && !st
                    .db
                    .legacy_hash_users()
                    .is_ok_and(|users| users.is_empty());
            (authenticate(&st, ip, action, credentials), padded)
        })
        .await;
        let (outcome, padded) = joined.unwrap_or((Err(Attempt::Failed), true));
        if padded {
            let finished = tokio::time::Instant::now();
            tokio::time::sleep_until(answer_at(acquired, deadline, finished)).await;
        }
        drop(permit);
        outcome
    });
    task.await.unwrap_or(Err(Attempt::Failed))
}

/// Measures one current-profile verify at boot, so the floor is not just
/// the 150 ms constant until the first sign-in after a restart. Runs only
/// while legacy hashes remain (the floor applies only then), under the
/// permit, so it never overlaps a sign-in; it costs one 7 MiB hash. The
/// legacy estimate is derived from it (no 19 MiB hash runs at boot).
pub(crate) async fn calibrate_floor(state: &Arc<AppState>) {
    let Some(permit) = state.credential_queue.acquire().await else {
        return;
    };
    let st = state.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = Instant::now();
        let _ = verify_password(DUMMY_HASH, "calibration");
        st.credential_queue
            .observe_verify(started.elapsed(), Instant::now());
    })
    .await;
}

fn authenticate(
    state: &AppState,
    ip: IpAddr,
    action: CredentialAction,
    credentials: Credentials,
) -> Result<String, Attempt> {
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
        // Unknown and disabled accounts can never sign in: check the dummy
        // record, so a disabled legacy account costs no 19 MiB hash.
        let stored = match &credential {
            Some((_, hash, false)) => hash.as_str(),
            _ => DUMMY_HASH,
        };
        let verified = (password.len() <= MAX_PASSWORD_BYTES && may_verify(state, stored))
            .then(|| {
                let started = Instant::now();
                let verified = verify_password(stored, &password);
                if !is_legacy_hash(stored) {
                    state
                        .credential_queue
                        .observe_verify(started.elapsed(), Instant::now());
                }
                verified
            })
            .flatten();
        match (credential, verified) {
            (Some((id, stored, false)), Some(profile)) => {
                db.sign_in(id, &stored, &hash, now, expires).map(|()| {
                    if profile == PasswordProfile::Legacy {
                        if let Ok(replacement) = hash_password(&password) {
                            db.upgrade_password_hash(id, &stored, &replacement);
                        }
                    }
                })
            }
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

fn logout(state: &AppState, session: &Session, secure: bool) -> Response<Body> {
    state.db.delete_session(&session.hash);
    let mut r = json_response(StatusCode::OK, &serde_json::json!({ "ok": true }));
    set(&mut r, SET_COOKIE, &session_cookie("", 0, secure));
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
    use std::net::Ipv4Addr;

    #[test]
    fn the_floor_is_the_scaled_legacy_estimate_of_recent_verifies() {
        let ms = Duration::from_millis;
        assert_eq!(credential_floor(Duration::ZERO), MIN_CREDENTIAL_FLOOR);
        assert_eq!(credential_floor(ms(45)), MIN_CREDENTIAL_FLOOR);
        assert_eq!(credential_floor(ms(200)), ms(450));
        assert_eq!(credential_floor(ms(975)), MAX_CREDENTIAL_FLOOR);
        assert_eq!(credential_floor(ms(9_000)), MAX_CREDENTIAL_FLOOR);

        let queue = CredentialQueue::default();
        let t0 = Instant::now();
        assert_eq!(queue.floor_at(t0), MIN_CREDENTIAL_FLOOR, "no sample yet");
        queue.observe_verify(ms(400), t0); // an outlier under load
        queue.observe_verify(ms(45), t0 + ms(10));
        assert_eq!(queue.floor_at(t0 + ms(20)), ms(850));
        // Still inside the previous window.
        let later = t0 + SAMPLE_WINDOW + ms(30);
        queue.observe_verify(ms(60), later);
        assert_eq!(queue.floor_at(later), ms(850));
        // Two windows on, the outlier is gone: 2 x 60 + 50 ms.
        let much_later = later + SAMPLE_WINDOW;
        assert_eq!(queue.floor_at(much_later), ms(170));
        // Without a recent sample the latest one holds: never below the
        // estimate of the last measured hash speed.
        queue.observe_verify(ms(300), much_later);
        assert_eq!(queue.floor_at(much_later + SAMPLE_WINDOW * 5), ms(650));
    }

    #[test]
    fn an_overrun_answers_at_the_fixed_cap() {
        let t0 = tokio::time::Instant::now();
        let ms = Duration::from_millis;
        let deadline = t0 + ms(150);
        assert_eq!(answer_at(t0, deadline, t0 + ms(80)), deadline);
        assert_eq!(answer_at(t0, deadline, deadline), deadline);
        assert_eq!(
            answer_at(t0, deadline, t0 + ms(151)),
            t0 + MAX_CREDENTIAL_FLOOR
        );
        assert_eq!(
            answer_at(t0, deadline, t0 + ms(1999)),
            t0 + MAX_CREDENTIAL_FLOOR
        );
        assert_eq!(answer_at(t0, deadline, t0 + ms(2500)), t0 + ms(2500));
    }

    #[test]
    fn legacy_account_attempts_never_change_the_bound() {
        let state = crate::testutil::state_with(vec![], "");
        let legacy = hash_with(&argon2(19_456, 2), "correct horse").unwrap();
        state.db.recover_owner(None, &legacy, 0).unwrap();
        let state = with_memory_limit(state, || None);
        let snapshot = |st: &AppState| format!("{:?}", st.credential_queue.samples.lock().unwrap());
        let before = snapshot(&state);
        // A wrong password, a refusal before hashing, and the same under a
        // memory limit that refuses the legacy verify.
        assert!(matches!(
            login_attempt(&state, 1, "admin", "wrong horse"),
            Err(Attempt::Wrong)
        ));
        assert!(matches!(
            login_attempt(&state, 1, "admin", "wrong horse"),
            Err(Attempt::Limited(_))
        ));
        let state = with_memory_limit(state, || Some(20 << 20));
        assert!(matches!(
            login_attempt(&state, 2, "admin", "correct horse"),
            Err(Attempt::Wrong)
        ));
        assert_eq!(snapshot(&state), before);
        // A successful legacy sign-in rehashes, still without a sample.
        let state = with_memory_limit(state, || None);
        login_attempt(&state, 3, "admin", "correct horse").unwrap();
        assert_eq!(snapshot(&state), before);
        // An unknown account verifies the current-profile dummy: recorded.
        assert!(matches!(
            login_attempt(&state, 4, "nobody", "wrong horse"),
            Err(Attempt::Wrong)
        ));
        assert_ne!(snapshot(&state), before);
    }

    #[tokio::test]
    async fn a_dropped_sign_in_keeps_the_permit_until_its_deadline() {
        let state = crate::testutil::state_with(vec![], "");
        set_password_for_tests(&state.db, "correct horse");
        state.db.with((), |i| {
            i.conn.execute(
                "INSERT INTO users(name,password_hash,role,created,updated)
                 VALUES ('old','$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA','viewer',0,0)",
                [],
            )?;
            Ok(())
        });
        let permit = state.credential_queue.acquire().await.unwrap();
        let acquired = tokio::time::Instant::now();
        let sign_in = run_credential(
            &state,
            permit,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            CredentialAction::Login,
            Credentials {
                username: "admin".into(),
                password: "wrong horse".into(),
                token: None,
            },
        );
        // The client goes away while the (fast, test-profile) hash runs.
        assert!(tokio::time::timeout(Duration::from_millis(20), sign_in)
            .await
            .is_err());
        let _next = state.credential_queue.acquire().await.unwrap();
        assert!(
            acquired.elapsed() >= MIN_CREDENTIAL_FLOOR,
            "released after {:?}",
            acquired.elapsed()
        );
    }

    #[test]
    fn memory_limits_gate_legacy_verification() {
        assert_eq!(parse_memory_max("max\n"), None);
        assert_eq!(parse_memory_max(""), None);
        assert_eq!(parse_memory_max("20971520\n"), Some(20 << 20));
        assert!(legacy_verify_fits(None));
        assert!(!legacy_verify_fits(Some(20 << 20)));
        assert!(!legacy_verify_fits(Some(LEGACY_VERIFY_MIN_LIMIT - 1)));
        assert!(legacy_verify_fits(Some(LEGACY_VERIFY_MIN_LIMIT)));
    }

    fn with_memory_limit(mut state: Arc<AppState>, limit: fn() -> Option<u64>) -> Arc<AppState> {
        Arc::get_mut(&mut state)
            .unwrap()
            .credential_queue
            .memory_limit = limit;
        state
    }

    fn login_attempt(
        state: &AppState,
        host: u8,
        username: &str,
        password: &str,
    ) -> Result<String, Attempt> {
        authenticate(
            state,
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, host)),
            CredentialAction::Login,
            Credentials {
                username: username.into(),
                password: password.into(),
                token: None,
            },
        )
    }

    #[test]
    fn a_small_memory_limit_refuses_legacy_verification_like_a_wrong_password() {
        let state = with_memory_limit(crate::testutil::state_with(vec![], ""), || Some(20 << 20));
        let previous = hash_with(&argon2(19_456, 2), "correct horse").unwrap();
        state.db.recover_owner(None, &previous, 0).unwrap();
        crate::log::capture_test_logs();
        assert!(matches!(
            login_attempt(&state, 1, "admin", "correct horse"),
            Err(Attempt::Wrong)
        ));
        let logs = crate::log::take_test_logs().join("");
        assert!(logs.contains("hobnail reset-password"), "{logs}");
        assert!(!logs.contains("correct horse") && !logs.contains("\"admin\""));
        assert_eq!(state.db.credential("admin").unwrap().1, previous);

        // Without a limit the same password signs in and migrates.
        let state = with_memory_limit(state, || None);
        login_attempt(&state, 2, "admin", "correct horse").unwrap();
        assert!(state.db.legacy_hash_users().unwrap().is_empty());
    }

    #[test]
    fn disabled_accounts_are_checked_against_the_dummy_hash() {
        let state = with_memory_limit(crate::testutil::state_with(vec![], ""), || Some(20 << 20));
        set_password_for_tests(&state.db, "owner password");
        let legacy = hash_with(&argon2(19_456, 2), "correct horse").unwrap();
        state.db.with((), |i| {
            i.conn.execute(
                "INSERT INTO users(name,password_hash,role,created,updated,disabled)
                 VALUES ('gone',?1,'viewer',0,0,1)",
                [&legacy],
            )?;
            Ok(())
        });
        crate::log::capture_test_logs();
        assert!(matches!(
            login_attempt(&state, 1, "gone", "correct horse"),
            Err(Attempt::Wrong)
        ));
        // The legacy record was never chosen for verification.
        assert_eq!(crate::log::take_test_logs(), Vec::<String>::new());
    }

    #[test]
    fn argon2id_hashes_are_salted_and_verifiable() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=7168,t=5,p=1$"));
        assert_eq!(
            verify_password(&hash, "correct horse"),
            Some(PasswordProfile::Current)
        );
        assert_eq!(verify_password(&hash, "wrong horse"), None);
        assert_eq!(verify_password("invalid", "correct horse"), None);
        assert_ne!(hash, hash_password("correct horse").unwrap());
    }

    #[test]
    fn console_reset_migrates_a_named_account_and_revokes_its_sessions() {
        let state = crate::testutil::state_with(vec![], "");
        let db = &state.db;
        let old = hash_with(&argon2(19_456, 2), "old password").unwrap();
        store_console_password(db, None, "owner password", false).unwrap();
        db.with((), |i| {
            i.conn.execute(
                "INSERT INTO users(name,password_hash,role,created,updated) VALUES ('guest',?1,'viewer',0,0)",
                [&old],
            )?;
            Ok(())
        });
        let (id, _, _) = db.credential("guest").unwrap();
        let now = now_s();
        db.sign_in(id, &old, "old-session", now, now + 100).unwrap();
        assert_eq!(db.legacy_hash_users().unwrap(), ["guest"]);

        store_console_password(db, Some("guest"), "new password", true).unwrap();

        let (same_id, hash, _) = db.credential("guest").unwrap();
        assert_eq!(same_id, id);
        assert_eq!(
            verify_password(&hash, "new password"),
            Some(PasswordProfile::Current)
        );
        assert!(!db.session_is_valid("old-session", now));
        assert!(db.legacy_hash_users().unwrap().is_empty());
        assert_eq!(db.accounts().unwrap().len(), 2);
        assert!(store_console_password(db, Some("absent"), "new password", true).is_err());
    }

    #[test]
    fn legacy_hash_upgrades_only_after_successful_login() {
        let state = crate::testutil::state_with(vec![], "");
        let previous = hash_with(&argon2(19_456, 2), "correct horse").unwrap();
        state.db.recover_owner(None, &previous, 0).unwrap();
        let login = |host, password: &str| {
            authenticate(
                &state,
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, host)),
                CredentialAction::Login,
                Credentials {
                    username: "admin".into(),
                    password: password.into(),
                    token: None,
                },
            )
        };
        let read_hash = || state.db.credential("admin").unwrap().1;
        assert!(matches!(login(1, "wrong horse"), Err(Attempt::Wrong)));
        assert_eq!(read_hash(), previous);
        assert_eq!(state.db.legacy_hash_users().unwrap(), ["admin"]);

        state.db.with((), |i| {
            i.conn.execute_batch(
                "CREATE TRIGGER fail_upgrade BEFORE UPDATE OF password_hash ON users
                 BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
            )
        });
        let token = login(2, "correct horse").unwrap();
        assert!(state
            .db
            .account_session(&sha256_hex(token.as_bytes()), now_s())
            .is_some());
        assert_eq!(read_hash(), previous);
        state
            .db
            .with((), |i| i.conn.execute_batch("DROP TRIGGER fail_upgrade"));

        login(2, "correct horse").unwrap();
        let upgraded = read_hash();
        assert!(state.db.legacy_hash_users().unwrap().is_empty());
        assert_eq!(
            verify_password(&upgraded, "correct horse"),
            Some(PasswordProfile::Current)
        );
        login(2, "correct horse").unwrap();
        assert_eq!(read_hash(), upgraded);
    }

    #[test]
    fn login_rejects_unknown_accounts_and_throttles_failures() {
        let state = crate::testutil::state_with(vec![], "");
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
        let c = session_cookie(&"aa".repeat(32), SESSION_TTL_SECS, true);
        assert!(c.starts_with("rh_session="));
        for part in [
            "HttpOnly",
            "SameSite=Strict",
            "Path=/",
            "Max-Age=604800",
            "Secure",
        ] {
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
