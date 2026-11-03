// #239 kitchen theme, design mockup: every sprite, the station scene, the 16 px row sprites and the icon come from
// the pixel maps below (one character = one pixel). The page (index.html, sheet.html) imports this module; phase 2
// runs `node build.mjs` to write the theme files (sprites.svg, scene.svg, icons) from the same maps.
// Sprites keep the farm's box: 132 × 100, feet on y 92–96, shadow below, one pixel = 4 units (a 33 × 25 grid).
// Colours that vary per lane are CSS custom properties (the #241 `looks`); `--fk` is the project colour slot.

// One character → a fill (and an opacity). `.` and space are transparent.
export const INK = {
  k: '#2f2a26', B: '#2a2522', m: '#7a3b2e', b: '#ec9a82', q: ['#2f2a26', 0.16],
  S: 'var(--sk,#f1c7a5)', s: 'var(--ss,#d9a684)', M: 'var(--hr,#5a3a24)',
  W: '#f4f1ea', w: '#d6cfc1',
  A: 'var(--ap,#34506e)', a: 'var(--as,#2a4058)', T: 'var(--fk,#58a6ff)',
  P: '#3b3d4a', p: '#2c2e39',
  H: 'var(--hc,#c0453a)', h: 'var(--hs,#9a352d)',
  G: '#8d9399', g: '#5b6168', L: '#c9ced2', N: '#33363b', n: '#24262a',
  O: '#b07a4a', o: '#8a5a34', V: '#7fae5a', v: '#5b8a3e', R: '#d0563b', Q: '#e8833a', Y: '#e8b84a', y: '#b8892a',
  E: '#f7f5ef', e: '#d4cdbf', C: '#c4955a', c: '#96693a',
  F: 'var(--f1,#3d7fd9)', f: 'var(--f2,#9cc8ff)', Z: ['#ffffff', 0.55], D: '#8ec5ff',
  X: 'var(--tp, #fbf6e6)', x: 'var(--tl, #c9bfa5)', r: 'var(--th, #c9bfa5)', K: '#1f2a38',
};

// The farm's 8 coats (src/serve.rs COATS, the same sha256 pick) map one-to-one onto 8 looks: hat × skin × hair × apron.
// `--ht/--hb/--hk` switch the toque, bandana and cap (one is 1). `--fk` (apron trim) is the fleet's #162 colour.
const SKIN = { light: ['#f1c7a5', '#d9a684'], medium: ['#d9a074', '#b98157'], tan: ['#b97d50', '#9a6340'], deep: ['#8a5a3c', '#6e452d'] };
const APRON = { navy: ['#34506e', '#2a4058'], charcoal: ['#4a4a52', '#3a3a40'], wine: ['#7a3b3b', '#622f2f'], olive: ['#6b7040', '#555a33'] };
const HATC = { white: ['#f4f1ea', '#d6cfc1'], red: ['#c0453a', '#9a352d'], blue: ['#3d6fa8', '#305886'], mustard: ['#d9a53a', '#b08528'], green: ['#4f8a5b', '#3e6e48'] };
const HAT = { toque: [1, 0, 0], bandana: [0, 1, 0], cap: [0, 0, 1] };
export const LOOKS = [
  // coat --c        hat        colour     skin      hair       apron
  ['#8a5a33', 'toque', 'white', 'light', '#5a3a24', 'navy'],
  ['#a05c2c', 'bandana', 'red', 'medium', '#1d1a17', 'charcoal'],
  ['#3b3b44', 'cap', 'blue', 'deep', '#1d1a17', 'wine'],
  ['#b9bcc6', 'toque', 'white', 'tan', '#d8c9a8', 'olive'],
  ['#c89a56', 'bandana', 'mustard', 'light', '#b5652d', 'navy'],
  ['#b08d5e', 'cap', 'green', 'medium', '#3a2a1e', 'charcoal'],
  ['#96685a', 'toque', 'white', 'deep', '#1d1a17', 'wine'],
  ['#bd8f4e', 'cap', 'red', 'tan', '#5a3a24', 'olive'],
].map(([coat, hat, hc, sk, hr, ap]) => ({
  coat, hat, sk,
  vars: { '--sk': SKIN[sk][0], '--ss': SKIN[sk][1], '--hr': hr, '--ap': APRON[ap][0], '--as': APRON[ap][1],
    '--hc': HATC[hc][0], '--hs': HATC[hc][1], '--ht': HAT[hat][0], '--hb': HAT[hat][1], '--hk': HAT[hat][2] },
}));
export const FLEET = { l: '#58a6ff', p: '#bc8cff', c: '#39c5cf', g: '#3fb950', a: '#d29922' };
export const style = (look, fk = 'l') => Object.entries({ ...LOOKS[look].vars, '--fk': FLEET[fk] || fk }).map(([k, v]) => `${k}:${v}`).join(';');

