// The drover-fleets tab, a test per herdsman#553 row it keeps (75, 77, 80-85; cooee-drover#5 amendment A): the real tab
// module (`lib/fleets.ts`) and client (`lib/api.ts`) against a fake `cooee` that serves the recorded `/api/v1`
// fixtures (`queue`, `host`, `state`, `events?since=&until=`, #16 `lessons` and a fleet's `digest`) and records each
// form POST.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import type { El, Kid } from "../src/lib/dom.ts";
import { fleetsTab } from "../src/lib/fleets.ts";
import type { Digest, Ev, FleetsTab, HostTab, Lessons, State } from "../src/lib/types.ts";
import { must } from "./must.ts";

const fx = (n: string) => JSON.parse(fs.readFileSync(new URL(`../fixtures/${n}.json`, import.meta.url), "utf8"));
type Api = {
  queue: FleetsTab;
  host: HostTab;
  state: State;
  lessons: Lessons;
  digest: Digest;
  events: Ev[];
  down?: string;
};
const api = (): Api => ({
  queue: fx("queue"),
  host: fx("host"),
  state: fx("state"),
  lessons: fx("lessons"),
  digest: fx("digest"),
  events: fx("events").list,
});

type Post = { path: string; f: Record<string, string> };
/** A fake herdsman behind `cooee.fetch`; `on` answers a POST (and may change `a`); `events` pages like herdsman's. */
function herdsman(a: Api, on: (p: Post) => string = () => '{"ok":true,"code":"OK","message":"done"}') {
  const posts: Post[] = [],
    gets: string[] = [];
  (globalThis as { cooee?: unknown }).cooee = {
    async fetch(path: string, init?: { method?: string; type?: string; body?: string }) {
      if (init?.method === "POST") {
        assert.equal(init.type, "application/x-www-form-urlencoded");
        const p = { path, f: Object.fromEntries(new URLSearchParams(init.body)) };
        posts.push(p);
        return { status: 200, ok: true, type: "application/json", text: on(p) };
      }
      gets.push(path);
      const m =
        /^\/api\/v1\/(queue|host|state|events|lessons)(?:\?since=(\d+)&until=(\d+)|\/([^/]+)\/digest\?days=7)?$/.exec(
          path,
        );
      assert.ok(m, `no such GET ${path}`);
      if (m[1] === a.down) return { status: 502, ok: false, type: "text/plain", text: "herdsman is unreachable" };
      const body =
        m[1] === "events"
          ? {
              seq: a.state.seq,
              refresh: false,
              list: a.events.filter((e) => e.seq > Number(m[2]) && e.seq <= Number(m[3])),
            }
          : m[4]
            ? { ...a.digest, fleet: decodeURIComponent(m[4]) }
            : a[m[1] as "queue"];
      return { status: 200, ok: true, type: "application/json", text: JSON.stringify(body) };
    },
  };
  return { posts, gets };
}

const text = (e: Kid): string => (typeof e === "string" ? e : e.k.map(text).join(""));
/** The tab loaded from the fake herdsman: its tree, finders over it, a tap. */
async function open() {
  let kids: Kid[] = [];
  const t = fleetsTab((k) => (kids = k));
  await t.load();
  const els = (k: Kid[] = kids): El[] => k.flatMap((x) => (typeof x === "string" ? [] : [x, ...els(x.k)]));
  const one = (s: string | RegExp, k?: Kid[]) => {
    const f = els(k).filter((e) => e.a.on && (typeof s === "string" ? text(e) === s : s.test(text(e))));
    assert.ok(f.length, `nothing tappable reads ${s}:\n${text({ t: "", a: {}, k: kids })}`);
    return must(f.at(-1));
  };
  const tap = async (s: string | RegExp, k?: Kid[]) => (one(s, k).a.on as (v: string) => unknown)("");
  return { els, one, tap, all: () => text({ t: "", a: {}, k: kids }) };
}
/** An event like the recorded one, at `ago` seconds before the state's `now`. */
const ev = (a: Api, seq: number, ago: number, f: Partial<Ev> = {}): Ev => ({
  ...must(a.events[0]),
  seq,
  ts: a.state.now - ago,
  fleet: "fleet-1",
  issue: 7,
  ...f,
});

