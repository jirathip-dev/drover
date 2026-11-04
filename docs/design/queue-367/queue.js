// #367 queue pure: the Queue tab's groups, chain summaries and phase times. No DOM, so node runs it (queue.test.js;
// phase 2: crates/server/tests, as chat_blocks.rs runs the chat part).
// One unit style for every time on the tab: 45s, 47m, 1h 47m, 2h, 3d 4h (two units at most, the second never 0).
function qdur(s) {
  s = Math.max(0, Math.floor(s));
  const two = (a, an, b, bn) => b ? `${a}${an} ${b}${bn}` : `${a}${an}`;
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s / 60) + 'm';
  if (s < 86400) return two(Math.floor(s / 3600), 'h', Math.floor(s % 3600 / 60), 'm');
  return two(Math.floor(s / 86400), 'd', Math.floor(s % 86400 / 3600), 'h');
}
// A row's group: 0 needs you, 1 in progress, 2 queued (blocked, parked and anything new wait there too).
const qrank = st => /^(needs|⏸|dead)/.test(st) ? 0 : /^(impl|review|fix|ready|update)/.test(st) ? 1 : 2;
// The time names the phase it measures: `in impl 1h 47m`, `ready 35m`, `needs-human 2h`; a queued row has none.
const qwhen = (st, s) => qrank(st) === 2 || s == null ? '' : `${/^(impl|review|fix|update)/.test(st) ? 'in ' : ''}${st} ${qdur(s)}`;
// A chain's name: the series its titles share (`Decouple 3/7: …` → `Decouple`), else its first step's title.
function qseries(titles) {
  const s = titles.map(t => (/^(.{1,30}?)\s+\d+\/\d+\s*:/.exec(t || '') || [])[1]);
  return s[0] && s.every(x => x === s[0]) ? s[0] : titles[0] || '';
}
// rows: [{n, st, b (blocker's number or null), t (title), s (seconds in the phase)}] in the server's order (needs you,
// ready, working, queued; each in queue order). Returns the three groups as slots: {row} or {chain}. A chain is every
// row linked by `blocked by` to another row of this repo: one slot in the group of its most urgent step, at that step's
// place. Its steps: the main line (the longest path, ties in queue order) at level 1, side branches after their step at
// level 2 (the cap: deeper rows stay at 2 and say `after #b`).
function qgroups(rows) {
  const by = new Map(rows.map(r => [r.n, r])), at = new Map(rows.map((r, i) => [r.n, i]));
  const up = r => {
    const seen = new Set([r.n]);
    for (let x = by.get(r.b); x; x = by.get(x.b)) { if (seen.has(x.n)) return null; seen.add(x.n); } // a loop: no chain
    return by.get(r.b) || null;
  };
  const kids = new Map(rows.map(r => [r.n, []]));
  rows.forEach(r => { const p = up(r); if (p) kids.get(p.n).push(r); });
  const depth = r => 1 + Math.max(0, ...kids.get(r.n).map(depth));
  const sub = r => [r, ...kids.get(r.n).flatMap(sub)];
  const out = [[], [], []], placed = new Set();
  const chain = root => {
    const steps = [];
    for (let x = root; x;) {
      steps.push({ row: x, lv: 1 });
      const ks = [...kids.get(x.n)].sort((a, b) => depth(b) - depth(a) || at.get(a.n) - at.get(b.n)), main = ks[0];
      for (const k of ks.slice(1)) for (const y of sub(k)) steps.push({ row: y, lv: 2, after: y === k ? null : y.b });
      x = main;
    }
    steps.forEach((s, i) => { s.last = !steps.slice(i + 1).some(t => t.lv === 1); });
    const line = steps.filter(s => s.lv === 1), top = steps.reduce((a, s) => qrank(s.row.st) < qrank(a.row.st) ? s : a, steps[0]);
    const next = steps.find(s => qrank(s.row.st) === 2);
    return { root, steps, end: line[line.length - 1].row, head: top.row, next: next && next.row,
      name: qseries(steps.map(s => s.row.t)), g: qrank(top.row.st), i: Math.min(...steps.filter(s => qrank(s.row.st) === qrank(top.row.st)).map(s => at.get(s.row.n))) };
  };
  const slots = [];
  for (const r of rows) {
    if (placed.has(r.n)) continue;
    const root = (() => { let x = r; while (up(x)) x = up(x); return x; })();
    if (kids.get(root.n).length) {
      const c = chain(root);
      c.steps.forEach(s => placed.add(s.row.n));
      slots.push({ chain: c, g: c.g, i: c.i });
    } else { placed.add(r.n); slots.push({ row: r, g: qrank(r.st), i: at.get(r.n) }); }
  }
  slots.sort((a, b) => a.i - b.i).forEach(s => out[s.g].push(s));
  return out;
}
// The collapsed chain's one line: `#265 → #270 · Decouple (6 steps) · next: #266`.
const qline = c => `#${c.root.n} → #${c.end.n} · ${c.name} (${c.steps.length} steps)${c.next ? ` · next: #${c.next.n}` : ''}`;
// queue:end

