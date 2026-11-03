// #290 phase 1: the look sheet, shot from the real board. Takes an exported demo (`serve --export <site> --demo`),
// makes a copy the way phase 2 will: the header's two `<label class=pk>` selects become one `[◐]` button (serve.rs),
// serve.js's two picker blocks become look.js, serve.css loses the Queue-tab hide and gains look.css. Then it shoots
// the four tabs' headers (today and proposed), the sheet at 390 and 1440, and measures what gate 2 asks.
// No deps: Chrome's --remote-debugging-pipe, like ../schemes and scripts/ui-close.mjs.
// Usage: node shoot.mjs <site dir> <chrome-headless-shell> [out dir = this folder]
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname, repo = join(here, '../../..');
const [site, chrome, out = here] = process.argv.slice(2);
const read = f => readFileSync(f, 'utf8');

// The phase-2 changes, applied to a copy. Each is (today, proposed) and must match.
const swap = (text, pairs, what) => pairs.reduce((t, [re, to]) => {
  if (!re.test(t)) throw new Error(`${what} changed: ${re} no longer matches`);
  re.lastIndex = 0;
  return t.replace(re, to);
}, text);
const CSS = [
  [/#herd-on,#thp\{margin-left:var\(--s2\)\}#scp\{margin-left:var\(--s4\)\} \/\* #244[^*]*\*\/ \/\* #241[^*]*\*\//, '#herd-on,#look{margin-left:var(--s2)}'],
  [/html\[data-tab=queue\] :is\(#thp,#scp\),/, ''],
  [/\n$/, '\n' + read(join(here, 'look.css'))],
];
const JS = [[/\/\/ #244: the `\[▦\]` colours[\s\S]*?\ncolours\(\);\n(?=\/\/ #241: the theme on the tabs)/, () => read(join(here, 'look.js'))],
  // The offline note says only what fits beside four controls at 390 (`last update` stays its title).
  [/  \$\('#stale'\)\.textContent = 'last update ' \+ new Date\(last\)\.toTimeString\(\)\.slice\(0, 5\);/,
    "  const at = new Date(last).toTimeString().slice(0, 5);\n  Object.assign($('#stale'), { textContent: 'as of ' + at, title: 'last update ' + at });"],
  // Closing the look sheet hands focus back to `[◐]` (a keyboard user lands where they were).
  [/function close\(\) \{\n  if \(!shown\(\) \|\| shut\) return;\n/, m => m + "  if (sh.classList.contains('look')) $('#look').focus({ preventScroll: true });\n"]];
const HTML = [[/<label class=pk id=thp>.*?<\/label><label class=pk id=scp>.*?<\/label>/,
  '<button id=look type=button aria-haspopup=dialog aria-label="look: theme and colours">◐</button>']];

const root = mkdtempSync(join(tmpdir(), 'pickers-'));
for (const v of ['today', 'next']) cpSync(site, join(root, v), { recursive: true });
const walk = d => readdirSync(d).flatMap(f => statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]);
for (const f of walk(join(root, 'next'))) {
  if (f.endsWith('/serve.css')) writeFileSync(f, swap(read(f), CSS, 'serve.css'));
  if (f.endsWith('/serve.js')) writeFileSync(f, swap(read(f), JS, 'serve.js'));
  if (f.endsWith('.html') && read(f).includes('id=thp>')) writeFileSync(f, swap(read(f), HTML, f));
}
// The exact phase-2 asset change, for review (serve.rs's is the HTML swap above: README).
for (const [f, pairs] of [['serve.css', CSS], ['serve.js', JS]]) {
  writeFileSync(join(root, f), swap(read(join(repo, 'assets', f)), pairs, f));
  writeFileSync(join(out, f + '.diff'), spawnSync('diff', ['-u', '--label', `a/assets/${f}`, '--label', `b/assets/${f}`,
    join(repo, 'assets', f), join(root, f)], { encoding: 'utf8' }).stdout);
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

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'pickers-prof-'))}`,
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

// A page in a fresh browser context (no service worker or localStorage carried over) at W×H, DPR; `ev` runs an
// expression (awaited) in it.
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
  await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile: W < 720 });
  await cmd('Page.navigate', { url: base + '/' + url });
  for (let i = 0; i < 100; i++, await sleep(100)) if (await ev(`document.readyState === 'complete' && typeof TT !== 'undefined' && !!TT`)) break;
  await ev('document.fonts.ready.then(() => 1)');
  await sleep(500);
  const png = async clip => Buffer.from((await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { x: 0, y: 0, width: W, height: H, scale: 1, ...clip } })).data, 'base64');
  const done = async () => { await send('Target.closeTarget', { targetId }); await send('Target.disposeBrowserContext', { browserContextId }); };
  return { ev, png, done, key: (key, code = key) => ['keyDown', 'keyUp'].reduce((p, type) => p.then(() => cmd('Input.dispatchKeyEvent', { type, key, code, windowsVirtualKeyCode: { ArrowDown: 40, ArrowUp: 38, Enter: 13, Tab: 9, Escape: 27 }[key], ...(type === 'keyDown' && key === 'Enter' ? { text: '\r' } : {}) })), Promise.resolve()) };
}
// Framing only (no style is touched): the demo banner goes and the needs-you cards stay out, so the board fills the shot.
const FRAME = `document.querySelector('.msg.a')?.remove(); document.querySelector('#needs')?.replaceChildren(); scrollTo(0, 0);`;
// A tap: a pointer click (detail 1), not a keyboard one.
const tap = sel => `(document.querySelector(${JSON.stringify(sel)}).dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })), new Promise(r => setTimeout(r, 900)))`;
const report = { headers: {}, sheet: {} };

// 1. The header on each tab, today and proposed (390, DPR 2), and what fits.
// `queue-offline`: the Queue header at its fullest: the live board offline (`last update hh:mm`, #dot red; the demo
// never goes offline, so its `demo` tag goes).
const TABS = ['queue', 'lanes', 'host', 'events'], ROWS = [...TABS, 'queue-offline'], strips = {};
for (const v of ['today', 'next']) for (const r of ROWS) {
  const t = r.split('-')[0];
  const p = await page(`${v}/${t === 'host' ? 'busy/' : ''}index.html#${t}`, 390, 844, 2);
  await p.ev(`tab('${t}'); ${FRAME} ${r === 'queue-offline' ? `document.querySelector('#demo')?.remove(); Object.assign(document.querySelector('#stale'), { hidden: false, textContent: '${v === 'today' ? 'last update' : 'as of'} 14:05' }); document.querySelector('#dot').className = 'r';` : ''}`);
  await sleep(300);
  const m = await p.ev(`(() => {
    const h = document.querySelector('header'), seen = e => e.checkVisibility({ visibilityProperty: true });
    const ctl = [...h.querySelectorAll('button, .pk')].filter(seen);
    return { height: h.offsetHeight, right: Math.max(...ctl.map(e => e.getBoundingClientRect().right)) | 0,
      controls: ctl.map(e => '[' + ((e.matches('.pk') ? e.firstChild : e).textContent.trim() || '🐎') + ']'), title: document.querySelector('header b').offsetHeight, staleCut: !document.querySelector('#stale').hidden && document.querySelector('#stale').scrollWidth > document.querySelector('#stale').clientWidth, look: !!document.querySelector('#look') && seen(document.querySelector('#look')),
      selects: document.querySelectorAll('select#theme, select#scheme').length };
  })()`);
  report.headers[`${v} ${r}`] = m;
  strips[`${v}-${r}`] = await p.png({ height: 54 });
  await p.done();
}
// The strips side by side on one sheet: a tab a row, today on the left, proposed on the right (each 390 px).
for (const [k, b] of Object.entries(strips)) writeFileSync(join(root, `${k}.png`), b);
writeFileSync(join(root, 'headers.html'), `<!doctype html><meta charset=utf-8><style>body{margin:0;background:#0d1117;color:#8b949e;font:13px ui-monospace,Menlo,monospace;width:${2 * 390 + 36}px;padding:12px}
h1{font-size:13px;color:#c9d1d9;margin:0 0 8px}.r{display:flex;gap:12px;margin:0 0 6px}.r div{width:390px}.r p{margin:6px 0 2px}img{display:block;width:390px;border:1px solid #30363d}</style>
<h1>#290 the header on each tab at 390 px: today (left), proposed (right)</h1>
${ROWS.map(t => `<div class=r>${['today', 'next'].map(v => `<div><p>${t} · ${v === 'today' ? 'today' : 'proposed'}: ${report.headers[`${v} ${t}`].controls.join(' ')}</p><img src="${v}-${t}.png"></div>`).join('')}</div>`).join('')}`);
{
  const p = await page('headers.html', 2 * 390 + 36, 400, 2);
  const h = await p.ev('document.body.scrollHeight');
  writeFileSync(join(out, 'headers-390.png'), await p.png({ height: h }));
  await p.done();
}

