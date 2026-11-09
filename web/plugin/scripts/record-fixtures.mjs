// Records herdsman /api/v1 responses into web/plugin/fixtures/ for the contract test (`node web/plugin/scripts/record-fixtures.mjs
// http://127.0.0.1:7777`). The repo is public: fleet names become fleet-1…, free text and repos are replaced, and
// each list (and the backlog's issues) keeps its first 3 rows, so only the shapes are recorded. A config sheet's paths
// are replaced; `host`'s push key, devices, subscriptions and accounts emptied. herdsman#576: a queue card keeps the
// marks of its kept rows; `issue` is the first queued row with a verdict (a repro one first, else the first row), its
// findings replaced and a `Repro:` block's text kept to its shape (#15). #16: `lessons`, and the first fleet's 7-day
// `digest` with only its headings kept. #17 herdsman#584: `endings`, the newest closed ask of each ending (answered,
// typed, delivered, withdrawn, failed, expired) and its `needs-decision`, read back through the whole log; ids, `by`,
// `for` and `to` kept.
import fs from "node:fs";

const base = process.argv[2] ?? "http://127.0.0.1:7777";
const TEXT = new Set(["title", "question", "detail", "fix", "shown", "message", "mtime", "line", "meta", "words", "code",
  "notes", "note", "paused", "decided", "kind", "paths", "labels", "why", "fail", "reason", "plain", "result", "run",
  "activity", "tail", "command", "thread", "from", "path", "local", "key", "label", "hash", "msg", "deny", "env",
  "notify", "global", "required", "findings"]);
const DROP = { subs: [], devices: [], accounts: [] };
const read = async (path) => (await fetch(`${base}/api/v1/${path}`)).json();
// state's fleets name every fleet; needs and events (the newest 40) are scrubbed by the same map.
const state = await read("state");
const fleets = new Map(state.fleets.map((f, i) => [f.name, `fleet-${i + 1}`]));
const longest = [...fleets].sort(([a], [b]) => b.length - a.length);
const cards = (await read("queue")).fleets, rows = cards.flatMap((c) => c.rows.map((r) => [c.name, r]));
const repro = (n, r) => cards.find((c) => c.name === n).marks[r.issue]?.repro;
const [fleet, row] = rows.find(([n, r]) => r.pr.verdict != null && repro(n, r)) ?? rows.find(([, r]) => r.pr.verdict != null) ?? rows[0];
const fence = (t) => `\`\`\`\n${t}\n\`\`\`\n`, RUN = `Base plus commit 1:\n${fence("command text")}${fence("output text")}Head:\n${fence("command text")}${fence("output text")}`;

for (const [path, file] of [["state", "state"], ["queue", "queue"], ["needs", "needs"], ["host", "host"], [`events?since=${state.seq - 40}`, "events"],
  [`issue/${encodeURIComponent(fleet)}/${row.issue}`, "issue"], ["lessons", "lessons"],
  [`lessons/${encodeURIComponent(state.fleets[0].name)}/digest?days=7`, "digest"]]) {
  const raw = path === "state" ? state : await read(path);
  const scrub = (s) => longest.reduce((t, [real, fake]) => t.replaceAll(real, fake), s);
  const walk = (v, key) => {
    if (Array.isArray(v)) return v.slice(0, 3).map((x) => walk(x, key));
    // An event's `data` is free-form: its strings become `<key> text`, its lists and objects null.
    if (key === "data") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, typeof x === "string" ? `${k} text` : typeof x === "object" ? null : x]));
    if (key in DROP) return DROP[key];
    if (key === "repro" && v) return { kind: v.kind, text: v.kind === "run" ? RUN.trimEnd() : "> reason text" };
    if (key === "issues") return Object.fromEntries(Object.entries(v).slice(0, 3).map(([n, x]) => [n, walk(x)]));
    if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x, k)]));
    if (typeof v !== "string") return v;
    if (key === "repo") return "example/repo";
    if (file === "digest" && key === "text") return v.split("\n").map((l) => (l && !l.startsWith("#") ? "- line text" : scrub(l))).join("\n");
    if (key === "kind" && /^(build|check|gate)$/.test(v)) return v; // a pipeline stage's kind
    if (key === "note" && /^verify/.test(v)) return v; // a row's verify note
    if (TEXT.has(key) || key === "options") return `${key} text`;
    return scrub(v);
  };
  const out = new URL(`../fixtures/${file}.json`, import.meta.url), json = walk(raw);
  if (file === "queue") for (const c of json.fleets) c.marks = Object.fromEntries(Object.entries(c.marks).filter(([n]) => c.rows.some((r) => r.issue === +n)));
  fs.writeFileSync(out, JSON.stringify(json, null, 2) + "\n");
  console.log(`recorded ${file}`);
}

// #17: an ending's kind; the log read back 500 at a time until each has one, then each one's ask.
const kind = (e) => e.type === "decision" ? (e.data.to ? "delivered" : e.data.by === "owner (typed)" ? "typed" : "answered")
  : /^ask-(withdrawn|delivered|failed|expired)$/.test(e.type) ? e.type : null;
const KINDS = ["answered", "typed", "delivered", "ask-withdrawn", "ask-delivered", "ask-failed", "ask-expired"];
const ends = new Map(), asks = new Map();
for (let u = state.seq; u > 0 && (ends.size < KINDS.length || [...ends.values()].some((e) => !asks.has(`${e.fleet} ${e.data.id}`))); u -= 500) {
  const list = (await read(`events?since=${Math.max(0, u - 500)}&until=${u}`)).list.reverse();
  for (const e of list) if (kind(e) && !ends.has(kind(e))) ends.set(kind(e), e);
  for (const e of list) if (e.type === "needs-decision" && [...ends.values()].some((x) => x.fleet === e.fleet && x.data.id === e.data.id)) asks.set(`${e.fleet} ${e.data.id}`, e);
}
for (const k of KINDS) if (!ends.has(k)) console.log(`endings: herdsman's log has no ${k}`);
const KEEP = new Set(["id", "by", "for", "to", "key", "agent", "thread", "kind", "recommend", "via"]);
const fake = (s) => longest.reduce((t, [real, f]) => t.replaceAll(real, f), s);
const scrubbed = (e) => ({ ...e, fleet: fleets.get(e.fleet) ?? e.fleet, text: `${e.type} text`, plain: `${e.type} text`,
  // A handoff card's object stays (herdsman reads its presence), emptied; the options keep their count.
  data: Object.fromEntries(Object.entries(e.data).map(([k, x]) => [k, Array.isArray(x) ? x.map(() => `${k} text`)
    : x && typeof x === "object" ? {} : typeof x !== "string" ? x : KEEP.has(k) ? fake(x) : `${k} text`])) });
const list = [...asks.values(), ...ends.values()].sort((a, b) => a.seq - b.seq).map(scrubbed);
fs.writeFileSync(new URL("../fixtures/endings.json", import.meta.url), JSON.stringify({ seq: state.seq, refresh: false, list }, null, 2) + "\n");
console.log("recorded endings");
