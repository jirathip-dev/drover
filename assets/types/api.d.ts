// Generated from the Rust payload types by `just types`; never edit by hand.

/**
 * API v1's `GET herd` (docs/serve.md).
 */
export type Herd = { fleets: Array<HerdFleet>, others: Array<HerdLane>, host: HerdHost, };

export type HerdFleet = { name: string, color: string, lanes: Array<HerdLane>, queue: Array<HerdQueued>, needs: Array<number>, };

/**
 * A lane in the Herd; `grow`, its issue's growth stage (a theme's `stages`).
 */
export type HerdLane = { name: string, act: string, grow: string, issue?: number | null, title?: string | null, role: string, harness: string, model?: string | null, effort?: string | null, account?: string | null, host?: string | null, runtime: string, where: string, state: string, shown: string, tone: string, age: string, coat: string, gone: boolean, };

export type HerdQueued = { issue: number, title?: string | null, blocked_by?: number | null, coat: string, };

export type HerdHost = { load?: number | null, limit: number, pressure?: string | null, swapouts?: number | null, swap_limit: number, gate?: string | null, };

/**
 * `/lane?agent=`: the sheet's chat. A lane with no pane sends no `said`, `notices`, `acts`, `pane` or `status`.
 */
export type Chat = { said?: Array<JsonValue> | null, older: boolean, notices: Array<Notice>, harness: string, acts: Array<string>, pane?: string | null, lines: Array<string>, live: number, status?: string | null, before: number, more: boolean, };

export type Notice = { at: number, text: string, };

/**
 * API v1's `lane/<agent>`: the sheet's chat plus `blocks`, its transcript as blocks: an ACP lane's from shed's events (no `lines`; `before` is the oldest event's `seq`, the next page's `before`), else one `screen` block (the screen is `lines`).
 */
export type LaneChat = { blocks: Array<Block>, said?: Array<JsonValue> | null, older: boolean, notices: Array<Notice>, harness: string, acts: Array<string>, pane?: string | null, lines: Array<string>, live: number, status?: string | null, before: number, more: boolean, };

/**
 * One of an approval's options, as ACP sent it (`allow_once`, `reject_once`, …).
 */
export type ApprovalChoice = { id: string, name: string, kind: string, };

/**
 * One block of a lane's transcript, tagged `k`, built from ACP's own fields only (never an agent's wording): a prompt, a reply's markdown (its text chunks joined), a tool call (its updates folded into one; `title_line`, the title's first line, cut), the plan, an approval (`answered`: the option taken); a PTY lane's screen is one `screen`, its `lines`.
 */
export type Block = { "k": "prompt", text: string, } | { "k": "reply", markdown: string, } | { "k": "tool", id: string, kind?: string | null, title_line: string, title: string, status: string, } | { "k": "plan", entries: Array<JsonValue>, } | { "k": "approval", tool: string, options: Array<ApprovalChoice>, answered?: string | null, } | { "k": "screen" };

/**
 * `/live`'s `ev`: an event's seq, time and theme moment (`serve::moment`); its fleet and issue, for row moments.
 */
export type Moment = { seq: number, ts: number, k?: string | null, f: string, n?: number | null, };

/**
 * API v1's `needs`, the cards above every tab as this device sees them: the snapshot's time `snap` (0: none; the strip under the header shows its age once over 2 min), the open asks, each blocked lane's card, the `needs-human` issues, and `choosing` (`[chat·N]`). `rw`: the board takes forms (the answer boxes); `live`: and is no export; `paired`: the device may answer a lane or an issue; `pair`: the page offers Pair this phone.
 */
export type NeedsTab = { now: number, rw: boolean, live: boolean, paired: boolean, pair: boolean, mtime: string, snap: number, choosing: number, asks: Array<NeedAsk>, blocked: Array<Blocked>, issues: Array<NeedIssue>, 
/**
 * Fleet.toml fails to load: `config invalid: <error> (fleet.toml:<line>)`, the red banner above every tab.
 */
config?: string | null, };

