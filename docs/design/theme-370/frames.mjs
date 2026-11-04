// #370: the second frame of every row sprite (Queue and Events, 16 px art at 24 px), and the Host card's load minis.
// Frame 1 is today's art, taken from its source, never redrawn: the kitchen's from ../kitchen/art.mjs, the farm's from
// ../farm-v2/make_sprites.py (stdlib python, read once). A loop is frame 1 ↔ frame 2, the `.kf1` / `.kf2` groups
// herd.css already swaps by --o1/--o2 (0.6 s a frame); reduced motion shows frame 1.
import { execFileSync } from 'node:child_process';
import { ROW, draw, mini as kmini } from '../kitchen/art.mjs';

const here = new URL('.', import.meta.url).pathname;

// ---- kitchen: a frame is a full layer list ([map, x, y, class?], one character a pixel, ../kitchen/art.mjs INK) ----
const head = k => ROW[k].slice(0, 5); // the hat variants and the apron: rowHead() in art.mjs
const move = (layers, dx, dy) => layers.map(([m, x, y, c]) => [m, x + dx, y + dy, c]);
export const KITCHEN = {
  // the wok tossed up a pixel, the food in the air, the flames flicker the other way
  cook: [...head('cook'), [['..V..R..', '........', 'N....N..', 'NNNNNNoo', '.NNNN...'], 8, 3], [['F.Ff.F..', '.FfFfFfF'], 8, 10], [['GGGGGGG'], 8, 12]],
  // the knife comes down on the board, the pieces jump
  chop: [...head('chop'), [['LLLL..', 'LLLLoo', 'LLLL..'], 9, 6], [['Q..QV..', 'OOOOOOO', 'ooooooo'], 9, 9]],
  // the spoon dips into the bowl; a wisp of steam
  taste: [...head('taste'), [['L...', '.L..', '..LL'], 9, 7], [['W'], 12, 8], [['.VQR.', 'EEEEEE', 'eeeeee'], 10, 10]],
  // a walking bob: the whole chef and the crate up a pixel
  carry: move(ROW.carry, 0, -1),
  // standing by: the eyes glance to the pass
  wait: [...ROW.wait, [['.SSkSSSk'], 0, 5]],
  // the plate lifted a pixel, a glint on it
  serve: [...head('serve'), [['.VQR..', 'EEEEEE', 'eeeeee'], 10, 4], [['WWS'], 8, 7], [['W'], 15, 2]],
  // the waving hand drops, the bell rings (two marks over it)
  bell: [...head('bell'), [['..N..', '.YYY.', 'YEYYY', 'YEYYY', 'yyyyy'], 10, 8], [['S.', '.W'], 14, 3], [['Y...Y'], 10, 6]],
  // the ticket swings on its clip
  ticket: [ROW.ticket[0], move([ROW.ticket[1]], 1, 0)[0]],
  // the plate slides back a pixel under the ✗, which drops
  back: [move([ROW.back[0]], 0, 1)[0], move([ROW.back[1]], 1, 0)[0], ROW.back[2]],
};
// The kitchen's row sprite ids (theme.json `row_sprites`) and the art each takes.
export const KITCHEN_IDS = { cook: 'row-cook', chop: 'row-chop', taste: 'row-taste', carry: 'row-carry', wait: 'row-wait', serve: 'row-serve', bell: 'row-bell', ticket: 'row-ticket', back: 'row-back' };
const crisp = s => `<g shape-rendering="crispEdges">${s}</g>`;
export function kitchenRow(k) {
  return `<g id="${KITCHEN_IDS[k]}" shape-rendering="crispEdges"><g transform="translate(16 0) scale(6.25)"><g class="kf1">${draw(ROW[k], 1)}</g><g class="kf2">${draw(KITCHEN[k], 1)}</g></g></g>`;
}

