//! The JSON command interface every UI talks to (desktop, Android, iOS and
//! the browser client all send the same commands).
//!
//! Request:  `{"cmd": "send", ...}`  Response: `{"ok": true, ...}` or
//! `{"ok": false, "error": "..."}`. Events arrive separately on the sink.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::node::{view_transfers, Answer, Node};
use crate::peers::Via;
use crate::proto::{DeviceInfo, Item};
use crate::storage::{collect_paths, Source, SourceItem};

#[derive(Deserialize)]
struct PlatformSource {
    token: String,
    path: String,
    size: u64,
    #[serde(default)]
    mtime: Option<i64>,
    #[serde(default)]
    mime: Option<String>,
}

pub async fn call(node: &Node, req: &str) -> String {
    let out = match serde_json::from_str::<Value>(req) {
        Ok(v) => dispatch(node, &v).await,
        Err(e) => Err(anyhow!("bad request: {e}")),
    };
    match out {
        Ok(mut v) => {
            if let Some(o) = v.as_object_mut() {
                o.insert("ok".into(), Value::Bool(true));
            }
            v.to_string()
        }
        Err(e) => json!({"ok": false, "error": format!("{e:#}")}).to_string(),
    }
}

fn s<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k).and_then(Value::as_str).ok_or_else(|| anyhow!("missing '{k}'"))
}

pub fn qr_uri(node: &Node) -> String {
    let sh = &node.shared;
    let ip = crate::node::local_addresses()
        .into_iter()
        .find(|(ip, _)| ip.is_ipv4())
        .map(|(ip, _)| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".into());
    let all: Vec<String> = crate::node::local_addresses().iter().filter(|(ip, _)| ip.is_ipv4()).map(|(ip, _)| ip.to_string()).collect();
    let secret = sh.pairing.qr_secret();
    let d = sh.device();
    format!(
        "http://{ip}:{port}/#s={secret}&i={id}&k={fp}&a={addrs}&n={name}",
        port = node.port(),
        id = d.id,
        fp = crate::util::hex(&sh.identity.fingerprint),
        addrs = all.join(","),
        name = urlencode(&d.name)
    )
}

pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parses a scanned code: `http://ip:port/#s=…&i=…&k=…&a=…&n=…`.
pub fn parse_qr(uri: &str) -> Result<(String, String, String, Vec<SocketAddr>, String)> {
    let (base, frag) = uri.split_once('#').ok_or_else(|| anyhow!("not a SweaxDrop code"))?;
    let mut get = std::collections::HashMap::new();
    for kv in frag.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            get.insert(k, urldecode(v));
        }
    }
    let port: u16 = base
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|p| p.split('/').next())
        .and_then(|p| p.parse().ok())
        .ok_or_else(|| anyhow!("code has no port"))?;
    let addrs = get
        .get("a")
        .map(|a| a.split(',').filter_map(|ip| ip.parse().ok()).map(|ip| SocketAddr::new(ip, port)).collect())
        .unwrap_or_default();
    let need = |k: &str| get.get(k).cloned().ok_or_else(|| anyhow!("not a SweaxDrop code"));
    Ok((need("i")?, need("k")?, need("s")?, addrs, get.get("n").cloned().unwrap_or_default()))
}

