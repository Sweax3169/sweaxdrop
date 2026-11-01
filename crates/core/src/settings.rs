use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::link::Transport;
use crate::tls::CipherPref;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReceiveMode {
    /// Anyone nearby may offer files; you confirm each one.
    #[default]
    Everyone,
    /// Only devices you paired with (QR or PIN) can reach you.
    Trusted,
    /// Not receiving; still visible so you can send.
    Off,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub name: String,
    /// Empty = the platform default (Downloads/SweaxDrop, or the gallery on phones).
    pub save_dir: String,
    /// Phones: a folder the host app manages (Android SAF tree URI); empty
    /// means Downloads/SweaxDrop plus the gallery for photos and videos.
    pub save_uri: String,
    pub save_label: String,
    pub receive: ReceiveMode,
    pub auto_accept_trusted: bool,
    /// Copy text received from trusted devices straight to the clipboard.
    pub copy_text: bool,
    /// Photos and videos go to the gallery (Android/iOS).
    pub media_to_gallery: bool,
    pub open_when_done: bool,
    pub notify: bool,
    pub discoverable: bool,
    /// Serve the no-install browser client (iPhone without the app).
    pub web_client: bool,
    pub streams: u8,
    pub chunk_mib: u32,
    pub transport: Transport,
    pub cipher: CipherPref,
    pub port: u16,
    pub keep_history: bool,
    // UI preferences live here so every platform restores them the same way.
    pub theme: String,
    pub palette: String,
    pub lang: String,
    pub start_with_windows: bool,
    pub close_to_tray: bool,
    /// Phones: keep a quiet notification so files can arrive while closed.
    pub stay_ready: bool,
}

pub const DEFAULT_PORT: u16 = 47470;

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: String::new(),
            save_dir: String::new(),
            save_uri: String::new(),
            save_label: String::new(),
            receive: ReceiveMode::Everyone,
            auto_accept_trusted: true,
            copy_text: false,
            media_to_gallery: true,
            open_when_done: false,
            notify: true,
            discoverable: true,
            web_client: true,
            streams: 4,
            chunk_mib: 4,
            transport: Transport::Tcp,
            cipher: CipherPref::Aes,
            port: DEFAULT_PORT,
            keep_history: true,
            theme: "system".into(),
            palette: "tide".into(),
            lang: String::new(),
            start_with_windows: false,
            close_to_tray: true,
            stay_ready: false,
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        let path = dir.join("settings.json");
        let tmp = path.with_extension("json.tmp");
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, path);
            }
        }
    }

    /// Applies a partial JSON object; unknown keys are ignored.
    pub fn merge(&mut self, patch: &serde_json::Value) {
        let mut cur = serde_json::to_value(&*self).unwrap_or_default();
        if let (Some(c), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                if c.contains_key(k) {
                    c.insert(k.clone(), v.clone());
                }
            }
        }
        if let Ok(s) = serde_json::from_value::<Settings>(cur) {
            *self = s;
        }
        self.streams = self.streams.clamp(1, 16);
        self.chunk_mib = self.chunk_mib.clamp(1, 32);
        self.name = self.name.trim().chars().take(48).collect();
    }

    pub fn chunk_bytes(&self) -> u32 {
        self.chunk_mib.clamp(1, 32) << 20
    }
}

pub fn default_save_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        return PathBuf::from(d).join("Downloads").join("SweaxDrop");
    }
    std::env::temp_dir().join("SweaxDrop")
}
