//! TLS for live sessions: TLS 1.3 only, rustls with the ring provider.
//!
//! The host makes a self-signed certificate (ECDSA P-256) for each session.
//! The invite carries its pin, the SHA-256 of the certificate. A guest
//! accepts that certificate and no other: no certificate authority and no
//! host name are involved, but the handshake signatures are still checked, so
//! only the holder of the certificate's key can complete it.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use guhit_model::IpcError;
use ring::digest::{digest, SHA256};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// The name in the certificate. Nothing checks it; certificates are pinned.
const CERT_NAME: &str = "guhit-live";
/// A guest gives each address this long to accept the connection, and again
/// for the TLS handshake.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn tls_err(what: &str, e: impl std::fmt::Display) -> IpcError {
    IpcError::new("io", format!("{what}: {e}"))
}

/// The pin of a certificate: base64url of its SHA-256.
pub fn pin_of(cert_der: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, cert_der))
}

/// A host's certificate for one session, ready to accept connections.
pub struct HostIdentity {
    pub pin: String,
    pub acceptor: TlsAcceptor,
}

impl HostIdentity {
    pub fn new() -> Result<Self, IpcError> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
            .map_err(|e| tls_err("could not make the session key", e))?;
        let cert = rcgen::CertificateParams::new(vec![CERT_NAME.to_string()])
            .and_then(|params| params.self_signed(&key))
            .map_err(|e| tls_err("could not make the session certificate", e))?;
        let der: CertificateDer<'static> = cert.der().clone();
        let pin = pin_of(&der);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| tls_err("could not set up TLS", e))?
            .with_no_client_auth()
            .with_single_cert(vec![der], key)
            .map_err(|e| tls_err("could not set up TLS", e))?;
        Ok(Self { pin, acceptor: TlsAcceptor::from(Arc::new(config)) })
    }
}

/// Accepts exactly one certificate: the one whose SHA-256 is the pin.
#[derive(Debug)]
struct Pinned {
    pin: Vec<u8>,
    provider: Arc<CryptoProvider>,
    /// Set when the server showed another certificate, so the caller can
    /// tell "wrong host" from "no host".
    mismatch: AtomicBool,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // The pin is public (it is in the invite), so a plain comparison is fine.
        if digest(&SHA256, end_entity.as_ref()).as_ref() == self.pin.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            self.mismatch.store(true, Ordering::Relaxed);
            Err(rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Why a connection to one address failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// Nothing answered, or the connection broke during the handshake.
    Unreachable,
    /// Something answered with another certificate than the pinned one.
    WrongHost,
}

/// Open a TLS connection to `addr` that trusts only the certificate `pin`
/// (base64url SHA-256) names.
pub async fn connect(addr: SocketAddr, pin: &str) -> Result<TlsStream<TcpStream>, ConnectError> {
    let pin = URL_SAFE_NO_PAD.decode(pin.as_bytes()).map_err(|_| ConnectError::WrongHost)?;
    let provider = provider();
    let verifier = Arc::new(Pinned { pin, provider: provider.clone(), mismatch: AtomicBool::new(false) });
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| ConnectError::Unreachable)?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    // There is no host name to send: the certificate is pinned.
    config.enable_sni = false;

    let tcp = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(Ok(tcp)) => tcp,
        _ => return Err(ConnectError::Unreachable),
    };
    let _ = tcp.set_nodelay(true);
    let name = ServerName::try_from(CERT_NAME).map_err(|_| ConnectError::Unreachable)?;
    match tokio::time::timeout(CONNECT_TIMEOUT, TlsConnector::from(Arc::new(config)).connect(name, tcp)).await {
        Ok(Ok(stream)) => Ok(stream),
        _ if verifier.mismatch.load(Ordering::Relaxed) => Err(ConnectError::WrongHost),
        _ => Err(ConnectError::Unreachable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn serve_once(identity: HostIdentity) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                if let Ok(mut tls) = identity.acceptor.accept(tcp).await {
                    let mut buf = [0u8; 5];
                    if tls.read_exact(&mut buf).await.is_ok() {
                        let _ = tls.write_all(&buf).await;
                    }
                }
            }
        });
        addr
    }

    #[tokio::test]
    async fn the_pinned_certificate_connects_and_any_other_is_refused() {
        let identity = HostIdentity::new().unwrap();
        let pin = identity.pin.clone();
        let other = HostIdentity::new().unwrap().pin;
        assert_ne!(pin, other, "every session has its own certificate");
        let addr = serve_once(identity).await;

        let mut tls = connect(addr, &pin).await.unwrap();
        tls.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        tls.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        let (_, conn) = tls.get_ref();
        assert_eq!(conn.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_3));

        assert_eq!(connect(addr, &other).await.unwrap_err(), ConnectError::WrongHost);
    }

    #[tokio::test]
    async fn a_closed_port_is_unreachable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let pin = HostIdentity::new().unwrap().pin;
        assert_eq!(connect(addr, &pin).await.unwrap_err(), ConnectError::Unreachable);
    }
}
