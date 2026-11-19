// #1354: write fixtures.js from the demo export: the phone's own screen JSON (queue, lanes, events, hosts, legend), plus
// the trees the demo has no state for (cards, an issue sheet, a verdict, backlog), built in the shape the builders in
// crates/server/src/screens.rs return. Usage (repo root):
//   DROVER_HOME=demo FLEET_CONFIG=demo/fleet.toml FLEET_STATE=demo/events.jsonl drover serve --export <dir> --demo
//   node docs/design/terminal-board/fixtures.mjs <dir>
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [dir] = process.argv.slice(2);
const read = s => JSON.parse(readFileSync(join(dir, 'screens', s, 'index.html'), 'utf8'));
const NOW = '2026-10-31T06:54:30Z';
const ago = m => new Date(Date.parse(NOW) - m * 60000).toISOString().replace('.000', '');
const ahead = m => new Date(Date.parse(NOW) + m * 60000).toISOString().replace('.000', '');

const queue = read('queue');
const walk = (b, f) => { if (b && typeof b === 'object') { f(b); for (const k of ['blocks', 'rows', 'fields', 'buttons', 'items']) (b[k] || []).forEach(c => walk(c, f)); } };
walk(queue, b => { if (b.id === 'q-acme-web-49') b.title = '#49 Settings: show the team name in full in every locale'; });
walk(queue, b => {
  if (b.id === 'done-acme-web') b.rows.push({ type: 'row', id: 'backlog-acme-web', title: 'backlog (23)', accessory: 'chevron', tap: { sheet: '/screens/backlog?repo=acme-web' } });
});

const hosts = read('hosts');
const list = hosts.blocks[0].blocks[0];
list.rows[0] = { ...list.rows[0], subtitle: { text: 'main host' }, value: '8 lanes' };
list.rows.push({ type: 'row', id: 'h-gpu-box', title: 'gpu-box', badge: { text: 'on', tone: 'ok' }, subtitle: { text: 'remote host' }, value: '2 lanes',
  swipe: [{ label: 'Off', tone: 'bad', action: 'host', values: { host: 'gpu-box', to: 'off' },
    confirm: { title: 'Turn gpu-box off?', message: 'No new lanes go to gpu-box; its 2 live lanes finish there.', ok: 'Turn off' } }] });
list.rows.push({ type: 'row', id: 'h-cloud-1', title: 'cloud-1', badge: { text: 'off', tone: 'warn' }, subtitle: { text: 'cloud host · no new lanes, 0 lanes finishing' }, value: '0 lanes',
  swipe: [{ label: 'On', action: 'host', values: { host: 'cloud-1', to: 'on' }, confirm: { title: 'Turn cloud-1 on?', message: 'New lanes may go to cloud-1 again.', ok: 'Turn on' } }] });
hosts.blocks[0].header = 'hosts (3)';
hosts.blocks.push({ type: 'section', id: 'proxy-sec', header: 'Token proxy', footer: 'The token shows no expiry: days left estimate saved + 365 d.',
  blocks: [{ type: 'list', id: 'proxy-list', rows: [{ type: 'row', id: 'px-0', title: 'token set 2026-04-02 · about 153 days left' }, { type: 'row', id: 'px-1', title: '2 keys live · gpu-box, cloud-1' }] }] });

const q33 = { type: 'choice', id: 'q33', action: 'answer', recommended: 'both: Retry-After on a 429 and X-RateLimit-* on every response',
  options: [{ value: 'both: Retry-After on a 429 and X-RateLimit-* on every response', label: 'both: Retry-After on a 429 and X-RateLimit-* on every response' },
    { value: 'Retry-After only, on a 429', label: 'Retry-After only, on a 429' }],
  default: { value: 'both: Retry-After on a 429 and X-RateLimit-* on every response', until: ahead(9), label: 'takes the recommendation in {rel}' },
  other: { placeholder: 'other…' } };
const askCard = { type: 'card', id: 'ask-q33', kind: 'ask', tone: 'warn', title: 'acme-api #21', meta: 'routine · 4m',
  fold: { line: '#21 · routine · Return 429 with `Retry-After` only, or also the `X-RateLimit-*` headers on every… · 4m',
    main: { block: 'q33', action: 'answer', value: q33.recommended, label: q33.recommended } },
  blocks: [{ type: 'text', markdown: '**Return 429 with `Retry-After` only, or also the `X-RateLimit-*` headers on every response?**' }, q33] };
const nhCard = { type: 'card', id: 'nhc-acme-api-22', kind: 'ask', tone: 'bad', title: 'acme-api #22 · OpenAPI spec served at /v1/openapi.json', meta: 'needs-human · 59m',
  fold: { line: '#22 · needs-human · review rounds ran out (4/3) · 59m', main: { block: 'nh-acme-api-22', action: 'needs-human', value: 'extend', label: 'one more round', values: { fleet: 'acme-api', issue: 22 } } },
  blocks: [{ type: 'text', markdown: '**review rounds ran out (4/3)**' }] };
