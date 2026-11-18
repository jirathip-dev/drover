import { type Act, type Ctx, el, kids, morph, render, sent, values } from './blocks';
import './herd';
import { apply, type Blk, blks, find, isBlk, isList, obj } from './ops';
import { R, colours, esc, get, nodes, put, schemes, theme, themes } from './theme';
import { type Kind, type Span, flush, painted, read, span } from './timing';

type Place = { type: string; id?: string; label?: string; subtitle?: string; screen?: string };
declare const PL: Place[];
const B = document.body, H = document.documentElement;
const ro = 'ro' in B.dataset, demo = 'demo' in B.dataset;
const $ = <T extends Element = HTMLElement>(s: string): T => { const e = document.querySelector<T>(s); if (!e) throw new Error(`no ${s}`); return e; };
const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const exported = B.dataset.root === '';
const url = (p: string) => new URL(p.replace(/^[/]/, '') + (exported && /^\/screens\/[^?]*$/.test(p) ? '/index.html' : ''), R).href;

type Pend = { id: string; text: string; n: number; st: 'sending' | 'sent' | 'failed'; a: Act };
type View = { path: string; model: Blk | null; at: number; live: boolean; es?: EventSource; root: HTMLElement; open: Set<string>; back: string[]; pend: Pend[] };
const view = (path: string, root: HTMLElement): View => ({ path, model: null, at: 0, live: false, root, open: new Set(), back: [], pend: [] });
const tabs = PL.flatMap(p => (p.type === 'header-strip' && p.screen ? [{ ...p, screen: p.screen, v: view(p.screen, el('section', 'tab')), a: el('a') }] : []));
const cards = PL.some(p => p.type === 'chat-cards') ? view('/screens/cards', $('#cards')) : null;
type Tab = (typeof tabs)[number];
const hashOf = (t: Tab, p: string) => '#' + t.id + (p.startsWith(t.screen + '/') ? p.slice(t.screen.length) : '');
const route = (h = location.hash.slice(1) || (get('tab') ?? '')): [Tab, string] | null => {
  const t = tabs.find(x => h === x.id || h.startsWith(x.id + '/'));
  return t ? [t, t.screen + h.slice((t.id ?? '').length)] : null;
};
const at = route();
let cur = at?.[0] ?? tabs[0];
const th = tabs.find(x => x.id === 'threads'), lst = th ? view(th.screen, el('section', 'lst')) : null, wide = matchMedia('(min-width:1000px)');
const duo = () => !!lst && wide.matches && cur === th;

const dot = $('#dot'), stale = $('#stale');
function sync() {
  const v = cur.v, live = !!v.live && navigator.onLine;
  dot.className = demo ? 'demo' : live ? 'g' : navigator.onLine ? 'a' : 'r';
  H.toggleAttribute('data-focus', duo() || (!!v.model && !isList(v.model) && !!v.model.focus));
  H.toggleAttribute('data-duo', duo());
  if (duo()) fit();
  const t = v.at ? new Date(v.at) : null, hm = t && t.toTimeString().slice(0, 5), old = t ? Date.now() - t.getTime() : 0;
  stale.hidden = !t || (live && old < 120_000);
  if (t) stale.textContent = `last update ${hm}${old > 60_000 ? ` · ${Math.round(old / 60_000)}m ago` : ''}`;
}

const cached = (p: string): { t: number; v: Blk } | null => {
  try { const c: unknown = JSON.parse(get('scr:' + p) ?? 'null'); return isBlk(c) && isBlk(c.v) ? { t: Number(c.t), v: c.v } : null; } catch { return null; }
};
async function fetchScreen(p: string, s?: Span): Promise<Blk> {
  const t0 = performance.now(), get = () => fetch(url(p), { cache: 'no-store' });
  const r = await get().catch(() => new Promise<Response>(ok => setTimeout(() => ok(get()), 300)));
  const j = obj(await read(r, s, t0).catch(() => ({ line: `${r.status}` })));
  if (!r.ok) throw Object.assign(new Error(String(j.line ?? r.status)), { status: r.status });
  return j;
}