/**
 * An open ask's card: `who` (fleet · #issue · agent · age), its `kind` line, the host it was imported `from`; a design's PNGs by name (`/design/<fleet>/<n>/<i>` serves the `i`th); the `thread` it was asked in, its `title`.
 */
export type NeedAsk = { ask: Ask, who: string, kind: string, from?: string | null, pngs: Array<string>, thread?: string | null, title?: string | null, };

/**
 * A blocked lane's card (`who`: fleet · #issue · lane; `push` 0 while a countdown runs): unpaired, its `activity`; else a harness `menu`, or the pane's last lines (`tail`).
 */
export type Blocked = { name: string, who: string, push: boolean, activity?: string | null, menu?: Menu | null, tail?: string | null, };

/**
 * A harness menu: its title, reason and command, the `countdown` (s) before it auto-denies, the masked question and options, `h` the hash of the menu the card shows (`POST /answer`).
 */
export type Menu = { title: string, reason: string, command: string, countdown?: number | null, question: string, options: Array<string>, h: string, };

/**
 * A `needs-human` issue's card (`pending`: one more round recorded, no lane yet): `who`, title, `why`, the last verdict (`pass`: its sha, `stale` not the head; else `fail`'s finding), the PR line (`repo`: none, the fleet is not configured), `mergeable` and `merge` (merge anyway is offered); the verdict's screenshot `shots`.
 */
export type NeedIssue = { fleet: string, issue: number, pending: boolean, who: string, title: string, why: string, pass?: string | null, stale: boolean, fail: string, pr: PrLine, repo?: string | null, mergeable: string, merge: boolean, shots: Array<string>, 
/**
 * The sha of the verdict `shots` are from, the round `/shot/<fleet>/<n>/<i>/<sha>` names.
 */
shots_at?: string | null, };

export type LanesTab = { now: number, rw: boolean, live: boolean, paired: boolean, pair: boolean, error?: string | null, failed: boolean, sections: Array<LaneSection>, 
/**
 * Each lane's workspace root and unsaved work.
 */
unsaved: Array<LaneUnsaved>, 
/**
 * Fleet.toml's mtime, for a row's [unpark] form.
 */
mtime: string, 
/**
 * A paired device's folded `Other in shed` rows (API v1's `shed`).
 */
shed: Array<ShedRow>, };

/**
 * A state section (`tone`) and its repo groups.
 */
export type LaneSection = { name: string, tone: string, repos: Array<LaneRepo>, };

export type LaneRepo = { name: string, lanes: Array<LaneRow>, };

export type LaneRow = { name: string, role: string, state: string, act: string, grow: string, atone: string, coat: string, tone: string, ago: string, why?: string | null, why_tone: string, activity: string, now?: LaneNow | null, gone: boolean, thread?: string | null, head: string, shown: string, facts: string, issue?: number | null, pr?: PrLine | null, repo?: string | null, dead: boolean, blocked?: Blocked | null, 
/**
 * Its issue is parked: the fleet its [unpark] posts to.
 */
unpark?: string | null, runtime: string, host: string, where: string, };

/**
 * Lane `name`'s workspace root, and its worktree's uncommitted files and unpushed commits.
 */
export type LaneUnsaved = { name: string, root?: string | null, dirty?: number | null, unpushed?: number | null, };

/**
 * A `now:` line: the spinner word `w`, the action `n`, the elapsed time `t`, the full text as `title`.
 */
export type LaneNow = { title: string, w?: string | null, n?: string | null, t?: string | null, };

/**
 * API v1's `shed` entry: a session no lane or conductor names, or (`error` alone) its host's shed failing.
 */
export type ShedRow = { host: string, error?: string | null, name?: string | null, runtime?: string | null, labels?: { [key in string]: string } | null, state?: string | null, last?: string | null, age?: string | null, };

/**
 * API v1's `chat`, the [chat] tab: a card per repo (`all` first) with its threads, then every thread's view (`error`: fleet.toml unread). `rw`: a paired device on the live board writes.
 */
export type ChatTab = { now: number, rw: boolean, error?: string | null, repos: Array<ChatRepo>, threads: Array<ThreadView>, };

