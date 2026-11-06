// #37 Herd mode: Corral's Herd screen (jirathip-dev/corral ios/FleetNotifier/UI/Herd: RanchEnvironment,
// RanchWind, HerdArt; MIT © jirathip-dev). One full-screen page per fleet (swipe ←→, Others last), the
// ranch strip on top, the pasture below in bands; queued issues wait at the gate above them. Data: `/herd.json`, then the
// `/live` stream's `herd` event (polled by refresh() while it is down). Host load is the weather.
// One requestAnimationFrame loop on one clock drives every motion, current page only, paused when hidden.
const NS = 'http://www.w3.org/2000/svg', M64 = (1n << 64n) - 1n;
// RanchPainter.noise: indexed, never random per frame (Swift's wrapping UInt64 math, in BigInt).
const noise = (i, s = 0) => { const n = (BigInt(i) * 1664525n + BigInt(s) * 1013904223n) & M64; return Number(((n ^ (n >> 13n)) * 1274126177n) % 65536n) / 65535; };
const hx = s => String(s ?? '').replace(/[&<>"]/g, c => '&#' + c.charCodeAt(0) + ';');
const f1 = n => +n.toFixed(1);
// RanchWind: one gust phase (period 5.2 s), per-anchor offsets, a coherent grass wave.
const WIND = { period: 5.2, canopy: 2.8, grass: 2.1 };
// #566: `?shot=1` (the audit's screenshots) paints one fixed frame (t = 2 s) and runs no loop: headless Chrome hung on
// the endless storm loop, never reaching the shot.
const SHOT = DBG.has('shot');
const phase = t => t * 2 * Math.PI / WIND.period;
const anchor = x => ((x % 390) / 390) * 2 * Math.PI * 0.7;
const sway = (t, a, amp) => amp * (0.74 * Math.sin(phase(t) + a) + 0.26 * Math.sin(1.93 * phase(t) + a * 1.7));
const bend = (t, x, amp) => amp * (0.8 * Math.sin(phase(t) - x * 0.028) + 0.2 * Math.sin(1.9 * phase(t) - x * 0.011));
const ell = (x, y, rx, ry, fill, op = 1) => `<ellipse cx="${f1(x)}" cy="${f1(y)}" rx="${f1(rx)}" ry="${f1(ry)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
const rect = (x, y, w, h, fill, op = 1) => `<rect x="${f1(x)}" y="${f1(y)}" width="${f1(w)}" height="${f1(h)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;

// The scene (390 × 320 of Corral's 390 × 640 world: sky to the rear fence and its grass). The sky is
// the page's CSS gradient (storm darkens it); clouds are their own blurred layer that only translates.
// #71: every fill is a custom property (herd.css: Corral's color(day, dark) pairs), so `#herd.night` repaints it.
function clouds() {
  let s = '';
  for (const copy of [0, 780]) for (let i = 0; i < 6; i++) {
    const x = i * 130 + 45 + copy, y = 36 + noise(i, 30) * 86;
    for (let j = 0; j < 5; j++) s += ell(x + j * 12, y - noise(j, i) * 9, 18, 5 + noise(j, 2) * 5, 'var(--cl)');
  }
  return `<svg viewBox="0 0 1560 320" preserveAspectRatio=none aria-hidden=true>${s}</svg>`;
}
// #71 night sky (RanchPainter.sky, night): the Milky Way and the seeded stars ride one slow drift (RanchSky.celestialSpeed,
// `.cel`, a 390 tile with its neighbours: a star keeps its identity, only moves), under the hills; the moon stays put.
function night() {
  let s = '';
  for (const k of [-390, 0, 390]) {
    for (let b = 0; b < 6; b++) s += `<path d="M${k - 60} 220Q${k + 60} 74 ${k + 220} 3Q${k + 275} -20 ${k + 335} -32" stroke-width=${66 - b * 8}/>`;
  }
  s = `<g class=mw filter="url(#hmw)">${s}</g>`;
  for (let i = 0; i < 130; i++) for (const k of [-390, 0, 390]) {
    const r = 0.35 + noise(i, 7) * 0.7;
    s += ell(k + i * 3 + noise(i, 8) * 3, noise(i, 3) * 225, r, r, '#e7e5f4', f1((0.3 + 0.45 * noise(i, 9) + 0.08 * Math.sin(i)) * 100) / 100);
  }
  return `<g class=sky-night><g class=cel>${s}</g><g class=moon>${ell(306, 56, 16, 16, '#d3dfeb', 0.84)}${ell(301, 51, 15, 15, '#1d2841')}</g></g>`;
}
function scene() {
  let s = night();
  ['var(--h0)', 'var(--h1)', 'var(--h2)', 'var(--h3)'].forEach((fill, r) => {
    let d = 'M-85 330';
    for (let i = -4; i <= 14; i++) d += `L${i * 35} ${f1(147 + r * 30 + noise(i, r + 17) * 36)}`;
    s += `<path d="${d}L475 330Z" fill="${fill}"/>`;
  });
  s += '<path class=gd d="M-60 258Q165 228 450 268L450 330L-60 330Z"/>';
  for (let i = -1; i <= 2; i++) {
    const x = i * 160;
    s += `<path d="M${x + 130} 258Q${x + 40} 430 ${x + 180} 640L${x + 226} 640Q${x + 62} 438 ${x + 145} 258Z" fill="var(--lw)" opacity=".18"/>`;
    s += `<path d="M${x} 277Q${x - 35} 395 ${x + 130} 520L${x + 55} 566Q${x - 55} 389 ${x - 30} 290Z" fill="var(--sh)" opacity=".12"/>`;
  }
  for (let i = -152; i <= 1650; i++) {
    const y = 270 + noise(i, 19) * 370;
    if (y < 330) s += ell(Math.floor(i / 19) * 5 + noise(i, 8) * 5, y, 0.3 + noise(i, 2) * 0.7, 0.3, 'var(--st)', 0.13);
  }
  // Barn, door, trim and lit window; then the trees (tile ±1 too, where it reaches the frame).
  s += '<path d="M153 243L180 219L210 242L210 279L153 279Z" fill="var(--bn)"/><path d="M148 245L180 215L216 243L209 246L180 224L155 248Z" fill="var(--rf)"/>';
  s += rect(175, 252, 14, 27, 'var(--dr)') + rect(175, 235, 9, 9, 'var(--wn)', 0.8);
  for (const tile of [-1, 0, 1]) for (const [tx, ty, sc] of [[7, 225, 1], [365, 228, 1.1], [238, 258, 0.5], [326, 262, 0.38]]) {
    const x = tile * 390 + tx;
    if (x < -60 || x > 450) continue;
    // The trunk stays put; leaf clusters sway in 4 groups on the shared phase, upper ones further.
    s += rect(x - 2 * sc, ty - 8 * sc, 4 * sc, 45 * sc, 'var(--tk)');
    const g = ['', '', '', ''], lift = [0, 0, 0, 0];
    for (let i = 0; i < 22; i++) {
      const dx = (noise(i, 20) - 0.5) * 48 * sc, dy = (noise(i, 7) - 0.5) * 35 * sc;
      g[i % 4] += ell(x + dx, ty + dy, 9 * sc, 11 * sc, i % 3 === 0 ? 'var(--l1)' : 'var(--l2)', 0.9) + ell(x + dx - 2 * sc, ty + dy - 3 * sc, 5 * sc, 4 * sc, 'var(--lh)', 0.1);
      lift[i % 4] += (1.1 - (dy / (35 * sc)) * 0.35) / 6;
    }
    g.forEach((c, k) => { s += `<g class=cn data-a="${f1(anchor(x) + k * 0.5 + 0.35)}" data-l="${f1(sc * lift[k] * 100) / 100}">${c}</g>`; });
  }
  for (let i = -2; i <= 9; i++) s += rect(i * 52, 249, 6, 44, 'var(--fp)') + rect(i * 52 + 1, 249, 1.4, 44, 'var(--fh)', 0.5);
  for (const dy of [4, 22]) s += rect(-60, 256 + dy, 510, 5, 'var(--fr)') + rect(-60, 256 + dy, 510, 1.3, 'var(--fl)', 0.8);
  for (let i = -5; i <= 48; i++) {
    const x = i * 9, y = 298 + noise(i, 43) * 24;
    let b = '';
    for (let k = 0; k < 3; k++) b += `M${x + 2 * k} ${f1(y)}Q${x + 2 * k - 1} ${f1(y - 5)} ${x + 2 * k - 3} ${f1(y - 8 - noise(i, 4) * 4)}`;
    s += `<path class=gt data-x=${x} data-y="${f1(y)}" d="${b}"/>`;
  }
  // Sun / moon direction: the cast light starts at the sun's side by day, the moon's (mirrored, x 390) at night.
  s += '<rect class=wa width="390" height="320"/>';
  return `<svg class=rs viewBox="0 0 390 320" preserveAspectRatio="xMidYMax slice" aria-hidden=true>${s}</svg>`;
}

// #172/#241 themes (docs/themes.md): the phone's theme (`html[data-theme]`, the `[◐]` picker; the demo also takes `?nopose=<pose>`)
// from `themes/<name>/`, its theme.json resolved by the server (what it lacks is farm's); a sprite id not in its sprites.svg is farm's.
let T = { poses: {}, glyphs: {}, words: {}, scene: {} }, RAW = '', PAL = {};
const w = k => T.words[k] || k;
// #239: heat is indoors (no night, no [A]), as is #423's window light; a lane wears the theme's look by the same hash as
// the Lanes avatars (serve.js).
const indoor = () => ['heat', 'light'].includes(T.load_scene?.kind);
// #292: nh() spreads a page's lanes over the looks (`l.look`), so neighbours don't wear the same hat.
const look = l => { const L = T.looks || []; return L.length ? Object.entries(L[l.look ?? hash(l.name) % L.length]).map(e => e.join(':')).join(';') : l.coat; };
const got = (u, j) => fetch(u).then(r => r.ok ? r[j ? 'json' : 'text']() : Promise.reject()).catch(() => j ? {} : '');
// The sprite symbols theme() builds (poses hp1..hp4 for the Herd, actions ha-*, row sprites hr-*, the rail, moments hm-*,
// the Host load minis hl-*), next to the scene's gradients.
const defs = document.createElementNS(NS, 'svg');
defs.setAttribute('class', 'hdefs');
defs.innerHTML = '<defs><linearGradient id=hgr gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#bcc17b"/><stop offset=".5" stop-color="#8e9c58"/><stop offset=1 stop-color="#485e3b"/></linearGradient>' +
  '<linearGradient id=hgrn gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#63746a"/><stop offset=".5" stop-color="#415d55"/><stop offset=1 stop-color="#273f3e"/></linearGradient>' +
  '<linearGradient id=hwd gradientUnits=userSpaceOnUse x1=0 y1=0 x2=195 y2=500><stop offset=0 stop-color="#ffeeab" stop-opacity=".03"/><stop offset=1 stop-color="#ffeeab" stop-opacity="0"/></linearGradient>' +
  '<linearGradient id=hwn gradientUnits=userSpaceOnUse x1=390 y1=0 x2=195 y2=500><stop offset=0 stop-color="#bbcfe4" stop-opacity=".03"/><stop offset=1 stop-color="#bbcfe4" stop-opacity="0"/></linearGradient>' +
  '<filter id=hmw x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation=6 /></filter></defs>';
function theme(name) {
  const d = n => `${R}themes/${n}/`;
  return Promise.all([got(d('farm') + 'sprites.svg'), got(d('farm') + 'theme.json', 1), got(d(name) + 'sprites.svg'), got(d(name) + 'theme.json', 1), got(d(name) + 'scene.svg')]).then(([fs, fj, ts, tj, sc]) => {
    if (document.documentElement.dataset.theme !== name) return; // a later pick won
    const svg = t => { const e = document.createElementNS(NS, 'svg'); e.innerHTML = t; return e; }, farm = svg(fs), own = svg(ts);
    if (DEMO && DBG.get('nopose')) delete (tj.poses || {})[DBG.get('nopose')];
    T = Object.assign({}, fj, tj);
    const sprite = (src, id) => id && src.querySelector('#' + CSS.escape(id));
    defs.querySelectorAll('symbol').forEach(s => s.remove());
    // A symbol `id` of sprite `want` (else farm's `alt`): legs rotate by the inherited `--gait`, manes skew by `--mane`
    // (storm), so instances need no per-leaf DOM.
    // #423: a sprite's `data-box` is its own box (a 24 × 24 row sprite), else `box`.
    const sym = (id, want, alt, box = '0 0 132 100') => {
      const g = (sprite(own, want) || sprite(farm, want) || sprite(farm, alt))?.cloneNode(true), s = document.createElementNS(NS, 'symbol');
      if (!g) return;
      s.id = id;
      s.setAttribute('viewBox', g.getAttribute('data-box') || box);
      g.removeAttribute('id');
      s.append(g);
      s.querySelectorAll('.leg').forEach(l => {
        const a = l.style.getPropertyValue('--a') || '0deg', sign = /leg-(nf|fh)/.test(l.getAttribute('class')) ? 1 : -1;
        l.setAttribute('style', `transform-box:fill-box;transform-origin:50% 0;transform:rotate(calc(${a} + ${sign} * var(--gait, 0deg)))`);
      });
      s.querySelectorAll('[fill="var(--m)"],[stroke="var(--m)"]').forEach(m => m.setAttribute('style', 'transform-box:fill-box;transform-origin:100% 100%;transform:skewX(var(--mane, 0deg))'));
      defs.append(s);
    };
    ['working', 'idle', 'done', 'blocked'].forEach((p, k) => sym('hp' + (k + 1), (tj.poses || {})[p], fj.poses?.[p]));
    Object.entries(T.actions || {}).forEach(([a, id]) => { sym('ha-' + a, id, fj.actions?.[a]); if (T.row_sprites) sym('hr-' + a, T.row_sprites[a], fj.actions?.[a]); });
    if (T.row_sprites) ['queued', 'needs', 'merged', 'fail'].forEach(k => sym('hr-' + k, T.row_sprites[k])); // #370: rows by kind
    ['calm', 'busy', 'held'].forEach(l => sym('hl-' + l, T.load_scene?.minis?.[l], null, '0 0 48 16')); // #370: the Host card's load
    sym('hrail', T.rail?.item, fj.rail?.item);
    sym('hrail-n', T.rail?.needs, fj.rail?.needs);
    Object.entries(T.moments || {}).forEach(([k, m]) => sym('hm-' + k, m.sprite));
    sym('hfan', T.load_scene?.fan); // #239: an idle lane's sprite while the heat is held
    // #423: each stage's plant per species (`hs-<id>`) and its Queue row one (`hq-<id>-row`), for serve.js's plant().
    Object.values(T.stages || {}).forEach(id => [...(T.species || []).map(s => `${id}-${s.id}`), id].forEach(i => { sym('hs-' + i, i); sym('hq-' + i + '-row', i + '-row'); }));
    sym('hload', T.loading); // #337: the loading strip's sprite; none (terminal): a spinning glyph
    $('#ld').classList.toggle('ld', !!defs.querySelector('#hload'));
    Object.keys(PAL).forEach(k => H.style.removeProperty(k));
    PAL = T.scene.palette || {};
    Object.entries(PAL).forEach(([k, v]) => H.style.setProperty(k, v));
    $('#herd-on').setAttribute('aria-label', w('herd') + ' mode');
    $('.hx').setAttribute('aria-label', `close ${w('herd')} mode`);
    if (T.icons) $$('link[rel=icon],link[rel=apple-touch-icon]').forEach(l => { l.href = d(name) + T.icons[l.rel === 'icon' ? 'favicon' : '180']; });
    // The pages are rebuilt with the theme's scene (or farm's ranch); the tabs take its sprites, modes and load scene.
    RAW = sc;
    pages.forEach(p => p.remove());
    pages.clear();
    placed = false;
    draw();
    ranch();
    B.classList.add('hz');
    TT = T;
    TM = T.modes || ['scene', 'ascii'];
    mode(get('mode'));
    rows(document);
    groups();
    if (lopen()) lsheet(); // #335: an open look sheet shows STYLE only for a theme with ascii
  });
}

B.insertAdjacentHTML('beforeend', `<div id=herd hidden><div class=hpages></div><div class=hrain></div><div class=hflash></div>
<button type=button class=hlt></button><button type=button class=hx aria-label="close herd mode">×</button><p class=hcap></p><div class=hq hidden></div></div>`);
const H = $('#herd'), PG = H.querySelector('.hpages'), cap = $('.hcap'), info = $('.hq'), rain = $('.hrain'), flash = $('.hflash');
B.prepend(defs);
// #58: the header's Herd button is our horse (the default coat), not an emoji.
$('#herd-on').innerHTML = '<svg viewBox="0 0 132 100" style="--c:#c89a56;--d:#a37a3c;--m:#e8dcc0;--b:#efe7dc" aria-hidden=true><use href="#hp2"/></svg>';
const pages = new Map();
// #242: a horse acts by its role while it works (serve.rs `act`), else by its state.
const ACT = l => ({ done: 'done', blocked: 'blocked', dead: 'blocked' })[l.state] || (l.state === 'working' ? ({ fix: 'fix', update: 'update', rev: 'rev', review: 'rev' })[l.role] || 'impl' : 'idle');
const BANDS = [['working', ['working']], ['idle', ['idle', 'unknown', 'parked']], ['done', ['done']], ['blocked/dead', ['blocked', 'dead']]];
let data = null, wind = 0, storm = false, saved = get('herdp'), placed = false;
// #334: the last Herd data this phone saw paints the scene at once (no request blocks the first paint); the live answer
// replaces it. The demo (fixed fixtures) never keeps one.
const keep = j => { if (!DEMO) put('herd:data', j); };
try { data = DEMO ? null : JSON.parse(get('herd:data')); } catch (e) {}

// #65: Corral's label card, 2 lines: the name, then state mark + word + age (the issue title is the tooltip; #115: the word's hint).
// #239: a working lane draws its role's action (cook, chop…) and says its word; an idle one fans while the heat is held.
// #292: a done lane whose review FAILed waits for its fix round: the theme's sent-back moment and `!`, never `✓`; a `·` glyph
// is an empty field, dropped (#162's join); the whole line is its `title` (ellipsised on a phone).
function horse(l, k) {
  const fail = l.state === 'done' && /\bFAIL\b/.test(l.shown || ''), a = l.state === 'working' && l.act || ACT(l);
  const use = fail && $('#hm-fail') ? 'hm-fail' : a === 'idle' && H.dataset.heat === 'held' && $('#hfan') ? 'hfan' : 'ha-' + a;
  const art = duo(T, i => defs.querySelector('#' + i), use, use !== 'hm-fail' && l.grow, l.issue ?? -1); // #423: over its plant
  const said = l.state === 'working' && T.words[a] || l.shown, g = fail ? T.glyphs.blocked || '!' : T.glyphs[l.state] || '?';
  const mark = [g === '·' ? '' : g, said].filter(Boolean).join(' '), full = [mark, l.age, l.host].filter(Boolean).join(' · ');
  return `<button type=button class="hh ${hx(l.state)}" data-agent="${hx(l.name)}" data-k=${k}${l.needs ? ` data-n=${l.issue}` : ''}${l.title ? ` title="${hx(l.title)}"` : ''}><svg viewBox="0 0 132 100" style="${hx(look(l))}">${art}</svg>` +
    `<span class=hl><b>${hx(l.name)}</b><span title="${hx(full)}"><i class=${hx(fail ? 'r' : l.tone)}${hint(l.shown) ? ` title="${hx(hint(l.shown))}"` : ''}>${hx(mark)}</i>${l.age ? ' · ' + hx(l.age) : ''}${l.host ? ' · ' + hx(l.host) : ''}</span></span></button>`;
}
// #423: with a theme's `stages`, the issue's cutting (its `queued` plant), `rail.needs` over it while it needs you.
const queued = q => {
  const p = plant(T, 'queued', q.issue), id = p && p.ids.find(i => defs.querySelector('#hs-' + i));
  const art = id ? `<use href="#hs-${id}" style="${p.style}"/>${q.needs ? '<use href="#hrail-n"/>' : ''}` : `<use href="#hrail${q.needs ? '-n' : ''}"/>`;
  return `<button type=button class="hqh${q.needs ? ' n' : ''}" data-i=${q.issue}><svg viewBox="0 0 132 100" style="${hx(q.coat)}">${art}</svg>` +
    `<span>#${q.issue}${q.needs ? '<br><i class=r>! needs you</i>' : q.blocked_by ? `<br><i class=r>⛔#${q.blocked_by}</i>` : ''}</span></button>`;
};
// #130: a fleet's `needs` (its needs-human issues): their horses wear a red `!` in the blocked band, an issue
// with no lane waits at the gate; a tap opens its needs-you card.
// #292: lanes of merged or closed issues (`gone`) aren't drawn (the Lanes tab and `clean` have them); each lane takes the next look
// not on the page yet (from its name's hash), the hats taken in turn (toque, bandana, cap).
function nh(f) {
  const n = f.needs || [], L = T.looks || [], used = new Set(), hat = k => ['--ht', '--hb', '--hk'].map(x => L[k][x]).join();
  const hats = [...new Set(L.map((_, k) => hat(k)))];
  const lanes = uniq(f.lanes.filter(l => !l.gone), l => l.name).map((l, i) => {
    let k = hash(l.name) % (L.length || 1);
    for (let t = 0; L.length && t < L.length && (used.has(k) || hat(k) !== hats[i % hats.length]); t++) k = (k + 1) % L.length;
    used.add(k);
    l = Object.assign({}, l, { look: L.length ? k : null });
    return n.includes(l.issue) ? Object.assign(l, { needs: true, state: 'blocked', act: 'blocked', grow: 'blocked', tone: 'r', shown: 'needs you' }) : l;
  });
  const queue = uniq(f.queue, q => q.issue).filter(q => !n.includes(q.issue) || !f.lanes.some(l => l.issue === q.issue)).map(q => Object.assign({}, q, { needs: n.includes(q.issue) }));
  return [f.name, lanes, queue, f.color];
}
function card(n) {
  const c = here().querySelector(`.hn .ask[data-n="${n}"]`);
  if (c && c.matches('.lt') && !unfold) { unfold = true; fold(); } // #154: a horse's hidden card unfolds the line
  if (c) { c.scrollIntoView({ block: 'nearest' }); c.animate([{ outline: '2px solid var(--r)' }, { outline: 'none' }], 1500); }
  return c;
}
const tpl = h => { const t = document.createElement('template'); t.innerHTML = h; return t.content.firstElementChild; };
// #65: the horse keyed `key` (taken from `have`), patched in place to `h`: button and coat attributes, pose, label.
function keyed(have, key, h) {
  let n = have.get(key);
  have.delete(key);
  if (!n) n = tpl(h);
  else if (n._h !== h) {
    const x = tpl(h);
    attrs(n, x);
    attrs(n.firstChild, x.firstChild); // the coat (also drops a stopped trot's transform)
    if (n.firstChild.innerHTML !== x.firstChild.innerHTML) n.firstChild.innerHTML = x.firstChild.innerHTML; // the pose (#423: and plant)
    n.lastChild.innerHTML = x.lastChild.innerHTML;
  }
  n._h = h;
  return n;
}
// #65: an update patches the page in place, so its scroll never moves: horses are keyed by agent (queued ones by
// issue), re-parented between bands, added or removed one by one; a band only comes or goes with its first or last horse.
// #516: keys are unique on the page: a node whose key is already taken goes, and data repeating a key draws it once.
const nodes = (hb, sel, k) => [...hb.querySelectorAll(sel)].reduce((m, n) => (m.has(n.dataset[k]) ? n.remove() : m.set(n.dataset[k], n), m), new Map());
const uniq = (xs, k) => xs.filter((x, i) => xs.findIndex(y => k(y) === k(x)) === i);
function patch(hb, lanes, queue) {
  const have = nodes(hb, '.hh', 'agent'), haveq = nodes(hb, '.hqh', 'i');
  const groups = [['queue', 'queued', queue, haveq, q => String(q.issue), queued, 'gate'],
    ...BANDS.map(([name, states]) => [name.split('/')[0], name, lanes.filter(l => states.includes(l.state)), have, l => l.name, horse, 'field'])];
  // Gone horses leave first, so a kept one never steps around them.
  const want = new Set(groups.flatMap(g => g[2].map(g[4])));
  for (const m of [have, haveq]) for (const [k, n] of m) if (!want.has(k)) { n.remove(); m.delete(k); }
  let at = null;
  const drop = [];
  for (const [cls, name, items, map, id, make, box] of groups) {
    let b = hb.querySelector(':scope > .b-' + cls);
    if (!items.length) { if (b) drop.push(b); continue; }
    if (!b) {
      b = tpl(`<div class="band b-${cls}"><h4>${hx(w(name))} <span></span></h4><div class=${box}></div></div>`);
      if (at) at.after(b); else hb.prepend(b);
    }
    const c = b.querySelector('h4 span');
    if (c.textContent !== String(items.length)) c.textContent = items.length;
    const f = b.lastChild;
    items.forEach((it, i) => { const n = keyed(map, id(it), make(it, i)); if (f.children[i] !== n) f.insertBefore(n, f.children[i] || null); });
    at = b;
  }
  drop.forEach(b => b.remove());
  // #292: ≥ 900 px every card is as wide as the page's widest line (320–520 px), so the columns line up.
  hb.style.setProperty('--cw', Math.min(520, Math.max(320, ...[...hb.querySelectorAll('.hl>*')].map(x => x.scrollWidth + 20))) + 'px');
  const e = hb.querySelector(':scope > .empty');
  if (at) { if (e) e.remove(); } else if (!e) hb.append(tpl(`<p class=empty>no ${w('horses')} here</p>`));
}

// #239/#292: the backdrop fills its band edge to edge at the band's height (`--ar`), top-anchored and never cropped: an odd
// number of tiles (slice: only the outer tiles' spare sides go) centred on this page's own, all at one tint. A theme's
// `scene.line` puts every fleet's station in turn; farm's ranch is mirrored every other tile so the edges meet (the moon
// only in the middle), its empty sky above the hills (y < Y0) left to the band's gradient and clouds, as the 390 mockup (#58's 22svh still fits).
// Rebuilt when the count changes.
const Y0 = 130;
function tiles(p) {
  const r = p.querySelector('.hr'), v = RAW.match(/viewBox="0 0 ([\d.]+) ([\d.]+)"/), [vw, vh] = v ? [+v[1], +v[2]] : [390, 320];
  const y0 = v ? 0 : Y0, h = vh - y0;
  r.style.setProperty('--ar', f1(h / vw * 1000) / 1000);
  if (!r.clientWidth || !r.clientHeight) return;
  const n = 2 * Math.ceil((r.clientWidth * h / vw / r.clientHeight - 1) / 2) + 1, mid = (n - 1) / 2;
  if (p._n === n) return;
  p._n = n;
  const fs = data?.fleets || [], i = fs.findIndex(f => f.name === p._key), body = (v ? RAW : scene()).replace(/^[\s\S]*?<svg[^>]*>|<\/svg>\s*$/g, '');
  let s = '';
  for (let k = -mid; k <= mid; k++) {
    const f = T.scene.line && i >= 0 && fs[((i + k) % fs.length + fs.length) % fs.length];
    s += `<svg x="${k * vw}" width="${vw}" height="${vh}" viewBox="0 0 ${vw} ${vh}"${k ? '' : ' class=t0'}${f ? ` style="--fk:var(--${hx(f.color)})"` : ''}>` +
      (v || k % 2 === 0 ? body : `<g transform="matrix(-1 0 0 1 ${vw} 0)">${body}</g>`) + '</svg>';
  }
  r.querySelector('.rs')?.remove();
  r.querySelector('.hc').insertAdjacentHTML('afterend', `<svg class=rs viewBox="${-mid * vw} ${y0} ${n * vw} ${h}" preserveAspectRatio="xMidYMin slice"${v ? ' shape-rendering=crispEdges' : ''} aria-hidden=true>${s}</svg>`);
  p._trees = [...r.querySelectorAll('.cn')].map(g => [g, +g.dataset.a, +g.dataset.l]);
  p._cel = [...r.querySelectorAll('.cel')];
  p._grass = [...r.querySelectorAll('.gt')].map(g => [g, +g.dataset.x, +g.dataset.y]);
}
function page(key) {
  const p = document.createElement('section');
  p.className = 'hp';
  p.innerHTML = `<div class=hs><p><b></b> <span class=n></span></p><p class=sum></p></div><div class=hn></div>
<div class=hr><div class=hc>${T.scene.weather === false ? '' : clouds()}</div><div class=fog></div>
<button type=button class="hnv hpv" aria-label="previous repo">‹</button><button type=button class="hnv hnx" aria-label="next repo">›</button></div><p class=hdt></p><div class=hb></div>`;
  p._trees = p._cel = p._grass = [];
  p._key = key;
  return p;
}

function draw() {
  if (!data) return;
  const list = data.fleets.map(nh);
  if (data.others.length) list.push(['Others', uniq(data.others, l => l.name), []]);
  for (const [k, p] of pages) if (!list.some(x => x[0] === k)) { p.remove(); pages.delete(k); }
  list.forEach(([key, lanes, queue, c], n) => {
    const p = pages.get(key) || page(key);
    pages.set(key, p);
    if (c) p.style.setProperty('--fk', `var(--${c})`); // #242: the halters and saddle cloths wear the fleet's colour (#162)
    // Never re-append a placed page: a detached scroller forgets its scrollTop (#65, the snap to the top).
    if (PG.children[n] !== p) PG.insertBefore(p, PG.children[n] || null);
    p.querySelector('.hs b').textContent = key;
    p.querySelector('.hs .n').textContent = `· ${lanes.length} here`;
    // #291: one dot button per repo under the scene; a tap jumps there.
    const d = p.querySelector('.hdt'), ks = list.map(x => x[0]).join('\n');
    if (d.dataset.k !== ks) { d.dataset.k = ks; d.innerHTML = list.map(x => `<button type=button aria-label="${hx(x[0])}"></button>`).join(''); }
    [...d.children].forEach((b, i) => b.toggleAttribute('aria-current', i === n));
    const count = states => lanes.filter(l => states.includes(l.state)).length;
    // #65 A: queued first, zero counts left out, a bare `·` (a full mono cell) between, so the line fits the 28 px strip at 390 px without an ellipsis.
    p.querySelector('.sum').textContent = [[queue.length, 'queued'], [count(['blocked', 'dead']), 'blocked'], [count(['working']), 'working'],
      [count(['idle', 'unknown', 'parked']), 'idle'], [count(['done']), 'done']].filter(c => c[0]).map(c => c.join(' ')).join('·');
    // Queued horses get their own row above the working band; a band with no horses is not drawn.
    const top = p.scrollTop;
    patch(p.querySelector('.hb'), lanes, queue);
    if (p.scrollTop !== top) p.scrollTo({ top, behavior: 'instant' });
    p._queue = queue;
    p._walk = [...p.querySelectorAll('.hh.working svg')];
  });
  H.classList.toggle('one', list.length < 2);
  needs();
  weather(data.host);
  if (!placed && list.length) {
    placed = true;
    const i = Math.max(0, list.findIndex(x => x[0] === saved));
    PG.scrollLeft = i * PG.clientWidth;
  }
  place();
  anim();
}
function herdData(j) { if (!DBG.get('weather')) { data = j; keep(JSON.stringify(j)); draw(); } }
async function herdPull() {
  if (H.hidden) return;
  const w = DBG.get('weather'), t = await wait(part('herd.json' + (w ? '?weather=' + encodeURIComponent(w) : '')));
  try { if (t) { data = JSON.parse(t); if (!w) keep(t); draw(); } } catch (e) {}
}

// #40/#42: each page carries its fleet's needs-you cards (the same per-audience HTML as the tabs).
function needs() {
  const keys = [...pages.keys()];
  pages.forEach((p, k) => {
    const mine = [...$$('#needs > .ask')].filter(c => {
      const f = (c.querySelector('.m') || {}).textContent || '';
      return k === 'Others' ? !keys.includes(f.split(' · ')[0]) : f.split(' · ')[0] === k;
    });
    p.querySelector('.hn').replaceChildren(...mine.map(c => c.cloneNode(true)));
  });
  fold(); // #154: [later] and the folded line on the copies too
}
new MutationObserver(needs).observe($('#needs'), { childList: true, subtree: true });

// Wind = load/cpu ÷ limit (0..1): canopy/grass ×1..3, clouds as fast. The spawn gate refusing = storm;
// memory pressure = fog. Reduced motion keeps the tint only.
function weather(h) {
  wind = Math.min(1, Math.max(0, (h.load || 0) / (h.limit || 1)));
  const wx = T.scene.weather !== false; // #172: a theme's `scene.weather: false` keeps the scene clear
  storm = wx && !!h.gate;
  H.classList.toggle('storm', storm);
  H.classList.toggle('fog1', wx && h.pressure === 'warn');
  H.classList.toggle('fog2', wx && h.pressure === 'critical');
  H.classList.toggle('gust', wind >= 0.5);
  if (!storm || still.matches) H.style.setProperty('--mane', '0deg');
  // #239 heat (#423: and the window light): calm < 0.5 ≤ busy; held = the spawn gate refusing (the farm's storm).
  const kd = indoor() ? T.load_scene.kind : '', lv = kd ? (h.gate ? 'held' : wind >= 0.5 ? 'busy' : 'calm') : '', was = H.dataset.heat || '';
  ['heat', 'light'].forEach(x => { H.classList.toggle(x, x === kd); ['calm', 'busy', 'held'].forEach(k => H.classList.toggle(`${x}-${k}`, x === kd && k === lv)); });
  H.dataset.heat = lv;
  if ((lv === 'held') !== (was === 'held') && data) draw();
  if (SHOT) anim(); // the storm's rain and mane at the fixed frame
  cap.textContent = `load ${h.load == null ? '?' : h.load.toFixed(2)} · mem ${h.pressure || '?'} · swap ${h.swapouts == null ? '?' : Math.round(h.swapouts)}`;
}

// The tiles at the band's height; the cloud layer spans the band plus one 780-unit period (`_cw`) to drift through.
function place() {
  for (const p of pages.values()) {
    tiles(p);
    const r = p.querySelector('.hr'), k = r.clientHeight / 320, c = p.querySelector('.hc');
    p._cw = Math.max(1560 * k, r.clientWidth + 780 * k);
    c.style.width = p._cw + 'px';
    c.style.height = 320 * k + 'px';
  }
}
addEventListener('resize', place);
const here = () => [...pages.values()][Math.round(PG.scrollLeft / Math.max(1, PG.clientWidth))];
function herdPage() { return H.hidden ? null : here(); }
PG.addEventListener('scroll', () => { const p = here(); if (p && p._key !== saved) { saved = p._key; put('herdp', saved); } }, { passive: true });
// #291 every way to switch repos besides the native touch swipe: [‹] [›] (wrapping), the dots, ←/→, a horizontal wheel
// (trackpad swipe, shift+wheel; one switch per gesture) and a mouse drag. One go(i); reduced motion switches instantly.
const pageAt = () => Math.round(PG.scrollLeft / Math.max(1, PG.clientWidth));
function go(i) {
  const n = PG.children.length;
  if (n) PG.scrollTo({ left: ((i % n + n) % n) * PG.clientWidth, behavior: still.matches ? 'instant' : 'smooth' });
}
const TEXT = 'input, textarea, select, [contenteditable]';
let swipe = null, swiped = false, wheelAt = 0, wheelX = 0, wheelDone = false;
H.addEventListener('click', e => {
  if (swiped) { swiped = false; e.stopPropagation(); return; } // the click that ends a swipe opens nothing
  const b = e.target.closest('.hnv, .hdt button');
  if (b) go(b.matches('.hnv') ? pageAt() + (b.matches('.hnx') ? 1 : -1) : [...b.parentNode.children].indexOf(b));
}, true);
document.addEventListener('keydown', e => {
  if (H.hidden || shown() || e.altKey || e.ctrlKey || e.metaKey || e.target.closest(TEXT)) return;
  const k = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
  if (k) { e.preventDefault(); go(pageAt() + k); }
});
PG.addEventListener('wheel', e => {
  const x = e.deltaX || (e.shiftKey ? e.deltaY : 0), y = e.deltaX ? e.deltaY : e.shiftKey ? 0 : e.deltaY;
  if (Math.abs(x) <= Math.abs(y)) return;
  e.preventDefault();
  if (e.timeStamp - wheelAt > 300) { wheelX = 0; wheelDone = false; } // a 300 ms pause ends the gesture (and its inertia)
  wheelAt = e.timeStamp;
  wheelX += x;
  if (!wheelDone && Math.abs(wheelX) > 40) { wheelDone = true; go(pageAt() + Math.sign(wheelX)); }
}, { passive: false });
// A touch swipe scrolls natively (the browser cancels the pointer); a mouse drag past 40 px switches like one.
// A press in a text field (a needs-you answer) is its own text selection, never a drag. The pointer is captured
// only past 40 px, so a click with a little jitter still lands on the horse.
PG.addEventListener('pointerdown', e => { swipe = e.button === 0 && !e.target.closest(TEXT) ? { x: e.clientX, id: e.pointerId } : null; swiped = false; });
PG.addEventListener('pointermove', e => {
  if (!swipe || e.pointerId !== swipe.id) return;
  const dx = Math.abs(e.clientX - swipe.x);
  if (dx >= 8 && !H.classList.contains('drag')) { H.classList.add('drag'); getSelection().removeAllRanges(); }
  if (dx > 40 && !PG.hasPointerCapture(e.pointerId)) try { PG.setPointerCapture(e.pointerId); } catch (_) {}
});
PG.addEventListener('pointerup', e => {
  const dx = swipe && e.pointerId === swipe.id ? e.clientX - swipe.x : 0;
  swipe = null;
  H.classList.remove('drag');
  if (Math.abs(dx) > 40) { swiped = true; go(pageAt() - Math.sign(dx)); }
});
PG.addEventListener('pointercancel', () => { swipe = null; H.classList.remove('drag'); });

let raf = 0, lastT = 0, drift = 0, cel = 0, flashAt = -99, nextFlash = 0, frames = 0, fpsAt = 0;
function frame(ms) {
  raf = 0;
  if (H.hidden || document.hidden || still.matches) return;
  const t = ms / 1000, dt = Math.min(0.1, t - (lastT || t)), m = 1 + 2 * wind, p = here();
  lastT = t;
  drift = (drift + dt * 2.4 * m) % 780;
  cel = (cel + dt * 0.7) % 390;
  if (p) {
    for (const [g, a, l] of p._trees) g.setAttribute('transform', `translate(${f1(sway(t, a, WIND.canopy * l * m))} ${f1(sway(t, a + 1.9, 0.5 * l * m))})`);
    for (const [g, x, y] of p._grass) { const b = bend(t, x, WIND.grass * m) / 10; g.setAttribute('transform', `matrix(1 0 ${-b.toFixed(3)} 1 ${(b * y).toFixed(2)} 0)`); }
    if (H.classList.contains('night')) p._cel.forEach(c => c.setAttribute('transform', `translate(${cel.toFixed(2)} 0)`));
    p.querySelector('.hc').style.transform = `translateX(${((drift - 780) / 1560 * (p._cw || 1560)).toFixed(1)}px)`;
    // Working horses trot (diagonal leg pairs, Corral's 0.4 s stride) and roam a little.
    p._walk.forEach((s, i) => {
      const g = Math.sin(t * Math.PI / 0.4 + i * 1.3);
      s.style.setProperty('--gait', (g * 16).toFixed(1) + 'deg');
      s.style.transform = `translate(${(Math.sin(t * 0.5 + i * 2.1) * 10).toFixed(1)}px, ${(-Math.abs(g) * 1.5).toFixed(1)}px)`;
    });
  }
  if (storm) {
    H.style.setProperty('--mane', (-12 - 6 * Math.sin(t * 3.1)).toFixed(1) + 'deg');
    rain.style.transform = `translate(${(-(t * 90) % 60).toFixed(1)}px, ${((t * 520) % 60).toFixed(1)}px)`;
    // Lightning: at most once every 20 s.
    if (t >= nextFlash) { flashAt = t; nextFlash = t + 20 + Math.random() * 25; }
    flash.style.opacity = Math.max(0, 0.7 - (t - flashAt) * 3.5).toFixed(2);
  }
  frames++;
  if (DBG.has('fps') && t - fpsAt >= 1) { cap.dataset.fps = Math.round(frames / (t - fpsAt)) + ' fps'; frames = 0; fpsAt = t; }
  if (!SHOT) raf = requestAnimationFrame(frame);
}
function anim() { if (!raf && !H.hidden && !document.hidden && !still.matches) { lastT = 0; nextFlash = performance.now() / 1000 + 4; SHOT ? frame(2000) : raf = requestAnimationFrame(frame); } }

// Taps: a horse opens the Lanes sheet (#43: card, transcript, Stop/Kill/Clean, reply); a queued one its row.
H.addEventListener('click', e => {
  const h = e.target.closest('.hh'), q = e.target.closest('.hqh');
  if (h) {
    const l = laneOf(h.dataset.agent);
    if (l && !(h.dataset.n && card(h.dataset.n))) open(l);
  } else if (q) {
    const it = (here()._queue || []).find(x => String(x.issue) === q.dataset.i);
    if (!it || it.needs && card(it.issue)) return;
    info.innerHTML = `<p><b>#${it.issue}</b> queued</p>${it.title ? `<p>${hx(it.title)}</p>` : ''}${it.blocked_by ? `<p class=r>⛔ blocked by #${it.blocked_by}</p>` : ''}<p class=m>tap to close</p>`;
    info.hidden = false;
  } else if (e.target.closest('.hq')) info.hidden = true;
});
// The 🐎 button enters Herd mode, ✕ leaves; remembered, so the app reopens where it was.
function herd(on) {
  H.hidden = !on;
  document.documentElement.classList.toggle('herding', on);
  put('herd', on ? '1' : '');
  if (on) { draw(); herdPull(); anim(); }
}
$('#herd-on').onclick = () => herd(true);
$('.hx').onclick = () => herd(false);
// #71 ranch light, Corral's Auto/Day/Night: [A]/[☀]/[☾] left of [×] cycles them, remembered on this phone (`?light=` overrides
// for a demo). Auto is the phone clock, Day 07:00–19:00, re-checked every minute and on resume.
const LIGHTS = ['auto', 'day', 'night'], ICON = { auto: 'A', day: '☀', night: '☾' }, lb = $('.hlt');
const nightAt = (l, d) => l === 'night' || (l === 'auto' && (d.getHours() < 7 || d.getHours() >= 19));
let light = [DBG.get('light'), get('herd-light')].find(l => LIGHTS.includes(l)) || 'auto';
function ranch() {
  const n = !indoor() && nightAt(light, new Date());
  H.classList.toggle('night', n);
  lb.hidden = indoor();
  lb.textContent = ICON[light];
  lb.setAttribute('aria-label', `${w('ranch')} light: ${light}${light === 'auto' ? ` (${n ? 'night' : 'day'} now)` : ''}`);
}
lb.onclick = () => { light = LIGHTS[(LIGHTS.indexOf(light) + 1) % 3]; put('herd-light', light); ranch(); };
ranch();
setInterval(() => document.hidden || ranch(), 60000);
document.addEventListener('visibilitychange', () => { anim(); ranch(); });
// #39 demo: a weather switcher sets the host values client-side, as `/herd.json?weather=` does on loopback.
const WX = { calm: [0.1, 'normal'], gusty: [0.7, 'normal'], storm: [1.3, 'normal'], fog: [0.3, 'warn'] };
if (DEMO) H.insertAdjacentHTML('beforeend', `<p class=hwx>${Object.keys(WX).map(w => `<button type=button value=${w}>${w}</button>`).join('')}</p>`);
H.addEventListener('click', e => {
  const w = e.target.closest('.hwx button'), h = data && data.host;
  if (!w || !h) return;
  const [k, p] = WX[w.value], why = [p !== 'normal' && 'memory pressure ' + p, k > 1 && `load/cpu ${(k * h.limit).toFixed(2)} > ${h.limit}`].filter(Boolean);
  weather(Object.assign(h, { load: k * h.limit, pressure: p, swapouts: 0, gate: why.join('; ') || null }));
  $$('.hwx button').forEach(b => b.classList.toggle('on', b === w));
});
herd(get('herd') === '1' || DBG.get('herd') === '1');
const themed = theme(document.documentElement.dataset.theme).catch(() => {}); // ui-audit.js waits for it
