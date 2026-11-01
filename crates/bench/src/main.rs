//! sweaxdrop-bench: measures SweaxDrop (and LocalSend) on the same machine,
//! network and files. Every number in docs/benchmarks.md comes from here.

mod localsend;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;
use sweaxdrop_core::link::Transport;
use sweaxdrop_core::peers::Via;
use sweaxdrop_core::proto::Item;
use sweaxdrop_core::send::EngineOpts;
use sweaxdrop_core::storage::{collect_paths, Source, SourceItem};
use sweaxdrop_core::tls::CipherPref;
use sweaxdrop_core::transfer::Phase;
use sweaxdrop_core::{api, Config, Host};

#[derive(Parser)]
#[command(name = "sweaxdrop-bench", about = "SweaxDrop throughput benchmarks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Creates the benchmark files: one huge file, many small files, photos.
    Dataset {
        #[arg(long, default_value = "bench/data")]
        out: PathBuf,
        #[arg(long, default_value_t = 4)]
        huge_gb: u64,
    },
    /// Two nodes on this machine over loopback.
    Loopback {
        /// Synthetic bytes (no disk read), e.g. 2G. Ignored with --path.
        #[arg(long, default_value = "2G")]
        bytes: String,
        /// Send these files/folders instead of synthetic bytes.
        #[arg(long)]
        path: Vec<PathBuf>,
        #[arg(long, default_value_t = 4)]
        streams: u8,
        #[arg(long, default_value = "tcp")]
        transport: String,
        #[arg(long, default_value = "aes")]
        cipher: String,
        #[arg(long, default_value_t = 4)]
        chunk_mib: u32,
        /// Write received files to disk (default: verify and discard).
        #[arg(long)]
        disk: bool,
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        #[arg(long, default_value = "loopback")]
        case: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// The loopback matrix used for docs/benchmarks.md.
    Matrix {
        #[arg(long, default_value = "bench/data")]
        data: PathBuf,
        #[arg(long, default_value = "bench/results/loopback.json")]
        out: PathBuf,
        #[arg(long, default_value_t = 3)]
        repeat: u32,
    },
    /// A receiving node that accepts everything (for device-to-device runs).
    Serve {
        #[arg(long, default_value_t = 47470)]
        port: u16,
        #[arg(long)]
        save: Option<PathBuf>,
        #[arg(long)]
        null: bool,
    },
    /// Sends to a SweaxDrop device at an address and reports the run.
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        path: Vec<PathBuf>,
        #[arg(long, default_value_t = 4)]
        streams: u8,
        #[arg(long, default_value = "tcp")]
        transport: String,
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Times creating many small files on this disk, several ways (diagnostics).
    Fstest {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 5000)]
        files: usize,
    },
    /// Sends with the LocalSend v2 protocol to a running LocalSend app.
    Localsend {
        #[arg(long)]
        to: String,
        #[arg(long)]
        path: Vec<PathBuf>,
        /// Files uploaded at the same time (LocalSend's own sender: see docs).
        #[arg(long, default_value_t = 1)]
        concurrency: usize,
        #[arg(long)]
        http: bool,
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Serialize, Clone)]
pub struct Run {
    pub tool: String,
    pub case: String,
    pub bytes: u64,
    pub files: u64,
    pub seconds: f64,
    pub ttfb_ms: f64,
    pub mb_s: f64,
    pub cpu_s_per_gb: f64,
    pub settings: serde_json::Value,
}

fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    let (num, mul) = match s.chars().last() {
        Some('K') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M') => (&s[..s.len() - 1], 1 << 20),
        Some('G') => (&s[..s.len() - 1], 1 << 30),
        _ => (&s[..], 1),
    };
    Ok((num.parse::<f64>()? * mul as f64) as u64)
}

pub fn cpu_seconds() -> f64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) != 0 {
            let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) as f64 / 1e7;
            return f(k) + f(u);
        }
        0.0
    }
    #[cfg(not(windows))]
    {
        0.0
    }
}

fn transport(s: &str) -> Transport {
    if s.eq_ignore_ascii_case("quic") {
        Transport::Quic
    } else {
        Transport::Tcp
    }
}

