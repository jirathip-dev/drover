// #462 bush theme, design mockup: the animals by role, the moments, the hollow-log rail, the outback clearing with its
// billabong and Fire Danger Rating sign, the sign minis, the row sprites, the icon and the ASCII figures all come from
// this module, like ../kitchen/art.mjs and #423's garden. index.html and sheet.html import it; phase 2's build writes
// the theme files (sprites.svg, scene.svg, icons) from the same code.
// Revision 1: twice the pixel density. Lane sprites keep the farm's box, 132 × 100, but one art pixel is 2 units (a
// 66 × 50 grid, was 33 × 25 at 4), feet on y 47, shadow on y 48. The clearing is a 196 × 94 grid at 2 (was 98 × 47
// at 4). The animals are drawn with shapes on a pixel canvas (`Cv`: ellipses, tapered limbs, polygons, one character
// = one pixel), then `shade` adds a lit upper rim, a shaded underside, fur strokes and a soft outline as a separate
// overlay, so the per-lane coat colours stay CSS custom properties (`looks`: one set covers all four animals, so a lane
// keeps its look across roles). `--fk` is the repo colour, worn as a drover's neckerchief. Row sprites stay a 16 × 16
// grid at 24 px (1.5 screen px per art pixel, already finer than 2).

// One character → a fill (and an opacity). `.` and space are transparent.
export const INK = {
  k: '#241a14', q: ['#241a14', 0.18], Q: '#ffffff', x: ['#ffffff', 0.7], T: 'var(--fk,#58a6ff)',
  // the shading overlay: lit rim, shade, fur strokes, the outline
  '+': ['#ffffff', 0.24], '-': ['#241a14', 0.24], '~': ['#241a14', 0.13], '#': ['#241a14', 0.5],
  // kangaroo (and joey): fur, its shade, the pale belly
  R: 'var(--ro,#b8643a)', r: 'var(--rs,#8c4527)', c: 'var(--rb,#ecd2aa)',
  // wombat: fur, shade, the broad nose
  W: 'var(--wo,#7d6a58)', w: 'var(--ws,#5b4b3e)', n: '#2e2420',
  // koala: fur, shade, ear fluff and chest, the big nose
  G: 'var(--ko,#9ea3a8)', g: 'var(--ks,#71777d)', E: '#efece6', N: '#2a2522',
  // emu: shaggy feathers, their shade, the blue neck skin, legs
  M: 'var(--em,#5c4a38)', m: 'var(--es,#3f3226)', B: '#7b9cc0', Y: '#8f7d5a',
  // kookaburra: brown back, its shade, cream head, the blue wing patch, beak (dark top, pale bottom), rufous tail
  U: '#7a5232', u: '#553820', F: '#efe6d2', V: '#5f93c4', X: '#3a3632', Z: '#cdbd9c', J: '#b8602e',
  // cockatoo and magpie white, the sulphur crest, grey beak and feet
  I: '#f6f3ec', H: '#f2d23a', h: '#d6b126', i: '#5f6670',
  // ghost gum: trunk, trunk shade, gum-leaf sage, its shade, its light
  P: '#e6dfcf', p: '#b6ab94', L: '#7d9a66', l: '#5b7a4c', y: '#a3bd8a',
  // red earth, its shade, dust; the hollow log: wood, shade, the dark inside, highlight; the emu egg
  D: '#b5562e', d: '#8a3d1f', e: '#dc9a66', O: '#8b6a48', o: '#62482f', A: '#2f2117', a: '#ad8a62', K: '#2f6a5c', j: '#5f9a88',
};

