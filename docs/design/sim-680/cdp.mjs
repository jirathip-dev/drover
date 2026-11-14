// #370: a dependency-free headless Chrome driver (--remote-debugging-pipe), like ../pickers/shoot.mjs, and a static
// file server for a folder. `open(url, W, H, dpr)` gives a fresh context: `ev` (awaited expression), `png`, `done`.
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml', '.webmanifest': 'application/json' };
export const sleep = ms => new Promise(r => setTimeout(r, ms));

export async function serve(root) {
  const srv = createServer((q, r) => {
    const p = join(root, decodeURIComponent(new URL(q.url, 'http://x').pathname));
    let body;
    try { body = readFileSync(statSync(p).isDirectory() ? join(p, 'index.html') : p); } catch { return r.writeHead(404).end(); }
    r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(body);
  }).listen(0, '127.0.0.1');
  await new Promise(ok => srv.once('listening', ok));
  return { base: `http://127.0.0.1:${srv.address().port}`, close: () => srv.close() };
}

export function chrome(bin) {
  const c = spawn(bin, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'd370-prof-'))}`, '--no-first-run'],
    { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
  let id = 0, buf = '';
  const wait = new Map(), errors = [];
  c.stdio[4].on('data', d => {
    buf += d;
    for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
      const m = JSON.parse(buf.slice(0, i));
      if (wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); }
      else if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description?.split('\n')[0] || m.params.exceptionDetails.text);
    }
  });
  const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
    wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
    c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
  });
  async function open(url, W, H, dpr = 1, { reduce = false, ready = 'document.readyState === "complete"' } = {}) {
    const { browserContextId } = await send('Target.createBrowserContext');
    const { targetId } = await send('Target.createTarget', { url: 'about:blank', browserContextId });
    const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
    const cmd = (m, p) => send(m, p, s);
    const ev = async expression => {
      const r = await cmd('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (r.exceptionDetails) throw new Error(`${expression.slice(0, 80)}: ${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
      return r.result.value;
    };
    await cmd('Page.enable');
    await cmd('Runtime.enable');
    await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile: W < 720 });
    if (reduce) await cmd('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    await cmd('Page.navigate', { url });
    for (let i = 0; i < 100; i++, await sleep(100)) if (await ev(ready).catch(() => false)) break;
    await ev('document.fonts.ready.then(() => 1)');
    await sleep(400);
    const png = async (clip = {}) => Buffer.from((await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { x: 0, y: 0, width: W, height: H, scale: 1, ...clip } })).data, 'base64');
    // The whole page: the viewport grows to the page's height first, so the fixed tab bar sits at its foot.
    const full = async () => {
      const h = await ev('Math.max(document.documentElement.scrollHeight, document.body.scrollHeight)');
      await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: h, deviceScaleFactor: dpr, mobile: W < 720 });
      await sleep(300);
      return png({ height: await ev('Math.max(document.documentElement.scrollHeight, document.body.scrollHeight)') });
    };
    const done = async () => { await send('Target.closeTarget', { targetId }); await send('Target.disposeBrowserContext', { browserContextId }); };
    return { ev, png, full, done, cmd };
  }
  return { open, errors, kill: () => c.kill() };
}