fn host(name: &str, port: u16, cipher: CipherPref, bench_sink: bool, save: Option<&Path>) -> Result<Host> {
    let dir = std::env::temp_dir().join(format!("swx-bench-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::new(dir.join("data"));
    cfg.discovery = false;
    cfg.loopback = port == 0;
    cfg.port = if port == 0 { 41000 + (std::process::id() % 2000) as u16 * 4 + name.len() as u16 % 4 } else { port };
    cfg.default_name = name.into();
    cfg.bench_sink = bench_sink;
    cfg.accept_all = true;
    let h = Host::start(cfg)?;
    let mut patch = json!({"cipher": cipher, "streams": 16, "keep_history": false});
    if let Some(p) = save {
        patch["save_dir"] = json!(p.to_string_lossy());
    }
    h.call_blocking(&json!({"cmd": "settings_set", "patch": patch}).to_string());
    Ok(h)
}

fn pair(a: &Host, b: &Host) {
    for (x, y) in [(a, b), (b, a)] {
        let d = y.node.shared.device();
        x.node.add_peer_addr(d.clone(), format!("127.0.0.1:{}", y.node.port()).parse().unwrap(), Via::Manual);
        x.node.shared.peers.trust(&d, &y.node.shared.identity.fingerprint);
    }
}

fn synthetic(bytes: u64) -> Vec<SourceItem> {
    vec![SourceItem {
        item: Item { id: 0, path: "synthetic.bin".into(), size: bytes, mtime: None, mime: None, dir: false },
        src: Source::Synthetic { seed: 0x5eed },
    }]
}

/// One SweaxDrop transfer between two in-process nodes.
fn loopback_once(a: &Host, b: &Host, sources: Vec<SourceItem>, opts: EngineOpts) -> Result<(f64, f64, f64)> {
    let cpu0 = cpu_seconds();
    let t0 = Instant::now();
    let id = a.rt.block_on(async { a.node.send(&b.node.id(), sources, None, Some(opts)) })?;
    let mut ttfb = None;
    loop {
        if ttfb.is_none() {
            if let Some(t) = b.node.transfer(&id) {
                if t.bytes.load(Ordering::Relaxed) > 0 {
                    ttfb = Some(t0.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        if let Some(t) = a.node.transfer(&id) {
            let p = t.phase();
            if p.is_final() {
                if p != Phase::Done {
                    return Err(anyhow!("transfer ended {p:?}: {:?}", t.error.read()));
                }
                break;
            }
        }
        std::thread::sleep(Duration::from_micros(500));
    }
    let secs = t0.elapsed().as_secs_f64();
    Ok((secs, ttfb.unwrap_or(secs * 1000.0), cpu_seconds() - cpu0))
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[allow(clippy::too_many_arguments)]
fn loopback_case(case: &str, paths: &[PathBuf], bytes: u64, streams: u8, tr: Transport, cipher: CipherPref, chunk_mib: u32, disk: bool, repeat: u32) -> Result<Run> {
    // SWX_BENCH_INBOX puts received files on a chosen drive (same as the
    // LocalSend runs, so both write to the same disk).
    let save = std::env::var_os("SWX_BENCH_INBOX")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("swx-bench-inbox-{}", std::process::id()));
    let a = host("bench-a", 0, cipher, false, None)?;
    let b = host("bench-bb", 0, cipher, !disk, Some(&save))?;
    pair(&a, &b);
    let opts = EngineOpts { streams, chunk: chunk_mib << 20, piece: 512 << 10, transport: tr };
    // One unrecorded transfer of 64 small files first. A long-running app pays
    // its one-time costs (thread pools, connections, and Windows Defender's
    // look at a process's first file writes) once; a fresh benchmark process
    // would otherwise pay them inside the first measured run.
    let warm: Vec<SourceItem> = (0..64)
        .map(|i| SourceItem {
            item: Item { id: i, path: format!("warmup/w{i:02}.bin"), size: 32 << 10, mtime: None, mime: None, dir: false },
            src: Source::Synthetic { seed: i as u64 },
        })
        .collect();
    loopback_once(&a, &b, warm, opts)?;
    if disk {
        let _ = std::fs::remove_dir_all(&save);
    }
    let (mut secs, mut ttfbs, mut cpus) = (vec![], vec![], vec![]);
    let (mut total, mut files) = (0, 0);
    for _ in 0..repeat {
        let sources = if paths.is_empty() { synthetic(bytes) } else { collect_paths(paths)? };
        total = sources.iter().map(|s| s.item.size).sum();
        files = sources.iter().filter(|s| !s.item.dir).count() as u64;
        let (s, t, c) = loopback_once(&a, &b, sources, opts)?;
        secs.push(s);
        ttfbs.push(t);
        cpus.push(c);
        if disk {
            let _ = std::fs::remove_dir_all(&save);
        }
    }
    let s = median(secs);
    let run = Run {
        tool: "sweaxdrop".into(),
        case: case.into(),
        bytes: total,
        files,
        seconds: s,
        ttfb_ms: median(ttfbs),
        mb_s: total as f64 / s / 1e6,
        cpu_s_per_gb: median(cpus) / (total as f64 / 1e9),
        settings: json!({"streams": streams, "transport": tr, "cipher": cipher, "chunk_mib": chunk_mib, "disk": disk, "repeat": repeat}),
    };
    println!(
        "{:<44} {:>8.1} MB/s  {:>7.2} s  ttfb {:>6.1} ms  cpu {:>5.2} s/GB",
        format!("{case} [{streams}x {tr:?} {cipher:?}]"),
        run.mb_s,
        run.seconds,
        run.ttfb_ms,
        run.cpu_s_per_gb
    );
    a.rt.shutdown_background();
    b.rt.shutdown_background();
    Ok(run)
}

pub fn save_runs(out: &Path, runs: &[Run]) -> Result<()> {
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut all: Vec<serde_json::Value> = std::fs::read(out).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for r in runs {
        let mut v = serde_json::to_value(r)?;
        v["machine"] = json!(machine());
        v["when"] = json!(sweaxdrop_core::util::now_ms());
        all.push(v);
    }
    std::fs::write(out, serde_json::to_vec_pretty(&all)?)?;
    Ok(())
}

pub fn machine() -> String {
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default()
}

fn dataset(out: &Path, huge_gb: u64) -> Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(out)?;
    let huge = out.join("huge").join(format!("huge-{huge_gb}g.bin"));
    if !huge.exists() {
        std::fs::create_dir_all(huge.parent().unwrap())?;
        let mut f = std::io::BufWriter::with_capacity(8 << 20, std::fs::File::create(&huge)?);
        let mut buf = vec![0u8; 8 << 20];
        for i in 0..(huge_gb << 30) / buf.len() as u64 {
            sweaxdrop_core::storage::fill_synthetic(77, i * buf.len() as u64, &mut buf);
            f.write_all(&buf)?;
        }
    }
    // 10 000 small files of 1–64 KiB in 100 folders, like a source tree or chat export.
    let small = out.join("small");
    if !small.exists() {
        let mut x: u64 = 12345;
        for i in 0..10_000u32 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let size = 1024 + (x % (63 * 1024)) as usize;
            let p = small.join(format!("d{:02}", i % 100)).join(format!("f{i:05}.dat"));
            std::fs::create_dir_all(p.parent().unwrap())?;
            let mut b = vec![0u8; size];
            sweaxdrop_core::storage::fill_synthetic(i as u64, 0, &mut b);
            std::fs::write(&p, &b)?;
        }
    }
    // 200 "photos" of 3–6 MB.
    let photos = out.join("photos");
    if !photos.exists() {
        std::fs::create_dir_all(&photos)?;
        for i in 0..200u32 {
            let size = 3_000_000 + (i as usize * 7919 % 3_000_000);
            let mut b = vec![0u8; size];
            sweaxdrop_core::storage::fill_synthetic(1000 + i as u64, 0, &mut b);
            std::fs::write(photos.join(format!("IMG_{:04}.jpg", 1000 + i)), &b)?;
        }
    }
    println!("dataset ready in {}", out.display());
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Dataset { out, huge_gb } => dataset(&out, huge_gb),
        Cmd::Loopback { bytes, path, streams, transport: tr, cipher, chunk_mib, disk, repeat, case, out } => {
            let c = if cipher == "chacha" { CipherPref::Chacha } else { CipherPref::Aes };
            let run = loopback_case(&case, &path, parse_size(&bytes)?, streams, transport(&tr), c, chunk_mib, disk, repeat)?;
            if let Some(o) = out {
                save_runs(&o, &[run])?;
            }
            Ok(())
        }
        Cmd::Matrix { data, out, repeat } => {
            let mut runs = Vec::new();
            let g2 = 2u64 << 30;
            println!("-- network path only (synthetic source, verified and discarded)");
            for tr in [Transport::Tcp, Transport::Quic] {
                for s in [1u8, 2, 4, 8] {
                    runs.push(loopback_case("2 GiB in memory", &[], g2, s, tr, CipherPref::Aes, 4, false, repeat)?);
                }
            }
            runs.push(loopback_case("2 GiB in memory", &[], g2, 4, Transport::Tcp, CipherPref::Chacha, 4, false, repeat)?);
            for c in [1u32, 8, 16] {
                runs.push(loopback_case(&format!("2 GiB, {c} MiB chunks"), &[], g2, 4, Transport::Tcp, CipherPref::Aes, c, false, repeat)?);
            }
            println!("-- disk to disk");
            let huge: Vec<PathBuf> = std::fs::read_dir(data.join("huge"))?.flatten().map(|e| e.path()).collect();
            for s in [1u8, 4] {
                runs.push(loopback_case("huge file disk to disk", &huge, 0, s, Transport::Tcp, CipherPref::Aes, 4, true, repeat)?);
            }
            for s in [1u8, 4, 8] {
                runs.push(loopback_case("10 000 small files", &[data.join("small")], 0, s, Transport::Tcp, CipherPref::Aes, 4, true, repeat)?);
            }
            runs.push(loopback_case("200 photos", &[data.join("photos")], 0, 4, Transport::Tcp, CipherPref::Aes, 4, true, repeat)?);
            save_runs(&out, &runs)
        }
        Cmd::Serve { port, save, null } => {
            let h = host("bench-serve", port, CipherPref::Aes, null, save.as_deref())?;
            h.node.set_event_sink(Arc::new(|e: String| {
                if e.contains("\"type\":\"history\"") {
                    println!("{e}");
                }
            }));
            println!("serving on port {} (id {})", h.node.port(), h.node.id());
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        Cmd::Send { to, path, streams, transport: tr, repeat, label, out } => {
            let a = host("bench-send", 0, CipherPref::Aes, false, None)?;
            let addr: std::net::SocketAddr = to.parse()?;
            let dev = a.rt.block_on(api::probe_address(&a.node, addr))?;
            a.node.add_peer_addr(dev.clone(), addr, Via::Manual);
            let opts = EngineOpts { streams, chunk: 4 << 20, piece: 512 << 10, transport: transport(&tr) };
            let (mut secs, mut cpus) = (vec![], vec![]);
            let mut total = 0;
            let mut files = 0;
            for _ in 0..repeat {
                let sources = collect_paths(&path)?;
                total = sources.iter().map(|s| s.item.size).sum();
                files = sources.len() as u64;
                let cpu0 = cpu_seconds();
                let t0 = Instant::now();
                let id = a.rt.block_on(async { a.node.send(&dev.id, sources, None, Some(opts)) })?;
                loop {
                    let t = a.node.transfer(&id).unwrap();
                    if t.phase().is_final() {
                        if t.phase() != Phase::Done {
                            return Err(anyhow!("{:?}: {:?}", t.phase(), t.error.read()));
                        }
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                secs.push(t0.elapsed().as_secs_f64());
                cpus.push(cpu_seconds() - cpu0);
            }
            let s = median(secs);
            let run = Run {
                tool: "sweaxdrop".into(),
                case: label.unwrap_or_else(|| "send".into()),
                bytes: total,
                files,
                seconds: s,
                ttfb_ms: 0.0,
                mb_s: total as f64 / s / 1e6,
                cpu_s_per_gb: median(cpus) / (total as f64 / 1e9),
                settings: json!({"to": dev.platform, "streams": streams, "transport": tr}),
            };
            println!("{}", serde_json::to_string_pretty(&run)?);
            if let Some(o) = out {
                save_runs(&o, &[run])?;
            }
            Ok(())
        }
        Cmd::Fstest { dir, files } => fstest(&dir, files),
        Cmd::Localsend { to, path, concurrency, http, repeat, label, out } => {
            let rt = tokio::runtime::Runtime::new()?;
            let run = rt.block_on(localsend::bench(&to, &path, concurrency, !http, repeat, label))?;
            println!("{}", serde_json::to_string_pretty(&run)?);
            if let Some(o) = out {
                save_runs(&o, &[run])?;
            }
            Ok(())
        }
    }
}

fn fstest(dir: &Path, n: usize) -> Result<()> {
    use std::io::Write;
    use sweaxdrop_core::storage::{FsSink, Sink};
    let data = vec![7u8; 33_000];
    for threads in [1usize, 4, 16] {
        for mode in ["plain", "exists+mtime", "fssink"] {
            let root = dir.join(format!("fs-{mode}-{threads}"));
            let _ = std::fs::remove_dir_all(&root);
            for d in 0..100 {
                std::fs::create_dir_all(root.join(format!("d{d:02}")))?;
            }
            let sink = Arc::new(FsSink::new(root.clone()));
            let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let t0 = Instant::now();
            std::thread::scope(|sc| {
                for _ in 0..threads {
                    let (next, root, data, sink) = (next.clone(), root.clone(), &data, sink.clone());
                    sc.spawn(move || loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            return;
                        }
                        let rel = format!("d{:02}/f{i:05}.dat", i % 100);
                        match mode {
                            "plain" => {
                                let mut f = std::fs::File::create_new(root.join(&rel)).unwrap();
                                f.write_all(data).unwrap();
                            }
                            "exists+mtime" => {
                                let p = root.join(&rel);
                                let _ = p.exists();
                                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).unwrap();
                                f.write_all(data).unwrap();
                                let _ = f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000));
                            }
                            _ => {
                                let it = Item { id: i as u32, path: rel, size: data.len() as u64, mtime: Some(1_700_000_000_000), mime: None, dir: false };
                                let out = sink.create(&it, "t", "s").unwrap();
                                out.write_at(data, 0).unwrap();
                                out.finish(it.mtime).unwrap();
                            }
                        }
                    });
                }
            });
            let dt = t0.elapsed().as_secs_f64();
            println!("{threads:>2} threads {mode:<13} {:>6.2} s  {:>6.3} ms/file", dt, dt * 1000.0 / n as f64);
            let _ = std::fs::remove_dir_all(&root);
        }
    }
    Ok(())
}
