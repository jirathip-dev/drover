// #1354 design mockup: the Board tab of the `drover chat` / `drover board` app, drawn as terminal character grids.
// lay() is a prototype of the ADR's renderer: it reads the phone's screen JSON (fixtures.js, from the demo export) by
// field name, knows no screen and no action, and draws any other block from its text fields. The frame around it is
// #1352's (top line, tabs, hint row, two-press panel). put() throws on a row past the grid: nothing is cropped.
'use strict';

const F = window.FIX;
const NOW = Date.parse(F.NOW);
const TEXTS = ['title', 'badge', 'value', 'subtitle', 'meta', 'header', 'label', 'markdown', 'text', 'command', 'dir', 'touches', 'sha', 'source', 'alt', 'placeholder', 'footer'];
const KIDS = ['blocks', 'rows', 'fields', 'buttons', 'items', 'options'];
const TONE = { ok: 'g', working: 'g', warn: 'a', needs: 'a', bad: 'r', fail: 'r', accent: 'l', done: 'l' };
const FIG = { working: ['●', 'g'], needs: ['!', 'a'], fail: ['✗', 'r'], done: ['✓', 'l'], idle: ['○', 'm'] };
const tone = t => TONE[t] || 'm';

const cells = (s, k = '') => [...String(s)].map(ch => ({ ch, k }));
const pad = (row, w, k = '') => row.concat(cells(' '.repeat(Math.max(0, w - row.length)), k));
const on = (row, bg) => row.map(c => ({ ...c, bg }));

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
const para = (segs, w, indent = []) => wrap(segs.flatMap(([s, k]) => cells(s, k)), w, indent);

