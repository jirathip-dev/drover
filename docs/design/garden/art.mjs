// #423 garden theme, design mockup (revision 1: finer pixel art). The shopkeeper, the six species at every growth stage,
// the moments, the shop scene, the window-light minis, the row sprites and the icon all come from this module: the keeper
// and the tools are pixel maps (one character = one pixel), the leaves are drawn per species by `leaf()` (shape, veins,
// fenestration, variegation, shading) onto a pixel canvas, so every species keeps the same hand at every size.
// index.html and sheet.html import this module; phase 2's build writes the theme files (sprites.svg, scene.svg, icons)
// from it.
// Scale (revision 1): 2 screen px per art pixel, half the farm's and the kitchen's 4. Lane sprites keep the farm's box,
// 132 × 100, one pixel = 2 units (a 66 × 50 grid), feet on y 46, shadow on y 47. The keeper stands on the left
// (x 0–27) and does the role; the plant stands on the right (x 36–62) at the issue's stage. Both are separate sprites drawn
// into the same box, so any action pairs with any stage (the contract's `stages`). Row sprites are a 24 × 24 grid drawn
// at the board's 24 px (one pixel = 1 CSS px, 2 device px on a phone). Per-lane colours are CSS custom properties
// (`looks`), per-issue leaf colours too (`species`); `--fk` is the repo colour.

const mix = (v, d, p, w) => `color-mix(in srgb,var(${v},${d}) ${p}%,${w})`;
// One character → a fill (and an opacity). `.` and space are transparent.
export const INK = {
  k: '#2b2420', q: ['#2b2420', 0.16], b: '#ec9a86', m: '#8a3b2e',
  S: 'var(--sk,#f1c7a5)', s: 'var(--ss,#d9a684)', M: 'var(--hr,#5a3a24)', 3: mix('--hr', '#5a3a24', 62, '#000'), 4: mix('--hr', '#5a3a24', 70, '#fff'),
  W: 'var(--sh,#e9e2cf)', w: 'var(--sd,#c9bfa5)', A: 'var(--ap,#5c7a4a)', a: 'var(--as,#46603a)', 0: mix('--ap', '#5c7a4a', 60, '#fff'), T: 'var(--fk,#58a6ff)', '^': mix('--fk', '#58a6ff', 65, '#000'),
  H: 'var(--hc,#c0453a)', h: 'var(--hs,#9a352d)', J: '#e8c983', j: '#c19d55', '~': '#a37f3e',
  P: '#4a3f36', p: '#3a312a', B: '#2a2320', '=': '#5a4a3e',
  // pot (rim light, body, shade, highlight, deep), soil (body, light, dark), stem (and its shade), roots
  U: '#e8936a', O: '#c8643c', o: '#9a4a2a', 5: '#f2b48e', 6: '#7a3820', d: '#5a3d2b', 7: '#86603f', 8: '#3e2a1e', t: '#4d7a34', '!': '#365a26', r: '#efe2c4',
  // leaves: the species' colours (--lf leaf, --ld its shade, --lv vein or variegation), the outline and gloss derived
  // from them; yellowed and brown leaves
  L: 'var(--lf,#3f8f45)', l: 'var(--ld,#2a6a34)', V: 'var(--lv,#6fb85f)', 1: mix('--ld', '#2a6a34', 60, '#0b160c'), 2: mix('--lf', '#3f8f45', 58, '#fff'),
  Y: '#e0bf4e', y: '#b08c32', n: '#8a6a3a', ';': '#5e4524',
  // glass jar and water, kraft paper and twine, the tag
  G: ['#d8eef0', 0.5], g: '#93bcc3', Z: '#6fb8e0', z: ['#6fb8e0', 0.45], K: '#d9b98a', c: '#b8956a', '-': '#ead2a8', X: '#9a3b2e', E: '#f7f1e3',
  // tools: the watering can, steel, the trowel's handle, the mug, the bell, the lens, the thirsty drop
  C: '#5f9aa6', e: '#3f6f7a', '+': '#9cd0d8', N: '#b9c1c5', i: '#6d7579', u: '#8a5a34', F: '#f1e8d6', f: '#c9bfa5',
  Q: '#ffffff', R: '#d9443a', '*': '#a8261f', D: '#8ec5ff', I: ['#cfe8f0', 0.7], x: ['#ffffff', 0.6],
};

// The six species (#423 Plants): per-issue, picked by issue number (n % 6, so neighbours always differ).
export const SPECIES = [
  { id: 'monstera', vars: { '--lf': '#3f8f45', '--ld': '#2a6a34', '--lv': '#7cc46a' } },
  { id: 'philodendron', vars: { '--lf': '#4f9a3a', '--ld': '#356f2a', '--lv': '#b4dc5a' } },
  { id: 'peperomia', vars: { '--lf': '#3c6b3a', '--ld': '#294c2b', '--lv': '#c2dcae' } },
  { id: 'alocasia', vars: { '--lf': '#2b5a45', '--ld': '#1c3f31', '--lv': '#c4e2b4' } },
  { id: 'pothos', vars: { '--lf': '#5aa040', '--ld': '#3d7a2c', '--lv': '#ecd96a' } },
  { id: 'calathea', vars: { '--lf': '#4a7c50', '--ld': '#2a4a31', '--lv': '#e08aa8' } },
];
export const speciesOf = n => SPECIES[((n % SPECIES.length) + SPECIES.length) % SPECIES.length];
export const sv = sp => Object.entries((typeof sp === 'number' ? speciesOf(sp) : SPECIES.find(s => s.id === sp)).vars).map(([k, v]) => `${k}:${v}`).join(';');

// ---- a pixel canvas: leaves and pots are painted onto it, then it is one layer ----
function canvas(W, H) {
  const g = Array.from({ length: H }, () => Array(W).fill('.'));
  const c = {
    set(x, y, ch) { x = Math.round(x); y = Math.round(y); if (ch && ch !== '.' && x >= 0 && y >= 0 && x < W && y < H) g[y][x] = ch; },
    get: (x, y) => (g[y] || [])[x] || '.',
    stamp(m, x, y) { m.forEach((row, j) => [...row].forEach((ch, i) => { if (ch !== ' ') c.set(x + i, y + j, ch); })); return c; },
    rows: () => g.map(r => r.join('')),
  };
  return c;
}
const flipH = m => m.map(r => [...r].reverse().join(''));
const recolor = (m, tr) => m.map(r => [...r].map(ch => tr[ch] || ch).join(''));
const YELLOW = { L: 'Y', l: 'y', V: 'Y', 1: 'n', 2: 'Y', t: 'y' }, WILT = { L: 'y', l: 'n', V: 'Y', 1: ';', 2: 'y', t: 'n', '!': 'n' };
const hash = (x, y) => { const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453; return s - Math.floor(s); };

