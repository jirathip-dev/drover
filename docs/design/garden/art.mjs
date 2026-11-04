// #423 garden theme, design mockup: the shopkeeper, the six species at every growth stage, the moments, the shop
// scene, the window-light minis, the row sprites and the icon all come from the pixel maps below (one character = one
// pixel), like ../kitchen/art.mjs. index.html and sheet.html import this module; phase 2's build writes the theme files
// (sprites.svg, scene.svg, icons) from the same maps.
// Lane sprites keep the farm's box: 132 × 100, one pixel = 4 units (a 33 × 25 grid), feet on y 23, shadow on y 24.
// The keeper stands on the left (x 0–15) and does the role; the plant stands on the right (x 17–32) at the issue's stage.
// Both are separate sprites drawn into the same box, so any action pairs with any stage (the contract's `stages`).
// Row sprites are a 16 × 16 grid, one pixel = 1 (drawn at 24 px, 1.5×). Per-lane colours are CSS custom properties
// (`looks`), per-issue leaf colours too (`species`); `--fk` is the repo colour.

// One character → a fill (and an opacity). `.` and space are transparent.
export const INK = {
  k: '#2b2420', q: ['#2b2420', 0.16], b: '#e8957a', m: '#7a3b2e',
  S: 'var(--sk,#f1c7a5)', s: 'var(--ss,#d9a684)', M: 'var(--hr,#5a3a24)',
  W: 'var(--sh,#e9e2cf)', w: 'var(--sd,#c9bfa5)', A: 'var(--ap,#5c7a4a)', a: 'var(--as,#46603a)', T: 'var(--fk,#58a6ff)',
  H: 'var(--hc,#c0453a)', h: 'var(--hs,#9a352d)', J: '#e2c27a', j: '#b8954e',
  P: '#4a3f36', p: '#3a312a', B: '#2a2320',
  // pot, soil, stem, roots
  U: '#e08a5c', O: '#c8643c', o: '#9a4a2a', d: '#5a3d2b', t: '#4d7a34', r: '#efe2c4',
  // leaves: the species' colours (--lf leaf, --ld its shade, --lv vein or variegation); yellowed and brown leaves
  L: 'var(--lf,#3f8f45)', l: 'var(--ld,#2a6a34)', V: 'var(--lv,#6fb85f)', Y: '#d9b84a', y: '#a8862e', n: '#8a6a3a',
  // glass jar and water, kraft paper and twine, the tag
  G: ['#d8eef0', 0.55], g: '#9ec6cc', Z: '#6fb8e0', z: ['#6fb8e0', 0.5], K: '#d9b98a', c: '#b8956a', X: '#8a3b2e', E: '#f7f1e3',
  // tools: the watering can, steel, the trowel's handle, the mug, the bell, the lens, the thirsty drop
  C: '#5f9aa6', e: '#3f6f7a', N: '#a9b1b5', i: '#6d7579', u: '#8a5a34', F: '#f1e8d6', f: '#c9bfa5',
  Q: '#ffffff', R: '#d9443a', D: '#8ec5ff', I: ['#cfe8f0', 0.7], x: ['#ffffff', 0.6],
};

// The six species (#423 Plants): per-issue, picked by issue number (n % 6, so neighbours always differ).
export const SPECIES = [
  { id: 'monstera', vars: { '--lf': '#3f8f45', '--ld': '#2a6a34', '--lv': '#6fb85f' } },
  { id: 'philodendron', vars: { '--lf': '#4f9a3a', '--ld': '#356f2a', '--lv': '#b4dc5a' } },
  { id: 'peperomia', vars: { '--lf': '#3c6b3a', '--ld': '#294c2b', '--lv': '#c2dcae' } },
  { id: 'alocasia', vars: { '--lf': '#2b5a45', '--ld': '#1c3f31', '--lv': '#b9d9a9' } },
  { id: 'pothos', vars: { '--lf': '#5aa040', '--ld': '#3d7a2c', '--lv': '#ecd96a' } },
  { id: 'calathea', vars: { '--lf': '#4a7c50', '--ld': '#2a4a31', '--lv': '#d27a98' } },
];
export const speciesOf = n => SPECIES[((n % SPECIES.length) + SPECIES.length) % SPECIES.length];
export const sv = sp => Object.entries((typeof sp === 'number' ? speciesOf(sp) : SPECIES.find(s => s.id === sp)).vars).map(([k, v]) => `${k}:${v}`).join(';');

