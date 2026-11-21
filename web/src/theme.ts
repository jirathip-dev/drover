import { type Blk, blks, isBlk, obj } from './ops';

declare const SD: { t: Record<string, unknown>; s: Record<string, Scheme> };
type Scheme = { tokens: Record<string, string>; 'color-scheme': string };
const css = (s: Scheme): string => `:root{${Object.entries(s.tokens).map(([k, v]) => `${k}:${v}`).join(';')};color-scheme:${s['color-scheme']}}`;
const theirs = (def: unknown, dark: boolean): unknown => (def && typeof def === 'object' && Reflect.get(def, dark ? 'dark' : 'light')) || def;
const choose = (pick: unknown, def: unknown, dark: boolean): string =>
  [pick, theirs(def, dark)].find((n): n is string => typeof n === 'string' && Object.prototype.hasOwnProperty.call(SD.s, n)) ?? 'github-dark';
const NS = 'http://www.w3.org/2000/svg';
const H = document.documentElement;
export const R = new URL('.', document.baseURI).href;
export const get = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
const set = (k: string, v: string | null) => (v == null ? localStorage.removeItem(k) : localStorage.setItem(k, v));
/** Stores `v` under `k` (null removes it); a full storage drops the cached screens first, so a draft is never lost. */
export const put = (k: string, v: string | null) => {
  try { set(k, v); } catch {
    try { Object.keys(localStorage).forEach(x => { if (x.startsWith('scr:') && x !== k) localStorage.removeItem(x); }); set(k, v); } catch {}
  }
};

export let T: Blk = {};
const defs = document.createElementNS(NS, 'svg');
defs.setAttribute('class', 'hdefs');
defs.setAttribute('aria-hidden', 'true');
document.body.prepend(defs);

const COATS: Record<string, string> = {
  bay: '--c:#8a5a33;--d:#6f4527;--m:#2e2019;--b:transparent',
  blk: '--c:#3b3b44;--d:#2a2a31;--m:#17161a;--b:transparent',
  ch: '--c:#a05c2c;--d:#7d4520;--m:#5d3317;--b:#efe7dc',
  gry: '--c:#b9bcc6;--d:#989ca9;--m:#7d8089;--b:transparent',
  wht: '--c:#c89a56;--d:#a37a3c;--m:#e8dcc0;--b:#efe7dc',
};

export const STYLES = ['terminal', 'ascii', 'pixel'];
const styled = (s: string | null) => (s && STYLES.includes(s) ? s : 'pixel');
H.dataset.style = styled(new URLSearchParams(location.search).get('style') || get('style'));
const st = () => H.dataset.style ?? 'pixel';
const has = (id: string) => !!document.getElementById(id);
const box = (id: string) => document.getElementById(id)?.getAttribute('viewBox') ?? '0 0 132 100';
/** This phone's style (terminal, ascii or pixel), kept; the loading strip repaints now, the rows on the caller's redraw. */
export function style(s: string) {
  put('style', s);
  H.dataset.style = styled(s);
  strip();
}

function act(f: Blk): string {
  if (typeof f.act === 'string' && f.act) return f.act;
  const s = String(f.state);
  return s === 'working' ? 'impl' : s === 'needs' || s === 'fail' ? 'blocked' : s === 'done' || s === 'merged' ? 'done' : 'idle';
}
const GLYPH: Record<string, string> = { working: '●', needs: '!', fail: '✗', done: '✓', merged: '✓', idle: '○' };
const WORD: Record<string, string> = { working: 'working', needs: 'needs you', fail: 'failed', done: 'done', merged: 'merged', idle: 'idle' };
const TONE: Record<string, string> = { working: 'g', needs: 'r', fail: 'r', done: 'mute', idle: 'mute', merged: 'g' };
const MK: Record<string, string> = { merged: 'merged', fail: 'fail', 'needs-you': 'needs' };
const decl = (o: Blk) => Object.entries(o).filter(e => e[0] !== 'id').map(e => e.join(':')).join(';');
const mo = (f: Blk): string => (typeof f.moment === 'string' ? f.moment : f.state === 'needs' ? 'needs-you' : f.state === 'fail' ? 'fail' : '');

