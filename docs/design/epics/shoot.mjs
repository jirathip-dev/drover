// #1786: export the demo page, patch epic.js's blocks into its Queue and acme-api thread JSON, serve it on 127.0.0.1
// and shoot the real board.js at 390 (DPR 2) and 1440, plus term.html's terminal grids. No deps: Chrome's
// --remote-debugging-pipe, like ../terminal-board/shoot.mjs.
// Usage (repo root): node docs/design/epics/shoot.mjs <chrome-headless-shell>
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, extname, normalize } from 'node:path';
import vm from 'node:vm';

const here = new URL('.', import.meta.url).pathname;
const root = join(here, '../../..');
const [chrome] = process.argv.slice(2);
const NOW = '2026-10-31T06:54:30Z';
const ctx = {};
vm.runInNewContext(readFileSync(join(here, 'epic.js'), 'utf8'), { globalThis: ctx });
const { EPIC } = ctx;

const site = mkdtempSync(join(tmpdir(), 'epics-site-'));
execFileSync('drover', ['serve', '--export', site, '--demo'], { cwd: root, stdio: 'inherit',
  env: { ...process.env, DROVER_HOME: 'demo', FLEET_CONFIG: 'demo/fleet.toml', FLEET_STATE: 'demo/events.jsonl' } });
const patch = (p, f) => { const file = join(site, p, 'index.html'); writeFileSync(file, JSON.stringify(f(JSON.parse(readFileSync(file, 'utf8'))))); };
patch('screens/queue', q => EPIC.queue(q, NOW, { fold: true }));
patch('screens/threads/t2', t => EPIC.thread(t, NOW));

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.webmanifest': 'application/json' };
const srv = createServer((req, res) => {
  const u = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const [base, rel] = u.startsWith('/d/') ? [join(root, 'docs/design'), u.slice(3)] : [site, u.slice(1)];
  let f = normalize(join(base, rel));
  if (!f.startsWith(base)) { res.writeHead(403); return res.end(); }
  if (existsSync(f) && statSync(f).isDirectory()) f = join(f, 'index.html');
  if (!existsSync(f)) { res.writeHead(404); return res.end(); }
  res.writeHead(200, { 'content-type': TYPES[extname(f)] || 'text/html' });
  res.end(readFileSync(f));
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.on('listening', ok));
const origin = `http://127.0.0.1:${srv.address().port}`;

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', '--no-first-run',
  `--user-data-dir=${mkdtempSync(join(tmpdir(), 'epics-prof-'))}`], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
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

const PUSH = `(() => { const p = ${JSON.stringify(EPIC.PUSH)}, d = document.createElement('div');
  d.style.cssText = 'position:fixed;z-index:9;top:8px;left:8px;right:8px;max-width:420px;margin-left:auto;padding:10px 12px;border-radius:14px;background:#2b2b2ef2;color:#f2f2f7;font:13px/1.35 -apple-system,system-ui,sans-serif;box-shadow:0 4px 20px #0008';
  d.innerHTML = '<div style="display:flex;gap:6px;color:#aeaeb2;font-size:11px"><b style="color:#f2f2f7">DROVER</b><span style="margin-left:auto">now</span></div><b>' + p.title + '</b><div>' + p.body + '</div>';
  document.body.append(d); })()`;

async function page(name, hash, width, { push, clipTo } = {}) {
  await cmd('Emulation.setDeviceMetricsOverride', { width, height: width < 1000 ? 844 : 900, deviceScaleFactor: width < 1000 ? 2 : 1, mobile: width < 1000 });
  await cmd('Page.navigate', { url: `${origin}/?theme=farm#${hash}` });
  await sleep(1500);
  if (push) await js(PUSH);
  const wide = await js('document.documentElement.scrollWidth > innerWidth + 1');
  if (wide) { console.error(`${name}: sideways scroll`); process.exitCode = 1; }
  if (clipTo) await js(`(() => { const e = [...document.querySelectorAll('.sh')].find(x => x.textContent.startsWith(${JSON.stringify(clipTo)}));
    const c = e && e.closest('.card'); if (c) scrollTo(0, c.getBoundingClientRect().top + scrollY - 96); })()`);
  await sleep(300);
  const [y, h] = await js('[scrollY, innerHeight]');
  const { data } = await cmd('Page.captureScreenshot', { format: 'png', clip: { x: 0, y, width, height: h, scale: 1 } });
  writeFileSync(join(here, name + '.png'), Buffer.from(data, 'base64'));
  console.log(`ok ${name}.png ${width}×${h} at ${y}`);
}

await page('queue-390', 'queue', 390, { clipTo: 'epics (' });
await page('queue-1440', 'queue', 1440, { clipTo: 'epics (' });
await page('landed-390', 'threads/t2', 390, { push: true });
await page('landed-1440', 'threads/t2', 1440, { push: true });

await cmd('Emulation.setDeviceMetricsOverride', { width: 1480, height: 1400, deviceScaleFactor: 2, mobile: false });
await cmd('Page.navigate', { url: `${origin}/d/epics/term.html` });
await sleep(800);
const problems = await js('window.problems || []');
if (problems.length) { console.error(problems.join('\n')); process.exitCode = 1; }
const shots = await js(`[...document.querySelectorAll('[data-shot]')].map(e => { const r = e.getBoundingClientRect();
  return { name: e.dataset.shot, x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height }; })`);
for (const t of shots) {
  const { data } = await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { ...t, scale: 1 } });
  writeFileSync(join(here, t.name + '.png'), Buffer.from(data, 'base64'));
  console.log(`ok ${t.name}.png`);
}
c.kill();
srv.close();
