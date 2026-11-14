// #969: serve's sw.js under node, at DROVER_BASE: an in-memory `caches`, the real `fetch`. Each stdin line is a URL
// fetched through the worker; its answer's body goes out as one hex line.
import readline from 'node:readline';
const base = process.env.DROVER_BASE, store = new Map(), pending = [];
let onfetch;
const self = {
  registration: { scope: base }, skipWaiting() {}, clients: { claim: async () => {} },
  addEventListener: (t, f) => { if (t === 'fetch') onfetch = f; },
};
const key = r => (typeof r === 'string' ? new URL(r, base).href : r.url);
const cache = { addAll: async () => {}, put: (r, res) => { const p = res.arrayBuffer().then(b => store.set(key(r), b)); pending.push(p); return p; } };
const open = () => { const p = Promise.resolve(cache); pending.push(p); return p; };
const caches = { open, keys: async () => [], delete: async () => true, match: async r => (store.has(key(r)) ? new Response(store.get(key(r))) : undefined) };
const src = await (await fetch(base + 'sw.js')).text();
new Function('self', 'location', 'caches', src)(self, new URL(base), caches);
for await (const line of readline.createInterface({ input: process.stdin })) {
  const req = new Request(new URL(line, base));
  let res;
  onfetch({ request: req, respondWith: p => { res = p; }, waitUntil: p => pending.push(p) });
  const body = Buffer.from(await (await (res ?? fetch(req))).arrayBuffer());
  while (pending.length) await pending.shift();
  console.log(body.toString('hex'));
}
