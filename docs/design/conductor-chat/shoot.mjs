// #355 step 0: shoot the [chat] mockup's views at full height and measure them against docs/ui.md's gate 2: no sideways
// scroll or box past the right edge, text 11/13 px (inputs 16), every bracket control 28 px tall (a choice row: ≤ 4
// lines), every tap row ≥ 44 px, one-line cells ellipsised rather than overflowing. Prints one line per view, exits 1 on
// a failure. No deps: Chrome's --remote-debugging-pipe, like ../schemes/shoot.mjs.
// Usage (repo root): node docs/design/conductor-chat/shoot.mjs <chrome-headless-shell>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname, repo = join(here, '../../..');
const [chrome] = process.argv.slice(2);
const VIEWS = [
  ['', 'chat-list-390.png', 390, 3], ['#new', 'chat-new-390.png', 390, 3], ['#t', 'chat-thread-390.png', 390, 3],
  ['#guest', 'chat-guest-390.png', 390, 2], ['#states', 'chat-states-390.png', 390, 3], ['#t', 'chat-1440.png', 1440, 1],
];
const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.png': 'image/png', '.svg': 'image/svg+xml' };
const srv = createServer((q, r) => {
  try {
    const p = join(repo, decodeURIComponent(new URL(q.url, 'http://x').pathname));
    r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(readFileSync(p));
  } catch { r.writeHead(404).end(); }
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.on('listening', ok));
const base = `http://127.0.0.1:${srv.address().port}/docs/design/conductor-chat/index.html`;

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

// Runs in the page: every rule above, as a list of "what · where" strings.
function audit() {
  const bad = [], W = innerWidth, name = e => e.tagName.toLowerCase() + (e.className && typeof e.className === 'string' ? '.' + e.className.trim().split(/\s+/).join('.') : '') + ` "${(e.textContent || e.placeholder || '').trim().slice(0, 30)}"`;
  const shown = e => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden';
  if (document.documentElement.scrollWidth > W) bad.push(`sideways scroll ${document.documentElement.scrollWidth} > ${W}`);
  for (const e of document.querySelectorAll('body *')) {
    if (!shown(e)) continue;
    const r = e.getBoundingClientRect(), s = getComputedStyle(e);
    if (r.right > W + 0.5 && !e.closest('[hidden]')) bad.push(`past the right edge ${Math.round(r.right)} · ${name(e)}`);
    const own = [...e.childNodes].some(n => n.nodeType === 3 && n.textContent.trim());
    const fs = parseFloat(s.fontSize), input = e.matches('input,textarea,select');
    if ((own || input) && !(input ? fs === 16 : fs === 13 || fs === 11)) bad.push(`font ${fs}px · ${name(e)}`);
    if (e.matches('button') && shown(e)) {
      const choice = e.matches('[name=choice],.pick');
      if (!choice && Math.round(r.height) !== 28) bad.push(`control ${r.height}px tall · ${name(e)}`);
      if (choice && r.height > 4 * 13 * 1.35 + 28) bad.push(`choice over 4 lines · ${name(e)}`);
      if (r.width + 16 < 44) bad.push(`tap ${r.width + 16}px wide · ${name(e)}`);
    }
    if (e.matches('.tr,.fl,.ocd>summary,.blk>summary,.fc>.done>summary') && r.height + (e.matches('.blk>summary') ? 16 : 0) < 44) bad.push(`tap row ${r.height}px · ${name(e)}`);
    if (own && e.scrollWidth > e.clientWidth + 1 && s.overflowX !== 'visible' && s.textOverflow !== 'ellipsis' && !e.matches('pre,textarea,input')) bad.push(`text cut without an ellipsis · ${name(e)}`);
  }
  return bad;
}

const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
const cmd = (m, p) => send(m, p, s);
const js = async x => (await cmd('Runtime.evaluate', { expression: x, returnByValue: true, awaitPromise: true })).result.value;
await cmd('Page.enable');
let fail = 0;
for (const [hash, out, w, dpr] of VIEWS) {
  await cmd('Emulation.setDeviceMetricsOverride', { width: w, height: 844, deviceScaleFactor: dpr, mobile: w < 1000 });
  await cmd('Page.navigate', { url: base + hash });
  await sleep(300);
  await js(`location.hash = ${JSON.stringify(hash)}; Promise.all([...document.images].map(i => i.decode().catch(() => 0)))`);
  await sleep(200);
  await js("document.documentElement.classList.add('shot')");
  const bad = await js(`(${audit})()`), h = await js('document.documentElement.scrollHeight');
  // The full page in one shot: a viewport as tall as the page (the fixed tab bar lands at its foot).
  await cmd('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: dpr, mobile: w < 1000 });
  await sleep(200);
  const { data } = await cmd('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(here, out), Buffer.from(data, 'base64'));
  // PEEK=<dir>: also the page in 844 px slices, for reading the shot at full size.
  for (let y = 0; process.env.PEEK && y < h; y += 844) {
    const { data } = await cmd('Page.captureScreenshot', { format: 'png', clip: { x: 0, y, width: w, height: Math.min(844, h - y), scale: 1 } });
    writeFileSync(join(process.env.PEEK, `${out.replace('.png', '')}-${y / 844}.png`), Buffer.from(data, 'base64'));
  }
  console.log(`${bad.length ? 'FAIL' : 'ok  '} ${out} ${w}×${h}${bad.length ? '\n  ' + bad.join('\n  ') : ''}`);
  fail += bad.length;
}
c.kill(); srv.close();
process.exit(fail ? 1 : 0);