/**
 * A repo's card in colour `k`: its threads newest first, the archived ones apart; its empty thread's `harness` chip.
 */
export type ChatRepo = { repo: string, k: string, rows: Array<ThreadRow>, archived: Array<ThreadRow>, harness: Chip, };

/**
 * A composer chip: `name`'s current value `cur` (none: the default) and its choices `all`.
 */
export type Chip = { name: string, cur?: string | null, all: Array<string>, };

/**
 * A thread's row: unread, title, age, its `state` (choose, working, asleep, …), the threads it links to, the last line; the ids of the config proposals `waiting` for the owner.
 */
export type ThreadRow = { id: string, title: string, age: string, unread: boolean, state: string, waiting: Array<string>, links: Array<Link>, line: string, };

/**
 * A linked thread's chip: `from t5`, `continued in t6`.
 */
export type Link = { w: string, t: string, };

/**
 * One thread: its bar (`ds`: fleet · harness · model · state), its last messages, the live line's `state`; working, the turn's start `since`, and the `queued` messages above the box; the config proposals `waiting`; its model and effort `chips`.
 */
export type ThreadView = { id: string, title: string, ds: string, state: string, waiting: Array<string>, since?: number | null, queued: Array<QueuedMsg>, chips: Array<Chip>, page: ThreadPage, };

/**
 * A message queued for the turn's end, by its id `q`.
 */
export type QueuedMsg = { q: number, text: string, };

/**
 * A page of a thread's messages; `older`: the seq to load the messages before, else the `started` line's time and conductor folder `dir`; the thread's `id`, and whether it is `archived` (opened read-only, [restore]).
 */
export type ThreadPage = { id: string, archived: boolean, msgs: Array<Msg>, older?: number | null, started: number, dir: string, };

/**
 * A thread's message (its event's `seq` and `ts`) by `kind`, `text` its words: `note` (a system line: `retry`, `signin` on `host`; a conductor's answer, its `ask` and `open` while the owner may [change] it), `handoff` (the title), `msg` (`from` who wrote it, its `files`), `cmd`, `ask` (the question; `answer` once decided, else `open`), `config` (the diff, `from` the proposal's id; `open` unless stale), `secret` (why, `from` its name), `resumed` (the session), `sleep`, `fold` (a finished turn's run of commands, `cmds`); `link` the thread it links to; `run` (a run card, `from` its id).
 */
export type Msg = { seq: number, ts: number, kind: string, text: string, link?: Link | null, from: string, files: Array<FileRef>, retry: boolean, signin?: string | null, host: string, ask?: Ask | null, routine: boolean, answer?: Answered | null, open: boolean, cmds: Array<string>, handoff?: Handoff | null, run?: RunCard | null, };

export type FileRef = { name: string, size: number, };

/**
 * A decided ask: the option `n` chosen (none: an "other" answer), its text, by whom, when.
 */
export type Answered = { n?: number | null, choice: string, by?: string | null, ts: number, };

/**
 * A handoff as one agent→agent item (cooee): `id` its seq (a repo handoff's: its card's), `from` the sending conductor, `to` the thread id once delivered, else `{repo, pending: true}`; `summary` the text's first line; `state` pending, delivered, failed (`error`) or forwarded; `folded` in the sender's thread, the message form in the receiver's; while pending, the card's `choices`.
 */
export type Handoff = { kind: string, id: number, from: HandoffFrom, to: JsonValue, title: string, summary: string, text: string, state: string, error?: string | null, folded: boolean, choices: Array<HandoffChoice>, };

export type HandoffFrom = { agent: string, thread: string, repo: string, };

/**
 * A pending handoff's row as the board's card shows it: option `n`, its `text`, the `thread` it delivers to (`new`: a new thread); picked by the paired `POST /api/v1/<op>` with `fleet`, `id`, `choice` = `n` and `mtime`.
 */
export type HandoffChoice = { n: number, text: string, thread: string, op: string, fleet: string, id: string, };

/**
 * API v1's `repo/<fleet>/threads`: the repo's conductor threads, newest-active first.
 */
export type RepoThreads = { repo: string, threads: Array<RepoThread>, };

