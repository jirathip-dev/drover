// The drover-asks tab, a test per herdsman#553 row (63-71): the real tab module (`lib/asks.ts`) and client
// (`lib/api.ts`) against a fake `cooee` that serves the recorded `/api/v1/needs` and `/api/v1/events` fixtures (cards
// added in the shapes `types.ts` checks), the backend's `data:` images, and records each form POST.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { asksTab, type Later, type Store } from "../src/lib/asks.ts";
import type { El, Kid } from "../src/lib/dom.ts";
import type { Blocked, Ev, Events, NeedAsk, NeedIssue, NeedsTab } from "../src/lib/types.ts";
import { must } from "./must.ts";

const fixture = (f: string) => JSON.parse(fs.readFileSync(new URL(`../fixtures/${f}.json`, import.meta.url), "utf8"));
const raw: NeedsTab = fixture("needs");
const rawEv: Events = fixture("events");
// #17: herdsman's newest closed ask of each ending it has logged, and each one's `needs-decision`.
const rawEnd: Events = fixture("endings");

type Post = { path: string; f: Record<string, string> };
const OK = '{"ok":true,"code":"OK","message":"done"}';
/** A fake herdsman behind `cooee.fetch`: `n` its needs, `ev` its events; `on` answers a POST (and may change `n`). */
function herdsman(
  n: NeedsTab,
  on: (p: Post) => Promise<string> | string = () => OK,
  ev: Ev[] = rawEv.list,
  imgs: Record<string, string> = {},
) {
  const posts: Post[] = [],
    gets: string[] = [],
    live: [string, (e: { event: string; data: string }) => unknown][] = [];
  (globalThis as { cooee?: unknown }).cooee = {
    events(path: string, on: (e: { event: string; data: string }) => unknown) {
      live.push([path, on]);
      return () => {};
    },
    async fetch(path: string, init?: { method?: string; type?: string; body?: string }) {
      const answer = (text: string, status = 200) => ({ status, ok: status === 200, type: "application/json", text });
      if (init?.method === "POST") {
        assert.equal(init.type, "application/x-www-form-urlencoded");
        const p = { path, f: Object.fromEntries(new URLSearchParams(init.body)) };
        posts.push(p);
        return answer(await on(p));
      }
      gets.push(path);
      if (path === "/api/v1/needs") return answer(JSON.stringify(n));
      // since=0: past 500, so none and `refresh`, and the newest seq.
      const since = /^\/api\/v1\/events\?since=(\d+)$/.exec(path);
      if (since) {
        const top = Math.max(0, ...ev.map((e) => e.seq));
        const list = Number(since[1]) === 0 ? [] : ev.filter((e) => e.seq > Number(since[1]));
        return answer(JSON.stringify({ seq: top, refresh: Number(since[1]) === 0, list }));
      }
      return path in imgs ? answer(imgs[path] ?? "") : answer("no such image", 404);
    },
  };
  // An SSE message on each stream drover opened, awaited.
  const emit = (event: string, data: string) => Promise.all(live.map(([, on]) => on({ event, data })));
  return { posts, gets, live, emit };
}

const text = (e: Kid): string => (typeof e === "string" ? e : e.k.map(text).join(""));
const mem = (): Store & { l: Later } => {
  const s = {
    l: {} as Later,
    get: () => structuredClone(s.l),
    set: (l: Later) => {
      s.l = l;
    },
  };
  return s;
};

/** The tab loaded from the fake herdsman, on a fake clock: its current tree, and finders over it. */
async function open(store: Store = mem(), clock = { t: raw.now }) {
  let kids: Kid[] = [];
  const t = asksTab(
    (k) => (kids = k),
    () => clock.t,
    store,
  );
  await t.load();
  await new Promise((r) => setTimeout(r)); // the images' reads
  const els = (k: Kid[] = kids): El[] => k.flatMap((x) => (typeof x === "string" ? [] : [x, ...els(x.k)]));
  const all = () => text({ t: "", a: {}, k: kids });
  const one = (s: string | RegExp) => {
    const f = els().filter((e) => e.a.on && (typeof s === "string" ? text(e).startsWith(s) : s.test(text(e))));
    assert.ok(f.length, `nothing tappable reads ${s}:\n${all()}`);
    return must(f.at(-1));
  };
  const tap = async (s: string | RegExp) => (one(s).a.on as (v: string) => unknown)("");
  const cards = () => els().filter((e) => e.a.class === "ask");
  const card = (i: number) => must(cards()[i], `card ${i}`);
  const badge = () =>
    text(
      must(
        els().find((e) => e.a.class?.toString().startsWith("badge")),
        "badge",
      ),
    );
  return { t, els, all, one, tap, cards, card, badge, clock };
}