function ctx(v: View, screen: string): Ctx {
  return { screen, open: v.open, act: (a, b) => act(a, v, screen, b), tap };
}
function rows(m: unknown, out = new Map<string, string>()) {
  if (isList(m)) m.forEach(x => rows(x, out));
  else if (isBlk(m)) {
    if ((m.type === 'row' || m.type === 'card') && m.id) out.set(String(m.id), JSON.stringify(m));
    Object.values(m).forEach(x => rows(x, out));
  }
  return out;
}
const gone = new Set<string>();
const hits = (all: string, text: string) => all.split(JSON.stringify(text).slice(1, -1)).length - 1;
const bubble = (p: Pend): Blk => ({
  type: 'card', id: p.id, kind: 'you', tone: { sending: 'pending', sent: '', failed: 'bad' }[p.st], title: 'you', meta: { sending: 'sending…', sent: '', failed: 'not sent' }[p.st],
  blocks: [{ type: 'text', id: 't', markdown: p.text }],
  buttons: p.st === 'failed' ? [{ type: 'button', id: `${p.id}-retry`, label: 'retry', style: 'bad', action: p.a.action, values: p.a.values }] : [],
});
function shown(v: View): Blk | null {
  const m = v.model;
  gone.forEach(id => { if (find(m, id)) apply({ m }, [{ op: 'remove', id }]); });
  if (!m || isList(m) || !v.pend.length) return m;
  const all = JSON.stringify(m);
  v.pend = v.pend.filter(p => p.st === 'failed' || hits(all, p.text) <= p.n);
  const add = blks(m.blocks), t = add.findIndex(b => b.id === 'transcript'), at = add.findIndex(b => b.type === 'composer'), more = v.pend.map(bubble);
  if (t >= 0) return { ...m, blocks: add.map((b, i) => (i === t ? { ...b, blocks: [...blks(b.blocks), ...more] } : b)) };
  return { ...m, blocks: at < 0 ? [...add, ...more] : [...add.slice(0, at), ...more, ...add.slice(at)] };
}
type Spot = { end: boolean; id?: string; dy: number };
const spots = new Map<string, Spot>();
function spot(sc: HTMLElement | null): Spot | null {
  if (!sc) return null;
  const top = sc.getBoundingClientRect().top, k = [...sc.querySelectorAll<HTMLElement>(':scope > .card[data-id]')].find(c => c.getBoundingClientRect().bottom > top + 1);
  return { end: sc.scrollTop + sc.clientHeight >= sc.scrollHeight - 2, id: k?.dataset.id, dy: k ? k.getBoundingClientRect().top - top : 0 };
}
function land(sc: HTMLElement | null, s: Spot | null) {
  if (!sc) return;
  const k = s && !s.end && s.id ? sc.querySelector<HTMLElement>(`:scope > [data-id="${CSS.escape(s.id)}"]`) : null;
  sc.scrollTop = k && s ? sc.scrollTop + k.getBoundingClientRect().top - sc.getBoundingClientRect().top - s.dy : sc.scrollHeight;
}
const seen = new WeakMap<HTMLElement, Spot | null>();
const docked = new ResizeObserver(es => es.forEach(({ target: t }) => { if (t instanceof HTMLElement && t.isConnected) land(t, seen.get(t) ?? null); }));
const scroller = (v: View) => v.root.querySelector<HTMLElement>(':scope > .scr > .msgs');
function paint(v: View, before?: Blk | null) {
  const m = shown(v), y = scrollY;
  if (!m) return;
  const focus = !isList(m) && !!m.focus, was = scroller(v), place = focus && was?.parentElement?.dataset.id === m.id ? spot(was) : (spots.get(v.path) ?? null);
  const c = ctx(v, isList(m) ? 'cards' : String(m.id));
  const body = isList(m) ? kids(m, c) : [render(m, c)].filter((e): e is HTMLElement => !!e);
  if (v.back.length && body[0] && !(duo() && v === th?.v)) {
    const b = el('button', '', '<span>‹ back</span>');
    b.type = 'button';
    b.onclick = () => { if (v === cur.v) history.back(); else { const p = v.back.pop(); if (p != null) go(v, p, false); } };
    (body[0].querySelector(':scope > .msgs') ?? body[0]).prepend(b);
  }
  morph(v.root, body);
  if (scrollY !== y) scrollTo(0, y);
  if (before) {
    const was = rows(before);
    rows(m).forEach((j, id) => { if (was.has(id) && was.get(id) !== j) v.root.querySelector(`[data-id="${CSS.escape(id)}"]`)?.classList.add('fl'); });
  }
  if (v === cur.v) sync();
  if (focus && !v.root.hidden) {
    fit();
    const sc = scroller(v);
    land(sc, place);
    if (sc) {
      seen.set(sc, spot(sc));
      sc.onscroll = () => {
        seen.set(sc, spot(sc));
        const o = sc.querySelector<HTMLElement>(':scope > [data-id=older]:not([aria-busy])');
        if (o && sc.isConnected && sc.scrollTop < 8) { o.setAttribute('aria-busy', 'true'); o.click(); }
      };
      docked.disconnect();
      docked.observe(sc);
    }
  }
  badges();
  mark();
}
function mark() {
  const id = th ? /^\/([^/?]+)/.exec(th.v.path.slice(th.screen.length))?.[1] : '';
  lst?.root.querySelectorAll<HTMLElement>('.row[data-id]').forEach(r => r.toggleAttribute('aria-current', r.dataset.id === id));
}
function blank(v: View) {
  const b = el('button', '', '<span>new thread</span>'), s = el('div', 'scr', '<p class=m>no thread open</p>');
  b.type = 'button';
  b.onclick = () => go(v, `${v.path}/new/all`);
  s.dataset.id = 'none';
  s.append(b);
  close(v);
  v.model = null;
  v.root.replaceChildren(s);
  sync();
  mark();
}
function badges() {
  tabs.forEach(t => {
    const m = t === th && duo() ? lst?.model : t.v.model, n = m && !isList(m) ? m.badge : null;
    t.a.replaceChildren(...nodes(`<span>${esc(t.label?.toLowerCase())}${n ? `<b class=n>·${n}</b>` : ''}</span>`));
  });
}