/**
 * A conductor thread: its `last` line and time, `awake` (its conductor not asleep), `newest` on the ongoing one (the first not archived).
 */
export type RepoThread = { id: string, title: string, last: LastLine, unread: boolean, awake: boolean, archived: boolean, newest: boolean, };

export type LastLine = { line: string, ts: number, };

/**
 * A run card as its thread shows it: what it runs, why, what it touches, where, its sha; open until `exp`, then `state` (`open`, `running`, `approved`, `denied`, `expired`) with `by`, the exit `code` and the output `tail`.
 */
export type RunCard = { id: string, cmd: string, why: string, blast: string, host: string, dir: string, sha: string, exp: number, state: string, by?: string | null, code?: number | null, tail: string, };

/**
 * One open issue of the Queue tab's backlog (`data-bl`, by number), as `gh::backlog` reads it.
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
 * API v1's `state`, the board in one read: the newest event's `seq`, the step's snapshot age (none saved: null) and the `mtime` a POST sends back (STALE when fleet.toml changed since). the server's clock `now` and UTC offset `tz` (s), the Events tab's time groups.
 */
export type State = { seq: number, now: number, tz: number, age?: number | null, mtime: string, theme: string, fleets: Array<Fleet>, queue: Array<Queued>, lanes: Array<Lane>, asks: Array<Ask>, hosts: Array<Row>, services: Array<Row>, alerts: Array<Row>, 
/**
 * When the background last read each source the lanes come from (shed, remote, host).
 */
sources: { [key in string]: number }, };

export type Fleet = { name: string, repo: string, color: string, paused: boolean, };

export type Queued = { fleet: string, issue: number, title?: string | null, state: string, blocked_by?: number | null, 
/**
 * Its blockers in another fleet or repo, as `owner/repo#N`.
 */
blocked_on: Array<string>, 
/**
 * A tapped start waiting for its lane, and the row's `starting, waiting for host: <reason> · <age>`.
 */
pending?: PendingStart | null, status?: string | null, };

/**
 * A card tap's lane start not begun yet: its `action` (extend, answer, approve, revise), since when, the last refusal `reason` (a code; a hard one with its words).
 */
export type PendingStart = { action: string, since: number, reason?: string | null, };

/**
 * `state` is the runtime's (or `dead`, `parked`); `shown` and `tone` (g, a, r, m) are the Lanes row's words.
 */
export type Lane = { name: string, fleet?: string | null, issue?: number | null, title?: string | null, role: string, harness: string, model?: string | null, state: string, shown: string, tone: string, since: number, runtime: string, host: string, where: string, };

export type Ask = { id: string, fleet: string, issue?: number | null, question: string, options: Array<string>, recommend?: number | null, ts: number, };

/**
 * A Host tab row: HOSTS' (`detail`: its state in words), SERVICES' and ALERTS' (push); `fix` clears a red or amber one.
 */
export type Row = { name: string, tone: string, detail: string, fix: string, };

/**
 * SERVICES (`services`, `theme`: `detail` its error, `deepseek`), FLEETS and ALERTS (`push`, and on a live
 */
export type HostTab = { now: number, rw: boolean, live: boolean, mtime: string, hosts: Array<HostCard>, disk?: Disk | null, orphans: number, leftovers?: Leftovers | null, spawns?: Row | null, services: Array<Row>, theme: Row, deepseek?: Balance | null, fleets: Array<FleetRow>, push: Row, key: string, subs: Array<Sub>, devices: Array<Paired>, notify: Array<Notifier>, vendors: Vendors, };

/**
 * A host's card: its dot and state `words` in `tone`, the lanes' 24 h series `n`, its rows, `to` the [off]/[on] its head offers (with the `lanes` running on it), and the logins its [sign in ▾] lists.
 */
export type HostCard = { name: string, main: boolean, dot: string, tone: string, words: string, n: string, rows: Array<Cell>, to?: string | null, lanes: number, setups: Array<string>, };