// ---- leaves (#423 Plants): each species' shape and markings, drawn at any length and angle ----
// The leaf's base (where the stem meets it) is the pixel (bx, by); ang 0 points up, positive leans right. `big` is the
// mature leaf (the monstera splits). The light comes from the window, on the leaf's left half; 1 is the outline.
const PROF = {
  heart: t => t < 0.36 ? Math.sqrt(Math.max(0, 1 - ((0.36 - t) / 0.36) ** 2)) : Math.pow(Math.max(0, 1 - (t - 0.36) / 0.64), 0.8),
  round: t => t < 0.42 ? Math.sqrt(Math.max(0, 1 - ((0.42 - t) / 0.42) ** 2)) : Math.pow(Math.max(0, 1 - ((t - 0.42) / 0.58) ** 1.6), 0.6),
  oval: t => Math.pow(Math.max(0, Math.sin(Math.PI * Math.pow(t, 0.75))), 0.7),
  lance: t => Math.pow(Math.max(0, Math.sin(Math.PI * t)), 0.62),
};
const SHAPE = {
  monstera: { prof: 'round', v0: -0.12, w: 0.6, mid: 'V', lat: 'l', sweep: 0.5, gloss: 1, fen: 1 },
  philodendron: { prof: 'heart', v0: -0.14, w: 0.5, mid: 'V', lat: 'V', sweep: 0.8, gloss: 1 },
  peperomia: { prof: 'oval', v0: 0, w: 0.42, mid: 'l', stripes: 1 },
  alocasia: { prof: 'arrow', v0: -0.34, w: 0.46, mid: 'V', lat: 'V', sweep: 0.42 },
  pothos: { prof: 'heart', v0: -0.1, w: 0.44, mid: 'l', gloss: 1, varieg: 1 },
  calathea: { prof: 'lance', v0: 0, w: 0.3, mid: 'l', feather: 1 },
};
function inLeaf(S, v, a, len) {
  const W = len * S.w;
  if (v > 1 || v < S.v0) return null;
  if (S.prof === 'arrow') {
    if (v >= 0) { let h = Math.pow(1 - v, 0.95); if (v > 0.12 && len >= 9 && Math.sin(v * 30) > 0.5) h -= 1 / W; return a <= W * h + 0.2 ? h : null; }
    const s = v / S.v0;
    return a >= W * (0.1 + 0.62 * s) - 0.2 && a <= W * (1.02 - 0.24 * s) + 0.2 ? 1 : null;
  }
  const t = (v - S.v0) / (1 - S.v0), h = PROF[S.prof](t);
  if (S.v0 < 0 && v < 0.04 && a < (0.04 - v) * len * 1.5) return null;
  return a <= W * h + 0.2 ? h : null;
}
function leaf(c, sp, bx, by, len, ang = 0, o = {}) {
  const S = SHAPE[sp], A = ang * Math.PI / 180, sa = Math.sin(A), ca = Math.cos(A), W = len * S.w, R = Math.ceil(len * 1.3) + 2;
  const cell = new Map(), key = (x, y) => x + ',' + y;
  for (let y = -R; y <= R; y++) for (let x = -R; x <= R; x++) {
    const along = x * sa - y * ca, ac = x * ca + y * sa, v = along / len, a = Math.abs(ac);
    const h = inLeaf(S, v, a, len);
    if (h === null) continue;
    const un = a / Math.max(0.5, W * h);
    let cut = false;
    if (S.fen && o.big) {
      // three slits a side, from near the midrib out to the margin, widening as they open
      cut = [0.06, 0.3, 0.54].some(k => Math.abs(v - (k + 0.62 * a / len)) * len < 0.4 + 0.75 * un && un > 0.3);
    }
    cell.set(key(x, y), { v, ac, a, un, cut });
  }
  const sharp = len >= 7;
  for (const [k, p] of cell) {
    if (p.cut) continue;
    const [x, y] = k.split(',').map(Number);
    // the outline runs round the leaf's margin only, not round its slits and holes
    const edge = sharp && [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([dx, dy]) => !cell.has(key(x + dx, y + dy)));
    const { v, ac, a, un } = p;
    let ch = ac > 0.4 ? 'l' : 'L';
    if (S.stripes && len >= 5) ch = [1, 3].includes(Math.floor(un * 5)) ? 'V' : ch;
    if (S.feather && len >= 6) {
      const on = [0.15, 0.27, 0.39, 0.51, 0.63, 0.75, 0.87].some(q => Math.abs(v - (q + 0.25 * a / len)) * len < 0.5);
      ch = on && un > 0.22 && un < 0.8 ? 'V' : un < 0.5 ? 'l' : 'L';
    }
    if (S.varieg && len >= 5 && Math.sin((ac * 0.9 + v * len * 0.6) * 1.25 + 1.7) > 0.45 && hash(bx + x, by + y) > 0.3) ch = 'V';
    if (S.lat && !(S.fen && o.big) && len >= 9 && a > 0.8 && un < 0.86 && v > 0.04) {
      const ks = len >= 13 ? [0.12, 0.28, 0.44, 0.6, 0.76] : [0.2, 0.45, 0.7];
      if (ks.some(q => Math.abs(v - (q + S.sweep * a / len)) * len < 0.5)) ch = S.lat === 'l' && ch === 'l' ? '1' : S.lat;
    }
    if (S.gloss && len >= 8 && ac < -0.4 && un > 0.54 && un < 0.74 && v > 0.26 && v < 0.52) ch = '2';
    if (a < 0.55 && v > -0.02 && v < 0.9 && len >= 5) ch = S.mid;
    if (edge) ch = '1';
    if (o.tint) ch = o.tint[ch] || ch;
    c.set(bx + x, by + y, ch);
  }
}
// A stem: a quadratic curve from the soil (sx, sy) to the leaf's base, rising first, then bending out.
function stem(c, sx, sy, bx, by, ch = 't', bend = 0.2) {
  const cx = sx + (bx - sx) * bend, cy = by, n = Math.ceil(Math.hypot(bx - sx, by - sy) * 2) + 2;
  for (let i = 0; i <= n; i++) {
    const t = i / n, x = (1 - t) ** 2 * sx + 2 * (1 - t) * t * cx + t * t * bx, y = (1 - t) ** 2 * sy + 2 * (1 - t) * t * cy + t * t * by;
    c.set(x, y, ch);
  }
}
// Leaves with their stems: [bx, by, len, ang, {big, tint, sway, sx}]; the frame k sways the upper leaves.
function foliage(c, sp, list, sx, sy, k = 0, tint) {
  for (const [bx, by, len, ang, o = {}] of list) stem(c, o.sx ?? sx, sy, bx, by, tint ? (tint.t || 't') : 't', o.bend);
  for (const [bx, by, len, ang, o = {}] of list) leaf(c, sp, bx, by, len, ang + (k && (o.sway ?? by < sy - 9) ? (ang < -20 ? -5 : 5) : 0), { big: o.big, tint: o.tint || tint });
}

// ---- pots, the jar, the wrap ----
// A terracotta pot: a rim with its lip light and the shadow under it, a tapered body lit from the left, the soil above.
function pot(c, cx, top, w = 22, h = 13) {
  const x0 = cx - Math.floor(w / 2);
  for (let i = 2; i < w - 2; i++) c.set(x0 + i, top - 1, hash(i, top) > 0.75 ? '7' : hash(i, 3) > 0.8 ? '8' : 'd');
  const rimH = h >= 10 ? 3 : 2;
  for (let i = 0; i < w; i++) {
    c.set(x0 + i, top, i === 0 || i === w - 1 ? 'O' : 'U');
    if (rimH === 3) c.set(x0 + i, top + 1, i === 0 ? 'O' : i < 3 ? '5' : i > w - 4 ? 'O' : 'U');
    c.set(x0 + i, top + rimH - 1, rimH === 3 ? 'O' : i < 2 ? '5' : 'U');
  }
  for (let i = 1; i < w - 1; i++) c.set(x0 + i, top + rimH, 'o');
  for (let j = rimH + 1; j < h; j++) {
    const ins = 1 + Math.round((j - rimH - 1) * (w >= 16 ? 2.6 : 1.4) / Math.max(1, h - rimH - 2)), n = w - 2 * ins;
    for (let i = 0; i < n; i++) {
      let ch = i < 1 ? 'O' : i < (w >= 16 ? 4 : 2) && i >= 1 ? (i === 1 ? 'O' : '5') : i >= n - (w >= 16 ? 4 : 2) ? (i === n - 1 ? '6' : 'o') : 'O';
      if (j === h - 1) ch = i === 0 || i === n - 1 ? '.' : '6';
      c.set(x0 + ins + i, top + j, ch);
    }
  }
}
// The cutting's jar: glass, the water line, roots in the water, a highlight down the left.
function jar(c, cx, top, w = 12, h = 16, frame = 0) {
  const x0 = cx - Math.floor(w / 2);
  for (let i = 2; i < w - 2; i++) { c.set(x0 + i, top, 'g'); c.set(x0 + i, top + 1, i === 2 || i === w - 3 ? 'g' : 'G'); }
  for (let j = 2; j < h; j++) for (let i = 0; i < w; i++) {
    const edge = i === 0 || i === w - 1 || j === h - 1, water = j >= Math.round(h * 0.38);
    let ch = edge ? 'g' : water ? (j === Math.round(h * 0.38) ? 'Z' : 'z') : 'G';
    if ((j === 2) && (i === 0 || i === w - 1)) ch = '.';
    if (!edge && i === 1 && j > 2 && j < h - 2) ch = 'x';
    c.set(x0 + i, top + j, ch);
  }
  const r = [[0, 0], [-1, 1], [-2, 2], [-2, 3], [-3, 4], [1, 1], [2, 2], [2, 3], [3, 4], [0, 1], [0, 2], [1, 3], [0, 4]];
  for (const [dx, dy] of r) c.set(cx + dx, top + Math.round(h * 0.55) + dy + 1, 'r');
}
// Kraft paper wrapped round the sold plant: a cone with its folds, twine with a bow, a tag on a string.
function wrap(c, cx, top, h = 22, wTop = 30, wBot = 16) {
  for (let j = 0; j < h; j++) {
    const w = Math.round(wTop + (wBot - wTop) * j / (h - 1)), x0 = cx - Math.floor(w / 2);
    for (let i = 0; i < w; i++) {
      let ch = i === 0 || i === w - 1 ? 'c' : 'K';
      if (j < 3 && (i + j) % 5 === 0) ch = '.';
      if (!(i === 0 || i === w - 1) && (i === Math.round(w * 0.3 + j * 0.2) || i === Math.round(w * 0.68 - j * 0.15))) ch = 'c';
      if (!(i === 0 || i === w - 1) && i < 3 && j > 2) ch = '-';
      if (j === h - 1) ch = 'c';
      c.set(x0 + i, top + j, ch);
    }
  }
  const ty = top + Math.round(h * 0.42);
  for (let j = 0; j < 2; j++) { const w = Math.round(wTop + (wBot - wTop) * (ty - top + j) / (h - 1)); for (let i = 0; i < w; i++) c.set(cx - Math.floor(w / 2) + i, ty + j, 'X'); }
  c.stamp(['XX...XX', 'XXX.XXX', '.XXXXX.', 'XXX.XXX', 'XX...XX', '.X...X.'], cx - 3, ty - 2);
  c.stamp(['X......', '.X.....', '..XEEEE', '..EcEEE', '..EEEEf', '...EEEf'], cx + Math.floor(wBot / 2) - 1, ty + 4);
}

