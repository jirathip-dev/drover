// #1352 design mockup: `drover chat` drawn as terminal character grids. One model, laid out at any width and height
// by the same rules the build follows (wrap, never cut; bottom rows fixed; newest at the bottom). put() throws on any
// cell past the grid, so a screen that does not fit fails the render instead of being cropped.
'use strict';

const CARD_CMD = 'launchctl kickstart -k gui/501/dev.drover.serve';
const SHED_CMD = 'launchctl kickstart gui/501/dev.shed.serve';

const THREADS = [
  { repo: 'all repos', id: 't34', title: 'Docs: issue names', last: 'Orchestrator: Filed both docs issues and queued each one last', age: '11d', st: 'asleep' },
  { repo: 'drover', id: 't139', title: 'Board 502: serve restart', last: 'Orchestrator asks q2170: Restart serve now, or wait for the fix?', age: '2m', st: 'choose', unread: true },
  { repo: 'drover', id: 't135', title: 'drover chat + board TUI', last: "Orchestrator: Done: the 'one app' line is in #1352's spec", age: '1h', st: 'asleep' },
  { repo: 'drover', id: 't138', title: 'synergy asks stuck open', last: 'Orchestrator: Filed #1353, held until #1299 merges', age: '14h', st: 'asleep', unread: true },
  { repo: 'drover', id: 't117', title: 'Orchestrator profile: Drover vs Cooee', last: "Orchestrator: No, you didn't lose anything you need", age: '24h', st: 'asleep' },
  { repo: 'cooee', id: 't77', title: 'cooee layout-blocks #300', last: 'Orchestrator asks q1858: Look gets New theme… and Edit…', age: '43h', st: 'working' },
  { repo: 'cooee', id: 't59', title: 'Cooee native app: designs', last: 'from t65: answered q1463: the composer mic', age: '6d', st: 'asleep' },
];

const ASK = {
  id: 'q2170', q: "Restart serve now, or wait for #1350's fix?",
  opts: ['Restart it now: I post a run card for you (recommended)', "Wait: #1350's fix is in review and lands tonight", 'Leave it down; I use chat until then'],
};

const HIST = [
  { k: 'you', t: '09:31', text: 'The board is 502 again. What happened?', file: '~/Downloads/board-502.png' },
  { k: 'orch', t: '09:33', text: "Tonight's self-update swapped the binary at 09:30 and serve's restart failed (#1350). serve is down: the page answers 502. The timer, the lanes and this chat still work. Its log:",
    fence: ['09:30:12 serve: bind 127.0.0.1:41942', '09:30:12 error: Address already in use', '09:30:12 serve: exit 1'] },
];
const ASKED = { k: 'ask', t: '09:34', ask: ASK };
const ANSWERED = { ...ASKED, settled: true };
const CHOSE = { k: 'sys', text: 'owner chose [1] for q2170 · 09:36' };
const CARD = { k: 'card', t: '09:37', id: 'r42', cmd: CARD_CMD, dir: '~/.local/share/drover', why: "serve's restart failed (#1350); a forced restart frees the port", blast: 'the board on mini; no lanes, no repo', left: '13 min left' };
const RAN = { k: 'sys', text: 'r42 approved by owner · ran, exit 0 · serve up again · 09:41' };

const SCREENS = [
  { name: 'list', title: 'thread list', view: 'list', sel: 't139', down: { serve: true } },
  { name: 'chat', title: 'chat: a reply streams in, typing', view: 'chat', down: { serve: true }, st: 'working',
    body: [...HIST, { k: 'work', text: 'working… 14s' }], input: 'so is anything lost?' },
  { name: 'choice', title: 'an open choice, 1 pressed', view: 'chat', down: { serve: true },
    body: [...HIST, ASKED], pick: { ask: 0 } },
  { name: 'card', title: 'a run card, a pressed: approve', view: 'chat', down: { serve: true }, st: 'awake',
    body: [...HIST, ANSWERED, CHOSE, { k: 'orch', t: '09:37', text: 'Posted r42: it restarts serve once. Approve it here.' }, CARD], pick: { card: 'approve' } },
  { name: 'refused', title: 'the card refused: expired', view: 'chat', down: { serve: true }, st: 'awake',
    body: [...HIST, ANSWERED, CHOSE, { k: 'orch', t: '09:37', text: 'Posted r42: it restarts serve once. Approve it here.' }, { ...CARD, left: 'expired' }],
    pick: { card: 'approve', refused: 'REFUSED EXPIRED: r42 expired 15 min after it was posted; post a new card' } },
  { name: 'shed-starting', title: 'what is down: serve back, shed down and being started', view: 'chat', down: { shed: 'starting' }, st: 'asleep',
    body: [...HIST, ANSWERED, CHOSE, { ...CARD, settled: true }, RAN, { k: 'you', t: '09:52', text: 'Back, thanks. Why did the restart fail?', wait: 'logged · waits for shed' }] },
  { name: 'shed-failed', title: 'what is down: shed cannot be started', view: 'chat', down: { shed: 'failed' }, st: 'asleep',
    body: [...HIST, ANSWERED, CHOSE, { ...CARD, settled: true }, RAN, { k: 'you', t: '09:52', text: 'Back, thanks. Why did the restart fail?', wait: 'logged · sent once shed answers' }] },
];

