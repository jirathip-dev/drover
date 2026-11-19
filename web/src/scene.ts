import { type Ctx, el } from './blocks';
import { type Blk, blks, obj } from './ops';
import { R as base, T, esc, get, nodes, put, sprite } from './theme';

const NS = 'http://www.w3.org/2000/svg', M64 = (1n << 64n) - 1n, H = document.documentElement;
const noise = (i: number, s = 0) => { const n = (BigInt(i) * 1664525n + BigInt(s) * 1013904223n) & M64; return Number(((n ^ (n >> 13n)) * 1274126177n) % 65536n) / 65535; };
const f1 = (n: number) => +n.toFixed(1);
const WIND = { period: 5.2, canopy: 2.8, grass: 2.1 };
const phase = (t: number) => (t * 2 * Math.PI) / WIND.period;
const anchor = (x: number) => ((x % 390) / 390) * 2 * Math.PI * 0.7;
const sway = (t: number, a: number, amp: number) => amp * (0.74 * Math.sin(phase(t) + a) + 0.26 * Math.sin(1.93 * phase(t) + a * 1.7));
const bend = (t: number, x: number, amp: number) => amp * (0.8 * Math.sin(phase(t) - x * 0.028) + 0.2 * Math.sin(1.9 * phase(t) - x * 0.011));
const ell = (x: number, y: number, rx: number, ry: number, fill: string, op = 1) => `<ellipse cx="${f1(x)}" cy="${f1(y)}" rx="${f1(rx)}" ry="${f1(ry)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
const rect = (x: number, y: number, w: number, h: number, fill: string, op = 1) => `<rect x="${f1(x)}" y="${f1(y)}" width="${f1(w)}" height="${f1(h)}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;

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
  for (const k of [-390, 0, 390]) for (let b = 0; b < 6; b++) s += `<path d="M${k - 60} 220Q${k + 60} 74 ${k + 220} 3Q${k + 275} -20 ${k + 335} -32" stroke-width=${66 - b * 8} />`;
  s = `<g class=mw filter="url(#hmw)">${s}</g>`;
  for (let i = 0; i < 130; i++) for (const k of [-390, 0, 390]) {
    const r = 0.35 + noise(i, 7) * 0.7;
    s += ell(k + i * 3 + noise(i, 8) * 3, noise(i, 3) * 225, r, r, '#e7e5f4', f1((0.3 + 0.45 * noise(i, 9) + 0.08 * Math.sin(i)) * 100) / 100);
  }
  return `<g class=sky-night><g class=cel>${s}</g><g class=moon>${ell(306, 56, 16, 16, '#d3dfeb', 0.84)}${ell(301, 51, 15, 15, '#1d2841')}</g></g>`;
}
function ranch() {
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
  return s + '<rect class=wa width="390" height="320"/>';
}

const D = nodes('<svg class=hdefs aria-hidden=true><defs><linearGradient id=hgr gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#bcc17b"/><stop offset=".5" stop-color="#8e9c58"/><stop offset=1 stop-color="#485e3b"/></linearGradient>' +
  '<linearGradient id=hgrn gradientUnits=userSpaceOnUse x1=0 y1=250 x2=0 y2=640><stop offset=0 stop-color="#63746a"/><stop offset=".5" stop-color="#415d55"/><stop offset=1 stop-color="#273f3e"/></linearGradient>' +
  '<linearGradient id=hwd gradientUnits=userSpaceOnUse x1=0 y1=0 x2=195 y2=500><stop offset=0 stop-color="#ffeeab" stop-opacity=".03"/><stop offset=1 stop-color="#ffeeab" stop-opacity="0"/></linearGradient>' +
  '<linearGradient id=hwn gradientUnits=userSpaceOnUse x1=390 y1=0 x2=195 y2=500><stop offset=0 stop-color="#bbcfe4" stop-opacity=".03"/><stop offset=1 stop-color="#bbcfe4" stop-opacity="0"/></linearGradient>' +
  '<filter id=hmw x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation=6 /></filter></defs></svg>')[0];
