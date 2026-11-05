// #267: the Host tab (#141, #234), drawn from API v1's `host` (pure: tests/host_tab.rs runs it under node). Its groups
// in a fixed order, HOSTS (a card per host), FLEET DISK, SERVICES, FLEETS; serve.js ranks them, draws each sparkline
// and chart from the rows' series, the theme's load miniature and the `theme` row's colours. #440: it is System's
// Hosts & health; ALERTS, the accounts and DeepSeek moved to its Notifications and Agent providers (system.ts).
import type { Bar, Cell, HostCard, HostTab, Row } from '../../assets/types/api';
import { esc } from './events.ts'; // node runs this file as is (tests/host_tab.rs): the extension

/** A form: a live board's posts; an export's are inert. */
export const form = (d: HostTab, rest = '') => `<form ${d.live ? 'method=post' : 'inert'}${rest}>`;
export const hidden = (op: string, mt: string) => `<input type=hidden name=op value=${op}><input type=hidden name=fleet value=""><input type=hidden name=mtime value=${mt}>`;

/** Escaped text with each `command` as a code chip. */
const chips = (s: string) => s.split('`').map((p, i) => (i % 2 ? `<code>${esc(p)}</code>` : esc(p))).join('');

/** #141: a 12-cell text gauge, closed by the limit's tick. */
export const gauge = (b: Bar) => `<span class="bar ${b.t}">▕${'█'.repeat(b.n)}<i>${'░'.repeat(12 - b.n)}</i>▏</span>`;
const pct = (b: Bar | null | undefined) => (b?.pct ? ` (${b.pct}%)` : '');

/** #141: a row with status `t`: its label, then `cells`; with `more` (a raw line, the accounts) it opens on tap. */
export function hrow(k: string, t: string, cells: string, ago: string, more: string): string {
  const l = `<span class=k>${esc(k)}</span>`;
  if (!more) return `<p class=gr data-t=${t}>${l}${cells}</p>`;
  return `<details class=rw data-t=${t}><summary>${l}${cells}<span class=t>${ago}<i class=cv></i></span></summary>${more}</details>`;
}
export const kv = (k: string, t: string, val: string) => hrow(k, t, `<span class=w>${val}</span>`, '', '');

/** A card row's value on one line: the gauge, its bold value (`▶` past the limit), text, the limit and muted notes. */
function val(c: Cell): string {
  const b = c.b == null ? '' : c.bt ? `<b class=${c.bt}>${c.bt === 'r' ? '▶ ' : ''}${esc(c.b)}</b>` : `<b>${esc(c.b)}</b>`;
  const parts = [c.bar ? gauge(c.bar) : '', b, esc(c.text), c.of && `<span class=m>${esc(c.of)}</span>`];
  return parts.filter(Boolean).concat(c.notes.map(n => `<span class=mt>${esc(n)}</span>`)).join(' ') + pct(c.bar);
}

/** #234: a metric's row carries its sparkline (`data-s`) and opens to its chart; the rest are plain rows. */
function cell(c: Cell): string {
  if (!c.key) return kv(c.k, c.t, val(c));
  const at = `${c.l == null ? '' : ` data-l=${c.l}`}${c.key === 'mem' ? ` data-p="${c.p}"` : ''}${c.x ? ` data-x="${esc(c.x)}"` : ''}`;
  return `<details class=rw data-t=${c.t} data-k=${c.key} data-s="${c.s}"${at}><summary><span class=k>${esc(c.k)}</span><span class=w>${val(c)}</span><span class=t><i class=cv></i></span><span class=sp><svg class=spk></svg><i>24h</i></span></summary></details>`;
}

/** #234: a host's card: its dot, name, state words, [sign in ▾] (#440: Claude's sign-in is Agent providers') and
 * [off]/[on], then its rows. */
