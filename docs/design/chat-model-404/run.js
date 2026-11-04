// #404: the composer's run chips. chat.rs gives each composer its run (`data-fleet`, and in a started thread the
// thread's `data-h` harness, `data-m` model, `data-e` effort), and the tab one `#runs` JSON: per repo the default
// harness and, per harness, the models and efforts it offers with its defaults. A new thread's picks wait in RUNP until
// its first message carries them (`op=new`); a live swap repaints the chips from them, as ATT's thumbnails.
var RUNS = null, RUNP = {}, RUNT = '';
const runCfg = () => RUNS || (RUNS = JSON.parse($('#runs')?.textContent || '{}'));
// A composer's run: { repo, h, m, e, c (its harness's choices), all (the repo's), draft }.
function runOf(tw) {
  const repo = tw.dataset.repo || tw.dataset.fleet, all = runCfg()[repo];
  if (!all) return null;
  const draft = !!tw.dataset.repo, p = RUNP[tw.dataset.t] || {}, h = p.h || tw.dataset.h || all.def, c = all.h[h];
  return { repo, h, m: p.m || tw.dataset.m || c.model, e: p.e || tw.dataset.e || c.effort, c, all, draft };
}
function runChip(cls, label, a, b) {
  const s = el('span', '', a);
  if (b) s.append(el('i', '', ' · ' + b));
  s.append(' ▾');
  return Object.assign(el('button', cls), { type: 'button', ariaLabel: label, ariaHasPopup: 'dialog' }).appendChild(s).parentNode;
}
// The chips under the box; a new thread's header names its run (`acme-api · claude · opus`).
function runPaint(root = $('#chat')) {
  if (!root || RUNP === undefined) return; // before the `var` line runs
  root.querySelectorAll('.tw').forEach(tw => {
    tw.querySelector('.rcs')?.remove();
    const r = runOf(tw), off = tw.querySelector('textarea').disabled;
    if (!r) return;
    const row = tw.appendChild(el('div', 'rcs'));
    if (r.draft) row.append(runChip('rh', 'harness: ' + r.h, r.h));
    if (r.c.models.length > 1 || r.c.efforts.length > 1) row.append(runChip('rm', `model ${r.m}, effort ${r.e}`, r.m, r.e === 'none' ? '' : r.e));
    else row.append(el('span', 'rn', `${r.h} sets its own model and effort`));
    row.querySelectorAll('button').forEach(b => { b.disabled = off; });
    if (r.draft) tw.closest('.cvp').querySelector('.th .ds').textContent = [r.repo === 'all' ? 'all repos' : r.repo, r.h, r.m].filter(Boolean).join(' · ');
  });
}
// The picker: the board's sheet with HARNESS (a new thread), MODEL and EFFORT, each a listbox; the current row ticked.
function rrow(k, v, on, note) {
  const o = el('div', 'lko');
  o.tabIndex = on ? 0 : -1;
  o.setAttribute('role', 'option');
  o.setAttribute('aria-selected', on);
  o.dataset.k = k;
  o.dataset.v = v;
  o.append(el('span', 'ln', v));
  if (note) o.append(el('small', '', note));
  return o;
}
function runSheet(tw, at, kb) {
  const r = runOf(tw), re = shown() && RUNT === tw.dataset.t && sh.classList.contains('rpk');
  const f = re && document.activeElement.closest('.lko'); // a keyboard pick keeps its focus on the list
  RUNT = tw.dataset.t;
  agent = null;
  sh.className = 'sh lg rpk' + (r.draft ? ' n3' : '');
  $('.hd b').textContent = r.draft ? 'new thread · conductor' : 'conductor · ' + tw.closest('.cvp').querySelector('.th b').textContent;
  const grp = (k, title, hint, rows) => {
    const g = el('section', 'cg lkg'), t = el('h3', '', title), l = el('div', 'lkl');
    t.append(el('span', '', hint));
    l.setAttribute('role', 'listbox');
    l.setAttribute('aria-label', k);
    l.append(...rows);
    g.append(t, l);
    return g;
  };
  const def = (a, b) => a === b ? '(default)' : '';
  const hs = Object.entries(r.all.h).map(([h, c]) => rrow('h', h, h === r.h, `${c.model || "its profile’s model"}${c.effort === "none" ? "" : " · " + c.effort}${h === r.all.def ? ' (default)' : ''}`));
  const ms = r.c.models.map(m => rrow('m', m, m === r.m, def(m, r.c.model)));
  const es = r.c.efforts.map(e => rrow('e', e, e === r.e, e === 'none' ? (def(e, r.c.effort) || `${r.h}’s own`) : def(e, r.c.effort)));
  $('.top').replaceChildren(...(r.draft ? [grp('harness', 'harness', 'fixed once the thread starts', hs)] : []),
    grp('model', 'model', r.h, ms.length ? ms : [rrow('m', r.m || 'the profile’s model', true, `set by the ${r.h} profile`)]),
    grp('effort', 'effort', r.m || r.h, es),
    el('p', 'lkn', r.draft ? 'for this thread; your first message starts it on this run' : 'from the next turn: the conductor resumes its session on it · logged in this thread'));
  if (f || kb) $(`.rpk .lko[data-k=${at}][aria-selected=true]`)?.focus({ preventScroll: true });
  if (!re) show(innerWidth >= 720 ? 'half' : 'mid');
}
// A tap applies at once and the sheet stays open: a new thread keeps the pick (a new harness takes its defaults); a
// started thread logs it (`op=model`) and its line shows in the thread.
function runPick(o) {
  const tw = $(`#chat .tw[data-t="${CSS.escape(RUNT)}"]`), r = tw && runOf(tw);
  if (!r || o.getAttribute('aria-selected') === 'true') return;
  const p = { h: r.h, m: r.m, e: r.e, [o.dataset.k]: o.dataset.v };
  if (o.dataset.k === 'h') Object.assign(p, { m: r.all.h[p.h].model, e: r.all.h[p.h].effort });
  if (!r.all.h[p.h].efforts.includes(p.e)) p.e = r.all.h[p.h].effort;
  if (r.draft) RUNP[RUNT] = p;
  else {
    Object.assign(tw.dataset, { m: p.m, e: p.e });
    chatPost({ op: 'model', t: RUNT, model: p.m, effort: p.e });
  }
  runPaint();
  runSheet(tw, o.dataset.k);
}
// A new thread's first message carries its run.
const runNew = f => { const r = runOf(f); return r ? { harness: r.h, model: r.m, effort: r.e } : {}; };
document.addEventListener('click', e => {
  const c = e.target.closest('#chat .rcs button'), o = e.target.closest('.rpk .lko');
  if (c) runSheet(c.closest('.tw'), c.classList.contains('rh') ? 'h' : 'm', e.detail === 0);
  else if (o) runPick(o);
});
// Keyboard, as the look sheet: ↑/↓ (Home/End) within a list, Tab between them, Enter or Space picks.
document.addEventListener('keydown', e => {
  const o = e.target.closest && e.target.closest('.rpk .lko');
  if (!o) return;
  if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); return runPick(o); }
  const all = [...o.parentNode.children], to = all[{ ArrowDown: all.indexOf(o) + 1, ArrowUp: all.indexOf(o) - 1, Home: 0, End: all.length - 1 }[e.key]];
  if (!to) return;
  e.preventDefault();
  to.focus();
});
