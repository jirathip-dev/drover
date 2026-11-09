// The drover-queue tab, a test per herdsman#553 row (36-48): the real tab module (`lib/queue.ts`) and client
// (`lib/api.ts`) against a fake `cooee` that serves the recorded `/api/v1/queue` fixture and records each form POST;
// herdsman#14's verify rows, marks and issue sheet on the recorded `queue` and `issue` fixtures.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import type { El, Kid } from "../src/lib/dom.ts";
import { HINT, queueTab } from "../src/lib/queue.ts";
import type { IssueView, QueueRow, QueueTab } from "../src/lib/types.ts";
import { must } from "./must.ts";

const raw: QueueTab = JSON.parse(fs.readFileSync(new URL("../fixtures/queue.json", import.meta.url), "utf8"));
const rawIssue: IssueView = JSON.parse(fs.readFileSync(new URL("../fixtures/issue.json", import.meta.url), "utf8"));

type Post = { path: string; f: Record<string, string> };
/**
 * A fake herdsman behind `cooee.fetch`: `q` is its queue; `on` answers a POST (and may change `q`) with a status;
 * `served` answers other GETs by path (an `issue/…` not in it has no verdicts; any other path is a 404).
 */
function herdsman(
  q: QueueTab,
  on: (p: Post) => Promise<string> | string = () => '{"ok":true,"code":"OK","message":"done"}',
  served: Record<string, string> = {},
) {
  const posts: Post[] = [],
    other: string[] = [];
  let gets = 0;
  (globalThis as { cooee?: unknown }).cooee = {
    async fetch(path: string, init?: { method?: string; type?: string; body?: string }) {
      if (init?.method === "POST") {
        assert.equal(init.type, "application/x-www-form-urlencoded");
        const p = { path, f: Object.fromEntries(new URLSearchParams(init.body)) };
        posts.push(p);
        const text = await on(p),
          ok = JSON.parse(text).code !== "UNPAIRED";
        return { status: ok ? 200 : 401, ok, type: "application/json", text };
      }
      if (path !== "/api/v1/queue") {
        other.push(path);
        const m = /^\/api\/v1\/issue\/([^/]+)\/(\d+)$/.exec(path),
          text = served[path] ?? (m && JSON.stringify({ fleet: m[1], issue: Number(m[2]), verdicts: [] }));
        return text == null
          ? { status: 404, ok: false, type: "text/plain", text: "no such path" }
          : { status: 200, ok: true, type: "text/plain", text };
      }
      gets++;
      return { status: 200, ok: true, type: "application/json", text: JSON.stringify(q) };
    },
  };
  return { posts, gets: () => gets, other };
}

/** The tab loaded from the fake herdsman: its current tree, and finders over it. */
async function open() {
  let kids: Kid[] = [];
  const t = queueTab((k) => (kids = k));
  await t.load();
  const els = (k: Kid[] = kids): El[] => k.flatMap((x) => (typeof x === "string" ? [] : [x, ...els(x.k)]));
  const find = (s: string | RegExp, k?: Kid[]) =>
    els(k).filter((e) => e.a.on && (typeof s === "string" ? text(e).startsWith(s) : s.test(text(e))));
  const one = (s: string | RegExp, k?: Kid[]) => {
    const f = find(s, k);
    assert.ok(f.length, `nothing tappable reads ${s}:\n${text({ t: "", a: {}, k: kids })}`);
    return must(f.at(-1));
  };
  const tap = async (s: string | RegExp | El) =>
    ((typeof s === "object" && "t" in s ? s : one(s)).a.on as (v: string) => unknown)("");
  return { t, kids: () => kids, els, find, tap, all: () => text({ t: "", a: {}, k: kids }) };
}
const text = (e: Kid): string => (typeof e === "string" ? e : e.k.map(text).join(""));
/** A row's line, by its issue. */
const at = (n: number) => new RegExp(`^[^#]*#${n} `);
const copy = (): QueueTab => structuredClone(raw);
const row = (q: QueueTab, fleet: number, issue: number) =>
  must(
    must(q.fleets[fleet]).rows.find((r) => r.issue === issue),
    `#${issue}`,
  );