async function load(v: View, quiet = false, s?: Span) {
  if (duo() && v === th?.v && v.path === th.screen) return blank(v);
  const c = cached(v.path);
  if (!v.model && c) [v.model, v.at] = [c.v, c.t];
  if (!v.model || !quiet) paint(v);
  const slow = setTimeout(() => ($('#ld').hidden = false), 300);
  try {
    const p = v.path, m = await fetchScreen(p, s), before = v.model === c?.v ? null : v.model;
    if (v.path !== p) return;
    [v.model, v.at] = [m, Date.now()];
    put('scr:' + v.path, JSON.stringify({ t: v.at, v: m }));
    paint(v, before);
    painted(s);
    if (!v.es) stream(v);
  } catch (e) {
    if (isBlk(e) && e.status === 401) unpaired();
    else if (!v.model) v.root.replaceChildren(el('p', 'msg r', esc(msg(e))));
    v.live = false;
    sync();
  } finally {
    clearTimeout(slow);
    $('#ld').hidden = true;
  }
}

function stream(v: View) {
  const s = v.model && !isList(v.model) && v.model.stream ? v.model.stream : v === cards || v === shv ? v.path + '/stream' : null;
  if (!s || exported || !('EventSource' in window)) return void (v.live = false);
  const es = (v.es = new EventSource(url(String(s))));
  es.onopen = () => { v.live = true; sync(); };
  es.onerror = () => { v.live = false; sync(); };
  es.onmessage = e => {
    const s = span('delta'), d = obj(JSON.parse(e.data)), was = JSON.stringify(v.model), before = v.model && obj(JSON.parse(was));
    v.model = apply(v.model ?? {}, blks(d.ops));
    s.parse = performance.now() - s.t0;
    [v.at, v.live] = [Date.now(), true];
    if (JSON.stringify(v.model) === was) { painted(s); return sync(); }
    if (v === shv) drawSheet();
    else {
      put('scr:' + v.path, JSON.stringify({ t: v.at, v: v.model }));
      paint(v, before);
    }
    painted(s);
    dot.classList.add('p');
    setTimeout(() => dot.classList.remove('p'), 600);
  };
}
const close = (v: View) => { v.es?.close(); v.es = undefined; v.live = false; };

const ys = new Map<string, number>();
function go(v: View, path: string, push = true, k?: Span) {
  if (v === cur.v) ys.set(v.path, scrollY);
  if (push) v.back.push(v.path);
  if (push && v === cur.v) history.pushState(null, '', hashOf(cur, path));
  const s = spot(scroller(v));
  if (s) spots.set(v.path, s);
  v.es?.close();
  v.es = undefined;
  [v.path, v.model] = [path, null];
  load(v, false, k);
  if (!push && v === cur.v) scrollTo(0, ys.get(path) ?? 0);
}

