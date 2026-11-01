//! Sending side of the engine.
//!
//! The work is a queue of units (one chunk of one file). Each lane is a data
//! stream with a reader thread (disk read + BLAKE3) feeding an async writer
//! (TLS + socket), so disk, hashing and network overlap. Small files are one
//! unit each and follow each other on the same lane without waiting for any
//! answer: there is no per-file round trip, which is where request-per-file
//! designs lose most of their time with thousands of photos.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::identity::device_id;
use crate::link::{Link, Stream, Transport};
use crate::node::Shared;
use crate::proto::{bits_from_ranges, chunk_count, Ctrl, Offer, PROTO_VERSION};
use crate::storage::{Reader, SourceItem};
use crate::transfer::{Phase, Transfer, MAX_LANES};
use crate::wire::{read_ctrl, write_ctrl, ChunkHeader, TAG_END};

#[derive(Clone, Copy, Debug)]
pub struct EngineOpts {
    pub streams: u8,
    pub chunk: u32,
    pub piece: usize,
    pub transport: Transport,
}

impl Default for EngineOpts {
    fn default() -> Self {
        Self { streams: 4, chunk: 4 << 20, piece: 512 << 10, transport: Transport::Tcp }
    }
}

pub struct SendJob {
    pub transfer: Arc<Transfer>,
    pub sources: Arc<Vec<SourceItem>>,
    pub addrs: Vec<SocketAddr>,
    pub expect_fp: Option<[u8; 32]>,
    pub opts: EngineOpts,
}

enum Outcome {
    Done,
    Declined(String),
    Cancelled,
}

#[derive(Clone, Copy)]
struct Unit {
    src: usize,
    index: u32,
    offset: u64,
    len: u32,
}

struct Queue {
    units: Mutex<VecDeque<Unit>>,
}

impl Queue {
    fn pop(&self) -> Option<Unit> {
        self.units.lock().pop_front()
    }
    fn push_front(&self, u: Unit) {
        self.units.lock().push_front(u);
    }
    fn is_empty(&self) -> bool {
        self.units.lock().is_empty()
    }
}

struct Ctx {
    t: Arc<Transfer>,
    sources: Arc<Vec<SourceItem>>,
    queue: Queue,
    piece: usize,
    platform: Option<Arc<dyn crate::storage::Platform>>,
    item_errors: mpsc::UnboundedSender<(u32, String)>,
    /// Recycled piece buffers: the writer returns them, readers reuse them.
    pool: Mutex<Vec<Vec<u8>>>,
}

pub async fn run(shared: Arc<Shared>, job: SendJob) {
    let t = job.transfer.clone();
    let mut resume = false;
    let mut accepted_once = false;
    let backoff = [1u64, 2, 3, 5, 8, 13, 20, 30];
    loop {
        t.attempts.fetch_add(1, Ordering::Relaxed);
        let res = attempt(&shared, &job, resume, &mut accepted_once).await;
        match res {
            Ok(Outcome::Done) => {
                t.set_phase(Phase::Done);
                break;
            }
            Ok(Outcome::Declined(r)) => {
                *t.error.write() = Some(r);
                t.set_phase(Phase::Declined);
                break;
            }
            Ok(Outcome::Cancelled) => {
                t.set_phase(Phase::Cancelled);
                break;
            }
            Err(e) => {
                if t.cancel.is_cancelled() {
                    t.set_phase(Phase::Cancelled);
                    break;
                }
                let n = t.attempts.load(Ordering::Relaxed) as usize;
                if !accepted_once || n > backoff.len() {
                    tracing::warn!("send {} failed: {e:#}", t.id);
                    t.fail(format!("{e:#}"));
                    break;
                }
                tracing::info!("send {} interrupted ({e:#}), resuming", t.id);
                *t.error.write() = Some(format!("{e:#}"));
                t.set_phase(Phase::Reconnecting);
                shared.emit_transfer(&t);
                let wait = Duration::from_secs(backoff[n - 1]);
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = t.cancel.cancelled() => { t.set_phase(Phase::Cancelled); break; }
                }
                resume = true;
            }
        }
    }
    shared.finish_transfer(&t);
}