// ---- map helpers (the row sprites) ----
// A pixel line (Bresenham) as layers of one-pixel maps.
function line(x0, y0, x1, y1, ch) {
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
const f = (l, k) => l.map(a => [a[0], a[1], a[2], k]);

// ---- the pixel canvas ----
// A stable hash in [0, 1) per cell: the fur strokes, the earth's pebbles, the stars. No randomness between renders.
const H32 = (x, y, s = 0) => {
  let h = Math.imul(x + 1013 * s + 1, 374761393) ^ Math.imul(y + 7, 668265263);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
};
// Shapes are tested at pixel centres, so the art is crisp at any size (no smoothing anywhere).
class Cv {
  constructor(w = 66, h = 50) { this.w = w; this.h = h; this.g = Array.from({ length: h }, () => Array(w).fill('.')); }
  at(x, y) { return this.g[y]?.[x] ?? '.'; }
  px(x, y, c) { x = Math.floor(x); y = Math.floor(y); if (x >= 0 && y >= 0 && x < this.w && y < this.h) this.g[y][x] = c; return this; }
  area(x0, y0, x1, y1, test, c) {
    for (let y = Math.max(0, Math.floor(y0)); y <= Math.min(this.h - 1, Math.ceil(y1)); y++)
      for (let x = Math.max(0, Math.floor(x0)); x <= Math.min(this.w - 1, Math.ceil(x1)); x++)
        if (test(x + 0.5, y + 0.5)) this.g[y][x] = typeof c === 'function' ? c(x, y) : c;
    return this;
  }
  ell(cx, cy, rx, ry, c, rot = 0) {
    const a = rot * Math.PI / 180, co = Math.cos(a), si = Math.sin(a), R = Math.max(rx, ry);
    return this.area(cx - R, cy - R, cx + R, cy + R, (x, y) => {
      const dx = x - cx, dy = y - cy, u = dx * co + dy * si, v = dy * co - dx * si;
      return (u / rx) ** 2 + (v / ry) ** 2 <= 1;
    }, c);
  }
  rect(x, y, w, h, c) { return this.area(x, y, x + w - 1, y + h - 1, () => true, c); }
  poly(P, c) {
    const xs = P.map(p => p[0]), ys = P.map(p => p[1]);
    return this.area(Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys), (x, y) => {
      let inside = false;
      for (let i = 0, j = P.length - 1; i < P.length; j = i++) {
        const [xi, yi] = P[i], [xj, yj] = P[j];
        if ((yi > y) !== (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi) inside = !inside;
      }
      return inside;
    }, c);
  }
  // A limb: discs along a path, the radius running r0 → r1 (tails, legs, necks, branches).
  limb(P, r0, r1, c) {
    const L = P.slice(1).map((p, i) => Math.hypot(p[0] - P[i][0], p[1] - P[i][1])), n = L.reduce((a, b) => a + b, 0) || 1;
    let s = 0;
    for (let i = 1; i < P.length; i++) {
      const [ax, ay] = P[i - 1], [bx, by] = P[i];
      for (let t = 0; t <= 1; t += 0.4 / Math.max(1, L[i - 1])) {
        const r = r0 + (r1 - r0) * (s + L[i - 1] * t) / n, x = ax + (bx - ax) * t, y = ay + (by - ay) * t;
        if (r < 0.75) this.px(x, y, c); else this.ell(x, y, r, r, c);
      }
      s += L[i - 1];
    }
    return this;
  }
  map(m, x, y) { m.forEach((r, j) => [...r].forEach((c, i) => { if (c !== '.' && c !== ' ') this.px(x + i, y + j, c); })); return this; }
  put(o, dx = 0, dy = 0) { o.g.forEach((r, y) => r.forEach((c, x) => { if (c !== '.') this.px(x + dx, y + dy, c); })); return this; }
  clip(y) { for (let j = y; j < this.h; j++) this.g[j].fill('.'); return this; }
  rows() { return this.g.map(r => r.join('')); }
}
// The light comes from the upper left: each material's upper/left rim is lit, its lower/right rim and the row above
// that are shaded; fur, feathers, bark and wood get short strokes; the silhouette gets a soft dark outline.
const FLAT = new Set([...'kQxnNAZXiqTH']), FUR = new Set([...'RWGMUOPDI']);
function shade(cv) {
  const o = new Cv(cv.w, cv.h), solid = (x, y) => !'.q'.includes(cv.at(x, y));
  for (let y = 0; y < cv.h; y++) for (let x = 0; x < cv.w; x++) {
    const c = cv.at(x, y);
    if (!solid(x, y)) { if (solid(x - 1, y) || solid(x + 1, y) || solid(x, y - 1) || solid(x, y + 1)) o.px(x, y, '#'); continue; }
    if (FLAT.has(c)) continue;
    const same = (dx, dy) => cv.at(x + dx, y + dy) === c;
    if (!same(0, 1) || !same(1, 0)) o.px(x, y, '-');
    else if (!same(0, -1) || !same(-1, 0)) o.px(x, y, '+');
    else if (!same(0, 2) || !same(2, 0)) o.px(x, y, '~');
    else if (FUR.has(c) && H32(x, y) < 0.11 && same(1, 1)) { o.px(x, y, '~'); o.px(x + 1, y + 1, '~'); }
  }
  return o.rows();
}
// One frame: a back layer (the ground under the animal, its tree, its post), the animal, and effects (dust, Zzz, HA),
// back and animal each with its own shading overlay. `two` makes the loop's f1/f2 pair.
function frame(fn) {
  const back = new Cv(), body = new Cv(), fx = new Cv();
  fn(body, fx, back);
  return [back.rows(), shade(back), body.rows(), shade(body), fx.rows()];
}
const two = (a, b) => [...frame(a).map(m => [m, 0, 0, 'f1']), ...frame(b).map(m => [m, 0, 0, 'f2'])];
const shadow = (c, x0, x1, y = 48) => c.rect(x0, y, x1 - x0, 1, 'q').rect(x0 + 2, y - 1, x1 - x0 - 4, 1, 'q');
// Tiny pixel glyphs for the effects.
const GLYPH = {
  H: ['Q..Q', 'Q..Q', 'QQQQ', 'Q..Q', 'Q..Q'], A: ['.QQ.', 'Q..Q', 'QQQQ', 'Q..Q', 'Q..Q'], '!': ['Q', 'Q', 'Q', '.', 'Q'],
  z: ['QQQ', '..Q', '.Q.', 'Q..', 'QQQ'], Z: ['QQQQ', '...Q', '..Q.', '.Q..', 'Q...', 'QQQQ'],
};
const word = (c, s, x, y) => { for (const ch of s) { c.map(GLYPH[ch], x, y); x += GLYPH[ch][0].length + 1; } return c; };

// ---- the animals (lane scale, 66 × 50) ----
// The kangaroo's head: ears up (alert), back (bounding) or flat (ducking), an eye with a glint, the nose, the mouth.
function rooHead(c, x, y, ears = 'up') {
  const E = { up: [[[x - 3, y - 3], [x - 5, y - 10]], [[x, y - 4], [x, y - 11]]], back: [[[x - 3, y - 3], [x - 9, y - 7]], [[x - 1, y - 4], [x - 6, y - 10]]],
    flat: [[[x - 3, y - 2], [x - 11, y - 3]], [[x - 1, y - 3], [x - 10, y - 6]]] }[ears];
  for (const e of E) c.limb(e, 1.8, 1, 'R');
  for (const [[ax, ay], [bx, by]] of E) c.limb([[ax + (bx - ax) * 0.25, ay + (by - ay) * 0.25], [ax + (bx - ax) * 0.7, ay + (by - ay) * 0.7]], 0.6, 0.6, 'c');
  c.ell(x, y, 5.6, 4.6, 'R').limb([[x + 2, y + 1], [x + 8, y + 3]], 3.3, 2, 'R');
  c.rect(x + 2, y - 1, 2, 2, 'k').px(x + 2, y - 1, 'Q').rect(x + 8, y + 2, 2, 1, 'n').px(x + 9, y + 3, 'n').px(x + 5, y + 4, 'r').px(x + 6, y + 4, 'r');
}
const kerchief = (c, x0, y0, x1, y1, drop = 5) => c.limb([[x0, y0], [x1, y1]], 1.7, 1.7, 'T')
  .poly([[x0 + 1, y0 + 1], [x1 - 1, y1 + 1], [(x0 + x1) / 2, Math.max(y0, y1) + drop]], 'T');
// impl: a kangaroo bounding. Frame 1 crouched on its big feet, frame 2 in the air, legs out behind.
function roo(pose = 'crouch', ears = 'up') {
  const c = new Cv();
  if (pose === 'crouch') {
    c.limb([[27, 41], [17, 44], [8, 46], [1, 47]], 3.6, 0.9, 'R').ell(28, 37, 10.5, 9, 'R', -15);
    c.limb([[32, 34], [40, 21]], 7.5, 5, 'R').limb([[40, 21], [45, 13]], 3.8, 3.4, 'R').limb([[36, 33], [42, 23]], 3.4, 2.4, 'c');
    c.ell(25, 37, 5, 6.5, 'r', -30).ell(24, 36, 4, 5.5, 'R', -30);
    c.limb([[22, 45.5], [39, 46.5]], 2.2, 1.6, 'R').limb([[42, 24], [46, 28], [47.5, 30.5]], 1.4, 0.9, 'R').px(48, 31, 'k').px(46, 31, 'k');
    rooHead(c, 46, 12, ears);
    kerchief(c, 40, 18.5, 45, 17.5);
  } else {
    c.limb([[19, 24], [9, 22], [0, 17]], 3.4, 1, 'R').limb([[20, 28], [11, 32], [3, 34]], 3, 1.6, 'R').ell(24, 25, 9, 6.5, 'R', -8);
    c.limb([[25, 24], [40, 17]], 7, 5, 'R').limb([[39, 17], [47, 11]], 3.8, 3.4, 'R').limb([[28, 27], [41, 20]], 3, 2.2, 'c');
    c.ell(21, 26, 4.5, 4, 'r').ell(22, 25, 4, 3.4, 'R');
    c.limb([[44, 19], [48, 23], [49.5, 25.5]], 1.4, 0.9, 'R').px(50, 26, 'k');
    rooHead(c, 50, 9, ears === 'up' ? 'back' : ears);
    kerchief(c, 43, 15, 48, 14);
  }
  return c;
}
// fix: a wombat digging into a mound, dirt flying out behind on two frames.
function wombat(c, f2) {
  const L = f2 ? [[[17, 42], [19, 47]], [[27, 43], [24, 47]], [[38, 43], [41, 47]], [[44, 41], [48, 47]]]
    : [[[17, 42], [15, 47]], [[27, 43], [28, 47]], [[38, 43], [36, 47]], [[44, 41], [51, 45]]];
  for (const l of L) c.limb(l, 2.7, 2.3, 'w');
  for (const [, [x, y]] of L) c.px(x + 1, y, 'Z').px(x + 2, y, 'Z');
  c.ell(28, 36, 18, 10, 'W').ell(44, 28.8, 2.3, 2.9, 'W').ell(50, 28.8, 2.3, 2.9, 'W').px(44, 28, 'w').px(50, 28, 'w');
  c.ell(48, 36.5, 8, 7.2, 'W');
  kerchief(c, 41, 30, 41, 43, 3);
  c.ell(55, 38.5, 3.2, 3.4, 'n').px(56, 39, 'k').px(57, 39, 'k').px(54, 36, 'x').rect(50, 34, 2, 1, 'k').px(50, 34, 'Q');
}
// The koala's head: fluffy ears, the big nose, eyes open (looking ahead or down) or shut.
function koalaHead(c, x, y, eyes = 'open') {
  for (const s of [-1, 1]) c.ell(x + 10 * s, y - 6, 5.3, 5, 'G').ell(x + 10 * s, y - 5.5, 3, 3, 'E');
  for (const s of [-1, 1]) for (let i = 0; i < 6; i++) c.px(x + s * (13 + (i % 2)) - (s < 0 ? 1 : 0), y - 9 + i * 1.3, 'E');
  c.ell(x, y, 10, 8.5, 'G').ell(x, y + 5.5, 4.5, 2.6, 'E');
  if (eyes === 'shut') for (const s of [-1, 1]) c.px(x + 5 * s - 1, y, 'k').px(x + 5 * s, y + 1, 'k').px(x + 5 * s + 1, y + 1, 'k').px(x + 5 * s + 2, y, 'k');
  else for (const s of [-1, 1]) c.rect(x + 5 * s - 1, y - 2, 2, 2, 'k').px(x + 5 * s - 1, y - (eyes === 'down' ? 1 : 2), 'Q');
  c.ell(x, y + 3, 2.7, 3.8, 'N').px(x - 1, y + 1, 'x');
}
const leaves = (c, x, y, n, s = 0) => {
  for (let i = 0; i < n; i++) { const a = H32(i, s, 3) * 6.28, d = H32(i, s, 4) * 3.5; c.ell(x + Math.cos(a) * d * 1.6 + 1, y + Math.sin(a) * d + 1, 2.6, 1.6, 'l', -20); }
  for (let i = 0; i < n; i++) { const a = H32(i, s, 3) * 6.28, d = H32(i, s, 4) * 3.5; c.ell(x + Math.cos(a) * d * 1.6, y + Math.sin(a) * d, 2.4, 1.4, i % 3 ? 'L' : 'y', -20); }
};
// The emu: a shaggy body, a long neck with blue skin, legs scissoring on the two frames.
function emu(c, f2) {
  const legs = f2 ? [[[25, 28], [27, 37], [26, 46]], [[22, 28], [19, 37], [22, 45]]] : [[[26, 28], [31, 37], [36, 46]], [[22, 28], [15, 36], [8, 41]]];
  for (const l of legs) c.limb(l, 1.5, 0.9, 'Y');
  for (const l of legs) { const [x, y] = l[2]; c.limb([[x - 1, y + 1], [x + 3, y + 1]], 0.6, 0.6, 'Y'); }
  c.limb([[10, 19], [5, 23]], 2.4, 1, 'm').ell(23, 21, 14, 9, 'M');
  for (let x = 11; x <= 35; x += 2) c.limb([[x, 25], [x - 1.5, 30 + (x % 4 ? 1 : 0)]], 1, 0.6, x % 4 ? 'M' : 'm');
  c.limb([[33, 18], [38, 11], [41, 4]], 3.2, 1.8, 'M').limb([[37, 13], [40.5, 5]], 1.3, 1, 'B');
  c.ell(42.5, 3, 3, 2.6, 'M').limb([[45, 3.5], [49, 4.5]], 0.9, 0.6, 'X').px(43, 2, 'J').px(44, 2, 'k');
  kerchief(c, 32, 17, 37, 15, 4);
}
// The kookaburra on its post: closed (frame 1) or laughing, head back and beak wide (frame 2).
function kook(c, back, open) {
  back.rect(29, 34, 9, 14, 'O').rect(29, 34, 9, 1, 'a');
  for (const x of [31, 34]) back.rect(x, 37 + (x % 2) * 3, 1, 5, 'o');
  c.limb([[26, 30], [22, 43]], 2, 1.6, 'J');
  for (const y of [34, 37, 40]) { const tx = 26 - (y - 30) * 4 / 13; c.limb([[tx - 1.4, y], [tx + 1.4, y - 0.5]], 0.5, 0.5, 'u'); }
  c.ell(31, 25, 7.5, 9.5, 'U').ell(34, 26, 5, 8, 'F').ell(28, 25, 5.5, 8, 'U', 10).ell(29, 21, 3.2, 2.2, 'V');
  for (let i = 0; i < 4; i++) c.px(26 + i * 2, 28 + (i % 2), 'u');
  c.rect(31, 34, 2, 1, 'Y').rect(35, 34, 2, 1, 'Y');
  const [x, y] = open ? [32, 11] : [33, 12];
  c.ell(x, y, 7.5, 6.5, 'F').limb([[x - 5, y - 4], [x + 3, y - 5]], 1.4, 1, 'u').limb([[x - 4, y + 1], [x + 5, y]], 0.6, 0.6, 'u');
  c.rect(x + 2, y - 1, 2, 2, 'k').px(x + 2, y - 1, 'Q');
  if (open) c.poly([[x + 5, y - 2], [x + 16, y - 6], [x + 6, y + 1]], 'X').poly([[x + 5, y + 2], [x + 15, y + 6], [x + 5, y + 4]], 'Z');
  else c.poly([[x + 5, y - 2], [x + 17, y + 1], [x + 5, y + 2]], 'X').poly([[x + 5, y + 2], [x + 16, y + 1.5], [x + 5, y + 4]], 'Z');
  kerchief(c, x - 4, y + 7, x + 3, y + 7, 3);
}
function kookSmall(c, x, y, open) {
  c.limb([[x - 2, y + 7], [x - 4, y + 11]], 1.3, 1, 'J').ell(x, y + 5, 3.5, 4.5, 'U').ell(x + 1.5, y + 5.5, 2.2, 3.5, 'F').ell(x + 1, y, 3.5, 3, 'F');
  c.limb([[x - 1, y + 0.5], [x + 3, y]], 0.5, 0.5, 'u').px(x + 2, y - 1, 'k').px(x - 1, y + 4, 'V').px(x, y + 4, 'V');
  if (open) c.poly([[x + 3.5, y - 1], [x + 9, y - 3.5], [x + 4, y + 0.5]], 'X').poly([[x + 3.5, y + 1], [x + 8, y + 3.5], [x + 3.5, y + 2]], 'Z');
  else c.poly([[x + 3.5, y - 1], [x + 9.5, y + 0.5], [x + 3.5, y + 1]], 'X').poly([[x + 3.5, y + 1], [x + 9, y + 1], [x + 3.5, y + 2]], 'Z');
}
// A magpie: black and white, red-brown eye, pale beak; wings up (frame 1) or down mid-swoop.
function magpie(c, x, y, down) {
  c.limb([[x - 5, y - 1.5], [x - 12, y - 3]], 1.6, 1, 'k').px(x - 6, y - 2, 'I').px(x - 7, y - 2, 'I');
  if (down) c.poly([[x - 3, y], [x + 3, y], [x - 2, y + 9], [x - 6, y + 7]], 'k').ell(x - 1, y + 2, 1.8, 1.2, 'I');
  else c.poly([[x - 3, y - 1], [x + 3, y - 1], [x - 5, y - 10], [x - 8, y - 8]], 'k').ell(x - 1, y - 2.5, 2, 1.2, 'I');
  c.ell(x, y, 6, 3.2, 'k', 15).ell(x + 6, y + 2.5, 3, 2.6, 'k').ell(x + 4, y + 0.5, 2, 1.4, 'I');
  c.limb([[x + 8.5, y + 3], [x + 12.5, y + 4.5]], 0.8, 0.5, 'Z').px(x + 7, y + 2, 'J');
}
// The hollow log of the rail: one section with a knot hole, so items in a row read as one log.
function log(c) {
  c.rect(0, 24, 66, 22, 'O');
  for (const [x0, x1, y] of [[2, 18, 28], [44, 62, 29], [6, 20, 41], [46, 60, 40], [24, 40, 44]]) c.limb([[x0, y], [x1, y]], 0.5, 0.5, 'o');
  c.ell(33, 34, 10, 7, 'o').ell(33, 34, 7.5, 5, 'A');
}
function joey(c, dy = 0) {
  const y = 33 + dy;
  c.limb([[30.5, y - 2], [29, y - 7]], 1.2, 0.8, 'R').limb([[35.5, y - 2], [37, y - 7]], 1.2, 0.8, 'R').px(29.5, y - 5, 'c').px(36.5, y - 5, 'c');
  c.ell(33, y, 4.8, 3.8, 'R').ell(33, y + 1.6, 2.6, 1.7, 'c').px(31, y - 1, 'k').px(35, y - 1, 'k').rect(32.5, y, 2, 1, 'n');
}
function cocky(c, open) {
  const [hx, hy] = open ? [49, 6] : [50, 7];
  c.limb([[45, 21], [43, 26]], 2, 1.2, 'I').ell(48, 15, 5.5, 7.5, 'I', -10).ell(46.5, 15.5, 3.8, 6, 'I', -10).rect(46, 23, 1, 1, 'i').rect(49, 23, 1, 1, 'i');
  const crest = open ? [[[hx - 1, hy - 3], [hx - 5, hy - 9]], [[hx, hy - 3], [hx - 1, hy - 10]], [[hx + 1, hy - 3], [hx + 3, hy - 9]]]
    : [[[hx - 1, hy - 3], [hx - 4, hy - 7]], [[hx, hy - 3], [hx - 2, hy - 8]], [[hx + 1, hy - 3], [hx, hy - 7]]];
  for (const l of crest) c.limb(l, 1.1, 0.7, 'H');
  c.ell(hx, hy, 4.4, 4, 'I').px(hx + 1, hy - 1, 'k').px(hx + 2, hy - 1, 'E');
  if (open) c.ell(hx + 4.5, hy, 1.6, 1.4, 'i').ell(hx + 4, hy + 3, 1.4, 1, 'i');
  else c.ell(hx + 4, hy + 2, 1.7, 2.2, 'i').px(hx + 4, hy + 3, 'k');
}

// The 7 actions (the contract's `actions`): f1/f2 are the loop's two frames.
export const ACTIONS = {
  bound: () => two((c, fx, b) => { shadow(b, 4, 46); c.put(roo('crouch')); },
    (c, fx, b) => { shadow(b, 14, 40); c.put(roo('air'), 2, 2); fx.map(['e.e', '.e.', 'e..'], 4, 44).map(['.e', 'e.'], 10, 46); }),
  dig: () => {
    const base = (b, fx, f2) => {
      shadow(b, 2, 58); b.ell(59, 47, 10, 8, 'D').clip(48).ell(61, 44, 3, 2, 'd');
      for (const [x, y, k] of f2 ? [[4, 22, 'd'], [1, 27, 'e'], [8, 18, 'e'], [3, 33, 'd']] : [[7, 26, 'e'], [3, 30, 'd'], [10, 21, 'd'], [1, 24, 'e']])
        fx.rect(x, y, 2, 2, k).px(x + 3, y + 3, k);
    };
    return two((c, fx, b) => { base(b, fx, 0); wombat(c, 0); }, (c, fx, b) => { base(b, fx, 1); wombat(c, 1); });
  },
  peer: () => {
    const base = b => {
      shadow(b, 50, 66); b.limb([[58, 0], [58, 49]], 4, 4.4, 'P').limb([[56, 31], [30, 32], [6, 33]], 2.6, 1.4, 'P');
      for (let y = 4; y < 48; y += 5) b.limb([[60, y], [60, y + 3]], 0.5, 0.5, 'p');
      b.ell(57, 18, 1.6, 2.2, 'p'); leaves(b, 52, 4, 9, 1); leaves(b, 62, 2, 7, 2); leaves(b, 6, 36, 5, 3);
    };
    const koala = (c, f2) => {
      c.limb([[24, 30], [23, 37]], 2.4, 2, 'G').limb([[37, 30], [38, 37]], 2.4, 2, 'G').ell(30, 26, 9, 7.5, 'G').ell(30, 27, 4, 4, 'E');
      c.limb([[22, 24], [19, 31]], 2, 1.8, 'G').limb([[38, 24], [41, 31]], 2, 1.8, 'G');
      for (const x of [18, 20, 40, 42]) c.px(x, 32, 'k');
      kerchief(c, 24, 21, 36, 21, 4);
      koalaHead(c, f2 ? 28 : 30, f2 ? 15 : 13, f2 ? 'down' : 'open');
    };
    return two((c, fx, b) => { base(b); koala(c, 0); }, (c, fx, b) => { base(b); koala(c, 1); });
  },
  run: () => two((c, fx, b) => { shadow(b, 6, 42); emu(c, 0); }, (c, fx, b) => { shadow(b, 12, 36); emu(c, 1); fx.map(['e.', '.e'], 4, 45); }),
  sleep: () => {
    const base = b => {
      shadow(b, 22, 44); b.limb([[32, 49], [32, 30]], 4, 3.5, 'P').limb([[31, 30], [18, 12], [12, 4]], 3, 1.2, 'P').limb([[34, 30], [46, 12], [52, 3]], 3, 1.2, 'P');
      for (let y = 34; y < 48; y += 4) b.px(34, y, 'p').px(34, y + 1, 'p');
      b.ell(31, 40, 1.4, 2, 'p'); leaves(b, 12, 4, 9, 4); leaves(b, 52, 4, 9, 5);
    };
    const koala = c => {
      c.ell(33, 26, 9, 6, 'G').limb([[38, 24], [43, 19]], 1.8, 1.6, 'G').px(43, 18, 'k').px(44, 19, 'k');
      kerchief(c, 28, 23, 33, 25, 2);
      koalaHead(c, 26, 20, 'shut');
    };
    return two((c, fx, b) => { base(b); koala(c); word(fx, 'z', 42, 7); },
      (c, fx, b) => { base(b); koala(c); word(fx, 'z', 42, 7); word(fx, 'Z', 35, 0); });
  },
  laugh: () => two((c, fx, b) => { shadow(b, 24, 42); kook(c, b, 0); },
    (c, fx, b) => { shadow(b, 24, 42); kook(c, b, 1); word(fx, 'HA', 50, 12); word(fx, 'HA', 52, 20); }),
  stuck: () => {
    const base = b => { shadow(b, 4, 62); b.ell(33, 49, 30, 19, 'D').ell(33, 47, 17, 13, 'A').clip(48); b.ell(14, 40, 2, 1, 'd').ell(52, 38, 2, 1, 'd'); };
    const rump = (c, f2) => {
      const L = f2 ? [[[23, 45], [15, 46]], [[43, 44], [51, 39]]] : [[[23, 44], [15, 39]], [[43, 45], [51, 46]]];
      for (const l of L) c.limb(l, 2.6, 2.2, 'W');
      for (const [, [x, y]] of L) c.px(x + (x < 33 ? -2 : 2), y, 'Z').px(x + (x < 33 ? -2 : 2), y + 1, 'Z');
      c.ell(33, 42, 12, 9, 'W').limb([[33, 36], [33, 44]], 0.5, 0.5, 'w').ell(33, 34.5, 1.2, 1, 'w').clip(48);
    };
    return two((c, fx, b) => { base(b); rump(c, 0); fx.map(['e.e', '.e.'], 6, 30); },
      (c, fx, b) => { base(b); rump(c, 1); fx.map(['.e.', 'e.e'], 56, 29); });
  },
};
// The contract's `actions`, by role and state: the sprite id is `a-<action>`.
export const ACT_OF = { impl: 'bound', fix: 'dig', rev: 'peer', update: 'run', idle: 'sleep', done: 'laugh', blocked: 'stuck' };
export const ACT_WORD = { bound: 'bound', dig: 'dig', peer: 'peer', run: 'run', sleep: 'asleep', laugh: 'laugh', stuck: 'stuck' };

// ---- the moments (#241 `moments`) and the rail ----
export const RAIL = {
  joey: () => two(c => { log(c); joey(c); }, c => { log(c); joey(c, 1); }),
  egg: () => two(c => { log(c); c.ell(33, 35, 4.5, 5, 'K'); for (const [x, y] of [[31, 33], [34, 36], [32, 38], [35, 32]]) c.px(x, y, 'j'); },
    c => { log(c); c.ell(33, 35, 4.5, 5, 'K'); for (const [x, y] of [[31, 33], [34, 36], [32, 38], [35, 32]]) c.px(x, y, 'j'); }),
  cocky: () => two(c => { log(c); joey(c, 1); cocky(c, 0); }, (c, fx) => { log(c); joey(c, 1); cocky(c, 1); word(fx, '!', 58, 1); fx.map(['Q.', '.Q', '..', 'QQ'], 61, 6); }),
};
export const MOMENTS = {
  // merged: the kangaroo clears the station fence in one bound; a kookaburra laughs on the post (hop)
  clear: () => {
    const fence = b => { shadow(b, 8, 52); b.rect(4, 33, 4, 15, 'O').rect(56, 33, 4, 15, 'O').rect(1, 36, 64, 2, 'O').rect(1, 42, 64, 2, 'O'); };
    return two((c, fx, b) => { fence(b); c.put(roo('air'), -4, 0); kookSmall(c, 53, 23, 0); },
      (c, fx, b) => { fence(b); c.put(roo('air'), -4, -1); kookSmall(c, 53, 23, 1); word(fx, 'HA', 56, 13); });
  },
  // review FAIL: a magpie swoops the kangaroo, which ducks, ears flat (shake)
  swoop: () => two((c, fx, b) => { shadow(b, 4, 46); c.put(roo('crouch', 'flat'), 0, 2); magpie(c, 18, 12, 0); },
    (c, fx, b) => { shadow(b, 4, 46); c.put(roo('crouch', 'flat'), 0, 2); magpie(c, 30, 8, 1); fx.map(['Q.Q', '.Q.', 'Q.Q'], 50, 2).map(['Q', '.', 'Q'], 56, 6); }),
  // needs you: a cockatoo screeches on the hollow log (pulse)
  screech: RAIL.cocky,
};
export const SAYS = { clear: 'over the fence!', swoop: 'swooped!', screech: 'screech!' };

// ---- #241 `row_sprites` (16 × 16) ----
const RR = [
  '...........r.r..',
  '...........RrRr.',
  '..........RRRRR.',
  '..........RkRRRk',
  '.........TTRRR..',
  '.....RRRRTTc....',
  'rr.RRRRRRRccR...',
  '.rrRRRRRRRcc.k..',
  '...rRRRRRRc.....',
  '....rrRRRr......',
  '...rrrrrr.......',
  '.rrrrk..........',
];
const RR_CROUCH = [
  '...........r.r..',
  '...........RrRr.',
  '..........RRRRR.',
  '..........RkRRRk',
  '.........TTRRR..',
  '........RRTTc...',
  '.......RRRRccRk.',
  '......RRRRRcc...',
  '....rrRRRRRRc...',
  '..rr..rRRRRRR...',
  '.rr...rrrRRRr...',
  'rr.....rrrrrrrk.',
];
export const ROWS = {
  bound: [[RR_CROUCH, 0, 3, 'f1'], [RR, 0, 1, 'f2'], [['.qqqqqqqqqq.'], 2, 15]],
  dig: [[['..wwwwww.......', '.wWWWWWWWw.....', 'wWWWWWWWWWwW...', 'wWWWWWWWWTWkW.e', 'wWWWWWWWWTWWWnD', '.wWWWWWWWWWWnnD', '..ww.ww..ww.DDD'], 1, 8], [['eDDDD'], 11, 15],
    [['d.', '.e'], 0, 5, 'f1'], [['e', '.', 'd'], 2, 3, 'f2']],
  peer: [[['pppPPPPPPPPPPPP'], 0, 11], [['pppPPPPPPPPPPPP'], 0, 12], [Array(16).fill('PPPp'), 12, 0], [['.l', 'lL'], 0, 13],
    ...f([[['EE......EE', 'EGGGGGGGGE', 'GGGGGGGGGG', 'GGGGGGGGGG', 'GkGGGGGGkG', 'GGGGNNGGGG', '.GGGNNGGG.', '..TTTTTT..'], 1, 2], [['gg....gg'], 2, 10]], 'f1'),
    ...f([[['EE......EE', 'EGGGGGGGGE', 'GGGGGGGGGG', 'GGGGGGGGGG', 'GkGGGGGGkG', 'GGGGNNGGGG', '.GGGNNGGG.', '..TTTTTT..'], 0, 3], [['gg....gg'], 2, 10]], 'f2')],
  run: [[['......kMM.', '.....MMMkX', '.....BBB..', '....BTB...', '.mMMMMB...', 'mMMMMMMM..', 'mMmMMmMMm.', '.mMMMMMm..'], 2, 1],
    ...f([...line(6, 9, 4, 13, 'Y'), [['YY'], 2, 14], ...line(7, 9, 9, 13, 'Y'), [['YY'], 9, 14]], 'f1'), ...f([...line(6, 9, 6, 13, 'Y'), [['YY'], 6, 14], ...line(7, 9, 8, 13, 'Y'), [['Y'], 9, 14]], 'f2')],
  sleep: [[['P.......P', 'PP.....PP', '.PP...PP.', '..PPPPP..', '...PPP...'], 3, 11], [['EE....EE', 'EGGGGGGE', 'GkkGGkkG', 'GGGNNGGG', '.GGNNGG.', '..gggg..'], 3, 5],
    [['QQ', '.Q', 'QQ'], 12, 1, 'f1'], [['QQ', '.Q', 'QQ'], 13, 0, 'f2']],
  laugh: [[['...FFFF.......', '..FFuuuF......', '.FFFFkuFXXXX..', '.FFFFFFFZZZZX.', '..FFFFFF......', '.UUFTTFF......', 'UUVUFFFF......', 'UVUUUFF.......', 'JUUUUU........', 'JJ.Y.Y........', '...oOOo.......'], 1, 3, 'f1'],
    [['........XX....', '...FFFFXXX....', '..FFuuuFX.....', '.FFFFkuF......', '.FFFFFFFZZ....', '..FFFFFF.ZZZ..', '.UUFTTFF......', 'UUVUFFFF......', 'UVUUUFF.......', 'JUUUUU........', 'JJ.Y.Y........', '...oOOo.......'], 1, 2, 'f2'],
    [['Q.Q', 'QQQ', 'Q.Q'], 13, 3, 'f2']],
  stuck: [[['....eDDDDDDe....', '..eDDDDDDDDDDe..', '.eDDDAAAAAADDDe.', 'eDDDAAAAAAAADDDd', 'DDDAAAAAAAAAADDd', 'DDDAAAAAAAAAADdd'], 0, 9],
    [['..wwwwww..', '.wWWWWWWw.', 'wWWWWwWWWw', 'wWWWWwWWWw', 'wwWWWWWWww'], 3, 10], [['w'], 7, 9], [['ww'], 2, 13, 'f1'], [['ww'], 12, 12, 'f1'], [['ww'], 2, 12, 'f2'], [['ww'], 12, 13, 'f2']],
  joey: [[['aaaaaaaaaaaaaaaa', ...Array(6).fill('OOOOOOOOOOOOOOOO'), 'oooooooooooooooo'], 0, 7], [['.oooooo.', 'oAAAAAAo', 'oAAAAAAo', 'oAAAAAAo', '.oooooo.'], 4, 8], [['r..r', 'RrRr', 'kRRk'], 6, 6, 'f1'], [['r..r', 'RrRr', 'kRRk'], 6, 7, 'f2']],
  needs: [[['..H.H.', '..HHH.', '.fQQQf', 'fQQkQQii', 'fQQQQQi.', 'fQQQQQf.', '.fQQQf..', '..i.i...'], 5, 0, 'f1'], [['.H..HH', '..HHHH.', '.fQQQf.', 'fQQkQQii', 'fQQQQf..', 'fQQQQii.', '.fQQQf..', '..i.i...'], 5, 0, 'f2'],
    [['aaaaaaaaaaaaaaaa', 'OOOOOOOOOOOOOOOO', 'OOooOOOOOOoOOOOO', 'OOOOOOOOOOOOOOOO', 'oooooooooooooooo'], 0, 8], [['Q', '.', 'Q'], 14, 0, 'f2']],
  merged: [[['o..........o', 'o..........o', 'iiiiiiiiiiii', 'o..........o', 'iiiiiiiiiiii', 'o..........o'], 2, 10], [RR, 0, -1, 'f1'], [RR, 0, -2, 'f2']],
  fail: [[RR_CROUCH.slice(3), 0, 7], [['kk.......', '.kkk...kQ', '..kQQkkkki', '...kkkQ..'], 5, 0, 'f1'], [['kk........', '.kkkQQkkkki', '..QQkkk...'], 6, 2, 'f2']],
};


// ---- rasterising: layers → <rect> runs, one <g> per frame class ----
function rects(map, x0, y0, u, ink = INK) {
  let s = '';
  map.forEach((row, j) => {
    let i = 0;
    while (i < row.length) {
      const c = row[i];
      if (c === '.' || c === ' ' || !ink[c]) { i++; continue; }
      let n = 1;
      while (row[i + n] === c) n++;
      const [fill, op] = Array.isArray(ink[c]) ? ink[c] : [ink[c], 1];
      s += `<rect x="${(x0 + i) * u}" y="${(y0 + j) * u}" width="${n * u}" height="${u}" fill="${fill}"${op < 1 ? ` opacity="${op}"` : ''}/>`;
      i += n;
    }
  });
  return s;
}
export function draw(layers, u = 2) {
  return layers.map(([map, x, y, cls]) => cls ? `<g class="${cls}">${rects(map, x, y, u)}</g>` : rects(map, x, y, u)).join('');
}
// Symbols for a page: a-<action>, m-<moment>, q-<rail item> (132 × 100, 66 × 50 at 2), r-<row> (16 × 16).
export function symbols() {
  const sym = (id, vb, body) => `<symbol id="${id}" viewBox="${vb}" shape-rendering="crispEdges">${body}</symbol>`;
  let s = '';
  for (const [k, fn] of Object.entries(ACTIONS)) s += sym(`a-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, fn] of Object.entries(MOMENTS)) s += sym(`m-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, fn] of Object.entries(RAIL)) s += sym(`q-${k}`, '0 0 132 100', draw(fn()));
  for (const [k, m] of Object.entries(ROWS)) s += sym(`r-${k}`, '0 0 16 16', draw(m, 1));
  s += sym('loading', '0 0 132 100', draw(ACTIONS.bound()));
  return s;
}