/**
 * A host card's row, status `t`: a gauge's `bar` then its value `b` (bold, in `bt`), `text`, the limit `of` and muted `notes`. `key`: a metric with history: its 24 h sparkline `s` (memory: pressure's `p`), its limit `l` and, in an export, its chart `x` inline.
 */
export type Cell = { k: string, t: string, key: string, bar?: Bar | null, b?: string | null, bt: string, text: string, of: string, notes: Array<string>, s: string, p: string, l?: number | null, x: string, };

/**
 * A 12-cell text gauge: its status `t`, `n` cells full, and how far past the limit (`pct`, else empty).
 */
export type Bar = { t: string, n: number, pct: string, };

/**
 * The worktree total (`gb`, `?` unread) against the budget, and how long ago it was walked.
 */
export type Disk = { bar: Bar, gb: string, budget: number, ago?: string | null, };

/**
 * Processes still running in closed lanes' worktrees, and the memory they hold.
 */
export type Leftovers = { n: number, mb: string, };

/**
 * The `DeepSeek` balance (currency and amount) and whether it is `low`, else the `hint` why none was read.
 */
export type Balance = { balance?: string | null, low: boolean, hint: string, };

/**
 * A fleet's last `driven` result in words (`word` in `tone`, the plain rest `text`), its age and `raw` line.
 */
export type FleetRow = { name: string, tone: string, word: string, text: string, ago: string, raw?: string | null, };

/**
 * A webpush subscription, removable by `id`.
 */
export type Sub = { id: string, label: string, created: number, };

/**
 * A paired device, removable by `hash`.
 */
export type Paired = { hash: string, label: string, created: number, used: number, };

/**
 * A global `[[notify]]` in words (System → Notifications): `web push` or `command`, and its events.
 */
export type Notifier = { kind: string, on: string, };

/**
 * The provider vendor of each account and each [sign in ▾] login that is a provider's.
 */
export type Vendors = { accounts: Array<Vendor>, setups: Array<Vendor>, };

/**
 * An account's or a login's `name` and its provider's `vendor`.
 */
export type Vendor = { name: string, vendor: string, };

/**
 * API v1's `queue`, the Queue tab at `now`: a card per fleet, the ones needing you first (`error`: fleet.toml unread). `rw`: the board takes forms; `live`: and is no export (an export's forms are inert); `mtime` they send back.
 */
export type QueueTab = { now: number, rw: boolean, live: boolean, mtime: string, error?: string | null, fleets: Array<QueueCard>, };

/**
 * A fleet's card in its colour `k`: its serial groups' names and paths, `need` rows needing you, a paused one's line (`paused`: since and lanes), else its drive line and red `notes`; its rows, `done`: today's `merged` events, newest first (`done_n` of them), the backlog and the [config] sheet.
 */
export type QueueCard = { name: string, k: string, serial: Array<string>, paths: Array<string>, need: number, paused?: string | null, drive: Drive, notes: Array<string>, rows: Array<QueueRow>, done: Array<DoneRow>, done_n: number, backlog?: Backlog | null, config: ConfigSheet, marks: { [key in string]: Marks }, };

/**
 * A queued row's marks by its issue (`QueueCard.marks`): `design`, `verify` (`on`, `off`, none: by `ui_paths`), `repro`, its serial groups and `needs` tags, and the stage `note` its row shows (`verify(n)`, `verify skipped: …`).
 */
export type Marks = { design: boolean, verify?: string | null, repro: boolean, serial: Array<string>, needs: Array<string>, note?: string | null, };

/**
 * A fleet's drive line in `tone`: its words, a tap's `meta` lines then the last pass's actions (`pass`: one ran, so none is `nothing was due`).
 */
export type Drive = { tone: string, line: string, meta: Array<string>, pass: boolean, acts: Array<DriveAct>, };

/**
 * A pass's action: its issue (`#N`) or step, in words, its raw `code` on tap.
 */
export type DriveAct = { who: string, words: string, code: string, };

export type QueueRow = { issue: number, state: string, tone: string, age: string, t: number, act: string, grow: string, coat: string, queued: boolean, check: boolean, groups: Array<string>, blocked_by?: number | null, title?: string | null, stage: Array<Stage>, lane: string, live: boolean, pr: PrLine, dead: boolean, notes: Array<string>, decided?: string | null, ask?: Ask | null, open?: NeedAsk | null, mv: boolean, up?: number | null, down?: number | null, lanes: Array<QueueLane>, runtime: string, host: string, where: string, };

