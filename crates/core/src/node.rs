//! The node: one running SweaxDrop instance. Owns the listeners, discovery,
//! the transfers and the event stream every UI subscribes to.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsAcceptor;

use crate::history::{Entry, History};
use crate::identity::{pretty_id, safety_code, Identity};
use crate::link::{self, Dialer, SocketTuning, Stream, Transport};
use crate::peers::{PeerView, Peers, Via};
use crate::proto::{Ctrl, DeviceInfo};
use crate::recv::RecvState;
use crate::settings::{default_save_dir, ReceiveMode, Settings};
use crate::storage::{FsSink, NullSink, Platform, PlatformSink, Sink};
use crate::tls;
use crate::transfer::{Direction, Phase, Transfer, TransferView};
use crate::util::now_ms;
use crate::wire::{read_ctrl, write_ctrl};

pub type EventSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub platform_name: String,
    pub model: String,
    pub default_name: String,
    pub platform: Option<Arc<dyn Platform>>,
    /// 0 = settings value; tests and benchmarks pass an explicit port.
    pub port: u16,
    pub discovery: bool,
    /// Benchmarks: accept everything and throw the bytes away.
    pub bench_sink: bool,
    pub tuning: SocketTuning,
}

impl Config {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            platform_name: std::env::consts::OS.to_string(),
            model: String::new(),
            default_name: hostname(),
            platform: None,
            port: 0,
            discovery: true,
            bench_sink: false,
            tuning: SocketTuning::default(),
        }
    }
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|h| {
            let mut c = h.to_lowercase().chars().collect::<Vec<_>>();
            if let Some(f) = c.first_mut() {
                *f = f.to_ascii_uppercase();
            }
            c.into_iter().collect()
        })
        .unwrap_or_else(|_| "SweaxDrop device".into())
}

pub enum Decision {
    Accept { sink: Arc<dyn Sink>, skip: Vec<u32> },
    Decline(String),
    Withdrawn(String),
}

