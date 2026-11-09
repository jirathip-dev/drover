// Reliability (cooee-drover#7, herdsman#553's "Reliability" rows R5, R6, R10, R15, R16, R17, R21): the real tab
// modules (`lib/asks.ts`, `lib/queue.ts`, `lib/fleets.ts`) and client (`lib/api.ts`) against one fake `cooee` that
// serves the recorded `/api/v1` fixtures, holds a read when a test says so, and records each form POST.
import assert from "node:assert/strict";
import fs from "node:fs";
import { type TestContext, test } from "node:test";
import { asksTab, type Later, type Store } from "../src/lib/asks.ts";
import type { El, Kid } from "../src/lib/dom.ts";
import { fleetsTab } from "../src/lib/fleets.ts";
import { queueTab } from "../src/lib/queue.ts";
import type { Ev, NeedAsk, NeedsTab, QueueTab } from "../src/lib/types.ts";
import { must } from "./must.ts";

const fx = (n: string) => JSON.parse(fs.readFileSync(new URL(`../fixtures/${n}.json`, import.meta.url), "utf8"));
const OK = '{"ok":true,"code":"OK","message":"done"}';

type Post = { path: string; f: Record<string, string> };
/** A fake herdsman behind `cooee.fetch`: `/api/v1/<name>` answers `api[name]` (events: all of `ev`, `seq` the newest);
 * a name in `hold` waits for `release()`; images answer `imgs[path]`. */
function herdsman(api: Record<string, unknown>, ev: Ev[] = fx("events").list, imgs: Record<string, string> = {}) {
  const posts: Post[] = [],
    gets: string[] = [],
    hold = new Set<string>();
  let waiting: (() => void)[] = [];
  (globalThis as { cooee?: unknown }).cooee = {
    async fetch(path: string, init?: { method?: string; body?: string }) {
      const answer = (text: string, status = 200) => ({ status, ok: status === 200, type: "application/json", text });
      if (init?.method === "POST") {
        posts.push({ path, f: Object.fromEntries(new URLSearchParams(init.body)) });
        return answer(OK);
      }
      gets.push(path);
      const name = /^\/api\/v1\/(\w+)/.exec(path)?.[1];
      if (!name) return path in imgs ? answer(imgs[path] ?? "") : answer("no such image", 404);
      if (hold.has(name)) await new Promise<void>((r) => waiting.push(r));
      if (name === "events")
        return answer(JSON.stringify({ seq: Math.max(0, ...ev.map((e) => e.seq)), refresh: false, list: ev }));
      return answer(JSON.stringify(api[name]));
    },
  };
  const release = () => {
    hold.clear();
    for (const r of waiting) r();
    waiting = [];
  };
  return { posts, gets, hold, release };
}
const api = () => ({
  needs: fx("needs") as NeedsTab,
  queue: fx("queue") as QueueTab,
  host: fx("host"),
  state: fx("state"),
  lessons: fx("lessons"),
});

const text = (e: Kid): string => (typeof e === "string" ? e : e.k.map(text).join(""));
/** A tab's tree as `draw` last gave it: finders and a tap. */
function view() {
  let kids: Kid[] = [];
  const els = (k: Kid[] = kids): El[] => k.flatMap((x) => (typeof x === "string" ? [] : [x, ...els(x.k)]));
  const all = () => text({ t: "", a: {}, k: kids });
  const tappable = (s: RegExp) => els().filter((e) => e.a.on && s.test(text(e)));
  const tap = (s: RegExp) => {
    const f = tappable(s);
    assert.ok(f.length, `nothing tappable reads ${s}:\n${all()}`);
    return (must(f.at(-1)).a.on as (v: string) => unknown)("");
  };
  return {
    draw: (k: Kid[]) => {
      kids = k;
    },
    els,
    all,
    tappable,
    tap,
    cards: () => els().filter((e) => e.a.class === "ask"),
  };
}
const mem = (l: Later = {}): Store => ({
  get: () => structuredClone(l),
  set: (x) => {
    l = x;
  },
});
const flush = () => new Promise<void>((r) => setImmediate(r));
const strip = (v: ReturnType<typeof view>) => v.els().filter((e) => e.a.class === "m strip").length;
const raw: NeedsTab = fx("needs");
const askOf = (id: string, f: Partial<NeedAsk["ask"]> = {}, c: Partial<NeedAsk> = {}): NeedAsk => ({
  ...structuredClone(must(raw.asks[0])),
  ...c,
  ask: { ...structuredClone(must(raw.asks[0]).ask), id, ...f },
});
const ev = (seq: number, type: string, id: string, ts = raw.now - 60): Ev => ({
  ...structuredClone(fx("events").list[0]),
  seq,
  ts,
  type,
  data: { id },
  plain: `fleet-1 #4 ${type}: ${id}`,
});

