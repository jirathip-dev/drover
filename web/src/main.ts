// #927: the phone page draws the screens cooee draws (cooee-plugin.toml's places, `PL` from the page shell): each
// header-strip place is a tab, the menu-row places open from [⚙], the chat-cards place is the cards above every tab.
// A screen is fetched, drawn block by block, patched by its `stream`, and its blocks post `/action`. No per-tab code.
import { type Act, type Ctx, el, kids, render, values } from './blocks';
import { apply, type Blk } from './ops';
import { R, colours, esc, get, put, schemes, theme, themes } from './theme';

type Place = { type: string; id?: string; label?: string; subtitle?: string; screen?: string };
declare const PL: Place[];
const B = document.body, H = document.documentElement;
const ro = 'ro' in B.dataset, demo = 'demo' in B.dataset;
const $ = <T extends Element = HTMLElement>(s: string) => document.querySelector<T>(s)!;
const exported = B.dataset.root === '';
const url = (p: string) => new URL(p.replace(/^\//, '') + (exported && /^\/screens\/[^?]*$/.test(p) ? '/index.html' : ''), R).href;

type View = { path: string; model: Blk | null; at: number; live: boolean; es?: EventSource; root: HTMLElement; open: Set<string>; back: string[] };
const view = (path: string, root: HTMLElement): View => ({ path, model: null, at: 0, live: false, root, open: new Set(), back: [] });
const tabs = PL.filter(p => p.type === 'header-strip' && p.screen).map(p => ({ ...p, v: view(p.screen!, el('section', 'tab')), a: el('a') }));
const cards = PL.some(p => p.type === 'chat-cards') ? view('/screens/cards', $('#cards')) : null;
let cur = tabs.find(t => t.id === (location.hash.slice(1) || get('tab'))) ?? tabs[0];

const dot = $('#dot'), stale = $('#stale');
function sync() {
  const v = cur.v, live = !!v.live && navigator.onLine;
  dot.className = demo ? 'demo' : live ? 'g' : navigator.onLine ? 'a' : 'r';
  H.toggleAttribute('data-focus', !!v.model && !Array.isArray(v.model) && !!v.model.focus);
  const t = v.at ? new Date(v.at) : null, hm = t && t.toTimeString().slice(0, 5), old = t ? Date.now() - t.getTime() : 0;
  stale.hidden = !t || (live && old < 120_000);
  if (t) stale.textContent = `last update ${hm}${old > 60_000 ? ` · ${Math.round(old / 60_000)}m ago` : ''}`;
}

// The last copy of each screen, painted at once (offline too), its age on the strip until a fresh one lands.
const cached = (p: string): { t: number; v: Blk } | null => { try { return JSON.parse(get('scr:' + p) ?? 'null'); } catch { return null; } };
async function fetchScreen(p: string): Promise<Blk> {
  const get = () => fetch(url(p), { cache: 'no-store' });
  const r = await get().catch(() => new Promise<Response>(ok => setTimeout(() => ok(get()), 300)));
  const j = await r.json().catch(() => ({ line: `${r.status}` }));
  if (!r.ok) throw Object.assign(new Error(j.line ?? String(r.status)), { status: r.status });
  return j;
}

function ctx(v: View, screen: string): Ctx {
  return { screen, open: v.open, act: (a, b) => act(a, v, screen, b), tap };
}
function rows(m: unknown, out = new Map<string, string>()) {
  if (Array.isArray(m)) m.forEach(x => rows(x, out));
  else if (m && typeof m === 'object') {
    const o = m as Blk;
    if ((o.type === 'row' || o.type === 'card') && o.id) out.set(o.id, JSON.stringify(o));
    Object.values(o).forEach(x => rows(x, out));
  }
  return out;
}
function paint(v: View, before?: Blk | null) {
  const m = v.model;
  if (!m) return;
  const c = ctx(v, Array.isArray(m) ? 'cards' : m.id);
  const body = Array.isArray(m) ? kids(m, c) : [render(m, c)].filter((e): e is HTMLElement => !!e);
  if (v.back.length && body[0]) {
    const b = el('button', '', '<span>‹ back</span>');
    b.type = 'button';
    b.onclick = () => go(v, v.back.pop()!, false);
    body[0].prepend(b);
  }
  v.root.replaceChildren(...body);
  if (before) {
    const was = rows(before);
    rows(m).forEach((j, id) => { if (was.has(id) && was.get(id) !== j) v.root.querySelector(`[data-id="${CSS.escape(id)}"]`)?.classList.add('fl'); });
  }
  if (v === cur.v) sync();
  badges();
}
function badges() {
  tabs.forEach(t => {
    const n = t.v.model && !Array.isArray(t.v.model) ? t.v.model.badge : null;
    t.a.innerHTML = `<span>${esc(t.label?.toLowerCase())}${n ? `<b class=n>·${n}</b>` : ''}</span>`;
  });
}

async function load(v: View, quiet = false) {
  const c = cached(v.path);
  if (!v.model && c) [v.model, v.at, v.live] = [c.v, c.t, false];
  if (!v.model || !quiet) paint(v);
  const slow = setTimeout(() => ($('#ld').hidden = false), 300);
  try {
    const m = await fetchScreen(v.path), before = v.model;
    [v.model, v.at] = [m, Date.now()];
    put('scr:' + v.path, JSON.stringify({ t: v.at, v: m }));
    paint(v, before);
    if (!v.es) stream(v);
  } catch (e) {
    if ((e as { status?: number }).status === 401) unpaired();
    else if (!v.model) v.root.replaceChildren(el('p', 'msg r', esc((e as Error).message)));
    sync();
  } finally {
    clearTimeout(slow);
    $('#ld').hidden = true;
  }
}

function stream(v: View) {
  const s = v.model && !Array.isArray(v.model) ? v.model.stream : v === cards ? v.path + '/stream' : null;
  if (!s || exported || !('EventSource' in window)) return;
  const es = (v.es = new EventSource(url(s)));
  es.onopen = () => { v.live = true; sync(); };
  es.onerror = () => { v.live = false; sync(); };
  es.onmessage = e => {
    const d = JSON.parse(e.data) as { ops: Blk[] }, before = v.model && JSON.parse(JSON.stringify(v.model));
    v.model = apply(v.model ?? {}, d.ops as never);
    [v.at, v.live] = [Date.now(), true];
    put('scr:' + v.path, JSON.stringify({ t: v.at, v: v.model }));
    paint(v, before);
    dot.classList.add('p');
    setTimeout(() => dot.classList.remove('p'), 600);
  };
}
const close = (v: View) => { v.es?.close(); v.es = undefined; v.live = false; };

function go(v: View, path: string, push = true) {
  if (push) v.back.push(v.path);
  close(v);
  [v.path, v.model] = [path, null];
  load(v);
}

function show(t: (typeof tabs)[number]) {
  if (cur !== t) close(cur.v);
  cur = t;
  put('tab', t.id ?? null);
  if (location.hash.slice(1) !== t.id) history.replaceState(null, '', '#' + t.id);
  tabs.forEach(x => { x.a.classList.toggle('on', x === t); x.v.root.hidden = x !== t; });
  H.dataset.tab = t.id;
  load(t.v, !!t.v.model);
  scrollTo(0, 0);
}

function toast(text: string, bad = false) {
  const p = $('#toast');
  p.className = `msg ${bad ? 'r' : 'g'}`;
  p.textContent = text;
  p.hidden = !text;
  clearTimeout(Number(p.dataset.t));
  p.dataset.t = String(setTimeout(() => { p.hidden = true; }, bad ? 8000 : 3000));
}

// The sheet: a screen of type `sheet` (or any screen) over the page, its `done` posting the fields' values.
const sh = $('#sheet'), shv = view('', $('#sheet .sb'));
function closeSheet() { sh.hidden = true; H.classList.remove('sheet'); shv.model = null; shv.back = []; }
async function sheet(path: string, push = true) {
  if (push && !sh.hidden && shv.model) shv.back.push(shv.path);
  shv.path = path;
  shv.open.clear();
  try {
    shv.model = await fetchScreen(path);
  } catch (e) {
    if ((e as { status?: number }).status === 401) return unpaired();
    return toast((e as Error).message, true);
  }
  drawSheet();
  sh.hidden = false;
  H.classList.add('sheet');
}
function drawSheet() {
  const m = shv.model!, c = ctx(shv, m.id), head = $('#sheet .sh');
  head.replaceChildren(el('b', '', esc(m.title)));
  if (shv.back.length) {
    const b = el('button', 'bk', '<span>‹ back</span>');
    b.type = 'button';
    b.onclick = () => sheet(shv.back.pop()!, false);
    head.prepend(b);
  }
  const x = el('button', '', `<span>${esc(m.cancel ?? 'close')}</span>`);
  x.type = 'button';
  x.onclick = closeSheet;
  head.append(x);
  if (m.done) {
    const d = el('button', 'primary', `<span>${esc(m.done.label)}</span>`);
    d.type = 'button';
    d.onclick = () => c.act({ action: m.done.action, block: m.id, values: { ...m.done.values, ...values(shv.root) } }, d);
    head.append(d);
  }
  shv.root.replaceChildren(...(m.type === 'sheet' ? kids(m.blocks, c) : [render(m, c)!]));
}
sh.addEventListener('click', e => { if (e.target === sh) closeSheet(); });
addEventListener('keydown', e => { if (e.key === 'Escape' && !sh.hidden) closeSheet(); });

function tap(t: Blk) {
  if (t.sheet) sheet(t.sheet);
  else if (t.open) go(cur.v, t.open);
  else if (t.url) window.open(t.url, '_blank', 'noopener');
  else if (t.thread) toast('threads open in cooee');
}

// A confirm: the block's own words, [cancel] and its [ok] (a destructive one in red).
function confirmed(c: Blk): Promise<boolean> {
  return new Promise(ok => {
    const box = $('#confirm');
    box.replaceChildren(el('p', '', `<b>${esc(c.title)}</b>`), ...(c.message ? [el('p', 'm', esc(c.message))] : []));
    const bar = el('div', 'bar'), no = el('button', '', '<span>cancel</span>'), yes = el('button', 'bad', `<span>${esc(c.ok ?? 'ok')}</span>`);
    [no.type, yes.type] = ['button', 'button'];
    const end = (v: boolean) => { box.hidden = true; ok(v); };
    no.onclick = () => end(false);
    yes.onclick = () => end(true);
    bar.append(no, yes);
    box.append(bar);
    box.hidden = false;
    yes.focus();
  });
}

async function act(a: Act, v: View, screen: string, b?: HTMLElement) {
  if (demo) return toast('Demo with fake data: install drover to act on your own fleets');
  if (ro) return toast('read-only page', true);
  if (a.confirm && !(await confirmed(a.confirm))) return;
  b?.setAttribute('aria-busy', 'true');
  try {
    const r = await fetch(url('action'), { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ...a, confirm: undefined, screen }) });
    const j = await r.json().catch(() => ({ line: `${r.status}` }));
    if (r.status === 401) return unpaired();
    if (!r.ok) return toast(j.line ?? `${r.status}`, true);
    const ops: Blk[] = j.ops ?? [];
    const whole = ops.find(o => o.op === 'screen');
    if (whole && v !== shv) {
      close(v);
      v.model = apply(v.model ?? {}, ops as never);
      v.path = String(v.model.stream ?? v.path).replace(/\/stream/, '') || v.path;
      [v.at] = [Date.now()];
      put('scr:' + v.path, JSON.stringify({ t: v.at, v: v.model }));
      paint(v);
      stream(v);
    } else if (ops.length && v.model) {
      v.model = apply(v.model, ops as never);
      v === shv ? drawSheet() : paint(v);
    }
    if (j.close && v === shv) closeSheet();
    if (j.toast) toast(j.toast);
    if (!whole) setTimeout(() => { if (cards) load(cards, true); load(cur.v, true); }, 300);
  } catch (e) {
    toast(`${(e as Error).message}`, true);
  } finally {
    b?.removeAttribute('aria-busy');
  }
}

