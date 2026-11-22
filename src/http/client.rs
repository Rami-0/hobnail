//! Minimal outbound HTTP/1.1 client on hyper's low-level `conn` API.
//!
//! One connection per request, no pool: widgets and notifications fire every
//! few seconds at most, and a pool would keep sockets and buffers alive while
//! nobody is looking. TLS is rustls (ring) with the webpki root set compiled
//! in, so the image needs no CA bundle. The TLS config is built lazily on the
//! first https request, so a setup with only local http targets never pays
//! for the root store.
//!
//! `request_on_io` is also used by the Docker client over the unix socket.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request as HRequest, Response as HResponse};
use hyper_util::rt::TokioIo;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::pin::{self, Fingerprint, Leaf};
use crate::config::HttpPolicy;

/// Largest response body we read (widgets, notification replies).
pub const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            url: url.into(),
            headers: vec![],
            body: None,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn post_json(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: Method::POST,
            url: url.into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: Some(body),
            timeout: Duration::from_secs(10),
        }
    }

    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
}

#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub body: Bytes,
    pub headers: hyper::HeaderMap,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Send a request. Errors are human-readable and never include the URL's
/// path or query (which may carry a secret such as a webhook token).
pub async fn send(req: Request, policy: &HttpPolicy) -> Result<Response, String> {
    send_with_policy(req, policy, None).await
}

async fn send_with_policy(
    req: Request,
    policy: &HttpPolicy,
    integration: Option<IntegrationPolicy<'_>>,
) -> Result<Response, String> {
    let timeout = req.timeout;
    match tokio::time::timeout(timeout, send_inner(req, policy, integration)).await {
        Ok(r) => r,
        Err(_) => Err(format!("timed out after {}ms", timeout.as_millis())),
    }
}

// Evaluate the saved path before appending any provider endpoint. Reject
// escaped percent signs as well, since another proxy can decode them again.
fn safe_base_path(path: &str) -> bool {
    path.split('/').all(|segment| {
        let mut decoded = Vec::with_capacity(segment.len());
        let mut bytes = segment.as_bytes().iter().copied();
        while let Some(byte) = bytes.next() {
            let byte = if byte == b'%' {
                let Some(high) = bytes.next().and_then(|b| char::from(b).to_digit(16)) else {
                    return false;
                };
                let Some(low) = bytes.next().and_then(|b| char::from(b).to_digit(16)) else {
                    return false;
                };
                (high * 16 + low) as u8
            } else {
                byte
            };
            if matches!(byte, b'/' | b'\\' | b'%') || byte.is_ascii_control() {
                return false;
            }
            decoded.push(byte);
        }
        !matches!(decoded.as_slice(), b"." | b"..")
    })
}

/// Canonical origin for a dashboard connection URL. Credentials belong in the
/// separate credential field, so URLs with user information or queries are rejected.
pub(crate) fn origin(url: &str) -> Option<String> {
    if url.len() > 2048 || url.contains(['#', '?', '\\']) {
        return None;
    }
    let uri: hyper::Uri = url.parse().ok()?;
    if !safe_base_path(uri.path()) {
        return None;
    }
    let scheme = uri.scheme_str()?;
    let default = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let authority = uri.authority()?;
    let host = authority.host();
    let port = uri.port_u16().unwrap_or(default);
    if host.is_empty()
        || authority.as_str().contains(['@', '%'])
        || port == 0
        || (authority.as_str() != host && uri.port_u16().is_none())
    {
        return None;
    }
    let host = host.to_ascii_lowercase();
    Some(if port == default {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    })
}

#[derive(Clone)]
pub(crate) struct BoundCredential {
    pub origin: String,
    pub token: String,
}

impl fmt::Debug for BoundCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BoundCredential([redacted])")
    }
}

/// Receives a streamed response body one chunk at a time. Return `Err` to
/// stop reading; the request then fails as an unexpected response.
pub(crate) type BodySink<'a> = &'a mut (dyn FnMut(&[u8]) -> Result<(), ()> + Send);

/// How a bound request reads a successful response body.
#[derive(Default)]
pub(crate) enum BodyMode<'a> {
    /// Collect up to `max_body` bytes; a larger body is an error.
    #[default]
    Collect,
    /// Pass each chunk to the sink and drop it, so memory stays at one
    /// chunk (bounded by the connection's read buffer) however long the
    /// body is. `limit` caps the total bytes read. A non-2xx response is
    /// collected as usual instead, so its status and body can be inspected.
    Scan { sink: BodySink<'a>, limit: usize },
}

