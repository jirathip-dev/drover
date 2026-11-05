// #462: the scheme gate serve.js runs on load, over the two proposed bush schemes. `node check.mjs [--json]`; exit 1 on a fail.
import { readFileSync } from 'node:fs';
import { gate } from './gate.mjs';

export const NAMES = ['bush-light', 'bush-dark'];
if (import.meta.url === `file://${process.argv[1]}`) {
  const out = NAMES.map(n => { const rows = gate(JSON.parse(readFileSync(new URL(`schemes/${n}.json`, import.meta.url)))); return { name: n, ok: rows.every(r => r[1] >= r[2]), rows }; });
  if (process.argv.includes('--json')) console.log(JSON.stringify(out, null, 1));
  else for (const r of out) {
    console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.name}`);
    for (const [w, v, m] of r.rows) if (v < m || v / m < 1.12) console.log(`  ${v < m ? '✗' : '·'} ${w} ${v.toFixed(2)} (min ${m})`);
  }
  process.exit(out.every(r => r.ok) ? 0 : 1);
}