// #367 the Queue tab, one repo at a time: [all] [repo n•] chips (tap, swipe, ←/→, wheel, mouse drag: #291's ways), the
// picked repo kept per phone (`queue:repo`); in it NEEDS YOU, IN PROGRESS, QUEUED (5, then `+ N more` in place),
// BACKLOG (#365); a chain is one row that opens in place. Runs after tidy() on the same root (a page, a frag).
const QSHOW = 5, GN = ['needs you', 'in progress', 'queued'];
let QPICK = get('queue:repo');
function qprep(root) {
  // Before graph(): each row's plain facts, in the server's order (graph() reorders and decorates them).
  root.querySelectorAll('details.q').forEach((d, i) => {
    if (d.dataset.qi) return;
    const s = d.querySelector('summary>span:nth-of-type(2)');
    Object.assign(d.dataset, { qi: i, st: s ? s.textContent.trim() : '', n: d.querySelector('summary>span').textContent.trim().slice(1) });
  });
}
function qtab(root) {
  const sec = root.id === 'queue' ? root : root.querySelector && root.querySelector('#queue');
  if (!sec) return;
  const cards = [...sec.querySelectorAll(':scope>.fc')];
  const chips = [];
  for (const fc of cards) {
    const f = fc.querySelector('.fn>b')?.textContent || '';
    fc.dataset.f = f;
    if (fc.classList.contains('off')) { chips.push({ f, n: 0, need: 0, k: fc.className }); continue; }
    if (fc.dataset.qn) { chips.push({ f, n: +fc.dataset.qn, need: +fc.dataset.qneed, k: fc.className }); continue; } // regrouped already
    const ds = [...fc.querySelectorAll(':scope>details.q')];
    const rows = ds.sort((a, b) => a.dataset.qi - b.dataset.qi).map(d => ({ n: +d.dataset.n, st: d.dataset.st, b: d.dataset.b ? +d.dataset.b : null,
      t: d.querySelector('.tl')?.textContent || '', s: d.dataset.a ? +d.dataset.a : null, d }));
    const gs = qgroups(rows), anchor = fc.querySelector(':scope>.done') || null;
    let pos = 0;
    gs.forEach((slots, g) => {
      if (!slots.length) return;
      const box = el('div', 'qg'), n = slots.length;
      box.dataset.g = g;
      box.append(Object.assign(el('p', 'qh'), { innerHTML: `<span>${GN[g]}</span><span>${n}</span>` }));
      let more = null;
      slots.forEach((s, k) => {
        const node = s.chain ? qchain(s.chain) : qrow(s.row.d, s.row);
        if (g === 2) { node.querySelector(':scope>summary .rl').textContent = ++pos; qmove(node, slots, k); }
        if (g === 2 && k === QSHOW && slots.length > QSHOW + 1) {
          more = el('details', 'qmore');
          const left = slots.slice(QSHOW).reduce((a, x) => a + (x.chain ? x.chain.steps.length : 1), 0);
          more.append(Object.assign(el('summary', ''), { innerHTML: `<span class=qmo>+ ${left} more</span><span class=qml>show the first ${QSHOW}</span>` }));
          box.append(more);
        }
        (more || box).append(node);
      });
      fc.insertBefore(box, anchor);
    });
    if (!fc.querySelector(':scope>.qbl')) fc.insertBefore(Object.assign(el('details', 'qbl'), { innerHTML: '<summary><span>backlog</span><span class=m>open issues not queued (#365)</span><i class=cv></i></summary><p class=m>#365 lists them here: search, filters, [queue N].</p>' }), anchor);
    const need = gs[0].reduce((a, s) => a + (s.chain ? s.chain.steps.filter(x => qrank(x.row.st) === 0).length : 1), 0);
    Object.assign(fc.dataset, { qn: rows.length, qneed: need });
    chips.push({ f, n: rows.length, need, k: fc.className });
  }
  // `all`: the rows that need you, from every repo, each with its repo's name.
  sec.querySelector(':scope>.qall')?.remove();
  const all = el('div', 'fc qall');
  all.dataset.f = '*';
  const need = cards.flatMap(fc => [...fc.querySelectorAll(':scope>.qg[data-g="0"]>details')].map(d => {
    const c = d.cloneNode(true);
    c.querySelector(':scope>summary .rl').textContent = '';
    c.dataset.f = fc.dataset.f;
    c.querySelector(':scope>summary .tl')?.insertAdjacentHTML('afterbegin', `<b class="qf ${[...fc.classList].find(x => x.startsWith('k-')) || ''}">${qhx(fc.dataset.f)}</b> · `);
    return c;
  }));
  all.innerHTML = `<div class=fh><span class=fn><b>needs you</b><span class=m>every repo</span></span></div>`;
  const box = el('div', 'qg');
  box.dataset.g = 0;
  box.append(...(need.length ? need : [el('p', 'm', 'nothing needs you')]));
  all.append(box);
  sec.prepend(all);
  const keys = ['*', ...chips.map(c => c.f)];
  if (!keys.includes(QPICK)) QPICK = chips.find(c => c.need)?.f || chips[0]?.f || '*';
  sec.querySelector(':scope>.qchips')?.remove();
  const bar = el('div', 'qchips');
  bar.setAttribute('role', 'tablist');
  bar.setAttribute('aria-label', 'repos');
  const total = chips.reduce((a, c) => a + c.need, 0);
  bar.innerHTML = `<button type=button class=qnv data-d=-1 aria-label="previous repo">‹</button>` +
    [{ f: '*', label: 'all', n: total, need: total, k: '' }, ...chips].map(c => `<button type=button role=tab class="qcp ${(c.k.match(/k-\w/) || [''])[0]}" data-f="${qhx(c.f)}"` +
      ` aria-selected=${c.f === QPICK}${c.need ? ` title="${c.need} need you"` : ''}><b>${qhx(c.label || c.f)}</b><span class=qn>${c.n}</span>${c.need ? '<i class=qd aria-label="needs you"></i>' : ''}</button>`).join('') +
    `<button type=button class=qnv data-d=1 aria-label="next repo">›</button>`;
  sec.prepend(bar);
  qshow(sec);
}
const qhx = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
// A row in the new layout: rail (position or chain elbow), its dot/sprite, #n, `in impl 1h 47m`, the title in one column.
function qrow(d, r) {
  d.classList.remove('d0', 'd1', 'd2');
  const sm = d.querySelector(':scope>summary'), gt = sm.querySelector('.gt'), dot = gt && gt.querySelector('b');
  if (gt) { gt.replaceChildren(el('i', 'rl'), dot); gt.querySelector('b')?.classList.remove('k'); }
  sm.querySelectorAll(':scope>span:nth-of-type(2) .m').forEach(t => /^after #/.test(t.textContent) && t.remove());
  const w = sm.querySelector(':scope>span:nth-of-type(2)'), st = w && w.firstElementChild, age = sm.querySelector(':scope>span.m');
  if (st && r.s != null && qrank(r.st) < 2) {
    const t = qwhen(r.st, r.s);
    if (t.startsWith('in ')) st.before('in ');
    st.after(el('span', 'qt', ' ' + qdur(r.s)));
  }
  if (qrank(r.st) === 2 && st && /^queued$/.test(st.textContent)) st.classList.add('qq');
  if (age) age.remove();
  return d;
}
// A chain: one row (`#265 → #270`, its most urgent step's state and time, `Decouple (6 steps) · next: #266`); it opens
// in place to its steps, each one level in (side branches two).
function qchain(c) {
  const d = el('details', 'q qch'), head = c.head.d;
  d.dataset.chain = c.root.n;
  d.dataset.act = head.dataset.act || 'idle'; // rows() redraws its sprite from this, as a row's
  const dot = head.querySelector(':scope>summary .gt>b')?.cloneNode(true) || el('b', 'm', '○');
  const hs = head.querySelector(':scope>summary>span:nth-of-type(2)>span')?.cloneNode(true);
  const s = el('summary', ''), tail = `\u00a0(${c.steps.length} steps)${c.next ? ` · next: #${c.next.n}` : ''}`;
  s.innerHTML = `<i class=gt><i class=rl></i></i><span>#${c.root.n}</span><span class=qcl><span class=m>→ #${c.end.n}</span></span><i class=cv aria-hidden=true></i>` +
    `<span class="tl qcs" title="${qhx(c.name + tail)}" style="--tail:${tail.length + 1}ch"><span class=qcn>${qhx(c.name)}</span>${qhx(tail)}</span>`;
  dot.classList.remove('k');
  s.querySelector('.gt').append(dot);
  if (hs && qrank(c.head.st) < 2) {
    const w = s.querySelector('.qcl');
    w.append(' · ');
    if (/^(impl|review|fix|update)/.test(c.head.st)) w.append('in ');
    w.append(hs, el('span', 'qt', ` ${c.head.s != null ? qdur(c.head.s) : ''}`));
    if (c.head !== c.root) w.append(el('span', 'm', ` #${c.head.n}`));
  }
  s.setAttribute('aria-label', qline(c));
  const box = el('div', 'qsteps');
  c.steps.forEach((x, i) => {
    const r = qrow(x.row.d, x.row);
    r.classList.add('st' + x.lv);
    r.classList.toggle('z', x.last);
    // a level-2 step whose own branch goes on below it keeps its second rail
    r.classList.toggle('zz', x.lv === 2 && !(c.steps[i + 1] && c.steps[i + 1].lv === 2));
    r.querySelectorAll(':scope>summary .tag.r').forEach(t => /^! #/.test(t.textContent) && t.remove());
    if (x.after) r.querySelector(':scope>summary>span:nth-of-type(2)').append(' ', el('span', 'm', 'after #' + x.after));
    r.querySelector(':scope>.cd .btns')?.remove(); // a step's order is its chain's: no ▲/▼
    box.append(r);
  });
  d.append(s, box);
  return d;
}
// ▲/▼ on a queued slot move it among the queued slots (a chain by its first step).
function qmove(node, slots, k) {
  const tmpl = document.querySelector('#queue details.q form:has([name=before],[name=after])') || node.querySelector('form:has([name=before],[name=after])');
  node.querySelector(':scope>.cd .btns')?.remove();
  if (!tmpl) return;
  const n = s => s.chain ? s.chain.root.n : s.row.n, me = n(slots[k]);
  const mv = (label, key, to) => { const f = tmpl.cloneNode(true), i = f.querySelector('[name=before],[name=after]'); i.name = key; i.value = to; f.querySelector('[name=issue]').value = me; const b = f.querySelector('button'); b.textContent = label; b.setAttribute('aria-label', `move #${me} ${key === 'before' ? 'up' : 'down'}`); return f; };
  const box = el('span', 'qmv');
  if (k > 0) box.append(mv('▲', 'before', n(slots[k - 1])));
  if (slots[k + 1]) box.append(mv('▼', 'after', n(slots[k + 1])));
  node.querySelector(':scope>summary').append(box);
}
function qshow(sec = $('#queue')) {
  sec.querySelectorAll(':scope>.fc').forEach(fc => fc.hidden = fc.dataset.f !== QPICK);
  sec.querySelectorAll('.qcp').forEach(b => b.setAttribute('aria-selected', b.dataset.f === QPICK));
}
function qgo(d, f) {
  const ks = [...$$('#queue .qcp')].map(b => b.dataset.f), i = ks.indexOf(QPICK);
  const to = f ?? ks[((i + d) % ks.length + ks.length) % ks.length];
  if (to === QPICK) return;
  QPICK = to;
  put('queue:repo', to);
  qshow();
  const fc = [...$$('#queue>.fc')].find(c => c.dataset.f === to);
  if (fc && !still.matches) fc.animate([{ transform: `translateX(${d < 0 ? -24 : 24}px)`, opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: 180, easing: 'ease-out' });
  $(`#queue .qcp[aria-selected=true]`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}
// The needs-you cards of the picked repo's rows open inside those rows (the stack above the tabs keeps the rest).
function qasks() {
  $$('#needs>.ask').forEach(a => {
    const m = /^(\S+) · #(\d+)\b/.exec(a.querySelector('.m')?.textContent || '');
    const rows = m ? [...$$(`#queue .fc[data-f="${m[1]}"] details.q[data-n="${m[2]}"], #queue .qall details.q[data-f="${m[1]}"][data-n="${m[2]}"]`)] : [];
    a.classList.toggle('inq', rows.length > 0);
    for (const r of rows) {
      const cd = r.querySelector(':scope>.cd');
      if (!cd || cd.querySelector(':scope>.ask')) continue;
      const q = a.querySelectorAll(':scope>p')[1]?.textContent;
      if (q && a.querySelector('[name=op][value=answer]')) { const t = r.querySelector(':scope>summary>.tl'), f = t.querySelector('.qf'); t.textContent = 'asks: ' + q; if (f) t.prepend(f, ' · '); t.title = t.textContent; t.classList.add('qa'); }
      cd.prepend(a.cloneNode(true));
    }
  });
}
document.addEventListener('click', e => {
  const b = e.target.closest('#queue .qcp, #queue .qnv');
  if (b) qgo(+(b.dataset.d || 0), b.dataset.f);
});
document.addEventListener('keydown', e => {
  if (document.documentElement.dataset.tab !== 'queue' || shown() || e.altKey || e.ctrlKey || e.metaKey || e.target.closest('input, textarea, select, [contenteditable]')) return;
  const k = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
  if (k) { e.preventDefault(); qgo(k); }
});
// A sideways swipe (touch events; a mouse drags by pointer events), 40 px and twice as far sideways as down, and a
// sideways wheel switch repos.
let qsw = null, qwAt = 0, qwX = 0, qwDone = false;
const QS = $('#queue');
QS.addEventListener('pointerdown', e => { if (e.pointerType !== 'mouse') return; qsw = e.button === 0 && !e.target.closest('input, textarea, select, button, a') ? { x: e.clientX, y: e.clientY, id: e.pointerId } : null; });
QS.addEventListener('pointerup', e => {
  if (e.pointerType !== 'mouse') return;
  const s = qsw; qsw = null;
  if (!s || e.pointerId !== s.id) return;
  const dx = e.clientX - s.x, dy = e.clientY - s.y;
  if (Math.abs(dx) > 40 && Math.abs(dx) > 2 * Math.abs(dy)) { e.preventDefault(); qgo(-Math.sign(dx)); }
});
// Touch listens on the page: a short repo leaves most of the screen below #queue.
const qon = e => document.documentElement.dataset.tab === 'queue' && !shown() && !e.target.closest('input, textarea, select, #needs, nav, header');
document.addEventListener('touchstart', e => { if (!qon(e)) return void (qsw = null); const t = e.touches[0]; qsw = { x: t.clientX, y: t.clientY, id: 'touch' }; }, { passive: true });
document.addEventListener('touchend', e => {
  const s = qsw, t = e.changedTouches[0]; qsw = null;
  if (!s || s.id !== 'touch') return;
  const dx = t.clientX - s.x, dy = t.clientY - s.y;
  if (Math.abs(dx) > 40 && Math.abs(dx) > 2 * Math.abs(dy)) qgo(-Math.sign(dx));
});
QS.addEventListener('wheel', e => {
  const x = e.deltaX || (e.shiftKey ? e.deltaY : 0), y = e.deltaX ? e.deltaY : e.shiftKey ? 0 : e.deltaY;
  if (Math.abs(x) <= Math.abs(y)) return;
  e.preventDefault();
  if (e.timeStamp - qwAt > 300) { qwX = 0; qwDone = false; }
  qwAt = e.timeStamp;
  qwX += x;
  if (!qwDone && Math.abs(qwX) > 40) { qwDone = true; qgo(Math.sign(qwX)); }
}, { passive: false });
// The tab bar's height as drawn (the home indicator, a larger text size): the last row clears it.
new ResizeObserver(() => document.documentElement.style.setProperty('--navh', $('nav').getBoundingClientRect().height + 'px')).observe($('nav'));
// tidy() gains two passes: qprep() first (the facts, before graph() reorders the rows: phase 2 calls it there; the mockup
// splices it in), then the regroup.
{
  const t0 = tidy;
  tidy = root => { t0(root); qtab(root); qasks(); };
  qtab(document);
  qasks();
}