#[derive(Default)]
pub(crate) struct BoundPolicy<'a> {
    pub headers: Vec<hyper::header::HeaderName>,
    pub revision: crate::providers::Revision,
    pub pin: Option<Fingerprint>,
    pub local_only: bool,
    pub max_body: Option<usize>,
    pub status_only: bool,
    pub body: BodyMode<'a>,
}

struct IntegrationPolicy<'a> {
    headers: Vec<hyper::header::HeaderName>,
    revision: crate::providers::Revision,
    tls: Option<Arc<rustls::ClientConfig>>,
    local_only: bool,
    max_body: usize,
    status_only: bool,
    body: BodyMode<'a>,
}

#[derive(Debug)]
pub(crate) struct BoundError {
    pub message: String,
    pub fingerprint: Option<String>,
}

/// One request to the saved destination. Redirects are terminal responses.
/// Recheck the actual request URI before a credential can become a header.
#[cfg(test)]
pub(crate) async fn send_bound(
    url: String,
    credential: Option<BoundCredential>,
    policy: &HttpPolicy,
    options: BoundPolicy<'_>,
) -> Result<Response, BoundError> {
    let blocked = |message: &str| BoundError {
        message: message.into(),
        fingerprint: None,
    };
    let destination = origin(&url).ok_or_else(|| blocked("blocked invalid destination"))?;
    let mut req = Request::get(url);
    if let Some(BoundCredential {
        origin: bound_origin,
        token,
    }) = credential
    {
        if destination != bound_origin {
            return Err(blocked("blocked credential destination"));
        }
        req = req.header("authorization", format!("Bearer {token}"));
    }
    send_step(req, &destination, policy, options).await
}

pub(crate) async fn send_step(
    req: Request,
    bound_origin: &str,
    policy: &HttpPolicy,
    options: BoundPolicy<'_>,
) -> Result<Response, BoundError> {
    let blocked = |message: &str| BoundError {
        message: message.into(),
        fingerprint: None,
    };
    if origin(req.url.split('?').next().unwrap_or_default()).as_deref() != Some(bound_origin) {
        return Err(blocked("blocked credential destination"));
    }
    let (leaf, tls) = if bound_origin.starts_with("https:") {
        let (leaf, tls) = Leaf::configure(options.pin, roots());
        (Some(leaf), Some(tls))
    } else if options.pin.is_some() {
        return Err(blocked("invalid TLS pin for HTTP"));
    } else {
        (None, None)
    };
    let integration = IntegrationPolicy {
        headers: options.headers,
        revision: options.revision,
        tls,
        local_only: options.local_only,
        max_body: options.max_body.unwrap_or(MAX_BODY).min(MAX_BODY),
        status_only: options.status_only,
        body: options.body,
    };
    send_with_policy(req, policy, Some(integration))
        .await
        .map_err(|message| {
            let fingerprint = message
                .starts_with("TLS")
                .then(|| leaf.as_ref()?.presented.get().map(pin::display))
                .flatten();
            BoundError {
                message,
                fingerprint,
            }
        })
}

/// Whether `ip` is a LAN-side address for the provider internet switch:
/// RFC 1918, loopback, link-local, unique-local and the 100.64.0.0/10
/// shared space (CGNAT, where Tailscale addresses its nodes). 100.100.100.200
/// sits in that range but is a cloud metadata endpoint, so it is not local.
fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            let shared = a == 100 && b & 0xc0 == 64 && ip != Ipv4Addr::new(100, 100, 100, 200);
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || shared
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => local_ip(ip.into()),
            None => ip.is_unique_local() || ip.is_loopback() || ip.is_unicast_link_local(),
        },
    }
}

fn restricted_integration_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || [
                    Ipv4Addr::new(100, 100, 100, 200),
                    Ipv4Addr::new(168, 63, 129, 16),
                    Ipv4Addr::new(192, 0, 0, 192),
                ]
                .contains(&ip)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4() {
                return restricted_integration_ip(v4.into());
            }
            let segments = ip.segments();
            ip.is_unicast_link_local() || ip.is_multicast()
                || segments[0] == 0x2002 // 6to4 can embed restricted IPv4 addresses.
                || segments[..2] == [0x64, 0xff9b] // Well-known NAT64 prefixes.
                || ip == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254)
        }
    }
}

const MAX_BOUND_ADDRESSES: usize = 64;

