//! Connection guards beyond hyper's own: a whole-body read deadline, a
//! write-stall timeout and a per-peer connection limit. Together with the
//! header timeout and the global cap (see [`super::server::Limits`]) they
//! keep a few slow or silent peers from holding every connection slot.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

/// When the request body must have arrived in full, set per request from
/// the moment its head was read.
#[derive(Clone, Copy)]
pub(crate) struct BodyDeadline(pub Instant);

pub(crate) enum BodyError {
    /// Over the limit, or the client went away.
    TooLarge,
    /// The whole body did not arrive before the deadline (answer 408).
    TimedOut,
}

/// Read a request body of at most `limit` bytes before its deadline (the
/// default request deadline when the request carries none).
pub(crate) async fn collect_body(
    deadline: Option<&BodyDeadline>,
    body: Incoming,
    limit: usize,
) -> Result<Bytes, BodyError> {
    let at = deadline.map_or_else(
        || Instant::now() + super::server::LIMITS.body_timeout,
        |d| d.0,
    );
    match tokio::time::timeout_at(at, Limited::new(body, limit).collect()).await {
        Err(_) => Err(BodyError::TimedOut),
        Ok(Err(_)) => Err(BodyError::TooLarge),
        Ok(Ok(collected)) => Ok(collected.to_bytes()),
    }
}

/// Paths whose responses may stream, so runtime gzip leaves them alone.
/// Only the response decides the write-stall exemption ([`Streaming`]).
pub(crate) fn streams(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/stream" | "/api/logs/stream" | "/api/logs/download"
    )
}

/// Response extension set by a handler that returns a body streaming for as
/// long as the client stays (a signed-in event stream, log stream or log
/// download). Such a body keeps its own keepalive and send deadline, so its
/// writes may stall. Every other response, including a 401 or 405 on the
/// same path, keeps the stall timer: an exemption taken from the request
/// path let a client that never reads pin its connection forever.
#[derive(Clone, Copy)]
pub(crate) struct Streaming;

/// Marks `response` as streaming (see [`Streaming`]).
pub(crate) fn mark_streaming<B>(response: &mut hyper::Response<B>) {
    response.extensions_mut().insert(Streaming);
}

/// Whether the write-stall timer is off for this response: only a 200 that a
/// streaming handler marked.
pub(crate) fn exempt_from_stall<B>(response: &hyper::Response<B>) -> bool {
    response.status() == hyper::StatusCode::OK && response.extensions().get::<Streaming>().is_some()
}

/// A socket whose writes fail after `limit` without progress, unless the
/// current response streams (`exempt`). A client that stops reading a
/// response cannot hold its connection slot.
pub(crate) struct Timed<T> {
    io: T,
    limit: Duration,
    exempt: Arc<AtomicBool>,
    stall: Option<Pin<Box<Sleep>>>,
}

impl<T> Timed<T> {
    pub(crate) fn new(io: T, limit: Duration, exempt: Arc<AtomicBool>) -> Self {
        Self {
            io,
            limit,
            exempt,
            stall: None,
        }
    }

    fn watch<R>(
        &mut self,
        cx: &mut Context<'_>,
        result: Poll<io::Result<R>>,
    ) -> Poll<io::Result<R>> {
        if result.is_ready() || self.exempt.load(Ordering::Relaxed) {
            self.stall = None;
            return result;
        }
        let limit = self.limit;
        let stall = self
            .stall
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(limit)));
        if stall.as_mut().poll(cx).is_ready() {
            self.stall = None;
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response write stalled",
            )));
        }
        Poll::Pending
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Timed<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Timed<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_write(cx, buf);
        this.watch(cx, result)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_write_vectored(cx, bufs);
        this.watch(cx, result)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_flush(cx);
        this.watch(cx, result)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_shutdown(cx);
        this.watch(cx, result)
    }
}

/// Open connections per peer. IPv4 peers count by address; IPv6 peers by
/// their /64, since one host usually holds a whole /64.
pub(crate) struct PeerSlots {
    limit: usize,
    open: Mutex<HashMap<IpAddr, usize>>,
}

/// One counted connection; frees its slot when dropped.
pub(crate) struct PeerSlot {
    slots: Arc<PeerSlots>,
    key: IpAddr,
}

impl PeerSlots {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            open: Mutex::default(),
        })
    }

    /// A slot for `ip`, or `None` when it already has `limit` connections.
    pub(crate) fn take(self: &Arc<Self>, ip: IpAddr) -> Option<PeerSlot> {
        let key = peer_key(ip);
        let mut open = self.open.lock().unwrap();
        let count = open.entry(key).or_default();
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(PeerSlot {
            slots: self.clone(),
            key,
        })
    }
}

impl Drop for PeerSlot {
    fn drop(&mut self) {
        let mut open = self.slots.open.lock().unwrap();
        if let Some(count) = open.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                open.remove(&self.key);
            }
        }
    }
}

fn peer_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.into(),
            None => {
                let s = v6.segments();
                std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0).into()
            }
        },
    }
}

/// What a peer over its limit gets before the connection closes: written
/// once without waiting, so a full socket just drops it.
pub(crate) const PEER_BUSY: &[u8] = b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Type: text/plain\r\nContent-Length: 35\r\nConnection: close\r\n\r\ntoo many connections from this host";

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn a_stalled_write_fails_unless_the_response_streams() {
        let exempt = Arc::new(AtomicBool::new(false));
        let (near, _far) = tokio::io::duplex(64);
        let mut timed = Timed::new(near, Duration::from_millis(100), exempt.clone());
        let error = tokio::time::timeout(Duration::from_secs(2), timed.write_all(&[0; 4096]))
            .await
            .expect("the stall timeout fires")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        exempt.store(true, Ordering::Relaxed);
        let (near, _far) = tokio::io::duplex(64);
        let mut streaming = Timed::new(near, Duration::from_millis(100), exempt);
        assert!(
            tokio::time::timeout(Duration::from_millis(400), streaming.write_all(&[0; 4096]))
                .await
                .is_err(),
            "an exempt write waits for the reader"
        );
    }

    #[test]
    fn only_a_marked_200_is_exempt_from_the_stall_timer() {
        let mut streaming = hyper::Response::new(());
        assert!(!exempt_from_stall(&streaming), "unmarked");
        mark_streaming(&mut streaming);
        assert!(exempt_from_stall(&streaming));
        *streaming.status_mut() = hyper::StatusCode::UNAUTHORIZED;
        assert!(!exempt_from_stall(&streaming), "a refusal keeps the timer");
    }

    #[test]
    fn peers_count_by_address_and_ipv6_by_64() {
        let slots = PeerSlots::new(2);
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        let first = slots.take(a).unwrap();
        let _second = slots.take(b).unwrap();
        assert!(slots.take(a).is_none(), "same /64");
        assert!(slots.take(other).is_some());
        drop(first);
        assert!(slots.take(b).is_some());
        let v4: IpAddr = "192.0.2.1".parse().unwrap();
        let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        let _x = slots.take(v4).unwrap();
        let _y = slots.take(mapped).unwrap();
        assert!(slots.take(v4).is_none());
        assert_eq!(
            PEER_BUSY.split(|&b| b == b'\n').next_back().unwrap().len(),
            35
        );
    }
}