// ---- the per-lane looks: one set for all four animals, 8 of them like the farm's coats (the same stable pick) ----
// A look is a kangaroo coat (red, grey, sandy), a wombat coat, a koala coat and an emu coat.
const ROO = { red: ['#b8643a', '#8c4527', '#ecd2aa'], grey: ['#9b8e82', '#76695e', '#e3dacb'], sandy: ['#c9935a', '#a0703f', '#f0dcb8'], dusk: ['#a3573f', '#7d3e2c', '#e8c7a6'] };
const WOM = { brown: ['#7d6a58', '#5b4b3e'], grey: ['#80786f', '#5f5852'], sandy: ['#9b8166', '#78624c'], dark: ['#5e5047', '#433832'] };
const KOA = { grey: ['#9ea3a8', '#71777d'], silver: ['#b3b6b6', '#878b8c'], brown: ['#9a8f86', '#71675f'] };
const EMUC = { brown: ['#5c4a38', '#3f3226'], dark: ['#3f362e', '#2a231d'], tawny: ['#7a6248', '#584532'] };
export const LOOKS = [
  ['red', 'brown', 'grey', 'brown'], ['grey', 'grey', 'silver', 'dark'], ['sandy', 'sandy', 'brown', 'tawny'], ['dusk', 'dark', 'grey', 'brown'],
  ['grey', 'brown', 'brown', 'tawny'], ['red', 'grey', 'silver', 'dark'], ['sandy', 'dark', 'grey', 'brown'], ['dusk', 'sandy', 'silver', 'tawny'],
].map(([ro, wo, ko, em]) => ({
  ro, wo, ko, em,
  vars: { '--ro': ROO[ro][0], '--rs': ROO[ro][1], '--rb': ROO[ro][2], '--wo': WOM[wo][0], '--ws': WOM[wo][1], '--ko': KOA[ko][0], '--ks': KOA[ko][1], '--em': EMUC[em][0], '--es': EMUC[em][1] },
}));
export const FLEET = { l: '#58a6ff', p: '#bc8cff', c: '#39c5cf', g: '#3fb950', a: '#d29922' };
export const style = (look, fk = 'l') => Object.entries({ ...LOOKS[look].vars, '--fk': FLEET[fk] || fk }).map(([k, v]) => `${k}:${v}`).join(';');