// ---- the plant at each stage (lane scale, 66 × 50) ----
// Stages (#423 Growth): queued a cutting in a jar, impl rooted in a pot, review / fix a few leaves, ready the full plant
// with its mature leaf, merged wrapped and sold; blocked the wilted plant. Two frames: the upper leaves sway.
export const STAGES = ['queued', 'impl', 'review', 'fix', 'ready', 'merged', 'blocked'];
const SX = 49, SY = 33;
function plantFrame(sp, stage, k = 0) {
  // drawn on a 69-wide canvas and cropped 3 columns from the left, so the plant stands at x 46 with room for its leaves
  const c = canvas(69, 50);
  const shadow = (x0, w, y = 47) => { for (let i = 0; i < w; i++) c.set(x0 + i, y, 'q'); };
  switch (stage) {
    case 'queued': {
      shadow(43, 14);
      jar(c, SX, 30, 12, 17);
      stem(c, SX, 41, 48, 21); stem(c, 49, 36, 52, 27);
      foliage(c, sp, [[48, 21, 10, -14], [52, 27, 7, 55, { sway: 0 }]], SX, 41, k);
      // the jar's front glass over the stem
      for (let j = 33; j < 46; j++) c.set(54, j, 'x');
      break;
    }
    case 'impl':
      shadow(40, 20); pot(c, SX, 34);
      foliage(c, sp, [[44, 26, 8, -42], [53, 23, 9, 28]], SX, SY, k);
      break;
    case 'review': case 'fix': {
      shadow(40, 20); pot(c, SX, 34);
      const L = [[39, 28, 11, -72, { sx: 47 }], [59, 27, 10, 66, { sx: 51 }], [45, 20, 11, -24, { sx: 48 }], [53, 17, 12, 14, { sx: 50 }]];
      if (stage === 'fix') {
        // a snipped stem: cut clean on the slant, its tip browned
        stem(c, 52, SY, 58, 22); c.set(59, 21, 'n'); c.set(58, 21, 'n');
        L.splice(1, 1, [60, 30, 8, 85, { sx: 51 }]);
      }
      foliage(c, sp, L, SX, SY, k);
      break;
    }
    case 'ready': {
      shadow(40, 20); pot(c, SX, 34);
      const L = sp === 'pothos'
        ? [[42, 22, 12, -48, { sx: 47 }], [56, 21, 12, 46, { sx: 51 }], [49, 17, 15, -2, { big: 1 }]]
        : [[38, 28, 11, -80, { sx: 46 }], [60, 28, 10, 76, { sx: 52 }], [42, 21, 12, -46, { sx: 47 }], [56, 20, 11, 44, { sx: 51 }], [49, 19, 18, -2, { big: 1 }]];
      if (sp === 'pothos') {
        // the vines trail over the rim and down the pot
        stem(c, 40, 33, 34, 46, 't', 0.9); stem(c, 58, 33, 63, 45, 't', 0.9);
        for (const [x, y, a] of [[37, 36, -120], [35, 41, -150], [34, 46, -170], [61, 36, 120], [62, 41, 150]]) leaf(c, sp, x, y, 5, a);
      }
      foliage(c, sp, L, SX, SY, k);
      break;
    }
    case 'merged': {
      shadow(41, 18);
      foliage(c, sp, [[42, 21, 12, -46], [56, 20, 12, 46], [49, 17, 15, -2, { big: 1 }]], SX, 30, k);
      wrap(c, SX, 25, 22, 28, 16);
      break;
    }
    case 'blocked': {
      // the stems arch over the rim and the yellowed leaves hang down beside the pot; one has already dropped
      shadow(36, 28); pot(c, SX, 34);
      stem(c, 47, SY, 40, 31, 'n', 0.1); stem(c, 51, SY, 58, 31, 'n', 0.1); stem(c, 49, SY, 47, 26, 'n', 0.4);
      leaf(c, sp, 40, 31, 10, -150, { tint: WILT }); leaf(c, sp, 58, 31, 10, 152, { tint: WILT }); leaf(c, sp, 47, 26, 9, -118, { tint: WILT });
      leaf(c, sp, 61, 46, 7, 100 - k * 2, { tint: WILT });
      break;
    }
  }
  return c.rows().map(r => r.slice(3));
}
export function plant(sp, stage, { still } = {}) {
  if (still) return [[plantFrame(sp, stage, 0), 0, 0]];
  return [[plantFrame(sp, stage, 0), 0, 0, 'f1'], [plantFrame(sp, stage, 1), 0, 0, 'f2']];
}

