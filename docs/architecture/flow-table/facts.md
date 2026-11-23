# flow-table: the facts (#1649)

A condition is one value of each fact. The list is closed: six enums in `flow::facts`. Values marked **+** are
not in the issue's list; `adr.md` says why each is needed.

## One source each

| Fact | Who observes it | How it reaches the main host | The one place its reader reads | Reader |
|---|---|---|---|---|
| `lane` | shed, on the lane's host | that host's frames | the view (`state.db`) | `facts::lane` |
| `host` | the host itself: when each frame arrives, and the load sample in it | that host's frames | the view's host row | `facts::host` |
| `pr` | GitHub | the webhook, the catch-up poll | the PR cache | `facts::pr` |
| `verdict` | the check lane's `verdict.md`, then `drover verdict` | intake logs `verdict-seen`; the verb logs `verdict` or `verdict-refused` | the log | `facts::verdict` |
| `ask` | `drover ask`, `drover answer` | the verbs log it | the log | `facts::ask` |
| `turn` | the lane's `drover turn` | the verb logs it | the log | `facts::turn` |

Four rules hold for every fact:

1. **One reader.** No other code works out a fact's value. Slice 2 answers `View::down`, `Sessions::running`,
   `Sessions::busy` and `check::live_lanes` from the readers; slice 6 deletes them.
2. **No value from a side signal.** A git call that ran out of time, an ssh probe's exit code, a session list
   that did not answer: none of these sets a fact.
3. **Late is not absent.** A value that means "gone" needs evidence of absence. An answer that is only late
   keeps the last value, and `host` says why it is late.
4. **Every value carries its evidence**: what was observed and when, for `waiting`, cards and refusals. For
   example `loaded: load 3.4 per CPU, last frame 40 s late`.

What a reader looks at (which session, which host, which head, which check stage) is drover's own record: the
episode's spawn and the PR head. The value comes from the one source.

## The six facts

### `lane`: the session of this round's spawn

Source: shed, as the view holds it (`View::sessions().seen(name, machine)`). A session left over from an
earlier round is not this round's lane.

| Value | Means | From today's types |
|---|---|---|
| `none` **+** | the round has no spawn yet, so there is nothing to look at | no `spawn` in the episode |
| `running` | shed lists it running and mid-turn, or waiting by design | `Status::Working`, `Running`, `Blocked`, `Approval`, `Unreported`, `Other` |
| `limited` **+** | shed says rate-limited | `Status::Limited` |
| `asleep` | shed hibernated it | `Status::Asleep` |
| `done` | its turn ended; the process is still there | `Status::Done`, `Idle`, `Failed` |
| `exited` | shed lists it exited | `Life::Exited` |
| `gone` | its host's fresh frame does not list it | `Seen::Absent` |

A host with no fresh frame changes no lane value: the view keeps the last word (rule 3).

### `host`: where this round's lane runs

Source: the host's own frames, as the view's host row holds them. Link intake is the row's only writer. Each
report frame carries the host's sample (load per CPU, memory pressure), so one arrives every
`link_sample_secs`. The row holds the last frame's arrival time, that sample, whether the link is open, and
the drain order in force when the frame was applied. With no lane the value is `up`.

One value, the first that applies:

| Value | Means |
|---|---|
| `offline` | the link is closed and no frame came for `2 × fast_secs`, while neither end is loaded; or the view has no row for the machine |
| `loaded` | the host's newest sample is over its limits (`load_per_cpu`, memory pressure); or its frame is late while that sample, or the main host's own, is over its limits |
| `draining` | a fresh frame under its limits, and an order keeps new lanes off the host: its lanes finish, none starts |
| `up` | a fresh frame under its limits, and it takes new lanes |

