// #1298: a thread whose stream is refused, in Playwright's WebKit at 390 px on a real serve: node revive.mjs <url> <cookie name=value>.
// Opens thread t2 with its first stream answered 503, then asks the test to log a message (`log <text>`) and times it to the screen.
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';

const pw = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const out = { errors: [], refused: 0 };
const browser = await pw[process.env.DROVER_ENGINE ?? 'webkit'].launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  const stream = u => new URL(u).pathname.endsWith('/screens/threads/t2/stream');
  await page.route(stream, async r => {
    out.refused++;
    await r.fulfill({ status: 503, contentType: 'application/json', body: '{"line":"no stream"}' });
    await page.unroute(stream);
  });
  await page.goto(url + '#threads/t2');
  for (const t = Date.now(); !out.refused && Date.now() - t < 15000; ) await wait(50);
  await wait(200);
  const text = 'logged after the refused stream';
  console.log(`log ${text}`);
  await lines.next();
  const t0 = Date.now();
  const shown = () => page.evaluate(t => !!document.querySelector('section.tab:not([hidden])')?.textContent.includes(t), text);
  while (Date.now() - t0 < 10000 && !(await shown())) await wait(50);
  out.ms = (await shown()) ? Date.now() - t0 : null;
  for (const t = Date.now(); Date.now() - t < 5000 && (await page.$eval('#dot', d => d.className.split(' ')[0])) !== 'g'; ) await wait(50);
  out.dot = await page.$eval('#dot', d => d.className.split(' ')[0]);
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
  process.exit(0);
}