// 2. The sheet: what gate 2 asks, measured on the open sheet.
const MEASURE = `(() => {
  const S = document.querySelector('#sheet'), seen = e => e.checkVisibility({ visibilityProperty: true });
  const lists = [...S.querySelectorAll('[role=listbox]')].map(l => ({ label: l.getAttribute('aria-label'),
    options: [...l.querySelectorAll('[role=option]')].map(o => o.querySelector('.ln').textContent + (o.getAttribute('aria-selected') === 'true' ? ' ✓' : '') + (o.querySelector('small') ? ' ' + o.querySelector('small').textContent : '')) }));
  const rows = [...S.querySelectorAll('.lko')].filter(seen), bad = [];
  for (const r of rows) if (r.offsetHeight < 44) bad.push(r.textContent + ': row ' + r.offsetHeight + ' px');
  for (const e of [...S.querySelectorAll('.top *')].filter(seen)) {
    const t = [...e.childNodes].some(n => n.nodeType === 3 && n.data.trim());
    if (t && !['11px', '13px'].includes(getComputedStyle(e).fontSize)) bad.push(e.textContent.slice(0, 20) + ': font ' + getComputedStyle(e).fontSize);
    if (e.getBoundingClientRect().right > innerWidth) bad.push(e.className + ' past the right edge');
  }
  return { open: S.classList.contains('on'), title: S.querySelector('.hd b').textContent, theme: document.documentElement.dataset.theme,
    scheme: document.documentElement.dataset.scheme, lists, selects: document.querySelectorAll('select#theme, select#scheme').length,
    focus: document.activeElement.closest('.lko')?.querySelector('.ln').textContent || null, outliers: bad };
})()`;
// 390: Queue, `[◐]` tapped: farm with its default colours (the demo phone never picked).
{
  const p = await page('next/index.html#queue', 390, 844, 2);
  await p.ev(`${FRAME} window.kept = 1; ${tap('#look')}`);
  report.sheet['390 open'] = await p.ev(MEASURE);
  writeFileSync(join(out, 'look-390.png'), await p.png());
  // A tap on kitchen: applied in place (no reload: `kept` survives), the sheet stays open, colours follow the theme.
  await p.ev(tap('.lko[data-t=kitchen]'));
  await sleep(600);
  report.sheet['390 tap kitchen'] = { ...await p.ev(MEASURE), reloaded: await p.ev('window.kept !== 1') };
  writeFileSync(join(out, 'look-kitchen-390.png'), await p.png());
  // terminal, then paper: a picked scheme (light) stays over the theme's default, which is still marked.
  await p.ev(tap('.lko[data-t=terminal]'));
  await p.ev(tap('.lko[data-s=paper]'));
  await sleep(600);
  report.sheet['390 terminal + paper'] = { ...await p.ev(MEASURE), reloaded: await p.ev('window.kept !== 1'), kept: await p.ev(`[localStorage.theme, localStorage.scheme]`) };
  writeFileSync(join(out, 'look-paper-390.png'), await p.png());
  await p.done();
}
// The other three tabs: `[◐]` opens the same sheet there.
for (const t of ['lanes', 'host', 'events']) {
  const p = await page(`next/${t === 'host' ? 'busy/' : ''}index.html#${t}`, 390, 844, 2);
  await p.ev(`tab('${t}'); ${FRAME} ${tap('#look')}`);
  const m = await p.ev(MEASURE);
  report.sheet[`390 ${t}`] = { open: m.open, lists: m.lists.map(l => `${l.label} ${l.options.length}`), outliers: m.outliers };
  await p.done();
}
// 1440: the lists side by side at half height; the keyboard: ↓ twice in COLOURS, Enter (applies, keeps focus).
{
  const p = await page('next/index.html#queue', 1440, 900, 1);
  // Opened from the keyboard ([◐] focused, Enter): focus lands on the current theme.
  await p.ev(`${FRAME} window.kept = 1; document.querySelector('#look').focus()`);
  await p.key('Enter');
  await sleep(900);
  report.sheet['1440 open by Enter'] = { focus: (await p.ev(MEASURE)).focus };
  writeFileSync(join(out, 'look-1440.png'), await p.png());
  // Tab to COLOURS, ↓ twice, Enter (applies, focus stays), ↓ once more.
  for (const k of ['Tab', 'ArrowDown', 'ArrowDown', 'Enter', 'ArrowDown']) { await p.key(k); await sleep(250); }
  await sleep(600);
  report.sheet['1440 keys Tab ↓ ↓ Enter ↓'] = { ...await p.ev(MEASURE), reloaded: await p.ev('window.kept !== 1') };
  writeFileSync(join(out, 'look-keys-1440.png'), await p.png());
  // Esc closes it and hands focus back to `[◐]`.
  await p.key('Escape');
  await sleep(600);
  report.sheet['1440 Esc'] = await p.ev(`({ open: document.querySelector('#sheet').classList.contains('on'), focus: document.activeElement.id })`);
  await p.done();
}
// The board's own audit (scripts/ui-audit.sh's measure, #63 gate 2) on each tab with the new header, at 390.
report.audit = {};
for (const t of TABS) {
  const p = await page(`next/${t === 'host' ? 'busy/' : ''}index.html?audit=${t}&check=1`, 390, 844, 3);
  for (let i = 0; i < 80 && !(await p.ev(`!!document.querySelector('#audit')`)); i++) await sleep(100);
  report.audit[t] = await p.ev(`JSON.parse(document.querySelector('#audit')?.textContent || '{"outliers":["no audit output"]}')`);
  await p.done();
}

writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 1) + '\n');
for (const [k, v] of Object.entries(report.headers)) console.log(`header ${k}: ${v.controls.join(' ')} · right ${v.right} · height ${v.height} · title ${v.title} · stale cut ${v.staleCut} · [◐] ${v.look}`);
for (const [k, v] of Object.entries(report.sheet)) console.log(`sheet ${k}: ${JSON.stringify(v)}`);
for (const [k, v] of Object.entries(report.audit)) console.log(`audit ${k}: ${v.controls} controls, outliers ${JSON.stringify(v.outliers)}`);
if (errors.length) console.log('page errors:', [...new Set(errors)]);
report.errors = [...new Set(errors)];
writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 1) + '\n');
c.kill();
srv.close();