function show(t: Tab, path = t.v.path, s?: Span) {
  if (cur !== t) close(cur.v);
  if (lst) close(lst);
  if (path !== t.v.path) {
    close(t.v);
    [t.v.path, t.v.model, t.v.back] = [path, null, path === t.screen ? [] : [t.screen]];
  }
  cur = t;
  put('tab', t.id ?? null);
  if (location.hash !== hashOf(t, t.v.path)) history.replaceState(null, '', hashOf(t, t.v.path));
  tabs.forEach(x => { x.a.classList.toggle('on', x === t); x.v.root.hidden = x !== t; });
  H.dataset.tab = t.id;
  if (lst) lst.root.hidden = !duo();
  sync();
  if (lst && duo()) load(lst, !!lst.model);
  load(t.v, !!t.v.model, s);
  scrollTo(0, 0);
}

function toast(text: string, bad = false, retry?: () => void) {
  const p = $('#toast');
  p.className = `msg ${bad ? 'r' : 'g'}`;
  p.textContent = text;
  p.hidden = !text;
  clearTimeout(Number(p.dataset.t));
  if (retry) {
    const b = el('button', '', '<span>retry</span>');
    b.type = 'button';
    b.onclick = () => { p.hidden = true; retry(); };
    p.append(' ', b);
  } else p.dataset.t = String(setTimeout(() => { p.hidden = true; }, bad ? 8000 : 3000));
}

const sh = $('#sheet'), shv = view('', $('#sheet .sb'));
function closeSheet() { sh.hidden = true; H.classList.remove('sheet'); close(shv); shv.model = null; shv.back = []; shv.pend = []; }
async function sheet(path: string, push = true, s = span('sheet')) {
  if (push && !sh.hidden && shv.model) shv.back.push(shv.path);
  close(shv);
  [shv.path, shv.pend] = [path, []];
  shv.open.clear();
  try {
    shv.model = await fetchScreen(path, s);
  } catch (e) {
    if (isBlk(e) && e.status === 401) return unpaired();
    return toast(msg(e), true);
  }
  drawSheet();
  sh.hidden = false;
  H.classList.add('sheet');
  painted(s);
}
function drawSheet() {
  const m = shown(shv);
  if (!m) return;
  const c = ctx(shv, String(m.id)), head = $('#sheet .sh'), done = m.done;
  head.replaceChildren(el('b', '', esc(m.title)));
  if (shv.back.length) {
    const b = el('button', 'bk', '<span>‹ back</span>');
    b.type = 'button';
    b.onclick = () => { const p = shv.back.pop(); if (p) void sheet(p, false); };
    head.prepend(b);
  }
  const x = el('button', '', `<span>${esc(m.cancel ?? 'close')}</span>`);
  x.type = 'button';
  x.onclick = closeSheet;
  head.append(x);
  if (isBlk(done)) {
    const d = el('button', 'primary', `<span>${esc(done.label)}</span>`);
    d.type = 'button';
    d.onclick = () => c.act({ action: done.action, block: m.id, values: { ...obj(done.values), ...values(shv.root) } }, d).then(ok => ok && sent(shv.root));
    head.append(d);
  }
  morph(shv.root, kids(m.type === 'sheet' ? m.blocks : [m], c));
}
sh.addEventListener('click', e => { if (e.target === sh) closeSheet(); });
addEventListener('keydown', e => { if (e.key === 'Escape' && !sh.hidden) closeSheet(); });

function tap(x: unknown) {
  const t = obj(x);
  if (t.sheet) sheet(String(t.sheet));
  else if (t.open) go(cur.v, String(t.open), !t.keep, span('tap'));
  else if (t.copy != null) void copy(String(t.copy));
  else if (t.thread) /^t\d+$/.test(String(t.thread)) ? thread(String(t.thread)) : sheet(`/screens/lanes/${String(t.thread)}`);
}
function thread(id: string, s = span('tap')) {
  const t = tabs.find(x => x.id === 'threads');
  if (!t) return toast('no Threads tab on this page', true);
  closeSheet();
  history.pushState(null, '', hashOf(t, `${t.screen}/${id}`));
  show(t, `${t.screen}/${id}`, s);
}

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