/**
 * A pipeline chip: its kind's glyph `g` and class `k`, `at` before (`-1`), on (`0`) or after (`1`) where the issue is, a check's PASS at the PR head `ok`.
 */
export type Stage = { name: string, k: string, g: string, at: number, ok: boolean, };

/**
 * `[issue] [PR #M] [checks]` and CI (`green`, `failed`, else pending; none: no PR read yet), then the last verdict in `vtone` and its round, `form` the form-only rounds used.
 */
export type PrLine = { pr?: number | null, run?: string | null, ci?: string | null, verdict?: string | null, vtone: string, rounds: number, form?: string | null, };

export type QueueLane = { name: string, state: string, tone: string, ago: string, link: string, };

/**
 * A `merged` event of `done today`: its issue's title, land note (else `merged`), age, time `t`, PR and merge sha.
 */
export type DoneRow = { issue: number, title?: string | null, note: string, age: string, t: number, pr?: string | null, sha?: string | null, };

/**
 * The open issues not queued and with no lane, by number, as last read `ago`.
 */
export type Backlog = { issues: { [key in string]: BacklogIssue }, ago: string, };

/**
 * A fleet's [config] sheet: the config file (`path`, `file`), GENERAL, the Pipeline group's data (`pipe`), LIMITS, ROLES (with `rw`, pickers from `models`, `harnesses` and their efforts), the Conductor, ALERTS.
 */
export type ConfigSheet = { path: string, file: string, repo: string, local: string, integration: string, effort?: string | null, color_set: boolean, env: Array<string>, node_modules: boolean, builtin: boolean, review_rounds: number, update_rounds: number, stall_minutes: number, ask_minutes: number, max_impl?: number | null, max_rev?: number | null, line_cap?: number | null, serial: Array<string>, deny: Array<string>, required: Array<string>, merge_label?: string | null, notify: Array<string>, global: Array<string>, pipe: JsonValue, models: Array<string>, harnesses: Array<Harness>, roles: Array<RoleRow>, conductor?: Conductor | null, };

export type Harness = { name: string, efforts: Array<string>, };

/**
 * A role as set: harness (taking `efforts`), model and effort (unset: what the lane gets, `model_default`, `effort_default`).
 */
export type RoleRow = { name: string, profile: string, harness: string, model?: string | null, effort?: string | null, efforts: Array<string>, model_default: string, effort_default: string, accounts: Array<string>, mcp: Array<string>, };

/**
 * The fleet's conductor (`default`: impl's), its choices: models the fleet's roles name.
 */
export type Conductor = { default: boolean, profile: string, harness: string, model?: string | null, effort?: string | null, efforts: Array<string>, models: Array<string>, };

/**
 * The events after `since` (up to `until`) in `list`; over 500, none and `refresh` (read `state` again).
 */
export type Events = { seq: number, refresh: boolean, list: Array<Ev>, };

/**
 * One logged event; `k` is the theme moment it plays (merged, fail, needs-you). as the Events tab says it (`serve::words`): `glyph`, `tone`, `text`, shown under [important] (`imp`), its row sprite's `act` and `coat` (its agent's), `plain` (a fold's line), and its issue's `pr` (else the branch's checks `run`).
 */
export type Ev = { seq: number, ts: number, fleet: string, issue?: number | null, type: string, data: { [key in string]: JsonValue }, k?: string | null, glyph: string, tone: string, text: string, imp: boolean, act: string, coat: string, plain: string, pr?: number | null, run?: string | null, };

/**
 * API v1's `issue/<fleet>/<n>`: its recorded verdicts, newest first; its latest why note.
 */
export type IssueView = { fleet: string, issue: number, verdicts: Array<Verdict>, why?: WhyNote | null, pending?: PendingStart | null, };

/**
 * The issue's latest why note: the round head it is on, its round and its `Owner:` line.
 */
