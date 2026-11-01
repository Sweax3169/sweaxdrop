//! Pairing: turning a nearby device into a trusted one.
//!
//! * PIN: one screen shows six digits, the other types them. Both run SPAKE2
//!   with the PIN as password over the already-encrypted TLS channel, then
//!   prove the shared key with a MAC over *both* TLS fingerprints. A relay in
//!   the middle sees different fingerprints on each side, so the MACs cannot
//!   match; a wrong PIN can be tried online only, five times at most.
//! * QR: the code carries the showing device's fingerprint and a one-time
//!   secret. The scanner checks the fingerprint in the TLS handshake and
//!   returns the secret, which only someone who saw the screen can know.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use spake2::{Ed25519Group, Identity as PakeId, Password, Spake2};

use crate::link::Stream;
use crate::node::{Event, Shared};
use crate::proto::{Ctrl, DeviceInfo, PROTO_VERSION};
use crate::util::random_id;
use crate::wire::{read_ctrl, write_ctrl};

const PAKE_ID: &[u8] = b"sweaxdrop pair v1";
const WINDOW: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct PairingState {
    pin: Mutex<Option<(String, Instant, u8)>>,
    qr: Mutex<Option<(String, Instant)>>,
}

impl PairingState {
    pub fn open_pin(&self) -> String {
        let n: u32 = rand::random::<u32>() % 1_000_000;
        let pin = format!("{n:06}");
        *self.pin.lock() = Some((pin.clone(), Instant::now() + WINDOW, 0));
        pin
    }

    pub fn close_pin(&self) {
        *self.pin.lock() = None;
    }

    /// The secret for the QR code; also the browser client's key.
    pub fn qr_secret(&self) -> String {
        let mut g = self.qr.lock();
        match g.as_ref() {
            Some((s, until)) if *until > Instant::now() + Duration::from_secs(60) => s.clone(),
            _ => {
                let s = random_id(16);
                *g = Some((s.clone(), Instant::now() + Duration::from_secs(900)));
                s
            }
        }
    }

    pub fn qr_valid(&self, secret: &str) -> bool {
        matches!(self.qr.lock().as_ref(), Some((s, until)) if *until > Instant::now() && constant_eq(s.as_bytes(), secret.as_bytes()))
    }

    pub fn rotate_qr(&self) {
        *self.qr.lock() = None;
    }
}

pub fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn mac(key: &[u8], from: &[u8; 32], to: &[u8; 32]) -> [u8; 32] {
    let k: [u8; 32] = blake3::derive_key("sweaxdrop pair confirm v1", key);
    let mut h = blake3::Hasher::new_keyed(&k);
    h.update(from);
    h.update(to);
    *h.finalize().as_bytes()
}

fn b64(b: &[u8]) -> String {
    data_encoding::BASE64.encode(b)
}

fn unb64(s: &str) -> Result<Vec<u8>> {
    Ok(data_encoding::BASE64.decode(s.as_bytes())?)
}

/// This device typed the PIN shown on `peer_id`.
pub async fn pair_with_pin(shared: &Arc<Shared>, peer_id: &str, pin: &str) -> Result<DeviceInfo> {
    let pin: String = pin.chars().filter(|c| c.is_ascii_digit()).collect();
    if pin.len() != 6 {
        bail!("the code has six digits");
    }
    let (mut s, peer, fp) = open_session(shared, peer_id, None).await?;
    let (pake, out) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(pin.as_bytes()), &PakeId::new(PAKE_ID));
    write_ctrl(&mut s.write, &Ctrl::PairPin { msg: b64(&out) }).await?;
    let theirs = match read_ctrl(&mut s.read).await? {
        Ctrl::PairPin { msg } => unb64(&msg)?,
        Ctrl::PairResult { reason, .. } => bail!("{reason}"),
        other => bail!("unexpected {other:?}"),
    };
    let key = pake.finish(&theirs).map_err(|_| anyhow!("pairing failed"))?;
    let me = shared.identity.fingerprint;
    write_ctrl(&mut s.write, &Ctrl::PairConfirm { mac: b64(&mac(&key, &me, &fp)) }).await?;
    match read_ctrl(&mut s.read).await? {
        Ctrl::PairConfirm { mac: m } if unb64(&m)? == mac(&key, &fp, &me) => {}
        Ctrl::PairResult { reason, .. } => bail!("{reason}"),
        _ => bail!("wrong code"),
    }
    match read_ctrl(&mut s.read).await? {
        Ctrl::PairResult { ok: true, .. } => {}
        Ctrl::PairResult { reason, .. } => bail!("{reason}"),
        _ => bail!("pairing failed"),
    }
    shared.peers.trust(&peer, &fp);
    shared.emit_peers();
    shared.emit(&Event::Pairing { state: "done", pin: None, peer: Some(&peer), reason: None });
    Ok(peer)
}

