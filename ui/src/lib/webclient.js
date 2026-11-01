// The browser client: a phone with no app talks to the PC that served this
// page. Uploads go out as parallel PUTs of 4 MiB chunks; the PC's event
// stream brings offers, progress and messages.

const SESSION_KEY = 'swx-web';

function phoneName() {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'iPad';
  if (/Android/.test(ua)) {
    const m = ua.match(/Android [\d.]+; ([^;)]+?)(?: Build|\))/);
    return m && m[1] && !/^K$/.test(m[1]) ? m[1].trim() : 'Android phone';
  }
  return 'Browser';
}

function platformName() {
  const ua = navigator.userAgent;
  if (/iPhone|iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'ios';
  if (/Android/.test(ua)) return 'android';
  return 'web';
}

export function createWebBridge() {
  const listeners = new Set();
  const emit = (ev) => {
    for (const fn of listeners) fn(ev);
  };
  const hash = new URLSearchParams(location.hash.slice(1));
  const secret = hash.get('s') || '';
  if (location.hash) history.replaceState(null, '', location.pathname);
  let saved = {};
  try {
    saved = JSON.parse(sessionStorage.getItem(SESSION_KEY) || localStorage.getItem(SESSION_KEY) || '{}');
  } catch {}
  let session = saved.session || '';
  let pc = null;
  let pcPeerId = 'pc';
  let es = null;
  const uploads = new Map();
  const offers = new Map();
  const local = {
    theme: localStorage.getItem('swx-theme') || 'system',
    palette: localStorage.getItem('swx-palette') || 'tide',
    lang: localStorage.getItem('swx-lang') || '',
  };
  let history = [];
  try {
    history = JSON.parse(localStorage.getItem('swx-history') || '[]');
  } catch {}

  const me = { id: 'THISPHONE', name: phoneName(), platform: platformName(), model: 'browser', version: '' };

  async function hello() {
    const r = await fetch('/api/w/hello', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-swx-key': secret },
      body: JSON.stringify({ name: phoneName(), platform: platformName(), session }),
    });
    const v = await r.json();
    if (!v.ok) throw new Error(v.error || 'could not connect');
    session = v.session;
    pc = v.device;
    pcPeerId = pc.id;
    const keep = JSON.stringify({ session });
    sessionStorage.setItem(SESSION_KEY, keep);
    localStorage.setItem(SESSION_KEY, keep);
    openEvents();
    return v;
  }

  function openEvents() {
    es?.close();
    es = new EventSource(`/api/w/events?s=${encodeURIComponent(session)}`);
    es.onmessage = (m) => {
      let ev;
      try {
        ev = JSON.parse(m.data);
      } catch {
        return;
      }
      if (ev.type === 'transfer') {
        const v = ev.transfer;
        if (uploads.has(v.id)) return;
        // The PC reports its own point of view; flip it for this phone.
        ev = { type: 'transfer', transfer: { ...v, dir: v.dir === 'send' ? 'receive' : 'send', peer: pc } };
      }
      if (ev.type === 'web_offer') {
        offers.set(ev.transfer.id, ev);
        ev = { ...ev, transfer: { ...ev.transfer, dir: 'receive', peer: pc, phase: 'asking' } };
      }
      emit(ev);
    };
    es.onerror = () => emit({ type: 'web', state: 'reconnecting' });
    es.onopen = () => emit({ type: 'web', state: 'connected' });
  }

  function pcPeer() {
    return {
      id: pcPeerId, name: pc?.name || 'Computer', platform: pc?.platform || 'windows', model: '', kind: 'app',
      trusted: true, online: true, via: 'lan', addrs: [location.host], seen_ms: Date.now(),
    };
  }

  function saveHistory(entry) {
    history = [entry, ...history].slice(0, 100);
    try {
      localStorage.setItem('swx-history', JSON.stringify(history));
    } catch {}
    emit({ type: 'history', entry });
  }

  async function upload(files) {
    const items = files.map((f, i) => ({ id: i, path: f.rel || f.name, size: f.file.size, mtime: f.file.lastModified || null, mime: f.file.type || null }));
    const r = await fetch('/api/w/offer', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-swx-session': session },
      body: JSON.stringify({ items }),
    }).then((x) => x.json());
    if (!r.ok) throw new Error(r.error);
    const id = r.transfer;
    const chunk = r.chunk;
    const total = items.reduce((a, i) => a + i.size, 0);
    const t = {
      id, dir: 'send', peer: pc, phase: 'running', total, bytes: 0, files: items.length, files_done: 0, speed: 0, average: 0, peak: 0,
      eta_ms: null, elapsed_ms: 0, created_ms: Date.now(), finished_ms: 0, lanes: [0, 0, 0, 0], transport: 'tcp', dest: '', text: null,
      names: items.slice(0, 6).map((i) => i.path), saved: [], attempts: 1, paused: false, aborts: new Set(),
    };
    uploads.set(id, t);
    const jobs = [];
    items.forEach((it, i) => {
      const n = Math.max(1, Math.ceil(it.size / chunk));
      for (let c = 0; c < n; c++) jobs.push({ i, c, start: c * chunk, end: Math.min(it.size, (c + 1) * chunk) });
    });
    const doneChunks = new Array(items.length).fill(0);
    const started = performance.now();
    let lastBytes = 0;
    let lastT = started;
    const samples = [];
    const view = () => {
      const { aborts, paused, ...v } = t;
      return { ...v, lanes: [...t.lanes] };
    };
    const ticker = setInterval(() => {
      const now = performance.now();
      const bps = ((t.bytes - lastBytes) / (now - lastT)) * 1000;
      lastBytes = t.bytes;
      lastT = now;
      t.speed = t.speed * 0.5 + bps * 0.5;
      t.peak = Math.max(t.peak, t.speed);
      t.elapsed_ms = now - started;
      t.average = t.bytes / (t.elapsed_ms / 1000);
      t.eta_ms = t.speed > 1 ? ((total - t.bytes) / t.speed) * 1000 : null;
      samples.push({ t: Math.round(t.elapsed_ms), bps: t.speed });
      emit({ type: 'transfer', transfer: view() });
    }, 250);
    let next = 0;
    const lane = async (k) => {
      while (next < jobs.length) {
        if (t.phase === 'cancelled') return;
        while (t.paused) await new Promise((r) => setTimeout(r, 120));
        const j = jobs[next++];
        const body = files[j.i].file.slice(j.start, j.end);
        for (let attempt = 0; ; attempt++) {
          const ac = new AbortController();
          t.aborts.add(ac);
          try {
            const res = await fetch(`/api/w/up/${id}/${j.i}/${j.c}`, { method: 'PUT', headers: { 'x-swx-session': session }, body, signal: ac.signal });
            if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || res.statusText);
            break;
          } catch (e) {
            if (t.phase === 'cancelled') return;
            if (attempt >= 4) throw e;
            await new Promise((r) => setTimeout(r, 400 * (attempt + 1)));
          } finally {
            t.aborts.delete(ac);
          }
        }
        t.bytes += j.end - j.start;
        t.lanes[k] += j.end - j.start;
        doneChunks[j.i]++;
        if (doneChunks[j.i] === Math.max(1, Math.ceil(items[j.i].size / chunk))) t.files_done++;
      }
    };
    (async () => {
      try {
        await Promise.all([0, 1, 2, 3].map(lane));
        if (t.phase !== 'cancelled') t.phase = 'done';
      } catch (e) {
        t.phase = 'failed';
        t.error = String(e.message || e);
      }
      clearInterval(ticker);
      t.speed = 0;
      t.finished_ms = Date.now();
      emit({ type: 'transfer', transfer: view() });
      saveHistory({ ...view(), samples, lanes: 4 });
    })();
    return id;
  }

  function download(url, name) {
    const a = document.createElement('a');
    a.href = `${url}${url.includes('?') ? '&' : '?'}s=${encodeURIComponent(session)}`;
    a.download = name || '';
    a.rel = 'noopener';
    document.body.appendChild(a);
    a.click();
    a.remove();
  }

  function pickFiles(accept) {
    return new Promise((resolve) => {
      const input = document.createElement('input');
      input.type = 'file';
      input.multiple = true;
      if (accept) input.accept = accept;
      input.style.display = 'none';
      input.onchange = () => {
        const files = [...(input.files || [])].map((f) => ({ file: f, name: f.name, size: f.size, mime: f.type, rel: f.webkitRelativePath || f.name }));
        input.remove();
        resolve({ files });
      };
      document.body.appendChild(input);
      input.click();
    });
  }

  return {
    kind: 'web',
    caps: { folders: !/iPhone|iPad/.test(navigator.userAgent), photos: true, drop: false, scan: false, reveal: false, saveDir: false, share: false, tray: false },
    session: () => session,
    dl: download,
    offer: (id) => offers.get(id),
    async call(cmd, args = {}) {
      switch (cmd) {
        case 'state': {
          if (!pc) await hello();
          return {
            ok: true,
            device: { ...me, pretty_id: '', addresses: [], port: 0 },
            settings: { ...local, name: me.name, receive: 'everyone', streams: 4 },
            peers: [pcPeer()],
            transfers: [],
            history,
            qr: '',
          };
        }
        case 'settings_set': {
          for (const k of ['theme', 'palette', 'lang']) {
            if (args.patch?.[k] !== undefined) {
              local[k] = args.patch[k];
              localStorage.setItem('swx-' + k, local[k]);
            }
          }
          return { ok: true, settings: { ...local, name: me.name, receive: 'everyone', streams: 4 } };
        }
        case 'send': {
          if (args.text && !args.files?.length) {
            const r = await fetch('/api/w/text', {
              method: 'POST',
              headers: { 'content-type': 'application/json', 'x-swx-session': session },
              body: JSON.stringify({ text: args.text }),
            }).then((x) => x.json());
            if (!r.ok) throw new Error(r.error);
            return { ok: true, id: 'text' };
          }
          return { ok: true, id: await upload(args.files || []) };
        }
        case 'pause': {
          const t = uploads.get(args.id);
          if (t) {
            t.paused = !!args.paused;
            t.phase = t.paused ? 'paused' : 'running';
          }
          return { ok: true };
        }
        case 'cancel': {
          const t = uploads.get(args.id);
          if (t) {
            t.phase = 'cancelled';
            for (const a of t.aborts) a.abort();
          }
          await fetch(`/api/w/cancel/${args.id}`, { method: 'POST', headers: { 'x-swx-session': session } });
          return { ok: true };
        }
        case 'history':
          return { ok: true, history };
        case 'history_clear':
          history = [];
          localStorage.removeItem('swx-history');
          return { ok: true };
        default:
          return { ok: true };
      }
    },
    on(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    async answerWeb(offer, accept) {
      await fetch(`/api/w/answer/${offer.transfer.id}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-swx-session': session },
        body: JSON.stringify({ accept }),
      });
    },
    pick: (what) => pickFiles(what === 'photos' ? 'image/*,video/*' : undefined),
    pickDir: async () => null,
    clipboard: {
      read: async () => {
        try {
          return await navigator.clipboard.readText();
        } catch {
          return '';
        }
      },
      write: async (text) => {
        try {
          await navigator.clipboard.writeText(text);
        } catch {
          const ta = document.createElement('textarea');
          ta.value = text;
          document.body.appendChild(ta);
          ta.select();
          document.execCommand('copy');
          ta.remove();
        }
      },
    },
    open: async () => {},
    reveal: async () => {},
    scan: async () => null,
    openUrl: async (u) => (location.href = u),
  };
}