let pends = 0;
const draw = (v: View) => (v === shv ? drawSheet() : paint(v));
async function act(a: Act, v: View, screen: string, b?: HTMLElement): Promise<boolean> {
  if (demo || ro) {
    toast(demo ? 'Demo with fake data: install drover to act on your own fleets' : 'read-only page', ro);
    return false;
  }
  if (isBlk(a.confirm) && !(await confirmed(a.confirm))) return false;
  const retry = b && (() => b.click()), k: Kind = a.action === 'answer' ? 'answer' : /say|send|queue|steer/.test(String(a.action)) ? 'send' : 'tap', s = span(k);
  const text = /^(thread-say|thread-steer|send)$/.test(String(a.action)) ? String(obj(a.values).text ?? '').trim() : '';
  const p: Pend | null = text ? { id: `pend-${++pends}`, text, n: hits(JSON.stringify(v.model), text), st: 'sending', a } : null;
  const hide = a.action === 'answer' ? [`ask-${String(a.block)}`, String(a.block)] : [];
  if (p) v.pend = [...v.pend.filter(x => !(x.st === 'failed' && x.text === text)), p];
  hide.forEach(id => gone.add(id));
  new Set(hide.length ? [cards, cur.v, v] : p ? [v] : []).forEach(x => { if (x?.model) draw(x); });
  const fail = () => {
    if (p) p.st = 'failed';
    if (hide.length) {
      hide.forEach(id => gone.delete(id));
      reload();
      if (v === shv) void sheet(shv.path, false);
    }
    if (p) draw(v);
    return false;
  };
  b?.setAttribute('aria-busy', 'true');
  try {
    const r = await fetch(url('action'), { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ...a, confirm: undefined, screen }) });
    const j = obj(await read(r, s, s.t0).catch(() => ({ line: `${r.status}` })));
    if (r.status === 401) return (unpaired(), fail());
    if (!r.ok) return (toast(String(j.line ?? r.status), true, retry), fail());
    if (p) p.st = 'sent';
    const ops = blks(j.ops);
    const whole = ops.find(o => o.op === 'screen');
    if (whole && v !== shv) {
      close(v);
      v.model = apply(v.model ?? {}, ops);
      v.path = String(v.model.stream ?? v.path).replace(/\/stream/, '') || v.path;
      if (v === cur.v) history.replaceState(null, '', hashOf(cur, v.path));
      [v.at] = [Date.now()];
      put('scr:' + v.path, JSON.stringify({ t: v.at, v: v.model }));
      paint(v);
      stream(v);
    } else if (v.model) {
      v.model = apply(v.model, ops);
      draw(v);
    }
    if (j.close && v === shv) closeSheet();
    else if (v === shv && !shv.es) stream(shv);
    if (j.toast) toast(String(j.toast));
    painted(s);
    return true;
  } catch (e) {
    toast(msg(e), true, retry);
    return fail();
  } finally {
    b?.removeAttribute('aria-busy');
  }
}

let pairing = false;
async function unpaired() {
  const r = await fetch(url('pair/status'), { cache: 'no-store' }).catch(() => null);
  const j: unknown = r && r.ok ? await r.json().catch(() => null) : null;
  if (isBlk(j) && !j.paired) {
    B.dataset.pair = '1';
    toast('pair this phone first', true);
  }
}
async function pair() {
  const r = await fetch(url('pair/request'), { method: 'POST' }).catch(() => null);
  const s = r ? (await r.text()).trim() : 'error: no answer', [code, hint, line] = s.split('\n'), p = $('#pairp');
  p.hidden = false;
  p.className = `pairp msg ${r && r.ok ? 'a' : 'r'}`;
  p.replaceChildren(...nodes(r && r.ok ? `<p class=code>${esc(code)}</p><p class=m>Waiting for approval on the Mac: drover serve approve ${esc(code)}${hint ? ` · ${esc(hint)}` : ''}</p>${line ? `<pre>${esc(line)}</pre>` : ''}` : esc(s)));
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
  const menu = PL.flatMap(p => (p.type === 'menu-row' && p.screen ? [{ ...p, screen: p.screen }] : []));
  if (menu.length) body.push(sec('screens', menu.map(p => row(p.label ?? p.screen, false, () => sheet(p.screen), p.subtitle))));
  body.push(sec('theme', themes().map(n => row(n, n === H.dataset.theme, () => { put('theme', n); H.dataset.theme = n; colours(); theme(n).then(redraw); settings(); }))));
  body.push(sec('colours', schemes().map(n => row(n, n === H.dataset.scheme, () => { put('scheme', n); colours(); settings(); }))));
  if (app && 'PushManager' in window && B.dataset.key) body.push(sec('alerts', [row('enable alerts on this phone', false, alerts)]));
  body.push(el('p', 'm foot', 'this phone only · the home-screen icon keeps its install-time look'));
  shv.root.replaceChildren(...body);
  sh.hidden = false;
  H.classList.add('sheet');
}
async function alerts() {
  try {
    if ((await Notification.requestPermission()) !== 'granted') throw new Error('notifications are off for this app');
    const key = Uint8Array.from(atob((B.dataset.key ?? '').replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
    const s = (await (await navigator.serviceWorker.ready).pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key })).toJSON();
    const r = await fetch(url('push/subscribe'), { method: 'POST', body: new URLSearchParams({ endpoint: s.endpoint ?? '', p256dh: s.keys?.p256dh ?? '', auth: s.keys?.auth ?? '' }) });
    if (r.status === 401) return unpaired();
    toast((await r.text()).trim(), !r.ok);
  } catch (e) {
    toast('alerts failed: ' + msg(e), true);
  }
}

