// #391: the live page in headless Chrome (DevTools over --remote-debugging-pipe, no dependency): node live.mjs <chrome>
// <url> <events.jsonl> <line1> <line2>. Loads the page, appends line1 then line2 (#268: each a `queue` on the stream, the
// Queue tab's data the TypeScript board draws), a `build` of another build, then the page again carrying the Lanes tab
// only. Prints one JSON object: the backlog as drawn at each step, the `queue`s seen, the reloads, the page's uncaught errors.
import { spawn } from 'node:child_process';
import { appendFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chrome, url, events, line1, line2] = process.argv.slice(2);
const p = spawn(chrome, ['--headless', '--no-sandbox', '--disable-gpu', '--remote-debugging-pipe', '--no-first-run',
  `--user-data-dir=${mkdtempSync(join(tmpdir(), 'live-'))}`, '--window-size=390,844', 'about:blank'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
const wait = ms => new Promise(r => setTimeout(r, ms));
let id = 0, buf = '', session = null, loads = 0;
const pend = new Map(), errors = [];
p.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
    if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
    if (m.method === 'Page.loadEventFired') loads++;
  }
});
const cmd = (method, params = {}) => new Promise((r, j) => {
  pend.set(++id, m => (m.error ? j(new Error(`${method}: ${m.error.message}`)) : r(m.result)));
  p.stdio[3].write(JSON.stringify({ id, method, params, ...(session && { sessionId: session }) }) + '\0');
});
const js = async e => (await cmd('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true })).result.value;
const until = async (e, ms = 15000) => { for (const t = Date.now(); Date.now() - t < ms; await wait(200)) if (await js(e).catch(() => false)) return true; return false; };
const load = async u => { const n = loads; await cmd('Page.navigate', { url: u }); for (const t = Date.now(); loads === n && Date.now() - t < 15000;) await wait(100); await wait(1500); };
// The backlog as a reader sees it: its count, rows, search box and quick filters, per repo.
const backlog = () => js(`[...document.querySelectorAll('#queue .bl')].map(b => ({ count: b.querySelector('summary>.m')?.textContent ?? null,
  rows: b.querySelectorAll('.bi').length, search: b.querySelectorAll('.bs input').length, chips: b.querySelectorAll('[data-bf]').length,
  flex: getComputedStyle(b.querySelector('summary')).display }))`);
const out = {};
try {
  const { targetId } = await cmd('Target.createTarget', { url: 'about:blank' });
  session = (await cmd('Target.attachToTarget', { targetId, flatten: true })).sessionId;
  await cmd('Page.enable');
  await cmd('Runtime.enable');
  await load(url);
  out.fresh = await backlog();
  await js(`window.__qs = 0; es.addEventListener('queue', () => window.__qs++); 1`);
  appendFileSync(events, line1 + '\n');
  out.frag = await until(`window.__qs > 0 && document.querySelector('#queue').textContent.includes('#${JSON.parse(line1).issues[0]}')`);
  appendFileSync(events, line2 + '\n');
  out.patched = await until(`window.__qs > 1 && document.querySelector('#queue').textContent.includes('#${JSON.parse(line2).issues[0]}')`);
  out.patches = await js('window.__qs');
  out.after_patch = await backlog();
  // A build event: this page's own build reloads nothing; another build reloads once, and not again in this session.
  const own = await js(`document.querySelector('script[src*="serve.js"]').getAttribute('src').match(/([0-9a-f]{8})\\/serve\\.js/)[1]`);
  const build = async b => { const n = loads; await js(`es.dispatchEvent(new MessageEvent('build', { data: '${b}' })); 1`); await wait(2500); return loads - n; };
  out.reloads = [await build(own), await build('0badc0de'), await build('0badc0de')];
  // The page carrying the Lanes tab only (its cookie) opened on #queue: the Queue tab paints its last copy.
  await js(`tab('lanes'); 1`);
  out.served_queue = await js(`fetch('/?carried=1').then(r => r.text()).then(t => !t.includes('<section id=queue></section>'))`);
  await load(url + '?again=1#queue');
  out.tab = await js('document.documentElement.dataset.tab');
  out.other_tab = await backlog();
} catch (e) {
  out.failed = String(e.stack || e);
}
out.errors = errors;
console.log(JSON.stringify(out));
p.kill();
process.exit(0);
