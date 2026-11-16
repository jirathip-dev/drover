// #927: the theme pack (docs/themes.md: `/themes/<name>/theme.json`, resolved, and `sprites.svg`) and the colour
// schemes. A block's `figure` ({state, tool, coat}) is the theme's sprite: a lane's avatar (with a coat) or a row's.
import type { Blk } from './ops';
import { block, choose } from './schemes';

declare const SD: { t: Record<string, unknown>; s: Record<string, Blk> };
const NS = 'http://www.w3.org/2000/svg';
const H = document.documentElement;
export const R = new URL('.', document.baseURI).href;
export const get = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
export const put = (k: string, v: string | null) => { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch {} };

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

/** The theme's action for a figure: a working lane by its tool (run impl, edit fix, read rev), else its state. */
export function act(f: Blk, lane: boolean): string {
  if (f.state === 'working') return ({ edit: 'fix', read: 'rev' } as Blk)[f.tool] ?? 'impl';
  if (f.state === 'needs' || f.state === 'fail') return lane ? 'blocked' : f.state;
  return ['done', 'merged'].includes(f.state) ? f.state : 'idle';
}
const GLYPH: Blk = { working: '●', needs: '!', fail: '✗', done: '✓', merged: '✓', idle: '○' };
const WORD: Blk = { working: 'working', needs: 'needs you', fail: 'failed', done: 'done', merged: 'merged', idle: 'idle' };

/** A figure as HTML: the theme's sprite (24 px row, its stage's plant where the theme has `stages`, or a lane's avatar in its coat), else its state glyph. */
export function figure(f: Blk): string {
  const lane = 'coat' in f, a = act(f, lane), word = WORD[f.state] ?? f.state, has = (id: string) => !!document.getElementById(id);
  const tone = ({ working: 'g', needs: 'r', fail: 'r', done: 'mute', idle: 'mute', merged: 'g' } as Blk)[f.state] ?? 'mute';
  const g = `<i class="gl ${tone}">${GLYPH[f.state] ?? '·'}</i>`;
  if (lane) {
    const looks: Blk[] = T.looks ?? [], k = Object.keys(COATS).indexOf(f.coat);
    const style = looks.length ? Object.entries(looks[Math.max(k, 0) % looks.length]).map(e => e.join(':')).join(';') : COATS[f.coat] ?? COATS.bay;
    const art = has('ha-' + a) ? `<svg class=px viewBox="0 0 132 100" style="${style}"><use href="#ha-${a}"/></svg>` : g;
    return `<span class="av ${f.state}" role=img aria-label="${word}" title="${word}">${art}</span>`;
  }
  const id = has('hg-' + f.grow) ? 'hg-' + f.grow : has('hr-' + a) ? 'hr-' + a : '';
  const box = id && document.getElementById(id)!.getAttribute('viewBox');
  return `<span class="rsg ${f.state}" role=img aria-label="${word}" title="${word}">${id ? `<svg class=rsp viewBox="${box}" aria-hidden=true><use href="#${id}"/></svg>` : g}</span>`;
}

/** A theme word dressing a plain one (`at the gate · needs you`), else the plain word. */
export function word(k: string, plain: string): string {
  const w = (T.words ?? {})[k];
  return w ? `<b class=thw>${esc(w)}</b><small class=pw> · ${esc(plain)}</small>` : esc(plain);
}

export const esc = (s: unknown) => String(s ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const got = (u: string, j: boolean) => fetch(u).then(r => (r.ok ? (j ? r.json() : r.text()) : Promise.reject())).catch(() => (j ? {} : ''));

/** Loads theme `name` (farm filling what it lacks, as herd.js did) and builds the `ha-*` and `hr-*` symbols. */
export async function theme(name: string): Promise<void> {
  const d = (n: string) => `${R}themes/${n}/`;
  const [fs, fj, ts, tj] = await Promise.all([got(d('farm') + 'sprites.svg', false), got(d('farm') + 'theme.json', true), got(d(name) + 'sprites.svg', false), got(d(name) + 'theme.json', true)]);
  if (H.dataset.theme !== name) return;
  const svg = (t: string) => { const e = document.createElementNS(NS, 'svg'); e.innerHTML = t; return e; }, farm = svg(fs), own = svg(ts);
  T = tj as Blk;
  defs.replaceChildren();
  const sprite = (src: Element, id?: string) => (id ? src.querySelector('#' + CSS.escape(id)) : null);
  const sym = (id: string, want?: string, alt?: string) => {
    const g = (sprite(own, want) || sprite(farm, want) || sprite(farm, alt))?.cloneNode(true) as Element | undefined;
    if (!g) return;
    const s = document.createElementNS(NS, 'symbol');
    s.id = id;
    s.setAttribute('viewBox', g.getAttribute('data-box') || '0 0 132 100');
    g.removeAttribute('id');
    s.append(g);
    defs.append(s);
  };
  const fa = (fj as Blk).actions ?? {};
  Object.entries((T.actions ?? {}) as Record<string, string>).forEach(([a, id]) => {
    sym('ha-' + a, id, fa[a]);
    if (T.row_sprites) sym('hr-' + a, T.row_sprites[a], fa[a]);
  });
  if (T.row_sprites) ['queued', 'needs', 'merged', 'fail'].forEach(k => sym('hr-' + k, T.row_sprites[k]));
  Object.entries((T.stages ?? {}) as Record<string, string>).forEach(([k, id]) => sym('hg-' + k, id));
}

const DARK = matchMedia('(prefers-color-scheme: dark)');
/** The colours: this phone's pick, else its theme's default; kept so the head script paints it first next time. */
export function colours() {
  const n = choose(SD.s, new URLSearchParams(location.search).get('scheme') || get('scheme'), SD.t[H.dataset.theme ?? ''], DARK.matches), b = block(SD.s[n]);
  H.dataset.scheme = n;
  document.getElementById('sch')!.textContent = b;
  document.querySelector<HTMLMetaElement>('meta[name=theme-color]')!.content = SD.s[n].tokens['--bg'];
  put('sb', b);
}
DARK.addEventListener('change', colours);
export const themes = () => Object.keys(SD.t);
export const schemes = () => Object.keys(SD.s).filter(n => choose(SD.s, n, null, DARK.matches) === n);