// #59 pairing: [pair] shows a code the Mac approves (`drover serve approve <code>`); claim polls until it is.
let pairing = false;
async function unpaired() {
  const r = await fetch(url('pair/status'), { cache: 'no-store' }).catch(() => null);
  const j = r && r.ok ? await r.json().catch(() => null) : null;
  if (j && !j.paired) {
    B.dataset.pair = '1';
    toast('pair this phone first', true);
  }
}
async function pair() {
  const r = await fetch(url('pair/request'), { method: 'POST' }).catch(() => null);
  const s = r ? (await r.text()).trim() : 'error: no answer', [code, hint, line] = s.split('\n'), p = $('#pairp');
  p.hidden = false;
  p.className = `pairp msg ${r && r.ok ? 'a' : 'r'}`;
  p.innerHTML = r && r.ok ? `<p class=code>${esc(code)}</p><p class=m>Waiting for approval on the Mac: drover serve approve ${esc(code)}${hint ? ` · ${esc(hint)}` : ''}</p>${line ? `<pre>${esc(line)}</pre>` : ''}` : esc(s);
  pairing = !!(r && r.ok);
}
setInterval(async () => {
  if (!pairing) return;
  const r = await fetch(url('pair/claim'), { method: 'POST' }).catch(() => null);
  if (!r || !r.ok) return;
  pairing = false;
  delete B.dataset.pair;
  $('#pairp').hidden = true;
  toast((await r.text()).trim());
  reload();
}, 2000);
if (!ro && !demo) fetch(url('pair/claim'), { method: 'POST' }).catch(() => null);