/** R5: a reload that takes `hold` past 300 ms shows a strip over the page, not in its place; the page stays tappable. */
async function slowReload(
  t: TestContext,
  hold: string,
  open: (draw: (k: Kid[]) => void) => { load: () => Promise<void> },
  taps: RegExp[],
) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const a = api(),
    h = herdsman(a),
    v = view(),
    tab = open(v.draw);
  await tab.load();
  const page = v.all();
  h.hold.add(hold);
  const again = tab.load();
  await flush();
  t.mock.timers.tick(299);
  assert.equal(strip(v), 0, "no strip before 300 ms");
  assert.equal(v.all(), page);
  t.mock.timers.tick(1);
  assert.equal(strip(v), 1, "a strip once the read runs past 300 ms");
  assert.ok(v.all().endsWith(page), "the page stays under the strip");
  const n = h.posts.length;
  for (const tap of taps) {
    v.tap(tap);
    await flush();
  }
  assert.equal(h.posts.length, n + 1, "a tap on the page still posts");
  h.release();
  await again;
  await flush();
  assert.equal(strip(v), 0, "the strip goes once the read lands");
}

test("R5: asks: a slow read is a strip after 300 ms, the cards stay tappable", (t) =>
  slowReload(t, "needs", (d) => asksTab(d, () => raw.now, mem()), [/^1\. /]));
test("R5: queue: a slow read is a strip after 300 ms, the rows stay tappable", (t) =>
  slowReload(t, "queue", (d) => queueTab(d), [/^(Pause|Resume)$/]));
test("R5: fleets: a slow read is a strip after 300 ms, the page stays tappable", (t) =>
  slowReload(t, "host", (d) => fleetsTab(d), [/^(off|on)$/, /^yes$/]));
test("R5: the first read, when slow, shows the strip after 300 ms and nothing before", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const h = herdsman(api()),
    v = view();
  h.hold.add("needs");
  const first = asksTab(v.draw, () => raw.now, mem()).load();
  await flush();
  t.mock.timers.tick(299);
  assert.equal(v.all(), "");
  t.mock.timers.tick(1);
  assert.equal(strip(v), 1);
  h.release();
  await first;
  assert.equal(strip(v), 0);
  assert.ok(v.cards().length > 0);
});

test("R6: 30k events and 1,000 asks (all hidden by [later]) load and draw in under 300 ms", async () => {
  const a = api(),
    store: Later = {};
  a.needs.asks = Array.from({ length: 1000 }, (_, i) => askOf(`q${i}`));
  const evs = Array.from({ length: 30000 }, (_, i) =>
    ev(i + 1, i % 2 ? "answer-undelivered" : "answer-delivered", `q${i % 1000}`),
  );
  herdsman(a, evs);
  const v = view(),
    tab = asksTab(v.draw, () => raw.now, mem(store));
  await tab.load();
  await v.tap(/^later all$/);
  let t0 = performance.now();
  await tab.load();
  const asks = performance.now() - t0;
  assert.equal(v.cards().length, 0);
  assert.match(v.all(), /^needs you 1000/);
  assert.ok(asks < 300, `asks: ${asks.toFixed(0)} ms`);

  const b = api();
  herdsman(
    b,
    Array.from({ length: 30000 }, (_, i) => ({
      ...structuredClone(fx("events").list[0]),
      seq: i + 1,
      ts: b.state.now - i,
      text: `e${i}`,
    })),
  );
  const w = view();
  t0 = performance.now();
  await fleetsTab(w.draw).load();
  const fleets = performance.now() - t0;
  assert.match(w.all(), /e0 /);
  assert.ok(fleets < 300, `fleets: ${fleets.toFixed(0)} ms`);
});

test("R10: an ask made before herdsman's start event is listed and answerable", async () => {
  const a = api();
  a.needs.asks = [askOf("q-early", { ts: raw.now - 3600, options: ["yes", "no"] })];
  const h = herdsman(a, [{ ...ev(2, "start", ""), data: {} }]);
  const v = view();
  await asksTab(v.draw, () => raw.now, mem()).load();
  assert.equal(v.cards().length, 1);
  await v.tap(/^1\. yes/);
  assert.deepEqual(
    h.posts.map((p) => [p.f.op, p.f.id, p.f.choice]),
    [["answer", "q-early", "1"]],
  );
});

