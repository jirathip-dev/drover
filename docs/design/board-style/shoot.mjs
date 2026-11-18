// Screenshots a page in chrome-headless-shell over the DevTools protocol, with localStorage set before load.
// Usage: node shoot.mjs <chrome> <url> <out.png> <width> <json localStorage> [js to run before the shot]
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chrome, url, out, width, ls = '{}', js = ''] = process.argv.slice(2);
const w = +width, dpr = w > 500 ? 1 : 2, h = w > 500 ? 900 : 844;
const port = 9300 + Math.floor(Math.random() * 500);
const prof = mkdtempSync(join(tmpdir(), 'shoot-'));
const c = spawn(chrome, ['--headless', '--no-sandbox', '--disable-gpu', '--hide-scrollbars', `--user-data-dir=${prof}`,
  `--remote-debugging-port=${port}`, `--window-size=${w},${h}`, 'about:blank'], { stdio: 'ignore' });
const wait = ms => new Promise(r => setTimeout(r, ms));
let t;
for (let i = 0; i < 50; i++) {
  try { t = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(x => x.type === 'page'); if (t) break; } catch {}
  await wait(100);
}
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise(r => ws.onopen = r);
let id = 0;
const pend = new Map();
ws.onmessage = e => { const m = JSON.parse(e.data); if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); } };
const send = (method, params = {}) => new Promise(r => { pend.set(++id, r); ws.send(JSON.stringify({ id, method, params })); });
await send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: dpr, mobile: w < 500 });
await send('Page.enable');
const origin = new URL(url).origin;
await send('Page.navigate', { url: origin + '/favicon.ico' });
await wait(300);
await send('Runtime.evaluate', { expression: `localStorage.clear();Object.entries(${ls}).forEach(([k,v])=>localStorage.setItem(k,v))` });
await send('Page.navigate', { url });
await wait(2500);
if (js) { await send('Runtime.evaluate', { expression: js, awaitPromise: true }); await wait(900); }
const m = await send('Page.getLayoutMetrics');
const ch = Math.min(Math.ceil(m.result.cssContentSize.height), 7000);
const s = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { x: 0, y: 0, width: w, height: ch, scale: 1 } });
writeFileSync(out, Buffer.from(s.result.data, 'base64'));
ws.close(); c.kill();
