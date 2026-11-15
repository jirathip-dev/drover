// #927: one renderer per block type, the set cooee draws (screen, section, list, row, card, text, stat, chips, choice,
// button, run, images, form and its fields and `submit`, sheet). A block that acts calls `ctx.act`; a tap that goes somewhere
// calls `ctx.tap`. No block knows which screen it is on.
import { type Blk, blks, isBlk, isList, obj } from './ops';
import { esc, figure, nodes, word } from './theme';

export type Act = { action: unknown; block?: unknown; values?: unknown; confirm?: unknown };
export type Ctx = {
  screen: string;
  act: (a: Act, el?: HTMLElement) => void | Promise<void>;
  tap: (t: unknown) => void;
  open: Set<string>;
};

export const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls = '', html = ''): HTMLElementTagNameMap[K] => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html) e.replaceChildren(...nodes(html));
  return e;
};
const btn = (label: unknown, cls = '') => {
  const b = el('button', cls);
  b.type = 'button';
  b.append(el('span', '', esc(label)));
  return b;
};
const GROUP: Record<string, string> = { waiting: 'needs', blocked: 'needs', asleep: 'idle', in: 'working' };
const tone = (v: unknown): Blk => (isBlk(v) ? v : { text: v });
const SH: Record<string, string> = { needs: 'r', working: 'g' };

/** Minimal markdown: code fences, `code`, **bold**, [links](url); everything else escaped. */
export function md(s: unknown): string {
  return String(s ?? '').split(/```(?:[a-z]*)\n?/).map((part, i) => i % 2 ? `<pre>${esc(part.replace(/\n$/, ''))}</pre>` :
    esc(part).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
      .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a href="$2" target=_blank rel=noopener>$1</a>').replace(/\n/g, '<br>')).join('');
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
      const x = btn(m.label);
      x.onclick = () => (m.tap ? c.tap(m.tap) : c.act({ action: m.action, values: m.values, confirm: m.confirm }, x));
      head.append(x);
    }
    s.append(head, ...kids(b.blocks, c));
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
    const r = el('div', `row${b.style === 'link' ? ' link' : ''}${b.indent ? ' in' + Math.min(Number(b.indent), 3) : ''}`);
    r.dataset.id = String(b.id ?? '');
    const sub = b.subtitle == null ? null : tone(b.subtitle), badge = b.badge == null ? null : tone(b.badge);
    r.replaceChildren(...nodes((isBlk(b.figure) ? figure(b.figure) : '') + `<span class=rm><span class=rt>${esc(b.title)}</span>` +
      (badge ? ` <span class="tag ${esc(badge.tone ?? '')}">${esc(badge.text)}</span>` : '') +
      (sub ? `<span class="rs ${esc(sub.tone ?? '')}">${esc(sub.text)}</span>` : '') + '</span>' +
      (b.value != null ? `<span class=rv>${esc(value(b.value))}</span>` : '')));
    const swipe = blks(b.swipe);
    if (b.tap || swipe.length) {
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
  text: b => el('div', 'txt', md(b.markdown)),
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
      const go = btn('send');
      go.type = 'submit';
      f.append(i, go);
      f.onsubmit = e => {
        e.preventDefault();
        if (i.value.trim()) c.act({ action: b.action, block: b.id, values: { ...obj(b.values), choice: other.for ?? '', other: i.value } }, go);
      };
      s.append(f);
    }
    return s;
  },
  button(b, c) {
    const x = btn(b.label, String(b.style ?? ''));
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
      const a = el('a');
      a.href = new URL(String(i.src).replace(/^\//, ''), document.baseURI).href;
      a.target = '_blank';
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
    for (const x of blks(b.fields)) f.append(field(x));
    const s = b.submit;
    if (!isBlk(s)) return f;
    const go = btn(s.label), word = go.firstElementChild ?? go;
    const send = () => {
      const v = values(f);
      if (!String(v.text ?? '').trim()) return;
      word.textContent = 'sending…';
      Promise.resolve(c.act({ action: s.action, block: b.id, values: { ...obj(s.values), ...v } }, go)).finally(() => { word.textContent = String(s.label ?? ''); });
    };
    go.onclick = send;
    f.querySelector('input')?.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); send(); } });
    f.append(go);
    return f;
  },
  sheet(b, c) {
    const s = el('div', 'shb');
    s.append(...kids(b.blocks, c));
    return s;
  },
};

function toggle(e: HTMLElement, id: unknown, c: Ctx, card = false) {
  const k = `${card ? '' : 'sw:'}${id}`;
  if (c.open.has(k)) c.open.delete(k); else c.open.add(k);
  if (card) e.dispatchEvent(new CustomEvent('redraw', { bubbles: true }));
  else if (e.nextElementSibling instanceof HTMLElement) e.nextElementSibling.hidden = !c.open.has(k);
}

/** A form field (toggle, text, picker, secret) as a labelled control named by the field's `name`. */
function field(x: Blk): HTMLElement {
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
