// #370 phase 1: the fixtures and the gate-2 measurements, shot from the real board. Takes an exported demo
// (`herdsman serve --export <site> --demo`), copies it twice: `today/` as exported, `next/` with the proposed theme
// files (proposed/, from build.mjs) and proposal.js/.css loaded after serve.js/herd.js. Then: every tab × farm, kitchen,
// terminal × 390 (DPR 2) and 1440; the loops, Reduce Motion, the moments, the empty states, contrast; report.json.
// Usage: node build.mjs && node shoot.mjs <site dir> <chrome-headless-shell>
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chrome, serve, sleep } from './cdp.mjs';

const here = new URL('.', import.meta.url).pathname;
const [site, bin] = process.argv.slice(2);
const read = f => readFileSync(f, 'utf8');
const root = mkdtempSync(join(tmpdir(), 'd370-'));
for (const v of ['today', 'next']) cpSync(site, join(root, v), { recursive: true });
const walk = d => readdirSync(d).flatMap(f => statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]);
const hash = readdirSync(join(root, 'next/a'))[0];
for (const f of walk(join(root, 'next'))) {
  const m = f.match(/themes\/(farm|kitchen)\/(theme\.json|sprites\.svg)$/);
  if (m) cpSync(join(here, 'proposed', m[1], m[2]), f);
}
for (const f of ['proposal.js', 'proposal.css']) cpSync(join(here, f), join(root, 'next/a', hash, f));
{
  const f = join(root, 'next/index.html');
  let h = read(f);
  const swap = (re, to) => { if (!re.test(h)) throw new Error(`index.html changed: ${re}`); h = h.replace(re, to); };
  swap(/(<link rel=stylesheet href=a\/\w+\/herd\.css>)/, `$1<link rel=stylesheet href=a/${hash}/proposal.css>`);
  swap(/(<script src=a\/\w+\/herd\.js><\/script>)/, `$1<script src=a/${hash}/proposal.js></script>`);
  writeFileSync(f, h);
}

const out = join(here, 'shots');
rmSync(out, { recursive: true, force: true });
const parts = join(out, 'parts');
mkdirSync(parts, { recursive: true });
const s = await serve(root), c = chrome(bin);
setTimeout(() => { console.error('shoot: timed out'); c.kill(); process.exit(1); }, 600000).unref();
const THEMES = ['farm', 'kitchen', 'terminal'], TABS = ['queue', 'lanes', 'host', 'events'];
const READY = `document.readyState === 'complete' && typeof TT !== 'undefined' && !!TT && (TT.row_sprites === false || !!document.getElementById('hr-queued') || !document.querySelector('.rsp'))`;
const open = (v, theme, tab, W, dpr, o = {}) => c.open(`${s.base}/${v}/index.html?theme=${theme}#${tab}`, W, W < 720 ? 844 : 900, dpr, { ready: READY, ...o });
// Framing: the demo banner goes; the needs-you cards stay on Queue (themed there) and leave the other tabs, so each
// shot shows its tab. Nothing is restyled.
const FRAME = tab => `tab('${tab}'); document.querySelector('.msg.a')?.remove(); ${tab === 'queue' ? '' : "document.querySelector('#needs')?.replaceChildren();"} scrollTo(0, 0);`;
// A /live swap of the tab's part: frag() with the section's server HTML (tidy, then morph into the live tree).
const PATCH = tab => `fetch(location.pathname).then(r => r.text()).then(t => { const d = new DOMParser().parseFromString(t, 'text/html'); frag('${tab}', d.getElementById('${tab}').innerHTML); return 1; })`;
const height = p => p.ev('Math.max(document.documentElement.scrollHeight, document.body.scrollHeight)');
const report = { fixtures: [], terminal: {}, loops: {}, reduce: {}, moments: {}, contrast: {}, labels: {} };

