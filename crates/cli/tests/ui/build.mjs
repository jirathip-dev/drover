// #1351: a page of one build meeting a newer serve, in Playwright's WebKit on a real serve: node build.mjs <url> <cookie name=value>.
// The first threads screen is marked build b: the page reloads and draws none of it first. Same build: no reload.
// A stream event marked build b while the composer has focus: no reload and no block until blur, then one reload; the draft survives.
// Prints one JSON object of what the page showed.
import { createRequire } from 'node:module';

const { webkit } = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [] };
const browser = await webkit.launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  await page.addInitScript(() => {
    const n = Number(sessionStorage.getItem('loads') ?? 0) + 1;
    sessionStorage.setItem('loads', String(n));
    new MutationObserver(() => {
      for (const id of ['t2', 'm-new']) if (document.querySelector(`[data-id="${id}"]`) && !sessionStorage.getItem('drew:' + id)) sessionStorage.setItem('drew:' + id, String(n));
    }).observe(document, { childList: true, subtree: true });
  });
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  const seen = () => page.evaluate(() => ({ loads: Number(sessionStorage.getItem('loads')), t2: sessionStorage.getItem('drew:t2'), new: sessionStorage.getItem('drew:m-new') }));

  let newer = 1;
  await page.route(/\/screens\/threads$/, async r => {
    const res = await r.fetch();
    if (newer-- <= 0) return r.fulfill({ response: res });
    await r.fulfill({ response: res, headers: { ...res.headers(), 'x-build': 'b' } });
  });
  await page.goto(url + '#threads');
  await until(() => !!document.querySelector('[data-id="t2"]'), 'the threads list after the reload');
  out.newer = await seen();

  await page.evaluate(() => document.querySelector('[data-id="t2"]').click());
  await until(() => !!document.querySelector('.cmp textarea'), 'the composer');
  await wait(1000);
  out.same = await seen();

  let send;
  const pushed = new Promise(r => { send = r; });
  let served;
  const done = new Promise(r => { served = r; });
  let streams = 0;
  await page.route('**/screens/threads/t2/stream', async r => {
    if (streams++ > 0) return r.fallback();
    await pushed;
    const card = { type: 'card', id: 'm-new', kind: 'orch', title: 'orchestrator', blocks: [{ type: 'text', id: 'm-new-t', markdown: 'from a newer build' }] };
    const ev = { seq: 99, build: 'b', ops: [{ op: 'insert', into: 'thread-t2', before: 'say', block: card }] };
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: `retry: 3600000\nid: 99\ndata: ${JSON.stringify(ev)}\n\n` });
    served();
  });
  await page.reload();
  await until(() => !!document.querySelector('.cmp textarea'), 'the composer again');
  await page.evaluate(() => document.querySelector('.cmp textarea').focus());
  await page.keyboard.type('half a thought');
  send();
  await done;
  await wait(1000);
  out.focused = { ...(await seen()), value: await page.evaluate(() => document.querySelector('.cmp textarea').value), focus: await page.evaluate(() => document.activeElement?.tagName) };
  await page.evaluate(() => document.activeElement.blur());
  await until(() => Number(sessionStorage.getItem('loads')) === 4 && !!document.querySelector('.cmp textarea'), 'the reload on blur');
  await wait(500);
  out.blurred = { ...(await seen()), value: await page.evaluate(() => document.querySelector('.cmp textarea').value), draft: await page.evaluate(() => localStorage.getItem('draft:thread-t2:say:text')) };
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
