//! The no-install client: any phone browser that scans the QR code on the
//! PC gets the SweaxDrop UI from this server and can send and receive.
//!
//! It shares the TCP port with the encrypted protocol (the listener looks at
//! the first byte: TLS starts with 0x16, HTTP with a letter). Browsers on a
//! LAN cannot do TLS to a self-signed certificate without a scary warning,
//! so this path is plain HTTP guarded by a 128-bit secret from the QR code
//! and per-session tokens; the app-to-app path is the encrypted one.
//!
//! Uploads arrive as parallel PUTs of fixed-size chunks written at their
//! offsets, so a dropped request is retried alone. Downloads support Range
//! (Safari resumes) and folders stream as a zip built on the fly.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::node::{Node, Shared};
use crate::proto::{DeviceInfo, Item};
use crate::recv::{chunk_len, RecvState};
use crate::storage::{Reader, SourceItem};
use crate::transfer::{Direction, Phase, Transfer};
use crate::util::random_id;

type Body = BoxBody<Bytes, std::io::Error>;

pub const WEB_CHUNK: u32 = 4 << 20;

pub struct WebSession {
    pub token: String,
    pub peer_id: String,
    pub device: DeviceInfo,
    last: Mutex<Instant>,
    subs: Mutex<Vec<mpsc::UnboundedSender<String>>>,
}

struct Outgoing {
    t: Arc<Transfer>,
    session: String,
    sources: Arc<Vec<SourceItem>>,
    served: Mutex<Vec<u64>>,
}

#[derive(Default)]
pub struct WebState {
    sessions: Mutex<HashMap<String, Arc<WebSession>>>,
    uploads: Mutex<HashMap<String, (String, Arc<RecvState>)>>,
    outgoing: Mutex<HashMap<String, Arc<Outgoing>>>,
    failures: Mutex<Vec<Instant>>,
}

impl WebState {
    fn session(&self, token: &str) -> Option<Arc<WebSession>> {
        let s = self.sessions.lock().get(token).cloned()?;
        *s.last.lock() = Instant::now();
        Some(s)
    }

    /// Sends one event to every open event stream of a browser peer.
    pub fn push(&self, peer_id: &str, ev: &str) {
        for s in self.sessions.lock().values() {
            if s.peer_id == peer_id {
                s.subs.lock().retain(|tx| tx.send(ev.to_string()).is_ok());
            }
        }
    }

    pub fn connected(&self) -> Vec<Value> {
        self.sessions
            .lock()
            .values()
            .filter(|s| !s.subs.lock().is_empty())
            .map(|s| json!({"peer": s.peer_id, "name": s.device.name, "platform": s.device.platform}))
            .collect()
    }
}

pub fn info(node: &Node) -> Value {
    json!({"url": crate::api::qr_uri(node), "clients": node.shared.web.connected(), "enabled": node.shared.settings().web_client})
}

fn full(status: StatusCode, ctype: &str, body: impl Into<Bytes>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", ctype)
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Full::new(body.into()).map_err(|e: Infallible| match e {}).boxed())
        .unwrap()
}

fn json_resp(status: StatusCode, v: Value) -> Response<Body> {
    full(status, "application/json", v.to_string())
}

fn err(status: StatusCode, msg: &str) -> Response<Body> {
    json_resp(status, json!({"ok": false, "error": msg}))
}

pub async fn serve_http(shared: Arc<Shared>, sock: TcpStream, from: SocketAddr) -> Result<()> {
    let svc = hyper::service::service_fn(move |req| {
        let sh = shared.clone();
        async move { Ok::<_, Infallible>(route(sh, req, from).await) }
    });
    hyper::server::conn::http1::Builder::new()
        .keep_alive(true)
        .serve_connection(TokioIo::new(sock), svc)
        .await?;
    Ok(())
}

