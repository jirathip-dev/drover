// #266: the Events tab (#170), drawn from API v1's `state` and `events` (pure: tests/events_tab.rs runs it under node).
// Chips `[all] [important] [● fleet]…`, then each view's rows newest first under sticky time groups; a fold is one row
// (`all`: every event, unfolded); `shown` rows a view, [older] the next 100. The words are the server's (`Ev.text`).
import type { Ev, Fleet } from '../../assets/types/api';

/** #170: noise a run of folds into one `kind ×N` row (by kind, so a failure never shares a row with `clean`). */
const NOISE: Record<string, string> = { clean: 'lanes cleaned', 'push-sent': 'alerts sent', 'notify-failed': 'alerts that never arrived' };
const MON = 'jan feb mar apr may jun jul aug sep oct nov dec'.split(' ');

/** The chips' choice, kept per phone (`events:filter`): a view, and the fleets shown (none: every fleet). */
export type Filter = { k: string; fl: string[] };
export type Draw = { now: number; tz: number; fleets: Fleet[]; all: boolean; shown: Record<string, number>; f: Filter };

export const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** `events::age`: 59s, 119s, 2m … 119m, 2h … 47h, 2d. */
export function age(s: number): string {
  if (s < 120) return `${s}s`;
  if (s < 7200) return `${Math.floor(s / 60)}m`;
  return s < 172_800 ? `${Math.floor(s / 3600)}h` : `${Math.floor(s / 86400)}d`;
}

const p2 = (n: number) => String(n).padStart(2, '0');
const mod = (a: number, b: number) => ((a % b) + b) % b;

/** #170: the event as pretty JSON, a multi-line or long string as its first line and `… (N more lines)`, 160 chars at most. */
export function raw(e: Ev): string {
  const o: Record<string, unknown> = {};
  if (e.seq) o.seq = e.seq;
  Object.assign(o, { ts: e.ts, fleet: e.fleet }, e.issue == null ? {} : { issue: e.issue }, { type: e.type });
  for (const [k, x] of Object.entries(e.data)) {
    const c = typeof x === 'string' ? [...x] : [];
    if (typeof x !== 'string' || (!x.includes('\n') && c.length <= 160)) { o[k] = x; continue; }
    const ls = x.split('\n').map(l => l.replace(/\r$/, ''));
    if (ls[ls.length - 1] === '') ls.pop();
    const one = [...(ls[0] ?? '')];
    o[k] = one.slice(0, 160).join('') + (one.length > 160 ? '…' : '') + (ls.length > 1 ? ` … (${ls.length - 1} more lines)` : '');
  }
  return JSON.stringify(o, null, 2);
}

type Fold = { evs: Ev[] };

/** A view's rows: `imp` only the important ones; a run of one kind of noise, or of the same words at the same second, folds. */
export function rows(list: Ev[], imp: boolean, all: boolean): Fold[] {
  const order = [...list].reverse().sort((a, b) => b.ts - a.ts), out: Fold[] = [];
  for (const e of order) {
    if (imp && !e.imp) continue;
    const r = out[out.length - 1], p = r?.evs[0], noise = e.type in NOISE;
    if (r && p && !all && p.fleet === e.fleet && (noise ? p.type === e.type : p.ts === e.ts && p.text === e.text)) r.evs.push(e);
    else out.push({ evs: [e] });
  }
  return out;
}