const defs = D instanceof Element ? D.firstElementChild ?? D : document.createElementNS(NS, 'defs');
if (D) document.body.prepend(D);

let made: Blk | null = null, pal: string[] = [];
function symbols() {
  if (made === T) return;
  made = T;
  pal.forEach(k => S.style.removeProperty(k));
  pal = Object.keys(obj(obj(T.scene).palette));
  Object.entries(obj(obj(T.scene).palette)).forEach(([k, v]) => S.style.setProperty(k, String(v)));
  defs.querySelectorAll('symbol').forEach(s => s.remove());
  const sym = (id: string, want: unknown) => {
    const g = sprite(want)?.cloneNode(true);
    if (!(g instanceof SVGElement)) return;
    const s = document.createElementNS(NS, 'symbol');
    s.id = id;
    s.setAttribute('viewBox', g.getAttribute('data-box') || '0 0 132 100');
    g.removeAttribute('id');
    s.append(g);
    s.querySelectorAll<SVGElement>('.leg').forEach(l => {
      const a = l.style.getPropertyValue('--a') || '0deg', sign = /leg-(nf|fh)/.test(l.getAttribute('class') ?? '') ? 1 : -1;
      l.setAttribute('style', `transform-box:fill-box;transform-origin:50% 0;transform:rotate(calc(${a} + ${sign} * var(--gait, 0deg)))`);
    });
    s.querySelectorAll('[fill="var(--m)"],[stroke="var(--m)"]').forEach(m => m.setAttribute('style', 'transform-box:fill-box;transform-origin:100% 100%;transform:skewX(var(--mane, 0deg))'));
    defs.append(s);
  };
  Object.entries(obj(T.actions)).forEach(([a, id]) => sym('sa-' + a, id));
  sym('srail', obj(T.rail).item);
  sym('srail-n', obj(T.rail).needs);
  sym('sload', T.loading);
  sym('sfan', obj(T.load_scene).fan);
  sym('sm-fail', obj(obj(T.moments).fail).sprite);
}

const raw = new Map<string, string>();
function backdrop(t: string): string | undefined {
  if (!raw.has(t)) {
    raw.set(t, '');
    void fetch(`${base}themes/${t}/scene.svg`).then(r => (r.ok ? r.text() : '')).catch(() => '').then(s => {
      raw.set(t, /^\s*<svg[\s>]/.test(s) ? s : '');
      if (s && model) draw();
    });
  }
  return raw.get(t);
}

const LIGHTS = ['auto', 'day', 'night'] as const, ICON = { auto: 'A', day: '☀', night: '☾' };
type Light = (typeof LIGHTS)[number];
const lit = (v: unknown): Light | undefined => LIGHTS.find(l => l === v);
let light: Light = lit(new URLSearchParams(location.search).get('light')) ?? lit(get('scene-light')) ?? 'auto';
let at = get('scene-repo') ?? '', model: Blk | null = null, cx: Ctx | null = null, cards: (r: string, all: string[]) => HTMLElement[] = () => [];
let wind = 0, storm = false, on = false, built = '';
let trees: [Element, number, number][] = [], cel: Element[] = [], grass: [Element, number, number][] = [], walk: SVGElement[] = [], cw = 1560;

/** The Scene: the old Herd's ranch, one page per repo, drawn from `/screens/scene`'s `scene` block. */
export const S = el('div');
S.id = 'scene';
S.hidden = true;
S.append(...nodes('<div class=hpages><section class=hp><div class=hs><p><b></b> <span class=n></span></p><p class=sum></p></div><div class=hn></div>' +
  `<div class=hr><div class=hc>${clouds()}</div><svg class=rsn aria-hidden=true></svg><div class=fog></div><button type=button class="hnv hpv" aria-label="previous repo"><span>‹</span></button><button type=button class="hnv hnx" aria-label="next repo"><span>›</span></button></div>` +
  '<p class=hdt></p><div class=hb></div></section></div><div class=hrain></div><div class=hflash></div><p class=hcap></p><div class=hq hidden></div>'));