fn query(req: &Request<Incoming>) -> HashMap<String, String> {
    req.uri()
        .query()
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

async fn read_json(req: Request<Incoming>, limit: usize) -> Result<Value> {
    let body = http_body_util::Limited::new(req.into_body(), limit).collect().await.map_err(|e| anyhow!("{e}"))?;
    Ok(serde_json::from_slice(&body.to_bytes())?)
}

async fn route(shared: Arc<Shared>, req: Request<Incoming>, from: SocketAddr) -> Response<Body> {
    if !shared.settings().web_client {
        return full(StatusCode::FORBIDDEN, "text/plain; charset=utf-8", "The browser client is turned off on this computer.");
    }
    let path = req.uri().path().to_string();
    let r = if let Some(api) = path.strip_prefix("/api/w/") {
        let api = api.to_string();
        api_route(&shared, req, &api, from).await
    } else if req.method() == Method::GET || req.method() == Method::HEAD {
        Ok(static_file(&path))
    } else {
        Ok(err(StatusCode::NOT_FOUND, "not found"))
    };
    r.unwrap_or_else(|e| err(StatusCode::BAD_REQUEST, &format!("{e:#}")))
}

fn session_of(shared: &Shared, req: &Request<Incoming>) -> Option<Arc<WebSession>> {
    let tok = req
        .headers()
        .get("x-swx-session")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .or_else(|| query(req).get("s").cloned())?;
    shared.web.session(&tok)
}

async fn api_route(shared: &Arc<Shared>, req: Request<Incoming>, api: &str, from: SocketAddr) -> Result<Response<Body>> {
    let parts: Vec<&str> = api.split('/').collect();
    match (req.method().clone(), parts.as_slice()) {
        (Method::POST, ["hello"]) => hello(shared, req, from).await,
        (Method::GET, ["events"]) => {
            let Some(s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            Ok(events(shared, s))
        }
        (Method::POST, ["offer"]) => {
            let Some(s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            let v = read_json(req, 32 << 20).await?;
            offer_from_browser(shared, &s, v).await
        }
        (Method::PUT, ["up", tid, item, index]) => {
            let Some(_s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            let (tid, item, index) = (tid.to_string(), item.parse::<u32>()?, index.parse::<u32>()?);
            upload_chunk(shared, req, &tid, item, index).await
        }
        (Method::POST, ["text"]) => {
            let Some(s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            let v = read_json(req, 1 << 20).await?;
            let text = v["text"].as_str().unwrap_or_default().chars().take(200_000).collect::<String>();
            let t = Arc::new(Transfer::new(random_id(10), Direction::Receive, s.device.clone(), vec![], Some(text.clone())));
            shared.add_transfer(t.clone());
            t.set_phase(Phase::Running);
            shared.text_received(&t, &text);
            t.set_phase(Phase::Done);
            shared.finish_transfer(&t);
            Ok(json_resp(StatusCode::OK, json!({"ok": true})))
        }
        (Method::POST, ["cancel", tid]) => {
            let Some(_s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            if let Some(t) = shared.transfer(tid) {
                if let Some((_, st)) = shared.web.uploads.lock().remove(*tid) {
                    st.abort(false);
                }
                shared.web.outgoing.lock().remove(*tid);
                t.set_phase(Phase::Cancelled);
                shared.finish_transfer(&t);
            }
            Ok(json_resp(StatusCode::OK, json!({"ok": true})))
        }
        (Method::POST, ["answer", tid]) => {
            let Some(_s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            let v = read_json(req, 1 << 16).await?;
            if let Some(t) = shared.transfer(tid) {
                if v["accept"].as_bool() == Some(true) {
                    t.set_phase(Phase::Running);
                } else {
                    *t.error.write() = Some("declined".into());
                    t.set_phase(Phase::Declined);
                    shared.web.outgoing.lock().remove(*tid);
                    shared.finish_transfer(&t);
                }
                shared.emit_transfer(&t);
            }
            Ok(json_resp(StatusCode::OK, json!({"ok": true})))
        }
        (Method::GET | Method::HEAD, ["dl", tid, item, ..]) => {
            let Some(_s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            let (tid, item) = (tid.to_string(), item.parse::<u32>()?);
            download(shared, req, &tid, item).await
        }
        (Method::GET, ["zip", tid, ..]) => {
            let Some(_s) = session_of(shared, &req) else { return Ok(err(StatusCode::UNAUTHORIZED, "session expired")) };
            zip_download(shared, &tid.to_string())
        }
        _ => Ok(err(StatusCode::NOT_FOUND, "unknown request")),
    }
}

async fn hello(shared: &Arc<Shared>, req: Request<Incoming>, from: SocketAddr) -> Result<Response<Body>> {
    {
        let mut f = shared.web.failures.lock();
        f.retain(|t| t.elapsed() < Duration::from_secs(60));
        if f.len() > 20 {
            return Ok(err(StatusCode::TOO_MANY_REQUESTS, "too many attempts, wait a minute"));
        }
    }
    let key = req.headers().get("x-swx-key").and_then(|h| h.to_str().ok()).unwrap_or_default().to_string();
    let v = read_json(req, 1 << 16).await?;
    // A known session may reconnect without the (possibly rotated) QR secret.
    if let Some(tok) = v["session"].as_str() {
        if let Some(s) = shared.web.session(tok) {
            return Ok(json_resp(StatusCode::OK, json!({"ok": true, "session": s.token, "peer": s.peer_id, "device": shared.device(), "chunk": WEB_CHUNK})));
        }
    }
    if !shared.pairing.qr_valid(&key) {
        shared.web.failures.lock().push(Instant::now());
        return Ok(err(StatusCode::UNAUTHORIZED, "this code expired — scan the QR code on the computer again"));
    }
    let name = v["name"].as_str().unwrap_or("Phone").chars().take(48).collect::<String>();
    let platform = v["platform"].as_str().unwrap_or("web").to_string();
    let token = random_id(20);
    let peer_id = format!("web{}", &token[..13]).to_uppercase();
    let device = DeviceInfo { id: peer_id.clone(), name: format!("{name} · browser"), platform, model: "browser".into(), version: String::new() };
    let s = Arc::new(WebSession { token: token.clone(), peer_id: peer_id.clone(), device: device.clone(), last: Mutex::new(Instant::now()), subs: Mutex::new(Vec::new()) });
    shared.web.sessions.lock().insert(token.clone(), s);
    let port = shared.port.load(Ordering::Relaxed);
    shared.peers.upsert(device.clone(), None, Some(SocketAddr::new(from.ip(), port)), None, "web");
    shared.emit_peers();
    shared.emit(&crate::node::Event::Web { state: "connected", name: &device.name });
    Ok(json_resp(StatusCode::OK, json!({"ok": true, "session": token, "peer": peer_id, "device": shared.device(), "chunk": WEB_CHUNK})))
}

fn events(shared: &Arc<Shared>, s: Arc<WebSession>) -> Response<Body> {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let hello = json!({"type": "hello", "device": shared.device(), "peer": s.peer_id}).to_string();
    let _ = tx.send(hello);
    // Offers that were made while the page was closed.
    for o in shared.web.outgoing.lock().values() {
        if o.session == s.token && matches!(o.t.phase(), Phase::Waiting) {
            let _ = tx.send(offer_event(o));
        }
    }
    s.subs.lock().push(tx.clone());
    let sh = shared.clone();
    let peer = s.peer_id.clone();
    let dev = s.device.clone();
    tokio::spawn(async move {
        // Keep-alive comments; also keeps the peer marked online.
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if tx.send(String::new()).is_err() {
                break;
            }
            sh.peers.upsert(dev.clone(), None, None, None, "web");
        }
        sh.emit(&crate::node::Event::Web { state: "disconnected", name: &dev.name });
        let _ = peer;
    });
    let stream = tokio_stream_from(rx).map(|ev| {
        let line = if ev.is_empty() { ": keep-alive\n\n".to_string() } else { format!("data: {ev}\n\n") };
        Ok::<_, std::io::Error>(Frame::data(Bytes::from(line)))
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("x-accel-buffering", "no")
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .unwrap()
}

fn tokio_stream_from<T: Send + 'static>(mut rx: mpsc::UnboundedReceiver<T>) -> impl futures_util::Stream<Item = T> + Send {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

async fn offer_from_browser(shared: &Arc<Shared>, s: &WebSession, v: Value) -> Result<Response<Body>> {
    let mut items: Vec<Item> = serde_json::from_value(v["items"].clone()).unwrap_or_default();
    for (i, it) in items.iter_mut().enumerate() {
        it.id = i as u32;
        it.dir = false;
        if it.mime.is_none() {
            it.mime = crate::util::mime_for(&it.path).map(str::to_string);
        }
    }
    if items.is_empty() {
        return Ok(err(StatusCode::BAD_REQUEST, "no files"));
    }
    let t = Arc::new(Transfer::new(random_id(10), Direction::Receive, s.device.clone(), items, None));
    shared.add_transfer(t.clone());
    // The QR code was on this computer's screen, so whoever scanned it is
    // treated like a paired device: accepted without another prompt.
    let sink = shared.default_sink(None);
    *t.dest.write() = sink.describe();
    let st = RecvState::new(t.clone(), s.device.clone(), WEB_CHUNK, sink, &[])?;
    t.lanes_used.store(4, Ordering::Relaxed);
    t.set_phase(Phase::Running);
    shared.emit_transfer(&t);
    shared.web.uploads.lock().insert(t.id.clone(), (s.token.clone(), st));
    Ok(json_resp(StatusCode::OK, json!({"ok": true, "transfer": t.id, "chunk": WEB_CHUNK})))
}

async fn upload_chunk(shared: &Arc<Shared>, req: Request<Incoming>, tid: &str, item: u32, index: u32) -> Result<Response<Body>> {
    let st = match shared.web.uploads.lock().get(tid) {
        Some((_, st)) => st.clone(),
        None => return Ok(err(StatusCode::NOT_FOUND, "unknown transfer")),
    };
    let size = st.item(item).map(|(_, it)| it.size).ok_or_else(|| anyhow!("unknown file"))?;
    let want = chunk_len(size, st.chunk, index) as u64;
    if index as u64 * st.chunk as u64 > size.max(1) - 1 && !(size == 0 && index == 0) {
        return Ok(err(StatusCode::BAD_REQUEST, "chunk out of range"));
    }
    let lane = (index % 4) as usize;
    let file = st.file(item)?;
    let mut body = req.into_body();
    let mut pos = 0u64;
    let base = index as u64 * st.chunk as u64;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| anyhow!("upload broke: {e}"))?;
        if let Ok(data) = frame.into_data() {
            if pos + data.len() as u64 > want {
                return Ok(err(StatusCode::BAD_REQUEST, "chunk too long"));
            }
            if let Some(f) = &file {
                let f = f.clone();
                let off = base + pos;
                let d = data.clone();
                tokio::task::spawn_blocking(move || f.write_at(&d, off)).await??;
            }
            pos += data.len() as u64;
            st.t.add_bytes(lane, data.len() as u64);
        }
    }
    if pos != want {
        st.t.sub_bytes(pos);
        return Ok(err(StatusCode::BAD_REQUEST, "chunk incomplete"));
    }
    st.chunk_ok(item, index);
    if st.complete() {
        shared.web.uploads.lock().remove(tid);
        let failed = st.failed_items();
        if failed.is_empty() {
            st.t.set_phase(Phase::Done);
        } else {
            st.t.fail(format!("{} file(s) could not be saved", failed.len()));
        }
        shared.finish_transfer(&st.t);
    }
    Ok(json_resp(StatusCode::OK, json!({"ok": true})))
}

fn offer_event(o: &Outgoing) -> String {
    let files: Vec<Value> = o
        .sources
        .iter()
        .filter(|s| !s.item.dir)
        .map(|s| {
            let name = s.item.path.rsplit('/').next().unwrap_or("file");
            json!({"id": s.item.id, "path": s.item.path, "size": s.item.size, "mime": s.item.mime,
                   "url": format!("/api/w/dl/{}/{}/{}", o.t.id, s.item.id, crate::api::urlencode(name))})
        })
        .collect();
    json!({"type": "web_offer", "transfer": o.t.view(false), "files": files, "text": o.t.text,
           "zip": format!("/api/w/zip/{}/SweaxDrop-{}.zip", o.t.id, &o.t.id[..6])})
    .to_string()
}

/// PC → browser: the browser gets an offer on its event stream and pulls the files.
pub fn offer_to_browser(shared: &Arc<Shared>, peer: &str, sources: Vec<SourceItem>, text: Option<String>) -> Result<String> {
    let session = shared
        .web
        .sessions
        .lock()
        .values()
        .find(|s| s.peer_id == peer)
        .cloned()
        .ok_or_else(|| anyhow!("that browser is no longer connected"))?;
    let items: Vec<Item> = sources.iter().map(|s| s.item.clone()).collect();
    let t = Arc::new(Transfer::new(random_id(10), Direction::Send, session.device.clone(), items, text));
    let n = sources.len();
    let o = Arc::new(Outgoing { t: t.clone(), session: session.token.clone(), sources: Arc::new(sources), served: Mutex::new(vec![0; n]) });
    shared.add_transfer(t.clone());
    t.set_phase(Phase::Waiting);
    shared.emit_transfer(&t);
    let ev = offer_event(&o);
    shared.web.outgoing.lock().insert(t.id.clone(), o);
    shared.web.push(peer, &ev);
    if t.items.is_empty() {
        // Text needs no download.
        t.set_phase(Phase::Running);
        t.set_phase(Phase::Done);
        shared.finish_transfer(&t);
    }
    Ok(t.id.clone())
}

fn content_disposition(name: &str, inline: bool) -> String {
    let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_ ()".contains(c) { c } else { '_' }).collect();
    format!("{}; filename=\"{ascii}\"; filename*=UTF-8''{}", if inline { "inline" } else { "attachment" }, crate::api::urlencode(name))
}

async fn download(shared: &Arc<Shared>, req: Request<Incoming>, tid: &str, item: u32) -> Result<Response<Body>> {
    let o = match shared.web.outgoing.lock().get(tid) {
        Some(o) => o.clone(),
        None => return Ok(err(StatusCode::GONE, "this transfer is over")),
    };
    let idx = o.sources.iter().position(|s| s.item.id == item).ok_or_else(|| anyhow!("unknown file"))?;
    let src = o.sources[idx].clone();
    let size = src.item.size;
    let inline = query(&req).contains_key("inline");
    let (start, end) = match req.headers().get("range").and_then(|h| h.to_str().ok()).and_then(parse_range) {
        Some((a, b)) => (a.min(size), b.map(|b| b + 1).unwrap_or(size).min(size)),
        None => (0, size),
    };
    let partial = start > 0 || end < size;
    let name = src.item.path.rsplit('/').next().unwrap_or("file").to_string();
    let mut b = Response::builder()
        .status(if partial { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header("content-type", src.item.mime.clone().unwrap_or_else(|| "application/octet-stream".into()))
        .header("content-length", end - start)
        .header("accept-ranges", "bytes")
        .header("content-disposition", content_disposition(&name, inline))
        .header("cache-control", "no-store");
    if partial {
        b = b.header("content-range", format!("bytes {start}-{}/{size}", end.saturating_sub(1)));
    }
    if req.method() == Method::HEAD {
        return Ok(b.body(Full::new(Bytes::new()).map_err(|e: Infallible| match e {}).boxed()).unwrap());
    }
    if o.t.phase() == Phase::Waiting {
        o.t.set_phase(Phase::Running);
    }
    let (tx, rx) = mpsc::channel::<std::io::Result<Bytes>>(8);
    let platform = shared.platform.clone();
    let sh = shared.clone();
    let oc = o.clone();
    std::thread::spawn(move || {
        let mut r = match Reader::open(&src.src, platform.as_ref()) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.blocking_send(Err(std::io::Error::other(e.to_string())));
                return;
            }
        };
        let mut off = start;
        while off < end {
            let n = ((end - off) as usize).min(256 << 10);
            let mut buf = vec![0u8; n];
            match r.read_full_at(&mut buf, off) {
                Ok(got) if got == n => {}
                _ => {
                    let _ = tx.blocking_send(Err(std::io::Error::other("file changed")));
                    return;
                }
            }
            if tx.blocking_send(Ok(Bytes::from(buf))).is_err() {
                return;
            }
            off += n as u64;
            oc.t.add_bytes(idx % 4, n as u64);
            let mut served = oc.served.lock();
            served[idx] = served[idx].max(off);
        }
        // Done when every file was served to its end at least once.
        let all = oc.served.lock().iter().zip(oc.sources.iter()).all(|(s, src)| *s >= src.item.size);
        if all && !oc.t.phase().is_final() {
            oc.t.bytes.store(oc.t.total, Ordering::Relaxed);
            oc.t.files_done.store(oc.sources.len() as u32, Ordering::Relaxed);
            oc.t.set_phase(Phase::Done);
            sh.finish_transfer(&oc.t);
        }
    });
    let stream = tokio_stream_from_bounded(rx).map(|r| r.map(Frame::data));
    Ok(b.body(BodyExt::boxed(StreamBody::new(stream))).unwrap())
}

fn tokio_stream_from_bounded<T: Send + 'static>(mut rx: mpsc::Receiver<T>) -> impl futures_util::Stream<Item = T> + Send {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

fn parse_range(h: &str) -> Option<(u64, Option<u64>)> {
    let r = h.strip_prefix("bytes=")?;
    let (a, b) = r.split_once('-')?;
    let a: u64 = a.trim().parse().ok()?;
    let b = b.trim();
    Some((a, if b.is_empty() { None } else { b.parse().ok() }))
}

/// Streams every file of the transfer as one zip ("stored", no compression:
/// photos and videos do not compress). CRCs are computed just before each
/// entry, so headers carry real sizes and need no data descriptors, which
/// keeps the archive readable by the iOS Files app.
fn zip_download(shared: &Arc<Shared>, tid: &str) -> Result<Response<Body>> {
    let o = match shared.web.outgoing.lock().get(tid) {
        Some(o) => o.clone(),
        None => return Ok(err(StatusCode::GONE, "this transfer is over")),
    };
    if o.t.phase() == Phase::Waiting {
        o.t.set_phase(Phase::Running);
    }
    let (tx, rx) = mpsc::channel::<std::io::Result<Bytes>>(8);
    let platform = shared.platform.clone();
    let sh = shared.clone();
    std::thread::spawn(move || {
        let res = write_zip(&o, platform.as_ref(), |b| tx.blocking_send(Ok(b)).map_err(|_| std::io::Error::other("closed")));
        match res {
            Ok(()) => {
                o.t.files_done.store(o.sources.len() as u32, Ordering::Relaxed);
                o.t.set_phase(Phase::Done);
                sh.finish_transfer(&o.t);
            }
            Err(e) => {
                let _ = tx.blocking_send(Err(e));
            }
        }
    });
    let stream = tokio_stream_from_bounded(rx).map(|r| r.map(Frame::data));
    Ok(Response::builder()
        .header("content-type", "application/zip")
        .header("content-disposition", content_disposition(&format!("SweaxDrop-{}.zip", &tid[..6.min(tid.len())]), false))
        .header("cache-control", "no-store")
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .unwrap())
}

fn dos_time(ms: Option<i64>) -> (u16, u16) {
    let secs = ms.unwrap_or(0) / 1000;
    if secs <= 315_532_800 {
        return (0, (1 << 5) | 1); // 1980-01-01
    }
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    let time = ((h << 11) | (m << 5) | (s / 2)) as u16;
    let date = (((y - 1980).clamp(0, 127) << 9) | (mo << 5) | d) as u16;
    (time, date)
}

fn write_zip(o: &Outgoing, platform: Option<&Arc<dyn crate::storage::Platform>>, mut out: impl FnMut(Bytes) -> std::io::Result<()>) -> std::io::Result<()> {
    struct Entry {
        name: Vec<u8>,
        crc: u32,
        size: u64,
        offset: u64,
        time: (u16, u16),
    }
    let mut entries = Vec::new();
    let mut offset = 0u64;
    for (i, s) in o.sources.iter().enumerate() {
        if s.item.dir {
            continue;
        }
        let mut r = Reader::open(&s.src, platform).map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut crc = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut off = 0u64;
        while off < s.item.size {
            let n = ((s.item.size - off) as usize).min(buf.len());
            r.read_full_at(&mut buf[..n], off)?;
            crc.update(&buf[..n]);
            off += n as u64;
        }
        let crc = crc.finalize();
        let name = s.item.path.as_bytes().to_vec();
        let time = dos_time(s.item.mtime);
        let zip64 = s.item.size >= 0xFFFF_FFFF || offset >= 0xFFFF_FFFF;
        let mut h = Vec::with_capacity(64 + name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&(if zip64 { 45u16 } else { 20 }).to_le_bytes());
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&time.0.to_le_bytes());
        h.extend_from_slice(&time.1.to_le_bytes());
        h.extend_from_slice(&crc.to_le_bytes());
        let sz32 = if zip64 { 0xFFFF_FFFFu32 } else { s.item.size as u32 };
        h.extend_from_slice(&sz32.to_le_bytes());
        h.extend_from_slice(&sz32.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&(if zip64 { 20u16 } else { 0 }).to_le_bytes());
        h.extend_from_slice(&name);
        if zip64 {
            h.extend_from_slice(&1u16.to_le_bytes());
            h.extend_from_slice(&16u16.to_le_bytes());
            h.extend_from_slice(&s.item.size.to_le_bytes());
            h.extend_from_slice(&s.item.size.to_le_bytes());
        }
        let hlen = h.len() as u64;
        out(Bytes::from(h))?;
        let mut off = 0u64;
        while off < s.item.size {
            let n = ((s.item.size - off) as usize).min(256 << 10);
            let mut b = vec![0u8; n];
            r.read_full_at(&mut b, off)?;
            out(Bytes::from(b))?;
            off += n as u64;
            o.t.add_bytes(i % 4, n as u64);
        }
        entries.push(Entry { name, crc, size: s.item.size, offset, time });
        offset += hlen + s.item.size;
    }
    let cd_start = offset;
    let mut cd = Vec::new();
    for e in &entries {
        let zip64 = e.size >= 0xFFFF_FFFF || e.offset >= 0xFFFF_FFFF;
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&(if zip64 { 45u16 } else { 20 }).to_le_bytes());
        cd.extend_from_slice(&(if zip64 { 45u16 } else { 20 }).to_le_bytes());
        cd.extend_from_slice(&0x0800u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&e.time.0.to_le_bytes());
        cd.extend_from_slice(&e.time.1.to_le_bytes());
        cd.extend_from_slice(&e.crc.to_le_bytes());
        let sz32 = if zip64 { 0xFFFF_FFFFu32 } else { e.size as u32 };
        cd.extend_from_slice(&sz32.to_le_bytes());
        cd.extend_from_slice(&sz32.to_le_bytes());
        cd.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&(if zip64 { 28u16 } else { 0 }).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes()); // comment
        cd.extend_from_slice(&0u16.to_le_bytes()); // disk
        cd.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        cd.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        cd.extend_from_slice(&(if zip64 { 0xFFFF_FFFFu32 } else { e.offset as u32 }).to_le_bytes());
        cd.extend_from_slice(&e.name);
        if zip64 {
            cd.extend_from_slice(&1u16.to_le_bytes());
            cd.extend_from_slice(&24u16.to_le_bytes());
            cd.extend_from_slice(&e.size.to_le_bytes());
            cd.extend_from_slice(&e.size.to_le_bytes());
            cd.extend_from_slice(&e.offset.to_le_bytes());
        }
    }
    let cd_len = cd.len() as u64;
    let n = entries.len() as u64;
    let need64 = cd_start >= 0xFFFF_FFFF || n >= 0xFFFF || entries.iter().any(|e| e.size >= 0xFFFF_FFFF || e.offset >= 0xFFFF_FFFF);
    if need64 {
        let z64_at = cd_start + cd_len;
        cd.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        cd.extend_from_slice(&44u64.to_le_bytes());
        cd.extend_from_slice(&45u16.to_le_bytes());
        cd.extend_from_slice(&45u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&n.to_le_bytes());
        cd.extend_from_slice(&n.to_le_bytes());
        cd.extend_from_slice(&cd_len.to_le_bytes());
        cd.extend_from_slice(&cd_start.to_le_bytes());
        cd.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&z64_at.to_le_bytes());
        cd.extend_from_slice(&1u32.to_le_bytes());
    }
    cd.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    let n16 = if need64 { 0xFFFF } else { n as u16 };
    cd.extend_from_slice(&n16.to_le_bytes());
    cd.extend_from_slice(&n16.to_le_bytes());
    cd.extend_from_slice(&(if need64 { 0xFFFF_FFFFu32 } else { cd_len as u32 }).to_le_bytes());
    cd.extend_from_slice(&(if need64 { 0xFFFF_FFFFu32 } else { cd_start as u32 }).to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    out(Bytes::from(cd))
}

// ------------------------------------------------------------------ static UI

#[cfg(feature = "embed-ui")]
#[derive(rust_embed::Embed)]
#[folder = "../../ui/dist"]
struct Ui;

fn ctype(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "json" | "webmanifest" => "application/json",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn static_file(path: &str) -> Response<Body> {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.contains("..") {
        return err(StatusCode::NOT_FOUND, "not found");
    }
    let found: Option<Vec<u8>> = {
        #[cfg(feature = "embed-ui")]
        {
            Ui::get(rel).map(|f| f.data.to_vec())
        }
        #[cfg(not(feature = "embed-ui"))]
        {
            std::env::var_os("SWX_UI_DIR").and_then(|d| std::fs::read(std::path::Path::new(&d).join(rel)).ok())
        }
    };
    match found {
        Some(b) => {
            let mut r = full(StatusCode::OK, ctype(rel), b);
            if rel.starts_with("assets/") {
                r.headers_mut().insert("cache-control", "public, max-age=31536000, immutable".parse().unwrap());
            }
            r
        }
        None if !rel.contains('.') => static_file("/index.html"),
        None => full(StatusCode::NOT_FOUND, "text/html; charset=utf-8", "<!doctype html><title>SweaxDrop</title><p>The browser client is not bundled in this build.</p>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time_is_sane() {
        // 2024-03-05 14:30:20 UTC
        let (t, d) = dos_time(Some(1_709_649_020_000));
        assert_eq!(d >> 9, 2024 - 1980);
        assert_eq!((d >> 5) & 15, 3);
        assert_eq!(d & 31, 5);
        assert_eq!(t >> 11, 14);
        assert_eq!((t >> 5) & 63, 30);
        assert_eq!(parse_range("bytes=100-"), Some((100, None)));
        assert_eq!(parse_range("bytes=0-99"), Some((0, Some(99))));
    }
}
