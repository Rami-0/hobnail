//! Signed-in JSON read API over the history database (GET only;
//! the central router only sends GET/HEAD here). See docs/DESIGN.md section 4.
//!
//! - `GET /api/history/system?metric=cpu&range=24h[&agg=max]`
//! - `GET /api/history/container/<name>?metric=mem&range=6h[&agg=max]`
//! - `GET /api/events?limit=50&before=<id>&subject=<name>&kind=<prefix>`
//!
//! Series: `{"range":"24h","step":60,"points":[[unix,value],...]}`, at most
//! ~400 points, oldest first; a gap in the data is a gap in the points.
//! `range` defaults to `1h`; `metric` is required.
//! Events: `{"events":[{"id","ts","kind","level","subject","detail"},...]}`,
//! newest first; page with `before=<last id>`.
//! Bad parameters give `400` with `{"error":"..."}`. Queries run on the
//! blocking pool.

use std::sync::Arc;

use hyper::{Request, Response, StatusCode};

use super::{Agg, CtrMetric, Db, EventFilter, Range, Series, SysMetric};
use crate::http::server::{full, set, Body};
use crate::state::AppState;

const MAX_NAME: usize = 256;

pub async fn route(
    state: &Arc<AppState>,
    req: &Request<hyper::body::Incoming>,
    admin: bool,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let db = state.db.clone();
    let now = crate::util::unix_now() as i64;
    let res = tokio::task::spawn_blocking(move || answer(&db, &path, &query, now, admin))
        .await
        .unwrap_or_else(|_| Err((StatusCode::INTERNAL_SERVER_ERROR, "query failed".into())));
    match res {
        Ok(body) => {
            let mut r = full(StatusCode::OK, body);
            set(&mut r, hyper::header::CONTENT_TYPE, "application/json");
            set(&mut r, hyper::header::CACHE_CONTROL, "no-cache");
            r
        }
        Err((status, msg)) => json_error(status, &msg),
    }
}

pub fn json_error(status: StatusCode, msg: &str) -> Response<Body> {
    let mut r = full(
        status,
        serde_json::json!({ "error": msg }).to_string().into_bytes(),
    );
    set(&mut r, hyper::header::CONTENT_TYPE, "application/json");
    set(&mut r, hyper::header::CACHE_CONTROL, "no-store");
    r
}

type ApiResult = Result<String, (StatusCode, String)>;

fn bad(msg: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

/// The whole API as a blocking function of path and query (testable).
pub(crate) fn answer(db: &Db, path: &str, query: &str, now: i64, signed_in: bool) -> ApiResult {
    let q = parse_query(query).ok_or_else(|| bad("malformed query string"))?;
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if path == "/api/events" {
        let f = events_filter(get)?.for_viewer(signed_in);
        return Ok(events_json(&db.events(&f)));
    }
    let rest = path
        .strip_prefix("/api/history/")
        .ok_or((StatusCode::NOT_FOUND, "unknown history route".to_string()))?;
    let range = match get("range") {
        None => Range::H1,
        Some(r) => Range::parse(r).ok_or_else(|| bad("range must be one of 1h 6h 24h 7d 30d"))?,
    };
    let agg = match get("agg") {
        None | Some("avg") => Agg::Avg,
        Some("max") => Agg::Max,
        Some(_) => return Err(bad("agg must be avg or max")),
    };
    let series = if rest == "system" {
        let m = get("metric").and_then(SysMetric::parse).ok_or_else(|| {
            bad("metric must be one of cpu mem swap disk net_rx net_tx temp load1")
        })?;
        db.system_series_agg(m, range, now, agg)
    } else if let Some(name) = rest.strip_prefix("container/") {
        let name = percent_decode(name).ok_or_else(|| bad("bad container name"))?;
        if name.is_empty()
            || name.len() > MAX_NAME
            || name.contains('/')
            || name.chars().any(char::is_control)
        {
            return Err(bad("bad container name"));
        }
        let m = get("metric")
            .and_then(CtrMetric::parse)
            .ok_or_else(|| bad("metric must be one of cpu mem"))?;
        db.container_series_agg(&name, m, range, now, agg)
    } else {
        return Err((StatusCode::NOT_FOUND, "unknown history route".into()));
    };
    Ok(series_json(range, &series))
}

fn events_filter<'a>(
    get: impl Fn(&str) -> Option<&'a str>,
) -> Result<EventFilter, (StatusCode, String)> {
    let mut f = EventFilter::default();
    if let Some(l) = get("limit") {
        let n: usize = l
            .parse()
            .ok()
            .filter(|n| (1..=500).contains(n))
            .ok_or_else(|| bad("limit must be a number from 1 to 500"))?;
        f.limit = n;
    }
    if let Some(b) = get("before") {
        f.before_id = Some(
            b.parse()
                .ok()
                .filter(|&n: &i64| n > 0)
                .ok_or_else(|| bad("before must be an event id"))?,
        );
    }
    if let Some(s) = get("subject").filter(|s| !s.is_empty()) {
        if s.len() > MAX_NAME {
            return Err(bad("subject too long"));
        }
        f.subject = Some(s.to_string());
    }
    if let Some(k) = get("kind").filter(|s| !s.is_empty()) {
        if k.len() > 64
            || !k
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_')
        {
            return Err(bad("kind must be an event kind prefix like container."));
        }
        f.kind_prefix = Some(k.to_string());
    }
    Ok(f)
}

/// `{"range":"24h","step":60,"points":[[unix,value],...]}`. Values keep
/// three decimals (plenty for charts, short JSON).
pub fn series_json(range: Range, s: &Series) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(48 + s.points.len() * 22);
    let _ = write!(
        out,
        "{{\"range\":\"{}\",\"step\":{},\"points\":[",
        range.as_str(),
        s.step
    );
    for (n, (t, v)) in s.points.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        let v = (v * 1000.0).round() / 1000.0;
        // `{}` of a finite f64 is valid JSON (no NaN/inf: filtered earlier).
        let _ = write!(out, "[{t},{v}]");
    }
    out.push_str("]}");
    out
}

fn events_json(evs: &[super::Event]) -> String {
    let v: Vec<_> = evs
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id, "ts": e.ts, "kind": e.kind, "level": e.level,
                "subject": e.subject, "detail": e.detail,
            })
        })
        .collect();
    serde_json::json!({ "events": v }).to_string()
}

/// `a=1&b=x%20y` -> pairs (`+` is a space). `None` for a malformed escape or
/// invalid UTF-8.
pub fn parse_query(q: &str) -> Option<Vec<(String, String)>> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            Some((
                percent_decode(&k.replace('+', " "))?,
                percent_decode(&v.replace('+', " "))?,
            ))
        })
        .collect()
}

/// `None` for malformed escapes or invalid UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
