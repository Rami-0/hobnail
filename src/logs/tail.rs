//! Live tail over SSE and plain-text downloads.
//!
//! One Docker log connection per container per viewer, opened when the
//! viewer's `EventSource` connects and closed when it goes away: the body
//! channel's receiver is dropped by hyper when the client disconnects, a
//! ping every [`PING`] notices a half-open connection, and every reader task
//! stops as soon as its channel is closed. Nothing is buffered beyond two
//! small bounded channels (8 batches of at most 64 KiB, then 4 events), so a fast log and
//! a slow browser means backpressure on Docker, never memory growth.

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Frame;
use hyper::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use hyper::Response;
use serde_json::json;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::client::{self, Info, Line, Query};
use super::frame::strip_ansi;
use super::level;
use crate::http::server::{set, sse_event, Body};
use crate::state::AppState;

/// Keep-alive comment; also how fast a dead client is noticed.
pub const PING: Duration = Duration::from_secs(2);
/// Lines per SSE event, and bytes per event (soft).
const BATCH_LINES: usize = 400;
const BATCH_BYTES: usize = 96 * 1024;
const LINE_QUEUE: usize = 8;
/// How long a batch may wait for more lines before it is sent.
const GATHER: Duration = Duration::from_millis(25);
const BODY_QUEUE: usize = 4;

/// A response body fed by a channel.
pub struct ChanBody {
    rx: mpsc::Receiver<Bytes>,
}

impl hyper::body::Body for ChanBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.rx.poll_recv(cx).map(|o| o.map(|b| Ok(Frame::data(b))))
    }
}

