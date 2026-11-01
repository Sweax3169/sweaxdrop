//! End-to-end tests: two nodes in one process talking over loopback.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sweaxdrop_core::api;
use sweaxdrop_core::link::Transport;
use sweaxdrop_core::peers::Via;
use sweaxdrop_core::proto::DeviceInfo;
use sweaxdrop_core::send::EngineOpts;
use sweaxdrop_core::storage::collect_paths;
use sweaxdrop_core::transfer::Phase;
use sweaxdrop_core::{Config, Node};

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("swx-test-{name}-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

async fn node(name: &str) -> (Node, PathBuf) {
    let dir = tmp(name);
    let mut cfg = Config::new(dir.join("data"));
    cfg.discovery = false;
    cfg.port = 0;
    cfg.default_name = name.to_string();
    // Port 0 in the config means "settings value"; give each node its own.
    cfg.port = 40000 + (rand::random::<u16>() % 20000);
    let n = Node::start(cfg).await.unwrap();
    let save = dir.join("inbox");
    api::call(&n, &json!({"cmd": "settings_set", "patch": {"save_dir": save.to_string_lossy()}}).to_string()).await;
    (n, dir)
}

/// Makes the two nodes know and trust each other (as after QR pairing).
fn introduce(a: &Node, b: &Node) {
    for (x, y) in [(a, b), (b, a)] {
        let d = y.shared.device();
        x.add_peer_addr(d.clone(), format!("127.0.0.1:{}", y.port()).parse().unwrap(), Via::Manual);
        x.shared.peers.trust(&d, &y.shared.identity.fingerprint);
    }
}

fn write_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut add = |rel: &str, data: Vec<u8>| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &data).unwrap();
        files.push((rel.to_string(), data));
    };
    add("album/a.jpg", (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect());
    add("album/sub/b.bin", (0..9_500_000u32).map(|i| (i ^ (i >> 9)) as u8).collect());
    add("album/empty.txt", Vec::new());
    add("album/ünïcode şğı.txt", "merhaba dünya".as_bytes().to_vec());
    for i in 0..120 {
        add(&format!("album/small/{i:03}.txt"), format!("file number {i}").repeat(i + 1).into_bytes());
    }
    std::fs::create_dir_all(root.join("album/nothing-here")).unwrap();
    files
}

