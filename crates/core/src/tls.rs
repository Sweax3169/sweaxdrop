//! TLS 1.3 with mutual authentication by key, not by certificate authority.
//!
//! Every device presents its self-signed Ed25519 certificate. The verifiers
//! below accept any well-formed Ed25519 certificate and check the handshake
//! signature; *which* key is acceptable is decided one level up, where the
//! fingerprint is compared with the one from discovery, the QR code or the
//! trust store. This is the piece LocalSend skips: there, the fingerprint is
//! advertised but the certificate is never checked against it.

use std::sync::Arc;

use anyhow::Result;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::identity::{fingerprint_of_cert, Identity};

pub const ALPN: &[u8] = b"swx/1";

/// Cipher preference. AES-128-GCM is first because every PC since ~2010 and
/// every ARMv8 phone has AES instructions; ChaCha20 stays available for the
/// rest. `Chacha` exists so the benchmark can measure the difference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CipherPref {
    #[default]
    Aes,
    Chacha,
}

pub fn provider(pref: CipherPref) -> Arc<CryptoProvider> {
    use rustls::crypto::ring::cipher_suite as cs;
    let mut p = rustls::crypto::ring::default_provider();
    p.cipher_suites = match pref {
        CipherPref::Aes => vec![
            cs::TLS13_AES_128_GCM_SHA256,
            cs::TLS13_CHACHA20_POLY1305_SHA256,
            cs::TLS13_AES_256_GCM_SHA384,
        ],
        // QUIC needs AES-128-GCM for its initial packets, so it stays listed last.
        CipherPref::Chacha => vec![cs::TLS13_CHACHA20_POLY1305_SHA256, cs::TLS13_AES_128_GCM_SHA256],
    };
    Arc::new(p)
}

#[derive(Debug)]
struct AnyKey {
    algs: WebPkiSupportedAlgorithms,
}

impl AnyKey {
    fn new(p: &CryptoProvider) -> Arc<Self> {
        Arc::new(Self { algs: p.signature_verification_algorithms })
    }
    fn check(&self, cert: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        fingerprint_of_cert(cert)
            .map(|_| ())
            .map_err(|_| rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding))
    }
}

impl ServerCertVerifier for AnyKey {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

impl ClientCertVerifier for AnyKey {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

pub fn server_config(id: &Identity, pref: CipherPref) -> Result<rustls::ServerConfig> {
    let p = provider(pref);
    let mut cfg = rustls::ServerConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(AnyKey::new(&p))
        .with_single_cert(vec![id.cert.clone()], id.key())?;
    cfg.alpn_protocols = vec![ALPN.to_vec()];
    cfg.max_early_data_size = 0;
    // No session tickets: data streams never read, and unread bytes in a
    // socket make Windows close it with RST, which can drop data in flight.
    cfg.send_tls13_tickets = 0;
    Ok(cfg)
}

pub fn client_config(id: &Identity, pref: CipherPref) -> Result<rustls::ClientConfig> {
    let p = provider(pref);
    let mut cfg = rustls::ClientConfig::builder_with_provider(p.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(AnyKey::new(&p))
        .with_client_auth_cert(vec![id.cert.clone()], id.key())?;
    cfg.alpn_protocols = vec![ALPN.to_vec()];
    Ok(cfg)
}

/// Fingerprint of the key the other side proved it owns during the handshake.
pub fn peer_fingerprint(certs: Option<&[CertificateDer<'_>]>) -> Option<[u8; 32]> {
    certs.and_then(|c| c.first()).and_then(|c| fingerprint_of_cert(c).ok())
}

pub fn server_name() -> ServerName<'static> {
    ServerName::try_from("sweaxdrop.local").expect("static name")
}
