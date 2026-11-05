// #496 design: shoots index.html's four views, full page, at 1440 (DPR 1) and 390 (DPR 3), and measures each 390
// view by assets/ui-audit.js measure()'s rules (that audit needs the demo board, so they are restated here): a control
// is 28 px high with a 44 px tap area, text is 11/13 px (16 in a field), nothing runs past its box or the screen.
// No deps: Chrome's --remote-debugging-pipe, like ../pickers/shoot.mjs.
// Usage: node shoot.mjs <chrome-headless-shell> [out dir = this folder]
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname, repo = join(here, '../../..');
const [chrome, out = here] = process.argv.slice(2);
const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.png': 'image/png' };
const srv = createServer((q, r) => {
  const p = join(repo, decodeURIComponent(new URL(q.url, 'http://x').pathname));
  try { r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(readFileSync(p)); } catch { r.writeHead(404).end(); }
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.once('listening', ok));
const base = `http://127.0.0.1:${srv.address().port}/docs/design/agent-providers/index.html`;

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'd496-'))}`,
  '--no-first-run'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let id = 0, buf = '';
const wait = new Map(), errors = [];
c.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); }
    else if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description?.split('\n')[0]);
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
  wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
  c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
setTimeout(() => { console.error('shoot: timed out'); c.kill(); process.exit(1); }, 120000).unref();

const AUDIT = `(() => {
  const bad = [], seen = e => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden';
  const label = e => (e.textContent || e.getAttribute('aria-label') || e.tagName).trim().slice(0, 24);
  for (const e of [...document.querySelectorAll('button, .pk')].filter(seen)) {
    const cs = getComputedStyle(e);
    if (!e.matches('.pick') && cs.height !== '28px') bad.push(label(e) + ': control height ' + cs.height);
    e.scrollIntoView({ block: 'center', behavior: 'instant' });
    const r = e.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    for (const [x, y] of [[cx - 16, cy], [cx + 16, cy], [cx, cy - 16], [cx, cy + 16]]) {
      if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
      const h = document.elementFromPoint(x, y);
      if (!h || !e.contains(h)) { bad.push(label(e) + ': tap area under 44 px at ' + (x | 0) + ',' + (y | 0) + ' hits ' + (h ? h.outerHTML.slice(0, 40) : 'nothing')); break; }
    }
  }
  const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t; (t = w.nextNode());) {
    const e = t.parentElement;
    if (!t.data.trim() || !seen(e) || e.closest('script, style, .sxi')) continue;
    const fs = getComputedStyle(e).fontSize, input = e.closest('input, textarea, select');
    if (input ? fs !== '16px' : !['11px', '13px'].includes(fs)) bad.push('"' + t.data.trim().slice(0, 20) + '": font ' + fs);
    let b = e;
    while (['inline', 'contents'].includes(getComputedStyle(b).display)) b = b.parentElement;
    const g = document.createRange(); g.selectNodeContents(t);
    const tr = g.getBoundingClientRect(), br = b.getBoundingClientRect();
    if ((tr.right > br.right + 1 || tr.right > innerWidth) && getComputedStyle(b).textOverflow !== 'ellipsis' && !/auto|scroll/.test(getComputedStyle(b).overflowX)) bad.push('"' + t.data.trim().slice(0, 20) + '": text past its box by ' + ((tr.right - br.right) | 0) + 'px');
  }
  return bad;
})()`;
const report = {};
for (const [view, file] of [['', 'providers'], ['signin', 'signin'], ['use', 'use'], ['forms', 'forms']]) {
  for (const [W, dpr] of [[1440, 1], [390, 3]]) {
    const { browserContextId } = await send('Target.createBrowserContext');
    const { targetId } = await send('Target.createTarget', { url: 'about:blank', browserContextId });
    const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
    const cmd = (m, p) => send(m, p, s);
    const ev = async expression => (await cmd('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result.value;
    await cmd('Runtime.enable');
    await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: 900, deviceScaleFactor: dpr, mobile: W < 720 });
    await cmd('Page.navigate', { url: `${base}#${view}` });
    await sleep(800);
    const H = await ev('Math.ceil(document.documentElement.scrollHeight)');
    await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile: W < 720 });
    await sleep(200);
    if (W === 390) report[file || 'providers'] = await ev(AUDIT);
    const { data } = await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { x: 0, y: 0, width: W, height: H, scale: 1 } });
    writeFileSync(join(out, `${file}-${W}.png`), Buffer.from(data, 'base64'));
    await send('Target.closeTarget', { targetId });
    await send('Target.disposeBrowserContext', { browserContextId });
  }
}
console.log(JSON.stringify({ errors, audit: report }, null, 1));
c.kill();
srv.close();
