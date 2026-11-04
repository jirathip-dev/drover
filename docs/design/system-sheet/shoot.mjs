// #440 phase 1: the fixtures and the measurements, shot from the real board. Takes an exported demo
// (`herdsman serve --export <site> --demo`) and copies it three times: `today/` as exported; `next/` with proposal.js/.css
// loaded after serve.js/herd.js and the Host data given a second host and its live controls ([sign in ▾], [off]);
// `calm/` as next with no host alert (the [⚙] dot off). Then the tabs, every System section at 390 (DPR 2) and 1440,
// the providers' states, the old routes and report.json.
// Usage: node shoot.mjs <site dir> <chrome-headless-shell>
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chrome, serve, sleep } from './cdp.mjs';

const here = new URL('.', import.meta.url).pathname;
const [site, bin] = process.argv.slice(2);
const read = f => readFileSync(f, 'utf8');
const root = mkdtempSync(join(tmpdir(), 'd440-'));
for (const v of ['today', 'next', 'calm']) cpSync(site, join(root, v), { recursive: true });
cpSync(join(here, 'fixture'), join(root, 'fixture'), { recursive: true });

// The Host fixture: the demo's `mini` (red: disk under its floor) and a second host, `studio`, green; live, so each
// card has its controls. `calm`: mini's disk is fine too.
const host = JSON.parse(read(join(site, 'api/v1/host')));
const green = h => ({ ...h, dot: 'g', tone: 'g', words: 'up · admits lanes', rows: h.rows.map(r => r.key === 'disk' ? { ...r, t: 'g', bt: '', b: 'free 212 GB', bar: r.bar && { ...r.bar, t: 'g', n: 3, pct: '' } } : r) });
const studio = { ...green(host.hosts[0]), name: 'studio', main: false, to: 'off', lanes: 3 };
const fx = (calm) => JSON.stringify({ ...host, live: true, hosts: [calm ? { ...green(host.hosts[0]), main: true } : host.hosts[0], studio] });
writeFileSync(join(root, 'next/api/v1/host'), fx(false));
writeFileSync(join(root, 'calm/api/v1/host'), fx(true));
for (const v of ['next', 'calm']) {
  const hash = readdirSync(join(root, v, 'a'))[0];
  for (const f of ['proposal.js', 'proposal.css']) cpSync(join(here, f), join(root, v, 'a', hash, f));
  const f = join(root, v, 'index.html');
  let h = read(f);
  const swap = (re, to) => { if (!re.test(h)) throw new Error(`index.html changed: ${re}`); h = h.replace(re, to); };
  swap(/(<link rel=stylesheet href=a\/\w+\/herd\.css>)/, `$1<link rel=stylesheet href=a/${hash}/proposal.css>`);
  swap(/(<script src=a\/\w+\/herd\.js><\/script>)/, `$1<script src=a/${hash}/proposal.js></script>`);
  writeFileSync(f, h);
}

// `node shoot.mjs <site> <chrome> --setup`: build the three copies, print where, shoot nothing.
if (process.argv.includes('--setup')) { console.log(root); process.exit(0); }
const out = join(here, 'shots');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const s = await serve(root), c = chrome(bin);
setTimeout(() => { console.error('shoot: timed out'); c.kill(); process.exit(1); }, 900000).unref();
const READY = `document.readyState === 'complete' && !!window.boardReady`;
const report = { shots: [], checks: {}, routes: {}, overflow: {} };
const SECS = ['providers', 'fleets', 'hosts', 'activity', 'notifications', 'conductor', 'appearance'];
async function open(v, hash, W, o = {}) {
  const p = await c.open(`${s.base}/${v}/index.html${o.q || ''}#${hash}`, W, W < 720 ? 844 : 900, W < 720 ? 2 : 1, { ready: READY });
  await p.ev(`Promise.all([window.boardReady, window.sysReady || 0]).then(() => 1)`);
  // Framing: the demo banner goes; the needs-you cards stay (they are on every tab today). Nothing is restyled.
  await p.ev(`document.querySelector('.msg.a')?.remove(); scrollTo(0, 0); 1`);
  await sleep(700);
  await p.ev(`document.getAnimations().forEach(a => { a.currentTime = 0; a.pause(); }); 1`);
  return p;
}
const save = async (name, png) => { writeFileSync(join(out, name), png); report.shots.push(name); };
// The sheet whole: the viewport grows until the scroller shows every section.
async function whole(p, W) {
  const h = await p.ev(`(() => { const sh = document.querySelector('#sys>.sh'), t = sh.querySelector('.top'); return Math.ceil(sh.offsetHeight - t.clientHeight + t.scrollHeight + 4); })()`);
  await p.cmd('Emulation.setDeviceMetricsOverride', { width: W, height: h, deviceScaleFactor: W < 720 ? 2 : 1, mobile: W < 720 });
  await sleep(500);
  return p.png({ height: h });
}
// Nothing wider than the phone: the page, the sheet's scroller, and any visible element not inside a box that clips
// it (a Host row's long lane list ellipsises, as on today's Host tab).
const overflow = (p, k) => p.ev(`(() => { const W = innerWidth, t = document.querySelector('#sys .top'), clipped = e => { for (let a = e.parentElement; a && a !== t; a = a.parentElement) if (getComputedStyle(a).overflowX !== 'visible') return true; return false; }; const bad = [...document.querySelectorAll('#sys *')].filter(e => e.checkVisibility() && e.getBoundingClientRect().right > W + 0.5 && !clipped(e)).map(e => e.tagName.toLowerCase() + '.' + [...e.classList].join('.')); return { pageWidth: document.documentElement.scrollWidth, sheetScrollWidth: t.scrollWidth, sheetWidth: t.clientWidth, wider: [...new Set(bad)].slice(0, 8) }; })()`).then(r => { report.overflow[k] = r; });

