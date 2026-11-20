// #1418: drover's bush theme from a swag checkout: `node scripts/swag-bush.mjs <swag dir> <out dir>`.
// The art is swag's design/bush/art.mjs (cooee#22's approved design, swag#7), written in drover's theme contract
// (docs/themes.md): one <g id> per sprite, .kf1/.kf2 frames, the sign and its levels in scene.svg; icons copied as is.
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [swag, out] = process.argv.slice(2).map(p => resolve(p));
if (!swag || !out) throw new Error('usage: node scripts/swag-bush.mjs <swag dir> <out dir>');
const design = join(swag, 'design/bush');
const art = await import(pathToFileURL(join(design, 'art.mjs')).href);
const { ACTIONS, ACT_OF, ROWS, RAIL, MOMENTS, LOOKS, LEVELS, ASCII, ASCII_ROWS, ASCII_LOAD, ASCII_LOADING, draw, fine, clearing, mini } = art;

const frames = s => s.replaceAll('class="f1"', 'class="kf1"').replaceAll('class="f2"', 'class="kf2"');
const inner = svg => svg.slice(svg.indexOf('>') + 1, svg.lastIndexOf('</svg>'));
const g = (id, body, box = '') => `<g id="${id}"${box && ` data-box="${box}"`} shape-rendering="crispEdges">${frames(body)}</g>\n`;

const RECT = /<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="([\d.]+)" fill="([^"]+)"(?: opacity="([\d.]+)")?\/>/y;
function paths(svg) {
  let s = '', at = 0, batch = [];
  const flush = () => {
    const by = new Map();
    for (const r of batch) by.set(r.paint, (by.get(r.paint) || '') + `M${r.x} ${r.y}h${r.w}v${r.h}h-${r.w}z`);
    for (const [paint, d] of by) s += `<path ${paint} d="${d}"/>`;
    batch = [];
  };
  while (at < svg.length) {
    RECT.lastIndex = at;
    const m = RECT.exec(svg);
    if (!m) {
      flush();
      const next = svg.indexOf('<', at + 1), end = next < 0 ? svg.length : next;
      s += svg.slice(at, end);
      at = end;
      continue;
    }
    const [x, y, w, h] = m.slice(1, 5).map(Number);
    if (batch.some(b => x < b.x + b.w && b.x < x + w && y < b.y + b.h && b.y < y + h)) flush();
    batch.push({ x, y, w, h, paint: `fill="${m[5]}"${m[6] ? ` opacity="${m[6]}"` : ''}` });
    at = RECT.lastIndex;
  }
  flush();
  return s;
}

let sprites = '<!-- drover #1418 bush: written by scripts/swag-bush.mjs from swag design/bush/art.mjs (cooee#22). Looks are CSS vars: --ro/--rs/--rb roo, --wo/--ws wombat, --ko/--ks koala, --em/--es emu, --fk the project neckerchief; kf1/kf2 the two frames of a loop. Rows are 16 × 16, load-calm|busy|held 48 × 16. -->\n';
for (const [k, fn] of Object.entries(ACTIONS)) sprites += g(`a-${k}`, fine(fn()));
for (const [k, fn] of Object.entries(MOMENTS)) if (k !== 'screech') sprites += g(`m-${k}`, fine(fn()));
for (const k of ['joey', 'cocky']) sprites += g(`q-${k}`, fine(RAIL[k]()));
for (const [k, m] of Object.entries(ROWS)) sprites += g(`r-${k}`, draw(m, 1), '0 0 16 16');
const LOAD = { calm: 'moderate', busy: 'high', held: 'catastrophic' };
for (const [k, l] of Object.entries(LOAD)) sprites += g(`load-${k}`, inner(mini(l)), '0 0 48 16');

const scene = frames(clearing(4).replaceAll('var(--fk)', 'var(--fk,#58a6ff)')).replace('<svg ', '<svg xmlns="http://www.w3.org/2000/svg" ') + '\n';

const row = a => `r-${ACT_OF[a]}`;
const theme = {
  scheme: { light: 'bush-light', dark: 'bush-dark' },
  actions: Object.fromEntries(Object.entries(ACT_OF).map(([a, k]) => [a, `a-${k}`])),
  poses: { working: 'a-bound', idle: 'a-sleep', done: 'a-laugh', blocked: 'a-stuck' },
  looks: LOOKS.map(l => l.vars),
  scene: { weather: false },
  load_scene: { kind: 'sign', levels: LOAD, minis: { calm: 'load-calm', busy: 'load-busy', held: 'load-held' } },
  rail: { item: 'q-joey', needs: 'q-cocky' },
  moments: { merged: { sprite: 'm-clear', anim: 'hop' }, fail: { sprite: 'm-swoop', anim: 'shake' }, 'needs-you': { sprite: 'q-cocky', anim: 'pulse' } },
  row_sprites: { ...Object.fromEntries(Object.keys(ACT_OF).map(a => [a, row(a)])), queued: 'r-joey', needs: 'r-needs', merged: 'r-merged', fail: 'r-fail' },
  loading: 'a-bound',
  icons: { 180: 'icon-180.png', 192: 'icon-192.png', 512: 'icon-512.png', favicon: 'icon-192.png' },
  modes: ['scene', 'ascii'],
  ascii: ASCII,
  ascii_rows: ASCII_ROWS,
  ascii_load: ASCII_LOAD,
  ascii_loading: ASCII_LOADING,
  words: {
    horse: 'roo', horses: 'roos', herd: 'mob', ranch: 'outback', barn: 'station',
    impl: 'bound', fix: 'dig', rev: 'peer', update: 'run', queued: 'in the log',
    working: 'on the move', needs: 'screech', done: 'home and hosed', fail: 'swooped', idle: 'having a kip',
    merged: 'over the fence', loading: 'bounding…', 'empty-threads': 'nobody about',
  },
};
if (LEVELS.join() !== Object.values(LOAD).join()) throw new Error(`art.mjs levels changed: ${LEVELS}`);

mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'sprites.svg'), paths(sprites));
writeFileSync(join(out, 'scene.svg'), scene);
writeFileSync(join(out, 'theme.json'), JSON.stringify(theme, null, 1) + '\n');
for (const s of [180, 192, 512]) copyFileSync(join(design, `icon/icon-${s}.png`), join(out, `icon-${s}.png`));