// ---- farm: frame 2 is frame 1 with some lines (by index) redrawn, 16 characters each (make_sprites.py PAL) ----
export const FARM = {
  // the trot: the legs gather under the body, the furrow runs on
  'row-plough': { 8: '..sss..cc...cc..', 9: '.sss...cc...cc..', 10: 'nnnnh...hh..hh..', 11: '.nnnnnn.........' },
  // the forge flares, a spark at the hoof
  'row-anvil': { 0: '..o.............', 1: 'o.o........mcc..', 2: '.oo.......mccce.', 8: 'bbbocc.c..c.c...' },
  // the vet leans in, the arm lower on the leg
  'row-vet': { 0: '..ff............', 1: '..ff.......mcc..', 5: 'vvvv.mckkccc....', 6: 'vvvvvccccccc....' },
  // the walk: the legs gather
  'row-hay': { 8: '.......cc...cc..', 9: '.......cc...cc..', 10: '.......hh...hh..' },
  // the tail swishes, a bite of grass
  'row-graze': { 4: '..mcccccccm.....', 5: '...ckkcccccm....', 10: '...h.h...h.h..cg', 11: '............ggg.' },
  // the walk into the stable: the legs gather, the tail lifts
  'row-stable': { 6: '.mccccccDDDDDbb.', 7: '..ckkcccDDDDDbb.', 9: '..cc..c.DDDDDbb.', 10: '..cc..c.DDDDDbb.', 11: '..hh..h.DDDDDbb.' },
  // the front legs paddle, the neigh marks move
  'row-rear': { 0: '...........R....', 1: '........mcc..R..', 5: '....ckkc..c...p.', 6: '...ccccc.c....pp' },
  // queued: straw lifts off the bale
  'row-bale': { 3: '.....y.....y....' },
  // needs you, queued: the red bale bobs a pixel
  'row-bale-red': 'up',
  // merged: higher over the fence, a pixel on
  'row-jump': 'over',
  // fail: the head tossed back, dust at the hooves
  'row-refuse': { 1: '.....mcc........', 2: '....mccce.......', 3: '...mccc.cc......', 11: '.nn........p...p' },
};
let farmSrc;
export function farmSource() {
  return farmSrc ||= JSON.parse(execFileSync('python3', ['-I', '-c',
    'import runpy,json,sys; ns=runpy.run_path(sys.argv[1]); print(json.dumps({k: ns[k] for k in ("ROWS","PAL","MINI")}))', here + '../farm-v2/make_sprites.py'], { encoding: 'utf8' }));
}
function pixels(rows, pal) {
  let s = '';
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length;) {
      const ch = row[x];
      if (ch === '.') { x++; continue; }
      let n = 1;
      while (row[x + n] === ch) n++;
      s += `<rect x="${x}" y="${y}" width="${n}" height="1" fill="${pal[ch]}"/>`;
      x += n;
    }
  });
  return crisp(s);
}
export function farmFrames(id) {
  const { ROWS } = farmSource(), f1 = ROWS[id], d = FARM[id];
  if (d === 'up') return [f1, [...f1.slice(1), '.'.repeat(16)]];
  // the horse (lines 1–5) up a line and on a pixel; the fence stays
  if (d === 'over') return [f1, f1.map((r, y) => y < 5 ? '.' + f1[y + 1].slice(0, 15) : y === 5 ? '.'.repeat(16) : r)];
  return [f1, f1.map((r, y) => d[y] ?? r)];
}
// Today's shipped farm rows are 16 × 12 at 8.25 (the 132 × 100 box); the four 16-line ones (bale, jump, refuse) fit it
// at 6.25, centred, like the kitchen's.
export function farmRow(id) {
  const { PAL } = farmSource(), [a, b] = farmFrames(id);
  for (const f of [a, b]) for (const r of f) if (r.length !== 16) throw new Error(`${id}: a line of ${r.length}`);
  const tall = a.length > 12 && !['row-stable', 'row-rear'].includes(id);
  return `<g id="${id}" ${tall ? 'transform="translate(16 0) scale(6.25)"' : 'transform="scale(8.25)"'} class="row ${id}"><g class="kf1">${pixels(a, PAL)}</g><g class="kf2">${pixels(b, PAL)}</g></g>`;
}
export const FARM_IDS = Object.keys(FARM);

// ---- the Host card's load mini, 48 × 16 (proposed `load_scene.minis`): two frames each, like the rows ----
// Kitchen: three burners (art.mjs mini()); frame 2 flickers (each flame's odd column the taller). Farm: the farm-v2
// weather minis; frame 2 drifts the clouds a pixel and moves the rain.
export function kitchenMini(level) {
  const one = kmini(level).replace(/^<svg[^>]*>|<\/svg>$/g, ''), style = kmini(level).match(/style="([^"]*)"/)[1];
  const two = one.replace(/<rect x="(\d+)" y="(\d+)" width="1" height="(\d+)" fill="var\(--f1\)"\/>/g, (m, x, y, h) => {
    const t = +h + (x % 2 ? 1 : -1) * (+h > 1 ? 1 : 0);
    return `<rect x="${x}" y="${12 - t}" width="1" height="${t}" fill="var(--f1)"/>`;
  });
  return `<g id="load-${level}" style="${style}" shape-rendering="crispEdges"><g class="kf1">${one}</g><g class="kf2">${two}</g></g>`;
}
export function farmMini(level) {
  const src = farmSource().MINI[{ calm: 'mini-calm', busy: 'mini-windy', held: 'mini-storm' }[level]];
  const two = src.replace(/<ellipse cx="([\d.]+)"/g, (m, x) => `<ellipse cx="${(+x + 1).toFixed(1)}"`).replace(/M (\d+) 9 l -1.4 3.6/g, (m, x) => `M ${+x - 1} 10 l -1.4 3.6`);
  return `<g id="load-${level}" transform="translate(4 0)"><g class="kf1">${src}</g><g class="kf2">${two}</g></g>`;
}