const runCard = { type: 'card', id: 'run-r7', kind: 'run', title: 'free 14 GB: three merged acme-web lanes still hold their worktrees', blocks: [
  { type: 'run', id: 'r7', action: 'run-card', command: 'drover clean --yes acme-web', sha: '3f9a1c2e8b04', dir: '~/.local/share/drover', touches: 'acme-web: 3 worktrees of merged issues; no branch, no PR', until: ahead(13) }] };
const cards = (...blocks) => ({ type: 'screen', id: 'cards', blocks });

const sheet = (id, title, blocks, extra = {}) => ({ type: 'sheet', id, title, cancel: 'Done', blocks, ...extra });
const sec = (id, header, blocks, footer) => ({ type: 'section', id, header, blocks, ...(footer ? { footer } : {}) });
const rows = (id, r) => ({ type: 'list', id, rows: r });
const row = (id, title, more = {}) => ({ type: 'row', id, title, ...more });
const gh = 'https://github.com/acme-example/acme-api';
const issue = sheet('issue-acme-api-15', '#15 Cursor pagination for /v1/orders', [
  sec('status', 'status', [
    { type: 'chips', id: 'stage', name: 'stage', selected: ['review'], options: [{ value: 'impl', label: '◆ impl ✓' }, { value: 'review', label: '▲ review' }, { value: 'ready', label: '◆ ready' }, { value: 'merged', label: '◫ merged' }] },
    rows('status-list', [row('now', 'review 1', { value: { since: ago(70) }, subtitle: 'rev idle without a verdict (check the pane)' }),
      row('at', 'lane acme-api-rev-15', { subtitle: 'local' }),
      row('ci', 'CI', { badge: { text: '✓ 6/6', tone: 'ok' } }),
      row('v-acme-api-impl-15', 'acme-api-impl-15', { badge: { text: 'pushed', tone: 'done' }, value: 'impl', tap: { sheet: '/screens/verdict?repo=acme-api&issue=15&seq=31' } }),
      row('v-acme-api-rev-15', 'acme-api-rev-15', { subtitle: { text: 'no verdict', tone: 'warn' }, value: 'r1' })])]),
  sec('links', 'links', [rows('links-list', [row('issue', 'issue #15', { style: 'link', tap: { url: `${gh}/issues/15` } }),
    row('pr', 'PR #33', { style: 'link', subtitle: 'orders: cursor pagination (#15)', tap: { url: `${gh}/pull/33` } }),
    row('checks', 'checks', { style: 'link', tap: { url: `${gh}/pull/33/checks` } }),
    row('lane', 'lane acme-api-rev-15', { style: 'link', tap: { sheet: '/screens/lanes/acme-api-rev-15' } })])]),
  sec('history', 'history', [rows('history-list', [row('h-2', 'review 1 started', { value: { since: ago(70) } }), row('h-1', 'impl ✓ pushed 9c41e07', { value: { since: ago(78) } }), row('h-0', 'impl started', { value: { since: ago(116) } })])]),
]);

const pick = (n, t, l) => ({ type: 'toggle', name: `acme-web.pick.${n}`, label: `#${n} ${t}${l ? ' · ' + l : ''}`, value: false });
const backlog = sheet('backlog-acme-web', 'acme-web backlog · 23 open', [
  { type: 'chips', id: 'on', name: 'acme-web.on', multi: true, selected: ['ac'], action: 'filter',
    options: [['new', 'new'], ['30', '>30 days'], ['90', '>90 days'], ['ac', 'has AC'], ['pr', 'in a closed PR']].map(([value, label]) => ({ value, label })) },
  sec('issues', 'open, not queued', [{ type: 'form', id: 'bl-acme-web', fields: [
    { type: 'text', name: 'acme-web.q', label: 'Search', value: '', placeholder: 'search #, title, label' },
    pick(31, 'Settings page: keyboard order follows the layout', 'a11y'), pick(36, 'Settings: undo a pane reset', ''),
    pick(39, 'Settings page loads its panes lazily', 'perf'), pick(52, 'Settings search finds a pane by any of its labels', 'settings, search'),
    pick(53, 'Settings: a team admin sees who changed what', 'settings')] }],
  'read 4m ago · Drive now reads it again; ticked ones queue as needs-check')],
{ cancel: 'Cancel', done: { label: 'Queue ticked', action: 'queue' } });

const json = JSON.stringify({ NOW, queue, lanes: read('lanes'), events: read('events'), hosts, legend: read('legend'),
  cardsAsk: cards(askCard, nhCard), cardsRun: cards(runCard, askCard, nhCard), issue, backlog }, null, 1);
writeFileSync(new URL('fixtures.js', import.meta.url), `// generated by fixtures.mjs from the demo export (#1354); do not edit\nwindow.FIX = ${json};\n`);
console.log('ok fixtures.js');
