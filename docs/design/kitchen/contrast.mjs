// #239 / #244 Q2: WCAG 2 contrast of each board colour on its backgrounds, today's board vs the kitchen scheme, and
// how far apart the colours that mean different things are (ΔE76 in Lab). `node contrast.mjs`
import { SCHEME } from './art.mjs';

const lin = c => { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const rgb = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16));
const lum = h => { const [r, g, b] = rgb(h).map(lin); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
export const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
const lab = h => {
  const [r, g, b] = rgb(h).map(lin);
  const f = t => t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116;
  const [X, Y, Z] = [(0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047, 0.2126 * r + 0.7152 * g + 0.0722 * b, (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883].map(f);
  return [116 * Y - 16, 500 * (X - Y), 200 * (Y - Z)];
};
const de = (a, b) => Math.hypot(...lab(a).map((v, i) => v - lab(b)[i]));

const TODAY = { '--bg': '#0d1117', '--fg': '#c9d1d9', '--mute': '#8b949e', '--g': '#3fb950', '--a': '#d29922', '--r': '#f85149', '--l': '#58a6ff', '--p': '#bc8cff', '--c': '#39c5cf', card: '#161b22', ask: '#2a1416' };
for (const [name, t] of [['today', TODAY], ['kitchen', SCHEME]]) {
  console.log(`\n${name}`);
  for (const bg of ['--bg', 'card', 'ask']) {
    const row = ['--fg', '--mute', '--l', '--p', '--c', '--g', '--a', '--r'].map(k => `${k.slice(2)} ${ratio(t[k], t[bg]).toFixed(1)}`);
    console.log(`  on ${bg.replace('--', '')} ${t[bg]}: ${row.join(' · ')}`);
  }
  const pairs = [['--r', '--l'], ['--r', '--a'], ['--a', '--l'], ['--g', '--c'], ['--a', '--p']];
  console.log('  ΔE ' + pairs.map(([a, b]) => `${a.slice(2)}/${b.slice(2)} ${de(t[a], t[b]).toFixed(0)}`).join(' · '));
}
