export type Kind = 'tap' | 'sheet' | 'tab' | 'send' | 'answer' | 'delta';
export type Span = { k: Kind; t0: number; net: number; parse: number };
type Sample = [Kind, number, number, number, number];

const out: Sample[] = [];
const now = () => performance.now();

export const span = (k: Kind, at?: number): Span => ({ k, t0: at && at > 0 && at <= now() ? at : now(), net: 0, parse: 0 });

export async function read(r: Response, s: Span | undefined, t0: number): Promise<unknown> {
  const txt = await r.text(), t = now();
  try {
    return JSON.parse(txt);
  } finally {
    if (s) [s.net, s.parse] = [s.net + t - t0, s.parse + now() - t];
  }
}

export function painted(s: Span | undefined) {
  if (!s || document.hidden) return;
  requestAnimationFrame(() => setTimeout(() => {
    const ms = now() - s.t0, r = Math.round;
    if (out.length < 500 && !document.hidden) out.push([s.k, r(ms), r(s.net), r(s.parse), r(Math.max(0, ms - s.net - s.parse))]);
  }));
}

let last = -Infinity;
export function flush(post: (body: string) => void, leaving = false) {
  if (!navigator.onLine) out.length = 0;
  else if (out.length && (leaving || now() - last >= 30_000)) {
    last = now();
    post(JSON.stringify(out.splice(0)));
  }
}