const cells = (s, k = '') => [...s].map(ch => ({ ch, k }));
const pad = (row, w, k = '') => row.concat(cells(' '.repeat(Math.max(0, w - row.length)), k));

function wrap(row, w, indent = []) {
  const out = [];
  let r = row;
  while (r.length > w) {
    let cut = -1;
    for (let i = Math.min(w, r.length - 1); i > indent.length; i--) if (r[i].ch === ' ') { cut = i; break; }
    if (out.length && cut <= indent.length) cut = -1;
    const at = cut > 0 ? cut : w;
    out.push(r.slice(0, at));
    r = indent.concat(r.slice(cut > 0 ? at + 1 : at));
  }
  out.push(r);
  return out;
}
const para = (segs, w, indent) => wrap(segs.flatMap(([s, k]) => cells(s, k)), w, indent);
const clamp = (s, w) => (s.length <= w ? s : s.slice(0, w - 1) + '…');

function topLine(w, down, tab = 'threads') {
  const left = [...cells(' Threads ', tab === 'threads' ? 'inv' : 'm'), ...cells(' '), ...cells(' Board ', tab === 'board' ? 'inv' : 'm')];
  const wide = w >= 80;
  const parts = [];
  if (down.serve) parts.push(cells(wide ? '✗ serve down: the page is off, chat works ' : '✗ serve ', 'r'));
  if (down.shed === 'starting') parts.push(cells(wide ? '↻ shed down: starting it ' : '↻ shed ', 'a'));
  if (down.shed === 'failed') parts.push(cells(wide ? '✗ shed down: would not start ' : '✗ shed ', 'r'));
  if (!parts.length) parts.push(cells('✓ ', 'g'));
  const right = parts.flat();
  const gap = w - left.length - right.length;
  if (gap >= 1) return [left.concat(cells(' '.repeat(gap)), right)];
  return [pad(left, w), pad([], w - right.length).concat(right)];
}

function shedRows(w, down) {
  if (down.shed === 'starting') return para([['↻ shed is down. Starting it with the shed job ', 'a'], ['(dev.shed.serve)', 'm'], [': messages wait, then go.', 'a']], w, cells('  '));
  if (down.shed === 'failed') return [
    ...para([['✗ shed did not start. Run it in a shell, then this clears:', 'r']], w, cells('  ')),
    ...para([['  $ ', 'a'], [SHED_CMD, 'c']], w, cells('    ')),
  ];
  return [];
}

function threadHead(w, t) {
  const st = { choose: ['! choose', 'a'], working: ['✻ working', 'p'], asleep: ['☾ asleep', 'm'], awake: ['· awake', 'm'] }[t.st];
  const left = cells(`‹ ${t.id} ${t.title} `, 'b');
  const full = cells(`${t.repo} · `, 'm').concat(cells(st[0] + ' ', st[1]));
  const right = left.length + full.length <= w ? full : cells(st[0].split(' ')[0] + ' ', st[1]);
  if (left.length + right.length <= w) return [left.concat(cells(' '.repeat(w - left.length - right.length)), right).map(c => ({ ...c, bg: 'card' }))];
  return [...wrap(left, w, cells('  ')), pad([], w - right.length).concat(right)].map(r => pad(r, w).map(c => ({ ...c, bg: 'card' })));
}

function listRows(w, sel) {
  const rows = [];
  let repo = '';
  for (const t of THREADS) {
    if (t.repo !== repo) {
      repo = t.repo;
      if (rows.length) rows.push([]);
      rows.push(cells(' ' + repo.toUpperCase(), 'm b'));
    }
    const on = t.id === sel;
    const st = { choose: ['!', 'a'], working: ['✻', 'p'], asleep: ['☾', 'm'] }[t.st];
    const tail = cells(` ${t.age} `, 'm').concat(cells(st[0], st[1]), cells(' '));
    const head = cells(on ? '›' : ' ', 'l').concat(cells(t.unread ? '●' : ' ', 'l'), cells(' '), cells(`${t.id} `, 'm'));
    const room = w - head.length - tail.length;
    const r1 = head.concat(cells(clamp(t.title, room), on ? 'b' : ''));
    const lastK = t.st === 'choose' ? 'a' : 'm';
    const r2 = cells('   ').concat(cells(clamp(t.last, w - 4), lastK));
    const mark = c => (on ? { ...c, bg: 'card' } : c);
    rows.push(pad(r1, w - tail.length).concat(tail).map(mark), pad(r2, w).map(mark));
  }
  return rows;
}

