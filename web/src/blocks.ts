import { type Blk, blks, isBlk, isList, obj } from './ops';
import { esc, figure, get, nodes, put, word } from './theme';

export type Act = { action: unknown; block?: unknown; values?: unknown; confirm?: unknown };
export type Ctx = {
  screen: string;
  act: (a: Act, el?: HTMLElement) => Promise<boolean>;
  tap: (t: unknown) => void;
  open: Set<string>;
};

export const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls = '', html = ''): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html) e.replaceChildren(...nodes(html));
  return e;
};
const href = (a: HTMLAnchorElement, url: string) => Object.assign(a, { href: url, target: '_blank', rel: 'noopener' });
const urlOf = (t: unknown): string | undefined => (isBlk(t) && typeof t.url === 'string' ? t.url : undefined);
const btn = (label: unknown, cls = '', url?: string) => {
  const b = url ? href(el('a', `btn ${cls}`), url) : Object.assign(el('button', cls), { type: 'button' });
  b.append(el('span', '', esc(label)));
  return b;
};
const GROUP: Record<string, string> = { waiting: 'needs', blocked: 'needs', asleep: 'idle', in: 'working' };
const tone = (v: unknown): Blk => (isBlk(v) ? v : { text: v });
const SH: Record<string, string> = { needs: 'r', working: 'g' };
const draft = (i: HTMLInputElement, key: string) => {
  i.dataset.draft = key;
  i.value = get(key) ?? i.value;
  i.addEventListener('input', () => put(key, i.value || null));
};
/** Clears the drafts of the fields under `box`, here and in any copy on the page, once their action succeeded. */
export const sent = (box: Element) => box.querySelectorAll<HTMLInputElement>('[data-draft]').forEach(i => {
  const k = i.dataset.draft ?? '';
  put(k, null);
  document.querySelectorAll<HTMLInputElement>(`[data-draft="${CSS.escape(k)}"]`).forEach(x => { x.value = x.dataset.was ?? ''; });
});

