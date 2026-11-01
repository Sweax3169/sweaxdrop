// A pretend core for the website demo and the media recorder. It speaks the
// same commands and events as the real one; numbers follow the shapes we
// measured (ramp-up, Wi-Fi jitter, one lane slightly behind the others).

const now = () => Date.now();
const rid = () => Math.random().toString(36).slice(2, 12);

const DEVICE = {
  id: 'K7Q29MXAL4TBN8ZE',
  pretty_id: 'K7Q2-9MXA-L4TB-N8ZE',
  name: 'Desk PC',
  platform: 'windows',
  model: '',
  version: '1.0.0',
  port: 47470,
  addresses: [{ ip: '192.168.1.24', iface: 'Wi-Fi', via: 'lan' }],
  fingerprint: '6f1c…',
};

function peers() {
  const t = now();
  return [
    { id: 'P8XQ4N2KD7RA1MVB', name: 'Pixel 8', platform: 'android', model: 'Pixel 8', kind: 'app', trusted: true, online: true, via: 'lan', addrs: ['192.168.1.31:47470'], seen_ms: t },
    { id: 'IP15ZW3HC9QE6LTU', name: 'iPhone 15', platform: 'ios', model: 'iPhone', kind: 'app', trusted: false, online: true, via: 'lan', addrs: ['192.168.1.40:47470'], seen_ms: t },
    { id: 'LTP7GX2MV8KQ3RWA', name: 'Studio laptop', platform: 'windows', model: '', kind: 'app', trusted: true, online: true, via: 'lan', addrs: ['192.168.1.12:47470'], seen_ms: t },
    { id: 'TAB9S2HJ6PL4NWQE', name: 'Galaxy Tab S9', platform: 'android', model: 'SM-X710', kind: 'app', trusted: false, online: false, via: null, addrs: [], seen_ms: t - 3_600_000 },
  ];
}

const PHOTOS = Array.from({ length: 48 }, (_, i) => ({ path: `Camera/PXL_2026103${(i % 9) + 1}_${String(140211 + i * 37)}.jpg`, size: 2_400_000 + ((i * 7919) % 2_900_000) }));

function sampleHistory() {
  const t = now();
  const mk = (id, dir, peer, files, total, avg, agoMin, names) => {
    const secs = total / avg;
    const samples = Array.from({ length: 60 }, (_, i) => {
      const x = i / 59;
      const ramp = Math.min(1, x * 9);
      return { t: Math.round(x * secs * 1000), bps: avg * ramp * (0.92 + 0.16 * Math.sin(i * 1.7) * Math.cos(i * 0.6)) };
    });
    return {
      id, dir, peer: { id: peer.id, name: peer.name, platform: peer.platform, model: '', version: '1.0.0' }, phase: 'done', error: null,
      files, total, bytes: total, created_ms: t - agoMin * 60000, finished_ms: t - agoMin * 60000 + secs * 1000,
      elapsed_ms: secs * 1000, average: avg, peak: avg * 1.12, transport: 'tcp', names, dest: 'C:\\Users\\you\\Downloads\\SweaxDrop', saved: [], text: null, samples, lanes: 4,
    };
  };
  const [p8, ip, lt] = peers();
  return [
    mk('h1', 'receive', p8, 48, 186_000_000, 96_400_000, 18, ['PXL_20261031_140211.jpg', 'PXL_20261031_140248.jpg']),
    mk('h2', 'send', ip, 1, 2_870_000_000, 88_200_000, 64, ['Holiday cut 4K.mp4']),
    mk('h3', 'send', lt, 3_412, 912_000_000, 71_500_000, 190, ['project/']),
    mk('h4', 'receive', ip, 6, 41_000_000, 54_000_000, 1500, ['IMG_4471.HEIC']),
  ];
}

