// App state: filled from `state`, kept current by core events.

import { detectBridge } from './bridge.js';
import { detectLang, setLang, t } from './i18n.js';
import { bytes, fileName, filesLabel } from './format.js';

export const app = $state({
  ready: false,
  kind: 'mock',
  caps: {},
  device: null,
  settings: {},
  peers: [],
  transfers: {},
  history: [],
  incoming: [],
  texts: [],
  toasts: [],
  staged: [],
  qr: '',
  pin: null,
  pairing: null,
  tab: 'send',
  sheet: null,
  web: null,
  dragging: false,
  dropTarget: null,
  series: {},
  wide: true,
  error: null,
});

let bridge = null;
let toastSeq = 0;

export function getBridge() {
  return bridge;
}

export function toast(message, kind = 'info', ms = 3600) {
  const id = ++toastSeq;
  app.toasts.push({ id, message, kind });
  setTimeout(() => {
    app.toasts = app.toasts.filter((x) => x.id !== id);
  }, ms);
}

export function applyLook(s = app.settings) {
  const root = document.documentElement;
  const dark = s.theme === 'dark' || (s.theme !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  root.dataset.theme = dark ? 'dark' : 'light';
  root.dataset.palette = s.palette || 'tide';
  setLang(detectLang(s.lang));
}

function onEvent(ev) {
  switch (ev.type) {
    case 'ready':
      app.device = ev.device;
      break;
    case 'peers':
      app.peers = ev.peers;
      break;
    case 'transfer': {
      const v = ev.transfer;
      app.transfers[v.id] = { ...(app.transfers[v.id] || {}), ...v };
      if (v.phase === 'running' || v.phase === 'paused') {
        const s = (app.series[v.id] ||= []);
        const last = s[s.length - 1];
        if (!last || v.elapsed_ms - last.t >= 150) {
          s.push({ t: v.elapsed_ms, bps: v.phase === 'paused' ? 0 : v.speed });
          if (s.length > 900) app.series[v.id] = s.filter((_, i) => i % 2 === 0);
        }
      }
      if (['done', 'failed', 'cancelled', 'declined'].includes(v.phase)) {
        app.incoming = app.incoming.filter((i) => i.transfer.id !== v.id);
      }
      break;
    }
    case 'incoming':
      if (!app.incoming.some((i) => i.transfer.id === ev.transfer.id)) app.incoming.push(ev);
      app.transfers[ev.transfer.id] = ev.transfer;
      break;
    case 'web_offer':
      if (!app.incoming.some((i) => i.transfer.id === ev.transfer.id)) app.incoming.push({ ...ev, web: true });
      app.transfers[ev.transfer.id] = ev.transfer;
      break;
    case 'text':
      app.texts.unshift({ id: ev.transfer, from: ev.from, text: ev.text, copied: ev.copied, at: Date.now() });
      app.texts = app.texts.slice(0, 20);
      if (ev.copied) toast(t('Copied'), 'recv');
      break;
    case 'history':
      app.history = [ev.entry, ...app.history.filter((h) => h.id !== ev.entry.id)].slice(0, 300);
      if (ev.entry.phase === 'done' && ev.entry.dir === 'receive' && ev.entry.files > 0) {
        toast(ev.entry.files === 1 ? t('Saved 1 file') : t('Saved {n} files', { n: ev.entry.files }), 'recv');
      }
      if (ev.entry.phase === 'failed' && ev.entry.error) toast(t('Sending stopped: {e}', { e: ev.entry.error }), 'bad', 6000);
      break;
    case 'pairing':
      app.pairing = ev;
      if (ev.state === 'done') {
        app.pin = null;
        toast(t('Paired with {name}', { name: ev.peer?.name || '' }), 'ok');
        if (app.sheet === 'pair') app.sheet = null;
      }
      if (ev.state === 'pin') app.pin = ev.pin;
      if (ev.state === 'failed') toast(t('Wrong code'), 'bad');
      break;
    case 'settings':
      app.settings = ev.settings;
      applyLook();
      break;
    case 'toast':
      toast(ev.message, ev.kind);
      break;
    case 'web':
      app.web = { ...(app.web || {}), ...ev };
      break;
    case 'hello':
      app.web = { ...(app.web || {}), state: 'connected', device: ev.device };
      break;
  }
}

export async function init() {
  bridge = detectBridge();
  app.kind = bridge.kind;
  app.caps = bridge.caps;
  bridge.on(onEvent);
  bridge.onDrop?.(onNativeDrop);
  const mq = matchMedia('(min-width: 860px)');
  app.wide = mq.matches;
  mq.addEventListener('change', (e) => (app.wide = e.matches));
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => applyLook());
  try {
    await refresh();
    app.ready = true;
  } catch (e) {
    app.error = String(e.message || e);
    app.ready = true;
  }
}

