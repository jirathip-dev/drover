// #927: a screen is JSON blocks (crates/server/src/screens.rs, the same cooee draws); its stream patches it in place
// with ops: `screen` (all of it), `badge`, `merge` (fields into a block), `replace`, `remove`, `insert`.
export type Blk = { [k: string]: unknown };
export const isList = (v: unknown): v is unknown[] => Array.isArray(v);
export const isBlk = (v: unknown): v is Blk => typeof v === 'object' && v !== null;
export const obj = (v: unknown): Blk => (isBlk(v) ? v : {});
export const blks = (v: unknown): Blk[] => (isList(v) ? v.filter(isBlk) : []);

export function find(v: unknown, id: unknown): Blk | null {
  if (isList(v)) {
    for (const x of v) {
      const f = find(x, id);
      if (f) return f;
    }
    return null;
  }
  if (!isBlk(v)) return null;
  if (v.id === id) return v;
  for (const k of Object.keys(v)) {
    const f = find(v[k], id);
    if (f) return f;
  }
  return null;
}

function drop(v: unknown, id: unknown) {
  if (isList(v)) {
    for (let i = v.length - 1; i >= 0; i--) {
      const x = v[i];
      if (isBlk(x) && x.id === id) v.splice(i, 1);
    }
    v.forEach(x => drop(x, id));
  } else if (isBlk(v)) Object.values(v).forEach(x => drop(x, id));
}

/** The screen after `ops`; a whole `screen` op replaces it, the others change it in place. */
export function apply(screen: Blk, ops: Blk[]): Blk {
  for (const op of ops) {
    const id = op.id ?? '';
    if (op.op === 'screen' && isBlk(op.screen)) screen = op.screen;
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
      const list = into.rows ?? (into.blocks ??= []);
      if (!isList(list)) continue;
      const at = op.before ? list.findIndex(x => isBlk(x) && x.id === op.before) : -1;
      list.splice(at < 0 ? list.length : at, 0, op.block);
    }
  }
  return screen;
}
