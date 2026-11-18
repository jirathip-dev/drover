// The phone page in headless Chrome (DevTools over --remote-debugging-pipe, no dependency): node page.mjs <chrome>
// <url> <cookie name=value>. As a paired phone it answers an ask, approves and denies a run card, pauses and resumes a
// fleet, stops a lane from its sheet and saves the Fleets sheet, each by tapping the page; then, offline, reloads it and
// reads what the cached screens paint. Prints one JSON object: each step's toast (the server's reply line), the actions the page posted and the page's uncaught errors.
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chrome, url, cookie] = process.argv.slice(2);
const p = spawn(chrome, ['--headless', '--no-sandbox', '--disable-gpu', '--remote-debugging-pipe', '--no-first-run',
  `--user-data-dir=${mkdtempSync(join(tmpdir(), 'page-'))}`, '--window-size=390,844', 'about:blank'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
const wait = ms => new Promise(r => setTimeout(r, ms));
let id = 0, buf = '', session = null;
const pend = new Map(), errors = [], posted = [];
p.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
    if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
    if (m.method === 'Network.requestWillBeSent' && m.params.request.method === 'POST' && m.params.request.url.endsWith('/action')) posted.push(JSON.parse(m.params.request.postData || '{}').action);
  }
});
const cmd = (method, params = {}) => new Promise((r, j) => {
  pend.set(++id, m => (m.error ? j(new Error(`${method}: ${m.error.message}`)) : r(m.result)));
  p.stdio[3].write(JSON.stringify({ id, method, params, ...(session && { sessionId: session }) }) + '\0');
});
const js = async e => (await cmd('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true })).result.value;
const until = async (e, what) => { for (const t = Date.now(); Date.now() - t < 15000; await wait(150)) if (await js(e).catch(() => false)) return; throw new Error(`timed out: ${what}`); };
const tap = async (sel, text = '') => {
  const q = `[...document.querySelectorAll(${JSON.stringify(sel)})].find(e => e.checkVisibility() && e.textContent.trim().toLowerCase().startsWith(${JSON.stringify(text)}))`;
  await until(`!!(${q})`, `${sel} ${text}`);
  await js(`(${q}).click()`);
};
const toast = async what => {
  await until(`!document.getElementById('toast').hidden`, `${what}'s toast`);
  const t = await js(`(t => (t.hidden = true, t.textContent))(document.getElementById('toast'))`);
  return t;
};
const out = { steps: {}, posted, errors };
try {
  const { targetId } = await cmd('Target.createTarget', { url: 'about:blank' });
  session = (await cmd('Target.attachToTarget', { targetId, flatten: true })).sessionId;
  await cmd('Network.enable');
  const [name, value] = cookie.split(/=(.*)/);
  await cmd('Network.setCookie', { name, value, url });
  await cmd('Page.navigate', { url });
  await until(`!!document.querySelector('main .scr')`, 'the first screen');

  await tap('[data-id="ask-q7"] .fl');
  await tap('[data-id="ask-q7"] button.pick', 'refuse');
  out.steps.answer = await toast('answer');
  await until(`!document.querySelector('[data-id="ask-q7"]')`, 'the answered card to go');

  await tap('[data-id="run-r1"] button', 'approve');
  out.steps.approve = await toast('approve');
  await tap('[data-id="run-r2"] button', 'deny');
  out.steps.deny = await toast('deny');

  await tap('button[data-id="pause-sendmeter"]', 'pause');
  out.steps.pause = await toast('pause');
  await tap('[data-id="fleet-sendmeter"] .fold button.main', 'resume');
  out.steps.resume = await toast('resume');

  await tap('nav a', 'lanes');
  await tap('.row[data-id="l-sendmeter-impl-12"]');
  await tap('#sheet button[data-id="act-stop"]', 'stop');
  await tap('#confirm button', 'stop');
  out.steps.stop = await toast('stop');
  await tap('#sheet .sh button', 'close');

  await tap('#sysb');
  await tap('#sheet .row', 'drover');
  await until(`!!document.querySelector('#sheet select[name="sendmeter.impl"]')`, 'the Fleets sheet');
  await js(`(s => { s.value = 'sonnet'; s.dispatchEvent(new Event('change')); })(document.querySelector('#sheet select[name="sendmeter.impl"]'))`);
  await tap('#sheet .sh button', 'save');
  out.steps.fleets = await toast('save');
  out.sheetClosed = await js(`document.getElementById('sheet').hidden`);

  await until(`!!navigator.serviceWorker.controller`, 'the service worker');
  await cmd('Page.reload');
  await until(`!!document.querySelector('main .scr .row')`, 'the page again');
  await wait(500);
  await cmd('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  await cmd('Page.reload');
  await until(`!!document.querySelector('main .scr .row')`, 'the cached screen offline');
  await wait(1000);
  out.offline = await js(`({ stale: document.getElementById('stale').hidden ? '' : document.getElementById('stale').textContent,
    dot: document.getElementById('dot').className, tab: document.querySelector('main .scr').dataset.id })`);
} catch (e) {
  out.error = e.message;
}
console.log(JSON.stringify(out));
p.kill();
process.exit(0);
