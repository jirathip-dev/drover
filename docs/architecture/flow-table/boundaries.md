# flow-table: module boundaries and statecharts (#1649)

## Who may call whom

```
 crates/core/flow/*.toml ─ build.rs ─▶ flow::table ◀── diagram, docs/flow.md, the table test
                                            │
 view (state.db) ─┐                         ▼
 PR cache ────────┼─▶ flow::facts ─▶ flow::decide ─▶ Step ─┬─▶ flow::act ─▶ spawn, gate, term, land, card …
 the log ─────────┘      (Cond)         (pure)            ├─▶ waiting (a line, nothing else)
                                            ▲              └─▶ flow::shadow (compare, log, report)
                                 flow::episode (tries, level, from the log)
                                                              flow::act ─▶ flow::escalate ─▶ orchestrator, ask
```

New code goes in new files under `crates/core/src/flow/*`. The `shared` files (`drive.rs`, `waiting.rs`,
`check.rs`, `events.rs`) only shrink.

| Module | Owns | May call | Must not |
|---|---|---|---|
| `flow::table` (new) | `Table`, `Row`, `Action`, `Owner`, `Timeout`; loading the embedded TOML once | serde, toml, `config::LIMIT_DOCS` (to check a timeout's key) | do any I/O; know an issue |
| `flow::facts` (new) | `Cond`, the six enums, `Seen::load`, `Seen::cond`, one reader per fact | `link::view::View`, `gh::cached_pr`, the log, `session` types, `pipeline` (which check is due) | call a host, `gh`, `git` or the runner; write anything; read a second source for one fact |
| `flow::episode` (new) | `Episode`: tries, the entered row, the level | the log | write |
| `flow::decide` (new) | `decide(table, state, cond, episode, now) -> Step` | `table`, `facts`, `episode` types, `config::Limits` | any I/O, the clock (it takes `now`), the log |
| `flow::act` (new) | the one `match Action`; `Outcome`; the `row-*` events; `order` | `spawn`, `gate`, `term`, `land`, `design`, `card`, `orchestrator`, `check::clean`, `hosts` to act, `flow::escalate` | decide: it never reads `Cond` to choose another action; run without a `drive::Pass` |
| `flow::escalate` (new) | the ladder: the thread, the card, `[retry]`, `[park]`; the only emitter of `needs-human` | `orchestrator::route`, `events::ask`, `gate::park` | spawn, merge, or pick a thread any other way; be called by today's drive (only `flow::act` calls it, with its `Pass`) |
| `flow::shadow` (new, gone at slice 6) | the shadow episode's `row-*` events, `flow-disagree`, `flow report` | `decide`, `episode`, `waiting::scan` (the old one), the log | act, ask, raise a card; write an event without a `drive::Pass` (`flow report` only reads) |
| `flow::chart` (new) | the statechart and `flow.md` in `docs/` from the table | `table`, `config::Limits` | read the log (but for `--live` counts) |
| `drive` (kept, smaller) | the pass and kick leases, which are rows of `state.db` (`drive-<fleet>.lock`, `drive-<fleet>-<n>.lock`, `kick-<fleet>-<n>.lock`); `Pass`, the proof one is held; kicks, job slots, the `drive-pass` event | `flow::facts`, `flow::decide`, `flow::act`, `flow::shadow` | take, wait on or check #1645's `api.lock`; hand a `Pass` to another process; match on a `Waiting` line or a command string; emit `needs-human`; count retries |
| `waiting` (kept, smaller) | `Waiting` for API v1 and the board, built from a `Step` | `flow::decide` | compute a next step of its own |
| `check` (kept, smaller) | host, disk, runner, keychain and config alerts; cleaning | as today, minus its issue rules | hold a stall, orphan-issue or ready-unmerged rule: an issue's alert is its row's `row-escalated` |
| `diagram` (kept) | `drover diagram` | `flow::chart`, `drive::STEPS`, `timer::TICK_STEPS` | read `events::STATES` by hand: it is generated |
| `events` (kept) | the fold | nothing new | know a row |
| `link` (kept) | frames, intake, the view; the host row with its sample and drain order; `verdict-seen` | `host::sample` on the reporting host, `hosts::drain` | turn lateness into `offline` (that is `facts::host`'s rule) |
| `runner` (kept) | the disk mark (`hung`, `sicken`) | `sys::loadavg` | mark a disk for one slow listing, or for any while the host is loaded; read the view |
| `crates/server` (after #1645 `drover-api`, the API host) | the board's screens, the owner's taps, the webhook listener | `waiting` (`/api/v1`'s `Waiting`, which carries the row's text), `flow::chart`, the verbs a tap runs (`answer`, `park`, `extend`), `drive::kick` | call `flow::act`, `flow::escalate` or `flow::shadow`, or run a pass in its own process: it logs the tap and kicks |
| `crates/server` after #1645 (the edge) | the page's files, the relay | the API host, over HTTP | link core, so no `flow::*` at all |

Three rules the checks hold (`adr.md`, the `.drover/arch.toml` lines):

1. Only `flow::act` turns a decision into an effect, and only `flow::escalate` raises a card.
2. No decision reads a display line.
3. `decide` is pure: the table test and the shadow run call it with no fixture but values.

Core gains no layer edge: `flow` is inside `crates/core`.

## Which process runs what

Today and under #1645 the answer is the same: **a row is acted on only inside a drive pass or kick, in the
service's processes, under drive's own lease.** The API host never acts on a row.

| Code | Process today | Process under #1645 | Lock it runs under |
|---|---|---|---|
| `flow::act`, and `flow::escalate`, which only `act` calls | a fleet pass: the `drover drive <fleet>` process the service's loop starts (the timer's tick starts it when no service runs). A kick: inside the service's process, else a detached `drover kick --now` | the same processes | the pass's lease `drive-<fleet>.lock` or the kick's `kick-<fleet>-<n>.lock`, then the issue's own `drive-<fleet>-<n>.lock` for each step; all rows of `state.db`. `Pass` is the first, held |
| `flow::shadow`'s events (`row-*` marked `shadow`, `flow-disagree`) | the same pass or kick, right after today's pass body | the same | the same lease |
| the owner's tap ([retry], [park], [one more round], an answer) | serve's process, or `drover answer` in a terminal | the API host (the edge relays the tap to it), or the CLI | the API host is the one writable host (`api.lock`). It logs `decision` and kicks the issue; `apply-decision` and `row-retry` are the kick's |
| a webhook delivery | serve's process | the API host | `api.lock`; it fills the PR cache and kicks |
| a lane's `drover turn`, `verdict`, `ask` | the lane's own CLI process; a remote lane's through its outbox and intake, in the service | the same | none of its own; its event is logged and the issue kicked |
| `flow::facts`, `decide`, `episode`, `chart`, `flow report` | any: a pass, the CLI, serve | any: a pass, the CLI, the API host (read-only ones too); never the edge | none: they write nothing |

What #1645's lock is and is not, as its ADR has it (the serve-split folder on branch `issue-1645`, `d4863aad3`):

- `api.lock` is a file in the state folder that one API host holds for its life. It makes that process the only
  one that binds the webhook listener and takes writes from outside: taps, pairing, deliveries.
- It is not a lock on the log. Every process that links core appends to `state.db` under SQLite's own write
  lock, as today: the pass, the kicks, intake, the CLI's verbs, the terminal's chat, the API host. #1645's
  `service` row keeps "the drive loops, the links, the kicks", may call core and must not link `api`.
- So no `flow` code takes or waits on `api.lock`, and a stopped API host stops no row: passes and kicks go
  on, the catch-up poll fills the PR cache, `drover answer` still answers a card. Only taps and deliveries wait.
- One actor per issue is the issue's lease (`drive-<fleet>-<n>.lock`), not `api.lock`: a pass and a kick both
  take it before a step, whichever process each is in. An issue whose lease is held is left to its holder: no
  try is spent.

The check is the type, not a rule line: `act`, `escalate` and `shadow::run` take a `&drive::Pass`, and `Pass`
has no constructor outside `drive` (`shapes.md`). The API crate links core and still cannot call them.

If the owner wants every write to go through drover-api instead, that is a change to #1645's ADR (its
`service` row and its lock), and here only this section changes: `Pass` would be built in the API host
(`adr.md`, open question 5).

## A row's life, for one issue

```text
            sample ─▶ decide
                 │
                 ▼
 [entered] ── do ──▶ outcome
     │                 ├─ Done ──────────────▶ the next sample picks another row ─▶ [left]
     │                 ├─ Queued{behind} ────▶ [waiting]   no try, no clock: the issues ahead have rows
     │                 ├─ Held{by} ──────────▶ [waiting]   no try; the holder's row runs its own clock
     │                 ├─ Failed ────────────▶ try + 1
     │                 └─ Final ─────────────▶ [escalated]
     ▼
 [waiting] ── deadline (since + timeout) passes ──▶ try + 1
     │
     ├─ try 2: then[0] ── outcome as above
     ├─ try 3: then[1] ── outcome as above
     └─ the third try fails or times out ──▶ [escalated]

 [left] and back in the same episode ──▶ [entered], tries kept
 a new episode (the PR head, the working state or the owner's decision changed) ──▶ tries zero
 an orchestrator's answer ──▶ this row's tries zero, once per episode
```

A row is never in a state with no arrow out: `[waiting]` always has a deadline, except the two `owner-held` rows.

## The escalation ladder

```text
 [auto]            tries 1..3, by the row
   │ third try spent, or Final, or esc = orch
   ├─ the orchestrator has answered this row in this episode ──▶ [owner]
   ▼
 [orchestrator]    one ask, `routed` to the thread that filed or queued the issue, else the repo's;
   │               the board shows the issue as `with orchestrator`
   ├─ it answers (a spec quote, a past decision; #1609 adds judgment) ──▶ apply-decision ──▶ [auto]:
   │               the same episode, the row's tries at zero, the row noted as answered
   ├─ escalate_minutes pass, or it passes the ask on, or no orchestrator is reached
   ▼
 [owner]           one card that names the row: `<row id>: <what was tried, 3 lines>`
   │               [retry] the row's tries go to zero ──▶ [auto]
   │               [park] ──▶ parked
   │               [one more round] on a round-limit row ──▶ an extended round
   └─ unanswered: a reminder every ask_waiting_minutes, then daily. Never a second card, never a retry.

 esc = owner (design and ADR approval, owner topics): [auto] ──▶ [owner] directly
```

One issue has at most one open card. A second row that escalates while a card is open adds a line to it.

The orchestrator answers a row at most once per episode. So the longest path to the owner for one row is three
tries, one orchestrator answer, three more tries, the card. Only the owner's decision, a new PR head or a move
to another working state starts a new episode.

Until cut-over this ladder runs for no fleet. Today's cards keep today's path through the shadow week; shadow
only works out when the ladder would have reached each level (`shapes.md`).

## Shadow to cut-over, per fleet

```text
 [legacy]   today's drive
   │ slice 3 ships: [fleet.<n>] flow = "shadow" is the default
   ▼
 [shadow]   today's drive acts and cards as today; decide runs beside it each pass with its own episode
   │        (tries, deadlines, level); `flow-disagree` per difference in action or in escalation timing
   │ 7 days, and every group in `drover flow report` has a class: `row fixed` or `today's bug`
   ▼
 [on]       slice 5: drive acts from the table and the ladder is live; the old pass body stays behind
   │        flow = "legacy"
   │ 7 days with no `flow = "legacy"` set on any fleet
   ▼
 [only]     slice 6: the old paths and the `flow` key are deleted
```

Back from `[on]` to `[legacy]` is one config card (`flow = "legacy"` for a fleet). There is no way back from
`[only]`.

Who gives a group its class (`adr.md`, decision 7):

| Class | Means | Who decides | Proof |
|---|---|---|---|
| `row fixed` | the table was wrong | slice 5's impl lane | the commit and a `decide` test of the case; rev checks both |
| `today's bug`, listed in `adr.md` | today's drive is wrong, known in advance | the owner, by approving the ADR | the ADR's line |
| `today's bug`, not listed | today's drive is wrong, found in the week | the orchestrator thread when an open issue, a spec line or a past decision already says so; else the owner | the issue, the quoted line or the decision id |

The impl lane never classes a group `today's bug` on its own word.
