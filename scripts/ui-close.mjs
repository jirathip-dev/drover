// #166 gate 2: a headless CDP screencast of opening the chat sheet and swipe-closing it with a finger; fails if any
// frame is more than 60 % near-black pixels (the page under the sheet went unpainted), or if the page under it moves:
// its scroll or <body> position changing on any frame (the old scroll lock, which iOS repainted black; Chrome never
// shows that black, so this is what fails the old close code here). No deps: Chrome's
// --remote-debugging-pipe (fd 3 in, fd 4 out) and a small PNG decoder over node:zlib.
// Usage: node scripts/ui-close.mjs <page url> <chrome binary> [shot dir]; the worst frame lands in <shot dir>/close.png.
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';

const [url, chrome, shots] = process.argv.slice(2);
const prof = mkdtempSync(join(tmpdir(), 'ui-close-'));
const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${prof}`,
  '--no-first-run', '--window-size=390,844'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let id = 0, buf = '';
const wait = new Map(), on = [];
c.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); } else on.forEach(f => f(m));
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
  wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
  c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
const fail = msg => { console.error(`ui-close: ${msg}`); c.kill(); process.exit(1); };
setTimeout(() => fail('timed out after 60 s'), 60000).unref();

// RGB(A) 8-bit PNG → share of pixels with every channel ≤ 8 (the scrim over the page is #04060a, the page #0d1117).
function black(png) {
  let p = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (p < png.length) {
    const n = png.readUInt32BE(p), t = png.toString('ascii', p + 4, p + 8), d = png.subarray(p + 8, p + 8 + n);
    if (t === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); ct = d[9]; }
    if (t === 'IDAT') idat.push(d);
    p += 12 + n;
  }
  const bpp = ct === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(idat)), row = w * bpp, px = Buffer.alloc(row * h);
  let dark = 0;
  for (let y = 0; y < h; y++) {
    const f = raw[y * (row + 1)], src = raw.subarray(y * (row + 1) + 1, (y + 1) * (row + 1)), o = y * row;
    for (let x = 0; x < row; x++) {
      const a = x >= bpp ? px[o + x - bpp] : 0, b = y ? px[o - row + x] : 0, cc = x >= bpp && y ? px[o - row + x - bpp] : 0;
      const pa = Math.abs(b - cc), pb = Math.abs(a - cc), pc = Math.abs(a + b - 2 * cc);
      px[o + x] = src[x] + [0, a, b, (a + b) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? b : cc][f];
    }
    for (let x = 0; x < row; x += bpp) if (px[o + x] <= 8 && px[o + x + 1] <= 8 && px[o + x + 2] <= 8) dark++;
  }
  return dark / (w * h);
}

const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
const cmd = (m, p) => send(m, p, s);
await cmd('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
await cmd('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
await cmd('Page.enable');
const loaded = new Promise(r => on.push(m => m.method === 'Page.loadEventFired' && r()));
await cmd('Page.navigate', { url });
await loaded;
const js = async e => (await cmd('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true })).result.value;
// Every frame from here to the end: the page's scroll and <body> position.
await js("tab('lanes'); scrollTo(0, 120); window.seen = new Set(); (function f() { seen.add(scrollY + ' ' + getComputedStyle(document.body).position); requestAnimationFrame(f); })(); 1");
const frames = [];
on.push(m => {
  if (m.method !== 'Page.screencastFrame') return;
  frames.push(Buffer.from(m.params.data, 'base64'));
  cmd('Page.screencastFrameAck', { sessionId: m.params.sessionId });
});
await cmd('Page.startScreencast', { format: 'png', everyNthFrame: 1 });
await sleep(300);
await js("open(document.querySelector('.lane')); 1");
await sleep(600);
// A finger on the handle, 420 px down over ~0.3 s (a flick), then let go.
const r = await js("(r => [r.left + r.width / 2, r.top + r.height / 2])(document.querySelector('.hd').getBoundingClientRect())");
const touch = (type, y) => cmd('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchEnd' ? [] : [{ x: r[0], y }] });
await touch('touchStart', r[1]);
for (let i = 1; i <= 14; i++) { await touch('touchMove', r[1] + 30 * i); await sleep(20); }
await touch('touchEnd');
await sleep(800);
await cmd('Page.stopScreencast');
const [hidden, seen] = await js("[document.querySelector('#sheet').hidden, [...seen]]");
c.kill();
if (frames.length < 5) fail(`${frames.length} screencast frames, too few to judge`);
const share = frames.map(black), worst = share.indexOf(Math.max(...share));
if (shots) writeFileSync(join(shots, 'close.png'), frames[worst]);
console.log(`ui-close: ${frames.length} frames, the darkest ${(share[worst] * 100).toFixed(1)} % near-black (frame ${worst}); sheet hidden ${hidden}; page (scroll, body position) ${seen.join(' | ')}`);
const bad = [];
if (share[worst] > 0.6) bad.push(`frame ${worst} is ${(share[worst] * 100).toFixed(1)} % near-black (over 60 %)`);
if (!hidden) bad.push('the sheet is still shown after the swipe');
if (seen.length !== 1) bad.push(`the page under the sheet moved: ${seen.join(' | ')}`);
if (bad.length) fail(bad.join('; '));
process.exit(0);
