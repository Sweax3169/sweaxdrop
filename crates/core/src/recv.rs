//! Receiving side of the engine.
//!
//! Each data stream gets an async reader (socket + TLS) and a writer thread
//! (BLAKE3 + positioned writes), so a slow disk never stalls the decryption
//! of the next piece. Chunks are verified one by one; a bad chunk is asked for
//! again, a lost connection keeps every verified chunk for the resume.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinSet;

use crate::identity::device_id;
use crate::link::{BoxRead, Stream};
use crate::node::{Decision, Shared};
use crate::proto::{chunk_count, ranges_from_bits, Ctrl, DeviceInfo, Have, Item, Offer, PROTO_VERSION};
use crate::storage::{OutFile, Sink};
use crate::transfer::{Direction, Phase, Transfer, MAX_LANES};
use crate::util::random_id;
use crate::wire::{read_chunk_header, read_ctrl, write_ctrl, ChunkHeader};

const PIECE: usize = 512 << 10;

struct Slot {
    out: Option<Arc<dyn OutFile>>,
    bits: Vec<bool>,
    have: u32,
    finished: bool,
    failed: bool,
}

/// Everything needed to continue a transfer after the connection drops.
pub struct RecvState {
    pub t: Arc<Transfer>,
    pub sender: DeviceInfo,
    chunk: u32,
    sink: Arc<dyn Sink>,
    pos: HashMap<u32, usize>,
    slots: Vec<Mutex<Slot>>,
    done: AtomicU32,
    needed: u32,
    ctrl: Mutex<Option<mpsc::UnboundedSender<Ctrl>>>,
    ended: AtomicU32,
    progress: Notify,
}

impl RecvState {
    fn new(t: Arc<Transfer>, sender: DeviceInfo, chunk: u32, sink: Arc<dyn Sink>, skip: &[u32]) -> Result<Arc<Self>> {
        let mut pos = HashMap::new();
        let mut slots = Vec::new();
        let mut done = 0;
        for (i, it) in t.items.iter().enumerate() {
            pos.insert(it.id, i);
            let n = if it.dir { 0 } else { chunk_count(it.size, chunk) };
            let skipped = skip.contains(&it.id);
            if it.dir && !skipped {
                sink.make_dir(&it.path)?;
            }
            if it.dir || skipped {
                done += 1;
            }
            slots.push(Mutex::new(Slot { out: None, bits: vec![false; n as usize], have: 0, finished: it.dir || skipped, failed: false }));
        }
        let needed = t.items.len() as u32;
        Ok(Arc::new(Self {
            t,
            sender,
            chunk,
            sink,
            pos,
            slots,
            done: AtomicU32::new(done),
            needed,
            ctrl: Mutex::new(None),
            ended: AtomicU32::new(0),
            progress: Notify::new(),
        }))
    }

    fn item(&self, id: u32) -> Option<(usize, &Item)> {
        self.pos.get(&id).map(|&i| (i, &self.t.items[i]))
    }

    fn have(&self) -> Vec<Have> {
        self.t
            .items
            .iter()
            .zip(&self.slots)
            .filter_map(|(it, s)| {
                let s = s.lock();
                (s.have > 0).then(|| Have { item: it.id, ranges: ranges_from_bits(&s.bits) })
            })
            .collect()
    }

