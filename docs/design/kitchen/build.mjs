// #239: writes the kitchen theme's files from art.mjs into ./theme/ (phase 2 copies them to assets/themes/kitchen/),
// and the icon SVGs that shoot.sh turns into PNGs. `node build.mjs`
import { writeFileSync, mkdirSync } from 'node:fs';
import { ACTIONS, TICKET, ROW, LOOKS, SCHEME, draw, station, icon, favicon } from './art.mjs';

const out = new URL('./theme/', import.meta.url);
mkdirSync(out, { recursive: true });
const g = (id, body, box = '0 0 132 100') => `<symbol id="${id}" viewBox="${box}" shape-rendering="crispEdges">${body}</symbol>`;
let sprites = '<!-- herdsman #239 kitchen: pixel chefs, 132 x 100 (one pixel = 4), from docs/design/kitchen/art.mjs. Looks are CSS vars: --sk/--ss skin, --hr hair, --ap/--as apron, --hc/--hs hat, --ht/--hb/--hk which hat, --fk the project trim; --f1/--f2 flame; f1/f2 the two frames of a loop. -->\n';
for (const [k, f] of Object.entries(ACTIONS)) sprites += g(`chef-${k}`, draw(f())) + '\n';
sprites += g('chef-ticket', draw(TICKET())) + '\n';
for (const [k, m] of Object.entries(ROW)) sprites += g(`row-${k}`, draw(m, 1), '0 0 16 16') + '\n';
writeFileSync(new URL('sprites.svg', out), sprites);
writeFileSync(new URL('scene.svg', out), station().replace('<svg ', '<svg xmlns="http://www.w3.org/2000/svg" '));

// theme.json, filled to the #241 contract (key names follow #241's list; the schema is #241's to fix).
const theme = {
  actions: { impl: 'chef-cook', fix: 'chef-chop', rev: 'chef-taste', update: 'chef-carry', idle: 'chef-wait', done: 'chef-serve', blocked: 'chef-bell' },
  poses: { working: 'chef-cook', idle: 'chef-wait', done: 'chef-serve', blocked: 'chef-bell' },
  looks: Object.fromEntries(LOOKS.map(l => [l.coat, l.vars])),
  load_scene: { kind: 'heat', levels: { calm: { below: 0.5 }, busy: { from: 0.5 }, held: { gate: true, idle: 'chef-fan' } } },
  rail: { item: 'chef-ticket', needs: 'needs-t' },
  moments: { merged: 'order-up', fail: 'sent-back', 'needs-you': 'needs-t' },
  row_sprites: { impl: 'row-cook', fix: 'row-chop', rev: 'row-taste', update: 'row-carry', idle: 'row-wait', done: 'row-serve', blocked: 'row-bell', queued: 'row-ticket', fail: 'row-back' },
  icons: { 180: 'icon-180.png', 192: 'icon-192.png', 512: 'icon-512.png', favicon: 'favicon.png' },
  ascii: {
    impl: ['♨', 'cook'], fix: ['♨', 'chop'], rev: ['♨', 'taste'], update: ['♨', 'carry'],
    idle: ['·', 'wait'], done: ['✓', 'serve'], blocked: ['!', 'bell'], dead: ['✗', 'out'],
  },
  modes: ['scene', 'ascii'],
  board: Object.fromEntries(Object.entries(SCHEME).filter(([k]) => k.startsWith('--'))),
  glyphs: { working: '♨', idle: '·', unknown: '?', parked: '·', done: '✓', blocked: '!', dead: '✗' },
  words: { horse: 'chef', horses: 'chefs', herd: 'kitchen line', ranch: 'kitchen', barn: 'kitchen' },
  scene: { weather: false, palette: { '--pa': 'repeating-conic-gradient(#4a3f38 0 25%,#41372f 0 50%) 0 0/24px 24px' } },
};
writeFileSync(new URL('theme.json', out), JSON.stringify(theme, null, 1) + '\n');

mkdirSync(new URL('./icon/', import.meta.url), { recursive: true });
for (const n of [180, 192, 512]) writeFileSync(new URL(`icon/icon-${n}.svg`, import.meta.url), icon(n));
writeFileSync(new URL('icon/favicon.svg', import.meta.url), favicon(32));
console.log('wrote theme/{sprites.svg,scene.svg,theme.json} and icon/*.svg');
