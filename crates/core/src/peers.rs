//! Devices we can see (discovery, mDNS, manual address, QR) and devices we
//! trust (paired by QR or PIN).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::proto::DeviceInfo;
use crate::util::{hex, now_ms};

/// How an address reaches the device, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Usb,
    Direct,
    Lan,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Addr {
    pub addr: SocketAddr,
    pub via: Via,
    pub seen_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub device: DeviceInfo,
    pub fp: Option<[u8; 32]>,
    pub addrs: Vec<Addr>,
    pub seen_ms: u64,
    /// web = a browser session (iPhone without the app).
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub model: String,
    pub kind: String,
    pub trusted: bool,
    pub online: bool,
    pub via: Option<Via>,
    pub addrs: Vec<String>,
    pub seen_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub fp: String,
    pub name: String,
    pub platform: String,
    pub added_ms: u64,
}

pub const ONLINE_MS: u64 = 40_000;

pub struct Peers {
    map: RwLock<HashMap<String, Peer>>,
    trusted: RwLock<Vec<TrustedDevice>>,
    trust_path: PathBuf,
}

/// Classifies an address. USB tethering hands out its own small subnets
/// (Android: 192.168.42.x / 192.168.44.x, Windows ICS: 192.168.137.x);
/// Android hotspots and Wi-Fi Direct groups use 192.168.43.x / 192.168.49.x.
pub fn classify(ip: IpAddr) -> Via {
    if let IpAddr::V4(v4) = ip {
        let o = v4.octets();
        if o[0] == 192 && o[1] == 168 {
            return match o[2] {
                42 | 44 | 137 => Via::Usb,
                43 | 49 => Via::Direct,
                _ => Via::Lan,
            };
        }
    }
    Via::Lan
}

impl Peers {
    pub fn new(dir: &std::path::Path) -> Self {
        let trust_path = dir.join("trusted.json");
        let trusted = std::fs::read(&trust_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { map: RwLock::new(HashMap::new()), trusted: RwLock::new(trusted), trust_path }
    }

    /// Returns true when something visible changed.
    pub fn upsert(&self, device: DeviceInfo, fp: Option<[u8; 32]>, addr: Option<SocketAddr>, via: Option<Via>, kind: &str) -> bool {
        let now = now_ms();
        let mut m = self.map.write();
        let p = m.entry(device.id.clone()).or_insert_with(|| Peer {
            device: device.clone(),
            fp: None,
            addrs: Vec::new(),
            seen_ms: 0,
            kind: kind.to_string(),
        });
        let was_online = now.saturating_sub(p.seen_ms) < ONLINE_MS;
        let mut changed = !was_online || p.device.name != device.name;
        if !device.name.is_empty() {
            p.device = device;
        }
        if fp.is_some() {
            p.fp = fp;
        }
        p.seen_ms = now;
        if let Some(a) = addr {
            let via = via.unwrap_or_else(|| classify(a.ip()));
            match p.addrs.iter_mut().find(|x| x.addr == a) {
                Some(x) => x.seen_ms = now,
                None => {
                    p.addrs.push(Addr { addr: a, via, seen_ms: now });
                    changed = true;
                }
            }
            p.addrs.sort_by_key(|x| (x.via, std::cmp::Reverse(x.seen_ms)));
            p.addrs.truncate(8);
        }
        changed
    }

    pub fn remove(&self, id: &str) {
        self.map.write().remove(id);
    }

    pub fn get(&self, id: &str) -> Option<Peer> {
        self.map.read().get(id).cloned()
    }

    pub fn addrs(&self, id: &str) -> Vec<SocketAddr> {
        let now = now_ms();
        self.map
            .read()
            .get(id)
            .map(|p| {
                let mut v: Vec<_> = p.addrs.iter().filter(|a| now - a.seen_ms < 10 * 60_000 || a.via == Via::Manual).collect();
                v.sort_by_key(|a| (a.via, std::cmp::Reverse(a.seen_ms)));
                v.into_iter().map(|a| a.addr).collect()
            })
            .unwrap_or_default()
    }

    /// Drops peers that went quiet; returns true when the list changed.
    pub fn expire(&self) -> bool {
        let now = now_ms();
        let trusted: Vec<String> = self.trusted.read().iter().map(|t| t.id.clone()).collect();
        let mut m = self.map.write();
        let before = m.len();
        m.retain(|id, p| now - p.seen_ms < 10 * 60_000 || trusted.contains(id));
        before != m.len() || m.values().any(|p| (ONLINE_MS..ONLINE_MS + 1600).contains(&(now - p.seen_ms)))
    }

    pub fn views(&self) -> Vec<PeerView> {
        let now = now_ms();
        let trusted = self.trusted.read();
        let m = self.map.read();
        let mut out: Vec<PeerView> = m
            .values()
            .map(|p| PeerView {
                id: p.device.id.clone(),
                name: p.device.name.clone(),
                platform: p.device.platform.clone(),
                model: p.device.model.clone(),
                kind: p.kind.clone(),
                trusted: trusted.iter().any(|t| t.id == p.device.id),
                online: now - p.seen_ms < ONLINE_MS,
                via: p.addrs.first().map(|a| a.via),
                addrs: p.addrs.iter().map(|a| a.addr.to_string()).collect(),
                seen_ms: p.seen_ms,
            })
            .collect();
        // Trusted devices that are not around still show (offline).
        for t in trusted.iter() {
            if !m.contains_key(&t.id) {
                out.push(PeerView {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    platform: t.platform.clone(),
                    model: String::new(),
                    kind: "app".into(),
                    trusted: true,
                    online: false,
                    via: None,
                    addrs: vec![],
                    seen_ms: 0,
                });
            }
        }
        out.sort_by(|a, b| b.online.cmp(&a.online).then(b.trusted.cmp(&a.trusted)).then(a.name.cmp(&b.name)));
        out
    }

    pub fn is_trusted(&self, id: &str, fp: &[u8; 32]) -> bool {
        let h = hex(fp);
        self.trusted.read().iter().any(|t| t.id == id && t.fp == h)
    }

    pub fn trusted_fp(&self, id: &str) -> Option<[u8; 32]> {
        let t = self.trusted.read();
        let e = t.iter().find(|t| t.id == id)?;
        let b = data_encoding::HEXLOWER.decode(e.fp.as_bytes()).ok()?;
        b.try_into().ok()
    }

    pub fn trust(&self, device: &DeviceInfo, fp: &[u8; 32]) {
        {
            let mut t = self.trusted.write();
            t.retain(|x| x.id != device.id);
            t.push(TrustedDevice {
                id: device.id.clone(),
                fp: hex(fp),
                name: device.name.clone(),
                platform: device.platform.clone(),
                added_ms: now_ms(),
            });
        }
        self.save_trust();
    }

    pub fn untrust(&self, id: &str) {
        self.trusted.write().retain(|x| x.id != id);
        self.save_trust();
    }

    pub fn trusted_list(&self) -> Vec<TrustedDevice> {
        self.trusted.read().clone()
    }

    fn save_trust(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(&*self.trusted.read()) {
            let tmp = self.trust_path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, &self.trust_path);
            }
        }
    }
}
