// herdsman API v1's shapes the tabs read (herdsman assets/types/api.d.ts), and `parseState`, `parseQueue`, `parseNeeds`,
// `parseEvents`, `parseIssue`, `parseLessons`, `parseDigest`, the contract checks the tabs run on their `/api/v1` reads:
// a field herdsman drops or retypes fails here, loudly, not as a blank tab.
export type Fleet = { name: string; repo: string; color: string; paused: boolean };
export type Queued = { fleet: string; issue: number; title?: string | null; state: string; blocked_by?: number | null };
export type Lane = {
  name: string;
  fleet?: string | null;
  issue?: number | null;
  title?: string | null;
  role: string;
  harness: string;
  model?: string | null;
  state: string;
  shown: string;
  tone: string;
  since: number;
};
export type Ask = {
  id: string;
  fleet: string;
  issue?: number | null;
  question: string;
  options: string[];
  recommend?: number | null;
  ts: number;
};
/** A Host tab row: a host, a service or an alert; `tone` is g, a, r or m. */
export type Row = { name: string; tone: string; detail: string; fix: string };
export type State = {
  seq: number;
  now: number;
  tz: number;
  fleets: Fleet[];
  queue: Queued[];
  lanes: Lane[];
  asks: Ask[];
  hosts: Row[];
  services: Row[];
  alerts: Row[];
};

/** API v1's `queue` (#268), the fields the Queue tab reads: a card per fleet, the ones needing you first. */
export type QueueTab = { now: number; rw: boolean; mtime: string; error?: string | null; fleets: QueueCard[] };
export type QueueCard = {
  name: string;
  k: string;
  serial: string[];
  paths: string[];
  need: number;
  paused?: string | null;
  drive: {
    tone: string;
    line: string;
    meta: string[];
    pass: boolean;
    acts: { who: string; words: string; code: string }[];
  };
  notes: string[];
  rows: QueueRow[];
  done: { issue: number; note: string; age: string; pr?: string | null }[];
  done_n: number;
  backlog?: { issues: Record<string, BacklogIssue>; ago: string } | null;
  config: { repo: string };
  marks: Record<string, Marks>;
};
/** herdsman#576: a queued row's marks by issue: `verify` `on`, `off` or none (by `ui_paths`); `note` `verify(n)` or `verify skipped: …`. */
export type Marks = {
  design: boolean;
  verify?: string | null;
  repro: boolean;
  serial: string[];
  needs: string[];
  note?: string | null;
};
export type QueueRow = {
  issue: number;
  state: string;
  tone: string;
  t: number;
  queued: boolean;
  check: boolean;
  groups: string[];
  blocked_by?: number | null;
  title?: string | null;
  stage: { name: string; k: string; g: string; at: number; ok: boolean }[];
  lane: string;
  live: boolean;
  pr: PrLine;
  dead: boolean;
  notes: string[];
  decided?: string | null;
  ask?: Ask | null;
  open?: { ask: Ask; thread?: string | null; title?: string | null } | null;
  mv: boolean;
  up?: number | null;
  down?: number | null;
  lanes: { name: string; state: string; tone: string; ago: string; link: string }[];
};
export type PrLine = {
  pr?: number | null;
  run?: string | null;
  ci?: string | null;
  verdict?: string | null;
  vtone: string;
  rounds: number;
  form?: string | null;
};
export type BacklogIssue = {
  title: string;
  created: string;
  updated: string;
  labels: string[];
  ac: boolean;
  pr: boolean;
};
/** API v1's `queue` as the Fleets tab reads it: each fleet's colour and [config] sheet (`error`: fleet.toml unread). */
export type FleetsTab = {
  rw: boolean;
  mtime: string;
  error?: string | null;
  fleets: { name: string; k: string; config: ConfigSheet }[];
};
/** #162: a fleet's [config] sheet, the fields the Fleets tab reads; `pipe` the Pipeline group's data (#173). */
export type ConfigSheet = {
  repo: string;
  path: string;
  local: string;
  integration: string;
  effort?: string | null;
  color_set: boolean;
  env: string[];
  review_rounds: number;
  update_rounds: number;
  stall_minutes: number;
  ask_minutes: number;
  max_impl?: number | null;
  max_rev?: number | null;
  line_cap?: number | null;
  serial: string[];
  deny: string[];
  required: string[];
  merge_label?: string | null;
  notify: string[];
  global: string[];
  builtin: boolean;
  pipe: Pipe;
  models: string[];
  harnesses: { name: string; efforts: string[] }[];
  roles: RoleRow[];
  conductor?: Conductor | null;
};
export type Pipe = {
  into: string;
  builtin: boolean;
  queued: number;
  parked: number;
  checks: string[];
  required: string[];
  bad: { rule: string; msg: string; stage?: string | null; line: number }[];
  stages: {
    name: string;
    kind: string;
    role?: string | null;
    run?: string | null;
    rounds?: number | null;
    here: number;
    loop: string;
    next: Record<string, string>;
  }[];
};
export type RoleRow = {
  name: string;
  profile: string;
  harness: string;
  model?: string | null;
  effort?: string | null;
  efforts: string[];
  model_default: string;
  effort_default: string;
};
export type Conductor = {
  default: boolean;
  profile: string;
  harness: string;
  model?: string | null;
  effort?: string | null;
  efforts: string[];
  models: string[];
};
/** #267: API v1's `host`, the parts the Fleets tab reads: the hosts' [off]/[on] for lanes, fleet disk, notifiers. */
export type HostTab = {
  rw: boolean;
  live: boolean;
  mtime: string;
  hosts: HostCard[];
  disk?: Disk | null;
  orphans: number;
  leftovers?: { n: number; mb: string } | null;
  notify: { kind: string; on: string }[];
};
export type HostCard = { name: string; main: boolean; tone: string; words: string; to?: string | null; lanes: number };
export type Disk = { bar: Bar; gb: string; budget: number; ago?: string | null };
export type Bar = { t: string; n: number; pct: string };
/** #269: API v1's `needs`, the needs-you cards: open asks, blocked lanes' cards and `needs-human` issues. */
export type NeedsTab = {
  now: number;
  rw: boolean;
  live: boolean;
  paired: boolean;
  pair: boolean;
  mtime: string;
  snap: number;
  choosing: number;
  asks: NeedAsk[];
  blocked: Blocked[];
  issues: NeedIssue[];
};
/** An ask's card: `who` (fleet · #issue · agent · age), its `kind` line, imported `from`; #412 a design's `pngs`. */
export type NeedAsk = {
  ask: Ask;
  who: string;
  kind: string;
  from?: string | null;
  pngs: string[];
  thread?: string | null;
  title?: string | null;
};
/** A blocked lane's card: unpaired, its `activity`; else a harness `menu`, or the pane's last lines (`tail`). */
export type Blocked = {
  name: string;
  who: string;
  push: boolean;
  activity?: string | null;
  menu?: Menu | null;
  tail?: string | null;
};
/** #127: a harness menu: `countdown` (s) before it auto-denies, `h` the hash of the menu the card shows. */
export type Menu = {
  title: string;
  reason: string;
  command: string;
  countdown?: number | null;
  question: string;
  options: string[];
  h: string;
};
/** #130: a `needs-human` issue (`pending`: one more round recorded, no lane yet); #417 the verdict's `shots`. */
export type NeedIssue = {
  fleet: string;
  issue: number;
  pending: boolean;
  who: string;
  title: string;
  why: string;
  pass?: string | null;
  stale: boolean;
  fail: string;
  pr: PrLine;
  repo?: string | null;
  mergeable: string;
  merge: boolean;
  shots: string[];
};
/** #259 #266: API v1's `events?since=N&until=M`: the events after N (up to M) in `list`; over 500, none and `refresh`.
 * Each as the Events tab says it: `glyph` in `tone`, `text`, `imp` (shown under [important]). */
