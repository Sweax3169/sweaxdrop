//! Links: the two transports SweaxDrop can run on, behind one interface.
//!
//! * `Tcp`: every stream is its own TCP connection with its own TLS 1.3
//!   session. Parallel connections each get their own congestion window,
//!   which is what fills a Wi-Fi link when one window cannot.
//! * `Quic`: one QUIC connection (quinn, TLS 1.3 inside), every stream is a
//!   QUIC stream. One handshake, no head-of-line blocking between streams.
//!
//! Which one is faster is measured, not assumed: see `bench` and
//! docs/benchmarks.md. Both deliver the peer fingerprint from the handshake.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::identity::Identity;
use crate::tls::{self, CipherPref};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Tcp,
    Quic,
}

pub type BoxRead = Box<dyn AsyncRead + Send + Unpin>;
pub type BoxWrite = Box<dyn AsyncWrite + Send + Unpin>;

/// One bidirectional stream with the authenticated peer behind it.
pub struct Stream {
    pub read: BoxRead,
    pub write: BoxWrite,
}

/// Socket tuning the benchmark can vary.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct SocketTuning {
    /// 0 = leave the OS default (keeps receive-window autotuning on).
    pub sndbuf: usize,
    pub rcvbuf: usize,
}

pub fn tune_tcp(s: &TcpStream, t: SocketTuning) {
    let _ = s.set_nodelay(true);
    let sock = socket2::SockRef::from(s);
    if t.sndbuf > 0 {
        let _ = sock.set_send_buffer_size(t.sndbuf);
    }
    if t.rcvbuf > 0 {
        let _ = sock.set_recv_buffer_size(t.rcvbuf);
    }
    let _ = sock.set_keepalive(true);
}

/// A connected session to a peer that can open more streams.
#[derive(Clone)]
pub enum Link {
    Tcp { addr: SocketAddr, connector: TlsConnector, tuning: SocketTuning },
    Quic { conn: quinn::Connection },
}

impl Link {
    pub async fn open(&self) -> Result<Stream> {
        match self {
            Link::Tcp { addr, connector, tuning } => {
                let (s, _) = tcp_tls_connect(*addr, connector, *tuning).await?;
                Ok(s)
            }
            Link::Quic { conn } => {
                let (w, r) = conn.open_bi().await?;
                Ok(Stream { read: Box::new(r), write: Box::new(QuicSend(w)) })
            }
        }
    }

    pub fn transport(&self) -> Transport {
        match self {
            Link::Tcp { .. } => Transport::Tcp,
            Link::Quic { .. } => Transport::Quic,
        }
    }

    pub fn close(&self) {
        if let Link::Quic { conn } = self {
            conn.close(0u32.into(), b"done");
        }
    }
}

/// quinn's SendStream flushes on `shutdown`; this wrapper makes `flush` a no-op
/// and finishes the stream on drop so a dropped writer ends the peer's reads.
struct QuicSend(quinn::SendStream);

impl AsyncWrite for QuicSend {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf).map_err(std::io::Error::other)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx)
    }
}

impl Drop for QuicSend {
    fn drop(&mut self) {
        let _ = self.0.finish();
    }
}

pub async fn tcp_tls_connect(
    addr: SocketAddr,
    connector: &TlsConnector,
    tuning: SocketTuning,
) -> Result<(Stream, [u8; 32])> {
    let tcp = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
        .await
        .map_err(|_| anyhow!("connect to {addr} timed out"))??;
    tune_tcp(&tcp, tuning);
    let tls = connector.connect(tls::server_name(), tcp).await.context("TLS handshake")?;
    let fp = tls::peer_fingerprint(tls.get_ref().1.peer_certificates())
        .ok_or_else(|| anyhow!("peer sent no usable certificate"))?;
    let (r, w) = tokio::io::split(tls);
    Ok((Stream { read: Box::new(r), write: Box::new(w) }, fp))
}

pub struct Dialer {
    pub tcp: TlsConnector,
    pub quic: Option<quinn::Endpoint>,
    pub tuning: SocketTuning,
}

impl Dialer {
    pub fn new(id: &Identity, pref: CipherPref, quic: Option<quinn::Endpoint>, tuning: SocketTuning) -> Result<Self> {
        let cfg = tls::client_config(id, pref)?;
        Ok(Self { tcp: TlsConnector::from(Arc::new(cfg)), quic, tuning })
    }

    /// Connects and returns the link, its first (control) stream and the
    /// fingerprint the peer proved.
    pub async fn connect(&self, addr: SocketAddr, transport: Transport) -> Result<(Link, Stream, [u8; 32])> {
        match transport {
            Transport::Tcp => {
                let (s, fp) = tcp_tls_connect(addr, &self.tcp, self.tuning).await?;
                Ok((Link::Tcp { addr, connector: self.tcp.clone(), tuning: self.tuning }, s, fp))
            }
            Transport::Quic => {
                let ep = self.quic.as_ref().ok_or_else(|| anyhow!("QUIC is off"))?;
                let conn = tokio::time::timeout(Duration::from_secs(4), ep.connect(addr, "sweaxdrop.local")?)
                    .await
                    .map_err(|_| anyhow!("QUIC connect to {addr} timed out"))??;
                let fp = quic_peer_fp(&conn)?;
                let link = Link::Quic { conn };
                let s = link.open().await?;
                Ok((link, s, fp))
            }
        }
    }
}

pub fn quic_peer_fp(conn: &quinn::Connection) -> Result<[u8; 32]> {
    let any = conn.peer_identity().ok_or_else(|| anyhow!("no peer identity"))?;
    let certs = any
        .downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
        .map_err(|_| anyhow!("unexpected peer identity"))?;
    tls::peer_fingerprint(Some(&certs)).ok_or_else(|| anyhow!("peer sent no usable certificate"))
}

pub fn quic_transport_config() -> quinn::TransportConfig {
    let mut t = quinn::TransportConfig::default();
    // Wi-Fi at 100+ MB/s with 5–20 ms RTT needs a few MB in flight.
    t.stream_receive_window((16u32 << 20).into());
    t.receive_window((64u32 << 20).into());
    t.send_window(64 << 20);
    t.max_concurrent_bidi_streams(64u32.into());
    t.keep_alive_interval(Some(Duration::from_secs(5)));
    t.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));
    t.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
    t
}

/// A QUIC endpoint that both accepts and dials on one UDP socket.
pub fn quic_endpoint(id: &Identity, pref: CipherPref, bind: SocketAddr) -> Result<quinn::Endpoint> {
    let transport = Arc::new(quic_transport_config());
    let server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls::server_config(id, pref)?)?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));
    server.transport_config(transport.clone());
    let client_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls::client_config(id, pref)?)?;
    let mut client = quinn::ClientConfig::new(Arc::new(client_crypto));
    client.transport_config(transport);

    let sock = std::net::UdpSocket::bind(bind)?;
    let s2 = socket2::SockRef::from(&sock);
    let _ = s2.set_recv_buffer_size(8 << 20);
    let _ = s2.set_send_buffer_size(8 << 20);
    let runtime = quinn::default_runtime().ok_or_else(|| anyhow!("no async runtime"))?;
    let mut ep = quinn::Endpoint::new(quinn::EndpointConfig::default(), Some(server), sock, runtime)?;
    ep.set_default_client_config(client);
    Ok(ep)
}

/// Wraps a TcpStream that already had its first bytes peeked, used by the
/// listener that serves both TLS and plain HTTP on one port.
pub struct Peeked<S> {
    pub inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for Peeked<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
