// #680: the design page at 1440 and 390 (DPR 2), whole page. Usage: node shoot.mjs <chrome-headless-shell>
import { writeFileSync } from 'node:fs';
import { chrome, serve } from './cdp.mjs';

const here = new URL('.', import.meta.url).pathname;
const c = chrome(process.argv[2]);
const s = await serve(here);
for (const [w, dpr] of [[1440, 1], [390, 2]]) {
  const p = await c.open(`${s.base}/index.html`, w, 900, dpr);
  writeFileSync(`${here}sheet-${w}.png`, await p.full());
  await p.done();
}
s.close(); c.kill();
if (c.errors.length) { console.error(c.errors); process.exit(1); }