// ---- the keeper, in parts (head at hx, hy; body two columns wider each side), like the kitchen's chef ----
const FRINGE = [
  '.....MMMMMMMM.....',
  '...MMMMMMMMMMMM...',
  '..MMMM4MMMMMMMMM..',
  '.MMM4MMMMMMMMMMMM.',
  '.MM4MMMMMMMMMMMM3.',
  'MMMMSMMMMMMSSMMM3M',
  'MM3SSSSSSSSSSSSS3M',
];
const EYES = {
  open: ['M3SSSSSSSSSSSSSSS3', 'MSSSkkSSSSSSkkSSSM', 'MSSSkQSSSSSSkQSSSM', '.sSSkkSSSSSSkkSSs.'],
  look: ['M3SSSSSSSSSSSSSSS3', 'MSSSSkkSSSSSSkkSSM', 'MSSSSkQSSSSSSkQSSM', '.sSSSkkSSSSSSkkSs.'],
  shut: ['M3SSSSSSSSSSSSSSS3', 'MSSSSSSSSSSSSSSSSM', 'MSSkkkSSSSSSkkkSSM', '.sSSSSSSSSSSSSSSs.'],
  happy: ['M3SSSSSSSSSSSSSSS3', 'MSSSkkSSSSSSkkSSSM', 'MSSkSSkSSSSkSSkSSM', '.sSSSSSSSSSSSSSSs.'],
  worried: ['M3SkkSSSSSSSSkkSS3', 'MSSSkkSSSSSSkkSSSM', 'MSSSkQSSSSSSkQSSSM', '.sSSkkSSSSSSkkSSs.'],
};
const MOUTH = {
  calm: ['.sSSSSSSmmSSSSSSs.', '..sSSSSSSSSSSSSs..'],
  look: ['.sSSSSSSSmmSSSSSs.', '..sSSSSSSSSSSSSs..'],
  smile: ['.sSSSSSmSSmSSSSSs.', '..sSSSSSmmSSSSSs..'],
  o: ['.sSSSSSSmmSSSSSSs.', '..sSSSSSmmSSSSSs..'],
};
const CHEEK = '.sSbbSSSSsSSSbbSs.';
const CHIN = ['...ssSSSSSSSSss...', '.....ssssssss.....'];
const face = (eyes, mouth) => [...FRINGE, ...EYES[eyes], CHEEK, ...MOUTH[mouth], ...CHIN];
const HEAD = face('open', 'calm'), HEAD_LOOK = face('look', 'look'), HEAD_SHUT = face('shut', 'smile');
const HEAD_SMILE = face('happy', 'smile'), HEAD_O = face('worried', 'o');
// three looks of head: a straw sun hat with a band, a spotted headscarf, a bun with a tie (--ht / --hb / --hk, one is 1)
const SUNHAT = [
  '........JJJJJJJJ........',
  '......JJjJJJJJJJJJ......',
  '.....JJjJJJ~JJJJjJJ.....',
  '.....HHHHHHHHHHHHHH.....',
  '.....hhhhhhhhhhhhhh.....',
  'JJJJJJJjJJJJJJJJjJJJJJJJ',
  '.~jjjjjjjjjjjjjjjjjjjj~.',
];
const SCARF = [
  '.....HHHHHHHH.......',
  '...HHHHEHHHHHHH.....',
  '..HHHHHHHHHHEHHH....',
  '.HHEHHHHHHHHHHHHH...',
  '.HHHHHHHEHHHHHHHHH..',
  '.hhhhhhhhhhhhhhhhhH.',
  '..................hH',
  '.................hH.',
];
const BUN = ['...MMMM...', '..MM4MMM..', '..M4MMMM..', '..MMMMM3..', '..HHHHHH..'];
// shirt, the repo neckerchief, the apron with its straps, bib pocket, waist tie and skirt pocket
const BODY = [
  '.......wWTTTTWw.......',
  '.....WWWWWTTWWWWW.....',
  '...WWWWWWW^^WWWWWWW...',
  '..WWWWW0AAAAAA0WWWWW..',
  '..WWWWW0AAAAAA0WWWWW..',
  '..WWWWWAAaaaaAAWWWWW..',
  '..wWWWWAAaAAaAAWWWWw..',
  '..wwWWWAAaaaaAAWWWww..',
  '...wwaaaaaaaaaaaaww...',
  '...AAAAAAAAAAAAAAaa...',
  '...AAAAAAAAAAAAAAaa...',
  '...AAaaaaaaAAAAAAaa...',
  '...AAaAAAAaAAAAAAaa...',
  '...AAaaaaaaAAAAAAaa...',
  '...AAAAAAAAAAAAAAaa...',
  '...aaaaaaaaaaaaaaaa...',
];
const LEGS = ['.....PPPPP..PPPPP.....', '.....PPPpP..PPPpP.....', '.....PPPpP..PPPpP.....', '....BBBBBB..BBBBBB....', '....B=BBBB..B=BBBB....', '....pppppp..pppppp....'];
const KSHADOW = ['...qqqqqqqqqqqqqqqq...'];
const ARM = ['.WW.', 'WWWW', 'WWWW', 'WWWW', 'WWWw', 'wwww', 'SSSS', 'SSSs', '.ss.'];
// An arm held out to the right: n columns of sleeve from the shoulder, then the hand.
const REACH = n => ['W'.repeat(n) + 'SS.', 'W'.repeat(n - 1) + 'wSSS', 'w'.repeat(n) + 'sS.'];
function keeper(hx, hy, { head = HEAD, arms = 'lr' } = {}) {
  const bx = hx - 2, L = [[KSHADOW, bx, hy + 38], [LEGS, bx, hy + 32], [BODY, bx, hy + 16]];
  if (arms.includes('l')) L.push([ARM, bx, hy + 18]);
  if (arms.includes('r')) L.push([ARM, bx + 18, hy + 18]);
  L.push([head, hx, hy]);
  L.push([SUNHAT, hx - 3, hy - 4, 'hat-t'], [SCARF, hx - 1, hy - 2, 'hat-b'], [BUN, hx + 4, hy - 4, 'hat-k']);
  return L;
}

// ---- the actions (the contract's `actions`, by role): impl water, fix prune, rev inspect, update repot, idle,
// done (the plant is ready), blocked (it wilts). f1/f2 are the two frames of the loop.
const CAN = [
  '....eeeeee..........',
  '...e......e.........',
  '...e......e.......+C',
  '.CCCCCCCCCCC.....CC.',
  '.C+CCCCCCCCCe...CC..',
  '.C+CCCCCCCCCCeeCCe..',
  '.CCCCCCCCCCCCCCe....',
  '.C+CCCCCCCCCe.......',
  '.eCCCCCCCCCCe.......',
  '..eeeeeeeeee........',
];
const SPRAY = k => { const c = canvas(10, 12); for (let i = 0; i < 6; i++) c.set(1 + i + (i > 2 ? 1 : 0), ((i * 5 + k * 3) % 11), i % 2 ? 'Z' : 'D'); for (let i = 0; i < 5; i++) c.set(2 + i, (i * 3 + 2 + k * 2) % 11, 'Z'); return c.rows(); };
const SCIS_OPEN = ['hhh.......NN', 'h.h....NNN..', '.hhhNNNN....', 'h.h....NNN..', 'hhh.......NN'];
const SCIS_SHUT = ['hhh.........', 'h.h.........', '.hhhNNNNNNNN', 'h.h.........', 'hhh.........'];
const LENS = ['...iiii...', '..iIIIIi..', '.iIxIIIIi.', '.iIxIIIIi.', '.iIIIIIIi.', '..iIIIIi..', '...iiii...', '.......u..', '........u.', '.........u'];
const TROWEL = ['uuuuNNNN...', 'uuuuNNNNNN.', '....NNNNNNN', '.....ddd...'];
const BAG = ['..KKKKKKKK..', '.KK8d7dd8KK.', 'KKdd7ddddKKK', 'KKKKKKKKKKKK', 'K-KKKKKKKKcK', 'K-KkKKkKKKcK', 'K-KKkkKKKKcK', 'K-KKKKKKKKcK', 'K-KKKKKKKKcK', 'KKKKKKKKKKcK', 'cccccccccccc'];
const MUG = ['FFFFFF..', 'FFFtFFff', 'FFtt!F.f', 'FtFFFF.f', 'FFFFFFff', 'fFFFFf..', '.ffff...'];
const STEAM = ['..x.', '.x..', '.x..', '..x.', '...x', '..x.'];
const SPARK = ['...Q...', '...J...', '.Q.J.Q.', 'QJJQJJQ', '.Q.J.Q.', '...J...', '...Q...'];
const SWEAT = ['.D.', 'DDD', 'DQD', '.D.'];
const HY = 9;
export const ACTIONS = {
  water: () => [
    ...keeper(4, HY, { arms: 'l', head: HEAD_LOOK }),
    [REACH(6), 20, HY + 19], [CAN, 23, HY + 15], [SPRAY(0), 40, HY + 19, 'f1'], [SPRAY(1), 40, HY + 19, 'f2'],
  ],
  prune: () => [
    ...keeper(7, HY, { arms: 'l', head: HEAD_LOOK }),
    [REACH(7), 23, HY + 18], [SCIS_OPEN, 33, HY + 17, 'f1'], [SCIS_SHUT, 33, HY + 17, 'f2'],
    [['nn', '.n'], 45, HY + 22, 'f2'], [['.n', 'nn'], 47, HY + 30, 'f2'],
  ],
  inspect: () => [
    ...keeper(5, HY + 1, { arms: 'l', head: HEAD_LOOK }),
    [['.SS.', 'WSSS', 'WWS.', 'WW..', 'WW..', 'WW..', 'ww..'], 23, HY + 14], [LENS, 25, HY + 4, 'f1'], [LENS, 27, HY + 6, 'f2'],
  ],
  repot: () => [
    ...keeper(3, HY, { arms: 'l', head: HEAD_LOOK }),
    [BAG, 24, 36], [REACH(8), 19, HY + 19], [TROWEL, 29, HY + 18, 'f1'], [TROWEL, 31, HY + 16, 'f2'],
    [['d..', '.7.', '..d', '.d.'], 41, HY + 20, 'f2'], [['.d', 'd.'], 38, HY + 23, 'f1'],
  ],
  idle: () => [
    ...keeper(6, HY, { arms: 'l', head: HEAD_SHUT }),
    [['.WW..', 'WWWW.', 'wWWW.', 'wwSSS', '..SSS'], 22, HY + 18], [MUG, 17, HY + 20], [STEAM, 18, HY + 13, 'f1'], [flipH(STEAM), 19, HY + 13, 'f2'],
  ],
  done: () => [
    ...keeper(4, HY, { arms: 'l', head: HEAD_SMILE }),
    [['..SS', '.SSS', '.SS.', 'WWW.', 'WWW.', 'WWW.', 'Www.', 'ww..'], 20, HY + 11], [SPARK, 35, 3, 'f1'], [SPARK, 58, 6, 'f2'], [['Q'], 31, 8, 'f1'], [['Q'], 63, 18, 'f2'],
  ],
  wilted: () => [
    ...keeper(4, HY, { arms: '', head: HEAD_O }),
    [['.SS.', 'SSSW', 'SSWW', '.WWW', '.WWW', '..WW', '..ww'], 0, HY + 9], [flipH(['.SS.', 'SSSW', 'SSWW', '.WWW', '.WWW', '..WW', '..ww']), 20, HY + 9],
    [SWEAT, 22, HY + 2, 'f1'], [SWEAT, 22, HY + 4, 'f2'],
  ],
};
export const ACT_OF = { impl: 'water', fix: 'prune', rev: 'inspect', update: 'repot', idle: 'idle', done: 'done', blocked: 'wilted' };