    fn held_bytes(&self) -> u64 {
        self.t
            .items
            .iter()
            .zip(&self.slots)
            .map(|(it, s)| {
                let s = s.lock();
                if it.dir {
                    return 0;
                }
                s.bits
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b)
                    .map(|(i, _)| chunk_len(it.size, self.chunk, i as u32) as u64)
                    .sum::<u64>()
            })
            .sum()
    }

    fn check(&self, h: &ChunkHeader) -> Result<()> {
        let (_, it) = self.item(h.item).ok_or_else(|| anyhow!("chunk for unknown item {}", h.item))?;
        if it.dir {
            bail!("chunk for a folder");
        }
        let n = chunk_count(it.size, self.chunk);
        if h.index >= n {
            bail!("chunk index out of range");
        }
        if h.raw_len != chunk_len(it.size, self.chunk, h.index) || h.wire_len != h.raw_len || h.flags != 0 {
            bail!("chunk length does not match the offer");
        }
        Ok(())
    }

    fn file(&self, item: u32) -> Result<Option<Arc<dyn OutFile>>> {
        let (i, it) = self.item(item).ok_or_else(|| anyhow!("unknown item"))?;
        let mut s = self.slots[i].lock();
        if s.finished || s.failed {
            return Ok(None);
        }
        if s.out.is_none() {
            s.out = Some(self.sink.create(it, &self.t.id, &self.sender.name)?);
        }
        Ok(s.out.clone())
    }

    fn chunk_ok(&self, item: u32, index: u32) {
        let Some((i, it)) = self.item(item) else { return };
        let finish = {
            let mut s = self.slots[i].lock();
            if s.finished || s.failed || s.bits[index as usize] {
                return;
            }
            s.bits[index as usize] = true;
            s.have += 1;
            if s.have as usize == s.bits.len() {
                s.finished = true;
                s.out.take()
            } else {
                None
            }
        };
        if let Some(out) = finish {
            match out.finish(it.mtime) {
                Ok(path) => {
                    if !path.is_empty() {
                        self.t.saved.lock().push(path);
                    }
                    self.t.files_done.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    tracing::warn!("could not save {}: {e:#}", it.path);
                    self.slots[i].lock().failed = true;
                }
            }
            self.done.fetch_add(1, Ordering::Relaxed);
            self.progress.notify_one();
        }
    }

    fn chunk_bad(&self, item: u32, index: u32) {
        if let Some(tx) = self.ctrl.lock().as_ref() {
            let _ = tx.send(Ctrl::Retry { item, index });
        }
    }

    fn item_error(&self, item: u32) {
        let Some((i, _)) = self.item(item) else { return };
        let out = {
            let mut s = self.slots[i].lock();
            if s.finished || s.failed {
                return;
            }
            s.failed = true;
            s.out.take()
        };
        if let Some(o) = out {
            o.abort(false);
        }
        self.done.fetch_add(1, Ordering::Relaxed);
        self.progress.notify_one();
    }

    fn complete(&self) -> bool {
        self.done.load(Ordering::Relaxed) >= self.needed
    }

    fn failed_items(&self) -> Vec<u32> {
        self.t.items.iter().zip(&self.slots).filter(|(_, s)| s.lock().failed).map(|(it, _)| it.id).collect()
    }

    /// Closes every open file. `keep` leaves partial data for a later resume.
    pub fn abort(&self, keep: bool) {
        for s in &self.slots {
            if let Some(o) = s.lock().out.take() {
                o.abort(keep);
            }
        }
    }
}

fn chunk_len(size: u64, chunk: u32, index: u32) -> u32 {
    let off = index as u64 * chunk as u64;
    size.saturating_sub(off).min(chunk as u64) as u32
}

/// Serves one control session opened by a peer. `first` is the frame the
/// listener already read (always a Hello here).
pub async fn serve_session(shared: Arc<Shared>, mut ctrl: Stream, fp: [u8; 32], first: Ctrl, from: String) -> Result<()> {
    let device = match first {
        Ctrl::Hello { device, .. } => device,
        other => bail!("expected Hello, got {other:?}"),
    };
    if device.id != device_id(&fp) {
        bail!("device id does not match its key");
    }
    write_ctrl(&mut ctrl.write, &Ctrl::Hello { proto: PROTO_VERSION, device: shared.device() }).await?;
    shared.note_peer_key(&device, fp);
    shared.note_contact(&device, &from);

    loop {
        let msg = match tokio::time::timeout(Duration::from_secs(600), read_ctrl(&mut ctrl.read)).await {
            Ok(Ok(m)) => m,
            Ok(Err(_)) | Err(_) => return Ok(()),
        };
        match msg {
            Ctrl::Offer(o) => return receive(shared, ctrl, device, fp, o).await,
            Ctrl::Ping { t } => write_ctrl(&mut ctrl.write, &Ctrl::Pong { t }).await?,
            m @ (Ctrl::PairPin { .. } | Ctrl::PairQr { .. }) => {
                return crate::pairing::respond(&shared, &mut ctrl, &device, fp, m).await;
            }
            Ctrl::Probe { kind } => crate::node::probe(&shared, &mut ctrl, &kind).await?,
            _ => {}
        }
    }
}

