// #1315: optimistic send and answer in Playwright's WebKit at 390 px on a real serve: node optimistic.mjs <url> <cookie name=value>.
// Sends in thread t2 with its POST held: the message's time to the DOM and its pending style; released, how often it shows once the stream confirms it.
// Answers ask q1 with its POST held for 3 s while the cards' stream keeps sending the whole cards screen that still has it: the card's time to leave, whether it came back.
// Both streams relay serve's own screen, read again on each reconnect (every 100 ms), so no test shares serve's stream slots.
import { createRequire } from 'node:module';

const pw = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [], reloads: 0 };
const browser = await pw[process.env.DROVER_ENGINE ?? 'webkit'].launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(50)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  let deltas = 0, posted = 0;
  const relay = (path, n) => page.route(`**/screens/${path}/stream`, async r => {
    const screen = await (await r.fetch({ url: url + 'screens/' + path })).json();
    const seq = n();
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: `retry: 100\nid: ${seq}\ndata: ${JSON.stringify({ seq, ops: [{ op: 'screen', screen }] })}\n\n` });
  });
  let pushes = 0;
  await relay('cards', () => ++deltas);
  await relay('threads/t2', () => ++pushes);
  let release = () => {};
  await page.route('**/action', async r => { await new Promise(ok => { release = ok; }); posted++; await r.continue(); });
  await page.goto(url + '#threads');
  await until(() => !!document.querySelector('[data-id="t2"]'), 'the threads list');
  await page.evaluate(() => document.querySelector('[data-id="t2"]').click());
  await until(() => !!document.querySelector('.cmp textarea') && !!document.querySelector('#cards .card[data-id="ask-q1"]'), 'the composer and the ask card');
  page.on('request', r => { if (r.method() === 'GET' && /\/screens\/(threads\/t2|cards)$/.test(new URL(r.url()).pathname)) out.reloads++; });

  const text = 'an optimistic hello';
  await page.fill('.cmp textarea', text);
  const shows = t => [...document.querySelectorAll('.scr[data-id="thread-t2"] .card')].filter(c => c.querySelector('.txt')?.textContent === t);
  out.send = await page.evaluate(async ([t, f]) => {
    const find = new Function('t', `return (${f})(t)`);
    const t0 = performance.now();
    document.querySelector('.cmp .cr button.primary').click();
    for (;;) {
      const c = find(t)[0];
      if (c) return { ms: performance.now() - t0, pending: c.classList.contains('pending') };
      if (performance.now() - t0 > 2000) return { ms: null };
      await new Promise(r => setTimeout(r, 5));
    }
  }, [text, shows.toString()]);
  release();
  await until(([t, f]) => new Function('t', `return (${f})(t)`)(t).some(c => !c.dataset.id.startsWith('pend-')), 'the confirmed message', [text, shows.toString()]);
  await wait(1500);
  out.confirmed = await page.evaluate(([t, f]) => {
    const all = new Function('t', `return (${f})(t)`)(t);
    return { shown: all.length, pending: document.querySelectorAll('.card.pending, [data-id^="pend-"]').length };
  }, [text, shows.toString()]);

  const card = '#cards .card[data-id="ask-q1"]';
  const d0 = deltas;
  out.answer = await page.evaluate(async s => {
    const t0 = performance.now();
    document.querySelector(`${s} .fold button`).click();
    for (;;) {
      if (!document.querySelector(s)) return { ms: performance.now() - t0 };
      if (performance.now() - t0 > 2000) return { ms: null };
      await new Promise(r => setTimeout(r, 5));
    }
  }, card);
  let back = false;
  for (const t = Date.now(); Date.now() - t < 3000; await wait(50)) back ||= await page.evaluate(s => !!document.querySelector(s), card);
  out.answer.deltas = deltas - d0;
  out.answer.back = back;
  const res = page.waitForResponse(r => r.url().endsWith('/action'));
  release();
  out.answer.status = (await res).status();
  await wait(1000);
  out.answer.after = await page.evaluate(s => !!document.querySelector(s), card);
  out.posted = posted;
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
