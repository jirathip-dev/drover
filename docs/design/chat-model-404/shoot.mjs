// #404 design: the harness pick and the model/effort chip, shot from the real board. Takes an exported demo
// (`serve --export <site> --demo`) and makes two copies: `today`, untouched, and `next`, the way phase 2 will be:
// serve.js gains run.js (and two one-line hooks), serve.css gains run.css; mock.js stands in for chat.rs's new HTML.
// Then it shoots each state, measures the chips and the sheet (docs/ui.md gate 2), and runs the board's own audit on
// the chat views. No deps: Chrome's --remote-debugging-pipe, like ../pickers/shoot.mjs.
// Usage: node shoot.mjs <site dir> <chrome-headless-shell> [out dir = this folder]
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const here = new URL('.', import.meta.url).pathname, repo = join(here, '../../..');
const [site, chrome, out = here] = process.argv.slice(2);
const read = f => readFileSync(f, 'utf8');
const swap = (text, pairs, what) => pairs.reduce((t, [re, to]) => {
  if (!re.test(t)) throw new Error(`${what} changed: ${re} no longer matches`);
  re.lastIndex = 0;
  return t.replace(re, to);
}, text);
// The phase-2 asset changes: chatPaint repaints the chips (a live swap keeps a new thread's picks), a new thread's
// first message carries its run, and run.js / run.css are added.
const JS = [
  [/(  root\.querySelectorAll\('\.tld'\)\.forEach\(i => \{ i\.innerHTML = \$\('#ld'\)\.innerHTML; \}\);\n)/, '$1  runPaint(root);\n'],
  [/\{ op: 'new', repo \}/, "{ op: 'new', repo, ...runNew(f) }"],
  [/\n$/, () => '\n' + read(join(here, 'run.js'))],
];
const CSS = [[/\n$/, () => '\n' + read(join(here, 'run.css'))]];

