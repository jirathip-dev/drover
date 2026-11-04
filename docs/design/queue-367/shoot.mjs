// #367 gate 2, shot from the real board: the fixture (fixture.py) exported with `serve --export --demo`, copied twice:
// `today` as is, `next` with this folder's queue.js spliced into serve.js (qprep() before graph(), the rest after
// tidy()) and queue.css appended to serve.css. Both get each row's seconds in its phase as `data-a` (phase 2's one
// server change: the export has only the coarse `18m`). Then the shots at 390 and 1440 and what gate 2 asks.
// No deps: Chrome's --remote-debugging-pipe, as ../pickers/shoot.mjs.
// Usage: node shoot.mjs <exported site> <fixture dir> <chrome-headless-shell> [out dir = this folder]
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname;
const [site, fx, chrome, out = here] = process.argv.slice(2);
const read = f => readFileSync(f, 'utf8');

// Each row's phase start: the last event that set its state (spawn, verdict, needs-human, an ask).
const evs = read(join(fx, 'events.jsonl')).split('\n').filter(Boolean).map(l => JSON.parse(l));
const now = Math.max(...evs.map(e => e.ts)) + 60, since = new Map();
for (const e of evs) if (e.issue && ['spawn', 'verdict', 'needs-human', 'needs-decision', 'merged'].includes(e.type)) since.set(`${e.fleet}/${e.issue}`, e.ts);
const ages = html => html.replace(/<details class="q"([^>]*)><summary><span><a href="https:\/\/github\.com\/[\w-]+\/([\w-]+)\/issues\/(\d+)"/g,
  (m, a, f, n) => since.has(`${f}/${n}`) ? m.replace('<details class="q"', `<details class="q" data-a=${now - since.get(`${f}/${n}`)}`) : m);