pub struct Answer {
    pub accept: bool,
    pub save_dir: Option<String>,
    pub skip: Vec<u32>,
    pub trust: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event<'a> {
    Ready { device: &'a Value },
    Peers { peers: &'a [PeerView] },
    Transfer { transfer: &'a TransferView },
    Incoming { transfer: &'a TransferView, safety: String, trusted: bool },
    Text { from: &'a DeviceInfo, text: &'a str, transfer: &'a str, copied: bool },
    History { entry: &'a Entry },
    Pairing { state: &'a str, pin: Option<&'a str>, peer: Option<&'a DeviceInfo>, reason: Option<&'a str> },
    Web { state: &'a str, name: &'a str },
    Settings { settings: &'a Settings },
    Toast { kind: &'a str, message: &'a str },
}

pub struct Shared {
    pub cfg: Config,
    pub identity: Identity,
    pub platform: Option<Arc<dyn Platform>>,
    settings: RwLock<Settings>,
    dialer: RwLock<Arc<Dialer>>,
    pub quic: RwLock<Option<quinn::Endpoint>>,
    events: RwLock<Option<EventSink>>,
    pub peers: Peers,
    pub history: History,
    transfers: RwLock<HashMap<String, Arc<Transfer>>>,
    routes: Mutex<HashMap<String, mpsc::Sender<Stream>>>,
    prompts: Mutex<HashMap<String, oneshot::Sender<Answer>>>,
    partials: Mutex<HashMap<(String, String), Arc<RecvState>>>,
    peer_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub port: AtomicU16,
    pub pairing: crate::pairing::PairingState,
    pub web: crate::web::WebState,
    pub(crate) sources: Mutex<HashMap<String, Arc<Vec<crate::storage::SourceItem>>>>,
}

impl Shared {
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn dialer(&self) -> Arc<Dialer> {
        self.dialer.read().clone()
    }

    pub fn device(&self) -> DeviceInfo {
        let s = self.settings.read();
        DeviceInfo {
            id: self.identity.id(),
            name: if s.name.is_empty() { self.cfg.default_name.clone() } else { s.name.clone() },
            platform: self.cfg.platform_name.clone(),
            model: self.cfg.model.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn device_view(&self) -> Value {
        let d = self.device();
        json!({
            "id": d.id,
            "pretty_id": pretty_id(&d.id),
            "name": d.name,
            "platform": d.platform,
            "model": d.model,
            "version": d.version,
            "port": self.port.load(Ordering::Relaxed),
            "addresses": local_addresses().iter().map(|(ip, name)| json!({"ip": ip.to_string(), "iface": name, "via": crate::peers::classify(*ip)})).collect::<Vec<_>>(),
            "fingerprint": crate::util::hex(&self.identity.fingerprint),
        })
    }

    pub fn emit(&self, ev: &Event) {
        if let Some(sink) = self.events.read().as_ref() {
            if let Ok(s) = serde_json::to_string(ev) {
                sink(s);
            }
        }
    }

    pub fn toast(&self, kind: &str, message: &str) {
        self.emit(&Event::Toast { kind, message });
    }

    pub fn emit_peers(&self) {
        let v = self.peers.views();
        self.emit(&Event::Peers { peers: &v });
    }

    pub fn emit_transfer(&self, t: &Transfer) {
        let v = t.view(false);
        let ev = Event::Transfer { transfer: &v };
        self.emit(&ev);
        if v.peer.model == "browser" {
            if let Ok(s) = serde_json::to_string(&ev) {
                self.web.push(&v.peer.id, &s);
            }
        }
    }

    pub fn transfer(&self, id: &str) -> Option<Arc<Transfer>> {
        self.transfers.read().get(id).cloned()
    }

    pub fn transfers(&self) -> Vec<Arc<Transfer>> {
        let mut v: Vec<_> = self.transfers.read().values().cloned().collect();
        v.sort_by_key(|t| t.created_ms);
        v
    }

    pub fn add_transfer(&self, t: Arc<Transfer>) {
        let mut m = self.transfers.write();
        // Keep finished transfers around for a while so the UI can show them.
        if m.len() > 200 {
            m.retain(|_, x| !x.phase().is_final() || now_ms() - x.created_ms < 3_600_000);
        }
        m.insert(t.id.clone(), t);
    }

    pub fn finish_transfer(&self, t: &Arc<Transfer>) {
        t.sample();
        self.emit_transfer(t);
        let s = self.settings();
        if s.keep_history {
            let e = Entry::from_transfer(t);
            self.history.add(e.clone());
            self.emit(&Event::History { entry: &e });
        }
        if t.dir == Direction::Receive && t.phase() == Phase::Done && s.notify {
            if let Some(p) = &self.platform {
                let peer = t.peer.read().name.clone();
                let n = t.files_done.load(Ordering::Relaxed);
                if n > 0 {
                    p.notify(&format!("From {peer}"), &format!("{n} file(s) received"));
                }
            }
        }
        self.sources.lock().remove(&t.id);
    }

    pub fn note_peer_key(&self, d: &DeviceInfo, fp: [u8; 32]) {
        if self.peers.upsert(d.clone(), Some(fp), None, None, "app") {
            self.emit_peers();
        }
    }

    pub fn note_contact(&self, d: &DeviceInfo, from: &str) {
        if let Ok(a) = from.parse::<SocketAddr>() {
            // The address it connected from + its announced port is reachable.
            let port = self.peers.get(&d.id).and_then(|p| p.addrs.first().map(|x| x.addr.port())).unwrap_or(crate::settings::DEFAULT_PORT);
            if self.peers.upsert(d.clone(), None, Some(SocketAddr::new(a.ip(), port)), None, "app") {
                self.emit_peers();
            }
        }
    }

    pub fn add_route(&self, token: &str, tx: mpsc::Sender<Stream>) {
        self.routes.lock().insert(token.to_string(), tx);
    }

    pub fn remove_route(&self, token: &str) {
        self.routes.lock().remove(token);
    }

    pub fn take_partial(&self, key: &(String, String)) -> Option<Arc<RecvState>> {
        self.partials.lock().remove(key)
    }

    pub fn keep_partial(self: &Arc<Self>, key: (String, String), st: Arc<RecvState>) {
        self.partials.lock().insert(key.clone(), st.clone());
        let me = self.clone();
        tokio::spawn(async move {
            // A sender that does not come back within 15 minutes is gone.
            tokio::time::sleep(Duration::from_secs(900)).await;
            let still = me.partials.lock().get(&key).map(|s| Arc::ptr_eq(s, &st)).unwrap_or(false);
            if still {
                me.partials.lock().remove(&key);
                st.abort(false);
                st.t.fail("the sender did not come back");
                me.finish_transfer(&st.t);
            }
        });
    }

    pub fn default_sink(&self, save_dir: Option<&str>) -> Arc<dyn Sink> {
        if self.cfg.bench_sink {
            return Arc::new(NullSink);
        }
        let s = self.settings();
        let dir = save_dir.map(str::to_string).filter(|d| !d.is_empty()).unwrap_or(s.save_dir);
        if dir.is_empty() {
            if let Some(p) = &self.platform {
                return Arc::new(PlatformSink { platform: p.clone() });
            }
            return Arc::new(FsSink { root: default_save_dir() });
        }
        Arc::new(FsSink { root: PathBuf::from(dir) })
    }

    pub async fn decide(&self, t: &Arc<Transfer>, sender: &DeviceInfo, fp: [u8; 32]) -> Decision {
        let s = self.settings();
        let trusted = self.peers.is_trusted(&sender.id, &fp);
        if s.receive == ReceiveMode::Off {
            return Decision::Decline("not receiving right now".into());
        }
        if s.receive == ReceiveMode::Trusted && !trusted {
            return Decision::Decline("only accepts paired devices".into());
        }
        if self.cfg.bench_sink || (trusted && s.auto_accept_trusted) {
            return Decision::Accept { sink: self.default_sink(None), skip: vec![] };
        }
        let (tx, rx) = oneshot::channel();
        self.prompts.lock().insert(t.id.clone(), tx);
        let v = t.view(true);
        self.emit(&Event::Incoming { transfer: &v, safety: safety_code(&self.identity.fingerprint, &fp), trusted });
        if let Some(p) = &self.platform {
            let what = if t.items.is_empty() { "a message".to_string() } else { format!("{} file(s)", v.files) };
            p.notify(&format!("{} wants to send you {what}", sender.name), "Open SweaxDrop to accept");
        }
        let res = tokio::time::timeout(Duration::from_secs(180), rx).await;
        self.prompts.lock().remove(&t.id);
        match res {
            Ok(Ok(a)) if a.accept => {
                if a.trust {
                    self.peers.trust(sender, &fp);
                    self.emit_peers();
                }
                Decision::Accept { sink: self.default_sink(a.save_dir.as_deref()), skip: a.skip }
            }
            Ok(Ok(_)) => Decision::Decline("declined".into()),
            _ => Decision::Decline("no answer".into()),
        }
    }

    pub fn answer(&self, id: &str, a: Answer) -> bool {
        match self.prompts.lock().remove(id) {
            Some(tx) => tx.send(a).is_ok(),
            None => false,
        }
    }

    pub fn text_received(&self, t: &Transfer, text: &str) {
        let s = self.settings();
        let peer = t.peer.read().clone();
        let trusted = self.peers.trusted_fp(&peer.id).is_some();
        let copied = s.copy_text && trusted && self.platform.is_some();
        if copied {
            if let Some(p) = &self.platform {
                p.set_clipboard(text);
            }
        }
        self.emit(&Event::Text { from: &peer, text, transfer: &t.id, copied });
    }

    pub fn peer_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.peer_locks.lock().entry(id.to_string()).or_default().clone()
    }

    pub fn set_settings(&self, s: Settings) {
        s.save(&self.cfg.data_dir);
        *self.settings.write() = s;
        let s = self.settings();
        self.emit(&Event::Settings { settings: &s });
    }

    pub fn rebuild_dialer(&self) -> Result<()> {
        let s = self.settings();
        let d = Dialer::new(&self.identity, s.cipher, self.quic.read().clone(), self.cfg.tuning)?;
        *self.dialer.write() = Arc::new(d);
        Ok(())
    }
}

/// IPv4/IPv6 addresses of this machine that peers could use, skipping
/// loopback and virtual adapters nobody else can reach.
pub fn local_addresses() -> Vec<(IpAddr, String)> {
    let mut out = Vec::new();
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        for i in ifs {
            if i.is_loopback() {
                continue;
            }
            let n = i.name.to_lowercase();
            if ["vethernet", "vmware", "virtualbox", "hyper-v", "docker", "wsl", "zerotier", "tailscale"]
                .iter()
                .any(|v| n.contains(v))
            {
                continue;
            }
            let ip = i.ip();
            match ip {
                IpAddr::V4(v4) if v4.is_link_local() => continue,
                IpAddr::V6(v6) if !is_ula(&v6) => continue,
                _ => {}
            }
            out.push((ip, i.name.clone()));
        }
    }
    out.sort_by_key(|(ip, _)| (crate::peers::classify(*ip), ip.is_ipv6()));
    out
}

fn is_ula(v6: &Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// The public handle every host app holds.
#[derive(Clone)]
pub struct Node {
    pub shared: Arc<Shared>,
}

impl Node {
    /// Starts listeners and discovery on the current tokio runtime.
    pub async fn start(cfg: Config) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let identity = Identity::load_or_create(&cfg.data_dir)?;
        let mut settings = Settings::load(&cfg.data_dir);
        if settings.port == 0 {
            settings.port = crate::settings::DEFAULT_PORT;
        }
        let want_port = if cfg.port != 0 { cfg.port } else { settings.port };
        let dialer = Dialer::new(&identity, settings.cipher, None, cfg.tuning)?;
        let shared = Arc::new(Shared {
            platform: cfg.platform.clone(),
            identity,
            settings: RwLock::new(settings),
            dialer: RwLock::new(Arc::new(dialer)),
            quic: RwLock::new(None),
            events: RwLock::new(None),
            peers: Peers::new(&cfg.data_dir),
            history: History::open(&cfg.data_dir),
            transfers: RwLock::new(HashMap::new()),
            routes: Mutex::new(HashMap::new()),
            prompts: Mutex::new(HashMap::new()),
            partials: Mutex::new(HashMap::new()),
            peer_locks: Mutex::new(HashMap::new()),
            port: AtomicU16::new(0),
            pairing: Default::default(),
            web: Default::default(),
            sources: Mutex::new(HashMap::new()),
            cfg,
        });

        let listener = bind_tcp(want_port).await?;
        let port = listener.local_addr()?.port();
        shared.port.store(port, Ordering::Relaxed);
        match link::quic_endpoint(&shared.identity, shared.settings().cipher, SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port))
            .or_else(|_| link::quic_endpoint(&shared.identity, shared.settings().cipher, SocketAddr::from(([0, 0, 0, 0], port))))
        {
            Ok(ep) => {
                *shared.quic.write() = Some(ep.clone());
                tokio::spawn(accept_quic(shared.clone(), ep));
            }
            Err(e) => tracing::warn!("QUIC unavailable: {e:#}"),
        }
        shared.rebuild_dialer()?;
        tokio::spawn(accept_tcp(shared.clone(), listener));
        tokio::spawn(ticker(shared.clone()));
        if shared.cfg.discovery {
            crate::discovery::start(shared.clone());
        }
        tracing::info!("SweaxDrop {} listening on port {port}", shared.identity.id());
        Ok(Self { shared })
    }

    pub fn set_event_sink(&self, sink: EventSink) {
        *self.shared.events.write() = Some(sink);
        let d = self.shared.device_view();
        self.shared.emit(&Event::Ready { device: &d });
    }

    pub fn port(&self) -> u16 {
        self.shared.port.load(Ordering::Relaxed)
    }

    pub fn id(&self) -> String {
        self.shared.identity.id()
    }

    /// Adds a peer by address (tests, manual entry, QR).
    pub fn add_peer_addr(&self, device: DeviceInfo, addr: SocketAddr, via: Via) {
        if self.shared.peers.upsert(device, None, Some(addr), Some(via), "app") {
            self.shared.emit_peers();
        }
    }

    pub fn transfer(&self, id: &str) -> Option<Arc<Transfer>> {
        self.shared.transfer(id)
    }

    /// Queues a transfer of `sources` (and/or `text`) to a peer and returns its id.
    pub fn send(&self, peer_id: &str, sources: Vec<crate::storage::SourceItem>, text: Option<String>, opts: Option<crate::send::EngineOpts>) -> Result<String> {
        let sh = &self.shared;
        let peer = sh.peers.get(peer_id).ok_or_else(|| anyhow!("unknown device"))?;
        let addrs = sh.peers.addrs(peer_id);
        if addrs.is_empty() {
            return Err(anyhow!("{} is not reachable right now", peer.device.name));
        }
        let s = sh.settings();
        let opts = opts.unwrap_or(crate::send::EngineOpts {
            streams: s.streams,
            chunk: s.chunk_bytes(),
            piece: 512 << 10,
            transport: s.transport,
        });
        // Text travels inside the offer itself; no data stream is opened for it.
        let items: Vec<_> = sources.iter().map(|s| s.item.clone()).collect();
        let id = crate::util::random_id(10);
        let t = Arc::new(Transfer::new(id.clone(), Direction::Send, peer.device.clone(), items, text));
        *t.transport.write() = opts.transport;
        sh.add_transfer(t.clone());
        let sources = Arc::new(sources);
        sh.sources.lock().insert(id.clone(), sources.clone());
        let expect_fp = sh.peers.trusted_fp(peer_id).or(peer.fp);
        let job = crate::send::SendJob { transfer: t.clone(), sources, addrs, expect_fp, opts };
        let me = sh.clone();
        let lock = sh.peer_lock(peer_id);
        tokio::spawn(async move {
            if lock.try_lock().is_err() {
                t.set_phase(Phase::Queued);
                me.emit_transfer(&t);
            }
            let _g = tokio::select! {
                g = lock.lock() => g,
                _ = t.cancel.cancelled() => { t.set_phase(Phase::Cancelled); me.finish_transfer(&t); return; }
            };
            crate::send::run(me, job).await;
        });
        Ok(id)
    }

    pub fn pause(&self, id: &str, paused: bool) -> bool {
        match self.shared.transfer(id) {
            Some(t) if !t.phase().is_final() => {
                t.paused.send_replace(paused);
                true
            }
            _ => false,
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        if self.shared.answer(id, Answer { accept: false, save_dir: None, skip: vec![], trust: false }) {
            return true;
        }
        match self.shared.transfer(id) {
            Some(t) if !t.phase().is_final() => {
                t.cancel.cancel();
                // A receiver waiting for a sender to come back has no task to
                // notice the token; finish it here.
                let key = (t.peer.read().id.clone(), t.id.clone());
                if let Some(st) = self.shared.take_partial(&key) {
                    st.abort(false);
                    t.set_phase(Phase::Cancelled);
                    self.shared.finish_transfer(&t);
                }
                true
            }
            _ => false,
        }
    }
}

async fn bind_tcp(port: u16) -> Result<TcpListener> {
    for p in [port, port.wrapping_add(1), port.wrapping_add(2), port.wrapping_add(3), 0] {
        let sock = socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::STREAM, None);
        let made = sock.and_then(|s| {
            s.set_only_v6(false)?;
            #[cfg(unix)]
            s.set_reuse_address(true)?;
            s.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), p).into())?;
            s.listen(256)?;
            s.set_nonblocking(true)?;
            Ok(s)
        });
        match made {
            Ok(s) => return Ok(TcpListener::from_std(s.into())?),
            Err(_) => {
                if let Ok(l) = TcpListener::bind(("0.0.0.0", p)).await {
                    return Ok(l);
                }
            }
        }
    }
    Err(anyhow!("could not open a listening port"))
}

async fn accept_tcp(shared: Arc<Shared>, listener: TcpListener) {
    let mut acceptor = None;
    loop {
        let (sock, from) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!("accept: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let acc = acceptor
            .get_or_insert_with(|| {
                let cfg = tls::server_config(&shared.identity, shared.settings().cipher).expect("tls config");
                TlsAcceptor::from(Arc::new(cfg))
            })
            .clone();
        let sh = shared.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_tcp(sh, acc, sock, from).await {
                tracing::debug!("connection from {from}: {e:#}");
            }
        });
    }
}

async fn handle_tcp(shared: Arc<Shared>, acc: TlsAcceptor, sock: TcpStream, from: SocketAddr) -> Result<()> {
    let from = SocketAddr::new(canonical(from.ip()), from.port());
    link::tune_tcp(&sock, shared.cfg.tuning);
    let mut b = [0u8; 1];
    let n = tokio::time::timeout(Duration::from_secs(10), sock.peek(&mut b)).await??;
    if n == 0 {
        return Ok(());
    }
    if b[0] != 0x16 {
        // Not TLS: a browser asking for the no-install client.
        return crate::web::serve_http(shared, sock, from).await;
    }
    let tls = acc.accept(sock).await?;
    let fp = tls::peer_fingerprint(tls.get_ref().1.peer_certificates()).ok_or_else(|| anyhow!("no client key"))?;
    let (r, w) = tokio::io::split(tls);
    let s = Stream { read: Box::new(r), write: Box::new(w) };
    dispatch(shared, s, fp, from).await
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

async fn dispatch(shared: Arc<Shared>, mut s: Stream, fp: [u8; 32], from: SocketAddr) -> Result<()> {
    let first = tokio::time::timeout(Duration::from_secs(10), read_ctrl(&mut s.read)).await??;
    match first {
        Ctrl::Attach { token } => {
            let tx = shared.routes.lock().get(&token).cloned();
            match tx {
                Some(tx) => tx.send(s).await.map_err(|_| anyhow!("transfer gone"))?,
                None => return Err(anyhow!("unknown transfer token")),
            }
            Ok(())
        }
        h @ Ctrl::Hello { .. } => crate::recv::serve_session(shared, s, fp, h, from.to_string()).await,
        other => Err(anyhow!("unexpected first frame {other:?}")),
    }
}

async fn accept_quic(shared: Arc<Shared>, ep: quinn::Endpoint) {
    while let Some(inc) = ep.accept().await {
        let sh = shared.clone();
        tokio::spawn(async move {
            let conn = match inc.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("QUIC handshake: {e}");
                    return;
                }
            };
            let Ok(fp) = link::quic_peer_fp(&conn) else { return };
            let from = SocketAddr::new(canonical(conn.remote_address().ip()), conn.remote_address().port());
            while let Ok((w, r)) = conn.accept_bi().await {
                let s = Stream { read: Box::new(r), write: Box::new(QuicW(w)) };
                let sh = sh.clone();
                tokio::spawn(async move {
                    if let Err(e) = dispatch(sh, s, fp, from).await {
                        tracing::debug!("QUIC stream: {e:#}");
                    }
                });
            }
        });
    }
}

struct QuicW(quinn::SendStream);

impl tokio::io::AsyncWrite for QuicW {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf).map_err(std::io::Error::other)
    }
    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.0), cx)
    }
    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut self.0), cx)
    }
}

/// Samples speed for every moving transfer and pushes progress to the UI
/// five times a second.
async fn ticker(shared: Arc<Shared>) {
    let mut iv = tokio::time::interval(Duration::from_millis(200));
    let mut n = 0u64;
    loop {
        iv.tick().await;
        n += 1;
        for t in shared.transfers() {
            let p = t.phase();
            if matches!(p, Phase::Running | Phase::Paused) {
                t.sample();
                shared.emit_transfer(&t);
            }
        }
        if n % 10 == 0 && shared.peers.expire() {
            shared.emit_peers();
        }
    }
}

/// Answers diagnostic probes (round-trip time for the benchmark).
pub async fn probe(_shared: &Shared, ctrl: &mut Stream, kind: &str) -> Result<()> {
    match kind {
        "rtt" => write_ctrl(&mut ctrl.write, &Ctrl::Probe { kind: "rtt".into() }).await,
        _ => Ok(()),
    }
}

pub fn view_transfers(shared: &Shared) -> Vec<TransferView> {
    shared.transfers().iter().map(|t| t.view(false)).collect()
}

pub fn default_transport() -> Transport {
    Transport::Tcp
}
