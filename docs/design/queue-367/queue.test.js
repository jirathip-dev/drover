// #367: the Queue tab's pure part (queue.js up to `// queue:end`) under node: the group order, the chain summary and the
// time format. Phase 2 moves these into crates/server/tests, run the way chat_blocks.rs runs serve.js's chat part.
const { readFileSync } = require('node:fs');
const src = readFileSync(__dirname + '/queue.js', 'utf8');
const { qdur, qwhen, qgroups, qline } = new Function(src.slice(0, src.indexOf('// queue:end')) + '\nreturn { qdur, qwhen, qgroups, qline };')();
let fails = 0;
const eq = (what, got, want) => {
  const ok = JSON.stringify(got) === JSON.stringify(want);
  if (!ok) fails++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${ok ? '' : `: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`}`);
};

// time: one unit style, two units at most, the second never 0
eq('time 45 s', qdur(45), '45s');
eq('time 6420 s', qdur(6420), '1h 47m');
eq('time 7200 s', qdur(7200), '2h');
eq('time 36300 s', qdur(36300), '10h 5m');
eq('time 3 d 4 h', qdur(3 * 86400 + 4 * 3600 + 59), '3d 4h');
eq('time 2 d', qdur(2 * 86400 + 59 * 60), '2d');
eq('phase: impl', qwhen('impl', 6420), 'in impl 1h 47m');
eq('phase: review(1)', qwhen('review(1)', 300), 'in review(1) 5m');
eq('phase: ready', qwhen('ready', 2100), 'ready 35m');
eq('phase: queued has none', qwhen('queued', 99), '');

// groups: needs you, in progress, queued; a chain is one slot where its most urgent step is
const rows = [
  { n: 331, st: '⏸ waiting: decision', b: null, t: 'Drive in parallel', s: 1080 },
  { n: 360, st: 'ready', b: null, t: 'Reviewer brief', s: 87 },
  { n: 265, st: 'ready', b: null, t: 'Decouple 2/7: JSON API', s: 2100 },
  { n: 359, st: 'impl', b: null, t: 'Lane start race', s: 540 },
  { n: 351, st: 'impl', b: null, t: 'Board streams changes', s: 540 },
  ...[266, 267, 268, 269, 270].map((n, i) => ({ n, st: 'queued', b: n - 1, t: `Decouple ${i + 3}/7: step`, s: null })),
  { n: 344, st: 'queued', b: null, t: 'Phase B: SQLite', s: null },
  { n: 345, st: 'queued', b: 344, t: 'Phase C: service', s: null },
  { n: 356, st: 'queued', b: 344, t: 'Phase B: migrate', s: null },
  { n: 358, st: 'queued', b: 345, t: 'Phase C: deltas', s: null },
  { n: 365, st: 'queued', b: 351, t: 'Backlog', s: null },
  { n: 371, st: 'queued', b: null, t: 'Host sampling', s: null },
  { n: 372, st: 'queued', b: 999, t: 'blocked by a merged or other-repo issue', s: null },
];
const name = s => s.chain ? `chain #${s.chain.root.n}` : `#${s.row.n}`;
const g = qgroups(rows);
eq('needs you', g[0].map(name), ['#331']);
eq('in progress: server order, a chain at its head', g[1].map(name), ['#360', 'chain #265', '#359', 'chain #351']);
eq('queued: queue order', g[2].map(name), ['chain #344', '#371', '#372']);
const dc = g[1][1].chain;
eq('chain summary', qline(dc), '#265 → #270 · Decouple (6 steps) · next: #266');
eq('chain steps all at level 1 (no staircase)', dc.steps.map(s => s.lv), [1, 1, 1, 1, 1, 1]);
const ph = g[2][0].chain;
eq('branching chain: main line first, the side branch at level 2', ph.steps.map(s => `${s.row.n}:${s.lv}`), ['344:1', '356:2', '345:1', '358:1']);
eq('branching chain summary (no shared series: the first title)', qline(ph), '#344 → #358 · Phase B: SQLite (4 steps) · next: #344');
eq('a running head with one queued step', qline(g[1][3].chain), '#351 → #365 · Board streams changes (2 steps) · next: #365');
eq('a blocker loop draws no chain', qgroups([{ n: 1, st: 'queued', b: 2, t: 'a' }, { n: 2, st: 'queued', b: 1, t: 'b' }]).flat().map(name), ['#1', '#2']);
process.exitCode = fails ? 1 : 0;
console.log(fails ? `${fails} failed` : 'all passed');
