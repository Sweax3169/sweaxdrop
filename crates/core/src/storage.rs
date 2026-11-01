//! Where bytes come from and where they go.
//!
//! Desktop builds read and write ordinary paths. On Android and iOS the host
//! app owns file access (content URIs, MediaStore, security-scoped URLs), so
//! the core asks it for file descriptors through [`Platform`]. Both are
//! hidden behind positioned reads/writes so parallel chunks of one file can
//! land at their own offsets without a shared cursor.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::proto::Item;

// ---------------------------------------------------------------- positioned IO

pub fn read_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(f, buf, off)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(f, buf, off)
    }
}

pub fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(f, buf, off)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(f, buf, off)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "disk accepted 0 bytes"));
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

/// Opens a file descriptor handed over by the host app (Android/iOS).
#[cfg(unix)]
pub fn file_from_fd(fd: i64) -> io::Result<File> {
    use std::os::unix::io::FromRawFd;
    if fd < 0 {
        return Err(io::Error::other("host returned no file descriptor"));
    }
    // SAFETY: the host detached this descriptor for us; we own it from here.
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

#[cfg(not(unix))]
pub fn file_from_fd(_fd: i64) -> io::Result<File> {
    Err(io::Error::other("file descriptors from the host are only used on Android/iOS"))
}

// ---------------------------------------------------------------- platform hooks

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputRequest {
    pub transfer: String,
    pub path: String,
    pub size: u64,
    pub mime: Option<String>,
    pub sender: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputHandle {
    pub fd: i64,
    /// Opaque to the core; passed back to `finish_output`.
    pub handle: String,
}

/// Implemented by the Android and iOS hosts. Every call may block briefly.
pub trait Platform: Send + Sync {
    fn open_read(&self, token: &str) -> io::Result<i64>;
    fn create_output(&self, req: &OutputRequest) -> io::Result<OutputHandle>;
    /// `ok=false` discards the output. Returns where the file ended up.
    fn finish_output(&self, handle: &str, ok: bool, mtime: Option<i64>) -> io::Result<String>;
    fn set_clipboard(&self, _text: &str) {}
    fn notify(&self, _title: &str, _body: &str) {}
}

// ---------------------------------------------------------------- sources

#[derive(Clone, Debug)]
pub enum Source {
    Path(PathBuf),
    /// Resolved by [`Platform::open_read`] (Android content URI, iOS URL).
    Platform(String),
    Memory(Arc<Vec<u8>>),
    /// Deterministic bytes generated on the fly (benchmarks only).
    Synthetic { seed: u64 },
}

#[derive(Clone, Debug)]
pub struct SourceItem {
    pub item: Item,
    pub src: Source,
}

pub enum Reader {
    File(File),
    Memory(Arc<Vec<u8>>),
    Synthetic(u64),
}

impl Reader {
    pub fn open(src: &Source, platform: Option<&Arc<dyn Platform>>) -> Result<Self> {
        Ok(match src {
            Source::Path(p) => Reader::File(File::open(p).map_err(|e| anyhow!("{}: {e}", p.display()))?),
            Source::Platform(t) => {
                let p = platform.ok_or_else(|| anyhow!("no platform bridge"))?;
                Reader::File(file_from_fd(p.open_read(t)?)?)
            }
            Source::Memory(b) => Reader::Memory(b.clone()),
            Source::Synthetic { seed } => Reader::Synthetic(*seed),
        })
    }

    /// Fills `buf` from `off`; returns bytes read (short only at end of data).
    pub fn read_full_at(&mut self, buf: &mut [u8], off: u64) -> io::Result<usize> {
        match self {
            Reader::File(f) => {
                let mut done = 0;
                while done < buf.len() {
                    let n = read_at(f, &mut buf[done..], off + done as u64)?;
                    if n == 0 {
                        break;
                    }
                    done += n;
                }
                Ok(done)
            }
            Reader::Memory(b) => {
                let start = (off as usize).min(b.len());
                let n = buf.len().min(b.len() - start);
                buf[..n].copy_from_slice(&b[start..start + n]);
                Ok(n)
            }
            Reader::Synthetic(seed) => {
                fill_synthetic(*seed, off, buf);
                Ok(buf.len())
            }
        }
    }
}

/// Cheap incompressible bytes (xorshift), position-addressable.
pub fn fill_synthetic(seed: u64, off: u64, buf: &mut [u8]) {
    let mut i = 0;
    let mut pos = off;
    while i < buf.len() {
        let word = pos / 8;
        let mut x = seed ^ word.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let bytes = x.to_le_bytes();
        let start = (pos % 8) as usize;
        let n = (8 - start).min(buf.len() - i);
        buf[i..i + n].copy_from_slice(&bytes[start..start + n]);
        i += n;
        pos += n as u64;
    }
}

/// Walks files and folders into items with relative paths. Folders keep
/// their own name as the first path component; empty folders become `dir` items.
pub fn collect_paths(paths: &[PathBuf]) -> Result<Vec<SourceItem>> {
    let mut out = Vec::new();
    for p in paths {
        let meta = fs::metadata(p).map_err(|e| anyhow!("{}: {e}", p.display()))?;
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "folder".into());
        if meta.is_dir() {
            walk(p, &name, &mut out)?;
        } else {
            push_file(&mut out, p.clone(), name, &meta);
        }
    }
    for (i, s) in out.iter_mut().enumerate() {
        s.item.id = i as u32;
    }
    Ok(out)
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<SourceItem>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    if entries.is_empty() {
        out.push(SourceItem {
            item: Item { id: 0, path: rel.to_string(), size: 0, mtime: None, mime: None, dir: true },
            src: Source::Memory(Arc::new(Vec::new())),
        });
    }
    for e in entries {
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = e.file_name().to_string_lossy().to_string();
        let child = format!("{rel}/{name}");
        if ft.is_dir() {
            walk(&e.path(), &child, out)?;
        } else if ft.is_file() {
            if let Ok(meta) = e.metadata() {
                push_file(out, e.path(), child, &meta);
            }
        }
    }
    Ok(())
}

fn push_file(out: &mut Vec<SourceItem>, path: PathBuf, rel: String, meta: &fs::Metadata) {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    let mime = crate::util::mime_for(&rel).map(str::to_string);
    out.push(SourceItem {
        item: Item { id: 0, path: rel, size: meta.len(), mtime, mime, dir: false },
        src: Source::Path(path),
    });
}

// ---------------------------------------------------------------- sinks

/// Makes a relative path from the network safe to join under a folder:
/// no `..`, no roots or drive letters, no characters Windows refuses,
/// no reserved device names, no trailing dots or spaces.
pub fn safe_relative(path: &str) -> PathBuf {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut out = PathBuf::new();
    for part in path.split(['/', '\\']) {
        let mut s: String = part
            .chars()
            .map(|c| if c < ' ' || "<>:\"|?*".contains(c) { '_' } else { c })
            .collect();
        while s.ends_with('.') || s.ends_with(' ') {
            s.pop();
        }
        if s.is_empty() || s == "." || s == ".." {
            continue;
        }
        let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
        if RESERVED.contains(&stem.as_str()) {
            s = format!("_{s}");
        }
        if s.len() > 240 {
            let mut cut = 240;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            s.truncate(cut);
        }
        out.push(s);
    }
    debug_assert!(out.components().all(|c| matches!(c, Component::Normal(_))));
    if out.as_os_str().is_empty() {
        out.push("file");
    }
    out
}

/// `photo.jpg` → `photo (2).jpg` until the name is free.
pub fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let parent = p.parent().unwrap_or(Path::new(""));
    for n in 2.. {
        let c = parent.join(format!("{stem} ({n}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    unreachable!()
}

pub trait OutFile: Send + Sync {
    fn write_at(&self, buf: &[u8], off: u64) -> io::Result<()>;
    /// Makes the file visible under its final name; returns the location.
    fn finish(&self, mtime: Option<i64>) -> Result<String>;
    /// Gives up on the file. With `keep`, the partial data stays for resume.
    fn abort(&self, keep: bool);
    /// Where partial data lives (for the resume index), if anywhere.
    fn partial_path(&self) -> Option<PathBuf> {
        None
    }
}

pub trait Sink: Send + Sync {
    fn create(&self, item: &Item, transfer: &str, sender: &str) -> Result<Arc<dyn OutFile>>;
    /// Reopens partial data kept by an earlier attempt.
    fn reopen(&self, _item: &Item, _partial: &Path) -> Result<Arc<dyn OutFile>> {
        Err(anyhow!("resume not supported here"))
    }
    fn make_dir(&self, path: &str) -> Result<()>;
    fn describe(&self) -> String;
}

pub struct FsSink {
    pub root: PathBuf,
}

struct FsOut {
    file: parking_lot::RwLock<Option<File>>,
    temp: PathBuf,
    target: PathBuf,
}

impl FsSink {
    fn target(&self, item: &Item) -> PathBuf {
        self.root.join(safe_relative(&item.path))
    }
}

impl Sink for FsSink {
    fn create(&self, item: &Item, _transfer: &str, _sender: &str) -> Result<Arc<dyn OutFile>> {
        let target = self.target(item);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut temp = target.clone().into_os_string();
        temp.push(".sdpart");
        let temp = unique_path(Path::new(&temp));
        let f = OpenOptions::new().read(true).write(true).create_new(true).open(&temp)?;
        if item.size > 0 {
            f.set_len(item.size)?;
        }
        Ok(Arc::new(FsOut { file: parking_lot::RwLock::new(Some(f)), temp, target }))
    }

    fn reopen(&self, item: &Item, partial: &Path) -> Result<Arc<dyn OutFile>> {
        let f = OpenOptions::new().read(true).write(true).open(partial)?;
        if f.metadata()?.len() != item.size {
            return Err(anyhow!("partial file has the wrong size"));
        }
        Ok(Arc::new(FsOut {
            file: parking_lot::RwLock::new(Some(f)),
            temp: partial.to_path_buf(),
            target: self.target(item),
        }))
    }

    fn make_dir(&self, path: &str) -> Result<()> {
        fs::create_dir_all(self.root.join(safe_relative(path)))?;
        Ok(())
    }

    fn describe(&self) -> String {
        self.root.display().to_string()
    }
}

impl OutFile for FsOut {
    fn write_at(&self, buf: &[u8], off: u64) -> io::Result<()> {
        // Positioned writes need no shared cursor, so many writers can hold
        // the read lock at once; only finish/abort take the write lock.
        match self.file.read().as_ref() {
            Some(f) => write_all_at(f, buf, off),
            None => Err(io::Error::other("file already closed")),
        }
    }

    fn finish(&self, mtime: Option<i64>) -> Result<String> {
        if let Some(f) = self.file.write().take() {
            if let Some(ms) = mtime.filter(|m| *m > 0) {
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
                let _ = f.set_modified(t);
            }
            drop(f);
        }
        let dest = unique_path(&self.target);
        fs::rename(&self.temp, &dest)?;
        Ok(dest.display().to_string())
    }

    fn abort(&self, keep: bool) {
        self.file.write().take();
        if !keep {
            let _ = fs::remove_file(&self.temp);
        }
    }

    fn partial_path(&self) -> Option<PathBuf> {
        Some(self.temp.clone())
    }
}

/// Throws data away after verifying it. Benchmarks use it to measure the
/// network path without the disk.
pub struct NullSink;

struct NullOut;

impl OutFile for NullOut {
    fn write_at(&self, _buf: &[u8], _off: u64) -> io::Result<()> {
        Ok(())
    }
    fn finish(&self, _mtime: Option<i64>) -> Result<String> {
        Ok(String::new())
    }
    fn abort(&self, _keep: bool) {}
}

impl Sink for NullSink {
    fn create(&self, _item: &Item, _t: &str, _s: &str) -> Result<Arc<dyn OutFile>> {
        Ok(Arc::new(NullOut))
    }
    fn make_dir(&self, _path: &str) -> Result<()> {
        Ok(())
    }
    fn describe(&self) -> String {
        "(discarded)".into()
    }
}

/// Hands every file to the host app (MediaStore on Android, the app's
/// Documents folder or Photos on iOS).
pub struct PlatformSink {
    pub platform: Arc<dyn Platform>,
}

struct PlatformOut {
    platform: Arc<dyn Platform>,
    file: parking_lot::RwLock<Option<File>>,
    handle: String,
}

impl Sink for PlatformSink {
    fn create(&self, item: &Item, transfer: &str, sender: &str) -> Result<Arc<dyn OutFile>> {
        let req = OutputRequest {
            transfer: transfer.into(),
            path: safe_relative(&item.path).to_string_lossy().replace('\\', "/"),
            size: item.size,
            mime: item.mime.clone(),
            sender: sender.into(),
        };
        let h = self.platform.create_output(&req)?;
        let f = file_from_fd(h.fd)?;
        Ok(Arc::new(PlatformOut { platform: self.platform.clone(), file: parking_lot::RwLock::new(Some(f)), handle: h.handle }))
    }
    fn make_dir(&self, _path: &str) -> Result<()> {
        Ok(())
    }
    fn describe(&self) -> String {
        "device storage".into()
    }
}

impl OutFile for PlatformOut {
    fn write_at(&self, buf: &[u8], off: u64) -> io::Result<()> {
        match self.file.read().as_ref() {
            Some(f) => write_all_at(f, buf, off),
            None => Err(io::Error::other("file already closed")),
        }
    }
    fn finish(&self, mtime: Option<i64>) -> Result<String> {
        if let Some(f) = self.file.write().take() {
            let _ = f.sync_all();
        }
        Ok(self.platform.finish_output(&self.handle, true, mtime)?)
    }
    fn abort(&self, _keep: bool) {
        self.file.write().take();
        let _ = self.platform.finish_output(&self.handle, false, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_paths_stay_inside() {
        assert_eq!(safe_relative("../../etc/passwd"), PathBuf::from("etc").join("passwd"));
        assert_eq!(safe_relative("C:\\Windows\\x.dll"), PathBuf::from("C_").join("Windows").join("x.dll"));
        assert_eq!(safe_relative("/abs/a"), PathBuf::from("abs").join("a"));
        assert_eq!(safe_relative("con.txt"), PathBuf::from("_con.txt"));
        assert_eq!(safe_relative("a/./b. /c"), PathBuf::from("a").join("b").join("c"));
        assert_eq!(safe_relative(""), PathBuf::from("file"));
    }

    #[test]
    fn synthetic_is_position_addressable() {
        let mut a = vec![0u8; 100];
        let mut b = vec![0u8; 37];
        fill_synthetic(9, 0, &mut a);
        fill_synthetic(9, 13, &mut b);
        assert_eq!(&a[13..50], &b[..]);
    }
}
