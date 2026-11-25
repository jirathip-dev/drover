// #1786 design fixture: the epic blocks, built only from block kinds the screens already have (section, list, row,
// badge, figure, card + text), patched into the demo export's own screen JSON. Loaded by shoot.mjs (the page) and
// term.html (the terminal Board), so both draw the same tree.
(function () {
  const ago = (now, m) => new Date(Date.parse(now) - m * 60000).toISOString().replace(/\.\d+Z$/, 'Z');
  const sheet = (f, n) => ({ sheet: `/screens/issue?repo=${f}&issue=${n}` });
  const bar = (k, n) => '▰'.repeat(k) + '▱'.repeat(n - k);

  function child(f, n, title, st, now) {
    const row = { type: 'row', id: `ep-${f}-${n}`, indent: 1, title: `#${n} ${title}`, tap: sheet(f, n) };
    if (st.merged) return { ...row, figure: { state: 'done', act: 'done', issue: n }, subtitle: `merged · PR #${st.pr}`, value: { since: ago(now, st.merged) } };
    if (st.dropped) return { ...row, figure: { state: 'idle', act: 'idle', issue: n }, badge: { text: 'dropped', tone: '' }, subtitle: 'closed unmerged · not counted', value: { since: ago(now, st.dropped) } };
    if (st.queued) return { ...row, figure: { state: 'idle', act: 'idle', issue: n }, subtitle: st.queued, value: { since: ago(now, st.age) } };
    return { ...row, figure: { state: 'working', act: st.act, grow: st.grow, issue: n }, badge: { text: st.badge, tone: 'working' }, subtitle: st.sub, value: { since: ago(now, st.age) } };
  }

  function head(f, n, title, kids, now, age, folded) {
    const live = kids.filter(k => !k.st.dropped), k = live.filter(c => c.st.merged).length, dropped = kids.length - live.length;
    const words = [bar(k, live.length)];
    const busy = live.filter(c => !c.st.merged && !c.st.queued).length, queued = live.filter(c => c.st.queued).length;
    if (busy) words.push(`${busy} in progress`);
    if (queued) words.push(`${queued} queued`);
    if (dropped) words.push(`${dropped} dropped`);
    const rows = [{ type: 'row', id: `epic-${f}-${n}`, title: `#${n} ${title}`, badge: { text: `${k}/${live.length} merged`, tone: 'done' }, subtitle: words.join(' · '), tap: sheet(f, n), value: { since: ago(now, age) } }];
    if (folded) rows.push({ type: 'row', id: `epic-fold-${f}-${n}`, style: 'link', indent: 1, title: `▸ ${kids.length} children of #${n}`, tap: { open: `/screens/queue?unfold=${f}-${n}` } });
    else {
      rows.push(...kids.map(c => child(f, c.n, c.title, c.st, now)));
      rows.push({ type: 'row', id: `epic-fold-${f}-${n}`, style: 'link', indent: 1, title: `▾ fold #${n}'s children`, tap: { open: `/screens/queue?fold=${f}-${n}` } });
    }
    return rows;
  }

  const API = [
    { n: 11, title: 'Refunds endpoint', st: { merged: 2880, pr: 26 } },
    { n: 12, title: 'Currency field on /v1/payments', st: { merged: 1500, pr: 28 } },
    { n: 13, title: 'Partial captures', st: { dropped: 1440 } },
    { n: 15, title: 'Cursor pagination for /v1/orders', st: { act: 'rev', grow: 'review', badge: '▲ [◆ rev · r1] ◆ ◫', sub: 'review 1 · PR #33 · CI ✓', age: 70 } },
    { n: 17, title: 'Idempotency keys on POST /v1/payments', st: { act: 'impl', grow: 'impl', badge: '[▲ impl] ◆ ◆ ◫', sub: 'impl ▸ working · acme-api-impl-17', age: 86 } },
  ];
  const WEB = [
    { n: 41, title: 'Settings shell', st: { merged: 600, pr: 52 } },
    { n: 42, title: 'Theme pane', st: { queued: 'queued · 3rd', age: 300 } },
    { n: 43, title: 'Account pane', st: { queued: 'queued · 4th', age: 300 } },
  ];

  function epics(f, list) {
    return { type: 'section', id: `epics-${f}`, header: `epics (${list.length})`, blocks: [{ type: 'list', id: `epics-list-${f}`, rows: list.flat() }] };
  }

  /** The Queue screen with each fleet card's open epics as its first section. */
  function queue(q, now, { fold } = {}) {
    const out = JSON.parse(JSON.stringify(q));
    for (const c of out.blocks) {
      if (c.id === 'fleet-acme-api') c.blocks.unshift(epics('acme-api', [head('acme-api', 24, 'Payments API v2', API, now, 4320, false)]));
      if (c.id === 'fleet-acme-web') c.blocks.unshift(epics('acme-web', [head('acme-web', 40, 'Settings redesign', WEB, now, 1440, fold !== false)]));
    }
    return out;
  }

  const LANDED = 'acme-api #24 Payments API v2 landed: 4 issues\n\n' +
    '- ✓ #11 Refunds endpoint · PR #26\n- ✓ #12 Currency field on /v1/payments · PR #28\n' +
    '- ✓ #15 Cursor pagination for /v1/orders · PR #33\n- ✓ #17 Idempotency keys on POST /v1/payments · PR #35\n' +
    '- dropped: #13 Partial captures (closed unmerged)\n\n#24 closed with one comment linking each PR.';

  /** A repo thread with the epic's one `merged` row at its end (the row cooee and the page draw; its push below). */
  function thread(t, now) {
    const out = JSON.parse(JSON.stringify(t));
    out.blocks.push({ type: 'card', id: 'm-90', kind: 'sys', meta: { since: now }, blocks: [{ type: 'text', id: 'm-90-t', markdown: LANDED }] });
    return out;
  }

  const PUSH = { title: 'acme-api #24 landed', body: 'Payments API v2: 4 issues merged, 1 dropped', tag: 'merged-epic-acme-api-24' };
  globalThis.EPIC = { queue, thread, PUSH, LANDED };
})();