// #241 `moments` (#423 Moments): merged the plant wrapped and sold, the shop bell rings (hop); FAIL a yellowed leaf drops
// off (shake); needs-you a red "thirsty" drop pulses over the pot (pulse).
const BELL = ['....ii....', '....JJ....', '..JJJJJJ..', '.JJQJJJJj.', '.JQJJJJJj.', '.JJJJJJJj.', 'JJJJJJJJJj', 'jjjjjjjjjj', '....jj....'];
const RING = ['J.......J', '.J.....J.', '..J...J..'];
const COUNTER = ['uuuuuuuuuuuuuuuuuuuuuu', 'oooooooooooooooooooooo', 'u.u.u.u.u.u.u.u.u.u.u.'];
export const DROP = ['....RR....', '...RRRR...', '..RRRRRR..', '.RRRQRRRR.', 'RRRQRRRRRR', 'RRRQRRRRRR', 'RRRRRRRR*R', '.RRRRRR**.', '..******..'];
const fallLeaf = (x, y, ang) => { const c = canvas(66, 50); leaf(c, 'monstera', x, y, 8, ang, { tint: YELLOW }); return c.rows(); };
export const MOMENTS = {
  sold: () => [
    ...keeper(3, HY, { arms: 'l', head: HEAD_SMILE }),
    [COUNTER, 13, 42], [BELL, 22, 33], [REACH(3), 19, HY + 21], [RING, 22, 29, 'f2'], ...plant('monstera', 'merged', { still: 1 }),
  ],
  'leaf-drop': () => [
    ...keeper(3, HY, { arms: 'l', head: HEAD_O }), [REACH(5), 19, HY + 19],
    ...plant('monstera', 'review', { still: 1 }),
    [fallLeaf(58, 24, 70), 0, 0, 'f1'], [fallLeaf(58, 40, 130), 0, 0, 'f2'],
  ],
  thirsty: () => [...plant('monstera', 'review', { still: 1 }), ...dropBubble(26, 2)],
};
// The thirsty bubble alone: over a jar on the counter (the rail's needs-you item) or a pot.
export function dropBubble(x, y, flip = 0) {
  const c = canvas(18, 20);
  for (let j = 0; j < 15; j++) for (let i = 0; i < 18; i++) if (!((j === 0 || j === 14) && (i < 2 || i > 15)) && !((j === 1 || j === 13) && (i === 0 || i === 17))) c.set(i, j, 'E');
  c.stamp(['EEEE', 'EEE.', 'EE..', 'E...', '.E..'], 12, 15);
  const B = c.rows();
  return [[flip ? flipH(B) : B, x, y], [DROP, x + 4, y + 3]];
}
export const SAYS = { sold: 'sold!', 'leaf-drop': 'leaf drop', thirsty: 'thirsty' };
export const bubble = t => `<rect x="60" y="0" width="72" height="16" rx="4" fill="#f7f1e3"/><text x="96" y="12" font-size="11" font-weight="700" font-family="ui-monospace,Menlo,monospace" text-anchor="middle" fill="#2b2420">${t}</text>`;

