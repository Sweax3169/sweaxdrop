// Records the UI in its demo mode (the built-in mock core) with headless
// Microsoft Edge over CDP: frames from Page.startScreencast, then ffmpeg.
//
//   node tools/record.mjs <ui-url> <outDir> [scene] [theme] [lang]
//   scenes: desktop | phone | pair | shots
//
// Produces <scene>-<theme>-<lang>.mp4 and .gif (and PNG stills for "shots").

import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const [url, out, scene = 'desktop', theme = 'dark', lang = 'en'] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const EDGE = process.env.EDGE || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const port = 9400 + Math.floor(Math.random() * 400);
const profile = mkdtempSync(join(tmpdir(), 'swx-rec-'));
const phone = scene === 'phone';
const size = phone ? { width: 390, height: 844, deviceScaleFactor: 2, mobile: true } : { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false };

const edge = spawn(EDGE, [
  '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, '--hide-scrollbars', '--mute-audio',
  '--force-color-profile=srgb', '--disable-gpu-vsync', `--window-size=${size.width},${size.height}`, 'about:blank',
], { stdio: 'ignore' });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const page = await connect(port);
await page.send('Page.enable');
await page.send('Emulation.setDeviceMetricsOverride', size);
if (phone) await page.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
await page.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: theme }] });

const q = `mock=1&theme=${theme}&lang=${lang}${phone ? '&mobile=1' : ''}`;
await page.send('Page.navigate', { url: `${url}${url.includes('?') ? '&' : '?'}${q}` });
for (let i = 0; i < 60; i++) {
  try {
    if (await page.eval(`!!(window.__swxApp && window.__swxApp.app.ready && document.querySelector('.shell'))`)) break;
  } catch {}
  await sleep(200);
}
await page.eval(`document.fonts.ready.then(() => true)`);
await sleep(600);

const frames = [];
const frameDir = mkdtempSync(join(tmpdir(), 'swx-frames-'));
page.on('Page.screencastFrame', async (f) => {
  frames.push({ data: f.data, t: f.metadata.timestamp });
  page.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
});

