// #265: the TypeScript board's start, a typed client for the JSON API v1 (`/api/v1/`, docs/serve.md). The tabs move
// here one PR each (Events, Host, Queue, Lanes); until then the HTML board is the page and dist/ is only served.
import type { Events, Reply, State, Transcript } from './api';

const base = new URL('api/v1/', document.baseURI);

async function get<T>(path: string): Promise<T> {
  const r = await fetch(new URL(path, base));
  if (!r.ok) throw new Error(`${path}: ${r.status} ${((await r.json()) as Reply).message}`);
  return (await r.json()) as T;
}

export const api = {
  state: () => get<State>('state'),
  /** #259: the events after `since`; `refresh` means read `state` again. */
  events: (since: number) => get<Events>(`events?since=${since}`),
  lane: (agent: string, before = 0) => get<Transcript>(`lane/${encodeURIComponent(agent)}?before=${before}`),
  /** A form op (`answer`, `drive`, `pause`, `extend`, …) with the HTML form's fields; `mtime` is `State.mtime`. */
  async post(op: string, fields: Record<string, string>): Promise<Reply> {
    const r = await fetch(new URL(op, base), { method: 'POST', body: new URLSearchParams(fields) });
    return (await r.json()) as Reply;
  },
};