// 1. Every tab, every theme, both widths (next); terminal also from today, which must match pixel for pixel.
for (const theme of THEMES) for (const tab of TABS) for (const [W, dpr] of [[390, 2], [1440, 1]]) {
  const p = await open('next', theme, tab, W, dpr);
  await p.ev(FRAME(tab));
  await sleep(700);
  // a still frame for the PNG (frame 1, no moment): the loops are measured below
  await p.ev(`document.getAnimations().forEach(a => { a.currentTime = 0; a.pause(); })`);
  const name = `${theme}-${tab}-${W}.png`, png = await p.full();
  writeFileSync(join(out, name), png);
  report.fixtures.push(name);
  if (W === 390) report.labels[`${theme} ${tab}`] = await p.ev(`[...new Set([...document.querySelectorAll('section#${tab} .tw, section#${tab} .tsub, #needs .tw')].filter(e => e.checkVisibility()).map(e => (e.closest('.tsub, h2, summary>span, .sm, p.m, .ask>p, .lsm') || e).textContent.replace(/\\s+/g, ' ').trim().slice(0, 60)))].slice(0, 14)`);
  await p.done();
  if (theme === 'terminal') {
    const q = await open('today', theme, tab, W, dpr);
    await q.ev(FRAME(tab));
    await sleep(700);
    await q.ev(`document.getAnimations().forEach(a => { a.currentTime = 0; a.pause(); })`);
    let same = (await q.full()).equals(png), again = false;
    // a difference is shot once more, both sides (a page still settling at 700 ms), and reported as such
    if (!same) {
      again = true;
      const r = await open('next', theme, tab, W, dpr);
      for (const x of [q, r]) { await x.ev(FRAME(tab)); }
      await sleep(1500);
      for (const x of [q, r]) await x.ev(`document.getAnimations().forEach(a => { a.currentTime = 0; a.pause(); })`);
      same = (await q.full()).equals(await r.full());
      await r.done();
    }
    report.terminal[`${tab} ${W}`] = same ? `identical to today${again ? ' (on a second shot)' : ''}` : 'DIFFERS from today';
    await q.done();
  }
}

// 2. The loops: a row sprite changes frame between two captures (≤ 0.6 s apart, a /live patch between), the same node;
// Reduce Motion: frame 1, still.
for (const theme of ['farm', 'kitchen']) for (const reduce of [false, true]) for (const tab of ['queue', 'events']) {
  const p = await open('next', theme, tab, 390, 2, { reduce });
  await p.ev(`${FRAME(tab)} window.__g = document.querySelector('section#${tab} .rsp'); __g.scrollIntoView({ block: 'center' }); 1`);
  await sleep(300);
  const box = await p.ev(`(() => { const r = __g.getBoundingClientRect(); return { x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height }; })()`);
  const shot = async () => ({ png: await p.png(box), o1: await p.ev(`getComputedStyle(__g).getPropertyValue('--o1').trim() || '1'`), same: await p.ev(`__g === document.querySelector('section#${tab} .rsp') && __g.isConnected`) });
  const a = await shot(), t0 = Date.now();
  await p.ev(PATCH(tab));
  // the next frame: --o1 flips within a step (0.6 s); under Reduce Motion it never does (waited 1.3 s)
  while (Date.now() - t0 < 1300 && await p.ev(`getComputedStyle(__g).getPropertyValue('--o1').trim() || '1'`) === a.o1) await sleep(25);
  const ms = Date.now() - t0, b = await shot();
  const r = { frames: `${a.o1} → ${b.o1}`, afterMs: ms, pixelsChanged: !a.png.equals(b.png), sameNodeAcrossPatch: a.same && b.same };
  (reduce ? report.reduce : report.loops)[`${theme} ${tab}`] = r;
  if (!reduce && tab === 'queue') for (const [k, x] of [['a', a], ['b', b]]) writeFileSync(join(parts, `loop-${theme}-${k}.png`), x.png);
  await p.done();
}

// 3. Moments on the row: merged (Queue), FAIL (Events), needs-you (Queue); each once per seq. Shot mid-moment.
const MOMENTS = [['merged', 'queue', 'acme-web', 38], ['fail', 'events', 'acme-api', 22], ['needs-you', 'queue', 'acme-api', 21]];
for (const theme of ['farm', 'kitchen']) for (const reduce of [false, true]) for (const [k, tab, fleet, n] of MOMENTS) {
  const p = await open('next', theme, tab, 390, 2, { reduce });
  await p.ev(`${FRAME(tab)} document.querySelector('#needs')?.replaceChildren(); 1`);
  const first = await p.ev(`rowMoment('${k}', '${fleet}', ${n}, 9001)`), second = await p.ev(`rowMoment('${k}', '${fleet}', ${n}, 9001)`);
  await sleep(1000);
  // a /live swap of the part mid-moment (at ~1.15 s): the moment resumes where it was (~1150 ms in), never restarts
  await p.ev(PATCH(tab));
  await sleep(150);
  const at = await p.ev(`Math.round(Math.max(0, ...document.getAnimations().filter(a => /^mo-/.test(a.animationName)).map(a => a.effect.getComputedTiming().localTime - a.effect.getTiming().delay)))`);
  const row = await p.ev(`(() => { const e = document.querySelector('[data-mo]'); if (!e) return null; e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return { x: 0, y: Math.max(0, r.y + scrollY - 4), width: innerWidth, height: r.height + 8 }; })()`);
  if (row) writeFileSync(join(parts, `mo-${theme}-${k}${reduce ? '-still' : ''}.png`), await p.png(row));
  await sleep(1700);
  report.moments[`${theme} ${k}${reduce ? ' (reduce)' : ''}`] = { first, second, msIntoMomentAfterPatch: reduce ? 'still' : at, endedAfter2_4s: await p.ev(`!document.querySelector('[data-mo]')`) };
  await p.done();
}