function redraw() {
  tabs.forEach(t => paint(t.v));
  if (lst) paint(lst);
  if (cards) paint(cards);
}
function reload() {
  if (cards) load(cards);
  load(cur.v);
}

const nav = $('nav'), main = $('main');
for (const t of tabs) {
  t.a.href = '#' + t.id;
  t.a.onclick = e => { e.preventDefault(); show(t, t.v.path, span('tab', e.timeStamp)); };
  nav.append(t.a);
  main.append(t.v.root);
  t.v.root.addEventListener('redraw', () => paint(t.v));
}
cards?.root.addEventListener('redraw', () => cards && paint(cards));
if (lst && th) {
  main.insertBefore(lst.root, th.v.root);
  lst.root.addEventListener('redraw', () => paint(lst));
  wide.addEventListener('change', () => show(cur));
}
shv.root.addEventListener('redraw', drawSheet);
function fit() {
  const vv = visualViewport, nav = $('nav').getBoundingClientRect().top, kb = vv ? vv.offsetTop + vv.height : innerHeight;
  H.style.setProperty('--top', `${$('main').getBoundingClientRect().top}px`);
  H.style.setProperty('--bot', `${Math.max(0, H.clientHeight - Math.min(nav, kb))}px`);
  if (H.hasAttribute('data-focus') && document.scrollingElement?.scrollTop) scrollTo(0, 0);
}
const refit = () => { const sc = scroller(cur.v), s = spot(sc); fit(); land(sc, s); };
visualViewport?.addEventListener('resize', refit);
visualViewport?.addEventListener('scroll', refit);
addEventListener('resize', refit);
async function copy(text: string, b?: HTMLElement) {
  try {
    await navigator.clipboard.writeText(text);
    const w = b?.firstElementChild;
    if (w) { w.textContent = 'copied'; setTimeout(() => { w.textContent = 'copy'; }, 1500); } else toast('copied');
  } catch {
    const s = getSelection(), g = document.createRange();
    g.selectNodeContents(b?.previousElementSibling ?? document.body);
    s?.removeAllRanges();
    s?.addRange(g);
  }
}
main.addEventListener('click', e => {
  const t = e.target;
  if (!(t instanceof Element)) return;
  const c = t.closest<HTMLElement>('.cpy'), a = t.closest<HTMLElement>('a[data-thread]');
  if (c) copy(c.previousElementSibling?.textContent ?? '', c);
  if (a) { e.preventDefault(); thread(a.dataset.thread ?? ''); }
});
function press(k: HTMLElement) {
  const txt = [...k.querySelectorAll<HTMLElement>(':scope > .txt')], plain = txt.map(t => t.innerText).join('\n\n'), raw = txt.map(t => t.dataset.md).join('\n\n');
  const pick = (label: string, f: () => void) => { const b = el('button', '', `<span>${esc(label)}</span>`); b.type = 'button'; b.onclick = () => { closeSheet(); f(); }; return b; };
  $('#sheet .sh').replaceChildren(el('b', '', 'message'), pick('close', () => {}));
  const quote = () => {
    const i = cur.v.root.querySelector<HTMLTextAreaElement>('.cmp textarea');
    if (!i) return;
    i.value = `> ${plain.replace(/\s+/g, ' ').trim()}\n${i.value}`;
    i.dispatchEvent(new Event('input'));
    i.focus();
  };
  const bar = el('div', 'bar');
  bar.append(pick('copy', () => copy(plain)), pick('copy as markdown', () => copy(raw)), pick('quote', quote));
  shv.model = null;
  shv.root.replaceChildren(bar);
  sh.hidden = false;
  H.classList.add('sheet');
}
const card = (t: EventTarget | null) => (t instanceof Element ? t.closest<HTMLElement>('.card.k-you, .card.k-orch') : null);
let hold = 0;
main.addEventListener('contextmenu', e => { const k = card(e.target); if (k) { e.preventDefault(); press(k); } });
main.addEventListener('touchstart', e => { const k = card(e.target); clearTimeout(hold); if (k) hold = setTimeout(() => press(k), 500); }, { passive: true });
for (const ev of ['touchend', 'touchmove', 'touchcancel']) main.addEventListener(ev, () => clearTimeout(hold), { passive: true });
$('#rf').onclick = reload;
$('#sysb').onclick = settings;
document.querySelectorAll<HTMLElement>('.pairb').forEach(b => { b.onclick = pair; });
addEventListener('online', sync);
addEventListener('offline', sync);
addEventListener('hashchange', () => {
  const r = route(location.hash.slice(1));
  if (!r) return;
  const [t, p] = r;
  if (t !== cur) return show(t, p);
  if (p === t.v.path) return;
  if (t.v.back[t.v.back.length - 1] === p) t.v.back.pop();
  else t.v.back.push(t.v.path);
  go(t.v, p, false);
});
const app = matchMedia('(display-mode: standalone)').matches || ('standalone' in navigator && navigator.standalone === true);
let swipe: { x: number; y: number; dx: number } | null = null;
const slide = (dx: number, ease = false) => {
  for (const k of cur.v.root.children) if (k instanceof HTMLElement) Object.assign(k.style, { transition: ease ? 'transform .2s ease-out' : '', transform: dx ? `translateX(${dx}px)` : '' });
};
addEventListener('touchstart', e => {
  const t = e.touches.item(0), x = e.target instanceof Element ? e.target : null;
  swipe = t && e.touches.length === 1 && t.clientX < 20 && cur.v.back.length && sh.hidden && !duo() && !x?.closest('#iv, .imgs, pre') ? { x: t.clientX, y: t.clientY, dx: 0 } : null;
}, { passive: true });
addEventListener('touchmove', e => {
  const t = e.touches.item(0);
  if (!swipe || !t) return;
  const dx = t.clientX - swipe.x;
  if (!swipe.dx && Math.abs(t.clientY - swipe.y) > Math.abs(dx)) return void (swipe = null);
  swipe.dx = Math.max(0, dx);
  slide(swipe.dx);
}, { passive: true });
const release = (e: TouchEvent) => {
  const s = swipe;
  swipe = null;
  if (s && e.type === 'touchend' && s.dx > innerWidth * 0.4) history.back();
  else if (s?.dx) slide(0, true);
};
addEventListener('touchend', release, { passive: true });
addEventListener('touchcancel', release, { passive: true });
addEventListener('hashchange', () => slide(0));
setInterval(() => { if (!document.hidden) sync(); }, 30_000);
document.addEventListener('visibilitychange', () => {
  if (!document.hidden && !cur.v.live) load(cur.v, true);
  if (!document.hidden && lst && duo() && !lst.live) load(lst, true);
});
colours();
theme(H.dataset.theme ?? 'farm').then(redraw);
if (cards) load(cards);
if (at && at[1] !== cur.screen) {
  history.replaceState(null, '', '#' + cur.id);
  history.pushState(null, '', hashOf(cur, at[1]));
}
show(cur, at?.[1]);
const report = (leaving = false) => flush(body => void fetch(url('api/v1/timings'), { method: 'POST', headers: { 'content-type': 'application/json' }, body, keepalive: true }).catch(() => {}), leaving);
if (!demo && !ro && !exported) { setInterval(report, 5_000); addEventListener('pagehide', () => report(true)); }
if ('serviceWorker' in navigator && !demo) navigator.serviceWorker.register(url('sw.js')).catch(() => {});