function plant(stage: unknown, n: number, row: boolean) {
  const id = obj(T.stages)[String(stage)], L = blks(T.species), s = L.length && n >= 0 ? L[n % L.length] : null, x = row ? '-row' : '';
  if (typeof id !== 'string') return null;
  return { ids: [s ? `${id}-${String(s.id)}${x}` : '', id + x].filter(Boolean), style: s ? decl(s) : '' };
}

function avatar(f: Blk, a: string, s: string, g: string, word: string): string {
  const frames = st() === 'ascii' ? obj(T.ascii)[a] : null;
  if (typeof frames === 'string') {
    const fs = [...new Set([frames, s === 'working' ? frames.replace(/[^\n]*$/, l => l.replace(/[/\\]/g, c => (c === '/' ? '\\' : '/'))) : frames])];
    return `<span class="av ${esc(s)}" role=img aria-label="${esc(word)}" title="${esc(word)}"><i class="asc${fs.length > 1 ? ' run' : ''}">${fs.map(x => `<b>${esc(x)}</b>`).join('')}</i></span>`;
  }
  if (st() !== 'pixel' || !has('ha-' + a)) return `<span class="av ${esc(s)}" role=img aria-label="${esc(word)}" title="${esc(word)}">${g}</span>`;
  const looks = blks(T.looks), coat = String(f.coat);
  const look = looks.length ? decl(looks[Math.max(Object.keys(COATS).indexOf(coat), 0) % looks.length]) : COATS[coat] ?? COATS.bay;
  const p = plant(f.grow, Number(f.issue ?? -1), false), under = p?.ids.find(i => has('hs-' + i));
  const art = (p && under ? `<use href="#hs-${under}" style="${p.style}"/>` : '') + `<use href="#ha-${a}"/>`;
  return `<span class="av ${esc(s)}" role=img aria-label="${esc(word)}" title="${esc(word)}"><svg class=px viewBox="0 0 132 100" style="${look}">${art}</svg></span>`;
}

/** A figure as HTML by this phone's style: terminal its state glyph; ascii the theme's figure; pixel its sprite (a Queue row's plant at its stage, a lane's look over its issue's plant). */
export function figure(f: Blk): string {
  const a = act(f), s = String(f.state), word = WORD[s] ?? String(f.state);
  const g = `<i class="gl ${TONE[s] ?? 'mute'}">${GLYPH[s] ?? '·'}</i>`;
  if ('coat' in f) return avatar(f, a, s, g, word);
  const span = (inner: string) => `<span class="rsg ${esc(s)}" role=img aria-label="${esc(word)}" title="${esc(word)}">${inner}</span>`;
  const row = st() === 'ascii' ? obj(T.ascii_rows)[a] : null;
  if (typeof row === 'string') return span(`<i class="rsp rsa" aria-hidden=true>${esc(row)}</i>`);
  if (st() !== 'pixel') return span(g);
  const k = MK[String(f.moment ?? '')] ?? (s === 'needs' ? 'needs' : ''), p = f.grow ? plant(f.grow, Number(f.issue ?? -1), true) : null;
  const plants = p ? p.ids.map(i => 'hq-' + i) : [];
  const id = [k ? 'hr-' + k : '', ...plants, f.grow === 'queued' ? 'hr-queued' : '', 'hr-' + a].find(i => i && has(i));
  if (!id) return span(g);
  return span(`<svg class=rsp viewBox="${box(id)}" aria-hidden=true><use href="#${id}"${p && plants.includes(id) ? ` style="${p.style}"` : ''}/></svg>`);
}

/** A Host row's load (calm, busy, held) by this phone's style: the theme's miniature (pixel) or ascii frame and its level word; nothing in terminal. */
export function load(l: string): string {
  const w = String(obj(obj(T.load_scene).levels)[l] ?? l), f = st() === 'ascii' ? obj(T.ascii_load)[l] : null;
  const tail = `<b class=thw>${esc(w)}</b><small class=pw> · ${esc(l)}</small></span>`, head = `<span class=lsm role=img aria-label="load: ${esc(w)} (${esc(l)})">`;
  if (typeof f === 'string') return `${head}<i class=lsa aria-hidden=true>${esc(f)}</i>${tail}`;
  if (st() !== 'pixel' || !has('hl-' + l)) return '';
  const [, , bw, bh] = box('hl-' + l).split(' ');
  return `${head}<svg viewBox="0 0 ${bw} ${bh}"${bw === '48' ? '' : ` style="width:${bw}px;height:${bh}px"`} aria-hidden=true><use href="#hl-${l}"/></svg>${tail}`;
}

