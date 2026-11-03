// #244 phase 1: the contact sheet, shot from the real board. Takes an exported demo (`serve --export <site> --demo`
// plus scripts/host-fixtures.sh), rewrites its serve.css the way phase 2 will (the colour tokens leave the first :root
// block, every raw colour becomes a token; serve.css.diff), inlines one scheme's token block first in <head> (as
// serve.rs will, so there is no flash) and screenshots Queue and Host at 390×900, DPR 2.
// No deps: Chrome's --remote-debugging-pipe, like scripts/ui-close.mjs.
// Usage: node shoot.mjs <site dir> <chrome-headless-shell> [out dir = this folder]
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';
import { check, load } from './check.mjs';

const here = new URL('.', import.meta.url).pathname;
const [site, chrome, out = here] = process.argv.slice(2);
const css = readFileSync(join(here, '../../../assets/serve.css'), 'utf8');

// The phase-2 serve.css change, applied to a copy. Each pair is (today, proposed); every one must match.
export const SWAPS = [
  [/--bg:#0d1117;--fg:#c9d1d9;--mute:#8b949e;--line:#21262d;--g:#3fb950;--a:#d29922;--r:#f85149;--l:#58a6ff;--p:#bc8cff;--c:#39c5cf;color-scheme:dark;\n  /, ''],
  [/#30363d/g, 'var(--edge)'],
  [/#161b22/g, 'var(--card)'],
  [/#000a/g, 'var(--scrim)'],
  [/^a\{color:var\(--l\)\}/m, 'a{color:var(--link)}'],
  [/\.fc\{--fk:var\(--l\);/, '.fc{--fk:var(--fk-l);'],
  [/\.k-l\{--fk:var\(--l\)\}\.k-p\{--fk:var\(--p\)\}\.k-c\{--fk:var\(--c\)\}\.k-g\{--fk:var\(--g\)\}\.k-a\{--fk:var\(--a\)\}/,
    '.k-l{--fk:var(--fk-l)}.k-p{--fk:var(--fk-p)}.k-c{--fk:var(--fk-c)}.k-g{--fk:var(--fk-g)}.k-a{--fk:var(--fk-a)}'],
];
let patched = css;
for (const [re, to] of SWAPS) {
  if (!re.test(patched)) throw new Error(`serve.css changed: ${re} no longer matches`);
  re.lastIndex = 0;
  patched = patched.replace(re, to);
}
// No raw colour may be left outside the scheme (comments blanked first, line numbers kept): phase 2's lint, here.
const raw = patched.replace(/\/\*[\s\S]*?\*\//g, m => m.replace(/[^\n]/g, ' ')).split('\n')
  .flatMap((l, i) => /#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i.test(l) ? [`serve.css:${i + 1} ${l.match(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i)[0]}`] : []);
if (raw.length) throw new Error(`raw colours left after the swaps: ${raw.join(' ')}`);

// A scheme's token block, as the head script (phase 2) writes it before the first paint.
const block = s => `:root{${Object.entries(s.tokens).map(([k, v]) => `${k}:${v}`).join(';')};color-scheme:${s['color-scheme']}}`;

// Two copies of the site: `today` untouched (the pixel-identity baseline), `next` with the proposed serve.css.
const root = mkdtempSync(join(tmpdir(), 'schemes-'));
for (const v of ['today', 'next']) cpSync(site, join(root, v), { recursive: true });
const walk = d => readdirSync(d).flatMap(f => statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]);
for (const f of walk(join(root, 'next')).filter(f => f.endsWith('/serve.css'))) writeFileSync(f, patched);
// The exact phase-2 serve.css change, for review.
writeFileSync(join(root, 'serve.css'), patched);
writeFileSync(join(out, 'serve.css.diff'), spawnSync('diff', ['-u', '--label', 'a/assets/serve.css', '--label', 'b/assets/serve.css',
  join(here, '../../../assets/serve.css'), join(root, 'serve.css')], { encoding: 'utf8' }).stdout);

const TYPES = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml' };
// `?scheme=<name>` inlines that scheme's block first in <head>, as serve.rs will (phase 2): no flash, no extra request.
const schemes = load(join(here, 'schemes'));
const srv = createServer((q, r) => {
  const u = new URL(q.url, 'http://x'), p = join(root, decodeURIComponent(u.pathname)), s = schemes.find(s => s.name === u.searchParams.get('scheme'));
  try {
    let body = readFileSync(p);
    if (s && p.endsWith('.html')) body = body.toString().replace('<head>', `<head><style id=scheme>${block(s)}</style>`);
    r.writeHead(200, { 'content-type': TYPES[extname(p)] || 'application/octet-stream' }).end(body);
  } catch { r.writeHead(404).end(); }
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.once('listening', ok));
const base = `http://127.0.0.1:${srv.address().port}`;

const c = spawn(chrome, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${mkdtempSync(join(tmpdir(), 'schemes-prof-'))}`,
  '--no-first-run'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let id = 0, buf = '';
const wait = new Map(), on = [];
c.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); } else on.forEach(f => f(m));
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
  wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
  c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
setTimeout(() => { console.error('shoot: timed out'); c.kill(); process.exit(1); }, 300000).unref();

// Loaded: the document complete with its board drawn, then fonts and a frame settled.
async function ready(cmd) {
  const ev = expression => cmd('Runtime.evaluate', { expression, returnByValue: true }).then(r => r.result.value);
  for (let i = 0; i < 100; i++, await sleep(100))
    if (await ev(`document.readyState === 'complete' && document.body?.innerHTML.length > 1000`)) break;
  await ev('document.fonts.ready.then(() => 1)');
  await sleep(500);
}

// One page load at 390×H DPR 2 (the tab bar at the shot's foot) (`scheme` names the token block the server inlines), then a 390×H shot from the top.
async function shot(path, scheme, H = 844, prep = '') {
  // A fresh browser context per shot: no service worker or localStorage carried over from the last page.
  const { browserContextId } = await send('Target.createBrowserContext');
  const { targetId } = await send('Target.createTarget', { url: 'about:blank', browserContextId });
  const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cmd = (m, p) => send(m, p, s);
  await cmd('Page.enable');
  await cmd('Emulation.setDeviceMetricsOverride', { width: 390, height: H, deviceScaleFactor: 2, mobile: false });
  const [file, hash] = path.split('#');
  await cmd('Page.navigate', { url: `${base}/${file}${scheme ? `?scheme=${scheme}` : ''}#${hash}` });
  await ready(cmd);
  // The #tab hash scrolls to its section; shots start at the top, so the fixed tab bar sits at the foot.
  await cmd('Runtime.evaluate', { expression: `${prep};scrollTo(0, 0)` });
  await sleep(300);
  const { data } = await cmd('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 390, height: H, scale: 1 }, captureBeyondViewport: true });
  await send('Target.closeTarget', { targetId });
  await send('Target.disposeBrowserContext', { browserContextId });
  return Buffer.from(data, 'base64');
}

// Framing only (no style is touched): the demo banner goes; Queue keeps the first needs-you card above its rows,
// Host drops them so its groups (the red load bar first) fill the screen.
const VIEWS = { queue: 'index.html#queue', host: 'busy/index.html#host' };
const PREP = {
  queue: `document.querySelector('.msg.a')?.remove(); [...document.querySelectorAll('#needs > *')].slice(1).forEach(e => e.remove())`,
  host: `document.querySelector('.msg.a')?.remove(); document.querySelector('#needs')?.replaceChildren()`,
};
const H = { queue: 900, host: 900 };
const report = [];
for (const s of schemes) {
  const r = check(s);
  report.push({ name: s.name, label: s.label, ok: r.ok, fails: r.fails, rows: r.rows.map(([k, v, m, ok]) => [k, +v.toFixed(2), m, ok]) });
  for (const [v, p] of Object.entries(VIEWS)) writeFileSync(join(out, `${s.name}-${v}-390.png`), await shot(`next/${p}`, s.name, H[v], PREP[v]));
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${s.name}`);
}
// Gate 2's pixel identity: today's board vs the proposed CSS under github-dark, both views.
const gh = schemes.find(s => s.name === 'github-dark');
const same = {};
for (const [v, p] of Object.entries(VIEWS)) {
  const a = await shot(`today/${p}`, null, H[v], PREP[v]), b = await shot(`next/${p}`, gh.name, H[v], PREP[v]);
  same[v] = a.equals(b);
  console.log(`github-dark ${v}: ${same[v] ? 'pixel-identical to today' : 'DIFFERS from today'}`);
}
writeFileSync(join(out, 'report.json'), JSON.stringify({ identical: same, schemes: report }, null, 1) + '\n');
writeFileSync(join(out, 'index.html'), sheet(schemes, report, same));

// The contact sheet itself: index.html shot at 390 (a scheme a row, its two phones at half size) and 1640 (two a row).
for (const [w, name] of [[390, 'contact-390.png'], [1640, 'contact-1640.png']]) {
  cpSync(out, join(root, 'sheet'), { recursive: true });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cmd = (m, p) => send(m, p, s);
  await cmd('Page.enable');
  await cmd('Emulation.setDeviceMetricsOverride', { width: w, height: 900, deviceScaleFactor: w === 390 ? 2 : 1, mobile: w === 390 });
  await cmd('Page.navigate', { url: `${base}/sheet/index.html` });
  await ready(cmd);
  const { cssContentSize } = await cmd('Page.getLayoutMetrics');
  const { data } = await cmd('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true,
    clip: { x: 0, y: 0, width: w, height: Math.ceil(cssContentSize.height), scale: 1 } });
  writeFileSync(join(out, name), Buffer.from(data, 'base64'));
  await send('Target.closeTarget', { targetId });
}
c.kill();
srv.close();

// index.html: per scheme its name, gate result, swatches and the tightest numbers, then Queue and Host side by side.
// A docs page, not the board: its own few colours are github-dark's.
function sheet(schemes, report, same) {
  const esc = t => String(t).replace(/[&<>"]/g, ch => `&#${ch.charCodeAt(0)};`);
  const SW = ['--bg', '--card', '--edge', '--line', '--fg', '--mute', '--l', '--link', '--g', '--a', '--r', '--p', '--c'];
  const min = (r, re) => r.rows.filter(([k]) => re.test(k)).reduce((m, x) => x[1] / x[2] < m[1] / m[2] ? x : m);
  const card = s => {
    const r = report.find(x => x.name === s.name);
    const nums = [['text', min(r, /^--(fg|mute) on/)], ['accent', min(r, /^--(l|link) on/)], ['status', min(r, /^--[gar] on --(bg|card)$/)],
      ['status ΔE', min(r, /^ΔE2000 --[gar] ↔/)], ['accent ΔE', min(r, /^ΔE2000 --(l|link) ↔/)]]
      .map(([n, [k, v, m]]) => `<span title="${esc(k)} (min ${m})">${n} <b>${v.toFixed(v < 10 ? 2 : 1)}</b>${k.startsWith('ΔE') ? '' : ':1'}</span>`).join('');
    return `<section id=${s.name}><h2><span>${esc(s.name)}</span> <i>${esc(s.label)}</i> <em class=${r.ok ? 'ok' : 'no'}>${r.ok ? 'PASS' : 'FAIL'}</em></h2>
<p class=sw>${SW.map(k => `<span style="background:${s.tokens[k]}" title="${k} ${s.tokens[k]}"></span>`).join('')}</p>
<p class=nums>${nums}</p>${r.ok ? '' : `<p class=no>${r.fails.map(esc).join('<br>')}</p>`}
<div class=pair><figure><img src=${s.name}-queue-390.png width=390 height=900 alt="${s.name}: Queue"><figcaption>Queue</figcaption></figure>
<figure><img src=${s.name}-host-390.png width=390 height=900 alt="${s.name}: Host"><figcaption>Host</figcaption></figure></div></section>`;
  };
  return `<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>Colour schemes</title>
<!-- #244 phase 1, generated by shoot.mjs: do not edit by hand. Each phone is the real board (the demo export, serve.css with
     the phase-2 token swaps) under one scheme, shot at 390×900. -->
<style>
:root{color-scheme:dark}
body{margin:0;padding:12px;background:#0d1117;color:#c9d1d9;font:13px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace}
h1{font-size:13px;margin:0 0 4px}
.lede{margin:0 0 12px;color:#8b949e;font-size:11px}
.sheet{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,796px),1fr));gap:16px}
section{border:1px solid #30363d;border-radius:6px;padding:8px}
h2{display:flex;gap:8px;align-items:baseline;margin:0;font-size:13px}
h2 i{flex:1;font-style:normal;color:#8b949e;font-weight:400}
em{font-style:normal;font-size:11px;border:1px solid currentColor;border-radius:4px;padding:0 4px}
.ok{color:#3fb950}.no{color:#f85149}
.sw{display:flex;gap:2px;margin:6px 0 2px}.sw span{flex:1;height:14px;border-radius:2px;outline:1px solid #30363d}
.nums{display:flex;flex-wrap:wrap;gap:0 12px;margin:0 0 6px;font-size:11px;color:#8b949e}.nums b{color:#c9d1d9;font-weight:400}
.pair{display:grid;grid-template-columns:1fr 1fr;gap:8px}
figure{margin:0}img{display:block;width:100%;height:auto;border-radius:4px}
figcaption{font-size:11px;color:#8b949e;text-align:center}
</style></head><body>
<h1>Colour schemes (#244): Queue and Host at 390 px, every built-in</h1>
<p class=lede>Each phone is the real board under one token set. Gates: text ≥ ${4.5}:1 on --bg and --card, status ≥ 3:1, status colours ΔE2000 ≥ 20 apart,
accent/link ΔE2000 ≥ 10 from every status colour. github-dark vs today: Queue ${same.queue ? 'pixel-identical' : 'DIFFERS'}, Host ${same.host ? 'pixel-identical' : 'DIFFERS'}.</p>
<div class=sheet>
${['github-dark', 'solarized-dark', 'gruvbox', 'nord', 'dracula', 'paper', 'farm', 'kitchen'].map(n => schemes.find(s => s.name === n)).filter(Boolean).map(card).join('\n')}
</div></body></html>
`;
}
process.exit(Object.values(same).every(Boolean) && report.every(r => r.ok) ? 0 : 1);
