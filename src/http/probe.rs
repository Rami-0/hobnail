//! Credential-free probes for the provider connection contract.

use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use tokio::net::TcpStream;

use super::{
    client::{self, BoundError, Destination},
    pin::Leaf,
};
use crate::{
    config::HttpPolicy,
    providers::{Class, Fault, Revision},
};

mod certificate;

#[derive(Debug)]
#[allow(dead_code)] // Result fields consumed by reachability and expiry providers.
pub struct TcpProbe {
    pub reachable: bool,
    /// Connect time, excluding name resolution.
    pub latency: Duration,
}

#[derive(Debug)]
#[allow(dead_code)] // Result fields consumed by reachability and expiry providers.
pub struct HttpProbe {
    pub status: u16,
    pub latency: Duration,
}

#[derive(Debug, PartialEq)]
pub enum TlsVerification {
    Trusted,
    Pinned,
    PinMismatch,
    Rejected(rustls::Error),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SubjectAltName {
    Dns(String),
    Ip(IpAddr),
    Email(String),
    Uri(String),
    DirectoryName(String),
    RegisteredId(String),
    Other(u8),
}

#[derive(Debug)]
#[allow(dead_code)] // Result fields consumed by reachability and expiry providers.
pub struct TlsLeaf {
    pub subject: String,
    pub issuer: String,
    /// Unix seconds.
    pub not_before: i64,
    /// Unix seconds.
    pub not_after: i64,
    pub sans: Vec<SubjectAltName>,
    pub name_matches: bool,
    pub fingerprint: [u8; 32],
    pub verification: TlsVerification,
}

pub(crate) struct ProbeTarget<'a> {
    pub url: &'a str,
    pub policy: &'a HttpPolicy,
    pub revision: &'a Revision,
    pub local_only: bool,
}

impl ProbeTarget<'_> {
    async fn resolve(&self, https: bool) -> Result<(String, Vec<SocketAddr>), Fault> {
        let uri = self.url.parse().map_err(|_| Class::Blocked)?;
        let target = Destination::parse(&uri).map_err(bound_error)?;
        if https && !target.tls {
            return Err(Class::Tls.into());
        }
        let addresses = target
            .resolve(self.policy, Some((self.revision, self.local_only)))
            .await
            .map_err(bound_error)?;
        Ok((target.host.to_owned(), addresses))
    }

    pub async fn dns(&self) -> Result<Vec<IpAddr>, Fault> {
        let (_, addresses) = self.resolve(false).await?;
        let mut ips: Vec<_> = addresses.into_iter().map(|address| address.ip()).collect();
        ips.sort_unstable();
        ips.dedup();
        Ok(ips)
    }

    pub async fn tcp(&self) -> Result<TcpProbe, Fault> {
        let (_, addresses) = self.resolve(false).await?;
        let start = Instant::now();
        let reachable = TcpStream::connect(addresses.as_slice()).await.is_ok();
        self.revision.check().map_err(bound_error)?;
        Ok(TcpProbe {
            reachable,
            latency: start.elapsed(),
        })
    }

    pub async fn tls(&self, pin: Option<[u8; 32]>) -> Result<TlsLeaf, Fault> {
        let (host, addresses) = self.resolve(true).await?;
        let socket = TcpStream::connect(addresses.as_slice())
            .await
            .map_err(|_| Class::Connect)?;
        self.revision.check().map_err(bound_error)?;
        let name = rustls::pki_types::ServerName::try_from(host).map_err(|_| Class::Tls)?;
        let (verifier, config) = Leaf::inspect(pin, client::roots());
        let tls = tokio_rustls::TlsConnector::from(config)
            .connect(name.clone(), socket)
            .await
            .map_err(|_| Class::Tls)?;
        self.revision.check().map_err(bound_error)?;
        let leaf = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .ok_or(Class::Tls)?;
        let fingerprint = *verifier.presented.get().ok_or(Class::Tls)?;
        let verdict = verifier
            .inspection
            .as_ref()
            .and_then(|value| value.get())
            .ok_or(Class::Tls)?;
        let verification = match (pin, verdict) {
            (Some(_), Ok(())) => TlsVerification::Pinned,
            (Some(_), Err(_)) => TlsVerification::PinMismatch,
            (None, Ok(())) => TlsVerification::Trusted,
            (None, Err(error)) => TlsVerification::Rejected(error.clone()),
        };
        let name_matches = rustls::server::ParsedCertificate::try_from(leaf)
            .is_ok_and(|cert| rustls::client::verify_server_name(&cert, &name).is_ok());
        certificate::read(leaf.as_ref(), fingerprint, verification, name_matches)
    }
}

fn bound_error(message: String) -> Fault {
    BoundError {
        message,
        fingerprint: None,
    }
    .into()
}