async fn dispatch(node: &Node, v: &Value) -> Result<Value> {
    let sh = &node.shared;
    let cmd = s(v, "cmd")?;
    match cmd {
        "state" => Ok(json!({
            "device": sh.device_view(),
            "settings": sh.settings(),
            "peers": sh.peers.views(),
            "transfers": view_transfers(sh),
            "history": sh.history.list(v.get("history").and_then(Value::as_u64).unwrap_or(60) as usize),
            "trusted": sh.peers.trusted_list(),
            "qr": qr_uri(node),
        })),
        "settings_set" => {
            let mut st = sh.settings();
            let old_cipher = st.cipher;
            st.merge(v.get("patch").unwrap_or(&Value::Null));
            let rebuild = st.cipher != old_cipher;
            sh.set_settings(st);
            if rebuild {
                sh.rebuild_dialer()?;
            }
            Ok(json!({"settings": sh.settings()}))
        }
        "send" => {
            let peer = s(v, "peer")?;
            let text = v.get("text").and_then(Value::as_str).map(str::to_string).filter(|t| !t.is_empty());
            let mut sources: Vec<SourceItem> = Vec::new();
            if let Some(paths) = v.get("paths").and_then(Value::as_array) {
                let paths: Vec<PathBuf> = paths.iter().filter_map(Value::as_str).map(PathBuf::from).collect();
                let blocking = tokio::task::spawn_blocking(move || collect_paths(&paths)).await??;
                sources.extend(blocking);
            }
            if let Some(list) = v.get("sources") {
                let list: Vec<PlatformSource> = serde_json::from_value(list.clone())?;
                for p in list {
                    sources.push(SourceItem {
                        item: Item { id: 0, mime: p.mime.or_else(|| crate::util::mime_for(&p.path).map(str::to_string)), path: p.path, size: p.size, mtime: p.mtime, dir: false },
                        src: Source::Platform(p.token),
                    });
                }
            }
            for (i, s) in sources.iter_mut().enumerate() {
                s.item.id = i as u32;
            }
            if sources.is_empty() && text.is_none() {
                bail!("nothing to send");
            }
            if let Some(p) = sh.peers.get(peer) {
                if p.kind == "web" {
                    let id = crate::web::offer_to_browser(sh, peer, sources, text)?;
                    return Ok(json!({"id": id}));
                }
            }
            let id = node.send(peer, sources, text, None)?;
            Ok(json!({"id": id}))
        }
        "respond" => {
            let id = s(v, "id")?;
            let a = Answer {
                accept: v.get("accept").and_then(Value::as_bool).unwrap_or(false),
                save_dir: v.get("save_dir").and_then(Value::as_str).map(str::to_string),
                skip: v.get("skip").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as u32).collect()).unwrap_or_default(),
                trust: v.get("trust").and_then(Value::as_bool).unwrap_or(false),
            };
            Ok(json!({"answered": sh.answer(id, a)}))
        }
        "pause" => Ok(json!({"done": node.pause(s(v, "id")?, v.get("paused").and_then(Value::as_bool).unwrap_or(true))})),
        "cancel" => Ok(json!({"done": node.cancel(s(v, "id")?)})),
        "transfer" => {
            let t = sh.transfer(s(v, "id")?).ok_or_else(|| anyhow!("no such transfer"))?;
            Ok(json!({"transfer": t.view(true), "samples": t.samples(400)}))
        }
        "history" => Ok(json!({"history": sh.history.list(v.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize)})),
        "history_entry" => Ok(json!({"entry": sh.history.get(s(v, "id")?)})),
        "history_remove" => {
            sh.history.remove(s(v, "id")?);
            Ok(json!({}))
        }
        "history_clear" => {
            sh.history.clear();
            Ok(json!({}))
        }
        "pin_open" => {
            let pin = sh.pairing.open_pin();
            sh.emit(&crate::node::Event::Pairing { state: "pin", pin: Some(&pin), peer: None, reason: None });
            Ok(json!({"pin": pin}))
        }
        "pin_close" => {
            sh.pairing.close_pin();
            Ok(json!({}))
        }
        "pair_pin" => {
            let peer = crate::pairing::pair_with_pin(sh, s(v, "peer")?, s(v, "pin")?).await?;
            Ok(json!({"peer": peer}))
        }
        "pair_qr" => {
            let (id, fp, secret, addrs, name) = parse_qr(s(v, "uri")?)?;
            for a in &addrs {
                node.add_peer_addr(DeviceInfo { id: id.clone(), name: name.clone(), platform: String::new(), model: String::new(), version: String::new() }, *a, Via::Manual);
            }
            let peer = crate::pairing::pair_with_qr(sh, &id, &fp, &secret).await?;
            Ok(json!({"peer": peer}))
        }
        "qr" => Ok(json!({"uri": qr_uri(node)})),
        "qr_rotate" => {
            sh.pairing.rotate_qr();
            Ok(json!({"uri": qr_uri(node)}))
        }
        "trust" => {
            let id = s(v, "peer")?;
            let p = sh.peers.get(id).ok_or_else(|| anyhow!("unknown device"))?;
            let fp = p.fp.ok_or_else(|| anyhow!("connect to this device once before trusting it"))?;
            sh.peers.trust(&p.device, &fp);
            sh.emit_peers();
            Ok(json!({}))
        }
        "untrust" => {
            sh.peers.untrust(s(v, "peer")?);
            sh.emit_peers();
            Ok(json!({}))
        }
        "forget" => {
            let id = s(v, "peer")?;
            sh.peers.untrust(id);
            sh.peers.remove(id);
            sh.emit_peers();
            Ok(json!({}))
        }
        "add_address" => {
            let raw = s(v, "address")?.trim();
            let addr: SocketAddr = raw
                .parse()
                .or_else(|_| format!("{raw}:{}", crate::settings::DEFAULT_PORT).parse())
                .map_err(|_| anyhow!("not an address: {raw}"))?;
            let dev = probe_address(node, addr).await?;
            node.add_peer_addr(dev.clone(), addr, Via::Manual);
            Ok(json!({"peer": dev}))
        }
        "peer_hint" => {
            // iOS feeds Bonjour results here (raw multicast needs an entitlement).
            let dev = DeviceInfo {
                id: s(v, "id")?.to_string(),
                name: v.get("name").and_then(Value::as_str).unwrap_or("Device").to_string(),
                platform: v.get("platform").and_then(Value::as_str).unwrap_or_default().to_string(),
                model: String::new(),
                version: String::new(),
            };
            if dev.id == sh.identity.id() {
                return Ok(json!({}));
            }
            for a in v.get("addrs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if let Ok(sa) = a.parse::<SocketAddr>() {
                    if sh.peers.upsert(dev.clone(), None, Some(sa), None, "app") {
                        sh.emit_peers();
                    }
                }
            }
            Ok(json!({}))
        }
        "web_info" => Ok(crate::web::info(node)),
        other => Err(anyhow!("unknown command '{other}'")),
    }
}

/// Connects to an address and learns which device lives there.
pub async fn probe_address(node: &Node, addr: SocketAddr) -> Result<DeviceInfo> {
    let sh = &node.shared;
    let (_link, mut s, fp) = sh.dialer().connect(addr, crate::link::Transport::Tcp).await?;
    crate::wire::write_ctrl(&mut s.write, &crate::proto::Ctrl::Hello { proto: crate::proto::PROTO_VERSION, device: sh.device() }).await?;
    match crate::wire::read_ctrl(&mut s.read).await? {
        crate::proto::Ctrl::Hello { device, .. } if device.id == crate::identity::device_id(&fp) => {
            sh.note_peer_key(&device, fp);
            Ok(device)
        }
        _ => bail!("no SweaxDrop device at {addr}"),
    }
}

pub type Shared = Arc<crate::node::Shared>;
