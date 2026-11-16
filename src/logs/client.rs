//! Docker requests for logs (GET only): inspect a container (TTY? running?
//! started when?) and open its log stream. Each stream is its own unix
//! socket connection, so dropping the response closes it at once and Docker
//! stops following the log file.

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::Method;
use std::time::Duration;
use tokio::net::UnixStream;

use super::frame::{split_timestamp, Parser, RawLine};
use crate::http::client::{read_body, request_on_io};

const INSPECT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_INSPECT: usize = 4 * 1024 * 1024;
/// Most lines a viewer can ask for in one go (the browser ring holds 5,000).
pub const MAX_TAIL: u32 = 5000;
/// Most lines / bytes one `Stream::next` returns.
const BATCH_LINES: usize = 256;
const BATCH_BYTES: usize = 64 * 1024;

/// Docker container names: `[a-zA-Z0-9][a-zA-Z0-9_.-]*`. Anything else is
/// refused before it reaches a URL.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 200
        && n.as_bytes()[0].is_ascii_alphanumeric()
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub tty: bool,
    pub running: bool,
    /// Docker state: running, exited, ...
    pub status: String,
    /// Unix seconds of the last start (None if never started).
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub exit_code: Option<i64>,
}

#[derive(Debug)]
pub enum Error {
    /// Socket missing or refused.
    Unreachable(String),
    NotFound,
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable(e) => write!(f, "Docker is unreachable: {e}"),
            Error::NotFound => f.write_str("container not found"),
            Error::Other(e) => f.write_str(e),
        }
    }
}

async fn get(socket: &str, path: &str) -> Result<hyper::Response<Incoming>, Error> {
    let io = UnixStream::connect(socket)
        .await
        .map_err(|e| Error::Unreachable(e.to_string()))?;
    let resp = request_on_io(io, Method::GET, "docker", path, &[], None)
        .await
        .map_err(Error::Other)?;
    match resp.status().as_u16() {
        200..=299 => Ok(resp),
        404 => Err(Error::NotFound),
        s => {
            let body = read_body(resp.into_body(), 4096).await.unwrap_or_default();
            let msg = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("message")?.as_str().map(str::to_string))
                .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
            Err(Error::Other(format!("docker returned {s}: {msg}")))
        }
    }
}

/// `GET /containers/{name}/json`, reduced to what the logs need.
pub async fn inspect(socket: &str, name: &str) -> Result<Info, Error> {
    if !valid_name(name) {
        return Err(Error::NotFound);
    }
    let fut = async {
        let resp = get(socket, &format!("/containers/{name}/json")).await?;
        read_body(resp.into_body(), MAX_INSPECT)
            .await
            .map_err(Error::Other)
    };
    let body = tokio::time::timeout(INSPECT_TIMEOUT, fut)
        .await
        .map_err(|_| Error::Other("docker: inspect timed out".into()))??;
    parse_inspect(&body).ok_or_else(|| Error::Other("docker: bad inspect reply".into()))
}

pub fn parse_inspect(body: &[u8]) -> Option<Info> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let st = v.get("State")?;
    let time = |k: &str| {
        st.get(k)
            .and_then(|t| t.as_str())
            .and_then(super::frame::parse_rfc3339)
            .map(|ns| ns / 1_000_000_000)
            .filter(|&s| s > 0)
    };
    Some(Info {
        id: v.get("Id")?.as_str()?.to_string(),
        name: v
            .get("Name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string(),
        tty: v
            .pointer("/Config/Tty")
            .and_then(|t| t.as_bool())
            .unwrap_or(false),
        running: st.get("Running").and_then(|r| r.as_bool()).unwrap_or(false),
        status: st
            .get("Status")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        started: time("StartedAt"),
        finished: time("FinishedAt"),
        exit_code: st.get("ExitCode").and_then(|c| c.as_i64()),
    })
}

/// What part of the log to read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub follow: bool,
    /// Last N lines (None = all lines in the range).
    pub tail: Option<u32>,
    /// Unix seconds, fractional allowed (Docker accepts `secs.nanos`).
    pub since: Option<f64>,
    pub until: Option<f64>,
    /// Stop after this many lines (the server's own limit, not Docker's).
    /// Docker applies `tail` before `since` / `until`, so a closed window
    /// (both set) is read from its start with `tail=all` and cut here.
    pub cap: Option<u32>,
}