test("the recorded host, events and queue config fit the contract; a retyped field breaks it, naming it", async () => {
  const a = api(),
    q = fx("queue");
  q.fleets[0].config.pipe.stages[0].here = "4";
  a.queue = q;
  herdsman(a);
  assert.match(
    (await open()).all(),
    /herdsman \/api\/v1\/queue broke the contract at fleets\[0\]\.config\.pipe\.stages\[0\]\.here/,
  );
  const b = api(),
    hs = fx("host");
  hs.hosts[0].to = 1;
  b.host = hs;
  herdsman(b);
  assert.match((await open()).all(), /herdsman \/api\/v1\/host broke the contract at hosts\[0\]\.to/);
});

test("80: a fleet opens to its grouped settings and its stage graph", async () => {
  const a = api();
  herdsman(a);
  const v = await open();
  assert.match(v.all(), /FLEETS · 3 repos · config path text/);
  assert.doesNotMatch(v.all(), /General/, "closed until tapped");
  await v.tap(/^fleet-1 /);
  const hs = v
    .els()
    .filter((e) => e.t === "h3")
    .map((e) => e.k[0]);
  assert.deepEqual(hs.slice(0, 6), ["General", "Pipeline", "Limits", "Roles", "Conductor", "Alerts"]);
  const all = v.all();
  assert.match(all, /integrationstaging/);
  assert.match(all, /review rounds6/);
  assert.match(all, /✓ valid · 3 stages \(impl, rev, gates\) · built-in/);
  assert.match(all, /○ queued 0 waiting▲ impl build4 hererole impl · run text◆ rev check/);
  assert.match(all, /FAIL → impl · ≤6 rounds, then park/);
  assert.match(
    all,
    /◫ gates gate · herdsmanCI green · PASS at head · mergeableci_red → implconflict → update in impl · ≤3 rounds, then park✓ merge into staging/,
  );
  assert.doesNotMatch(all, /push → rev|PASS → gates|ok → merge/, "the spine draws no outcome line");
  must(a.queue.fleets[0]).config.pipe.bad = [
    { rule: "NO_LIMIT", msg: "loop rev has no rounds", stage: "rev", line: 12 },
  ];
  herdsman(a);
  const w = await open();
  await w.tap(/^fleet-1 /);
  assert.match(
    w.all(),
    /BAD_PIPELINE · 1 error: nothing moves for this fleet until fleet\.toml is fixed✗ NO_LIMIT: loop rev has no rounds fleet\.toml:12/,
  );
});

test("81: role pickers: the effort list follows the harness; [Set] posts the role's harness, model and effort", async () => {
  const a = api();
  must(a.queue.fleets[0]).config.models = ["claude-opus-5-5", "claude-sonnet-5-5"];
  const h = herdsman(a, ({ f }) => `{"ok":true,"code":"OK","message":"${f.fleet} ${f.role}: effort ${f.effort}"}`);
  const v = await open();
  await v.tap(/^fleet-1 /);
  const roleOf = (n: string) => must(v.els().find((e) => e.a.class === "role" && text(e).startsWith(n)));
  const sel = (n: string, k: string) =>
    must(v.els(roleOf(n).k).find((e) => e.t === "select" && e.a["aria-label"] === k));
  const opts = (n: string, k: string) => sel(n, k).k.map(text);
  assert.deepEqual(opts("impl", "model"), ["claude-opus-5-5", "claude-sonnet-5-5"]);
  (sel("impl", "harness").a.oninput as (v: string) => void)("dsh");
  assert.deepEqual(opts("impl", "effort"), ["default (none)", "none"], "dsh takes effort none only");
  (sel("impl", "harness").a.oninput as (v: string) => void)("claude");
  (sel("impl", "effort").a.oninput as (v: string) => void)("medium");
  await v.tap("Set", roleOf("impl").k);
  const { mtime } = a.queue;
  assert.deepEqual(
    h.posts.map((p) => [p.path, p.f]),
    [
      [
        "/api/v1/set",
        {
          op: "set",
          fleet: "fleet-1",
          mtime,
          role: "impl",
          harness: "claude",
          model: "claude-opus-5-5",
          effort: "medium",
        },
      ],
    ],
  );
  assert.match(v.all(), /fleet-1 impl: effort medium/);
  assert.equal(h.gets.filter((g) => g === "/api/v1/queue").length, 2, "read again after the tap");
  must(a.queue.fleets[0]).config.models = [];
  herdsman(a);
  const w = await open();
  await w.tap(/^fleet-1 /);
  const m = must(w.els().find((e) => e.t === "select" && e.a["aria-label"] === "model"));
  assert.equal(m.a.disabled, "", "no models: the model stays as set");
  assert.match(w.all(), /add models = \[\.\.\.\] to fleet\.toml to pick a model/);
});

