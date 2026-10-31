//! rustls glue: SNI override ("bug host") plus an opt-in insecure verifier.

use crate::error::{CoreError, Result};
use crate::transport::{BoxStream, TlsOptions};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio_rustls::TlsConnector;

fn provider() -> Arc<CryptoProvider> {
    static P: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider()))
        .clone()
}

/// Install the process-wide crypto provider. Safe to call more than once.
pub fn init() {
    let _ = CryptoProvider::install_default(rustls::crypto::ring::default_provider());
}

fn root_store() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    roots
}

/// Wrap an established stream in TLS using `sni` as the server name.
pub async fn wrap(
    stream: BoxStream,
    sni: &str,
    opts: &TlsOptions,
    timeout: Duration,
    logs: Option<&Arc<crate::logging::LogBus>>,
) -> Result<BoxStream> {
    let mut config = if opts.insecure {
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| CoreError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier(provider())))
            .with_no_client_auth()
    } else {
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| CoreError::Tls(e.to_string()))?
            .with_root_certificates(root_store())
            .with_no_client_auth()
    };

    if !opts.alpn.is_empty() {
        config.alpn_protocols = opts.alpn.iter().map(|p| p.as_bytes().to_vec()).collect();
    }
    // Many censorship-circumvention endpoints are picky about session tickets.
    config.enable_sni = true;

    let server_name: ServerName<'static> = ServerName::try_from(sni.to_string())
        .map_err(|_| CoreError::Tls(format!("'{sni}' is not a valid SNI host name")))?;

    let connector = TlsConnector::from(Arc::new(config));
    let tls = tokio::time::timeout(timeout, connector.connect(server_name, stream))
        .await
        .map_err(|_| CoreError::Timeout(timeout))?
        .map_err(|e| CoreError::Tls(e.to_string()))?;

    let version = match tls.get_ref().1.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2".to_string(),
        Some(other) => format!("{other:?}"),
        None => "unknown".to_string(),
    };
    if let Some(logs) = logs {
        logs.info("tls", format!("SSL version: {version}"));
    }

    Ok(Box::new(tls))
}

#[derive(Debug)]
struct NoVerifier(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