const EDGE = { you: ['┃ ', 'a'], orch: ['┃ ', 'p'], ask: ['┃ ', 'p'], card: ['┃ ', 'p'], work: ['┃ ', 'p'] };

function block(m, w, pick) {
  const e = EDGE[m.k] ? cells(...EDGE[m.k]) : [];
  const iw = w - e.length;
  const edge = rows => rows.map(r => e.concat(r));
  const out = [];
  const who = { you: ['you', 'a'], orch: ['orchestrator', 'p'], ask: ['orchestrator asks', 'p'], card: ['orchestrator posted a run card', 'p'] }[m.k];
  if (m.k === 'sys') return para([['· ' + m.text, 'm']], w, cells('  '));
  if (m.k === 'work') return edge([cells('✻ ' + m.text, 'p')]);
  out.push(...para([[who[0], who[1] + ' b'], [` ${m.t}`, 'm'], [m.k === 'you' && !m.wait ? ' ✓' : '', 'g']], iw, []));
  if (m.text) out.push(...para([[m.text, '']], iw, []));
  if (m.wait) out.push(...para([['… ' + m.wait, 'a']], iw, cells('  ')));
  if (m.file) out.push(...para([['▸ file ', 'm'], [m.file, 'link']], iw, cells('  ')));
  if (m.fence) out.push(...box(m.fence.map(l => [[l, 'c']]), iw, 'm'));
  if (m.ask) {
    out.push(...para([[`? ${m.ask.id} `, 'a b'], [m.ask.q, '']], iw, cells('  ')));
    m.ask.opts.forEach((o, i) => {
      const on = pick && pick.ask === i;
      const k = on ? 'inv' : m.settled ? 'm' : '';
      const rows = para([[`[${i + 1}] `, on ? 'inv' : m.settled ? 'm' : 'l'], [o, k]], iw, cells('    ', k));
      out.push(...rows.map(r => (on ? pad(r, iw, 'inv') : r)));
    });
  }
  if (m.cmd) {
    out.push(...para([[`$ ${m.id} `, 'a b'], [`· ${m.left}`, m.left === 'expired' ? 'r' : 'm']], iw, []));
    out.push(...box([[['$ ', 'a'], [m.cmd, 'c']], [['in ', 'm'], [m.dir, '']], [['why ', 'm'], [m.why, '']], [['touches ', 'm'], [m.blast, '']]], iw, 'm'));
    if (!m.settled && (m.left !== 'expired' || pick)) {
      const sel = pick && pick.card;
      out.push(cells('[a] approve', sel === 'approve' ? 'inv' : 'l').concat(cells('  '), cells('[d] deny', sel === 'deny' ? 'inv' : 'r')));
    }
  }
  return edge(out);
}

function box(lines, w, k, title) {
  const iw = w - 4;
  const t = title ? cells(`┌ ${title[0]} `, k).map(c => (c.ch === '┌' ? c : { ...c, k: title[1] })) : cells('┌', k);
  t[0] = { ch: '┌', k };
  const top = t.concat(cells('─'.repeat(Math.max(0, w - t.length - 1)), k), cells('┐', k));
  const rows = [top];
  for (const segs of lines) for (const r of para(segs, iw, cells('  '))) rows.push(cells('│ ', k).concat(pad(r, iw), cells(' │', k)));
  rows.push(cells('└' + '─'.repeat(w - 2) + '┘', k));
  return rows;
}

function confirmRows(w, s) {
  const p = s.pick;
  if (!p) return [];
  if (p.ask !== undefined) {
    const o = ASK.opts[p.ask];
    return box([[['sends ', 'm'], [`[${p.ask + 1}] ${o}`, '']], [['to ', 'm'], [`${ASK.id} in t139, as owner`, '']]], w, 'l', [`answer ${ASK.id}`, 'l b']);
  }
  if (p.refused) return box([[['✗ ' + p.refused, 'r']], [['nothing ran', 'm']]], w, 'r', [`approve ${CARD.id}`, 'r b']);
  return box([[['runs ', 'm'], ['$ ', 'a'], [CARD.cmd, 'c']], [['in ', 'm'], [CARD.dir, '']], [['once, on mini · sha ', 'm'], ['3f9a1c2', ''], [` · ${CARD.left}`, 'm']]], w, 'l', [`approve ${CARD.id}`, 'l b']);
}