// ---- #241 `row_sprites` and #423 `stages` at row scale (24 × 24, drawn at 24 px) ----
function rowFrame(sp, stage, k = 0, dx = 0) {
  const c = canvas(24, 24), cx = 12 + dx, sy = 15;
  const P = () => { for (let i = 0; i < 10; i++) c.set(cx - 5 + i, 23, 'q'); pot(c, cx, 16, 12, 8); };
  const F = (L, y = sy) => foliage(c, sp, L.map(([x, ...r]) => [x + dx, ...r]), cx, y, k);
  switch (stage) {
    case 'queued': jar(c, cx, 10, 9, 14); F([[11, 6, 7, -12, { sway: 1 }], [14, 10, 4, 60, { sway: 0 }]], 18); break;
    case 'impl': P(); F([[9, 11, 5, -45, { sway: 1 }], [14, 9, 6, 30, { sway: 1 }]]); break;
    case 'review': case 'fix':
      P();
      if (stage === 'fix') { stem(c, cx + 1, sy, cx + 5, 9); c.set(cx + 6, 8, 'n'); }
      F(stage === 'fix' ? [[5, 13, 6, -78], [10, 8, 7, -22, { sway: 1 }], [14, 6, 7, 14, { sway: 1 }]] : [[5, 13, 6, -78], [19, 12, 6, 78], [10, 8, 7, -24, { sway: 1 }], [14, 6, 7, 18, { sway: 1 }]]);
      break;
    case 'ready': P(); F([[5, 13, 7, -80], [19, 12, 7, 80], [8, 9, 7, -42], [16, 8, 7, 42], [12, 7, 9, -2, { big: 1, sway: 1 }]]); break;
    case 'merged':
      for (let i = 0; i < 10; i++) c.set(cx - 5 + i, 23, 'q');
      F([[7, 9, 7, -48], [17, 9, 7, 48], [12, 8, 9, -2, { big: 1, sway: 1 }]], 13);
      wrap(c, cx, 11, 13, 16, 9);
      break;
    case 'blocked':
      P();
      stem(c, cx - 1, sy, cx - 5, 13, 'n', 0.1); stem(c, cx + 1, sy, cx + 5, 13, 'n', 0.1);
      leaf(c, sp, cx - 5, 13, 6, -150, { tint: WILT }); leaf(c, sp, cx + 5, 13, 6, 150, { tint: WILT }); leaf(c, sp, cx, 11, 5, -125, { tint: WILT });
      break;
    case 'needs':
      P(); F([[5, 13, 6, -78], [10, 8, 7, -24]]);
      c.stamp(k ? ['.....', '..R..', '.RRR.', '.RQR.', '..*..'] : ['..R..', '.RRR.', 'RRQRR', 'RQRRR', 'RRRR*', '.***.'], 17, 0);
      break;
    case 'fail': {
      P(); F([[5, 13, 6, -78], [10, 8, 7, -24]]);
      stem(c, cx + 1, sy, cx + 5, 8);
      if (k) leaf(c, sp, 20, 20, 5, 120, { tint: YELLOW }); else leaf(c, sp, cx + 5, 8, 5, 40, { tint: YELLOW });
      break;
    }
  }
  return c.rows();
}
export function rowPlant(sp, stage, dx = 0) { return [[rowFrame(sp, stage, 0, dx), 0, 0, 'f1'], [rowFrame(sp, stage, 1, dx), 0, 0, 'f2']]; }
// The Events rows: an action's row sprite is its tool beside a small plant.
const RCAN = ['..eeee....', '.e....e...', '.e....e..+', 'CCCCCCC..C', 'C+CCCCCe.C.', 'CCCCCCCCC..', 'C+CCCCe....', 'eCCCCCe....', '.eeeeee....'];
export const ROWACT = {
  water: [...rowPlant('monstera', 'impl', 5), [RCAN, 0, 5], [['D', '.', 'Z'], 11, 9, 'f1'], [['.', 'Z', '.', 'D'], 11, 10, 'f2']],
  prune: [...rowPlant('philodendron', 'review', 5).slice(0, 1), [['hh....N', 'h.h.NN.', '.hNN...', 'h.h.NN.', 'hh....N'], 0, 6, 'f1'], [['hh.....', 'h.h....', '.hNNNNN', 'h.h....', 'hh.....'], 0, 6, 'f2']],
  inspect: [...rowPlant('peperomia', 'review', 5).slice(0, 1), [['..iii..', '.iIIIi.', 'iIxIIIi', 'iIxIIIi', 'iIIIIIi', '.iIIIi.', '..iii..', '......u', '.......u'], 0, 2, 'f1'], [['..iii..', '.iIIIi.', 'iIxIIIi', 'iIxIIIi', 'iIIIIIi', '.iIIIi.', '..iii..', '......u', '.......u'], 1, 3, 'f2']],
  repot: [...rowPlant('alocasia', 'impl', 5), [['uuuNNN.', 'uuuNNNN', '...NNNN', '....dd.'], 0, 6, 'f1'], [['uuuNNN.', 'uuuNNNN', '...NNNN', '.......', '....d..'], 1, 4, 'f2'],
    [['.KKKKK.', 'Kd7dd8K', 'KKKKKKK', 'K-KKKcK', 'K-kKKcK', 'K-KKKcK', 'ccccccc'], 0, 16]],
  idle: [[MUG, 7, 12], [STEAM, 9, 4, 'f1'], [flipH(STEAM), 10, 4, 'f2']],
};
// The row sprites for the contract's `row_sprites`: by action (Events), by kind, and the stage set (Queue rows).
export function rows() {
  return {
    water: ROWACT.water, prune: ROWACT.prune, inspect: ROWACT.inspect, repot: ROWACT.repot, idle: ROWACT.idle,
    done: rowPlant('monstera', 'ready'), wilted: rowPlant('monstera', 'blocked'),
    jar: rowPlant('pothos', 'queued'), thirsty: rowPlant('calathea', 'needs'), sold: rowPlant('monstera', 'merged'), fall: rowPlant('monstera', 'fail'),
  };
}