const root = mkdtempSync(join(tmpdir(), 'queue367-'));
for (const v of ['today', 'next']) cpSync(site, join(root, v), { recursive: true });
const walk = d => readdirSync(d).flatMap(f => statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]);
const must = (t, re, to, what) => { if (!re.test(t)) throw new Error(`${what} changed: ${re} no longer matches`); return t.replace(re, to); };
for (const v of ['today', 'next']) for (const f of walk(join(root, v))) {
  if (f.endsWith('.html')) writeFileSync(f, ages(read(f)));
  if (v === 'next' && f.endsWith('/serve.css')) writeFileSync(f, read(f) + '\n' + read(join(here, 'queue.css')));
  if (v === 'next' && f.endsWith('/serve.js')) writeFileSync(f, must(read(f), /function tidy\(root\) \{\n  graph\(root\);/, 'function tidy(root) {\n  qprep(root);\n  graph(root);', 'serve.js') + '\n' + read(join(here, 'queue.js')));
}

const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml' };
const srv = createServer((q, r) => {
  const p = join(root, decodeURIComponent(new URL(q.url, 'http://x').pathname));
  let body;
  try { body = readFileSync(statSync(p).isDirectory() ? join(p, 'index.html') : p); } catch { return r.writeHead(404).end(); }
  r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(body);
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.once('listening', ok));
const base = `http://127.0.0.1:${srv.address().port}`;

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'queue367-prof-'))}`,
  '--no-first-run'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
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
const sleep = ms => new Promise(r => setTimeout(r, ms));
setTimeout(() => { console.error('shoot: timed out'); c.kill(); process.exit(1); }, 300000).unref();

async function page(url, W, H, dpr) {
  const { browserContextId } = await send('Target.createBrowserContext');
  const { targetId } = await send('Target.createTarget', { url: 'about:blank', browserContextId });
  const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cmd = (m, p) => send(m, p, s);
  const ev = async expression => {
    const r = await cmd('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(`${expression.slice(0, 60)}: ${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
    return r.result.value;
  };
  await cmd('Page.enable');
  await cmd('Runtime.enable');
  const mobile = W < 720;
  await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile });
  if (mobile) await cmd('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await cmd('Page.navigate', { url: base + '/' + url });
  for (let i = 0; i < 100; i++, await sleep(100)) if (await ev(`document.readyState === 'complete' && typeof TT !== 'undefined' && !!TT`)) break;
  await ev('document.fonts.ready.then(() => 1)');
  await sleep(500);
  const png = async clip => Buffer.from((await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: !!clip, ...(clip ? { clip: { x: 0, y: 0, width: W, height: H, scale: 1, ...clip } } : {}) })).data, 'base64');
  // A tall shot grows the viewport (a beyond-viewport capture draws the fixed tab bar mid-page); `h` defaults to the page.
  const tall = async h => {
    h = h || await ev('document.documentElement.scrollHeight');
    await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: h, deviceScaleFactor: dpr, mobile });
    await sleep(400);
    const b = await png();
    await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile });
    await sleep(200);
    return b;
  };
  const full = () => tall();
  const done = async () => { await send('Target.closeTarget', { targetId }); await send('Target.disposeBrowserContext', { browserContextId }); };
  const center = async sel => ev(`(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
  const tap = async sel => {
    const [x, y] = await center(sel);
    if (mobile) for (const type of ['touchStart', 'touchEnd']) await cmd('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchEnd' ? [] : [{ x, y }] });
    else for (const type of ['mousePressed', 'mouseReleased']) await cmd('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 });
    await sleep(400);
  };
  const swipe = async (dx, y = 500) => {
    const x0 = dx < 0 ? 330 : 60;
    await cmd('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: x0, y }] });
    for (let k = 1; k <= 6; k++) await cmd('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x0 + dx * k / 6, y: y + k }] });
    await cmd('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await sleep(400);
  };
  const key = async k => { for (const type of ['keyDown', 'keyUp']) await cmd('Input.dispatchKeyEvent', { type, key: k, code: k, windowsVirtualKeyCode: { ArrowLeft: 37, ArrowRight: 39 }[k] }); await sleep(300); };
  const drag = async dx => {
    await cmd('Input.dispatchMouseEvent', { type: 'mousePressed', x: 700, y: 600, button: 'left', clickCount: 1 });
    for (let k = 1; k <= 6; k++) await cmd('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 700 + dx * k / 6, y: 600, button: 'left', buttons: 1 });
    await cmd('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 700 + dx, y: 600, button: 'left', clickCount: 1 });
    await sleep(400);
  };
  return { ev, png, full, tall, done, tap, swipe, key, drag };
}
// Framing only: the demo banner goes, so the board fills the shot (the needs-you stack stays: it is part of the problem).
const FRAME = `document.querySelector('.msg.a')?.remove(); scrollTo(0, 0);`;
const PICK = f => `localStorage.setItem('queue:repo', ${JSON.stringify(f)})`;
// What gate 2 asks, on the page as it stands.
const MEASURE = `(() => {
  const nav = document.querySelector('nav').getBoundingClientRect(), seen = e => e.checkVisibility({ visibilityProperty: true });
  const fc = [...document.querySelectorAll('#queue>.fc')].find(seen);
  const rows = fc ? [...fc.querySelectorAll('details.q')].filter(seen) : [];
  const prog = fc ? [...fc.querySelectorAll('.qg[data-g="1"]>details')].filter(seen) : [];
  const below = prog.filter(r => r.querySelector('summary').getBoundingClientRect().bottom + scrollY > innerHeight - nav.height);
  const wide = [...document.querySelectorAll('#queue *')].filter(seen).filter(e => e.getBoundingClientRect().right > innerWidth + 0.5).map(e => e.className || e.tagName);
  const tl = rows.map(r => r.querySelector(':scope>summary>.tl')).filter(Boolean).map(t => Math.round(t.getBoundingClientRect().left));
  return { repo: fc?.dataset.f, chip: document.querySelector('.qcp[aria-selected=true]')?.dataset.f, rows: rows.length, pageHeight: document.documentElement.scrollHeight,
    inProgress: prog.length, inProgressBelowFold: below.map(r => r.dataset.n || r.dataset.chain),
    sideways: document.documentElement.scrollWidth > innerWidth, pastRightEdge: [...new Set(wide)].slice(0, 5),
    titleColumns: [...new Set(tl)], more: fc?.querySelector('.qmore>summary .qmo')?.textContent || null };
})()`;
// Contrast of the title line (and the --mute it replaces) over the page background, per scheme.
const CONTRAST = `(() => {
  const rgb = s => { const c = document.createElement('canvas').getContext('2d'); c.fillStyle = s; c.fillRect(0, 0, 1, 1); return [...c.getImageData(0, 0, 1, 1).data].slice(0, 3); };
  const lum = c => { const [r, g, b] = c.map(v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return Math.round((x + 0.05) / (y + 0.05) * 100) / 100; };
  const bg = rgb(getComputedStyle(document.body).backgroundColor), t = document.querySelector('#queue .qg[data-g="2"] details.q:not(.qch)>summary>.tl') || document.querySelector('#queue details.q>summary>.tl');
  return { scheme: document.documentElement.dataset.scheme, title: ratio(rgb(getComputedStyle(t).color), bg), mute: ratio(rgb(getComputedStyle(document.body).getPropertyValue('--mute').trim()), bg) };
})()`;
const report = { fixture: {}, today: {}, next: {}, switch: {}, contrast: {}, audit: {} };
const save = (name, b) => writeFileSync(join(out, name), b);

// Today, for comparison: the page height and where the first running row is.
{
  const p = await page('today/index.html#queue', 390, 844, 2);
  await p.ev(FRAME);
  report.today['390'] = await p.ev(`(() => { const r = [...document.querySelectorAll('#queue details.q')].find(d => /^(impl|review|fix|ready)/.test(d.dataset.st || d.querySelector('summary>span:nth-of-type(2)')?.textContent.trim().replace(/^\\S+\\s/, '') || ''));
    return { pageHeight: document.documentElement.scrollHeight, rows: document.querySelectorAll('#queue details.q').length, firstRunningRowTop: r ? Math.round(r.getBoundingClientRect().top) : null, viewport: innerHeight }; })()`);
  save('today-390.png', await p.png());
  await p.done();
}
// 390: herdsman, first screen (running rows above the fold), the whole repo collapsed, the chain open, `+ N more` open.
{
  const p = await page(`next/index.html#queue`, 390, 844, 2);
  await p.ev(`${PICK('herdsman')}; location.reload()`).catch(() => {});
  await sleep(1500);
  await p.ev(FRAME);
  report.next['390 first screen'] = await p.ev(MEASURE);
  save('queue-390.png', await p.png());
  save('queue-full-390.png', await p.full());
  await p.tap('details.qch[data-chain="265"]>summary');
  report.next['390 chain open'] = { ...await p.ev(MEASURE), chainOpen: await p.ev(`document.querySelector('details.qch[data-chain="265"]').open`),
    summary: await p.ev(`document.querySelector('details.qch[data-chain="265"]>summary').getAttribute('aria-label')`) };
  await p.ev(`document.querySelector('details.qch[data-chain="344"]').open = true`);
  save('chain-open-390.png', await p.tall(await p.ev(`Math.ceil(document.querySelector('details.qch[data-chain="344"]').getBoundingClientRect().bottom + scrollY + 120)`)));
  await p.ev(`document.querySelectorAll('details.qch').forEach(d => d.open = false)`);
  const y0 = await p.ev(`Math.round(document.querySelector('.qmore').getBoundingClientRect().top + scrollY)`);
  await p.ev(`document.querySelector('.qmore>summary').scrollIntoView({ block: 'center' })`);
  await p.tap('.qmore>summary');
  report.next['390 +N more open'] = { ...await p.ev(MEASURE), open: await p.ev(`document.querySelector('.qmore').open`),
    inPlace: await p.ev(`Math.round(document.querySelector('.qmore').getBoundingClientRect().top + scrollY) === ${y0}`) };
  save('more-open-390.png', await p.full());
  // The bottom: the last row (backlog, done today) above the tab bar.
  await p.ev(`scrollTo(0, document.documentElement.scrollHeight)`);
  await sleep(300);
  report.next['390 bottom'] = await p.ev(`(() => { const nav = document.querySelector('nav').getBoundingClientRect(), fc = [...document.querySelectorAll('#queue>.fc')].find(f => !f.hidden);
    const last = [...fc.children].filter(e => e.checkVisibility()).pop().getBoundingClientRect(); return { lastBottom: Math.round(last.bottom), navTop: Math.round(nav.top), clear: Math.round(nav.top - last.bottom) }; })()`);
  save('bottom-390.png', await p.png());
  await p.done();
}
// Today's bottom, the same way.
{
  const p = await page('today/index.html#queue', 390, 844, 2);
  await p.ev(`${FRAME} scrollTo(0, document.documentElement.scrollHeight)`);
  await sleep(300);
  report.today['390 bottom'] = await p.ev(`(() => { const nav = document.querySelector('nav').getBoundingClientRect(), fc = [...document.querySelectorAll('#queue>.fc')].pop();
    const last = fc.getBoundingClientRect(); return { lastBottom: Math.round(last.bottom), navTop: Math.round(nav.top), clear: Math.round(nav.top - last.bottom) }; })()`);
  await p.done();
}
// Switching: a tap on a chip, a swipe each way, ←/→, then `all`; the pick survives a reload.
{
  const p = await page('next/index.html#queue', 390, 844, 2);
  await p.ev(`${PICK('herdsman')}; location.reload()`).catch(() => {});
  await sleep(1500);
  await p.ev(FRAME);
  const at = () => p.ev(`document.querySelector('.qcp[aria-selected=true]').dataset.f`);
  const s = report.switch;
  s.start = await at();
  await p.tap('.qcp[data-f=synergy]'); s['tap synergy'] = await at();
  save('synergy-390.png', await p.png());
  await p.swipe(-200); s['swipe left'] = await at();
  await p.swipe(200); s['swipe right'] = await at();
  await p.key('ArrowRight'); s['→'] = await at();
  await p.key('ArrowLeft'); s['←'] = await at();
  await p.tap('.qcp[data-f="*"]'); s['tap all'] = await at();
  s['all rows'] = await p.ev(`[...document.querySelectorAll('#queue>.qall details.q')].map(d => d.querySelector('.qf').textContent + ' #' + d.dataset.n)`);
  save('all-390.png', await p.png());
  // a needs-you row opens on its ask card (the options in place)
  await p.tap('.qall details.q>summary');
  s['all: open row has its ask card'] = await p.ev(`!!document.querySelector('.qall details.q[open] .cd>.ask')`);
  save('all-open-390.png', await p.png());
  await p.ev('location.reload()').catch(() => {});
  await sleep(1500);
  s['after reload'] = await at();
  report.next['390 all'] = await p.ev(MEASURE);
  await p.done();
}
// 1440: one column; [‹] [›], a mouse drag and a sideways wheel switch too.
{
  const p = await page('next/index.html#queue', 1440, 900, 1);
  await p.ev(`${PICK('herdsman')}; location.reload()`).catch(() => {});
  await sleep(1500);
  await p.ev(FRAME);
  report.next['1440 first screen'] = await p.ev(MEASURE);
  await p.ev(`document.querySelector('details.qch[data-chain="265"]').open = true`);
  save('queue-1440.png', await p.png());
  const at = () => p.ev(`document.querySelector('.qcp[aria-selected=true]').dataset.f`);
  await p.tap('.qnv[data-d="1"]'); report.switch['1440 [›]'] = await at();
  await p.drag(200); report.switch['1440 mouse drag right'] = await at();
  await p.ev(`document.querySelector('#queue').dispatchEvent(new WheelEvent('wheel', { deltaX: 80, bubbles: true, cancelable: true }))`); report.switch['1440 wheel'] = await at();
  await p.done();
  const t = await page('today/index.html#queue', 1440, 900, 1);
  await t.ev(FRAME);
  save('today-1440.png', await t.png());
  await t.done();
}
// Contrast in every scheme (#244's eight): the new title line vs today's --mute.
for (const sc of ['github-dark', 'solarized-dark', 'gruvbox', 'nord', 'dracula', 'paper', 'farm', 'kitchen']) {
  const p = await page(`next/index.html?scheme=${sc}#queue`, 390, 844, 1), t = await page(`today/index.html?scheme=${sc}#queue`, 390, 844, 1);
  const n = await p.ev(CONTRAST), o = await t.ev(CONTRAST);
  report.contrast[sc] = { title: n.title, todayTitle: o.title, mute: n.mute };
  if (sc === 'paper') save('queue-paper-390.png', await p.png());
  await p.done();
  await t.done();
}
// The board's own audit on the Queue tab (scripts/ui-audit.sh's measure, #63) at 390 and 1440, herdsman picked.
for (const [w, h, d] of [[390, 844, 3], [1440, 900, 2]]) {
  const p = await page(`next/index.html?audit=queue&check=1${w > 720 ? '&w=1440' : ''}`, w, h, d);
  for (let i = 0; i < 80 && !(await p.ev(`!!document.querySelector('#audit')`)); i++) await sleep(100);
  const a = await p.ev(`JSON.parse(document.querySelector('#audit')?.textContent || '{"outliers":["no audit output"]}')`);
  // #331's one-line-title rule, on the rows a reader can see: a closed chain, `+ N more` and the other repos hide theirs
  // by design (phase 2 narrows the audit's rule the same way); #331's 4-deep clamp is gone with the staircase.
  const hidden = await p.ev(`[...document.querySelectorAll('#queue details.q>summary')].filter(s => !s.checkVisibility({ visibilityProperty: true })).length`);
  const shown = await p.ev(`[...document.querySelectorAll('#queue details.q>summary')].filter(s => s.checkVisibility({ visibilityProperty: true })).filter(s => { const t = s.querySelector(':scope>.tl');
    return !t || t.title !== t.textContent || t.offsetHeight > 1.5 * parseFloat(getComputedStyle(t).lineHeight); }).map(s => s.textContent.trim().slice(0, 30))`);
  report.audit[w] = { controls: a.controls, outliers: a.outliers.filter(o => !/^no one-line title|^no clamped/.test(o)), rowsHiddenByDesign: hidden, shownRowsWithoutOneLineTitle: shown };
  await p.done();
}
// One sheet for the owner: today's first screen beside the proposed one (390, the same fixture and phone height).
writeFileSync(join(root, 'ba.html'), `<!doctype html><meta charset=utf-8><style>body{margin:0;padding:12px;background:#0d1117;color:#c9d1d9;font:13px ui-monospace,Menlo,monospace;width:${2 * 390 + 36}px}
.r{display:flex;gap:12px}.r div{width:390px}p{margin:0 0 6px}img{display:block;width:390px;border:1px solid #30363d}</style>
<p>#367 the Queue tab's first screen at 390 × 844, the same fixture (3 repos, 6-step chain, 40+ queued)</p>
<div class=r><div><p>today: ${report.today['390'].pageHeight} px long, first running row at y ${report.today['390'].firstRunningRowTop}</p><img src="today/shot.png"></div>
<div><p>proposed: herdsman picked, ${report.next['390 first screen'].inProgress} in progress, all above the fold</p><img src="next/shot.png"></div></div>`);
writeFileSync(join(root, 'today', 'shot.png'), readFileSync(join(out, 'today-390.png')));
writeFileSync(join(root, 'next', 'shot.png'), readFileSync(join(out, 'queue-390.png')));
{
  const p = await page('ba.html', 2 * 390 + 36, 400, 2);
  save('before-after-390.png', await p.full());
  await p.done();
}
// The pure part's tests (phase 2 moves them to crates/server/tests).
const t = spawnSync('node', [join(here, 'queue.test.js')], { encoding: 'utf8' });
report.tests = (t.stdout + t.stderr).trim().split('\n');
report.errors = [...new Set(errors)];
writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 1) + '\n');
console.log(JSON.stringify(report, null, 1));
c.kill();
srv.close();
