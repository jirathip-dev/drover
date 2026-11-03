// Tabs, lane display mode (both kept in localStorage), the live board (SSE `/live`, sync dot, ↻,
// pull to refresh), SW, the Lanes bottom sheet: card, Stop/Kill/Clean (`/act`), transcript per turn
// (5 s poll, load more), reply box with a `/` menu, Pair this phone, and blocked agents' menu buttons.
const B = document.body, $ = s => document.querySelector(s), $$ = s => document.querySelectorAll(s);
const get = k => { try { return localStorage.getItem(k); } catch (e) { return null; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch (e) {} };
// URLs hang off R (`/` live, `` relative in a `serve --export`). #39 demo: static files, never a POST, never `/live`.
const R = B.dataset.root ?? '/', DEMO = !!B.dataset.demo;
if (DEMO) { const f = fetch; window.fetch = (u, o) => o && o.method === 'POST' ? Promise.reject(new Error('demo: no writes')) : f(u, o); }
function tab(t) {
  document.documentElement.dataset.tab = t; // #206: serve.css shows this tab's section (serve.rs sets it before the first paint)
  $$('main>section').forEach(s => s.hidden = s.id !== t);
  $$('nav a').forEach(a => a.classList.toggle('on', a.hash === '#' + t));
  put('tab', t);
  $('.qhelp').hidden = t !== 'queue';
  // #60: the live page leaves the Events body out; it loads when the tab opens.
  if (t === 'events' && !$('#events').childElementCount) part('frag?id=events').then(h => h && frag('events', h));
}
// #60: a part of the page read on demand (null when it cannot be read).
const part = src => fetch(R + src, { cache: 'no-store' }).then(r => r.ok ? r.text() : null).catch(() => null);
// A form POST of `data` (an object or FormData) to `path`.
const post = (path, data) => fetch(R + path, { method: 'POST', body: new URLSearchParams(data) });
$$('nav a').forEach(a => a.onclick = e => { e.preventDefault(); tab(a.hash.slice(1)); });
// A tapped alert opens `/?issue=N#tab`: that tab (the head script picked it: the hash, else the last one), the issue's row open.
const DBG = new URLSearchParams(location.search), want = DBG.get('issue');
tab(document.documentElement.dataset.tab || 'queue');
// #206 `?debug` (kept for the tab's session): how the page came up. A load or reload repaints it all; a pull only refresh()es.
const dbg = (...a) => { try { if (DBG.has('debug') || sessionStorage.debug) { sessionStorage.debug = 1; console.log('[herdsman]', ...a); } } catch (e) {} };
dbg('page', performance.getEntriesByType('navigation')[0]?.type, 'tab', document.documentElement.dataset.tab);
addEventListener('pageshow', e => dbg('pageshow', e.persisted ? 'from bfcache' : 'loaded'));
$$('details.q').forEach(d => { if (want && d.querySelector('summary>span').textContent === '#' + want) { d.open = true; d.scrollIntoView(); } });
// #241: the Lanes modes are the theme's `modes` (farm, kitchen: scene, ascii; terminal: glyph alone, no button); TT is
// the theme herd.js loaded (docs/themes.md). The button sits in the Lanes tab (swapped with it): delegated, relabelled after each swap.
let TT = null, TM = ['scene', 'ascii'];
function mode(m) {
  B.dataset.mode = m = TM.includes(m) ? m : TM[0];
  const b = $('#mode span') || $('#mode');
  if (b) { b.textContent = m; b.closest('.lh').hidden = TM.length < 2; }
  put('mode', m);
}
mode(get('mode'));
document.addEventListener('click', e => { if (e.target.closest('#mode')) mode(TM[(TM.indexOf(B.dataset.mode) + 1) % TM.length]); });
// #244 schemes: pure from here to `schemes:end` (tests/schemes.rs runs it under node). A scheme is
// { "label", "color-scheme": "dark" | "light", "tokens": { "--bg": "#rrggbb", … } } (docs/themes.md, docs/design/schemes).
// gate() is every check it must pass, with what failed; a refused one is named on the Host tab and never shown.
const TOKENS = ['--bg', '--card', '--edge', '--line', '--scrim', '--fg', '--mute', '--l', '--link',
  '--g', '--a', '--r', '--p', '--c', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a'];
const hex = s => {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(s);
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
};
const lin = c => (c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
// `color-mix(in srgb, c p%, transparent)` over bg, as the board's tints are (.ask, .later, .grp[data-t]).
const over = (c, bg, p) => c.map((v, i) => v * p + bg[i] * (1 - p));
function lab([r, g, b]) {
  const [x, y, z] = [[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]]
    .map(row => row[0] * lin(r) + row[1] * lin(g) + row[2] * lin(b));
  const f = t => t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116;
  const [fx, fy, fz] = [x / 0.95047, y, z / 1.08883].map(f);
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}
// CIEDE2000 (Sharma et al. 2005).
function de(c1, c2) {
  const [L1, a1, b1] = lab(c1), [L2, a2, b2] = lab(c2), rad = Math.PI / 180;
  const C = (Math.hypot(a1, b1) + Math.hypot(a2, b2)) / 2, G = 0.5 * (1 - Math.sqrt(C ** 7 / (C ** 7 + 25 ** 7)));
  const ap1 = a1 * (1 + G), ap2 = a2 * (1 + G), C1 = Math.hypot(ap1, b1), C2 = Math.hypot(ap2, b2);
  const h = (b, a) => { const v = Math.atan2(b, a) / rad; return v < 0 ? v + 360 : v; };
  const h1 = h(b1, ap1), h2 = h(b2, ap2), dL = L2 - L1, dC = C2 - C1;
  const dh = C1 * C2 === 0 ? 0 : h2 - h1 > 180 ? h2 - h1 - 360 : h2 - h1 < -180 ? h2 - h1 + 360 : h2 - h1;
  const dH = 2 * Math.sqrt(C1 * C2) * Math.sin(dh * rad / 2), Lm = (L1 + L2) / 2, Cm = (C1 + C2) / 2;
  const hm = C1 * C2 === 0 ? h1 + h2 : Math.abs(h1 - h2) > 180 ? (h1 + h2 + 360) / 2 : (h1 + h2) / 2;
  const T = 1 - 0.17 * Math.cos((hm - 30) * rad) + 0.24 * Math.cos(2 * hm * rad) + 0.32 * Math.cos((3 * hm + 6) * rad) - 0.2 * Math.cos((4 * hm - 63) * rad);
  const SL = 1 + 0.015 * (Lm - 50) ** 2 / Math.sqrt(20 + (Lm - 50) ** 2), SC = 1 + 0.045 * Cm, SH = 1 + 0.015 * Cm * T;
  const RT = -2 * Math.sqrt(Cm ** 7 / (Cm ** 7 + 25 ** 7)) * Math.sin(60 * Math.exp(-(((hm - 275) / 25) ** 2)) * rad);
  return Math.sqrt((dL / SL) ** 2 + (dC / SC) ** 2 + (dH / SH) ** 2 + RT * (dC / SC) * (dH / SH));
}
// Every check as [what, value, minimum]: text 4.5:1 (on --bg, --card and the tinted surfaces: the needs-you card with
// its option buttons, the red and amber group headers), status colours 3:1, green/amber/red ΔE2000 ≥ 20 apart, and
// the accent and links ≥ 20 from each status colour (a control never reads as a warning).
function gate(s) {
  const t = (s && s.tokens) || {}, missing = TOKENS.filter(k => !hex(t[k] || ''));
  if (missing.length) return [[`missing or not #rrggbb[aa]: ${missing.join(' ')}`, 0, 1]];
  const c = Object.fromEntries(TOKENS.map(k => [k, hex(t[k])])), rows = [];
  const on = (ks, bg, min, name = bg) => ks.forEach(k => rows.push([`${k} on ${name}`, ratio(c[k], bg.map ? bg : c[bg]), min]));
  const card = over(c['--r'], c['--bg'], 0.15), red = over(c['--r'], c['--bg'], 0.12), amber = over(c['--a'], c['--bg'], 0.12);
  for (const bg of ['--bg', '--card']) {
    on(['--fg', '--mute', '--l', '--link', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a'], bg, 4.5);
    on(['--g', '--a', '--r'], bg, 3);
  }
  on(['--bg'], '--l', 4.5, '--l (pressed control, active tab)');
  on(['--fg', '--mute', '--l'], card, 4.5, 'the needs-you card');
  on(['--r'], card, 3, 'the needs-you card');
  on(['--r'], red, 3, 'a red group header');
  on(['--a'], amber, 3, 'an amber group header');
  for (const [x, y] of [['--g', '--a'], ['--a', '--r'], ['--g', '--r'], ...['--l', '--link'].flatMap(x => ['--g', '--a', '--r'].map(y => [x, y]))])
    rows.push([`ΔE2000 ${x} ↔ ${y}`, de(c[x], c[y]), 20]);
  if (!['dark', 'light'].includes(s['color-scheme'])) rows.push(['color-scheme is not "dark" or "light"', 0, 1]);
  return rows;
}
// What a scheme fails, in words (none: it passes).
const fails = s => gate(s).filter(r => r[1] < r[2]).map(([w, v, m]) => v ? `${w} ${v.toFixed(2)} < ${m}` : w);
// A passing scheme's token block, as the head script applies it.
const block = s => `:root{${TOKENS.map(k => `${k}:${s.tokens[k]}`).join(';')};color-scheme:${s['color-scheme']}}`;
// The scheme a phone shows: its pick, else its theme's default, else github-dark; each only if it passes.
const choose = (all, pick, def) => [pick, def, 'github-dark'].find(n => all[n] && !fails(all[n]).length) || 'github-dark';
// schemes:end
// #244: the `[▦]` colours: every scheme that passes the gate (built-in and `<config dir>/schemes/`; SD from the head
// script), this phone's pick (localStorage) else its theme's default. Applied in place; the block is kept so the head
// script paints it first next time (no flash), and the status bar takes its --bg.
const scp = $('#scheme'), SQ = new URLSearchParams(location.search).get('scheme');
scp.replaceChildren(...Object.keys(SD.s).filter(n => !fails(SD.s[n]).length).map(n => Object.assign(document.createElement('option'), { textContent: n })));
function colours() {
  const h = document.documentElement, n = choose(SD.s, SQ || get('scheme'), SD.t[h.dataset.theme]), b = block(SD.s[n]);
  h.dataset.scheme = scp.value = n;
  scp.closest('.pk').title = 'colours: ' + n;
  $('#sch').textContent = b;
  $('meta[name=theme-color]').content = SD.s[n].tokens['--bg'];
  put('sb', b);
  hostrow(document);
}
scp.onchange = () => { put('scheme', scp.value); colours(); };
// #244: the Host tab's `theme` row names this phone's theme and colours; under it, each refused scheme with why.
function hostrow(root) {
  const r = [...root.querySelectorAll('.gr')].find(p => p.firstChild.textContent === 'theme'), h = document.documentElement;
  if (!r) return;
  const w = r.querySelector('.w'), b = t => Object.assign(document.createElement('b'), { textContent: t });
  if (r.dataset.t === 'g') w.replaceChildren(b(h.dataset.theme));
  w.querySelectorAll('.sx').forEach(x => x.remove());
  w.append(...[Object.assign(document.createElement('span'), { textContent: ' · colours ' }), b(h.dataset.scheme)].map(x => (x.classList.add('sx'), x)));
  root.querySelectorAll('.gr.sx').forEach(x => x.remove());
  r.after(...Object.entries(SD.s).map(([n, s]) => [n, fails(s)]).filter(([, f]) => f.length).map(([n, f]) => {
    const p = Object.assign(document.createElement('p'), { className: 'gr sx', innerHTML: '<span class=k>colours</span><span class=w><b class=a></b> <span class=mt></span></span>' });
    p.dataset.t = 'a';
    p.querySelector('b').textContent = n;
    p.querySelector('.mt').textContent = `refused: ${f[0]}${f.length > 1 ? ` (+${f.length - 1} more)` : ''}; phones show their theme's default`;
    return p;
  }));
}
// #241: the `[◐]` picker: this phone's theme, applied in place (no reload); the home-screen icon is iOS's from the install.
const thp = $('#theme');
thp.value = document.documentElement.dataset.theme;
thp.closest('.pk').title = 'theme: ' + thp.value;
thp.onchange = () => {
  document.documentElement.dataset.theme = thp.value;
  thp.closest('.pk').title = 'theme: ' + thp.value;
  put('theme', thp.value);
  theme(thp.value);
  colours();
  if (!get('theme-hint')) { put('theme-hint', 1); toast('the home-screen icon keeps its install-time look'); }
};
colours();
// #241: the theme on the tabs. A Queue or Events row's state glyph becomes its action's 24 px sprite (#281; its title and label
// the plain state word); a Lanes avatar takes the theme's ASCII words and look. Terminal (`row_sprites: false`): none.
const AW = { impl: 'building', fix: 'fixing', rev: 'reviewing', update: 'updating', idle: 'idle', done: 'done', blocked: 'needs you' };
const hash = t => [...t].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
function rows(root) {
  root.querySelectorAll('.rsg').forEach(g => { g.textContent = g.dataset.g; g.classList.remove('rsg'); ['title', 'role', 'aria-label', 'data-g'].forEach(a => g.removeAttribute(a)); });
  const rs = TT && TT.row_sprites, sp = (g, act, word) => {
    if (!g || !rs || !rs[act]) return;
    Object.assign(g.dataset, { g: g.textContent });
    g.classList.add('rsg');
    g.title = word;
    g.setAttribute('role', 'img');
    g.setAttribute('aria-label', word);
    g.innerHTML = `<svg class=rsp viewBox="0 0 132 100" aria-hidden=true><use href="#hr-${act}"/></svg>`;
  };
  root.querySelectorAll('details.q').forEach(d => sp(d.querySelector('summary .gt>b'), d.dataset.act, d.querySelector('summary>span:nth-of-type(2)>span')?.textContent || AW[d.dataset.act]));
  root.querySelectorAll('.kg[data-act]').forEach(g => sp(g, g.dataset.act, AW[g.dataset.act]));
  const L = (TT && TT.looks) || [];
  root.querySelectorAll('.av[data-act]').forEach(v => {
    const a = v.querySelector('.asc'), px = v.querySelector('.px'), t = TT && TT.ascii && TT.ascii[v.dataset.act], n = v.closest('[data-agent]');
    if (a.dataset.o == null) Object.assign(a.dataset, { o: a.innerHTML, c: px.getAttribute('style') });
    a.innerHTML = t ? '<b></b>' : a.dataset.o;
    if (t) a.firstChild.textContent = t;
    a.classList.toggle('w', !!t);
    px.setAttribute('style', L.length && n ? Object.entries(L[hash(n.dataset.agent) % L.length]).map(e => e.join(':')).join(';') : a.dataset.c);
  });
}
// #241: a Host group's load scene in miniature (its status: calm, busy or held): wind and clouds, or flame height.
const MINI = {
  weather: ['<circle cx=8 cy=8 r=4 fill="var(--a)"/>', '<path d="M3 12h10a3 3 0 0 0-2-6 4 4 0 0 0-7 1 3 3 0 0 0-1 5z" fill="var(--mute)"/><path d="M1 4h5M2 7h3" stroke="var(--mute)"/>',
    '<path d="M3 9h10a3 3 0 0 0-2-6 4 4 0 0 0-7 1 3 3 0 0 0-1 5z" fill="var(--mute)"/><path d="M5 11l-1 4M8 11l-1 4M11 11l-1 4" stroke="var(--l)"/>'],
  heat: [5, 9, 13].map((h, i) => `<path d="M8 ${15 - h}C12 ${15 - h / 2} 12 15 8 15C4 15 4 ${15 - h / 2} 8 ${15 - h}Z" fill="var(--${i > 1 ? 'r' : 'a'})"/>`),
};
function mini(g) {
  const s = g.querySelector(':scope>summary'), ls = TT && TT.load_scene, k = { a: 1, r: 2 }[g.dataset.t] || 0, l = ['calm', 'busy', 'held'][k];
  s.querySelector('.lsc')?.remove();
  if (!ls || !MINI[ls.kind]) return;
  const word = esc('load: ' + ((ls.levels || {})[l] || l));
  s.querySelector('.dt').insertAdjacentHTML('afterend', `<svg class=lsc viewBox="0 0 16 16" role=img aria-label="${word}"><title>${word}</title>${MINI[ls.kind][k]}</svg>`);
}
// #241: a moment (`ev` on /live with a `k`: merged, fail, needs-you) plays the theme's sprite for 2.4 s, silent; reduced motion: a still frame.
function moment(k) {
  const m = TT && TT.moments && TT.moments[k];
  if (!m || !m.sprite) return;
  let o = $('#mo');
  if (!o) B.append(o = Object.assign(document.createElement('i'), { id: 'mo' }));
  o.className = 'mo-' + (m.anim || 'hop');
  o.innerHTML = `<svg viewBox="0 0 132 100" style="--c:#c89a56;--d:#a37a3c;--m:#e8dcc0;--b:#efe7dc" aria-hidden=true><use href="#hm-${k}"/></svg>`;
  o.hidden = false;
  clearTimeout(o.t);
  o.t = setTimeout(() => { o.hidden = true; }, 2400);
}
// Sync dot: g = live and up to date (pulses on an update), a = reconnecting (↻ spins), r = offline.
const dot = $('#dot'), rf = $('#rf');
let last = Number(B.dataset.ts) * 1000, seq = 0, es = null, down = Date.now(), tries = 0, poller = 0, busy = false;
function sync(s) {
  if (DEMO) s = 'demo';
  dot.className = s;
  rf.classList.toggle('spin', s === 'a' || busy);
  $('#stale').hidden = s !== 'r';
  $('#stale').textContent = 'last update ' + new Date(last).toTimeString().slice(0, 5);
  $$('form button, form select:not([data-off]), .ask button:not(.lat)').forEach(b => b.disabled = s === 'r' || DEMO);
  if (DEMO) $$('input, textarea, .acts button').forEach(b => b.disabled = true);
}
function pulse() { dot.classList.remove('p'); void dot.offsetWidth; dot.classList.add('p'); }
const live = () => es && es.readyState === 1;
// Rows keep their open state across a swap (by agent, or heading + first cell); changed ones flash.
const head = e => { while ((e = e.previousElementSibling)) if (e.tagName === 'H2') return e.textContent; return ''; };
const key = e => e.dataset.agent || head(e) + (e.querySelector('summary>span') || e).textContent;
const sig = e => (e.querySelector('summary,.lr') || e).textContent.replace(/\b\d+[smhd]\b/g, '');
// #65: an update patches the live tree to the new one: rows (`details`, `.lane`) matched by key(), the rest by
// position; equal nodes are left alone, `open` is the user's. So the page keeps its scroll, open rows and focus.
const rk = n => n.nodeType === 1 && n.matches('details,.lane') ? key(n) : '';
// Give `a` the attributes of `b` (also herd.js's horses), but the ones in `keep`, which stay `a`'s own.
function attrs(a, b, keep = []) {
  for (const { name } of [...a.attributes]) if (!b.hasAttribute(name) && !keep.includes(name)) a.removeAttribute(name);
  for (const { name, value } of b.attributes) if (a.getAttribute(name) !== value) a.setAttribute(name, value);
}
function morph(a, b) {
  if (a.nodeType !== 1) { if (a.data !== b.data) a.data = b.data; return; }
  if (a.isEqualNode(b)) return;
  attrs(a, b, ['open', 'data-on']);
  if (a.dataset.on) return; // a loaded part keeps its body; refresh() re-reads it
  const rows = new Map(), rest = [], kids = [...b.childNodes];
  for (const c of a.childNodes) { const k = rk(c); if (k && !rows.has(k)) rows.set(k, c); else if (!k) rest.push(c); }
  kids.forEach((n, i) => {
    const k = rk(n), j = k ? -1 : rest.findIndex(c => c.nodeName === n.nodeName);
    let o = k ? rows.get(k) : rest[j];
    if (k) rows.delete(k); else if (j >= 0) rest.splice(j, 1);
    if (o) morph(o, n); else o = n;
    if (a.childNodes[i] !== o) a.insertBefore(o, a.childNodes[i] || null);
  });
  while (a.childNodes.length > kids.length) a.lastChild.remove();
}
function frag(id, html) {
  const el = document.getElementById(id);
  if (!el) return;
  const rows = e => new Map([...e.querySelectorAll('details,.lane')].map(r => [key(r), r]));
  const old = new Map([...rows(el)].map(([k, r]) => [k, sig(r)])), nu = el.cloneNode(false), y = scrollY;
  nu.innerHTML = html;
  tidy(nu);
  morph(el, nu);
  nows();
  if (id === 'host') groups();
  if (scrollY !== y) scrollTo(0, y);
  rows(el).forEach((r, k) => { if (old.get(k) !== sig(r)) r.classList.add('fl'); });
  last = Date.now();
  alerts();
  badge();
  if (id === 'lanes') mode(B.dataset.mode);
  if (id === 'needs') fold();
}
// #234: a Host row's history: its 24 h sparkline (`data-s`, 120 buckets, the worst row of each) and, on tap, its chart
// (1 h / 6 h / 24 h from `frag?id=hist`; an export carries it as `data-x`). The value's line is red past the limit
// (dashed), lanes a purple step line on a right axis, memory pressure a band along the floor. Inline SVG, no library.
// Scales are fixed per metric (a multiple of the limit), so cards compare at a glance and the limit sits clear of zero.
const K = { load: ['load/cpu', 2, 1.6, ''], swap: ['swap-out', 0, 2, ' pages/2s'], mem: ['free memory', 1, 4, ' GB'], disk: ['free disk', 0, 4, ' GB'], lanes: ['lanes', 0] };
const fl = k => k === 'mem' || k === 'disk', OPEN = new Map(), DATA = new Map(), NSS = ' vector-effect=non-scaling-stroke';
const nums = s => (s || '').split(',').map(x => x === '' ? null : +x);
const hsvg = (w, h, body) => `<svg viewBox="0 0 ${w} ${h}" preserveAspectRatio=none>${body}</svg>`;
// A series as polylines (a null breaks the line); `step` draws a stepped line.
function segs(v, w, h, top, step) {
  const out = [], dx = w / v.length;
  let cur = [];
  v.forEach((y, j) => {
    if (y == null) { if (cur.length) out.push(cur); cur = []; return; }
    const Y = h - Math.min(y, top) / top * h;
    if (step) cur.push([j * dx, Y], [(j + 1) * dx, Y]); else cur.push([(j + .5) * dx, Y]);
  });
  if (cur.length) out.push(cur);
  return out;
}
const pts = s => s.map(p => p[0].toFixed(1) + ',' + p[1].toFixed(2)).join(' ');
let uid = 0;
// The value's area and line, its part past the limit red (clipped at the limit), the dashed limit, pressure bands,
// lanes, a dotted line where there is no data. Sparkline and chart share it.
function plot(v, k, l, w, h, press, lanes) {
  const top = k === 'lanes' ? 10 : l * K[k][2], s = segs(v, w, h, top, k === 'lanes'), ly = l ? h - l / top * h : null;
  let b = '';
  (press || []).forEach((p, j) => {
    if (!p || press[j - 1] === p) return;
    let e = j;
    while (press[e + 1] === p) e++;
    const dx = w / press.length;
    b += `<rect class=${p > 1 ? 'pc' : 'pw'} x=${(j * dx).toFixed(1)} y=${h * .92} width=${((e - j + 1) * dx).toFixed(1)} height=${h * .08} />`;
  });
  for (const p of s) {
    if (k !== 'lanes') b += `<polygon class=ar points="${p[0][0]},${h} ${pts(p)} ${p[p.length - 1][0]},${h}"/>`;
    b += `<polyline class=${k === 'lanes' ? 'st' : 'ln'}${NSS} points="${pts(p)}"/>`;
  }
  if (ly != null) {
    const clip = fl(k) ? `<rect x=0 y=${ly} width=${w} height=${h - ly} />` : `<rect x=0 y=-10 width=${w} height=${ly + 10} />`;
    b += `<clipPath id=hc${++uid}>${clip}</clipPath><g clip-path=url(#hc${uid})>${s.map(p => `<polyline class=ov${NSS} points="${pts(p)}"/>`).join('')}</g>`;
    b += `<line class=lm${NSS} x1=0 x2=${w} y1=${ly} y2=${ly} />`;
  }
  if (lanes) b += segs(lanes, w, h, 10, true).map(p => `<polyline class=st${NSS} points="${pts(p)}"/>`).join('');
  const gap = v.findLastIndex(x => x != null) + 1;
  if (gap < v.length) b += `<line class=na${NSS} x1=${(gap * w / v.length).toFixed(1)} x2=${w} y1=${h - .5} y2=${h - .5} />`;
  return b;
}
function sparks() {
  $$('#host .hcd').forEach(c => c.querySelectorAll('.rw[data-k]').forEach(r => {
    const k = r.dataset.k, sp = r.querySelector('.spk'), o = OPEN.get(c.dataset.h + k);
    if (sp && !sp.childElementCount) sp.outerHTML = hsvg(120, 24, plot(nums(r.dataset.s), k, +r.dataset.l, 120, 24, nums(r.dataset.p), k === 'lanes' ? null : nums(c.dataset.n))).replace('<svg', '<svg class=spk aria-hidden=true');
    if (r.open && o && !r.querySelector('.hch')) chart(r, o, null, true);
  }));
}
const clock = t => new Date(t * 1000).toTimeString().slice(0, 5), X = q => `calc(5ch + (100% - 8ch) * ${(q / 100).toFixed(4)})`;
async function chart(r, range, pin, fresh) {
  const c = r.closest('.hcd'), k = r.dataset.k, l = +r.dataset.l || null, h = c.dataset.h, [name, p, , u] = K[k];
  OPEN.set(h + k, range);
  if (fresh || !DATA.has(h + k)) DATA.set(h + k, r.dataset.x || await part(`frag?id=hist&host=${encodeURIComponent(h)}&k=${k}`));
  const d = DATA.get(h + k);
  if (!d || !r.open) return;
  const i = ['24h', '6h', '1h'].indexOf(range), [v, lanes, press, ago] = d.split('|').map(x => nums(x.split(';')[i]));
  const secs = { '1h': 3600, '6h': 21600, '24h': 86400 }[range], n = v.length, top = k === 'lanes' ? 10 : l * K[k][2];
  const now = +B.dataset.ts || Date.now() / 1000, W = 1000, H = 100, f = x => x.toFixed(p);
  let body = plot(v, k, l, W, H, k === 'mem' ? press : null, k === 'lanes' ? null : lanes) + `<line class=cx${NSS} x1=0 x2=${W} y1=${H} y2=${H} />`, lab = '', rd = 'no data yet';
  // The read-out: the window's worst point (or the finger's), the worst row's own minute, the lanes then.
  const j = pin ?? v.reduce((b, x, q) => x != null && (b < 0 || (fl(k) ? x < v[b] : x > v[b])) ? q : b, -1);
  if (j >= 0 && v[j] != null) {
    const x = (j + .5) / n * 100, y = 100 - Math.min(v[j], top) / top * 100, over = l && (fl(k) ? v[j] < l : v[j] > l);
    body += `<line class=cx${NSS} x1=${x * 10} x2=${x * 10} y1=0 y2=${H} />`;
    lab += `<i class="dot${over ? ' r' : ''}" style="left:${X(x)};top:${y.toFixed(1)}%"></i>`;
    if (k !== 'lanes' && lanes[j] != null) lab += `<i class="dot l" style="left:${X(x)};top:${100 - Math.min(lanes[j], 10) * 10}%"></i>`;
    rd = `${clock(now - ago[j] * 60)} · <b class=${over ? 'r' : ''}>${over ? '▶ ' : ''}${f(v[j])}${u}</b>` + (k !== 'lanes' && lanes[j] != null ? ` · <span class=p>${lanes[j]} lanes</span>` : '');
  }
  (l ? [0, l, top] : [0, top / 2, top]).forEach(q => lab += `<i class="yl${q === l ? ' ylm' : ''}" style=top:${(100 - q / top * 100).toFixed(1)}%>${f(q)}</i>`);
  if (k !== 'lanes') [0, 5, 10].forEach(q => lab += `<i class=yr style=top:${100 - q * 10}%>${q}</i>`);
  const every = { '1h': 900, '6h': 3600, '24h': 21600 }[range];
  for (let t = Math.ceil((now - secs) / every) * every; t < now; t += every) {
    const q = (t - now + secs) / secs * 100;
    if (q > 6 && q < 94) lab += `<i class=xl style="left:${X(q)}">${clock(t)}</i>`;
  }
  lab += `<i class=xl style="left:${X(100)}">now</i>`;
  const lim = l ? `<span><i>┄</i> ${fl(k) ? 'floor' : 'limit'} ${l}${u}</span>` : '';
  const rule = k === 'swap' ? `<span>new lanes wait while the average stays over ${l} and memory is under pressure</span>` : '';
  const tabs = ['1h', '6h', '24h'].map(x => `<button type=button aria-pressed=${x === range} data-r=${x}><span>${x}</span></button>`).join('');
  let ch = r.querySelector('.hch');
  if (!ch) r.append(ch = Object.assign(document.createElement('div'), { className: 'hch' }));
  ch.innerHTML = `<div class=htb>${tabs}<span class=hrd>${rd}</span></div><div class=hpl data-n=${n}>${hsvg(W, H, body)}${lab}</div>`
    + `<p class=hlg><span><i>━</i> ${name}</span>${lim}${k !== 'lanes' ? '<span><i class=p>⌐</i> lanes (right)</span>' : ''}${k === 'mem' ? '<span><i class=a>▬</i> pressure warn</span>' : ''}${rule}</p>`;
  ch.dataset.r = range;
}
// A tab picks the range; a touch on the plot moves the read-out to that point; closing a row forgets its chart.
document.addEventListener('click', e => { const b = e.target.closest('.htb button'); if (b) chart(b.closest('.rw'), b.dataset.r); });
document.addEventListener('pointermove', e => {
  const p = e.target.closest('.hpl'), n = p && +p.dataset.n;
  if (!p) return;
  const box = p.querySelector('svg').getBoundingClientRect();
  chart(p.closest('.rw'), p.closest('.hch').dataset.r, Math.max(0, Math.min(n - 1, Math.floor((e.clientX - box.left) / box.width * n))));
});
document.addEventListener('toggle', e => {
  const r = e.target, k = r.matches('.hcd .rw[data-k]') && r.closest('.hcd').dataset.h + r.dataset.k;
  if (k && r.open && !r.querySelector('.hch')) chart(r, OPEN.get(k) || '24h', null, true);
  else if (k && !r.open) OPEN.delete(k);
}, true);
// #141: the Host groups, in the server's fixed order (#234: HOSTS, FLEET DISK, SERVICES, FLEETS, ALERTS). A group takes
// its worst row's status (grey rows don't count); a red or amber one starts open, the header summing up the rows at
// that status (a host card's rows by host). A tap's open/closed holds for the session until the group's status changes.
const sget = k => { try { return sessionStorage.getItem(k); } catch (e) { return null; } };
const sput = (k, v) => { try { sessionStorage.setItem(k, v); } catch (e) {} };
const txt = x => [...x.childNodes].filter(n => !n.matches?.('.fx,.bar')).map(n => n.textContent).join('').trim();
const sum = r => { const [k, ...c] = (r.matches('details') ? r.firstElementChild : r).children, h = r.closest('.hcd'); return (h ? h.dataset.h + ' ' : '') + k.textContent + ': ' + c.filter(x => !x.matches('.bar,.mt,.t,.sp')).map(txt).join(' '); };
const gname = g => 'host:' + g.querySelector('.gn').textContent;
function groups() {
  const rank = t => ({ r: 0, a: 1 })[t] ?? 2;
  $$('#host>.grp').forEach(g => {
    const rows = [...g.querySelectorAll('[data-t]')], w = Math.min(2, ...rows.map(r => rank(r.dataset.t))), s = sget(gname(g));
    g.dataset.t = 'rag'[w];
    g.querySelector('.gs').textContent = rows.filter(r => rank(r.dataset.t) === w).map(sum).join(' · ');
    g.open = s && s[0] === g.dataset.t ? s[1] === '1' : w < 2;
    mini(g);
  });
  sparks();
}
document.addEventListener('toggle', e => { if (e.target.matches('.grp')) sput(gname(e.target), e.target.dataset.t + +e.target.open); }, true);
// #141: `[queue·N]`, N the needs-you cards above the tabs.
function badge() { const n = $$('#needs .ask').length; $('nav a[href="#queue"]>span').innerHTML = 'queue' + (n ? `<b class=n>·${n}</b>` : ''); }
// #68: buttons hold labels, never sentences: a control's runtime result is an 11 px `p.st` under it (red on error).
function note(b, msg, ok) {
  let s = b.nextElementSibling;
  if (!s || !s.matches('p.st')) b.after(s = Object.assign(document.createElement('p'), { className: 'st' }));
  s.textContent = msg;
  s.classList.toggle('r', !ok);
}
// Enable alerts: Home Screen app only (iOS allows push there alone); subscribe with the VAPID key, POST it.
// Unpaired (any 401): `[enable alerts]` dims, "pair this phone first" under it; kept across live swaps.
const standalone = matchMedia('(display-mode: standalone)').matches || navigator.standalone === true;
let nopair = false;
function alerts() {
  const b = $('#push');
  if (!b) return;
  b.hidden = !standalone || !('PushManager' in window);
  if (nopair) { b.disabled = true; note(b, 'pair this phone first', false); }
}
// Also shows the sheet's Pair button (open() hides it again).
function unpaired() { nopair = true; alerts(); $('.pairf').hidden = false; $$('[data-op=close]').forEach(b => b.hidden = true); }
alerts();
groups();
badge();
document.addEventListener('click', async e => {
  const b = e.target.closest('#push');
  if (!b) return;
  try {
    if (await Notification.requestPermission() !== 'granted') throw new Error('notifications are off for this app');
    const key = Uint8Array.from(atob(b.dataset.key.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
    const reg = await navigator.serviceWorker.ready;
    const s = (await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key })).toJSON();
    const r = await post('push/subscribe', { endpoint: s.endpoint, p256dh: s.keys.p256dh, auth: s.keys.auth });
    const msg = (await r.text()).trim();
    if (r.status === 401) return unpaired();
    note(b, msg, r.ok);
    if (r.ok) refresh();
  } catch (err) { note(b, 'alerts failed: ' + err.message, false); }
});
// Network only (the SW passes `frags` through); spins ↻ until done.
async function refresh() {
  dbg('refresh');
  if (DEMO) return herdPull();
  claim();
  busy = true;
  sync('a');
  try {
    const j = await (await fetch(R + 'frags', { cache: 'no-store' })).json();
    for (const id in j) frag(id, j[id]);
    herdPull();
    busy = false;
    sync(live() ? 'g' : 'a');
    pulse();
  } catch (e) { busy = false; sync('r'); }
}
// Backoff 1, 2, 5, 10 s; after 30 s down, poll `frags` every 10 s until SSE is back.
function connect() {
  if (DEMO) return;
  if (es) es.close();
  try { es = new EventSource('live' + (seq ? '?since=' + seq : '')); } catch (e) { es = null; return sync('r'); }
  // #259: every message carries the log's seq: a reconnect replays what this tab missed (`?since`), each seq once;
  // a moment plays only within 2 min of its event; past the 500-event bound the server says `refresh`.
  const at = e => (seq = Math.max(seq, +e.lastEventId || 0));
  // After a gap (the first open included: a cached page may have painted) the dot is refresh()'s: amber until `frags` lands.
  es.onopen = () => { if (down) refresh(); else sync('g'); down = 0; tries = 0; clearInterval(poller); poller = 0; };
  es.addEventListener('frag', e => { at(e); const j = JSON.parse(e.data); frag(j.id, j.html); sync('g'); pulse(); claim(); });
  es.addEventListener('herd', e => (at(e), herdData(JSON.parse(e.data))));
  es.addEventListener('ev', e => { const j = JSON.parse(e.data); if (j.seq <= seq) return; at(e); if (j.k && Date.now() / 1e3 - j.ts < 120) moment(j.k); });
  es.addEventListener('refresh', e => (at(e), refresh()));
  es.addEventListener('toast', e => (at(e), toast(JSON.parse(e.data)), refresh())); // #140: a background job finished; #217: its real result
  es.onerror = () => {
    es.close();
    es = null;
    down = down || Date.now();
    sync(navigator.onLine ? 'a' : 'r');
    if (Date.now() - down > 30000 && !poller) poller = setInterval(refresh, 10000);
    setTimeout(() => es || connect(), 1000 * [1, 2, 5, 10][Math.min(tries++, 3)]);
  };
}
connect();
sync('a');
rf.onclick = () => { refresh(); if (!live()) { tries = 0; connect(); } };
document.addEventListener('visibilitychange', () => { if (!document.hidden) { down = down || Date.now(); tries = 0; connect(); } });
addEventListener('online', () => { tries = 0; connect(); });
addEventListener('offline', () => sync('r'));
if (location.search) history.replaceState(null, '', location.pathname + (location.search.includes('all=1') ? '?all=1' : '') + location.hash);
// #66: an action's result is a one-line toast under the header: ok fades after 4 s, an error stays; a tap dismisses it.
function toast(text) {
  $$('.msg.g, .msg.r').forEach(m => m.remove());
  const m = document.createElement('p'), ok = !/^(REFUSED|error)/.test(text);
  m.className = 'msg ' + (ok ? 'g' : 'r');
  m.textContent = text;
  m.onclick = () => m.remove();
  $('header').after(m);
  if (ok) setTimeout(() => { m.classList.add('out'); setTimeout(() => m.remove(), 400); }, 4000);
}
if ($('.msg.g, .msg.r')) toast($('.msg.g, .msg.r').textContent);
// Every op form POSTs with fetch (same checks), toasts the redirect's `msg` and patches in place: the scroll stays
// (#65, #140: no navigation); the tapped button spins until the reply. The plain form POST is the no-JS fallback.
const PEND = { extend: 'one more round · starting a lane…', park: 'park · closing its lanes…', override: 'merge anyway · merging…' };
const pend = (card, op) => { card.outerHTML = `<div class=pend data-n=${card.dataset.n}><p class=m>⟳ ${PEND[op]}</p></div>`; };
document.addEventListener('submit', async e => {
  const f = e.target, b = e.submitter, c = b && b.dataset.confirm;
  if (c && !confirm(c)) return e.preventDefault(); // #130: merge anyway
  if (!f.elements.op) return;
  e.preventDefault();
  if (b) b.ariaBusy = 'true';
  const d = new FormData(f, b), k = b && b.name === 'op' && b.closest('#needs>.ask');
  if (k && PEND[b.value]) pend(k, b.value); // #217: the card is a pending line at once (no buttons, no second tap)
  try {
    const r = await post('', d), m = r.redirected ? new URL(r.url).searchParams.get('msg') || '' : 'error: ' + (await r.text()).trim();
    toast(m);
    if (!m.includes(' started for ')) refresh(); // a background job's result comes by /live and its toast
  } catch (err) { toast('error: ' + err.message); }
  if (b) b.ariaBusy = null;
});
B.insertAdjacentHTML('beforeend', `<div id=sheet><div class=bd></div><div class=sh role=dialog aria-modal=true>
<div class=hd><p><b></b></p></div><div class=top></div><div class=acts><button type=button data-op=stop>Stop</button><button type=button data-op=kill>Kill</button><button type=button data-op=clean>Clean</button><button type=button data-op=close>Close pane</button></div>
<div class=cf hidden><p></p><button type=button class=yes>Kill</button><button type=button class=no>Cancel</button></div>
<p class=fvr><button type=button class=fv aria-pressed=false>full conversation</button></p>
<div class=chat><button type=button class=more hidden>Load more</button><div class=now></div></div>
<p class=live hidden></p>
<p class=pairf hidden><button type=button class=pairb>Pair this phone</button></p>
<ul class=cmds hidden></ul>
<form class=reply><textarea name=text rows=1 maxlength=2000 placeholder="Message, or / for commands" aria-label=message></textarea><button>Send</button></form>
<p class=st></p></div></div>`);
if (DEMO) sync();
const S = $('#sheet'), shown = () => S.classList.contains('on'), sh = $('.sh'), chat = $('.chat'), st = $('.st'), now = $('.now'), more = $('.more');
let agent = null, timer = 0, nopane = false, opane = null;
// The Lanes row of `agent` (its sheet card), from the needs-you cards and the herd.
const laneOf = agent => [...$$('.lane')].find(l => l.dataset.agent === agent);
const el = (tag, cls, text) => Object.assign(document.createElement(tag), { className: cls, textContent: text || '' });
// #58: controls are bracket text, the label in a span that ellipsises (#68). A select hides behind its short `[opus ▾]` label (the native picker
// keeps full names); ▲/▼ drop their words; a choice (ask or menu option) wraps in a span, its full text in title (#63 A).
const short = o => (o ? o.textContent : '').replace(/^default.*/, 'default').replace(/.*\b(opus|sonnet|haiku)\b.*/, '$1');
// #115: one map of what each state word means: the [?] legend sheet and every state word's title.
const HINT = { queued: 'waiting its turn', impl: 'being built', 'review(n)': 'being checked, round n',
  'fix(n)': 'fixing review findings, round n', ready: 'passed, waiting to merge', merged: 'merged into staging',
  parked: 'lane closed, its work saved on the PR', dead: 'lane gone with unsaved work', 'needs you': 'waiting for your answer',
  working: 'its lane is running', idle: 'its lane waits for a next step', done: 'its lane finished its turn' };
const word = t => { const w = t.trim().split(/[\s·]/)[0].replace(/\(\d+\)/, '(n)'); return /^(needs-human|blocked|⏸)/.test(w) ? 'needs you' : w; };
const hint = t => HINT[word(t)] ? `${word(t)} = ${HINT[word(t)]}` : '';
const tip = e => { const h = hint(e.textContent); if (h) e.title = h; };
// #115: a fleet's rows as a `git log --graph`: a row blocked by a row in view hangs off it (│ ├─ └─), children in queue
// order, depth-first; past depth 2 it clamps (#162: drawn at depth 2, `after #b`), so the gutter is at most 4 columns. A blocker out of view is
// a `! #b` tag, a --serial issue the lock and its fleet's label. The card sits in a box; ▲/▼ move among siblings.
function graph(root) {
  for (const p of new Set([...root.querySelectorAll('details.q')].map(d => d.parentNode))) {
    let run = [];
    for (const c of [...p.children, null]) {
      if (c && c.matches('details.q') && !c.querySelector('.gt')) { run.push(c); continue; }
      if (run.length) tree(p, run, c);
      run = [];
    }
  }
}
function tree(p, rows, end) {
  const n = d => d.querySelector('summary>span').textContent.slice(1), by = new Map(rows.map(d => [n(d), d]));
  const up = d => by.get(d.dataset.b) || null, kids = new Map(rows.map(d => [d, []])), roots = [];
  // A blocker loop (a → b → a) draws its rows as roots.
  const par = d => { let x = up(d); const seen = new Set([d]); while (x && !seen.has(x)) { seen.add(x); x = up(x); } return x ? null : up(d); };
  rows.forEach(d => (par(d) ? kids.get(par(d)) : roots).push(d));
  const walk = (list, pre, depth) => list.forEach((d, i) => {
    const last = i === list.length - 1, deep = depth > 2, st = d.querySelector('summary>span:nth-of-type(2)'), s = st.textContent;
    const dot = st.className === 'r' || /^(⏸|needs)/.test(s) ? '!' : s === 'ready' ? '✓' : /^(impl|review|fix)/.test(s) ? '●' : '○';
    const elbow = !depth ? '' : last || deep ? '<i class=e>└─</i>' : '<i class=t>├─</i>';
    const k = kids.get(d).length && depth < 2 ? ' k' : '';
    d.classList.add('d' + Math.min(depth, 2));
    d.querySelector('summary').insertAdjacentHTML('afterbegin', `<i class=gt>${pre.slice(0, 1).join('')}${elbow}<b class="${st.className}${k}">${dot}</b></i>`);
    const w = el('span', '');
    st.replaceWith(w);
    w.append(st);
    tip(st);
    const tag = (cls, text, title = '') => { const t = el('span', cls, text); if (title) { t.title = title; t.setAttribute('aria-label', title); } w.append(' ', t); };
    if (d.dataset.s) tag('tag m', '🔒' + d.dataset.s, 'serial: one lane at a time');
    if (d.dataset.b && !par(d)) tag('tag r', '! #' + d.dataset.b);
    if (deep) tag('m', 'after #' + d.dataset.b);
    const cd = el('div', 'cd');
    cd.append(...[...d.children].filter(c => c.tagName !== 'SUMMARY'));
    d.append(cd);
    // The parser lifts the ▲/▼ forms out of their `<p><span class=btns>`; they go back in one `.btns` line.
    const sib = list.filter(x => 'q' in x.dataset), j = sib.indexOf(d), old = cd.querySelectorAll('form:has([name=before],[name=after])');
    const mv = (label, key, to) => { const c = old[0].cloneNode(true), i = c.querySelector('[name=before],[name=after]'); i.name = key; i.value = n(to); c.querySelector('button').textContent = label; return c; };
    const fs = old.length ? [j > 0 && mv('▲ up', 'before', sib[j - 1]), sib[j + 1] && mv('▼ down', 'after', sib[j + 1])].filter(Boolean) : [];
    cd.querySelectorAll('.btns').forEach(b => b.parentNode.remove());
    old.forEach(f => f.remove());
    if (fs.length) { const line = el('p', 'btns'); line.append(...fs); cd.append(line); }
    // #218: the card in sections: STATUS (stage, lane, notes), LINKS, LANES (n) newest first, HISTORY (+N older).
    const ls = cd.querySelector(':scope>.qls'), lk = [...cd.children].find(c => c.matches('p') && c.querySelector('a[href*="/issues/"]'));
    const ch = t => el('p', 'ch', t), hist = ls && ls.querySelector(':scope>.qm');
    if (lk) { cd.insertBefore(lk, ls || cd.querySelector(':scope>.btns')); lk.before(ch('LINKS')); }
    if (cd.firstElementChild !== lk?.previousElementSibling) cd.prepend(ch('STATUS'));
    if (ls) ls.before(ch(`LANES (${ls.querySelectorAll(':scope>.ql').length})`));
    if (hist) ls.after(ch('HISTORY'), hist);
    p.insertBefore(d, end);
    walk(kids.get(d), depth ? [...pre, last ? '<i>  </i>' : '<i class=p>│ </i>'] : [], depth + 1);
  });
  walk(roots, [], 0);
}
// #170: the Events chips, kept per phone (`events:filter` → {"k":"imp","fl":["atlas"]}); [important], every fleet by default.
let EF = null;
try { EF = JSON.parse(get('events:filter')); } catch (e) {}
if (!EF || !['all', 'imp'].includes(EF.k) || !Array.isArray(EF.fl)) EF = { k: 'imp', fl: [] };
function evf(root) {
  const fls = [...root.querySelectorAll('.chips [data-fl]')].map(b => b.dataset.fl);
  if (fls.length) EF.fl = EF.fl.filter(f => fls.includes(f)); // a fleet gone from fleet.toml no longer hides every row
  root.querySelectorAll('.chips [data-k]').forEach(b => b.setAttribute('aria-pressed', b.dataset.k === EF.k));
  root.querySelectorAll('.chips [data-fl]').forEach(b => b.setAttribute('aria-pressed', EF.fl.includes(b.dataset.fl)));
  root.querySelectorAll('[data-v]').forEach(v => v.hidden = v.dataset.v !== EF.k);
  root.querySelectorAll('.er').forEach(r => r.hidden = EF.fl.length > 0 && !EF.fl.includes(r.dataset.f));
  root.querySelectorAll('.eg').forEach(g => {
    const n = g.querySelectorAll('.er:not([hidden])').length;
    g.hidden = !n;
    g.querySelector('h3>span+span').textContent = n;
  });
}
document.addEventListener('click', e => {
  const b = e.target.closest('.chips button'), f = b && b.dataset.fl;
  if (!b) return;
  if (b.dataset.k) EF.k = b.dataset.k; else EF.fl = EF.fl.includes(f) ? EF.fl.filter(x => x !== f) : [...EF.fl, f];
  put('events:filter', JSON.stringify(EF));
  evf(document);
});
function tidy(root) {
  graph(root);
  rows(root);
  evf(root);
  hostrow(root);
  root.querySelectorAll('.lr>span:nth-of-type(3)').forEach(tip);
  root.querySelectorAll('select').forEach(s => {
    if (s.parentNode.matches('.pk')) return;
    const k = el('label', 'pk'), t = el('span', '');
    s.replaceWith(k);
    k.append(t, s);
    const show = () => { t.textContent = short(s.selectedOptions[0]) + ' ▾'; k.title = s.selectedOptions[0] ? s.selectedOptions[0].textContent : ''; };
    s.addEventListener('change', show);
    show();
  });
  for (const b of root.querySelectorAll('button')) {
    const m = b.textContent.match(/^([▲▼]) (up|down)$/);
    if (m) { b.textContent = m[1]; b.setAttribute('aria-label', 'move ' + m[2]); }
    if (b.matches('.lr, .hh, .hqh') || (b.childNodes.length === 1 && b.firstChild.nodeName === 'SPAN')) continue;
    const t = el('span', '');
    t.append(...b.childNodes);
    b.replaceChildren(t);
    if (b.matches('[name=choice], .pick')) b.title = b.textContent;
  }
}
tidy(document);
nows();
// #305: the theme's `loading` sprite (herd.js builds #hload and sets .ld), drawn in the ↻'s place while it spins.
rf.innerHTML = '<i>↻</i><svg class=lds viewBox="0 0 132 100" style="--c:#c89a56;--d:#a37a3c;--m:#e8dcc0;--b:#efe7dc" aria-hidden=true><use href="#hload"/></svg>';
// #138 chat blocks: pure from here to `chat:end` (tests/chat_blocks.rs runs it under node). A role run is a block:
// you (a typed prompt), the assistant (prose), a tool run (`⏺ Name(…)`/`•`/`$`/`┊` plus its `⎿`/indented output),
// status (a line all in grey: `Ran 2 shell commands`), and the gap line where the rolling transcript missed lines.
const GAP = '┄ not captured: scrolled past between reads ┄';
const esc = s => s.replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const plain = l => l.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '');
// A terminal colour to the nearest board token by hue (no terminal hex reaches the page); grey is the default text
// when light, `m` when dark. Bands: red < 12° or ≥ 330°, amber < 70°, green < 160°, cyan < 200°, blue < 255°, else purple.
function tone(r, g, b) {
  const mx = Math.max(r, g, b), d = mx - Math.min(r, g, b);
  if (d < 40) return mx >= 200 ? '' : 'm';
  const h = (60 * (mx === r ? (g - b) / d : mx === g ? (b - r) / d + 2 : (r - g) / d + 4) + 360) % 360;
  return h < 12 || h >= 330 ? 'r' : h < 70 ? 'a' : h < 160 ? 'g' : h < 200 ? 'c' : h < 255 ? 'l' : 'p';
}
const BASIC = ['m', 'r', 'g', 'a', 'l', 'p', 'c', ''], CUBE = [0, 95, 135, 175, 215, 255];
const rgb256 = n => n < 232 ? [CUBE[(n - 16) / 36 | 0], CUBE[((n - 16) / 6 | 0) % 6], CUBE[(n - 16) % 6]] : Array(3).fill(8 + 10 * (n - 232));
// An ANSI line as [text, classes] runs: fg `t-<token>`, bold `t-b`, dim `t-m`, and red/green backgrounds (diffs)
// as `bg-r`/`bg-g`, `…2` when bright (changed words).
function runs(line) {
  const out = []; let fg = '', bg = '', b = false, dim = false;
  const col = (c, i) => c[i] === 5 ? [rgb256(c[i + 1]), i + 1] : [c.slice(i + 1, i + 4), i + 3];
  const tint = ([r, g, bl]) => { const t = tone(r, g, bl), x = c => c[0] === c[1] && c[1] === c[2] ? '' : c; return /^[rg]$/.test(t) ? `bg-${t}${Math.max(r, g, bl) > 65 ? '2' : ''}` : x([r, g, bl]) && ''; };
  for (const [i, part] of line.split('\x1b[').entries()) {
    const m = i ? /^([0-9;?]*)([A-Za-z])/.exec(part) : null;
    if (m && m[2] === 'm') {
      const c = m[1].split(';').map(Number);
      for (let j = 0; j < c.length; j++) {
        const n = c[j];
        if (n === 0) { fg = bg = ''; b = dim = false; } else if (n === 1) b = true; else if (n === 2) dim = true;
        else if (n === 22) b = dim = false; else if (n === 39) fg = ''; else if (n === 49) bg = '';
        else if (n >= 30 && n <= 37 || n >= 90 && n <= 97) fg = BASIC[n % 10];
        else if (n === 41 || n === 42) bg = n === 41 ? 'bg-r' : 'bg-g';
        else if (n === 38 || n === 48) {
          const [v, k] = c[j + 1] === 5 && c[j + 2] < 16 ? [null, j + 2] : col(c, j + 1);
          if (n === 38) fg = v ? tone(...v) : BASIC[c[j + 2] % 8]; else bg = v ? tint(v) : '';
          j = k;
        }
      }
    }
    const t = m ? part.slice(m[0].length) : part;
    const cls = [fg ? 't-' + fg : dim ? 't-m' : '', b ? 't-b' : '', bg].filter(Boolean).join(' ');
    if (t && out.length && out[out.length - 1][1] === cls) out[out.length - 1][0] += t; else if (t) out.push([t, cls]);
  }
  return out;
}
const span = ([t, c], md) => {
  if (md && c === 't-l') return `<code>${esc(t)}</code>`;
  const h = md ? esc(t).replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/`([^`]+)`/g, '<code>$1</code>') : esc(t);
  return c ? `<span class="${c}">${h}</span>` : h;
};
// Prose (md): Claude's code runs (blue) and raw markdown's `code` become chips, `**bold**` bold; text is escaped first.
const html = (rs, md) => rs.map(r => span(r, md)).join('');
// Drop the first n characters of runs.
const cut = (rs, n) => rs.flatMap(([t, c]) => { const k = Math.min(n, t.length); n -= k; return k < t.length ? [[t.slice(k), c]] : []; });
const muted = rs => rs.some(([t]) => t.trim()) && rs.every(([t, c]) => !t.trim() || /\bt-m\b/.test(c));
const pad = p => p.length - p.trimStart().length;
// Lines to role runs: {k: me | ai | run | qs | gap, head, lines}.
function group(lines) {
  const out = []; let cur = null;
  const add = (k, l) => out.push(cur = { k, head: l, lines: [], ind: pad(plain(l)) });
  for (const l of lines) {
    const p = plain(l), t = p.trim(), n = pad(p);
    if (!t) { if (cur && /me|ai|run/.test(cur.k)) cur.lines.push(l); }
    else if (t === GAP) add('gap', l);
    else if (/^[❯›>]\s/.test(t)) add('me', l);
    else if (/^[⏺•]\s*\w+\(/.test(t) || /^(\$|┊|💻)\s/.test(t)) add('run', l);
    else if (cur && cur.k === 'run' && (t[0] === '⎿' || n > cur.ind + 2)) cur.lines.push(l);
    else if (muted(runs(l))) add('qs', l);
    else if (cur && !/^[⏺•]/.test(t) && (cur.k === 'ai' || cur.k === 'me' && n > cur.ind)) cur.lines.push(l);
    else add('ai', l);
  }
  return out;
}
// Minimal markdown on prose: paragraphs (wrapped lines joined), `#` headings, `-`/`1.` lists.
function prose(lines) {
  const items = []; let last = null;
  lines.forEach((l, i) => {
    const rs0 = runs(l), lead = /^\s*([⏺❯›>•]\s*)?/.exec(plain(l))[0].length, rs = cut(rs0, i ? pad(plain(l)) : lead), t = rs.map(r => r[0]).join('');
    let m;
    if (!t.trim()) last = null;
    else if ((m = /^#{1,6}\s+/.exec(t))) { items.push({ t: 'h', h: [html(cut(rs, m[0].length), true)] }); last = null; }
    else if ((m = /^([-*•]|\d+[.)])\s+/.exec(t))) {
      const tag = /\d/.test(m[1]) ? 'ol' : 'ul', end = items[items.length - 1];
      last = end && end.t === tag ? end : (items.push({ t: tag, h: [] }), items[items.length - 1]);
      last.h.push(html(cut(rs, m[0].length), true));
    } else if (last) last.h[last.h.length - 1] += ' ' + html(rs, true);
    else items.push(last = { t: 'p', h: [html(rs, true)] });
  });
  return items.map(x => x.t === 'p' ? `<p>${x.h[0]}</p>` : x.t === 'h' ? `<p class=h>${x.h[0]}</p>` : `<${x.t}>${x.h.map(h => `<li>${h}</li>`).join('')}</${x.t}>`).join('');
}
// A tool run's output: `⎿` and the output indent dropped, each line a hanging indent under its own start (past a diff
// gutter `52 -`); over 20 lines, the first 20, or on a failure the first and last 10 around `⋯ N lines`.
function output(lines, fail) {
  while (lines.length && !plain(lines[lines.length - 1]).trim()) lines = lines.slice(0, -1);
  const n0 = lines.length ? /^[\s⎿]*/.exec(plain(lines[0]))[0].length : 0;
  const row = l => {
    let rs = runs(l); rs = cut(rs, Math.min(n0, /^[\s⎿]*/.exec(plain(l))[0].length));
    const t = rs.map(r => r[0]).join(''), bg = (rs.find(r => /bg-[rg]\b/.test(r[1])) || ['', ''])[1].match(/bg-[rg]\b/);
    return `<span class="ol${bg ? ' ' + bg[0] : ''}" style="--i:${/^(\s*\d+ [-+ ]?)?\s*/.exec(t)[0].length}">${html(rs) || ' '}</span>`;
  };
  const n = lines.length, all = lines.map(row);
  if (n <= 20) return `<div class=out><pre>${all.join('')}</pre></div>`;
  fail = fail || lines.some(l => /FAILED|error/.test(plain(l)));
  const [a, b] = fail ? [10, n - 10] : [20, n];
  return `<div class="out cap"><pre>${all.slice(0, a).join('')}<span class=rest>${all.slice(a, b).join('')}</span>` +
    (fail ? `<span class="ol cut t-m">⋯ ${n - 20} lines</span>${all.slice(b).join('')}` : '') +
    `</pre><p class=more2><button type=button><span>show all</span></button><span class=m>20 of ${n} lines</span></p></div>`;
}
// Blocks as {key, k, html, open}: tool runs start collapsed, you and the assistant open (Q4); status is a quiet line.
// `busy`: the live status line shows activity, so a last tool run with no output yet is `running`.
function blocks(lines, harness, busy) {
  const gs = group(lines);
  return gs.map((g, i) => {
    const head = plain(g.head).trim(), key = g.k + ':' + head + ':' + plain(g.lines[0] || '').trim();
    if (g.k === 'qs' || g.k === 'gap') return { key, k: g.k, html: `<p class="qs${g.k === 'gap' ? ' gap' : ''}">${html(cut(runs(g.head), pad(plain(g.head))))}</p>` };
    const sum = (k, rest) => `<summary><span class=k>${k}</span>${rest}<i class=cv></i></summary>`;
    if (g.k !== 'run') {
      const body = [g.head, ...g.lines], pv = esc(head.replace(/^[⏺❯›>•]\s*(#+\s+)?/, ''));
      return { key, k: g.k, open: true, html: `<details class="blk ${g.k}">${sum(g.k === 'me' ? 'you' : esc(harness || 'agent'), `<span class=pv>${pv}</span>`)}<div class=md>${prose(body)}</div></details>` };
    }
    const m = /^(?:[⏺•]\s*(\w+)\((.*?)\)?(\s+·.*)?|(?:┊\s*)?(?:💻\s*)?(\$|terminal)?\s*(.*))$/s.exec(head);
    const name = m[1] || (m[4] ? '$' : ''), shell = !name || /^(\$|Bash|Shell|terminal)$/.test(name);
    const cmd = m[1] ? m[2] : m[5], text = g.lines.map(plain).join('\n'), dot = runs(g.head).find(r => r[0].trim());
    const fail = !!dot && /\bt-r\b/.test(dot[1]), code = /exit code (\d+)/i.exec(text);
    const tag = fail ? `<span class=t-r>${code ? 'exit ' + code[1] : 'failed'}</span>` : busy && i === gs.length - 1 && !text.trim() ? '<span class=t-a>running</span>' : '';
    return { key, k: 'run', html: `<details class="blk run">${sum(shell ? '$' : esc(name), `<span class=cmd>${esc(cmd)}</span>${tag}`)}${text.trim() ? output(g.lines, fail) : ''}</details>` };
  });
}
// #260 the default view: prompts, each turn's final reply, asks and herdsman notices (`{at, text}`: spawned, ask,
// verdict, merged). #315 screen-first: the pane's role runs (you, and the last assistant run before each prompt; none:
// every block), the notices first (pane lines have no times); `said` (Claude's session transcript, `[{k: me | ai, text,
// at}]`), when it has a message, is an enhancement in time order. A Claude lane without one says so in a line.
function matters(lines, said, notices, harness) {
  const note = (key, t, at) => ({ key, k: 'note', at, html: `<p class="qs note">${esc(t)}</p>` });
  const ns = (notices || []).map(n => note('n:' + n.at + ':' + n.text, n.text, n.at * 1000));
  if (!said || !said.length) {
    const bs = blocks(lines, harness), last = i => (bs.slice(i + 1).find(b => /^(me|ai)$/.test(b.k)) || {}).k !== 'ai';
    const rs = bs.filter((b, i) => b.k === 'me' || b.k === 'ai' && last(i));
    return (harness === 'claude' ? [note('n:none', 'no transcript on this host yet')] : []).concat(ns, rs.length ? rs : bs);
  }
  const say = s => {
    const ls = s.text.split('\n'), who = s.k === 'me' ? 'you' : esc(harness || 'agent'), pv = esc(ls[0].replace(/^#+\s+/, ''));
    return { key: s.k + ':' + s.at, k: s.k, open: true, at: Date.parse(s.at) || 0, html: `<details class="blk ${s.k}"><summary><span class=k>${who}</span><span class=pv>${pv}</span><i class=cv></i></summary><div class=md>${prose(ls)}</div></details>` };
  };
  return ns.concat(said.map(say)).sort((a, b) => a.at - b.at);
}
// The longest suffix of a that is a prefix of b (as text): two reads' overlap.
const overlap = (a, b) => { for (let k = Math.min(a.length, b.length); k > 0; k--) if (a.slice(-k).every((x, i) => plain(x) === plain(b[i]))) return k; return 0; };
// chat:end
const lane = (q, a = agent) => fetch(DEMO ? 'lane/' + encodeURIComponent(a) + '.json' : R + 'lane?agent=' + encodeURIComponent(a) + '&lines=200' + q);
// The settled lines seen (merged across reads by their overlap) plus the live block, drawn as blocks: a block whose key
// and HTML are unchanged keeps its node (no scroll jump, #65), new lines land in the last block; open state per lane.
let hist = [], tail = [], nlive = 0, harness = '', spin = false, said = null, notes = [];
// #260 [full conversation]: every tool run and output (the pane view), else what matters; remembered per phone.
let full = false;
try { full = localStorage.getItem('chat:full') === '1'; } catch (e) {}
const fv = $('.fv');
fv.ariaPressed = String(full);
fv.onclick = () => {
  full = !full;
  fv.ariaPressed = String(full);
  try { localStorage.setItem('chat:full', full ? '1' : ''); } catch (e) {}
  paint();
  chat.scrollTop = chat.scrollHeight;
};
const store = () => { try { return JSON.parse(sessionStorage.getItem('chat:' + agent)) || {}; } catch (e) { return {}; } };
function paint() {
  const seen = new Map([...now.children].map(n => [n.dataset.k, n])), keep = store(), end = chat.scrollTop + chat.clientHeight >= chat.scrollHeight - 8;
  const ls = hist.concat(tail);
  let bs;
  // #315: a view that fails to draw falls back to the pane view; neither is blank while the pane has lines.
  try { bs = full ? blocks(ls, harness, spin) : matters(ls, said, notes, harness); } catch (e) { console.warn('chat', e); bs = blocks(ls, harness, spin); }
  now.replaceChildren(...bs.map(b => {
    const n = seen.get(b.key);
    if (n && n.dataset.h === b.html) return n;
    const t = document.createElement('template');
    t.innerHTML = b.html;
    const x = t.content.firstChild;
    Object.assign(x.dataset, { k: b.key, h: b.html });
    if (x.tagName === 'DETAILS') x.open = keep[b.key] ?? (n ? n.open : !!b.open);
    return x;
  }));
  if (end) chat.scrollTop = chat.scrollHeight;
}
async function poll() {
  // #83: a lane with no pane has no transcript to read.
  if (nopane) return now.replaceChildren(el('p', 'm', 'no pane: the transcript closed with it'));
  try {
    const r = await lane('');
    // Unpaired (401): say so, with the Pair button; never a silent one-liner.
    if (r.status === 401) {
      unpaired();
      return now.replaceChildren(el('p', 'unp', 'Pair this phone to see the conversation'));
    }
    if (!r.ok) return now.replaceChildren(el('p', 'm', (await r.text()).trim()));
    const j = await r.json(), done = j.lines.slice(0, j.lines.length - j.live), k = overlap(hist, done);
    opane = j.pane;
    if (!hist.length) more.hidden = !j.more;
    hist = k || !hist.length || !done.length ? hist.concat(done.slice(k)) : done;
    [tail, nlive, harness, spin, said, notes] = [j.lines.slice(done.length), j.live, j.harness, !!j.status, j.said, j.notices];
    $('.live').hidden = !j.status;
    if (j.status) $('.live').innerHTML = `<b>${esc([...j.status][0])}</b>${esc(j.status.slice([...j.status][0].length))}`;
    paint();
  } catch (e) { now.textContent = 'no pane output'; }
}
// Older lines from the rolling transcript, joined above the seen ones by their overlap.
more.onclick = async () => {
  const r = await lane('&before=' + Math.max(0, hist.length + nlive - 20));
  if (!r.ok) return;
  const j = await r.json(), h = chat.scrollHeight;
  hist = j.lines.slice(0, j.lines.length - overlap(j.lines, hist)).concat(hist);
  more.hidden = !j.more;
  paint();
  chat.scrollTop += chat.scrollHeight - h;
};
chat.addEventListener('toggle', e => {
  const k = e.target.dataset && e.target.dataset.k, s = store();
  if (!k) return;
  s[k] = e.target.open;
  try { sessionStorage.setItem('chat:' + agent, JSON.stringify(s)); } catch (err) {}
}, true);
// [show all] / [show less] on a capped tool output.
chat.addEventListener('click', e => {
  const b = e.target.closest('.more2 button'), o = b && b.closest('.out');
  if (o) b.firstChild.textContent = o.classList.toggle('all') ? 'show less' : 'show all';
});
// `ok` null: a neutral note, e.g. why a control is disabled (#83).
function say(msg, ok) { st.textContent = msg; st.className = 'st ' + (ok == null ? 'm' : ok ? 'g' : 'r'); }
// Stop/Kill/Clean call `/act`; kill and clean dry-run first (kill shows what is lost, clean why it is unsafe).
// #95: an Others agent (not a lane) gets only Close pane, confirmed here before `/act` closes it.
const act = (op, confirm) => post('act', { agent: op === 'close' && opane || agent, op, confirm: confirm ? '1' : '' });
async function run(op) {
  const r = op === 'close' ? null : await act(op, op === 'stop');
  const msg = r ? (await r.text()).trim() : `close ${agent}'s pane? its worktree and files stay`;
  if (r && r.status === 401) unpaired();
  if (op === 'stop' || (r && !r.ok)) return say(msg, r.ok);
  $('.cf p').textContent = msg.split('; ').find(l => !l.startsWith('dry run')) || msg;
  $('.cf .yes span').textContent = { kill: 'Kill', clean: 'Clean', close: 'Close' }[op];
  $('.cf').dataset.op = op;
  $('.cf').hidden = false;
}
$('.acts').onclick = e => { const b = e.target.closest('button'); if (b) run(b.dataset.op); };
$('.cf .no').onclick = () => { $('.cf').hidden = true; };
$('.cf .yes').onclick = async () => {
  const r = await act($('.cf').dataset.op, true);
  $('.cf').hidden = true;
  say((await r.text()).trim(), r.ok);
  refresh();
};
async function cleanable() {
  const b = $('[data-op=clean]'), r = await act('clean', false), msg = (await r.text()).trim();
  b.disabled = !r.ok;
  b.title = r.ok ? '' : msg;
  if (!r.ok && r.status !== 401 && !st.textContent) say('clean: ' + msg.split(': ').pop(), null);
}
function open(e) {
  agent = e.dataset.agent;
  opane = null;
  const top = $('.top');
  top.innerHTML = e.querySelector('.card').innerHTML;
  top.scrollTop = 0;
  $('.hd b').textContent = agent;
  const rev = e.dataset.role === 'rev';
  $('.reply').hidden = !!B.dataset.ro || rev;
  $('.acts').hidden = !!B.dataset.ro;
  // #83: a lane with no pane has nothing to stop or kill; #95: an Others agent is no lane, only its pane closes.
  nopane = 'gone' in e.dataset;
  const other = e.dataset.role === 'other';
  $$('[data-op=stop], [data-op=kill]').forEach(b => b.hidden = nopane || other);
  $('[data-op=clean]').hidden = other;
  $('[data-op=close]').hidden = !other || nopair;
  $$('.pairf, .cf, .cmds').forEach(x => x.hidden = true);
  now.replaceChildren();
  hist = []; tail = []; said = null; notes = [];
  $('.live').hidden = true;
  say(rev ? 'reviewer lane: read-only' : other ? 'not a herdsman lane: herdsman can only close its pane' : '', other ? null : true);
  sh.className = 'sh';
  show(kb() ? 'full' : 'mid');
  poll();
  if (!other && !B.dataset.ro && !DEMO) cleanable();
  timer = setInterval(poll, 5000);
}
// #260: a Queue card's lane shows its Lanes row's `now:` line under it (that row's, as /live last drew it).
function nows() {
  $$('#queue .qls>.ql[data-agent]').forEach(q => {
    const r = laneOf(q.dataset.agent), t = r ? r.querySelector('.act').textContent : '', n = q.nextElementSibling;
    const x = n && n.matches('.qnow') ? n : null;
    if (!t.startsWith('now: ')) return x && x.remove();
    (x || q.insertAdjacentElement('afterend', el('span', 'qnow nw el m'))).textContent = t;
  });
}
// #218: a Queue card's lane opens by its name, not by a Lanes row (that tab may not list it): one transcript read
// first, then a sheet of its own; a gone lane shows its stored transcript, read-only. No transcript: a toast.
async function byName(q) {
  const a = q.dataset.agent, row = !('gone' in q.dataset) && laneOf(a);
  if (row) return open(row);
  const r = await lane('', a).catch(() => null);
  if (!r || (!r.ok && r.status !== 401)) return toast(`lane ${a} is gone`);
  const x = el('div');
  x.innerHTML = '<div class=card><p class="nw el"></p></div>';
  x.querySelector('p').textContent = q.textContent.trim();
  Object.assign(x.dataset, { agent: a, role: /-rev-/.test(a) ? 'rev' : 'impl' });
  open(x);
  if ('gone' in q.dataset) {
    $$('[data-op=stop], [data-op=kill], .reply').forEach(b => b.hidden = true);
    say('lane gone: its last stored transcript', null);
  }
}
// #115 amendment A: [?] shows the legend in the same sheet, its lane parts hidden (`.lg`): a line per state from HINT,
// the dots, and each fleet's serial mark with the paths it guards (none without serial_paths).
function legend() {
  agent = null;
  sh.className = 'sh lg';
  $('.hd b').textContent = 'what the marks mean';
  const serial = new Set([...$$('#queue [data-sp]')].map(f => `🔒${f.dataset.s} one at a time: touches ${f.dataset.sp}`));
  const dots = ['● a lane is on it', '○ waiting', '✓ ready', '! needs you'];
  $('.top').replaceChildren(...Object.entries(HINT).map(([w, h]) => el('p', '', `${w} = ${h}`)), ...[...dots, ...serial].map(t => el('p', '', t)));
  show('mid');
}
// #162: [config] shows the fleet's settings in the same sheet, the lane parts hidden and its bar in the fleet's colour.
// The page loads it (`frag?id=config`); an export carries it inline (`template[data-cfg]`).
async function config(b) {
  const f = b.dataset.f, t = [...$$('template[data-cfg]')].find(t => t.dataset.cfg === f);
  const h = t ? t.innerHTML : await part('frag?id=config&fleet=' + encodeURIComponent(f));
  if (h === null) return toast('error: cannot read the config');
  agent = null;
  sh.className = 'sh lg cs ' + [...b.closest('.fc').classList].find(c => c.startsWith('k-'));
  $('.hd b').textContent = f + ' · config';
  const top = $('.top');
  top.innerHTML = h;
  top.querySelectorAll('[data-pipe]').forEach(pipeline); // #173
  top.scrollTop = 0;
  tidy(top);
  top.querySelectorAll('form button, form select:not([data-off])').forEach(c => c.disabled = DEMO || dot.className === 'r');
  show('full');
}
// A role's effort list follows its harness (each harness option's data-e).
document.addEventListener('change', e => {
  const s = e.target, ef = s.name === 'harness' && s.form.elements.effort;
  if (!ef) return;
  const cur = ef.value;
  ef.replaceChildren(...s.selectedOptions[0].dataset.e.split(' ').map(v => Object.assign(el('option', '', v), { selected: v === cur })));
  ef.dispatchEvent(new Event('change'));
});
// #166: one helper for every sheet (lane chat, legend, config). #200: the page under it is never touched while it
// opens or closes, not even <html>'s overflow (iOS repainted the whole page grey), and the sheet stays in the DOM:
// shown is `.on` (visible), only transform and opacity animate; the backdrop alone eats the page's touchmove.
function show(d) {
  if (shut) { clearTimeout(shut); shut = 0; S.classList.remove('shut'); } else {
    S.classList.add('on', 'drag');
    pos(sh.offsetHeight, 0);
    void sh.offsetHeight;
    S.classList.remove('drag');
  }
  fit();
  snap(d);
}
// Only the sheet slides off (200 ms ease-out, none under reduced motion) and the backdrop fades; when the slide ends
// (or 250 ms on) it turns invisible.
function close() {
  if (!shown() || shut) return;
  clearInterval(timer);
  agent = null;
  d0 = null;
  S.classList.remove('drag');
  S.classList.add('shut');
  pos(sh.offsetHeight);
  shut = setTimeout(hide, matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 250);
}
function hide() {
  clearTimeout(shut);
  shut = 0;
  S.classList.remove('on', 'shut');
}
sh.addEventListener('transitionend', e => { if (shut && e.target === sh && e.propertyName === 'transform') hide(); });
// #70: the sheet is a full-height box placed by translateY at a detent (`full` stops under the safe area).
// pos() moves it, fades the backdrop with it, and pads its bottom by the hidden part so the composer shows.
const DET = { full: 1, mid: .78, half: .5 }, bd = $('.bd');
bd.addEventListener('touchmove', e => e.preventDefault(), { passive: false });
let at = 'mid', ty = 0, shut = 0, padT = 0, d0 = null, moves = [], to = null, rq = 0;
const off = d => Math.max(0, S.clientHeight * (1 - DET[d]) - sh.offsetTop);
function pos(t, pad) {
  ty = t;
  sh.style.transform = `translateY(${t}px)`;
  const h = sh.offsetHeight, half = off('half');
  bd.style.opacity = Math.min(1, Math.max(0, (h - t) / (h - half || 1)));
  if (pad !== undefined) sh.style.paddingBottom = `calc(${pad}px + env(safe-area-inset-bottom))`;
}
// Animate to a detent; the composer stays on screen (pad by the larger offset, then the detent's once settled).
function snap(d) {
  at = d;
  sh.classList.toggle('full', d === 'full');
  const t = off(d);
  pos(t, Math.max(t, ty));
  clearTimeout(padT);
  padT = setTimeout(() => pos(t, t), 300);
}
// The sheet follows the visual viewport: the composer stays above the keyboard and returns when it closes.
// While the keyboard is up (a field in the sheet focused) the sheet is full and does not drag.
const vv = window.visualViewport;
const kb = () => !!sh.querySelector('input:focus, textarea:focus') || !!vv && vv.height < innerHeight * .75;
function fit() {
  if (shut) return;
  if (vv) { S.style.top = vv.offsetTop + 'px'; S.style.height = vv.height + 'px'; }
  if (shown() && !d0) snap(kb() ? 'full' : at);
}
if (vv) { vv.addEventListener('resize', fit); vv.addEventListener('scroll', fit); }
sh.addEventListener('focusin', fit);
sh.addEventListener('focusout', () => setTimeout(fit, 100));
// `/` first in the reply box: a filtered command list (tap or Enter); stop/kill/clean act, the rest go as text.
const CMDS = ['/stop', '/kill', '/clean', '/compact', '/clear', '/status'], ta = $('.reply textarea'), cmds = $('.cmds');
const matches = () => /^\/\S*$/.test(ta.value) ? CMDS.filter(c => c.startsWith(ta.value)) : [];
function pick(c) {
  cmds.hidden = true;
  if (['/stop', '/kill', '/clean'].includes(c)) { ta.value = ''; return run(c.slice(1)); }
  ta.value = c;
  $('.reply').requestSubmit();
}
ta.oninput = () => {
  const m = matches();
  cmds.replaceChildren(...m.map(c => el('li', '', c)));
  cmds.hidden = !m.length;
};
ta.onkeydown = e => { if (e.key === 'Enter' && !e.shiftKey && matches().length) { e.preventDefault(); pick(matches()[0]); } };
cmds.onclick = e => { const li = e.target.closest('li'); if (li) pick(li.textContent); };
// #154: [later] hides a needs-you card on this phone (localStorage `later`: key → {until, sig}) for 12 h, until its
// content changes, or until it is gone. Hidden cards fold into one red line before their list (#needs, each Herd
// page's .hn), `n need you ▸ [later all]`; a tap on it shows them in place. View only: the cards stay in the DOM.
let unfold = false;
const lget = () => { try { return JSON.parse(get('later')) || {}; } catch (e) { return {}; } };
// key: the kind plus fleet, issue and agent (the header less its age); sig: a short hash of the rest (verdict, ask id, prompt).
const ckey = c => (c.dataset.ask ? 'prompt' : c.dataset.n ? 'nh' : 'ask') + ' ' + ((c.querySelector('.m') || {}).textContent || '').split(' · ').filter(s => !/^\d+[smhd]$/.test(s)).join(' · ');
function csig(c) {
  const x = c.cloneNode(true);
  x.querySelectorAll(':scope>p:first-child, .cd, .st, .as, .lat').forEach(e => e.remove());
  let h = 0;
  for (const ch of x.textContent.replace(/\b\d+[smhd]\b/g, '') + ((x.querySelector('[name=id]') || {}).value || '')) h = Math.imul(h, 31) + ch.charCodeAt(0) | 0;
  return (h >>> 0).toString(36);
}
function fold() {
  const st = lget(), now = Date.now(), keep = {};
  B.classList.toggle('unf', unfold);
  for (const box of [$('#needs'), ...$$('.hn')]) {
    let n = 0;
    for (const c of box.querySelectorAll(':scope>.ask')) {
      if (!c.querySelector(':scope>.lat')) c.append(Object.assign(document.createElement('button'), { type: 'button', className: 'lat', textContent: 'later' }));
      const k = ckey(c), e = st[k], off = !!e && e.until > now && e.sig === csig(c);
      if (off) { keep[k] = e; n++; }
      c.classList.toggle('lt', off);
    }
    let l = box.previousElementSibling;
    if (!l || !l.matches('.later')) box.before(l = Object.assign(document.createElement('div'), { className: 'later', innerHTML: '<button type=button class=lo></button><button type=button class=lall>later all</button>' }));
    l.hidden = !n;
    l.firstChild.textContent = `${n} need you ${unfold ? '▾' : '▸'}`;
  }
  put('later', JSON.stringify(keep));
}
fold();
document.addEventListener('click', e => {
  const b = e.target.closest('.lat, .later button');
  if (!b) return;
  const st = lget(), hide = c => { st[ckey(c)] = { until: Date.now() + 12 * 3600e3, sig: csig(c) }; };
  if (b.matches('.lo')) unfold = !unfold;
  else if (b.matches('.lat')) hide(b.closest('.ask'));
  else { b.parentNode.nextElementSibling.querySelectorAll(':scope>.ask' + (unfold ? '' : ':not(.lt)')).forEach(hide); unfold = false; }
  put('later', JSON.stringify(st));
  fold();
});
// Delegated: rows and needs-you cards are swapped in place. A menu option POSTs `/answer`.
// #127: a prompt's command expands on tap; `auto-denies in m:ss` counts down from the pane read.
document.addEventListener('click', e => { const c = e.target.closest('.ask .command'); if (c) c.classList.toggle('open'); });
setInterval(() => $$('.ask .cd').forEach(c => {
  const s = Math.max(0, c.dataset.until - Math.floor(Date.now() / 1000));
  c.textContent = `auto-denies in ${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}), 1000);
document.addEventListener('click', async e => {
  if (e.target.closest('.qhelp')) return legend();
  if (e.target.closest('.cfg')) return config(e.target.closest('.cfg'));
  // #205: a lane in an open Queue card opens its Lanes sheet in place; closing returns to the card.
  const q = e.target.closest('a.ql');
  if (q) { e.preventDefault(); return byName(q); }
  const b = e.target.closest('.lane .lr, .ask .pick, .ask .rp');
  if (!b) return;
  if (b.classList.contains('lr')) return open(b.parentNode);
  const a = b.closest('.ask'), lane = laneOf(a.dataset.ask);
  if (b.classList.contains('rp')) return lane && open(lane);
  // `h` names the menu this card shows: the server refuses a tap on a menu that has since changed.
  const r = await post('answer', { agent: a.dataset.ask, choice: b.value, h: b.dataset.h });
  const msg = (await r.text()).trim();
  // Re-render (fresh pane read) so stale buttons go at once; the message lands on the new card.
  await refresh();
  [a, ...[...$$('#needs .ask')].filter(c => c.dataset.ask === a.dataset.ask)].forEach(c => {
    const s = c.querySelector('.as');
    if (s) { s.textContent = msg; s.className = 'as ' + (r.ok ? 'g' : 'r'); }
  });
});
$('.bd').onclick = close;
addEventListener('keydown', e => { if (e.key === 'Escape' && shown()) close(); });
// #70: the sheet follows the finger 1:1 (light rubber band above full), one move per frame. On release (#166): down
// past 30 % of its shown height, or flicked down (over 0.5 px/ms in the last 80 ms), closes; flicked up goes one detent
// up; else it springs back to the nearest detent.
function grab(y) {
  if (kb() || shut) return false;
  clearTimeout(padT);
  d0 = { y, t: ty, h: S.clientHeight - ty };
  moves = [];
  S.classList.add('drag');
  return true;
}
function drag(y, ts) {
  const raw = d0.t + y - d0.y;
  to = raw < 0 ? raw / 3 : raw;
  moves.push([y, ts]);
  if (!rq) rq = requestAnimationFrame(() => { rq = 0; if (to !== null) pos(to); });
}
function drop() {
  cancelAnimationFrame(rq);
  rq = 0;
  if (to !== null) pos(to);
  const g = d0;
  to = d0 = null;
  S.classList.remove('drag');
  const now = performance.now(), m = moves.filter(([, t]) => now - t <= 80), a = m[0], b = m[m.length - 1];
  const v = m.length > 1 && b[1] > a[1] ? (b[0] - a[0]) / (b[1] - a[1]) : 0, ds = ['full', 'mid', 'half'], os = ds.map(off);
  if (v > .5 || ty - g.t > .3 * g.h) return close();
  const i = v < -.5 ? Math.max(0, os.findLastIndex(o => o < ty - 1)) : os.reduce((k, o, j) => Math.abs(o - ty) < Math.abs(os[k] - ty) ? j : k, 0);
  snap(ds[i]);
}
const hd = $('.hd');
hd.onpointerdown = e => { if (grab(e.clientY)) try { hd.setPointerCapture(e.pointerId); } catch (_) {} };
hd.onpointermove = e => { if (d0) drag(e.clientY, e.timeStamp); };
hd.onpointerup = hd.onpointercancel = () => { if (d0) drop(); };
// The conversation scrolls; pulled down from its top, it hands the touch to the sheet. A touch that starts scrolled
// down (#166) only scrolls. Move and end listen on the touched node itself: a poll may swap it out mid-touch.
chat.addEventListener('touchstart', e => {
  const t = e.target;
  if (chat.scrollTop > 0) return;
  let c0 = e.touches[0].clientY;
  const move = e => {
    const y = e.touches[0].clientY;
    if (!d0 && (chat.scrollTop > 0 || y <= c0 || !e.cancelable || !grab(c0))) { c0 = y; return; }
    e.preventDefault();
    drag(y, e.timeStamp);
  };
  const end = () => {
    t.removeEventListener('touchmove', move);
    t.removeEventListener('touchend', end);
    t.removeEventListener('touchcancel', end);
    if (d0) drop();
  };
  t.addEventListener('touchmove', move, { passive: false });
  t.addEventListener('touchend', end);
  t.addEventListener('touchcancel', end);
}, { passive: true });
$('.reply').onsubmit = async e => {
  e.preventDefault();
  const r = await post('send', { agent, text: e.target.text.value });
  const msg = (await r.text()).trim();
  if (r.status === 401) unpaired();
  say(msg, r.ok);
  if (r.ok) { e.target.text.value = ''; setTimeout(poll, 1500); }
};
// #59: Pair this phone (header, blocked cards, the sheet): POST `pair/request` shows a code; its 10 min HttpOnly cookie is this
// browser's alone. The Mac approves it (`serve approve`, logged: the live Events frag arrives, or ↻), then `pair/claim` keeps it a year.
// The panel flows under the header (or the sheet's Pair button), never over a card; a tap closes it; it expires with the code.
B.insertAdjacentHTML('beforeend', '<div id=pairp class=pairp hidden></div>');
const pp = $('#pairp');
let pairing = false, ttl;
async function claim() {
  if (!pairing) return;
  pairing = false;
  const r = await fetch(R + 'pair/claim', { method: 'POST' }).catch(() => null);
  if (!r || !r.ok) { pairing = true; return; }
  nopair = false;
  clearTimeout(ttl);
  pp.className = 'pairp g';
  pp.replaceChildren(el('p', '', (await r.text()).trim()));
  $$('.pairb').forEach(b => b.hidden = true);
  setTimeout(() => { pp.hidden = true; }, 4000);
  connect();
  refresh();
  if (agent) poll();
}
// #288: claim polls too, so the page pairs itself within 2 s of `serve approve`; a paired browser's
// one claim per load renews its year-long cookie (unpaired: a harmless 403).
setInterval(claim, 2000);
fetch(R + 'pair/claim', { method: 'POST' }).catch(() => null);
document.addEventListener('click', async e => {
  const cp = e.target.closest('.pairc');
  if (cp) { navigator.clipboard.writeText(cp.previousElementSibling.textContent).then(() => { cp.textContent = 'copied'; }).catch(() => null); return; }
  if (e.target.closest('.pairx')) { pairing = false; clearTimeout(ttl); pp.hidden = true; fetch(R + 'pair/cancel', { method: 'POST' }).catch(() => null); return; }
  if (!e.target.closest('.pairb')) { if (e.target.closest('#pairp')) { pairing = false; clearTimeout(ttl); pp.hidden = true; } return; }
  const r = await fetch(R + 'pair/request', { method: 'POST' }).catch(err => ({ ok: false, text: () => 'error: ' + err.message })), s = (await r.text()).trim(), [t, h, c] = s.split('\n');
  pairing = r.ok;
  pp.className = 'pairp' + (r.ok ? '' : ' r');
  if (!e.target.closest('#pairp')) (e.target.closest('.pairf') || $('header')).after(pp);
  pp.hidden = false;
  clearTimeout(ttl);
  if (r.ok) pp.replaceChildren(el('p', 'code', t), el('p', 'm', `Waiting for approval on the Mac: herdsman serve approve ${t}` + (h ? ` · ${h}` : '')), ...(c ? [el('p', '', c), el('button', 'pairc', 'copy'), el('span', 'm', ' paste in Terminal ')] : []), el('button', 'pairx', 'cancel'));
  else pp.replaceChildren(el('p', '', s));
  pp.scrollIntoView({ block: 'nearest' });
  if (!r.ok) return;
  ttl = setTimeout(() => { pairing = false; pp.className = 'pairp r'; pp.replaceChildren(el('span', '', 'code expired — '), $('.pairb').cloneNode(true)); }, 6e5);
});
// Pull to refresh from the top (×0.5 resistance, past 70 px); never with the sheet open or a field
// focused; the axis locks after 10 px so a sideways swipe never pulls. In Herd mode: the current page.
const M = $('main'), ptr = $('#ptr'), still = matchMedia('(prefers-reduced-motion: reduce)');
let p0 = null, py = 0, axis = '';
addEventListener('touchstart', e => {
  const typing = document.activeElement && document.activeElement.matches('input, textarea, select');
  const hp = herdPage();
  p0 = (hp ? hp.scrollTop : scrollY) <= 0 && !shown() && !typing ? e.touches[0] : null;
  py = 0;
  axis = '';
}, { passive: true });
addEventListener('touchmove', e => {
  if (!p0) return;
  const dx = e.touches[0].clientX - p0.clientX, dy = e.touches[0].clientY - p0.clientY;
  if (!axis && Math.hypot(dx, dy) > 10) axis = dy > Math.abs(dx) ? 'y' : 'x';
  if (axis !== 'y') return;
  py = Math.max(0, dy * 0.5);
  const box = herdPage() || M;
  box.style.transition = 'none';
  box.style.transform = `translateY(${py}px)`;
  ptr.hidden = false;
  ptr.style.transform = still.matches ? '' : `rotate(${py * 4}deg)`;
}, { passive: true });
addEventListener('touchend', () => {
  if (!p0) return;
  p0 = null;
  [M, ...$$('.hp')].forEach(b => { b.style.transition = b.style.transform = ''; });
  ptr.hidden = true;
  if (py > 70) refresh();
});
// #114: iOS Safari ignores user-scalable=no for pinch.
addEventListener('gesturestart', e => e.preventDefault(), { passive: false });
if ('serviceWorker' in navigator) navigator.serviceWorker.register(R + 'sw.js').catch(() => {});
// #173: a fleet's Pipeline group, drawn from its `data-pipe` (serve.rs): the validity line or the broken rules (each
// with its fleet.toml line), the graph, the legend, the pipeline as fleet.toml text (docs/design/pipeline).
function pipeline(sec) {
  const d = JSON.parse(sec.dataset.pipe), S = d.stages, n = d.bad.length;
  const hint = n ? `<b class=r>BAD_PIPELINE · ${n} error${n === 1 ? '' : 's'}</b>` : d.builtin ? 'built-in · edit fleet.toml' : `fleet.toml:${S[0]?.line ?? 0}`;
  let h = `<h3>Pipeline<span>${hint}</span></h3>`;
  if (n) {
    const li = d.bad.map(b => `<li><b class=r>${b.rule}</b>${esc(b.msg)}<span class=at>fleet.toml:${b.line}</span></li>`).join('');
    h += `<div class=pe><p>Nothing moves for ${esc(d.fleet)} until fleet.toml is fixed: <span class=qn>drive, spawn, verdict and merge</span> refuse <span class=r>BAD_PIPELINE</span>. Lanes already running keep working. Other fleets are not affected.</p><ol>${li}</ol></div>`;
  } else {
    const both = d.checks.length > 1 ? ` · merge needs a PASS from ${d.checks.join(' and ')} at the same head` : '';
    h += `<p class=pv><span class=g>✓ valid</span> · ${S.length} stages (${esc(S.map(s => s.name).join(', '))}) · every stage reachable, every path ends, every loop has a limit${esc(both)}</p>`;
  }
  const head = d.builtin ? `# Built-in: herdsman runs this while fleet.toml has no\n# [[fleet.${d.fleet}.pipeline.stage]]. Copy it there to change it.\n` : '';
  const toml = S.map(s => `\n[[fleet.${d.fleet}.pipeline.stage]]\nname = "${s.name}"\nkind = "${s.kind}"\n${s.role ? `role = "${s.role}"\n` : ''}${s.rounds != null ? `rounds = ${s.rounds}\n` : ''}next = { ${Object.entries(s.next).map(([o, t]) => `${o} = "${t}"`).join(', ')} }\n`).join('');
  const what = d.builtin ? 'the built-in pipeline' : `[fleet.${d.fleet}.pipeline]`;
  const lg = '<i class="kd b">▲</i> build pushes code · <i class="kd c">◆</i> check: PASS or FAIL · <i class="kd g">◫</i> gate: herdsman\'s own · <i class="kd m">✓</i> merged · <i class="kd x">!</i> parked';
  sec.innerHTML = h + `<div class=pw2><div>${pgraph(d)}<p class=lg>${lg}</p></div><details class=tm><summary><i class=cv></i><span>fleet.toml · ${esc(what)}</span></summary><pre>${esc((head + toml).trim())}</pre></details></div>`;
}
// One grid row per text line (`--ln` tall), so a rail meets each line at its middle: column 1 the park rail, 2 the
// boxes, then one rail per loop target, the shortest span innermost. An outcome off the spine leaves its box on a rail.
function pgraph(d) {
  const S = d.stages, base = t => t.split(':')[0], main = k => k === 'build' ? 'push' : k === 'check' ? 'PASS' : 'ok';
  const W = { PASS: '<b class=g>PASS</b>', ok: '<b class=g>ok</b>', FAIL: '<b class=r>FAIL</b>', ci_red: '<b class=r>CI red</b>', conflict: '<b class=a>conflict</b>' };
  const word = o => W[o] ?? `<b>${esc(o)}</b>`, K = { build: 'b', check: 'c', gate: 'g' }, G = { b: '▲', c: '◆', g: '◫', x: '!' };
  const spine = i => S[i + 1]?.name ?? 'merge', off = (i, o, t) => !(o === main(S[i].kind) && t === spine(i));
  const loops = S.flatMap((s, i) => Object.entries(s.next).filter(([o, t]) => off(i, o, t)).map(([, t]) => base(t)));
  const reach = [S[0]?.name ?? '', ...S.flatMap(s => Object.values(s.next).map(base))];
  const rails = {}, park = [], heads = {};
  let h = '', row = 0;
  const put = (cls, inner) => { h += `<p class="${cls}" style="grid-row:${++row}">${inner}</p>`; return row; };
  put('nl st', `<i class=kd>○</i><b>queued</b><span class=m>${d.queued} waiting</span>`);
  put('pl', '<b>start</b><span>if unblocked · serial slot · host room</span>');
  S.forEach((s, i) => {
    const errs = d.bad.filter(b => b.stage === s.name), dead = !reach.includes(s.name), k = K[s.kind] ?? 'x';
    const tag = (s.here ? `<span class="tag m">${s.here} here</span>` : '') + (loops.includes(s.name) ? `<span class=lp>↺ ${esc(s.name)}</span>` : '');
    const lines = [[`<i class="kd ${k}">${G[k]}</i><b>${esc(s.name)}</b><span class=m>${s.kind === 'gate' ? 'gate · herdsman' : esc(s.kind)}</span>${tag}`]];
    const ci = d.required.length ? d.required.join(' + ') : 'green', from = d.checks.length > 1 ? `from ${d.checks.join(' + ')} ` : '';
    const dt = esc(s.role ? `role ${s.role} · ${s.run ?? 'not defined'}` : `CI ${ci} · PASS ${from}at head · serial · mergeable`);
    lines.push([`<span class=dt title="${dt}">${dt}</span>`]);
    for (const b of errs) { const t = esc(`${b.rule}: ${b.msg}`); lines.push([`<span class="dt r" title="${t}">✗ ${t}</span>`]); }
    for (const [o, t] of Object.entries(s.next).filter(([o, t]) => off(i, o, t))) {
      const up = t.endsWith(':update'), to = up ? base(t) : t;
      const what = to === 'park' ? 'park' : up ? `update in ${to}` : ['FAIL', 'ci_red', 'conflict'].includes(o) ? `fix in ${to}` : to;
      const lim = ['FAIL', 'conflict'].includes(o) && s.rounds != null ? ` · ≤${s.rounds} rounds, then park` : o === 'ci_red' ? ` · ${s.loop}'s rounds` : '';
      const tt = esc(what + lim);
      lines.push([`<span title="${tt}">${word(o)} → ${tt}</span>`, to]);
    }
    if (s.rounds != null && (s.kind === 'check' || (s.next.conflict ?? '').endsWith(':update'))) lines.push(['<span><b>out of rounds</b> → park</span>', 'park']);
    const last = lines.length - 1;
    lines.forEach(([inner, rail], j) => {
      const edge = j === 0 ? (last ? ' t' : ' t b') : j === last ? ' b' : '';
      const r = put('nb' + edge + (rail === 'park' ? ' po l' : rail ? ' po' : '') + (dead ? ' dead' : errs.length ? ' bad' : ''), inner);
      if (rail === 'park') park.push(r); else if (rail) (rails[rail] ??= []).push(r);
      if (!j) heads[s.name] = r;
    });
    const o = main(s.kind), sp = spine(i);
    const why = o === 'push' ? 'a new head' : o === 'PASS' ? (i && S[i - 1].kind === 'check' ? 'at the same head' : 'at the exact head') : '';
    if (s.next[o] === sp) put('pl', `${word(o)}<span>${why}</span>`);
    else put('pl no', reach.includes(sp) ? '' : `<span class=r>✗ nothing leads to ${esc(sp)}</span>`);
  });
  const gone = reach.includes('merge') ? '' : ' dead';
  const mr = put(`nb t b end${gone}`, `<i class="kd m">✓</i><b>merge</b><span class=m>into ${esc(d.into)}${gone && ' · unreachable'}</span>`);
  put('nl', '');
  const pr = put('nb t b end', `<i class="kd x">!</i><b>park</b><span class=m>needs you</span><span class="tag m">${d.parked} here</span>`);
  put('pl pn', '<span>on its card: one more round, or park for good</span>');
  const area = (a, b, c, e) => `style="grid-area:${a}/${b}/${c}/${e}"`;
  const spans = Object.keys(rails).sort().map(t => { const all = [...rails[t], heads[t] ?? 1]; return [t, Math.min(...all), Math.max(...all)]; });
  spans.sort((a, b) => (a[2] - a[1]) - (b[2] - b[1]));
  spans.forEach(([t, lo, hi], c) => {
    const col = c + 3, hd = heads[t] ?? 1;
    h += `<i class=ey ${area(lo, col, hi + 1, col + 1)}></i><i class="ea in" ${area(hd, 3, hd + 1, col + 1)}></i>`;
    h += rails[t].map(r => `<i class=ea ${area(r, 3, r + 1, col + 1)}></i>`).join('');
  });
  if (park.length) {
    h += `<i class="ey l" ${area(Math.min(...park), 1, pr + 1, 2)}></i><i class="ea l in" ${area(pr, 1, pr + 1, 2)}></i>`;
    h += park.map(r => `<i class="ea l" ${area(r, 1, r + 1, 2)}></i>`).join('');
    h += `<i class=ek ${area(mr, 1, mr + 1, 2)}></i>`; // a gap where the park rail passes merge: never merge → park
  }
  const rep = spans.length ? ` repeat(${spans.length},2ch)` : '';
  return `<div class=pg role=img aria-label="${esc(d.fleet)} pipeline: ${esc(S.map(s => s.name).join(', '))}" style="grid-template-columns:2ch minmax(0,1fr)${rep};grid-template-rows:repeat(${row},var(--ln))">${h}</div>`;
}
