//! TLS, on rustls: this Mac's own certificate for the phone at home, which trusts it by its print, and the way to the
//! server's room (wss), which is trusted as any site is.

use crate::codec;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer, PrivateSec1KeyDer, ServerName};
use std::path::Path;
use std::sync::{Arc, OnceLock};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The blocks of a PEM file: what each says it is (`CERTIFICATE`, `EC PRIVATE KEY`…) and its bytes.
pub fn pem_blocks(text: &str) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(label) = line.strip_prefix("-----BEGIN ").and_then(|rest| rest.strip_suffix("-----")) {
            open = Some((label.to_string(), String::new()));
        } else if let Some(label) = line.strip_prefix("-----END ").and_then(|rest| rest.strip_suffix("-----")) {
            if let Some((name, body)) = open.take().filter(|(name, _)| name == label) {
                if let Some(der) = codec::unbase64(&body) {
                    found.push((name, der));
                }
            }
        } else if let Some((_, body)) = open.as_mut() {
            if !line.contains(':') {
                body.push_str(line);
            }
        }
    }
    found
}

/// The first certificate of a PEM file, as DER: what the phone's print is taken of.
pub fn certificate(path: &Path) -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    pem_blocks(&text).into_iter().find(|(label, _)| label == "CERTIFICATE").map(|(_, der)| der)
        .ok_or_else(|| format!("{}: no certificate in it", path.display()))
}

fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for (label, der) in pem_blocks(&text) {
        match label.as_str() {
            "EC PRIVATE KEY" => return Ok(PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(der))),
            "PRIVATE KEY" => return Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der))),
            "RSA PRIVATE KEY" => return Ok(PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(der))),
            _ => {}
        }
    }
    Err(format!("{}: no private key in it", path.display()))
}

/// What the Mac answers a phone at home with: its own certificate and key.
pub fn server(certificate_file: &Path, key_file: &Path) -> Result<Arc<rustls::ServerConfig>, String> {
    let chain = vec![CertificateDer::from(certificate(certificate_file)?)];
    let key = private_key(key_file)?;
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|error| format!("the certificate for the phone is not usable: {error}"))?;
    Ok(Arc::new(config))
}

/// A connection to a site, trusted by the roots every browser trusts.
pub fn client(host: &str) -> Result<rustls::ClientConnection, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    let config = CONFIG.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map(|builder| Arc::new(builder.with_root_certificates(roots).with_no_client_auth()))
            .map_err(|error| error.to_string())
    }).clone()?;
    let name = ServerName::try_from(host.to_string()).map_err(|error| format!("{host}: {error}"))?;
    rustls::ClientConnection::new(config, name).map_err(|error| error.to_string())
}

/// A client that takes any certificate, for this crate's own tests: the phone they stand in for trusts one by its
/// print, which they check themselves.
#[doc(hidden)]
pub fn trusting_client() -> rustls::ClientConnection {
    #[derive(Debug)]
    struct Anything(Arc<rustls::crypto::CryptoProvider>);
    impl rustls::client::danger::ServerCertVerifier for Anything {
        fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8],
                              _: rustls::pki_types::UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(&self, message: &[u8], certificate: &CertificateDer<'_>, signature: &rustls::DigitallySignedStruct)
                                  -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, certificate, signature, &self.0.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, message: &[u8], certificate: &CertificateDer<'_>, signature: &rustls::DigitallySignedStruct)
                                  -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, certificate, signature, &self.0.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("the default versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Anything(provider())))
        .with_no_client_auth();
    rustls::ClientConnection::new(Arc::new(config), ServerName::try_from("hitl").expect("a name")).expect("a client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_blocks_are_read_by_their_labels() {
        let text = "junk\n-----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n\
                    -----BEGIN CERTIFICATE-----\nZm9v\nYmFy\n-----END CERTIFICATE-----\n";
        let blocks = pem_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!((blocks[1].0.as_str(), blocks[1].1.as_slice()), ("CERTIFICATE", &b"foobar"[..]));
    }
}
