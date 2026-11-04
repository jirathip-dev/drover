// #63 gate 2 (docs/ui.md), shipped only in the `serve --export --demo` page: `?audit=<view>` sets up one view
// for scripts/ui-audit.sh; `&check=1` then measures it at 390 px and writes {"view","controls","outliers"} into
// <pre id=audit>. Rows (summary, lane rows, horses) are 44 px tap rows, not 28 px controls.
(() => {
  const V = DBG.get('audit'); // serve.js read the query before it tidied the URL
  if (!V) return;
  const hp = n => H.querySelectorAll('.hp')[n];
  // #68: each inline control's failure path with a canned long reply (the old 401 pair text, a 140-char refusal,
  // a send error); measured after each step (`mid`), then with the rest at the end.
  const F = window.fetch, early = [], wait = () => new Promise(r => setTimeout(r, 300));
  const LONG = {
    pair: 'pair this device: tap Pair this phone, then `herdsman serve approve <code>` on the Mac',
    sub: 'push subscribe refused: the endpoint https://web.push.apple.com/QGx2c3Rlc3Qtc3Vic2NyaXB0aW9uLWVuZHBvaW50LWlk is not https or not a known push service',
    kill: 'REFUSED UNSAFE_CLEAN: acme-web-impl-48-contrast-audit-for-every-settings-pane-abxx has 3 unpushed commits and a dirty tree; push them first',
    dry: 'dry run; would push 3 commits and 2 dirty files to wip/acme-web-impl-48-contrast-audit-for-every-settings-pane-abxx-20261031-1943, then close pane w1:p5',
    send: 'error: herdr pane send w1:p5 failed: pane closed while sending (exit 1); the message was not delivered, nothing was typed into the agent',
  };
  const canned = async (status, text, then) => { window.fetch = (u, o) => o && o.method === 'POST' ? Promise.resolve(new Response(text, { status })) : F(u, o); then(); await wait(); };
  const mid = what => early.push(...measure().outliers.map(o => `${what}: ${o}`));
  // #234 gate 2: the Host groups in their fixed order (HOSTS, FLEET DISK, SERVICES, FLEETS, ALERTS), main's card first;
  // one tab is active, its label on the accent fill.
  const ranked = () => {
    const t = [...$$('#host>.grp .gn')].map(g => g.textContent).join(' ');
    if (!/^HOSTS FLEET DISK SERVICES (FLEETS )?ALERTS$/.test(t)) early.push(`Host groups not in order: ${t}`);
    if (!$('#host .hcd .tag')?.closest('.hcd').matches(':first-child')) early.push('the main host is not the first card');
    const on = $$('nav a.on'), probe = B.appendChild(Object.assign(document.createElement('i'), { style: 'color:var(--l)' }));
    const accent = getComputedStyle(probe).color;
    probe.remove();
    if (on.length !== 1) early.push(`${on.length} active tabs, not 1`);
    else if (getComputedStyle(on[0].firstElementChild).backgroundColor !== accent) early.push(`the active tab's fill is ${getComputedStyle(on[0].firstElementChild).backgroundColor}, not the accent ${accent}`);
    // #183: every Host label is whole on one line, ≥ 1ch before its value, and every value starts at one x (groups
    // opened for the measure, then put back).
    const gs = [...$$('#host>.grp')], was = gs.map(g => g.open), xs = new Map();
    gs.forEach(g => { g.open = true; });
    for (const k of $$('#host .k')) {
      const g = document.createRange(), v = k.nextElementSibling, ch = k.parentNode.appendChild(Object.assign(document.createElement('i'), { style: 'position:absolute;width:1ch' }));
      g.selectNodeContents(k);
      const lines = new Set([...g.getClientRects()].map(r => Math.round(r.top))).size, end = g.getBoundingClientRect().right, x = v.getBoundingClientRect().left, w = ch.getBoundingClientRect().width;
      ch.remove();
      if (lines !== 1 || k.scrollWidth > k.clientWidth) early.push(`Host label "${k.textContent}": ${lines} lines, ${k.scrollWidth}px of text in ${k.clientWidth}px`);
      if (x - end < w - 0.5) early.push(`Host label "${k.textContent}": its value ${(x - end).toFixed(1)}px after it, under 1ch (${w.toFixed(1)}px)`);
      xs.set(k.textContent, Math.round(x));
    }
    if (new Set(xs.values()).size > 1) early.push(`Host values start at different x: ${[...xs].map(([k, x]) => `${k} ${x}`).join(', ')}`);
    gs.forEach((g, i) => { g.open = was[i]; });
  };
  // #173: a fleet's [config] sheet, scrolled to its Pipeline group.
  const pipe = async fleet => {
    tab('queue');
    const b = [...$$('#queue .cfg')].find(b => b.dataset.f === fleet);
    if (!b) return early.push(`no [config] button for ${fleet} in the demo`);
    await config(b);
    const g = $('#sheet .pp');
    if (!g?.querySelector(`.pg[aria-label^="${fleet} "]`)) return early.push(`no pipeline graph for ${fleet} in the demo`);
    g.scrollIntoView();
  };
  const VIEWS = {
    queue: () => tab('queue'),
    // #281 gate 2: the Queue and Events rows in the shot (no needs cards above them, in the check too), so their sprites' pixels are checked.
    'queue-rows': () => { tab('queue'); $('#needs').hidden = true; },
    'events-rows': () => { tab('events'); $('#needs').hidden = true; },
    'queue-open': () => { tab('queue'); const d = $('#queue details.q'); d.open = true; d.scrollIntoView(); },
    // #205: acme-web #38's open card lists its 3 lanes newest first: one line each (ellipsis), a 44 px row; gone ones are no link.
    // #218: renamed to real-length names (≥ 30 chars), moved to the top: the name ellipsises inside the card, the age stays
    // in view, the sections read STATUS, LINKS, LANES (3); a tap on a lane no Lanes row lists opens the sheet by its name.
    'queue-lanes': async () => {
      tab('queue');
      const d = [...$$('#queue details.q')].find(d => d.querySelector('summary a')?.textContent === '#38');
      if (!d) return early.push('no acme-web #38 row in the demo');
      $('#queue').prepend(d);
      if (!DBG.get('check')) $('#needs').hidden = true;
      d.open = true;
      const ls = [...d.querySelectorAll('.qls>.ql')], long = ['herdsman-impl-157-queue-card-lane-overflow', 'acme-rev-2940-r1-settings-contrast-audit', 'herdsman-impl-205-queue-card-lanes-newest-first'];
      if (ls.length !== 3) early.push(`#38 lists ${ls.length} lanes, not 3`);
      ls.forEach((l, i) => { l.querySelector('.qn').textContent = long[i]; if (l.dataset.agent) l.dataset.agent = long[i]; });
      const hs = [...d.querySelectorAll('.cd>.ch')].map(h => h.textContent).join(', ');
      if (hs !== 'STATUS, LINKS, LANES (3)') early.push(`#38 sections "${hs}", not STATUS, LINKS, LANES (3)`);
      for (const l of ls) {
        const n = l.querySelector('.qn'), a = l.querySelector('.qa'), r = l.getBoundingClientRect();
        if (r.height !== 44 || getComputedStyle(n).textOverflow !== 'ellipsis' || n.scrollWidth <= n.clientWidth) early.push(`lane "${l.textContent}": ${r.height}px, not one 44 px line, its name cut by an ellipsis`);
        if (a.getBoundingClientRect().right > r.right + 1 || a.scrollWidth > a.clientWidth) early.push(`lane "${l.textContent}": its age pushed out of the line`);
      }
      if (ls.some(l => l.textContent.includes(' gone ') && l.matches('a:not([data-gone])'))) early.push('a gone lane is a link');
      // The tap: the Lanes tab never lists this name, its transcript read answers; the sheet opens on it.
      const q = ls.find(l => l.matches('a')), name = q?.dataset.agent;
      window.fetch = u => String(u).includes(encodeURIComponent(name)) ? Promise.resolve(new Response('{"harness":"claude","lines":[],"live":0}')) : F(u);
      q?.click();
      await wait();
      window.fetch = F;
      if (!shown() || $('.hd b').textContent !== name) early.push(`tap on ${name}: sheet ${shown() ? 'shows ' + $('.hd b').textContent : 'not opened'}`);
      close();
      await wait();
    },
    // #139: the demo's acme-web #46 row, its ask decided by default, re-answered through [change] (3 long choices),
    // opened and moved to the top (a scrolled view shoots blank).
    'queue-change': () => {
      tab('queue');
      const d = [...$$('#queue details.q')].find(d => d.querySelector('[name=choice]'));
      if (!d) return early.push('no queue row with a [change] form in the demo');
      d.open = true;
      $('#queue').prepend(d);
      if (!DBG.get('check')) $('#needs').hidden = true; // the shot shows the row; the check keeps the needs cards
    },
    // #92: the demo's `driver = "herdsman"` fleet (docs): its card with the `drive · …` line and [drive now], moved to the
    // top (a scrolled view shoots blank).
    'queue-drive': () => {
      tab('queue');
      $('#queue').prepend([...$$('#queue .fc')].find(c => c.querySelector('.dp li details'))); // the fleet with a pass, not `drive · no pass yet`
    },
    // #162: the [config] sheet of the fleet that needs you (acme-api): every group, the role pickers.
    config: () => { tab('queue'); config($('#queue .cfg')); },
    // #166: the driven fleet's result line in its 3 colours: the demo's amber pass (open, an action's code open), then
    // green and red copies under it, its card moved to the top; each must read in its colour.
    'queue-result': () => {
      tab('queue');
      const d = [...$$('#queue details.dp')].find(d => d.querySelector('li details'));
      if (!d) return early.push('no drive result line in the demo');
      const copy = (tone, text) => { const c = d.cloneNode(true); c.className = 'dp ' + tone; c.querySelector('summary>span').textContent = text; return c; };
      const all = [d, copy('g', 'drive · 4m ago · started 1 · merged 1'), copy('r', 'drive · 2m ago · started 0 · merged 0 · 1 failed')];
      d.after(...all.slice(1));
      $('#queue').prepend(d.closest('.fc'));
      d.open = true;
      d.querySelector('li details').open = true;
      if (!DBG.get('check')) $('#needs').hidden = true; // the shot shows the lines; the check keeps the needs cards
      for (const c of all) {
        const tone = c.classList[1], probe = B.appendChild(Object.assign(document.createElement('i'), { className: tone })), want = getComputedStyle(probe).color;
        probe.remove();
        if (getComputedStyle(c.querySelector('summary')).color !== want) early.push(`.dp.${tone} reads ${getComputedStyle(c.querySelector('summary')).color}, not ${want}`);
      }
    },
    // #173: the Pipeline group of a fleet's [config] sheet: the built-in one (acme-web), then acme-api's custom
    // impl → rev → sec → gates.
    pipeline: () => pipe('acme-web'),
    'pipeline-custom': () => pipe('acme-api'),
    lanes: () => tab('lanes'),
    // #386: the [chat] tab's thread list (the needs-you stack folded with [later all]), and one thread open from its
    // first message, with the choice rows, images, a file row and the composer (scrolled to the top: a scrolled view
    // shoots blank); at 1440 the list and the thread side by side.
    chat: () => { tab('chat'); chatOpen(''); $('.lall')?.click(); },
    'chat-thread': () => { tab('chat'); chatOpen('t4'); $('.lall')?.click(); scrollTo({ top: 0, behavior: 'instant' }); },
    // #397: a start error on the [chat] tab, as `thread say` failed on the mini.
    'chat-error': () => { tab('chat'); chatOpen(''); $('.lall')?.click(); scrollTo({ top: 0, behavior: 'instant' }); toast(ERR); },
    // #276: long `now:` lines as the server emits them (one-line classes, full text in title): plain, Claude's spinner word
    // + a long action + its time, and an idle row with no spinner, drawn by a /live redraw of the Lanes tab, then a
    // second one: one ellipsised line in each mode, nothing past the row.
    'lanes-now': async () => {
      tab('lanes');
      const sp = (...p) => p.map((x, i) => `<span class=${i < 2 ? 'el' : 't'}>${esc(x)}</span>`).join('<i> · </i>');
      const L = [
        sp('now: running git fetch origin && git merge origin/staging 2>&1 | tail -5; just --list; cat ci/cap.toml'),
        sp('✻ Simmering…', 'running git merge origin/staging +2 cd ~/w/issue-2684; S=/tmp/s; FILES=$(git diff --name-only | rg -v x)', '4m'),
        sp('✻ Herding…', 'editing src/a/very/long/path/to/some/deeply/nested/module/that/runs/past/the/edge.rs'),
        sp('now: running cd ~/w/issue-2684; S=/tmp/s; FILES=$(git diff --name-only | rg -v \'^docs/\'); cargo fmt --check $FILES')];
      const redraw = n => {
        const c = $('#lanes').cloneNode(true);
        [...c.querySelectorAll('.lane .act')].slice(0, 5).forEach((a, i) => {
          a.outerHTML = i === 4 ? '<span class=act></span>' : `<span class="act nw el now" title="full">${L[(i + n) % 4]}</span>`;
        });
        frag('lanes', c.innerHTML);
      };
      const one = what => $$('.lane .act.now').forEach(a => {
        // Lines by box height (a Range's rects mix the span boxes and their text runs, whose tops differ on one line).
        const r = a.getBoundingClientRect(), lines = Math.round(r.height / parseFloat(getComputedStyle(a).lineHeight));
        const cut = [...a.querySelectorAll('.el')].some(e => getComputedStyle(e).textOverflow !== 'ellipsis' || e.getBoundingClientRect().right > r.right + 1);
        if (lines !== 1 || cut || r.right > a.closest('.lane').getBoundingClientRect().right + 1) early.push(`${what}: now-line "${a.textContent.slice(0, 30)}…" ${lines} lines, right ${r.right}`);
        const [w, x] = a.querySelectorAll('.el');
        if (x && w.scrollWidth > w.clientWidth + 1 && x.clientWidth > 0) early.push(`${what}: the word "${w.textContent}" cut before the action`);
      });
      for (const n of [0, 1]) {
        redraw(n);
        for (const m of TM) { mode(m); await wait(); one(`after /live ${n + 1}, ${m}`); }
      }
      if ($$('.lane .act.now').length < 4) early.push('no long now-lines drawn');
    },
    sheet: async () => { // the longest agent name, then kill refused, kill's dry run, clean refused, a send error
      tab('lanes');
      open([...$$('.lane')].sort((a, b) => b.dataset.agent.length - a.dataset.agent.length)[0]);
      await wait();
      await canned(400, LONG.kill, () => run('kill'));
      mid('kill refused');
      await canned(200, LONG.dry, () => run('kill'));
      mid('kill dry run');
      $('.cf').hidden = true;
      say('', true);
      await canned(400, LONG.kill, cleanable);
      mid('clean refused');
      await canned(502, LONG.send, () => $('.reply').requestSubmit());
    },
    // #260: the same sheet with [full conversation] on (every tool run); `sheet` itself is the default view.
    'sheet-full': async () => {
      tab('lanes');
      open([...$$('.lane')].sort((a, b) => b.dataset.agent.length - a.dataset.agent.length)[0]);
      await wait();
      $('.fv').click();
      await wait();
      if ($('.fv').ariaPressed !== 'true' || !$('.chat>.now .blk.run')) early.push('[full conversation]: not pressed, or no tool run shown');
    },
    // #83: the demo's parked lane (no pane, its work on PR #15): the card's parked line, [clean] disabled with its
    // reason in p.st, [stop]/[kill] hidden, and nothing in the sheet in the red (danger) colour.
    'sheet-parked': async () => {
      tab('lanes');
      const row = $('.lane[data-agent="docs-impl-7"]');
      if (!row) return early.push('no parked lane docs-impl-7 in the demo');
      row.scrollIntoView({ block: 'start', behavior: 'instant' });
      open(row);
      await wait();
      await canned(409, 'REFUSED UNSAFE_CLEAN: docs-impl-7: only after merge (PR #15 open)', cleanable);
      const probe = S.appendChild(Object.assign(document.createElement('i'), { className: 'r' })), red = getComputedStyle(probe).color;
      probe.remove();
      if (st.textContent !== 'clean: only after merge (PR #15 open)') early.push(`p.st reads "${st.textContent}", not clean's reason`);
      if (!$('.top').textContent.includes('parked · work on PR #15')) early.push('the card has no `parked · work on PR #15` line');
      for (const op of ['stop', 'kill']) if (seen($(`[data-op=${op}]`))) early.push(`[${op}] shown on a lane with no pane`);
      if (!$('[data-op=clean]').disabled) early.push('[clean] enabled before merge');
      if ($('.chat>.now').textContent !== 'no pane: the transcript closed with it') early.push(`chat reads "${$('.chat>.now').textContent.slice(0, 40)}"`);
      for (const e of [S, ...S.querySelectorAll('*')].filter(seen)) if (e.matches('.r') || getComputedStyle(e).color === red) early.push(`red in the sheet: ${e.outerHTML.slice(0, 60)}`);
    },
    // #95: the demo's Others agent (not a lane): only [close pane], the reason in p.st, nothing in the red colour.
    'sheet-other': async () => {
      tab('lanes');
      const row = $('.lane[data-role=other]');
      if (!row) return early.push('no Others agent in the demo');
      open(row);
      await wait();
      const probe = S.appendChild(Object.assign(document.createElement('i'), { className: 'r' })), red = getComputedStyle(probe).color;
      probe.remove();
      if (st.textContent !== 'not a herdsman lane: herdsman can only close its pane') early.push(`p.st reads "${st.textContent}"`);
      for (const op of ['stop', 'kill', 'clean']) if (seen($(`[data-op=${op}]`))) early.push(`[${op}] shown on an Others agent`);
      if (!seen($('[data-op=close]'))) early.push('no [close pane] on an Others agent');
      for (const e of [S, ...S.querySelectorAll('*')].filter(seen)) if (e.matches('.r') || getComputedStyle(e).color === red) early.push(`red in the sheet: ${e.outerHTML.slice(0, 60)}`);
    },
    // #70: the handle is ≥ 56 px; a −200 px pointer drag moves the sheet mid-drag, release snaps to a detent,
    // and close leaves it shown, sliding off-screen, for a frame before `hidden` (the slide exists). Virtual time
    // (--virtual-time-budget) runs no frames and freezes transitions: rAF is a 16 ms timer here, and the slide is
    // read as its target transform while `.out` is on.
    'sheet-drag': async () => {
      window.requestAnimationFrame = f => setTimeout(() => f(performance.now()), 16);
      tab('lanes');
      open($('.lane'));
      await wait();
      const r = hd.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2, frame = () => new Promise(f => requestAnimationFrame(() => requestAnimationFrame(f)));
      const pe = (type, y) => hd.dispatchEvent(new PointerEvent(type, { pointerId: 1, pointerType: 'touch', isPrimary: true, clientX: x, clientY: y, bubbles: true }));
      const tz = () => new DOMMatrix(getComputedStyle(sh).transform).m42, aim = () => new DOMMatrix(sh.style.transform).m42, ds = ['full', 'mid', 'half'].map(off), t0 = aim();
      if (hd.offsetHeight < 56) dragged.push(`handle ${hd.offsetHeight}px tall, under 56`);
      if (Math.abs(t0 - ds[1]) > 1) dragged.push(`opened at ${t0 | 0}px, not the 78 % detent ${ds[1] | 0}`);
      pe('pointerdown', y);
      for (let i = 1; i <= 4; i++) {
        pe('pointermove', y - 50 * i);
        await frame();
        if (i === 2 && Math.abs(tz() - (t0 - 100)) > 1) dragged.push(`mid-drag (−100 px) the sheet is at ${tz() | 0}px, not ${t0 - 100 | 0}`);
      }
      pe('pointerup', y - 200);
      await wait();
      if (!ds.some(o => Math.abs(o - aim()) < 1)) dragged.push(`released at ${aim() | 0}px, not a detent (${ds.map(o => o | 0)})`);
      close();
      await frame();
      const slide = getComputedStyle(sh).transitionDuration;
      if (!shown() || !S.matches('.shut') || aim() < sh.offsetHeight - 1 || slide !== '0.2s') dragged.push(`a frame after close: shown ${shown()}, sliding to ${aim() | 0}px of ${sh.offsetHeight} over ${slide}: no slide`);
      await wait();
      if (shown()) dragged.push('300 ms after close the sheet is still shown');
      if (!DBG.get('check')) { open($('.lane')); S.classList.add('drag'); snap('half'); } // the screenshot: the sheet at half, unanimated
    },
    host: async () => { // the live Host tab's [enable alerts] (the export leaves it out) as the Home Screen app shows it: an error, then unpaired
      tab('host');
      ranked();
      const push = Object.assign(document.createElement('button'), { id: 'push', type: 'button', textContent: 'Enable alerts' });
      push.dataset.key = 'AA';
      const al = [...$$('#host>.grp')].find(g => g.querySelector('.gn').textContent === 'ALERTS');
      al.open = true;
      al.append(push);
      tidy(al);
      Object.defineProperty(window, 'Notification', { value: { requestPermission: async () => 'granted' } });
      const sub = { toJSON: () => ({ endpoint: 'e', keys: { p256dh: 'p', auth: 'a' } }) };
      Object.defineProperty(navigator, 'serviceWorker', { value: { ready: Promise.resolve({ pushManager: { subscribe: async () => sub } }) } });
      const st = () => ($('#push + p.st') || {}).textContent;
      await canned(400, LONG.sub, () => $('#push').click());
      if (st() !== LONG.sub) early.push(`alerts error: p.st reads "${st()}", not the reply`);
      mid('alerts error');
      await canned(401, LONG.pair, () => $('#push').click());
      if (!$('#push').disabled || st() !== 'pair this phone first') early.push(`unpaired: [enable alerts] disabled ${$('#push').disabled}, p.st "${st()}"`);
      $('#push').hidden = false;
      // `--screenshot` captures the document's top: the shot drops the needs cards so Alerts is in it (the check keeps them).
      if (!DBG.get('check')) { $('#needs').hidden = true; scrollTo(0, 0); }
    },
    // #234 gate 2: the hosts fixture with mini's load/cpu chart open (24 h), read from the export's inline series.
    'host-chart': async () => {
      tab('host');
      const r = $('#host .hcd .rw[data-k=load]');
      r.closest('.grp').open = true;
      r.open = true;
      await chart(r, '24h', null, true);
      if (!r.querySelector('.hpl polyline.ln') || !$$('#host .spk polyline').length) early.push('host chart: no sparkline or chart line drawn');
      if (!DBG.get('check')) { $('#needs').hidden = true; r.closest('.hcd').scrollIntoView(); }
    },
    // #141 gate 2: the busy and healthy fixtures (scripts/host-fixtures.sh), as the tab first shows them.
    'host-ranked': () => {
      tab('host');
      ranked();
      if (location.pathname.includes('/healthy/') && $$('#host>.grp:is([open],:not([data-t=g]))').length) early.push('healthy: a group is open or not green');
      if (!DBG.get('check')) { $('#needs').hidden = true; scrollTo(0, 0); }
    },
    // #154: three needs-you cards through the live path (frag): [later] on one → `1 need you ▸` and 2 cards; [later all]
    // → the line alone; a tap on it → all 3; a changed card (its question) comes back by itself. Measured as it ends.
    'needs-later': () => {
      tab('queue');
      put('later', '{}');
      const cards = [...$$('#needs>.ask')].slice(0, 3).map(c => { const x = c.cloneNode(true); x.querySelector('.lat').remove(); x.className = 'ask'; return x; });
      if (cards.length < 3) return early.push(`${cards.length} needs-you cards in the demo, not 3`);
      frag('needs', cards.map(c => c.outerHTML).join(''));
      const line = () => $('#needs').previousElementSibling, shown = () => [...$$('#needs>.ask')].filter(seen).length;
      const want = (what, text, n) => { const t = seen(line()) ? line().textContent : 'no line'; if (t !== text || shown() !== n) early.push(`${what}: line "${t}", ${shown()} cards, not "${text}" and ${n}`); };
      want('fresh', 'no line', 3);
      $('#needs .lat').click();
      want('[later] on one', '1 need you ▸later all', 2);
      $('.later .lall').click();
      want('[later all]', '3 need you ▸later all', 0);
      $('.later .lo').click();
      want('tap the line', '3 need you ▾later all', 3);
      $('.later .lo').click();
      cards[1].querySelectorAll(':scope>p')[1].textContent += ' (changed)';
      frag('needs', cards.map(c => c.outerHTML).join(''));
      want('a changed card', '2 need you ▸later all', 1);
      if (!seen($$('#needs>.ask')[1])) early.push('the changed card did not come back');
    },
    // #217 gate 2: a tapped [one more round]: the needs-human card is one muted pending line with no buttons, at the top.
    'needs-pending': () => {
      tab('queue');
      const c = [...$$('#needs>.ask[data-n]')].find(c => /needs you/.test(c.textContent));
      if (!c) return early.push('no needs-human card in the demo');
      $('#needs').prepend(c);
      pend(c, 'extend');
      const p = $('#needs>.pend');
      if (!p || p.querySelector('button,form') || p.textContent !== '⟳ one more round · starting a lane…') early.push(`pending card: ${p ? p.outerHTML : 'none'}`);
    },
    // #170 gate 2: [important] in all 5 time groups, a red glyph, its first row open; no fold holds clean and a failure.
    events: () => {
      tab('events');
      const imp = $('#events [data-v=imp]'), gs = [...imp.querySelectorAll('.eg>h3>span:first-child')].map(s => s.textContent);
      if (gs.join() !== 'now,5m,1h,earlier today,yesterday') early.push(`[important] groups: ${gs.join()}`);
      if (!imp.querySelector('.er .kg.t-r')) early.push('no red glyph under [important]');
      if ([...$$('#events .er')].some(r => /clean/.test(r.textContent) && /alert failed/.test(r.textContent))) early.push('a row holds clean and a failure');
      imp.querySelector('.er').open = true;
    },
    // #241: a pick applies in place: no reload, the favicon swaps, terminal strips every sprite, kitchen draws its own.
    'theme-switch': async () => {
      tab('queue');
      window.kept = 1;
      $('#look').click(); // #290: the look sheet's rows, not a select
      const pick = n => { $(`.look .lko[data-t="${n}"]`).click(); return new Promise(r => setTimeout(r, 800)); };
      await pick('terminal');
      if (window.kept !== 1 || document.documentElement.dataset.theme !== 'terminal' || get('theme') !== 'terminal') early.push('the switch to terminal reloaded the page or was not kept');
      if (!$('link[rel=icon]').href.includes('themes/terminal/icon-')) early.push(`favicon ${$('link[rel=icon]').href}, not terminal's`);
      if ($$('.rsp, .lsc').length || B.dataset.mode !== 'glyph') early.push('terminal kept sprites or a scene mode');
      await pick('kitchen');
      if (!$$('#queue .rsp').length || !$('#hr-impl .chef, #hr-impl [shape-rendering]')) early.push("kitchen: no row sprites, or not the chef's");
      close(); // the board is measured, not the look sheet over it
      await new Promise(r => setTimeout(r, 400));
    },
    // #290 gate 2: `[◐]` on all 4 tabs, no native select; its sheet holds THEME and COLOURS listboxes, one row ticked
    // each; a tap applies in place (no reload) and the sheet stays open.
    look: async () => {
      for (const t of ['lanes', 'host', 'events', 'queue']) { tab(t); if (!seen($('#look'))) early.push(`no [◐] on ${t}`); }
      if ($$('select#theme, select#scheme').length) early.push('a native theme/colours select remains');
      $('#look').click();
      const ls = [...$$('#sheet [role=listbox]')].map(l => `${l.getAttribute('aria-label')}:${l.querySelectorAll('[role=option]').length}/${l.querySelectorAll('[aria-selected=true]').length}`);
      if (ls.length !== 3 || !/^theme:3\/1,colours:\d+\/1,style:2\/1$/.test(ls.join())) early.push(`look sheet lists: ${ls.join()}`);
      // #335: STYLE ascii applies in place (rows take the figures, the sheet stays open), pixel puts the sprites back.
      $('.look .lko[data-y=ascii]').click();
      if (B.dataset.mode !== 'ascii' || get('mode') !== 'ascii' || !$$('#queue .rsa').length || $$('#queue .rsg>svg').length || !shown()) early.push('style ascii: not applied in place');
      $('.look .lko[data-y=pixel]').click();
      if (B.dataset.mode !== 'scene' || $$('.rsa, .lsa').length) early.push('style pixel: ascii figures remain');
      window.kept = 1;
      const s = [...$$('.look .lko[data-s]')].find(o => o.getAttribute('aria-selected') !== 'true');
      s.click();
      if (window.kept !== 1 || document.documentElement.dataset.scheme !== s.dataset.s || !$('.sh.look') || !shown()) early.push('a colours tap reloaded, missed or closed the sheet');
      close(); // closed, `[◐]` opens it again (the closed sheet keeps its class)
      await new Promise(r => setTimeout(r, 400));
      $('#look').click();
      if (!shown()) early.push('[◐] did not reopen the look sheet after a close');
    },
    // #241: a moment under reduced motion is a still frame (scripts/ui-audit.sh runs this view with it forced).
    'moment-still': () => {
      tab('queue');
      moment('merged');
      const o = $('#mo');
      if (!matchMedia('(prefers-reduced-motion: reduce)').matches) early.push('reduced motion not forced');
      if (!o || o.hidden || getComputedStyle(o).animationName !== 'none') early.push(`moment: ${o ? getComputedStyle(o).animationName : 'none shown'}, not a still frame`);
    },
    // #337 gate 2: a wait in the phone's theme (`loading~kitchen`, `loading~terminal`; `loading-still` with reduced motion
    // forced; `-1440` at the desktop width): [↻] keeps its box and spins its plain ↻ (no sprite in the header, idle or
    // busy); the overlay's sprite is 96 px, centred near the top of the content under the header, over a light scrim that
    // takes no taps; terminal: a spinning glyph; reduced motion: one still frame. A strip holds the sprite's first four
    // frames (8 fps: 0, 125, 250, 375 ms) for the shot.
    loading: () => {
      tab('queue');
      const box = () => [rf, rf.firstChild, $('header')].map(e => { const r = e.getBoundingClientRect(); return [r.left, r.top, r.width, r.height].map(Math.round).join(); }).join(' ');
      const idle = box(), still = matchMedia('(prefers-reduced-motion: reduce)').matches, ov = $('#ld'), svg = ov.querySelector('svg'), g = ov.querySelector('i');
      if (rf.matches('.spin') || seen(ov)) early.push('busy before the refresh');
      if ($('header svg.lds, header use[href="#hload"]')) early.push('a loading sprite in the header');
      busy = true;
      sync('a');
      loading(true);
      if (box() !== idle) early.push(`[↻] busy moved: ${idle} → ${box()}`);
      if (V === 'loading-still' && !still) early.push('reduced motion not forced');
      if (rf.querySelector('svg') || rf.textContent !== '↻') early.push('[↻] is not the plain ↻');
      const spin = getComputedStyle(rf.firstChild).animationName;
      if (spin !== (still ? 'none' : 's')) early.push(`[↻] ${spin}, not spinning`);
      if (getComputedStyle(ov).pointerEvents !== 'none') early.push('the overlay takes taps');
      const hb = $('header').getBoundingClientRect(), t = document.documentElement.dataset.theme, term = t === 'terminal';
      if (ov.getBoundingClientRect().top < hb.bottom - 1) early.push('the overlay covers the header');
      if (term ? seen(svg) || !seen(g) : !ov.matches('.ld') || !$('#hload') || !seen(svg) || seen(g)) early.push(`${t}: not its loading animation (#hload ${!!$('#hload')})`);
      const el = term ? g : svg, r = el.getBoundingClientRect(), cs = getComputedStyle(el);
      if (!term && (Math.round(r.width) !== 96 || Math.round(r.height) !== 96)) early.push(`${t}: loading sprite ${r.width}×${r.height}, not 96×96`);
      if (Math.abs(r.left + r.width / 2 - innerWidth / 2) > 2) early.push(`${t}: loading animation off centre (${r.left}+${r.width} in ${innerWidth})`);
      if (r.top < hb.bottom || r.top > hb.bottom + 160) early.push(`${t}: loading animation ${r.top - hb.bottom | 0} px under the header, not near the top`);
      if (cs.animationName !== (still ? 'none' : term ? 's' : 'ldg, ldk')) early.push(`${t}: loading animation ${cs.animationName}`);
      if (term) return;
      const strip = B.appendChild(Object.assign(document.createElement('div'), { style: 'position:fixed;z-index:3;bottom:calc(var(--hit) + var(--ctl) + var(--s3));left:0;right:0;display:flex;justify-content:center;gap:var(--s2);padding:var(--s1);background:var(--card);pointer-events:none' }));
      [0, 1, 2, 3].forEach(i => {
        const c = strip.appendChild(svg.cloneNode(true));
        c.style.cssText += ';width:48px;height:48px;image-rendering:pixelated;shape-rendering:crispEdges;animation:' + cs.animation;
        c.getAnimations().forEach(a => { a.pause(); a.currentTime = i * 125; });
      });
    },
    'loading-still': () => VIEWS.loading(),
    // #335 gate 2: ascii style (the look sheet's STYLE): the rows, Lanes, Host and the wait in the theme's ASCII figures.
    'ascii-queue': () => { style('ascii'); return VIEWS.queue(); },
    'ascii-lanes': () => { style('ascii'); return VIEWS.lanes(); },
    'ascii-host': () => { style('ascii'); return VIEWS.host(); },
    'ascii-events': () => { style('ascii'); return VIEWS.events(); },
    // The overlay runs its frames by CSS alone: no node or text changes while it shows (#334), [↻] stays the plain ↻.
    'ascii-loading': () => {
      style('ascii');
      tab('queue');
      const ov = $('#ld'), a = ov.querySelector('.lda');
      let n = 0;
      const mo = new MutationObserver(m => { n += m.length; });
      busy = true;
      sync('a');
      loading(true);
      mo.observe(ov, { subtree: true, childList: true, characterData: true, attributes: true });
      if (!a || !seen(a) || seen(ov.querySelector('svg')) || seen(ov.querySelector('i'))) early.push('ascii: not the ascii loading frames');
      if (a && (a.children.length !== 2 || getComputedStyle(a.lastChild).animationName !== 'f')) early.push(`ascii: ${a.children.length} loading frames, ${getComputedStyle(a.lastChild).animationName}`);
      if (rf.querySelector('svg') || rf.textContent !== '↻') early.push('[↻] is not the plain ↻');
      setTimeout(() => { mo.disconnect(); if (n) early.push(`${n} changes to the loading overlay while it ran`); }, 1100);
    },
    herd1: () => { herd(true); light = 'day'; ranch(); }, // #71: day shots, whatever the runner's clock says
    herd2: () => { herd(true); light = 'day'; ranch(); },
    // #172: the kitchen theme (`?theme=kitchen`), and it missing `done` (`&nopose=done`): farm's done horse.
    // #239: its stations and chefs by role at calm, busy (the demo's [gusty]) and held ([storm]: smoke, wall flames, the red glow,
    // idle chefs fanning); `herd-kitchen-still` is held under reduced motion: a still frame that still shows smoke and glow.
    'herd-kitchen': () => kitchen('calm'),
    'herd-kitchen-busy': () => kitchen('gusty'),
    'herd-kitchen-held': () => kitchen('storm'),
    'herd-kitchen-still': () => kitchen('storm'),
    'herd-nodone': () => { herd(true); light = 'day'; ranch(); setTimeout(() => { if (!$('#hp3 .horse-done') || $('#hp1 .horse')) early.push("nodone: done is not farm's horse, or the rest not chefs"); }, 500); },
    // #71: Auto is night before 07:00 and from 19:00; [A] cycles auto → day → night, each stored; a fresh load of the
    // page on this phone (an iframe: virtual time runs no reload) comes back at the last tap, night: the night shot.
    'herd-night': async () => {
      herd(true);
      const at = (h, m) => new Date(2026, 10, 1, h, m);
      for (const [h, m, n] of [[6, 59, true], [7, 0, false], [18, 59, false], [19, 0, true]]) if (nightAt('auto', at(h, m)) !== n) early.push(`auto at ${h}:${m}: not ${n ? 'night' : 'day'}`);
      if (light !== 'auto') early.push(`a new phone starts at ${light}, not auto`);
      for (const want of ['day', 'night', 'auto', 'day', 'night']) {
        lb.click();
        const n = nightAt(want, new Date()), label = 'ranch light: ' + want + (want === 'auto' ? ` (${n ? 'night' : 'day'} now)` : '');
        if (lb.getAttribute('aria-label') !== label || lb.textContent !== ICON[want] || get('herd-light') !== want || H.classList.contains('night') !== n) early.push(`tap to ${want}: [${lb.textContent}] "${lb.getAttribute('aria-label')}", stored ${get('herd-light')}, night ${H.classList.contains('night')}`);
      }
      const f = Object.assign(document.createElement('iframe'), { src: 'index.html', hidden: true });
      await new Promise(r => { f.onload = r; B.append(f); });
      await wait();
      const d = f.contentDocument, b = d.querySelector('.hlt');
      if (!b || b.textContent !== '☾' || !d.querySelector('#herd.night')) early.push(`after a reload: [${b && b.textContent}], #herd "${(d.querySelector('#herd') || {}).className}": the choice is lost`);
      f.remove();
    },
    'herd-night-storm': async () => { // the demo's [storm] at night: darker sky, rain on
      herd(true);
      light = 'night';
      ranch();
      await wait();
      await wait();
      $('.hwx button[value=storm]').click();
      if (!H.matches('.night.storm')) early.push(`#herd is "${H.className}", not night and storm`);
    },
    'herd-scroll': () => herd(true),
    // #291 gate 2: 3 repos; [›], a dot, →, a horizontal wheel (one gesture of three events), shift+wheel and a mouse drag
    // across a horse each move exactly one repo; a vertical wheel and a drag in a text field none; the drag's click opens nothing, a plain click does.
    'herd-nav': async () => {
      herd(true); light = 'day'; ranch();
      await new Promise(r => setTimeout(r, 500));
      const f = structuredClone(data);
      f.fleets = f.fleets.slice(0, 3); f.others = [];
      herdData(f);
      const wh = o => PG.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, ...o }));
      const pe = (t, el, x) => el.dispatchEvent(new PointerEvent(t, { bubbles: true, cancelable: true, pointerId: 7, pointerType: 'mouse', button: 0, buttons: t === 'pointerup' ? 0 : 1, clientX: x, clientY: 400 }));
      const steps = [['[›]', 1, () => hp(0).querySelector('.hnx').click()], ['the next dot', 1, () => hp(0).querySelectorAll('.hdt button')[1].click()],
        ['→', 1, () => B.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))],
        ['a horizontal wheel', 1, () => [50, 50, 50].forEach(x => wh({ deltaX: x }))], ['a vertical wheel', 0, () => wh({ deltaY: 120 })],
        ['shift+wheel', 1, () => wh({ deltaY: 100, shiftKey: true })],
        ['a drag in a text field', 0, () => { const i = hp(0).appendChild(document.createElement('input')); pe('pointerdown', i, 300); [280, 240, 200].forEach(x => pe('pointermove', i, x)); pe('pointerup', i, 200); i.remove(); }],
        ['a mouse drag', 1, () => { const h = H.querySelector('.hh'); pe('pointerdown', h, 300); [280, 240, 200].forEach(x => pe('pointermove', h, x)); pe('pointerup', h, 200); h.click(); }]];
      // Reduced motion (the gate run): each switch lands at once; else after the smooth scroll. A step waits out a wheel gesture.
      for (const [what, want, run] of steps) {
        PG.scrollTo({ left: 0, behavior: 'instant' });
        await new Promise(r => setTimeout(r, 350));
        run();
        if (!still.matches) await new Promise(r => setTimeout(r, 900));
        const got = Math.round(PG.scrollLeft / PG.clientWidth);
        if (got !== want) early.push(`${what}: on repo ${got}, not ${want}${still.matches ? ' at once (reduced motion)' : ''}`);
      }
      if (shown()) early.push('the click ending a drag opened a lane');
      if (getSelection().toString()) early.push(`the drag selected "${getSelection()}"`);
      H.querySelector('.hh').click();
      if (!shown()) early.push('a plain click on a horse opened nothing');
      close();
      hide();
      $$('.hnv, .hdt button').forEach(b => { if (getComputedStyle(b).cursor !== 'pointer') early.push(`${label(b)}: cursor ${getComputedStyle(b).cursor}`); });
    },
    // #367 gate 2 (the `queue` fixture: 3 repos, 40 queued, a 6-step chain): a tap, ←/→ and a sideways drag switch repos
    // (only the picked card shows, the pick stored); back on acme-core its running rows are above the fold, the chain one
    // closed row with its summary, `+ 30 more` closed, and every title line ≥ 4.5:1 on what is under it.
    'queue-repos': () => {
      tab('queue');
      const vis = () => [...$$('#queue .fc')].filter(seen).map(qname).join(), chip = n => $(`.qch [data-qr="${n}"]`);
      const tries = [['tap', () => chip('docs').click(), 'docs'], ['→', () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })), 'all'],
        ['←', () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true })), 'docs'],
        ['swipe', () => { const t = $('#queue .fc:not([hidden]) .qg'), o = { bubbles: true, pointerId: 7, clientY: 300 }; t.dispatchEvent(new PointerEvent('pointerdown', { ...o, clientX: 300 })); t.dispatchEvent(new PointerEvent('pointerup', { ...o, clientX: 150 })); t.click(); }, 'all']]; // a mouse drag ends in a click, which opens nothing
      for (const [how, act, want] of tries) {
        act();
        const shows = want === 'all' ? [...$$('#queue .fc')].filter(c => c.querySelector('.qg[data-g=need]')).map(qname).join() : want;
        if (vis() !== shows || get('queue:repo') !== want || chip(want).getAttribute('aria-pressed') !== 'true') early.push(`${how}: shows "${vis()}", stored ${get('queue:repo')}, not ${want}`);
      }
      if ([...$$('.fc.qa .qg')].some(g => seen(g) && g.dataset.g !== 'need')) early.push('[all] shows a group that is not needs you');
      chip('acme-core').click();
      const nav = $('nav').getBoundingClientRect().top, prog = [...$$('.qg[data-g=prog]>details.q')].filter(seen);
      if (!prog.length || prog.some(d => d.getBoundingClientRect().bottom > nav)) early.push(`running rows not above the fold (tab bar at ${nav})`);
      const gs = [...$$('#queue .qg>h3')].filter(seen).map(h => h.textContent).join(', ');
      if (gs !== 'needs you (1), in progress (2), queued (40)') early.push(`groups "${gs}"`);
      const c = $('#queue .qc'), x = $('#queue .qx');
      // #391: the chain row is a row: the first step's number and title, `chain · 6`, its place, its columns where a queued
      // row has them, and `▸ #302 → #307 · next: #400 (in impl)` under them.
      const cs = c && c.querySelector('summary'), cells = s => [':scope>.gt', ':scope>span:first-of-type', ':scope>span:nth-of-type(2)', ':scope>.m', ':scope>.tl'].map(k => s.querySelector(k));
      const words = cs && [...cells(cs).slice(1), cs.querySelector('.cst')].map(x => x?.textContent).join(' | ');
      if (!c || c.open || words !== '#302 | chain · 6 | #3 | decoupling: split the serve module | #302 → #307 · next: #400 (in impl)' || getComputedStyle(cs.querySelector('.cst'), '::before').content !== '"▸ "' || [...c.querySelectorAll('details.q')].some(seen)) early.push(`chain row: ${words}`);
      const q1 = $('#queue .fc:not([hidden]) .qg[data-g=queue]>details.q'), at = s => cells(s).map(x => x && `${x.className}@${Math.round(x.getBoundingClientRect().left)}`).join();
      if (cs && at(cs) !== at(q1.querySelector('summary'))) early.push(`chain row columns ${at(cs)}, a queued row's ${at(q1.querySelector('summary'))}`);
      if (!x || x.open || x.querySelector('summary').textContent !== '+ 30 more' || [...x.querySelectorAll('details.q')].some(seen)) early.push(`more row: ${x && x.querySelector('summary').textContent}`);
      const rgb = s => s.match(/[\d.]+/g).map(Number), bg = e => { for (; e; e = e.parentElement) { const v = rgb(getComputedStyle(e).backgroundColor); if (v[3] !== 0) return v.slice(0, 3); } return rgb(getComputedStyle(B).backgroundColor); };
      for (const t of [...$$('#queue summary>.tl')].filter(seen)) if (ratio(rgb(getComputedStyle(t).color), bg(t)) < 4.5) early.push(`title "${t.textContent.slice(0, 20)}" under 4.5:1`);
    },
    // #367: the chain and `+ 30 more` opened in place (the steps right under the chain row, depth ≤ 2, the rest right under
    // `+ 30 more`), every title starting at one x; scrolled to the end, the last row clears the tab bar.
    'queue-chain': () => {
      tab('queue');
      $('.qch [data-qr="acme-core"]').click();
      const c = $('#queue .qc'), x = $('#queue .qx');
      c.open = x.open = true;
      const steps = [...c.querySelectorAll(':scope>details.q')], rest = [...x.querySelectorAll(':scope>*:not(summary)')];
      if (steps.length !== 6 || !steps.every(seen) || steps.some(d => !/\bd[012]\b/.test(d.className))) early.push(`chain opened ${steps.filter(seen).length} of ${steps.length} steps`);
      if (getComputedStyle(c.querySelector('summary>.cst'), '::before').content !== '"▾ "') early.push('an open chain row shows no ▾');
      const under = (a, b) => Math.abs(b.getBoundingClientRect().top - a.getBoundingClientRect().bottom) > 1;
      if (under(c.querySelector('summary'), steps[0]) || rest.length !== 30 || !rest.every(seen) || under(x.querySelector('summary'), rest[0])) early.push('chain or more not opened in place');
      const xs = new Set([...$$('#queue summary>.tl')].filter(seen).map(t => Math.round(t.getBoundingClientRect().left + parseFloat(getComputedStyle(t).paddingLeft))));
      if (xs.size !== 1) early.push(`titles start at ${[...xs].join(', ')}`);
      scrollTo({ top: document.documentElement.scrollHeight, behavior: 'instant' });
      const last = [...$$('#queue .fc:not([hidden]) summary')].filter(seen).at(-1).getBoundingClientRect().bottom, nav = $('nav').getBoundingClientRect().top;
      if (last > nav) early.push(`last row ends at ${last}, under the tab bar at ${nav}`);
      scrollTo({ top: 0, behavior: 'instant' }); // a scrolled view shoots blank: the shot is the top, the needs cards hidden
      if (!DBG.get('check')) $('#needs').hidden = true;
    },
    // #365 gate 2 (the `queue` fixture: 100 open issues in acme-api's backlog): search, a quick filter and a label narrow
    // the rows; three ticks show [queue 3] above the tab bar with the issues in tick order; no row is wider than the screen.
    'queue-backlog': () => {
      tab('queue');
      $('.qch [data-qr="acme-api"]').click();
      const b = $('#queue .fc:not([hidden]) .bl'), rows = () => [...b.querySelectorAll('.bi')].filter(seen), n = () => b.querySelector('summary>.m').textContent;
      b.open = true;
      const all = rows().length, q = b.querySelector('.bs input'), f = b.querySelector('.bbar'), nav = $('nav').getBoundingClientRect().top;
      if (all !== 100 || n() !== '(100)') early.push(`backlog shows ${all} rows, ${n()}`);
      Object.assign(q, { value: 'past one' }).dispatchEvent(new Event('input', { bubbles: true }));
      if (!rows().length || rows().length === all || rows().some(r => !r.textContent.includes('past one')) || n() !== `(${rows().length} of 100)`) early.push(`search: ${rows().length} rows, ${n()}`);
      Object.assign(q, { value: '' }).dispatchEvent(new Event('input', { bubbles: true }));
      b.querySelector('[data-bf="90"]').click();
      const is = JSON.parse(b.dataset.bl), old = Number(document.body.dataset.ts) - 90 * 86400;
      if (!rows().length || rows().some(r => Date.parse(is[r.dataset.n].created) / 1000 > old + 86400) || b.querySelector('[data-bf="90"]').getAttribute('aria-pressed') !== 'true') early.push(`>90 days: ${rows().length} rows`);
      b.querySelector('[data-bf="90"]').click();
      const sel = b.querySelector('.bfs select');
      Object.assign(sel, { value: 'phone' }).dispatchEvent(new Event('change', { bubbles: true }));
      if (!rows().length || rows().some(r => !JSON.parse(b.dataset.bl)[r.dataset.n].labels.includes('phone')) || !sel.closest('.pk').textContent.startsWith('phone')) early.push(`label phone: ${rows().length} rows`);
      Object.assign(sel, { value: '' }).dispatchEvent(new Event('change', { bubbles: true }));
      const [x, y, z] = rows();
      for (const r of [z, x, y]) r.querySelector('input').click();
      const want = [z, x, y].map(r => r.dataset.n).join();
      if (!seen(f) || f.elements.issues.value !== want || f.querySelector('button').textContent !== 'queue 3') early.push(`bar: ${seen(f)} "${f.elements.issues.value}" (want ${want}) "${f.querySelector('button').textContent}"`);
      if (f.getBoundingClientRect().bottom > nav + 1) early.push(`[queue 3] ends at ${f.getBoundingClientRect().bottom}, under the tab bar at ${nav}`);
    },
    'queue-scroll': () => { tab('queue'); $$('#queue details.q').forEach(d => { d.open = true; }); },
    // #66: a fixture ▲ POST (the redirect a live server sends), through the page's fetch-submit path;
    // #140: then [Drive now], which must toast too and never navigate.
    toast: () => {
      tab('queue');
      href = location.href;
      const msg = o => ({ drive: 'drive started for ' + o.body.get('fleet'), mv: 'moved #12 before #18' })[o.body.get('op')] || 'error: empty POST body';
      window.fetch = (u, o) => Promise.resolve(Object.defineProperties(new Response(''), { redirected: { value: true }, url: { value: location.origin + '/?msg=' + encodeURIComponent(msg(o)) } }));
      // The demo disables every input (no writes), which would leave the POST body empty: enable the form's, as live.
      const submit = op => { const f = $(`#queue input[name=op][value=${op}]`).form; f.inert = false; [...f.elements].forEach(e => { e.disabled = false; }); f.requestSubmit(); return f; };
      submit('mv');
      setTimeout(() => { toasted = $('.msg.g') && $('.msg.g').textContent; drove = 'drive started for ' + submit('drive').elements.fleet.value; }, 500);
      setTimeout(() => { const m = $('.msg.g, .msg.r'); drove = m && m.textContent === drove || `want "${drove}", saw "${m && m.textContent}"`; }, 1000);
    },
  };
  async function kitchen(wx) {
    herd(true);
    await wait();
    await wait();
    $(`.hwx button[value=${wx}]`).click();
    const lv = { calm: 'calm', gusty: 'busy', storm: 'held' }[wx], s = getComputedStyle(H);
    if ($('#hp1 .horse') || !$('.hr .rs .h-held') || !H.classList.contains('heat-' + lv)) early.push(`kitchen: not its chefs and stations, or not ${lv} (#herd "${H.className}")`);
    if (H.matches('.night') || !lb.hidden) early.push('kitchen: night or the [A] light shown');
    if (lv === 'held' && (!$('.hh.idle use[href="#hfan"]') || getComputedStyle($('.hr .rs .ksmoke')).display === 'none')) early.push('held: no fanning chef or no smoke');
    if (V === 'herd-kitchen-still' && (!matchMedia('(prefers-reduced-motion: reduce)').matches || s.animationName !== 'none')) early.push(`still: #herd animates (${s.animationName})`);
    const t = $('.hqh.n span');
    if (t && t.scrollWidth > t.clientWidth) early.push('"! needs you" overflows its ticket');
  }
  let toasted = null, drove = null, href = null;
  const ERR = 'error: herdsman: herdr agent start failed: {"error":{"code":"invalid_agent_argument","message":"agent arguments cannot be encoded safely for the target shell"},"id":"cli:agent:start"}';
  const dragged = [];
  const HP = { herd1: 0, herd2: 1, 'herd-kitchen': 0, 'herd-kitchen-busy': 0, 'herd-kitchen-held': 0, 'herd-kitchen-still': 0, 'herd-nodone': 0, 'herd-night': 0, 'herd-night-storm': 0, 'herd-nav': 1 }; // the views measuring one Herd page
  const ROOTS = { look: () => [$('#sheet')], sheet: () => [$('#sheet')], 'sheet-full': () => [$('#sheet')], config: () => [$('#sheet')], pipeline: () => [$('#sheet')], 'pipeline-custom': () => [$('#sheet')], 'sheet-parked': () => [$('#sheet')], 'sheet-other': () => [$('#sheet')], ...Object.fromEntries(Object.entries(HP).map(([v, n]) => [v, () => [hp(n), $('.hlt'), $('.hx'), $('.hwx')]])) };
  const CTL = 'button:not(.lr,.hh,.hqh), .pk, [role=button]', ROW = 'summary, .lr, .hh, .hqh, .ql, .lko', NAME = '.nw, .hl b';
  const CHOICE = 'button[name=choice], .pick'; // amendment A (#139: wherever it renders): wraps, ≤ 4 lines, instead of 28 px
  // A closed <details> keeps its content's boxes (content-visibility), so ask the browser what is shown.
  const seen = e => e.checkVisibility({ visibilityProperty: true });
  const label = e => (e.getAttribute('aria-label') || e.textContent).trim().replace(/\s+/g, ' ').slice(0, 24) || e.tagName.toLowerCase();
  function check() {
    let res;
    // #281: the row sprites' boxes (css px, viewport) as the screenshot shows them, above the fixed tab bar (one under it
    // would take in the bar's text); ui-audit.sh checks their pixels are crisp.
    const nav = $('nav').getBoundingClientRect().top;
    const sprites = [...$$('.rsg:not(.k)>.rsp:not(.rsa)')].filter(seen).map(e => e.getBoundingClientRect()).filter(r => r.top >= 0 && r.bottom <= nav).map(r => [r.left, r.top, r.width, r.height]);
    try { res = (SCROLL[V] || measure)(); } catch (e) { res = { view: V, controls: 0, outliers: ['audit error: ' + e.message] }; }
    const W = +(DBG.get('w') || 390); // #337: `&w=1440`, a desktop shot
    if (innerWidth !== W) res.outliers.push(`viewport ${innerWidth} px wide, not ${W}`); // #68: new-headless Chrome keeps windows ≥ 500 px
    // #114 app-like: no pinch or double-tap zoom.
    const vp = $('meta[name=viewport]').content, ta = getComputedStyle(document.documentElement).touchAction;
    if (vp !== 'width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover') res.outliers.push(`viewport meta "${vp}"`);
    if (ta !== 'manipulation') res.outliers.push(`html touch-action ${ta}, not manipulation`);
    if (['queue', 'lanes', 'host', 'events', 'queue-rows', 'events-rows', 'ascii-queue', 'ascii-lanes', 'ascii-host', 'ascii-events'].includes(V)) Object.assign(res, { sprites }).outliers.push(...drawn());
    // #331: every Queue row at every depth (the demo has a 4-deep chain) shows its whole title on one ellipsised line.
    if (V === 'queue') {
      if (![...$$('#queue summary .m')].some(e => e.textContent.startsWith('after #'))) res.outliers.push('no clamped (4-deep) chain row');
      for (const s of [...$$('#queue details.q>summary')].filter(seen)) { // #367: the picked repo's, outside closed chains
        const t = s.querySelector(':scope>.tl');
        if (!t || !seen(t) || t.title !== t.textContent || t.offsetHeight > 1.5 * parseFloat(getComputedStyle(t).lineHeight)) res.outliers.push(`no one-line title: ${label(s)}`);
      }
    }
    // #397: the error shows whole (wraps, never ellipsised) and covers no row: "all repos" starts under it.
    if (V === 'chat-error') {
      const m = $('.msg.r'), row = $('#chat .fc');
      if (!m || m.textContent !== ERR) res.outliers.push(`error banner reads "${m && m.textContent}"`);
      else if (m.scrollWidth > m.clientWidth || m.scrollHeight > m.clientHeight) res.outliers.push(`error banner clipped: ${m.scrollWidth}x${m.scrollHeight} in ${m.clientWidth}x${m.clientHeight}`);
      else if (m.getBoundingClientRect().bottom > row.getBoundingClientRect().top) res.outliers.push(`error banner covers "${label(row)}"`);
    }
    const out = document.createElement('pre');
    out.id = 'audit';
    out.textContent = JSON.stringify(res);
    B.append(out);
  }
  // #241 gate 2: the phone's theme on the tab. Terminal is today's glyph board: no sprite, no load scene, no mode button,
  // glyph avatars. Others draw a 16 px action sprite for each Queue/Events glyph (its title and label the plain word), a
  // load scene on each Host group header, and offer [scene] [ascii].
  function drawn() {
    const t = document.documentElement.dataset.theme, tb = document.documentElement.dataset.tab, bad = [];
    const on = s => [...$$(s)].filter(seen);
    if (!TT) return [`theme ${t} never loaded`];
    if (t === 'terminal') {
      if ($$('.rsp, .lsc, #mo').length) bad.push('terminal draws sprites');
      if (B.dataset.mode !== 'glyph' || $$('#mode').length || on('.av .px, .av .asc, .rsa, .lsa').length) bad.push(`terminal Lanes: mode ${B.dataset.mode}, or a mode button or sprite avatar shows`);
      return bad;
    }
    if (tb === 'queue' && !on('#queue .gt>b.rsg>.rsp').length) bad.push('no Queue row sprite');
    if (tb === 'events' && !on('#events .kg.rsg>.rsp').length) bad.push('no Events row sprite');
    if (tb === 'host' && $$('#host>.grp>summary').length !== $$('#host>.grp>summary>.lsc').length) bad.push('a Host group without its load scene');
    if ($$('#mode').length || TM.join() !== 'scene,ascii') bad.push(`Lanes modes ${TM}, a mode button ${$$('#mode').length}`);
    // #335: ascii style draws the theme's figures, never a sprite; pixel style never a figure.
    const asc = B.dataset.mode === 'ascii';
    if (asc ? on('.rsg>svg, svg.lsc, .av .px').length : on('.rsa, .lsa').length) bad.push(`${B.dataset.mode}: a ${asc ? 'sprite' : 'figure'} of the other style`);
    if (asc && tb === 'lanes' && !on('.av .asc').length) bad.push('ascii: no ascii Lanes avatar');
    for (const g of on('.rsg')) if (!g.title || g.getAttribute('aria-label') !== g.title) bad.push(`a row sprite without its word: ${g.outerHTML.slice(0, 60)}`);
    // #281: a row sprite is 24×24 in a fixed 28 px slot, and the row with it is at most 4 px taller than with its glyph (no wrap).
    for (const g of on('.rsg')) {
      const s = g.firstChild.getBoundingClientRect(), w = g.getBoundingClientRect().width, sum = g.closest('summary'), h = sum.offsetHeight, svg = g.firstChild;
      if (Math.round(s.width) !== 24 || Math.round(s.height) !== 24 || Math.round(w) !== 28) bad.push(`row sprite ${s.width}×${s.height} in a ${w} px slot, not 24×24 in 28`);
      g.classList.remove('rsg');
      g.textContent = g.dataset.g;
      const d = h - sum.offsetHeight;
      g.replaceChildren(svg);
      g.classList.add('rsg');
      if (d > 4) bad.push(`a row ${d} px taller with its sprite than its glyph: ${label(sum)}`);
    }
    return bad;
  }
  function measure() {
    $$('[inert]').forEach(e => { e.inert = false; });
    const roots = (ROOTS[V] || (() => [B]))(), bad = [];
    const all = sel => roots.flatMap(r => [...(r.matches(sel) ? [r] : []), ...r.querySelectorAll(sel)]).filter(seen);
    const ctl = all(CTL), rows = all(ROW);
    for (const e of ctl) {
      const cs = getComputedStyle(e), lines = (e.offsetHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom)) / parseFloat(cs.lineHeight);
      if (!e.matches(CHOICE) && cs.height !== '28px') bad.push(`${label(e)}: control height ${cs.height}`);
      if (e.matches(CHOICE) && (e.offsetHeight < 28 || lines > 4.05)) bad.push(`${label(e)}: choice ${e.offsetHeight}px, ${lines.toFixed(1)} lines (28 px .. 4 lines)`);
      // #139: the CSS brackets a label once; one that carries its own reads `[[change]]`.
      if (/^\s*\[/.test(e.textContent) && getComputedStyle(e, '::before').content === '"["') bad.push(`${label(e)}: label in brackets of its own, shown [[…]]`);
    }
    for (const e of [...ctl, ...rows]) {
      e.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' }); // `.hp` scrolls smooth otherwise
      const r = e.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      for (const [x, y] of [[cx - 16, cy], [cx + 16, cy], [cx, cy - 16], [cx, cy + 16]]) {
        if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
        const h = document.elementFromPoint(x, y);
        if (!h || !e.contains(h)) { bad.push(`${label(e)}: tap area under 44 px, (${x | 0},${y | 0}) hits ${h ? h.outerHTML.slice(0, 40) : 'nothing'}`); break; }
      }
    }
    if (V === 'herd2') hp(1).scrollIntoView();
    for (const r of roots) {
      const w = document.createTreeWalker(r, NodeFilter.SHOW_TEXT);
      for (let t; (t = w.nextNode());) {
        const e = t.parentElement;
        if (!t.data.trim() || !seen(e) || e.closest('svg, .gl, .asc')) continue;
        const fs = getComputedStyle(e).fontSize, input = e.closest('input, textarea, select');
        if (input ? fs !== '16px' : !['11px', '13px'].includes(fs)) bad.push(`"${t.data.trim().slice(0, 20)}": font ${fs}`);
        // Text past its box's sides, unless that box ellipsises it (the button's `]` ring is not text).
        let b = e;
        while (['inline', 'contents'].includes(getComputedStyle(b).display)) b = b.parentElement;
        const g = document.createRange();
        g.selectNodeContents(t);
        const tr = g.getBoundingClientRect(), br = b.getBoundingClientRect();
        if (!input && (tr.right > br.right + 1 || tr.left < br.left - 1) && getComputedStyle(b).textOverflow !== 'ellipsis') bad.push(`"${t.data.trim().slice(0, 20)}": text overflows its box by ${Math.max(tr.right - br.right, br.left - tr.left) | 0}px`);
      }
      // #68: no box holds more than it shows unless it ellipsises (or scrolls by design); nothing past the screen's right edge.
      // Tap rings (`]`, the queued horse's ::before, a picker's invisible select grown 8 px each side) are hit area, not content:
      // off while measuring. The sky's clouds drift past its clipped sides.
      // #162 A: a summary's widest text box is at least half its row (a 7ch grid column once squeezed a 1-line summary into
      // 14); a text run straight in a grid/flex summary counts only when it wraps.
      for (const s of [...r.querySelectorAll('summary')].filter(seen)) {
        const box = e => { while (['inline', 'contents'].includes(getComputedStyle(e).display)) e = e.parentElement; return e.getBoundingClientRect().width; };
        const run = t => { const g = document.createRange(); g.selectNodeContents(t); return new Set([...g.getClientRects()].map(x => Math.round(x.top))).size > 1 ? g.getBoundingClientRect().width : Infinity; };
        const texts = [...s.querySelectorAll('*'), s].flatMap(e => [...e.childNodes].filter(n => n.nodeType === 3 && n.data.trim()));
        const w = Math.max(0, ...texts.map(t => t.parentElement === s ? run(t) : box(t.parentElement)));
        if (texts.length && w < s.getBoundingClientRect().width / 2) bad.push(`summary "${s.textContent.trim().slice(0, 24)}": widest text box ${w | 0}px of its ${s.getBoundingClientRect().width | 0}px row`);
      }
      const ring = Object.assign(document.createElement('style'), { textContent: 'button::after,.pk::after,.hqh::before{display:none!important}.pk select{inset:0!important;width:100%!important}' });
      B.append(ring);
      for (const e of [r, ...r.querySelectorAll('*')]) {
        if (e.closest('svg, .hr') || !seen(e)) continue;
        const cs = getComputedStyle(e), what = `${e.localName}${typeof e.className === 'string' && e.className ? '.' + e.className.split(' ')[0] : ''} "${label(e)}"`;
        if (cs.display === 'inline' || /auto|scroll/.test(cs.overflowX)) continue;
        if (e.scrollWidth > e.clientWidth + 1 && !(cs.textOverflow === 'ellipsis' && cs.overflowX === 'hidden')) bad.push(`${what}: ${e.scrollWidth}px of content in ${e.clientWidth}px`);
        if (e.getBoundingClientRect().right > innerWidth + 1) bad.push(`${what}: right edge ${e.getBoundingClientRect().right | 0} past the screen's ${innerWidth}`);
      }
      ring.remove();
      for (const e of [...r.querySelectorAll(NAME)].filter(seen)) {
        const cs = getComputedStyle(e), h = e.getBoundingClientRect().height;
        const lh = cs.lineHeight === 'normal' ? 1.2 * parseFloat(cs.fontSize) : parseFloat(cs.lineHeight);
        if (h > 2 * lh + 1) bad.push(`${label(e)}: name ${h | 0}px tall, over 2 lines`);
      }
    }
    // #139: nothing in an open queue row's card pokes past the card's right edge.
    for (const d of all('details.q[open]')) {
      const right = d.getBoundingClientRect().right;
      for (const e of [...d.querySelectorAll('*')].filter(seen)) if (e.getBoundingClientRect().right > right + 1) bad.push(`${label(e)}: right edge ${e.getBoundingClientRect().right | 0} past the card's ${right | 0}`);
    }
    const se = document.scrollingElement;
    if (se.scrollWidth > se.clientWidth) bad.push(`page scrolls sideways: ${se.scrollWidth}>${se.clientWidth}`);
    if (V in HP) {
      const p = hp(HP[V]), pr = p.getBoundingClientRect();
      for (const e of [p.querySelector('.hs'), $('.hlt'), $('.hx')]) if (!e.hidden && e.offsetHeight !== 28) bad.push(`${e.className}: Herd strip/[A]/[×] ${e.offsetHeight}px, not 28`);
      // #71: the ranch-light button sits inside the strip, left of [×] (the tap checks above scrolled the page, and
      // in a storm #herd itself: the rain tile overflows it, which only scrollIntoView can scroll).
      p.scrollTo({ top: 0, behavior: 'instant' });
      H.scrollTop = 0;
      const lt = $('.hlt').getBoundingClientRect(), xr = $('.hx').getBoundingClientRect(), sr = p.querySelector('.hs').getBoundingClientRect();
      if (lt.top < sr.top || lt.bottom > sr.bottom || lt.right > xr.left) bad.push(`[${$('.hlt').textContent}] at ${lt.left | 0}..${lt.right | 0} × ${lt.top | 0}..${lt.bottom | 0}: not in the strip (${sr.top | 0}..${sr.bottom | 0}) left of [×] (${xr.left | 0})`);
      // #65: Corral's size: lane horses 132 px wide, queued ones 53 (40 %).
      for (const [sel, w] of [['.hh svg', 132], ['.hqh svg', 53]]) {
        for (const e of p.querySelectorAll(sel)) { const x = e.getBoundingClientRect().width; if (Math.abs(x - w) > 1) bad.push(`${label(e.parentNode)}: ${sel} ${x.toFixed(1)}px wide, not ${w}`); }
      }
      // #65 A: nothing on the page pokes past its sides (`.hp` clips, so the page-level check cannot see it).
      for (const e of p.querySelectorAll('.hs *, .hb *')) {
        if (e.parentElement.closest('svg') || !seen(e) || getComputedStyle(e).display === 'inline') continue; // inline text: its box clips it
        const r = e.getBoundingClientRect();
        if (r.left < pr.left - 1 || r.right > pr.right + 1) bad.push(`${label(e)}: past the page's side (${r.left | 0}..${r.right | 0} of ${pr.left | 0}..${pr.right | 0})`);
      }
      // #65 A: scrolled to the end, no text sits under the weather caption or the demo's switcher.
      p.scrollTo({ top: p.scrollHeight, behavior: 'instant' });
      const bars = [cap, $('.hwx')].filter(Boolean).map(e => [e.className, e.getBoundingClientRect()]), w = document.createTreeWalker(p, NodeFilter.SHOW_TEXT);
      for (let t; (t = w.nextNode());) {
        if (!t.data.trim() || !seen(t.parentElement)) continue;
        const g = document.createRange();
        g.selectNodeContents(t);
        const r = g.getBoundingClientRect();
        for (const [k, c] of bars) if (r.bottom > c.top && r.top < c.bottom && r.right > c.left && r.left < c.right) bad.push(`"${t.data.trim().slice(0, 20)}": under .${k}`);
      }
      // #65 A: no strip text is cut to an ellipsis, also at the owner's counts (13 queued, 11 done, 5 idle, 3 working).
      // #114: every queued horse in a row stands on one ground line (its SVG's bottom ±1 px), blocked or not, and
      // none overlaps the gate post (`.gate::after`).
      const ground = () => {
        for (const g of p.querySelectorAll('.gate')) {
          const a = getComputedStyle(g, '::after'), post = g.getBoundingClientRect().left + parseFloat(a.left) + parseFloat(a.width), rows = new Map();
          for (const h of g.querySelectorAll('.hqh')) {
            const r = h.getBoundingClientRect(), y = h.querySelector('svg').getBoundingClientRect().bottom, k = Math.round(r.top);
            if (r.left < post) bad.push(`${label(h)}: left ${r.left | 0} over the gate post (to ${post | 0})`);
            if (!rows.has(k)) rows.set(k, []);
            rows.get(k).push([label(h), y]);
          }
          for (const row of rows.values()) for (const [n, y] of row) if (Math.abs(y - row[0][1]) > 1) bad.push(`${n}: feet at ${y.toFixed(1)}, ${row[0][0]} at ${row[0][1].toFixed(1)}`);
        }
      };
      ground();
      const clip = () => [...p.querySelectorAll('.hs p')].filter(e => e.scrollWidth > e.clientWidth).forEach(e => bad.push(`"${e.textContent.slice(0, 24)}": strip text ellipsized, ${e.scrollWidth}>${e.clientWidth}`));
      clip();
      if (V === 'herd1') {
        const f = structuredClone(data), l = f.fleets[0].lanes[0], q = f.fleets[0].queue[0];
        f.fleets[0].lanes = [...'wwwiiiiiddddddddddd'].map((s, i) => ({ ...l, name: 'audit-' + i, state: { w: 'working', i: 'idle', d: 'done' }[s] }));
        f.fleets[0].queue = Array.from({ length: 13 }, (_, i) => ({ ...q, issue: 900 + i, blocked_by: i % 3 ? null : 2683 })); // #114: mixed rows
        herdData(f);
        clip();
        ground();
      }
    }
    return { view: V, controls: ctl.length, outliers: [...early.splice(0), ...bad] };
  }
  // #65: a live update never moves the scroll or replaces a kept node. Scroll to 400 px, push a changed fixture
  // through the live path (herdData / frag), and count what the MutationObserver saw leave the tree for good.
  function gone(root, run, sel) {
    const mo = new MutationObserver(() => {}), out = new Set();
    mo.observe(root, { childList: true, subtree: true });
    run();
    for (const r of mo.takeRecords()) for (const n of r.removedNodes) if (n.nodeType === 1) for (const e of [n, ...n.querySelectorAll(sel)]) if (e.matches(sel) && !e.isConnected) out.add(e);
    mo.disconnect();
    return out.size;
  }
  const SCROLL = {
    'sheet-drag': () => ({ view: V, controls: 1, outliers: dragged }),
    toast: () => {
      const bad = [];
      if (toasted !== 'moved #12 before #18') bad.push(`toast after the POST: ${toasted}`);
      if (drove !== true) bad.push(`[Drive now] gave no toast: ${drove}`);
      if (location.href !== href) bad.push(`[Drive now] navigated: ${location.href}`);
      if ($('.msg.g, .msg.r')) bad.push('.msg still shown 5 s after the POST');
      if (/[?&]msg=/.test(location.search)) bad.push(`location.search keeps msg: ${location.search}`);
      return { view: V, controls: 0, outliers: bad };
    },
    'herd-scroll': () => {
      const base = structuredClone(data), bad = [], f = base.fleets[0];
      // Tall enough to scroll 400 px: the first fleet's lanes ×3.
      f.lanes.slice().concat(f.lanes).forEach((l, i) => f.lanes.push({ ...l, name: 'audit-' + i }));
      herdData(base);
      const p = hp(0), next = structuredClone(base), l = next.fleets[0].lanes;
      p.scrollTo({ top: 400, behavior: 'instant' });
      const top = p.scrollTop, keep = new Map([...p.querySelectorAll('.hh, .hqh')].map(e => [e.dataset.agent || e.dataset.i, e]));
      if (top !== 400) bad.push(`Herd page 1 scrolled to ${top}, not 400`);
      l[1].age = '1m'; // a label changes
      Object.assign(l[0], { state: 'working', shown: 'working' }); // a horse changes band
      l.splice(2, 1); // a lane goes
      l.push({ ...l[0], name: 'audit-new' }); // one comes
      next.fleets[0].queue.shift(); // a queued issue goes
      const n = gone(p, () => herdData(next), '.hh, .hqh');
      if (p.scrollTop !== top) bad.push(`Herd page 1 scrollTop ${top} -> ${p.scrollTop} on a live update`);
      if (n !== 2) bad.push(`${n} horse nodes removed, 2 agents/issues left`);
      for (const [k, e] of keep) if (p.querySelector(`[data-agent="${k}"], [data-i="${k}"]`) !== e && (l.some(x => x.name === k) || next.fleets[0].queue.some(q => String(q.issue) === k))) bad.push(`${k}: horse replaced, not patched`);
      return { view: V, controls: keep.size, outliers: bad };
    },
    'queue-scroll': () => {
      const q = $('#queue'), x = q.cloneNode(true), rows = x.querySelectorAll('details.q'), bad = [];
      scrollTo({ top: 400, behavior: 'instant' });
      const top = scrollY, keep = [...q.querySelectorAll('details')];
      if (top !== 400) bad.push(`Queue scrolled to ${top}, not 400`);
      rows[0].querySelector('summary .m').textContent = '1m'; // a row changes
      rows[rows.length - 1].remove(); // a row goes
      const n = gone(q, () => frag('queue', x.innerHTML), 'details');
      if (scrollY !== top) bad.push(`Queue scrollY ${top} -> ${scrollY} on a live update`);
      if (n !== 1) bad.push(`${n} rows removed, 1 left`);
      if (keep.filter(d => !d.isConnected).length !== 1) bad.push('a kept row was replaced, not patched');
      if (keep.some(d => d.isConnected && d.matches('.q') && !d.open)) bad.push('an open row closed');
      return { view: V, controls: keep.length, outliers: bad };
    },
  };
  const go = async () => {
    await themed; // #241: the phone's theme drawn first
    await VIEWS[V]();
    setTimeout(() => {
      if (V === 'herd2') PG.scrollLeft = PG.clientWidth;
      setTimeout(() => { if (DBG.get('check')) check(); }, 1000);
    }, V === 'toast' ? 5000 : 2000);
  };
  if (document.readyState === 'complete') go(); else addEventListener('load', go);
})();