/** The loading strip by this phone's style: the theme's sprite (pixel), its ascii frames (ascii), else the spinning glyph. */
export function strip() {
  const ld = document.getElementById('ld'), fs = String(T.ascii_loading ?? '').split('\n').filter(Boolean);
  if (!ld) return;
  const art = st() === 'pixel' && has('hload') ? `<svg class=lds viewBox="${box('hload')}" style="${COATS.wht}" aria-hidden=true><use href="#hload"/></svg>`
    : st() === 'ascii' && fs.length ? `<span class="asc lda${fs.length > 1 ? ' run' : ''}" aria-hidden=true>${fs.map(f => `<b>${esc(f)}</b>`).join('')}</span>` : '<i>↻</i>';
  ld.replaceChildren(...nodes(`${art} loading`));
}

const PLAYING = new Map<string, { k: string; t0: number }>();
const rowOf = (root: Element, id: string) => root.querySelector<HTMLElement>(`.row[data-id="${CSS.escape(id)}"]`);
function play(r: HTMLElement, k: string, t0: number) {
  r.dataset.mo = k;
  r.style.setProperty('--mo-d', `${t0 - Date.now()}ms`);
  const w = obj(T.words)[MK[k]];
  if (!r.querySelector('.mot') && typeof w === 'string') r.append(...nodes(`<i class=mot>${esc(w)}${k === 'merged' ? '!' : ''}</i>`));
}
function overlay(k: string) {
  let o = document.getElementById('mo');
  if (!o) document.body.append((o = Object.assign(document.createElement('i'), { id: 'mo' })));
  const m = o;
  m.className = 'mo-' + String(obj(obj(T.moments)[k]).anim ?? 'hop');
  m.replaceChildren(...nodes(`<svg viewBox="${box('hm-' + k)}" style="${COATS.wht}" aria-hidden=true><use href="#hm-${k}"/></svg>`));
  m.hidden = false;
  clearTimeout(Number(m.dataset.t));
  m.dataset.t = String(setTimeout(() => { m.hidden = true; }, 2400));
}
/** Plays the theme's moment (2.4 s: merged hops, FAIL shakes, needs you pulses) on each row of `now` that arrived with one since `was`, on its row when shown, else the #mo mark; a rebuilt row resumes it. */
export function moments(root: HTMLElement, was: Map<string, string> | null, now: Map<string, string>) {
  if (was && st() !== 'terminal') now.forEach((j, id) => {
    const k = mo(obj(obj(JSON.parse(j)).figure)), b = was.get(id);
    if (!k || (b != null && mo(obj(obj(JSON.parse(b)).figure)) === k) || !has('hm-' + k)) return;
    const t0 = Date.now(), r = rowOf(root, id);
    if (!r || !r.offsetParent) return overlay(k);
    PLAYING.set(id, { k, t0 });
    setTimeout(() => {
      PLAYING.delete(id);
      const x = rowOf(root, id);
      if (!x) return;
      delete x.dataset.mo;
      x.querySelector('.mot')?.remove();
    }, 2400);
  });
  PLAYING.forEach((m, id) => { const r = rowOf(root, id); if (r && r.dataset.mo !== m.k) play(r, m.k, m.t0); });
}

/** A theme word dressing a plain one (`at the gate · needs you`), else the plain word. */
export function word(k: string, plain: unknown): string {
  const w = obj(T.words)[k];
  return w ? `<b class=thw>${esc(w)}</b><small class=pw> · ${esc(plain)}</small>` : esc(plain);
}

