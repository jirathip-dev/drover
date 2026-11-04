// #265: the TypeScript board's start, a typed client for the JSON API v1 (`/api/v1/`, docs/serve.md). The tabs move
// here one PR each (Events, Host, Queue, Lanes); #266: Events is drawn here, #267 Host; the other tabs are still the HTML board's.
import type { Chat, Ev, Events, HostTab, Reply, State } from '../../assets/types/api';
import { rows, tab, type Filter } from './events';
import { tab as hostTab } from './host';

/** serve.js's: paint a section's new HTML (rows keep their open state, changed ones flash, row sprites drawn). */
declare function frag(id: string, html: string): void;

const base = new URL('api/v1/', document.baseURI);

async function get<T>(path: string): Promise<T> {
  const r = await fetch(new URL(path, base));
  if (!r.ok) throw new Error(`${path}: ${r.status} ${((await r.json()) as Reply).message}`);
  return (await r.json()) as T;
}

export const api = {
  state: () => get<State>('state'),
  /** #259: the events after `since` (#266: up to `until`); `refresh` means read `state` again. */
  events: (since: number, until?: number) => get<Events>(`events?since=${since}${until == null ? '' : `&until=${until}`}`),
  host: () => get<HostTab>('host'),
  lane: (agent: string, before = 0) => get<Chat>(`lane/${encodeURIComponent(agent)}?before=${before}`),
  /** A form op (`answer`, `drive`, `pause`, `extend`, …) with the HTML form's fields; `mtime` is `State.mtime`. */
  async post(op: string, fields: Record<string, string>): Promise<Reply> {
    const r = await fetch(new URL(op, base), { method: 'POST', body: new URLSearchParams(fields) });
    return (await r.json()) as Reply;
  },
};

// #266: the Events tab. `evs` holds the log's events after `lo` up to `seq`: the newest 500 at first, 500 more while
// the open view has no more rows than it shows ([older] adds 100); `?all=1` reads the whole log, unfolded. serve.js's
// `/live` says `herd-ev` (read the new events) and `herd-refresh` (read it all again).
const all = new URLSearchParams(location.search).get('all') === '1', shown: Record<string, number> = { imp: 100, all: 100 };
let evs: Ev[] = [], lo = 0, seq = 0, skew = 0, st: State | null = null, q: Promise<unknown> = Promise.resolve();
let F: Filter = { k: 'imp', fl: [] };
try {
  const f = JSON.parse(localStorage.getItem('events:filter') ?? '') as Filter;
  if (['all', 'imp'].includes(f.k) && Array.isArray(f.fl)) F = f;
} catch {}
const run = (f: () => Promise<unknown>) => (q = q.then(f).catch(() => {}));
const draw = () => st && frag('events', tab(evs, { now: Math.floor(Date.now() / 1000 + skew), tz: st.tz, fleets: st.fleets, all, shown, f: F }));
async function fill() {
  while (lo > 0 && (all || rows(evs, F.k === 'imp', false).length <= shown[F.k])) {
    const since = Math.max(0, lo - 500), r = await api.events(since, lo);
    evs = r.list.filter(e => e.seq > since && e.seq <= lo).concat(evs);
    lo = since;
  }
}
async function load() {
  st = await api.state();
  [skew, seq, lo, evs] = [st.now - Date.now() / 1000, st.seq, st.seq, []];
  await fill();
  draw();
}
async function since() {
  const r = await api.events(seq);
  if (r.refresh) return load();
  evs = evs.concat(r.list.filter(e => e.seq > seq));
  seq = Math.max(seq, r.seq);
  draw();
}
document.addEventListener('click', e => {
  const b = (e.target as Element).closest<HTMLElement>('#events .chips button, #events .eo');
  if (!b) return;
  if (b.matches('.eo')) shown[b.closest<HTMLElement>('[data-v]')?.dataset.v ?? F.k] += 100;
  else if (b.dataset.k) F.k = b.dataset.k;
  else if (b.dataset.fl) F.fl = F.fl.includes(b.dataset.fl) ? F.fl.filter(x => x !== b.dataset.fl) : [...F.fl, b.dataset.fl];
  try { localStorage.setItem('events:filter', JSON.stringify(F)); } catch {}
  run(async () => { await fill(); draw(); });
});
addEventListener('herd-ev', () => run(since));
addEventListener('herd-refresh', () => run(load));
setInterval(draw, 60_000); // the ages and time groups move on

// #267: the Host tab, read with the rest (`herd-refresh`) and drawn again from each `host` on `/live` (`herd-host`).
const host = (d: HostTab) => frag('host', hostTab(d));
const hostLoad = () => api.host().then(host).catch(() => {});
addEventListener('herd-host', e => host((e as CustomEvent<HostTab>).detail));
addEventListener('herd-refresh', hostLoad);
/** ui-audit.js waits for the first draw. */
(window as unknown as { boardReady: Promise<unknown> }).boardReady = Promise.all([run(load), hostLoad()]);