// 4. Empty states: the server's plain line (Queue `no open work`, Lanes `no agents`, Events `no events`) under its theme line.
const EMPTY = { queue: `document.querySelectorAll('#queue>*:not(.tsub)').forEach(e => e.remove()); document.querySelector('#queue').insertAdjacentHTML('beforeend', '<p class=m>no open work</p>')`,
  lanes: `document.querySelectorAll('#lanes>*:not(.tsub)').forEach(e => e.remove()); document.querySelector('#lanes').insertAdjacentHTML('beforeend', '<p class=m>no agents</p>')`,
  events: `document.querySelectorAll('#events .eg, #events .es').forEach(e => e.remove()); document.querySelector('#events [data-v=imp]').insertAdjacentHTML('beforeend', '<p class=m>no events</p>')` };
for (const theme of THEMES) for (const tab of Object.keys(EMPTY)) {
  const p = await open('next', theme, tab, 390, 2);
  await p.ev(`${FRAME(tab)} document.querySelector('#needs')?.replaceChildren(); ${EMPTY[tab]}; rows(document); 1`);
  await sleep(300);
  writeFileSync(join(parts, `empty-${theme}-${tab}.png`), await p.png({ height: 300 }));
  await p.done();
}
// the loading overlay with its word
for (const theme of THEMES) {
  const p = await open('next', theme, 'queue', 390, 2);
  await p.ev(`${FRAME('queue')} loading(true); 1`);
  await sleep(400);
  await p.ev(`document.getAnimations().forEach(a => { a.currentTime = 0; a.pause(); })`);
  writeFileSync(join(parts, `loading-${theme}.png`), await p.png({ height: 320 }));
  await p.done();
}

// 5. Contrast: every visible plain word (.pw) on what is really under it (the backgrounds composited up the tree).
const CONTRAST = `(() => {
  const rgb = s => { const m = s.match(/[\\d.]+/g).map(Number); return [m[0], m[1], m[2], m[3] ?? 1]; };
  const over = (a, b) => [0, 1, 2].map(i => a[i] * a[3] + b[i] * (1 - a[3])).concat(1);
  const bg = e => { const st = []; for (let x = e; x; x = x.parentElement) st.push(rgb(getComputedStyle(x).backgroundColor)); return st.reverse().reduce((b, a) => over(a, b), rgb(getComputedStyle(document.documentElement).getPropertyValue('--bg').trim().replace(/^#(..)(..)(..)$/, (m, r, g, b) => 'rgb(' + [r, g, b].map(h => parseInt(h, 16)) + ')'))); };
  const L = c => { const [r, g, b] = c.slice(0, 3).map(v => (v /= 255) <= .03928 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4); return .2126 * r + .7152 * g + .0722 * b; };
  const ratio = (a, b) => { const [x, y] = [L(a), L(b)].sort((p, q) => q - p); return (x + .05) / (y + .05); };
  const all = [...document.querySelectorAll('.pw')].filter(e => e.checkVisibility()).map(e => ({ t: e.textContent.trim().slice(0, 30), r: ratio(rgb(getComputedStyle(e).color), bg(e)) }));
  all.sort((a, b) => a.r - b.r);
  return { count: all.length, min: all[0] && +all[0].r.toFixed(2), worst: all[0] && all[0].t };
})()`;
for (const theme of ['farm', 'kitchen']) for (const tab of TABS) {
  const p = await open('next', theme, tab, 390, 2);
  await p.ev(FRAME(tab));
  await sleep(500);
  report.contrast[`${theme} ${tab}`] = await p.ev(CONTRAST);
  await p.done();
}
// the needs-you cards' muted words, on their 15 % red
for (const theme of ['farm', 'kitchen']) {
  const p = await open('next', theme, 'queue', 390, 2);
  await p.ev(FRAME('queue'));
  await sleep(500);
  report.contrast[`${theme} needs cards`] = await p.ev(CONTRAST.replace("document.querySelectorAll('.pw')", "document.querySelectorAll('#needs .pw')"));
  await p.done();
}

// 6. The sheet (sheet.html: words, loops, minis, moments, empty states, loading, push), served from this folder.
const d = await serve(here);
for (const [W, dpr] of [[390, 2], [1440, 1]]) {
  const p = await c.open(`${d.base}/sheet.html?still=1`, W, 900, dpr, { ready: 'window.sheetReady === true' });
  writeFileSync(join(here, `sheet-${W}.png`), await p.png({ height: await height(p) }));
  await p.done();
}
d.close();
report.errors = c.errors;
writeFileSync(join(here, 'report.json'), JSON.stringify(report, null, 1) + '\n');
console.log(JSON.stringify({ terminal: report.terminal, loops: report.loops, reduce: report.reduce, moments: report.moments, contrast: report.contrast, errors: c.errors }, null, 1));
c.kill();
s.close();