// Leaf stamps, lane scale: `big` the mature leaf (the ready stage; the monstera's is split), `mid`, `small`.
// The stem meets a stamp at the middle of its bottom row.
const LEAF = {
  monstera: {
    big: ['..LLLL..', '.LLLLLL.', 'LL.LVL.L', 'LLLLVLLL', 'L.LLVL.L', '.LLLVLL.', '..LLVL..'],
    mid: ['.LL.LL.', 'LLLLLLL', 'LLLVLLL', '.LLVLL.', '..LVL..'],
    small: ['L.L', 'LLL', '.L.'],
  },
  philodendron: {
    big: ['...LL...', '..LLLL..', '.LLLLLL.', 'LLLVVLLL', 'LLLVVLLL', 'LLLVVLLL', '.LL..LL.'],
    mid: ['..LL..', '.LLLL.', 'LLVVLL', 'LLVVLL', '.L..L.'],
    small: ['.L.', 'LLL', 'L.L'],
  },
  peperomia: {
    big: ['.LLLL.', 'LVLLVL', 'LLVVLL', 'LVLLVL', 'LLVVLL', '.LLLL.'],
    mid: ['.LLL.', 'LVLVL', 'LLVLL', 'LVLVL', '.LLL.'],
    small: ['.L.', 'LVL', '.L.'],
  },
  alocasia: {
    big: ['...L...', '..LLL..', '.LLVLL.', '.LVVVL.', 'LVLVLVL', 'LLLVLLL', 'LL.V.LL'],
    mid: ['..L..', '.LVL.', 'LVVVL', 'LLVLL', 'L.V.L'],
    small: ['.L.', 'LVL', 'L.L'],
  },
  pothos: {
    big: ['..LL..', '.LVLL.', 'LLLVLL', 'LVLLVL', 'LLVLLL', '.LLLL.', '..LL..'],
    mid: ['.LL..', 'LVLL.', 'LLVLL', '.LLVL', '..LL.'],
    small: ['LL.', 'LVL', '.L.'],
  },
  calathea: {
    big: ['..LL..', '.LVVL.', 'LLllLL', 'LVllVL', 'LLllLL', 'LVllVL', '.LllL.', '..LL..'],
    mid: ['.LL.', 'LVVL', 'LllL', 'LVVL', '.LL.'],
    small: ['.L.', 'LVL', 'LVL'],
  },
};
// Row scale (16 px): a 3 × 3 leaf and a 5 × 5 mature leaf per species.
const RLEAF = {
  monstera: { s: ['L.L', 'LLL', '.L.'], h: ['.LLL.', 'L.V.L', 'LLVLL', 'L.VLL', '.LVL.'] },
  philodendron: { s: ['.L.', 'LLL', 'L.L'], h: ['..L..', '.LLL.', 'LLVLL', 'LLVLL', '.L.L.'] },
  peperomia: { s: ['.L.', 'LVL', '.L.'], h: ['.LLL.', 'LVLVL', 'LLVLL', 'LVLVL', '.LLL.'] },
  alocasia: { s: ['.L.', 'LVL', 'L.L'], h: ['..L..', '.LVL.', 'LVVVL', 'LLVLL', 'L.V.L'] },
  pothos: { s: ['LL.', 'LVL', '.L.'], h: ['.LL..', 'LVLL.', 'LLVLL', '.LLVL', '..LL.'] },
  calathea: { s: ['.L.', 'LVL', 'LVL'], h: ['.LL.', 'LVVL', 'LllL', 'LVVL', '.LL.'] },
};

// ---- map helpers ----
const flipH = m => m.map(r => [...r].reverse().join(''));
const flipV = m => [...m].reverse();
const recolor = (m, tr) => m.map(r => [...r].map(c => tr[c] || c).join(''));
const YELLOW = { L: 'Y', V: 'y', l: 'y' }, WILT = { L: 'y', V: 'n', l: 'n' };
// A pixel line (Bresenham) as layers of one-pixel maps.
function line(x0, y0, x1, y1, ch = 't') {
  const L = [], dx = Math.abs(x1 - x0), dy = -Math.abs(y1 - y0), sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  let err = dx + dy;
  for (;;) {
    L.push([[ch], x0, y0]);
    if (x0 === x1 && y0 === y1) break;
    const e2 = 2 * err;
    if (e2 >= dy) { err += dy; x0 += sx; }
    if (e2 <= dx) { err += dx; y0 += sy; }
  }
  return L;
}
// A leaf placed at x, y with its stem from the soil point (bx, by): [stamp, x, y, {h, v, tint}].
function leaves(list, bx, by) {
  const stems = [], ls = [];
  for (const [m0, x, y, o = {}] of list) {
    let m = o.h ? flipH(m0) : m0;
    if (o.v) m = flipV(m);
    if (o.tint) m = recolor(m, o.tint);
    const w = m[0].length, h = m.length;
    const sx = x + Math.floor(w / 2), sy = o.v ? y - 1 : y + h;
    if (!o.free) stems.push(...line(bx, by, sx, sy, o.v ? 'y' : 't'));
    ls.push([m, x, y, o.cls]);
  }
  return [...stems, ...ls];
}

// ---- the pot and the plant at each stage (lane scale) ----
const POT = ['UUUUUUUUUUU', 'OOOOOOOOOOo', '.OOOOOOOOo.', '.OOOOOOOOo.', '..OOOOOOo..', '..ooooooo..'];
const SOIL = ['ddddddddd'];
const JAR = ['.ggggg.', 'gGGGGGg', 'gGGGGGg', 'gzzzzzg', 'gzzrzzg', 'gzrzrzg', 'grzzzrg', '.ggggg.'];
const WRAP = [
  'K.KKKcKKK.KK.',
  'KKKKKcKKKKKKK',
  '.KKKKKcKKKKK.',
  '.KKKKKcKKKKK.',
  '.KKKKKKcKKKK.',
  '..KKKKKcKKK..',
  '..XXXXXXXXX..',
  '..KKKKKKcKK..',
  '...KKKKKcK...',
  '...KKKKKKc...',
  '....KKKKK....',
];
const BOW = ['X.X', '.X.', 'X.X'];
const TAG = ['EE', 'EE', 'Ek'];
const PSHADOW = ['.qqqqqqqqqqq.'];
// Stages (#423 Growth): queued a cutting in a jar, impl rooted in a pot, review / fix a few leaves, ready the full plant
// with its mature leaf, merged wrapped and sold; blocked the wilted plant. f1/f2 sway the top leaf (the row loop).
export const STAGES = ['queued', 'impl', 'review', 'fix', 'ready', 'merged', 'blocked'];
export function plant(sp, stage) {
  const F = LEAF[sp], base = [[PSHADOW, 19, 23]];
  const pot = [[SOIL, 21, 16], [POT, 20, 17]];
  const sway = (l, k) => l.map(([m, x, y, o = {}], i) => [m, x + (i === l.length - 1 ? k : 0), y, o]);
  const two = l => [...leaves(sway(l, 0), 25, 16).map(a => [...a.slice(0, 3), a[3] ? a[3] + ' f1' : 'f1']), ...leaves(sway(l, 1), 25, 16).map(a => [...a.slice(0, 3), 'f2'])];
  switch (stage) {
    case 'queued': return [[['.qqqqqqq.'], 21, 23], ...line(25, 20, 25, 13), ...line(25, 15, 24, 11), [JAR, 22, 15],
      [F.mid, 22, 6, 'f1'], [F.mid, 23, 6, 'f2']];
    case 'impl': return [...base, ...pot, ...two([[F.small, 21, 11, { h: 1 }], [F.small, 25, 9]])];
    case 'review': case 'fix': {
      const L = [[F.mid, 17, 9, { h: 1 }], [F.mid, 27, 8], [F.small, 23, 5]];
      const cut = stage === 'fix' ? [...line(25, 16, 29, 13), [['n'], 29, 12]] : [];
      return [...base, ...cut, ...pot, ...two(L)];
    }
    case 'ready': {
      const L = sp === 'pothos'
        ? [[F.mid, 16, 6, { h: 1 }], [F.mid, 28, 5], [F.small, 19, 11, { h: 1 }], [F.big, 22, 0]]
        : [[F.mid, 16, 7, { h: 1 }], [F.mid, 27, 6], [F.small, 20, 11, { h: 1 }], [F.small, 28, 12], [F.big, 21, 0]];
      const trail = sp === 'pothos' ? [...line(20, 17, 18, 21), [flipH(F.small), 16, 20], ...line(30, 17, 31, 20), [F.small, 30, 21]] : [];
      return [...base, ...pot, ...two(L), ...trail];
    }
    case 'merged': return [[PSHADOW, 19, 23], ...leaves([[F.mid, 17, 3, { h: 1 }], [F.mid, 28, 2], [F.big, 21, 0]], 25, 12),
      [WRAP, 19, 12], [BOW, 24, 17], [TAG, 30, 19], [['X'], 29, 18]];
    case 'blocked': {
      // the stems arch over the rim and the yellowed leaves hang down beside the pot; one has already dropped
      const w = m => recolor(flipV(m), WILT);
      return [...base, ...pot,
        ...line(24, 16, 21, 13, 'y'), ...line(21, 13, 19, 15, 'y'), ...line(26, 16, 29, 13, 'y'), ...line(29, 13, 31, 15, 'y'),
        ...line(25, 16, 25, 11, 'y'), ...line(25, 11, 27, 10, 'y'),
        [w(flipH(F.mid)), 16, 16], [w(F.small), 30, 16], [w(F.small), 26, 11], [recolor(['LLL.', '.LLL'], WILT), 14, 23]];
    }
  }
}