// Heat (#241 `load_scene: {kind: "heat"}`): flame colours per level; the scene and the cook's burner share them.
export const HEAT = {
  calm: { '--f1': '#3d7fd9', '--f2': '#9cc8ff' },
  busy: { '--f1': '#ee7a22', '--f2': '#ffd15a' },
  held: { '--f1': '#e04a1e', '--f2': '#ffb03a' },
};

// ---- the chef, in parts (head at hx, hy; body one column wider each side) ----
const HEAD = [
  '.MMMMMMM.',
  'MSSSSSSSM',
  'SSkSSSkSS',
  'SSkSSSkSS',
  'SbSSSSSbS',
  'sSSSmSSSs',
  '.sSSSSSs.',
];
const HEAD_SHUT = ['.MMMMMMM.', 'MSSSSSSSM', 'SSSSSSSSS', 'SkkSSSkkS', 'SbSSSSSbS', 'sSSSmSSSs', '.sSSSSSs.'];
const HEAD_O = ['.MMMMMMM.', 'MSSSSSSSM', 'SSkSSSkSS', 'SSkSSSkSS', 'SbSSmSSbS', 'sSSSmSSSs', '.sSSSSSs.'];
const HEAD_SMILE = ['.MMMMMMM.', 'MSSSSSSSM', 'SSkSSSkSS', 'SSkSSSkSS', 'SbSSSSSbS', 'sSSmmmSSs', '.sSSSSSs.'];
const TOQUE = ['..WWW.WWW..', '.WWWWWWWWW.', 'WWWWWWWWWWW', 'WWWwWWWwWWW', '.WWWWWWWWW.', '.wwwwwwwww.'];
const BANDANA = ['..HHWHHWH...', '.HHHHHHHHHh.', '.........hHh'];
const CAP = ['...HHHHH...', '..HHHHHHH..', 'hhhhhhhhhhh'];
// Jacket and apron: the neckerchief in the project colour (fix 3), bib, skirt, the apron's own seams.
const BODY = [
  '..wTTTTTw..',
  '.WWWWTWWWW.',
  'WWaaaaaaaWW',
  'WWAAAAAAAWW',
  'WWAAaAAAAWW',
  'WWAAAAAAAWW',
  '.AAAAAAAAA.',
  '.AAAAAAaAA.',
  '.aaaaaaaaa.',
];
const LEGS = ['..PpP.PpP..', '..PPP.PPP..', '.BBBB.BBBB.'];
const WALK1 = ['.PpP...PpP.', 'PPP.....PPP', 'BBB......BBB'];
const WALK2 = ['...PpPPpP..', '...PPPPPP..', '..BBBBBBB..'];
const SHADOW = ['..qqqqqqqqqqq..'];
const ARM_L = ['W', 'W', 'W', 'w', 'S']; // hanging at the left side
const ARM_R = ['W', 'W', 'W', 'w', 'S'];

// A sprite = layers [map, x, y, class?] drawn in order; hats are three layers switched by --ht/--hb/--hk.
function chef(hx, hy, { head = HEAD, legs = LEGS, arms = 'lr', walk = 0 } = {}) {
  const L = [[SHADOW, hx - 3, hy + 19]];
  L.push([walk ? WALK1 : legs, hx - 1, hy + 16, walk ? 'f1' : null]);
  if (walk) L.push([WALK2, hx - 1, hy + 16, 'f2']);
  L.push([BODY, hx - 1, hy + 7]);
  if (arms.includes('l')) L.push([ARM_L, hx - 2, hy + 8]);
  if (arms.includes('r')) L.push([ARM_R, hx + 10, hy + 8]);
  L.push([head, hx, hy]);
  L.push([TOQUE, hx - 1, hy - 5, 'hat-t'], [BANDANA, hx - 1, hy - 1, 'hat-b'], [CAP, hx - 1, hy - 2, 'hat-k']);
  return L;
}