export type WhyNote = { sha: string, round: number, owner: string, };

/**
 * A verdict: its `role` (the stage: `rev`, `verify`, or the gate's), round, sha, `verdict` (PASS or FAIL), `findings` (verdict.md after its first line); the newest one's screenshot `shots` by name (`/shot/<fleet>/<n>/<i>` serves the `i`th); a `--repro` PASS's `Repro:` block.
 */
export type Verdict = { role: string, round: number, sha: string, verdict: string, ts: number, findings: string, shots: Array<string>, repro?: Repro | null, };

/**
 * A `Repro:` block: `run` (the commands and both outputs) or `verify` (the quoted reason), its lines as `text`.
 */
export type Repro = { kind: string, text: string, };

/**
 * API v1's `lessons`: per fleet, its `lessons` setting (`weekly`, `off`), the `last` lesson run and when `tick` runs the `next` (none: `off`).
 */
export type Lessons = { fleets: Array<LessonFleet>, };

export type LessonFleet = { fleet: string, lessons: string, last?: LessonRun | null, next?: number | null, };

/**
 * A lesson issue: its lane's spawn time `ts` and the issue's state now.
 */
export type LessonRun = { issue: number, ts: number, state: string, };

/**
 * `lessons/<fleet>/digest?days=N`: `drover lesson digest`'s text.
 */
export type Digest = { fleet: string, days: number, text: string, };

/**
 * API v1's `setup/<pane>` (the old board's `/lane?setup=`): the setup pane's masked `lines` and every `https://` URL in it (a wrapped one joined), or `closed`.
 */
export type SetupScreen = { lines: Array<string>, urls: Array<string>, closed: boolean, };

/**
 * API v1's `roles?fleet=`: the fleet's [config] sheet (its roles, pickers and conductor) and `[chat]`'s new-thread defaults (`POST chat` writes them).
 */
export type Roles = { fleet: string, config: ConfigSheet, chat: ChatDefaults, };

/**
 * `[chat]`'s harness, model and effort (none: the conductor role's), and the `harnesses` it may name.
 */
export type ChatDefaults = { harness?: string | null, model?: string | null, effort?: string | null, harnesses: Array<Harness>, };

/**
 * A POST's answer: `code` is OK, a `REFUSED` code, or ERROR; `message` the line the board toasts.
 */
export type Reply = { ok: boolean, code: string, message: string, 
/**
 * A tap's job id (`code` ACCEPTED): `GET /api/v1/job/<id>` gives how it ended.
 */
job?: string | null, };

/**
 * A tap run as a job: `state` running, waiting (for host), done, refused (`code`) or error; `message` its toast.
 */
export type Job = { id: string, op: string, fleet: string, issue?: number | null, state: string, code: string, message: string, ts: number, };

/**
 * `POST /api/v1/conductor-attach`'s answer (a refusal is a `Reply`): what a cooee conductor profile needs.
 */
export type ConductorAttach = { ok: boolean, code: string, message: string, 
/**
 * `roles/conductor.md`, the repo and the owner's conductor instructions.
 */
brief: string, 
/**
 * The synced conductor provider settings (`PreToolUse`: `drover hook`).
 */
settings: string, env: AttachEnv, 
/**
 * The ACP guard holds; else `why` not.
 */
guarded: boolean, why: string, };

export type AttachEnv = { DROVER_LANE: string, DROVER_THREAD: string, DROVER_PORT: string, };

/**
 * `GET /api/v1/config`: fleet.toml as cooee's config screens read it.
 */
export type ConfigView = { 
/**
 * The file's sha256: a manual edit sends it back and is refused `STALE` once the file moved.
 */
hash: string, 
/**
 * The raw TOML for the editor (a secret-shaped value never loads: `SECRET_IN_CONFIG`).
 */
text: string, fields: Array<ConfigField>, };

/**
 * One setting: its dotted path, TOML kind, allowed values, doc line and `secret:NAME`'s name.
 */
export type ConfigField = { path: string, kind: string, value: JsonValue, allowed: Array<string>, doc: string, secret?: string | null, };

