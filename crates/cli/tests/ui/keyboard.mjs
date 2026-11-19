// the working thread composer in Playwright's WebKit on a real serve, its stream live: node keyboard.mjs <url> <cookie name=value>.
// Types into thread t2 with a caret selection, prints `typed`, waits for a line on stdin (the test appends events then),
// waits for the stream to turn the composer's working pill off, and prints one JSON object of the textarea after it.
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';

const { webkit } = createRequire(import.meta.url)('playwright');
const [url, cookie] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const stdin = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const out = { errors: [] };
const browser = await webkit.launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const [name, value] = cookie.split(/=(.*)/);
  await ctx.addCookies([{ name, value, url }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  const until = async (fn, what) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  await page.goto(url + '#threads/t2');
  await until(() => !!document.querySelector('.cmp .pill') && document.getElementById('dot').classList.contains('g'), 'the working composer, its stream live');
  await page.evaluate(() => document.querySelector('.cmp textarea').focus());
  await page.keyboard.type('half a thought');
  await page.evaluate(() => {
    const i = document.querySelector('.cmp textarea');
    i.setSelectionRange(4, 6);
    window.kept = { i, blurs: 0 };
    i.addEventListener('blur', () => kept.blurs++);
  });
  console.log('typed');
  await stdin.next();
  await until(() => !document.querySelector('.cmp .pill'), 'the stream turning the working pill off');
  out.after = await page.evaluate(() => {
    const i = kept.i;
    return { working: !!document.querySelector('.cmp .pill'), same: document.querySelector('.cmp textarea') === i && i.isConnected, focus: document.activeElement === i, value: i.value, caret: [i.selectionStart, i.selectionEnd], blurs: kept.blurs };
  });
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
  process.exit(0);
}
