// Generated from the Rust payload types by `just types` (#375); never edit by hand.

/**
 * `/herd.json` (Herd mode, docs/serve.md).
 */
export type Herd = { fleets: Array<HerdFleet>, others: Array<HerdLane>, host: HerdHost, };

export type HerdFleet = { name: string, color: string, lanes: Array<HerdLane>, queue: Array<HerdQueued>, needs: Array<number>, };

export type HerdLane = { name: string, act: string, issue?: number | null, title?: string | null, role: string, harness: string, model?: string | null, effort?: string | null, account?: string | null, host?: string | null, state: string, shown: string, tone: string, age: string, coat: string, gone: boolean, };

export type HerdQueued = { issue: number, title?: string | null, blocked_by?: number | null, coat: string, };

export type HerdHost = { load?: number | null, limit: number, pressure?: string | null, swapouts?: number | null, swap_limit: number, gate?: string | null, };

/**
 * `/lane?agent=`: the sheet's chat. A lane with no pane sends no `said`, `notices`, `acts`, `pane` or `status`.
 */
export type Chat = { said?: Array<JsonValue> | null, older: boolean, notices: Array<Notice>, harness: string, acts: Array<string>, pane?: string | null, lines: Array<string>, live: number, status?: string | null, before: number, more: boolean, };

export type Notice = { at: number, text: string, };

/**
 * `/live`'s `ev`: an event's seq, time and theme moment (`serve::moment`); #370 its fleet and issue, for row moments.
 */
export type Moment = { seq: number, ts: number, k?: string | null, f: string, n?: number | null, };

/**
 * `/live`'s `frag`: a fragment's new HTML.
 */
export type Frag = { id: string, html: string, };

/**
 * #351: `/live`'s `patch`: a `frag` as length `n`, common prefix `p` and the changed pieces by index.
 */
export type Patch = { id: string, n: number, p: number, set: { [key in string]: string }, };

/**
 * #365: one open issue of the Queue tab's backlog (`data-bl`, by number), as `gh::backlog` reads it.
 */
export type BacklogIssue = { number: number, title: string, state: string, created: string, updated: string, labels: Array<string>, ac: boolean, pr: boolean, };

/**
 * `POST /send`: a reply to lane `agent`, or to the setup pane `setup`.
 */
export type Send = { agent: string, setup?: string | null, text: string, };

/**
 * `POST /answer`: menu option `choice` of the menu hashed `h`.
 */
export type Answer = { agent: string, choice: string, h: string, };

/**
 * `POST /act`: stop | kill | clean | close; `confirm` `1` does what a dry run showed.
 */
export type Act = { agent: string, op: string, confirm: string, };

/**
 * `POST /setup`: start the setup pane `name` on `host`.
 */
export type Setup = { host: string, name: string, };

/**
 * #265: API v1's `state`, the board in one read: the newest event's `seq`, the step's snapshot age (none saved: null) and the
 * `mtime` a POST sends back (STALE when fleet.toml changed since).
 */
export type State = { seq: number, age?: number | null, mtime: string, theme: string, fleets: Array<Fleet>, queue: Array<Queued>, lanes: Array<Lane>, asks: Array<Ask>, hosts: Array<Row>, services: Array<Row>, alerts: Array<Row>, };

export type Fleet = { name: string, color: string, paused: boolean, };

export type Queued = { fleet: string, issue: number, title?: string | null, state: string, blocked_by?: number | null, };

/**
 * `state` is herdr's (or `dead`, `parked`); `shown` and `tone` (g, a, r, m) are the Lanes row's words.
 */
export type Lane = { name: string, fleet?: string | null, issue?: number | null, title?: string | null, role: string, harness: string, model?: string | null, state: string, shown: string, tone: string, since: number, };

export type Ask = { id: string, fleet: string, issue?: number | null, question: string, options: Array<string>, recommend?: number | null, ts: number, };