/// This device scanned a QR code shown by `peer_id` (fingerprint `fp_hex`).
pub async fn pair_with_qr(shared: &Arc<Shared>, peer_id: &str, fp_hex: &str, secret: &str) -> Result<DeviceInfo> {
    let want: [u8; 32] = data_encoding::HEXLOWER_PERMISSIVE
        .decode(fp_hex.as_bytes())
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| anyhow!("the code is not a SweaxDrop pairing code"))?;
    let (mut s, peer, fp) = open_session(shared, peer_id, Some(want)).await?;
    write_ctrl(&mut s.write, &Ctrl::PairQr { secret: secret.to_string() }).await?;
    match read_ctrl(&mut s.read).await? {
        Ctrl::PairResult { ok: true, .. } => {}
        Ctrl::PairResult { reason, .. } => bail!("{reason}"),
        _ => bail!("pairing failed"),
    }
    shared.peers.trust(&peer, &fp);
    shared.emit_peers();
    shared.emit(&Event::Pairing { state: "done", pin: None, peer: Some(&peer), reason: None });
    Ok(peer)
}

async fn open_session(shared: &Arc<Shared>, peer_id: &str, want: Option<[u8; 32]>) -> Result<(Stream, DeviceInfo, [u8; 32])> {
    let addrs = shared.peers.addrs(peer_id);
    let dialer = shared.dialer();
    let mut last = anyhow!("device not reachable");
    for a in addrs {
        match dialer.connect(a, crate::link::Transport::Tcp).await {
            Ok((_link, mut s, fp)) => {
                if let Some(w) = want {
                    if w != fp {
                        bail!("this device is not the one in the code");
                    }
                }
                write_ctrl(&mut s.write, &Ctrl::Hello { proto: PROTO_VERSION, device: shared.device() }).await?;
                let peer = match read_ctrl(&mut s.read).await? {
                    Ctrl::Hello { device, .. } => device,
                    _ => bail!("unexpected greeting"),
                };
                if peer.id != crate::identity::device_id(&fp) {
                    bail!("device id does not match its key");
                }
                return Ok((s, peer, fp));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The responding side, called from the session loop.
pub async fn respond(shared: &Arc<Shared>, s: &mut Stream, peer: &DeviceInfo, fp: [u8; 32], first: Ctrl) -> Result<()> {
    match first {
        Ctrl::PairQr { secret } => {
            if shared.pairing.qr_valid(&secret) {
                shared.peers.trust(peer, &fp);
                shared.emit_peers();
                write_ctrl(&mut s.write, &Ctrl::PairResult { ok: true, reason: String::new() }).await?;
                shared.emit(&Event::Pairing { state: "done", pin: None, peer: Some(peer), reason: None });
            } else {
                write_ctrl(&mut s.write, &Ctrl::PairResult { ok: false, reason: "this code expired; show a new one".into() }).await?;
            }
            Ok(())
        }
        Ctrl::PairPin { msg } => {
            let pin = {
                let mut g = shared.pairing.pin.lock();
                match g.as_mut() {
                    Some((pin, until, tries)) if *until > Instant::now() && *tries < 5 => {
                        *tries += 1;
                        Some(pin.clone())
                    }
                    _ => None,
                }
            };
            let Some(pin) = pin else {
                write_ctrl(&mut s.write, &Ctrl::PairResult { ok: false, reason: "pairing is not open on that device".into() }).await?;
                return Ok(());
            };
            let (pake, out) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(pin.as_bytes()), &PakeId::new(PAKE_ID));
            write_ctrl(&mut s.write, &Ctrl::PairPin { msg: b64(&out) }).await?;
            let key = pake.finish(&unb64(&msg)?).map_err(|_| anyhow!("pairing failed"))?;
            let me = shared.identity.fingerprint;
            let ok = match read_ctrl(&mut s.read).await? {
                Ctrl::PairConfirm { mac: m } => unb64(&m)? == mac(&key, &fp, &me),
                _ => false,
            };
            if !ok {
                write_ctrl(&mut s.write, &Ctrl::PairResult { ok: false, reason: "wrong code".into() }).await?;
                shared.emit(&Event::Pairing { state: "failed", pin: None, peer: Some(peer), reason: Some("wrong code") });
                return Ok(());
            }
            write_ctrl(&mut s.write, &Ctrl::PairConfirm { mac: b64(&mac(&key, &me, &fp)) }).await?;
            write_ctrl(&mut s.write, &Ctrl::PairResult { ok: true, reason: String::new() }).await?;
            shared.pairing.close_pin();
            shared.peers.trust(peer, &fp);
            shared.emit_peers();
            shared.emit(&Event::Pairing { state: "done", pin: None, peer: Some(peer), reason: None });
            Ok(())
        }
        _ => Ok(()),
    }
}