// 1. The tabs: [⚙] on each, three tabs in the bar (next), against today.
for (const t of ['queue', 'lanes', 'chat']) for (const W of [390, 1440]) {
  const p = await open('next', t, W);
  await save(`tab-${t}-${W}.png`, await p.png());
  if (W === 390) {
    report.checks[`tab ${t}`] = await p.ev(`({ tabs: [...document.querySelectorAll('nav a')].map(a => a.hash.slice(1)), gear: !!document.querySelector('header #sysb')?.checkVisibility(), look: !!document.querySelector('#look'), dot: !document.querySelector('#sysb .sd').hidden })`);
  }
  await p.done();
}
for (const [v, name] of [['next', 'gear-dot-390.png'], ['calm', 'gear-calm-390.png']]) {
  const p = await open(v, 'queue', 390);
  await save(name, await p.png({ height: 56 }));
  report.checks[name] = await p.ev(`({ dot: !document.querySelector('#sysb .sd').hidden, label: document.querySelector('#sysb').getAttribute('aria-label'), hostsChipDot: !document.querySelector('.sxi a[data-s=hosts] .sd').hidden })`);
  await p.done();
}

// 2. The System sheet at each section, 390 and 1440, and whole.
for (const W of [390, 1440]) {
  for (const sec of SECS) {
    if (W === 1440 && !['providers', 'fleets', 'hosts', 'activity', 'notifications'].includes(sec)) continue;
    const p = await open('next', `system/${sec}${sec === 'fleets' ? '/acme-api' : ''}`, W);
    await sleep(500);
    await save(`system-${sec}-${W}.png`, await p.png());
    if (W === 390) await overflow(p, sec);
    await p.done();
  }
  const p = await open('next', 'system', W);
  await save(`system-all-${W}.png`, await whole(p, W));
  await p.done();
}
{
  // [⚙] opens System at Agent providers.
  const p = await open('next', 'queue', 390);
  await p.ev(`document.querySelector('#sysb').click(); 1`);
  await sleep(400);
  report.checks['gear opens'] = await p.ev(`({ open: !document.querySelector('#sys').hidden, hash: location.hash, current: document.querySelector('.sxi a.on').dataset.s, top: document.querySelector('#sys .top').scrollTop })`);
  // Every former Host, Events, [config] and [◐] control, inside its section.
  report.checks.moved = await p.ev(`(() => {
    const n = (sec, q) => document.querySelectorAll('#sy-' + sec + ' ' + q).length;
    return {
      'hosts: host cards': n('hosts', '.hcd'), 'hosts: charts (rows with history)': n('hosts', '.hcd details.rw[data-k]'),
      'hosts: [sign in ▾] logins': [...document.querySelectorAll('#sy-hosts [data-su]')].map(b => b.dataset.h + ':' + b.dataset.su).length ? [...document.querySelectorAll('#sy-hosts [data-su]')].map(b => b.dataset.su) : [],
      'hosts: [on]/[off]': [...document.querySelectorAll('#sy-hosts input[name=to]')].map(i => i.closest('.hcd').dataset.h + ' ' + i.value),
      'hosts: groups': [...document.querySelectorAll('#sy-hosts .grp .gn')].map(g => g.textContent),
      'activity: rows': n('activity', 'details.er'), 'activity: chips': n('activity', '.chips button'),
      'fleets: rows': [...document.querySelectorAll('.sfl')].map(f => f.dataset.f),
      'fleets: config groups': [...document.querySelectorAll('.sfl[data-f=acme-api] .cg>h3')].map(h => h.firstChild.textContent),
      'fleets: role pickers': n('fleets', '.role select'), 'fleets: conductor choices (#401)': n('fleets', 'form[aria-label=harness] button'),
      'appearance: theme rows': n('appearance', '.lko[data-t]'), 'appearance: colour rows': n('appearance', '.lko[data-s]'), 'appearance: style rows': n('appearance', '.lko[data-y]'),
      'notifications: push, enable, alert devices, paired devices, telegram': [n('notifications', 'dd b'), n('notifications', '#push2'), document.querySelectorAll('#sy-notifications .cg')[1].querySelectorAll('.role2').length, document.querySelectorAll('#sy-notifications .cg')[2].querySelectorAll('.role2').length, /Telegram/.test(document.querySelector('#sy-notifications').textContent)],
      'providers: cards, rows': [n('providers', '.pvc'), n('providers', '.pvr')],
      'not left elsewhere': { look: !!document.querySelector('#look'), hostTab: !!document.querySelector('nav a[href="#host"]'), eventsTab: !!document.querySelector('nav a[href="#events"]'), hostInMain: !!document.querySelector('main>#host'), eventsInMain: !!document.querySelector('main>#events') },
    };
  })()`);
  // A theme pick in Appearance applies at once (serve.js lpick), the sheet stays open.
  await p.ev(`document.querySelector('#sy-appearance .lko[data-t=kitchen]').click(); 1`);
  await sleep(600);
  report.checks['appearance pick'] = await p.ev(`({ theme: document.documentElement.dataset.theme, open: !document.querySelector('#sys').hidden })`);
  await p.done();
}

