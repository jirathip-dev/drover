// #1316 in Playwright's WebKit at 390 px on a real serve: node pages.mjs <url> <cookie name=value>.
// Times 12 switches to the Events tab (tap to fresh rows painted, the page's own tab spans), then scrolls it to its end:
// the next page appends with the scroll position unchanged, until every row is there once. Prints one JSON object.
import { createRequire } from 'node:module';

const pw = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [], paints: [] };
const browser = await pw[process.env.DROVER_ENGINE ?? 'webkit'].launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  const batches = [];
  page.on('request', r => { if (r.url().endsWith('/api/v1/timings') && r.method() === 'POST') batches.push(JSON.parse(r.postData() ?? '[]')); });
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(50)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  const rows = 'main section.tab:not([hidden]) .row[data-id^="e-"]';
  await page.goto(url + '#queue');
  await until(() => document.querySelector('main section.tab:not([hidden]) .row'), 'first paint');
  for (let i = 0; i < 12; i++) {
    await page.click('nav a[href="#events"]');
    await until(s => document.querySelectorAll(s).length >= 100, 'events rows', rows);
    await wait(300);
    await page.click('nav a[href="#queue"]');
    await until(() => document.querySelector('main section.tab:not([hidden]) .row[data-id^="q-"]'), 'queue rows');
    await wait(300);
  }
  await page.click('nav a[href="#events"]');
  await until(s => document.querySelectorAll(s).length >= 100, 'events rows', rows);
  await wait(500);
  out.first = await page.evaluate(s => document.querySelectorAll(s).length, rows);
  const at = await page.evaluate(s => {
    const r = document.querySelectorAll(s)[60];
    scrollTo(0, document.scrollingElement.scrollHeight);
    return { y: scrollY, id: r.dataset.id, top: r.getBoundingClientRect().top };
  }, rows);
  await until(s => document.querySelectorAll(s).length >= 200, 'the second page', rows);
  out.after = await page.evaluate(s => document.querySelectorAll(s).length, rows);
  out.moved = await page.evaluate(a => [scrollY - a.y, document.querySelector(`[data-id="${a.id}"]`).getBoundingClientRect().top - a.top].map(Math.round), at);
  for (let n = 0; n < 10; n++) {
    await page.evaluate(() => scrollTo(0, document.scrollingElement.scrollHeight));
    await wait(400);
  }
  const ids = await page.evaluate(s => [...document.querySelectorAll(s)].map(r => r.dataset.id), rows);
  [out.total, out.unique] = [ids.length, new Set(ids).size];
  await page.evaluate(() => dispatchEvent(new Event('pagehide')));
  await wait(1000);
  const tabs = batches.flat().filter(t => t[0] === 'tab').map(t => t[1]);
  out.paints = tabs.filter((_, i) => i % 2 === 0);
} catch (e) {
  out.error = String(e);
} finally {
  await browser.close();
}
console.log(JSON.stringify(out));