/**
 * `config-check` and `config-edit`'s body: raw TOML, field edits, a fleet added or removed, at `hash`.
 */
export type ConfigChange = { hash: string, text?: string | null, set: Array<ConfigSet>, add?: NewFleet | null, remove?: string | null, confirm: boolean, };

export type ConfigSet = { path: string, value: JsonValue, };

/**
 * A fleet from a GitHub repo; its roles copied from the first fleet's.
 */
export type NewFleet = { name: string, repo: string, local: string, integration: string, };

/**
 * `POST /api/v1/config-check`'s answer: the change's diff and its errors; nothing written.
 */
export type ConfigCheck = { diff: string, errors: Array<string>, };

export type Event = { 
/**
 * The log's monotonic number, set on append (an old row's is its line number); 0 before.
 */
seq?: number, ts: number, fleet: string, issue?: number | null, type: string, 
/**
 * Not activity (an alert, a note, a prompt): the fold and the idle-pane reader skip it.
 */
passive?: boolean, 
/**
 * Reaches a phone whose notifier sets no `on`; kinds pushed before the flag are in `push::LEGACY`.
 */
push?: boolean, } & ({ [key in string]: number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null });

export type IssueState = { fleet: string, issue: number, state: string, lane?: string | null, branch?: string | null, 
/**
 * The latest lane's reasoning effort, as spawned.
 */
effort?: string | null, rounds: number, 
/**
 * Extra rounds the owner allowed (`round-extended`); the latest grants the round after any stage's.
 */
extra: number, granted: number, 
/**
 * Passed `--update` rounds, apart from `rounds`; `updating` while one is open (state `fix(n)+uk`).
 */
updates: number, updating: boolean, last_ts: number, last_event: string, 
/**
 * Latest agent per role (impl, rev).
 */
lanes: { [key in string]: string }, 
/**
 * (sha, PASS|FAIL, the check stage whose rounds it counts) in log order; older logs: `rev`.
 */
verdicts: Array<[string, string, string]>, 
/**
 * The pipeline stage of the latest spawn, the latest check and build roles, review rounds per check stage.
 */
stage: string, check?: string | null, build?: string | null, stage_rounds: { [key in string]: number }, 
/**
 * `FAIL (form-only)` verdicts that did not count toward the rounds.
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
 * What the last look at a host saw (`<state dir>/hosts.json`): detected tags and `$HOME` (probed hourly), reachability, load/cpu, free memory and disk in GB.
 */
export type HostSeen = { tags: Array<string>, home: string, probed: number, seen: number, up: boolean, load?: number | null, mem?: number | null, disk?: number | null, 
/**
 * A remote host's failed checks from `health` (`✗` keeps lanes off it, `!` warns), with the probe.
 */
health: Array<string>, 
/**
 * Memory pressure 0 normal, 1 warn, 2 critical; swap-out pages/2s; the top memory users (remote hosts).
 */
pressure?: number | null, swap?: number | null, top: Array<string>, 
/**
 * Memory and disk size in GB (a bar shows the used part).
 */
mem_total?: number | null, disk_total?: number | null, 
/**
 * When it last had a live agent (a cloud host's idle stop).
 */
busy: number, 
/**
 * A cloud host's gh token, as its card says it.
 */
github: string, 
/**
 * Its set workspace roots.
 */
roots: Array<HostRoot>, 
/**
 * A remote host's `drover --version` at the last look.
 */
drover: string, };

/**
 * A root as the last look saw it (`hosts.json`, the host card, `/api/v1`).
 */
export type HostRoot = { path: string, state: string, free_gb?: number | null, };

/**
 * One row a minute per host in `<state dir>/host-history.jsonl`, written by the fast step; 24 h kept.
 */
export type HostRow = { ts: number, host: string, load_cpu?: number | null, pressure?: number | null, swap_out?: number | null, mem_free_gb?: number | null, disk_free_gb?: number | null, lanes: number, 
/**
 * MB per `host::groups` group, only while `/live?tab=host` was seen in the last 2 min (else no `ps` walk).
 */
procs?: [number, number, number, number, number] | null, };

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
