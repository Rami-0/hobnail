//! Per-request leaf pinning. Even with a pin, rustls verifies the handshake
//! signatures, so the peer must prove possession of the pinned key.

use rustls::{
    client::{danger::ServerCertVerifier, WebPkiServerVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};
use std::sync::{Arc, OnceLock};

pub(super) type Fingerprint = [u8; 32];

pub(crate) fn parse(text: &str) -> Option<Fingerprint> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut pin = [0; 32];
    for (byte, pair) in pin.iter_mut().zip(text.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(pin)
}

pub(super) fn display(pin: &Fingerprint) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in pin {
        write!(out, "{byte:02x}").expect("format into string");
    }
    out
}

#[derive(Debug)]
pub(super) struct Leaf {
    pub(super) presented: OnceLock<Fingerprint>,
    expected: Option<Fingerprint>,
    webpki: Arc<WebPkiServerVerifier>,
}

impl Leaf {
    pub(super) fn configure(
        expected: Option<Fingerprint>,
        roots: Arc<RootCertStore>,
    ) -> (Arc<Self>, Arc<ClientConfig>) {
        let crypto = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(Self {
            presented: OnceLock::new(),
            expected,
            webpki: WebPkiServerVerifier::builder_with_provider(roots, crypto.clone())
                .build()
                .expect("nonempty TLS roots"),
        });
        let config = ClientConfig::builder_with_provider(crypto)
            .with_safe_default_protocol_versions()
            .expect("rustls protocol versions")
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        (verifier, Arc::new(config))
    }
}

impl ServerCertVerifier for Leaf {
    fn verify_server_cert(
        &self,
        leaf: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, Error> {
        use rustls::{client::danger::ServerCertVerified, CertificateError};
        let digest = ring::digest::digest(&ring::digest::SHA256, leaf.as_ref());
        let fingerprint: Fingerprint = digest.as_ref().try_into().expect("SHA-256 digest");
        let _ = self.presented.set(fingerprint);
        match self.expected {
            Some(pin) if pin == fingerprint => Ok(ServerCertVerified::assertion()),
            Some(_) => Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            )),
            None => self.webpki.verify_server_cert(leaf, chain, name, ocsp, now),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        self.webpki.verify_tls12_signature(message, cert, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        self.webpki.verify_tls13_signature(message, cert, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use tokio::net::{TcpListener, TcpStream};

    const KEY: &[u8] = include_bytes!("testdata/integration-test-key.der");
    const SELF_SIGNED: &[u8] = include_bytes!("testdata/integration-self-signed.der");
    const CHAIN_LEAF: &[u8] = include_bytes!("testdata/integration-chain-leaf.der");
    const CHAIN_ROOT: &[u8] = include_bytes!("testdata/integration-chain-root.der");

    async fn handshake(
        leaf: &'static [u8],
        expected: Option<Fingerprint>,
        trust: bool,
    ) -> (bool, Fingerprint) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(leaf)],
            PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
        )
        .unwrap();
        let accept = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            tokio_rustls::TlsAcceptor::from(Arc::new(server))
                .accept(socket)
                .await
                .is_ok()
        });
        let mut roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        if trust {
            roots.add(CertificateDer::from(CHAIN_ROOT)).unwrap();
        }
        let (verifier, config) = Leaf::configure(expected, Arc::new(roots));
        let socket = TcpStream::connect(addr).await.unwrap();
        let tls = tokio_rustls::TlsConnector::from(config)
            .connect(ServerName::IpAddress(addr.ip().into()), socket)
            .await;
        assert_eq!(tls.is_ok(), accept.await.unwrap());
        (tls.is_ok(), *verifier.presented.get().unwrap())
    }

    #[tokio::test]
    async fn enforces_leaf_pin_and_preserves_root_verification() {
        let pin: Fingerprint = ring::digest::digest(&ring::digest::SHA256, SELF_SIGNED)
            .as_ref()
            .try_into()
            .unwrap();
        assert_eq!(handshake(SELF_SIGNED, None, false).await, (false, pin));
        assert_eq!(
            handshake(SELF_SIGNED, Some([0; 32]), false).await,
            (false, pin)
        );
        assert_eq!(handshake(SELF_SIGNED, Some(pin), false).await, (true, pin));
        assert!(handshake(CHAIN_LEAF, None, true).await.0);
        assert!(!handshake(CHAIN_LEAF, None, false).await.0);
    }
}
