//! Transfer history as JSON lines, newest last. One line per finished
//! transfer, with a down-sampled speed curve for the graphs.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::link::Transport;
use crate::proto::DeviceInfo;
use crate::transfer::{Direction, Phase, Sample, Transfer};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub dir: Direction,
    pub peer: DeviceInfo,
    pub phase: Phase,
    #[serde(default)]
    pub error: Option<String>,
    pub files: u32,
    pub total: u64,
    pub bytes: u64,
    pub created_ms: u64,
    pub finished_ms: u64,
    pub elapsed_ms: u64,
    pub average: f64,
    pub peak: f64,
    pub transport: Transport,
    pub names: Vec<String>,
    #[serde(default)]
    pub dest: String,
    #[serde(default)]
    pub saved: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub lanes: u32,
}

impl Entry {
    pub fn from_transfer(t: &Transfer) -> Self {
        let v = t.view(false);
        Self {
            id: v.id,
            dir: v.dir,
            peer: v.peer,
            phase: v.phase,
            error: v.error,
            files: v.files,
            total: v.total,
            bytes: v.bytes,
            created_ms: v.created_ms,
            finished_ms: v.finished_ms,
            elapsed_ms: v.elapsed_ms,
            average: v.average,
            peak: v.peak,
            transport: v.transport,
            names: v.names,
            dest: v.dest,
            saved: t.saved.lock().iter().take(50).cloned().collect(),
            text: t.text.as_ref().map(|s| s.chars().take(4000).collect()),
            samples: t.samples(160),
            lanes: v.lanes.len() as u32,
        }
    }
}

pub struct History {
    path: PathBuf,
    cache: Mutex<Vec<Entry>>,
}

const KEEP: usize = 2000;

impl History {
    pub fn open(dir: &std::path::Path) -> Self {
        let path = dir.join("history.jsonl");
        let mut entries: Vec<Entry> = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        if entries.len() > KEEP {
            entries.drain(..entries.len() - KEEP);
        }
        Self { path, cache: Mutex::new(entries) }
    }

    pub fn add(&self, e: Entry) {
        if let Ok(line) = serde_json::to_string(&e) {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
                let _ = writeln!(f, "{line}");
            }
        }
        let mut c = self.cache.lock();
        c.retain(|x| x.id != e.id);
        c.push(e);
        if c.len() > KEEP {
            let n = c.len() - KEEP;
            c.drain(..n);
        }
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Vec<Entry> {
        self.cache.lock().iter().rev().take(limit).cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<Entry> {
        self.cache.lock().iter().rev().find(|e| e.id == id).cloned()
    }

    pub fn remove(&self, id: &str) {
        let mut c = self.cache.lock();
        c.retain(|e| e.id != id);
        self.rewrite(&c);
    }

    pub fn clear(&self) {
        let mut c = self.cache.lock();
        c.clear();
        self.rewrite(&c);
    }

    fn rewrite(&self, entries: &[Entry]) {
        let tmp = self.path.with_extension("jsonl.tmp");
        let body: String = entries.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(tmp, &self.path);
        }
    }
}