document.body.append(S);
const q = <E extends Element = HTMLElement>(s: string): E => { const e = S.querySelector<E>(s); if (!e) throw new Error(`no ${s}`); return e; };
const P = q('.hp'), HR = q('.hr'), HB = q('.hb'), HQ = q('.hq'), rain = q('.hrain'), flash = q('.hflash'), lb = document.getElementById('hlt');
const still = matchMedia('(prefers-reduced-motion: reduce)');
const words = (k: string) => String(obj(T.words)[k] ?? k);
const indoor = () => ['heat', 'light'].includes(String(obj(T.load_scene).kind));
const nightNow = () => !indoor() && (light === 'night' || (light === 'auto' && (h => h < 7 || h >= 19)(new Date().getHours())));
const hash = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
const look = (r: Blk) => {
  const L = blks(T.looks);
  return L.length ? Object.entries(L[hash(String(r.title)) % L.length]).map(e => e.join(':')).join(';') : String(obj(r.figure).coat ?? '');
};
const has = (id: string) => !!document.getElementById(id);

function horse(r: Blk): string {
  const f = obj(r.figure), st = String(f.state), a = String(f.act ?? 'idle'), fail = st === 'done' && /\bFAIL\b/.test(String(r.subtitle));
  const use = fail && has('sm-fail') ? 'sm-fail' : a === 'idle' && S.classList.contains('heat-held') && has('sfan') ? 'sfan' : 'sa-' + a;
  const said = (st === 'working' && obj(T.words)[a]) || r.subtitle, g = String(obj(T.glyphs)[fail ? 'blocked' : st] ?? '?');
  const mark = [g === '·' ? '' : g, said].filter(Boolean).join(' '), tail = [r.value, r.host].filter(Boolean).map(x => ' · ' + esc(x)).join('');
  return `<button type=button class="hh ${esc(st)}" data-k="${esc(r.id)}"><svg viewBox="0 0 132 100" style="${esc(look(r))}"><use href="#${use}"/></svg>` +
    `<span class=hl><b>${esc(r.title)}</b><span title="${esc([mark, r.value, r.host].filter(Boolean).join(' · '))}"><i class=${esc(fail ? 'r' : r.tone ?? 'm')}>${esc(mark)}</i>${tail}</span></span></button>`;
}
function bale(r: Blk): string {
  const n = obj(r.figure).state === 'needs';
  return `<button type=button class="hqh${n ? ' n' : ''}" data-k="${esc(r.id)}"><svg viewBox="0 0 132 100"><use href="#srail${n ? '-n' : ''}"/></svg>` +
    `<span>${esc(r.title)}${n ? '<br><i class=r>! needs you</i>' : r.blocked_by ? `<br><i class=r>⛔#${esc(r.blocked_by)}</i>` : ''}</span></button>`;
}
function band(s: Blk): string {
  const h = String(s.header), cls = h.split('/')[0], gate = cls === 'queued', rows = blks(s.rows);
  return `<div class="band b-${esc(cls)}"><h4>${esc(words(h))} <span>${rows.length}</span></h4><div class=${gate ? 'gate' : 'field'}>${rows.map(gate ? bale : horse).join('')}</div></div>`;
}