fn valid_offer(o: &Offer) -> Result<()> {
    if !(64 << 10..=64 << 20).contains(&o.chunk) {
        bail!("chunk size out of range");
    }
    let mut ids = std::collections::HashSet::new();
    for it in &o.items {
        if !ids.insert(it.id) {
            bail!("duplicate item id");
        }
        if it.path.is_empty() || it.path.len() > 4096 {
            bail!("bad path");
        }
    }
    if o.items.len() > 2_000_000 {
        bail!("too many items");
    }
    Ok(())
}

async fn receive(shared: Arc<Shared>, mut ctrl: Stream, sender: DeviceInfo, fp: [u8; 32], offer: Offer) -> Result<()> {
    valid_offer(&offer)?;
    let key = (sender.id.clone(), offer.transfer.clone());
    let resumed = if offer.resume { shared.take_partial(&key) } else { None };
    let resumed = resumed.filter(|st| st.t.items == offer.items && st.chunk == offer.chunk);

    let (st, skip) = if let Some(st) = resumed {
        st.t.set_phase(Phase::Running);
        (st, Vec::new())
    } else {
        let t = Arc::new(Transfer::new(offer.transfer.clone(), Direction::Receive, sender.clone(), offer.items.clone(), offer.text.clone()));
        if let Some(old) = shared.transfer(&t.id) {
            if !old.phase().is_final() {
                bail!("transfer {} is already running", t.id);
            }
        }
        shared.add_transfer(t.clone());
        t.set_phase(Phase::Asking);
        let decision = tokio::select! {
            d = shared.decide(&t, &sender, fp) => d,
            r = read_ctrl(&mut ctrl.read) => match r {
                Ok(Ctrl::Cancel { reason }) => Decision::Withdrawn(reason),
                _ => Decision::Withdrawn("the sender went away".into()),
            },
        };
        match decision {
            Decision::Accept { sink, skip } => {
                *t.dest.write() = sink.describe();
                let st = RecvState::new(t.clone(), sender.clone(), offer.chunk, sink, &skip)?;
                (st, skip)
            }
            Decision::Decline(reason) => {
                let _ = write_ctrl(&mut ctrl.write, &Ctrl::Decline { reason: reason.clone() }).await;
                *t.error.write() = Some(reason);
                t.set_phase(Phase::Declined);
                shared.finish_transfer(&t);
                return Ok(());
            }
            Decision::Withdrawn(reason) => {
                *t.error.write() = Some(reason);
                t.set_phase(Phase::Cancelled);
                shared.finish_transfer(&t);
                return Ok(());
            }
        }
    };
    let t = st.t.clone();

    if t.items.iter().all(|i| i.dir) {
        // Text and empty folders need no data streams.
        write_ctrl(&mut ctrl.write, &Ctrl::Accept { token: String::new(), streams: 0, have: vec![], skip }).await?;
        t.set_phase(Phase::Running);
        if let Some(text) = &t.text {
            shared.text_received(&t, text);
        }
        t.set_phase(Phase::Done);
        shared.finish_transfer(&t);
        return Ok(());
    }

    let held = st.held_bytes();
    let skipped: u64 = t.items.iter().filter(|i| skip.contains(&i.id)).map(|i| i.size).sum();
    t.bytes.store(held + skipped, Ordering::Relaxed);
    t.resumed.store(held + skipped, Ordering::Relaxed);
    for l in &t.lanes {
        l.store(0, Ordering::Relaxed);
    }
    st.ended.store(0, Ordering::Relaxed);

    let token = random_id(16);
    let (data_tx, mut data_rx) = mpsc::channel::<Stream>(MAX_LANES);
    shared.add_route(&token, data_tx);
    let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded_channel();
    *st.ctrl.lock() = Some(ctrl_tx);

    let streams = shared.settings().streams.clamp(1, MAX_LANES as u8);
    write_ctrl(&mut ctrl.write, &Ctrl::Accept { token: token.clone(), streams, have: st.have(), skip }).await?;
    t.set_phase(if *t.paused.borrow() { Phase::Paused } else { Phase::Running });
    shared.emit_transfer(&t);

    let mut lanes: JoinSet<Result<()>> = JoinSet::new();
    let mut lane_count = 0usize;
    let mut expected: Option<u32> = None;
    let mut paused_rx = t.paused.subscribe();
    let mut remote_paused = false;

    let outcome: Result<bool> = loop {
        if let Some(n) = expected {
            if st.ended.load(Ordering::Relaxed) >= n && lanes.is_empty() && st.complete() {
                let failed = st.failed_items();
                write_ctrl(&mut ctrl.write, &Ctrl::Receipt { ok: failed.is_empty(), failed: failed.clone() }).await?;
                break Ok(failed.is_empty());
            }
        }
        tokio::select! {
            Some(s) = data_rx.recv() => {
                let lane = lane_count.min(MAX_LANES - 1);
                lane_count += 1;
                t.lanes_used.store(lane_count.min(MAX_LANES) as u32, Ordering::Relaxed);
                let st2 = st.clone();
                lanes.spawn(async move { recv_lane(st2, lane, s.read).await });
            }
            r = lanes.join_next(), if !lanes.is_empty() => {
                match r {
                    Some(Ok(Ok(()))) => { st.ended.fetch_add(1, Ordering::Relaxed); }
                    Some(Ok(Err(e))) => break Err(e),
                    Some(Err(e)) => break Err(anyhow!("lane stopped: {e}")),
                    None => {}
                }
            }
            m = read_ctrl(&mut ctrl.read) => {
                match m {
                    Ok(Ctrl::AllSent { streams }) => expected = Some(streams),
                    Ok(Ctrl::Pause) => { remote_paused = true; t.paused.send_replace(true); t.set_phase(Phase::Paused); shared.emit_transfer(&t); }
                    Ok(Ctrl::Resume) => { remote_paused = false; t.paused.send_replace(false); t.set_phase(Phase::Running); shared.emit_transfer(&t); }
                    Ok(Ctrl::ItemError { item, reason }) => {
                        tracing::warn!("sender could not read item {item}: {reason}");
                        st.item_error(item);
                    }
                    Ok(Ctrl::Cancel { reason }) => {
                        *t.error.write() = Some(reason);
                        st.abort(false);
                        t.set_phase(Phase::Cancelled);
                        shared.remove_route(&token);
                        shared.finish_transfer(&t);
                        return Ok(());
                    }
                    Ok(Ctrl::Ping { t: x }) => { write_ctrl(&mut ctrl.write, &Ctrl::Pong { t: x }).await?; }
                    Ok(_) => {}
                    Err(e) => break Err(e),
                }
            }
            Some(c) = ctrl_rx.recv() => { write_ctrl(&mut ctrl.write, &c).await?; }
            _ = st.progress.notified() => {}
            c = paused_rx.changed() => {
                if c.is_ok() {
                    let p = *paused_rx.borrow_and_update();
                    if p != remote_paused {
                        remote_paused = p;
                        write_ctrl(&mut ctrl.write, &if p { Ctrl::Pause } else { Ctrl::Resume }).await?;
                    }
                    t.set_phase(if p { Phase::Paused } else { Phase::Running });
                    shared.emit_transfer(&t);
                }
            }
            _ = t.cancel.cancelled() => {
                let _ = write_ctrl(&mut ctrl.write, &Ctrl::Cancel { reason: "cancelled by receiver".into() }).await;
                st.abort(false);
                t.set_phase(Phase::Cancelled);
                shared.remove_route(&token);
                shared.finish_transfer(&t);
                return Ok(());
            }
        }
    };
    shared.remove_route(&token);
    lanes.abort_all();
    *st.ctrl.lock() = None;

    match outcome {
        Ok(true) => {
            t.set_phase(Phase::Done);
            shared.finish_transfer(&t);
            Ok(())
        }
        Ok(false) => {
            t.fail(format!("{} file(s) could not be saved", st.failed_items().len()));
            shared.finish_transfer(&t);
            Ok(())
        }
        Err(e) => {
            // Keep every verified chunk; the sender reconnects with resume=true.
            tracing::info!("receive {} interrupted: {e:#}", t.id);
            *t.error.write() = Some(format!("{e:#}"));
            t.set_phase(Phase::Reconnecting);
            shared.emit_transfer(&t);
            shared.keep_partial(key, st);
            Ok(())
        }
    }
}

