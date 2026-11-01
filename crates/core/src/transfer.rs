//! State of one transfer, shared by the engine, the UI events and history.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::link::Transport;
use crate::proto::{DeviceInfo, Item};
use crate::util::now_ms;

pub const MAX_LANES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Queued behind another transfer to the same device.
    Queued,
    Connecting,
    /// Offer sent; the other side has not answered yet.
    Waiting,
    /// Offer received; this side has not answered yet.
    Asking,
    Running,
    Paused,
    /// Connection lost; trying to pick up where it stopped.
    Reconnecting,
    Done,
    Failed,
    Cancelled,
    Declined,
}

impl Phase {
    pub fn is_final(self) -> bool {
        matches!(self, Phase::Done | Phase::Failed | Phase::Cancelled | Phase::Declined)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Sample {
    /// Milliseconds since the transfer started moving data.
    pub t: u32,
    /// Bytes per second over the last sampling window.
    pub bps: f64,
}

pub struct Transfer {
    pub id: String,
    pub dir: Direction,
    pub peer: RwLock<DeviceInfo>,
    pub items: Vec<Item>,
    pub total: u64,
    pub text: Option<String>,
    pub created_ms: u64,

    pub bytes: AtomicU64,
    /// Bytes that were already on the receiver when a resume started.
    pub resumed: AtomicU64,
    pub files_done: AtomicU32,
    pub lanes: [AtomicU64; MAX_LANES],
    pub lanes_used: AtomicU32,

    pub phase: RwLock<Phase>,
    pub error: RwLock<Option<String>>,
    pub transport: RwLock<Transport>,
    pub dest: RwLock<String>,
    pub saved: Mutex<Vec<String>>,
    pub attempts: AtomicU32,

    pub paused: watch::Sender<bool>,
    pub cancel: CancellationToken,

    started: Mutex<Option<Instant>>,
    finished_ms: AtomicU64,
    moving_ms: AtomicU64,
    samples: Mutex<Vec<Sample>>,
    last: Mutex<(Instant, u64)>,
    peak: Mutex<f64>,
}

impl Transfer {
    pub fn new(id: String, dir: Direction, peer: DeviceInfo, items: Vec<Item>, text: Option<String>) -> Self {
        let total = items.iter().map(|i| i.size).sum();
        Self {
            id,
            dir,
            peer: RwLock::new(peer),
            items,
            total,
            text,
            created_ms: now_ms(),
            bytes: AtomicU64::new(0),
            resumed: AtomicU64::new(0),
            files_done: AtomicU32::new(0),
            lanes: Default::default(),
            lanes_used: AtomicU32::new(0),
            phase: RwLock::new(Phase::Connecting),
            error: RwLock::new(None),
            transport: RwLock::new(Transport::Tcp),
            dest: RwLock::new(String::new()),
            saved: Mutex::new(Vec::new()),
            attempts: AtomicU32::new(0),
            paused: watch::channel(false).0,
            cancel: CancellationToken::new(),
            started: Mutex::new(None),
            finished_ms: AtomicU64::new(0),
            moving_ms: AtomicU64::new(0),
            samples: Mutex::new(Vec::new()),
            last: Mutex::new((Instant::now(), 0)),
            peak: Mutex::new(0.0),
        }
    }

    pub fn phase(&self) -> Phase {
        *self.phase.read()
    }

    pub fn set_phase(&self, p: Phase) {
        let mut g = self.phase.write();
        if g.is_final() {
            return;
        }
        *g = p;
        if p == Phase::Running {
            let mut s = self.started.lock();
            if s.is_none() {
                *s = Some(Instant::now());
                *self.last.lock() = (Instant::now(), self.bytes.load(Ordering::Relaxed));
            }
        }
        if p.is_final() {
            self.finished_ms.store(now_ms(), Ordering::Relaxed);
            if let Some(s) = *self.started.lock() {
                self.moving_ms.store(s.elapsed().as_millis() as u64, Ordering::Relaxed);
            }
        }
    }

    pub fn fail(&self, e: impl ToString) {
        *self.error.write() = Some(e.to_string());
        self.set_phase(Phase::Failed);
    }

    pub fn add_bytes(&self, lane: usize, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
        if let Some(l) = self.lanes.get(lane) {
            l.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn sub_bytes(&self, n: u64) {
        self.bytes.fetch_sub(n.min(self.bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
    }

    /// Called by the progress ticker; records one speed sample.
    pub fn sample(&self) {
        let Some(start) = *self.started.lock() else { return };
        let now = Instant::now();
        let bytes = self.bytes.load(Ordering::Relaxed);
        let mut last = self.last.lock();
        let dt = now.duration_since(last.0).as_secs_f64();
        if dt < 0.05 {
            return;
        }
        let bps = bytes.saturating_sub(last.1) as f64 / dt;
        *last = (now, bytes);
        let paused = self.phase() == Phase::Paused;
        let mut s = self.samples.lock();
        s.push(Sample { t: start.elapsed().as_millis() as u32, bps: if paused { 0.0 } else { bps } });
        if s.len() > 2400 {
            // keep memory flat on very long transfers: halve the resolution
            let halved: Vec<Sample> = s.chunks(2).map(|c| c[c.len() - 1]).collect();
            *s = halved;
        }
        let mut p = self.peak.lock();
        if bps > *p {
            *p = bps;
        }
    }

    /// Average over the last ~1 s of samples.
    pub fn speed(&self) -> f64 {
        let s = self.samples.lock();
        let n = s.len().min(4);
        if n == 0 {
            return 0.0;
        }
        s[s.len() - n..].iter().map(|x| x.bps).sum::<f64>() / n as f64
    }

    pub fn elapsed_ms(&self) -> u64 {
        match *self.started.lock() {
            Some(s) if !self.phase().is_final() => s.elapsed().as_millis() as u64,
            Some(_) => self.moving_ms.load(Ordering::Relaxed),
            None => 0,
        }
    }

    pub fn average(&self) -> f64 {
        let ms = self.elapsed_ms();
        if ms == 0 {
            return 0.0;
        }
        let moved = self.bytes.load(Ordering::Relaxed).saturating_sub(self.resumed.load(Ordering::Relaxed));
        moved as f64 / (ms as f64 / 1000.0)
    }

    pub fn samples(&self, max: usize) -> Vec<Sample> {
        let s = self.samples.lock();
        if s.len() <= max || max == 0 {
            return s.clone();
        }
        let step = s.len() as f64 / max as f64;
        (0..max).map(|i| s[((i as f64 + 1.0) * step) as usize - 1]).collect()
    }

    pub fn view(&self, with_items: bool) -> TransferView {
        let bytes = self.bytes.load(Ordering::Relaxed);
        let speed = self.speed();
        let left = self.total.saturating_sub(bytes);
        let lanes = self.lanes_used.load(Ordering::Relaxed) as usize;
        TransferView {
            id: self.id.clone(),
            dir: self.dir,
            peer: self.peer.read().clone(),
            phase: self.phase(),
            error: self.error.read().clone(),
            total: self.total,
            bytes,
            files: self.items.iter().filter(|i| !i.dir).count() as u32,
            files_done: self.files_done.load(Ordering::Relaxed),
            speed,
            average: self.average(),
            peak: *self.peak.lock(),
            eta_ms: if speed > 1.0 && left > 0 { Some((left as f64 / speed * 1000.0) as u64) } else { None },
            elapsed_ms: self.elapsed_ms(),
            created_ms: self.created_ms,
            finished_ms: self.finished_ms.load(Ordering::Relaxed),
            lanes: self.lanes[..lanes.min(MAX_LANES)].iter().map(|l| l.load(Ordering::Relaxed)).collect(),
            transport: *self.transport.read(),
            dest: self.dest.read().clone(),
            text: self.text.clone(),
            names: self.items.iter().filter(|i| !i.dir).take(6).map(|i| i.path.clone()).collect(),
            items: if with_items { Some(self.items.clone()) } else { None },
            saved: self.saved.lock().iter().take(200).cloned().collect(),
            attempts: self.attempts.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferView {
    pub id: String,
    pub dir: Direction,
    pub peer: DeviceInfo,
    pub phase: Phase,
    pub error: Option<String>,
    pub total: u64,
    pub bytes: u64,
    pub files: u32,
    pub files_done: u32,
    pub speed: f64,
    pub average: f64,
    pub peak: f64,
    pub eta_ms: Option<u64>,
    pub elapsed_ms: u64,
    pub created_ms: u64,
    pub finished_ms: u64,
    pub lanes: Vec<u64>,
    pub transport: Transport,
    pub dest: String,
    pub text: Option<String>,
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Item>>,
    pub saved: Vec<String>,
    pub attempts: u32,
}