async fn wait_final(n: &Node, id: &str, secs: u64) -> Phase {
    for _ in 0..secs * 20 {
        if let Some(t) = n.transfer(id) {
            if t.phase().is_final() {
                return t.phase();
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("transfer {id} did not finish");
}

async fn wait_receiver_done(b: &Node, id: &str) {
    for _ in 0..200 {
        if let Some(t) = b.transfer(id) {
            if t.phase() == Phase::Done {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("receiver did not finish {id}");
}

fn check_tree(inbox: &Path, files: &[(String, Vec<u8>)]) {
    for (rel, data) in files {
        let got = std::fs::read(inbox.join(rel)).unwrap_or_else(|_| panic!("missing {rel}"));
        assert_eq!(&got, data, "content of {rel}");
    }
    assert!(inbox.join("album/nothing-here").is_dir(), "empty folder kept");
    let leftovers: Vec<_> = walk(inbox).into_iter().filter(|p| p.to_string_lossy().ends_with(".sdpart")).collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                v.extend(walk(&e.path()));
            } else {
                v.push(e.path());
            }
        }
    }
    v
}

async fn send_folder(transport: Transport, streams: u8, chunk: u32) {
    let (a, da) = node("alpha").await;
    let (b, db) = node("beta").await;
    introduce(&a, &b);
    let src = da.join("src");
    let files = write_tree(&src);
    let sources = collect_paths(&[src.join("album")]).unwrap();
    let opts = EngineOpts { streams, chunk, piece: 256 << 10, transport };
    let id = a.send(&b.id(), sources, None, Some(opts)).unwrap();
    assert_eq!(wait_final(&a, &id, 60).await, Phase::Done, "{:?}", a.transfer(&id).unwrap().error.read());
    wait_receiver_done(&b, &id).await;
    check_tree(&db.join("inbox"), &files);
    let t = b.transfer(&id).unwrap();
    assert_eq!(t.files_done.load(std::sync::atomic::Ordering::Relaxed), files.len() as u32);
    assert_eq!(b.shared.history.list(5)[0].id, id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_over_tcp_four_lanes() {
    send_folder(Transport::Tcp, 4, 1 << 20).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_over_quic() {
    send_folder(Transport::Quic, 4, 1 << 20).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_one_lane_big_chunks() {
    send_folder(Transport::Tcp, 1, 8 << 20).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn text_message() {
    let (a, _) = node("alpha").await;
    let (b, _) = node("beta").await;
    introduce(&a, &b);
    let got = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let g = got.clone();
    b.set_event_sink(Arc::new(move |e: String| g.lock().push(e)));
    let id = a.send(&b.id(), vec![], Some("hello from alpha — şğüı".into()), None).unwrap();
    assert_eq!(wait_final(&a, &id, 10).await, Phase::Done);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(got.lock().iter().any(|e| e.contains("\"type\":\"text\"") && e.contains("hello from alpha")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn untrusted_sender_is_asked_and_can_be_declined() {
    let (a, _) = node("alpha").await;
    let (b, _) = node("beta").await;
    let d = b.shared.device();
    a.add_peer_addr(d, format!("127.0.0.1:{}", b.port()).parse().unwrap(), Via::Manual);
    let bb = b.clone();
    b.set_event_sink(Arc::new(move |e: String| {
        if e.contains("\"type\":\"incoming\"") {
            let v: serde_json::Value = serde_json::from_str(&e).unwrap();
            let id = v["transfer"]["id"].as_str().unwrap().to_string();
            let bb = bb.clone();
            tokio::spawn(async move {
                api::call(&bb, &json!({"cmd": "respond", "id": id, "accept": false}).to_string()).await;
            });
        }
    }));
    let id = a.send(&b.id(), vec![], Some("hi".into()), None).unwrap();
    assert_eq!(wait_final(&a, &id, 10).await, Phase::Declined);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_key_is_refused() {
    let (a, _) = node("alpha").await;
    let (b, _) = node("beta").await;
    let (c, _) = node("gamma").await;
    // alpha trusts beta's id, but the address belongs to gamma.
    let d = b.shared.device();
    a.add_peer_addr(d.clone(), format!("127.0.0.1:{}", c.port()).parse().unwrap(), Via::Manual);
    a.shared.peers.trust(&d, &b.shared.identity.fingerprint);
    let id = a.send(&b.id(), vec![], Some("secret".into()), None).unwrap();
    assert_eq!(wait_final(&a, &id, 10).await, Phase::Failed);
    let err = a.transfer(&id).unwrap().error.read().clone().unwrap();
    assert!(err.contains("different key"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume() {
    let (a, da) = node("alpha").await;
    let (b, db) = node("beta").await;
    introduce(&a, &b);
    let p = da.join("big.bin");
    let data: Vec<u8> = (0..24_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    std::fs::write(&p, &data).unwrap();
    let id = a.send(&b.id(), collect_paths(&[p]).unwrap(), None, Some(EngineOpts { streams: 2, chunk: 1 << 20, piece: 128 << 10, transport: Transport::Tcp })).unwrap();
    a.pause(&id, true);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let t = a.transfer(&id).unwrap();
    let held = t.bytes.load(std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(held, t.bytes.load(std::sync::atomic::Ordering::Relaxed), "no progress while paused");
    assert!(held < data.len() as u64);
    a.pause(&id, false);
    assert_eq!(wait_final(&a, &id, 30).await, Phase::Done);
    wait_receiver_done(&b, &id).await;
    assert_eq!(std::fs::read(db.join("inbox/big.bin")).unwrap(), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pin_pairing() {
    let (a, _) = node("alpha").await;
    let (b, _) = node("beta").await;
    let d = b.shared.device();
    a.add_peer_addr(d, format!("127.0.0.1:{}", b.port()).parse().unwrap(), Via::Manual);
    let r: serde_json::Value = serde_json::from_str(&api::call(&b, r#"{"cmd":"pin_open"}"#).await).unwrap();
    let pin = r["pin"].as_str().unwrap().to_string();
    let wrong = if pin == "000000" { "111111" } else { "000000" };
    let bad: serde_json::Value = serde_json::from_str(&api::call(&a, &json!({"cmd":"pair_pin","peer": b.id(),"pin": wrong}).to_string()).await).unwrap();
    assert_eq!(bad["ok"], false);
    assert!(!a.shared.peers.is_trusted(&b.id(), &b.shared.identity.fingerprint));
    let good: serde_json::Value = serde_json::from_str(&api::call(&a, &json!({"cmd":"pair_pin","peer": b.id(),"pin": pin}).to_string()).await).unwrap();
    assert_eq!(good["ok"], true, "{good}");
    assert!(a.shared.peers.is_trusted(&b.id(), &b.shared.identity.fingerprint));
    assert!(b.shared.peers.is_trusted(&a.id(), &a.shared.identity.fingerprint));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qr_pairing() {
    let (a, _) = node("alpha").await;
    let (b, _) = node("beta").await;
    let uri = api::qr_uri(&b).replace(&format!("{}/", b.port()), &format!("{}/", b.port()));
    // The code lists LAN addresses; point it at loopback for the test.
    let (id, fp, secret, _, name) = api::parse_qr(&uri).unwrap();
    let uri = format!("http://127.0.0.1:{}/#s={secret}&i={id}&k={fp}&a=127.0.0.1&n={name}", b.port());
    let r: serde_json::Value = serde_json::from_str(&api::call(&a, &json!({"cmd":"pair_qr","uri": uri}).to_string()).await).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert!(a.shared.peers.is_trusted(&b.id(), &b.shared.identity.fingerprint));
    assert!(b.shared.peers.is_trusted(&a.id(), &a.shared.identity.fingerprint));
    let _ = DeviceInfo { id: String::new(), name: String::new(), platform: String::new(), model: String::new(), version: String::new() };
}

/// A loopback proxy that cuts every connection once `cut_after` bytes went
/// through it (the first time only), like Wi-Fi dropping mid-transfer.
async fn flaky_proxy(target: u16, cut_after: u64) -> u16 {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let moved = Arc::new(AtomicU64::new(0));
    let cut = Arc::new(AtomicBool::new(false));
    let kill = Arc::new(tokio::sync::Notify::new());
    tokio::spawn(async move {
        loop {
            let (c, _) = l.accept().await.unwrap();
            let s = tokio::net::TcpStream::connect(("127.0.0.1", target)).await.unwrap();
            let (moved, cut, kill) = (moved.clone(), cut.clone(), kill.clone());
            tokio::spawn(async move {
                let (mut cr, mut cw) = c.into_split();
                let (mut sr, mut sw) = s.into_split();
                let up = async {
                    let mut b = vec![0u8; 64 << 10];
                    loop {
                        let n = cr.read(&mut b).await.unwrap_or(0);
                        if n == 0 { break; }
                        if sw.write_all(&b[..n]).await.is_err() { break; }
                        let total = moved.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                        if total > cut_after && !cut.swap(true, Ordering::Relaxed) {
                            kill.notify_waiters();
                            break;
                        }
                    }
                };
                let down = async {
                    let mut b = vec![0u8; 64 << 10];
                    loop {
                        let n = sr.read(&mut b).await.unwrap_or(0);
                        if n == 0 { break; }
                        if cw.write_all(&b[..n]).await.is_err() { break; }
                    }
                };
                tokio::select! { _ = up => {}, _ = down => {}, _ = kill.notified() => {} }
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resumes_after_the_connection_drops() {
    let (a, da) = node("alpha").await;
    let (b, db) = node("beta").await;
    introduce(&a, &b);
    let proxy = flaky_proxy(b.port(), 20_000_000).await;
    let d = b.shared.device();
    a.shared.peers.remove(&d.id);
    a.add_peer_addr(d, format!("127.0.0.1:{proxy}").parse().unwrap(), Via::Manual);
    let p = da.join("movie.bin");
    let data: Vec<u8> = (0..48_000_000u32).map(|i| (i.wrapping_mul(2246822519) >> 11) as u8).collect();
    std::fs::write(&p, &data).unwrap();
    let opts = EngineOpts { streams: 4, chunk: 1 << 20, piece: 256 << 10, transport: Transport::Tcp };
    let id = a.send(&b.id(), collect_paths(&[p]).unwrap(), None, Some(opts)).unwrap();
    assert_eq!(wait_final(&a, &id, 60).await, Phase::Done, "{:?}", a.transfer(&id).unwrap().error.read());
    wait_receiver_done(&b, &id).await;
    let t = a.transfer(&id).unwrap();
    assert!(t.attempts.load(std::sync::atomic::Ordering::Relaxed) >= 2, "it reconnected");
    assert!(t.resumed.load(std::sync::atomic::Ordering::Relaxed) > 4_000_000, "kept verified chunks");
    assert_eq!(std::fs::read(db.join("inbox/movie.bin")).unwrap(), data);
}