// ---- the actions (#241 `actions`): impl cook, fix chop, rev taste, update carry, idle wait, done serve, blocked bell;
// plus fan (held heat: standby chefs fan the fire) and the rail ticket. f1/f2 are the two frames of a loop.
const STOVE = [
  'nnnnnnnnnnnnn',
  'GLLLLLLLLLLLG',
  'GgkgggggggkgG',
  'GgNNNNNNNNNgG',
  'GgNnnnnnnnNgG',
  'GgNNNNNNNNNgG',
  'GgggggggggggG',
  'GGGGGGGGGGGGG',
];
const FLAME1 = ['..F...F..', '.FfF.FfF.', 'FfYfFfYfF', 'FfffffffF'];
const FLAME2 = ['.F...F...', 'FfF.FfF.F', 'FfYfFfYfF', 'FfffffffF'];
const WOK_A = ['NVRQVYQVN', '.NNNNNNN.', '..NNNNN..'];
const WOK_B = ['N.......N', '.NNNNNNN.', '..NNNNN..'];
const TOSS = ['.V..R.', '...Q..', 'Y...V.'];
const STEAM = ['.Z..Z.', 'Z..Z..', '.Z...Z'];
const TABLE = [
  'LLLLLLLLLLLLLL',
  'GGGGGGGGGGGGGG',
  '.g..........g.',
  '.g..........g.',
  '.gGGGGGGGGGGg.',
  '.g..........g.',
  '.g..........g.',
];
const BOARD = ['OOOOOOOOOOO', 'ooooooooooo'];
const VEG = ['QQQQQ.Q.Q.VV', '.....Q.Q..VV'];
const KNIFE_UP = ['NNLLLLL', '..GGGGG'];
const PASS = ['LLLLLLLLLLLLL', 'GGGGGGGGGGGGG', '.g.........g.', '.g.........g.', '.g.........g.', '.g.........g.', '.g.........g.'];
const LAMP = ['ggggggggggg', '.NNNNNNNNN.', '..Y.....Y..'];
const PLATE = ['..VQR..', '.EEEEE.', 'eeeeeee'];
const CRATE = ['.V.QV.VQ.', 'CCCCCCCCC', 'CcCcCcCcC', 'CCCCCCCCC', 'CcCcCcCcC', 'CCCCCCCCC'];
const BELL = ['...N...', '..YYY..', '.YEYYY.', '.YEYYY.', 'yyyyyyy'];
const RING = ['Y.....Y', '.Y...Y.'];
const STAND = ['ooooooooooo', 'OOOOOOOOOOO', '.o.......o.', '.o.......o.', '.o.......o.', '.o.......o.'];
const TOWEL_UP = ['WWWW', 'wWWW', '.wWW', '..wW'];
const TOWEL_DN = ['W...', 'WW..', 'wWW.', '.wWW'];
const SWEAT = ['D', 'D'];