async function center(sel) {
  return page.eval(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
}
async function move(sel, steps = 18) {
  const p = await center(sel);
  if (!p) return null;
  const from = (await page.eval('window.__mouse || {x: 640, y: 700}')) || { x: 640, y: 700 };
  for (let i = 1; i <= steps; i++) {
    const k = 1 - Math.pow(1 - i / steps, 3);
    const x = from.x + (p.x - from.x) * k;
    const y = from.y + (p.y - from.y) * k;
    await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
    await sleep(16);
  }
  await page.eval(`window.__mouse = {x: ${p.x}, y: ${p.y}}`);
  return p;
}
async function click(sel) {
  const p = phone ? await center(sel) : await move(sel);
  if (!p) return;
  if (phone) {
    await page.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: p.x, y: p.y }] });
    await sleep(70);
    await page.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await page.eval(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); e && e.click(); })()`);
  } else {
    await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: p.x, y: p.y, button: 'left', clickCount: 1 });
    await sleep(70);
    await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: p.x, y: p.y, button: 'left', clickCount: 1 });
  }
}
const ev = (js) => page.eval(js);

async function record(fn, name) {
  frames.length = 0;
  await page.send('Page.startScreencast', { format: 'jpeg', quality: 92, everyNthFrame: 1 });
  await fn();
  await page.send('Page.stopScreencast');
  await sleep(200);
  encode(name);
}

function encode(name) {
  // Concat demuxer with real frame durations, resampled to 30 fps.
  const list = [];
  frames.forEach((f, i) => {
    const file = join(frameDir, `f${String(i).padStart(5, '0')}.jpg`);
    writeFileSync(file, Buffer.from(f.data, 'base64'));
    const next = frames[i + 1]?.t ?? f.t + 0.5;
    list.push(`file '${file.replace(/\\/g, '/')}'`, `duration ${Math.max(0.001, next - f.t).toFixed(4)}`);
  });
  list.push(list[list.length - 2]);
  const lst = join(frameDir, 'list.txt');
  writeFileSync(lst, list.join('\n'));
  const mp4 = join(out, `${name}.mp4`);
  const w = phone ? 390 * 2 : 1280;
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', lst, '-vf', `fps=30,scale=${w}:-2:flags=lanczos,format=yuv420p`, '-c:v', 'libx264', '-preset', 'slow', '-crf', '20', '-movflags', '+faststart', mp4]);
  const gw = phone ? 360 : 860;
  const gif = join(out, `${name}.gif`);
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', mp4, '-vf', `fps=15,scale=${gw}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle`, gif]);
  for (const f of readdirSync(frameDir)) rmSync(join(frameDir, f));
  console.log('wrote', mp4, gif, frames.length, 'frames');
}

const tag = `${theme}-${lang}`;
if (scene === 'desktop') {
  await record(async () => {
    await sleep(900);
    await ev(`window.__swxApp.stage([{path:'C:/Users/you/Videos/Holiday cut 4K.mp4', size: 2870000000},{path:'C:/Users/you/Pictures/poster-final.png', size: 8400000},{path:'C:/Users/you/Documents/Contract (signed).pdf', size: 1200000}])`);
    await sleep(1200);
    await move('[data-peer="IP15ZW3HC9QE6LTU"]', 26);
    await sleep(900);
    await click('[data-peer="IP15ZW3HC9QE6LTU"]');
    await sleep(6200);
    await ev(`window.__swxMock.incoming(12)`);
    await sleep(1600);
    await click('.buttons .btn.recv');
    await sleep(4200);
    await ev(`window.__swxMock.text('https://maps.app.goo.gl/x7Qp')`);
    await sleep(2600);
    await click('.tabs button:nth-child(2)');
    await sleep(1400);
    await click('.history li:first-child .line');
    await sleep(2600);
  }, `desktop-${tag}`);
} else if (scene === 'phone') {
  await record(async () => {
    await sleep(900);
    await click('.tray .actions .btn:first-child');
    await sleep(1400);
    await click('[data-peer="P8XQ4N2KD7RA1MVB"]');
    await sleep(5500);
    await ev(`window.__swxMock.incoming(6)`);
    await sleep(1600);
    await click('.buttons .btn.recv');
    await sleep(3200);
    await click('.mobile-nav button:nth-child(2)');
    await sleep(2400);
  }, `phone-${tag}`);
} else if (scene === 'pair') {
  await record(async () => {
    await sleep(600);
    await click('.top .tools .btn.small:nth-of-type(1)');
    await sleep(500);
    await ev(`(() => { const b = [...document.querySelectorAll('.top .tools button')].find(x => x.textContent.match(/Pair|Eşleştir/)); b && b.click(); })()`);
    await sleep(2600);
    await ev(`(() => { const b = document.querySelectorAll('.tabs [role=tab]')[1]; b && b.click(); })()`);
    await sleep(1800);
  }, `pair-${tag}`);
} else if (scene === 'shots') {
  const shot = async (name) => {
    const r = await page.send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(out, `${name}-${tag}.png`), Buffer.from(r.data, 'base64'));
  };
  await ev(`window.__swxMock.bigSend(1); window.__swxMock.send(0, 30)`);
  await sleep(3500);
  await shot('home');
  await ev(`window.__swxMock.incoming(12)`);
  await sleep(1200);
  await shot('incoming');
  await ev(`window.__swxApp.app.incoming = []`);
  await sleep(600);
  await click('.tabs button:nth-child(2)');
  await sleep(900);
  await click('.history li:first-child .line');
  await sleep(1200);
  await shot('history');
  await ev(`window.__swxApp.app.sheet = { type: 'pair', peer: null }`);
  await sleep(1500);
  await shot('pair');
  await ev(`window.__swxApp.app.sheet = { type: 'settings' }`);
  await sleep(1200);
  await shot('settings');
}

page.close();
edge.kill();
setTimeout(() => {
  try {
    rmSync(profile, { recursive: true, force: true });
    rmSync(frameDir, { recursive: true, force: true });
  } catch {}
  process.exit(0);
}, 600);