async fn connect_any(shared: &Shared, addrs: &[SocketAddr], transport: Transport) -> Result<(Link, Stream, [u8; 32])> {
    if addrs.is_empty() {
        bail!("no address for this device");
    }
    let dialer = shared.dialer();
    // Happy eyeballs: start one attempt every 200 ms, keep the first that works.
    let mut set = JoinSet::new();
    for (i, a) in addrs.iter().copied().enumerate() {
        let d = dialer.clone();
        set.spawn(async move {
            tokio::time::sleep(Duration::from_millis(200 * i as u64)).await;
            d.connect(a, transport).await.map_err(|e| anyhow!("{a}: {e:#}"))
        });
    }
    let mut last = anyhow!("could not reach the device");
    while let Some(r) = set.join_next().await {
        match r {
            Ok(Ok(v)) => {
                set.abort_all();
                return Ok(v);
            }
            Ok(Err(e)) => last = e,
            Err(e) => last = anyhow!("{e}"),
        }
    }
    Err(last)
}

async fn attempt(shared: &Arc<Shared>, job: &SendJob, resume: bool, accepted_once: &mut bool) -> Result<Outcome> {
    let t = &job.transfer;
    t.set_phase(Phase::Connecting);
    shared.emit_transfer(t);
    let mut transport = job.opts.transport;
    let conn = match connect_any(shared, &job.addrs, transport).await {
        Ok(c) => c,
        Err(e) if transport == Transport::Quic => {
            // UDP can be filtered where TCP is not; fall back rather than fail.
            tracing::info!("QUIC failed ({e:#}), trying TCP");
            transport = Transport::Tcp;
            connect_any(shared, &job.addrs, transport).await?
        }
        Err(e) => return Err(e),
    };
    let (link, mut ctrl, fp) = conn;
    *t.transport.write() = transport;
    if let Some(exp) = job.expect_fp {
        if fp != exp {
            bail!("this device presented a different key than expected; not sending");
        }
    }
    write_ctrl(&mut ctrl.write, &Ctrl::Hello { proto: PROTO_VERSION, device: shared.device() }).await?;
    let peer = match read_ctrl(&mut ctrl.read).await? {
        Ctrl::Hello { device, .. } => device,
        other => bail!("unexpected greeting: {other:?}"),
    };
    if peer.id != device_id(&fp) {
        bail!("device id does not match its key");
    }
    shared.note_peer_key(&peer, fp);
    *t.peer.write() = peer;

    let items: Vec<_> = job.sources.iter().map(|s| s.item.clone()).collect();
    let offer = Offer {
        transfer: t.id.clone(),
        total: t.total,
        items,
        chunk: job.opts.chunk,
        text: t.text.clone(),
        resume,
    };
    write_ctrl(&mut ctrl.write, &Ctrl::Offer(offer)).await?;
    if !resume {
        t.set_phase(Phase::Waiting);
        shared.emit_transfer(t);
    }

    let reply = tokio::select! {
        r = read_ctrl(&mut ctrl.read) => r?,
        _ = t.cancel.cancelled() => {
            let _ = write_ctrl(&mut ctrl.write, &Ctrl::Cancel { reason: "cancelled by sender".into() }).await;
            return Ok(Outcome::Cancelled);
        }
    };
    let (token, streams, have, skip) = match reply {
        Ctrl::Accept { token, streams, have, skip } => (token, streams, have, skip),
        Ctrl::Decline { reason } => return Ok(Outcome::Declined(reason)),
        other => bail!("unexpected answer: {other:?}"),
    };
    *accepted_once = true;

    if job.sources.iter().all(|s| s.item.dir) {
        // Text, clipboard or empty folders: the answer is the receipt.
        t.set_phase(Phase::Running);
        return Ok(Outcome::Done);
    }

    // Build the queue, leaving out what the receiver already holds.
    let mut units = VecDeque::new();
    let mut already = 0u64;
    for (si, s) in job.sources.iter().enumerate() {
        if s.item.dir {
            continue;
        }
        let n = chunk_count(s.item.size, job.opts.chunk);
        if skip.contains(&s.item.id) {
            continue;
        }
        let held = have.iter().find(|h| h.item == s.item.id).map(|h| bits_from_ranges(&h.ranges, n));
        for idx in 0..n {
            let offset = idx as u64 * job.opts.chunk as u64;
            let len = (s.item.size - offset.min(s.item.size)).min(job.opts.chunk as u64) as u32;
            if held.as_ref().is_some_and(|b| b[idx as usize]) {
                already += len as u64;
                continue;
            }
            units.push_back(Unit { src: si, index: idx, offset, len });
        }
    }
    let skipped: u64 = job.sources.iter().filter(|s| skip.contains(&s.item.id)).map(|s| s.item.size).sum();
    t.bytes.store(already + skipped, Ordering::Relaxed);
    t.resumed.store(already + skipped, Ordering::Relaxed);
    for l in &t.lanes {
        l.store(0, Ordering::Relaxed);
    }

    let (err_tx, mut err_rx) = mpsc::unbounded_channel();
    let ctx = Arc::new(Ctx {
        t: t.clone(),
        sources: job.sources.clone(),
        queue: Queue { units: Mutex::new(units) },
        piece: job.opts.piece,
        platform: shared.platform.clone(),
        item_errors: err_tx,
        pool: Mutex::new(Vec::new()),
    });

    let lanes = (streams.min(job.opts.streams).max(1) as usize).min(MAX_LANES);
    let initial = lanes.min(ctx.queue.units.lock().len().max(1));
    t.lanes_used.store(initial as u32, Ordering::Relaxed);
    t.set_phase(if *t.paused.borrow() { Phase::Paused } else { Phase::Running });
    shared.emit_transfer(t);

    let opened = Arc::new(AtomicU32::new(0));
    let mut set: JoinSet<Result<()>> = JoinSet::new();
    let spawn_lane = |set: &mut JoinSet<Result<()>>, lane: usize| {
        let ctx = ctx.clone();
        let link = link.clone();
        let token = token.clone();
        let opened = opened.clone();
        set.spawn(async move {
            let mut s = link.open().await?;
            opened.fetch_add(1, Ordering::Relaxed);
            write_ctrl(&mut s.write, &Ctrl::Attach { token }).await?;
            run_lane(ctx, lane, s).await
        });
    };
    for lane in 0..initial {
        spawn_lane(&mut set, lane);
    }

    let mut paused_rx = t.paused.subscribe();
    let mut remote_paused = false;
    let mut all_sent = false;
    let result = loop {
        tokio::select! {
            m = read_ctrl(&mut ctrl.read) => {
                match m? {
                    Ctrl::Retry { item, index } => {
                        if let Some(si) = job.sources.iter().position(|s| s.item.id == item) {
                            let s = &job.sources[si];
                            let offset = index as u64 * job.opts.chunk as u64;
                            if offset < s.item.size || (s.item.size == 0 && index == 0) {
                                let len = (s.item.size - offset).min(job.opts.chunk as u64) as u32;
                                t.sub_bytes(len as u64);
                                ctx.queue.push_front(Unit { src: si, index, offset, len });
                                if set.is_empty() {
                                    all_sent = false;
                                    spawn_lane(&mut set, 0);
                                }
                            }
                        }
                    }
                    Ctrl::Pause => { remote_paused = true; t.paused.send_replace(true); t.set_phase(Phase::Paused); shared.emit_transfer(t); }
                    Ctrl::Resume => { remote_paused = false; t.paused.send_replace(false); t.set_phase(Phase::Running); shared.emit_transfer(t); }
                    Ctrl::Cancel { reason } => { *t.error.write() = Some(reason); break Ok(Outcome::Cancelled); }
                    Ctrl::Receipt { ok, failed } => {
                        if ok { break Ok(Outcome::Done); }
                        break Err(anyhow!("{} file(s) could not be saved on the other device", failed.len()));
                    }
                    Ctrl::Ping { t: x } => write_ctrl(&mut ctrl.write, &Ctrl::Pong { t: x }).await?,
                    _ => {}
                }
            }
            r = set.join_next(), if !set.is_empty() => {
                match r {
                    Some(Ok(Ok(()))) => {}
                    Some(Ok(Err(e))) => break Err(e),
                    Some(Err(e)) => break Err(anyhow!("lane stopped: {e}")),
                    None => {}
                }
                if set.is_empty() && ctx.queue.is_empty() && !all_sent {
                    all_sent = true;
                    write_ctrl(&mut ctrl.write, &Ctrl::AllSent { streams: opened.load(Ordering::Relaxed) }).await?;
                }
            }
            Some((item, reason)) = err_rx.recv() => {
                write_ctrl(&mut ctrl.write, &Ctrl::ItemError { item, reason }).await?;
            }
            c = paused_rx.changed() => {
                if c.is_err() { continue; }
                let p = *paused_rx.borrow_and_update();
                if p != remote_paused {
                    remote_paused = p;
                    write_ctrl(&mut ctrl.write, &if p { Ctrl::Pause } else { Ctrl::Resume }).await?;
                }
                t.set_phase(if p { Phase::Paused } else { Phase::Running });
                shared.emit_transfer(t);
            }
            _ = t.cancel.cancelled() => {
                let _ = write_ctrl(&mut ctrl.write, &Ctrl::Cancel { reason: "cancelled by sender".into() }).await;
                break Ok(Outcome::Cancelled);
            }
        }
    };
    set.abort_all();
    let _ = ctrl.write.shutdown().await;
    crate::link::linger(&mut ctrl.read, 2).await;
    link.close();
    result
}

