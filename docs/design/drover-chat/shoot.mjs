// #1352: shoot every terminal grid of index.html to <screen>-<cols>x<rows>.png (DPR 2), and fail on any layout
// problem screens.js reports (a row past the grid, a screen with no room for its body). No deps: Chrome's
// --remote-debugging-pipe, like ../threads-tab/shoot.mjs.
// Usage (repo root): node docs/design/drover-chat/shoot.mjs <chrome-headless-shell>
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const here = new URL('.', import.meta.url).pathname;
const [chrome] = process.argv.slice(2);
const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', '--no-first-run',
  `--user-data-dir=${mkdtempSync(join(tmpdir(), 'chat-prof-'))}`], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let buf = '', id = 0;
const wait = new Map();
c.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (m.id && wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); }
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
  wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
  c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const sleep = ms => new Promise(ok => setTimeout(ok, ms));
const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
const cmd = (m, p) => send(m, p, s);
const js = async x => (await cmd('Runtime.evaluate', { expression: x, returnByValue: true, awaitPromise: true })).result.value;
await cmd('Page.enable');
await cmd('Emulation.setDeviceMetricsOverride', { width: 1480, height: 1200, deviceScaleFactor: 2, mobile: false });
await cmd('Page.navigate', { url: 'file://' + join(here, 'index.html') });
await sleep(1000);
const problems = await js('window.problems || []');
if (problems.length) { console.error(problems.join('\n')); c.kill(); process.exit(1); }
const h = await js('document.documentElement.scrollHeight');
await cmd('Emulation.setDeviceMetricsOverride', { width: 1480, height: h, deviceScaleFactor: 2, mobile: false });
await sleep(300);
const shots = await js(`[...document.querySelectorAll('[data-shot]')].map(e => { const r = e.getBoundingClientRect();
  return { name: e.dataset.shot, x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height,
    over: e.scrollWidth > e.clientWidth + 1 }; })`);
for (const t of shots) {
  if (t.over) { console.error(`${t.name}: content wider than the grid`); process.exitCode = 1; }
  const { data } = await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { x: t.x, y: t.y, width: t.width, height: t.height, scale: 1 } });
  writeFileSync(join(here, t.name + '.png'), Buffer.from(data, 'base64'));
  console.log(`ok ${t.name}.png ${t.width}×${Math.round(t.height)}`);
}
c.kill();
