// #440 design: the System sheet, as a script the mockup loads after serve.js and herd.js (before board.js, a module, runs).
// [⚙] replaces [◐] on every tab and opens one sheet, its sections in the owner's order: Agent providers (#441's cards),
// Fleets (each fleet's [config] content, #162), Hosts & health (the Host tab), Activity (the Events tab), Notifications &
// devices, Conductor (#404's chat defaults), Appearance (#290's look content). The tabs are queue · lanes · chat.
// Phase 2 folds this into serve.rs `page()` (the sheet's markup, the nav), serve.js (open, index, routes) and
// web/src (the providers, alerts and conductor parts, from API v1). Data: `fixture/system.json`, the real demo for the rest.
(() => {
  const SECS = [
    ['providers', 'agent providers', 'providers'],
    ['fleets', 'fleets', 'fleets'],
    ['hosts', 'hosts & health', 'hosts'],
    ['activity', 'activity', 'activity'],
    ['notifications', 'notifications & devices', 'alerts'],
    ['conductor', 'conductor', 'conductor'],
    ['appearance', 'appearance', 'look'],
  ];
  // The old tabs' theme words name their sections now: `barn · hosts & health`, `logbook · activity`.
  const SW = { hosts: 'tab-host', activity: 'tab-events' };
  const OLD = { host: 'hosts', events: 'activity', config: 'fleets', look: 'appearance' };
  const H = document.documentElement, X = {};
  let F = null; // fixture/system.json

  // 1. The header: [⚙] in [◐]'s place, its dot red while a host card is red. The nav: three tabs.
  $('#look').outerHTML = '<button id=sysb type=button aria-haspopup=dialog aria-label="system: providers, fleets, hosts, activity, alerts, conductor, look">⚙<i class=sd hidden></i></button>';
  $$('nav a[href="#host"], nav a[href="#events"]').forEach(a => a.remove());

  // 2. The sheet: a layer of its own over the tabs (the lane and setup sheets still open above it), at the full detent.
  B.insertAdjacentHTML('beforeend', `<div id=sys hidden><div class=sbd></div><div class="sh lg sys full" role=dialog aria-modal=true aria-labelledby=syst>
<div class=hd><p><b id=syst>system</b><button type=button class=sxc aria-label="close system">close</button></p></div>
<div class=sxi role=navigation aria-label="system sections">${SECS.map(([id, , c]) => `<a href=#system/${id} data-s=${id}><span>${c}</span><i class=sd hidden></i></a>`).join('')}</div>
<div class=top>${SECS.map(([id, t]) => `<section class=sy id=sy-${id} data-s=${id} aria-labelledby=syh-${id}><h2 id=syh-${id} class=syh><span class=syt data-p="${t}">${esc(t)}</span><small class=sym></small></h2><div class=syb></div></section>`).join('')}
<p class=syf>herdsman · demo/fleet.toml</p></div></div></div>`);
  const S = $('#sys'), top = S.querySelector('.top'), body = id => $(`#sy-${id}>.syb`), meta = (id, t) => { $(`#sy-${id} .sym`).textContent = t; };
  // Host and Events keep their ids, so board.js, `/live` and frag() paint them where they now live.
  body('hosts').append($('#host'));
  body('activity').append($('#events'));
  $$('#host, #events').forEach(s => s.hidden = false);

  // The Host part, as it comes: ALERTS goes to Notifications, the accounts and deepseek rows to Agent providers, and a
  // host's [sign in ▾] keeps its non-agent logins (gh). Everything else is the Host tab's, unchanged.
  const paint = frag;
  window.frag = (id, html) => {
    if (id === 'host') {
      const t = document.createElement('template');
      t.innerHTML = html;
      t.content.querySelectorAll('.grp').forEach(g => { if (g.querySelector('.gn').textContent === 'ALERTS') g.remove(); });
      t.content.querySelectorAll('.k').forEach(k => { if (['accounts', 'deepseek'].includes(k.textContent)) k.closest('.gr, .rw').remove(); });
      t.content.querySelectorAll('[data-su=claude]').forEach(b => b.remove());
      html = t.innerHTML;
    }
    paint(id, html);
    if (id === 'host') alarm();
  };
  // [⚙]'s dot and the Hosts index chip's: a host card is red (past a limit, under a floor, or down).
  function alarm() {
    const r = [...$$('#host .hcd')].filter(c => c.dataset.c === 'r').map(c => c.dataset.h);
    const why = r.length ? `${r.join(', ')}: ${($(`#host .hcd[data-h="${CSS.escape(r[0])}"] .hw`)?.textContent || 'alert').split(' · ').pop()}` : '';
    $$('#sysb .sd, .sxi a[data-s=hosts] .sd').forEach(d => { d.hidden = !r.length; });
    $('#sysb').setAttribute('aria-label', 'system' + (r.length ? ` (host alert: ${why})` : ''));
    $('#sysb').title = r.length ? `host alert · ${why}` : 'system';
    $('#sy-hosts').dataset.t = r.length ? 'r' : '';
    meta('hosts', r.length ? `alert · ${why}` : `${$$('#host .hcd').length} hosts · all clear`);
  }

  // 3. Agent providers (#441): a card per provider, an account or key per row, [use] asks its scope, then one "move N
  // running lanes" prompt. The token or key is never shown: a sign-in runs in the setup terminal (#303), a key is pasted
  // into a masked field and checked before it is saved.
  const gauge = p => { const n = Math.round(p / 100 * 12), t = p >= 100 ? 'r' : p >= 80 ? 'a' : 'g'; return `<span class="bar ${t}">▕${'█'.repeat(n)}<i>${'░'.repeat(12 - n)}</i>▏</span> <b class=${t}>${p}%</b>`; };
  const ORD = ['', '1st', '2nd', '3rd', '4th', '5th'];
  function prow(p, r) {
    const state = { ok: ['g', p.kind === 'keys' ? `✓ ${r.balance}` : 'ok'], capped: ['a', `capped until ${r.until}`], invalid: ['r', p.kind === 'keys' ? 'invalid: replace it' : 'token invalid: sign in again'] }[r.state];
    const badge = r.order === 1 ? `<span class="tag g">active · 1st</span>` : `<span class="tag m">${ORD[r.order]}</span>`;
    const roles = r.roles.length ? `used by ${r.roles.join(', ')}${r.lanes ? ` · ${r.lanes} running lane${r.lanes > 1 ? 's' : ''}` : ''}` : 'in no role';
    const use = (r.meters || []).map(m => `<span class=pm><span class=k>${m.label}</span>${gauge(m.pct)}</span>`).join('');
    const ago = r.ago ? `<span class=mt>${p.kind === 'keys' ? 'checked' : 'usage'} ${r.ago} ago</span>` : '';
    const fix = r.state === 'invalid' ? (p.kind === 'keys' ? 'replace' : 'sign in again') : p.kind === 'accounts' ? 'sign in again' : '';
    const btns = [r.order !== 1 && r.state !== 'invalid' ? `<button type=button data-use="${r.name}">use</button>` : '', fix && `<button type=button data-fix="${r.name}">${fix}</button>`, `<button type=button data-rm="${r.name}" data-confirm="remove ${p.name} ${r.name}? its ${p.kind === 'keys' ? 'key' : 'token'} leaves the keychain">remove</button>`];
    return `<div class=pvr data-p=${p.id} data-a="${r.name}" data-t=${state[0]}><p class=pvh><b class=pvn>${esc(r.name)}</b>${badge}<span class="pvs ${state[0]}">${esc(state[1])}</span></p>`
      + `<p class=pvm>${esc(roles)}</p>${use || ago ? `<p class=pvu>${use}${ago}</p>` : ''}<p class=pvb>${btns.filter(Boolean).join('')}</p></div>`;
  }
  function pcard(p) {
    const order = p.rows.filter(r => r.order).map(r => r.name).join(' → ');
    const how = p.how ? `<p class=pvk><a href="${p.how}" target=_blank rel=noopener>[get a key]</a> <span class=m>platform.deepseek.com → API keys</span></p>` : '';
    return `<article class=pvc data-p=${p.id}><div class=pvch><b>${esc(p.name)}</b><span class=m>${esc(p.sub)}</span><span class="m pvo">order ${esc(order)}</span></div>`
      + `${p.rows.map(r => prow(p, r)).join('')}<p class=pva><button type=button data-add=${p.id}>${esc(p.add)}</button></p>${how}</article>`;
  }
  function providers() {
    body('providers').innerHTML = `<div class=pvg>${F.providers.map(pcard).join('')}</div><p class=pvf>secrets stay in the main host's keychain · remote lanes get theirs over ssh stdin · a conductor proposes, you tap</p>`;
    const n = F.providers.flatMap(p => p.rows), bad = n.filter(r => r.state !== 'ok').length;
    meta('providers', `${F.providers.length} providers${bad ? ` · ${bad} need you` : ' · all ok'}`);
    $('#sy-providers').dataset.t = n.some(r => r.state === 'invalid') ? 'a' : '';
  }
  // [use] B: its scope first (every role, or one fleet's role), then, with lanes running on another account, the prompt.
  function scope(row, f, role) {
    row.querySelector('.pvq')?.remove();
    const a = row.dataset.a, one = !!f, fl = Object.keys(F.scopes);
    const chip = (k, v, on, lbl) => `<button type=button data-${k}="${v}" aria-pressed=${on}>${lbl || v}</button>`;
    const pick = one ? `<p class=pvl><span class=k>fleet</span>${fl.map(x => chip('qf', x, x === f)).join('')}</p><p class=pvl><span class=k>role</span>${F.scopes[f].map(x => chip('qr', x, x === role)).join('')}</p>` : '';
    const what = one ? `${f} · ${role}` : 'every role';
    row.insertAdjacentHTML('beforeend', `<div class=pvq role=group aria-label="use ${a} for"><p class=pvqh>use ${a} for</p><p class="chips pvl">${chip('qs', 'all', !one, 'every role')}${chip('qs', 'one', one, "one fleet's role")}</p>${pick}`
      + `<p class=pvb><button type=button data-go="${a}"><span>use <i class=an>${a}</i> for ${what}</span></button><button type=button data-qx>cancel</button></p><p class=mt>moves ${a} to the front there · new lanes use it at once · logged by you</p></div>`);
  }
  function move(row) {
    row.querySelector('.pvq')?.remove();
    const a = row.dataset.a, on = F.running.filter(r => r.account !== a);
    row.insertAdjacentHTML('beforeend', `<div class="pvq pmv" role=alertdialog aria-label="move running lanes"><p class=pvqh><b>move ${on.length} running lanes to ${a}?</b></p>`
      + `<ul class=ml>${on.map(r => `<li>${esc(r.agent)} <span class=m>on ${r.account}</span></li>`).join('')}</ul>`
      + `<p class=mt>each finishes its turn, then resumes the same session on ${a}</p><p class=pvb><button type=button data-mv=yes>move ${on.length} lanes</button><button type=button data-qx><span>keep them on <i class=an>${on[0].account}</i></span></button></p></div>`);
  }
  // [+ add key]: a name, a masked paste field, checked against DeepSeek's balance endpoint before it is saved.
  function addkey(card, err) {
    card.querySelector('.pvq')?.remove();
    card.querySelector('.pva').insertAdjacentHTML('beforebegin', `<form class="pvq pk2" autocomplete=off><p class=pvqh>add a DeepSeek key</p>`
      + `<p class=pvk><a href=https://platform.deepseek.com/api_keys target=_blank rel=noopener>[get a key]</a> <span class=m>platform.deepseek.com → API keys → create, then copy it</span></p>`
      + `<label class=pvl><span class=k>name</span><input name=name value="${err ? 'backup' : ''}" placeholder="e.g. backup" spellcheck=false></label>`
      + `<label class=pvl><span class=k>key</span><input name=key type=password ${err ? 'value="sk-0000000000000000"' : ''} placeholder="paste the key" spellcheck=false autocapitalize=off></label>`
      + (err ? `<p class="st r">not saved: DeepSeek says 401 Authentication Fails (the key is wrong or revoked)</p>` : '')
      + `<p class=pvb><button>${err ? 'check again' : 'check and save'}</button><button type=button data-qx>cancel</button></p><p class=mt>checked with DeepSeek's balance endpoint · kept in the keychain · never shown again</p></form>`);
  }

  // 4. Fleets: each fleet's [config] content (#162) in a row that opens; the same groups and pickers, the bar its colour.
  function fleets() {
    const ks = Object.fromEntries([...$$('#queue button.cfg')].map(b => [b.dataset.f, [...b.closest('.fc').classList].find(c => c.startsWith('k-'))]));
    const ts = [...$$('template[data-cfg]')];
    body('fleets').innerHTML = ts.map(t => {
      const f = t.dataset.cfg, d = t.content, dd = k => [...d.querySelectorAll('.kv2 dt')].find(x => x.textContent === k)?.nextElementSibling?.textContent || '';
      const roles = [...d.querySelectorAll('.role .rh b')].map(b => b.textContent).join(' · ');
      return `<details class="sfl ${ks[f] || 'k-g'}" data-f="${esc(f)}"><summary><i class=fq>■</i><b>${esc(f)}</b><span class=m>${esc(dd('integration'))} · ${esc(roles)}</span><i class=cv></i></summary><div class=cs>${t.innerHTML}</div></details>`;
    }).join('');
    body('fleets').querySelectorAll('[data-pipe]').forEach(pipeline);
    tidy(body('fleets'));
    body('fleets').querySelectorAll('form button, form select:not([data-off])').forEach(c => c.disabled = DEMO);
    meta('fleets', `${ts.length} repos · config demo/fleet.toml`);
  }

  // 5. Notifications & devices: push (this phone's [enable alerts]), the alert devices, the paired devices, the
  // global notifiers and the Telegram bridge. A fleet's own notifiers stay in its Fleets row (ALERTS).
  function notifications() {
    const a = F.alerts, age2 = s => s < 3600 ? `${Math.round(s / 60)}m` : s < 86400 ? `${Math.round(s / 3600)}h` : `${Math.round(s / 86400)}d`;
    const dl = rows => `<dl class=kv2>${rows.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('')}</dl>`;
    const g = (t, hint, h) => `<section class=cg><h3>${t}<span>${hint}</span></h3>${h}</section>`;
    const tg = a.telegram;
    body('notifications').innerHTML = `<div class=cs>`
      + g('Push', 'web push to the home-screen app', dl([['push', `<b class=${a.push.tone}>${esc(a.push.detail)}</b>`], ['this phone', '<button type=button id=push2>enable alerts</button> <span class=mt>home-screen app only</span>']]))
      + g('Alert devices', `${a.subs.length} get alerts`, a.subs.map(s => `<form class=role2><span>${esc(s.label)}<span class=mt>alerts on ${age2(s.created)} ago</span></span><button type=button>remove</button></form>`).join(''))
      + g('Paired devices', `${a.devices.length} paired`, a.devices.map(d => `<form class=role2><span>${esc(d.label)}<span class=mt>paired ${age2(d.created)} ago · used ${age2(d.used)} ago</span></span><button type=button>remove</button></form>`).join('') + '<p class=pva><button type=button class=pairb2>pair another device</button></p><p class=mt>then <code>herdsman serve approve &lt;code&gt;</code> on the main host</p>')
      + g('Notifiers', 'global · edit fleet.toml', dl(a.notify.map(([k, v]) => [k, esc(v)])))
      + g('Telegram bridge', tg.on ? 'on' : 'off', dl([['state', tg.on ? `<b class=g>on</b> · polled ${tg.polled} ago` : '<b class=m>off</b>'], ['chat', esc(tg.chat)], ['owner', esc(tg.owner)], ['topics', `${tg.topics} threads`], ['token', '<span class=m>keychain · TELEGRAM_BOT_TOKEN</span>']]))
      + `</div>`;
    meta('notifications', `push ${a.push.tone === 'g' ? 'on' : 'off'} · ${a.devices.length} devices · telegram ${tg.on ? 'on' : 'off'}`);
  }

  // 6. Conductor: `[chat]`'s defaults for new threads (#404); each fleet's own conductor stays in its Fleets row (#401).
  function conductor() {
    const c = F.conductor, row = (k, cur, opts) => `<form class=chips aria-label=${k}><b class=m>${k}</b>${opts.map(o => `<button type=button aria-pressed=${[].concat(cur).includes(o)}${DEMO ? ' disabled' : ''}>${k === 'account' ? `<span class=an>${esc(o)}</span>` : esc(o)}</button>`).join('')}</form>`;
    const acc = F.providers[0].rows.map(r => r.name);
    body('conductor').innerHTML = `<div class=cs><section class=cg><h3>Chat defaults<span>[chat] · ${c.set ? 'set' : 'unset → each fleet\'s conductor'}</span></h3>`
      + `<p class=rk>a new thread starts with these · a thread keeps its harness · its composer changes model and effort</p>`
      + row('harness', c.harness, c.harnesses) + row('model', c.model, c.models) + row('effort', c.effort, c.efforts) + row('account', c.accounts, acc) + `</section>`
      + `<section class=cg><h3>Per fleet<span>in Fleets → Conductor</span></h3><dl class=kv2>${c.fleets.map(([f, v]) => `<dt>${esc(f)}</dt><dd>${esc(v)} <a class=go href=#system/fleets/${f}>[edit]</a></dd>`).join('')}</dl></section></div>`;
    meta('conductor', `${c.harness} · ${c.model} · ${c.effort}`);
  }

  // 7. Appearance: the look sheet's content (#290), in place: THEME, COLOURS, STYLE, applied at once (serve.js lpick).
  function appearance() {
    const grp = (k, title, hint, rows) => {
      const g = el('section', 'cg lkg'), t = el('h3', '', title), l = el('div', 'lkl');
      t.append(el('span', '', hint));
      l.setAttribute('role', 'listbox');
      l.setAttribute('aria-label', k);
      l.append(...rows);
      g.append(t, l);
      return g;
    };
    const th = Object.keys(SD.t).map(n => { const s = el('i', 'lka'); lsprite(n).then(x => { s.innerHTML = x; }); return lrow('t', n, n === H.dataset.theme, s); });
    const sw = s => { const i = el('i', 'lsw'); i.append(Object.assign(el('b', 'aa', 'Aa'), { style: `background:${s.tokens['--bg']};color:${s.tokens['--fg']}` }), ...['--g', '--a', '--r'].map(t => Object.assign(el('b'), { style: `background:${s.tokens[t]}` }))); return i; };
    const ok = Object.keys(SD.s).filter(n => !fails(SD.s[n]).length), no = Object.keys(SD.s).length - ok.length;
    const sc = ok.map(n => lrow('s', n, n === H.dataset.scheme, sw(SD.s[n]), n === choose(SD.s, null, SD.t[H.dataset.theme])));
    const px = el('i', 'lka'), as = el('i', 'lka');
    lsprite(H.dataset.theme).then(x => { px.innerHTML = x; });
    as.append(el('b', 'asc', TT && TT.ascii_rows && TT.ascii_rows.impl));
    const st = TM.includes('ascii') ? [grp('style', 'style', 'rows, lanes, host, loading', [lrow('y', 'pixel', B.dataset.mode !== 'ascii', px), lrow('y', 'ascii', B.dataset.mode === 'ascii', as)])] : [];
    const b = body('appearance');
    b.className = 'syb look';
    b.replaceChildren(grp('theme', 'theme', 'sprites, scene, icon', th), grp('colours', 'colours', `${ok.length} pass the gate${no ? ` · ${no} refused: hosts` : ''}`, sc), ...st,
      el('p', 'lkn', 'this phone only · the home-screen icon keeps its install-time look'));
    meta('appearance', `${H.dataset.theme} · ${H.dataset.scheme}${B.dataset.mode === 'ascii' ? ' · ascii' : ''}`);
  }

  // Open at a section (and a fleet): the index follows the scroll; the hash is `#system/<section>[/<fleet>]`.
  let back = 'queue';
  function open(sec = 'providers', sub = '', smooth = false) {
    if (!SECS.some(s => s[0] === sec)) sec = 'providers';
    if (S.hidden) { back = H.dataset.tab; S.hidden = false; void S.offsetHeight; }
    S.classList.add('on');
    H.classList.add('sysopen');
    appearance();
    if (sec === 'fleets' && sub) { const d = $(`.sfl[data-f="${CSS.escape(sub)}"]`); if (d) d.open = true; }
    const t = sec === 'fleets' && sub ? $(`.sfl[data-f="${CSS.escape(sub)}"]`) : $('#sy-' + sec);
    top.scrollTo({ top: sec === 'providers' && !sub ? 0 : t.offsetTop, behavior: smooth ? 'smooth' : 'instant' });
    spy();
    history.replaceState(null, '', `#system/${sec}${sub ? '/' + sub : ''}`);
  }
  function shut() {
    S.classList.remove('on');
    H.classList.remove('sysopen');
    S.hidden = true;
    history.replaceState(null, '', '#' + back);
    $('#sysb').focus({ preventScroll: true });
  }
  function spy() {
    const y = top.scrollTop + 8, at = [...$$('.sy')].filter(s => s.offsetTop <= y).pop() || $('.sy');
    const end = top.scrollTop + top.clientHeight >= top.scrollHeight - 2, cur = end ? $$('.sy')[SECS.length - 1] : at;
    $$('.sxi a').forEach(a => {
      const on = a.dataset.s === cur.dataset.s;
      a.classList.toggle('on', on);
      if (on) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current');
      if (on) { const n = a.parentNode, l = a.offsetLeft - n.offsetLeft; if (l < n.scrollLeft || l + a.offsetWidth > n.scrollLeft + n.clientWidth) n.scrollLeft = l - 16; }
    });
  }
  X.open = open; X.shut = shut; X.spy = spy; X.scope = scope; X.move = move; X.addkey = addkey;
  top.addEventListener('scroll', () => requestAnimationFrame(spy), { passive: true });

  // Routes: `#system/<section>[/<fleet>]` opens there; the old `#host`, `#events`, `#config[/<fleet>]` (and a stored
  // `host`/`events` tab) land on the Queue tab with the sheet at hosts, activity, fleets.
  function route() {
    const [h, sec, sub] = decodeURIComponent(location.hash.slice(1)).split('/');
    if (h === 'system') return open(sec, sub), true;
    if (OLD[h]) {
      if (['host', 'events'].includes(H.dataset.tab)) tab('queue');
      return open(OLD[h], sec), true;
    }
    return false;
  }
  if (['host', 'events'].includes(H.dataset.tab)) tab('queue');
  addEventListener('hashchange', route);
  document.addEventListener('click', e => {
    const t = e.target;
    if (t.closest('#sysb')) return open();
    if (t.closest('.sxc') || t.closest('.sbd')) return shut();
    const ix = t.closest('.sxi a, a.go');
    if (ix) { e.preventDefault(); const [, s, f] = ix.getAttribute('href').slice(1).split('/'); return open(s, f, true); }
    // [config] on a Queue card opens its fleet's row here (the button stays, a shortcut).
    const cf = t.closest('#queue .cfg');
    if (cf) { e.stopImmediatePropagation(); return open('fleets', cf.dataset.f); }
    const r = t.closest('.pvr'), q = t.closest('.pvq');
    if (t.closest('[data-use]')) return scope(r);
    if (t.closest('[data-qs]')) { const one = t.closest('[data-qs]').dataset.qs === 'one'; return scope(r, one ? 'acme-api' : '', one ? 'impl' : ''); }
    if (t.closest('[data-qf]')) { const f = t.closest('[data-qf]').dataset.qf; return scope(r, f, F.scopes[f][0]); }
    if (t.closest('[data-qr]')) return scope(r, q.querySelector('[data-qf][aria-pressed=true]').dataset.qf, t.closest('[data-qr]').dataset.qr);
    if (t.closest('[data-go]')) return move(r);
    if (t.closest('[data-qx], [data-mv]')) return q.remove();
    if (t.closest('[data-add=deepseek]')) return addkey(t.closest('.pvc'));
  }, true);
  document.addEventListener('keydown', e => { if (e.key === 'Escape' && !S.hidden && !$('#sheet.on')) shut(); });

  // The data: the fixture for the new parts; the rest is the demo's own.
  window.SYS = X;
  window.sysReady = fetch('../fixture/system.json').then(r => r.json()).then(j => {
    F = j;
    providers();
    fleets();
    notifications();
    conductor();
    appearance();
    $$('.sy').forEach(s => { const k = SW[s.dataset.s], p = s.querySelector('.syt'); if (k && TW()[k]) p.innerHTML = tw(k, p.dataset.p); });
    return (window.boardReady || Promise.resolve()).then(() => { alarm(); route(); });
  });
})();
