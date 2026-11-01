// Minimal Chrome DevTools Protocol client for WebView2 / Chrome pages.
// Used by the media recorder and the desktop smoke test.
//   import { connect } from './cdp.mjs';
//   const page = await connect(9333);
//   await page.eval('document.title'); await page.shot('out.png');

import { writeFileSync } from 'node:fs';

export async function connect(port, match = () => true, timeoutMs = 20000) {
  const until = Date.now() + timeoutMs;
  let target;
  while (!target) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      target = list.find((t) => t.type === 'page' && match(t));
    } catch {}
    if (!target) {
      if (Date.now() > until) throw new Error('no page on port ' + port);
      await new Promise((r) => setTimeout(r, 300));
    }
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, err) => {
    ws.onopen = ok;
    ws.onerror = err;
  });
  let seq = 0;
  const pending = new Map();
  const handlers = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? p.err(new Error(msg.error.message)) : p.ok(msg.result);
    } else if (msg.method && handlers.has(msg.method)) {
      for (const fn of handlers.get(msg.method)) fn(msg.params);
    }
  };
  const send = (method, params = {}) =>
    new Promise((ok, err) => {
      const id = ++seq;
      pending.set(id, { ok, err });
      ws.send(JSON.stringify({ id, method, params }));
    });
  return {
    send,
    on(method, fn) {
      if (!handlers.has(method)) handlers.set(method, new Set());
      handlers.get(method).add(fn);
    },
    async eval(expr) {
      const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
      return r.result.value;
    },
    async shot(file, opts = {}) {
      const r = await send('Page.captureScreenshot', { format: file.endsWith('.jpg') ? 'jpeg' : 'png', ...opts });
      writeFileSync(file, Buffer.from(r.data, 'base64'));
    },
    close: () => ws.close(),
  };
}