pub(super) struct Destination<'a> {
    pub host: &'a str,
    pub port: u16,
    pub tls: bool,
}

impl<'a> Destination<'a> {
    pub fn parse(uri: &'a hyper::Uri) -> Result<Self, String> {
        let host = uri
            .host()
            .ok_or("URL has no host")?
            .trim_matches(['[', ']']);
        let tls = match uri.scheme_str().unwrap_or("http") {
            scheme if scheme.eq_ignore_ascii_case("http") => false,
            scheme if scheme.eq_ignore_ascii_case("https") => true,
            scheme => return Err(format!("unsupported scheme {scheme}")),
        };
        Ok(Self {
            host,
            tls,
            port: uri.port_u16().unwrap_or(if tls { 443 } else { 80 }),
        })
    }

    pub async fn resolve(
        &self,
        policy: &HttpPolicy,
        bound: Option<(&crate::providers::Revision, bool)>,
    ) -> Result<Vec<SocketAddr>, String> {
        let Self { host, port, .. } = *self;
        if !policy.allows(host) {
            return Err(format!("host {host} is not in http.allow_hosts"));
        }
        let resolved = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| format!("resolve {host}: {e}"))?;
        let addresses: Vec<_> = match bound {
            Some(_) => resolved.take(MAX_BOUND_ADDRESSES + 1).collect(),
            None => resolved.collect(),
        };
        if addresses.is_empty() {
            return Err(format!("resolve {host}: no addresses"));
        }
        if let Some((revision, local_only)) = bound {
            if addresses.len() > MAX_BOUND_ADDRESSES {
                return Err("response too large: too many resolved addresses".into());
            }
            if policy.allow_hosts.is_empty()
                && addresses
                    .iter()
                    .any(|addr| restricted_integration_ip(addr.ip()))
            {
                return Err("blocked integration address".into());
            }
            if local_only && addresses.iter().any(|addr| !local_ip(addr.ip())) {
                return Err("blocked by public internet setting".into());
            }
            revision.check()?;
        }
        Ok(addresses)
    }
}

async fn send_inner(
    req: Request,
    policy: &HttpPolicy,
    integration: Option<IntegrationPolicy<'_>>,
) -> Result<Response, String> {
    let uri: hyper::Uri = req.url.parse().map_err(|_| "invalid URL".to_string())?;
    let destination = Destination::parse(&uri)?;
    let Destination {
        host: host_bare,
        port,
        tls,
    } = destination;
    let host = uri.host().ok_or("URL has no host")?;
    let host_header = match uri.port_u16() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_owned(),
    };
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    let addresses = destination
        .resolve(
            policy,
            integration
                .as_ref()
                .map(|policy| (&policy.revision, policy.local_only)),
        )
        .await?;
    // Connect using the exact addresses that passed the policy check.
    let tcp = TcpStream::connect(addresses.as_slice())
        .await
        .map_err(|e| format!("connect {host_bare}:{port}: {e}"))?;
    let _ = tcp.set_nodelay(true);

    let request = http_request(req.method, &host_header, path, &req.headers, req.body)?;
    if tls {
        let server_name = rustls::pki_types::ServerName::try_from(host_bare.to_owned())
            .map_err(|_| format!("invalid TLS server name {host_bare}"))?;
        let stream = tokio_rustls::TlsConnector::from(
            integration
                .as_ref()
                .and_then(|policy| policy.tls.clone())
                .unwrap_or_else(tls_config),
        )
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("TLS handshake with {host_bare}: {e}"))?;
        exchange(stream, request, integration).await
    } else {
        exchange(tcp, request, integration).await
    }
}

async fn exchange(
    io: impl AsyncRead + AsyncWrite + Unpin,
    request: HRequest<Full<Bytes>>,
    policy: Option<IntegrationPolicy<'_>>,
) -> Result<Response, String> {
    let mut builder = hyper::client::conn::http1::Builder::new();
    if policy.is_some() {
        builder.max_buf_size(32 * 1024);
    }
    let (mut sender, mut driver) = builder
        .handshake(TokioIo::new(io))
        .await
        .map_err(|e| format!("http handshake: {e}"))?;
    if let Some(policy) = &policy {
        policy.revision.check()?;
    }
    let response = async {
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| "request failed")?;
        let status = response.status().as_u16();
        let mut headers = hyper::HeaderMap::new();
        if let Some(policy) = &policy {
            for name in &policy.headers {
                for value in response.headers().get_all(name) {
                    headers.append(name, value.clone());
                }
            }
        }
        let body = match policy {
            Some(IntegrationPolicy {
                status_only: true, ..
            }) => Bytes::new(),
            Some(IntegrationPolicy {
                body: BodyMode::Scan { sink, limit },
                ..
            }) if response.status().is_success() => {
                scan_body(response.into_body(), limit, sink).await?;
                Bytes::new()
            }
            policy => {
                read_body(
                    response.into_body(),
                    policy.map_or(MAX_BODY, |p| p.max_body),
                )
                .await?
            }
        };
        Ok(Response {
            status,
            body,
            headers,
        })
    };
    tokio::pin!(response);
    tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut driver => {
            result.map_err(|_| "HTTP connection failed")?;
            response.await
        }
    }
}