function card(d: HostTab, h: HostCard): string {
  const n = esc(h.name), tag = h.main ? '<span class="tag m">main</span>' : '';
  const su = d.live ? `<details class=su><summary>sign in ▾</summary><div>${h.setups.filter(s => s !== 'claude').map(s => `<button type=button data-su="${esc(s)}" data-host="${n}">${esc(s)}</button>`).join('')}</div></details>` : '';
  const why = h.to === 'off' ? ` no new lanes; ${h.lanes} running lanes finish` : '';
  const btn = d.live && h.to ? `${form(d)}${hidden('host', d.mtime)}<input type=hidden name=host value="${n}"><input type=hidden name=to value=${h.to}><button data-confirm="turn ${n} ${h.to}?${why}">${h.to}</button></form>` : '';
  return `<article class=hcd data-h="${n}" data-c=${h.dot} data-n="${h.n}"><div class=hhd><span class=hnm><i class="dt ${h.dot}"></i><b>${n}</b>${tag}<span class="hw ${h.tone}">${esc(h.words)}</span></span>${su}${btn}</div>${h.rows.map(cell).join('')}</article>`;
}

/** #98: a service: green, amber or red, what clears it otherwise. */
const service = (r: Row) => kv(r.name, r.tone, `<b class=${r.tone}>${esc(r.detail)}</b>${r.tone !== 'g' ? ` <span class="mt fx">→ ${chips(r.fix)}</span>` : ''}`);

/** The tab's HTML: its groups, each a `<details>` whose label column is the longest label's width (at most 11ch). */
export function tab(d: HostTab): string {
  let m = '', s = '', f = '';
  const k = d.disk;
  if (k) {
    const ago = k.ago == null ? '' : ` <span class="mt fx">· ${k.ago} ago</span>`;
    m += kv('fleet disk', k.bar.t, `${gauge(k.bar)} <b class=${k.bar.t}>${k.bar.t === 'r' ? '▶ ' : ''}${k.gb}</b> <span class=m>/ ${k.budget} GB</span>${pct(k.bar)}${ago}`);
  }
  if (d.orphans > 0 && d.rw) {
    const n = d.orphans, x = n === 1 ? '' : 's';
    m += `${form(d)}${hidden('orphans', d.mtime)}<button data-confirm="Remove ${n} orphan folder${x} under the worktree root?">remove orphans</button> <span class=m>${n} folder${x} from no project</span></form>`;
  }
  const l = d.leftovers;
  if (l) {
    const btn = d.rw ? `${form(d)}${hidden('reap', d.mtime)}<button data-confirm="TERM, then KILL, ${l.n} leftover process(es)?">reap</button></form>` : '';
    m += kv('leftovers', 'a', `<b class=a>${l.n}</b> processes left in closed lanes <span class=mt>· ${l.mb} MB</span>${btn}`);
  }
  if (d.spawns) m += kv(d.spawns.name, d.spawns.tone, esc(d.spawns.detail));
  s += d.services.map(service).join('');
  // #172: the board's theme; amber when its theme.json is invalid (farm shows), with the first problem (#241).
  const th = d.theme;
  s += th.tone === 'g' ? kv('theme', 'g', `<b>${esc(th.name)}</b>`) : kv('theme', 'a', `<b class=a>farm</b> <span class=mt>\`${esc(th.name)}\`: ${esc(th.detail)}</span>`);
  // Each fleet's last `driven` result in plain words, the raw line on tap; an owner's pause is grey (no problem).
  for (const x of d.fleets) {
    const sep = x.tone === 'g' || !x.text ? ' ' : ' · ';
    f += hrow(x.name, x.tone, `<span class=x><b class=${x.tone}>${esc(x.word)}</b>${sep}${chips(x.text)}</span>`, x.ago, x.raw == null ? '' : `<pre>${esc(x.raw)}</pre>`);
  }
  const hosts = `<div class=hgd>${d.hosts.map(h => card(d, h)).join('')}</div>`;
  const gs: [string, string][] = [['HOSTS', hosts], ['FLEET DISK', m], ['SERVICES', s], ['FLEETS', f]];
  // #183: the label column is the longest label's width, at most 11ch (serve.css `--kw`).
  const kw = Math.min(11, Math.max(0, ...gs.flatMap(([, h]) => h.split('<span class=k>').slice(1).map(x => [...x.split('<')[0]].length))));
  return gs.filter(g => g[1]).map(([n, rows]) => `<details class=grp style=--kw:${kw}ch><summary><i class=dt></i><span class=gn>${n}</span><span class=gs></span><i class=cv></i></summary>${rows}</details>`).join('');
}
