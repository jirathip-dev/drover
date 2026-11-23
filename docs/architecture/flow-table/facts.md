# flow-table: the facts (#1649)

A condition is one value of each fact. The list is closed: six enums in `flow::facts`. Each fact has one reader
function and one source. Values marked **+** are not in the issue's list; `adr.md` says why each is needed.

## The six facts

### `lane`: the session of this round's spawn

Source: shed, as the link's view holds it (`link::view::View::sessions().seen(name, machine)`), for the spawn
logged since the issue entered its state. A session left over from an earlier round is not this round's lane.

| Value | Means | From today's types |
|---|---|---|
| `none` **+** | no spawn in this round yet | no `spawn` since the state was entered |
| `running` | shed lists it running and mid-turn, or waiting by design | `Status::Working`, `Running`, `Blocked`, `Approval`, `Unreported`, `Other` |
| `limited` **+** | shed says rate-limited | `Status::Limited` |
| `asleep` | shed hibernated it | `Status::Asleep` |
| `done` | its turn ended; the process is still there | `Status::Done`, `Idle`, `Failed` |
| `exited` | shed lists it exited | `Life::Exited` |
| `gone` | its host answered and does not list it | `Seen::Absent` |

A host that did not answer changes no lane value: the view keeps the last word, and `host` says it is stale.

### `host`: where this round's lane runs

Source: the link's view (`View::down`, `View::live`) and `[hosts.<name>] enabled`. With no lane the value is `up`.

| Value | Means |
|---|---|
| `up` | a fresh word from the host, and it takes new lanes |
| `draining` | a fresh word; `enabled = false`, or a release swap is under way: its lanes finish, none starts |
| `offline` | no fresh word within `2 × fast_secs`, a hung root, or a machine no `[hosts]` entry names |

### `pr`: the issue branch's pull request

Source: GitHub, through the PR cache the webhook and the catch-up poll fill (`gh::cached_pr`). One value, the
first that applies, top to bottom.

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

Source: the log's `verdict` events and the check lane's `verdict.md` as the view holds it. "At the head" follows
an `auto-updated` clean merge back to the head that was reviewed, as today. The stage is the pipeline's next
check (`rev`, then `verify` where it is due).

| Value | Means |
|---|---|
| `none` | no verdict for this stage at this head |
| `written` | a `verdict.md` naming this head, first line `PASS` or `FAIL`, not in the log |
| `PASS` | recorded PASS at this head, and no further check stage is due |
| `FAIL` | recorded FAIL at this head |
| `refused` | `drover verdict` refused the file at this head (`NO_PROOF`, `NO_REPRO`, `NO_BASELINE`, `PAPERWORK`, `TESTS_CHANGED`) |

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
 drover verbs ─▶ the log ────────────────────────────┘
```

- `Seen::load` is the only sampler. It reads `state.db`, the PR cache and the log. It never calls a host, `gh`
  or `git`: a pass that decides makes no remote read (drive-push's rule, kept).
- One sample per pass per fleet. A kick samples its own issue again before it acts.
- A fact older than its source's freshness limit is not guessed: `host` turns `offline`, and the PR cache's age
  rides in the `Step` so `waiting` can print `checked 4m ago`.
- The sampler holds no clock. "CI never started" is the `open` row's timeout, not a PR value.
- Reads never write. The stamp (`lane-state.since`) and the link's intake stay the only writers of what was seen.

## What is not a fact

These are known to drover itself, not observed, so they are not conditions:

| Thing | Where it lives |
|---|---|
| The state | the fold of the log, as today |
| Tries so far, when the row was entered | the row's episode (`shapes.md`) |
| Queue marks (`--design`, `--architect`, `--verify`, `--repro`, `--perf`, `--serial`), blockers, round limits | read by the action: `start` picks the first role, `spawn-check` picks the stage, a blocker or a limit comes back as the action's outcome |
| Host room, spawn slots, caps, a paused fleet | the action's outcome (`held` or `queued`, `shapes.md`) |

## Adding a fact or a value

- A new **value** is an enum variant. The table test fails for every state that reads that fact until a row
  names the value there. A state that truly does not care says so by not listing the fact in its `reads`.
- A new **fact** is a field of `Cond`. `Cond::every()` grows, and every state file must list it in `reads` or
  `ignores`; a file that names neither fails to load.

So "adding a fact forces new rows" is enforced by the test, not by review.