// ---- the Fire Danger Rating sign: four bands, Moderate · High · Extreme · Catastrophic (the AFDRS colours) ----
export const LEVELS = ['moderate', 'high', 'catastrophic'];
const BANDS = ['#3e9a4a', '#f2c31a', '#e8731e', '#c41f1e'];
const NEEDLE = { moderate: 157.5, high: 112.5, catastrophic: 22.5 };
// The dial as grid cells: a half ring centred on (cx, cy), radius r0..r1, each cell coloured by its angle.
function dial(cx, cy, r0, r1, u, x0 = 0, y0 = 0) {
  let s = '';
  for (let y = Math.floor(cy - r1); y < cy; y++) for (let x = Math.floor(cx - r1); x < cx + r1; x++) {
    const dx = x + 0.5 - cx, dy = cy - (y + 0.5), d = Math.hypot(dx, dy);
    if (d < r0 || d > r1) continue;
    const ang = Math.atan2(dy, dx) * 180 / Math.PI, b = Math.min(3, Math.floor((180 - ang) / 45));
    s += `<rect x="${(x0 + x) * u}" y="${(y0 + y) * u}" width="${u}" height="${u}" fill="${BANDS[b]}"/>`;
  }
  return s;
}
function needle(cx, cy, len, deg, u, x0 = 0, y0 = 0, w = 1) {
  const a = deg * Math.PI / 180, cells = new Set();
  for (let t = 0; t <= len; t += 0.35) cells.add(`${Math.floor(cx + Math.cos(a) * t)},${Math.floor(cy - Math.sin(a) * t)}`);
  let s = '';
  for (const c of cells) { const [x, y] = c.split(',').map(Number); s += `<rect x="${(x0 + x) * u}" y="${(y0 + y) * u}" width="${u * w}" height="${u}" fill="#1b1b1b"/>`; }
  return s;
}
// The roadside sign, in scene units (u = 4): the board, the dial, the needle per level (`.s-<level>`, a 4° wobble on f2).

