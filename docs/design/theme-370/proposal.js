// #370 phase 1: what phase 2 adds to serve.js (and two lines to herd.js), as one script the mockup loads after them.
// It reads only the theme's data (proposed/<theme>/theme.json, sprites.svg). Terminal has no `words` and
// `row_sprites: false`, so nothing here touches its board.
//
//  1. words: every label pairs the theme word with the plain one, `<b class=tw>on the line</b><small class=pw> · working</small>`;
//     a word the theme lacks shows the plain word alone (no `·`).
//  2. row loops: CSS only (proposal.css): the row sprite's .kf1/.kf2 swap by --o1/--o2, no node replaced.
//  3. Queue/Events sprites by kind: queued, needs, merged, fail (`row_sprites`), by `<use href>`, never a new node.
//  4. moments on the row: rowMoment(k, fleet, issue, seq), once per seq; today's #mo overlay when no row is in view.
//  5. empty states and the loading word; 6. the Host card's load mini with its level word.
(() => {
  const PLAIN = { needs: 'needs you', working: 'working', idle: 'idle', done: 'done', queued: 'queued', parked: 'parked', dead: 'dead',
    impl: 'build', fix: 'fix', rev: 'review', update: 'catch up', merged: 'merged', fail: 'FAIL' };
  const TABS = ['queue', 'lanes', 'host', 'events'];
  const W = () => (TT && TT.row_sprites !== false && TT.words) || {};
  const tw = (k, plain, rest = '') => {
    const w = W()[k];
    return w ? `<b class=tw>${esc(w)}</b><small class=pw> · ${esc(plain)}${esc(rest)}</small>` : esc(plain + rest);
  };
  // A label set once keeps its plain text in data-p, so a theme switch re-pairs it from the original.
  const relabel = (node, key, plain, rest = '') => {
    if (!node) return;
    if (node.dataset.p == null) node.dataset.p = node.textContent;
    const html = W()[key] ? tw(key, plain, rest) : esc(node.dataset.p);
    if (node.innerHTML !== html) node.innerHTML = html;
  };

  // ---- the extra symbols (herd.js theme() in phase 2: two lines beside `sym('hr-' + a, …)`) ----
  const NS = 'http://www.w3.org/2000/svg';
  let built = '';
  async function symbols() {
    const name = document.documentElement.dataset.theme;
    if (built === name || !TT) return;
    built = name;
    const defs = document.querySelector('svg.hdefs'), src = await fetch(`${R}themes/${name}/sprites.svg`).then(r => r.ok ? r.text() : '');
    const sheet = document.createElementNS(NS, 'svg');
    sheet.innerHTML = src;
    defs.querySelectorAll('symbol[data-370]').forEach(s => s.remove());
    const sym = (id, want, box) => {
      const g = want && sheet.querySelector('#' + CSS.escape(want))?.cloneNode(true);
      if (!g) return;
      const s = document.createElementNS(NS, 'symbol');
      Object.assign(s, { id });
      s.dataset[370] = '';
      s.setAttribute('viewBox', box);
      g.removeAttribute('id');
      s.append(g);
      defs.append(s);
    };
    for (const k of ['queued', 'needs', 'merged', 'fail']) sym('hr-' + k, TT.row_sprites && TT.row_sprites[k], '0 0 132 100');
    for (const l of ['calm', 'busy', 'held']) sym('hl-' + l, TT.load_scene && TT.load_scene.minis && TT.load_scene.minis[l], '0 0 48 16');
    words(document);
  }

  // ---- 1, 3, 5, 6: the labels on every tab ----
  function words(root) {
    const has = Object.keys(W()).length > 0;
    document.documentElement.classList.toggle('tw-on', has);
    // the tab's own line, first in its section: `the rail · events`
    for (const t of TABS) {
      // a swap's new tree (frag: tidy before morph) carries the line too, so the morph keeps it
      const s = root.id === t ? root : root === document ? document.getElementById(t) : null;
      if (!s) continue;
      let p = s.querySelector(':scope>.tsub');
      if (!has || !W()['tab-' + t]) { p?.remove(); continue; }
      if (!p) s.prepend(p = Object.assign(document.createElement('p'), { className: 'tsub' }));
      p.innerHTML = tw('tab-' + t, t);
    }
    // the needs-you cards: `the bell · needs you`
    root.querySelectorAll('#needs .ask>p:first-child>b, .ask>p:first-child>b').forEach(b => relabel(b, 'needs', 'needs you'));
    // Queue rows: the state word by role or state
    root.querySelectorAll('details.q>summary>span:nth-of-type(2)').forEach(o => {
      const s = o.querySelector(':scope>span') || o; // the coloured word inside (graph() wraps it)
      const p = s.dataset.p ?? s.textContent, m = p.match(/^(impl|review|fix|update)(\(\d+\))?(.*)$/);
      const role = m && { impl: 'impl', review: 'rev', fix: 'fix', update: 'update' }[m[1]];
      if (role) return relabel(s, role, PLAIN[role], (m[2] || '') + m[3]);
      if (/^queued/.test(p)) return relabel(s, 'queued', p);
      if (/needs-human|waiting/.test(p)) return relabel(s, 'needs', /needs-human/.test(p) ? 'needs you' : p.replace(/^⏸ /, ''));
      if (p === 'dead' || p === 'parked') return relabel(s, p, p);
    });
    root.querySelectorAll('details.done>summary>span').forEach(s => relabel(s, 'done', (s.dataset.p ?? s.textContent)));
    // Lanes: the state groups
    root.querySelectorAll('#lanes h2>span').forEach(s => {
      const [, sec, n] = (s.dataset.p ?? s.textContent).match(/^(\S+) (\(\d+\))$/) || [];
      const k = sec === 'blocked' ? 'needs' : sec;
      if (k && PLAIN[k]) relabel(s, k, PLAIN[k], ' ' + n);
    });
    // Events: the kind's theme word before the row's words, and its sprite
    root.querySelectorAll('details.er>summary').forEach(sum => {
      const sm = sum.querySelector('.sm'), g = sum.querySelector('.kg'), p = sm.dataset.p ?? sm.textContent;
      const k = /^merged/.test(p) ? 'merged' : /review FAIL/.test(p) ? 'fail' : /^review PASS/.test(p) ? 'rev' : /^(asks you|needs you)/.test(p) ? 'needs'
        : /^queue\b/.test(p) ? 'queued' : (p.match(/^(impl|rev|fix|update) started/) || [])[1];
      if (sm.dataset.p == null) sm.dataset.p = p;
      const html = k && W()[k] ? `<b class="tw ev">${esc(W()[k])}</b> ${esc(p)}` : esc(p);
      if (sm.innerHTML !== html) sm.innerHTML = html;
      if (k && g) use(g, k);
    });
    // Queue sprites by state: a queued issue, a needs-you one
    root.querySelectorAll('details.q').forEach(d => {
      const o = d.querySelector('summary>span:nth-of-type(2)'), s = o && (o.querySelector(':scope>span') || o), p = s && (s.dataset.p ?? s.textContent), g = d.querySelector('summary .gt>b');
      if (p && g) use(g, /^queued/.test(p) ? 'queued' : /needs-human|waiting/.test(p) ? 'needs' : null);
    });
    // empty states: the server's plain line gains its theme line
    root.querySelectorAll('p.m').forEach(p => {
      const t = p.dataset.p ?? p.textContent, k = { 'no open work': 'empty-queue', 'no agents': 'empty-lanes', 'no events': 'empty-events' }[t];
      if (!k) return;
      if (p.dataset.p == null) p.dataset.p = t;
      p.classList.toggle('empty', !!W()[k]);
      const html = W()[k] ? `<b class=tw>${esc(W()[k])}</b><small class=pw>${esc(t)}</small>` : esc(t);
      if (p.innerHTML !== html) p.innerHTML = html;
    });
    // the loading overlay's word (its sprite is #337's)
    const ld = document.getElementById('ld');
    if (ld) {
      let q = ld.querySelector('.ldw');
      if (!q) ld.append(q = Object.assign(document.createElement('p'), { className: 'ldw' }));
      q.innerHTML = W().loading ? tw('loading', 'loading') : '';
    }
    host(root);
  }
  // A row sprite shows `hr-<k>` while the theme has it, else its action's; the same <use>, its href changed.
  function use(g, k) {
    const u = g.querySelector('use');
    if (!u) return;
    if (u.dataset.a == null) u.dataset.a = u.getAttribute('href');
    const want = k && document.getElementById('hr-' + k) ? '#hr-' + k : u.dataset.a;
    if (u.getAttribute('href') !== want) u.setAttribute('href', want);
  }
  // 6: each host card's header: its load mini (48 × 16, two frames) and the level word, `simmer · calm`.
  function host(root) {
    root.querySelectorAll('article.hcd').forEach(c => {
      const h = c.querySelector('.hhd .hnm'), had = h.querySelector('.lsm');
      const ls = TT && TT.load_scene, r = c.querySelector('details.rw[data-k=load]'), v = r && r.querySelector('summary .w')?.textContent.match(/([\d.]+) \/ ([\d.]+)/);
      if (!ls || !ls.minis || !v || !document.getElementById('hl-calm')) return void had?.remove();
      const x = v[1] / v[2], l = x >= 1 ? 'held' : x >= 0.5 ? 'busy' : 'calm', word = (ls.levels || {})[l] || l;
      if (had && had.dataset.l === l + word) return; // the same level: the mini (and its loop) stays
      had?.remove();
      h.insertAdjacentHTML('beforeend', `<span class=lsm data-l="${esc(l + word)}" role=img aria-label="load: ${esc(word)} (${l})"><svg viewBox="0 0 48 16" aria-hidden=true><use href="#hl-${l}"/></svg><b class=tw>${esc(word)}</b><small class=pw> · ${l}</small></span>`);
    });
  }

  // ---- 4: moments on the row ----
  // /live's `ev` gains the event's fleet and issue (serve.rs: `"f": e.fleet, "n": e.issue` beside `"k"`); the row of
  // that issue on the visible tab plays the theme's moment once (keyed by seq), else today's #mo overlay does. A swap
  // that rebuilds the row mid-moment re-applies it with a negative delay, so it never restarts.
  const MO = new Map(), PLAYED = new Set(), MS = 2400;
  const TAG = { merged: 'merged', fail: 'fail', 'needs-you': 'needs' };
  const KIND = { merged: /^merged/, fail: /review FAIL/, 'needs-you': /^(asks you|needs you)/ };
  function target(fleet, n, k) {
    const tab = document.documentElement.dataset.tab;
    const hit = a => a && a.textContent === '#' + n;
    if (tab === 'queue') return [...document.querySelectorAll('#queue details.q')].find(d => hit(d.querySelector('summary a')) && d.closest(`[data-fleet="${fleet}"], #queue`));
    // Events: the newest row of that issue and kind (the event's own row), else its newest row
    if (tab === 'events') {
      const all = [...document.querySelectorAll('#events details.er')].filter(d => d.dataset.f === fleet && hit(d.querySelector('summary .n')) && d.offsetParent);
      return all.find(d => KIND[k]?.test(d.querySelector('.sm').dataset.p ?? '')) || all[0];
    }
    if (tab === 'lanes') return [...document.querySelectorAll('#lanes .lane')].find(l => (l.dataset.agent || '').startsWith(fleet + '-') && (l.dataset.agent || '').endsWith('-' + n));
    return null;
  }
  function play(row, k, t0) {
    const sum = row.querySelector('summary') || row.querySelector('.lr') || row, g = sum.querySelector('.rsg, .kg, .av');
    sum.dataset.mo = k;
    sum.style.setProperty('--mo-d', `${-(Date.now() - t0)}ms`);
    if (g && g.querySelector('use')) use(g, TAG[k]);
    let tag = sum.querySelector('.mot');
    if (!tag && W()[TAG[k]]) {
      sum.insertAdjacentHTML('beforeend', `<i class=mot>${esc(W()[TAG[k]])}${k === 'merged' ? '!' : ''}</i>`);
    }
  }
  function end(row) {
    const sum = row.querySelector('summary') || row.querySelector('.lr') || row;
    delete sum.dataset.mo;
    sum.querySelector('.mot')?.remove();
    words(row);
  }
  window.rowMoment = (k, fleet, n, seq) => {
    if (PLAYED.has(seq)) return 'again';
    PLAYED.add(seq);
    if (!TT || !TT.moments || !TT.moments[k]) return 'none';
    const row = n != null && target(fleet, n, k);
    if (!row) return moment(k), 'overlay';
    const m = { k, fleet, n, t0: Date.now() };
    MO.set(seq, m);
    play(row, k, m.t0);
    setTimeout(() => { MO.delete(seq); const r = target(fleet, n, k); if (r) end(r); }, MS);
    return 'row';
  };
  const again = () => MO.forEach(m => { const r = target(m.fleet, m.n, m.k); if (r && !(r.querySelector('summary') || r).dataset.mo) play(r, m.k, m.t0); });

  // serve.js's rows(root) runs after every swap and theme load: the labels and any running moment follow it.
  const base = window.rows;
  window.rows = root => { const r = base(root); words(root); again(); return r; };
  // frag(): tidy(new) runs rows() before morph(old, new); the morph drops a running moment's marks, so it resumes after.
  const f0 = window.frag;
  window.frag = (id, html) => { const r = f0(id, html); again(); return r; };
  const ready = setInterval(() => { if (TT) { clearInterval(ready); symbols(); } }, 50);
  new MutationObserver(() => { if (TT && built !== document.documentElement.dataset.theme) symbols(); }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
})();