export type Events = { seq: number; refresh: boolean; list: Ev[] };
export type Ev = {
  seq: number;
  ts: number;
  fleet: string;
  issue?: number | null;
  type: string;
  data: Record<string, unknown>;
  plain: string;
  glyph: string;
  tone: string;
  text: string;
  imp: boolean;
};
/** herdsman#576: API v1's `issue/<fleet>/<n>`: its verdicts newest first; the newest one's screenshot `shots` (`/shot/…/<i>`). */
export type IssueView = { fleet: string; issue: number; verdicts: Verdict[] };
export type Verdict = {
  role: string;
  round: number;
  sha: string;
  verdict: string;
  ts: number;
  findings: string;
  shots: string[];
  repro?: Repro | null;
};
/** herdsman#420: a `--repro` PASS's `Repro:` block: `kind` `run` (both runs fenced) or `verify` (a `>` quoted reason), its lines `text`. */
export type Repro = { kind: string; text: string };
/** herdsman#576: API v1's `lessons`: per fleet its `lessons` (`weekly`, `off`), the `last` run (its issue, spawn `ts`, the issue's `state`), the `next` due (null: off). */
export type Lessons = {
  fleets: {
    fleet: string;
    lessons: string;
    last?: { issue: number; ts: number; state: string } | null;
    next?: number | null;
  }[];
};
/** herdsman#576: `lessons/<fleet>/digest?days=N`: `herdsman lesson digest`'s text. */
export type Digest = { fleet: string; days: number; text: string };
/** A POST's answer: `code` OK, a `REFUSED` code or ERROR; `message` the line to show. */
export type Reply = { ok: boolean; code: string; message: string };

