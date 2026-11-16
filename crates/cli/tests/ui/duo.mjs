// #1118: the two-pane Threads tab in Playwright's WebKit at 1440×900 on a real serve: node duo.mjs <url> [shots dir].
// The wheel scrolls the thread to its top, then the list to its end; a stream update to the list lands while the composer
// holds a focused draft; a row click swaps the thread in place. With a shots dir: 1440 and 1000 px, light and dark.
// Prints one JSON object of what the page showed.
import { createRequire } from 'node:module';

const { webkit } = createRequire(import.meta.url)('playwright');
const [url, shots] = process.argv.slice(2);
const wait = ms => new Promise(r => setTimeout(r, ms));
const out = { errors: [] };
const browser = await webkit.launch();
try {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, serviceWorkers: 'block' });
  const page = await ctx.newPage();
  page.on('pageerror', e => out.errors.push(e.message));
  const until = async (fn, what, arg) => {
    for (const t = Date.now(); Date.now() - t < 15000; await wait(100)) if (await page.evaluate(fn, arg).catch(() => false)) return;
    throw new Error(`timed out: ${what}`);
  };
  const frame = () => page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
  let go;
  const pushed = new Promise(r => { go = r; });
  await page.route('**/screens/threads/stream', async r => {
    const row = await pushed;
    const ops = [{ op: 'replace', id: 't9', block: { ...row, value: 'pushed' } }];
    await r.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body: `retry: 3600000\nid: 1\ndata: ${JSON.stringify({ seq: 1, ops })}\n\n` });
  });
  const look = () => page.evaluate(() => {
    const l = document.querySelector('main>.lst'), sc = document.querySelector('.scr[data-id^=thread-] > .msgs'), b = document.querySelector('.cmp textarea');
    return {
      list: Math.round(l.scrollTop), listEnd: +(l.scrollTop > 0 && l.scrollTop + l.clientHeight >= l.scrollHeight - 2), thread: sc ? Math.round(sc.scrollTop) : -1,
      doc: document.scrollingElement.scrollTop, draft: b?.value, focus: +(!!b && document.activeElement === b), hash: location.hash,
      current: [...document.querySelectorAll('.lst .row[aria-current]')].map(r => r.dataset.id), back: +[...document.querySelectorAll('main button')].some(b => b.textContent.includes('back')),
      rows: document.querySelectorAll('.lst .row[data-id]').length, cards: document.querySelectorAll('.scr[data-id^=thread-] .card.k-orch').length,
    };
  });
  const wheel = async (sel, dy) => {
    const b = await page.locator(sel).boundingBox();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
    await page.mouse.wheel(0, dy);
    await wait(300);
    await frame();
  };

  await page.goto(url + '#threads/t2');
  await until(() => !!document.querySelector('.lst .row[data-id="t60"]') && !!document.querySelector('.scr[data-id=thread-t2] .cmp textarea'), 'both panes');
  await page.locator('.lst [data-id=older-sendmeter] .fold').click();
  await until(() => document.querySelectorAll('.lst .row[data-id]').length === 60, 'the older threads unfolded');
  await frame();
  out.open = await look();
  await wheel('main>.lst', 600);
  out.listed = await look();
  await wheel('.scr[data-id=thread-t2] > .msgs', -100000);
  await until(() => document.querySelectorAll('.scr[data-id^=thread-] .card.k-orch').length > 30 && !document.querySelector('[data-id=older][aria-busy]'), 'the older messages');
  await frame();
  out.top = await look();
  await wheel('main>.lst', 100000);
  out.end = await look();

  await page.locator('.cmp textarea').click();
  await page.keyboard.type('half a thought');
  out.typed = await look();
  go(await page.evaluate(async () => {
    const find = (v, id) => v && typeof v === 'object' ? (v.id === id ? v : Object.values(v).map(x => find(x, id)).find(Boolean)) : null;
    return find(await (await fetch('screens/threads')).json(), 't9');
  }));
  await until(() => document.querySelector('.lst .row[data-id="t9"] .rv')?.textContent === 'pushed', 'the list update');
  await frame();
  out.streamed = await look();

  await page.locator('.lst .row[data-id="t9"]').click();
  await until(() => !!document.querySelector('.scr[data-id=thread-t9] .cmp textarea'), 'thread t9');
  await frame();
  out.swapped = await look();
  out.panes = await page.evaluate(() => [document.querySelector('main>.lst').getBoundingClientRect().right, document.querySelector('.scr[data-id=thread-t9]').getBoundingClientRect().left].map(Math.round));

  await page.evaluate(() => { location.hash = '#threads'; });
  await until(() => !!document.querySelector('.scr[data-id=none] button'), 'the empty state');
  out.empty = { text: await page.locator('.scr[data-id=none]').innerText(), list: await page.evaluate(() => document.querySelectorAll('.lst .row[data-id]').length) };

  if (shots) {
    for (const [scheme, name] of [['light', 'garden-light'], ['dark', 'garden-dark']]) {
      for (const width of [1440, 1000]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(`${url}?scheme=${name}#threads/t2`);
        await until(() => !!document.querySelector('.scr[data-id=thread-t2] .cmp textarea'), 'thread t2 for the shot');
        await wait(300);
        await page.screenshot({ path: `${shots}/threads-${width}-${scheme}.png` });
      }
    }
  }
} catch (e) {
  out.error = String(e);
} finally {
  await browser.close();
}
console.log(JSON.stringify(out));