enum WPiece {
    Begin(ChunkHeader),
    Data(Vec<u8>, usize),
    End([u8; 32]),
}

async fn recv_lane(st: Arc<RecvState>, lane: usize, mut r: BoxRead) -> Result<()> {
    let (tx, rx) = mpsc::channel::<WPiece>(8);
    let pool: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let (wst, wpool) = (st.clone(), pool.clone());
    let writer = std::thread::Builder::new()
        .name(format!("swx-write-{lane}"))
        .spawn(move || write_pieces(wst, lane, rx, wpool))?;
    let res: Result<()> = async {
        while let Some(h) = read_chunk_header(&mut r).await? {
            st.check(&h)?;
            tx.send(WPiece::Begin(h)).await.map_err(|_| anyhow!("writer stopped"))?;
            let mut left = h.wire_len as usize;
            while left > 0 {
                let n = left.min(PIECE);
                let mut buf = pool.lock().pop().unwrap_or_else(|| vec![0u8; PIECE]);
                r.read_exact(&mut buf[..n]).await?;
                tx.send(WPiece::Data(buf, n)).await.map_err(|_| anyhow!("writer stopped"))?;
                left -= n;
            }
            let mut hash = [0u8; 32];
            r.read_exact(&mut hash).await?;
            tx.send(WPiece::End(hash)).await.map_err(|_| anyhow!("writer stopped"))?;
        }
        Ok(())
    }
    .await;
    drop(tx);
    let werr = tokio::task::spawn_blocking(move || writer.join()).await;
    res?;
    match werr {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => Err(e),
        _ => Err(anyhow!("writer thread failed")),
    }
}