const base = must(raw.asks[0]);
const askOf = (id: string, f: Partial<NeedAsk["ask"]> = {}, c: Partial<NeedAsk> = {}): NeedAsk => ({
  ...structuredClone(base),
  ...c,
  ask: { ...structuredClone(base.ask), id, ...f },
});
const menu = (f: Partial<NonNullable<Blocked["menu"]>> = {}): Blocked => ({
  name: "fleet-1-impl-7",
  who: "fleet-1 · #7 · fleet-1-impl-7",
  push: false,
  menu: {
    title: "Bash command",
    reason: "Run the tests",
    command: "cargo test --locked",
    countdown: 14,
    question: "Do you want to proceed?",
    options: ["Yes", "No"],
    h: "8812",
    ...f,
  },
});
const nh = (f: Partial<NeedIssue> = {}): NeedIssue => ({
  fleet: "fleet-2",
  issue: 9,
  pending: false,
  who: "fleet-2 · #9",
  title: "a title",
  why: "round limit reached",
  pass: null,
  stale: false,
  fail: "a finding",
  pr: { pr: 12, ci: "green", verdict: "FAIL", vtone: "r", rounds: 3 },
  repo: "example/repo",
  mergeable: "MERGEABLE",
  merge: true,
  shots: [],
  ...f,
});
const needs = (f: Partial<NeedsTab>): NeedsTab => ({ ...structuredClone(raw), ...f });

test("63: a card per open ask with its options and free text, a count badge; a tap answers, the card goes, the badge drops", async () => {
  const n = needs({ asks: [askOf("q1", { options: ["one", "two"], recommend: 2 }), askOf("q2")] });
  const h = herdsman(n, (p) => {
    n.asks = n.asks.filter((a) => a.ask.id !== p.f.id);
    return OK;
  });
  const v = await open();
  assert.equal(v.badge(), "2");
  assert.equal(v.cards().length, 2);
  const btns = v
    .els(v.card(0).k)
    .filter((e) => e.t === "button")
    .map(text);
  assert.deepEqual(
    btns.slice(0, 3),
    ["2. two (recommended)", "1. one", "Send"],
    "recommended first, then the rest and Send",
  );
  await v.tap("1. one");
  assert.deepEqual(h.posts[0], {
    path: "/api/v1/answer",
    f: { op: "answer", mtime: raw.mtime, fleet: base.ask.fleet, id: "q1", choice: "1" },
  });
  assert.equal(v.badge(), "1");
  assert.equal(v.cards().length, 1);
  assert.doesNotMatch(v.all(), /1\. one/);
  // Free text: typed, then Send.
  (must(v.els().find((e) => e.t === "input")).a.oninput as (s: string) => void)("ship it");
  await v.tap("Send");
  assert.deepEqual(h.posts[1]?.f, { op: "answer", mtime: raw.mtime, fleet: base.ask.fleet, id: "q2", text: "ship it" });
  assert.equal(v.badge(), "0");
  assert.match(v.all(), /nothing needs you/);
});

test("63: a refused answer shows herdsman's code and the card stays", async () => {
  herdsman(needs({}), () => '{"ok":false,"code":"UNPAIRED","message":"pair this device first"}');
  const v = await open();
  await v.tap(/^1\. /);
  assert.match(v.all(), /UNPAIRED: pair this device first/);
  assert.equal(v.cards().length, 1);
});

test("64: a routine ask reads its default and countdown, an owner ask that it waits", async () => {
  herdsman(
    needs({
      asks: [
        askOf("q1", {}, { kind: "routine · defaults to 2 in 24m" }),
        askOf("q2", {}, { kind: "owner · waits for you" }),
      ],
    }),
  );
  const v = await open();
  const r = v.card(0),
    o = v.card(1);
  assert.match(text(r), /routine · defaults to 2 in 24m/);
  assert.equal(v.els(r.k).find((e) => text(e) === "routine · defaults to 2 in 24m")?.a.class, "tag a");
  assert.equal(v.els(o.k).find((e) => text(e) === "owner · waits for you")?.a.class, "tag r");
});