// ---- the keeper, in parts (head at hx, hy; body one column wider each side), like the kitchen's chef ----
const HEAD = ['.MMMMMMM.', 'MMSSSSSMM', 'MSkSSSkSM', 'SSkSSSkSS', 'SbSSSSSbS', 'sSSSmSSSs', '.sSSSSSs.'];
const HEAD_SMILE = ['.MMMMMMM.', 'MMSSSSSMM', 'MSkSSSkSM', 'SSkSSSkSS', 'SbSSSSSbS', 'sSSmmmSSs', '.sSSSSSs.'];
const HEAD_O = ['.MMMMMMM.', 'MMSSSSSMM', 'MSkSSSkSM', 'SSkSSSkSS', 'SbSSmSSbS', 'sSSSmSSSs', '.sSSSSSs.'];
const HEAD_LOOK = ['.MMMMMMM.', 'MMSSSSSMM', 'MSSkSSSkM', 'SSSkSSSkS', 'SbSSSSSbS', 'sSSSSmSSs', '.sSSSSSs.'];
const HEAD_SHUT = ['.MMMMMMM.', 'MMSSSSSMM', 'MSSSSSSSM', 'SkkSSSkkS', 'SbSSSSSbS', 'sSSmmmSSs', '.sSSSSSs.'];
// three looks of head: a straw sun hat, a headscarf, a bun (--ht / --hb / --hk, one is 1)
const SUNHAT = ['...JJJJJ...', '..JJJJJJJ..', '..jjjjjjj..', 'JJJJJJJJJJJ'];
const SCARF = ['..HHHHH...', '.HHHHHHHH.', 'HHhhhhhhHh', '.........h'];
const BUN = ['...MMM...', '..MMMMM..', '.MMMMMMM.'];
// shirt, the repo neckerchief, the apron with its pocket
const BODY = ['..WTTTTTW..', '.WWWWTWWWW.', 'WWAAAAAAAWW', 'WWAAAAAAAWW', 'WWAaaaAAAWW', 'WWAAAAAAAWW', '.AAAAAAAAA.', '.AAAAAAAAA.', '.aaaaaaaaa.'];
const LEGS = ['..PpP.PpP..', '..PPP.PPP..', '.BBBB.BBBB.'];
const KSHADOW = ['..qqqqqqqqqqq..'];
const ARM = ['W', 'W', 'W', 'w', 'S'];
function keeper(hx, hy, { head = HEAD, arms = 'lr' } = {}) {
  const L = [[KSHADOW, hx - 3, hy + 19], [LEGS, hx - 1, hy + 16], [BODY, hx - 1, hy + 7]];
  if (arms.includes('l')) L.push([ARM, hx - 2, hy + 8]);
  if (arms.includes('r')) L.push([ARM, hx + 10, hy + 8]);
  L.push([head, hx, hy]);
  L.push([SUNHAT, hx - 1, hy - 3, 'hat-t'], [SCARF, hx - 1, hy - 1, 'hat-b'], [BUN, hx, hy - 3, 'hat-k']);
  return L;
}