enum Msg {
    Meta(usize, Info),
    Lines(usize, Vec<Line>),
    Error(usize, String, &'static str),
    /// One container's stream ended; `true`: it stopped on the cap with
    /// more lines left in the window (the pane says so).
    End(usize, bool),
}

/// Number of open Docker log connections for viewers (for tests and /api/logs/status).
pub static OPEN_STREAMS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct OpenGuard;
impl OpenGuard {
    fn new() -> Self {
        OPEN_STREAMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        OpenGuard
    }
}
impl Drop for OpenGuard {
    fn drop(&mut self) {
        OPEN_STREAMS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// One container's reader: inspect, open, forward lines until the stream
/// ends or nobody listens any more.
async fn reader(socket: String, idx: usize, name: String, q: Query, tx: mpsc::Sender<Msg>) {
    let info = match client::inspect(&socket, &name).await {
        Ok(i) => i,
        Err(e) => {
            let kind = match e {
                client::Error::NotFound => "notfound",
                client::Error::Unreachable(_) => "unreachable",
                client::Error::Other(_) => "error",
            };
            let _ = tx.send(Msg::Error(idx, e.to_string(), kind)).await;
            return;
        }
    };
    let mut q = q;
    // "since container start" is resolved here, where we know the start.
    if q.since == Some(-1.0) {
        q.since = info.started.map(|s| s as f64);
    }
    // A stopped container has nothing to follow: show its last lines.
    q.follow = q.follow && info.running;
    if tx.send(Msg::Meta(idx, info.clone())).await.is_err() {
        return;
    }
    let mut stream = tokio::select! {
        r = client::open(&socket, &info.id, info.tty, &q) => match r {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(Msg::Error(idx, e.to_string(), "error")).await;
                return;
            }
        },
        _ = tx.closed() => return,
    };
    let _open = OpenGuard::new();
    let mut buf = Vec::new();
    let mut left = q.cap.map(|c| c as usize);
    loop {
        let more = tokio::select! {
            m = stream.next(&mut buf) => m,
            _ = tx.closed() => return,
        };
        let mut cut = left.is_some_and(|l| take_capped(&mut buf, l));
        if let Some(l) = left.as_mut() {
            *l -= buf.len();
        }
        if !buf.is_empty() {
            let lines: Vec<Line> = buf.drain(..).map(Line::from_raw).collect();
            if tx.send(Msg::Lines(idx, lines)).await.is_err() {
                return;
            }
        }
        if left == Some(0) && more && !cut {
            // Exactly at the cap: one more read tells a window that ends
            // here from one with more lines after.
            cut = tokio::select! {
                m = stream.next(&mut buf) => m || !buf.is_empty(),
                _ = tx.closed() => return,
            };
        }
        if !more || left == Some(0) {
            let _ = tx.send(Msg::End(idx, cut)).await;
            return;
        }
    }
}

/// Keeps at most `left` lines of `buf`; `true` when some were dropped (the
/// reader stopped on the cap).
fn take_capped<T>(buf: &mut Vec<T>, left: usize) -> bool {
    let cut = buf.len() > left;
    buf.truncate(left);
    cut
}

fn line_json(idx: usize, l: &Line) -> serde_json::Value {
    let plain = strip_ansi(&l.text);
    let lv = level::detect(&plain);
    let mut v = json!({
        "c": idx,
        "t": l.ts as f64 / 1e6,
        "m": l.text,
    });
    let o = v.as_object_mut().unwrap();
    if l.stream == 2 {
        o.insert("s".into(), json!(2));
    }
    if lv != level::Level::Other {
        o.insert("l".into(), json!(lv.code()));
    }
    if l.truncated {
        o.insert("x".into(), json!(1));
    }
    v
}

fn meta_json(idx: usize, i: &Info) -> String {
    json!({
        "c": idx, "name": i.name, "tty": i.tty, "running": i.running,
        "status": i.status, "started": i.started, "finished": i.finished,
        "exit": i.exit_code,
    })
    .to_string()
}

/// `GET /api/logs/stream`: SSE with events `meta`, `l` (array of lines),
/// `err`, `end` (one container's stream ended) and `done` (all ended).
pub fn sse(state: &Arc<AppState>, names: Vec<String>, q: Query) -> Response<Body> {
    let (tx, rx) = mpsc::channel::<Bytes>(BODY_QUEUE);
    let socket = state.env.docker_socket.clone();
    tokio::spawn(async move {
        let (ltx, mut lrx) = mpsc::channel::<Msg>(LINE_QUEUE);
        for (i, n) in names.iter().enumerate() {
            tokio::spawn(reader(socket.clone(), i, n.clone(), q.clone(), ltx.clone()));
        }
        drop(ltx);
        if tx.send(sse_event("hello", "")).await.is_err() {
            return;
        }
        let mut ping = tokio::time::interval(PING);
        ping.tick().await;
        let mut batch = String::new();
        loop {
            let first = tokio::select! {
                m = lrx.recv() => m,
                _ = ping.tick() => {
                    if tx.send(Bytes::from_static(b": p\n\n")).await.is_err() {
                        return;
                    }
                    continue;
                }
                _ = tx.closed() => return,
            };
            let Some(first) = first else {
                let _ = tx.send(sse_event("done", "")).await;
                return;
            };
            // Gather what is already queued into one event.
            let gather_until = Instant::now() + GATHER;
            let mut next = Some(first);
            let mut n = 0;
            batch.clear();
            while let Some(m) = next.take() {
                let ev = match m {
                    Msg::Lines(i, ls) => {
                        for l in &ls {
                            batch.push(if n == 0 { '[' } else { ',' });
                            batch.push_str(&line_json(i, l).to_string());
                            n += 1;
                        }
                        None
                    }
                    Msg::Meta(i, info) => Some(sse_event("meta", &meta_json(i, &info))),
                    Msg::Error(i, e, kind) => Some(sse_event(
                        "err",
                        &json!({"c": i, "msg": e, "kind": kind}).to_string(),
                    )),
                    Msg::End(i, cut) => Some(sse_event(
                        "end",
                        &if cut {
                            json!({"c": i, "cut": true})
                        } else {
                            json!({"c": i})
                        }
                        .to_string(),
                    )),
                };
                if let Some(ev) = ev {
                    // Keep order: flush the lines gathered so far first.
                    if n > 0 {
                        batch.push(']');
                        if tx.send(sse_event("l", &batch)).await.is_err() {
                            return;
                        }
                        batch.clear();
                        n = 0;
                    }
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                }
                if n < BATCH_LINES && batch.len() < BATCH_BYTES {
                    next = lrx.try_recv().ok();
                    // A trickle: wait a moment for more rather than send
                    // one tiny event per line.
                    if next.is_none() && n > 0 && Instant::now() < gather_until {
                        next = tokio::time::timeout_at(gather_until, lrx.recv())
                            .await
                            .ok()
                            .flatten();
                    }
                }
            }
            if n > 0 {
                batch.push(']');
                if tx.send(sse_event("l", &batch)).await.is_err() {
                    return;
                }
            }
            if batch.capacity() > 4 * BATCH_BYTES {
                batch = String::new();
            }
        }
    });
    let mut r = Response::new(ChanBody { rx }.boxed_unsync());
    set(&mut r, CONTENT_TYPE, "text/event-stream");
    set(&mut r, CACHE_CONTROL, "no-cache");
    // Tell reverse proxies not to buffer the stream.
    set(
        &mut r,
        hyper::header::HeaderName::from_static("x-accel-buffering"),
        "no",
    );
    r
}

/// `GET /api/logs/download`: the requested range as plain text (ANSI
/// removed), streamed; one container after the other for several.
pub fn download(
    state: &Arc<AppState>,
    names: Vec<String>,
    q: Query,
    filename: &str,
) -> Response<Body> {
    let (tx, rx) = mpsc::channel::<Bytes>(BODY_QUEUE);
    let socket = state.env.docker_socket.clone();
    let several = names.len() > 1;
    tokio::spawn(async move {
        for name in names {
            let (ltx, mut lrx) = mpsc::channel::<Msg>(LINE_QUEUE);
            let mut q = q.clone();
            q.follow = false;
            tokio::spawn(reader(socket.clone(), 0, name.clone(), q, ltx));
            let mut chunk = String::new();
            while let Some(m) = lrx.recv().await {
                match m {
                    Msg::Lines(_, ls) => {
                        for l in ls {
                            if l.ts > 0 {
                                chunk.push_str(&iso_ms(l.ts));
                                chunk.push(' ');
                            }
                            if several {
                                chunk.push_str(&name);
                                chunk.push(' ');
                            }
                            if l.stream == 2 {
                                chunk.push_str("stderr ");
                            }
                            chunk.push_str(&strip_ansi(&l.text));
                            if l.truncated {
                                chunk.push_str(" [line truncated]");
                            }
                            chunk.push('\n');
                        }
                    }
                    Msg::Error(_, e, _) => {
                        chunk.push_str(&format!("# {name}: {e}\n"));
                    }
                    _ => {}
                }
                let flush = chunk.len() >= 64 * 1024 || (lrx.is_empty() && !chunk.is_empty());
                if flush
                    && tx
                        .send(Bytes::from(std::mem::take(&mut chunk)))
                        .await
                        .is_err()
                {
                    return;
                }
            }
            if !chunk.is_empty() && tx.send(Bytes::from(chunk)).await.is_err() {
                return;
            }
        }
    });
    let mut r = Response::new(ChanBody { rx }.boxed_unsync());
    set(&mut r, CONTENT_TYPE, "text/plain; charset=utf-8");
    set(&mut r, CACHE_CONTROL, "no-store");
    set(
        &mut r,
        CONTENT_DISPOSITION,
        &format!("attachment; filename=\"{filename}\""),
    );
    r
}

/// Unix nanoseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso_ms(ns: i64) -> String {
    let secs = ns.div_euclid(1_000_000_000).max(0) as u64;
    let ms = ns.rem_euclid(1_000_000_000) / 1_000_000;
    let s = crate::util::fmt_utc(secs);
    format!("{}.{ms:03}Z", &s[..s.len() - 1])
}

#[cfg(test)]
mod cap_tests {
    #[test]
    fn take_capped_says_when_it_dropped_lines() {
        let mut b = vec![1, 2, 3];
        assert!(!super::take_capped(&mut b, 3));
        assert_eq!(b, [1, 2, 3]);
        assert!(!super::take_capped(&mut b, 5));
        assert!(super::take_capped(&mut b, 2));
        assert_eq!(b, [1, 2]);
        let mut e: Vec<u8> = vec![];
        assert!(!super::take_capped(&mut e, 0));
    }
}