// ---- rasterising: layers → <rect> runs, one <g> per class ----
function rects(map, x0, y0, u) {
  let s = '';
  map.forEach((row, j) => {
    let i = 0;
    while (i < row.length) {
      const ch = row[i];
      if (ch === '.' || ch === ' ' || !INK[ch]) { i++; continue; }
      let n = 1;
      while (row[i + n] === ch) n++;
      const [fill, op] = Array.isArray(INK[ch]) ? INK[ch] : [INK[ch], 1];
      s += `<rect x="${(x0 + i) * u}" y="${(y0 + j) * u}" width="${n * u}" height="${u}" style="fill:${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
      i += n;
    }
  });
  return s;
}
const HATVAR = { 'hat-t': ['--ht', 1], 'hat-b': ['--hb', 0], 'hat-k': ['--hk', 0] };
export function draw(layers, u = 2) {
  return layers.map(([map, x, y, cls]) => {
    const r = rects(map, x, y, u);
    if (!cls) return r;
    const hv = HATVAR[cls];
    return `<g class="${cls}"${hv ? ` style="opacity:var(${hv[0]},${hv[1]})"` : ''}>${r}</g>`;
  }).join('');
}
// Symbols for a page: g-<action>, p-<species>-<stage> (132 × 100), r-<row>, rs-<species>-<stage> (24 × 24), m-<moment>.
export function symbols() {
  const sym = (id, vb, body) => `<symbol id="${id}" viewBox="${vb}" shape-rendering="crispEdges">${body}</symbol>`;
  let s = '';
  for (const [k, f] of Object.entries(ACTIONS)) s += sym(`g-${k}`, '0 0 132 100', draw(f()));
  for (const { id } of SPECIES) for (const st of STAGES) s += sym(`p-${id}-${st}`, '0 0 132 100', draw(plant(id, st)));
  for (const [k, f] of Object.entries(MOMENTS)) s += sym(`m-${k}`, '0 0 132 100', draw(f()));
  s += sym('m-drop', '0 0 132 100', draw(dropBubble(29, 4)));
  for (const [k, m] of Object.entries(rows())) s += sym(`r-${k}`, '0 0 24 24', draw(m, 1));
  for (const { id } of SPECIES) for (const st of [...STAGES, 'needs', 'fail']) s += sym(`rs-${id}-${st}`, '0 0 24 24', draw(rowPlant(id, st), 1));
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
  ['sun', 'teal', 'tan', '#d8c9a8', 'sky', 'clay'],
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

// ---- the shop (scene.svg): one repo's corner of the shop, 392 × 188, one pixel = 2 (a 196 × 94 grid). The window on the
// left shows the load as light (`.l-sun`, `.l-overcast`, `.l-rain`; the page's `light-<level>` class shows one); the
// shelves on the right are the repo's (`--fk` on the shelf edges); the counter runs along the foot. ----
function shelfPlant(c, cx, base, sp, k) {
  pot(c, cx, base - 6, 9, 6);
  const L = [
    [[cx - 3, base - 9, 5, -60], [cx + 3, base - 10, 5, 55], [cx, base - 12, 6, -5, { big: 1 }]],
    [[cx - 2, base - 11, 6, -35], [cx + 3, base - 9, 5, 70], [cx + 1, base - 13, 5, 15]],
    [[cx - 3, base - 9, 4, -75], [cx - 1, base - 12, 6, -18], [cx + 2, base - 11, 6, 30]],
  ][k % 3];
  foliage(c, sp, L, cx, base - 7, 0);
}
export function shop() {
  const u = 2, W = 196, H = 94;
  const r = (x, y, w, h, fill, extra = '') => `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${fill}"${extra}/>`;
  let s = r(0, 0, W, H, '#ead9b8');
  // plaster wall with a brick course showing through, each brick lit on top
  for (let y = 4; y < 70; y += 6) for (let x = (y % 12 === 4 ? 0 : 6); x < W; x += 12) {
    if (hash(x, y) < 0.45) continue;
    s += r(x, y, 11, 5, '#e2cba3') + r(x, y, 11, 1, '#efe0c4') + r(x, y + 5, 11, 1, '#d8bf94', ' opacity=".6"');
  }
  // the ceiling beam
  s += r(0, 0, W, 4, '#6b4a30') + r(0, 4, W, 1, '#4f3522') + r(0, 1, W, 1, '#7d5839');
  for (let x = 6; x < W; x += 17) s += r(x, 2, 6, 1, '#5d3f28');
  // the window: wood frame with a lit bevel, four panes; sky and weather by level
  const wx = 6, wy = 9, ww = 72, wh = 52;
  s += r(wx - 3, wy - 3, ww + 6, wh + 6, '#7a5236') + r(wx - 3, wy - 3, ww + 6, 1, '#9a6a44') + r(wx - 3, wy - 3, 1, wh + 6, '#9a6a44') + r(wx - 1, wy - 1, ww + 2, wh + 2, '#5d3d26');
  const sky = (cls, top, bot) => `<linearGradient id="sky-${cls}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bot}"/></linearGradient>` +
    `<g class="${cls}"><rect x="${wx * u}" y="${wy * u}" width="${ww * u}" height="${wh * u}" fill="url(#sky-${cls})"/></g>`;
  s += sky('l-sun', '#8fcbe8', '#d4eef5') + sky('l-overcast', '#aab3b8', '#cfd5d6') + sky('l-rain', '#4f5d68', '#7b8790');
  // outdoors: a street lamp, the houses across the street, a hedge with a leafy top
  s += r(wx + 10, wy + 22, 14, 16, '#c9a27a') + r(wx + 10, wy + 20, 14, 2, '#a0583c') + r(wx + 13, wy + 26, 3, 4, '#7b8fa0') + r(wx + 19, wy + 26, 3, 4, '#7b8fa0');
  s += r(wx + 46, wy + 18, 18, 20, '#b9b0a0') + r(wx + 45, wy + 16, 20, 2, '#7a5236') + r(wx + 49, wy + 23, 4, 5, '#7b8fa0') + r(wx + 57, wy + 23, 4, 5, '#7b8fa0');
  s += r(wx + 33, wy + 14, 1, 24, '#3a4248') + r(wx + 31, wy + 12, 5, 3, '#3a4248') + r(wx + 32, wy + 13, 3, 1, '#ffe6a0');
  s += r(wx, wy + 38, ww, 14, '#5f8a4a');
  for (let x = 0; x < ww; x += 3) { const h = 1 + Math.floor(hash(x, 9) * 3); s += r(wx + x, wy + 38 - h, 3, h, '#5f8a4a') + r(wx + x, wy + 38 - h, 2, 1, '#78a35c'); }
  for (let i = 0; i < 26; i++) s += r(wx + Math.floor(hash(i, 2) * ww), wy + 40 + Math.floor(hash(i, 5) * 11), 1, 1, i % 2 ? '#4a6f3a' : '#78a35c');
  s += `<g class="l-sun">${r(58, 16, 10, 10, '#ffd25a')}${r(56, 18, 14, 6, '#ffd25a')}${r(60, 14, 6, 14, '#ffd25a')}${r(60, 18, 4, 4, '#fff0b0')}${r(59, 17, 2, 2, '#fff0b0')}` +
    [[63, 10], [63, 30], [52, 21], [73, 21], [55, 13], [71, 13], [55, 28], [71, 28]].map(([x, y]) => r(x, y, 1, 2, '#ffe58a', ' opacity=".9"')).join('') +
    `${r(14, 18, 14, 3, '#ffffff', ' opacity=".85"')}${r(17, 16, 8, 2, '#ffffff', ' opacity=".85"')}${r(16, 21, 10, 1, '#e4f2f8', ' opacity=".85"')}</g>`;
  const cloud = (x, y, cl, dk) => r(x + 2, y + 2, 20, 5, cl) + r(x + 5, y, 8, 2, cl) + r(x + 12, y + 1, 6, 1, cl) + r(x, y + 4, 24, 3, cl) + r(x + 1, y + 7, 22, 2, dk) + r(x + 6, y + 1, 3, 1, '#ffffff', ' opacity=".35"');
  s += `<g class="l-overcast">${cloud(10, 14, '#e9ecec', '#c4cacb')}${cloud(44, 18, '#dfe3e3', '#b9c0c2')}${cloud(26, 10, '#f1f3f3', '#cdd2d3')}</g>`;
  s += `<g class="l-rain">${cloud(8, 10, '#6b7781', '#56626c')}${cloud(40, 12, '#727e88', '#5a6670')}${cloud(22, 8, '#7b8791', '#636f79')}${r(wx, wy + 36, ww, 16, '#3f5a36', ' opacity=".5"')}</g>`;
  const rain = k => { let t = ''; for (let i = 0; i < 22; i++) { const x = wx + 2 + ((i * 13 + k * 5) % (ww - 4)), y = wy + 4 + ((i * 9 + k * 7) % (wh - 12)); t += r(x, y, 1, 4, '#cfe3ef', ' opacity=".8"') + r(x, y + 4, 1, 1, '#ffffff', ' opacity=".8"'); } for (let i = 0; i < 9; i++) t += r(wx + 3 + ((i * 17 + k * 3) % (ww - 6)), wy + 6 + ((i * 11) % (wh - 10)), 2, 2, '#e6f2f8', ' opacity=".55"'); return t; };
  s += `<g class="l-rain f1">${rain(0)}</g><g class="l-rain f2">${rain(1)}</g>`;
  // glazing bars, the sill with its lit edge, two plants on it
  s += r(wx + ww / 2 - 1, wy, 2, wh, '#7a5236') + r(wx + ww / 2 - 1, wy, 1, wh, '#9a6a44') + r(wx, wy + wh / 2 - 1, ww, 2, '#7a5236') + r(wx, wy + wh / 2 - 1, ww, 1, '#9a6a44');
  s += r(wx - 5, wy + wh + 3, ww + 10, 3, '#9a6a44') + r(wx - 5, wy + wh + 3, ww + 10, 1, '#b8865a') + r(wx - 5, wy + wh + 6, ww + 10, 1, '#6b4a30');
  const art = [], grow = (sp, f) => { const c = canvas(W, H); f(c); art.push(`<g style="${sv(sp)}">${draw([[c.rows(), 0, 0]], u)}</g>`); };
  grow('peperomia', c => shelfPlant(c, 18, wy + wh + 3, 'peperomia', 2)); grow('calathea', c => shelfPlant(c, 62, wy + wh + 3, 'calathea', 1));
  // a hanging pothos from the ceiling, by the window
  s += r(86, 5, 1, 11, '#6b4a30') + r(84, 5, 1, 11, '#6b4a30') + r(88, 5, 1, 11, '#6b4a30');
  grow('pothos', c => {
    pot(c, 86, 16, 10, 6); stem(c, 83, 16, 80, 34, 't', 0.9); stem(c, 89, 16, 92, 38, 't', 0.9);
    for (const [x, y, a] of [[81, 20, -130], [80, 26, -150], [80, 32, -160], [91, 21, 130], [92, 27, 150], [92, 33, 160], [86, 15, 0]]) leaf(c, 'pothos', x, y, 5, a);
  });
  // the repo's shelves: three planks with their grain, the edge in the repo colour, brackets; pots of every species
  const sp = ['monstera', 'philodendron', 'peperomia', 'alocasia', 'pothos', 'calathea'];
  for (const [i, y] of [[0, 22], [1, 42], [2, 62]]) {
    s += r(100, y, 92, 3, '#9a6a44') + r(100, y, 92, 1, '#b8865a') + r(100, y + 3, 92, 1, 'var(--fk)') + r(100, y + 4, 92, 1, '#6b4a30', ' opacity=".5"');
    for (let x = 104; x < 190; x += 11) s += r(x, y + 1, 5, 1, '#8a5a34', ' opacity=".6"');
    for (const bx of [106, 182]) s += r(bx, y + 4, 2, 4, '#6b4a30') + r(bx, y + 4, 4, 1, '#6b4a30');
    for (let k = 0; k < 6; k++) { const p = sp[(k + i * 2) % 6]; grow(p, c => shelfPlant(c, 106 + k * 14 + (i % 2) * 4, y, p, k + i)); }
  }
  // the counter along the foot: a lit top, its edge in the repo colour, panelled front; a till and a bell
  s += r(0, 72, W, 3, '#b07a4a') + r(0, 72, W, 1, '#c99464') + r(0, 75, W, 1, 'var(--fk)') + r(0, 76, W, H - 76, '#8a5a34');
  for (const x of [4, 52, 100, 148]) s += r(x, 80, 42, 12, '#7a4e2c') + r(x, 80, 42, 1, '#6b4426') + r(x + 1, 91, 41, 1, '#9a6a44') + r(x + 41, 80, 1, 12, '#9a6a44');
  s += r(156, 62, 20, 10, '#5b6168') + r(158, 64, 16, 4, '#a9d3a0') + r(159, 65, 6, 1, '#5b8a54') + r(152, 68, 28, 4, '#4a4f55') + r(152, 68, 28, 1, '#6b7178');
  s += r(133, 68, 8, 4, '#e2c27a') + r(134, 66, 6, 2, '#e2c27a') + r(136, 65, 2, 1, '#6d7579') + r(134, 67, 2, 2, '#fff3cf') + r(132, 71, 10, 1, '#b8954e');
  s += art.join('');
  // sunbeams through the window (sun only), the room dimmer under cloud and rain
  s += `<g class="l-sun beams"><path d="M${(wx + 8) * u} ${wy * u}L${(wx + ww) * u} ${wy * u}L${(wx + ww + 60) * u} ${H * u}L${(wx + 36) * u} ${H * u}Z" fill="#fff3c4" opacity=".26"/></g>`;
  s += `<g class="l-overcast">${r(0, 0, W, H, '#5a6a72', ' opacity=".10"')}</g><g class="l-rain">${r(0, 0, W, H, '#20303c', ' opacity=".22"')}</g>`;
  return `<svg viewBox="0 0 ${W * u} ${H * u}" preserveAspectRatio="xMidYMax slice" shape-rendering="crispEdges" aria-hidden="true"><defs></defs>${s}</svg>`;
}

// The window light in miniature for the Host card header (#370 `minis`): 72 × 24 at one pixel = 1 (drawn at 72 × 24):
// a window with sun, cloud or rain.
export function mini(level) {
  const sky = { sun: '#8fcbe8', overcast: '#aab3b8', rain: '#4f5d68' }[level];
  const R = (x, y, w, h, f, ex = '') => `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${f}"${ex}/>`;
  let s = R(0, 0, 72, 24, '#7a5236') + R(0, 0, 72, 1, '#9a6a44') + R(2, 2, 68, 19, sky) + R(2, 16, 68, 5, '#5f8a4a');
  for (let x = 2; x < 70; x += 3) s += R(x, 15 - (x % 2), 3, 1 + (x % 2), '#5f8a4a') + R(x, 15 - (x % 2), 2, 1, '#78a35c');
  if (level === 'sun') s += R(53, 4, 7, 7, '#ffd25a') + R(52, 5, 9, 5, '#ffd25a') + R(55, 6, 3, 3, '#fff0b0') + R(56, 2, 1, 1, '#ffe58a') + R(56, 12, 1, 1, '#ffe58a') + R(50, 7, 1, 1, '#ffe58a') + R(62, 7, 1, 1, '#ffe58a') + R(2, 2, 68, 19, '#fff3c4', ' opacity=".15"');
  if (level === 'overcast') s += R(8, 5, 16, 4, '#e9ecec') + R(11, 4, 8, 1, '#e9ecec') + R(9, 9, 14, 1, '#c4cacb') + R(40, 7, 16, 4, '#dfe3e3') + R(43, 6, 8, 1, '#dfe3e3') + R(41, 11, 14, 1, '#b9c0c2');
  if (level === 'rain') {
    s += R(5, 4, 24, 4, '#6b7781') + R(8, 3, 12, 1, '#6b7781') + R(37, 4, 28, 4, '#727e88') + R(41, 3, 12, 1, '#727e88');
    for (const k of [1, 2]) { let d = ''; for (let i = 0; i < 13; i++) d += R(4 + i * 5 + (k - 1) * 2, 9 + ((i + k) % 4), 1, 3, '#cfe3ef'); s += `<g class="f${k}">${d}</g>`; }
  }
  s += R(35, 2, 2, 19, '#7a5236') + R(0, 21, 72, 3, '#9a6a44') + R(0, 21, 72, 1, '#b8865a');
  return `<svg class="hm" viewBox="0 0 72 24" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// ---- #241 `icons`: a monstera leaf on the linen tile (a 64 × 64 grid, drawn procedurally so the slits stay even) ----
function monsteraLeaf(N = 64, k = 2) {
  // k scales the grid (2: the 64 px icon grid, 1: the 32 px favicon, 0.5: 16); three slits a side, open at the margin,
  // one hole in each gap by the midrib, side veins; the light half faces the window (left), the right half is the shade.
  const g = Array.from({ length: N }, () => Array(N).fill('.'));
  const cx = N / 2, cy = 13.2 * k, rx = 12.8 * k, ry = 11.6 * k, slits = [-5.6, 0.2, 6].map(v => v * k), sw = Math.max(0.55, 0.95 * k);
  const inside = (x, y) => {
    const X = x + 0.5 - cx, Y = y + 0.5 - cy, dx = Math.abs(X);
    if ((X / rx) ** 2 + (Y / ry) ** 2 > 1) return false;
    if (Y > 6 * k && dx < (Y - 6 * k) * 1.1) return false; // the heart's notch at the stem
    if (slits.some(y0 => Math.abs(Y - (y0 - 0.62 * dx)) < sw && dx > 4.6 * k)) return false;
    if (k >= 1 && [-2.7, 3.1].some(y0 => (dx - 3 * k) ** 2 / (1.2 * k * k) + (Y - (y0 - 0.62 * 3) * k) ** 2 / (0.9 * k * k) < 1)) return false;
    return true;
  };
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    if (!inside(x, y)) continue;
    const X = x + 0.5 - cx, Y = y + 0.5 - cy, dx = Math.abs(X);
    const edge = k >= 1 && [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([a, b]) => !inside(x + a, y + b));
    let ch = X < 0 ? 'L' : 'l';
    if (k >= 2 && slits.map(v => v + 2.9 * k).some(y0 => Math.abs(Y - (y0 - 0.62 * dx)) < 0.5) && dx > 1.5 * k && dx < 4.2 * k) ch = X < 0 ? '1' : '1';
    if (k >= 2 && X < 0 && dx > 6 * k && dx < 8.5 * k && Y > -8 * k && Y < -3 * k && (X / rx) ** 2 + (Y / ry) ** 2 > 0.55) ch = '2';
    if (dx < 0.6 + 0.4 * k) ch = 'V';
    if (edge) ch = '1';
    g[y][x] = ch;
  }
  const by = Math.round(cy + ry * 0.62);
  for (let y = by; y < N; y++) { const x = Math.round(cx - 1 + (y - by) * 0.3); g[y][x] = 't'; if (k >= 1) g[y][x + 1] = k >= 2 ? '!' : 't'; if (k >= 2) g[y][x - 1] = 't'; }
  return g.map(r => r.join(''));
}
export function icon(size = 512, { tile = '#f1e7cf', round = 0 } = {}) {
  const u = 8, st = '--lf:#4a9a48;--ld:#2f7036;--lv:#a6d98c';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 512 512" shape-rendering="crispEdges" style="${st}">` +
    `<rect width="512" height="512" rx="${round}" fill="${tile}"/><rect y="448" width="512" height="64" fill="#c8643c"/><rect y="448" width="512" height="16" fill="#e08a5c"/><rect y="464" width="512" height="8" fill="#9a4a2a"/>` +
    `${draw([[monsteraLeaf(64, 2), 0, 1]], u)}</svg>`;
}
// The favicon: the leaf alone on a 32 grid (32 px) or a 16 grid (16 px), readable at 16.
export function favicon(size = 32) {
  const N = size >= 32 ? 32 : 16;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${N} ${N}" shape-rendering="crispEdges" style="--lf:#3f8f45;--ld:#2a6a34;--lv:#9fd68a"><rect width="${N}" height="${N}" fill="#f1e7cf"/>${draw([[monsteraLeaf(N, N / 32), 0, 0]], 1)}</svg>`;
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