enum Piece {
    Header(ChunkHeader),
    Data(Vec<u8>, usize),
    Trailer([u8; 32]),
}

async fn run_lane(ctx: Arc<Ctx>, lane: usize, s: Stream) -> Result<()> {
    let Stream { read: mut r, write: mut w } = s;
    let (tx, mut rx) = mpsc::channel::<Piece>(8);
    let rctx = ctx.clone();
    let reader = std::thread::Builder::new()
        .name(format!("swx-read-{lane}"))
        .spawn(move || read_units(rctx, tx))?;
    while let Some(p) = rx.recv().await {
        match p {
            Piece::Header(h) => w.write_all(&h.encode()).await?,
            Piece::Data(buf, n) => {
                w.write_all(&buf[..n]).await?;
                ctx.t.add_bytes(lane, n as u64);
                let mut pool = ctx.pool.lock();
                if pool.len() < 64 {
                    pool.push(buf);
                }
            }
            Piece::Trailer(h) => w.write_all(&h).await?,
        }
    }
    w.write_all(&[TAG_END]).await?;
    w.flush().await?;
    let _ = tokio::task::spawn_blocking(move || reader.join()).await;
    // Half-close and wait for the receiver to close its side: the lane only
    // counts as finished once every byte was read on the other end.
    let _ = w.shutdown().await;
    crate::link::linger(&mut r, 60).await;
    Ok(())
}

