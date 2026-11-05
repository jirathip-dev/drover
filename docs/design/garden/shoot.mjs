// #423: regenerates the mockup's PNGs: index (390 at DPR 2, 1440) and the sprite sheet, each in light and dark, plus the
// icon PNGs. No dependencies: Chrome over --remote-debugging-pipe (../theme-370/cdp.mjs).
// `node shoot.mjs <chrome-headless-shell>` from this folder; the server roots at the repo so serve.css resolves.
import { writeFileSync, mkdirSync } from 'node:fs';
import { serve, chrome } from '../theme-370/cdp.mjs';
import { icon, favicon } from './art.mjs';

const bin = process.argv[2] || 'chrome-headless-shell', only = process.argv[3];
const root = new URL('../../../', import.meta.url).pathname, here = 'docs/design/garden';
mkdirSync(new URL('icon/', import.meta.url), { recursive: true });
for (const n of [180, 192, 512]) writeFileSync(new URL(`icon/icon-${n}.svg`, import.meta.url), icon(n));
writeFileSync(new URL('icon/favicon.svg', import.meta.url), favicon(32));
writeFileSync(new URL('icon/favicon-16.svg', import.meta.url), favicon(16));
const srv = await serve(root), c = chrome(bin);
const shots = [
  ...[180, 192, 512].map(n => [`icon/icon-${n}.svg`, `icon/icon-${n}.png`, n, n, 1]),
  ['icon/favicon.svg', 'icon/favicon-32.png', 32, 32, 1], ['icon/favicon-16.svg', 'icon/favicon-16.png', 16, 16, 1],
  ...['light', 'dark'].flatMap(s => [
    [`sheet.html?s=${s}&still=1`, `sprite-sheet-${s}.png`, 1240, 900, 1, 'full'],
    [`index.html?s=${s}&still=1`, `garden-${s}-390.png`, 390, 844, 2, 'full'],
    [`index.html?s=${s}&still=1`, `garden-${s}-1440.png`, 1440, 900, 1, 'full'],
  ]),
].filter(s => !only || s[1].includes(only));
try {
  for (const [page, out, W, H, dpr, how] of shots) {
    const p = await c.open(`${srv.base}/${here}/${page}`, W, H, dpr, { reduce: /still/.test(page), ready: page.endsWith('.svg') ? 'document.readyState === "complete"' : 'document.body.dataset.ready === "1"' });
    if (how === 'zoom') await p.cmd('Emulation.setDeviceMetricsOverride', { width: 16, height: 16, deviceScaleFactor: 0.5, mobile: false });
    writeFileSync(new URL(out, import.meta.url), how === 'full' ? await p.full() : await p.png());
    await p.done();
    console.log('wrote', out);
  }
} finally { c.kill(); srv.close(); }
if (c.errors.length) { console.error(c.errors.join('\n')); process.exit(1); }
