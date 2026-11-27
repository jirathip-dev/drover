// #1312's phone-timings gate in Playwright's WebKit at 390 px on a real serve: node timings.mjs <url> <cookie name=value> <reps>.
// Each rep switches tabs (queue, lanes) and opens a lane's sheet;
// then the page leaves, so its batch of timings posts to serve. Prints one JSON object: every sample it posted, the steps done, errors.
import { createRequire } from 'node:module';

const pw = createRequire(import.meta.url)('playwright');
const [url, cookie, reps = '3'] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { steps: [], errors: [], batch: null };
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
  const shown = 'main section.tab:not([hidden])';
  const step = async (what, act, done, arg) => {
    await act();
    await until(done, what, arg);
    await wait(400);
    out.steps.push(what);
  };
  await page.goto(url);
  await until(() => document.querySelector('main section.tab:not([hidden]) .row'), 'first paint');
  for (let i = 0; i < Number(reps); i++) {
    await step('tab queue', () => page.click('nav a[href="#queue"]'), s => !!document.querySelector(`${s} .row[data-id^="q-"]`), shown);
    await step('tab lanes', () => page.click('nav a[href="#lanes"]'), s => !!document.querySelector(`${s} .row[data-id*="sendmeter-impl-"]`), shown);
    await step('lane sheet', () => page.click('main section.tab:not([hidden]) .row[data-id*="sendmeter-impl-"]'), () => !document.getElementById('sheet').hidden && /sendmeter-impl-/.test(document.querySelector('#sheet .sh b')?.textContent ?? ''));
    await page.keyboard.press('Escape');
  }
  await page.goto('about:blank');
  await wait(1000);
  out.batch = batches.flat();
} catch (e) {
  out.error = String(e);
} finally {
  await browser.close();
}
console.log(JSON.stringify(out));