function tiles(i: number, all: Blk[]) {
  const t = H.dataset.theme ?? 'farm', svg = backdrop(t) ?? '', v = /viewBox="0 0 ([\d.]+) ([\d.]+)"/.exec(svg), [vw, vh] = v ? [+v[1], +v[2]] : [390, 320];
  const y0 = v ? 0 : 130, h = vh - y0;
  HR.style.setProperty('--ar', String(f1((h / vw) * 1000) / 1000));
  if (!HR.clientWidth || !HR.clientHeight) return;
  const n = 2 * Math.ceil(((HR.clientWidth * h) / vw / HR.clientHeight - 1) / 2) + 1, mid = (n - 1) / 2, line = !!obj(T.scene).line, key = `${t}:${n}:${!!v}:${line ? i : ''}`;
  const z = HR.clientHeight / 320, c = q('.hc');
  cw = Math.max(1560 * z, HR.clientWidth + 780 * z);
  Object.assign(c.style, { width: `${cw}px`, height: `${320 * z}px` });
  if (key === built) return;
  built = key;
  const body = v ? svg.replace(/^[\s\S]*?<svg[^>]*>|<\/svg>\s*$/g, '') : ranch();
  let s = '';
  for (let k = -mid; k <= mid; k++) {
    const f = line && all.length ? all[(((i + k) % all.length) + all.length) % all.length] : null;
    s += `<svg x="${k * vw}" width="${vw}" height="${vh}" viewBox="0 0 ${vw} ${vh}"${k ? '' : ' class=t0'}${f ? ` style="--fk:var(--${esc(f.color)})"` : ''}>` +
      (v || k % 2 === 0 ? body : `<g transform="matrix(-1 0 0 1 ${vw} 0)">${body}</g>`) + '</svg>';
  }
  const r = nodes(`<svg class=rsn viewBox="${-mid * vw} ${y0} ${n * vw} ${h}" preserveAspectRatio="xMidYMin slice"${v ? ' shape-rendering=crispEdges' : ''} aria-hidden=true>${s}</svg>`)[0];
  if (r) q('.rsn').replaceWith(r);
  trees = [...HR.querySelectorAll<SVGElement>('.cn')].map(g => [g, +(g.dataset.a ?? 0), +(g.dataset.l ?? 0)]);
  cel = [...HR.querySelectorAll('.cel')];
  grass = [...HR.querySelectorAll<SVGElement>('.gt')].map(g => [g, +(g.dataset.x ?? 0), +(g.dataset.y ?? 0)]);
}

function weather(b: Blk) {
  const w = obj(b.weather), wx = obj(T.scene).weather !== false, kind = String(obj(T.load_scene).kind ?? ''), lv = String(b.load ?? ''), fog = Number(w.fog ?? 0);
  wind = Math.min(1, Math.max(0, Number(w.wind ?? 0)));
  storm = wx && !!w.storm;
  const cls: [string, boolean][] = [['storm', storm], ['fog1', wx && fog === 1], ['fog2', wx && fog === 2], ['gust', wind >= 0.5], ['night', nightNow()]];
  for (const k of ['heat', 'light', 'sign']) {
    cls.push([k, k === kind && !!lv]);
    for (const l of ['calm', 'busy', 'held']) cls.push([`${k}-${l}`, k === kind && l === lv]);
  }
  cls.forEach(([k, v]) => S.classList.toggle(k, v));
  if (!storm || still.matches) S.style.setProperty('--mane', '0deg');
  q('.hcap').textContent = String(w.line ?? '');
}

/** Draws the Scene from `/screens/scene` (`m`; null: the loading horse), the repo's needs-you cards from `needs`. */
export function scene(m: Blk | null, c: Ctx, needs: (repo: string, all: string[]) => HTMLElement[]) {
  [model, cx, cards] = [m, c, needs];
  draw();
}
function draw() {
  symbols();
  const b = blks(model?.blocks).find(x => x.type === 'scene') ?? {}, all = blks(b.repos), i = Math.max(0, all.findIndex(r => r.id === at)), r = all[i] ?? {};
  weather(b);
  lamp();
  S.classList.toggle('one', all.length < 2);
  if (typeof r.color === 'string' && r.color) P.style.setProperty('--fk', `var(--${r.color})`);
  const secs = blks(r.blocks), lanes = secs.filter(s => s.header !== 'queued').reduce((n, s) => n + blks(s.rows).length, 0);
  q('.hs b').textContent = String(r.title ?? '');
  q('.hs .n').textContent = model ? `· ${lanes} here` : '';
  q('.sum').textContent = String(r.subtitle ?? '');
  q('.hn').replaceChildren(...(model ? cards(String(r.id ?? ''), all.map(x => String(x.id))) : []));
  q('.hdt').replaceChildren(...all.map((x, k) => {
    const d = el('button');
    d.type = 'button';
    d.setAttribute('aria-label', String(x.title));
    d.toggleAttribute('aria-current', k === i);
    d.onclick = () => go(k);
    return d;
  }));
  const y = P.scrollTop;
  HB.replaceChildren(...nodes(!model ? `<div class=ldg><svg viewBox="0 0 132 100" style="--c:#c89a56;--d:#a37a3c;--m:#e8dcc0;--b:#efe7dc"><use href="#sload"/></svg><span>${esc(words('loading'))}…</span></div>`
    : secs.length ? secs.map(band).join('') : `<p class=empty>no ${esc(words('horses'))} here</p>`));
  P.scrollTop = y;
  HB.style.setProperty('--cw', `${Math.min(520, Math.max(320, ...[...HB.querySelectorAll<HTMLElement>('.hl>*')].map(x => x.scrollWidth + 20)))}px`);
  walk = [...S.querySelectorAll<SVGElement>('.hh.working > svg, .ldg > svg')];
  tiles(i, all);
  anim();
}

