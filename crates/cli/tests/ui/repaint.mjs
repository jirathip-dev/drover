// #1169: thread t2 in Playwright's WebKit at 390 px on a real serve: node repaint.mjs <url> <cookie name=value>.
// localStorage is full of cached screens (a phone's), the composer is typed into and blurred, its messages scrolled mid-way;
// the stream sends 5 messages, a replace that changes nothing and a badge; then the 30 s tick, a resume and a reload.
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
  await page.clock.install();
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  let go;
  const pushed = new Promise(r => { go = r; });
  await page.route('**/screens/threads/t2/stream', async r => {
    await pushed;
    const card = n => ({ type: 'card', id: `m-x${n}`, kind: 'orch', title: 'orchestrator', blocks: [{ type: 'text', id: `m-x${n}-t`, markdown: `update ${n}` }] });
    const ops = [1, 2, 3, 4, 5].map(n => [{ op: 'insert', into: 'thread-t2', before: 'say', block: card(n) }]);
    ops.push([{ op: 'replace', id: 'm-x5', block: card(5) }], [{ op: 'badge', count: 7 }]);
    const body = ops.map((o, i) => `id: ${i + 1}\ndata: ${JSON.stringify({ seq: i + 1, ops: o })}\n\n`).join('');
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: 'retry: 3600000\n' + body });
  });
  const F = '.cmp textarea';
  const box = () => page.evaluate(F => {
    const i = document.querySelector(F);
    return { tag: i.tagName, value: i.value, same: i === kept.i && i.isConnected, removed: kept.removed, fl: kept.fl, top: Math.round(document.querySelector('.scr[data-id=thread-t2] > .msgs').scrollTop), nodes: kept.nodes.every(n => n.isConnected && document.querySelector(`[data-id="${n.dataset.id}"]`) === n) };
  }, F);

  await page.goto(url + '#threads');
  await until(() => !!document.querySelector('[data-id="t2"]'), 'the threads list');
  await page.evaluate(() => document.querySelector('[data-id="t2"]').click());
  await until(F => !!document.querySelector(F), 'the composer', F);
  out.full = await page.evaluate(() => {
    let n = 0;
    for (let size = 1 << 20; size >= 1; size >>= 1) {
      const s = 'x'.repeat(size);
      for (;;) {
        if (n > 200) return false;
        try { localStorage.setItem(`scr:/screens/threads/t${100 + n++}`, s); } catch { break; }
      }
    }
    return true;
  });
  await page.evaluate(F => { document.querySelector(F).focus(); document.querySelector('.scr[data-id=thread-t2] > .msgs').scrollTop = 400; }, F);
  await page.keyboard.type('half a thought');
  await page.evaluate(F => {
    const i = document.querySelector(F), sc = document.querySelector('.scr[data-id=thread-t2]');
    i.blur();
    window.kept = { i, removed: 0, fl: 0, nodes: [...sc.querySelectorAll(':scope > [data-id], :scope > .msgs > [data-id]')] };
    new MutationObserver(rs => {
      for (const r of rs) {
        if (r.type === 'childList') kept.removed += [...r.removedNodes].filter(n => n instanceof Element).length;
        else if (r.target.classList.contains('fl') && !(r.oldValue ?? '').split(' ').includes('fl')) kept.fl++;
      }
    }).observe(sc.parentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'], attributeOldValue: true });
  }, F);
  out.before = await box();
  go();
  await until(() => document.querySelectorAll('[data-id^="m-x"]').length === 5 && document.querySelector('nav').textContent.includes('·7'), 'the stream updates');
  out.stream = await box();
  await page.clock.runFor(30_000);
  out.tick = await box();
  await until(() => !document.getElementById('stale').hidden, 'the stream to end');
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await until(() => !document.querySelector('[data-id^="m-x"]'), 'the resume load');
  out.resumed = await box();
  await page.reload();
  await until(F => !!document.querySelector(F), 'the reloaded thread', F);
  out.reloaded = await page.evaluate(F => document.querySelector(F).value, F);
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
