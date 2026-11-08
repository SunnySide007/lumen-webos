//! TLS 1.3 with rustls and ring (D4): the TV's certificate, issued by
//! Lumen's own authority on the PC (`tools/remote/ca.sh`), whose name
//! constraints let it vouch for the TV's names and nothing else. Only the
//! certificate and its key reach the TV; the authority's key stays on the
//! PC.

use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

pub fn config(cert_pem: &[u8], key_pem: &[u8]) -> Result<Arc<ServerConfig>, String> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("the certificate: {e}"))?;
    if certs.is_empty() {
        return Err("the certificate file holds no certificate".to_owned());
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| format!("the key: {e}"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS: {e}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("the certificate and key: {e}"))?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(cfg))
}