/** The tab's HTML: the chips, then a `data-v` per view (the filter's shown), each its `shown` rows and [older] past them. */
export function tab(list: Ev[], d: Draw): string {
  const fc = new Map(d.fleets.map(f => [f.name, f])), local = (t: number) => t + d.tz, day = (t: number) => Math.floor(local(t) / 86400);
  const fl = d.f.fl.filter(f => fc.has(f)), hide = (f: string) => fl.length > 0 && !fl.includes(f);
  const group = (t: number) => {
    const a = Math.max(0, d.now - t);
    if (a < 300) return 'now';
    if (a < 3600) return '5m';
    if (a < 7200) return '1h';
    if (day(t) === day(d.now)) return 'earlier today';
    if (day(t) + 1 === day(d.now)) return 'yesterday';
    const x = new Date(Math.max(0, day(t)) * 86_400_000);
    return `${MON[x.getUTCMonth()]} ${x.getUTCDate()}`;
  };
  const hm = (t: number) => { const l = mod(local(t), 86400); return [Math.floor(l / 3600), Math.floor((l % 3600) / 60), l % 60].map(p2); };
  let h = '<div class=chips role=group aria-label="filter events">';
  for (const [k, l] of [['all', 'all'], ['imp', 'important']]) h += `<button type=button data-k=${k} aria-pressed=${k === d.f.k}><span>${l}</span></button>`;
  for (const f of d.fleets) {
    const n = esc(f.name);
    h += `<button type=button class="fc2 k-${f.color}" data-fl="${n}" aria-pressed=${fl.includes(f.name)}><span><i class=fd></i>${n}</span></button>`;
  }
  h += '</div>';
  for (const [v, imp] of [['imp', true], ['all', false]] as const) {
    const every = rows(list, imp, d.all), rs = d.all ? every : every.slice(0, d.shown[v] ?? 100), last = rs[rs.length - 1];
    const back = last ? ` · back to ${group(last.evs[0].ts)} ${hm(last.evs[0].ts).slice(0, 2).join(':')}` : '';
    const more = d.all ? '' : ' · <a href="?all=1">all events, unfolded</a>';
    h += `<div data-v=${v}${v === d.f.k ? '' : ' hidden'}><p class="m es">${rs.length} ${imp ? 'important rows' : 'rows'}${back}${more}</p>`;
    for (let i = 0; i < rs.length;) {
      const g = group(rs[i].evs[0].ts);
      let n = 0;
      while (i + n < rs.length && group(rs[i + n].evs[0].ts) === g) n++;
      const seen = rs.slice(i, i + n).filter(r => !hide(r.evs[0].fleet)).length;
      h += `<div class=eg${seen ? '' : ' hidden'}><h3><span>${g}</span><span>${seen}</span></h3>`;
      for (const { evs } of rs.slice(i, i + n)) {
        const e = evs[0], f = fc.get(e.fleet), k = f ? ` k-${f.color}` : '';
        const num = f && e.issue != null
          ? `<a class=n href="https://github.com/${esc(f.repo)}/issues/${e.issue}">#${e.issue}</a>`
          : `<span class=n>${e.issue == null ? '' : '#' + e.issue}</span>`;
        const solo = evs.length === 1 || (e.issue != null && evs.every(x => x.issue === e.issue));
        const w = NOISE[e.type] && evs.length > 1 ? `${e.type} ×${evs.length} · ${NOISE[e.type]}` : solo ? e.text : e.text.replace(' · ', ` ×${evs.length} · `);
        let lk = '';
        if (f && e.issue != null) {
          const gh = `https://github.com/${esc(f.repo)}`, a = (u: string, t: string) => `[<a href="${u}">${t}</a>]`, l = [a(`${gh}/issues/${e.issue}`, 'issue')];
          if (e.pr != null) l.push(a(`${gh}/pull/${e.pr}`, `PR #${e.pr}`), a(`${gh}/pull/${e.pr}/checks`, 'checks'));
          else if (e.run) l.push(a(esc(e.run), 'checks'));
          lk = `<span>${l.join(' ')}</span>`;
        }
        const at = `${hm(e.ts).join(':')} · ${esc(e.fleet)} · ${esc(e.type)}`, via = e.data.via === 'web' ? ' (web)' : '';
        const body = evs.length > 1 ? `<ul class=fl2>${evs.map(x => `<li>${esc(x.plain)}</li>`).join('')}</ul>` : `<pre>${esc(raw(e))}</pre>`;
        const coat = e.coat ? ` style="${e.coat.replace(/--/g, '--r')}"` : '';
        h += `<details class="er${k}${e.imp ? ' imp' : ''}" data-f="${esc(e.fleet)}"${hide(e.fleet) ? ' hidden' : ''}><summary><i class="kg t-${e.tone}" data-act=${e.act}${coat}>${e.glyph}</i><i class=fd></i>${num}<span class=sm>${esc(w)}${via}</span><span class=ag>${age(Math.max(0, d.now - e.ts))}</span></summary><div class=ex><p class=lk>${lk}<span class=m>${at}</span></p>${body}</div></details>`;
      }
      h += '</div>';
      i += n;
    }
    if (rs.length < every.length) h += '<button type=button class=eo><span>older</span></button>';
    h += '</div>';
  }
  return h;
}