/// Reader thread: takes units from the shared queue and turns them into
/// pieces. Blocks (in this thread only) while the transfer is paused.
fn read_units(ctx: Arc<Ctx>, tx: mpsc::Sender<Piece>) {
    let mut open: Option<(usize, Reader)> = None;
    let paused = ctx.t.paused.subscribe();
    loop {
        while *paused.borrow() && !ctx.t.cancel.is_cancelled() {
            std::thread::sleep(Duration::from_millis(40));
        }
        if ctx.t.cancel.is_cancelled() || tx.is_closed() {
            return;
        }
        let Some(u) = ctx.queue.pop() else { return };
        let src = &ctx.sources[u.src];
        if open.as_ref().map(|o| o.0) != Some(u.src) {
            match Reader::open(&src.src, ctx.platform.as_ref()) {
                Ok(r) => open = Some((u.src, r)),
                Err(e) => {
                    let _ = ctx.item_errors.send((src.item.id, format!("{e:#}")));
                    open = None;
                    // Still send the unit (as zeros) so the receiver's
                    // bookkeeping stays simple; it drops the item on ItemError.
                }
            }
        }
        let h = ChunkHeader { item: src.item.id, index: u.index, raw_len: u.len, flags: 0, wire_len: u.len };
        if tx.blocking_send(Piece::Header(h)).is_err() {
            return;
        }
        let mut hasher = blake3::Hasher::new();
        let mut left = u.len as usize;
        let mut off = u.offset;
        let mut failed = false;
        while left > 0 {
            let n = left.min(ctx.piece);
            let mut buf = ctx.pool.lock().pop().unwrap_or_else(|| vec![0u8; ctx.piece]);
            if let Some((_, r)) = open.as_mut() {
                match r.read_full_at(&mut buf[..n], off) {
                    Ok(got) if got == n => {}
                    Ok(_) | Err(_) if !failed => {
                        failed = true;
                        let _ = ctx.item_errors.send((src.item.id, "the file changed while it was being sent".into()));
                    }
                    _ => {}
                }
            }
            hasher.update(&buf[..n]);
            if tx.blocking_send(Piece::Data(buf, n)).is_err() {
                return;
            }
            left -= n;
            off += n as u64;
        }
        if tx.blocking_send(Piece::Trailer(*hasher.finalize().as_bytes())).is_err() {
            return;
        }
    }
}