function go(k: number) {
  const all = blks(blks(model?.blocks).find(x => x.type === 'scene')?.repos), n = all.length;
  if (!n) return;
  put('scene-repo', (at = String(all[((k % n) + n) % n].id)));
  P.scrollTop = 0;
  draw();
}
const here = () => Math.max(0, blks(blks(model?.blocks).find(x => x.type === 'scene')?.repos).findIndex(r => r.id === at));

function lamp() {
  const n = nightNow();
  S.classList.toggle('night', n);
  if (!lb) return;
  lb.hidden = !on || indoor();
  lb.replaceChildren(el('span', '', ICON[light]));
  lb.setAttribute('aria-label', `${words('ranch')} light: ${light}${light === 'auto' ? ` (${n ? 'night' : 'day'} now)` : ''}`);
}
if (lb) lb.onclick = () => { light = LIGHTS[(LIGHTS.indexOf(light) + 1) % 3]; put('scene-light', light); lamp(); };
setInterval(() => { if (!document.hidden) lamp(); }, 60_000);

let raf = 0, last = 0, drift = 0, celx = 0, flashAt = -99, nextFlash = 0;
function frame(ms: number) {
  raf = 0;
  if (!on || document.hidden || still.matches) return;
  const t = ms / 1000, dt = Math.min(0.1, t - (last || t)), m = 1 + 2 * wind;
  last = t;
  drift = (drift + dt * 2.4 * m) % 780;
  celx = (celx + dt * 0.7) % 390;
  for (const [g, a, l] of trees) g.setAttribute('transform', `translate(${f1(sway(t, a, WIND.canopy * l * m))} ${f1(sway(t, a + 1.9, 0.5 * l * m))})`);
  for (const [g, x, y] of grass) { const b = bend(t, x, WIND.grass * m) / 10; g.setAttribute('transform', `matrix(1 0 ${-b.toFixed(3)} 1 ${(b * y).toFixed(2)} 0)`); }
  if (S.classList.contains('night')) cel.forEach(c => c.setAttribute('transform', `translate(${celx.toFixed(2)} 0)`));
  q('.hc').style.transform = `translateX(${(((drift - 780) / 1560) * cw).toFixed(1)}px)`;
  walk.forEach((s, i) => {
    const g = Math.sin((t * Math.PI) / 0.4 + i * 1.3);
    s.style.setProperty('--gait', `${(g * 16).toFixed(1)}deg`);
    s.style.transform = `translate(${(Math.sin(t * 0.5 + i * 2.1) * 10).toFixed(1)}px, ${(-Math.abs(g) * 1.5).toFixed(1)}px)`;
  });
  if (storm) {
    S.style.setProperty('--mane', `${(-12 - 6 * Math.sin(t * 3.1)).toFixed(1)}deg`);
    rain.style.transform = `translate(${(-(t * 90) % 60).toFixed(1)}px, ${((t * 520) % 60).toFixed(1)}px)`;
    if (t >= nextFlash) { flashAt = t; nextFlash = t + 20 + Math.random() * 25; }
    flash.style.opacity = Math.max(0, 0.7 - (t - flashAt) * 3.5).toFixed(2);
  }
  raf = requestAnimationFrame(frame);
}
function anim() {
  if (raf || !on || document.hidden || still.matches) return;
  last = 0;
  nextFlash = performance.now() / 1000 + 4;
  raf = requestAnimationFrame(frame);
}
document.addEventListener('visibilitychange', () => { anim(); lamp(); });
addEventListener('resize', () => { if (on) tiles(here(), blks(blks(model?.blocks).find(x => x.type === 'scene')?.repos)); });