export const ACTIONS = {
  cook: () => [
    ...chef(5, 5, { arms: 'l' }),
    [STOVE, 18, 16],
    [FLAME1, 20, 12, 'f1 fl'], [FLAME2, 20, 12, 'f2 fl'],
    [STEAM, 21, 0, 'steam'],
    // the wok, the right arm and the toss: frame 1 at rest, frame 2 lifted with the food in the air
    [['W', 'WS'], 14, 11, 'f1'], [['oooN'], 16, 11, 'f1'], [WOK_A, 19, 9, 'f1'],
    [['.S', 'W.'], 14, 10, 'f2'], [['oooN'], 16, 9, 'f2'], [WOK_B, 19, 8, 'f2'], [TOSS, 20, 3, 'f2'],
  ],
  chop: () => [
    ...chef(5, 5, { arms: 'l' }),
    [TABLE, 18, 17], [BOARD, 19, 15], [VEG, 24, 13],
    [['WWWWS'], 14, 13, 'f1'], [KNIFE_UP, 19, 11, 'f1'],
    [['WWWWS'], 14, 14, 'f2'], [['NNLLLLL'], 19, 13, 'f2'], [['..Q.Q'], 19, 14, 'f2'],
  ],
  taste: () => [
    ...chef(6, 5, { arms: 'l', head: HEAD }).map(l => l[0] === HEAD ? [HEAD_SHUT, l[1], l[2], 'f2'] : l),
    [HEAD, 6, 5, 'f1'],
    [LAMP, 20, 3], [['..y.....y..', '.y.......y.'], 20, 6, 'steam'],
    [PASS, 19, 17], [PLATE, 22, 14],
    // the right arm bends up: a spoon at the mouth (frame 2, eyes shut) or held off (frame 1)
    [['.L', 'LS', '.W', '.W'], 15, 10, 'f1'],
    [['LL..', '..LS', '...W', '...W'], 10, 10, 'f2'], [['W'], 14, 13, 'f2'],
  ],
  carry: () => [
    ...chef(7, 5, { arms: '', walk: 1 }),
    [['W', 'W', 'S'], 6, 13], [CRATE, 10, 12], [['S'], 19, 15],
  ],
  wait: () => [
    ...chef(12, 5, { arms: '' }),
    // arms crossed over the bib (fix 1): upper arms at the sides, forearms crossing diagonally, hands at the elbows
    [['WS.......SW', 'W.ww...WW.W', 'W...WWW...W', 'W.WW...ww.W', '.WW.....ww.'], 11, 13],
  ],
  serve: () => [
    ...chef(5, 5, { arms: 'l', head: HEAD_SMILE }),
    [['WWWWS'], 14, 13], [PLATE, 18, 10],
    [['Y'], 26, 7, 'f1'], [['Y'], 17, 8, 'f2'],
  ],
  bell: () => [
    ...chef(6, 5, { arms: '', head: HEAD_O }),
    // the left arm waves overhead (two frames), the right hand rings the bell
    [['SS', 'WW', 'WW', 'WW', 'wW', 'wW', 'WW'], 3, 6, 'f1'], [['SS..', '.WW.', '..WW', '..WW', '..wW', '..wW', '..WW'], 1, 6, 'f2'],
    [STAND, 19, 18], [BELL, 21, 13],
    [['WWWWS'], 15, 13, 'f1'], [['WWWWS'], 15, 12, 'f2'], [RING, 21, 10, 'f2'],
  ],
  fan: () => [
    ...chef(8, 5, { arms: 'l', head: HEAD_O }),
    [SWEAT, 18, 6],
    [['W', 'W', 'WS'], 18, 10], [TOWEL_UP, 20, 7, 'f1'], [TOWEL_DN, 20, 11, 'f2'],
  ],
};
// The rail item (#241 `rail`): an order ticket on its clip; the needs-you variant sets --tp/--th red.
export const TICKET = () => [
  [['GGGGG', 'gGGGg'], 14, 0],
  [['XXXXXXXXXXXXXXXXX', 'XrrrrrrrrrrrrrrrX', 'XrrrrrrrrrrrrrrrX', 'XXXXXXXXXXXXXXXXX',
    'XxxxxxxxxxxxxXXXX', 'XXXXXXXXXXXXXXXXX', 'XxxxxxxxxXXXXXXXX', 'XXXXXXXXXXXXXXXXX',
    'XxxxxxxxxxxXXXXXX', 'XXXXXXXXXXXXXXXXX', 'XxxxxxXXXXXXXXXXX', 'XXXXXXXXXXXXXXXXX',
    'XXXXXXXXXXXXXXXXX', 'X.X.X.X.X.X.X.X.X'], 8, 2],
  [['qqqqqqqqqqqqqqqqq'], 9, 16],
];

