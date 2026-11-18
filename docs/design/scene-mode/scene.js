// Design-only (#1305): the old Herd (c34235855^ assets/herd.js: scene, clouds, night sky, tiles, cards, gate, bands and the
// one-clock frame loop) drawn from fixture.json, the shape of the `/screens/scene` JSON. The art is swag's legacy-board farm
// at the pinned commit (here read from the repo's vendored copy of that commit; impl fetches it and checks SHA256SUMS).
// Query: old=1 (the old Herd chrome), style=pixel|ascii, light=auto|day|night, mode=scene|screens, sheet=look,
// loading=1 (first paint), t=<s> (one frozen frame at that clock time, for the shots and the clip).
const Q = new URLSearchParams(location.search), OLD = Q.has('old'), STYLE = Q.get('style') || 'pixel';
const LIGHT = Q.get('light') || 'auto', MODE = Q.get('mode') || 'scene', T0 = Q.has('t') ? +Q.get('t') : null;
const NS = 'http://www.w3.org/2000/svg', M64 = (1n << 64n) - 1n;
const noise = (i, s = 0) => { const n = (BigInt(i) * 1664525n + BigInt(s) * 1013904223n) & M64; return Number(((n ^ (n >> 13n)) * 1274126177n) % 65536n) / 65535; };
const hx = s => String(s ?? '').replace(/[&<>"]/g, c => '&#' + c.charCodeAt(0) + ';');
const f1 = n => +n.toFixed(1);
const WIND = { period: 5.2, canopy: 2.8, grass: 2.1 };
const phase = t => t * 2 * Math.PI / WIND.period;
const anchor = x => ((x % 390) / 390) * 2 * Math.PI * 0.7;
const sway = (t, a, amp) => amp * (0.74 * Math.sin(phase(t) + a) + 0.26 * Math.sin(1.93 * phase(t) + a * 1.7));
const bend = (t, x, amp) => amp * (0.8 * Math.sin(phase(t) - x * 0.028) + 0.2 * Math.sin(1.9 * phase(t) - x * 0.011));
const ell = (x, y, rx, ry, fill, op = 1) => `<ellipse cx="${f1(x)}" cy="${f1(y)}" rx="${f1(rx)}" ry="${f1(ry)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
const rect = (x, y, w, h, fill, op = 1) => `<rect x="${f1(x)}" y="${f1(y)}" width="${f1(w)}" height="${f1(h)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
const COATS = [['8a5a33', '6f4527', '2e2019', 0], ['a05c2c', '7d4520', '5d3317', 1], ['3b3b44', '2a2a31', '17161a', 0], ['b9bcc6', '989ca9', '7d8089', 0],
  ['c89a56', 'a37a3c', 'e8dcc0', 1], ['b08d5e', '8e6f45', '41321f', 1], ['96685a', '77524a', '4a342c', 0], ['bd8f4e', '96703a', '241a10', 1]];
const coat = k => { const [c, d, m, b] = COATS[k % 8]; return `--c:#${c};--d:#${d};--m:#${m};--b:${b ? '#efe7dc' : 'transparent'}`; };

function clouds() {
  let s = '';
  for (const copy of [0, 780]) for (let i = 0; i < 6; i++) {
    const x = i * 130 + 45 + copy, y = 36 + noise(i, 30) * 86;
    for (let j = 0; j < 5; j++) s += ell(x + j * 12, y - noise(j, i) * 9, 18, 5 + noise(j, 2) * 5, 'var(--cl)');
  }
  return `<svg viewBox="0 0 1560 320" preserveAspectRatio=none aria-hidden=true>${s}</svg>`;
}
function night() {
  let s = '';
  for (const k of [-390, 0, 390]) for (let b = 0; b < 6; b++) s += `<path d="M${k - 60} 220Q${k + 60} 74 ${k + 220} 3Q${k + 275} -20 ${k + 335} -32" stroke-width=${66 - b * 8}/>`;
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
  s += '<path d="M153 243L180 219L210 242L210 279L153 279Z" fill="var(--bn)"/><path d="M148 245L180 215L216 243L209 246L180 224L155 248Z" fill="var(--rf)"/>';
  s += rect(175, 252, 14, 27, 'var(--dr)') + rect(175, 235, 9, 9, 'var(--wn)', 0.8);
  for (const tile of [-1, 0, 1]) for (const [tx, ty, sc] of [[7, 225, 1], [365, 228, 1.1], [238, 258, 0.5], [326, 262, 0.38]]) {
    const x = tile * 390 + tx;
    if (x < -60 || x > 450) continue;
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
  s += '<rect class=wa width="390" height="320"/>';
  return s;
}

const defs = document.createElementNS(NS, 'svg');
defs.setAttribute('class', 'hdefs');
defs.innerHTML = '<defs><linearGradient id=hgr gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#bcc17b"/><stop offset=".5" stop-color="#8e9c58"/><stop offset=1 stop-color="#485e3b"/></linearGradient>' +
  '<linearGradient id=hgrn gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#63746a"/><stop offset=".5" stop-color="#415d55"/><stop offset=1 stop-color="#273f3e"/></linearGradient>' +
  '<linearGradient id=hwd gradientUnits=userSpaceOnUse x1=0 y1=0 x2=195 y2=500><stop offset=0 stop-color="#ffeeab" stop-opacity=".03"/><stop offset=1 stop-color="#ffeeab" stop-opacity="0"/></linearGradient>' +
  '<linearGradient id=hwn gradientUnits=userSpaceOnUse x1=390 y1=0 x2=195 y2=500><stop offset=0 stop-color="#bbcfe4" stop-opacity=".03"/><stop offset=1 stop-color="#bbcfe4" stop-opacity="0"/></linearGradient>' +
  '<filter id=hmw x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation=6 /></filter></defs>';
document.body.prepend(defs);

const SWAG = '../../../assets/swag/farm/';
let TH = {};
function sym(src, id, want) {
  const g = src.querySelector('#' + CSS.escape(want || ''))?.cloneNode(true);
  if (!g) return;
  const s = document.createElementNS(NS, 'symbol');
  s.id = id;
  s.setAttribute('viewBox', g.getAttribute('data-box') || '0 0 132 100');
  g.removeAttribute('id');
  s.append(g);
  s.querySelectorAll('.leg').forEach(l => {
    const a = l.style.getPropertyValue('--a') || '0deg', sign = /leg-(nf|fh)/.test(l.getAttribute('class')) ? 1 : -1;
    l.setAttribute('style', `transform-box:fill-box;transform-origin:50% 0;transform:rotate(calc(${a} + ${sign} * var(--gait, 0deg)))`);
  });
  defs.firstChild.append(s);
}

const lightNow = () => LIGHT === 'night' || (LIGHT === 'auto' && (h => h < 7 || h >= 19)(new Date().getHours()));
const ICON = { auto: 'A', day: '☀', night: '☾' };
const figure = (f, small) => {
  if (STYLE === 'ascii') {
    const t = small ? (TH.ascii_rows || {}).idle && '[#]' : (TH.ascii || {})[f.act] || '';
    const two = f.state === 'working' ? t.replace(/[^\n]*$/, l => l.replace(/[/\\]/g, c => (c === '/' ? '\\' : '/'))) : null;
    return `<i class="asc${two ? ' run' : ''}" aria-hidden=true><b>${hx(t)}</b>${two ? `<b hidden>${hx(two)}</b>` : ''}</i>`;
  }
  return small ? `<svg viewBox="0 0 132 100"><use href="#hrail"/></svg>` : `<svg viewBox="0 0 132 100" style="${coat(f.coat || 0)}"><use href="#ha-${hx(f.act)}"/></svg>`;
};
const card = r => `<button type=button class="hh ${hx(r.figure.state)}">${figure(r.figure)}<span class=hl><b>${hx(r.title)}</b><span><i class=${hx(r.tone || 'm')}>${hx([(TH.glyphs || {})[r.figure.state], r.subtitle].filter(Boolean).join(' '))}</i>${r.value ? ' · ' + hx(r.value) : ''}${r.host ? ' · ' + hx(r.host) : ''}</span></span></button>`;
const bale = r => `<button type=button class=hqh>${figure(r.figure, 1)}<span>${hx(r.title)}${r.subtitle ? `<br><i class=${hx(r.tone || 'm')}>⛔${hx(r.subtitle.replace('blocked by ', ''))}</i>` : ''}</span></button>`;
const word = k => (TH.words || {})[k] || k;

function band(sec) {
  const cls = sec.header.split('/')[0], q = cls === 'queued';
  return `<div class="band b-${cls}"><h4>${hx(word(q ? 'queued' : sec.header))} <span>${sec.rows.length}</span></h4><div class=${q ? 'gate' : 'field'}>${sec.rows.map(q ? bale : card).join('')}</div></div>`;
}

function pageOf(repo, all, i) {
  const dots = all.map((r, k) => `<button type=button aria-label="${hx(r.title)}"${k === i ? ' aria-current=true' : ''}></button>`).join('');
  const needs = (repo.needs || []).map(n => `<div class="card bad"><div class=fold><span class=fl>${hx(n.line)}</span><button type=button class=main><span>${hx(n.button)}</span></button></div></div>`).join('');
  const lanes = repo.blocks.filter(b => b.header !== 'queued').reduce((n, b) => n + b.rows.length, 0);
  const body = Q.has('loading') ? `<div class=ldg>${STYLE === 'ascii' ? `<i class="asc run"><b>${hx((TH.ascii_loading || '').split('\n')[0])}</b><b hidden>${hx((TH.ascii_loading || '').split('\n')[1] || '')}</b></i>` : `<svg viewBox="0 0 132 100" style="${coat(4)}"><use href="#hload"/></svg>`}<span>${hx(word('loading'))}…</span></div>` : repo.blocks.map(band).join('');
  return `<section class=hp><div class=hs><p><b>${hx(repo.title)}</b> <span class=n>· ${lanes} here</span></p><p class=sum>${hx(repo.subtitle)}</p></div><div class=hn>${needs}</div>` +
    `<div class=hr><div class=hc>${clouds()}</div><button type=button class="hnv hpv" aria-label="previous repo"><span>‹</span></button><button type=button class="hnv hnx" aria-label="next repo"><span>›</span></button></div><p class=hdt>${dots}</p><div class=hb>${body}</div></section>`;
}

function tiles(p) {
  const r = p.querySelector('.hr'), vw = 390, vh = 320, y0 = 130, h = vh - y0;
  r.style.setProperty('--ar', f1(h / vw * 1000) / 1000);
  const n = 2 * Math.ceil((r.clientWidth * h / vw / r.clientHeight - 1) / 2) + 1, mid = (n - 1) / 2, body = scene();
  let s = '';
  for (let k = -mid; k <= mid; k++) s += `<svg x="${k * vw}" width="${vw}" height="${vh}" viewBox="0 0 ${vw} ${vh}"${k ? '' : ' class=t0'}>${k % 2 === 0 ? body : `<g transform="matrix(-1 0 0 1 ${vw} 0)">${body}</g>`}</svg>`;
  r.querySelector('.hc').insertAdjacentHTML('afterend', `<svg class=rsn viewBox="${-mid * vw} ${y0} ${n * vw} ${h}" preserveAspectRatio="xMidYMin slice" aria-hidden=true>${s}</svg>`);
  const k = r.clientHeight / 320, c = r.querySelector('.hc');
  p._cw = Math.max(1560 * k, r.clientWidth + 780 * k);
  c.style.width = p._cw + 'px';
  c.style.height = 320 * k + 'px';
  p._trees = [...r.querySelectorAll('.cn')].map(g => [g, +g.dataset.a, +g.dataset.l]);
  p._cel = [...r.querySelectorAll('.cel')];
  p._grass = [...r.querySelectorAll('.gt')].map(g => [g, +g.dataset.x, +g.dataset.y]);
  p._walk = [...p.querySelectorAll('.hh.working > svg, .hh.working > .asc, .ldg > svg, .ldg > .asc')];
  p.querySelector('.hb').style.setProperty('--cw', Math.min(520, Math.max(320, ...[...p.querySelectorAll('.hl>*')].map(x => x.scrollWidth + 20))) + 'px');
}

let P = null, wind = 0;
function frame(t) {
  const m = 1 + 2 * wind, drift = (t * 2.4 * m) % 780, cel = (t * 0.7) % 390;
  for (const [g, a, l] of P._trees) g.setAttribute('transform', `translate(${f1(sway(t, a, WIND.canopy * l * m))} ${f1(sway(t, a + 1.9, 0.5 * l * m))})`);
  for (const [g, x, y] of P._grass) { const b = bend(t, x, WIND.grass * m) / 10; g.setAttribute('transform', `matrix(1 0 ${-b.toFixed(3)} 1 ${(b * y).toFixed(2)} 0)`); }
  P._cel.forEach(c => c.setAttribute('transform', `translate(${cel.toFixed(2)} 0)`));
  P.querySelector('.hc').style.transform = `translateX(${((drift - 780) / 1560 * P._cw).toFixed(1)}px)`;
  P._walk.forEach((s, i) => {
    const g = Math.sin(t * Math.PI / 0.4 + i * 1.3);
    if (s.tagName === 'svg') s.style.setProperty('--gait', (g * 16).toFixed(1) + 'deg');
    else s.querySelectorAll('b').forEach((b, k) => (b.hidden = k !== Math.floor(t / 0.25) % 2));
    s.style.transform = `translate(${(Math.sin(t * 0.5 + i * 2.1) * 10).toFixed(1)}px, ${(-Math.abs(g) * 1.5).toFixed(1)}px)`;
  });
}

const BOARD = `<main class=board><div class="card warn"><p style="margin:var(--s2) 0;color:var(--a)">Demo with fake data. Install drover to run your own.</p></div>
<div class="card warn"><div class=fold><span class=fl>#21 · owner · Rate-limit the public search</span><button type=button class=main><span>per api key</span></button></div></div>
<div class="card warn"><div class=fold><span class=fl>#44 · routine · Split the worker queue</span><button type=button class=main><span>no, keep one</span></button></div></div>
<div class="card bad"><div class=fold><span class=fl>#22 · needs-human · review round 3 of 3</span><button type=button class=main><span>one more round</span></button></div></div>
<p class=sh><b style="color:var(--fg)">the paddock</b> · queue</p><div class=chips><button class=on type=button><span>all</span></button><button type=button><span>acme-api</span></button><button type=button><span>acme-web</span></button></div>
<div class=card><div class=ch><b>acme-api</b><span class=tag style="color:var(--g)">driving</span></div><p class=m>on the trail · in progress (5)</p><p>#21 Rate-limit the public search</p><p>#19 Return 429 with retry-after</p></div></main>
<nav><a class=on><span>queue</span></a><a><span>events</span></a><a><span>lanes</span></a></nav>`;

function header() {
  const scn = MODE === 'scene';
  return `<header><b>drover</b><span class=tag>demo</span><i id=dot class=demo></i>` +
    `<button type=button class="scb${scn ? ' on' : ''}" aria-pressed=${scn} aria-label="scene mode"><span>scene</span></button>` +
    (scn ? `<button type=button class=hlt aria-label="ranch light: ${LIGHT}"><span>${ICON[LIGHT]}</span></button>` : '') +
    `<button type=button aria-label=refresh><span>↻</span></button><button type=button aria-label=settings><span>⚙</span></button></header>`;
}

function lookSheet() {
  const row = (on, label, art = '', sub = '') => `<div class="row tap${on ? ' on' : ''}">${art}<span class=rm><span class=rt>${on ? '✓ ' : ''}${hx(label)}</span>${sub ? `<span class=rs>${hx(sub)}</span>` : ''}</span></div>`;
  const sec = (h, rows) => `<section class=sec><h3 class=sh>${hx(h)}</h3><div class=list>${rows.join('')}</div></section>`;
  const px = `<i class=lka><svg viewBox="0 0 132 100" style="${coat(4)}"><use href="#ha-impl"/></svg></i>`, as = `<i class=lka><b>${hx((TH.ascii_rows || {}).impl || '')}</b></i>`;
  return `<div id=sheet><div class=sp><p class=sh><b>settings</b><button type=button><span>close</span></button></p><div class=sb>` +
    sec('screens', [row(false, 'Hosts', '', 'on/off and reap, per host')]) +
    sec('theme', ['farm', 'kitchen', 'garden', 'terminal'].map(n => row(n === 'farm', n))) +
    sec('style', [row(STYLE !== 'ascii', 'pixel', px, 'sprites in Scene, rows, lanes, host and loading'), row(STYLE === 'ascii', 'ascii', as, 'the theme’s ascii figures, same places')]) +
    sec('colours', ['farm', 'github-dark', 'nord', 'paper'].map(n => row(n === 'farm', n))) +
    `<p class="m foot">this phone only · the home-screen icon keeps its install-time look</p></div></div></div>`;
}

Promise.all([fetch(SWAG + 'sprites.svg').then(r => r.text()), fetch(SWAG + 'theme.json').then(r => r.json()), fetch('fixture.json').then(r => r.json())]).then(([sv, th, fx]) => {
  TH = th;
  const src = document.createElementNS(NS, 'svg');
  src.innerHTML = sv;
  Object.entries(th.actions).forEach(([a, id]) => sym(src, 'ha-' + a, id));
  sym(src, 'hrail', th.rail.item);
  sym(src, 'hload', th.loading);
  const blk = fx.blocks[0], repos = blk.repos.map(r => ({ ...r, blocks: r.blocks.map(b => ({ ...b, rows: b.rows || [] })) }));
  const B = document.body;
  if (MODE === 'screens') {
    B.insertAdjacentHTML('beforeend', header() + BOARD + (Q.get('sheet') === 'look' ? lookSheet() : ''));
    document.documentElement.classList.toggle('sheet', Q.get('sheet') === 'look');
    return;
  }
  document.documentElement.classList.add('scening');
  if (!OLD) B.classList.add('page'), B.insertAdjacentHTML('beforeend', header());
  B.insertAdjacentHTML('beforeend', `<div id=scene class="${lightNow() ? 'night' : ''}"><div class=hpages>${pageOf(repos[0], repos, 0)}</div>` +
    (OLD ? `<button type=button class=hlt aria-label="ranch light"><span>${ICON[LIGHT]}</span></button><button type=button class=hx aria-label="close herd mode"><span>×</span></button>` : '') +
    `<p class=hcap>${hx(blk.weather.line)}</p></div>`);
  wind = blk.weather.wind;
  P = B.querySelector('.hp');
  requestAnimationFrame(() => {
    tiles(P);
    if (T0 != null) frame(T0);
    else { const go = ms => { frame(ms / 1000); requestAnimationFrame(go); }; requestAnimationFrame(go); }
  });
});