// ---- the clearing (scene.svg): one repo's clearing of ghost gums on red earth along a dirt track, 392 × 188, a
// 196 × 94 grid at 2 units a pixel. A billabong at the front, the station gate over the track in the repo colour
// (`--fk`), a hollow log by the track and, at the clearing's right edge, a small roadside Fire Danger Rating sign about
// a kangaroo's height (`.fdr`). `.sky-day` / `.sky-night` follow the scheme's light/dark; `.s-<level>` and `.s-up`
// (high and catastrophic) the load: the needle, heat shimmer over the track, cicadas rising from the gums. ----
const SW = 196, SH = 94, HZ = 52, BAYER = [[0, 2], [3, 1]];
const dith = (x, y, t) => (BAYER[y & 1][x & 1] + 0.5) / 4 < t;
// A dithered vertical gradient: bands '0'..'n-1' down to the horizon.
function skyBands(c, n) {
  for (let y = 0; y < HZ; y++) for (let x = 0; x < SW; x++) { const t = y / HZ * (n - 1), b = Math.floor(t); c.px(x, y, String(Math.min(n - 1, b + (dith(x, y, t - b) ? 1 : 0)))); }
  return c;
}
const SKY_DAY = { 0: '#93c6e0', 1: '#a8d2e2', 2: '#bfdde0', 3: '#d5e3d6', 4: '#e7e5ca', 5: '#f3e2bd', S: '#fff0b8', s: '#fffbe6', c: '#fbf6ea', C: '#e4dccb' };
const SKY_NIGHT = { 0: '#090b2b', 1: '#0f1236', 2: '#161942', 3: '#1e1f4d', 4: '#282657', 5: '#342c60', '*': '#e8e6f5', '+': '#8d8ab8', X: '#ffffff', W: '#fff6d8', m: '#2f2d63' };
const GROUND = {
  M: '#c4794f', m: '#a35d3c', h: '#d99c6e', g: '#bf8f6c', G: '#a97c5d',
  D: '#b5562e', d: '#9c4626', E: '#c96a3a', e: '#d98a55', k: '#7d3519',
  T: '#d9a070', t: '#c48558', Y: '#e6b688', S: '#b8a84a', s: '#8f8a3a', Z: '#d6c86a',
  P: '#ece6d6', p: '#c2b49a', o: '#dcc0b0', L: '#7d9a66', l: '#5b7a4c', y: '#a3bd8a', v: '#44603a',
  W: '#62482f', w: '#8b6a48', F: 'var(--fk,#3fb950)', f: ['#241a14', 0.3], Q: ['#ffffff', 0.6],
  B: '#5d8ea3', b: '#4a7a90', R: '#9cc4cf', r: '#c9e1e6', n: '#6d8a4a', c: '#4f3a2a', K: '#8a7a6a', j: '#6b5c4f',
  O: '#8b6a48', U: '#62482f', A: '#2f2117', a: '#ad8a62',
};
const FDR = { g: '#1d4a2c', W: '#f4efe2', w: ['#f4efe2', 0.8], 1: '#3e9a4a', 2: '#f2c31a', 3: '#e8731e', 4: '#c41f1e', k: '#1b1b1b', P: '#6b6f73', p: '#8d9196', q: ['#241a14', 0.25] };
const FX = { N: ['#0c0f33', 0.45], h: ['#fff4d8', 0.5], c: '#2e3a24', w: ['#e6efe0', 0.75] };
const paint = (cv, ink) => rects(cv.rows(), 0, 0, 2, ink);