test("82: a tap on a conductor model posts `cond`; read-only shows them disabled; chat defaults wait for v1", async () => {
  const a = api();
  const h = herdsman(a);
  const v = await open();
  await v.tap(/^fleet-1 /);
  const cg = must(v.els().find((e) => e.a.class === "cg" && text(e).startsWith("Conductor")));
  assert.equal(v.els(cg.k).find((e) => text(e) === "claude-opus-5-5")?.a["aria-pressed"], "true");
  await v.tap("claude-sonnet-5-5", cg.k);
  assert.deepEqual(
    h.posts.map((p) => [p.path, p.f]),
    [["/api/v1/cond", { op: "cond", fleet: "fleet-1", mtime: a.queue.mtime, model: "claude-sonnet-5-5" }]],
  );
  assert.match(v.all(), /CHAT DEFAULTS.*waits for herdsman's v1 write/);
  a.queue.rw = false;
  herdsman(a);
  const w = await open();
  await w.tap(/^fleet-1 /);
  const b = must(w.els().find((e) => e.t === "button" && text(e) === "claude-sonnet-5-5"));
  assert.equal(b.a.on, undefined);
  assert.equal(b.a.disabled, "");
});

test("83: activity: time groups, kind glyphs, [important] hides noise, folds, [older] pages back", async () => {
  const a = api();
  a.events = [
    ev(a, 10, 3 * 86400, { text: "old one" }),
    ev(a, 11, 86400, { text: "from yesterday" }),
    ev(a, 12, 4000, { text: "an hour ago" }),
    ...[13, 14, 15].map((n) => ev(a, n, 600 + n, { type: "clean", text: `clean ${n}`, imp: false })),
    ev(a, 16, 60, { type: "merged", glyph: "✓", tone: "g", text: "merged PR #8", imp: true }),
  ];
  a.state.seq = 16;
  herdsman(a);
  const v = await open();
  const hs = v
    .els()
    .filter((e) => e.t === "h3")
    .map(text);
  assert.deepEqual(hs.slice(0, 4), ["now", "5m", "1h", "yesterday"]);
  assert.match(must(hs[4]), /^[a-z]{3} \d+$/, "older days by date");
  assert.match(v.all(), /✓ fleet-1 #7 merged PR #8 1m/);
  assert.match(v.all(), /clean ×3 · lanes cleaned/);
  assert.match(v.all(), /ACTIVITY · 5 rows/);
  await v.tap("important");
  assert.match(v.all(), /ACTIVITY · 1 important row/);
  assert.doesNotMatch(v.all(), /lanes cleaned|from yesterday/);
  await v.tap("all");
  await v.tap(/clean ×3/);
  assert.match(v.all(), /clean · plain text.*clean · plain text.*clean · plain text/);

  const b = api();
  b.events = Array.from({ length: 600 }, (_, i) => ev(b, i + 1, 600 - i, { text: `e${i + 1}` }));
  b.state.seq = 600;
  const h = herdsman(b);
  const w = await open();
  assert.deepEqual(
    h.gets.filter((g) => g.startsWith("/api/v1/events")),
    ["/api/v1/events?since=100&until=600"],
  );
  assert.equal(w.els().filter((e) => e.a.class === "er").length, 100);
  for (let i = 0; i < 4; i++) await w.tap("older");
  assert.equal(w.els().filter((e) => e.a.class === "er").length, 500);
  await w.tap("older");
  assert.equal(h.gets.at(-1), "/api/v1/events?since=0&until=100", "the 500 before the oldest");
  assert.equal(w.els().filter((e) => e.a.class === "er").length, 600);
  assert.match(w.all(), /e1 /);
});

test("84: notifiers listed; no Telegram", async () => {
  const a = api();
  a.host.notify.push({ kind: "command", on: "merged, needs-human" });
  herdsman(a);
  const v = await open();
  assert.match(v.all(), /NOTIFIERS · global · edit fleet\.tomlweb push · default eventscommand · merged, needs-human/);
  assert.doesNotMatch(v.all(), /telegram/i);
  a.host.notify = [];
  herdsman(a);
  assert.match((await open()).all(), /none: \[\[notify\]\] in fleet\.toml/);
});

test("85: fleet.toml unread: herdsman's own error, plainly; herdsman down: the call's error; the rest still shows", async () => {
  const a = api();
  const real = "fleet.toml: TOML parse error at line 3, column 9: expected `=`";
  Object.assign(a.queue, { error: real, fleets: [] });
  herdsman(a);
  const v = await open();
  const e = must(v.els().find((x) => text(x) === real));
  assert.equal(e.a.class, "r");
  assert.match(v.all(), /HOSTS FOR LANES.*ACTIVITY/);
  const b = api();
  b.down = "queue";
  herdsman(b);
  const w = await open();
  assert.match(w.all(), /^herdsman \/api\/v1\/queue answered 502: herdsman is unreachable/);
  assert.match(w.all(), /FLEET DISK/);
});

test("75: [off] on a host asks first; [yes] posts `host`; it reads off and offers [on]", async () => {
  const a = api();
  Object.assign(must(a.host.hosts[0]), { to: "off", words: "up · admits lanes", lanes: 22 });
  const h = herdsman(a, ({ f }) => {
    Object.assign(must(a.host.hosts.find((x) => x.name === f.host)), { to: "on", words: "off · not used for lanes" });
    return `{"ok":true,"code":"OK","message":"${f.host} off"}`;
  });
  const v = await open();
  assert.match(v.all(), /minimain up · admits lanes · 22 lanes off/);
  await v.tap("off");
  assert.match(v.all(), /turn mini off\? no new lanes; 22 running lanes finish yescancel/);
  await v.tap("cancel");
  assert.equal(h.posts.length, 0);
  await v.tap("off");
  await v.tap("yes");
  assert.deepEqual(
    h.posts.map((p) => [p.path, p.f]),
    [["/api/v1/host", { op: "host", fleet: "", mtime: a.host.mtime, host: "mini", to: "off" }]],
  );
  assert.match(v.all(), /mini off/);
  assert.match(v.all(), /minimain off · not used for lanes · 22 lanes on/);
  a.host.rw = false;
  herdsman(a);
  assert.doesNotMatch((await open()).all(), /lanes on|lane on/, "read-only: no switch");
});

test("77: fleet disk against its budget; [remove orphans] and [reap] ask first, then show herdsman's answer", async () => {
  const a = api();
  Object.assign(a.host, { orphans: 2, leftovers: { n: 3, mb: "120" } });
  const h = herdsman(a, ({ f }) => {
    if (f.op === "reap") {
      a.host.leftovers = null;
      return '{"ok":true,"code":"OK","message":"reaped 3 leftover process(es)"}';
    }
    a.host.orphans = 0;
    return '{"ok":true,"code":"OK","message":"removed 2 orphan folder(s)"}';
  });
  const v = await open();
  assert.match(v.all(), /worktrees ▕██████░░░░░░▏ 38\.9 GB of 80 GB budget · walked 2m ago/);
  assert.match(
    v.all(),
    /2 orphan folders from no project remove orphans3 processes left in closed lanes · 120 MB reap/,
  );
  await v.tap("reap");
  assert.match(v.all(), /TERM, then KILL, 3 leftover process\(es\)\? yes/);
  await v.tap("yes");
  assert.match(v.all(), /reaped 3 leftover process\(es\)/);
  await v.tap("remove orphans");
  assert.match(v.all(), /Remove 2 orphan folders under the worktree root\?/);
  await v.tap("yes");
  assert.deepEqual(
    h.posts.map((p) => p.f.op),
    ["reap", "orphans"],
  );
  assert.deepEqual(h.posts[0]?.f, { op: "reap", fleet: "", mtime: a.host.mtime });
  assert.match(v.all(), /removed 2 orphan folder\(s\).*no orphan folders, no leftover processes/);
});

/** #16: the recorded lessons with fleet-1's run merged and fleet-3's lessons off; fleet-2 has never run, as recorded. */
const lessonsApi = () => {
  const a = api(),
    f = a.lessons.fleets,
    one = must(f[0], "fleet-1"),
    two = must(f[1], "fleet-2"),
    three = must(f[2], "fleet-3");
  assert.deepEqual([two.last, two.next], [null, 1794443324], "the recorded fleet that never ran");
  one.last = { issue: 31, ts: a.state.now - 5 * 86400, state: "merged" };
  Object.assign(three, { lessons: "off", next: null });
  return a;
};
/** A fleet's LESSONS row, and its words. */
const lrow = (v: Awaited<ReturnType<typeof open>>, fleet: string) =>
  must(
    v.els().find((e) => e.a.class === "rw" && e.k[0] != null && text(e.k[0]) === fleet),
    `${fleet}'s lessons row`,
  );
const row = (v: Awaited<ReturnType<typeof open>>, fleet: string) => text(lrow(v, fleet));

test("16: per fleet its lessons, the last run (issue, date, state) or never run, and the next due", async () => {
  herdsman(lessonsApi());
  const v = await open();
  assert.match(v.all(), /LESSONS · weekly lesson runs/);
  assert.equal(row(v, "fleet-1"), "fleet-1 weekly · last #31 nov 1 merged · next due nov 12 Run lessonsdigest");
  assert.equal(row(v, "fleet-2"), "fleet-2 weekly · never run · first run due nov 12 Run lessonsdigest");
  assert.equal(row(v, "fleet-3"), "fleet-3 off · never run Run lessonsdigest");
});

test("16: [Run lessons] asks first; [yes] posts `lesson`; unpaired, the tap answers UNPAIRED", async () => {
  const { posts } = herdsman(lessonsApi(), () => '{"ok":false,"code":"UNPAIRED","message":"pair this device first"}');
  const v = await open();
  await v.tap("Run lessons", [lrow(v, "fleet-2")]);
  assert.equal(posts.length, 0, "asks first");
  assert.match(v.all(), /Run lessons for fleet-2 now\? It files a lessons issue and spawns its lane yescancel/);
  await v.tap("yes");
  assert.deepEqual(posts, [{ path: "/api/v1/lesson", f: { op: "lesson", fleet: "fleet-2" } }]);
  assert.match(v.all(), /UNPAIRED: pair this device first/);
});

test("16: a read-only board offers no [Run lessons]", async () => {
  const a = lessonsApi();
  a.queue.rw = false;
  herdsman(a);
  assert.equal(row(await open(), "fleet-1"), "fleet-1 weekly · last #31 nov 1 merged · next due nov 12 digest");
});

test("16: [digest] reads the fleet's 7-day lesson digest and shows it read-only; tapped again it closes", async () => {
  const { gets, posts } = herdsman(lessonsApi());
  const v = await open();
  await v.tap("digest", [lrow(v, "fleet-3")]);
  assert.equal(gets.at(-1), "/api/v1/lessons/fleet-3/digest?days=7");
  const pres = v.els().filter((e) => e.t === "pre");
  assert.equal(pres.length, 1);
  const pre = must(pres[0], "the digest");
  assert.match(text(pre), /^# Lessons digest: fleet-1, last 7 days\n\n## Review FAIL findings/);
  assert.equal(pre.k.length, 1, "plain text, nothing tappable");
  await v.tap("digest", [lrow(v, "fleet-3")]);
  assert.equal(v.els().filter((e) => e.t === "pre").length, 0);
  assert.equal(posts.length, 0);
});