/**
 * A Host tab row: HOSTS' (`detail`: its state in words), SERVICES' and ALERTS' (push); `fix` clears a red or amber one.
 */
export type Row = { name: string, tone: string, detail: string, fix: string, };

/**
 * #259's contract: the events after `since` (`list`); over 500, none and `refresh` (read `state` again).
 */
export type Events = { seq: number, refresh: boolean, list: Array<Ev>, };

/**
 * One logged event; `k` is the theme moment it plays (merged, fail, needs-you).
 */
export type Ev = { seq: number, ts: number, fleet: string, issue?: number | null, type: string, data: { [key in string]: JsonValue }, k?: string | null, };

/**
 * A POST's answer: `code` is OK, a `REFUSED` code, or ERROR; `message` the line the board toasts.
 */
export type Reply = { ok: boolean, code: string, message: string, };

export type Event = { 
/**
 * #259: the log's monotonic number, set on append (an old row's is its line number); 0 before.
 */
seq?: number, ts: number, fleet: string, issue?: number | null, type: string, } & ({ [key in string]: number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null });

export type IssueState = { fleet: string, issue: number, state: string, lane?: string | null, branch?: string | null, 
/**
 * The latest lane's reasoning effort, as spawned.
 */
effort?: string | null, rounds: number, 
/**
 * Extra rounds the owner allowed (`round-extended`, #130); #309: the latest grants the round after any stage's.
 */
extra: number, granted: number, 
/**
 * #123: passed `--update` rounds, apart from `rounds`; `updating` while one is open (state `fix(n)+uk`).
 */
updates: number, updating: boolean, last_ts: number, last_event: string, 
/**
 * Latest agent per role (impl, rev).
 */
lanes: { [key in string]: string }, 
/**
 * (sha, PASS|FAIL, the check stage whose rounds it counts) in log order; logs before #173: `rev`.
 */
verdicts: Array<[string, string, string]>, 
/**
 * #173: the pipeline stage of the latest spawn, the latest check and build roles, review rounds per check stage.
 */
stage: string, check?: string | null, build?: string | null, stage_rounds: { [key in string]: number }, 
/**
 * #228: `FAIL (form-only)` verdicts that did not count toward the rounds.
 */
form: number, };

export type Waiting = { fleet: string, issue: number, 
/**
 * The lane this row is for; every lane of an issue gets one.
 */
lane: string, 
/**
 * drive | owner | ci | none
 */
who: string, what: string, since: number, next_cmd?: string | null, };

/**
 * What the last look at a host saw (`<state dir>/hosts.json`): detected tags and `$HOME` (probed hourly), reachability,
 * load/cpu, free memory and disk in GB.
 */
export type HostSeen = { tags: Array<string>, home: string, probed: number, seen: number, up: boolean, load?: number | null, mem?: number | null, disk?: number | null, 
/**
 * #235: a remote host's failed checks from `health` (`✗` keeps lanes off it, `!` warns), with the probe.
 */
health: Array<string>, 
/**
 * #234: memory pressure 0 normal, 1 warn, 2 critical; swap-out pages/2s; the top memory users (remote hosts).
 */
pressure?: number | null, swap?: number | null, top: Array<string>, 
/**
 * #234: memory and disk size in GB (a bar shows the used part).
 */
mem_total?: number | null, disk_total?: number | null, 
/**
 * #254: when it last had a live agent (a cloud host's idle stop).
 */
busy: number, 
/**
 * #255: a cloud host's gh token, as its card says it.
 */
github: string, };

/**
 * #234: one row a minute per host in `<state dir>/host-history.jsonl`, written by the fast step; 24 h kept.
 */
export type HostRow = { ts: number, host: string, load_cpu?: number | null, pressure?: number | null, swap_out?: number | null, mem_free_gb?: number | null, disk_free_gb?: number | null, lanes: number, 
/**
 * #352: MB per `host::groups` group, only while `/live?tab=host` was seen in the last 2 min (else no `ps` walk).
 */
procs?: [number, number, number, number, number] | null, };

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
