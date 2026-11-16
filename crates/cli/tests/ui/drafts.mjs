// #1084: the Threads composer in Playwright's WebKit on a real serve: node drafts.mjs <url> <cookie name=value>.
// Types into thread t2 with its messages scrolled 400 px down (a message card mid-screen must not move on any change while the stream is live, nor on the tick; the route's stream then ends, and the page's offline strip showing is no repaint), pushes 5 stream ops and a 30 s tick (fake clock), reloads, fails one send (500)
// and retries it, then sends. Prints one JSON object of what the page showed at each step.
import { createRequire } from 'node:module';

const { webkit } = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [], posts: 0 };
const browser = await webkit.launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  await page.clock.install();
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  let typed;
  const pushed = new Promise(r => { typed = r; });
  await page.route('**/screens/threads/t2/stream', async r => {
    await pushed;
    const card = n => ({ type: 'card', id: `m-x${n}`, kind: 'orch', title: 'orchestrator', blocks: [{ type: 'text', id: `m-x${n}-t`, markdown: `update ${n}` }] });
    const body = [1, 2, 3, 4, 5].map(n => `id: ${n}\ndata: ${JSON.stringify({ seq: n, ops: [{ op: 'insert', into: 'thread-t2', before: 'say', block: card(n) }] })}\n\n`).join('');
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: 'retry: 3600000\n' + body });
  });
  const open = async () => {
    await until(() => !!document.querySelector('[data-id="t2"]'), 'the threads list');
    await page.evaluate(() => document.querySelector('[data-id="t2"]').click());
    await until(() => !!document.querySelector('.cmp textarea'), 'the composer');
  };
  const box = () => page.evaluate(() => {
    const i = document.querySelector('.cmp textarea');
    return { value: i.value, focus: document.activeElement === i, caret: [i.selectionStart, i.selectionEnd], draft: localStorage.getItem('draft:thread-t2:say:text') };
  });

  await page.goto(url + '#threads');
  await open();
  await page.evaluate(() => { const i = document.querySelector('.cmp textarea'); i.focus(); document.querySelector('.scr[data-id=thread-t2] > .msgs').scrollTop = 400; });
  await page.keyboard.type('half a thought');
  await page.evaluate(() => {
    const i = document.querySelector('.cmp textarea');
    i.setSelectionRange(4, 6);
    const a = document.elementFromPoint(195, 422).closest('.card');
    window.kept = { i, gone: 0, anchor: a.dataset.id, top: a.getBoundingClientRect().top };
    kept.live = [];
    new MutationObserver(rs => { if (document.getElementById('stale').hidden) kept.live.push([Math.round(document.querySelector('.scr[data-id=thread-t2] > .msgs').scrollTop), document.querySelector(`[data-id="${kept.anchor}"]`)?.getBoundingClientRect().top]); for (const r of rs) for (const n of r.removedNodes) if (n === i || n.contains?.(i)) kept.gone++; }).observe(document.body, { childList: true, subtree: true });
  });
  out.before = { ...(await box()), top: await page.evaluate(() => kept.top) };
  typed();
  await until(() => document.querySelectorAll('[data-id^="m-x"]').length === 5, 'the 5 stream updates');
  const view = () => page.evaluate(() => [Math.round(document.querySelector('.scr[data-id=thread-t2] > .msgs').scrollTop), document.querySelector(`[data-id="${kept.anchor}"]`).getBoundingClientRect().top]);
  const tick = [await view()];
  await page.clock.runFor(30_000);
  tick.push(await view());
  out.after = { ...(await box()), tick, same: await page.evaluate(() => document.querySelector('.cmp textarea') === kept.i && kept.i.isConnected), gone: await page.evaluate(() => kept.gone), live: await page.evaluate(() => kept.live) };

  await page.reload();
  await until(() => !!document.querySelector('.cmp textarea'), 'the reloaded thread');
  out.reloaded = { ...(await box()), hash: await page.evaluate(() => location.hash) };

  await page.route('**/action', r => { out.posts++; return r.fulfill({ status: 500, contentType: 'application/json', body: '{"line":"boom: not sent"}' }); });
  await page.evaluate(() => document.querySelector('.cmp textarea').focus());
  await page.keyboard.press('Control+Enter');
  await until(() => !document.getElementById('toast').hidden, 'the failed send toast');
  out.failed = { ...(await box()), toast: await page.evaluate(() => { const t = document.getElementById('toast'); return { red: t.classList.contains('r'), text: t.textContent, retry: [...t.querySelectorAll('button')].map(b => b.textContent) }; }) };
  await page.evaluate(() => [...document.querySelectorAll('#toast button')].find(b => b.textContent === 'retry').click());
  await until(() => !document.getElementById('toast').hidden, 'the retried send toast');
  out.retried = { posts: out.posts, ...(await box()) };

  await page.unroute('**/action');
  await page.evaluate(() => document.querySelector('.cmp textarea').focus());
  await page.keyboard.press('Control+Enter');
  await until(() => document.querySelector('.cmp textarea').value === '', 'the box to empty');
  await page.clock.runFor(1000);
  await until(() => [...document.querySelectorAll('.card')].some(c => c.textContent.includes('half a thought')), 'the sent message');
  out.sent = await box();
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
