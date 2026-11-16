export type Blk = { [k: string]: any };
export type Op = { op: string; id?: string; screen?: Blk; count?: number; fields?: Blk; block?: Blk; into?: string; before?: string };

export function find(v: unknown, id: string): Blk | null {
  if (Array.isArray(v)) {
    for (const x of v) {
      const f = find(x, id);
      if (f) return f;
    }
    return null;
  }
  if (!v || typeof v !== 'object') return null;
  const o = v as Blk;
  if (o.id === id) return o;
  for (const k of Object.keys(o)) {
    const f = find(o[k], id);
    if (f) return f;
  }
  return null;
}

function drop(v: unknown, id: string) {
  if (Array.isArray(v)) {
    for (let i = v.length - 1; i >= 0; i--) if (v[i] && v[i].id === id) v.splice(i, 1);
    v.forEach(x => drop(x, id));
  } else if (v && typeof v === 'object') Object.values(v as Blk).forEach(x => drop(x, id));
}

/** The screen after `ops`; a whole `screen` op replaces it, the others change it in place. */
export function apply(screen: Blk, ops: Op[]): Blk {
  for (const op of ops) {
    const id = op.id ?? '';
    if (op.op === 'screen' && op.screen) screen = op.screen;
    else if (op.op === 'badge') screen.badge = op.count;
    else if (op.op === 'merge') Object.assign(find(screen, id) ?? {}, op.fields);
    else if (op.op === 'replace') {
      const b = find(screen, id);
      if (b && op.block) {
        Object.keys(b).forEach(k => delete b[k]);
        Object.assign(b, op.block);
      }
    } else if (op.op === 'remove') drop(screen, id);
    else if (op.op === 'insert' && op.block) {
      const into = find(screen, op.into ?? '');
      if (!into) continue;
      const list: Blk[] = into.rows ?? (into.blocks ??= []);
      const at = op.before ? list.findIndex(x => x.id === op.before) : -1;
      list.splice(at < 0 ? list.length : at, 0, op.block);
    }
  }
  return screen;
}