test("65: a blocked lane's menu answered by tap, with the menu's hash (the stale-menu guard); sent, the card clears", async () => {
  const n = needs({ asks: [], blocked: [menu()] });
  const h = herdsman(n, (p) => {
    if (p.f.h !== "8812") return '{"ok":false,"code":"STALE","message":"menu changed — refreshed"}';
    n.blocked = [];
    return '{"ok":true,"code":"OK","message":"sent"}';
  });
  const v = await open();
  await v.tap("1. Yes");
  assert.deepEqual(h.posts[0], {
    path: "/api/v1/answer",
    f: { op: "answer", mtime: raw.mtime, agent: "fleet-1-impl-7", choice: "1", h: "8812" },
  });
  assert.match(v.all(), /sent/);
  assert.equal(v.cards().length, 0);
  // The menu changed under the card: herdsman refuses its hash, the card says so.
  n.blocked = [menu({ h: "old" })];
  await v.t.load();
  await v.tap("2. No");
  assert.match(v.all(), /STALE: menu changed — refreshed/);
});

test("66: a permission prompt's card: title, reason, command and the auto-deny countdown, ticking", async () => {
  herdsman(needs({ asks: [], blocked: [menu()] }));
  const v = await open();
  const c = text(v.card(0));
  for (const s of [
    "Bash command",
    "Run the tests",
    "cargo test --locked",
    "Do you want to proceed?",
    "auto-denies in 0:14",
  ])
    assert.ok(c.includes(s), `${s} in ${c}`);
  v.clock.t += 5;
  v.t.tick();
  assert.match(v.all(), /auto-denies in 0:09/);
  v.clock.t += 60;
  v.t.tick();
  assert.match(v.all(), /auto-denies in 0:00/);
});

test("66: unpaired, a blocked lane shows its activity and why it can't be answered; no menu, the pane's tail", async () => {
  herdsman(
    needs({
      asks: [],
      blocked: [
        { name: "a", who: "fleet-1 · #3 · a", push: true, activity: "running cargo test" },
        { name: "b", who: "b", push: false, tail: "last lines" },
      ],
    }),
  );
  const v = await open();
  assert.match(text(v.card(0)), /running cargo test.*HERDSMAN_COOKIE/);
  assert.match(text(v.card(1)), /last lines/);
  assert.equal(v.els().filter((e) => /^\d\. /.test(text(e))).length, 0);
});

