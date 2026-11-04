// #370: writes the proposed theme files into `proposed/<theme>/` (theme.json, sprites.svg), from today's
// assets/themes/<theme>/ plus this design: theme words for every tab, a two-frame loop on every row sprite, the event
// row sprites and the Host load minis. Phase 2 copies them to assets/themes/. `node docs/design/theme-370/build.mjs`
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { FARM_IDS, KITCHEN_IDS, farmMini, farmRow, kitchenMini, kitchenRow } from './frames.mjs';

const here = new URL('.', import.meta.url), assets = new URL('../../../assets/themes/', import.meta.url);
const read = (t, f) => readFileSync(new URL(`${t}/${f}`, assets), 'utf8');
const LEVELS = ['calm', 'busy', 'held'];

// Each theme's words, by key: the board shows `<theme word> · <plain word>` (proposal.js PLAIN), the plain part muted.
// A key a theme lacks shows the plain word alone; terminal has none, so its board is today's.
export const WORDS = {
  farm: {
    needs: 'at the gate', working: 'on the trail', idle: 'grazing', done: 'in the stable', queued: 'in the paddock',
    parked: 'turned out', dead: 'lame', 'blocked/dead': 'at the gate',
    impl: 'plough', fix: 'farrier', rev: 'vet', update: 'haul',
    merged: 'over the fence', fail: 'refused',
    'tab-queue': 'the paddock', 'tab-lanes': 'the herd', 'tab-host': 'barn', 'tab-events': 'logbook',
    'empty-queue': 'the paddock is empty', 'empty-lanes': 'no horses out', 'empty-events': "the logbook's blank",
    loading: 'saddling up',
  },
  kitchen: {
    needs: 'the bell', parked: 'off the clock', dead: "86'd",
    merged: 'order up', fail: 'sent back',
    'tab-queue': 'tickets', 'tab-lanes': 'the line', 'tab-host': 'the kitchen', 'tab-events': 'the rail',
    'empty-queue': "the kitchen's quiet: no tickets", 'empty-lanes': 'nobody on the line', 'empty-events': "the rail's clear",
    loading: 'firing up',
  },
};
// The Queue's and Events' own row sprites, beside the seven actions: a queued issue, a needs-you one, a merge, a FAIL.
const EVENT_ROWS = {
  farm: { queued: 'row-bale', needs: 'row-bale-red', merged: 'row-jump', fail: 'row-refuse' },
  kitchen: { queued: 'row-ticket', needs: 'row-bell', merged: 'row-serve', fail: 'row-back' },
};

for (const t of ['farm', 'kitchen']) {
  const dir = new URL(`proposed/${t}/`, here);
  mkdirSync(dir, { recursive: true });
  const j = JSON.parse(read(t, 'theme.json'));
  j.words = { ...j.words, ...WORDS[t] };
  j.row_sprites = { ...j.row_sprites, ...EVENT_ROWS[t] };
  j.load_scene = { ...j.load_scene, minis: Object.fromEntries(LEVELS.map(l => [l, `load-${l}`])) };
  writeFileSync(new URL('theme.json', dir), JSON.stringify(j, null, 1) + '\n');

  const rows = t === 'farm' ? Object.fromEntries(FARM_IDS.map(id => [id, farmRow(id)])) : Object.fromEntries(Object.keys(KITCHEN_IDS).map(k => [KITCHEN_IDS[k], kitchenRow(k)]));
  const lines = read(t, 'sprites.svg').trimEnd().split('\n').map(l => {
    const id = l.match(/^<g id="([^"]+)"/)?.[1];
    if (id && rows[id]) { const r = rows[id]; delete rows[id]; return r; }
    return l;
  });
  // the rows today's sheet lacks (farm: bale, red bale, jump, refuse), then the load minis
  lines.push(...Object.values(rows), ...LEVELS.map(t === 'farm' ? farmMini : kitchenMini));
  lines[0] = lines[0].replace(/-->$/, ` #370: row sprites loop .kf1/.kf2 (frame 2 from docs/design/theme-370/frames.mjs); load-calm|busy|held are 48 × 16. -->`);
  writeFileSync(new URL('sprites.svg', dir), lines.join('\n') + '\n');
}
console.log('wrote docs/design/theme-370/proposed/{farm,kitchen}/{theme.json,sprites.svg}');
