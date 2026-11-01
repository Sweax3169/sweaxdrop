//! Device identity: one Ed25519 key pair per installation, wrapped in a
//! self-signed certificate that both sides present in TLS. The device id is
//! derived from the public key, so a peer can only claim an id if it holds the
//! matching private key.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

/// DER prefix of an Ed25519 SubjectPublicKeyInfo; the 32-byte key follows it.
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

#[derive(Clone)]
pub struct Identity {
    pub cert: CertificateDer<'static>,
    key_der: Vec<u8>,
    pub fingerprint: [u8; 32],
}

impl Identity {
    /// Loads `identity.pem` from `dir`, or creates it on first run.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let path = dir.join("identity.pem");
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(id) = Self::from_pem(&text) {
                return Ok(id);
            }
            tracing::warn!("identity file unreadable, creating a new identity");
        }
        let id = Self::generate()?;
        fs::create_dir_all(dir).ok();
        let tmp = path.with_extension("pem.tmp");
        fs::write(&tmp, id.to_pem()).context("write identity")?;
        fs::rename(&tmp, &path).context("store identity")?;
        Ok(id)
    }

    pub fn generate() -> Result<Self> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let mut params = rcgen::CertificateParams::new(vec!["sweaxdrop.local".to_string()])?;
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        params.not_after = rcgen::date_time_ymd(2124, 1, 1);
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "SweaxDrop device");
        params.distinguished_name = dn;
        let cert = params.self_signed(&key)?;
        let cert_der = cert.der().clone();
        let fingerprint = fingerprint_of_cert(&cert_der)?;
        Ok(Self { cert: cert_der, key_der: key.serialize_der(), fingerprint })
    }

    fn from_pem(text: &str) -> Result<Self> {
        let mut cert = None;
        let mut key = None;
        for block in text.split("-----BEGIN ").skip(1) {
            let (label, rest) = block.split_once("-----").ok_or_else(|| anyhow!("pem"))?;
            let body: String = rest
                .split("-----END")
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let bytes = data_encoding::BASE64.decode(body.as_bytes())?;
            match label {
                "CERTIFICATE" => cert = Some(bytes),
                "PRIVATE KEY" => key = Some(bytes),
                _ => {}
            }
        }
        let cert = CertificateDer::from(cert.ok_or_else(|| anyhow!("no certificate"))?);
        let key_der = key.ok_or_else(|| anyhow!("no key"))?;
        let fingerprint = fingerprint_of_cert(&cert)?;
        Ok(Self { cert, key_der, fingerprint })
    }

    fn to_pem(&self) -> String {
        fn block(label: &str, der: &[u8]) -> String {
            let b64 = data_encoding::BASE64.encode(der);
            let mut out = format!("-----BEGIN {label}-----\n");
            for line in b64.as_bytes().chunks(64) {
                out.push_str(std::str::from_utf8(line).unwrap());
                out.push('\n');
            }
            out.push_str(&format!("-----END {label}-----\n"));
            out
        }
        block("CERTIFICATE", &self.cert) + &block("PRIVATE KEY", &self.key_der)
    }

    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }

    pub fn id(&self) -> String {
        device_id(&self.fingerprint)
    }
}

/// SHA-256 of the raw Ed25519 public key inside a certificate.
pub fn fingerprint_of_cert(der: &[u8]) -> Result<[u8; 32]> {
    let pos = der
        .windows(ED25519_SPKI_PREFIX.len())
        .position(|w| w == ED25519_SPKI_PREFIX)
        .ok_or_else(|| anyhow!("certificate does not carry an Ed25519 key"))?;
    let start = pos + ED25519_SPKI_PREFIX.len();
    let key = der.get(start..start + 32).ok_or_else(|| anyhow!("short key"))?;
    Ok(Sha256::digest(key).into())
}

/// 16 base32 characters (80 bits) of the fingerprint: the stable device id.
pub fn device_id(fp: &[u8; 32]) -> String {
    data_encoding::BASE32_NOPAD.encode(&fp[..10])
}

/// The id as people read it aloud: `K7Q2-9MXA-…` (4 groups of 4).
pub fn pretty_id(id: &str) -> String {
    id.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("-")
}

/// Six digits both screens can show to confirm that two devices really see
/// each other's keys (used before the first transfer when there is no QR/PIN).
pub fn safety_code(a: &[u8; 32], b: &[u8; 32]) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update(b"sweaxdrop safety code v1");
    h.update(lo);
    h.update(hi);
    let d = h.finalize();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_stable_id() {
        let dir = std::env::temp_dir().join(format!("swx-id-{}", rand::random::<u64>()));
        let a = Identity::load_or_create(&dir).unwrap();
        let b = Identity::load_or_create(&dir).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.id().len(), 16);
        let other = Identity::generate().unwrap();
        assert_ne!(a.id(), other.id());
        assert_eq!(safety_code(&a.fingerprint, &other.fingerprint), safety_code(&other.fingerprint, &a.fingerprint));
        std::fs::remove_dir_all(dir).ok();
    }
}