test("67: a needs-human card: why, verdict, PR; one more round shows its pending line at once; merge anyway confirms; park", async () => {
  const n = needs({ asks: [], paired: true, issues: [nh()] });
  let release = () => {};
  const h = herdsman(n, (p) => (p.f.op === "extend" ? new Promise((r) => (release = () => r(OK))) : OK));
  const v = await open();
  assert.match(text(v.card(0)), /#9 a title · round limit reached/);
  assert.match(text(v.card(0)), /FAIL: a finding · PR #12 · CI green/);
  const pr = must(v.els().find((e) => text(e) === "PR #12"));
  assert.equal(pr.a.href, "https://github.com/example/repo/pull/12");
  const done = v.tap("one more round");
  assert.match(v.all(), /⟳ one more round · starting a lane…/, "before herdsman answers");
  assert.doesNotMatch(v.all(), /merge anyway/);
  release();
  await done;
  assert.deepEqual(h.posts[0]?.f, { op: "extend", mtime: raw.mtime, fleet: "fleet-2", issue: "9" });
  assert.match(v.all(), /⟳ one more round/, "herdsman runs extend as a job: its card may not say pending yet");
  must(n.issues[0]).pending = true;
  await v.t.load();
  assert.match(v.all(), /⟳ one more round/, "then herdsman's own pending line");

  n.issues = [nh({ issue: 10, pass: "0123456789abcdef", stale: true })];
  await v.t.load();
  assert.match(v.all(), /PASS at 0123456 \(not the head\) · PR #12 · CI green · MERGEABLE/);
  await v.tap("merge anyway");
  assert.equal(h.posts.length, 1, "the first tap only asks");
  await v.tap("yes, merge #10 past the round limit");
  await v.tap("park");
  assert.deepEqual(
    h.posts.slice(1).map((p) => [p.f.op, p.f.issue]),
    [
      ["override", "10"],
      ["park", "10"],
    ],
  );
});

test("67: unpaired or merge not offered: the buttons that can't work don't show", async () => {
  herdsman(needs({ asks: [], paired: false, issues: [nh()] }));
  let v = await open();
  assert.doesNotMatch(v.all(), /one more round|park/);
  herdsman(needs({ asks: [], paired: true, issues: [nh({ merge: false })] }));
  v = await open();
  assert.doesNotMatch(v.all(), /merge anyway/);
  assert.match(v.all(), /one more round/);
});

test("68: [later] hides a card on this phone, [later all] leaves one red line; a changed card or 12 h shows it again", async () => {
  const n = needs({ blocked: [menu()], issues: [nh()], paired: true });
  herdsman(n);
  const store = mem(),
    clock = { t: raw.now };
  let v = await open(store, clock);
  assert.equal(v.cards().length, 3);
  await v.tap(/^later$/);
  assert.equal(v.cards().length, 2);
  assert.match(v.all(), /1 need you ▸/);
  await v.tap("later all");
  assert.equal(v.cards().length, 0);
  const red = v.els().filter((e) => e.a.class === "r later");
  assert.equal(red.length, 1);
  assert.match(text(must(red[0])), /^3 need you ▸/);
  assert.equal(v.badge(), "3", "the badge still counts them");
  await v.tap("3 need you");
  assert.equal(v.cards().length, 3, "a tap on the line shows them in place");
  // Another load (a reopened tab) on the same phone keeps them hidden.
  v = await open(store, clock);
  assert.equal(v.cards().length, 0);
  must(n.issues[0]).why = "a new finding";
  v = await open(store, clock);
  assert.equal(v.cards().length, 1, "the changed card shows again");
  clock.t += 12 * 3600 + 1;
  v = await open(store, clock);
  assert.equal(v.cards().length, 3);
});

test("69: a design review: its PNGs inline, approve / revise / reject answer the ask", async () => {
  const d = askOf(
    "q7",
    { fleet: "fleet-2", issue: 7, options: ["approve", "revise", "reject"], recommend: null },
    { pngs: ["home-1440.png", "home-390.png"] },
  );
  const h = herdsman(needs({ asks: [d] }), undefined, rawEv.list, {
    "/design/fleet-2/7/0": "data:image/png;base64,AAAA",
    "/design/fleet-2/7/1": "data:image/png;base64,BBBB",
  });
  const v = await open();
  const imgs = v
    .els()
    .filter((e) => e.t === "img")
    .map((e) => [e.a.src, e.a.alt]);
  assert.deepEqual(imgs, [
    ["data:image/png;base64,AAAA", "home-1440.png"],
    ["data:image/png;base64,BBBB", "home-390.png"],
  ]);
  assert.equal(h.gets.filter((g) => g.startsWith("/design/")).length, 2);
  await v.tap("1. approve");
  assert.deepEqual(h.posts[0]?.f, { op: "answer", mtime: raw.mtime, fleet: "fleet-2", id: "q7", choice: "1" });
  assert.equal(h.gets.filter((g) => g.startsWith("/design/")).length, 2, "each image read once");
});

test("70: a needs-human card shows the verdict's screenshots, only the ones the backend serves as images", async () => {
  herdsman(needs({ asks: [], issues: [nh({ shots: ["a-1440.png", "b-390.png"] })] }), undefined, rawEv.list, {
    "/shot/fleet-2/9/0": "data:image/png;base64,CCCC",
    "/shot/fleet-2/9/1": "<html>not an image",
  });
  const v = await open();
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.t === "img")
      .map((e) => e.a.src),
    ["data:image/png;base64,CCCC"],
  );
  assert.match(v.all(), /b-390\.png/, "the missing one keeps its caption");
});

test("71: an answer herdsman couldn't deliver shows as a red undelivered line; a later delivery or a day clears it", async () => {
  const top = Math.max(...rawEv.list.map((e) => e.seq));
  const e = (seq: number, type: string, id: string, ts = raw.now - 60): Ev => ({
    ...structuredClone(must(rawEv.list[0])),
    seq,
    ts,
    type,
    data: { id, agent: "a", reason: "lane gone" },
    plain: `fleet-1 #4 ${type}: ${id}`,
  });
  herdsman(needs({}), undefined, [
    ...rawEv.list,
    e(top + 1, "answer-undelivered", "q8"),
    e(top + 2, "answer-delivered", "q8"),
    e(top + 3, "answer-undelivered", "q9"),
    e(top + 4, "answer-undelivered", "q5", raw.now - 90000),
  ]);
  const v = await open();
  const lines = v.els().filter((x) => x.a.class === "r" && text(x).startsWith("undelivered"));
  assert.deepEqual(lines.map(text), ["undelivered: fleet-1 #4 answer-undelivered: q9"]);
});

// #17 herdsman#584: the recorded endings after the recorded events, in their order (the tab reads the newest 500).
const top = Math.max(...rawEv.list.map((e) => e.seq));
const later = (list: Ev[]) => list.map((e, i) => ({ ...structuredClone(e), seq: top + 1 + i }));
const of = (type: string, f: (e: Ev) => boolean = () => true) =>
  must(
    rawEnd.list.find((e) => e.type === type && f(e)),
    type,
  );
// herdsman's log has logged no typed answer or withdrawal yet: the recorded answer, as `answer --typed` and
// `ask withdraw --for` log them (herdsman#584's `by: owner (typed)`, `for`).
const answered = of("decision", (e) => !e.data.to);
const typed = { ...structuredClone(answered), data: { ...answered.data, id: "q900", by: "owner (typed)" } };
const withdrawn = {
  ...structuredClone(answered),
  type: "ask-withdrawn",
  data: { id: "q901", by: "conductor-t42", for: "q902" },
};
const asked = (id: string): Ev => {
  const e = structuredClone(of("needs-decision"));
  return { ...e, fleet: answered.fleet, data: { ...e.data, id } };
};

test("#17: each ended ask is no needs-you card and no choose dot; the tab shows its ending", async () => {
  const ends = [...rawEnd.list, asked("q900"), typed, asked("q901"), withdrawn];
  herdsman(needs({ asks: [askOf("q1")] }), undefined, [...rawEv.list, ...later(ends)]);
  const v = await open();
  assert.equal(v.cards().length, 1);
  assert.equal(v.badge(), "1");
  assert.match(text(v.card(0)), /q1|question text/);
  assert.doesNotMatch(v.all(), /choose/);
  const lines = v
    .els()
    .filter((x) => x.a.class === "m" && text(x).startsWith("ended: "))
    .map(text);
  assert.deepEqual(lines, [
    "ended: fleet-2 q901 withdrawn → q902 · question text",
    "ended: fleet-2 q900 answered by owner (typed) · question text",
    "ended: fleet-2 q825 answered by owner · question text",
    "ended: fleet-2 q807 failed · question text",
    "ended: fleet-2 q803 delivered to t27 · question text",
  ]);
  for (const e of ends) assert.ok(!v.cards().some((c) => text(c).includes(`${e.data.id}`)), `${e.data.id} has a card`);
});

test("#17: the recorded expired and delivered asks read their ending", async () => {
  herdsman(needs({ asks: [] }), undefined, [...rawEv.list, ...later(rawEnd.list.slice(0, 4))]);
  const v = await open();
  assert.equal(v.cards().length, 0);
  assert.match(v.all(), /ended: fleet-2 q651 expired · question text/);
  assert.match(v.all(), /ended: fleet-1 q605 delivered to t17 · question text/);
});

test("#17: an open ask from a live lane older than 24 h is still a card", async () => {
  const old = askOf("q9", { ts: raw.now - 2 * 86400 }, { who: "fleet-1 · #3075 · fleet-1-impl-3075 · 2d" });
  herdsman(needs({ asks: [old] }));
  const v = await open(mem(), { t: raw.now });
  assert.equal(v.cards().length, 1);
  assert.match(text(v.card(0)), /fleet-1-impl-3075 · 2d/);
});

test("#17: an ask-expired event on herdsman's /live takes the card away without a reload", async () => {
  const expired = of("ask-expired");
  const n = needs({ asks: [askOf(String(expired.data.id), {}, { who: "fleet-2 · t22 · 1d" })] });
  const list = [...rawEv.list, ...later([of("needs-decision", (e) => e.data.id === expired.data.id)])];
  const h = herdsman(n, undefined, list);
  const v = await open();
  v.t.listen();
  assert.deepEqual(
    h.live.map(([p]) => p),
    ["/live"],
  );
  assert.equal(v.cards().length, 1);
  // herdsman's check logs the expiry: `needs` drops the ask, `/live` sends the event's moment.
  n.asks = [];
  const e = { ...structuredClone(expired), seq: top + 2 };
  list.push(e);
  await h.emit("ev", JSON.stringify({ seq: e.seq, ts: e.ts, k: null, f: e.fleet, n: null }));
  assert.equal(v.cards().length, 0);
  assert.equal(v.badge(), "0");
  assert.match(v.all(), /ended: fleet-2 q651 expired/);
});