// #241 `moments` (fix 5): each scene draws the station it names. Order up: the chef rings the bell at the serving
// pass and the plate slides out (f1 → f2). Sent back: the plate comes back to the cook line (the stove) under a red ✗.
const XMARK = ['R...R', '.R.R.', '..R..', '.R.R.', 'R...R'];
export const MOMENTS = {
  'order-up': () => [
    ...chef(3, 5, { arms: 'l', head: HEAD_SMILE }),
    [PASS, 16, 17], [BELL, 17, 12], [['WWWWS'], 12, 13],
    [PLATE, 24, 14, 'f1'], [PLATE, 26, 14, 'f2'], [RING, 17, 9, 'f2'],
  ],
  'sent-back': () => [
    ...chef(3, 5, { arms: 'l', head: HEAD_O }),
    [STOVE, 18, 16], [FLAME1, 20, 14, 'f1 fl'], [FLAME2, 20, 14, 'f2 fl'],
    [['WWWWS'], 12, 13], [PLATE, 21, 13], [XMARK, 22, 6],
  ],
};
// The words each moment says, in a bubble over the scene.
export const SAYS = { 'order-up': 'order up!', 'sent-back': 'sent back' };
export const bubble = t => `<rect x="60" y="0" width="72" height="16" rx="4" fill="#f4f1ea"/><text x="96" y="12" font-size="11" font-weight="700" font-family="ui-monospace,Menlo,monospace" text-anchor="middle" fill="#2f2a26">${t}</text>`;

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
// Symbols for a page: <symbol id="k-cook" viewBox="0 0 132 100">… (shape-rendering crisp, the pixel look).
export function symbols() {
  let s = '';
  for (const [k, f] of Object.entries(ACTIONS)) s += `<symbol id="k-${k}" viewBox="0 0 132 100" shape-rendering="crispEdges"><g class="ka ka-${k}">${draw(f())}</g></symbol>`;
  s += `<symbol id="k-ticket" viewBox="0 0 132 100" shape-rendering="crispEdges"><g class="ka ka-ticket">${draw(TICKET())}</g></symbol>`;
  for (const [k, m] of Object.entries(ROW)) s += `<symbol id="r-${k}" viewBox="0 0 16 16" shape-rendering="crispEdges">${draw(m, 1)}</symbol>`;
  return s;
}

// ---- #241 `row_sprites`: 16 px per action for the Queue and Events rows (one pixel = 1 px) ----
// Each is a chef's head (the look's hat) with the action's prop; the queued ticket and the sent-back plate too.
const RH = ['..WWWWW.', '.WWWWWWW', '.wwwwwww', '.SSSSSSS', '.SkSSSkS', '.SSSSSSS', '..sSSSs.'];
const RHB = ['........', '..HHHHH.', '.HHHHHHH', '.SSSSSSS', '.SkSSSkS', '.SSSSSSS', '..sSSSs.'];
const RHK = ['........', '..HHHHH.', 'hhhhhhhhh', '.SSSSSSS', '.SkSSSkS', '.SSSSSSS', '..sSSSs.'];
const rowHead = () => [[RH, 0, 1, 'hat-t'], [RHB, 0, 1, 'hat-b'], [RHK, 0, 1, 'hat-k'], [['..TTTTT..', 'WWWaTaWWW', '.AAAAAAA.'], 0, 8], [['.AAAAAAA.', '.aaaaaaa.'], 0, 11]];
export const ROW = {
  cook: [...rowHead(), [['.V.R....', 'N....N..', 'NNNNNNoo', '.NNNN...', '.FfF.F..', 'FfFfFfF.'], 8, 6], [['GGGGGGG'], 8, 12]],
  chop: [...rowHead(), [['LLLL..', 'LLLLoo', 'LLLL..'], 9, 4], [['.Q.QV.', 'OOOOOOO', 'ooooooo'], 9, 9]],
  taste: [...rowHead(), [['LL', '.L', '..L'], 8, 6], [['.VQR.', 'EEEEEE', 'eeeeee'], 10, 10]],
  carry: [...rowHead(), [['.V.QV.V.', 'CCCCCCCC', 'CcCcCcCc', 'CCCCCCCC', 'CcCcCcCc', 'CCCCCCCC'], 8, 7]],
  wait: [...rowHead(), [['W.......W', 'WWWWWWWWW', 'WSwwwwwSW'], 0, 9]],
  serve: [...rowHead(), [['.VQR..', 'EEEEEE', 'eeeeee'], 10, 5], [['WWS'], 8, 8]],
  bell: [...rowHead(), [['..N..', '.YYY.', 'YEYYY', 'YEYYY', 'yyyyy'], 10, 8], [['S', 'W'], 15, 2]],
  ticket: [[['..GGG..'], 4, 0], [['XXXXXXXXX', 'XrrrrrrrX', 'XXXXXXXXX', 'XxxxxxxXX', 'XXXXXXXXX', 'XxxxxXXXX', 'XXXXXXXXX', 'XxxxxxXXX', 'XXXXXXXXX', 'XXXXXXXXX', 'X.X.X.X.X'], 3, 1]],
  back: [[['..R...R', '...R.R.', '....R..', '...R.R.', '..R...R'], 8, 0], [['.VQR...', 'EEEEEEE', 'eeeeeee'], 0, 10], [['k...', 'kk..', 'kkkk', 'kk..', 'k...'], 0, 3]],
};

