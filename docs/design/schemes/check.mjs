// #244 phase 1: the scheme gate the board will run on load (phase 2 ports it to src/), run here over the mockup's
// token sets. No deps. Usage: node check.mjs [schemes dir] [--json]; exit 1 if any scheme fails.
// A scheme is { "label", "color-scheme": "dark"|"light", "tokens": { "--bg": "#rrggbb", … } }.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

// Every colour token the board uses, defined once per scheme (README "The token set").
export const TOKENS = ['--bg', '--card', '--edge', '--line', '--scrim', '--fg', '--mute', '--l', '--link',
  '--g', '--a', '--r', '--p', '--c', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a'];
export const TEXT = 4.5, STATUS = 3, DE_STATUS = 20, DE_ACCENT = 10;

const hex = s => {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(s);
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [n >> 16, (n >> 8) & 255, n & 255, m[2] ? parseInt(m[2], 16) / 255 : 1];
};
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

// One scheme → { ok, fails: [reason], rows: [[check, value, min, ok]] }. A failing scheme is refused with its reasons.
export function check(s) {
  const t = s.tokens || {}, fails = [], rows = [];
  const missing = TOKENS.filter(k => !hex(t[k] || ''));
  if (missing.length) return { ok: false, fails: [`missing or not #rrggbb[aa]: ${missing.join(' ')}`], rows };
  if (!['dark', 'light'].includes(s['color-scheme'])) fails.push('color-scheme is not "dark" or "light"');
  const c = Object.fromEntries(TOKENS.map(k => [k, hex(t[k])]));
  const gate = (name, v, min) => { rows.push([name, v, min, v >= min]); if (v < min) fails.push(`${name} ${v.toFixed(2)} < ${min}`); };
  for (const bg of ['--bg', '--card'])
    for (const k of ['--fg', '--mute', '--l', '--link', '--fk-l', '--fk-p', '--fk-c', '--fk-g', '--fk-a']) gate(`${k} on ${bg}`, ratio(c[k], c[bg]), TEXT);
  for (const bg of ['--bg', '--card']) for (const k of ['--g', '--a', '--r']) gate(`${k} on ${bg}`, ratio(c[k], c[bg]), STATUS);
  gate('--bg on --l (pressed control, active tab)', ratio(c['--bg'], c['--l']), TEXT);
  gate('--fg on the needs-you tint (--r 15 % over --bg)', ratio(c['--fg'], over(c['--r'], c['--bg'], 0.15)), TEXT);
  gate('--r on the needs-you tint', ratio(c['--r'], over(c['--r'], c['--bg'], 0.15)), STATUS);
  for (const [x, y] of [['--g', '--a'], ['--a', '--r'], ['--g', '--r']]) gate(`ΔE2000 ${x} ↔ ${y}`, de(c[x], c[y]), DE_STATUS);
  for (const x of ['--l', '--link']) for (const y of ['--g', '--a', '--r']) gate(`ΔE2000 ${x} ↔ ${y}`, de(c[x], c[y]), DE_ACCENT);
  return { ok: !fails.length, fails, rows };
}

export const load = dir => readdirSync(dir).filter(f => f.endsWith('.json')).sort()
  .map(f => ({ name: f.slice(0, -5), ...JSON.parse(readFileSync(join(dir, f), 'utf8')) }));

if (import.meta.url === `file://${process.argv[1]}`) {
  const dir = process.argv[2] && !process.argv[2].startsWith('--') ? process.argv[2] : new URL('schemes', import.meta.url).pathname;
  const out = load(dir).map(s => ({ name: s.name, ...check(s) }));
  if (process.argv.includes('--json')) console.log(JSON.stringify(out, null, 1));
  else for (const r of out) {
    const low = r.rows.filter(x => x[3]).sort((p, q) => p[1] / p[2] - q[1] / q[2])[0];
    console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.name.padEnd(14)} ${r.ok ? `tightest: ${low[0]} ${low[1].toFixed(2)} (min ${low[2]})` : r.fails.join('; ')}`);
  }
  process.exit(out.every(r => r.ok) ? 0 : 1);
}
