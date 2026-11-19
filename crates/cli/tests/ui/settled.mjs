// #1393: settled owner cards in Playwright's WebKit at 390 px on a real serve: node settled.mjs <url> <cookie name=value>.
// On the Threads list taps, in the cards strip, [discard] on config c2, [apply] on c1, [deny] on run card "run one" and [dismiss] on secret API_KEY.
// Then asks the test for c3 and c4 (`need c3`), opens thread t1, types a draft, taps [discard] on c3 in the thread and asks the test to settle
// c4 and "run two" in the terminal (`need settle`). Each step: the /action status and the ms until the card has no button left in the strip
// or the thread. Prints one JSON object last.
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';

const pw = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const need = async what => { console.log(`need ${what}`); await lines.next(); };
const out = { errors: [], steps: {} };
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
  const open = t => page.evaluate(t => ['#cards', 'section.tab:not([hidden])'].map(s =>
    [...document.querySelectorAll(`${s} .card`)].some(c => c.textContent.includes(t) && c.querySelector('button:not(.cpy):not([data-id$="-later"])'))), t);
  const gone = async t => {
    const t0 = Date.now();
    for (; Date.now() - t0 < 5000; await wait(25)) if (!(await open(t)).some(Boolean)) return Date.now() - t0;
    return null;
  };
  const tap = async (at, t, label) => {
    const posted = page.waitForResponse(r => r.url().endsWith('/action'), { timeout: 5000 }).then(r => r.status(), e => String(e.message).split('\n')[0]);
    await page.locator(`${at} .card`, { hasText: t }).last().locator('button', { hasText: new RegExp(`^${label}$`) }).first().click();
    const ms = await gone(t);
    out.steps[t] = { status: await posted, ms, after: await open(t) };
  };
  const shown = ts => until(ts => ts.every(t => [...document.querySelectorAll('#cards .card')].some(c => c.textContent.includes(t))), ts.join(', '), ts);

  await page.goto(url + '#threads');
  await shown(['config proposal c1', 'config proposal c2', 'run one', 'run two', 'secret API_KEY']);
  await page.evaluate(() => { window.kept = true; });
  await tap('#cards', 'config proposal c2', 'discard');
  await tap('#cards', 'config proposal c1', 'apply');
  await tap('#cards', 'run one', 'deny');
  await tap('#cards', 'secret API_KEY', 'dismiss');

  await need('c3');
  await page.evaluate(() => { location.hash = '#threads/t1'; });
  await until(() => !!document.querySelector('section.tab:not([hidden]) .cmp textarea'), 'the composer');
  await shown(['config proposal c3', 'config proposal c4']);
  out.before = await open('config proposal c3');
  await page.click('section.tab:not([hidden]) .cmp textarea');
  await page.keyboard.type('half a thought');
  await page.evaluate(() => { window.box = document.querySelector('section.tab:not([hidden]) .cmp textarea'); });
  await tap('section.tab:not([hidden])', 'config proposal c3', 'discard');
  await need('settle');
  const [c4, r2] = await Promise.all([gone('config proposal c4'), gone('run two')]);
  out.steps.terminal = { c4, r2 };
  out.draft = await page.evaluate(() => { const i = document.querySelector('section.tab:not([hidden]) .cmp textarea'); return { value: i?.value, same: i === window.box }; });
  out.reloaded = !(await page.evaluate(() => window.kept === true));
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
  process.exit(0);
}