/**
 * A contract: a kind (`string`, `number`, `boolean`, `?` nullable), `[spec]` a list, `{field: spec}` an object
 * (with `"?": ""` nullable, `"*"` every value of a map).
 */
type Spec = string | Spec[] | { [k: string]: Spec };
const ASK = {
  id: "string",
  fleet: "string",
  issue: "number?",
  question: "string",
  options: ["string"],
  recommend: "number?",
  ts: "number",
};
const ROW = { name: "string", tone: "string", detail: "string", fix: "string" };
const STATE = {
  seq: "number",
  now: "number",
  tz: "number",
  fleets: [{ name: "string", repo: "string", color: "string", paused: "boolean" }],
  queue: [{ fleet: "string", issue: "number", title: "string?", state: "string", blocked_by: "number?" }],
  lanes: [
    {
      name: "string",
      fleet: "string?",
      issue: "number?",
      title: "string?",
      role: "string",
      harness: "string",
      model: "string?",
      state: "string",
      shown: "string",
      tone: "string",
      since: "number",
    },
  ],
  asks: [ASK],
  hosts: [ROW],
  services: [ROW],
  alerts: [ROW],
};
const ROLE = {
  name: "string",
  profile: "string",
  harness: "string",
  model: "string?",
  effort: "string?",
  efforts: ["string"],
  model_default: "string",
  effort_default: "string",
};
const CONFIG = {
  repo: "string",
  path: "string",
  local: "string",
  integration: "string",
  effort: "string?",
  color_set: "boolean",
  env: ["string"],
  review_rounds: "number",
  update_rounds: "number",
  stall_minutes: "number",
  ask_minutes: "number",
  max_impl: "number?",
  max_rev: "number?",
  line_cap: "number?",
  serial: ["string"],
  deny: ["string"],
  required: ["string"],
  merge_label: "string?",
  notify: ["string"],
  global: ["string"],
  builtin: "boolean",
  models: ["string"],
  harnesses: [{ name: "string", efforts: ["string"] }],
  roles: [ROLE],
  conductor: {
    "?": "",
    default: "boolean",
    profile: "string",
    harness: "string",
    model: "string?",
    effort: "string?",
    efforts: ["string"],
    models: ["string"],
  },
  pipe: {
    into: "string",
    builtin: "boolean",
    queued: "number",
    parked: "number",
    checks: ["string"],
    required: ["string"],
    bad: [{ rule: "string", msg: "string", stage: "string?", line: "number" }],
    stages: [
      {
        name: "string",
        kind: "string",
        role: "string?",
        run: "string?",
        rounds: "number?",
        here: "number",
        loop: "string",
        next: { "*": "string" },
      },
    ],
  },
};
const PR = {
  pr: "number?",
  run: "string?",
  ci: "string?",
  verdict: "string?",
  vtone: "string",
  rounds: "number",
  form: "string?",
};
const NEEDS = {
  now: "number",
  rw: "boolean",
  live: "boolean",
  paired: "boolean",
  pair: "boolean",
  mtime: "string",
  snap: "number",
  choosing: "number",
  asks: [
    { ask: ASK, who: "string", kind: "string", from: "string?", pngs: ["string"], thread: "string?", title: "string?" },
  ],
  blocked: [
    {
      name: "string",
      who: "string",
      push: "boolean",
      activity: "string?",
      tail: "string?",
      menu: {
        "?": "",
        title: "string",
        reason: "string",
        command: "string",
        countdown: "number?",
        question: "string",
        options: ["string"],
        h: "string",
      },
    },
  ],
  issues: [
    {
      fleet: "string",
      issue: "number",
      pending: "boolean",
      who: "string",
      title: "string",
      why: "string",
      pass: "string?",
      stale: "boolean",
      fail: "string",
      pr: PR,
      repo: "string?",
      mergeable: "string",
      merge: "boolean",
      shots: ["string"],
    },
  ],
};
const EVENTS = {
  seq: "number",
  refresh: "boolean",
  list: [
    {
      seq: "number",
      ts: "number",
      fleet: "string",
      issue: "number?",
      type: "string",
      data: {},
      plain: "string",
      glyph: "string",
      tone: "string",
      text: "string",
      imp: "boolean",
    },
  ],
};
const QUEUE = {
  now: "number",
  rw: "boolean",
  mtime: "string",
  error: "string?",
  fleets: [
    {
      name: "string",
      k: "string",
      serial: ["string"],
      paths: ["string"],
      need: "number",
      paused: "string?",
      drive: {
        tone: "string",
        line: "string",
        meta: ["string"],
        pass: "boolean",
        acts: [{ who: "string", words: "string", code: "string" }],
      },
      notes: ["string"],
      done: [{ issue: "number", note: "string", age: "string", pr: "string?" }],
      done_n: "number",
      backlog: {
        "?": "",
        ago: "string",
        issues: {
          "*": {
            title: "string",
            created: "string",
            updated: "string",
            labels: ["string"],
            ac: "boolean",
            pr: "boolean",
          },
        },
      },
      config: { repo: "string" },
      marks: {
        "*": {
          design: "boolean",
          verify: "string?",
          repro: "boolean",
          serial: ["string"],
          needs: ["string"],
          note: "string?",
        },
      },
      rows: [
        {
          issue: "number",
          state: "string",
          tone: "string",
          t: "number",
          queued: "boolean",
          check: "boolean",
          groups: ["string"],
          blocked_by: "number?",
          title: "string?",
          stage: [{ name: "string", k: "string", g: "string", at: "number", ok: "boolean" }],
          lane: "string",
          live: "boolean",
          dead: "boolean",
          notes: ["string"],
          decided: "string?",
          mv: "boolean",
          up: "number?",
          down: "number?",
          pr: PR,
          ask: { "?": "", ...ASK },
          open: { "?": "", ask: ASK, thread: "string?", title: "string?" },
          lanes: [{ name: "string", state: "string", tone: "string", ago: "string", link: "string" }],
        },
      ],
    },
  ],
};