// [⚙]: the menu-row screens (Fleets…), the look (theme, colours) and alerts on this phone.
function settings() {
  const b = $('#sheet .sh'), body: HTMLElement[] = [];
  shv.model = null;
  b.replaceChildren(el('b', '', 'settings'));
  const x = el('button', '', '<span>close</span>');
  x.type = 'button';
  x.onclick = closeSheet;
  b.append(x);
  const row = (label: string, on: boolean, f: () => void, sub = '') => {
    const r = el('div', `row tap${on ? ' on' : ''}`, `<span class=rm><span class=rt>${on ? '✓ ' : ''}${esc(label)}</span>${sub ? `<span class=rs>${esc(sub)}</span>` : ''}</span>`);
    r.tabIndex = 0;
    r.setAttribute('role', 'button');
    r.onclick = f;
    return r;
  };
  const sec = (h: string, rs: HTMLElement[]) => { const s = el('section', 'sec', `<h3 class=sh>${esc(h)}</h3>`); const l = el('div', 'list'); l.append(...rs); s.append(l); return s; };
  const menu = PL.filter(p => p.type === 'menu-row' && p.screen);
  if (menu.length) body.push(sec('screens', menu.map(p => row(p.label ?? p.screen!, false, () => sheet(p.screen!), p.subtitle))));
  body.push(sec('theme', themes().map(n => row(n, n === H.dataset.theme, () => { put('theme', n); H.dataset.theme = n; colours(); theme(n).then(redraw); settings(); }))));
  body.push(sec('colours', schemes().map(n => row(n, n === H.dataset.scheme, () => { put('scheme', n); colours(); settings(); }))));
  const standalone = matchMedia('(display-mode: standalone)').matches || (navigator as { standalone?: boolean }).standalone === true;
  if (standalone && 'PushManager' in window && B.dataset.key) body.push(sec('alerts', [row('enable alerts on this phone', false, alerts)]));
  body.push(el('p', 'm foot', 'this phone only · the home-screen icon keeps its install-time look'));
  shv.root.replaceChildren(...body);
  sh.hidden = false;
  H.classList.add('sheet');
}
async function alerts() {
  try {
    if ((await Notification.requestPermission()) !== 'granted') throw new Error('notifications are off for this app');
    const key = Uint8Array.from(atob(B.dataset.key!.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
    const s = (await (await navigator.serviceWorker.ready).pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key })).toJSON();
    const r = await fetch(url('push/subscribe'), { method: 'POST', body: new URLSearchParams({ endpoint: s.endpoint ?? '', p256dh: s.keys?.p256dh ?? '', auth: s.keys?.auth ?? '' }) });
    if (r.status === 401) return unpaired();
    toast((await r.text()).trim(), !r.ok);
  } catch (e) {
    toast('alerts failed: ' + (e as Error).message, true);
  }
}