// A gum's crown: dark undersides, then the sage leaves, then the light on top, in clumps.
function crown(c, x, y, rx, ry, s) {
  const B = Array.from({ length: Math.round(rx * ry / 3) + 3 }, (_, i) => [x + (H32(i, s, 1) - 0.5) * 2 * rx, y + (H32(i, s, 2) - 0.5) * 2 * ry]);
  for (const [bx, by] of B) c.ell(bx + 1, by + 1.2, 3.2, 2, 'v');
  for (const [bx, by] of B) c.ell(bx + 0.5, by + 0.6, 3, 1.8, 'l');
  for (const [bx, by] of B) c.ell(bx, by, 2.6, 1.5, 'L');
  B.forEach(([bx, by], i) => { if (H32(i, s, 5) < 0.6) c.ell(bx - 1, by - 0.8, 1.5, 0.7, 'y'); });
}
// A ghost gum: a pale trunk with bark streaks and pink patches, limbs out to its crowns.
function gum(c, x, base, h, s) {
  const top = base - h, A = [[[x, top + h * 0.45], [x - h * 0.3, top + h * 0.12], [x - h * 0.45, top]], [[x, top + h * 0.38], [x + h * 0.32, top + h * 0.1], [x + h * 0.5, top - 1]], [[x, top + h * 0.3], [x + 1, top - 2]]];
  for (const b of A) c.limb(b, Math.max(1, h / 22), 0.6, 'P');
  c.limb([[x, base], [x + 1, base - h * 0.5], [x, top + h * 0.3]], Math.max(1.4, h / 16), Math.max(1, h / 26), 'P');
  for (let y = Math.round(top + h * 0.35); y < base; y++) { c.px(x + Math.max(1, h / 18) + (y % 7 < 3 ? 0 : 0.6), y, 'p'); if (H32(x, y, s) < 0.12) c.px(x - 0.5, y, 'o'); }
  const R = Math.max(5, h / 4);
  crown(c, x - h * 0.45, top - 1, R, R * 0.55, s + 1); crown(c, x + h * 0.5, top - 2, R, R * 0.55, s + 2); crown(c, x + 1, top - 4, R * 0.9, R * 0.6, s + 3);
}
function spinifex(c, x, y, w, s) {
  c.ell(x, y + 0.5, w, w * 0.4, 's').ell(x - 0.5, y, w * 0.85, w * 0.35, 'S');
  for (let k = 0; k < w * 2; k++) { const sx = x + (H32(k, s, 6) - 0.5) * w * 1.6; c.limb([[sx, y], [sx + (H32(k, s, 7) - 0.5) * 3, y - 2 - H32(k, s, 8) * w * 0.8]], 0.5, 0.5, k % 3 ? 'Z' : 'S'); }
}
function ground() {
  const c = new Cv(SW, SH);
  // the far range: a flat-topped mesa with eroded flanks, low hills either side
  c.poly([[0, HZ], [0, 45], [18, 43], [40, 46], [58, HZ]], 'g').poly([[150, HZ], [168, 45], [196, 43], [196, HZ]], 'g');
  for (let x = 0; x < SW; x++) for (let y = 47; y < HZ; y++) if (c.at(x, y) === 'g' && dith(x, y, (y - 47) / 5)) c.px(x, y, 'G');
  c.poly([[102, HZ], [108, 41], [113, 37.5], [145, 37.5], [150, 41], [156, HZ]], 'M').rect(113, 37, 32, 1, 'h');
  for (let x = 106; x < 154; x += 3) c.limb([[x + 0.5, 39 + H32(x, 1) * 3], [x + 0.5, 44 + H32(x, 2) * 6]], 0.5, 0.5, 'm');
  c.poly([[140, 38], [145, 37.5], [150, 41], [156, HZ], [146, HZ]], 'm');
  // red earth, paler toward the horizon, pebbles with their shadows, a few cracks
  c.rect(0, HZ, SW, SH - HZ, 'D');
  for (let y = HZ; y < HZ + 8; y++) for (let x = 0; x < SW; x++) if (dith(x, y, 1 - (y - HZ) / 8)) c.px(x, y, 'E');
  for (let i = 0; i < 900; i++) {
    const x = H32(i, 1, 9) * SW, y = HZ + 2 + H32(i, 2, 9) * (SH - HZ - 2), k = H32(i, 3, 9);
    if (k < 0.5) c.px(x, y, 'd'); else if (k < 0.8) c.px(x, y, 'E'); else c.px(x, y, 'e').px(x, y + 1, 'k');
  }
  for (let i = 0; i < 6; i++) { let x = 10 + H32(i, 4, 9) * 176, y = 70 + H32(i, 5, 9) * 20; const P = [[x, y]]; for (let j = 0; j < 4; j++) { x += 2 + H32(i, j, 10) * 3; y += (H32(i, j, 11) - 0.5) * 3; P.push([x, y]); } c.limb(P, 0.5, 0.5, 'd'); }
  // the dirt track, from the front to the horizon: pale dust, two ruts, its own pebbles
  for (let y = HZ; y < SH; y++) {
    const t = (y - HZ) / (SH - 1 - HZ), cx = 98 + t * 6, hw = 2 + t * 28;
    for (let x = Math.floor(cx - hw); x <= cx + hw; x++) c.px(x, y, Math.abs(x - cx) > hw - 1.5 && dith(x, y, 0.5) ? 'E' : 'T');
    for (const s of [-1, 1]) c.rect(Math.round(cx + s * hw * 0.45), y, Math.max(1, Math.round(t * 2.5)), 1, 't');
    for (let x = Math.floor(cx - hw); x <= cx + hw; x++) if (H32(x, y, 12) < 0.04) c.px(x, y, H32(x, y, 13) < 0.5 ? 'Y' : 't');
  }
  // the gums and the spinifex
  gum(c, 150, 55, 13, 40); gum(c, 166, 56, 15, 50); gum(c, 66, 61, 28, 20); gum(c, 186, 72, 40, 30); gum(c, 24, 76, 46, 10);
  for (const [x, y, w, s] of [[12, 64, 4, 1], [42, 60, 3, 2], [58, 72, 5, 3], [146, 62, 4, 4], [134, 92, 5, 5], [80, 64, 3, 6], [124, 60, 3, 7], [6, 80, 4, 8], [176, 88, 4, 9]]) spinifex(c, x, y, w, s);
  // the station gate over the track, its name board in the repo colour
  c.rect(88, 34, 2, 27, 'W').rect(111, 34, 2, 27, 'W').rect(88, 34, 1, 27, 'w').rect(111, 34, 1, 27, 'w').rect(85, 35, 31, 2, 'w').rect(85, 36, 31, 1, 'W');
  c.rect(91, 28, 19, 7, 'F').rect(91, 34, 19, 1, 'f').rect(91, 28, 19, 1, 'Q');
  for (let x = 93; x < 108; x++) if (x % 5 !== 2) c.px(x, 31, 'Q');
  // the billabong at the front: still water, its deep middle, ripples and the sky in it, reeds and rocks at the edge
  c.ell(36, 91, 33, 8, 'B').ell(26, 87.5, 22, 4.5, 'B').ell(40, 93, 26, 4.5, 'b');
  for (const [x, y, w] of [[14, 87, 6], [30, 90, 8], [46, 88, 5], [22, 92, 5]]) c.rect(x, y, w, 1, 'R');
  for (const [x, y] of [[18, 86], [38, 87], [52, 91], [10, 90]]) c.rect(x, y, 2, 1, 'r');
  for (const [x, h] of [[4, 9], [6, 12], [8, 8], [64, 10], [66, 13], [68, 9]]) c.limb([[x, 88], [x + 0.5, 88 - h]], 0.5, 0.5, 'n').ell(x + 0.5, 87 - h, 0.6, 1.4, 'c');
  for (const [x, y, r] of [[60, 86, 2.5], [2, 86, 2], [70, 92, 3]]) c.ell(x, y, r, r * 0.7, 'K').rect(x - r + 1, y + r * 0.4, r * 2 - 1, 1, 'j');
  // the hollow log by the track (the rail's log, in the scene)
  c.rect(130, 80, 22, 7, 'O').rect(130, 80, 22, 1, 'a').rect(130, 86, 22, 1, 'U').limb([[134, 83], [146, 83]], 0.5, 0.5, 'U');
  c.ell(130, 83.5, 2.6, 3.6, 'U').ell(130, 83.5, 1.6, 2.4, 'A');
  return c;
}
// The sign: a small board on two posts at the clearing's edge, about a kangaroo's height (28 × 38, 56 × 76 units):
// a header strip, the half dial in the four AFDRS bands. The needle is a layer per level.
const SX = 162, SY = 42;
function signBoard() {
  const c = new Cv(SW, SH);
  c.rect(SX + 1, SY + 38, 27, 1, 'q');
  for (const x of [SX + 5, SX + 21]) c.rect(x, SY + 19, 2, 19, 'P').rect(x, SY + 19, 1, 19, 'p');
  c.rect(SX, SY, 28, 20, 'g').rect(SX + 1, SY + 4, 26, 15, 'W');
  for (let i = 0; i < 24; i++) if ((i + 1) % 6) c.px(SX + 2 + i, SY + 1.5, 'w');
  for (let i = 0; i < 14; i++) if ((i + 1) % 5) c.px(SX + 7 + i, SY + 2.5, 'w');
  c.area(SX + 1, SY + 5, SX + 27, SY + 18, (x, y) => { const d = Math.hypot(x - (SX + 14), (SY + 18) - y); return y < SY + 18 && d >= 5.5 && d <= 12; },
    (x, y) => String(1 + Math.min(3, Math.floor((180 - Math.atan2(SY + 18 - (y + 0.5), x + 0.5 - (SX + 14)) * 180 / Math.PI) / 45))));
  return c;
}
function needleLayer(deg) {
  const c = new Cv(SW, SH), a = deg * Math.PI / 180;
  c.limb([[SX + 14, SY + 17.5], [SX + 14 + Math.cos(a) * 10, SY + 17.5 - Math.sin(a) * 10]], 0.8, 0.5, 'k').rect(SX + 13, SY + 16, 2, 2, 'k');
  return paint(c, FDR);
}
// The static layers, once per page as <symbol>s (`sceneDefs`); each clearing <use>s them inside its class groups, so
// day/night, the levels and `.fdr` stay plain CSS and a page of many clearings stays light.
export function sceneDefs() {
  let s = '';
  const sym = (id, body) => `<symbol id="${id}" viewBox="0 0 ${SW * 2} ${SH * 2}" shape-rendering="crispEdges">${body}</symbol>`;
  const day = skyBands(new Cv(SW, SH), 6);
  day.ell(24, 12, 5, 5, 'S').ell(24, 12, 3.6, 3.6, 's');
  for (const [x, y, w] of [[120, 8, 7], [64, 16, 5], [170, 22, 4]]) day.ell(x, y, w, w * 0.4, 'c').ell(x - w * 0.5, y - 1, w * 0.5, w * 0.35, 'c').rect(x - w + 1, y + 1, w * 2 - 1, 1, 'C');
  s += sym('bs-day', paint(day, SKY_DAY));
  const night = skyBands(new Cv(SW, SH), 6);
  for (let i = 0; i < 120; i++) { const x = H32(i, 1, 20) * SW, y = H32(i, 2, 20) * (HZ - 6); night.px(x, y, H32(i, 3, 20) < 0.3 ? '*' : '+'); }
  for (let x = 0; x < SW; x++) for (let y = 0; y < HZ; y++) { const d = Math.abs(y - (40 - x * 0.18)); if (d < 6 && H32(x, y, 21) < 0.5 - d / 12) night.px(x, y, 'm'); }
  // the Southern Cross (Gacrux, Acrux, Mimosa, δ and the small ε) and the Pointers
  for (const [x, y, k] of [[140, 4, 2], [141, 22, 2], [133, 12, 2], [148, 11, 1], [144, 16, 0], [114, 20, 2], [123, 17, 1]]) {
    night.px(x, y, 'X'); if (k) for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) night.px(x + dx, y + dy, k > 1 ? '*' : '+');
    if (k > 1) for (const [dx, dy] of [[2, 0], [-2, 0], [0, 2], [0, -2]]) night.px(x + dx, y + dy, '+');
  }
  night.ell(30, 12, 3.5, 3.5, 'W').ell(31.5, 11, 3, 3, '0');
  s += sym('bs-night', paint(night, SKY_NIGHT));
  const g = ground();
  s += sym('bs-ground', paint(g, GROUND));
  // the night over everything but the sky
  const nm = new Cv(SW, SH);
  g.g.forEach((r, y) => r.forEach((ch, x) => { if (ch !== '.') nm.px(x, y, 'N'); }));
  s += sym('bs-dark', paint(nm, FX)) + sym('bs-sign', paint(signBoard(), FDR));
  for (const l of LEVELS) s += sym(`bs-${l}-1`, needleLayer(NEEDLE[l])) + sym(`bs-${l}-2`, needleLayer(NEEDLE[l] - 6));
  // the load: heat shimmer over the track and the horizon (high, catastrophic), cicadas rising, the haze
  const shimmer = k => { const c = new Cv(SW, SH); for (let i = 0; i < 16; i++) { const y = HZ - 4 + (i % 4) * 2, x = (i * 23 + k * 5) % 180; for (let j = 0; j < 9; j++) c.px(x + j, y + ((j + k) % 4 < 2 ? 0 : -1), 'h'); } return paint(c, FX); };
  s += sym('bs-up-1', shimmer(0)) + sym('bs-up-2', shimmer(1));
  const cic = (n, k) => { const c = new Cv(SW, SH); for (let i = 0; i < n; i++) { const x = 6 + H32(i, k, 30) * 184, y = 4 + H32(i, k, 31) * 34; c.rect(x, y, 2, 1, 'c').px(x - 1, y - 1, 'w').px(x + 2, y - 1, 'w'); } return paint(c, FX); };
  s += sym('bs-high-c1', cic(5, 0)) + sym('bs-high-c2', cic(5, 1));
  s += sym('bs-catastrophic-c1', cic(16, 0)) + sym('bs-catastrophic-c2', cic(16, 1));
  return s;
}
export function clearing() {
  const u = id => `<use href="#${id}"/>`, fr = id => `<g class="f1">${u(id + '1')}</g><g class="f2">${u(id + '2')}</g>`;
  let s = `<g class="sky-day">${u('bs-day')}</g><g class="sky-night">${u('bs-night')}</g>${u('bs-ground')}<g class="sky-night">${u('bs-dark')}</g>`;
  s += `<g class="fdr">${u('bs-sign')}${LEVELS.map(l => `<g class="s-${l}">${fr(`bs-${l}-`)}</g>`).join('')}</g>`;
  s += `<g class="s-up">${fr('bs-up-')}</g><g class="s-high">${fr('bs-high-c')}</g>`;
  s += `<g class="s-catastrophic">${fr('bs-catastrophic-c')}<rect width="392" height="${HZ * 2}" fill="#f0a050" opacity=".16"/></g>`;
  
  return `<svg viewBox="0 0 ${SW * 2} ${SH * 2}" preserveAspectRatio="xMidYMax slice" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// The sign in miniature for the Host card header (#370 `minis`, 48 × 16): the dial and its needle, no words.
export function mini(level, frame = 0) {
  let s = '<rect width="48" height="16" rx="1" fill="#1d4a2c"/><rect x="1" y="1" width="46" height="14" fill="#f4efe2"/>';
  s += dial(24, 15, 5, 13.4, 1) + needle(24, 15, 12, NEEDLE[level] - frame * 5, 1, 0, 0) + '<rect x="22" y="13" width="4" height="2" fill="#1b1b1b"/>';
  return `<svg class="hm" viewBox="0 0 48 16" shape-rendering="crispEdges" aria-hidden="true">${s}</svg>`;
}

// ---- #241 `icons`: a yellow roo-crossing sign (black kangaroo on a yellow diamond) on a post, sky over red earth ----
const ROO_SIGN = [
  '......................k..k....',
  '......................kk.kk...',
  '.......................kkkk...',
  '......................kkkkkk..',
  '......................kkkkkkkk',
  '.....................kkkkkkkkk',
  '....................kkkkk.....',
  '..................kkkkkk......',
  '...............kkkkkkkk.......',
  '............kkkkkkkkkkk.k.....',
  '..........kkkkkkkkkkkkk..k....',
  '.........kkkkkkkkkkkkkk...k...',
  '........kkkkkkkkkkkkkk........',
  '.......kkkkkkkkkkkkkk.........',
  '......kkkkkkkkkkkkkkk.........',
  '.....kkk.kkkkkkkkkkk..........',
  '....kkk...kkkkkkkkkk..........',
  '...kkk.....kkkkkkkkk..........',
  '..kkk.......kkkkkkkk..........',
  '.kkk.........kkkkkk...........',
  'kkk...........kkkkkkk.........',
  'kk.............kkkkkkkkkk.....',
  '.................kkkkkkkkkkk..',
];
function diamond(N, cx, cy, R, roo, rx, ry, bw = 1.6) {
  const g = Array.from({ length: N }, () => Array(N).fill('.'));
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    const d = Math.abs(x + 0.5 - cx) + Math.abs(y + 0.5 - cy);
    if (d <= R) g[y][x] = d > R - bw ? 'k' : 'Y';
  }
  roo.forEach((row, j) => [...row].forEach((c, i) => { if (c === 'k' && g[ry + j]?.[rx + i] === 'Y') g[ry + j][rx + i] = 'k'; }));
  return g.map(r => r.join(''));
}
const SIGNINK = { Y: '#f6c512', H: '#f6c512', k: '#111111' };
const px = (map, u, ink) => map.map((row, j) => [...row].map((c, i) => ink[c] ? `<rect x="${i * u}" y="${j * u}" width="${u}" height="${u}" fill="${ink[c]}"/>` : '').join('')).join('');
export function icon(size = 512) {
  const d = diamond(64, 32, 30, 29, ROO_SIGN, 17, 19, 3);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 512 512" shape-rendering="crispEdges">` +
    `<defs><linearGradient id="isky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#9fcde0"/><stop offset="1" stop-color="#f1e2c0"/></linearGradient></defs>` +
    `<rect width="512" height="512" fill="url(#isky)"/><rect y="432" width="512" height="80" fill="#b5562e"/><rect y="432" width="512" height="16" fill="#c9703f"/>` +
    `<rect x="240" y="472" width="32" height="40" fill="#6b6f73"/><rect x="240" y="472" width="10" height="40" fill="#8d9196"/>${px(d, 8, SIGNINK)}</svg>`;
}
// The favicon (16 px): the diamond alone, a simpler roo, readable at 16.
const ROO_FAV = ['......k.k', '......kkk', '....kkkk.', '..kkkkkk.', 'kkkkkkk..', 'k..kkk...', '....kkkk.'];
export function favicon(size = 32) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 16 16" shape-rendering="crispEdges"><rect width="16" height="13" fill="#9fcde0"/><rect y="13" width="16" height="3" fill="#b5562e"/>${px(diamond(16, 8, 8, 8, ROO_FAV, 3, 5), 1, SIGNINK)}</svg>`;
}

// ---- ascii (#335): the Lanes cells (≤ 3 lines of 8, the farm's), the row figures (3 characters), the Host load (≤ 5),
// the loading frames. The roo, the wombat, the koala, the emu, the kookaburra, by role and state. ----
export const ASCII = {
  impl: '   //_\n~__( o>\n //  \\\\', fix: "'. ,\n(___)o^\n '' ''", rev: '=====||\n (oYo)||\n      ||',
  update: '    _o>\n (###)/\n  / >', idle: ' \\ z /\n (-Y-)\n   \\|/', done: 'ha! ha!\n  (o<=\n  |##|', blocked: "  _..._\n / () \\\n/_/''\\_\\",
};
export const ASCII_ROWS = { impl: '~R^', fix: '.W:', rev: '=K|', update: 'E>>', idle: 'zK.', done: 'ha!', blocked: '[W]' };
export const ASCII_LOAD = { calm: '(\\  )', busy: '( | )', held: '(  /)' };
export const ASCII_LOADING = '~R^\n_R_';
