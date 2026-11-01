//! Messages of the SweaxDrop protocol (`swx/1`). See docs/protocol.md.
//!
//! Control messages are length-prefixed JSON on the first stream of a
//! session. Data streams carry one JSON `Attach` and then binary chunk frames
//! (see `wire.rs`), so no JSON is parsed per chunk.

use serde::{Deserialize, Serialize};

pub const PROTO_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    /// windows | android | ios | macos | linux | web
    pub platform: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Item {
    pub id: u32,
    /// Relative path with `/` separators; folders keep their structure.
    pub path: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// An (empty) directory to create, no data follows.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dir: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub transfer: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub total: u64,
    /// Size of every chunk except the last chunk of a file.
    pub chunk: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `true` when the sender re-offers an interrupted transfer.
    #[serde(default)]
    pub resume: bool,
}

/// Chunks of one item that the receiver already holds, as inclusive ranges.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Have {
    pub item: u32,
    pub ranges: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "m", rename_all = "snake_case")]
pub enum Ctrl {
    Hello { proto: u32, device: DeviceInfo },
    Offer(Offer),
    Accept {
        token: String,
        streams: u8,
        #[serde(default)]
        have: Vec<Have>,
        /// Items the receiver does not want (unchecked in the prompt).
        #[serde(default)]
        skip: Vec<u32>,
    },
    Decline { reason: String },
    Pause,
    Resume,
    Cancel { reason: String },
    /// A chunk failed verification; send it again.
    Retry { item: u32, index: u32 },
    /// Everything arrived and was verified (or not).
    Receipt { ok: bool, failed: Vec<u32> },
    /// The sender has written every chunk on `streams` data streams; the
    /// receiver answers with Receipt once that many streams have ended.
    AllSent { streams: u32 },
    /// The sender could not read an item (deleted or changed while sending).
    ItemError { item: u32, reason: String },
    Ping { t: u64 },
    Pong { t: u64 },
    /// PIN pairing (SPAKE2, symmetric) and its key confirmation.
    PairPin { msg: String },
    PairConfirm { mac: String },
    /// QR pairing: proves the scanner saw the secret on the other screen.
    PairQr { secret: String },
    PairResult { ok: bool, reason: String },
    /// Opens a data stream for an accepted transfer.
    Attach { token: String },
    /// Short request/response used by the benchmark and diagnostics.
    Probe { kind: String },
}

pub fn chunk_count(size: u64, chunk: u32) -> u32 {
    if size == 0 {
        1
    } else {
        size.div_ceil(chunk as u64) as u32
    }
}

pub fn ranges_from_bits(bits: &[bool]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bits.len() {
        if bits[i] {
            let s = i;
            while i + 1 < bits.len() && bits[i + 1] {
                i += 1;
            }
            out.push((s as u32, i as u32));
        }
        i += 1;
    }
    out
}

pub fn bits_from_ranges(ranges: &[(u32, u32)], n: u32) -> Vec<bool> {
    let mut bits = vec![false; n as usize];
    for &(a, b) in ranges {
        for i in a..=b.min(n.saturating_sub(1)) {
            bits[i as usize] = true;
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_roundtrip() {
        let bits = vec![true, true, false, true, false, false, true, true, true];
        let r = ranges_from_bits(&bits);
        assert_eq!(r, vec![(0, 1), (3, 3), (6, 8)]);
        assert_eq!(bits_from_ranges(&r, 9), bits);
    }

    #[test]
    fn ctrl_json_shape() {
        let m = Ctrl::Retry { item: 3, index: 9 };
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"m":"retry","item":3,"index":9}"#);
        assert_eq!(chunk_count(0, 4), 1);
        assert_eq!(chunk_count(9, 4), 3);
    }
}