function inputRows(w, s) {
  const text = s.input || '';
  const r = text ? cells('› ', 'l').concat(cells(text), cells('▌', 'l')) : cells('› ', 'l').concat(cells('▌', 'l'), cells('message t139', 'm'));
  return wrap(r, w, cells('  ')).map(x => pad(x, w).map(c => ({ ...c, bg: 'card' })));
}

function hint(w, s) {
  const wide = w >= 80;
  let h;
  if (s.view === 'list') h = wide ? '↑↓ j k move · ↵ open · tab board · ? keys · q quit' : '↑↓ jk move · ↵ open · tab board · q';
  else if (s.pick && s.pick.refused) h = 'esc or ⌫ back';
  else if (s.pick) h = (s.pick.card ? '↵ run' : '↵ send') + ' · esc or ⌫ back';
  else if (s.input) h = wide ? '↵ send · ←→ move · ^U clear · ↑↓ scroll' : '↵ send · ←→ move · ^U clear';
  else if (!s.body.some(m => (m.ask || m.cmd) && !m.settled)) h = wide ? '↵ send · ↑↓ scroll · esc ⌫ list · tab board · ? keys' : '↵ send · ↑↓ scroll · esc list · ?';
  else h = wide ? '↵ send · 1-3 answer · a d card · ↑↓ scroll · esc ⌫ list · tab board · ? keys' : '1-3 answer · a d card · esc list · ?';
  return wrap(cells(' ' + h, 'm'), w, cells(' '));
}

function chatColumn(w, h, s) {
  const t = { ...THREADS.find(x => x.id === 't139'), st: s.st || 'choose' };
  const head = [...shedRows(w, s.down), ...threadHead(w, t)];
  const foot = [...confirmRows(w, s), ...(s.pick ? [] : inputRows(w, s)), ...hint(w, s)];
  const body = [];
  for (const m of s.body) {
    if (body.length) body.push([]);
    body.push(...block(m, w, s.pick));
  }
  const room = h - head.length - foot.length;
  if (room < 1) throw new Error(`${s.name}: no room for the body at ${w}x${h}`);
  let shown = body.slice(-room);
  if (body.length > room) shown = [cells(`  ▲ ${body.length - room + 1} lines above · ↑ scroll`, 'm'), ...body.slice(-(room - 1))];
  while (shown.length < room) shown.unshift([]);
  return [...head, ...shown, ...foot];
}

function listColumn(w, h, s, focus) {
  const rows = listRows(w, s.sel || 't139');
  const foot = focus ? hint(w, { view: 'list' }) : [];
  const room = h - foot.length;
  const shown = rows.slice(0, room);
  while (shown.length < room) shown.push([]);
  return [...shown, ...foot];
}

function layout(cols, rows, s) {
  const top = topLine(cols, s.down);
  const h = rows - top.length;
  let main;
  if (cols >= 100) {
    const lw = 40, cw = cols - lw - 1;
    const listFocus = s.view === 'list';
    const left = listColumn(lw, h, s, listFocus);
    const chatS = s.view === 'list' ? { ...SCREENS[2], pick: undefined, view: 'chat', down: s.down, preview: true } : s;
    const right = chatColumn(cw, h, chatS);
    main = left.map((r, i) => pad(r, lw).concat(cells('│', 'edge'), pad(right[i] || [], cw)));
    if (!listFocus) main = main.map(r => r.map((c, x) => (x < lw && !c.bg ? { ...c, k: c.k.includes('a') || c.k.includes('l') ? c.k : 'm' } : c)));
  } else {
    main = s.view === 'list' ? listColumn(cols, h, s, true) : chatColumn(cols, h, s);
  }
  const grid = [...top, ...main].map(r => pad(r, cols));
  if (grid.length !== rows) throw new Error(`${s.name}: ${grid.length} rows, not ${rows} at ${cols}x${rows}`);
  grid.forEach((r, y) => { if (r.length > cols) throw new Error(`${s.name}: row ${y} is ${r.length} wide at ${cols}`); });
  return grid;
}

function render(cols, rows, s, into) {
  const g = layout(cols, rows, s);
  const pre = document.createElement('div');
  pre.className = 'term';
  pre.dataset.shot = `${s.name}-${cols}x${rows}`;
  pre.style.setProperty('--cols', cols);
  pre.style.setProperty('--rows', rows);
  for (const r of g) {
    const line = document.createElement('div');
    for (const c of r) {
      const i = document.createElement('i');
      i.textContent = c.ch;
      if (c.k) i.className = c.k;
      if (c.bg) i.classList.add('bg-' + c.bg);
      line.append(i);
    }
    pre.append(line);
  }
  into.append(pre);
}

window.CHAT = { SCREENS, render };
