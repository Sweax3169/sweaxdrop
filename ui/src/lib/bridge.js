// One interface to the core, whatever hosts the UI:
//   tauri   – Windows desktop app (Rust core in-process)
//   android – Kotlin WebView host (core over JNI)
//   ios     – Swift WKWebView host (core as a static library)
//   web     – a phone browser talking to the PC over HTTP (no install)
//   mock    – the website demo and the media recorder (no core at all)

import { createWebBridge } from './webclient.js';
import { createMockBridge } from './mock.js';

function nativeChannel(post) {
  const pending = new Map();
  const listeners = new Set();
  let seq = 0;
  window.__swx = {
    resolve(id, json) {
      const p = pending.get(id);
      if (!p) return;
      pending.delete(id);
      try {
        p.ok(typeof json === 'string' ? (json ? JSON.parse(json) : null) : json);
      } catch (e) {
        p.err(e);
      }
    },
    reject(id, message) {
      const p = pending.get(id);
      if (!p) return;
      pending.delete(id);
      p.err(new Error(message || 'failed'));
    },
    event(json) {
      let ev;
      try {
        ev = typeof json === 'string' ? JSON.parse(json) : json;
      } catch {
        return;
      }
      for (const fn of listeners) fn(ev);
    },
  };
  const send = (op, args = {}) =>
    new Promise((ok, err) => {
      const id = ++seq;
      pending.set(id, { ok, err });
      post(JSON.stringify({ id, op, args }));
    });
  return {
    send,
    on(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
  };
}

function unwrap(res) {
  const v = typeof res === 'string' ? JSON.parse(res) : res;
  if (v && v.ok === false) throw new Error(v.error || 'failed');
  return v;
}

function createNativeBridge(kind, post) {
  const ch = nativeChannel(post);
  const mobile = kind !== 'tauri';
  return {
    kind,
    caps: {
      folders: true,
      photos: mobile,
      drop: false,
      scan: mobile,
      reveal: false,
      saveDir: kind === 'android',
      share: mobile,
      tray: false,
    },
    async call(cmd, args = {}) {
      const res = await ch.send('call', { req: JSON.stringify({ cmd, ...args }) });
      return unwrap(res);
    },
    on: ch.on,
    pick: (what) => ch.send('pick', { what }),
    pickDir: () => ch.send('pickDir').then((r) => r?.path ?? null),
    clipboard: {
      read: () => ch.send('clipboardRead').then((r) => r?.text ?? ''),
      write: (text) => ch.send('clipboardWrite', { text }),
    },
    open: (path) => ch.send('open', { path }),
    reveal: (path) => ch.send('open', { path }),
    scan: () => ch.send('scan').then((r) => r?.text ?? null),
    openUrl: (url) => ch.send('openUrl', { url }),
  };
}

function createTauriBridge() {
  const T = window.__TAURI__;
  const invoke = T.core.invoke;
  const listeners = new Set();
  T.event.listen('core', (e) => {
    let ev;
    try {
      ev = JSON.parse(e.payload);
    } catch {
      return;
    }
    for (const fn of listeners) fn(ev);
  });
  const dropFns = new Set();
  for (const name of ['tauri://drag-enter', 'tauri://drag-over', 'tauri://drag-drop', 'tauri://drag-leave']) {
    T.event.listen(name, (e) => {
      const kind = name.split('-').pop();
      const p = e.payload || {};
      const scale = window.devicePixelRatio || 1;
      const pos = p.position ? { x: p.position.x / scale, y: p.position.y / scale } : null;
      for (const fn of dropFns) fn({ kind, paths: p.paths || [], pos });
    });
  }
  return {
    kind: 'tauri',
    caps: { folders: true, photos: false, drop: true, scan: false, reveal: true, saveDir: true, share: false, tray: true },
    async call(cmd, args = {}) {
      const res = await invoke('core_call', { req: JSON.stringify({ cmd, ...args }) });
      return unwrap(res);
    },
    on(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    onDrop(fn) {
      dropFns.add(fn);
      return () => dropFns.delete(fn);
    },
    pick: (what) => invoke('pick', { what }),
    pickDir: () => invoke('pick_dir').then((p) => p || null),
    clipboard: {
      read: () => invoke('clipboard_read'),
      write: (text) => invoke('clipboard_write', { text }),
    },
    open: (path) => invoke('open_path', { path }),
    reveal: (path) => invoke('reveal_path', { path }),
    scan: async () => null,
    openUrl: (url) => invoke('open_path', { path: url }),
    window: {
      minimize: () => invoke('win', { op: 'minimize' }),
      maximize: () => invoke('win', { op: 'maximize' }),
      close: () => invoke('win', { op: 'close' }),
      drag: () => invoke('win', { op: 'drag' }),
    },
  };
}

export function detectBridge() {
  const q = new URLSearchParams(location.search);
  if (q.has('mock') || q.has('demo')) return createMockBridge(q);
  if (window.__TAURI__?.core) return createTauriBridge();
  if (window.SweaxNative?.post) return createNativeBridge('android', (m) => window.SweaxNative.post(m));
  if (window.webkit?.messageHandlers?.swx) return createNativeBridge('ios', (m) => window.webkit.messageHandlers.swx.postMessage(m));
  if (location.protocol.startsWith('http') && /^\/api\/w|^\/$|index\.html$/.test(location.pathname) && location.hash.includes('s=')) {
    return createWebBridge();
  }
  if (location.protocol.startsWith('http') && sessionStorage.getItem('swx-web')) return createWebBridge();
  return createMockBridge(q);
}
