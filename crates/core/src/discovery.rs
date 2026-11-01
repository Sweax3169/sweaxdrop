//! Finding devices on the local network, three ways at once:
//!
//! 1. UDP announcements to a multicast group, answered right away when the
//!    packet is a query, so a newly opened app sees others in one round trip.
//! 2. The same packet as directed broadcast on every IPv4 interface, for
//!    routers that drop multicast and phones that filter it.
//! 3. mDNS/DNS-SD (`_sweaxdrop._tcp`), which is what iOS lets an app browse
//!    without Apple's multicast entitlement.
//!
//! None of these is trusted: an announcement only says where to connect.
//! Who answers is proven by the TLS handshake.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::node::Shared;
use crate::peers::classify;
use crate::proto::DeviceInfo;

pub const DISCOVERY_PORT: u16 = 47471;
pub const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 71, 71);
pub const MDNS_TYPE: &str = "_sweaxdrop._tcp.local.";

#[derive(Serialize, Deserialize, Debug)]
struct Announce {
    swx: u32,
    id: String,
    name: String,
    plat: String,
    #[serde(default)]
    model: String,
    port: u16,
    #[serde(default)]
    q: bool,
    #[serde(default)]
    v: String,
}

pub fn start(shared: Arc<Shared>) {
    tokio::spawn(udp_loop(shared.clone()));
    let sh = shared.clone();
    std::thread::Builder::new()
        .name("swx-mdns".into())
        .spawn(move || {
            if let Err(e) = mdns(sh) {
                tracing::warn!("mDNS unavailable: {e:#}");
            }
        })
        .ok();
}

fn ipv4_ifaces() -> Vec<(Ipv4Addr, Option<Ipv4Addr>)> {
    let mut out = Vec::new();
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        for i in ifs {
            if let if_addrs::IfAddr::V4(v4) = &i.addr {
                if v4.ip.is_loopback() || v4.ip.is_link_local() {
                    continue;
                }
                out.push((v4.ip, v4.broadcast));
            }
        }
    }
    out
}

fn bind() -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(target_os = "solaris")))]
    let _ = s.set_reuse_port(true);
    s.set_broadcast(true)?;
    s.set_multicast_loop_v4(true)?;
    s.set_multicast_ttl_v4(1)?;
    s.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).into())?;
    s.set_nonblocking(true)?;
    Ok(s.into())
}

fn packet(shared: &Shared, query: bool) -> Vec<u8> {
    let d = shared.device();
    let a = Announce {
        swx: 1,
        id: d.id,
        name: d.name,
        plat: d.platform,
        model: d.model,
        port: shared.port.load(std::sync::atomic::Ordering::Relaxed),
        q: query,
        v: d.version,
    };
    serde_json::to_vec(&a).unwrap_or_default()
}

async fn send_all(sock: &UdpSocket, shared: &Shared, query: bool) {
    if !shared.settings().discoverable && !query {
        return;
    }
    let pkt = packet(shared, query);
    let raw = socket2::SockRef::from(sock);
    for (ip, bcast) in ipv4_ifaces() {
        let _ = raw.set_multicast_if_v4(&ip);
        let _ = sock.send_to(&pkt, (GROUP, DISCOVERY_PORT)).await;
        if let Some(b) = bcast {
            let _ = sock.send_to(&pkt, (b, DISCOVERY_PORT)).await;
        }
    }
    let _ = sock.send_to(&pkt, (Ipv4Addr::BROADCAST, DISCOVERY_PORT)).await;
}

fn join(sock: &UdpSocket, joined: &mut HashSet<Ipv4Addr>) {
    for (ip, _) in ipv4_ifaces() {
        if joined.insert(ip) {
            if let Err(e) = sock.join_multicast_v4(GROUP, ip) {
                tracing::debug!("join {GROUP} on {ip}: {e}");
            }
        }
    }
}

async fn udp_loop(shared: Arc<Shared>) {
    let sock = match bind().and_then(UdpSocket::from_std) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("discovery socket: {e}");
            return;
        }
    };
    let mut joined = HashSet::new();
    join(&sock, &mut joined);
    let me = shared.identity.id();
    let mut buf = vec![0u8; 2048];
    let mut tick = 0u32;
    let mut next = tokio::time::Instant::now();
    loop {
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let Ok((n, from)) = r else { continue };
                let Ok(a) = serde_json::from_slice::<Announce>(&buf[..n]) else { continue };
                if a.swx != 1 || a.id == me || a.id.len() != 16 {
                    continue;
                }
                let dev = DeviceInfo { id: a.id, name: a.name.chars().take(64).collect(), platform: a.plat, model: a.model, version: a.v };
                let addr = SocketAddr::new(from.ip(), a.port);
                if shared.peers.upsert(dev, None, Some(addr), Some(classify(from.ip())), "app") {
                    shared.emit_peers();
                }
                if a.q && shared.settings().discoverable {
                    let pkt = packet(&shared, false);
                    let _ = sock.send_to(&pkt, from).await;
                    let _ = sock.send_to(&pkt, (GROUP, DISCOVERY_PORT)).await;
                }
            }
            _ = tokio::time::sleep_until(next) => {
                if tick % 5 == 0 {
                    join(&sock, &mut joined);
                }
                // Query at start and then now and then; announce in between.
                send_all(&sock, &shared, tick < 3 || tick % 12 == 0).await;
                tick += 1;
                let gap = match tick { 0..=2 => 1, 3..=14 => 4, _ => 12 };
                next = tokio::time::Instant::now() + Duration::from_secs(gap);
            }
        }
    }
}

fn mdns(shared: Arc<Shared>) -> anyhow::Result<()> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let d = shared.device();
    let port = shared.port.load(std::sync::atomic::Ordering::Relaxed);
    let host = format!("{}.local.", d.id.to_lowercase());
    let props = [("id", d.id.as_str()), ("name", d.name.as_str()), ("plat", d.platform.as_str()), ("v", d.version.as_str())];
    if shared.settings().discoverable {
        let ips: Vec<IpAddr> = crate::node::local_addresses().into_iter().map(|(ip, _)| ip).filter(|ip| ip.is_ipv4()).collect();
        let info = mdns_sd::ServiceInfo::new(MDNS_TYPE, &d.id, &host, &ips[..], port, &props[..])?.enable_addr_auto();
        daemon.register(info)?;
    }
    let rx = daemon.browse(MDNS_TYPE)?;
    let me = d.id;
    while let Ok(ev) = rx.recv() {
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = ev {
            let id = info.get_property_val_str("id").unwrap_or_default().to_string();
            if id.len() != 16 || id == me {
                continue;
            }
            let dev = DeviceInfo {
                id,
                name: info.get_property_val_str("name").unwrap_or("Device").chars().take(64).collect(),
                platform: info.get_property_val_str("plat").unwrap_or_default().to_string(),
                model: String::new(),
                version: info.get_property_val_str("v").unwrap_or_default().to_string(),
            };
            let mut changed = false;
            for ip in info.get_addresses_v4() {
                let a = SocketAddr::new(IpAddr::V4(ip), info.get_port());
                changed |= shared.peers.upsert(dev.clone(), None, Some(a), Some(classify(a.ip())), "app");
            }
            if changed {
                shared.emit_peers();
            }
        }
    }
    Ok(())
}
