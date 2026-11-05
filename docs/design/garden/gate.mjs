// #423: serve.js's scheme gate (assets/serve.js `gate`), browser- and node-safe, so the mockup's table and check.mjs
// share it. Contrast and CIEDE2000 are ../schemes/check.mjs's, unchanged.
export const TOKENS = ['--bg', '--card', '--edge', '--line', '--scrim', '--fg', '--mute', '--l', '--link',
  '--g', '--a', '--r', '--p', '--c', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a'];
const lin = c => (c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
export const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
// `color-mix(in srgb, c p%, transparent)` painted over bg, as the board's tints are (.ask, .later, .grp[data-t]).
const over = (c, bg, p) => c.slice(0, 3).map((v, i) => v * p + bg[i] * (1 - p));

function lab([r, g, b]) {
  const [x, y, z] = [[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]]
    .map(row => row[0] * lin(r) + row[1] * lin(g) + row[2] * lin(b));
  const f = t => t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116;
  const [fx, fy, fz] = [x / 0.95047, y, z / 1.08883].map(f);
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}
// CIEDE2000 (Sharma et al. 2005).
export function de(c1, c2) {
  const [L1, a1, b1] = lab(c1), [L2, a2, b2] = lab(c2), rad = Math.PI / 180;
  const C = (Math.hypot(a1, b1) + Math.hypot(a2, b2)) / 2, G = 0.5 * (1 - Math.sqrt(C ** 7 / (C ** 7 + 25 ** 7)));
  const ap1 = a1 * (1 + G), ap2 = a2 * (1 + G), C1 = Math.hypot(ap1, b1), C2 = Math.hypot(ap2, b2);
  const h = (b, a) => { const v = Math.atan2(b, a) / rad; return v < 0 ? v + 360 : v; };
  const h1 = h(b1, ap1), h2 = h(b2, ap2), dL = L2 - L1, dC = C2 - C1;
  let dh = C1 * C2 === 0 ? 0 : h2 - h1 > 180 ? h2 - h1 - 360 : h2 - h1 < -180 ? h2 - h1 + 360 : h2 - h1;
  const dH = 2 * Math.sqrt(C1 * C2) * Math.sin(dh * rad / 2), Lm = (L1 + L2) / 2, Cm = (C1 + C2) / 2;
  const hm = C1 * C2 === 0 ? h1 + h2 : Math.abs(h1 - h2) > 180 ? (h1 + h2 + 360) / 2 : (h1 + h2) / 2;
  const T = 1 - 0.17 * Math.cos((hm - 30) * rad) + 0.24 * Math.cos(2 * hm * rad) + 0.32 * Math.cos((3 * hm + 6) * rad) - 0.2 * Math.cos((4 * hm - 63) * rad);
  const SL = 1 + 0.015 * (Lm - 50) ** 2 / Math.sqrt(20 + (Lm - 50) ** 2), SC = 1 + 0.045 * Cm, SH = 1 + 0.015 * Cm * T;
  const RT = -2 * Math.sqrt(Cm ** 7 / (Cm ** 7 + 25 ** 7)) * Math.sin(60 * Math.exp(-(((hm - 275) / 25) ** 2)) * rad);
  return Math.sqrt((dL / SL) ** 2 + (dC / SC) ** 2 + (dH / SH) ** 2 + RT * (dC / SC) * (dH / SH));
}

const hex = s => { const n = parseInt(s.slice(1, 7), 16); return [n >> 16, (n >> 8) & 255, n & 255]; };
export function gate(s) {
  const c = Object.fromEntries(TOKENS.map(k => [k, hex(s.tokens[k])])), rows = [];
  const on = (ks, bg, min, name = bg) => ks.forEach(k => rows.push([`${k} on ${name}`, ratio(c[k], bg.map ? bg : c[bg]), min]));
  const card = over(c['--r'], c['--bg'], 0.15), red = over(c['--r'], c['--bg'], 0.12), amber = over(c['--a'], c['--bg'], 0.12);
  for (const bg of ['--bg', '--card']) {
    on(['--fg', '--mute', '--l', '--link', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a'], bg, 4.5);
    on(['--g', '--a', '--r'], bg, 3);
  }
  on(['--bg'], '--l', 4.5, '--l (pressed control, active tab)');
  on(['--fg', '--mute', '--l'], card, 4.5, 'the needs-you card');
  on(['--r'], card, 3, 'the needs-you card');
  on(['--r'], red, 3, 'a red group header');
  on(['--a'], amber, 3, 'an amber group header');
  for (const [x, y] of [['--g', '--a'], ['--a', '--r'], ['--g', '--r'], ...['--l', '--link'].flatMap(x => ['--g', '--a', '--r'].map(y => [x, y]))])
    rows.push([`ΔE2000 ${x} ↔ ${y}`, de(c[x], c[y]), 20]);
  return rows;
}