- **A late frame from a loaded host is `loaded`, never `offline`.** Load at either end explains lateness: the
  host's own (its last sample), or the main host's, whose reading of every link is then slow (#1652's case).
- A sample is evidence for `host_offline_minutes`. A host that was loaded and then stays silent with a closed
  link past that turns `offline`.
- `loaded` asks "can this host answer on time". Room for one more lane is a different question, and stays the
  spawn action's `Held HOST_BUSY` outcome.
- `draining` is the one value that is an order, not an observation: the owner's `hosts off`, a release swap or
  a sign-in hold. One function gives it (`hosts::drain`). Intake stamps it on the row with each frame, so the
  reader still reads one row. Placement is policy and reads the order directly; no row does.
- The load number has one function, `sys::loadavg` (no process is started to read it). The frame's sample and
  the runner's disk probe both use it.

#1652 is the same rule one level down. `runner::hung` marks a disk "not answering" only when a listing never
returns, or when three listings in a row are slow while load is under its limit. While the host is loaded the
call waits and then runs. The refusal says what was seen (`folder listing took 4.1 s at load 28`) and names no
command that did not run.

### `pr`: the issue branch's pull request

Source: GitHub, through the PR cache (`gh::cached_pr`). One value, the first that applies, top to bottom.

| Value | Means |
|---|---|
| `none` | no PR for the branch |
| `merged` **+** | merged, and drover did not log the merge |
| `closed` **+** | closed unmerged |
| `conflicting` | `mergeable = CONFLICTING` |
| `red` | a required check failed at the head |
| `pending` | a required check is queued or running at the head |
| `green` | every required check passed at the head |
| `open` | open, and no required check has reported at the head yet |

A draft PR, a missing merge label and a branch that is only behind are handled inside the `merge` action (it
marks the PR ready, adds the label, or merges integration in), not as PR values.

### `verdict`: the check stage now due, at the PR head

Source: the log. Intake logs a passive `verdict-seen` (head, first line) once for each new `verdict.md` a
frame brings. `drover verdict` logs `verdict`, or a passive `verdict-refused` with its code. "At the head"
follows an `auto-updated` clean merge back to the head that was reviewed, as today. The stage is the
pipeline's next check (`rev`, then `verify` where it is due).

| Value | Means |
|---|---|
| `none` | nothing logged for this stage at this head |
| `written` | a `verdict-seen` at this head, first line `PASS` or `FAIL`, and no `verdict` or refusal after it |
| `PASS` | `verdict` PASS at this head, and no further check stage is due |
| `FAIL` | `verdict` FAIL at this head |
| `refused` | the latest word at this head is `verdict-refused` (`NO_PROOF`, `NO_REPRO`, `NO_BASELINE`, `PAPERWORK`, `TESTS_CHANGED`) |

`verdict-refused` replaces what `waiting.rs` and `drive::no_proof` infer today from repeated verdicts.

### `ask`: the issue's ask

Source: the log (`events::open_asks`, `decision`, `undelivered`).

| Value | Means |
|---|---|
| `none` **+** | no open ask and no decision waiting |
| `open` | an ask on the issue or by one of its lanes is open (with the orchestrator or with the owner) |
| `answered` | a decision is logged and not yet delivered or applied |

### `turn` **+**: what the build lane last said

Source: the log's latest `turn-end` of this round's build lane (`waiting::turn`). A new spawn, a verdict or an
extended round starts it at `none`.

| Value | Means |
|---|---|
| `none` | no `drover turn` this round |
| `working` | `turn working` |
| `done` | `turn done` passed its gate at the pushed head |
| `blocked` | `turn blocked --why` |
| `need-input` | `turn need-input` |

## Sampling

```
 hosts ══ link frames ══▶ intake ─▶ view (state.db) ─┐
 GitHub ─ webhook, catch-up poll ─▶ PR cache ────────┼─▶ Seen::load (once a pass) ─▶ Seen::cond(issue) ─▶ Cond
 drover verbs, intake ─▶ the log ────────────────────┘
```

- `Seen::load` is the only sampler. It reads `state.db`, the PR cache and the log. It never calls a host, `gh`
  or `git`: a pass that decides makes no remote read (drive-push's rule, kept).
- One sample per pass per fleet. A kick samples its own issue again before it acts.
- A source that is late is not guessed (rule 3): `host` turns `loaded` or `offline` by its table, and the PR
  cache's age rides in the `Step` so `waiting` can print `checked 4m ago`.
- The sampler holds no clock. "CI never started" is the `open` row's timeout, not a PR value.
- Reads never write. The stamp (`lane-state.since`) and intake stay the only writers of what was seen.

## What changes under #1567 and #1645

No row changes under either. #1567 (shed owns machines, links, stats and files) rewrites two readers. #1645
(serve becomes its own process, an `/api/v1` client) rewrites none.

| Fact | Under #1567 | Under #1645 |
|---|---|---|
| `lane` | shed still observes it. It arrives through shed's client over machine links, not in `drover hosts report` frames. `facts::lane` reads the new store | nothing |
| `host` | `offline` comes from `machine.links` (link down since), `loaded` from `machine.stats`. The frame's sample and the link's host row go. `facts::host` is rewritten; its four values and rule 3 stay. The drain order needs one home: #1567's ADR names it (recommended: a flag on shed's machine record, so the fact stays one read) | nothing |
| `pr` | nothing | the webhook's HTTP end leaves with serve. A delivery reaches core as an `/api/v1` action. The cache, its two fillers and `facts::pr` stay in core |
| `verdict` | `verdict.md` is fetched with shed's `file.read`. Whatever fetches it logs `verdict-seen`, as intake does now | nothing |
| `ask` | nothing | an answer from the page is an `/api/v1` action that logs the same `decision` |
| `turn` | a remote lane's `drover turn` arrives through shed, not as an outbox line in a frame. The verb still logs it | nothing |

Also under #1567: shed switches accounts on a usage limit, so `limited` shows only when no account has room,
and the sign-in hold behind `draining` becomes shed's account state. Under #1645 serve reads `Waiting` (with
`row`, `try`, `deadline`, `level` and the line already built) over `/api/v1` and never links `flow`; a card's
[retry] and [park] are `/api/v1` actions.

## What is not a fact

These are known to drover itself, not observed, so they are not conditions:

| Thing | Where it lives |
|---|---|
| The state | the fold of the log, as today |
| Tries so far, when the row was entered, which session and head a reader looks at | the row's episode (`shapes.md`) |
| Queue marks (`--design`, `--architect`, `--verify`, `--repro`, `--perf`, `--serial`), blockers, round limits | read by the action: `start` picks the first role, `spawn-check` picks the stage, a blocker or a limit comes back as the action's outcome |
| Host room, spawn slots, caps, a paused fleet | the action's outcome (`held` or `queued`, `shapes.md`) |

## Adding a fact or a value

- A new **value** is an enum variant. The table test fails for every state that reads that fact until a row
  names the value there. A state that truly does not care says so by not listing the fact in its `reads`.
- A new **fact** is a field of `Cond`. `Cond::every()` grows, and every state file must list it in `reads` or
  `ignores`; a file that names neither fails to load.

So "adding a fact forces new rows" is enforced by the test, not by review. `loaded` is the first case: the
tables in `tables.md` name it in every state that reads `host`.
