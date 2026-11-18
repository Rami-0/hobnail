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
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::config::HttpPolicy;

/// Largest response body we read (widgets, notification replies).
pub const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
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

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Bytes,
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
    let timeout = req.timeout;
    match tokio::time::timeout(timeout, send_inner(req, policy)).await {
        Ok(r) => r,
        Err(_) => Err(format!("timed out after {}ms", timeout.as_millis())),
    }
}

/// Canonical origin for a dashboard connection URL. Credentials belong in the
/// separate credential field, so URLs with user information or queries are rejected.
pub(crate) fn origin(url: &str) -> Option<String> {
    if url.len() > 2048 || url.contains(['#', '?', '\\']) {
        return None;
    }
    let uri: hyper::Uri = url.parse().ok()?;
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

/// One request to the saved destination. Redirects are terminal responses.
/// Recheck the actual request URI before a credential can become a header.
pub(crate) async fn send_bound(
    url: String,
    credential: Option<(String, String)>,
    policy: &HttpPolicy,
) -> Result<Response, String> {
    let destination = origin(&url).ok_or("blocked invalid destination")?;
    let mut req = Request::get(url);
    if let Some((bound_origin, secret)) = credential {
        if destination != bound_origin {
            return Err("blocked credential destination".into());
        }
        req = req.header("authorization", format!("Bearer {secret}"));
    }
    send(req, policy).await
}

async fn send_inner(req: Request, policy: &HttpPolicy) -> Result<Response, String> {
    let uri: hyper::Uri = req.url.parse().map_err(|_| "invalid URL".to_string())?;
    let scheme = uri.scheme_str().unwrap_or("http").to_ascii_lowercase();
    let host = uri.host().ok_or("URL has no host")?.to_string();
    let host_bare = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if !policy.allows(&host_bare) {
        return Err(format!("host {host_bare} is not in http.allow_hosts"));
    }
    let tls = match scheme.as_str() {
        "http" => false,
        "https" => true,
        s => return Err(format!("unsupported scheme {s}")),
    };
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let host_header = match uri.port_u16() {
        Some(p) => format!("{host}:{p}"),
        None => host.clone(),
    };
    let path = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());

    let addresses: Vec<_> = tokio::net::lookup_host((host_bare.as_str(), port))
        .await
        .map_err(|e| format!("resolve {host_bare}: {e}"))?
        .collect();
    if addresses.is_empty() {
        return Err(format!("resolve {host_bare}: no addresses"));
    }
    let tcp = TcpStream::connect(addresses.as_slice())
        .await
        .map_err(|e| format!("connect {host_bare}:{port}: {e}"))?;
    let _ = tcp.set_nodelay(true);

    let resp = if tls {
        let server_name = rustls::pki_types::ServerName::try_from(host_bare.clone())
            .map_err(|_| format!("invalid TLS server name {host_bare}"))?;
        let stream = tokio_rustls::TlsConnector::from(tls_config())
            .connect(server_name, tcp)
            .await
            .map_err(|e| format!("TLS handshake with {host_bare}: {e}"))?;
        request_on_io(
            stream,
            req.method,
            &host_header,
            &path,
            &req.headers,
            req.body,
        )
        .await?
    } else {
        request_on_io(tcp, req.method, &host_header, &path, &req.headers, req.body).await?
    };
    let status = resp.status().as_u16();
    let body = read_body(resp.into_body(), MAX_BODY).await?;
    Ok(Response { status, body })
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
    let mut b = HRequest::builder()
        .method(method)
        .uri(path_and_query)
        .header("host", host_header)
        .header("user-agent", concat!("hobnail/", env!("CARGO_PKG_VERSION")))
        .header("accept", "application/json, text/plain, */*");
    for (k, v) in headers {
        b = b.header(k.as_str(), v.as_str());
    }
    let req = b
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|e| format!("bad request: {e}"))?;
    sender
        .send_request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))
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

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CFG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("rustls protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        Arc::new(cfg)
    })
    .clone()
}