export function createMockBridge(q = new URLSearchParams()) {
  const listeners = new Set();
  const emit = (ev) => {
    for (const fn of listeners) fn(structuredClone(ev));
  };
  const settings = {
    name: DEVICE.name, save_dir: '', receive: 'everyone', auto_accept_trusted: true, copy_text: false, media_to_gallery: true,
    open_when_done: false, notify: true, discoverable: true, web_client: true, streams: 4, chunk_mib: 4, transport: 'tcp', cipher: 'aes',
    port: 47470, keep_history: true, theme: q.get('theme') || 'system', palette: q.get('palette') || 'tide', lang: q.get('lang') || '',
    start_with_windows: true, close_to_tray: true,
  };
  const transfers = new Map();
  let history = sampleHistory();
  const ps = peers();

  function view(t) {
    const elapsed = t.started ? now() - t.started - t.pausedFor : 0;
    const left = t.total - t.bytes;
    return {
      id: t.id, dir: t.dir, peer: t.peer, phase: t.phase, error: t.error || null, total: t.total, bytes: Math.round(t.bytes),
      files: t.items.length, files_done: t.filesDone, speed: t.speed, average: elapsed > 0 ? t.bytes / (elapsed / 1000) : 0, peak: t.peak,
      eta_ms: t.speed > 1 && left > 0 ? (left / t.speed) * 1000 : null, elapsed_ms: elapsed, created_ms: t.created, finished_ms: t.finished || 0,
      lanes: t.lanes.map((l) => Math.round(l)), transport: 'tcp', dest: t.dir === 'receive' ? 'Downloads\\SweaxDrop' : '', text: t.text || null,
      names: t.items.slice(0, 6).map((i) => i.path), saved: [], attempts: 1, items: t.items,
    };
  }

  function run(t, target = 96e6) {
    t.phase = 'running';
    t.started = now();
    t.pausedFor = 0;
    const laneShare = [1.02, 1, 0.97, 0.91].slice(0, t.lanes.length);
    let last = now();
    let pausedAt = 0;
    const tick = () => {
      const n = now();
      const dt = (n - last) / 1000;
      last = n;
      if (t.phase === 'cancelled') return;
      if (t.phase === 'paused') {
        if (!pausedAt) pausedAt = n;
        t.speed = 0;
        emit({ type: 'transfer', transfer: view(t) });
        setTimeout(tick, 200);
        return;
      }
      if (pausedAt) {
        t.pausedFor += n - pausedAt;
        pausedAt = 0;
      }
      const age = (n - t.started - t.pausedFor) / 1000;
      const ramp = 1 - Math.exp(-age * 3.2);
      const jitter = 0.93 + 0.1 * Math.sin(age * 5.3) * Math.cos(age * 1.9) + 0.03 * Math.sin(age * 17);
      t.speed = target * ramp * jitter;
      t.peak = Math.max(t.peak, t.speed);
      const step = Math.min(t.total - t.bytes, t.speed * dt);
      t.bytes += step;
      const sum = laneShare.reduce((a, b) => a + b, 0);
      t.lanes = t.lanes.map((l, i) => l + (step * laneShare[i]) / sum);
      let acc = 0;
      t.filesDone = 0;
      for (const it of t.items) {
        acc += it.size;
        if (acc <= t.bytes + 1) t.filesDone++;
      }
      if (t.bytes >= t.total - 1) {
        t.bytes = t.total;
        t.phase = 'done';
        t.speed = 0;
        t.finished = now();
        emit({ type: 'transfer', transfer: view(t) });
        const v = view(t);
        const entry = { ...v, samples: t.samples, lanes: t.lanes.length };
        history = [entry, ...history];
        emit({ type: 'history', entry });
        return;
      }
      t.samples.push({ t: Math.round((n - t.started) / 1), bps: t.speed });
      emit({ type: 'transfer', transfer: view(t) });
      setTimeout(tick, 200);
    };
    tick();
  }

  function startSend(peer, items, text) {
    const t = {
      id: rid(), dir: 'send', peer: { id: peer.id, name: peer.name, platform: peer.platform, model: '', version: '1.0.0' }, phase: 'connecting',
      items, total: items.reduce((a, i) => a + i.size, 0), bytes: 0, speed: 0, peak: 0, filesDone: 0, lanes: [0, 0, 0, 0],
      created: now(), samples: [], text,
    };
    transfers.set(t.id, t);
    emit({ type: 'transfer', transfer: view(t) });
    setTimeout(() => {
      t.phase = peer.trusted ? 'running' : 'waiting';
      emit({ type: 'transfer', transfer: view(t) });
      setTimeout(() => {
        if (t.phase === 'cancelled') return;
        if (!items.length) {
          t.phase = 'done';
          t.finished = now();
          emit({ type: 'transfer', transfer: view(t) });
          return;
        }
        run(t, peer.platform === 'ios' ? 88e6 : 97e6);
      }, peer.trusted ? 120 : 1400);
    }, 260);
    return t.id;
  }

  function offerFrom(peer, items, text) {
    const t = {
      id: rid(), dir: 'receive', peer: { id: peer.id, name: peer.name, platform: peer.platform, model: '', version: '1.0.0' }, phase: 'asking',
      items, total: items.reduce((a, i) => a + i.size, 0), bytes: 0, speed: 0, peak: 0, filesDone: 0, lanes: [0, 0, 0, 0],
      created: now(), samples: [], text,
    };
    transfers.set(t.id, t);
    emit({ type: 'incoming', transfer: view(t), safety: '482 915', trusted: peer.trusted });
    return t.id;
  }

  // Scripted moments for the website and the recorder.
  window.__swxMock = {
    incoming: (n = 12) => offerFrom(ps[1], PHOTOS.slice(0, n), null),
    text: (s = 'https://maps.app.goo.gl/x7Qp — meet here at 7') => emit({ type: 'text', from: ps[0], text: s, transfer: rid(), copied: false }),
    send: (peerIndex = 0, n = 24) => startSend(ps[peerIndex], PHOTOS.slice(0, n), null),
    bigSend: (peerIndex = 1) => startSend(ps[peerIndex], [{ path: 'Holiday cut 4K.mp4', size: 2_870_000_000 }], null),
    pin: () => emit({ type: 'pairing', state: 'pin', pin: '204 817' }),
  };

  if (q.get('demo') === 'live') {
    setTimeout(() => window.__swxMock.send(0, 36), 1200);
    setTimeout(() => window.__swxMock.incoming(12), 5200);
  }

  return {
    kind: 'mock',
    caps: { folders: true, photos: q.get('mobile') === '1', drop: false, scan: q.get('mobile') === '1', reveal: true, saveDir: true, share: false, tray: false },
    async call(cmd, args = {}) {
      await new Promise((r) => setTimeout(r, 20));
      switch (cmd) {
        case 'state':
          return {
            ok: true, device: { ...DEVICE, name: settings.name }, settings: { ...settings }, peers: ps, transfers: [...transfers.values()].map(view), history,
            trusted: [], qr: 'http://192.168.1.24:47470/#s=demo&i=K7Q29MXAL4TBN8ZE&k=6f1c&a=192.168.1.24&n=Desk%20PC',
          };
        case 'settings_set':
          Object.assign(settings, args.patch || {});
          return { ok: true, settings: { ...settings } };
        case 'send': {
          const peer = ps.find((p) => p.id === args.peer);
          const items = [
            ...(args.paths || []).map((p, i) => ({ path: p.split(/[\\/]/).pop(), size: 1_800_000 + i * 913_000 })),
            ...(args.sources || []).map((s) => ({ path: s.path, size: s.size || 2_000_000 })),
            ...(args.files || []).map((f) => ({ path: f.name, size: f.size || 2_000_000 })),
          ];
          return { ok: true, id: startSend(peer, items, args.text || null) };
        }
        case 'respond': {
          const t = transfers.get(args.id);
          if (t && args.accept) run(t, 92e6);
          else if (t) {
            t.phase = 'declined';
            emit({ type: 'transfer', transfer: view(t) });
          }
          return { ok: true, answered: true };
        }
        case 'pause': {
          const t = transfers.get(args.id);
          if (t) t.phase = args.paused ? 'paused' : 'running';
          if (t) emit({ type: 'transfer', transfer: view(t) });
          return { ok: true, done: true };
        }
        case 'cancel': {
          const t = transfers.get(args.id);
          if (t) {
            t.phase = 'cancelled';
            t.finished = now();
            emit({ type: 'transfer', transfer: view(t) });
          }
          return { ok: true, done: true };
        }
        case 'history':
          return { ok: true, history };
        case 'history_clear':
          history = [];
          return { ok: true };
        case 'history_remove':
          history = history.filter((h) => h.id !== args.id);
          return { ok: true };
        case 'pin_open':
          setTimeout(() => emit({ type: 'pairing', state: 'pin', pin: '204817' }), 10);
          return { ok: true, pin: '204817' };
        case 'pin_close':
          return { ok: true };
        case 'pair_pin': {
          const p = ps.find((x) => x.id === args.peer);
          if (String(args.pin).replace(/\D/g, '').length !== 6) throw new Error('the code has six digits');
          p.trusted = true;
          emit({ type: 'peers', peers: ps });
          emit({ type: 'pairing', state: 'done', peer: p });
          return { ok: true, peer: p };
        }
        case 'trust':
        case 'untrust': {
          const p = ps.find((x) => x.id === args.peer);
          if (p) p.trusted = cmd === 'trust';
          emit({ type: 'peers', peers: ps });
          return { ok: true };
        }
        case 'forget':
          return { ok: true };
        case 'qr':
        case 'qr_rotate':
          return { ok: true, uri: 'http://192.168.1.24:47470/#s=demo' };
        case 'add_address':
          return { ok: true, peer: ps[2] };
        case 'web_info':
          return { ok: true, url: 'http://192.168.1.24:47470/', clients: [], enabled: true };
        default:
          return { ok: true };
      }
    },
    on(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    pick: async (what) => {
      if (what === 'folder') return { paths: ['D:\\Projects\\launch-video'] };
      if (what === 'photos') return { sources: PHOTOS.slice(0, 18).map((p, i) => ({ token: 'ph' + i, path: p.path, size: p.size, mime: 'image/jpeg' })) };
      return { paths: ['C:\\Users\\you\\Videos\\Holiday cut 4K.mp4', 'C:\\Users\\you\\Documents\\Contract — signed.pdf', 'C:\\Users\\you\\Pictures\\poster-final.png'] };
    },
    pickDir: async () => 'D:\\Inbox',
    clipboard: { read: async () => 'https://github.com/Sweax3169/sweaxdrop', write: async () => {} },
    open: async () => {},
    reveal: async () => {},
    scan: async () => null,
    openUrl: async (u) => window.open(u, '_blank'),
  };
}