/// Perform one HTTP/1.1 request over any byte stream (TCP, TLS, unix socket)
/// and return the streaming response.
pub async fn request_on_io<S>(
    io: S,
    method: Method,
    host_header: &str,
    path_and_query: &str,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<HResponse<Incoming>, String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io))
        .await
        .map_err(|e| format!("http handshake: {e}"))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = http_request(method, host_header, path_and_query, headers, body)?;
    sender
        .send_request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))
}

fn http_request(
    method: Method,
    host_header: &str,
    path_and_query: &str,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<HRequest<Full<Bytes>>, String> {
    let mut b = HRequest::builder()
        .method(method)
        .uri(path_and_query)
        .header("host", host_header)
        .header("user-agent", concat!("hobnail/", env!("CARGO_PKG_VERSION")));
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
    {
        b = b.header("accept", "application/json, text/plain, */*");
    }
    for (k, v) in headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|_| "bad request".into())
}

pub async fn read_body(body: Incoming, limit: usize) -> Result<Bytes, String> {
    Limited::new(body, limit)
        .collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| {
            if e.is::<http_body_util::LengthLimitError>() {
                "response too large".into()
            } else {
                format!("reading body: {e}")
            }
        })
}

/// Feed a body to `sink` chunk by chunk without retaining it.
async fn scan_body(mut body: Incoming, limit: usize, sink: BodySink<'_>) -> Result<(), String> {
    let mut total = 0usize;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| format!("reading body: {e}"))?;
        let Ok(data) = frame.into_data() else {
            continue; // Trailers carry no body bytes.
        };
        total = total.saturating_add(data.len());
        if total > limit {
            return Err("response too large".into());
        }
        sink(&data).map_err(|()| "unexpected streamed body".to_string())?;
    }
    Ok(())
}

pub(super) fn roots() -> Arc<rustls::RootCertStore> {
    static ROOTS: OnceLock<Arc<rustls::RootCertStore>> = OnceLock::new();
    ROOTS
        .get_or_init(|| {
            Arc::new(rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            })
        })
        .clone()
}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CFG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("rustls protocol versions")
        .with_root_certificates(roots())
        .with_no_client_auth();
        Arc::new(cfg)
    })
    .clone()
}

#[cfg(test)]
mod integration_policy_tests {
    use super::{local_ip, restricted_integration_ip};

    #[test]
    fn shared_address_space_counts_as_local_except_the_metadata_address() {
        for ip in [
            "10.0.0.2",
            "192.168.1.1",
            "127.0.0.1",
            "100.64.0.1",
            "100.101.102.103",
            "100.127.255.254",
            "::ffff:100.80.0.1",
            "fd7a:115c:a1e0::1",
        ] {
            assert!(local_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "100.63.255.255",
            "100.128.0.0",
            "100.100.100.200",
            "::ffff:100.100.100.200",
            "8.8.8.8",
            "2001:db8::1",
        ] {
            assert!(!local_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn restrict_special_destinations_but_allow_private_services() {
        for ip in [
            "0.0.0.0",
            "::",
            "127.0.0.1",
            "::1",
            "169.254.169.254",
            "::ffff:169.254.169.254",
            "fe80::1",
            "224.0.0.1",
            "ff02::1",
            "100.100.100.200",
            "168.63.129.16",
            "192.0.0.192",
            "fd00:ec2::254",
            "64:ff9b::a9fe:a9fe",
            "2002:a9fe:a9fe::1",
        ] {
            assert!(restricted_integration_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "10.0.0.2",
            "172.16.1.1",
            "192.168.1.1",
            "fd01::1",
            "8.8.8.8",
        ] {
            assert!(!restricted_integration_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