const FLEETS = {
  rw: "boolean",
  mtime: "string",
  error: "string?",
  fleets: [{ name: "string", k: "string", config: CONFIG }],
};
const BAR = { t: "string", n: "number", pct: "string" };
const HOST = {
  rw: "boolean",
  live: "boolean",
  mtime: "string",
  orphans: "number",
  leftovers: { "?": "", n: "number", mb: "string" },
  hosts: [{ name: "string", main: "boolean", tone: "string", words: "string", to: "string?", lanes: "number" }],
  disk: { "?": "", bar: BAR, gb: "string", budget: "number", ago: "string?" },
  notify: [{ kind: "string", on: "string" }],
};
const ISSUE = {
  fleet: "string",
  issue: "number",
  verdicts: [
    {
      role: "string",
      round: "number",
      sha: "string",
      verdict: "string",
      ts: "number",
      findings: "string",
      shots: ["string"],
      repro: { "?": "", kind: "string", text: "string" },
    },
  ],
};
const LESSONS = {
  fleets: [
    {
      fleet: "string",
      lessons: "string",
      last: { "?": "", issue: "number", ts: "number", state: "string" },
      next: "number?",
    },
  ],
};
const DIGEST = { fleet: "string", days: "number", text: "string" };

function fits(v: unknown, s: Spec, at: string): string | null {
  if (typeof s === "string")
    return (s.endsWith("?") ? v == null || typeof v === s.slice(0, -1) : typeof v === s) ? null : at;
  if (Array.isArray(s)) {
    const [el] = s;
    return Array.isArray(v) && el !== undefined
      ? v.reduce((e: string | null, x, i) => e ?? fits(x, el, `${at}[${i}]`), null)
      : at;
  }
  if (v == null && "?" in s) return null;
  if (typeof v !== "object" || v === null || Array.isArray(v)) return at || "the top";
  const o = v as Record<string, unknown>,
    dot = (k: string) => (at ? `${at}.${k}` : k);
  const all = s["*"];
  if (all !== undefined) return Object.keys(o).reduce((e: string | null, k) => e ?? fits(o[k], all, dot(k)), null);
  return Object.entries(s).reduce(
    (e: string | null, [k, sk]) => e ?? (k === "?" ? null : fits(o[k], sk, dot(k))),
    null,
  );
}

/** `json` as herdsman's `/api/v1/<path>` answer, or an error naming the first field that breaks the contract. */
function parse<T>(path: string, spec: Spec, json: unknown): T {
  const at = fits(json, spec, "");
  if (at != null) throw new Error(`herdsman /api/v1/${path} broke the contract at ${at}`);
  return json as T;
}

export const parseState = (json: unknown): State => parse("state", STATE, json);
export const parseQueue = (json: unknown): QueueTab => parse("queue", QUEUE, json);
export const parseFleets = (json: unknown): FleetsTab => parse("queue", FLEETS, json);
export const parseHost = (json: unknown): HostTab => parse("host", HOST, json);
export const parseNeeds = (json: unknown): NeedsTab => parse("needs", NEEDS, json);
export const parseIssue = (json: unknown): IssueView => parse("issue", ISSUE, json);
export const parseEvents = (json: unknown): Events => parse("events", EVENTS, json);
export const parseLessons = (json: unknown): Lessons => parse("lessons", LESSONS, json);
export const parseDigest = (json: unknown): Digest => parse("lessons/<fleet>/digest", DIGEST, json);
