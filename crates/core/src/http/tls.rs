//! TLS configuration (rustls with the `ring` backend, Mozilla root certificates).

use crate::model::HttpVersionPref;
use std::sync::{Arc, OnceLock};
use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use tokio_rustls::rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, ring};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, RootCertStore, SignatureScheme};

/// Returns a shared TLS config for the given verification mode and HTTP version.
pub fn client_config(verify: bool, version: HttpVersionPref) -> Arc<ClientConfig> {
    static CONFIGS: OnceLock<[Arc<ClientConfig>; 6]> = OnceLock::new();
    let configs = CONFIGS.get_or_init(|| {
        let versions = [HttpVersionPref::Auto, HttpVersionPref::Http1, HttpVersionPref::Http2];
        let build = |i: usize| build_config(i < 3, versions[i % 3]);
        std::array::from_fn(build)
    });
    let version_index = match version {
        HttpVersionPref::Auto => 0,
        HttpVersionPref::Http1 => 1,
        HttpVersionPref::Http2 => 2,
    };
    configs[if verify { 0 } else { 3 } + version_index].clone()
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

fn build_config(verify: bool, version: HttpVersionPref) -> Arc<ClientConfig> {
    let builder = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions");
    let mut config = if verify {
        let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider().signature_verification_algorithms)))
            .with_no_client_auth()
    };
    config.alpn_protocols = match version {
        HttpVersionPref::Auto => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        HttpVersionPref::Http1 => vec![b"http/1.1".to_vec()],
        HttpVersionPref::Http2 => vec![b"h2".to_vec()],
    };
    Arc::new(config)
}

/// Accepts any server certificate. Used only when the user turns off TLS
/// verification (`options.verify_tls: false`), e.g. for local self-signed servers.
/// Handshake signatures are still checked.
#[derive(Debug)]
struct AcceptAnyCertificate(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        tokio_rustls::rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        tokio_rustls::rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}
