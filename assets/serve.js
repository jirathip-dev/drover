// Tabs, lane display mode (the look sheet's style; both kept in localStorage), the live board (SSE `/live`, sync dot, ↻,
// pull to refresh), SW, the Lanes bottom sheet: card, Stop/Kill/Clean (`/act`), transcript per turn
// (5 s poll, load more), reply box with a `/` menu, Pair this phone, and blocked agents' menu buttons.
const B = document.body, $ = s => document.querySelector(s), $$ = s => document.querySelectorAll(s);
const get = k => { try { return localStorage.getItem(k); } catch (e) { return null; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch (e) {} };
// Per tab session (sessionStorage): #141's open groups; #337's last copy of each part (`c:<id>`), painted before fresh data lands.
const sget = k => { try { return sessionStorage.getItem(k); } catch (e) { return null; } };
const sput = (k, v) => { try { sessionStorage.setItem(k, v); } catch (e) {} };
// URLs hang off R (`/` live, `` relative in a `serve --export`). #39 demo: static files, never a POST, never `/live`.
const R = B.dataset.root ?? '/', DEMO = !!B.dataset.demo;
if (DEMO) { const f = fetch; window.fetch = (u, o) => o && o.method === 'POST' ? Promise.reject(new Error('demo: no writes')) : f(u, o); }
function tab(t) {
  document.documentElement.dataset.tab = t; // #206: serve.css shows this tab's section (serve.rs sets it before the first paint)
  $$('main>section').forEach(s => s.hidden = s.id !== t);
  $$('nav a').forEach(a => a.classList.toggle('on', a.hash === '#' + t));
  put('tab', t);
  $('.qhelp').hidden = t !== 'queue';
  // #337: a tab not read yet (or a refresh made stale) loads when it opens, its last copy painted first.
  if (have.has(t) || DEMO || B.dataset.root === '') return;
  have.add(t);
  // #391: after this script has run (the page's first tab() is before the parts frag() draws with are defined); #268:
  // after the first frame, so the page paints first (#269: no tab is in the page, the TypeScript board draws each).
  if (!$('#' + t).childElementCount && sget('c:' + t)) requestAnimationFrame(() => setTimeout(() => $('#' + t).childElementCount || frag(t, sget('c:' + t))));
  // #269: the board reads the needs cards and the tab's part (on the page's first tab(), not loaded yet: its own first read).
  if (window.boardTab) wait(window.boardTab(t).catch(() => have.delete(t)));
}
const have = new Set();
// #337: a load, refresh or action over 300 ms shows the theme's loading animation, large, over a light scrim near the
// top of the content (the page stays tappable; [↻] spins as well); it goes as soon as the data paints.
let waits = 0, ovt = 0;
function wait(p) {
  if (!waits++) ovt = setTimeout(() => loading(true), 300);
  return Promise.resolve(p).finally(() => { if (!--waits) { clearTimeout(ovt); loading(false); } });
}
function loading(on) { $('#ld').hidden = !on; }
// #60: a part of the page read on demand (null when it cannot be read).
const part = src => fetch(R + src, { cache: 'no-store' }).then(r => r.ok ? r.text() : null).catch(() => null);
// A form POST of `data` (an object or FormData) to `path`; #403: `send` without the loading overlay (every chat post).
const send = (path, data) => fetch(R + path, { method: 'POST', body: new URLSearchParams(data) });
const post = (path, data) => wait(send(path, data));
// #352: a tab change reopens the stream, which names the open tab (the Host tab turns the host's process sampling on).
$$('nav a').forEach(a => a.onclick = e => { e.preventDefault(); tab(a.hash.slice(1)); if (es) connect(); });
// A tapped alert opens `/?issue=N#tab`: that tab (the head script picked it: the hash, else the last one), the issue's row open.
const DBG = new URLSearchParams(location.search);
let want = DBG.get('issue');
tab(document.documentElement.dataset.tab || 'queue');
// #206 `?debug` (kept for the tab's session): how the page came up. A load or reload repaints it all; a pull only refresh()es.
const dbg = (...a) => { try { if (DBG.has('debug') || sessionStorage.debug) { sessionStorage.debug = 1; console.log('[herdsman]', ...a); } } catch (e) {} };
dbg('page', performance.getEntriesByType('navigation')[0]?.type, 'tab', document.documentElement.dataset.tab);
addEventListener('pageshow', e => dbg('pageshow', e.persisted ? 'from bfcache' : 'loaded'));
// #241: the Lanes modes are the theme's `modes` (farm, kitchen: scene, ascii; terminal: glyph alone); TT is the theme
// herd.js loaded (docs/themes.md). #335: the look sheet's STYLE picks it (pixel: scene, ascii: ascii), no Lanes button. In
// ascii the loading overlay runs the theme's `ascii_loading` frames: built once, then CSS swaps them (no node replaced).
let TT = null, TM = ['scene', 'ascii'];
function mode(m) {
  if (TM.includes(m)) put('mode', m); // a theme without it (terminal) keeps the pick for the next one
  B.dataset.mode = m = TM.includes(m) ? m : TM[0];
  const fs = ((TT && TT.ascii_loading) || '').split('\n').filter(Boolean), ld = $('#ld');
  let a = ld.querySelector('.lda');
  if (!a) ld.append(a = Object.assign(document.createElement('span'), { className: 'asc lda' }));
  if (a.dataset.f !== fs.join()) {
    a.dataset.f = fs.join();
    a.replaceChildren(...fs.map(f => Object.assign(document.createElement('b'), { textContent: f })));
    a.classList.toggle('run', fs.length > 1);
  }
  ld.classList.toggle('la', m === 'ascii' && fs.length > 0);
  chatPaint();
}
mode(get('mode'));
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
// The scheme a phone shows: its pick, else its theme's default (#423: a `{light, dark}` one by the phone's setting,
// `dark`), else github-dark; each only if it passes.
const choose = (all, pick, def, dark) => [pick, (def && def[dark ? 'dark' : 'light']) || def, 'github-dark'].find(n => all[n] && !fails(all[n]).length) || 'github-dark';
// schemes:end
// #423 plants: pure from here to `plants:end` (tests/themes.rs runs it under node). An issue's plant: theme `t`'s
// `stages` sprite for its growth stage, in its species (`species`, picked by issue number `n`, so it never changes);
// `row`: the Queue row's 24 × 24 one. None for a theme without `stages`.
const species = (t, n) => { const L = (t && t.species) || []; return L.length && n >= 0 ? L[n % L.length] : null; };
function plant(t, stage, n, row) {
  const id = t && t.stages && t.stages[stage], s = species(t, n), x = row ? '-row' : '';
  if (!id) return null;
  return { ids: [s && `${id}-${s.id}${x}`, id + x].filter(Boolean), style: s ? Object.entries(s).filter(e => e[0] !== 'id').map(e => e.join(':')).join(';') : '' };
}
// A lane's sprite: symbol `top` (the role's action) over its issue's plant (`hs-<sprite id>`, the first that `has`);
// `top` alone without one, as before #423.
function duo(t, has, top, stage, n) {
  const p = plant(t, stage, n), id = p && p.ids.find(i => has('hs-' + i));
  return (id ? `<use href="#hs-${id}" style="${p.style}"></use>` : '') + `<use href="#${top}"></use>`;
}
// plants:end
const DARK = matchMedia('(prefers-color-scheme: dark)');
// #244: the colours: every scheme that passes the gate (built-in and `<config dir>/schemes/`; SD from the head script),
// this phone's pick (localStorage) else its theme's default. Applied in place; the block is kept so the head script
// paints it first next time (no flash), and the status bar takes its --bg.
const SQ = new URLSearchParams(location.search).get('scheme');
function colours() {
  const h = document.documentElement, n = choose(SD.s, SQ || get('scheme'), SD.t[h.dataset.theme], DARK.matches), b = block(SD.s[n]);
  h.dataset.scheme = n;
  $('#sch').textContent = b;
  $('meta[name=theme-color]').content = SD.s[n].tokens['--bg'];
  put('sb', b);
  hostrow(document);
  if (sysOn()) lsheet(); // #290: Appearance (#440: in System) moves its ticks
}
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
// #290: the look sheet's content, #440: System's Appearance: THEME (each theme's 24 px impl row sprite, else its
// favicon) and COLOURS (each passing scheme's bg, fg, green, amber, red), each a listbox. A tap, or Enter on the focused
// row, applies it in place (#206: no reload) and the sheet stays open, so the board behind it shows the change. Picking
// the theme's default colours forgets the pick: they follow the theme again.
const LS = {};
const lsprite = n => LS[n] || (LS[n] = Promise.all(['theme.json', 'sprites.svg'].map(f => fetch(`${R}themes/${n}/${f}`).then(r => r.ok ? r.text() : '').catch(() => ''))).then(([j, s]) => {
  let t = {};
  try { t = JSON.parse(j); } catch (e) {}
  const id = t.row_sprites && t.row_sprites.impl, g = id && new DOMParser().parseFromString(s, 'image/svg+xml').getElementById(id);
  // The art is 16 sprite px wide: its viewBox is cropped to them, so each is 1.5 css px at 24 (#281: never smoothed).
  const m = g && /(?:translate\(([\d.]+)[ ,][^)]*\)\s*)?scale\(([\d.]+)\)/.exec(g.outerHTML), k = m ? +m[2] : 8.25;
  if (g) { g.removeAttribute('id'); return `<svg viewBox="${m && m[1] || 0} 0 ${16 * k} 100">${g.outerHTML}</svg>`; }
  return `<img alt="" src="${R}${t.icons && t.icons.favicon ? `themes/${n}/${t.icons.favicon}` : 'icon-192.png'}">`;
}));
function lrow(k, n, on, art, def) {
  const o = el('div', 'lko');
  o.tabIndex = on ? 0 : -1;
  o.setAttribute('role', 'option');
  o.setAttribute('aria-selected', on);
  o.dataset[k] = n;
  o.append(art, el('span', 'ln', n));
  if (def) o.append(el('small', '', '(theme default)'));
  return o;
}
function lsheet() {
  const h = document.documentElement, top = $('#sy-appearance>.look'), f = top && top.contains(document.activeElement) && document.activeElement.closest('.lko');
  if (!top) return;
  const key = f && Object.entries(f.dataset)[0];
  const grp = (k, title, hint, rows) => {
    const g = el('section', 'cg lkg'), t = el('h3', '', title), l = el('div', 'lkl');
    t.append(el('span', '', hint));
    l.setAttribute('role', 'listbox');
    l.setAttribute('aria-label', k);
    l.append(...rows);
    g.append(t, l);
    return g;
  };
  const th = Object.keys(SD.t).map(n => {
    const s = el('i', 'lka');
    lsprite(n).then(x => { s.innerHTML = x; });
    return lrow('t', n, n === h.dataset.theme, s);
  });
  // A scheme's art: its text on its background (`Aa`), then its green, amber and red chips.
  const sw = s => { const i = el('i', 'lsw'); i.append(Object.assign(el('b', 'aa', 'Aa'), { style: `background:${s.tokens['--bg']};color:${s.tokens['--fg']}` }), ...['--g', '--a', '--r'].map(t => Object.assign(el('b'), { style: `background:${s.tokens[t]}` }))); return i; };
  const ok = Object.keys(SD.s).filter(n => !fails(SD.s[n]).length), no = Object.keys(SD.s).length - ok.length;
  const sc = ok.map(n => lrow('s', n, n === h.dataset.scheme, sw(SD.s[n]), n === choose(SD.s, null, SD.t[h.dataset.theme], DARK.matches)));
  // #335: STYLE, pixel (sprites) or ascii (the theme's ASCII figures); a theme without ascii (terminal) has none.
  const px = el('i', 'lka'), as = el('i', 'lka');
  lsprite(h.dataset.theme).then(x => { px.innerHTML = x; });
  as.append(el('b', 'asc', TT && TT.ascii_rows && TT.ascii_rows.impl));
  const st = TM.includes('ascii') ? [grp('style', 'style', 'rows, lanes, host, loading', [lrow('y', 'pixel', B.dataset.mode !== 'ascii', px), lrow('y', 'ascii', B.dataset.mode === 'ascii', as)])] : [];
  top.replaceChildren(grp('theme', 'theme', 'sprites, scene, icon', th), grp('colours', 'colours', `${ok.length} pass the gate${no ? ` · ${no} refused: host` : ''}`, sc), ...st,
    el('p', 'lkn', 'this phone only · the home-screen icon keeps its install-time look'));
  if (key) top.querySelector(`.lko[data-${key[0]}="${CSS.escape(key[1])}"]`).focus();
  $('#sy-appearance .sym').textContent = `${h.dataset.theme} · ${h.dataset.scheme}${B.dataset.mode === 'ascii' ? ' · ascii' : ''}`;
}
// #335: STYLE applies in place: pixel is the scene mode, ascii the ascii mode, on every tab.
function style(y) {
  mode(y === 'ascii' ? 'ascii' : 'scene');
  rows(document);
  groups();
}
function lpick(o) {
  const h = document.documentElement;
  if (o.dataset.y) style(o.dataset.y);
  else if (o.dataset.t) {
    h.dataset.theme = o.dataset.t;
    put('theme', o.dataset.t);
    theme(o.dataset.t);
  } else if (o.dataset.s === choose(SD.s, null, SD.t[h.dataset.theme], DARK.matches)) {
    try { localStorage.removeItem('scheme'); } catch (e) {}
  } else put('scheme', o.dataset.s);
  colours();
}
document.addEventListener('click', e => {
  const o = e.target.closest('.look .lko');
  if (o) lpick(o);
});
// Keyboard: ↑/↓ (Home/End) move within a list, Tab between the lists, Enter or Space applies, Esc closes.
document.addEventListener('keydown', e => {
  const o = e.target.closest && e.target.closest('.look .lko');
  if (!o) return;
  if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); return lpick(o); }
  const all = [...o.parentNode.children], i = all.indexOf(o);
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: all.length - 1 }[e.key];
  if (to === undefined || !all[to]) return;
  e.preventDefault();
  all.forEach(x => { x.tabIndex = -1; });
  all[to].tabIndex = 0;
  all[to].focus();
});
colours();
DARK.addEventListener('change', colours); // #423: a `{light, dark}` default follows the phone live
// #241: the theme on the tabs. A Queue or Events row's state glyph becomes its action's 24 px sprite (#281; its title and label
// the plain state word); a Lanes avatar takes the theme's ASCII words and look. Terminal (`row_sprites: false`): none.
const AW = { impl: 'building', fix: 'fixing', rev: 'reviewing', update: 'updating', idle: 'idle', done: 'done', blocked: 'needs you' };
const hash = t => [...t].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
function rows(root) {
  root.querySelectorAll('.rsg').forEach(g => { g.textContent = g.dataset.g; g.classList.remove('rsg'); ['title', 'role', 'aria-label', 'data-g'].forEach(a => g.removeAttribute(a)); });
  // #423: a sprite drawn in its own box (`data-box`, herd.js) keeps it, so its pixels stay whole.
  const box = id => document.getElementById(id)?.getAttribute('viewBox') || '0 0 132 100';
  const rs = TT && TT.row_sprites, sp = (g, act, word, grow, n) => {
    if (!g || !rs || !rs[act]) return;
    Object.assign(g.dataset, { g: g.textContent });
    g.classList.add('rsg');
    g.title = word;
    g.setAttribute('role', 'img');
    g.setAttribute('aria-label', word);
    // #335: ascii style: the theme's 3-character figure in the sprite's 24 px box (the same 28 px slot).
    const f = B.dataset.mode === 'ascii' && TT.ascii_rows && TT.ascii_rows[act];
    // #423: a Queue row draws its issue's plant at its stage (a theme's `stages`), else the action's sprite.
    const p = grow && plant(TT, grow, +n, 1), id = p && p.ids.map(i => 'hq-' + i).find(i => document.getElementById(i));
    const u = id ? `<use href="#${id}" style="${p.style}"/>` : `<use href="#hr-${act}"/>`;
    if (!f) return void (g.innerHTML = `<svg class=rsp viewBox="${box(id || 'hr-' + act)}" aria-hidden=true>${u}</svg>`);
    g.replaceChildren(el('i', 'rsp rsa', f));
    g.firstChild.setAttribute('aria-hidden', true);
  };
  root.querySelectorAll('details.q, .qc').forEach(d => sp(d.querySelector('summary .gt>b'), d.dataset.act, d.querySelector('summary>span:nth-of-type(2)>span')?.textContent || AW[d.dataset.act], d.dataset.grow, d.dataset.i));
  root.querySelectorAll('.kg[data-act]').forEach(g => sp(g, g.dataset.act, AW[g.dataset.act]));
  const L = (TT && TT.looks) || [];
  root.querySelectorAll('.av[data-act]').forEach(v => {
    const a = v.querySelector('.asc'), px = v.querySelector('.px'), t = TT && TT.ascii && TT.ascii[v.dataset.act], n = v.closest('[data-agent]');
    if (a.dataset.o == null) Object.assign(a.dataset, { o: a.innerHTML, c: px.getAttribute('style'), r: a.classList.contains('run') ? 1 : '' });
    // #334: a working lane's word gallops in two frames, its legs' `/` and `\` swapped; a word with no legs is one
    // still frame (never the two-frame blink on its own).
    const fs = t ? [...new Set([t, a.dataset.r ? t.replace(/[^\n]*$/, l => l.replace(/[/\\]/g, c => c === '/' ? '\\' : '/')) : t])] : [];
    a.classList.toggle('run', t ? fs.length > 1 : !!a.dataset.r);
    if (t) a.replaceChildren(...fs.map(f => Object.assign(document.createElement('b'), { textContent: f })));
    else a.innerHTML = a.dataset.o;
    px.setAttribute('style', L.length && n ? Object.entries(L[hash(n.dataset.agent) % L.length]).map(e => e.join(':')).join(';') : a.dataset.c);
    // #423: the keeper over the issue's plant; the action alone for a theme without `stages`.
    const d = duo(TT, i => document.getElementById(i), 'ha-' + v.dataset.act, v.dataset.grow, v.dataset.i == null ? -1 : +v.dataset.i);
    if (px.innerHTML !== d) px.innerHTML = d;
  });
  words(root);
}
// #241: a Host group's load scene in miniature (its status: calm, busy or held): wind and clouds, or flame height.
const MINI = {
  weather: ['<circle cx=8 cy=8 r=4 fill="var(--a)"/>', '<path d="M3 12h10a3 3 0 0 0-2-6 4 4 0 0 0-7 1 3 3 0 0 0-1 5z" fill="var(--mute)"/><path d="M1 4h5M2 7h3" stroke="var(--mute)"/>',
    '<path d="M3 9h10a3 3 0 0 0-2-6 4 4 0 0 0-7 1 3 3 0 0 0-1 5z" fill="var(--mute)"/><path d="M5 11l-1 4M8 11l-1 4M11 11l-1 4" stroke="var(--l)"/>'],
  heat: [5, 9, 13].map((h, i) => `<path d="M8 ${15 - h}C12 ${15 - h / 2} 12 15 8 15C4 15 4 ${15 - h / 2} 8 ${15 - h}Z" fill="var(--${i > 1 ? 'r' : 'a'})"/>`),
};
MINI.light = MINI.weather; // #423: sun, cloud, rain
function mini(g) {
  const s = g.querySelector(':scope>summary'), ls = TT && TT.load_scene, k = { a: 1, r: 2 }[g.dataset.t] || 0, l = ['calm', 'busy', 'held'][k];
  s.querySelector('.lsc')?.remove();
  if (!ls || !MINI[ls.kind]) return;
  const word = esc('load: ' + ((ls.levels || {})[l] || l)), f = B.dataset.mode === 'ascii' && TT.ascii_load && TT.ascii_load[l];
  s.querySelector('.dt').insertAdjacentHTML('afterend', f ? `<i class="lsc lsa" role=img aria-label="${word}" title="${word}">${esc(f)}</i>` // #335
    : `<svg class=lsc viewBox="0 0 16 16" role=img aria-label="${word}"><title>${word}</title>${MINI[ls.kind][k]}</svg>`);
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
// #370: the theme's words on every tab, `<b class=thw>on the line</b><small class=pw> · working</small>` (the plain word
// muted); a word the theme lacks is the plain word alone, so terminal (no words) is today's board. A label keeps its
// plain text in data-p, so a theme switch re-pairs it from the original.
const PW = { needs: 'needs you', working: 'working', idle: 'idle', done: 'done', queued: 'queued', parked: 'parked', dead: 'dead',
  impl: 'build', fix: 'fix', rev: 'review', update: 'catch up', merged: 'merged', fail: 'FAIL' };
const TW = () => (TT && TT.words) || {};
const tw = (k, plain, rest = '') => TW()[k] ? `<b class=thw>${esc(TW()[k])}</b><small class=pw> · ${esc(plain + rest)}</small>` : esc(plain + rest);
function relabel(n, k, plain, rest = '') {
  if (!n) return;
  if (n.dataset.p == null) n.dataset.p = n.textContent;
  const h = TW()[k] ? tw(k, plain, rest) : esc(n.dataset.p);
  if (n.innerHTML !== h) n.innerHTML = h;
}
// A row sprite shows `hr-<k>` (a queued, needs-you, merged or FAIL row) while the theme has it, else its action's: the
// same <use>, its href changed, never a new node (#334).
function use(g, k) {
  const u = g && g.querySelector('use');
  if (!u) return;
  if (u.dataset.a == null) u.dataset.a = u.getAttribute('href');
  // #423: a queued row keeps its own plant (a theme's `stages`): a cutting already.
  const h = k && document.getElementById('hr-' + k) && !(k === 'queued' && u.dataset.a.startsWith('#hq-')) ? '#hr-' + k : u.dataset.a;
  if (u.getAttribute('href') !== h) u.setAttribute('href', h);
}
const QK = { need: 'needs', prog: 'working', queue: 'queued' }, ROLE = { impl: 'impl', review: 'rev', fix: 'fix', update: 'update' };
const EVK = d => {
  const k = d.dataset.k, a = d.querySelector('.kg')?.dataset.act;
  return k === 'merged' ? 'merged' : k === 'verdict' ? (a === 'blocked' ? 'fail' : 'rev') : /^needs-(human|decision)$/.test(k) ? 'needs' : k === 'queue' ? 'queued' : k === 'spawn' ? a : null;
};
function words(root) {
  if (!TT) return; // esc and the theme come later; herd.js's theme() runs rows(document) again
  for (const t of ['queue', 'lanes', 'host', 'events']) { // each tab's own first line: `the rail · events`
    const s = root.id === t ? root : root === document ? document.getElementById(t) : null;
    if (!s) continue;
    let p = s.querySelector(':scope>.tsub');
    if (!TW()['tab-' + t]) { p?.remove(); continue; }
    if (!p) s.prepend(p = el('p', 'tsub'));
    p.innerHTML = tw('tab-' + t, t);
  }
  if (root === document) syswords();
  root.querySelectorAll('.ask>p:first-child>b').forEach(b => relabel(b, 'needs', 'needs you'));
  root.querySelectorAll('.qg>h3').forEach(h => relabel(h, QK[h.parentNode.dataset.g], h.dataset.p ?? h.textContent));
  root.querySelectorAll('details.q').forEach(d => {
    const o = d.querySelector('summary>span:nth-of-type(2)'), s = o && (o.querySelector(':scope>span') || o);
    if (!s) return;
    const p = s.dataset.p ?? s.textContent, m = p.match(/^(impl|review|fix|update)(.*)$/), k = m ? ROLE[m[1]]
      : /^queued/.test(p) ? 'queued' : /needs-human|waiting/.test(p) ? 'needs' : p === 'dead' || p === 'parked' ? p : null;
    if (k) relabel(s, k, m ? PW[k] : k === 'needs' ? p.replace(/^⏸ /, '').replace('needs-human', 'needs you') : p, m ? m[2] : '');
    use(d.querySelector('summary .gt>b'), k === 'queued' || k === 'needs' ? k : null);
  });
  root.querySelectorAll('details.done>summary>span').forEach(s => relabel(s, 'done', s.dataset.p ?? s.textContent));
  root.querySelectorAll('#lanes h2>span').forEach(s => {
    const [, g, n] = (s.dataset.p ?? s.textContent).match(/^(\S+) (\(\d+\))$/) || [], k = g === 'blocked' ? 'needs' : g;
    if (PW[k]) relabel(s, k, PW[k], ' ' + n);
  });
  // Events: the kind's theme word before the row's words, in the row's tone, and its sprite by kind.
  root.querySelectorAll('details.er').forEach(d => {
    const sm = d.querySelector('.sm'), k = EVK(d);
    if (sm.dataset.p == null) sm.dataset.p = sm.innerHTML;
    const h = k && TW()[k] ? `<b class="thw ev">${esc(TW()[k])}</b> ${sm.dataset.p}` : sm.dataset.p;
    if (sm.innerHTML !== h) sm.innerHTML = h;
    use(d.querySelector('.kg'), k);
  });
  root.querySelectorAll('[data-v]').forEach(v => { if (!v.querySelector('.er, .ne')) v.append(el('p', 'm ne', 'no events')); });
  root.querySelectorAll('p.m').forEach(p => { // an empty tab: the theme line over the server's plain one
    const t = p.dataset.p ?? p.textContent, k = { 'no open work': 'empty-queue', 'no agents': 'empty-lanes', 'no events': 'empty-events' }[t];
    if (!k) return;
    p.dataset.p = t;
    p.classList.toggle('empty', !!TW()[k]);
    const h = TW()[k] ? `<b class=thw>${esc(TW()[k])}</b><small class=pw>${esc(t)}</small>` : esc(t);
    if (p.innerHTML !== h) p.innerHTML = h;
  });
  const ld = $('#ld');
  let q = ld.querySelector('.ldw');
  if (!q) ld.append(q = el('p', 'ldw'));
  if (q.innerHTML !== (TW().loading ? tw('loading', 'loading') : '')) q.innerHTML = TW().loading ? tw('loading', 'loading') : '';
  // A Host card's load: the theme's 48 × 16 mini (two frames) and its level word, `simmer · calm`; the same level keeps it.
  root.querySelectorAll('article.hcd').forEach(c => {
    const h = c.querySelector('.hhd .hnm'), had = h.querySelector('.lsm'), r = c.querySelector('details.rw[data-k=load]'), ls = TT.load_scene || {};
    const x = r && +r.querySelector('summary .w b')?.textContent / +r.dataset.l;
    if (!ls.minis || ls.kind === 'none' || !(x >= 0) || !document.getElementById('hl-calm')) return void had?.remove();
    const l = x >= 1 ? 'held' : x >= 0.5 ? 'busy' : 'calm', w = (ls.levels || {})[l] || l;
    if (had && had.dataset.l === l + w) return;
    had?.remove();
    const [, , bw, bh] = document.getElementById('hl-' + l)?.getAttribute('viewBox').split(' ') || [], size = bw == 48 ? '' : ` style="width:${bw}px;height:${bh}px"`; // #423: its own box
    h.insertAdjacentHTML('beforeend', `<span class=lsm data-l="${esc(l + w)}" role=img aria-label="load: ${esc(w)} (${l})"><svg viewBox="0 0 ${bw || 48} ${bh || 16}"${size} aria-hidden=true><use href="#hl-${l}"/></svg><b class=thw>${esc(w)}</b><small class=pw> · ${l}</small></span>`);
  });
  pushwords();
}
// #370: a moment plays on its issue's row on the visible tab, once per seq, 2.4 s (else the #mo overlay, as before). A
// swap that rebuilds the row mid-moment re-applies it with a negative delay: it resumes, never restarts.
const MO = new Map(), PLAYED = new Set(), MK = { merged: 'merged', fail: 'fail', 'needs-you': 'needs' };
function target(f, n, k) {
  const t = document.documentElement.dataset.tab, hit = a => a && a.textContent === '#' + n;
  if (t === 'queue') return [...$$('#queue details.q')].find(d => d.offsetParent && hit(d.querySelector('summary a')) && d.closest('.fc')?.querySelector('.fn>b')?.textContent === f);
  if (t === 'events') {
    const all = [...$$('#events details.er')].filter(d => d.dataset.f === f && d.offsetParent && hit(d.querySelector('summary .n')));
    return all.find(d => EVK(d) === MK[k]) || all[0];
  }
  if (t === 'lanes') return [...$$('#lanes .lane')].find(l => (l.dataset.agent || '').startsWith(f + '-') && l.dataset.agent.endsWith('-' + n));
}
const msum = r => r.querySelector('summary') || r.querySelector('.lr') || r;
function play(r, k, t0) {
  const s = msum(r);
  s.dataset.mo = k;
  s.style.setProperty('--mo-d', `${t0 - Date.now()}ms`);
  use(s.querySelector('.rsg, .kg'), MK[k]);
  if (!s.querySelector('.mot') && TW()[MK[k]]) s.insertAdjacentHTML('beforeend', `<i class=mot>${esc(TW()[MK[k]])}${k === 'merged' ? '!' : ''}</i>`);
}
function rmoment(k, f, n, seq) {
  if (PLAYED.has(seq)) return;
  PLAYED.add(seq);
  const r = n != null && TT && TT.moments && TT.moments[k] && target(f, n, k);
  if (!r) return moment(k);
  const m = { k, f, n, t0: Date.now() };
  MO.set(seq, m);
  play(r, k, m.t0);
  setTimeout(() => {
    MO.delete(seq);
    const x = target(f, n, k), s = x && msum(x);
    if (!s) return;
    delete s.dataset.mo;
    s.querySelector('.mot')?.remove();
    words(x.parentNode);
  }, 2400);
}
const again = () => MO.forEach(m => { const r = target(m.f, m.n, m.k); if (r && !msum(r).dataset.mo) play(r, m.k, m.t0); });
// #370: the push words of this phone's theme go with its subscription (`Order up: #2957 merged`); a theme switch re-sends
// them quietly (no test push). None: today's titles.
const pw = () => ({ words: JSON.stringify(Object.fromEntries(['merged', 'needs'].filter(k => TW()[k]).map(k => [k, TW()[k][0].toUpperCase() + TW()[k].slice(1)]))) });
let PWT = '';
async function pushwords() {
  const w = JSON.stringify(pw());
  if (DEMO || PWT === w || (get('push:w') ?? JSON.stringify({ words: '{}' })) === w || !('serviceWorker' in navigator)) return;
  PWT = w; // one try per page and theme
  try {
    const s = (await (await navigator.serviceWorker.ready).pushManager.getSubscription())?.toJSON();
    if (s && (await post('push/subscribe', { endpoint: s.endpoint, p256dh: s.keys.p256dh, auth: s.keys.auth, quiet: 1, ...pw() })).ok) put('push:w', w);
  } catch (e) {}
}
// Sync dot: g = live and up to date (pulses on an update), a = reconnecting (↻ spins), r = offline.
const dot = $('#dot'), rf = $('#rf');
let last = Number(B.dataset.ts) * 1000, seq = 0, es = null, down = Date.now(), tries = 0, poller = 0, busy = false;
function sync(s) {
  if (DEMO) s = 'demo';
  dot.className = s;
  rf.classList.toggle('spin', s === 'a' || busy);
  $('#stale').hidden = s !== 'r' && (DEMO || Date.now() - last < 120000); // #337: offline, or a copy over 2 min old
  const at = new Date(last).toTimeString().slice(0, 5);
  Object.assign($('#stale'), { textContent: 'as of ' + at, title: 'last update ' + at });
  $$('form button, form select:not([data-off]), .ask button:not(.lat)').forEach(b => b.disabled = s === 'r' || DEMO);
  if (DEMO) $$('input:not(.bl *), textarea, .acts button').forEach(b => b.disabled = true); // #365: the backlog's search and ticks write nothing
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
  if (id === 'chat') chatPaint(nu);
  tidy(nu);
  morph(el, nu);
  if (id === 'chat') chatPin(); // #407: before the scroll is put back (the room under a pinned message)
  again();
  bshow(el);
  nows();
  if (id === 'host') { groups(); alarm(); }
  if (id === 'sy-fleets') sysfleets(el);
  if (sysPin && sysOn()) sysgo(sysPin); // the board's first draws move the sections: the one asked for stays on top
  if (scrollY !== y) scrollTo(0, y);
  if (id === 'chat') chatFollow();
  if (old.size) rows(el).forEach((r, k) => { if (old.get(k) !== sig(r)) r.classList.add('fl'); }); // #267: a first draw (the TS board's) never flashes
  last = Date.now();
  sput('c:' + id, html);
  alerts();
  badge();
  if (id === 'needs') fold();
  if (id !== 'host' && id !== 'events' && !id.startsWith('sy-')) sync(dot.classList[0]); // #268, #269: the TypeScript board's draw takes the dot's state (offline, demo: disabled)
  if (id === 'queue' && want) wanted();
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
function badge() {
  const n = $$('#needs .ask').length, c = +($('#chn')?.dataset.n || 0); // #386: `[chat·N]`, the threads where a choice waits
  $('nav a[href="#queue"]>span').innerHTML = 'queue' + (n ? `<b class=n>·${n}</b>` : '');
  const ch = $('nav a[href="#chat"]>span');
  if (ch) ch.innerHTML = 'chat' + (c ? `<b class=n>·${c}</b>` : '');
}
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
    const r = await post('push/subscribe', { endpoint: s.endpoint, p256dh: s.keys.p256dh, auth: s.keys.auth, ...pw() });
    if (r.ok) put('push:w', JSON.stringify(pw()));
    const msg = (await r.text()).trim();
    if (r.status === 401) return unpaired();
    note(b, msg, r.ok);
    if (r.ok) refresh();
  } catch (err) { note(b, 'alerts failed: ' + err.message, false); }
});
// Network only (the SW passes the API through); spins ↻ until done; `quiet` (#403: after a chat post): no overlay.
async function refresh(quiet) {
  dbg('refresh');
  if (DEMO) return herdPull();
  claim();
  busy = true;
  sync('a');
  try {
    const t = document.documentElement.dataset.tab;
    await (quiet ? x => x : wait)(window.boardTab ? window.boardTab(t) : Promise.resolve()); // #269: the needs cards and the tab's part
    have.clear();
    have.add(t);
    dispatchEvent(new Event('herd-refresh')); // #266: the TypeScript board's tabs read the API again
    herdPull();
    busy = false;
    sync(live() ? 'g' : 'a');
    pulse();
  } catch (e) { busy = false; sync('r'); }
}
// Backoff 1, 2, 5, 10 s; after 30 s down, refresh every 10 s until SSE is back.
function connect() {
  if (DEMO) return;
  if (es) es.close();
  const t = watching(); // #407: the open thread: no push for it while this stream lives
  try { es = new EventSource('live?tab=' + (sysOn() ? 'host' : document.documentElement.dataset.tab) + (t ? '&thread=' + t : '') + (seq ? '&since=' + seq : '')); es.t = t; } catch (e) { es = null; return sync('r'); }
  // #259: every message carries the log's seq: a reconnect replays what this tab missed (`?since`), each seq once;
  // a moment plays only within 2 min of its event; past the 500-event bound the server says `refresh`.
  const at = e => (seq = Math.max(seq, +e.lastEventId || 0));
  // After a gap (the first open included: a cached page may have painted) the dot is refresh()'s: amber until the data lands.
  es.onopen = () => { if (down) refresh(); else sync('g'); down = 0; tries = 0; clearInterval(poller); poller = 0; };
  // #269: a changed part's data (the needs cards, Lanes, [chat]) as this device sees it; the TypeScript board draws it.
  for (const id of ['needs', 'lanes', 'chat']) es.addEventListener(id, e => { at(e); dispatchEvent(new CustomEvent('herd-' + id, { detail: JSON.parse(e.data) })); sync('g'); pulse(); claim(); });
  // #391: a page of another build (the service worker's copy) cannot draw this server's parts: it loads again, once a build.
  es.addEventListener('build', e => { if (!$(`script[src*="/${e.data}/"]`) && sget('build') !== e.data) { sput('build', e.data); location.reload(); } });
  es.addEventListener('herd', e => (at(e), herdData(JSON.parse(e.data))));
  es.addEventListener('host', e => (at(e), dispatchEvent(new CustomEvent('herd-host', { detail: JSON.parse(e.data) })))); // #267: the TypeScript board draws it
  es.addEventListener('queue', e => (at(e), dispatchEvent(new CustomEvent('herd-queue', { detail: JSON.parse(e.data) })))); // #268: and the Queue tab
  es.addEventListener('ev', e => { const j = JSON.parse(e.data); if (j.seq <= seq) return; at(e); dispatchEvent(new Event('herd-ev')); if (j.k && Date.now() / 1e3 - j.ts < 120) rmoment(j.k, j.f, j.n, j.seq); });
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
document.addEventListener('visibilitychange', () => { if (!document.hidden) { down = down || Date.now(); tries = 0; connect(); } else if (es && es.t) connect(); });
addEventListener('online', () => { tries = 0; connect(); });
addEventListener('offline', () => sync('r'));
if (location.search) history.replaceState(null, '', location.pathname + (location.search.includes('all=1') ? '?all=1' : '') + location.hash);
// #66: an action's result is a one-line toast under the header: ok fades after 4 s (report mode's stays; #407: or
// after `stay` ms), an error stays; a tap dismisses it.
function toast(text, stay) {
  $$('.msg.g, .msg.r').forEach(m => m.remove());
  const m = document.createElement('p'), ok = !/^(REFUSED|error)/.test(text);
  m.className = 'msg ' + (ok ? 'g' : 'r');
  m.textContent = text;
  m.onclick = () => m.remove();
  $('header').after(m);
  if (ok && stay !== true) setTimeout(() => { m.classList.add('out'); setTimeout(() => m.remove(), 400); }, stay || 4000);
  return m;
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
const S = $('#sheet'), shown = () => S.classList.contains('on'), sh = $('.sh'), chat = $('.chat'), st = $('.st'), now = $('.chat>.now'), more = $('.more');
let agent = null, timer = 0, nopane = false, opane = null, spane = null;
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
    for (const g of (d.dataset.s || '').split(' ').filter(Boolean)) tag('tag m', '🔒' + g, 'serial: one lane at a time');
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
// #367 queue: pure from here to `queue:end` (tests/queue_tab.rs runs it under node). A row's group: needs you, in
// progress, queued (the rest); its time in one unit style, naming the phase it measures; a blocked chain's one line.
const QG = { need: 'needs you', prog: 'in progress', queue: 'queued' };
const qgroup = s => /^(needs-human|⏸)/.test(s) ? 'need' : /^(impl|review|fix|ready|update|dead|parked)/.test(s) ? 'prog' : 'queue';
const dur = t => t < 60 ? `${t}s` : t < 3600 ? `${Math.floor(t / 60)}m` : t < 86400 ? `${Math.floor(t / 3600)}h ${Math.floor(t % 3600 / 60)}m` : `${Math.floor(t / 86400)}d ${Math.floor(t % 86400 / 3600)}h`;
const qphase = s => s.replace(/^⏸\s*/, '').split(/[\s(:]/)[0], inph = (s, t) => `in ${qphase(s)} ${dur(t)}`;
// steps: [{n, title, b}] depth-first, the root first (b: its blocker); s: the state word of the step the chain waits on
// next (the root's open blocker, else the root), '' when off the board. #391: the chain row's third line, first → last.
const qsum = (st, s) => `#${st[0].n} → #${st.at(-1).n} · next: #${st[0].b || st[0].n}${s ? ` (${qgroup(s) === 'prog' ? 'in ' : ''}${qphase(s)})` : ''}`;
// queue:end
// #365 backlog: pure from here to `backlog:end` (tests/backlog.rs runs it under node). The server's last read {n: {title,
// labels, created, updated, ac, pr}} as rows newest first, `stale` when untouched for 90 days; a row shows when it has
// the picked label, every pressed quick filter and every search word; ticks keep their order ([queue N] queues so).
const BQ = [['new', 'new'], ['30', '>30 days'], ['90', '>90 days'], ['ac', 'has AC'], ['pr', 'in a closed PR']];
const BF = { new: d => d.age < 7 * 86400, 30: d => d.age > 30 * 86400, 90: d => d.age > 90 * 86400, ac: d => d.ac, pr: d => d.pr };
const brows = (is, now) => Object.entries(is).map(([n, i]) => ({ n, title: i.title || '', labels: i.labels || [], ac: !!i.ac, pr: !!i.pr,
  age: now - Date.parse(i.created) / 1000, stale: now - Date.parse(i.updated) / 1000 > 90 * 86400 })).sort((a, b) => b.n - a.n);
const bpass = (d, s) => (!s.label || d.labels.includes(s.label)) && s.on.every(k => BF[k](d)) &&
  s.q.toLowerCase().split(/\s+/).every(w => `#${d.n} ${d.title} ${d.labels.join(' ')}`.toLowerCase().includes(w));
const btick = (ticks, n, on) => [...ticks.filter(x => x !== n), ...(on ? [n] : [])];
// backlog:end
// #365: each repo's backlog state (search, filters, label, ticks) lives here, so a /live patch never drops it; a
// ticked issue that left the backlog (queued) drops its tick. bdraw builds a fresh copy's rows (tidy), bshow applies it.
const BS = {}, bst = b => (BS[qname(b.closest('.fc'))] ??= { q: '', on: [], label: '', ticks: [] });
const bx = s => String(s).replace(/[&<>"]/g, c => `&#${c.charCodeAt(0)};`), bnow = () => Date.now() / 1000 + SKEW;
function bdraw(root) {
  for (const b of root.querySelectorAll('.bl')) {
    const rs = brows(JSON.parse(b.dataset.bl), bnow()), ls = [...new Set(rs.flatMap(r => r.labels))].sort();
    b.querySelector('summary').insertAdjacentHTML('afterend', `<p class=bs><input type=search placeholder="search #, title, label" aria-label="search the backlog"></p>
<p class="chips bfs">${BQ.map(([k, l]) => `<button type=button data-bf=${k}>${bx(l)}</button>`).join('')}<select aria-label=label><option value="">any label</option>${ls.map(l => `<option>${bx(l)}</option>`).join('')}</select></p>`);
    b.querySelector('.bls').innerHTML = rs.map(r => `<label class=bi data-n=${r.n}><input type=checkbox value=${r.n}><span>#${r.n}</span><span class=tl>${bx(r.title)}</span><span class=m>${bx(r.labels.join(' '))}${r.stale ? ' <span class="tag m">stale</span>' : ''} ${dur(Math.max(0, Math.round(r.age)))}</span></label>`).join('');
  }
}
function bshow(root) {
  for (const b of root.querySelectorAll('.bl')) {
    const s = bst(b), rs = brows(JSON.parse(b.dataset.bl), bnow()), f = b.querySelector('.bbar');
    s.ticks = s.ticks.filter(n => rs.some(r => +r.n === n));
    for (const d of rs) {
      const r = b.querySelector(`.bi[data-n="${d.n}"]`);
      if (!r) continue;
      r.hidden = !bpass(d, s);
      r.querySelector('input').checked = s.ticks.includes(+d.n);
    }
    const shown = rs.filter(d => bpass(d, s)).length, i = b.querySelector('.bs input'), sel = b.querySelector('.bfs select');
    b.querySelector('summary>.m').textContent = shown === rs.length ? `(${rs.length})` : `(${shown} of ${rs.length})`;
    if (i.value !== s.q) i.value = s.q;
    if (sel.value !== s.label) { sel.value = s.label; sel.dispatchEvent(new Event('change')); }
    b.querySelectorAll('[data-bf]').forEach(x => x.setAttribute('aria-pressed', s.on.includes(x.dataset.bf)));
    f.hidden = !s.ticks.length;
    f.elements.issues.value = s.ticks.join(',');
    const t = f.querySelector('button');
    (t.firstElementChild || t).textContent = `queue ${s.ticks.length}`;
  }
}
document.addEventListener('input', e => { if (e.target.matches('.bl .bs input')) { bst(e.target).q = e.target.value; bshow(document); } });
document.addEventListener('change', e => {
  const t = e.target, s = t.closest('.bl') && bst(t);
  if (!s) return;
  if (t.matches('.bi input')) s.ticks = btick(s.ticks, +t.value, t.checked);
  else if (t.matches('select')) s.label = t.value;
  bshow(document);
});
document.addEventListener('click', e => {
  const x = e.target.closest('.bl [data-bf]'), s = x && bst(x), k = x && x.dataset.bf;
  if (s) { s.on = s.on.includes(k) ? s.on.filter(y => y !== k) : [...s.on, k]; bshow(document); }
});
// #367: the Queue tab one repo at a time. Chips on top ([repo n], a red dot when it needs you; [all]: every repo's
// needs-you rows), the pick kept per phone; a tap, a sideways swipe or ←/→ switches. A repo's rows in groups: needs
// you, in progress (`in impl 1h 47m`), queued: the next 5 by position, `+ N more` opening the rest in place; a blocked
// chain is one row whose [▾] opens its steps. Every step is idempotent: queue-scroll pushes a tidied copy back.
const SKEW = Number(B.dataset.ts) - Date.now() / 1000 || 0; // the server's clock, as the page's ages count by it
const qname = c => c.querySelector('.fn>b').textContent;
const qst = d => d.querySelector('summary>span:nth-of-type(2)').textContent;
function qsplit(root) {
  for (const c of root.querySelectorAll('.fc')) {
    const rs = [...c.querySelectorAll(':scope>details.q')];
    if (!rs.length || c.querySelector('.qg')) continue;
    const gs = {};
    for (const d of rs) {
      const g = qgroup(qst(d)), m = d.querySelector('summary>.m');
      const t = Math.max(0, Math.round(Date.now() / 1000 + SKEW - m.dataset.t));
      // A running row names its phase; a needs-you row's long state word (`⏸ waiting: decision`) names it already.
      if (g !== 'queue' && m.dataset.t) m.textContent = g === 'prog' ? inph(qst(d), t) : dur(t);
      (gs[g] = gs[g] || []).push(d);
    }
    const at = el('i', '');
    rs[0].before(at);
    at.replaceWith(...Object.keys(QG).filter(g => gs[g]).map(g => {
      const x = el('div', 'qg');
      x.dataset.g = g;
      x.append(el('h3', '', `${QG[g]} (${gs[g].length})`), ...gs[g]);
      return x;
    }));
  }
}
function qfold(root) {
  for (const g of root.querySelectorAll('.qg[data-g=queue]:not(.qd)')) {
    g.classList.add('qd');
    const items = [];
    for (const d of g.querySelectorAll(':scope>details.q')) if (d.matches('.d0') || !items.length) items.push([d]); else items.at(-1).push(d);
    const tops = items.map(([d, ...rest], i) => {
      // A row's place in the queue; a chain's steps leave theirs to the chain row.
      [d, ...rest].forEach(x => { x.querySelector('summary>.m').textContent = rest.length ? '' : '#' + (i + 1); });
      if (!rest.length) return d;
      // #391: a normal row: the first step's avatar, number and title, `chain · N` as its state, its place, and a third
      // line naming the steps and the one it waits on next, with that one's state when it is on the board.
      const n = x => x.querySelector('summary>span').textContent, st = [d, ...rest].map(x => ({ n: n(x).slice(1), b: x.dataset.b }));
      const nx = [...g.parentNode.querySelectorAll('details.q')].find(x => n(x) === '#' + (st[0].b || st[0].n));
      const c = el('details', 'qc'), s = d.querySelector('summary').cloneNode(true);
      s.querySelector('.gt>b')?.classList.remove('k');
      s.querySelector(':scope>span:nth-of-type(2)').replaceChildren(el('span', 'a', `chain · ${st.length}`));
      s.querySelector(':scope>.m').textContent = '#' + (i + 1);
      s.append(el('span', 'cst', qsum(st, nx ? nx.querySelector('summary>span:nth-of-type(2)>span').textContent : '')));
      c.dataset.act = d.dataset.act;
      if (d.dataset.grow) Object.assign(c.dataset, { grow: d.dataset.grow, i: d.dataset.i }); // #423: its plant
      c.append(s);
      d.before(c);
      c.append(d, ...rest);
      return c;
    });
    if (tops.length <= 5) continue;
    const x = el('details', 'qx');
    x.innerHTML = `<summary><span>+</span><span> ${tops.length - 5} more</span></summary>`;
    tops[4].after(x);
    x.append(...tops.slice(5));
  }
}
let QR = get('queue:repo');
function qchips(root) {
  const q = root.id === 'queue' ? root : root.querySelector('#queue'), cs = q ? [...q.querySelectorAll('.fc')] : [];
  if (!cs.length) return;
  const all = q.querySelectorAll('.qg[data-g=need]>details.q').length, p = q.querySelector(':scope>.qch') || el('p', 'chips qch');
  const chip = (k, name, n, need) => {
    const b = el('button', 'fc2 ' + k), t = el('span', '', `${name} ${n}`);
    Object.assign(b, { type: 'button' }).dataset.qr = name;
    if (need) t.append(el('b', 'r', '●'));
    b.append(t);
    return b;
  };
  p.replaceChildren(...cs.map(c => chip([...c.classList].find(k => k.startsWith('k-')) || '', qname(c), c.querySelectorAll('details.q').length, c.querySelector('.fn .tag.r'))), chip('', 'all', all, all));
  q.prepend(p);
  qshow(q);
}
function qshow(q) {
  const ns = [...q.querySelectorAll('.qch [data-qr]')].map(b => b.dataset.qr), pick = ns.includes(QR) ? QR : ns[0];
  q.querySelectorAll('.qch [data-qr]').forEach(b => b.setAttribute('aria-pressed', b.dataset.qr === pick));
  q.querySelectorAll('.fc').forEach(c => {
    c.hidden = pick === 'all' ? !c.querySelector('.qg[data-g=need]') : qname(c) !== pick;
    c.classList.toggle('qa', pick === 'all');
  });
}
function qpick(name) { QR = name; put('queue:repo', name); if ($('#queue')) qshow($('#queue')); }
const qstep = k => { const ns = [...$$('.qch [data-qr]')].map(b => b.dataset.qr), i = ns.indexOf($('.qch [aria-pressed=true]')?.dataset.qr); if (ns.length) qpick(ns[((i + k) % ns.length + ns.length) % ns.length]); };
document.addEventListener('click', e => { const b = e.target.closest('.qch [data-qr]'); if (b) qpick(b.dataset.qr); });
document.addEventListener('keydown', e => {
  if (document.documentElement.dataset.tab !== 'queue' || $('#herd')?.hidden === false || shown() || e.altKey || e.ctrlKey || e.metaKey || e.target.closest?.('input, textarea, select')) return;
  const k = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
  if (k) { e.preventDefault(); qstep(k); }
});
// A sideways swipe (touch or mouse) over 40 px, more across than down, switches; the click that ends it opens nothing.
let qsw = null, qswiped = false;
document.addEventListener('pointerdown', e => { qsw = e.target.closest('#queue') && !e.target.closest('input, textarea, select') ? { x: e.clientX, y: e.clientY, id: e.pointerId } : null; qswiped = false; });
document.addEventListener('pointerup', e => {
  const dx = qsw && qsw.id === e.pointerId ? e.clientX - qsw.x : 0, dy = qsw ? e.clientY - qsw.y : 0;
  qsw = null;
  if (Math.abs(dx) > 40 && Math.abs(dx) > 2 * Math.abs(dy)) { qswiped = true; qstep(-Math.sign(dx)); }
});
document.addEventListener('click', e => { if (qswiped && e.target.closest('#queue')) { qswiped = false; e.preventDefault(); e.stopPropagation(); } }, true);
function tidy(root) {
  bdraw(root);
  qsplit(root);
  graph(root);
  qfold(root);
  qchips(root);
  rows(root);
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
  bshow(root);
}
tidy(document);
nows();
// A tapped alert's issue: its repo picked, its row open (the chain or `+ N more` around it too), in view (#268: once
// the TypeScript board first draws the Queue tab).
function wanted() {
  if (document.documentElement.dataset.tab === 'queue') for (const d of $$('#queue details.q')) if (want && d.querySelector('summary>span').textContent === '#' + want) {
    qpick(qname(d.closest('.fc')));
    for (let p = d; p; p = p.parentElement.closest('details')) p.open = true;
    d.scrollIntoView();
  }
  want = null;
}
// [↻] is the plain glyph, spinning while busy (#337: the theme's loading sprite is the overlay's, herd.js builds #hload).
rf.innerHTML = '<i>↻</i>';
document.addEventListener('visibilitychange', () => document.documentElement.classList.toggle('bg', document.hidden));
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
// #351: a chat opens with its last 10 turns (~64 KB at most, the server's cap); scrolling to the top loads 10 more.
const lane = (q, a = agent) => fetch(DEMO ? 'lane/' + encodeURIComponent(a) + '.json' : R + 'lane?agent=' + encodeURIComponent(a) + '&lines=200&turns=' + turns + q);
// The settled lines seen (merged across reads by their overlap) plus the live block, drawn as blocks: a block whose key
// and HTML are unchanged keeps its node (no scroll jump, #65), new lines land in the last block; open state per lane.
let hist = [], tail = [], nlive = 0, harness = '', spin = false, said = null, notes = [], turns = 10;
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
// #303: a setup terminal's screen (masked), an [open] link per printed login URL; closed: the reply box goes.
async function spoll() {
  const r = await fetch(R + 'lane?setup=' + encodeURIComponent(spane)).catch(() => null);
  if (r && r.status === 401) unpaired();
  const j = r && r.ok ? await r.json() : { closed: true, lines: [] };
  now.innerHTML = `<pre class=out>${j.lines.map(esc).join('\n')}</pre>` + (j.urls || []).map(u => `<p><a href="${esc(u)}" target=_blank rel=noopener>[open]</a> <span class=m>${esc(u.slice(0, 48))}…</span></p>`).join('');
  if (j.closed) { clearInterval(timer); $('.reply').hidden = true; say('sign-in closed', null); }
}
async function poll() {
  if (spane) return spoll();
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
    if (!hist.length) more.hidden = !(j.more || j.older);
    hist = k || !hist.length || !done.length ? hist.concat(done.slice(k)) : done;
    [tail, nlive, harness, spin, said, notes] = [j.lines.slice(done.length), j.live, j.harness, !!j.status, j.said, j.notices];
    $('.live').hidden = !j.status;
    if (j.status) $('.live').innerHTML = `<b>${esc([...j.status][0])}</b>${esc(j.status.slice([...j.status][0].length))}`;
    paint();
  } catch (e) { now.textContent = 'no pane output'; }
}
// Older lines from the rolling transcript, joined above the seen ones by their overlap.
more.onclick = async () => {
  more.disabled = true;
  turns += 10;
  const r = await lane('&before=' + Math.max(0, hist.length + nlive - 20)).catch(() => null);
  more.disabled = false;
  if (!r || !r.ok) return;
  const j = await r.json(), h = chat.scrollHeight;
  hist = j.lines.slice(0, j.lines.length - overlap(j.lines, hist)).concat(hist);
  said = j.said || said;
  more.hidden = !(j.more || j.older);
  paint();
  chat.scrollTop += chat.scrollHeight - h;
};
chat.addEventListener('scroll', () => { if (chat.scrollTop < 40 && !more.hidden && !more.disabled) more.onclick(); }, { passive: true });
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
  spane = e.dataset.setup || null;
  const top = $('.top');
  top.innerHTML = e.querySelector('.card').innerHTML;
  top.scrollTop = 0;
  $('.hd b').textContent = agent;
  const rev = e.dataset.role === 'rev';
  $('.reply').hidden = !!B.dataset.ro || rev;
  $('.acts').hidden = !!B.dataset.ro || !!spane;
  $('.fvr').hidden = !!spane;
  // #83: a lane with no pane has nothing to stop or kill; #95: an Others agent is no lane, only its pane closes.
  nopane = 'gone' in e.dataset;
  const other = e.dataset.role === 'other';
  $$('[data-op=stop], [data-op=kill]').forEach(b => b.hidden = nopane || other);
  $('[data-op=clean]').hidden = other;
  $('[data-op=close]').hidden = !other || nopair;
  $$('.pairf, .cf, .cmds').forEach(x => x.hidden = true);
  now.replaceChildren();
  hist = []; tail = []; said = null; notes = []; turns = 10;
  $('.live').hidden = true;
  say(spane ? 'typed text goes to the login, then Enter (empty: Enter only)' : rev ? 'reviewer lane: read-only' : other ? 'not a herdsman lane: herdsman can only close its pane' : '', other || spane ? null : true);
  sh.className = 'sh';
  show(kb() ? 'full' : 'mid');
  poll();
  if (!other && !spane && !B.dataset.ro && !DEMO) cleanable();
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
// #303: [sign in ▾] on a host card: the host starts that configured login by name in a pane; its sheet shows the screen.
document.addEventListener('click', async e => {
  const b = e.target.closest('[data-su]');
  if (!b) return;
  const r = await post('setup', { host: b.dataset.host, name: b.dataset.su }), m = (await r.text()).trim();
  if (r.status === 401) unpaired();
  if (!r.ok) return toast(m);
  const x = el('div');
  x.innerHTML = '<div class=card><p class="nw el"></p></div>';
  x.querySelector('p').textContent = `sign in: ${b.dataset.su} on ${b.dataset.host}; closes when it exits or in 10 min`;
  Object.assign(x.dataset, { agent: 'sign in · ' + b.dataset.su, role: 'setup', setup: m });
  const d = b.closest('details'); // a chat line's [sign in] has none
  if (d) d.open = false;
  open(x);
});
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
  // #294: a card's groups: names by spaces, their paths by ` | `.
  const serial = new Set([...$$('#queue [data-sp]')].flatMap(f => f.dataset.sp.split(' | ').map((p, i) => `🔒${f.dataset.s.split(' ')[i]} one at a time: touches ${p}`)));
  const dots = ['● a lane is on it', '○ waiting', '✓ ready', '! needs you'];
  $('.top').replaceChildren(...Object.entries(HINT).map(([w, h]) => el('p', '', `${w} = ${h}`)), ...[...dots, ...serial].map(t => el('p', '', t)));
  show('mid');
}
// #162: [config] on a Queue card, #440: System opened at that fleet's row (the button stays, a shortcut).
function config(b) { sysopen('fleets', b.dataset.f); }
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
const CMDS = ['/stop', '/kill', '/clean', '/compact', '/clear', '/status', '/report'], ta = $('.reply textarea'), cmds = $('.cmds');
const matches = () => /^\/\S*$/.test(ta.value) ? CMDS.filter(c => c.startsWith(ta.value)) : [];
function pick(c) {
  cmds.hidden = true;
  if (['/stop', '/kill', '/clean'].includes(c)) { ta.value = ''; return run(c.slice(1)); }
  if (c === '/report') { ta.value = ''; return report(); }
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
// #363 report mode: `/report` closes the sheet; a tap outlines an element, a second tap on it confirms, and a one-line
// note sends its markup (8 KB), key styles, the board's context and a PNG of it plus 24 px to `/report` (paired
// phones only; serve keeps it under state/reports/). The PNG is drawn from the layout: boxes, borders and words.
const KEYS = ['display', 'position', 'width', 'height', 'margin', 'padding', 'border', 'border-radius', 'font-family', 'font-size',
  'font-weight', 'line-height', 'color', 'background-color', 'opacity', 'overflow', 'white-space', 'text-overflow', 'z-index'];
let rep = null;
function report() { close(); B.classList.add('rep'); toast('report: tap an element, then tap it again · tap here to stop', true); }
function unreport() { B.classList.remove('rep'); if (rep) rep.classList.remove('repo'); rep = null; $$('.repf').forEach(f => f.remove()); }
document.addEventListener('click', e => {
  if (!B.classList.contains('rep') || e.target.closest('.repf')) return;
  if (e.target.closest('.msg')) return unreport(); // the toast: a tap on it stops report mode
  e.preventDefault();
  e.stopImmediatePropagation();
  if (rep === e.target) return repnote();
  unreport();
  B.classList.add('rep');
  (rep = e.target).classList.add('repo');
}, true);
function repnote() {
  const f = el('form', 'repf');
  f.innerHTML = '<input name=note maxlength=200 placeholder="what is wrong here?" aria-label="report note"><button>Send</button><button type=button>Cancel</button>';
  B.append(f);
  f.note.focus();
  f.lastChild.onclick = unreport;
  f.onsubmit = async e => {
    e.preventDefault();
    const x = rep, note = f.note.value, h = document.documentElement, cs = getComputedStyle(x);
    unreport();
    const ctx = { tab: h.dataset.tab, theme: h.dataset.theme, scheme: h.dataset.scheme, style: B.dataset.mode,
      viewport: [innerWidth, innerHeight, devicePixelRatio], live: seq };
    const d = { html: x.outerHTML.slice(0, 8192), styles: JSON.stringify(Object.fromEntries(KEYS.map(k => [k, cs.getPropertyValue(k)]))),
      ctx: JSON.stringify(ctx), note, png: shot(x) };
    try { const r = await post('report', d); toast((r.ok ? '' : 'error: ') + (await r.text()).trim()); } catch (err) { toast('error: ' + err.message); }
  };
}
// The visible part of `x` plus 24 px, at the device's pixel ratio: each box's background and border, then each word.
function shot(x) {
  const r = x.getBoundingClientRect(), dpr = devicePixelRatio || 1, L = Math.max(0, r.left - 24), T = Math.max(0, r.top - 24);
  const W = Math.max(1, Math.min(innerWidth, r.right + 24) - L), H = Math.max(1, Math.min(innerHeight, r.bottom + 24) - T);
  const c = Object.assign(document.createElement('canvas'), { width: Math.ceil(W * dpr), height: Math.ceil(H * dpr) }), g = c.getContext('2d');
  g.scale(dpr, dpr);
  g.translate(-L, -T);
  g.fillStyle = getComputedStyle(B).backgroundColor;
  g.fillRect(L, T, W, H);
  const hit = q => q.width && q.height && q.right > L && q.left < L + W && q.bottom > T && q.top < T + H, rg = document.createRange();
  const w = document.createTreeWalker(B, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  for (let n = w.currentNode; n; n = w.nextNode()) {
    const s = getComputedStyle(n.nodeType === 1 ? n : n.parentElement);
    if (s.visibility !== 'visible') continue;
    if (n.nodeType === 1) {
      const q = n.getBoundingClientRect(), b = parseFloat(s.borderTopWidth);
      if (!hit(q)) continue;
      g.fillStyle = s.backgroundColor;
      g.fillRect(q.left, q.top, q.width, q.height);
      if (b) { g.strokeStyle = s.borderTopColor; g.lineWidth = b; g.strokeRect(q.left + b / 2, q.top + b / 2, q.width - b, q.height - b); }
      continue;
    }
    [g.font, g.fillStyle, g.textBaseline] = [`${s.fontWeight} ${s.fontSize} ${s.fontFamily}`, s.color, 'middle'];
    for (const m of n.textContent.matchAll(/\S+/g)) {
      rg.setStart(n, m.index);
      rg.setEnd(n, m.index + m[0].length);
      const q = rg.getBoundingClientRect();
      if (hit(q)) g.fillText(m[0], q.left, q.top + q.height / 2);
    }
  }
  return c.toDataURL('image/png');
}
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
  // #422: a conductor's row opens its thread in the chat tab (a message there wakes an asleep one).
  const th = b.classList.contains('lr') && b.parentNode.dataset.thread;
  if (th) { tab('chat'); if (es) connect(); chatOpen(th); return history.pushState({ thr: th }, ''); }
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
  const r = await post('send', spane ? { setup: spane, text: e.target.text.value } : { agent, text: e.target.text.value });
  const msg = (await r.text()).trim();
  if (r.status === 401) unpaired();
  say(msg, r.ok);
  if (r.ok) { e.target.text.value = ''; setTimeout(poll, 1500); }
};
// #59: Pair this phone (header, blocked cards, the sheet): POST `pair/request` shows a code; its 10 min HttpOnly cookie is this
// browser's alone. The Mac approves it (`serve approve`, logged: its `ev` arrives on `/live`, or ↻), then `pair/claim` keeps it a year.
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
  // #443: not in a phone's open thread: it is fixed, and its messages scroll by themselves
  const thr = innerWidth < 1000 && e.target.closest && e.target.closest('#chat .cvp');
  p0 = (hp ? hp.scrollTop : scrollY) <= 0 && !shown() && !sysOn() && !typing && !thr ? e.touches[0] : null;
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
// #386: the [chat] tab. The page carries every thread's view; one is open (sessionStorage, or a push's `?thread=`):
// on a phone the list or that thread, from 1000 px both. #cht is the style that picks it, so a live swap keeps it.
// `var`: tab() may paint a stored copy of the tab (frag → chatPaint) before this line runs.
// #405: ATS, the files picked for each thread's next message: { <t>: [{ name, file, src }] } (src: an image's object URL);
// #407: kept on the device (IndexedDB), as each thread's draft text is (localStorage `draft:<t>`).
var THR = DBG.get('thread') || sget('thread') || '', ATS = {}, HP = {}; // #404 HP: an empty thread's harness pick
B.appendChild(Object.assign(document.createElement('style'), { id: 'cht' }));
// #443: `--kb`, the keyboard's height: the part of the layout viewport under the visual one (iOS never shrinks the
// first); a phone's open thread ends there (serve.css). iOS scrolls the page to show a focused box: put back. A change
// in the messages' height (the keyboard, the box growing, a turn) moves their scroll by as much, so the message just
// above the box stays just above it.
// #488: the layout viewport is the root's clientHeight: iOS shrinks `innerHeight` with the visual one. A Home Screen
// app may fire no `resize` as the keyboard opens or closes, so a focus change measures again as the keyboard moves.
const kbFit = () => {
  if (vv) document.documentElement.style.setProperty('--kb', Math.max(0, Math.round(document.documentElement.clientHeight - vv.height - vv.offsetTop)) + 'px');
  if (THR && document.documentElement.dataset.tab === 'chat' && innerWidth < 1000 && scrollY) scrollTo(0, 0);
};
if (vv) { vv.addEventListener('resize', kbFit); vv.addEventListener('scroll', kbFit); }
addEventListener('resize', kbFit);
['focusin', 'focusout'].forEach(k => addEventListener(k, () => [0, 150, 300, 600, 1000].forEach(ms => setTimeout(kbFit, ms))));
// (Their bottom edge is kept, `end`: a scroll only moves it at the height last seen, so the browser clamping a grown
// list's scroll is never counted.)
const tcKeep = new ResizeObserver(es => es.forEach(({ target: c }) => {
  const h = c.clientHeight;
  if (c.kh && h) c.scrollTop = c.end - h;
  c.kh = h;
  c.end = c.scrollTop + h;
}));
document.addEventListener('scroll', e => { const c = e.target; if (c.kh && c.clientHeight === c.kh) c.end = c.scrollTop + c.kh; }, true);
// The open thread's scroller: on a phone its messages, from 1000 px the page.
const thrSc = () => innerWidth < 1000 && THR ? $(`#chat .cvp[data-t="${CSS.escape(THR)}"] .tc`) : null;
function chatOpen(t) {
  THR = t;
  sput('thread', t);
  const q = CSS.escape(t);
  $('#cht').textContent = t ? `#chat .cvp:not([data-t="${q}"]){display:none}@media (max-width:999px){#chat .clp,html[data-tab=chat] :is(#needs,.later){display:none}html[data-tab=chat],html[data-tab=chat] body{height:100%;overflow:hidden}html[data-tab=chat] .msg.r{position:fixed;top:calc(2 * var(--hit) + var(--s1) + 2 * var(--s2))}}
@media (min-width:1000px){#chat .tr[data-t="${q}"]{background:color-mix(in srgb,var(--fk) 10%,transparent);box-shadow:inset 2px 0 var(--fk)}}` : '#chat .cvp{display:none}';
  if (t && !t.startsWith('new-') && !B.dataset.ro && document.documentElement.dataset.tab === 'chat') send('thread', { op: 'read', t }).catch(() => {});
  if (es && (es.t || '') !== watching()) connect(); // #407: the stream names the open thread (no push for it)
  FOL = true;
  if (PIN && PIN.t !== t) PIN = null;
  if (t) chatFollow();
}
// #403: back to the list: the history entry a thread's open pushed, else straight there.
const chatBack = () => history.state && history.state.thr ? history.back() : chatOpen('');
addEventListener('popstate', e => chatOpen((e.state && e.state.thr) || ''));
// #407: the thread a board is looking at (the chat tab open on it, the page shown): its stream names it, so the
// server pushes nothing for it.
function watching() { return document.documentElement.dataset.tab === 'chat' && !document.hidden && THR && !THR.startsWith('new-') ? THR : ''; }
// #407: the thread follows its end (a live update scrolls to it) until your own drag or wheel leaves the end; back at
// the very end, or on a send, it follows again. PIN (after a send): that message stays ~16 px under the thread bar,
// the room under it kept free until the reply fills it.
var FOL = true, PIN = null, held = -1e9;
addEventListener('touchstart', () => { held = 1; }, { passive: true });
addEventListener('touchend', () => { held = performance.now(); }, { passive: true });
addEventListener('wheel', () => { held = performance.now(); }, { passive: true });
addEventListener('scroll', e => {
  if (!THR || document.documentElement.dataset.tab !== 'chat') return;
  const c = thrSc();
  if (c ? e.target !== c : e.target.matches) return;
  const end = c ? c.scrollTop + c.clientHeight >= c.scrollHeight - 2 : scrollY + innerHeight >= document.documentElement.scrollHeight - 2;
  if (end) FOL = true; else if (held === 1 || performance.now() - held < 500) FOL = false;
}, { passive: true, capture: true });
function chatPin() {
  const v = PIN && $(`#chat .cvp[data-t="${CSS.escape(PIN.t)}"]`), tc = v && v.querySelector('.tc');
  if (!tc) return;
  tc.style.paddingBottom = '';
  const mine = tc.querySelectorAll('.blk.me:not(.snt)'), e = mine[PIN.n] || tc.querySelector(`.snt[data-p="${SENT.indexOf(PIN)}"]`);
  if (!e) return;
  if (!FOL || THR !== PIN.t || document.documentElement.dataset.tab !== 'chat') return void (tc.style.paddingBottom = PIN.pad + 'px');
  // At the end, its top 16 px under whatever bar shows at the top (the header, the thread bar), else the screen's
  // top: measured there, the room corrected (a bar that sticks once scrolled moves it).
  const base = parseFloat(getComputedStyle(tc).paddingBottom), s4 = parseFloat(getComputedStyle(B).getPropertyValue('--s4'));
  let pad = base;
  for (let i = 0; i < 5; i++) { // (room that a short page's screen still swallows takes another pass)
    tc.style.paddingBottom = pad + 'px';
    chatBottom();
    const err = e.getBoundingClientRect().top - Math.max(0, ...[$('header'), v.querySelector('.th')].map(x => x.getBoundingClientRect().bottom)) - s4;
    if (Math.abs(err) < 1) break;
    pad = Math.max(base, pad + err);
  }
  PIN.pad = pad;
}
// (Also when the thread's size changes under it: an image loading, the layout above settling.)
if ($('#chat')) new ResizeObserver(() => chatFollow()).observe($('#chat'));
function chatBottom() {
  const c = thrSc();
  if (!c) return void scrollTo({ top: document.documentElement.scrollHeight, behavior: 'instant' });
  tcKeep.observe(c);
  c.scrollTop = c.scrollHeight;
  c.end = c.scrollTop + c.clientHeight;
}
function chatFollow() {
  chatPin();
  if (FOL && THR && document.documentElement.dataset.tab === 'chat') chatBottom();
}
// Each block's time in the phone's clock; picked files wait above the box as a row of thumbnails, each with [×].
// (A live swap paints its new copy before the patch, so nothing flashes.) #403: a grown box keeps its height, a
// working thread's mark is the loading overlay's art (#ld: the theme's sprite, ascii frames or the glyph), and each
// message sent this session shows: `sending…`, then ✓ on the thread's copy; failed, red with [retry].
const SENT = [];
function chatPaint(root = $('#chat')) {
  if (!root || ATS === undefined) return; // before the `var` line runs
  if (root !== $('#chat')) chatKeep(root);
  clocks(root);
  root.querySelectorAll('.ats, .qe, .qh').forEach(e => e.remove());
  root.querySelectorAll('.fold').forEach(d => { d.open = FOLD.has(d.dataset.n); });
  for (const tw of root.querySelectorAll('.tw')) {
    const t = tw.dataset.t, a = tw.elements.text, wk = tw.closest('.cvp').querySelector('.tlv.wk');
    // #407: while a turn runs [send] is [queue] (a long press offers [steer]); editing a queued message says so.
    (tw.querySelector('.snd') || {}).textContent = wk && !(EDIT && EDIT.t === t) ? 'queue' : 'send';
    if (EDIT && EDIT.t === t) tw.insertBefore(el('p', 'qe', 'editing queued message · '), tw.querySelector('input[type=file]')).append(Object.assign(el('button', 'ecl', 'cancel'), { type: 'button' }));
    else if (wk) tw.append(el('p', 'qh', 'delivered when this turn ends · hold [queue] to steer'));
    if (!a.value && !(EDIT && EDIT.t === t)) a.value = get('draft:' + t) || '';
    if (HP[t] && tw.elements.harness) tw.elements.harness.value = HP[t]; // #404: the harness picked for a new thread
    if (!ATS[t]) continue;
    const row = tw.insertBefore(el('div', 'ats'), tw.querySelector('input[type=file]'));
    ATS[t].forEach((f, i) => {
      const im = f.src ? Object.assign(document.createElement('img'), { src: f.src, alt: f.name }) : el('span', '', f.name);
      row.appendChild(el('div', 'att')).append(im, Object.assign(el('button', '', '×'), { type: 'button', ariaLabel: 'remove ' + f.name, value: i }));
    });
  }
  root.querySelectorAll('.tw textarea').forEach(a => { a.style.height = $(`#chat .tw[data-t="${CSS.escape(a.form.dataset.t)}"] textarea`)?.style.height || ''; if (a.isConnected && a.value && !a.style.height) grow(a); });
  pill(root);
  root.querySelectorAll('.tld').forEach(i => { i.innerHTML = $('#ld').innerHTML; });
  root.querySelectorAll('.snt').forEach(e => e.remove());
  for (const p of SENT) {
    const tc = root.querySelector(`.cvp[data-t="${CSS.escape(p.t)}"] .tc`), mine = tc ? tc.querySelectorAll('.blk.me:not(.snt)') : [];
    if (mine.length > p.n) { mine[p.n].querySelector('summary .ct').before(el('b', 'snt sk', '✓')); continue; }
    if (!tc) continue;
    const b = tc.appendChild(el('details', 'blk me snt' + (p.st === '✗' ? ' bad' : '')));
    b.dataset.p = SENT.indexOf(p);
    b.open = true;
    b.innerHTML = '<summary><span class=k>you</span><span class=pv></span><span class=ct></span><i class=cv></i></summary><div class=md><p></p></div>';
    b.querySelector('.pv').textContent = p.text.split('\n')[0];
    b.querySelector('.ct').textContent = p.st === '✗' ? '✗ not sent' : p.st;
    b.querySelector('.md p').textContent = p.text;
    if (p.st === '✗') b.querySelector('.md').append(Object.assign(el('button', 'rt', 'retry'), { type: 'button' }));
  }
}
function clocks(root) { root.querySelectorAll('.ct[data-ts]').forEach(c => { c.textContent = new Date(c.dataset.ts * 1e3).toTimeString().slice(0, 5); }); }
// #408: a thread opens with its last 30 messages; near the top a scroll (or a tap on the marker) loads the 30 before
// them (API v1 `thread/<t>?before=`, the TypeScript board draws them) above, the reading position kept. A live copy's marker becomes the live thread's blocks
// older than the copy's first (`data-n`), so a swap never drops the pages loaded.
function chatKeep(root) {
  root.querySelectorAll('.cvp .tc>.tmo').forEach(m => {
    const tc = $(`#chat .cvp[data-t="${CSS.escape(m.closest('.cvp').dataset.t)}"] .tc`);
    // #460: a block the copy carries anyway (a waiting config card kept under its marker) is not taken twice.
    const nu = m.closest('.tc'), old = tc ? [...tc.children].filter(c => +c.dataset.n < +m.dataset.b && !nu.querySelector(`:scope>[data-n="${c.dataset.n}"]`)) : [];
    if (old.length) m.replaceWith(tc.firstElementChild.cloneNode(true), ...old.map(c => c.cloneNode(true)));
  });
}
var OLDQ = false;
async function chatOlder() {
  const m = THR && $(`#chat .cvp[data-t="${CSS.escape(THR)}"] .tc>.tmo`);
  if (!m || OLDQ) return;
  OLDQ = true;
  FOL = false; // #407: up there reading older ones: the end is not followed
  const h = await window.threadOlder?.(THR, m.dataset.b), a = m.nextElementSibling, y = a && a.getBoundingClientRect().top;
  OLDQ = false;
  if (!h || !m.isConnected) return;
  const t = el('div', '');
  t.innerHTML = h;
  clocks(t);
  m.replaceWith(...t.childNodes);
  // (#443: on a phone the messages scroll by themselves, the page never)
  const c = innerWidth < 1000 ? a && a.parentElement : null;
  if (a) c ? c.scrollTop += a.getBoundingClientRect().top - y : scrollBy(0, a.getBoundingClientRect().top - y);
}
addEventListener('scroll', e => {
  const c = e.target;
  if (document.documentElement.dataset.tab === 'chat' && (c.matches ? c.matches('#chat .tc') && c.scrollTop < c.clientHeight : scrollY < innerHeight)) chatOlder();
}, { passive: true, capture: true });
// #403: the message box grows with its text to 6 lines (then scrolls inside, serve.css) and shrinks back after a send.
const grow = a => { a.style.height = ''; if (a.value) a.style.height = a.scrollHeight + 2 + 'px'; };
// #407: each thread's draft is kept as you type (not a queued message being edited).
// #404: an empty thread's harness pick (HP, kept across live updates until its first message); a thread's model/effort
// chip applies from its next turn.
document.addEventListener('change', e => {
  const s = e.target, f = s.closest?.('#chat .tw');
  if (!f || !s.matches('select')) return;
  if (s.name === 'harness') HP[f.dataset.t] = s.value;
  else chatPost({ op: 'model', t: f.dataset.t, [s.name]: s.value });
});
const draft = (t, v) => { try { if (v) localStorage.setItem('draft:' + t, v); else localStorage.removeItem('draft:' + t); } catch (e) {} };
document.addEventListener('input', e => {
  const a = e.target;
  if (!a.matches('#chat .tw textarea')) return;
  grow(a);
  if (!(EDIT && EDIT.t === a.form.dataset.t)) draft(a.form.dataset.t, a.value);
});
// #407: the picked files of each thread in IndexedDB (their bytes, which every WebKit keeps), read back once at load.
const IDB = new Promise(ok => { try { const q = indexedDB.open('herdsman', 1); q.onupgradeneeded = () => q.result.createObjectStore('att'); q.onsuccess = () => ok(q.result); q.onerror = () => ok(null); } catch (e) { ok(null); } });
const idb = f => IDB.then(d => d && new Promise(ok => { const q = f(d.transaction('att', 'readwrite').objectStore('att')); q.onsuccess = () => ok(q.result); q.onerror = () => ok(null); })).catch(() => null);
const attSave = async t => { const l = ATS[t] && await Promise.all(ATS[t].map(async f => ({ name: f.name, type: f.file.type, data: await f.file.arrayBuffer() }))); return idb(o => l ? o.put(l, t) : o.delete(t)); };
var ATL = Promise.all([idb(o => o.getAllKeys()), idb(o => o.getAll())]).then(([ks, vs]) => {
  (ks || []).forEach((t, i) => { ATS[t] ??= vs[i].map(({ name, type, data }) => { const file = new File([data], name, { type }); return { name, file, src: type.startsWith('image/') ? URL.createObjectURL(file) : '' }; }); });
  chatPaint();
});
// #407: the open thread's pill floats above its box: `working 1m 23s` with the theme's loading art and [stop], then
// `stopping…` (STOP: the turn a stop or steer was tapped in); `reconnecting…` / `offline · tap to retry` while the
// live stream is down. Idle and connected, none.
// EDIT: the queued message being edited { t, q, draft } (the draft comes back after).
const STOP = new Map(), FOLD = new Set();
let EDIT = null;
const took = s => s < 60 ? `${s}s` : s < 3600 ? `${s / 60 | 0}m ${s % 60}s` : `${s / 3600 | 0}h ${(s % 3600) / 60 | 0}m`;
function pill(root = $('#chat')) {
  root.querySelectorAll('.pill').forEach(p => p.remove());
  const v = THR && root.querySelector(`.cvp[data-t="${CSS.escape(THR)}"]`), tw = v && v.querySelector('.tw'), wk = v && v.querySelector('.tlv.wk');
  if (!tw) return;
  // (The demo has no stream: off there, as `data-demo` says; the audit takes it off to show these.)
  const gone = !B.dataset.demo && !live() && down && Date.now() - down > 1500, off = gone && (!navigator.onLine || Date.now() - down > 30000);
  if (!wk) STOP.delete(THR);
  if (!gone && !wk) return;
  const p = tw.insertBefore(el('div', 'pill'), tw.firstChild);
  if (off) p.append(Object.assign(el('button', 'rtr', 'offline · tap to retry'), { type: 'button' }));
  else if (gone) p.textContent = 'reconnecting…';
  else if (STOP.get(THR) === wk.dataset.since) p.textContent = 'stopping…';
  else {
    p.append(el('i', 'tld'), el('span', '', 'working ' + took(Math.max(0, Math.round(Date.now() / 1e3 - wk.dataset.since)))));
    p.firstChild.innerHTML = $('#ld').innerHTML;
    p.append(Object.assign(el('button', 'stp', 'stop'), { type: 'button', disabled: !!B.dataset.ro }));
  }
}
setInterval(() => { if (document.documentElement.dataset.tab === 'chat' && THR) pill(); }, 1000);
chatOpen(THR);
chatPaint();
document.addEventListener('click', async e => {
  const t = e.target, row = t.closest('#chat .tr'), c = t.closest('#chat .chq button[name=choice]');
  const lk = t.closest('#chat .lc'); // #413: a linked row's chip opens the other thread
  if (lk) { e.preventDefault(); chatOpen(lk.dataset.go); history.pushState({ thr: lk.dataset.go }, ''); }
  else if (row) { e.preventDefault(); chatOpen(row.dataset.t); history.pushState({ thr: row.dataset.t }, ''); }
  else if (t.closest('#chat .bk')) chatBack();
  else if (t.closest('#chat .tmo')) chatOlder();
  else if (t.closest('#chat .th .cw')) { FOL = false; t.closest('.cvp').querySelector('.tc .chc')?.closest('.blk').scrollIntoView({ block: 'center' }); } // #460: to the waiting card
  else if (t.closest('#chat .snt .rt')) { const v = t.closest('.cvp'); SENT.splice(SENT.findIndex(p => p.st === '✗' && p.t === v.dataset.t), 1); v.querySelector('.tw').requestSubmit(); }
  else if (t.closest('#chat .rty')) chatPost({ op: 'retry', t: t.closest('.cvp').dataset.t, n: t.closest('.rty').dataset.n });
  else if (t.closest('#chat .ntb')) { const n = 'new-' + t.closest('.ntb').dataset.repo; chatOpen(n); history.pushState({ thr: n }, ''); $(`#chat .tw[data-t="${CSS.escape(n)}"] textarea`)?.focus(); }
  else if (t.closest('#chat .sb')) { const b = t.closest('.sb'); chatPost({ op: b.name, t: b.dataset.t }).then(ok => { if (ok && b.name === 'archive') undo(b.dataset.t); }); }
  else if (t.closest('#chat .tq button')) { // #407: a queued message's [send now] (steered in), edit, [remove]
    const b = t.closest('button'), q = b.closest('.tq').dataset.q, f = b.closest('.tw'), box = f.elements.text;
    if (b.name === 'qe') { EDIT = { t: f.dataset.t, q, draft: box.value }; box.value = b.closest('.tq').querySelector('p').textContent; grow(box); chatPaint(); box.focus(); }
    else chatPost({ op: b.name === 'qn' ? 'steer' : 'unqueue', t: f.dataset.t, q });
  }
  else if (t.closest('#chat .ecl')) { const box = t.closest('.tw').elements.text; box.value = EDIT.draft; EDIT = null; grow(box); chatPaint(); }
  else if (t.closest('#chat .pill .stp')) { STOP.set(THR, $(`#chat .cvp[data-t="${CSS.escape(THR)}"] .tlv.wk`).dataset.since); pill(); chatPost({ op: 'stop', t: THR }); }
  else if (t.closest('#chat .pill .rtr')) rf.onclick();
  else if (t.closest('#chat .add')) t.closest('.tw').querySelector('input[type=file]').click();
  else if (t.closest('#chat .att button')) { const k = t.closest('.tw').dataset.t; ATS[k].splice(+t.closest('button').value, 1); if (!ATS[k].length) delete ATS[k]; attSave(k); chatPaint(); }
  else if (c) { const q = c.closest('.chq'); chatPost({ op: 'answer', fleet: q.dataset.fleet, id: q.dataset.id, choice: c.value }); }
  else if (t.closest('#chat .cpy')) copyBox(t.closest('.cpy'));
  else if (t.closest('#chat .im img')) iv([...t.closest('.blk').querySelectorAll('.im img')].map(i => [i.src, i.alt]), t.src);
});
// #407: a finished turn's fold keeps its open state (FOLD, by its first command) across live updates.
document.addEventListener('toggle', e => { const d = e.target; if (d.matches?.('#chat .fold')) FOLD[d.open ? 'add' : 'delete'](d.dataset.n); }, true);
// #407: after a swipe's [archive], `archived · [undo]` for 5 s; [undo] restores it.
function undo(t) {
  toast('archived · ', 5000).append(Object.assign(el('button', 'und', 'undo'), { type: 'button', onclick: () => chatPost({ op: 'restore', t }) }));
}
// #407: a long press (½ s, the finger still) on [queue] offers [queue] or [steer], on a message its menu: [copy],
// [copy as markdown], [quote] (into the box), then `copied`. The click it ends does nothing else.
const MM = B.appendChild(Object.assign(document.createElement('div'), { id: 'mm', hidden: true }));
let lp = null, lpd = false;
function hold(x, y) {
  const v = x.closest('.cvp');
  if (x.matches('.snd')) {
    if (!v.querySelector('.tlv.wk') || x.disabled) return false;
    MM.innerHTML = '<button type=button name=choice value=queue><span>queue <small>· delivered when this turn ends</small></span></button><button type=button name=choice value=steer><span>steer <small>· stops the turn and sends it now</small></span></button>';
  } else MM.innerHTML = '<button type=button value=copy>copy</button><button type=button value=md>copy as markdown</button><button type=button value=quote>quote</button>';
  MM.on = x;
  MM.hidden = false;
  MM.style.top = Math.max(0, Math.min(y - MM.offsetHeight - 16, innerHeight - MM.offsetHeight)) + 'px';
  return true;
}
document.addEventListener('pointerdown', e => {
  const x = e.target.closest('#chat .tw .snd, #chat .tc .blk:is(.me,.ai)');
  if (!MM.hidden && !MM.contains(e.target)) MM.hidden = true;
  clearTimeout(lp?.t);
  lpd = false;
  lp = x && { x: e.clientX, y: e.clientY, t: setTimeout(() => { lp = null; lpd = hold(x, e.clientY); }, 500) };
});
document.addEventListener('pointermove', e => { if (lp && Math.hypot(e.clientX - lp.x, e.clientY - lp.y) > 10) { clearTimeout(lp.t); lp = null; } });
for (const k of ['pointerup', 'pointercancel']) document.addEventListener(k, () => { if (lp) { clearTimeout(lp.t); lp = null; } });
document.addEventListener('click', e => { if (lpd) { lpd = false; e.preventDefault(); e.stopPropagation(); } }, true);
document.addEventListener('contextmenu', e => { const b = e.target.closest('#chat .tc .blk:is(.me,.ai)'); if (b && hold(b, e.clientY)) e.preventDefault(); });
MM.addEventListener('click', async e => {
  const b = e.target.closest('button'), x = MM.on;
  if (!b) return;
  if (b.name === 'choice') { MM.hidden = true; return chatSay(x.form, b.value); }
  const md = x.querySelector('.md'), src = md.dataset.md ?? md.textContent, box = x.closest('.cvp').querySelector('.tw textarea');
  const text = b.value === 'md' ? src : src.replace(/^\s*```.*\n?/gm, '').replace(/`/g, '').trim();
  if (b.value === 'quote') {
    box.value = text.split('\n').map(l => '> ' + l).join('\n') + '\n\n' + box.value;
    grow(box);
    draft(box.form.dataset.t, box.value);
    MM.hidden = true;
    return box.focus();
  }
  MM.textContent = await navigator.clipboard.writeText(text).then(() => 'copied', () => 'not copied: no clipboard here');
  setTimeout(() => { MM.hidden = true; }, 1000);
});
// #405: a thread row swiped left shows its [archive] (one row at a time); swiped right, it hides again. The click that
// ends a swipe opens nothing.
let rsw = null, rswiped = false;
document.addEventListener('pointerdown', e => { const r = e.target.closest('#chat .sw>.tr'); rsw = r && { r, x: e.clientX, y: e.clientY }; rswiped = false; });
document.addEventListener('pointerup', e => {
  if (!rsw) return;
  const dx = e.clientX - rsw.x, dy = e.clientY - rsw.y, sw = rsw.r.parentNode;
  rsw = null;
  if (Math.abs(dx) < 40 || Math.abs(dx) < 2 * Math.abs(dy)) return;
  rswiped = true;
  $$('#chat .sw.op').forEach(o => { if (o !== sw) o.classList.remove('op'); });
  sw.classList.toggle('op', dx < 0);
});
document.addEventListener('click', e => { if (rswiped && e.target.closest('#chat .sw>.tr')) { rswiped = false; e.preventDefault(); e.stopPropagation(); } }, true);
// #406: an image tap opens the viewer over the page (the URL stays): black, the image fitted; pinch or double-tap to
// zoom, drag to pan; swipe down or [×] to close; left/right through the message's images; [save] shares or downloads.
const IV = B.appendChild(Object.assign(document.createElement('div'), { id: 'iv', hidden: true }));
IV.innerHTML = '<img alt=""><div class=ivb><button type=button class=ivx aria-label=close>×</button><span class=ivn></span><button type=button class=ivs>save</button></div>';
const IVI = IV.querySelector('img'), PT = new Map();
let ivl = [], ivn = 0, ivz = { s: 1, x: 0, y: 0 }, ivg = null, ivt = -1e4;
const ivDraw = () => { IVI.style.transform = `translate(${ivz.x}px,${ivz.y}px) scale(${ivz.s})`; };
function ivShow(i) {
  ivn = Math.max(0, Math.min(ivl.length - 1, i));
  [IVI.src, IVI.alt] = ivl[ivn];
  IV.querySelector('.ivn').textContent = ivl.length > 1 ? `${ivn + 1} / ${ivl.length}` : '';
  ivz = { s: 1, x: 0, y: 0 };
  ivDraw();
}
function iv(list, src) { ivl = list; ivShow(list.findIndex(l => l[0] === src)); IV.hidden = false; }
// Pointer positions from the viewer's centre (the image's transform origin); their mean, their spread.
const ivAt = ps => { const r = IV.getBoundingClientRect(); return [ps.reduce((a, p) => a + p[0], 0) / ps.length - r.width / 2, ps.reduce((a, p) => a + p[1], 0) / ps.length - r.height / 2]; };
const ivSpan = ps => Math.hypot(ps[0][0] - ps[1][0], ps[0][1] - ps[1][1]);
// A double tap zooms 2.5× on that point, or back to fitted.
function ivTap(x, y) {
  const [px, py] = ivAt([[x, y]]), s = ivz.s > 1 ? 1 : 2.5;
  ivz = s > 1 ? { s, x: px * (1 - s), y: py * (1 - s) } : { s: 1, x: 0, y: 0 };
  ivDraw();
}
IV.addEventListener('click', async e => {
  if (e.target.closest('.ivx')) IV.hidden = true;
  if (!e.target.closest('.ivs')) return;
  const [src, name] = ivl[ivn], b = await fetch(src).then(r => r.blob()).catch(() => null);
  if (!b) return toast('error: could not read ' + name);
  const f = new File([b], name, { type: b.type });
  if (navigator.canShare?.({ files: [f] })) return navigator.share({ files: [f] }).catch(() => {});
  const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(b), download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1e3);
});
IV.addEventListener('pointerdown', e => {
  if (e.target.closest('button')) return;
  PT.set(e.pointerId, [e.clientX, e.clientY]);
  ivg = { z: ivz, p: [...PT.values()], moved: PT.size > 1 };
  IV.classList.add('dr');
});
IV.addEventListener('pointermove', e => {
  if (!PT.has(e.pointerId)) return;
  PT.set(e.pointerId, [e.clientX, e.clientY]);
  const p = [...PT.values()], [x0, y0] = ivAt(ivg.p), [x, y] = ivAt(p), dx = x - x0, dy = y - y0;
  if (p.length > 1 && ivg.p.length > 1) {
    const s = Math.min(6, Math.max(1, ivg.z.s * ivSpan(p) / ivSpan(ivg.p))), k = s / ivg.z.s;
    ivz = { s, x: x - (x0 - ivg.z.x) * k, y: y - (y0 - ivg.z.y) * k };
  } else if (ivg.z.s > 1) ivz = { s: ivg.z.s, x: ivg.z.x + dx, y: ivg.z.y + dy };
  else ivz = Math.abs(dy) > Math.abs(dx) ? { s: 1, x: 0, y: Math.max(0, dy) } : { s: 1, x: dx, y: 0 };
  ivg.moved ||= Math.hypot(dx, dy) > 10;
  ivDraw();
});
const ivUp = e => {
  if (!PT.delete(e.pointerId)) return;
  if (PT.size) return void (ivg = { z: ivz, p: [...PT.values()], moved: true }); // one finger of a pinch lifted: it pans on
  IV.classList.remove('dr');
  if (ivz.s === 1) { // fitted again: a swipe from fitted closes or turns; any other ends centred (a pinch back out too)
    if (ivg.z.s === 1 && ivz.y > 80) return void (IV.hidden = true);
    if (ivg.z.s === 1 && Math.abs(ivz.x) > 60) return ivShow(ivn + (ivz.x < 0 ? 1 : -1));
    ivz = { s: 1, x: 0, y: 0 };
    ivDraw();
  }
  if (ivg.moved) return;
  const now = performance.now();
  if (now - ivt < 300) { ivt = -1e4; ivTap(e.clientX, e.clientY); } else ivt = now;
};
IV.addEventListener('pointerup', ivUp);
IV.addEventListener('pointercancel', ivUp);
document.addEventListener('keydown', e => { if (e.key === 'Escape') IV.hidden = true; });
// #405: the picker multi-selects and [+] adds more, up to 10 a message (the rest are dropped with a toast).
document.addEventListener('change', e => {
  const i = e.target;
  if (!i.matches('#chat .tw input[type=file]') || !i.files.length) return;
  const t = i.closest('.tw').dataset.t, fs = [...i.files];
  i.value = '';
  const l = ATS[t] ??= [], room = 10 - l.length;
  if (fs.length > room) toast(`at most 10 files a message: ${fs.length - room} left out`);
  l.push(...fs.slice(0, room).map(f => ({ name: f.name, file: f, src: f.type.startsWith('image/') ? URL.createObjectURL(f) : '' })));
  attSave(t);
  chatPaint();
});
// #409: a code box's [copy]: its exact text to the clipboard, then `copied` for 1.5 s; without the clipboard API
// (plain http) the text is selected instead.
function copyBox(b) {
  const p = b.previousElementSibling, pick = () => getSelection().selectAllChildren(p);
  if (!navigator.clipboard) return pick();
  navigator.clipboard.writeText(p.textContent).then(() => { b.textContent = 'copied'; setTimeout(() => { b.textContent = 'copy'; }, 1500); }, pick);
}
// A file as base64, read when its message is sent.
const b64 = f => new Promise(ok => { const r = new FileReader(); r.onload = () => ok(r.result.split(',')[1]); r.readAsDataURL(f); });
async function chatPost(d, f) {
  try {
    const r = await send('thread', d), m = (await r.text()).trim();
    if (r.status === 401) unpaired();
    if (m && !(r.ok && ['say', 'new', 'archive', 'queue', 'steer', 'stop', 'unqueue'].includes(d.op))) toast(m);
    if (r.ok) { if (f) f.reset(); refresh(true); }
    return r.ok && (m || true);
  } catch (err) { toast('error: ' + err.message); }
}
// #403: [send] clears the box at once and shows the message `sending…`; ✓ once the server logged it; failed, red with
// [retry], the text (and file) back in the box.
// #405: in an empty thread (`new-<repo>`, from [+ thread]) the first message starts it: `new`, then its thread opens.
// #407: while a turn runs it is queued (`how`: the long press's queue or steer; an edit of a queued message replaces
// it). Every send follows the thread again and pins the message near the top; a failed one keeps the draft.
async function chatSay(f, how) {
  const t = f.dataset.t, a = ATS[t] || null, box = f.elements.text, text = box.value, repo = f.dataset.repo, ed = EDIT && EDIT.t === t ? EDIT : null;
  if (!text.trim() && !a) return;
  const op = repo ? 'new' : ed ? 'queue' : how || (f.closest('.cvp').querySelector('.tlv.wk') ? 'queue' : 'say');
  const names = a ? a.map(x => x.name).join(', ') : '', harness = repo && f.elements.harness ? { harness: f.elements.harness.value } : {};
  const p = { t, text: text.trim() ? text : names, st: 'sending…', n: f.closest('.cvp').querySelectorAll('.tc .blk.me:not(.snt)').length };
  if (op !== 'queue') SENT.push(p);
  f.reset();
  if (ed) { box.value = ed.draft; EDIT = null; }
  draft(t, box.value);
  grow(box);
  if (a) { delete ATS[t]; attSave(t); }
  FOL = true;
  if (op !== 'queue') PIN = p;
  chatPaint();
  chatFollow();
  const files = a ? { name: a.map(x => x.name.replaceAll('/', '_')).join('/'), data: (await Promise.all(a.map(x => b64(x.file)))).join(',') } : {};
  const ok = await chatPost({ op, ...(repo ? { repo, ...harness } : { t }), ...(ed ? { q: ed.q } : {}), text, ...files });
  if (ok && repo) delete HP[t];
  if (ok && repo) { Object.assign(p, { t: ok.split(' ')[0], n: 0 }); history.replaceState({ thr: p.t }, ''); chatOpen(p.t); }
  p.st = ok ? '✓' : '✗';
  if (!ok && ed) EDIT = { ...ed, draft: box.value };
  if (!ok && (ed || !box.value)) { box.value = text; grow(box); if (!ed) draft(t, text); }
  if (!ok && a && !ATS[t]) { ATS[t] = a; attSave(t); }
  chatPaint();
  chatFollow();
}
document.addEventListener('submit', async e => {
  const f = e.target;
  if (f.matches('#chat .chq')) {
    e.preventDefault();
    chatPost({ op: 'answer', fleet: f.dataset.fleet, id: f.dataset.id, text: f.elements.text.value }, f);
  } else if (f.matches('#chat .chc')) { // #387: a config card's [apply] / [discard]
    e.preventDefault();
    chatPost({ op: 'config', id: f.dataset.id, act: e.submitter.value });
  } else if (f.matches('#chat .chs')) { // #387: a secret card's [save] / [dismiss]; the value only goes in the POST
    e.preventDefault();
    const act = e.submitter.value;
    chatPost({ op: 'secret', t: f.dataset.t, name: f.dataset.name, act, ...(act === 'save' ? { value: f.elements.value.value } : {}) }, f);
  } else if (f.matches('#chat .tw')) {
    e.preventDefault();
    chatSay(f);
  }
});
// #403: on a phone a swipe right from the left edge (iOS style) takes a thread back to the list: it follows the finger,
// past a third of the width it goes, else it snaps back. Only from the edge, and only sideways: a diff's scroll stays.
let esw = null;
addEventListener('touchstart', e => {
  const v = THR && document.documentElement.dataset.tab === 'chat' && $(`#chat .cvp[data-t="${CSS.escape(THR)}"]`), x = e.touches[0];
  esw = v && innerWidth < 1000 && !shown() && x.clientX < 24 ? { v, x: x.clientX, y: x.clientY, on: false } : null;
  if (esw) v.style.transition = 'none';
}, { passive: true });
addEventListener('touchmove', e => {
  if (!esw) return;
  const dx = e.touches[0].clientX - esw.x, dy = e.touches[0].clientY - esw.y;
  if (!esw.on && Math.hypot(dx, dy) > 10) { if (dx < 2 * Math.abs(dy)) return void (esw = null); esw.on = true; }
  if (esw.on) esw.v.style.transform = `translateX(${Math.max(0, dx)}px)`;
}, { passive: true });
for (const k of ['touchend', 'touchcancel']) addEventListener(k, e => {
  if (!esw) return;
  const { v, x, on } = esw, back = on && k === 'touchend' && e.changedTouches[0].clientX - x > innerWidth / 3;
  esw = null;
  v.style.transition = back ? 'none' : '';
  v.style.transform = '';
  if (back) chatBack();
});
// #440: [⚙] (every tab, in [◐]'s place) opens the System sheet: one scroller, a section index on top (a rail at ≥ 900
// px), the sections in the owner's order. Hosts & health and Activity are the old Host and Events tabs' sections (same
// ids: the TypeScript board, `/live` and frag() paint them here); Agent providers, Fleets, Notifications and Conductor
// are the board's (web/src/system.ts), Appearance the look sheet's. Its own layer: a lane or setup sheet opens over it.
const SECS = [['providers', 'agent providers', 'providers'], ['fleets', 'fleets'], ['hosts', 'hosts & health', 'hosts', 'tab-host'],
  ['activity', 'activity', 'activity', 'tab-events'], ['notifications', 'notifications & devices', 'alerts'], ['conductor', 'conductor'], ['appearance', 'appearance', 'look']];
const syh = (n, w) => `<h2 class=syh><span class=syt data-p="${n}"${w ? ` data-w=${w}` : ''}>${n}</span><small class=sym></small></h2>`;
B.insertAdjacentHTML('beforeend', `<div id=sys hidden><div class=sbd></div><div class="sh lg sys full" role=dialog aria-modal=true aria-labelledby=syst>
<div class=hd><p><b id=syst>system</b><button type=button class=sxc aria-label="close system">close</button></p></div>
<div class=sxi role=navigation aria-label="system sections">${SECS.map(([id, , c]) => `<a href=#system/${id} data-s=${id}><span>${c || id}</span><i class=sd hidden></i></a>`).join('')}</div>
<div class=top>${SECS.map(([id, n, , w]) => `<section class=sy id=sy-${id} data-s=${id}>${id === 'hosts' ? syh(n, w) + '<section id=host></section>' : id === 'activity' ? syh(n, w) + '<section id=events></section>' : id === 'appearance' ? syh(n) + '<div class=look></div>' : ''}</section>`).join('')}</div></div></div>`);
// `var`: connect() and frag() may ask before this line runs.
var SY = $('#sys'), ytop = SY.querySelector('.top'), sysb = $('#sysb');
function sysOn() { return !!SY && !SY.hidden; }
var sysBack = 'queue', sysWant = '', sysPin = '';
// Open at a section (Fleets: and that fleet's row, open); the hash says `#system/<section>[/<fleet>]`.
function sysopen(sec = 'providers', sub = '', smooth = false) {
  if (!SECS.some(s => s[0] === sec)) sec = 'providers';
  if (!sysOn()) {
    sysBack = document.documentElement.dataset.tab;
    SY.hidden = false;
    document.documentElement.classList.add('sysopen');
    if (es) connect(); // the stream names `host` while System is open (#352: process sampling on)
  }
  lsheet();
  sysWant = sec === 'fleets' ? sub || '' : '';
  sysPin = smooth ? '' : sec;
  sysgo(sec, smooth);
  history.replaceState(null, '', `#system/${sec}${sysWant ? '/' + sysWant : ''}`);
}
function sysgo(sec, smooth) {
  const d = sysWant && [...$$('.sfl')].find(x => x.dataset.f === sysWant);
  if (d) d.open = true;
  const t = d || $('#sy-' + sec);
  ytop.scrollTo({ top: sec === 'providers' && !d ? 0 : t.offsetTop, behavior: smooth ? 'smooth' : 'instant' });
  spy();
}
function sysshut() {
  if (!sysOn()) return;
  SY.hidden = true;
  document.documentElement.classList.remove('sysopen');
  history.replaceState(null, '', '#' + sysBack);
  sysb.focus({ preventScroll: true });
  if (es) connect();
}
// The index follows the scroll: the section at the top is filled (the last one at the end) and kept in view.
function spy() {
  const y = ytop.scrollTop + 8, all = [...$$('.sy')], end = ytop.scrollTop + ytop.clientHeight >= ytop.scrollHeight - 2;
  const cur = end ? all[all.length - 1] : all.filter(s => s.offsetTop <= y).pop() || all[0];
  $$('.sxi a').forEach(a => {
    const on = a.dataset.s === cur.dataset.s, n = a.parentNode, l = a.offsetLeft - n.offsetLeft;
    a.classList.toggle('on', on);
    if (on) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current');
    if (on && (l < n.scrollLeft || l + a.offsetWidth > n.scrollLeft + n.clientWidth)) n.scrollLeft = l - 16;
  });
}
ytop.addEventListener('scroll', () => requestAnimationFrame(spy), { passive: true });
for (const k of ['pointerdown', 'wheel', 'keydown']) ytop.addEventListener(k, () => { sysPin = sysWant = ''; }, { passive: true });
// Fleets as the board drew it: each pipeline graph, the pickers off in a demo or offline, the row a link named opened.
function sysfleets(root) {
  root.querySelectorAll('[data-pipe]').forEach(pipeline);
  root.querySelectorAll('form button, form select:not([data-off])').forEach(c => c.disabled = DEMO || dot.className === 'r');
  if (sysWant && sysOn()) sysgo('fleets');
}
// [⚙]'s dot and the [hosts] chip's: a host card is red (past a limit, under a floor, or down); the Hosts head says why.
function alarm() {
  const r = [...$$('#host .hcd')].filter(c => c.dataset.c === 'r');
  const why = r.length ? `${r.map(c => c.dataset.h).join(', ')}: ${(r[0].querySelector('.hw')?.textContent || 'alert').split(' · ').pop()}` : '';
  $$('#sysb .sd, .sxi a[data-s=hosts] .sd').forEach(d => { d.hidden = !r.length; });
  sysb.setAttribute('aria-label', r.length ? `system (host alert: ${why})` : 'system');
  sysb.title = r.length ? `host alert · ${why}` : 'system';
  $('#sy-hosts').dataset.t = r.length ? 'r' : '';
  $('#sy-hosts .sym').textContent = r.length ? `alert · ${why}` : `${$$('#host .hcd').length} hosts · all clear`;
}
// The old tabs' theme words name their sections now: `barn · hosts & health`, `logbook · activity`.
function syswords() { $$('#sys .syt[data-w]').forEach(p => { const h = tw(p.dataset.w, p.dataset.p); if (p.innerHTML !== h) p.innerHTML = h; }); }
// Routes: `#system/<section>[/<fleet>]` opens there; the old `#host`, `#events`, `#config[/<fleet>]` land on the Queue
// tab with the sheet at Hosts & health, Activity, Fleets (the head script already took a stored `host`/`events` to Queue).
const OLD = { host: 'hosts', events: 'activity', config: 'fleets' };
function sysroute() {
  const [h, sec, sub] = decodeURIComponent(location.hash.slice(1)).split('/');
  if (h === 'system') return sysopen(sec, sub);
  if (OLD[h]) { tab('queue'); sysopen(OLD[h], sec); }
}
addEventListener('hashchange', sysroute);
document.addEventListener('click', e => {
  const t = e.target;
  if (t.closest('#sysb')) return sysopen();
  if (t.closest('.sxc, .sbd')) return sysshut();
  const a = t.closest('.sxi a, #sys a.go');
  if (a) { e.preventDefault(); const [, s, f] = a.getAttribute('href').slice(1).split('/'); sysopen(s, f && decodeURIComponent(f), true); }
});
addEventListener('keydown', e => { if (e.key === 'Escape' && sysOn() && !shown()) sysshut(); });
sysroute();