const root = mkdtempSync(join(tmpdir(), 'chat404-'));
for (const v of ['today', 'next']) cpSync(site, join(root, v), { recursive: true });
const walk = d => readdirSync(d).flatMap(f => statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]);
for (const f of walk(join(root, 'next'))) {
  if (f.endsWith('/serve.css')) writeFileSync(f, swap(read(f), CSS, 'serve.css'));
  if (f.endsWith('/serve.js')) writeFileSync(f, swap(read(f), JS, 'serve.js') + read(join(here, 'mock.js')));
}
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

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'chat404-prof-'))}`,
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
  await cmd('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile: W < 720 });
  await cmd('Page.navigate', { url: base + '/' + url });
  for (let i = 0; i < 100; i++, await sleep(100)) if (await ev(`document.readyState === 'complete' && typeof TT !== 'undefined' && !!TT`)) break;
  await ev('document.fonts.ready.then(() => 1)');
  await sleep(500);
  const png = async () => Buffer.from((await cmd('Page.captureScreenshot', { format: 'png' })).data, 'base64');
  const done = async () => { await send('Target.closeTarget', { targetId }); await send('Target.disposeBrowserContext', { browserContextId }); };
  return { ev, png, done };
}
// Framing: the demo banner and the needs-you cards go; the demo's inert controls are enabled, as a paired phone's.
const OPEN = (t, paired = true) => `document.querySelector('.msg.a')?.remove(); tab('chat'); chatOpen(''); document.querySelector('.lall')?.click();
  document.querySelector('#needs')?.replaceChildren(); document.querySelector('.later')?.remove(); chatOpen('${t}');
  ${paired ? `document.querySelectorAll('#chat [disabled]').forEach(e => { e.disabled = false; }); document.querySelectorAll('#chat .ns').forEach(e => e.remove());` : ''}
  typeof runPaint === 'function' && runPaint(); scrollTo(0, document.body.scrollHeight);`;
const tap = sel => `(document.querySelector(${JSON.stringify(sel)}).dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })), new Promise(r => setTimeout(r, 700)))`;
const closeSheet = `(close(), new Promise(r => setTimeout(r, 500)))`;
// Gate 2 on what #404 adds: the chips (28 px, ≥ 44 px tap with the ring, one line, inside the screen), the sheet's rows
// (≥ 44 px), text 11/13 px, and the composer, header and sheet inside the screen.
const MEASURE = `(() => {
  const seen = e => e.checkVisibility({ visibilityProperty: true }), W = innerWidth, bad = [];
  const v = document.querySelector('#chat .cvp:not([style*="display: none"])'), cvp = [...document.querySelectorAll('#chat .cvp')].find(seen);
  const tw = cvp && cvp.querySelector('.tw'), run = tw && tw.querySelector('.rcs');
  const chips = run ? [...run.querySelectorAll('button')].map(b => { const r = b.getBoundingClientRect(), a = getComputedStyle(b, '::after');
    if (Math.round(r.height) !== 28) bad.push(b.textContent + ': chip ' + r.height + ' px');
    if (r.height + 2 * parseFloat(a.top === 'auto' ? 0 : -parseFloat(a.top)) < 44) bad.push(b.textContent + ': tap under 44');
    if (r.right > W || r.left < 0) bad.push(b.textContent + ': past the screen');
    const s = b.querySelector('span'); if (s.scrollWidth > s.clientWidth + 1 && getComputedStyle(s).textOverflow !== 'ellipsis') bad.push(b.textContent + ': overflows');
    return '[' + b.textContent.trim() + ']' + (b.disabled ? ' (disabled)' : ''); }) : [];
  const note = run && run.querySelector('.rn')?.textContent || null;
  for (const e of [...(tw ? tw.querySelectorAll('*') : []), ...(cvp ? cvp.querySelectorAll('.th *, .tc .qs') : [])].filter(seen)) {
    const own = [...e.childNodes].some(n => n.nodeType === 3 && n.data.trim()), fs = getComputedStyle(e).fontSize;
    if (own && !e.matches('textarea') && !['11px', '13px'].includes(fs)) bad.push((e.textContent || '').slice(0, 20) + ': font ' + fs);
    if (e.getBoundingClientRect().right > W + 0.5) bad.push(e.className + ' past the right edge');
  }
  const S = document.querySelector('#sheet'), open = S.classList.contains('on') && document.querySelector('.sh.rpk');
  const lists = open ? [...S.querySelectorAll('[role=listbox]')].map(l => l.getAttribute('aria-label') + ': ' + [...l.children].map(o =>
    o.querySelector('.ln').textContent + (o.getAttribute('aria-selected') === 'true' ? ' ✓' : '') + (o.querySelector('small')?.textContent ? ' ' + o.querySelector('small').textContent : '')).join(', ')) : [];
  if (open) for (const r of S.querySelectorAll('.rpk .lko')) { if (r.offsetHeight < 44) bad.push(r.textContent + ': row ' + r.offsetHeight); if (r.getBoundingClientRect().right > W) bad.push(r.textContent + ': row past the edge'); }
  if (open) for (const e of [...S.querySelectorAll('.top *')].filter(seen)) { const own = [...e.childNodes].some(n => n.nodeType === 3 && n.data.trim()); if (own && !['11px', '13px'].includes(getComputedStyle(e).fontSize)) bad.push(e.textContent.slice(0, 20) + ': font ' + getComputedStyle(e).fontSize); }
  if (document.documentElement.scrollWidth > W) bad.push('sideways scroll');
  return { header: cvp && cvp.querySelector('.th .ds').textContent, chips, note, composer: tw && Math.round(tw.getBoundingClientRect().height),
    sheet: open ? S.querySelector('.hd b').textContent : null, lists, lines: cvp ? [...cvp.querySelectorAll('.tc .qs')].slice(-2).map(p => p.textContent) : [], outliers: bad };
})()`;
const report = {};
async function shot(name, p) {
  report[name] = await p.ev(MEASURE);
  writeFileSync(join(out, name + '.png'), await p.png());
}

// 1. A new thread (acme-api), today and proposed.
{
  const p = await page('today/index.html#chat', 390, 844, 3);
  await p.ev(OPEN('new-acme-api'));
  await sleep(400);
  await shot('today-new-390', p);
  await p.done();
}
{
  const p = await page('next/index.html#chat', 390, 844, 3);
  await p.ev(OPEN('new-acme-api'));
  await sleep(400);
  await shot('new-390', p);
  await p.ev(tap('#chat .cvp[data-t="new-acme-api"] .rcs .rh'));
  await shot('new-sheet-390', p);
  await p.ev(tap('.rpk .lko[data-k=h][data-v=hermes]'));
  await shot('new-hermes-sheet-390', p);
  await p.ev(closeSheet);
  await shot('new-hermes-390', p);
  // dsh: nothing to pick, so its run is a line, not a chip.
  await p.ev(tap('#chat .cvp[data-t="new-acme-api"] .rcs .rh'));
  await p.ev(tap('.rpk .lko[data-k=h][data-v=dsh]'));
  await p.ev(closeSheet);
  await shot('new-dsh-390', p);
  // The first message carries the run (`op=new` … harness, model, effort): what chatSay would post.
  report['new-dsh-390'].posts = await p.ev(`JSON.stringify({ op: 'new', repo: 'acme-api', ...runNew(document.querySelector('#chat .tw[data-t="new-acme-api"]')) })`);
  await p.done();
}
// 2. A started thread on claude (t4): the chip alone; tap it; pick sonnet, then xhigh: one line; the next turn resumes on it.
{
  const p = await page('next/index.html#chat', 390, 844, 3);
  await p.ev(OPEN('t4'));
  await sleep(400);
  await shot('thread-390', p);
  await p.ev(tap('#chat .cvp[data-t="t4"] .rcs .rm'));
  await shot('model-390', p);
  await p.ev(tap('.rpk .lko[data-k=m][data-v=sonnet]'));
  await p.ev(tap('.rpk .lko[data-k=e][data-v=xhigh]'));
  await shot('model-picked-390', p);
  await p.ev(closeSheet);
  await p.ev('scrollTo(0, document.body.scrollHeight)');
  await shot('changed-390', p);
  // The next turn (as chat.rs will render its events): the owner's message, the resume on the new run, the reply.
  await p.ev(`(() => { const tc = document.querySelector('#chat .cvp[data-t="t4"] .tc'), b = (k, who, t) => { const d = tc.appendChild(el('details', 'blk ' + k)); d.open = true;
    d.innerHTML = '<summary><span class=k>' + who + '</span><span class=pv></span><span class=ct>14:12</span><i class=cv></i></summary><div class=md><p></p></div>'; d.querySelector('.pv').textContent = t; d.querySelector('.md p').textContent = t; };
    b('me', 'you', 'Go with the token bucket; draft the issue.');
    tc.appendChild(el('p', 'qs', '↻ resumed · same session (7f3c9…) · sonnet · xhigh'));
    b('ai', 'conductor', 'Drafted: 60 requests a minute per key, a 429 with Retry-After past it. Queue it?');
    document.querySelector('#chat .cvp[data-t="t4"] .tlv').innerHTML = '<b class=g>●</b> awake · sleeps after 30 min idle';
    scrollTo(0, document.body.scrollHeight); })()`);
  await sleep(300);
  await shot('next-turn-390', p);
  await p.done();
}
// 3. A thread started on hermes (t2), and a guest (unpaired) view of t4: the chip inert.
{
  const p = await page('next/index.html#chat', 390, 844, 3);
  await p.ev(OPEN('t2'));
  await sleep(400);
  await shot('hermes-390', p);
  await p.done();
}
{
  const p = await page('next/index.html#chat', 390, 844, 3);
  await p.ev(OPEN('t4', false));
  await sleep(400);
  await p.ev('scrollTo(0, document.body.scrollHeight)');
  await sleep(300);
  await shot('guest-390', p);
  await p.done();
}
// 4. 1440: the lists side by side (t3code's model list beside its effort column).
{
  const p = await page('next/index.html#chat', 1440, 900, 1);
  await p.ev(OPEN('new-acme-api'));
  await p.ev(tap('#chat .cvp[data-t="new-acme-api"] .rcs .rh'));
  await shot('new-sheet-1440', p);
  await p.ev(closeSheet);
  await p.ev(OPEN('t4'));
  await p.ev(tap('#chat .cvp[data-t="t4"] .rcs .rm'));
  await shot('model-1440', p);
  await p.done();
}
// 5. The board's own audit (ui-audit.js, gate 2) on the chat views, with #404's chips in place.
report.audit = {};
for (const [v, W] of [['chat', 390], ['chat-thread', 390], ['chat-new', 390], ['chat-send', 390], ['chat-long', 390], ['chat-kb', 390], ['chat-thumbs', 390], ['chat-back', 390], ['chat-code', 390], ['chat-thread', 1440], ['chat-new', 1440]]) {
  const p = await page(`next/index.html?audit=${v}&check=1${W > 400 ? '&w=' + W : ''}`, W, W > 400 ? 900 : 844, W > 400 ? 1 : 3);
  for (let i = 0; i < 80 && !(await p.ev(`!!document.querySelector('#audit')`)); i++) await sleep(100);
  report.audit[`${v}@${W}`] = await p.ev(`JSON.parse(document.querySelector('#audit')?.textContent || '{"outliers":["no audit output"]}')`);
  report.audit[`${v}@${W}`].chips = await p.ev(`[...document.querySelectorAll('#chat .cvp')].filter(v => v.checkVisibility()).map(v => [...v.querySelectorAll('.rcs > *')].map(b => b.textContent.trim()).join(' ')).join(' | ')`);
  await p.done();
}

report.errors = [...new Set(errors)];
writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 1) + '\n');
for (const [k, v] of Object.entries(report)) if (k !== 'audit' && k !== 'errors') console.log(`${k}: ${v.header} · ${v.chips.join(' ') || v.note} · composer ${v.composer} px${v.sheet ? ` · sheet "${v.sheet}" ${v.lists.join(' | ')}` : ''}${v.lines.length ? ` · lines ${JSON.stringify(v.lines)}` : ''} · outliers ${JSON.stringify(v.outliers)}`);
for (const [k, v] of Object.entries(report.audit)) console.log(`audit ${k}: ${v.controls} controls, chips "${v.chips}", outliers ${JSON.stringify(v.outliers)}`);
if (report.errors.length) console.log('page errors:', report.errors);
c.kill();
srv.close();
