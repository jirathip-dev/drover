// #1306 mockup: draws the demo's rows in three styles from row JSON (figure: state, action, stage, issue) and the
// pinned swag art (assets/swag/garden: theme.json and sprites.svg at the SOURCE commit). Mockup only, no product code.
const SWAG = '../../../assets/swag/garden/';
const NS = 'http://www.w3.org/2000/svg';
const GLYPH = { working: '●', needs: '!', fail: '✗', done: '✓', merged: '✓', idle: '○' };
const TONE = { working: 'g', needs: 'r', fail: 'r', done: 'mute', idle: 'mute', merged: 'g' };
const hash = t => [...t].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
export let T = {};
const box = id => document.getElementById(id)?.getAttribute('viewBox') || '0 0 132 100';

export async function load() {
  const [t, s] = await Promise.all([fetch(SWAG + 'theme.json').then(r => r.json()), fetch(SWAG + 'sprites.svg').then(r => r.text())]);
  T = t;
  const src = new DOMParser().parseFromString(`<svg>${s}</svg>`, 'text/html').body.firstElementChild, defs = document.createElementNS(NS, 'svg');
  defs.setAttribute('class', 'hdefs');
  src.querySelectorAll('[id]').forEach(g => {
    if (g.closest('symbol') && g.closest('symbol') !== g) return;
    const sym = document.createElementNS(NS, 'symbol');
    sym.id = 'hs-' + g.id;
    sym.setAttribute('viewBox', g.getAttribute('data-box') || '0 0 132 100');
    const c = g.cloneNode(true);
    c.removeAttribute('id');
    sym.append(c);
    defs.append(sym);
  });
  const d = src.querySelector('defs');
  if (d) defs.prepend(d.cloneNode(true));
  document.body.prepend(defs);
}

const act = f => f.state === 'working' ? ({ edit: 'fix', read: 'rev' }[f.tool] ?? 'impl') : f.state === 'needs' || f.state === 'fail' ? 'blocked' : f.state === 'done' || f.state === 'merged' ? 'done' : 'idle';
const species = n => T.species[n % T.species.length];
const spStyle = s => Object.entries(s).filter(e => e[0] !== 'id').map(e => e.join(':')).join(';');
const glyph = f => `<i class="gl ${TONE[f.state] ?? 'mute'}">${GLYPH[f.state] ?? '·'}</i>`;
const svg = (cls, id, style = '') => `<svg class=${cls} viewBox="${box(id)}" style="${style}" aria-hidden=true><use href="#${id}"/></svg>`;

/** A Queue or Events row's figure: terminal its glyph, ascii the action's 3 chars, pixel the stage's plant or the event's sprite. */
export function rowFig(y, f) {
  const a = f.kind ?? act(f), word = (T.words[f.state] ?? f.state);
  let art = glyph(f);
  if (y === 'ascii') art = `<i class="rsa" aria-hidden=true>${esc(T.ascii_rows[{ merged: 'done', fail: 'blocked', needs: 'blocked', queued: 'idle' }[a] ?? a])}</i>`;
  if (y === 'pixel') {
    const s = f.grow != null && species(f.n), id = s ? `hs-plant-${f.grow}-${s.id}-row` : 'hs-' + T.row_sprites[a];
    art = svg('rsp', id, s ? spStyle(s) : '');
  }
  return `<span class="rsg ${f.state}" role=img aria-label="${esc(word)}" title="${esc(word)}">${art}</span>`;
}

/** A Lanes avatar: terminal the glyph, ascii the theme's figure (two frames while working), pixel the keeper over the issue's plant in its look. */
export function laneFig(y, f) {
  const a = act(f);
  let art = glyph(f);
  if (y === 'ascii') {
    const t = T.ascii[a], two = f.state === 'working' ? t.replace(/[^\n]*$/, l => l.replace(/[/\\]/g, c => c === '/' ? '\\' : '/')) : t;
    art = `<i class="asc${two !== t ? ' run' : ''}"><b>${esc(t)}</b>${two !== t ? `<b>${esc(two)}</b>` : ''}</i>`;
  }
  if (y === 'pixel') {
    const look = spStyle(T.looks[hash(f.agent) % T.looks.length]), s = f.grow && species(f.n);
    const plant = s ? `<use href="#hs-plant-${f.grow}-${s.id}" style="${spStyle(s)}"/>` : '';
    art = `<svg class=px viewBox="0 0 132 100" style="${look}" aria-hidden=true>${plant}<use href="#hs-${T.actions[a]}"/></svg>`;
  }
  return `<span class="av ${f.state}">${art}</span>`;
}

/** A Host row's load miniature: none in terminal, the ascii_load chars, or the load_scene mini. */
export function loadFig(y, l) {
  if (y === 'terminal') return '';
  const w = T.load_scene.levels[l];
  if (y === 'ascii') return `<span class=lsm role=img aria-label="load: ${w}"><i class=lsa>${esc(T.ascii_load[l])}</i></span>`;
  return `<span class=lsm role=img aria-label="load: ${w}">${svg('', 'hs-' + T.load_scene.minis[l])}</span>`;
}

/** The loading strip's mark. */
export function loadingFig(y) {
  if (y === 'terminal') return '<span id=ld><i>↻</i> loading</span>';
  if (y === 'ascii') { const [a, b] = T.ascii_loading.split('\n'); return `<span id=ld><i class="asc run"><b>${esc(a)}</b><b>${esc(b)}</b></i> ${esc(T.words.loading)}</span>`; }
  return `<span id=ld class=ldp>${svg('', 'hs-' + T.loading, 'width:24px;height:24px')} ${esc(T.words.loading)}</span>`;
}

export const row = (fig, title, sub, val, extra = '') => `<div class=row${extra}>${fig}<span class=rm><span class=rt>${esc(title)}</span>${sub ? `<span class=rs>${esc(sub)}</span>` : ''}</span>${val ? `<span class=rv>${esc(val)}</span>` : ''}</div>`;
export const word = (k, plain) => T.words[k] ? `<b class=thw>${esc(T.words[k])}</b><small class=pw> · ${esc(plain)}</small>` : esc(plain);
export const head = y => `<div class=ct><b>${y}</b><span class=m>${{ terminal: 'plain glyphs, no figures (today)', ascii: 'the theme’s 3-char figures (#335)', pixel: 'the animated swag sprites' }[y]}</span></div>
<header><b>drover</b><span class=tag>demo</span><i id=dot class=g></i><button><span>↻</span></button><button><span>⚙</span></button></header>`;
