//! The relay's TLS identity for QUIC, and how clients trust it.
//!
//! The relay makes a P-256 key and self-signed certificate, with no ACME
//! and no file shared with the HTTPS proxy. Clients pin the certificate's
//! SHA-256. With an identity directory (`--quic-identity-dir`) the relay
//! keeps the same certificate across restarts, so its pin can be published
//! in Sheprd's signed relay list and a client never has to trust a pin it
//! read over ordinary HTTPS, which a TLS-inspecting proxy or a mis-issued
//! certificate could forge. Without one it makes a fresh certificate each
//! start, and clients read the pin from `GET /v1/info` (or a cached copy).

use super::Error;
use crate::wire::ALPN;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use std::{fmt, fs, io, path::Path, sync::Arc};

/// The files an identity directory holds, as DER.
const KEY_FILE: &str = "quic-key.der";
const CERTIFICATE_FILE: &str = "quic-certificate.der";

/// The name in the relay's certificate. Clients check the pin, not names.
pub const SERVER_NAME: &str = "sheprd-relay";

/// A certificate's SHA-256, as clients pin it.
pub type Pin = [u8; 32];

/// The relay's certificate and key for this run.
pub struct Identity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    pub pin: Pin,
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("pin", &self.pin)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// A fresh key and self-signed certificate.
    pub fn generate() -> Result<Self, Error> {
        let certified = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_owned()])?;
        let certificate = certified.cert.der().clone();
        let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
        Ok(Self::new(certificate, key))
    }

    fn new(certificate: CertificateDer<'static>, key: PrivatePkcs8KeyDer<'static>) -> Self {
        let pin = Sha256::digest(&certificate).into();
        Self {
            certificate,
            key,
            pin,
        }
    }

    /// The identity kept in `directory`, made and saved there (readable by
    /// this user only) the first time, so the pin outlives restarts. A
    /// certificate whose key does not match is refused when TLS starts.
    pub fn kept(directory: &Path) -> Result<Self, Error> {
        let (key_path, certificate_path) =
            (directory.join(KEY_FILE), directory.join(CERTIFICATE_FILE));
        let read = |path: &Path| fs::read(path);
        match (read(&key_path), read(&certificate_path)) {
            (Ok(key), Ok(certificate)) => Ok(Self::new(
                CertificateDer::from(certificate),
                PrivatePkcs8KeyDer::from(key),
            )),
            (Err(error), _) | (_, Err(error)) if error.kind() != io::ErrorKind::NotFound => {
                Err(Error::Identity {
                    path: directory.to_owned(),
                    source: error,
                })
            }
            _ => {
                let identity = Self::generate()?;
                let write = |path: &Path, bytes: &[u8]| {
                    private_file(path, bytes).map_err(|source| Error::Identity {
                        path: path.to_owned(),
                        source,
                    })
                };
                write(&key_path, identity.key.secret_pkcs8_der())?;
                write(&certificate_path, &identity.certificate)?;
                Ok(identity)
            }
        }
    }

    /// The rustls side of the QUIC server: TLS 1.3, ALPN `sheprd-relay/1`.
    pub fn server_crypto(&self) -> Result<QuicServerConfig, Error> {
        let mut config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(
                vec![self.certificate.clone()],
                PrivateKeyDer::Pkcs8(self.key.clone_key()),
            )?;
        config.alpn_protocols = vec![ALPN.to_vec()];
        Ok(QuicServerConfig::try_from(config)?)
    }
}

/// Writes `bytes` to a new file at `path` only this user can read.
fn private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write as _;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A client config that trusts only the certificate with `pin`.
pub fn client_crypto(pin: Pin) -> Result<QuicClientConfig, Error> {
    let provider = provider();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { pin, provider }))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(QuicClientConfig::try_from(config)?)
}

/// Accepts exactly the pinned certificate, and still checks that the
/// handshake was signed by its key.
#[derive(Debug)]
struct Pinned {
    pin: Pin,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen: Pin = Sha256::digest(end_entity).into();
        if seen == self.pin {
            return Ok(ServerCertVerified::assertion());
        }
        Err(rustls::Error::InvalidCertificate(
            rustls::CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