// ---- the actions (the contract's `actions`, by role): impl water, fix prune, rev inspect, update repot, idle,
// done (the plant is ready), blocked (it wilts). f1/f2 are the two frames of the loop.
const CAN = ['.eee......', 'e...e....C', 'eCCCCC..C.', 'eCCCCCCC..', '.CCCCCC...', '.CCCCCC...', '..eeee....'];
const SCIS_OPEN = ['hh...NN', '.hhNN..', 'hh...NN'];
const SCIS_SHUT = ['hh.....', '.hhNNNN', 'hh.....'];
const LENS = ['.iii.', 'iIIIi', 'iIxIi', 'iIIIi', '.iii.'];
const TROWEL = ['uuuNNN.', '...NNNN', '....dd.'];
const BAG = ['.KKKK.', 'KddddK', 'KKKKKK', 'KcKKKK', 'KKKKcK', 'KKKKKK'];
const MUG = ['FFFf', 'FFFff', 'FFFf'];
const STEAM = ['.x.', 'x..', '.x.'];
const SPARK = ['.Q.', 'QJQ', '.Q.'];
const SWEAT = ['D', 'D'];
export const ACTIONS = {
  water: () => [
    ...keeper(3, 5, { arms: 'l', head: HEAD_LOOK }),
    [['W', 'W', 'WS'], 13, 13], [CAN, 13, 10],
    [['Z', '.', 'Z'], 23, 12, 'f1'], [['.', 'Z', '.', 'Z'], 24, 12, 'f2'],
  ],
  prune: () => [
    ...keeper(3, 5, { arms: 'l', head: HEAD_LOOK }),
    [['WWWWS'], 13, 13], [SCIS_OPEN, 17, 12, 'f1'], [SCIS_SHUT, 17, 12, 'f2'], [['n'], 25, 15, 'f2'], [['.n'], 24, 18, 'f2'],
  ],
  inspect: () => [
    ...keeper(4, 6, { arms: 'l', head: HEAD_LOOK }),
    [['.S', 'WW', 'W.'], 14, 12], [['u', '.u'], 15, 10], [LENS, 15, 6, 'f1'], [LENS, 16, 7, 'f2'], [['u'], 16, 11, 'f2'],
  ],
  repot: () => [
    ...keeper(2, 5, { arms: 'l', head: HEAD_LOOK }),
    [BAG, 12, 18], [['WWWS'], 12, 13], [TROWEL, 15, 13, 'f1'], [TROWEL, 16, 12, 'f2'], [['d', '.', '.d'], 21, 15, 'f2'],
  ],
  idle: () => [
    ...keeper(6, 5, { arms: 'l', head: HEAD_SHUT }),
    [['W', 'W', 'WS'], 16, 13], [MUG, 13, 14], [STEAM, 13, 10, 'f1'], [flipH(STEAM), 14, 10, 'f2'],
  ],
  done: () => [
    ...keeper(4, 5, { arms: 'l', head: HEAD_SMILE }),
    [['.S', 'WS', 'W.', 'W.'], 13, 10], [SPARK, 19, 2, 'f1'], [SPARK, 29, 4, 'f2'],
  ],
  wilted: () => [
    ...keeper(4, 5, { arms: '', head: HEAD_O }),
    [['SW', 'SW', '.W', '.W', '.w'], 1, 10], [['WS', 'WS', 'W.', 'W.', 'w.'], 14, 10], [SWEAT, 14, 5, 'f1'], [SWEAT, 14, 6, 'f2'],
  ],
};
export const ACT_OF = { impl: 'water', fix: 'prune', rev: 'inspect', update: 'repot', idle: 'idle', done: 'done', blocked: 'wilted' };

// #241 `moments` (#423 Moments): merged the plant wrapped and sold, the shop bell rings (hop); FAIL a yellowed leaf drops
// off (shake); needs-you a red "thirsty" drop pulses over the pot (pulse).
const BELL = ['..i..', '.JJJ.', 'JJQJJ', 'JJJJJ', 'jjjjj'];
const RING = ['J...J', '.J.J.'];
const COUNTER = ['uuuuuuuuuuu', 'ooooooooooo'];
export const DROP = ['..R..', '.RRR.', 'RRQRR', 'RRRQR', 'RRRRR', '.RRR.'];
export const MOMENTS = {
  sold: () => [
    ...keeper(2, 5, { arms: 'l', head: HEAD_SMILE }),
    [COUNTER, 7, 21], [BELL, 12, 16], [['WWS'], 12, 13], [RING, 12, 13, 'f2'], ...plant('monstera', 'merged'),
  ],
  'leaf-drop': () => [
    ...keeper(2, 5, { arms: 'l', head: HEAD_O }), [['WWWS'], 12, 13],
    ...plant('monstera', 'review').filter(a => !/f2/.test(a[3] || '')).map(a => [a[0], a[1], a[2]]),
    [recolor(LEAF.monstera.small, YELLOW), 30, 11, 'f1'], [recolor(flipV(LEAF.monstera.small), YELLOW), 31, 18, 'f2'],
  ],
  thirsty: () => [...plant('monstera', 'review').filter(a => !/f2/.test(a[3] || '')).map(a => [a[0], a[1], a[2]]), ...dropBubble(9, 2)],
};
// The thirsty bubble alone: over a jar on the counter (the rail's needs-you item) or a pot.
export function dropBubble(x, y, flip = 0) {
  const B = ['.EEEEEEE.', 'EEEEEEEEE', 'EEEEEEEEE', 'EEEEEEEEE', 'EEEEEEEEE', 'EEEEEEEEE', 'EEEEEEEEE', '.EEEEEEE.', '..EE.....', '.E.......'];
  return [[flip ? flipH(B) : B, x, y], [DROP, x + 2, y + 1]];
}
export const SAYS = { sold: 'sold!', 'leaf-drop': 'leaf drop', thirsty: 'thirsty' };
export const bubble = t => `<rect x="60" y="0" width="72" height="16" rx="4" fill="#f7f1e3"/><text x="96" y="12" font-size="11" font-weight="700" font-family="ui-monospace,Menlo,monospace" text-anchor="middle" fill="#2b2420">${t}</text>`;