// 3. Agent providers: [use] → its scope → the move prompt; [+ add key] with a refused key; a sign-in's setup terminal.
{
  const p = await open('next', 'system/providers', 390);
  await p.ev(`SYS.scope(document.querySelector('.pvr[data-a=B]'), 'acme-api', 'rev'); document.querySelector('.pvr[data-a=B]').scrollIntoView(); 1`);
  await sleep(300);
  await save('providers-use-390.png', await p.png());
  await p.ev(`SYS.move(document.querySelector('.pvr[data-a=B]')); document.querySelector('.pvr[data-a=B]').scrollIntoView(); 1`);
  await sleep(300);
  await save('providers-move-390.png', await p.png());
  await p.ev(`document.querySelector('.pvr[data-a=B] .pvq').remove(); SYS.addkey(document.querySelector('.pvc[data-p=deepseek]'), true); document.querySelector('.pvc[data-p=deepseek]').scrollIntoView(); 1`);
  await sleep(300);
  await save('providers-addkey-390.png', await p.png());
  await p.done();
}
for (const W of [390, 1440]) {
  const p = await open('next', 'system/providers', W);
  // The sign-in (Anthropic [+ add account] / [sign in again]): #303's setup terminal over the sheet, the token masked.
  await p.ev(`(() => {
    sh.className = 'sh';
    document.querySelector('#sheet .hd b').textContent = 'sign in · Anthropic account B';
    document.querySelectorAll('#sheet :is(.acts, .fvr)').forEach(x => x.hidden = true);
    now.innerHTML = '<p class=m>claude setup-token on mini · the token goes straight to the keychain, never shown · closes when it exits or in 10 min</p><pre class=out>$ claude setup-token\\nOpening the browser to sign in…\\nIf it did not open, visit: https://claude.ai/oauth/authorize?code=true&amp;client_id=…\\n\\nPaste code here if prompted &gt; ▌</pre><p><a href=# target=_blank>[open]</a> <span class=m>https://claude.ai/oauth/authorize?code=true&amp;c…</span></p>';
    document.querySelector('.reply textarea').placeholder = 'paste the code';
    show('mid');
    return 1;
  })()`);
  await sleep(700);
  await save(`providers-signin-${W}.png`, await p.png());
  await p.done();
}

// 4. The routes: old links land in the sheet.
for (const [from, v] of [['host', 'next'], ['events', 'next'], ['config/acme-web', 'next'], ['system/conductor', 'next'], ['system/fleets/docs', 'next']]) {
  const p = await open(v, from, 390);
  await sleep(300);
  report.routes['#' + from] = await p.ev(`({ hash: location.hash, tab: document.documentElement.dataset.tab, open: !document.querySelector('#sys').hidden, current: document.querySelector('.sxi a.on')?.dataset.s, fleetOpen: [...document.querySelectorAll('.sfl[open]')].map(d => d.dataset.f) })`);
  await p.done();
}
{
  // A stored `host` tab (localStorage, the `tab` cookie) opens on Queue.
  const p = await open('next', 'queue', 390);
  await p.ev(`localStorage.tab = 'host'; 1`);
  await p.cmd('Page.navigate', { url: `${s.base}/next/index.html` });
  await sleep(1500);
  await p.ev(`Promise.all([window.boardReady, window.sysReady]).then(() => 1)`);
  report.routes['stored tab host'] = await p.ev(`({ tab: document.documentElement.dataset.tab, hash: location.hash, open: !document.querySelector('#sys').hidden })`);
  await p.done();
}
{
  // [config] on a Queue card opens its fleet's row in Fleets.
  const p = await open('next', 'queue', 390);
  await p.ev(`document.querySelector('#queue button.cfg').click(); 1`);
  await sleep(400);
  report.routes['[config] tap'] = await p.ev(`({ hash: location.hash, open: !document.querySelector('#sys').hidden, fleetOpen: [...document.querySelectorAll('.sfl[open]')].map(d => d.dataset.f) })`);
  await p.done();
}
report.errors = c.errors;
writeFileSync(join(here, 'report.json'), JSON.stringify(report, null, 1) + '\n');
console.log(JSON.stringify(report, null, 1));
c.kill();
s.close();
process.exit(0);