export function md(s: unknown, refs = false): string {
  const link = new RegExp(String.raw`\[([^\]]+)\]\((https?:[^)\s]+)\)|(https?:\/\/[^\s<]*[^\s<.,;:!?)])|\b(${refs ? 't\\d+' : '(?!)'})\b`, 'g');
  const a = (_: string, t: string, u: string, bare: string, ref: string) => ref ? `<a href="#" data-thread="${ref}">${ref}</a>` :
    `<a href="${u ?? bare}" target=_blank rel=noopener>${t ?? bare}</a>`;
  return String(s ?? '').split(/```(?:[a-z]*)\n?/).map((part, i) => i % 2 ? `<div class=cb><pre>${esc(part.replace(/\n$/, ''))}</pre><button type=button class=cpy><span>copy</span></button></div>` :
    esc(part).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(link, a).replace(/\n/g, '<br>')).join('');
}

/** `{since: iso}` as an age (`4m`, `1h 9m`), else the value as written. */
export function value(v: unknown): string {
  if (isBlk(v) && 'since' in v) {
    const s = Math.max(0, (Date.now() - Date.parse(String(v.since))) / 1000);
    return s < 60 ? 'now' : s < 3600 ? `${Math.floor(s / 60)}m` : s < 86400 ? `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m` : `${Math.floor(s / 86400)}d`;
  }
  return String(v ?? '');
}

const R: Record<string, (b: Blk, c: Ctx) => HTMLElement | null> = {
  screen(b, c) {
    const s = el('div', 'scr');
    s.dataset.id = String(b.id);
    const head = el('p', 'tabh', `<span class=tw>${word(`tab-${b.id}`, String(b.title ?? '').toLowerCase())}</span>`);
    for (const a of blks(b.actions)) for (const m of blks(a.menu ?? [a])) {
      const x = btn(m.label, '', urlOf(m.tap));
      x.onclick = () => (m.tap ? c.tap(m.tap) : c.act({ action: m.action, values: m.values, confirm: m.confirm }, x));
      head.append(x);
    }
    s.append(head, ...kids(b.blocks, c));
    if (b.focus) {
      const m = el('div', 'msgs');
      m.append(...[...s.children].filter(e => !e.matches('.row.sticky, .cmp')));
      s.insertBefore(m, s.querySelector(':scope > .cmp'));
    }
    return s;
  },
  section(b, c) {
    const s = el('section', 'sec');
    const w = String(b.header ?? '').split(' ')[0], k = GROUP[w] ?? w;
    if (b.header) s.append(el('h3', `sh ${SH[k] ?? ''}`, word(k, b.header)));
    s.append(...kids(b.blocks, c));
    if (b.footer) s.append(el('p', 'm foot', esc(b.footer)));
    return s;
  },
  list(b, c) {
    const l = el('div', 'list');
    l.append(...kids(b.rows, c));
    return l;
  },
  row(b, c) {
    const cls = `row${b.style === 'link' ? ' link' : ''}${b.sticky ? ' sticky' : ''}${b.indent ? ' in' + Math.min(Number(b.indent), 3) : ''}`;
    const url = urlOf(b.tap);
    const r: HTMLElement = url ? href(el('a', cls), url) : el('div', cls);
    r.dataset.id = String(b.id ?? '');
    const sub = b.subtitle == null ? null : tone(b.subtitle), badge = b.badge == null ? null : tone(b.badge);
    r.replaceChildren(...nodes((isBlk(b.figure) ? figure(b.figure) : '') + `<span class=rm><span class=rt>${esc(b.title)}</span>` +
      (badge ? ` <span class="tag ${esc(badge.tone ?? '')}">${esc(badge.text)}</span>` : '') +
      (sub ? `<span class="rs ${esc(sub.tone ?? '')}">${esc(sub.text)}</span>` : '') + '</span>' +
      (b.value != null ? `<span class=rv>${esc(value(b.value))}</span>` : '')));
    const swipe = blks(b.swipe);
    if (url) r.classList.add('tap');
    else if (b.tap || swipe.length) {
      r.classList.add('tap');
      r.tabIndex = 0;
      r.setAttribute('role', 'button');
      r.onclick = () => (b.tap ? c.tap(b.tap) : toggle(r, b.id, c));
    }
    if (!swipe.length) return r;
    const w = el('div', 'rw');
    w.append(r);
    const bar = el('div', 'ra');
    bar.hidden = !c.open.has(`sw:${b.id}`);
    for (const s of swipe) {
      const x = btn(s.label, s.tone === 'bad' ? 'bad' : '');
      x.onclick = () => c.act({ action: s.action, block: b.id, values: s.values ?? {}, confirm: s.confirm }, x);
      bar.append(x);
    }
    w.append(bar);
    let x0 = 0;
    r.addEventListener('touchstart', e => { x0 = e.touches[0].clientX; }, { passive: true });
    r.addEventListener('touchend', e => { if (x0 - e.changedTouches[0].clientX > 40 && bar.hidden) toggle(r, b.id, c); }, { passive: true });
    return w;
  },
  card(b, c) {
    const k = el('div', `card ${esc(b.tone ?? '')} k-${esc(b.kind ?? '')}`);
    k.dataset.id = String(b.id ?? '');
    const fold = isBlk(b.fold) ? b.fold : null, main = fold && isBlk(fold.main) ? fold.main : null, shut = fold && !c.open.has(String(b.id));
    if (fold) {
      const line = el('div', 'fold');
      line.tabIndex = 0;
      line.setAttribute('role', 'button');
      line.setAttribute('aria-expanded', String(!shut));
      line.append(el('span', 'fl', esc(fold.line)));
      line.onclick = e => { if (!(e.target instanceof Element && e.target.closest('button'))) toggle(k, b.id, c, true); };
      if (main && shut) {
        const m = btn(main.label, 'main');
        m.onclick = () => c.act({ action: main.action, block: main.block, values: { ...obj(main.values), choice: main.value } }, m);
        line.append(m);
      }
      k.append(line);
      if (shut) return k;
    }
    const head = el('div', 'ch', `<b>${esc(b.title)}</b>` + (b.badge ? ` <span class="tag ${esc(tone(b.badge).tone ?? '')}">${esc(tone(b.badge).text)}</span>` : '') +
      (b.meta ? ` <span class=m>${esc(b.meta)}</span>` : ''));
    if (b.title || b.badge) k.append(head);
    if (b.subtitle) k.append(el('p', 'm', esc(tone(b.subtitle).text)));
    k.append(...kids(b.blocks, c));
    const buttons = blks(b.buttons);
    if (buttons.length) {
      const bar = el('div', 'bar');
      bar.append(...kids(buttons, c));
      k.append(bar);
    }
    return k;
  },
  text(b, c) {
    const t = el('div', 'txt', md(b.markdown, c.screen.startsWith('thread-')));
    t.dataset.md = String(b.markdown ?? '');
    return t;
  },
  stat(b) {
    const s = el('div', 'stats');
    for (const i of blks(b.items)) {
      const sp = isList(i.spark) ? i.spark.map(Number) : [], hi = Math.max(1, ...sp);
      const line = sp.length > 1 ? `<svg class=spark viewBox="0 0 ${sp.length - 1} 10" preserveAspectRatio=none aria-hidden=true><polyline points="${sp.map((v, x) => `${x},${10 - (v / hi) * 10}`).join(' ')}"/></svg>` : '';
      s.append(el('div', `st ${esc(i.tone ?? '')}`, `<b>${esc(i.value)}</b>${i.unit ? ` <small>${esc(i.unit)}</small>` : ''}<span class=m>${esc(i.label)}</span>${line}`));
    }
    return s;
  },
  chips(b, c) {
    const s = el('div', 'chips');
    const sel = isList(b.selected) ? b.selected : [];
    for (const o of blks(b.options)) {
      const x = btn(o.label, sel.includes(o.value) ? 'on' : '');
      x.setAttribute('aria-pressed', String(sel.includes(o.value)));
      x.onclick = () => {
        const next = !b.multi || o.value === 'all' ? [o.value] : sel.includes(o.value) ? sel.filter(v => v !== o.value) : [...sel.filter(v => v !== 'all'), o.value];
        c.act({ action: b.action, block: b.id, values: { [String(b.name)]: next.length ? next : ['all'] } }, x);
      };
      s.append(x);
    }
    return s;
  },
  choice(b, c) {
    const s = el('div', 'choice');
    const opts = blks(b.options).sort((x, y) => Number(y.value === b.recommended) - Number(x.value === b.recommended));
    for (const o of opts) {
      const x = btn(`${o.label}${o.value === b.recommended ? ' · recommended' : ''}`, 'pick');
      x.name = 'choice';
      x.onclick = () => c.act({ action: b.action, block: b.id, values: { ...obj(b.values), ...obj(o.values), choice: o.value }, confirm: o.confirm }, x);
      s.append(x);
    }
    const dflt = b.default, other = b.other;
    if (isBlk(dflt)) {
      const until = Date.parse(String(dflt.until)), m = Math.max(0, Math.round((until - Date.now()) / 60000));
      s.append(el('p', 'm', esc(String(dflt.label ?? '').replace('{rel}', `${m}m`))));
    }
    if (isBlk(other)) {
      const f = el('form', 'other');
      const i = el('input');
      Object.assign(i, { name: 'other', placeholder: other.placeholder ?? 'other…', autocomplete: 'off' });
      draft(i, `draft:${c.screen}:${String(b.id)}:other`);
      const go = btn('send');
      go.type = 'submit';
      f.append(i, go);
      f.onsubmit = e => {
        e.preventDefault();
        if (i.value.trim()) c.act({ action: b.action, block: b.id, values: { ...obj(b.values), choice: other.for ?? '', other: i.value } }, go).then(ok => ok && sent(f));
      };
      s.append(f);
    }
    return s;
  },
  button(b, c) {
    const x = btn(b.label, String(b.style ?? ''), urlOf(b.tap));
    x.dataset.id = String(b.id ?? '');
    x.onclick = () => (b.tap ? c.tap(b.tap) : c.act({ action: b.action, block: b.id, values: b.values ?? {}, confirm: b.confirm }, x));
    return x;
  },
  run(b, c) {
    const s = el('div', 'run', `<pre class=cmd>${esc(b.command)}</pre><p class=m>${[b.dir, b.touches, b.sha && `sha ${b.sha}`].filter(Boolean).map(esc).join(' · ')}</p>`);
    const bar = el('div', 'bar');
    for (const [choice, label] of [['approve', 'approve'], ['deny', 'deny']]) {
      const x = btn(label, choice === 'deny' ? 'bad' : 'primary');
      x.onclick = () => c.act({ action: b.action, block: b.id, values: { choice, sha: b.sha } }, x);
      bar.append(x);
    }
    s.append(bar);
    return s;
  },
  images(b) {
    const s = el('div', 'imgs');
    for (const i of blks(b.items)) {
      const a = href(el('a'), new URL(String(i.src).replace(/^[/]/, ''), document.baseURI).href);
      a.onclick = e => {
        e.preventDefault();
        const all = [...document.querySelectorAll<HTMLAnchorElement>('main .imgs a')];
        viewer(all.map(x => [x.href, x.querySelector('img')?.alt ?? '']), Math.max(0, all.indexOf(a)));
      };
      const im = el('img');
      Object.assign(im, { src: a.href, alt: i.alt ?? '', loading: 'lazy' });
      a.append(im);
      s.append(a);
    }
    return s;
  },
  form(b, c) {
    const f = el('div', `form${b.submit ? ' say' : ''}`);
    f.dataset.id = String(b.id ?? '');
    for (const x of blks(b.fields)) f.append(field(x, `draft:${c.screen}:${String(b.id)}:`));
    const s = b.submit;
    if (!isBlk(s)) return f;
    const go = btn(s.label), word = go.firstElementChild ?? go;
    const send = () => {
      const v = values(f);
      if (!String(v.text ?? '').trim()) return;
      word.textContent = 'sending…';
      c.act({ action: s.action, block: b.id, values: { ...obj(s.values), ...v } }, go).then(ok => ok && sent(f)).finally(() => { word.textContent = String(s.label ?? ''); });
    };
    go.onclick = send;
    f.querySelector('input')?.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); send(); } });
    f.append(go);
    return f;
  },
  composer,
  sheet(b, c) {
    const s = el('div', 'shb');
    s.append(...kids(b.blocks, c));
    return s;
  },
};

type Att = { name: string; type: string; data: string };
type Draft = { files: Att[]; picks: Blk; q?: number; st?: string; steer?: boolean; stopping?: boolean };
const drafts = new Map<string, Draft>();

const read = (f: File) => new Promise<Att>(ok => {
  const r = new FileReader();
  r.onload = () => ok({ name: f.name, type: f.type, data: String(r.result).split(',')[1] ?? '' });
  r.readAsDataURL(f);
});

function composer(b: Blk, c: Ctx): HTMLElement {
  const key = `draft:${c.screen}:${String(b.id)}:text`, acts = obj(b.acts), base = obj(b.values), act = (k: string) => String(acts[k] ?? '');
  const d: Draft = drafts.get(key) ?? { files: [], picks: {} };
  drafts.set(key, d);
  if (!b.working) d.stopping = d.steer = false;
  const f = el('div', 'cmp');
  f.dataset.id = String(b.id ?? '');
  const ta = el('textarea');
  Object.assign(ta, { name: 'text', rows: 1, value: get(key) ?? '', placeholder: String(b.placeholder ?? '') });
  const grow = () => { ta.style.height = 'auto'; ta.style.height = `${ta.scrollHeight}px`; };
  ta.oninput = () => { put(key, ta.value || null); d.st = d.st === 'sent' ? '' : d.st; grow(); };
  ta.onkeydown = e => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); send(); } };
  const pick = el('input');
  Object.assign(pick, { type: 'file', multiple: true, hidden: true });
  pick.onchange = async () => {
    d.files = [...d.files, ...(await Promise.all([...(pick.files ?? [])].map(read)))].slice(0, 10);
    pick.value = '';
    d.st = '';
    draw();
  };
  const post = async (action: string, values: Blk, x?: HTMLElement) => c.act({ action, block: b.id, values: { ...base, ...values } }, x);
  const send = async (steer = false) => {
    if (!ta.value.trim() && !d.files.length) return;
    const queue = !steer && (b.working || d.q != null) ? act('queue') : '', action = steer ? act('steer') : queue || String(b.action);
    const v: Blk = { ...d.picks, text: ta.value, name: d.files.map(x => x.name).join('/'), data: d.files.map(x => x.data).join(',') };
    if (queue && d.q != null) v.q = d.q;
    [d.st, d.steer] = ['sending', false];
    draw();
    if (await post(action, v, steer ? undefined : f.querySelector<HTMLElement>('.cr button.primary') ?? undefined)) {
      Object.assign(d, { files: [], q: undefined, st: 'sent' });
      ta.value = '';
      put(key, null);
      grow();
    } else d.st = 'failed';
    draw();
  };
  function draw() {
    const top: HTMLElement[] = [];
    if (b.working) {
      const p = el('div', 'pill');
      const tick = () => {
        if (!navigator.onLine) return p.replaceChildren(el('span', 'r', 'offline · tap to retry'));
        const s = Math.max(0, Math.floor((Date.now() - Date.parse(String(obj(b.working).since))) / 1000));
        p.replaceChildren(...nodes(`${figure({ state: 'working' })}<span class=pt>${d.stopping ? 'stopping…' : `working ${s >= 60 ? `${Math.floor(s / 60)}m ` : ''}${s % 60}s`}</span>`));
        const x = btn('stop', 'bad');
        x.onclick = async e => { e.stopPropagation(); if (await post(act('stop'), {}, x)) { d.stopping = true; tick(); } };
        if (act('stop') && !d.stopping) p.append(x);
      };
      p.onclick = () => { if (!navigator.onLine) f.dispatchEvent(new CustomEvent('redraw', { bubbles: true })); };
      tick();
      const t = setInterval(() => (p.isConnected ? tick() : clearInterval(t)), 1000);
      top.push(p);
    }
    for (const m of blks(b.queued)) {
      const text = String(m.text ?? '');
      const r = el('div', 'qd', `<span class=qt>${esc(text)}</span>`);
      r.dataset.q = String(m.q);
      const now = btn('send now', 'primary'), edit = btn('edit'), rm = btn('remove', 'bad');
      now.onclick = () => post(act('steer'), { q: m.q }, now);
      edit.onclick = () => { [d.q, ta.value] = [Number(m.q), text]; put(key, text); grow(); ta.focus(); draw(); };
      rm.onclick = () => post(act('unqueue'), { q: m.q }, rm);
      r.append(now, edit, rm);
      top.push(r);
    }
    if (d.files.length) {
      const th = el('div', 'th');
      d.files.forEach((a, i) => {
        const t = el('span', 'tb', a.type.startsWith('image/') ? `<img alt="${esc(a.name)}" src="data:${esc(a.type)};base64,${a.data}">` : `<span>${esc(a.name)}</span>`);
        const x = btn('×');
        x.setAttribute('aria-label', `remove ${a.name}`);
        x.onclick = () => { d.files.splice(i, 1); draw(); };
        t.append(x);
        th.append(t);
      });
      top.push(th);
    }
    if (d.st) {
      const s = el('p', `cs ${d.st === 'failed' ? 'r' : 'm'}`, esc({ sending: 'sending…', sent: '✓ sent', failed: 'not sent' }[d.st] ?? ''));
      if (d.st === 'failed') {
        const x = btn('retry', 'bad');
        x.onclick = () => send();
        s.append(' ', x);
      }
      top.push(s);
    }
    const row = el('div', 'cr'), add = btn('+'), go = btn(d.q != null ? 'save' : b.working ? 'queue' : String(b.label ?? ''), 'primary');
    add.setAttribute('aria-label', 'attach');
    add.onclick = () => pick.click();
    let held = false, hold = 0;
    go.onpointerdown = () => { held = false; hold = window.setTimeout(() => { held = true; if (b.working && act('steer')) { d.steer = true; draw(); } }, 500); };
    go.onpointerup = go.onpointerleave = () => clearTimeout(hold);
    go.oncontextmenu = e => e.preventDefault();
    go.onclick = () => { if (!held) send(); held = false; };
    if (d.st === 'sending') go.setAttribute('aria-busy', 'true');
    row.append(add, pick, ta, go);
    if (d.steer) {
      const st = btn('steer');
      st.onclick = () => send(true);
      row.append(st);
    }
    const chips = blks(b.chips).map(ch => {
      const s = el('div', 'chips'), n = String(ch.name);
      const cur = String(d.picks[n] ?? ch.cur ?? '');
      for (const o of isList(ch.all) ? ch.all.map(String) : []) {
        const x = btn(`${n} ${o}`, o === cur ? 'on' : '');
        x.setAttribute('aria-pressed', String(o === cur));
        x.onclick = () => (act('model') ? post(act('model'), { [n]: o }, x) : ((d.picks[n] = o), draw()));
        s.append(x);
      }
      return s;
    });
    f.replaceChildren(...top, row, ...chips);
  }
  draw();
  setTimeout(grow);
  return f;
}

/** #1086: the image viewer over the page: fitted; a double tap or a pinch zooms, a swipe goes to the next, [save]. */
export function viewer(list: [string, string][], at: number) {
  const v = el('div', 'iv'), im = el('img'), bar = el('div', 'bar'), save = el('a', '', '<span>save</span>');
  v.id = 'iv';
  let z = 1, x0 = 0, d0 = 0, t0 = 0;
  const zoom = (k: number) => { z = Math.min(4, Math.max(1, k)); im.style.transform = `scale(${z})`; v.classList.toggle('z', z > 1); };
  const to = (i: number) => {
    at = (i + list.length) % list.length;
    [im.src, im.alt] = list[at];
    save.href = list[at][0];
    save.download = list[at][1] || 'image';
    const n = bar.querySelector('.m');
    if (n) n.textContent = `${at + 1} / ${list.length}`;
    zoom(1);
  };
  const shut = () => { v.remove(); removeEventListener('keydown', key); };
  const key = (e: KeyboardEvent) => { if (e.key === 'Escape') shut(); else if (e.key === 'ArrowRight') to(at + 1); else if (e.key === 'ArrowLeft') to(at - 1); };
  const close = btn('close'), prev = btn('‹'), next = btn('›');
  close.onclick = shut;
  prev.onclick = () => to(at - 1);
  next.onclick = () => to(at + 1);
  bar.append(prev, el('span', 'm'), next, save, close);
  im.addEventListener('dblclick', () => zoom(z > 1 ? 1 : 2.5));
  v.addEventListener('click', e => { if (e.target === v) shut(); });
  v.addEventListener('touchstart', e => {
    const [a, b] = [e.touches[0], e.touches[1]];
    if (b) d0 = Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) / z;
    else if (Date.now() - t0 < 300) zoom(z > 1 ? 1 : 2.5);
    [x0, t0] = [a.clientX, Date.now()];
  }, { passive: true });
  v.addEventListener('touchmove', e => {
    const [a, b] = [e.touches[0], e.touches[1]];
    if (b && d0) zoom(Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) / d0);
  }, { passive: true });
  v.addEventListener('touchend', e => {
    const dx = e.changedTouches[0].clientX - x0;
    if (!e.touches.length) d0 = 0;
    if (z === 1 && Math.abs(dx) > 50 && list.length > 1) to(at + (dx < 0 ? 1 : -1));
  }, { passive: true });
  v.append(im, bar);
  to(at);
  document.body.append(v);
  addEventListener('keydown', key);
  close.focus();
}

function toggle(e: HTMLElement, id: unknown, c: Ctx, card = false) {
  const k = `${card ? '' : 'sw:'}${id}`;
  if (c.open.has(k)) c.open.delete(k); else c.open.add(k);
  if (card) e.dispatchEvent(new CustomEvent('redraw', { bubbles: true }));
  else if (e.nextElementSibling instanceof HTMLElement) e.nextElementSibling.hidden = !c.open.has(k);
}

function field(x: Blk, key: string): HTMLElement {
  const l = el('label', 'fd');
  l.append(el('span', 'fk', esc(x.label ?? x.name)));
  if (x.type === 'toggle') {
    const i = el('input');
    Object.assign(i, { type: 'checkbox', name: x.name, checked: !!x.value });
    l.append(i);
  } else if (x.type === 'picker') {
    const s = el('select');
    s.name = String(x.name);
    const opts = [...new Set([x.value, ...(isList(x.options) ? x.options : [])].filter(o => o != null && o !== ''))];
    for (const o of opts) s.append(Object.assign(el('option'), { value: o, textContent: o, selected: o === x.value }));
    l.append(s);
  } else {
    const i = el('input');
    Object.assign(i, { type: x.type === 'secret' ? 'password' : 'text', name: x.name, value: x.value ?? '', placeholder: x.placeholder ?? '', autocomplete: 'off' });
    i.dataset.was = i.value;
    if (x.type !== 'secret') draft(i, key + String(x.name));
    l.append(i);
  }
  return l;
}

/** The values of every field under `root`, as an action's `values` (a toggle is a bool). */
export function values(root: HTMLElement): Blk {
  const v: Blk = {};
  root.querySelectorAll<HTMLInputElement | HTMLSelectElement>('.fd input, .fd select').forEach(i => {
    v[i.name] = i instanceof HTMLInputElement && i.type === 'checkbox' ? i.checked : i.value;
  });
  return v;
}

export function kids(bs: unknown, c: Ctx): HTMLElement[] {
  return blks(bs).map(b => render(b, c)).filter((e): e is HTMLElement => !!e);
}

/** A block as DOM; a type this page has no renderer for draws its children, else nothing. */
export function render(b: Blk, c: Ctx): HTMLElement | null {
  const f = R[String(b.type)];
  if (f) return f(b, c);
  if (!b.blocks) return null;
  const d = el('div');
  d.append(...kids(b.blocks, c));
  return d;
}