function value(v) {
  if (v && typeof v === 'object' && 'until' in v) return String(v.label || '').replace('{rel}', `${Math.max(0, Math.round((Date.parse(v.until) - NOW) / 60000))}m`);
  if (v && typeof v === 'object' && 'since' in v) {
    const s = Math.max(0, (NOW - Date.parse(v.since)) / 1000);
    if (v.label != null) return String(v.label).replace('{}', s < 120 ? `${Math.floor(s)}s` : s < 7200 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h`);
    return s < 60 ? 'now' : s < 3600 ? `${Math.floor(s / 60)}m` : s < 86400 ? `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m` : `${Math.floor(s / 86400)}d`;
  }
  if (v && typeof v === 'object') return String(v.text ?? '');
  return v == null ? '' : String(v);
}
const toneOf = (v, dflt = '') => (v && typeof v === 'object' && v.tone ? tone(v.tone) : dflt);
const seg = (v, dflt = '') => [value(v), toneOf(v, dflt)];

function md(s) {
  const out = [];
  String(s).split(/```[a-z]*\n?/).forEach((part, i) => {
    if (i % 2) { out.push({ box: part.replace(/\n$/, '').split('\n') }); return; }
    for (const line of part.split('\n').filter(Boolean)) {
      const segs = [];
      for (const [j, b] of line.split('**').entries()) for (const [n, c] of b.split('`').entries()) if (c) segs.push([c, (j % 2 ? 'b' : '') + (n % 2 ? ' c' : '')]);
      out.push({ segs });
    }
  });
  return out;
}

function lay(tree, w, ui = {}) {
  const L = { lines: [], hots: [], w, ui, screen: tree.id };
  const edits = ui.edits || {};
  const add = (pre, rows, hot) => {
    for (const r of rows) {
      let line = pre.concat(r);
      if (hot && hot.id === ui.sel) line = on(pad(line, w), 'card');
      L.lines.push(line);
    }
  };
  const gap = () => { if (L.lines.length && L.lines[L.lines.length - 1].length) L.lines.push([]); };
  const hot = (id, extra) => { const h = { id, from: L.lines.length, acts: [], ...extra }; L.hots.push(h); return h; };
  const end = h => { h.to = L.lines.length; };
  const send = (label, toneName, block, action, values, confirm, shows) => ({ label, tone: toneName, send: { screen: L.screen, block, action, values: values || {} }, confirm, shows });

  function buttons(pre, w2, list) {
    let line = [];
    const flush = () => { if (line.length) { L.lines.push(pre.concat(line)); line = []; } };
    for (const b of list) {
      const id = b.id || b.label;
      const k = b.style === 'primary' ? 'l b' : b.style === 'destructive' ? 'r' : 'l';
      const sel = id === ui.sel;
      const label = cells(`[${b.label}]`, sel ? 'inv' : k);
      if (line.length && line.length + 1 + label.length > w2) flush();
      const h = hot(id, b.tap ? { tap: b.tap } : {});
      if (b.action) h.acts.push(send(b.label, b.style === 'destructive' ? 'bad' : '', b.block ?? (b.type === 'button' ? b.id : undefined), b.action, b.values, b.confirm));
      if (line.length) line.push(...cells(' '));
      h.x = pre.length + line.length;
      line.push(...label);
      h.to = L.lines.length + 1;
    }
    flush();
  }

  function rowBlock(b, pre, w2) {
    const ind = cells('  '.repeat(b.indent || 0));
    const fig = b.figure ? FIG[b.figure.state] || FIG.idle : null;
    const head = cells(' ').concat(ind, fig ? cells(fig[0], fig[1]).concat(cells(' ')) : []);
    const tk = b.style === 'link' ? 'l' : '';
    const lead = cells(' '.repeat(head.length));
    const tail = [];
    if (b.badge) tail.push(...cells(value(b.badge), toneOf(b.badge, 'm')));
    if (b.value != null && value(b.value)) tail.push(...cells((tail.length ? ' ' : '') + value(b.value), 'm'));
    if (b.accessory === 'chevron') tail.push(...cells(' ▸', 'm'));
    const beside = tail.length && tail.length * 2 <= w2;
    const title = wrap(head.concat(cells(value(b.title), toneOf(b.title, tk))), beside ? w2 - tail.length - 1 : w2, lead);
    if (beside) title[0] = pad(title[0], w2 - tail.length).concat(tail);
    if (tail.length && !beside) title.push(pad(cells(' '), w2 - tail.length).concat(tail));
    const sub = b.subtitle && value(b.subtitle) ? wrap(lead.concat(cells(value(b.subtitle), toneOf(b.subtitle, 'm'))), w2, lead) : [];
    const isHot = b.tap || (b.swipe && b.swipe.length);
    const h = isHot ? hot(b.id, { tap: b.tap }) : null;
    if (h) for (const s of b.swipe || []) h.acts.push(send(s.label, s.tone, b.id, s.action, s.values, s.confirm));
    const rowsOut = [...title, ...sub].map(r => pad(r, w2));
    if (h && h.id === ui.sel) rowsOut[0][0] = { ch: '›', k: 'l' };
    add(pre, rowsOut, h);
    if (h) end(h);
  }

  function field(f, pre, w2, form) {
    const name = f.name;
    const v = name in edits ? edits[name] : f.value;
    if (f.type === 'toggle') {
      const h = hot(name, { tap: { local: 'flip' } });
      add(pre, para([[' '], [v ? '[x] ' : '[ ] ', v ? 'g' : 'm'], [f.label || '', '']], w2, cells('     ')).map(r => pad(r, w2)), h);
      if (h.id === ui.sel) L.lines[h.from][pre.length] = { ch: '›', k: 'l', bg: 'card' };
      end(h);
    } else if (f.type === 'secret') {
      add(pre, para([[' ' + (f.label || 'secret') + ': ', 'm'], ['enter it on a paired device', 'a']], w2, cells('   ')));
      form.secret = true;
    } else if (f.type === 'picker') {
      const h = hot(name);
      for (const o of f.options || []) h.acts.push({ label: o.label || o.value, set: { name, value: o.value } });
      add(pre, para([[' ' + f.label + ' ', 'm'], [value(v), ''], [' ▾', 'm']], w2, cells('   ')).map(r => pad(r, w2)), h);
      end(h);
    } else {
      const h = hot(name, { tap: { local: 'edit' } });
      const editing = ui.editing === name;
      const shown = editing ? [[value(v), ''], ['▌', 'l']] : value(v) ? [[value(v), '']] : [[f.placeholder || '', 'm']];
      const box = para([[' ' + (f.label || name) + '  ', 'm'], ...shown], w2, cells('   ')).map(r => pad(r, w2));
      add(pre, box.map(r => (editing ? r.map(c => ({ ...c, bg: 'card' })) : r)), h);
      if (h.id === ui.sel) L.lines[h.from][pre.length] = { ch: '›', k: 'l', bg: 'card' };
      end(h);
    }
  }

  function boxed(pre, w2, lines, k = 'm', title) {
    const iw = w2 - 4;
    const top = title ? cells('┌ ', k).concat(cells(title[0], title[1]), cells(' ')) : cells('┌', k);
    L.lines.push(pre.concat(top, cells('─'.repeat(Math.max(0, w2 - top.length - 1)), k), cells('┐', k)));
    for (const segs of lines) for (const r of para(segs, iw, cells('  '))) L.lines.push(pre.concat(cells('│ ', k), pad(r, iw), cells(' │', k)));
    L.lines.push(pre.concat(cells('└' + '─'.repeat(w2 - 2) + '┘', k)));
  }

  function draw(b, pre, w2) {
    if (!b || typeof b !== 'object') return;
    const t = b.type;
    if (t === 'screen' || t === 'sheet') {
      const acts = (b.actions || []).flatMap(a => a.menu || [a]).map(m => ({ ...m, id: `act-${m.label}` }));
      if (acts.length) buttons(pre, w2, acts);
      (b.blocks || []).forEach(c => draw(c, pre, w2));
      if (b.done) { gap(); buttons(pre, w2, [{ ...b.done, id: `${b.id}-done`, block: b.id, style: 'primary' }]); }
    } else if (t === 'section') {
      gap();
      if (b.header) add(pre, para([[' ' + value(b.header), 'm b']], w2, cells(' ')));
      (b.blocks || []).forEach(c => draw(c, pre, w2));
      if (b.footer) add(pre, para([[' ' + value(b.footer), 'm']], w2, cells(' ')));
    } else if (t === 'card') {
      gap();
      const ek = tone(b.tone) === 'm' && !b.tone ? 'p' : tone(b.tone);
      const open = (ui.open || []).includes(b.id) || !b.fold;
      if (!open) {
        const h = hot(b.id, { tap: { local: 'fold' } });
        if (b.fold.main && b.fold.main.action) h.acts.push(send(b.fold.main.label, '', b.fold.main.block, b.fold.main.action, { ...(b.fold.main.values || {}), ...(b.fold.main.value ? { choice: b.fold.main.value } : {}) }));
        const rows = para([['▸ ', ek], [b.fold.line, '']], w2 - 2, cells('    '));
        add(pre, rows.map(r => pad(cells('┃ ', ek).concat(r), w2)), h);
        if (h.id === ui.sel) L.lines[h.from][pre.length] = { ch: '›', k: 'l', bg: 'card' };
        end(h);
        return;
      }
      const e = pre.concat(cells('┃ ', ek));
      const iw = w2 - 2;
      const badge = b.badge ? cells(value(b.badge), toneOf(b.badge, 'm')) : [];
      const tl = para([[value(b.title), 'b']], iw);
      const last = tl[tl.length - 1];
      if (badge.length) { if (last.length + 1 + badge.length <= iw) tl[tl.length - 1] = pad(last, iw - badge.length).concat(badge); else tl.push(pad([], iw - badge.length).concat(badge)); }
      add(e, tl);
      for (const k of ['meta', 'subtitle']) if (b[k]) add(e, para([seg(b[k], 'm')], iw));
      (b.blocks || []).forEach(c => draw(c, e, iw));
      if (b.buttons && b.buttons.length) { L.lines.push(e.slice()); buttons(e, iw, b.buttons); }
    } else if (t === 'list') {
      (b.rows || []).forEach(r => draw(r, pre, w2));
      if (b.more) add(pre, [cells('   ↓ more as you scroll', 'm')]);
    } else if (t === 'row') {
      rowBlock(b, pre, w2);
    } else if (t === 'text' && 'markdown' in b) {
      for (const m of md(b.markdown)) {
        if (m.box) boxed(pre, w2, m.box.map(l => [[l, 'c']]));
        else add(pre, para([[' '], ...m.segs], w2, cells(' ')));
      }
    } else if (t === 'button') {
      buttons(pre, w2, [b]);
    } else if (t === 'form') {
      const form = {};
      (b.fields || []).forEach(f => field(f, pre, w2, form));
      if (b.submit && !form.secret) buttons(pre, w2, [{ ...b.submit, id: `${b.id}-submit`, block: b.id, style: 'primary' }]);
    } else if (t === 'toggle' || (t === 'text' && b.name) || t === 'picker' || t === 'secret') {
      field(b, pre, w2, {});
    } else if (t === 'chips') {
      const sel = new Set(b.selected || []);
      const h = b.action ? hot(b.id, { inline: false }) : null;
      const row = [];
      (b.options || []).forEach((o, i) => {
        if (row.length) row.push(...cells(' '));
        row.push(...cells(` ${o.label} `.replace(/ (?=\S)/g, (m, i) => (i ? '\u00a0' : ' ')), sel.has(o.value) ? 'inv' : 'm'));
        if (h) {
          const next = b.multi ? [...sel].filter(x => x !== 'all' && x !== o.value).concat(sel.has(o.value) ? [] : [o.value]) : [o.value];
          h.acts.push(send(o.label, '', b.id, b.action, { [b.name || b.id]: next.length ? next : ['all'] }));
        }
      });
      add(pre, wrap(cells(h && h.id === ui.sel ? '›' : ' ', 'l').concat(row), w2, cells(' ')), null);
      if (h) end(h);
    } else if (t === 'choice') {
      const h = hot(b.id, { inline: true });
      const opts = b.options || [];
      opts.forEach((o, i) => {
        const vals = { ...(b.values || {}), ...(o.values || {}), choice: o.value };
        h.acts.push(send(o.label, '', b.id, b.action, vals, o.confirm));
        const picked = ui.picked === i;
        const rec = o.value === b.recommended;
        const k = picked ? 'inv' : '';
        const rows = para([[` [${i + 1}] `, picked ? 'inv' : 'l'], [o.label, k], [rec ? ' ★ recommended' : '', picked ? 'inv' : 'a']], w2, cells('     ', k));
        add(pre, rows.map(r => (picked ? pad(r, w2, 'inv') : r)));
      });
      if (b.other) {
        const n = opts.length + 1;
        h.acts.push({ ...send('other', '', b.id, b.action, { ...(b.values || {}), choice: b.other.for || 'other' }), typed: { key: 'other', placeholder: b.other.placeholder } });
        add(pre, para([[` [${n}] `, 'l'], [b.other.placeholder || 'other…', 'm']], w2, cells('     ')));
      }
      if (b.default) add(pre, para([['  ⏱ ' + value({ until: b.default.until, label: b.default.label }), 'a']], w2, cells('    ')));
      if (h.id === ui.sel) L.lines[h.from][pre.length] = { ch: '›', k: 'l' };
      end(h);
    } else if (t === 'stat') {
      const row = [];
      for (const it of b.items || []) {
        const bars = it.spark ? it.spark.map(x => '▁▂▃▄▅▆▇█'[Math.min(7, Math.round((x / Math.max(...it.spark, 1)) * 7))]).join('') : '';
        const piece = cells((it.label + ' ').replace(/ (?=.)/g, '\u00a0'), 'm').concat(cells(value(it.value), it.tone ? tone(it.tone) : 'b'), bars ? cells(' ' + bars, 'l') : []);
        if (row.length) row.push(...cells(' · ', 'edge'));
        row.push(...piece);
      }
      add(pre, wrap(cells(' ').concat(row), w2, cells(' ')));
    } else if (t === 'run') {
      const h = hot(b.id, { inline: true });
      const shows = [[['$ ', 'a'], [b.command, 'c']], [['in ', 'm'], [b.dir, '']], [['sha ', 'm'], [b.sha, ''], [` · ${value({ until: b.until, label: '{rel} left' })}`, 'm']]];
      for (const [i, c] of ['approve', 'deny'].entries()) h.acts.push(send(c, i ? 'bad' : '', b.id, b.action, { choice: c, sha: b.sha }, null, shows));
      boxed(pre, w2, [shows[0], [['in ', 'm'], [b.dir, '']], [['touches ', 'm'], [b.touches, '']], shows[2]]);
      const p = ui.picked;
      add(pre, [cells(h.id === ui.sel ? '›' : ' ', 'l').concat(cells('[1] approve', p === 0 ? 'inv' : 'l'), cells('  '), cells('[2] deny', p === 1 ? 'inv' : 'r'))]);
      end(h);
    } else if (t === 'images') {
      for (const it of b.items || []) add(pre, para([[' ▸ image ', 'm'], [it.alt || '', ''], [' · shown on the phone', 'm']], w2, cells('   ')));
    } else if (t === 'diagram') {
      boxed(pre, w2, String(b.source || '').split('\n').map(l => [[l, 'c']]));
    } else if (t === 'scene') {
      add(pre, para([[' Herd is drawn on the phone', 'm']], w2));
    } else {
      const segs = TEXTS.filter(k => b[k] != null && value(b[k])).map(k => [value(b[k]) + ' ', toneOf(b[k], k === 'title' ? 'b' : '')]);
      const h = b.action || b.tap ? hot(b.id || b.type, { tap: b.tap }) : null;
      if (h && b.action) h.acts.push(send(b.label || b.title || b.action, '', b.id, b.action, b.values, b.confirm));
      if (segs.length) add(pre, para([[' '], ...segs], w2, cells(' ')), h);
      KIDS.forEach(k => (Array.isArray(b[k]) ? b[k] : []).forEach(c => draw(c, pre, w2)));
      if (h) end(h);
    }
  }
  draw(tree, [], w);
  L.lines = L.lines.map(r => { if (r.length > w) throw new Error(`${tree.id}: a line is ${r.length} wide at ${w}: ${r.map(c => c.ch).join('')}`); return r; });
  return L;
}

const PLACES = [['queue', 'Queue'], ['events', 'Events'], ['lanes', 'Lanes']];

function topLine(w, s) {
  const badge = s.badge ? ` ${s.badge}` : '';
  const tab = s.tab || 'board';
  const left = [...cells(' Threads ', tab === 'threads' ? 'inv' : 'm'), ...cells(' '), ...cells(` Board${badge} `, tab === 'board' ? 'inv' : 'm')];
  if (s.badge && tab !== 'board') left.splice(left.length - 1 - badge.length, badge.length + 1, ...cells(badge + ' ', 'a b'));
  const wide = w >= 80, d = s.down || {};
  const parts = [];
  if (d.serve) parts.push(cells(wide ? '✗ serve down: the page is off, this app works ' : '✗ serve ', 'r'));
  if (d.shed === 'starting') parts.push(cells(wide ? '↻ shed down: starting it ' : '↻ shed ', 'a'));
  if (d.snap) parts.push(cells(wide ? '! snapshot 12m old: the timer is late ' : '! snap ', 'a'));
  if (!parts.length) parts.push(cells('✓ ', 'g'));
  const right = parts.flat();
  const gap = w - left.length - right.length;
  if (gap >= 1) return [left.concat(cells(' '.repeat(gap)), right)];
  return [pad(left, w), pad([], w - right.length).concat(right)];
}

function downRows(w, d = {}) {
  if (d.shed === 'starting') return para([[' ↻ shed is down. Starting it with the shed job ', 'a'], ['(dev.shed.serve)', 'm'], [': lanes and their panes show as last seen.', 'a']], w, cells('   '));
  return [];
}

function strip(w, place, scrolled) {
  const row = cells(' ');
  for (const [i, [id, label]] of PLACES.entries()) {
    if (i) row.push(...cells(' · ', 'edge'));
    row.push(...cells(label, id === place ? 'l b u' : 'm'));
  }
  const right = cells(w >= 80 ? '←→ place · m menu ' : 'm menu ', 'm');
  return [pad(row, w - right.length).concat(right)];
}

function sheetHead(w, t) {
  return wrap(cells(`‹ ${t.title}`, 'b'), w, cells('  ')).map(r => on(pad(r, w), 'card'));
}

function actBar(w, h) {
  if (!h || h.inline || !h.acts.length) return [];
  const row = cells(' ');
  h.acts.forEach((a, i) => {
    if (i) row.push(...cells('  '));
    row.push(...cells(`${i + 1} `, 'l b'), ...cells(a.label, a.tone ? tone(a.tone) : ''));
  });
  return wrap(row, w, cells(' ')).map(r => on(pad(r, w), 'card'));
}

function panel(w, s, h) {
  const p = s.pick;
  const a = h.acts[p.act];
  const iw = w - 4;
  const k = p.refused ? 'r' : 'l';
  const name = a.label.length <= 16 ? `${a.label} · ${a.send.action}` : a.send.action;
  const title = p.refused ? `${name}: refused` : name;
  const lines = a.confirm && a.confirm.title && !p.refused ? [[[a.confirm.title, 'b']]] : [];
  if (p.refused) lines.push([['✗ ' + p.refused, 'r']], [[a.send.action === 'run-card' ? 'nothing ran' : 'nothing changed', 'm']]);
  else {
    if (a.confirm && a.confirm.message) lines.push([[a.confirm.message, '']]);
    if (a.shows) lines.push([['runs ', 'm'], ...a.shows[0]], a.shows[1], [[a.shows[2][2][0].replace(' · ', ''), 'm']]);
    lines.push([['sends ', 'm'], [a.send.action, 'b'], [' to ', 'm'], [[a.send.screen, a.send.block].filter(Boolean).join(' · '), '']]);
    const vals = Object.entries(a.send.values).map(([key, v]) => [[`${key}\u00a0`, 'm'], [(Array.isArray(v) ? v.join(', ') : String(v)).replace(/ /g, key === 'choice' ? ' ' : '\u00a0'), '']]);
    if (vals.length) lines.push(vals.flatMap((x, i) => (i ? [[' · ', 'm'], ...x] : x)));
    if (a.typed) lines.push([['other ', 'm'], [p.typed || '', ''], ['▌', 'l']]);
  }
  const top = cells('┌ ', k).concat(cells(title, k + ' b'), cells(' ' + '─'.repeat(Math.max(0, w - title.length - 4)), k), cells('┐', k));
  const out = [top];
  for (const segs of lines) for (const r of para(segs, iw, cells('  '))) out.push(cells('│ ', k).concat(pad(r, iw), cells(' │', k)));
  out.push(cells('└' + '─'.repeat(w - 2) + '┘', k));
  return out;
}

function hint(w, s, h, sheet) {
  const wide = w >= 80 || s.cols >= 100;
  let t;
  if (s.pick && s.pick.refused) t = '↵ esc ⌫ close';
  else if (s.pick) t = (h.acts[s.pick.act].shows ? '↵ run' : '↵ send') + ' · esc or ⌫ back';
  else if (s.ui && s.ui.editing) t = wide ? 'type · ↵ search · ⌫ delete · ^U clear · esc undo' : 'type · ↵ search · esc undo';
  else {
    const n = h && h.acts.length;
    const open = h && h.tap ? (h.tap.local === 'flip' ? '↵ tick' : h.tap.local === 'edit' ? '↵ type' : h.tap.local === 'fold' ? '↵ unfold' : h.tap.url ? '↵ link' : '↵ open') : '';
    const act = n ? (n === 1 ? (open ? '1 act' : '↵ 1 act') : `1-${n} ${h.inline ? 'pick' : 'act'}`) : '';
    const parts = wide
      ? ['↑↓ move', open, act, sheet ? `esc ⌫ ${sheet.toLowerCase()}` : '←→ place', 'tab threads', '? keys']
      : ['↑↓', open, act, sheet ? `esc ${sheet.toLowerCase()}` : '←→', '?'];
    let p = parts.filter(Boolean);
    if (wide && (' ' + p.join(' · ')).length > w) p = p.filter(x => x !== 'tab threads').map(x => (x === '? keys' ? '?' : x));
    t = p.join(' · ');
  }
  return wrap(cells(' ' + t, 'm'), w, cells(' '));
}

function windowed(lines, room, top) {
  let t = Math.max(0, Math.min(top, Math.max(0, lines.length - room)));
  let shown = lines.slice(t, t + room);
  if (t > 0) shown = [cells(`  ▲ ${t + 1} lines above`, 'm'), ...lines.slice(t + 1, t + room)];
  const below = lines.length - (t + room);
  if (below > 0) shown[shown.length - 1] = cells(`  ▼ ${below + 1} more below`, 'm');
  while (shown.length < room) shown.push([]);
  return shown;
}

function column(w, h, s, focus, parts) {
  const { head, trees, sheet } = parts;
  const laid = { lines: [], hots: [] };
  for (const [tree, ui] of trees) {
    const l = lay(tree, w, ui);
    if (laid.lines.length && l.lines.length) laid.lines.push([]);
    const off = laid.lines.length;
    laid.lines.push(...l.lines);
    laid.hots.push(...l.hots.map(x => ({ ...x, from: x.from + off, to: x.to + off })));
  }
  const sel = laid.hots.find(x => x.id === s.ui.sel);
  const foot = focus ? [...(s.toast ? para([[' ' + s.toast[0], s.toast[1]]], w, cells('   ')) : []), ...(s.pick ? panel(w, s, sel) : actBar(w, sel)), ...hint(w, s, sel, sheet)] : [];
  const room = h - head.length - foot.length;
  if (room < 3) throw new Error(`${s.name}: no room for the body at ${w}x${h}`);
  const top = s.top !== undefined ? s.top : sel ? Math.max(0, sel.from - Math.floor(room / 3)) : 0;
  return [...head, ...windowed(laid.lines, room, top), ...foot];
}

const TREES = { queue: F.queue, events: F.events, lanes: F.lanes };

function boardLayout(cols, rows, s) {
  s = { ...s, cols };
  const top = [...topLine(cols, s), ...downRows(cols, s.down)];
  const h = rows - top.length;
  const root = TREES[s.place];
  const cards = s.cards ? [[s.cards, s.ui]] : [];
  const sheet = s.sheet ? F[s.sheet] : null;
  let main;
  if (cols >= 100) {
    const lw = 68, rw = cols - lw - 1;
    const leftFocus = !sheet && !s.cardsFocus;
    const left = column(lw, h, leftFocus ? s : { ...s, ui: {} }, leftFocus, { head: strip(lw, s.place), trees: [[root, leftFocus ? s.ui : {}]] });
    const rightHead = sheet ? sheetHead(rw, sheet) : [on(pad(cells(cards.length ? ' asks and cards' : ' no asks or cards open', 'm b'), rw), 'card')];
    const right = column(rw, h, leftFocus ? { ...s, ui: {} } : s, !leftFocus, { head: rightHead, trees: sheet ? [[sheet, s.ui]] : cards.length ? cards.map(([t]) => [t, leftFocus ? {} : s.ui]) : [], sheet: sheet && (sheet.cancel || 'Done') });
    main = left.map((r, i) => {
      const l = pad(r, lw).map(c => (sheet && !c.bg ? { ...c, k: /[arg]/.test(c.k) ? c.k : 'm' } : c));
      return l.concat(cells('│', 'edge'), pad(right[i] || [], rw));
    });
  } else if (sheet) {
    main = column(cols, h, s, true, { head: sheetHead(cols, sheet), trees: [[sheet, s.ui]], sheet: sheet.cancel || 'Done' });
  } else {
    main = column(cols, h, s, true, { head: strip(cols, s.place), trees: [...cards, [root, s.ui]] });
  }
  return [...top, ...main];
}

const THREADS = [
  { repo: 'all repos', id: 't6', title: 'Weekly ship notes', last: 'Orchestrator: Drafted this week\'s notes; 4 merges', age: '35h', st: 'asleep' },
  { repo: 'acme-api', id: 't4', title: 'Rate limits for /v1/search', last: 'Orchestrator asks q33: Retry-After only, or also X-RateLimit-*?', age: '4m', st: 'choose', unread: true },
  { repo: 'acme-api', id: 't2', title: 'OpenAPI spec at /v1/openapi.json', last: 'Orchestrator: #22 ran out of rounds; your call on its card', age: '59m', st: 'asleep' },
  { repo: 'acme-api', id: 't3', title: 'Pipeline: land mode and premerge', last: 'Orchestrator: Done: premerge runs on the merged tree', age: '5h', st: 'asleep' },
  { repo: 'acme-web', id: 't5', title: 'Settings page follow-ups', last: 'Orchestrator: Queued #47, #49, #50 as one chain under #46', age: '2h', st: 'asleep' },
];
const clamp = (x, w) => (x.length <= w ? x : x.slice(0, w - 1) + '…');

function threadsLayout(cols, rows, s) {
  const top = topLine(cols, s);
  const w = cols >= 100 ? 40 : cols;
  const out = [];
  let repo = '';
  for (const t of THREADS) {
    if (t.repo !== repo) { repo = t.repo; if (out.length) out.push([]); out.push(cells(' ' + repo.toUpperCase(), 'm b')); }
    const sel = t.id === 't4';
    const st = { choose: ['!', 'a'], asleep: ['☾', 'm'] }[t.st];
    const tail = cells(` ${t.age} `, 'm').concat(cells(st[0], st[1]), cells(' '));
    const head = cells(sel ? '›' : ' ', 'l').concat(cells(t.unread ? '●' : ' ', 'l'), cells(' '), cells(`${t.id} `, 'm'));
    const r1 = head.concat(cells(clamp(t.title, w - head.length - tail.length), sel ? 'b' : ''));
    const r2 = cells('   ').concat(cells(clamp(t.last, w - 4), t.st === 'choose' ? 'a' : 'm'));
    const mark = r => (sel ? on(r, 'card') : r);
    out.push(mark(pad(r1, w - tail.length).concat(tail)), mark(pad(r2, w)));
  }
  const hintRow = wrap(cells(cols >= 80 ? ' ↑↓ j k move · ↵ open · tab board (back where you were) · ? keys · q quit' : ' ↑↓ jk · ↵ open · tab board · q', 'm'), cols, cells(' '));
  const room = rows - top.length - hintRow.length;
  while (out.length < room) out.push([]);
  let body = out.slice(0, room);
  if (cols >= 100) {
    const note = para([[' t4 · Rate limits for /v1/search', 'b']], cols - 41).concat([[]], para([[' Threads is #1352\'s tab, unchanged. Tab goes back to the Board where you left it: Queue, the cursor on #21.', 'm']], cols - 42, cells(' ')));
    body = body.map((r, i) => pad(r, 40).concat(cells('│', 'edge'), note[i] || []));
  }
  return [...top, ...body, ...hintRow];
}

const ROOTS = { queue: 'queue', events: 'events', lanes: 'lanes' };
const SCREENS = [
  { name: 'board', title: 'the board: the Queue, its fleet cards and rows with their marks; asks and cards above it', place: 'queue', badge: 5, cards: F.cardsAsk, ui: { sel: 'q-acme-api-21' } },
  { name: 'reorder', title: 'a queue reorder: the cursor on a queued row, 1 (▲) pressed', place: 'queue', badge: 5, cards: F.cardsAsk, ui: { sel: 'q-acme-web-46' }, pick: { act: 0 } },
  { name: 'issue', title: 'the issue sheet: #15 in review, the cursor on its verdict row', place: 'queue', badge: 5, cards: F.cardsAsk, sheet: 'issue', ui: { sel: 'v-acme-api-impl-15' } },
  { name: 'ask', title: 'an ask: the q33 card unfolded, 1 pressed', place: 'queue', badge: 5, cards: F.cardsAsk, cardsFocus: true, ui: { sel: 'q33', open: ['ask-q33'], picked: 0 }, pick: { act: 0 } },
  { name: 'run', title: 'a run card selected for approve: 1 pressed', place: 'queue', badge: 6, cards: F.cardsRun, cardsFocus: true, ui: { sel: 'r7', picked: 0 }, pick: { act: 0 } },
  { name: 'refused', title: 'the run card refused: its text changed since it was drawn', place: 'queue', badge: 6, cards: F.cardsRun, cardsFocus: true, ui: { sel: 'r7', picked: 0 }, pick: { act: 0, refused: 'REFUSED CARD_CHANGED: r7 was posted again with another command; approve the new card' } },
  { name: 'backlog', title: 'backlog: a filter on, typing a search', place: 'queue', badge: 5, cards: F.cardsAsk, sheet: 'backlog', ui: { sel: 'acme-web.q', editing: 'acme-web.q', edits: { 'acme-web.q': 'settings', 'acme-web.pick.39': true, 'acme-web.pick.52': true } } },
  { name: 'hosts', title: 'hosts: the cursor on a remote host, its action Off; a done action\'s toast', place: 'queue', badge: 5, cards: F.cardsAsk, sheet: 'hosts', ui: { sel: 'h-gpu-box' }, toast: ['✓ reaped 2 leftover processes on mini', 'g'] },
  { name: 'lanes', title: 'lanes: the Lanes place, the cursor on the lane that waits', place: 'lanes', badge: 5, ui: { sel: 'l-acme-web-impl-44' } },
  { name: 'events', title: 'events: the Events place, its kind filter, the cursor on an ask', place: 'events', badge: 5, ui: { sel: 'e-96' } },
  { name: 'tab', title: 'the tab switch: Tab on the Board opens Threads; the Board tab keeps its count', tab: 'threads', badge: 5 },
  { name: 'down', title: 'what is down: serve stopped and shed being started; the board still works', place: 'queue', badge: 5, cards: F.cardsAsk, ui: { sel: 'q-acme-api-21' }, down: { serve: true, shed: 'starting' } },
];
for (const s of SCREENS) s.ui = s.ui || {};

function layout(cols, rows, s) {
  const g = (s.tab === 'threads' ? threadsLayout(cols, rows, s) : boardLayout(cols, rows, s)).map(r => pad(r, cols));
  if (g.length !== rows) throw new Error(`${s.name}: ${g.length} rows, not ${rows} at ${cols}x${rows}`);
  g.forEach((r, y) => { if (r.length > cols) throw new Error(`${s.name}: row ${y} is ${r.length} wide at ${cols}`); });
  return g;
}

function render(cols, rows, s, into) {
  const g = layout(cols, rows, s);
  const pre = document.createElement('div');
  pre.className = 'term';
  pre.dataset.shot = `${s.name}-${cols}x${rows}`;
  pre.style.setProperty('--cols', cols);
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

window.BOARD = { SCREENS, render, lay, layout };