// ---- the station: one fleet's kitchen wall (392 × 188, one pixel = 4; xMidYMax, so 390 shows all of it) ----
// Layers by heat: `.h-calm`, `.h-busy`, `.h-held` (the page's class shows one); `--fk` paints the hood stripe and the
// counter trim, so each fleet's station carries its colour.
export function station(u = 4) {
  const W = 98, H = 47, r = (x, y, w, h, fill, extra = '') => `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${fill}"${extra}/>`;
  let s = r(0, 0, W, H, '#e6dcc6');
  for (let y = 3; y < 34; y += 4) s += r(0, y, W, 0.25, '#d3c5a8');
  for (let y = 3; y < 34; y += 8) for (let x = (y % 16 === 3 ? 0 : 3); x < W; x += 6) s += r(x, y, 0.25, 4, '#d3c5a8');
  s += r(0, 0, W, 3, '#3a332e') + r(0, 3, W, 0.5, '#2a2522');
  // left: utensil rail and shelf with jars
  s += r(4, 6, 34, 1, '#6a6a6a');
  for (const [x, h] of [[7, 6], [12, 8], [17, 5], [22, 7], [27, 6], [32, 8]]) s += r(x, 7, 1, h, '#5b6168') + r(x - 1, 7 + h, 3, 2, '#8d9399');
  s += r(3, 21, 36, 1.5, '#8a5a34') + r(3, 22.5, 36, 0.5, '#6b4428');
  for (const [x, w, h, c] of [[5, 4, 5, '#c0583a'], [10, 3, 4, '#e2b84a'], [14, 5, 6, '#6f8f5a'], [20, 3, 4, '#d9d2c3'], [24, 6, 5, '#8d9399'], [31, 4, 6, '#b07a4a']]) s += r(x, 21 - h, w, h, c) + r(x, 21 - h, w, 1, '#0002');
  // the hood over the range, its stripe in the fleet colour
  s += `<path d="M${50 * u} ${4 * u}H${92 * u}L${96 * u} ${14 * u}H${46 * u}Z" fill="#8d9399"/>` + r(46, 14, 50, 1.5, '#5b6168') + r(48, 11, 46, 1.5, 'var(--fk)') + r(66, 3.5, 10, 0.5, '#5b6168');
  // left counter (prep) with a board and bowls; the range on the right with three burners
  s += r(0, 33, 42, 1.5, '#c9ced2') + r(0, 34.5, 42, 0.5, '#8d9399') + r(0, 35, 42, 12, '#7b5b43');
  for (const x of [2, 15, 28]) s += r(x, 37, 11, 9, '#8d6b50') + r(x + 9, 40, 1, 3, '#d5cfc4');
  s += r(0, 35, 42, 0.75, 'var(--fk)');
  s += r(4, 31, 12, 2, '#b07a4a') + r(19, 30, 6, 3, '#d9d2c3') + r(27, 31, 8, 2, '#6f8f5a');
  s += r(44, 33, 54, 1.5, '#c9ced2') + r(44, 34.5, 54, 12.5, '#33363b') + r(44, 35, 54, 0.75, 'var(--fk)');
  for (const x of [47, 65, 83]) s += r(x, 38, 14, 7, '#24262a') + r(x + 1, 39, 12, 1, '#5b6168');
  for (const x of [50, 59, 68, 77, 86]) s += r(x, 36, 1.5, 1, '#c9ced2');
  // pots on the burners
  const pots = [[49, 26, 11], [66, 25, 12], [84, 27, 10]];
  for (const [x, y, w] of pots) s += r(x, y, w, 33 - y - 2, '#5b6168') + r(x - 1, y, w + 2, 1, '#8d9399') + r(x + w / 2 - 1, y - 1, 2, 1, '#33363b');
  // flames: calm small blue, busy taller orange (+ steam), held flares (+ wall flames, smoke, glow); two frames each
  const flame = (x, y, hgt, k) => { let f = ''; for (let i = 0; i < 5; i++) { const t = hgt - ((i + k) % 2); f += r(x + i * 2, y + 2 - t, 1, t, 'var(--f1)') + r(x + i * 2, y + 2 - Math.max(1, t - 1), 1, Math.max(1, t - 1), 'var(--f2)', ' opacity=".9"'); } return f; };
  const lvl = (cls, hgt) => [0, 1].map(k => `<g class="${cls} f${k + 1}">${pots.map(([x, y, w]) => flame(x + 1, 31, hgt, k)).join('')}</g>`).join('');
  s += lvl('h-calm', 1) + lvl('h-busy', 2) + lvl('h-held', 3);
  let steam = '';
  for (const [x, y, w] of pots) for (const [dx, dy] of [[2, -4], [5, -7], [3, -10], [6, -13]]) steam += r(x + dx, y + dy, 2, 2, '#ffffff', ' opacity=".45"');
  s += `<g class="h-busy h-held steam">${steam}</g>`;
  // held: flames lick the wall up both sides of the range and between the pots, smoke under the ceiling, a red glow
  const tongue = (x, top, k) => { let t = ''; for (let i = 0; i < 4; i++) { const y = top + ((i + k) % 2) * 2 + Math.abs(i - 1.5) * 2; t += r(x + i, y, 1, 33 - y, 'var(--f1)') + r(x + i, y + 3, 1, 30 - y, 'var(--f2)', ' opacity=".9"'); } return t; };
  s += [0, 1].map(k => `<g class="h-held f${k + 1}">${[[44, 14], [61, 18], [79, 17], [94, 15]].map(([x, top]) => tongue(x, top, k)).join('')}</g>`).join('');
  s += `<g class="h-held smoke">${r(0, 3.5, W, 3, '#4a4440', ' opacity=".55"')}${r(0, 6.5, W, 2, '#4a4440', ' opacity=".35"')}${[6, 20, 37, 55, 71, 88].map((x, i) => r(x, 8.5, 8 + (i % 3) * 2, 1.5, '#4a4440', ' opacity=".3"')).join('')}</g>`;
  s += `<rect class="h-held glow" width="${W * u}" height="${H * u}" fill="url(#kglow)"/>`;
  return `<svg viewBox="0 0 ${W * u} ${H * u}" preserveAspectRatio="xMidYMax slice" shape-rendering="crispEdges" aria-hidden="true"><defs><radialGradient id="kglow" cx=".6" cy="1" r=".9"><stop offset="0" stop-color="#ff5a2a" stop-opacity=".45"/><stop offset="1" stop-color="#ff5a2a" stop-opacity="0"/></radialGradient></defs>${s}</svg>`;
}

