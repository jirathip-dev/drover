// #1091: shoot the Hosts menu-row mockup with the real screens page. Exports the demo (`drover serve --export --demo`)
// to a temp dir, serves fixtures/hosts.json as /screens/hosts, adds preview.js to the shell, shoots each view.
// No deps: Chrome's --remote-debugging-pipe, like ../threads-tab/shoot.mjs.
// Usage (repo root): node docs/design/hosts-menu/shoot.mjs <chrome-headless-shell>
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname, repo = join(here, '../../..');
const [chrome] = process.argv.slice(2);
const site = join(mkdtempSync(join(tmpdir(), 'hosts-site-')), 'site');
execFileSync('drover', ['serve', '--export', site, '--demo'], {
  cwd: repo, stdio: 'inherit',
  env: { ...process.env, DROVER_HOME: 'demo', FLEET_CONFIG: 'demo/fleet.toml', FLEET_STATE: 'demo/events.jsonl' },
});
mkdirSync(join(site, 'screens/hosts'), { recursive: true });
copyFileSync(join(here, 'fixtures/hosts.json'), join(site, 'screens/hosts/index.html'));
copyFileSync(join(here, 'preview.js'), join(site, 'preview.js'));
const shell = readFileSync(join(site, 'index.html'), 'utf8').replace(/<script type=module /, '<script src=preview.js></script>$&');
writeFileSync(join(site, 'index.html'), shell);

const VIEWS = [
  ['?v=settings#queue', 'settings-390.png', 390, 3],
  ['?v=hosts#queue', 'hosts-390.png', 390, 3],
  ['?v=confirm#queue', 'confirm-390.png', 390, 3],
  ['?v=settings#queue', 'settings-1440.png', 1440, 1],
  ['?v=hosts#queue', 'hosts-1440.png', 1440, 1],
  ['?v=confirm#queue', 'confirm-1440.png', 1440, 1],
];
const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml' };
const srv = createServer((q, r) => {
  try {
    let p = join(site, decodeURIComponent(new URL(q.url, 'http://x').pathname));
    if (p.endsWith('/')) p += 'index.html';
    const body = readFileSync(p);
    r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(body);
  } catch { r.writeHead(404).end(); }
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.on('listening', ok));
const base = `http://127.0.0.1:${srv.address().port}/`;

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', '--no-first-run',
  `--user-data-dir=${mkdtempSync(join(tmpdir(), 'hosts-prof-'))}`], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
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
await cmd('Page.enable');
for (const [q, out, w, dpr] of VIEWS) {
  await cmd('Emulation.setDeviceMetricsOverride', { width: w, height: w < 1000 ? 844 : 900, deviceScaleFactor: dpr, mobile: w < 1000 });
  await cmd('Page.navigate', { url: base + 'index.html' + q });
  await sleep(2500);
  const { data } = await cmd('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(here, out), Buffer.from(data, 'base64'));
  console.log(`ok ${out}`);
}
c.kill(); srv.close();