function redraw() {
  tabs.forEach(t => paint(t.v));
  if (cards) paint(cards);
}
function reload() {
  if (cards) load(cards);
  load(cur.v);
}

const nav = $('nav'), main = $('main');
for (const t of tabs) {
  t.a.href = '#' + t.id;
  t.a.onclick = e => { e.preventDefault(); show(t); };
  nav.append(t.a);
  main.append(t.v.root);
  t.v.root.addEventListener('redraw', () => paint(t.v));
}
cards?.root.addEventListener('redraw', () => cards && paint(cards));
shv.root.addEventListener('redraw', drawSheet);
$('#rf').onclick = reload;
$('#sysb').onclick = settings;
document.querySelectorAll<HTMLElement>('.pairb').forEach(b => { b.onclick = pair; });
addEventListener('online', sync);
addEventListener('offline', sync);
addEventListener('hashchange', () => { const t = tabs.find(x => x.id === location.hash.slice(1)); if (t && t !== cur) show(t); });
setInterval(() => { if (!document.hidden) { paint(cur.v); sync(); } }, 30_000);
document.addEventListener('visibilitychange', () => { if (!document.hidden && !cur.v.live) load(cur.v, true); });
colours();
theme(H.dataset.theme ?? 'farm').then(redraw);
if (cards) load(cards);
show(cur);
if ('serviceWorker' in navigator && !demo) navigator.serviceWorker.register(url('sw.js')).catch(() => {});