fn write_pieces(st: Arc<RecvState>, lane: usize, mut rx: mpsc::Receiver<WPiece>, pool: Arc<Mutex<Vec<Vec<u8>>>>) -> Result<()> {
    let mut cur: Option<(ChunkHeader, Option<Arc<dyn OutFile>>, blake3::Hasher, u64)> = None;
    while let Some(p) = rx.blocking_recv() {
        match p {
            WPiece::Begin(h) => {
                let f = st.file(h.item)?;
                cur = Some((h, f, blake3::Hasher::new(), 0));
            }
            WPiece::Data(buf, n) => {
                let (h, f, hasher, pos) = cur.as_mut().ok_or_else(|| anyhow!("data before header"))?;
                hasher.update(&buf[..n]);
                if let Some(f) = f {
                    let off = h.index as u64 * st.chunk as u64 + *pos;
                    f.write_at(&buf[..n], off).map_err(|e| anyhow!("write failed: {e}"))?;
                }
                *pos += n as u64;
                st.t.add_bytes(lane, n as u64);
                let mut pl = pool.lock();
                if pl.len() < 16 {
                    pl.push(buf);
                }
            }
            WPiece::End(hash) => {
                let (h, f, hasher, _) = cur.take().ok_or_else(|| anyhow!("trailer before header"))?;
                if f.is_none() {
                    continue;
                }
                if hasher.finalize().as_bytes() == &hash {
                    st.chunk_ok(h.item, h.index);
                } else {
                    tracing::warn!("chunk {}/{} failed verification, asking again", h.item, h.index);
                    st.t.sub_bytes(h.raw_len as u64);
                    st.chunk_bad(h.item, h.index);
                }
            }
        }
    }
    Ok(())
}
