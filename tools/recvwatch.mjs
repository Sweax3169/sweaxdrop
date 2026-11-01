// Waits for transfers coming into a running desktop app (see appctl.mjs) and
// prints one JSON line per finished one: size, files, time, speed.
//   node tools/recvwatch.mjs <port> <seconds> [count]

import { connect } from './cdp.mjs';

const [port, secs = '600', count = '99'] = process.argv.slice(2);
const page = await connect(Number(port), (t) => !t.url.startsWith('devtools'));
const call = (o) => page.eval(`window.__TAURI__.core.invoke('core_call', { req: ${JSON.stringify(JSON.stringify(o))} }).then(JSON.parse)`);
const known = new Set(((await call({ cmd: 'state' })).transfers || []).map((t) => t.id));
const end = Date.now() + Number(secs) * 1000;
let n = 0;
while (Date.now() < end && n < Number(count)) {
  for (const t of (await call({ cmd: 'state' })).transfers || []) {
    if (known.has(t.id) || t.dir !== 'receive' || !['done', 'failed', 'cancelled'].includes(t.phase)) continue;
    known.add(t.id);
    n++;
    console.log(JSON.stringify({
      from: t.peer?.name, phase: t.phase, files: t.files, files_done: t.files_done, bytes: t.bytes,
      elapsed_ms: t.elapsed_ms, MBps: +(t.bytes / Math.max(1, t.elapsed_ms) / 1000).toFixed(1),
      peak_MBps: +((t.peak || 0) / 1e6).toFixed(1), lanes: t.lanes?.length, transport: t.transport,
      first: t.items?.[0]?.path, error: t.error,
    }));
  }
  await new Promise((r) => setTimeout(r, 500));
}
page.close();
process.exit(0);
