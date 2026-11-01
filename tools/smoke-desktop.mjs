// Drives the real Windows app through its WebView: starts it hidden with a
// throwaway data folder (loopback only), starts a headless peer, sends a
// folder and a message to it, and saves a screenshot of the window contents.
//   node tools/smoke-desktop.mjs <sweaxdrop.exe> <sweaxdrop-bench.exe> <outDir> [en|tr]

import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const [exe, bench, out = 'smoke-out', lang] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const root = mkdtempSync(join(tmpdir(), 'swx-smoke-'));
const peerInbox = join(root, 'peer-inbox');
const src = join(root, 'src', 'Trip photos');
mkdirSync(src, { recursive: true });
for (let i = 0; i < 40; i++) writeFileSync(join(src, `IMG_${1000 + i}.jpg`), Buffer.alloc(250_000 + i * 997, i));

const env = { ...process.env, SWEAXDROP_LOOPBACK: '1' };
const peer = spawn(bench, ['serve', '--port', '47911', '--save', peerInbox], { env, stdio: ['ignore', 'pipe', 'pipe'] });
peer.stdout.on('data', (d) => process.stdout.write('[peer] ' + d));

const app = spawn(exe, ['--background'], {
  env: { ...env, SWEAXDROP_DATA: join(root, 'app-data'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9333' },
  stdio: 'ignore',
});

let failed = false;
try {
  const page = await connect(9333, (t) => !t.url.startsWith('devtools'));
  const call = (o) => page.eval(`window.__TAURI__.core.invoke('core_call', { req: ${JSON.stringify(JSON.stringify(o))} }).then(JSON.parse)`);
  for (let i = 0; i < 60; i++) {
    try {
      if (await page.eval(`!!(window.__TAURI__ && document.querySelector('.shell'))`)) break;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  const ready = async () => {
    for (let i = 0; i < 60; i++) {
      try {
        if (await page.eval(`!!(window.__TAURI__ && document.querySelector('.shell'))`)) return;
      } catch {}
      await new Promise((r) => setTimeout(r, 250));
    }
  };
  if (lang) {
    await call({ cmd: 'settings_set', patch: { lang, name: 'Studio PC' } });
    await page.eval('location.reload()').catch(() => {});
    await new Promise((r) => setTimeout(r, 500));
    await ready();
  }
  const st = await call({ cmd: 'state' });
  console.log('app device', st.device.name, st.device.pretty_id, 'port', st.device.port);
  const added = await call({ cmd: 'add_address', address: '127.0.0.1:47911' });
  if (!added.ok) throw new Error('add_address: ' + added.error);
  const peerId = added.peer.id;
  await call({ cmd: 'trust', peer: peerId });
  const sent = await call({ cmd: 'send', peer: peerId, paths: [src] });
  if (!sent.ok) throw new Error('send: ' + sent.error);
  let phase = '';
  for (let i = 0; i < 100 && !['done', 'failed', 'declined', 'cancelled'].includes(phase); i++) {
    await page.eval('new Promise(r => setTimeout(r, 200))');
    phase = (await call({ cmd: 'transfer', id: sent.id })).transfer?.phase;
  }
  console.log('folder transfer:', phase);
  if (phase !== 'done') failed = true;
  const text = await call({ cmd: 'send', peer: peerId, text: 'hello from the desktop app' });
  console.log('text send ok:', text.ok);
  const files = readdirSync(join(peerInbox, 'Trip photos'));
  console.log('peer received', files.length, 'files');
  if (files.length !== 40) failed = true;
  // UI renders the same state
  await page.eval('new Promise(r => setTimeout(r, 800))');
  const ui = await page.eval(`({ tiles: document.querySelectorAll('[data-peer]').length, cards: document.querySelectorAll('article.card').length, title: document.title })`);
  console.log('ui', JSON.stringify(ui));
  await page.send('Emulation.setDeviceMetricsOverride', { width: 1080, height: 720, deviceScaleFactor: 1, mobile: false });
  await page.shot(join(out, lang ? `desktop-smoke-${lang}.png` : 'desktop-smoke.png'));
  page.close();
} catch (e) {
  console.error('SMOKE FAILED', e);
  failed = true;
} finally {
  app.kill();
  peer.kill();
  setTimeout(() => {
    try {
      rmSync(root, { recursive: true, force: true });
    } catch {}
    process.exit(failed ? 1 : 0);
  }, 800);
}
