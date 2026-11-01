// Talks to a running SweaxDrop desktop app through its WebView (started with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>) and
// runs core calls, for measurements with real phones.
//
//   node tools/appctl.mjs <port> '{"cmd":"state"}' ['{"cmd":...}' ...]
//   node tools/appctl.mjs <port> watch <transferId>   (prints progress until it ends)

import { connect } from './cdp.mjs';

const [port, ...args] = process.argv.slice(2);
const page = await connect(Number(port), (t) => !t.url.startsWith('devtools'));
const call = (o) => page.eval(`window.__TAURI__.core.invoke('core_call', { req: ${JSON.stringify(JSON.stringify(o))} }).then(JSON.parse)`);

if (args[0] === 'watch') {
  const id = args[1];
  let last = '';
  for (;;) {
    const t = (await call({ cmd: 'transfer', id })).transfer;
    if (!t) break;
    const line = `${t.phase} ${t.done_bytes ?? t.bytes_done ?? ''}/${t.total_bytes ?? t.bytes ?? ''} ${t.elapsed_ms ?? ''} ms`;
    if (line !== last) console.log(line);
    last = line;
    if (['done', 'failed', 'declined', 'cancelled'].includes(t.phase)) {
      console.log(JSON.stringify(t));
      break;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
} else {
  for (const a of args) console.log(JSON.stringify(await call(JSON.parse(a))));
}
page.close();
process.exit(0);
