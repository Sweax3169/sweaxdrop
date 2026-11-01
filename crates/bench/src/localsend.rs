//! A LocalSend protocol v2 sender, used to measure the LocalSend app as a
//! receiver on exactly the same files and network as SweaxDrop.
//!
//! It follows LocalSend's own flow: one `prepare-upload` with the file list,
//! then one HTTPS `upload` request per file. `concurrency` sets how many of
//! those requests run at once; the LocalSend app's own value is documented in
//! docs/research.md and used for the published runs. Written in Rust on
//! tokio + rustls, this sender is at least as fast as LocalSend's Dart one,
//! so the LocalSend numbers are, if anything, flattering.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::{json, Value};
use sweaxdrop_core::storage::{collect_paths, Source};

use crate::{cpu_seconds, Run};

#[derive(Debug)]
struct AcceptAll(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAll {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// LocalSend 1.18 servers require a client certificate (any valid one) and
/// use its SHA-256 as the sender's fingerprint.
fn client() -> Result<(reqwest::Client, String)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = rcgen::CertificateParams::new(vec!["localsend-bench".to_string()])?.self_signed(&key)?;
    let der = cert.der().clone();
    let fp = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&der[..]));
    let fingerprint: String = fp.iter().map(|b| format!("{b:02x}")).collect();
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()));
    let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAll(provider)))
        .with_client_auth_cert(vec![der], key_der)?;
    Ok((reqwest::Client::builder().tls_backend_preconfigured(cfg).pool_max_idle_per_host(32).build()?, fingerprint))
}

pub async fn bench(to: &str, paths: &[PathBuf], concurrency: usize, https: bool, repeat: u32, label: Option<String>) -> Result<Run> {
    let scheme = if https { "https" } else { "http" };
    let base = format!("{scheme}://{to}/api/localsend/v2");
    let (http, fingerprint) = client()?;
    let (mut secs, mut ttfbs, mut cpus) = (vec![], vec![], vec![]);
    let mut total = 0u64;
    let mut count = 0u64;
    for _ in 0..repeat {
        let sources: Vec<_> = collect_paths(paths)?.into_iter().filter(|s| !s.item.dir).collect();
        total = sources.iter().map(|s| s.item.size).sum();
        count = sources.len() as u64;
        let mut files = serde_json::Map::new();
        for s in &sources {
            let id = format!("f{}", s.item.id);
            files.insert(
                id.clone(),
                json!({
                    "id": id,
                    "fileName": s.item.path,
                    "size": s.item.size,
                    "fileType": s.item.mime.clone().unwrap_or_else(|| "application/octet-stream".into()),
                }),
            );
        }
        let body = json!({
            "info": {
                "alias": "sweaxdrop-bench",
                "version": "2.1",
                "deviceModel": "bench",
                "deviceType": "desktop",
                "fingerprint": if https { fingerprint.clone() } else { format!("bench{}", std::process::id()) },
                "port": 53317,
                "protocol": scheme,
                "download": false
            },
            "files": files
        });
        let cpu0 = cpu_seconds();
        let t0 = Instant::now();
        let resp = http.post(format!("{base}/prepare-upload")).json(&body).send().await?;
        if !resp.status().is_success() {
            bail!("prepare-upload: {} (turn on Quick Save in LocalSend)", resp.status());
        }
        let v: Value = resp.json().await?;
        let session = v["sessionId"].as_str().ok_or_else(|| anyhow!("no session"))?.to_string();
        let tokens = v["files"].as_object().cloned().unwrap_or_default();
        let first = Arc::new(parking_lot::Mutex::new(None::<f64>));
        let jobs = sources.into_iter().map(|s| {
            let id = format!("f{}", s.item.id);
            let token = tokens.get(&id).and_then(Value::as_str).unwrap_or_default().to_string();
            let url = format!("{base}/upload?sessionId={session}&fileId={id}&token={token}");
            let http = http.clone();
            let first = first.clone();
            async move {
                let Source::Path(p) = s.src else { bail!("only files on disk") };
                let f = tokio::fs::File::open(&p).await?;
                let stream = tokio_util::io::ReaderStream::with_capacity(f, 1 << 20).map(move |c| {
                    let mut g = first.lock();
                    if g.is_none() {
                        *g = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                    c
                });
                let r = http
                    .post(url)
                    .header("content-length", s.item.size)
                    .body(reqwest::Body::wrap_stream(stream))
                    .send()
                    .await?;
                if !r.status().is_success() {
                    bail!("upload {}: {}", s.item.path, r.status());
                }
                Ok::<(), anyhow::Error>(())
            }
        });
        let results: Vec<_> = futures_util::stream::iter(jobs).buffer_unordered(concurrency.max(1)).collect().await;
        for r in results {
            r?;
        }
        secs.push(t0.elapsed().as_secs_f64());
        ttfbs.push(first.lock().unwrap_or(0.0));
        cpus.push(cpu_seconds() - cpu0);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let med = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let s = med(secs);
    Ok(Run {
        tool: "localsend".into(),
        case: label.unwrap_or_else(|| "localsend".into()),
        bytes: total,
        files: count,
        seconds: s,
        ttfb_ms: med(ttfbs),
        mb_s: total as f64 / s / 1e6,
        cpu_s_per_gb: med(cpus) / (total as f64 / 1e9),
        settings: json!({"concurrency": concurrency, "https": https}),
    })
}