// ---- #241 `row_sprites` and #423 `stages` at row scale (16 × 16) ----
const RPOT = [[['.dddddd.'], 4, 10], [['UUUUUUUU', 'OOOOOOOo', '.OOOOOo.', '.OOOOOo.', '..oooo..'], 4, 11]];
const RJAR = ['.gggg.', 'gGGGGg', 'gzzzzg', 'gzrzzg', 'grzrzg', 'gzzzzg', '.gggg.'];
const RWRAP = ['K.KcKKK.K', 'KKKcKKKKK', '.KKKcKKK.', '.XXXXXXX.', '..KKcKK..', '..KKKcK..', '...KKK...'];
export function rowPlant(sp, stage) {
  const F = RLEAF[sp], two = (l, bx = 8, by = 10) => [
    ...leaves(l, bx, by).map(a => [...a.slice(0, 3), 'f1']),
    ...leaves(l.map((x, i) => i === l.length - 1 ? [x[0], x[1] + 1, x[2], x[3]] : x), bx, by).map(a => [...a.slice(0, 3), 'f2'])];
  switch (stage) {
    case 'queued': return [...line(8, 13, 8, 5), [RJAR, 5, 9], ...two([[F.s, 6, 2]], 8, 6)];
    case 'impl': return [...RPOT, ...two([[F.s, 4, 6, { h: 1 }], [F.s, 8, 5]])];
    case 'review': case 'fix': return [...(stage === 'fix' ? [...line(8, 10, 12, 7), [['n'], 13, 6]] : []), ...RPOT, ...two([[F.s, 2, 5, { h: 1 }], [F.s, 11, 4], [F.s, 6, 2]])];
    case 'ready': return [...RPOT, ...two([[F.s, 1, 5, { h: 1 }], [F.s, 12, 5], [F.h, 5, 0]])];
    case 'merged': return [...leaves([[F.s, 2, 1, { h: 1 }], [F.s, 11, 1], [F.h, 5, 0]], 8, 8), [RWRAP, 3, 8], [['XE', '.E'], 12, 11]];
    case 'blocked': return [...RPOT, ...line(7, 10, 5, 8, 'y'), ...line(5, 8, 3, 10, 'y'), ...line(9, 10, 11, 8, 'y'), ...line(11, 8, 12, 10, 'y'), ...line(8, 10, 8, 7, 'y'),
      [recolor(flipV(flipH(F.s)), WILT), 1, 11], [recolor(flipV(F.s), WILT), 12, 11], [recolor(flipV(F.s), WILT), 8, 7]];
    case 'needs': return [...RPOT, ...leaves([[F.s, 1, 5, { h: 1 }], [F.s, 6, 2]], 8, 10), [['..R..', '.RRR.', 'RRQRR', 'RRRRR', '.RRR.'], 11, 0, 'f1'], [['.R.', 'RQR', 'RRR', '.R.'], 12, 1, 'f2']];
    case 'fail': return [...RPOT, ...leaves([[F.s, 2, 5, { h: 1 }], [F.s, 6, 2]], 8, 10), ...line(8, 10, 11, 6), [recolor(F.s, YELLOW), 11, 4, 'f1'], [recolor(flipV(F.s), YELLOW), 12, 10, 'f2']];
  }
}
// The Events rows: an action's row sprite is its tool beside a small plant.
export const ROWACT = {
  water: [[['.ee....', 'e..e..C', 'eCCCCC.', '.CCCC..', '.eeee..'], 0, 5], [['Z', '.', 'Z'], 7, 7, 'f1'], [['.', 'Z', '.', 'Z'], 7, 7, 'f2'], ...rowPlant('monstera', 'impl').map(a => [a[0], a[1] + 4, a[2], a[3]])],
  prune: [...rowPlant('philodendron', 'review').filter(a => a[3] !== 'f2').map(a => [a[0], a[1] + 3, a[2]]), [['hh..N', '.hN..', 'hh..N'], 0, 3, 'f1'], [['hh...', '.hNNN', 'hh...'], 0, 3, 'f2']],
  inspect: [...rowPlant('peperomia', 'review').filter(a => a[3] !== 'f2').map(a => [a[0], a[1] + 3, a[2]]), [['.iii.', 'iIIIi', 'iIxIi', 'iIIIi', '.iii.', '....u', '.....u'], 0, 1, 'f1'], [['.iii.', 'iIIIi', 'iIxIi', 'iIIIi', '.iii.', '....u', '.....u'], 1, 2, 'f2']],
  repot: [...rowPlant('alocasia', 'impl').map(a => [a[0], a[1] + 4, a[2], a[3]]), [['uuNN', '..NNN', '...d'], 0, 6, 'f1'], [['uuNN', '..NNN', '.....', '...d'], 1, 5, 'f2'], [['KKKK', 'KddK', 'KKKK', 'KcKK', 'KKKK'], 0, 11]],
  idle: [[['FFFf.', 'FFFff', 'FFFf.', 'fff..'], 4, 9], [['.x', 'x.', '.x'], 5, 5, 'f1'], [['x.', '.x', 'x.'], 5, 5, 'f2']],
};
// The 16 px sprites for the contract's `row_sprites`: by action (Events), by kind, and the stage set (Queue rows).
export function rows() {
  const R = {
    water: ROWACT.water, prune: ROWACT.prune, inspect: ROWACT.inspect, repot: ROWACT.repot, idle: ROWACT.idle,
    done: rowPlant('monstera', 'ready'), wilted: rowPlant('monstera', 'blocked'),
    jar: rowPlant('pothos', 'queued'), thirsty: rowPlant('calathea', 'needs'), sold: rowPlant('monstera', 'merged'), fall: rowPlant('monstera', 'fail'),
  };
  return R;
}

