//! SweaxDrop for Windows: the core runs in this process, the shared web UI
//! runs in WebView2. Tray icon, drag and drop from Explorer, "Send to"
//! (`--send <paths>`, forwarded to the running instance), start with Windows.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use sweaxdrop_core::{api, Config, Node};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

struct Core {
    node: Node,
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SWEAXDROP_DATA") {
        return PathBuf::from(d);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SweaxDrop")
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn show(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Paths passed after `--send` (Explorer's "Send to" appends them).
fn send_args(args: &[String]) -> Vec<String> {
    match args.iter().position(|a| a == "--send") {
        Some(i) => args[i + 1..].iter().filter(|a| !a.starts_with("--")).cloned().collect(),
        None => Vec::new(),
    }
}

fn set_autostart(on: bool) {
    use winreg::enums::*;
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let _ = key.set_value("SweaxDrop", &format!("\"{}\" --background", exe.display()));
            }
        } else {
            let _ = key.delete_value("SweaxDrop");
        }
    }
}

#[tauri::command]
async fn core_call(core: State<'_, Arc<Core>>, req: String) -> Result<String, String> {
    let out = api::call(&core.node, &req).await;
    // Settings that live outside the core.
    if let Ok(v) = serde_json::from_str::<Value>(&req) {
        if v["cmd"] == "settings_set" {
            if let Some(on) = v["patch"]["start_with_windows"].as_bool() {
                set_autostart(on);
            }
        }
    }
    Ok(out)
}

#[tauri::command]
async fn pick(app: AppHandle, what: String) -> Result<Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let d = app.dialog().file();
    if what == "folder" {
        d.pick_folders(move |r| {
            let _ = tx.send(r.unwrap_or_default());
        });
    } else {
        d.pick_files(move |r| {
            let _ = tx.send(r.unwrap_or_default());
        });
    }
    let picked = rx.await.unwrap_or_default();
    let paths: Vec<String> = picked.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.display().to_string()).collect();
    Ok(json!({ "paths": paths }))
}

#[tauri::command]
async fn pick_dir(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |r| {
        let _ = tx.send(r);
    });
    Ok(rx.await.ok().flatten().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

#[tauri::command]
fn clipboard_read() -> String {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default()
}

#[tauri::command]
fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_path(path: String) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

#[tauri::command]
fn reveal_path(path: String) {
    let p = PathBuf::from(&path);
    if p.is_dir() {
        let _ = std::process::Command::new("explorer.exe").arg(&p).spawn();
    } else {
        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", p.display())).spawn();
    }
}

#[tauri::command]
fn win(window: WebviewWindow, core: State<'_, Arc<Core>>, op: String) {
    match op.as_str() {
        "minimize" => {
            let _ = window.minimize();
        }
        "maximize" => {
            if window.is_maximized().unwrap_or(false) {
                let _ = window.unmaximize();
            } else {
                let _ = window.maximize();
            }
        }
        "close" => {
            if core.node.shared.settings().close_to_tray {
                let _ = window.hide();
            } else {
                window.app_handle().exit(0);
            }
        }
        "drag" => {
            let _ = window.start_dragging();
        }
        _ => {}
    }
}

#[tauri::command]
fn take_launch_paths(paths: State<'_, LaunchPaths>) -> Vec<String> {
    std::mem::take(&mut *paths.0.lock().unwrap())
}

struct LaunchPaths(std::sync::Mutex<Vec<String>>);

/// Reacts to core events that need the desktop: notifications, bringing the
/// window up for an incoming offer, copying text, opening the folder.
fn on_core_event(app: &AppHandle, node: &Node, json: &str) {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return };
    let visible = main_window(app).map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false)).unwrap_or(false);
    let s = node.shared.settings();
    match v["type"].as_str() {
        Some("incoming") => {
            show(app);
            if s.notify {
                let name = v["transfer"]["peer"]["name"].as_str().unwrap_or("A device");
                let n = v["transfer"]["files"].as_u64().unwrap_or(0);
                let what = if n == 0 { "a message".to_string() } else if n == 1 { "1 file".into() } else { format!("{n} files") };
                let _ = app.notification().builder().title("SweaxDrop").body(format!("{name} wants to send you {what}")).show();
            }
        }
        Some("text") => {
            let trusted = node.shared.peers.trusted_fp(v["from"]["id"].as_str().unwrap_or("")).is_some();
            if s.copy_text && trusted {
                if let Some(t) = v["text"].as_str() {
                    let _ = clipboard_write(t.to_string());
                }
            }
            if !visible && s.notify {
                let name = v["from"]["name"].as_str().unwrap_or("A device");
                let text: String = v["text"].as_str().unwrap_or("").chars().take(120).collect();
                let _ = app.notification().builder().title(format!("From {name}")).body(text).show();
            }
        }
        Some("history") => {
            let e = &v["entry"];
            if e["dir"] == "receive" && e["phase"] == "done" && e["files"].as_u64().unwrap_or(0) > 0 {
                if !visible && s.notify {
                    let name = e["peer"]["name"].as_str().unwrap_or("a device");
                    let n = e["files"].as_u64().unwrap_or(0);
                    let _ = app
                        .notification()
                        .builder()
                        .title("SweaxDrop")
                        .body(format!("{n} file(s) received from {name}"))
                        .show();
                }
                if s.open_when_done {
                    if let Some(p) = e["saved"][0].as_str().or(e["dest"].as_str()) {
                        reveal_path(p.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let background = args.iter().any(|a| a == "--background");
    let launch = send_args(&args);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let paths = send_args(&argv);
            show(app);
            if !paths.is_empty() {
                let _ = app.emit("send-paths", paths);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(LaunchPaths(std::sync::Mutex::new(launch)))
        .setup(move |app| {
            let handle = app.handle().clone();
            let mut cfg = Config::new(data_dir());
            cfg.platform_name = "windows".into();
            let node = tauri::async_runtime::block_on(Node::start(cfg))?;
            let core = Arc::new(Core { node: node.clone() });
            app.manage(core);
            let h2 = handle.clone();
            let n2 = node.clone();
            node.set_event_sink(Arc::new(move |json: String| {
                on_core_event(&h2, &n2, &json);
                let _ = h2.emit("core", json);
            }));
            set_autostart(node.shared.settings().start_with_windows);

            // Tray
            let open = MenuItem::with_id(app, "open", "Open SweaxDrop", true, None::<&str>)?;
            let send = MenuItem::with_id(app, "send", "Send files…", true, None::<&str>)?;
            let pair = MenuItem::with_id(app, "pair", "Pair a phone…", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &send, &pair, &sep, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("SweaxDrop")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| match ev.id().as_ref() {
                    "open" => show(app),
                    "send" => {
                        show(app);
                        let _ = app.emit("ui-action", "pick-files");
                    }
                    "pair" => {
                        show(app);
                        let _ = app.emit("ui-action", "pair");
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;

            if let Some(w) = main_window(app.handle()) {
                if !background {
                    let _ = w.show();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app.try_state::<Arc<Core>>().map(|c| c.node.shared.settings().close_to_tray).unwrap_or(true);
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![core_call, pick, pick_dir, clipboard_read, clipboard_write, open_path, reveal_path, win, take_launch_paths])
        .run(tauri::generate_context!())
        .expect("SweaxDrop failed to start");
}