// #241 `load_scene` in miniature for the Host card header: three burners, flame height by level (48 × 16, 1 px).
export function mini(level) {
  const hgt = { calm: 2, busy: 4, held: 6 }[level];
  let s = '<rect width="48" height="16" fill="#2a2522"/>';
  for (const x of [3, 19, 35]) {
    for (let i = 0; i < 5; i++) { const t = hgt - (i % 2); s += `<rect x="${x + i * 2}" y="${12 - t}" width="1" height="${t}" fill="var(--f1)"/><rect x="${x + i * 2}" y="${12 - Math.max(1, t - 2)}" width="1" height="${Math.max(1, t - 2)}" fill="var(--f2)"/>`; }
    s += `<rect x="${x - 1}" y="12" width="11" height="1" fill="#8d9399"/>`;
  }
  s += '<rect y="13" width="48" height="3" fill="#33363b"/>';
  if (level === 'held') s += '<rect width="48" height="3" fill="#6a605a" opacity=".7"/><rect width="48" height="16" fill="#ff5a2a" opacity=".18"/>';
  return `<svg class="hm" viewBox="0 0 48 16" shape-rendering="crispEdges" style="${Object.entries(HEAT[level]).map(([k, v]) => `${k}:${v}`).join(';')}" aria-hidden="true">${s}</svg>`;
}