// ---- rasterising: layers → <rect> runs, one <g> per class ----
function rects(map, x0, y0, u) {
  let s = '';
  map.forEach((row, j) => {
    let i = 0;
    while (i < row.length) {
      const c = row[i];
      if (c === '.' || c === ' ' || !INK[c]) { i++; continue; }
      let n = 1;
      while (row[i + n] === c) n++;
      const [fill, op] = Array.isArray(INK[c]) ? INK[c] : [INK[c], 1];
      s += `<rect x="${(x0 + i) * u}" y="${(y0 + j) * u}" width="${n * u}" height="${u}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
      i += n;
    }
  });
  return s;
}
const HATVAR = { 'hat-t': ['--ht', 1], 'hat-b': ['--hb', 0], 'hat-k': ['--hk', 0] };
export function draw(layers, u = 4) {
  return layers.map(([map, x, y, cls]) => {
    const r = rects(map, x, y, u);
    if (!cls) return r;
    const hv = HATVAR[cls];
    return `<g class="${cls}"${hv ? ` style="opacity:var(${hv[0]},${hv[1]})"` : ''}>${r}</g>`;
  }).join('');
}
// Symbols for a page: g-<action>, p-<species>-<stage> (132 × 100), r-<row>, rs-<species>-<stage> (16 × 16), m-<moment>.
export function symbols() {
  const sym = (id, vb, body) => `<symbol id="${id}" viewBox="${vb}" shape-rendering="crispEdges">${body}</symbol>`;
  let s = '';
  for (const [k, f] of Object.entries(ACTIONS)) s += sym(`g-${k}`, '0 0 132 100', draw(f()));
  for (const { id } of SPECIES) for (const st of STAGES) s += sym(`p-${id}-${st}`, '0 0 132 100', draw(plant(id, st)));
  for (const [k, f] of Object.entries(MOMENTS)) s += sym(`m-${k}`, '0 0 132 100', draw(f()));
  s += sym('m-drop', '0 0 132 100', draw(dropBubble(13, 0)));
  for (const [k, m] of Object.entries(rows())) s += sym(`r-${k}`, '0 0 16 16', draw(m, 1));
  for (const { id } of SPECIES) for (const st of [...STAGES, 'needs', 'fail']) s += sym(`rs-${id}-${st}`, '0 0 16 16', draw(rowPlant(id, st), 1));
  return s;
}

// The per-lane looks: hat × skin × hair × shirt × apron, 8 of them like the farm's coats (the same stable pick).
const SKIN = { light: ['#f1c7a5', '#d9a684'], medium: ['#d9a074', '#b98157'], tan: ['#b97d50', '#9a6340'], deep: ['#8a5a3c', '#6e452d'] };
const APRON = { sage: ['#6f8f5a', '#58744a'], moss: ['#4f6b3a', '#3e552e'], clay: ['#a8603e', '#8a4c30'], denim: ['#4f6f8f', '#3f5a75'] };
const SHIRT = { cream: ['#efe6d0', '#cfc3a6'], rose: ['#e7b7a8', '#c99484'], sky: ['#bcd3e2', '#98b3c6'], butter: ['#efd98f', '#cdb76a'] };
const HATC = { red: ['#c0453a', '#9a352d'], mustard: ['#d9a53a', '#b08528'], teal: ['#3f8f8a', '#2f6e6a'] };
const HAT = { sun: [1, 0, 0], scarf: [0, 1, 0], bun: [0, 0, 1] };
export const LOOKS = [
  ['sun', 'red', 'light', '#5a3a24', 'cream', 'sage'],
  ['scarf', 'red', 'medium', '#1d1a17', 'butter', 'denim'],
  ['bun', 'red', 'deep', '#1d1a17', 'rose', 'moss'],
  ['sun', 'red', 'tan', '#d8c9a8', 'sky', 'clay'],
  ['scarf', 'mustard', 'light', '#b5652d', 'cream', 'moss'],
  ['bun', 'red', 'medium', '#3a2a1e', 'sky', 'sage'],
  ['sun', 'red', 'deep', '#1d1a17', 'butter', 'clay'],
  ['scarf', 'teal', 'tan', '#5a3a24', 'rose', 'denim'],
].map(([hat, hc, sk, hr, sh, ap]) => ({
  hat, sk,
  vars: { '--sk': SKIN[sk][0], '--ss': SKIN[sk][1], '--hr': hr, '--sh': SHIRT[sh][0], '--sd': SHIRT[sh][1], '--ap': APRON[ap][0], '--as': APRON[ap][1],
    '--hc': HATC[hc][0], '--hs': HATC[hc][1], '--ht': HAT[hat][0], '--hb': HAT[hat][1], '--hk': HAT[hat][2] },
}));
export const FLEET = { l: '#58a6ff', p: '#bc8cff', c: '#39c5cf', g: '#3fb950', a: '#d29922' };
export const style = (look, fk = 'l') => Object.entries({ ...LOOKS[look].vars, '--fk': FLEET[fk] || fk }).map(([k, v]) => `${k}:${v}`).join(';');

// ---- the shop (scene.svg): one repo's corner of the shop, 392 × 188, one pixel = 4. The window on the left shows the
// load as light (`.l-sun`, `.l-overcast`, `.l-rain`; the page's `light-<level>` class shows one); the shelves on the right
// are the repo's (`--fk` on the shelf edges); the counter runs along the foot. ----
const SHELFPOT = (x, y, sp, k) => {
  const F = { monstera: '#3f8f45', philodendron: '#4f9a3a', peperomia: '#3c6b3a', alocasia: '#2b5a45', pothos: '#5aa040', calathea: '#4a7c50' }[sp];
  const V = { monstera: '#6fb85f', philodendron: '#b4dc5a', peperomia: '#c2dcae', alocasia: '#b9d9a9', pothos: '#ecd96a', calathea: '#d27a98' }[sp];
  const shapes = [
    [[1, -5, 1, 2], [0, -4, 3, 2], [3, -6, 2, 3], [-1, -3, 2, 2]],
    [[0, -6, 2, 4], [2, -5, 2, 3], [-1, -4, 2, 3], [3, -3, 1, 2]],
    [[1, -7, 2, 5], [-1, -5, 2, 3], [3, -4, 2, 3]],
  ][k % 3];
  let s = '';
  for (const [dx, dy, w, h] of shapes) s += `<rect x="${(x + dx) * 4}" y="${(y + dy) * 4}" width="${w * 4}" height="${h * 4}" fill="${F}"/>` + `<rect x="${(x + dx) * 4}" y="${(y + dy) * 4}" width="4" height="4" fill="${V}"/>`;
  return s + `<rect x="${(x - 1) * 4}" y="${(y - 1) * 4}" width="20" height="4" fill="#e08a5c"/><rect x="${x * 4}" y="${y * 4}" width="12" height="8" fill="#c8643c"/><rect x="${(x + 2) * 4}" y="${y * 4}" width="4" height="8" fill="#9a4a2a"/>`;
};
export function shop(u = 4) {
  const W = 98, H = 47, r = (x, y, w, h, fill, extra = '') => `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${fill}"${extra}/>`;
  let s = r(0, 0, W, H, '#ead9b8');
  // plaster wall with a faint brick course, wood wainscot below the counter line
  for (let y = 2; y < 33; y += 3) for (let x = (y % 6 === 2 ? 0 : 3); x < W; x += 6) s += r(x, y, 5.5, 2.75, '#e3cfaa', ' opacity=".55"');
  s += r(0, 0, W, 2, '#6b4a30') + r(0, 2, W, 0.5, '#4f3522');
  // the window: wood frame, four panes; sky and weather by level
  const wx = 3, wy = 4, ww = 36, wh = 26;
  s += r(wx - 1, wy - 1, ww + 2, wh + 2, '#7a5236');
  const sky = (cls, top, bot) => `<g class="${cls}"><rect x="${wx * u}" y="${wy * u}" width="${ww * u}" height="${wh * u}" fill="url(#sky-${cls})"/></g>` +
    `<linearGradient id="sky-${cls}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bot}"/></linearGradient>`;
  s += sky('l-sun', '#8fcbe8', '#d4eef5') + sky('l-overcast', '#aab3b8', '#cfd5d6') + sky('l-rain', '#4f5d68', '#7b8790');
  // outdoors: a hedge and a lamp post under the sky
  s += r(wx, wy + 19, ww, 7, '#5f8a4a') + r(wx, wy + 19, ww, 1, '#78a35c');
  for (const x of [5, 13, 22, 30]) s += r(wx + x, wy + 18, 4, 2, '#5f8a4a');
  s += `<g class="l-sun">${r(29, 7, 6, 6, '#ffd25a')}${r(28, 8, 8, 4, '#ffd25a')}${r(30, 6, 4, 8, '#ffd25a')}${r(30, 8, 3, 3, '#fff0b0')}` +
    `${r(8, 9, 7, 2, '#ffffff', ' opacity=".85"')}${r(10, 8, 4, 1, '#ffffff', ' opacity=".85"')}</g>`;
  const cloud = (x, y, c, d) => r(x, y + 1, 10, 3, c) + r(x + 2, y, 6, 1, c) + r(x + 1, y + 4, 9, 1, d);
  s += `<g class="l-overcast">${cloud(6, 7, '#e9ecec', '#c4cacb')}${cloud(22, 9, '#dfe3e3', '#b9c0c2')}${cloud(14, 5, '#f1f3f3', '#cdd2d3')}</g>`;
  s += `<g class="l-rain">${cloud(5, 5, '#6b7781', '#56626c')}${cloud(20, 6, '#727e88', '#5a6670')}${cloud(12, 4, '#7b8791', '#636f79')}${r(wx, wy + 19, ww, 7, '#3f5a36', ' opacity=".55"')}</g>`;
  const rain = k => { let t = ''; for (let i = 0; i < 12; i++) { const x = wx + 1 + ((i * 7 + k * 3) % (ww - 2)), y = wy + 2 + ((i * 5 + k * 4) % (wh - 6)); t += r(x, y, 0.5, 3, '#cfe3ef', ' opacity=".8"') + r(x - 0.5, y + 3, 1.5, 1, '#e6f2f8', ' opacity=".7"'); } return t; };
  s += `<g class="l-rain f1">${rain(0)}</g><g class="l-rain f2">${rain(1)}</g>`;
  // glazing bars, sill, a small plant on it
  s += r(wx + ww / 2 - 0.5, wy, 1, wh, '#7a5236') + r(wx, wy + wh / 2 - 0.5, ww, 1, '#7a5236');
  s += r(wx - 2, wy + wh + 1, ww + 4, 1.5, '#9a6a44') + r(wx - 2, wy + wh + 2.5, ww + 4, 0.5, '#6b4a30');
  s += SHELFPOT(8, 29, 'peperomia', 2) + SHELFPOT(30, 29, 'calathea', 1);
  // a hanging pothos from the ceiling, by the window
  s += r(43, 2, 0.5, 6, '#6b4a30') + r(41, 8, 5, 3, '#c8643c') + r(41, 8, 5, 1, '#e08a5c');
  for (const [x, y] of [[41, 11], [40, 13], [41, 15], [45, 11], [46, 13], [45, 15], [46, 17]]) s += r(x, y, 2, 2, '#5aa040') + r(x, y, 1, 1, '#ecd96a');
  // the repo's shelves: three planks, their edge in the repo colour, brackets, pots of every species
  const sp = ['monstera', 'philodendron', 'peperomia', 'alocasia', 'pothos', 'calathea'];
  for (const [i, y] of [[0, 11], [1, 21], [2, 31]]) {
    s += r(50, y, 46, 1.5, '#9a6a44') + r(50, y + 1.5, 46, 0.75, 'var(--fk)') + r(53, y + 2.25, 1, 2, '#6b4a30') + r(92, y + 2.25, 1, 2, '#6b4a30');
    for (let k = 0; k < 6; k++) s += SHELFPOT(52 + k * 7 + (i % 2) * 2, y - 2, sp[(k + i * 2) % 6], k + i);
  }
  // the counter along the foot: wood, its edge in the repo colour; a till and a bell
  s += r(0, 36, W, 1.5, '#b07a4a') + r(0, 37.5, W, 0.75, 'var(--fk)') + r(0, 38.25, W, H - 38.25, '#8a5a34');
  for (const x of [2, 26, 50, 74]) s += r(x, 40, 21, 6, '#7a4e2c') + r(x + 1, 41, 19, 0.5, '#9a6a44');
  s += r(78, 31, 10, 5, '#5b6168') + r(79, 32, 8, 2, '#a9d3a0') + r(76, 34, 14, 2, '#4a4f55');
  s += r(66, 34, 4, 2, '#e2c27a') + r(67, 33, 2, 1, '#e2c27a') + r(66, 35.5, 4, 0.5, '#b8954e');
  // sunbeams through the window (sun only), the room dimmer under cloud and rain
  s += `<g class="l-sun beams"><path d="M${(wx + 4) * u} ${wy * u}L${(wx + ww) * u} ${wy * u}L${(wx + ww + 30) * u} ${H * u}L${(wx + 18) * u} ${H * u}Z" fill="#fff3c4" opacity=".28"/></g>`;
  s += `<g class="l-overcast">${r(0, 0, W, H, '#5a6a72', ' opacity=".10"')}</g><g class="l-rain">${r(0, 0, W, H, '#20303c', ' opacity=".22"')}</g>`;
  return `<svg viewBox="0 0 ${W * u} ${H * u}" preserveAspectRatio="xMidYMax slice" shape-rendering="crispEdges" aria-hidden="true"><defs></defs>${s}</svg>`;
}

// The window light in miniature for the Host card header (#370 `minis`, 48 × 16): a window with sun, cloud or rain.
export function mini(level, frame = 1) {
  const sky = { sun: '#8fcbe8', overcast: '#aab3b8', rain: '#4f5d68' }[level];
  let s = `<rect width="48" height="16" fill="#7a5236"/><rect x="2" y="2" width="44" height="12" fill="${sky}"/><rect x="2" y="11" width="44" height="3" fill="#5f8a4a"/>`;
  if (level === 'sun') s += '<rect x="35" y="3" width="6" height="6" fill="#ffd25a"/><rect x="36" y="4" width="2" height="2" fill="#fff0b0"/><rect x="2" y="2" width="44" height="12" fill="#fff3c4" opacity=".15"/>';
  if (level === 'overcast') s += '<rect x="6" y="4" width="12" height="4" fill="#e9ecec"/><rect x="8" y="3" width="7" height="1" fill="#e9ecec"/><rect x="26" y="5" width="12" height="4" fill="#dfe3e3"/><rect x="28" y="4" width="6" height="1" fill="#dfe3e3"/>';
  if (level === 'rain') {
    s += '<rect x="4" y="3" width="16" height="4" fill="#6b7781"/><rect x="24" y="3" width="18" height="4" fill="#727e88"/>';
    for (const k of [1, 2]) { let d = ''; for (let i = 0; i < 9; i++) d += `<rect x="${4 + i * 5 + (k - 1) * 2}" y="${7 + ((i + k) % 3)}" width="1" height="3" fill="#cfe3ef"/>`; s += `<g class="f${k}">${d}</g>`; }
  }
  s += '<rect x="23.5" y="2" width="1" height="12" fill="#7a5236"/>';
  return `<svg class="hm" viewBox="0 0 48 16" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// ---- #241 `icons`: a monstera leaf on the linen tile (32 × 32 grid, drawn procedurally so the slits stay even) ----
function monsteraLeaf(N = 32, k = 1) {
  // k scales the grid (1: 32 px icon grid, 0.5: the 16 px favicon); three slits a side, open at the margin, one hole
  // in each gap by the midrib; the light half faces the window (left), the right half is the leaf's shade.
  const g = Array.from({ length: N }, () => Array(N).fill('.'));
  const cx = N / 2, cy = 13.2 * k, rx = 12.8 * k, ry = 11.6 * k, slits = [-5.6, 0.2, 6].map(v => v * k), sw = Math.max(0.55, 0.95 * k);
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    const X = x + 0.5 - cx, Y = y + 0.5 - cy, dx = Math.abs(X);
    if ((X / rx) ** 2 + (Y / ry) ** 2 > 1) continue;
    if (Y > 6 * k && dx < (Y - 6 * k) * 1.1) continue; // the heart's notch at the stem
    if (slits.some(y0 => Math.abs(Y - (y0 - 0.62 * dx)) < sw && dx > 4.6 * k)) continue;
    if (k === 1 && [-2.7, 3.1].some(y0 => (dx - 3) ** 2 / 1.2 + (Y - (y0 - 0.62 * 3)) ** 2 / 0.9 < 1)) continue;
    g[y][x] = dx < 0.6 + 0.4 * k ? 'V' : X < 0 ? 'L' : 'l';
  }
  const by = Math.round(cy + ry * 0.62);
  for (let y = by; y < N; y++) { const x = Math.round(cx - 1 + (y - by) * 0.3); g[y][x] = 't'; if (k === 1) g[y][x + 1] = 't'; }
  return g.map(r => r.join(''));
}
export function icon(size = 512, { tile = '#f1e7cf', round = 0 } = {}) {
  const u = 16, st = '--lf:#4a9a48;--ld:#2f7036;--lv:#a6d98c';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 512 512" shape-rendering="crispEdges" style="${st}">` +
    `<rect width="512" height="512" rx="${round}" fill="${tile}"/><rect y="448" width="512" height="64" fill="#c8643c"/><rect y="448" width="512" height="16" fill="#e08a5c"/>` +
    `${draw([[monsteraLeaf(32), 0, 1]], u)}</svg>`;
}
// The favicon (16 px): the leaf alone, readable at 16.
export function favicon(size = 32) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 16 16" shape-rendering="crispEdges" style="--lf:#3f8f45;--ld:#2a6a34;--lv:#9fd68a"><rect width="16" height="16" fill="#f1e7cf"/>${draw([[monsteraLeaf(16, 0.5), 0, 0]], 1)}</svg>`;
}

// ---- ascii (#335): the Lanes cells (≤ 3 lines of 8), the row figures (≤ 3 characters), the Host load, the loading frames ----
export const ASCII = {
  impl: ' o~. \\|/\n |=|  |\n     \\_/', fix: ' 8<  \\|/\n      |/\n     \\_/', rev: ' (o) \\|/\n   \\  |\n     \\_/',
  update: ' =.  \\|/\n [#]  |\n     \\_/', idle: ' c[]  \\|/\n       |\n      \\_/', done: ' *  \\Y/*\n    \\|/\n    \\_/', blocked: ' !  ,  ,\n    \\|/\n    \\_/',
};
export const ASCII_ROWS = { impl: '~.Y', fix: '8<Y', rev: 'oY.', update: '=uY', idle: 'c.Y', done: '*Y*', blocked: ',v,' };
export const ASCII_STAGES = { queued: '[v]', impl: '.v.', review: 'vYv', fix: 'vY,', ready: '\\Y/', merged: '/Y\\', blocked: ',v,' };
export const ASCII_LOAD = { calm: '-O-', busy: '~~~', held: "','," };
export const ASCII_LOADING = '~.Y\n~:Y';
