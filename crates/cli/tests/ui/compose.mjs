// #1219: thread t2 in Playwright's WebKit at 390 px on a real serve: node compose.mjs <url> <cookie name=value>.
// The composer is focused and typed into, a range selected; the stream then starts a turn (working) and ends it with a new
// orchestrator message, as serve's diff sends them. Prints one JSON object: focus, caret, text, blurs, then a key typed.
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
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  let go;
  const pushed = new Promise(r => { go = r; });
  await page.route('**/screens/threads/t2/stream', async r => {
    await pushed;
    const card = { type: 'card', id: 'm-reply', kind: 'orch', title: 'orchestrator', blocks: [{ type: 'text', id: 'm-reply-t', markdown: 'a reply' }] };
    const ops = [
      [{ op: 'merge', id: 'say', fields: { working: { since: new Date().toISOString() } } }],
      [{ op: 'insert', into: 'thread-t2', before: 'say', block: card }, { op: 'merge', id: 'say', fields: { working: null } }],
    ];
    const body = ops.map((o, i) => `id: ${i + 1}\ndata: ${JSON.stringify({ seq: i + 1, ops: o })}\n\n`).join('');
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: 'retry: 3600000\n' + body });
  });
  const F = '.cmp textarea';
  const box = () => page.evaluate(F => {
    const i = document.querySelector(F);
    return { value: i.value, focus: document.activeElement === i, same: i === kept.i, caret: [i.selectionStart, i.selectionEnd], blurs: kept.blurs };
  }, F);

  await page.goto(url + '#threads');
  await until(() => !!document.querySelector('[data-id="t2"]'), 'the threads list');
  await page.evaluate(() => document.querySelector('[data-id="t2"]').click());
  await until(F => !!document.querySelector(F), 'the composer', F);
  await page.evaluate(F => document.querySelector(F).focus(), F);
  await page.keyboard.type('half a thought');
  await page.evaluate(F => {
    const i = document.querySelector(F);
    i.setSelectionRange(4, 6);
    window.kept = { i, blurs: 0 };
    i.addEventListener('blur', () => kept.blurs++);
  }, F);
  out.before = await box();
  go();
  await until(() => !!document.querySelector('[data-id="m-reply"]') && !document.querySelector('.cmp .pill'), 'the reply');
  out.after = await box();
  await page.keyboard.type('!');
  out.typed = await page.evaluate(F => document.querySelector(F).value, F);
} catch (e) {
  out.error = String(e.stack ?? e);
} finally {
  console.log(JSON.stringify(out));
  await browser.close();
}