const queued = (base: QueueRow, issue: number, f: Partial<QueueRow> = {}): QueueRow => ({
  ...structuredClone(base),
  issue,
  ...f,
});

test("36: one repo at a time: chips, swipe to the next repo, a needs dot, [all] the needs-you rows", async () => {
  const q = copy();
  must(q.fleets[1]).need = 1;
  Object.assign(row(q, 2, 4), { state: "⏸ waiting: decision", tone: "r" });
  herdsman(q);
  const v = await open();
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.a.class === "chip")
      .map(text),
    ["fleet-1 3", "fleet-2 3 ●", "fleet-3 3 ●", "all 1 ●", "?"],
  );
  assert.match(v.all(), /fleet-1/);
  assert.doesNotMatch(v.all(), /#25/);
  v.t.step(1); // a swipe left
  assert.match(v.all(), /chain · 2/, "fleet-2's rows show");
  assert.doesNotMatch(v.all(), /#32/);
  v.t.step(-1);
  v.t.step(-1); // from the first repo back round to [all]
  assert.equal(v.els().find((e) => e.a["aria-pressed"] === "true")?.k[0], "all 1");
  assert.match(v.all(), /needs you \(1\)/);
  assert.match(v.all(), /#4 ⏸ waiting: decision/);
  assert.doesNotMatch(v.all(), /#3 |#32/, "only the needs-you rows");
});

test("37: a fleet card in its colour; a paused fleet folds to one line; needs-you rows first", async () => {
  const q = copy();
  must(q.fleets[0]).paused = "paused · 6d · 1 lane";
  Object.assign(row(q, 0, 30), { state: "needs-human", tone: "r" });
  herdsman(q);
  const v = await open();
  const card = must(v.els().find((e) => e.a.class?.toString().startsWith("fc")));
  assert.equal(card.a.class, "fc off");
  assert.equal(card.a.style, `border-left-color:var(--${must(q.fleets[0]).k})`);
  assert.equal(text(card), "fleet-1 ● paused · 6d · 1 laneResume", "one line");
  await v.tap("fleet-1");
  const gs = v
    .els()
    .filter((e) => e.t === "h3")
    .map(text);
  assert.deepEqual(gs, ["needs you (1)", "in progress (2)"]);
});

test("38: a blocked chain is one row; a tap opens its steps as a tree", async () => {
  herdsman(raw);
  const v = await open();
  v.t.step(1);
  assert.equal(row(raw, 1, 26).blocked_by, 25, "the recorded chain: #26 waits on #25");
  assert.match(v.all(), /#25 chain · 2 #1 title text#25 → #26 · next: #25/);
  assert.doesNotMatch(v.all(), /#26 queued/);
  await v.tap(/chain · 2/);
  assert.match(v.all(), /○ #25 queued[\s\S]*└─○ #26 queued/);
});

test("39: groups, queue places, `+ N more` opening in place, stage times, done today", async () => {
  const q = copy();
  const base = row(q, 0, 30);
  must(q.fleets[0]).rows.push(...[100, 101, 102, 103, 104, 105, 106].map((n) => queued(base, n)));
  row(q, 0, 32).t = q.now - 3720;
  herdsman(q);
  const v = await open();
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.t === "h3")
      .map(text),
    ["in progress (2)", "queued (8)"],
  );
  assert.match(v.all(), /#32 impl in impl 1h 2m/);
  assert.match(v.all(), /#30 queued #1 .*#103 queued #5 title text\+ 3 more/);
  assert.doesNotMatch(v.all(), /#104/);
  await v.tap("+ 3 more");
  assert.match(v.all(), /#103 queued #5 .*\+ 3 more.*#104 queued #6 .*#106 queued #8 /);
  await v.tap(`done today: ${must(q.fleets[0]).done_n}`);
  const pr = must(v.els().find((e) => e.t === "a" && text(e) === "PR #29"));
  assert.equal(pr.a.href, "https://github.com/example/repo/pull/29");
});

test("40: issue and PR titles; links to the issue, the PR and its checks", async () => {
  const q = copy();
  row(q, 0, 32).pr = { pr: 41, ci: "green", verdict: "PASS", vtone: "g", rounds: 1, run: null, form: null };
  herdsman(q);
  const v = await open();
  assert.match(v.all(), /#32 impl .*title text/);
  await v.tap(at(32));
  const href = (t: string) => v.els().find((e) => e.t === "a" && text(e) === t)?.a.href;
  assert.equal(href("[issue]"), "https://github.com/example/repo/issues/32");
  assert.equal(href("[PR #41]"), "https://github.com/example/repo/pull/41");
  assert.equal(href("[checks]"), "https://github.com/example/repo/pull/41/checks");
  assert.match(v.all(), /CI ✓ · rev PASS r1/);
});

test("41: ▲ posts the move and the reloaded queue keeps it; a refusal shows", async () => {
  const q = copy();
  const h = herdsman(q, ({ f }) => {
    if (f.mtime !== q.mtime) return '{"ok":false,"code":"STALE","message":"reload and retry"}';
    const rows = must(q.fleets[2]).rows,
      i = rows.findIndex((r) => r.issue === Number(f.issue));
    rows.splice(
      rows.findIndex((r) => r.issue === Number(f.before)),
      0,
      ...rows.splice(i, 1),
    );
    return `{"ok":true,"code":"OK","message":"moved #${f.issue} before #${f.before}"}`;
  });
  const v = await open();
  v.t.step(2);
  assert.match(v.all(), /#3 queued #1 .*#4 queued #2 /);
  await v.tap(at(4));
  await v.tap(must(v.els().find((e) => e.a["aria-label"] === "move up")));
  assert.deepEqual(h.posts, [
    { path: "/api/v1/mv", f: { op: "mv", fleet: "fleet-3", mtime: q.mtime, issue: "4", before: "3" } },
  ]);
  assert.equal(h.gets(), 2, "read again");
  assert.match(v.all(), /moved #4 before #3/);
  assert.match(v.all(), /#4 queued #1 .*#3 queued #2 /);
  q.mtime = "changed";
  await v.tap(must(v.els().find((e) => e.a["aria-label"] === "move down")));
  assert.match(v.all(), /STALE: reload and retry/);
});

test("42: Pause and Resume a fleet", async () => {
  const q = copy();
  const h = herdsman(q, ({ f }) => {
    must(q.fleets[0]).paused = f.op === "pause" ? "paused · 0s · 2 lanes" : null;
    return `{"ok":true,"code":"OK","message":"fleet-1 ${f.op}d"}`;
  });
  const v = await open();
  await v.tap("Pause");
  assert.equal(h.posts[0]?.path, "/api/v1/pause");
  assert.match(v.all(), /fleet-1 ● paused · 0s · 2 lanesResume/);
  await v.tap("Resume");
  assert.deepEqual(h.posts[1]?.f, { op: "resume", fleet: "fleet-1", mtime: q.mtime });
  assert.match(v.all(), /Drive now/);
});

test("43: Drive now: driving… while it runs, then the result line and its details", async () => {
  const q = copy();
  let done = (_: string) => {};
  const h = herdsman(q, () => new Promise((r) => (done = r)));
  const v = await open();
  const going = v.tap("Drive now");
  assert.match(v.all(), /● ⟳ driving…/);
  assert.equal(h.posts[0]?.path, "/api/v1/drive");
  Object.assign(must(q.fleets[0]).drive, {
    line: "started 1",
    tone: "g",
    meta: ["2 lanes"],
    acts: [{ who: "#30", words: "spawned impl", code: "spawn fleet-1 30 impl" }],
  });
  done('{"ok":true,"code":"OK","message":"started 1"}');
  await going;
  assert.match(v.all(), /● started 1/);
  await v.tap("● started 1");
  assert.match(v.all(), /2 lanes/);
  await v.tap("#30: spawned impl");
  assert.match(v.all(), /spawn fleet-1 30 impl/);
});

test("44: pipeline stage chips: the current stage lit", async () => {
  herdsman(raw);
  const v = await open();
  await v.tap(at(32));
  const chips = v.els().filter((e) => e.a.class?.toString().startsWith("pt"));
  assert.deepEqual(
    chips.map((e) => [text(e), e.a.class]),
    [
      ["○", "pt dn"],
      ["▲ impl", "pt on"],
      ["◆ rev", "pt"],
      ["◫ gates", "pt"],
      ["✓ merge", "pt"],
    ],
  );
});

test("45: an issue card's sections; its lanes newest first, a tap opens the lane's sheet", async () => {
  const q = copy();
  const r = row(q, 1, 9),
    l = must(r.lanes[0]);
  r.lanes = [1, 2, 3, 4, 5, 6, 7].map((i) => ({ ...l, name: `fleet-2-impl-9-r${i}`, link: i === 7 ? "" : "live" }));
  herdsman(q);
  const v = await open();
  v.t.step(1);
  await v.tap(at(9));
  assert.match(v.all(), /STATUS.*LINKS.*LANES \(7\)/);
  // The current lane's link under STATUS, then the newest 5 under LANES.
  const lanes = () =>
    v
      .els()
      .filter((e) => e.a.class === "ql")
      .map((e) => e.a.href);
  assert.deepEqual(
    lanes(),
    ["", 1, 2, 3, 4, 5].map((i) => `/p/drover-lanes#lane/fleet-2-impl-9${i && `-r${i}`}`),
  );
  await v.tap("HISTORY +2 older");
  assert.match(v.all(), /fleet-2-impl-9-r7/);
  assert.equal(lanes().length, 7, "a lane with no link is text");
});

test("46: waiting: decision with its options; a link to the asking thread", async () => {
  const q = copy();
  const ask = {
    id: "q12",
    fleet: "fleet-3",
    issue: 4,
    question: "which way?",
    options: ["left", "right"],
    recommend: 2,
    ts: 0,
  };
  Object.assign(row(q, 2, 4), {
    state: "⏸ waiting: decision",
    tone: "r",
    open: { ask, thread: "t6", title: "queue plan" },
  });
  const h = herdsman(q);
  const v = await open();
  v.t.step(2);
  await v.tap(at(4));
  const link = must(v.els().find((e) => e.t === "a" && text(e).startsWith("asked in")));
  assert.equal(text(link), "asked in queue plan · t6");
  assert.equal(link.a.href, "/t/t6");
  assert.deepEqual(v.find(/^\d\. /).map(text), ["2. right (recommended)", "1. left"]);
  await v.tap("2. right");
  assert.deepEqual(h.posts[0]?.f, { op: "answer", fleet: "fleet-3", mtime: q.mtime, id: "q12", choice: "2" });
  (must(v.els().find((e) => e.t === "input")).a.oninput as (s: string) => void)("neither");
  await v.tap("Send");
  assert.deepEqual(h.posts[1]?.f, { op: "answer", fleet: "fleet-3", mtime: q.mtime, id: "q12", text: "neither" });
});

test("47: backlog: search, filters, tick 2, [queue 2]: they come back marked needs-check", async () => {
  const q = copy();
  const one = must(must(must(q.fleets[1]).backlog).issues["22"]),
    ago = (d: number) => new Date((q.now - d * 86400) * 1000).toISOString();
  must(must(q.fleets[1]).backlog).issues = {
    "40": { ...one, title: "fix login", labels: ["bug"], created: ago(1), updated: ago(1), ac: true },
    "41": { ...one, title: "old idea", labels: [], created: ago(100), updated: ago(100), ac: false },
    "42": { ...one, title: "fix logout", labels: ["bug"], created: ago(2), updated: ago(2), ac: true },
  };
  const h = herdsman(q, ({ f }) => {
    must(q.fleets[1]).rows.push(
      ...must(f.issues)
        .split(",")
        .map((n) => queued(row(q, 1, 25), +n, { check: true })),
    );
    must(must(q.fleets[1]).backlog).issues = { "41": must(must(must(q.fleets[1]).backlog).issues["41"]) };
    return '{"ok":true,"code":"OK","message":"queued (needs-check)"}';
  });
  const v = await open();
  v.t.step(1);
  await v.tap("backlog (3)");
  const search = must(v.els().find((e) => e.a.type === "search")).a.oninput as (s: string) => void;
  search("fix");
  assert.match(v.all(), /backlog \(2 of 3\)/);
  search("");
  await v.tap("has AC");
  assert.doesNotMatch(v.all(), /old idea/);
  await v.tap(">90 days");
  assert.match(v.all(), /backlog \(0 of 3\)/);
  await v.tap(">90 days");
  await v.tap("has AC");
  const tick = (n: number) => v.tap(must(v.els().find((e) => e.a.value === String(n))));
  await tick(42);
  await tick(40);
  await v.tap("queue 2");
  assert.deepEqual(h.posts[0], {
    path: "/api/v1/bqueue",
    f: { op: "bqueue", fleet: "fleet-2", mtime: q.mtime, issues: "42,40" },
  });
  assert.match(v.all(), /#42 queued #\d+ title textneeds-check/);
  assert.match(v.all(), /#40 queued #\d+ title textneeds-check/);
  assert.match(v.all(), /backlog \(1\)/);
});

test("48: the [?] legend explains every state and dot; serial groups' 🔒 tags", async () => {
  const q = copy();
  Object.assign(must(q.fleets[0]), { serial: ["db"], paths: ["src/db/**"] });
  row(q, 0, 30).groups = ["db"];
  herdsman(q);
  const v = await open();
  const lock = must(v.els().find((e) => text(e) === "🔒db"));
  assert.equal(lock.a.title, "serial: one lane at a time");
  await v.tap(must(v.els().find((e) => e.a["aria-label"] === "legend")));
  for (const [w, h] of Object.entries(HINT)) assert.ok(v.all().includes(`${w} = ${h}`), w);
  for (const d of ["● a lane is on it", "○ waiting", "✓ ready", "! needs you"]) assert.ok(v.all().includes(d), d);
  assert.match(v.all(), /🔒db one at a time: touches src\/db\/\*\*/);
  const state = v.els().find((e) => e.a.title?.toString().startsWith("impl ="));
  assert.equal(state?.a.title, "impl = being built", "a state word's title is its hint");
});

test("14: a row in verify(1) is in progress with its verify lane; its card shows the mark and note", async () => {
  const h = herdsman(raw);
  const v = await open();
  v.t.step(2);
  assert.equal(row(raw, 2, 2).state, "verify(1)", "the recorded fixture: fleet-3 #2 in verify(1)");
  assert.match(v.all(), /in progress \(1\)● #2 verify\(1\) in verify /);
  assert.equal(
    v.els().find((e) => text(e) === "verify(1)")?.a.title,
    "verify(n) = being tried in the real app, round n",
  );
  assert.match(v.all(), /#2 verify\(1\) .*verify on/, "the mark set on shows on the row");
  await v.tap(at(2));
  assert.deepEqual(h.other, ["/api/v1/issue/fleet-3/2"], "opening reads its verdicts");
  assert.match(v.all(), /VERIFYverify: on · verify\(1\)/);
  assert.match(v.all(), /◆ rev ✓ ─ ◆ verify/);
  assert.equal(v.els().find((e) => e.a.class === "ql")?.a.href, "/p/drover-lanes#lane/fleet-3-verify-2");
});

test("14: a row whose verify was skipped shows why", async () => {
  herdsman(raw);
  const v = await open();
  v.t.step(1);
  const tag = v.els().find((e) => e.a.class === "tag a");
  assert.equal(tag && text(tag), "verify skipped: no .herdsman/verify.md");
  assert.match(v.all(), /#9 parked .*verify skipped: no \.herdsman\/verify\.md/);
});

test("14: a mark by UI paths; [verify on] posts qset; off shows on its row; unpaired, the tap answers UNPAIRED", async () => {
  const q = copy();
  const h = herdsman(q, ({ f }) => {
    if (f.value === "off") return '{"ok":false,"code":"UNPAIRED","message":"pair this device first"}';
    must(must(q.fleets[0]).marks[must(f.issue)]).verify = must(f.value);
    return `{"ok":true,"code":"OK","message":"#${f.issue}: ${f.key} ${f.value}"}`;
  });
  const v = await open();
  assert.match(v.all(), /#30 queued #1 title textverify off/);
  await v.tap(at(32));
  assert.match(v.all(), /VERIFYverify: by UI paths/);
  await v.tap("verify on");
  assert.deepEqual(h.posts[0], {
    path: "/api/v1/qset",
    f: { op: "qset", fleet: "fleet-1", mtime: q.mtime, issue: "32", key: "verify", value: "on" },
  });
  assert.match(v.all(), /#32: verify on/);
  assert.match(v.all(), /VERIFYverify: on/, "the reloaded queue's mark");
  assert.equal(v.els().find((e) => text(e) === "verify on" && e.t === "button")?.a["aria-pressed"], "true");
  await v.tap("verify off");
  assert.match(v.all(), /UNPAIRED: pair this device first/);
  assert.match(v.all(), /VERIFYverify: on/, "unchanged");
});

test("14: the issue sheet: verdicts newest first, a verify PASS's 1440 and 390 screenshots inline, the design beside", async () => {
  const shot = (i: number) => `data:image/png;base64,SHOT${i}`;
  const h = herdsman(raw, undefined, {
    "/api/v1/issue/fleet-1/22": JSON.stringify(rawIssue),
    "/shot/fleet-1/22/0": shot(0),
    "/shot/fleet-1/22/1": shot(1),
    "/design/fleet-1/22/0": "data:image/png;base64,DESIGN",
  });
  const v = await open();
  await v.tap(at(22));
  assert.match(
    v.all(),
    /VERDICTS \(2\)verify r1 PASS 0123456findings textqueue-1440\.pngqueue-390\.pngdesign 1rev r1 PASS 0123456findings text/,
  );
  const imgs = v
    .els()
    .filter((e) => e.t === "img")
    .map((e) => [e.a.alt, e.a.src]);
  assert.deepEqual(imgs, [
    ["queue-1440.png", shot(0)],
    ["queue-390.png", shot(1)],
    ["design 1", "data:image/png;base64,DESIGN"],
  ]);
  assert.deepEqual(
    h.other,
    [
      "/api/v1/issue/fleet-1/22",
      "/shot/fleet-1/22/0",
      "/shot/fleet-1/22/1",
      "/design/fleet-1/22/0",
      "/design/fleet-1/22/1",
    ],
    "each screenshot, then the design's PNGs until one is missing",
  );
  assert.equal(v.els().find((e) => e.t === "span" && text(e) === "PASS")?.a.class, "g");
  await v.tap(at(32));
  assert.ok(!h.other.some((p) => p.startsWith("/design/fleet-1/32")), "no design stage, no design read");
  assert.equal(
    v.els().filter((e) => e.a.class === "cd" && /VERDICTS/.test(text(e))).length,
    1,
    "an issue with no verdicts shows none",
  );
});

test("15: a repro issue's row shows the mark; a NO_REPRO merge refusal shows on its row in plain words", async () => {
  const q = copy();
  must(q.fleets[0]).drive.acts = [
    { who: "#22", words: "rev's `Repro: verify` PASS has no verify PASS at 0123456", code: "NO_REPRO" },
  ];
  herdsman(q);
  const v = await open();
  assert.equal(must(q.fleets[0]).marks["22"]?.repro, true, "the fixture: fleet-1 #22 queued --repro");
  assert.match(v.all(), /#22 .*title textreprorev's PASS has no repro proof/);
  assert.doesNotMatch(v.all(), /#(30|32) [^#]*repro/, "unmarked rows show no mark");
  assert.equal(
    v
      .els()
      .filter((e) => text(e) === "rev's PASS has no repro proof")
      .map((e) => e.a.class)
      .join(),
    "tag r",
  );
  must(must(q.fleets[0]).drive.acts[0]).code = "CI_NOT_GREEN";
  await v.t.load();
  assert.doesNotMatch(v.all(), /no repro proof/, "another refusal is not NO_REPRO");
});

test("15: [repro off] posts qset and the reloaded mark shows; unpaired, the tap answers UNPAIRED", async () => {
  const q = copy();
  const h = herdsman(q, ({ f }) => {
    if (f.value === "on") return '{"ok":false,"code":"UNPAIRED","message":"pair this device first"}';
    must(must(q.fleets[0]).marks[must(f.issue)]).repro = f.value === "on";
    return `{"ok":true,"code":"OK","message":"#${f.issue}: ${f.key} ${f.value}"}`;
  });
  const v = await open();
  await v.tap(at(22));
  assert.match(v.all(), /REPROrepro: on · rev's PASS needs the failing test's proof/);
  assert.equal(v.els().find((e) => text(e) === "repro on" && e.t === "button")?.a["aria-pressed"], "true");
  await v.tap("repro off");
  assert.deepEqual(h.posts[0], {
    path: "/api/v1/qset",
    f: { op: "qset", fleet: "fleet-1", mtime: q.mtime, issue: "22", key: "repro", value: "off" },
  });
  assert.match(v.all(), /#22: repro off/);
  assert.match(v.all(), /REPROrepro: off/, "the reloaded queue's mark");
  assert.doesNotMatch(v.all(), /#22 [^#]*title textrepro/, "the row's mark is gone");
  await v.tap("repro on");
  assert.match(v.all(), /UNPAIRED: pair this device first/);
  assert.match(v.all(), /REPROrepro: off/, "unchanged");
});

/** The issue sheet of fleet-1 #22, its verdicts `vs`. */
async function sheet(vs: IssueView["verdicts"]) {
  herdsman(raw, undefined, { "/api/v1/issue/fleet-1/22": JSON.stringify({ ...rawIssue, verdicts: vs }) });
  const v = await open();
  await v.tap(at(22));
  return v;
}

test("15: a `Repro: run` PASS renders both runs: the command failing on the base, then passing at the head", async () => {
  assert.equal(rawIssue.verdicts[1]?.repro?.kind, "run", "the fixture's rev PASS carries a Repro: run block");
  const body =
    "Base plus commit 1:\n```\nnpm test -- repro\n```\n```sh\nnot ok 1 repro\n```\nHead:\n```\nnpm test -- repro\n```\n```\nok 1 repro\n```";
  const v = await sheet([{ ...must(rawIssue.verdicts[1]), repro: { kind: "run", text: body } }]);
  assert.match(
    v.all(),
    /rev r1 PASS 0123456findings textREPRO: runon the base, fails:npm test -- repronot ok 1 reproat the head 0123456, passes:npm test -- reprook 1 repro/,
  );
  assert.deepEqual(
    v
      .els()
      .filter((e) => e.t === "code")
      .map(text)
      .slice(-4),
    ["npm test -- repro", "not ok 1 repro", "npm test -- repro", "ok 1 repro"],
  );
});

test("15: a `Repro: verify` PASS quotes its reason and waits on verify until a verify PASS at its head", async () => {
  const rev = {
    ...must(rawIssue.verdicts[1]),
    repro: { kind: "verify", text: "> a pure visual bug: the meter's colour\n> has no unit test path" },
  };
  let v = await sheet([rev]);
  assert.match(
    v.all(),
    /REPRO: verifya pure visual bug: the meter's colour has no unit test pathwaits on verify: no verify PASS at 0123456 yet/,
  );
  assert.equal(
    text(must(v.els().find((e) => e.t === "blockquote"))),
    "a pure visual bug: the meter's colour has no unit test path",
  );
  v = await sheet([{ ...must(rawIssue.verdicts[0]), sha: "f".repeat(40) }, rev]);
  assert.match(
    v.all(),
    /waits on verify: no verify PASS at 0123456 yet/,
    "a verify PASS at another sha does not count",
  );
  v = await sheet([must(rawIssue.verdicts[0]), rev]);
  assert.match(v.all(), /REPRO: verify.*verify PASS at 0123456/);
  assert.doesNotMatch(v.all(), /waits on verify/);
});