test("R15: a lane idle without progress, once herdsman lists it, is a needs-you card on the next 15 s read", async () => {
  const a = api();
  a.needs.asks = [];
  herdsman(a);
  const v = view(),
    tab = asksTab(v.draw, () => raw.now, mem());
  await tab.load();
  assert.match(v.all(), /nothing needs you/);
  // 29 minutes on, herdsman's stall check lists the idle lane (no menu: its pane's last lines).
  a.needs.blocked = [
    { name: "fleet-1-impl-4", who: "fleet-1 · #4 · fleet-1-impl-4 · idle 29m", push: false, tail: "waiting for input" },
  ];
  await tab.load();
  assert.equal(v.cards().length, 1);
  assert.match(v.all(), /^needs you 1/);
  assert.match(text(must(v.cards()[0])), /idle 29m.*waiting for input/);
});

test("R16: a list repeating one id, read 5 times, is one row (asks, undelivered, queue rows, activity)", async () => {
  const a = api(),
    q = must(a.queue.fleets[0]),
    r = must(q.rows[0]);
  a.needs.asks = [askOf("q1"), askOf("q1")];
  q.rows = [r, structuredClone(r)];
  const e = ev(9, "answer-undelivered", "q1");
  const evs = [e, e, { ...structuredClone(fx("events").list[0]), seq: 10, ts: a.state.now - 5, text: "one event" }];
  herdsman(a, [...evs, evs[2]]);
  const v = view(),
    asks = asksTab(v.draw, () => raw.now, mem());
  const w = view(),
    queue = queueTab(w.draw);
  const x = view(),
    fleets = fleetsTab(x.draw);
  for (let i = 0; i < 5; i++) await Promise.all([asks.load(), queue.load(), fleets.load()]);
  assert.equal(v.cards().length, 1);
  assert.equal(v.els().filter((e) => e.a.class === "r" && text(e).startsWith("undelivered")).length, 1);
  w.tap(new RegExp(`^${q.name} `));
  assert.equal(w.tappable(new RegExp(`^[^#]*#${r.issue} `)).length, 1, w.all());
  assert.equal(x.els().filter((e) => e.a.class?.toString().startsWith("er") && /one event/.test(text(e))).length, 1);
  assert.doesNotMatch(x.all(), /one event ×2|×2 · one event/);
});

test("R17: text naming a file (`/tmp/a.png`, `../x.png`) is never an image or a read", async () => {
  const a = api(),
    bad = "see /tmp/a.png and ../x.png";
  a.needs.asks = [askOf("q1", { question: bad }, { pngs: ["../x.png", "/tmp/a.png"] })];
  a.needs.blocked = [{ name: "l", who: "l", push: false, tail: bad }];
  const h = herdsman(a, [ev(9, "answer-undelivered", bad)], {
    [`/design/${must(a.needs.asks[0]).ask.fleet}/${must(a.needs.asks[0]).ask.issue}/0`]: "data:image/png;base64,AAAA",
  });
  const v = view();
  await asksTab(v.draw, () => raw.now, mem()).load();
  await flush();
  assert.match(v.all(), /see \/tmp\/a\.png and \.\.\/x\.png/);
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.t === "img")
      .map((e) => e.a.src),
    ["data:image/png;base64,AAAA"],
    "the design's own PNG only",
  );
  const reads = h.gets.filter((g) => !g.startsWith("/api/v1/"));
  assert.ok(reads.length > 0 && reads.every((g) => /^\/design\/[\w-]+\/\d+\/\d+$/.test(g)), reads.join(" "));
});

test("R21: an answer to a gone lane is a red undelivered line after the next read, not silence", async () => {
  const a = api(),
    evs: Ev[] = [];
  a.needs.asks = [askOf("q4", { options: ["go"] })];
  const h = herdsman(a, evs);
  const v = view(),
    tab = asksTab(v.draw, () => raw.now, mem());
  await tab.load();
  await v.tap(/^1\. go/);
  assert.equal(h.posts.length, 1);
  evs.push(ev(5, "answer-undelivered", "q4"));
  await tab.load();
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.a.class === "r" && text(e).startsWith("undelivered"))
      .map(text),
    ["undelivered: fleet-1 #4 answer-undelivered: q4"],
  );
});
