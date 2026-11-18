// #1185: the cards in Playwright's WebKit at 390 px on a real serve whose answers stay in flight: node answered.mjs <url> <cookie name=value>.
// Approves the architect ask (q2), types a revise note on the design ask (q3), answers a plain ask (q1), then sends the note.
// Each step: the /action reply, whether the card is on screen 1 s later and after the page's own reload of the cards. Prints one JSON object.
import { createRequire } from 'node:module';

const { webkit } = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [], steps: {} };
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
  const asks = () => page.evaluate(() => [...document.querySelectorAll('#cards .card[data-id^="ask-"]')].map(c => c.dataset.id).sort());
  const step = async (id, tap) => {
    const posted = page.waitForResponse(r => r.url().endsWith('/action'));
    const reloaded = page.waitForResponse(r => r.request().method() === 'GET' && new URL(r.url()).pathname.endsWith('/screens/cards'));
    const t0 = await page.evaluate(() => Date.now());
    await tap();
    const status = (await posted).status();
    await page.clock.runFor(1000);
    const at1s = (await asks()).includes(id);
    await reloaded;
    await until(t => JSON.parse(localStorage.getItem('scr:/screens/cards') ?? '{"t":0}').t >= t, `the reloaded cards after ${id}`, t0 + 1000);
    const left = await asks();
    out.steps[id] = { status, at1s, after: left.includes(id), left };
  };
  const card = id => `#cards .card[data-id="${id}"]`;

  await page.goto(url);
  await until(() => document.querySelectorAll('#cards .card[data-id^="ask-"]').length === 3, 'the three ask cards');
  await page.evaluate(() => { window.kept = true; });
  out.before = await asks();

  await step('ask-q2', () => page.click(`${card('ask-q2')} .fold button`));

  await page.click(`${card('ask-q3')} .fold .fl`);
  await until(s => !!document.querySelector(s), 'the revise note box', `${card('ask-q3')} input[name=other]`);
  await page.click(`${card('ask-q3')} input[name=other]`);
  await page.keyboard.type('make the rows taller');
  const box = () => page.evaluate(s => { const i = document.querySelector(s); window.note ??= i; return { value: i?.value, same: i === window.note }; }, `${card('ask-q3')} input[name=other]`);
  out.typed = await box();

  await step('ask-q1', () => page.click(`${card('ask-q1')} .fold button`));
  out.note = await box();

  await step('ask-q3', () => page.click(`${card('ask-q3')} form.other button[type=submit]`));
  out.reloaded = !(await page.evaluate(() => window.kept === true));
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