/** Shows or hides the Scene (the header toggle); the frame loop runs only while it shows. */
export function sceneOn(v: boolean) {
  on = v;
  S.hidden = !v;
  HQ.hidden = true;
  if (v) draw();
  else lamp();
}

const card = (n: unknown) => {
  const c = [...S.querySelectorAll<HTMLElement>('.hn [data-id]')].find(k => (k.textContent ?? '').includes(`#${String(n)} `));
  if (c) { c.scrollIntoView({ block: 'nearest' }); c.animate([{ outline: '2px solid var(--r)' }, { outline: 'none' }], 1500); }
  return !!c;
};
const row = (k: string | undefined) => blks(blks(model?.blocks).find(x => x.type === 'scene')?.repos).flatMap(r => blks(r.blocks)).flatMap(s => blks(s.rows)).find(r => r.id === k);
let swipe: { x: number; id: number } | null = null, swiped = false, wheelAt = 0, wheelX = 0, wheelDone = false;
S.addEventListener('click', e => {
  const t = e.target instanceof Element ? e.target : null, h = t?.closest<HTMLElement>('.hh'), b = t?.closest<HTMLElement>('.hqh'), r = row((h ?? b)?.dataset.k);
  if (swiped) { swiped = false; e.stopPropagation(); return; }
  if (t?.closest('.hnv')) return go(here() + (t.closest('.hnx') ? 1 : -1));
  if (t?.closest('.hq')) { HQ.hidden = true; return; }
  if (!r) return;
  if (h) { if (!(r.needs && card(r.needs)) && r.tap) cx?.tap(r.tap); return; }
  if (obj(r.figure).state === 'needs' && card(String(r.title).slice(1))) return;
  HQ.replaceChildren(...nodes(`<p><b>${esc(r.title)}</b> queued</p>${r.subtitle ? `<p${r.blocked_by ? ' class=r' : ''}>${esc(r.subtitle)}</p>` : ''}<p class=m>tap to close</p>`));
  HQ.hidden = false;
});
document.addEventListener('keydown', e => {
  if (!on || H.classList.contains('sheet') || e.altKey || e.ctrlKey || e.metaKey || (e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable]'))) return;
  const k = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0;
  if (k) { e.preventDefault(); go(here() + k); }
});
P.addEventListener('wheel', e => {
  const x = e.deltaX || (e.shiftKey ? e.deltaY : 0), y = e.deltaX ? e.deltaY : e.shiftKey ? 0 : e.deltaY;
  if (Math.abs(x) <= Math.abs(y)) return;
  e.preventDefault();
  if (e.timeStamp - wheelAt > 300) [wheelX, wheelDone] = [0, false];
  wheelAt = e.timeStamp;
  wheelX += x;
  if (!wheelDone && Math.abs(wheelX) > 40) { wheelDone = true; go(here() + Math.sign(wheelX)); }
}, { passive: false });
P.addEventListener('pointerdown', e => { swipe = e.button === 0 && !(e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable]')) ? { x: e.clientX, id: e.pointerId } : null; swiped = false; });
P.addEventListener('pointermove', e => {
  if (!swipe || e.pointerId !== swipe.id) return;
  const dx = Math.abs(e.clientX - swipe.x);
  if (dx >= 8 && !S.classList.contains('drag')) { S.classList.add('drag'); getSelection()?.removeAllRanges(); }
  if (dx > 40 && !P.hasPointerCapture(e.pointerId)) try { P.setPointerCapture(e.pointerId); } catch { return; }
});
P.addEventListener('pointerup', e => {
  const dx = swipe && e.pointerId === swipe.id ? e.clientX - swipe.x : 0;
  swipe = null;
  S.classList.remove('drag');
  if (Math.abs(dx) > 40) { swiped = true; go(here() - Math.sign(dx)); }
});
P.addEventListener('pointercancel', () => { swipe = null; S.classList.remove('drag'); });