export const esc = (s: unknown) => String(s ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
export const nodes = (h: string) => new DOMParser().parseFromString('<body>' + h, 'text/html').body.childNodes;
let srcs: Element[] = [];
const pick = (src: Element | undefined, id: unknown) => (src && id ? src.querySelector('#' + CSS.escape(String(id))) : null);
/** The theme's sprite `want` (farm's where the theme lacks it), else farm's `alt`. */
export const sprite = (want: unknown, alt?: unknown): Element | null => pick(srcs[0], want) || pick(srcs[1], want) || pick(srcs[1], alt);
const got = (u: string, j: boolean): Promise<unknown> => fetch(u).then(r => (r.ok ? (j ? r.json() : r.text()) : Promise.reject())).catch(() => (j ? {} : ''));

/** Loads theme `name` (farm filling what it lacks) and builds the `ha-*` and `hr-*` symbols. */
export async function theme(name: string): Promise<void> {
  const d = (n: string) => `${R}themes/${n}/`;
  const [fs, fj, ts, tj] = await Promise.all([got(d('farm') + 'sprites.svg', false), got(d('farm') + 'theme.json', true), got(d(name) + 'sprites.svg', false), got(d(name) + 'theme.json', true)]);
  if (H.dataset.theme !== name) return;
  const svg = (t: unknown) => new DOMParser().parseFromString(`<svg>${t}</svg>`, 'text/html').body.firstElementChild ?? document.createElementNS(NS, 'svg'), farm = svg(fs), own = svg(ts);
  T = obj(tj);
  srcs = [own, farm];
  defs.replaceChildren();
  const sym = (id: string, want: unknown, alt?: unknown, vb = '0 0 132 100') => {
    const g = sprite(want, alt)?.cloneNode(true);
    if (!(g instanceof Element)) return;
    const s = document.createElementNS(NS, 'symbol');
    s.id = id;
    s.setAttribute('viewBox', g.getAttribute('data-box') || vb);
    g.removeAttribute('id');
    s.append(g);
    s.querySelectorAll('.leg').forEach(l => {
      const a = l.getAttribute('style')?.match(/--a:\s*([^;]+)/)?.[1] ?? '0deg', sign = /leg-(nf|fh)/.test(l.getAttribute('class') ?? '') ? 1 : -1;
      l.setAttribute('style', `transform-box:fill-box;transform-origin:50% 0;transform:rotate(calc(${a} + ${sign} * var(--gait, 0deg)))`);
    });
    defs.append(s);
  };
  const fa = obj(obj(fj).actions), rs = isBlk(T.row_sprites) ? T.row_sprites : null;
  Object.entries(obj(T.actions)).forEach(([a, id]) => {
    sym('ha-' + a, id, fa[a]);
    if (rs) sym('hr-' + a, rs[a], fa[a]);
  });
  if (rs) ['queued', 'needs', 'merged', 'fail'].forEach(k => sym('hr-' + k, rs[k]));
  const ls = obj(T.load_scene);
  ['calm', 'busy', 'held'].forEach(l => sym('hl-' + l, obj(ls.minis)[l], null, '0 0 48 16'));
  Object.entries(obj(T.moments)).forEach(([k, m]) => sym('hm-' + k, obj(m).sprite));
  Object.values(obj(T.stages)).forEach(id => [...blks(T.species).map(x => `${String(id)}-${String(x.id)}`), String(id)].forEach(i => { sym('hs-' + i, i); sym(`hq-${i}-row`, i + '-row'); }));
  sym('hload', T.loading);
  sym('hfan', ls.fan);
  strip();
}

const DARK = matchMedia('(prefers-color-scheme: dark)');
/** The colours: this phone's pick, else its theme's default; kept so the head script paints it first next time. */
export function colours() {
  const n = choose(new URLSearchParams(location.search).get('scheme') || get('scheme'), SD.t[H.dataset.theme ?? ''], DARK.matches), b = css(SD.s[n]);
  H.dataset.scheme = n;
  const sch = document.getElementById('sch'), meta = document.querySelector<HTMLMetaElement>('meta[name=theme-color]');
  if (sch) sch.textContent = b;
  if (meta) meta.content = SD.s[n].tokens['--bg'];
  put('sb', b);
}
DARK.addEventListener('change', colours);
export const themes = () => Object.keys(SD.t);
export const schemes = () => Object.keys(SD.s);