// ---- #241 `icons`: a pixel chef (toque) with a pan over a blue flame, on the board's dark tile ----
export function icon(size = 512, { round = 0 } = {}) {
  const u = 16, g = 32; // a 32 × 32 grid
  const L = [
    [['...WWWW.WWWW....', '..WWWWWWWWWWW...', '.WWWWWWWWWWWWW..', '.WWWWwWWWWwWWW..', '..WWWWWWWWWWW...', '..wwwwwwwwwww...'], 5, 3],
    [['..MMMMMMMMMMM..', '.MSSSSSSSSSSSM.', '.SSSSSSSSSSSSS.', '.SSSkkSSSSkkSS.', '.SSSkkSSSSkkSS.', '.SbbSSSSSSSbbS.', '.SSSSSmmmSSSSS.', '..sSSSSSSSSSs..'], 4, 9],
    [['.....wwWWWWww......', '...WWWWWWWWWWWW....', '..WWTTTTTTTTTWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAaAAAAWW....',
      '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAaAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....', '..WWAAAAAAAAAWW....'], 2, 17],
    [['SS', 'WS'], 15, 19],
    [['ooooooNNN', '.....NNNNNNNNN', '......NNNNNNNN', '.......NNNNNN'], 15, 20],
    [['.VR.Q.Y.'], 21, 19],
    [['..F..F..F', '.FfF.FfF.F', 'FfffFfffFf'], 20, 24],
    [['GGGGGGGGGGGGGGGG'], 16, 27],
  ];
  const look = { '--sk': '#f1c7a5', '--ss': '#d9a684', '--hr': '#5a3a24', '--ap': '#34506e', '--as': '#2a4058', '--fk': '#d29922', '--f1': '#3d7fd9', '--f2': '#9cc8ff' };
  const st = Object.entries(look).map(([k, v]) => `${k}:${v}`).join(';');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${g * u} ${g * u}" shape-rendering="crispEdges" style="${st}">` +
    `<rect width="${g * u}" height="${g * u}" rx="${round}" fill="#1f2a38"/>${draw(L, u)}</svg>`;
}
// The favicon (16/32 px): the toque and the pan alone, readable at 16.
export function favicon(size = 32) {
  const L = [
    [['...WWW.WWW...', '.WWWWWWWWWWW.', 'WWWWWWWWWWWWW', 'WWWWWWWWWWWWW', '.WWWWWWWWWWW.', '.wwwwwwwwwww.'], 1, 0],
    [['.SSSSSSSSS.', 'SSkkSSSkkSS', 'SSkkSSSkkSS', 'SbbSSSSSbbS', 'SSSSmmmSSSS', '.sSSSSSSSs.'], 2, 6],
    [['.WWTTTTTWW.', 'WWWAAAAAWWW'], 2, 12],
    [['WWWAAAAAWWW'], 2, 14], [['WWWAAAAAWWW'], 2, 15],
  ];
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 16 16" shape-rendering="crispEdges" style="--sk:#f1c7a5;--ss:#d9a684;--ap:#34506e;--fk:#d29922"><rect width="16" height="16" fill="#1f2a38"/>${draw(L, 1)}</svg>`;
}

// #244 Q2: the kitchen's whole-board colour scheme (theme.json `board`): charcoal tile, copper controls and links, brass
// structure (borders, the title), warm cream text. Status colours stay green / amber / red, each clear of copper and brass
// (contrast.mjs checks both: WCAG ratio on every background, ΔE between colours that mean different things).
export const SCHEME = {
  '--bg': '#1a1816', '--fg': '#efe3cb', '--mute': '#a89a87', '--line': '#2e2a26',
  '--l': '#e89a62', '--p': '#d39ad0', '--c': '#62c9c0', '--g': '#6cc96e', '--a': '#f4d24e', '--r': '#ff5c84',
  '--brass': '#c9a55a', '--edge': '#4a3f2e', card: '#24201c', ask: '#2e1618',
};