impl Query {
    pub fn path(&self, id: &str) -> String {
        let mut p = format!(
            "/containers/{id}/logs?stdout=1&stderr=1&timestamps=1&follow={}",
            self.follow as u8
        );
        match self.tail {
            Some(n) => p.push_str(&format!("&tail={n}")),
            None => p.push_str("&tail=all"),
        }
        if let Some(s) = self.since {
            p.push_str(&format!("&since={}", fmt_ts(s)));
        }
        if let Some(u) = self.until {
            p.push_str(&format!("&until={}", fmt_ts(u)));
        }
        p
    }
}

fn fmt_ts(t: f64) -> String {
    let t = t.max(0.0);
    let secs = t.trunc() as i64;
    let nanos = ((t - t.trunc()) * 1e9).round() as i64;
    format!("{secs}.{nanos:09}")
}

/// An open log stream.
pub struct Stream {
    body: Incoming,
    parser: Parser,
    done: bool,
}

/// A parsed line: unix nanoseconds (0 if Docker gave none), stream, text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub ts: i64,
    pub stream: u8,
    pub text: String,
    pub truncated: bool,
}

impl Line {
    pub fn from_raw(raw: RawLine) -> Self {
        let (ts, rest) = split_timestamp(&raw.bytes);
        Line {
            ts: ts.unwrap_or(0),
            stream: raw.stream,
            text: String::from_utf8_lossy(rest).into_owned(),
            truncated: raw.truncated,
        }
    }
}

pub async fn open(socket: &str, id: &str, tty: bool, q: &Query) -> Result<Stream, Error> {
    if !valid_name(id) {
        return Err(Error::NotFound);
    }
    let resp = tokio::time::timeout(INSPECT_TIMEOUT, get(socket, &q.path(id)))
        .await
        .map_err(|_| Error::Other("docker: opening the log timed out".into()))??;
    // Docker also tells us the framing in the content type.
    let raw = resp
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("raw-stream"));
    let multiplexed = resp
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("multiplexed-stream"));
    let tty = if multiplexed { false } else { tty || raw };
    Ok(Stream {
        body: resp.into_body(),
        parser: Parser::new(tty),
        done: false,
    })
}

impl Stream {
    /// Next batch of complete lines; empty Vec plus `false` at the end.
    /// Cancel-safe: dropping the future loses nothing already parsed.
    pub async fn next(&mut self, out: &mut Vec<RawLine>) -> bool {
        if self.done {
            return false;
        }
        let mut bytes = 0;
        loop {
            // Wait for the first frame; after that take only what is already
            // there (Docker sends one chunk per log line), so a burst becomes
            // one batch instead of one wake-up per line.
            let next = if out.is_empty() {
                Some(self.body.frame().await)
            } else {
                tokio::select! {
                    biased;
                    f = self.body.frame() => Some(f),
                    _ = std::future::ready(()) => None,
                }
            };
            match next {
                None => return true,
                Some(Some(Ok(f))) => {
                    if let Some(d) = f.data_ref() {
                        bytes += d.len();
                        self.parser.push(d, out);
                        if out.len() >= BATCH_LINES || bytes >= BATCH_BYTES {
                            return true;
                        }
                    }
                }
                Some(Some(Err(_)) | None) => {
                    self.parser.finish(out);
                    self.done = true;
                    return !out.is_empty();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("web-1"));
        assert!(valid_name("a.b_c"));
        assert!(!valid_name(""));
        assert!(!valid_name("-x"));
        assert!(!valid_name("../etc"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a?b"));
        assert!(!valid_name("<script>"));
    }

    #[test]
    fn query_paths() {
        let q = Query {
            follow: true,
            tail: Some(500),
            since: Some(1700000000.5),
            until: None,
            cap: None,
        };
        assert_eq!(
            q.path("abc"),
            "/containers/abc/logs?stdout=1&stderr=1&timestamps=1&follow=1&tail=500&since=1700000000.500000000"
        );
        let q = Query::default();
        assert!(q.path("x").ends_with("follow=0&tail=all"));
    }

    #[test]
    fn inspect_reply() {
        let body = br#"{"Id":"abc","Name":"/web","Config":{"Tty":true},
            "State":{"Status":"exited","Running":false,"ExitCode":137,
            "StartedAt":"2026-11-14T21:30:00.5Z","FinishedAt":"0001-01-01T00:00:00Z"}}"#;
        let i = parse_inspect(body).unwrap();
        assert_eq!(i.name, "web");
        assert!(i.tty);
        assert!(!i.running);
        assert_eq!(i.started, Some(1_794_691_800));
        assert_eq!(i.finished, None);
        assert_eq!(i.exit_code, Some(137));
    }
}
