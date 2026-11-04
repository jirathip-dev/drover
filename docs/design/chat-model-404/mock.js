// #404 design mock only: stands in for what chat.rs renders in phase 2, on the demo export (whose events predate
// #404). It adds the tab's `#runs` JSON (the README's [chat] example), each composer's run attributes, the headers
// (`repo · harness · model · state`) and the server's side of `op=model` (the logged line). Not part of phase 2.
(() => {
  const E = ['none', 'low', 'medium', 'high', 'xhigh'];
  const H = { claude: { models: ['opus', 'sonnet'], efforts: E, model: 'opus', effort: 'high' },
    hermes: { models: ['kimi-k2.5', 'gpt-5.6-luna'], efforts: E, model: 'kimi-k2.5', effort: 'high' },
    dsh: { models: [], efforts: ['none'], model: '', effort: 'none' } };
  const repos = ['all', ...[...document.querySelectorAll('#chat .tw[data-repo]')].map(f => f.dataset.repo).filter(r => r !== 'all')];
  $('#chat').append(Object.assign(document.createElement('script'), { type: 'application/json', id: 'runs',
    textContent: JSON.stringify(Object.fromEntries(repos.map(r => [r, { def: 'claude', h: H }]))) }));
  // The demo's threads: t2 started on hermes; t1 and t3 predate #404 (no run logged: the default's); t4 on claude.
  const T = { t1: ['all'], t2: ['acme-api', 'hermes', 'kimi-k2.5', 'medium'], t3: ['all'], t4: ['acme-api', 'claude', 'opus', 'high'] };
  for (const [t, [fleet, h = 'claude', m = 'opus', e = 'high']] of Object.entries(T)) {
    const v = $(`#chat .cvp[data-t="${t}"]`), tw = v && v.querySelector('.tw');
    if (!tw) continue;
    Object.assign(tw.dataset, { fleet, h, m, e });
    const ds = v.querySelector('.th .ds'), st = ds.textContent.split(' · ').pop();
    ds.lastChild.textContent = [fleet, h, m, ...(['asleep', 'working', 'awake'].includes(st) ? [st] : [])].join(' · ');
  }
  // The server's `op=model`: one `thread-model` event, its line `model A → B · from the next turn · you`; changes with no
  // turn between them render as one line (the first's from, the last's to).
  const post = chatPost;
  chatPost = async (d, f) => {
    if (d.op !== 'model') return post(d, f);
    const v = $(`#chat .cvp[data-t="${CSS.escape(d.t)}"]`), tc = v.querySelector('.tc'), last = tc.lastElementChild;
    const line = last && last.matches('.qs.rm') ? last : tc.appendChild(el('p', 'qs rm'));
    const to = d.model + (d.effort === 'none' ? '' : ' · ' + d.effort);
    if (!line.dataset.from) line.dataset.from = v.querySelector('.th .ds').dataset.run;
    line.textContent = `model ${line.dataset.from} → ${to} · from the next turn · you`;
    const ds = v.querySelector('.th .ds'), p = ds.lastChild.textContent.split(' · ');
    p[2] = d.model;
    ds.lastChild.textContent = p.join(' · ');
    ds.dataset.run = to;
    return true;
  };
  document.querySelectorAll('#chat .cvp:not([data-t^="new-"])').forEach(v => {
    const tw = v.querySelector('.tw');
    if (tw.dataset.m) v.querySelector('.th .ds').dataset.run = tw.dataset.m + (tw.dataset.e === 'none' ? '' : ' · ' + tw.dataset.e);
  });
  runPaint();
})();