export async function refresh() {
  const s = await bridge.call('state');
  app.device = s.device;
  app.settings = s.settings;
  app.peers = s.peers;
  app.history = s.history || [];
  app.qr = s.qr || '';
  for (const tv of s.transfers || []) app.transfers[tv.id] = tv;
  applyLook();
}

export async function call(cmd, args) {
  try {
    return await bridge.call(cmd, args);
  } catch (e) {
    toast(String(e.message || e), 'bad', 5200);
    throw e;
  }
}

export async function setSetting(patch) {
  app.settings = { ...app.settings, ...patch };
  applyLook();
  const r = await call('settings_set', { patch });
  app.settings = r.settings;
  applyLook();
}

// ------------------------------------------------------------- staging

let stageSeq = 0;

export function stage(items) {
  for (const it of items) {
    const key = it.path || it.token || it.name + ':' + it.size + ':' + ++stageSeq;
    if (app.staged.some((s) => s.key === key)) continue;
    app.staged.push({ key, ...it, name: it.name || fileName(it.path) });
  }
}

export function unstage(key) {
  app.staged = app.staged.filter((s) => s.key !== key);
}

export function clearStaged() {
  app.staged = [];
}

export function stagedSummary() {
  const n = app.staged.length;
  const size = app.staged.reduce((a, s) => a + (s.size || 0), 0);
  return size > 0 ? t('{n} items · {size}', { n, size: bytes(size) }) : filesLabel(n);
}

export async function pick(what) {
  const r = await bridge.pick(what);
  if (!r) return;
  if (r.paths?.length) stage(r.paths.map((p) => ({ path: p, folder: what === 'folder' })));
  if (r.sources?.length) stage(r.sources.map((s) => ({ ...s, name: fileName(s.path) })));
  if (r.files?.length) stage(r.files);
}

/** Sends what is staged (or `items`) to a peer; text alone is fine too. */
export async function sendTo(peerId, items = app.staged, text = null) {
  const paths = items.filter((s) => s.path && !s.token && !s.file).map((s) => s.path);
  const sources = items.filter((s) => s.token).map((s) => ({ token: s.token, path: s.rel || s.name, size: s.size, mime: s.mime, mtime: s.mtime }));
  const files = items.filter((s) => s.file);
  if (!paths.length && !sources.length && !files.length && !text) {
    toast(t('Nothing to send yet'), 'info');
    return null;
  }
  const args = { peer: peerId };
  if (paths.length) args.paths = paths;
  if (sources.length) args.sources = sources;
  if (files.length) args.files = files;
  if (text) args.text = text;
  const r = await call('send', args);
  if (items === app.staged) clearStaged();
  return r.id;
}

async function onNativeDrop({ kind, paths, pos }) {
  if (kind === 'enter' || kind === 'over') {
    app.dragging = true;
    app.dropTarget = pos ? targetAt(pos) : null;
    return;
  }
  if (kind === 'leave') {
    app.dragging = false;
    app.dropTarget = null;
    return;
  }
  // drop
  const target = pos ? targetAt(pos) : null;
  app.dragging = false;
  app.dropTarget = null;
  if (!paths.length) return;
  const items = paths.map((p) => ({ path: p }));
  if (target) {
    await sendTo(target, items);
  } else {
    stage(items);
  }
}

function targetAt(pos) {
  const el = document.elementFromPoint(pos.x, pos.y);
  return el?.closest?.('[data-peer]')?.dataset.peer || null;
}

// ------------------------------------------------------------- incoming

export async function answer(offer, accept, opts = {}) {
  app.incoming = app.incoming.filter((i) => i !== offer);
  if (offer.web) {
    await bridge.answerWeb?.(offer, accept);
    return;
  }
  await call('respond', { id: offer.transfer.id, accept, ...opts });
}

export const sortedTransfers = () =>
  Object.values(app.transfers)
    .filter((t) => !['asking'].includes(t.phase))
    .sort((a, b) => b.created_ms - a.created_ms);

export const activeTransfers = () =>
  sortedTransfers().filter((t) => !['done', 'failed', 'cancelled', 'declined'].includes(t.phase) || Date.now() - (t.finished_ms || 0) < 12_000);
