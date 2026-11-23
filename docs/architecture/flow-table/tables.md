# flow-table: every flow's table (#1649)

How to read a table:

- Rows are tried top to bottom; the first match is the row. A blank fact cell is `*` (any value).
- **do** runs when the issue enters the row. **timeout** is a `[limits]` key; its clock starts when the row was
  entered unless the cell says `· heard` (shed last heard the lane) or `· reset` (shed's limit reset time).
- **then** is what the first and second retry do; blank means `do` again. After three tries the ladder runs:
  the orchestrator thread, then one owner card with [retry] and [park] (`boundaries.md`).
- **esc** is blank for the ladder, `orch` to skip the retries, `owner` for an owner topic.
- **to** is the state a success leads to; blank stays.
- Every spawn action replaces a same-name session whose lane fact is `done`, `asleep`, `exited` or `gone`. One
  that is `running` comes back `Final LANE_LIVE`.

The rows below are the content of `crates/core/flow/*`. Slice 1 writes them as TOML; the shadow week corrects them.

## Flow `issue`: the lifecycle

### `queued` · reads ask

| id | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|
| `ask-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | answered | apply-decision | drive | `act_minutes` | | | |
| `start` | none | start | drive | `act_minutes` | | | design, impl |

`start` spawns the first role the queue marks name (architect, then design, else the pipeline's start). It comes
back `Queued` behind blockers, a serial group, the in-flight cap or a pause, `Held` by a host, and `Final` for a
stranded blocker.

### `design` · reads lane, host, ask

| id | lane | host | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|---|
| `ask-open` | | | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | | | answered | deliver | drive | `act_minutes` | | | |
| `host-offline` | | offline | none | wait | drive | `host_offline_minutes` | move | | |
| `no-lane` | none | up, draining | none | reconcile | drive | `act_minutes` | | orch | |
| `working` | running | up, draining | none | wait | lane | `silent_minutes` · heard | nudge, respawn | | |
| `limited` | limited | up, draining | none | continue-at-reset | drive | `stall_minutes` · reset | respawn | | |
| `ended` | done, asleep | up, draining | none | hand-off | drive | `act_minutes` | nudge, respawn | | design-review |
| `lost` | exited, gone | up, draining | none | respawn | drive | `act_minutes` | | | |

`hand-off` logs `design-review` and raises the approve, revise, reject ask when the head carries the stage's
files; it fails when it does not.

### `design-review` · reads ask

| id | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|
| `ask-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | answered | apply-decision | drive | `act_minutes` | | | queued, design, needs-human |
| `no-ask` | none | hand-off | drive | `act_minutes` | | | |

### `impl`, `fix` (file `build`) · reads all six · in flight

| id | lane | host | pr | verdict | ask | turn | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | | | merged | | | | record-merge | drive | `act_minutes` | | | merged |
| `pr-closed` | | | closed | | | | raise-card | drive | `act_minutes` | | orch | needs-human |
| `ask-open` | | | | | open | | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | | | | | answered | | deliver | drive | `act_minutes` | | | |
| `host-offline` | | offline | | | none | | wait | drive | `host_offline_minutes` | move | | |
| `reviewed-here` | | up, draining | | written, PASS, refused | none | | reconcile | drive | `act_minutes` | | orch | |
| `pushed` | | up, draining | open, pending, red, green, conflicting | none | none | done | spawn-check | drive | `act_minutes` | | | review |
| `no-change` | | up, draining | | FAIL | none | done | no-change | drive | `act_minutes` | | | review, ready |
| `done-no-pr` | | up, draining | none | none | none | done | nudge | lane | `stall_minutes` | | | |
| `lane-blocked` | | up, draining | | none, FAIL | none | blocked | raise-card | drive | `act_minutes` | | orch | needs-human |
| `need-input` | | up, draining | | none, FAIL | none | need-input | nudge | lane | `stall_minutes` | | | |
| `round-start` | none | up, draining | | none, FAIL | none | none, working | spawn-build | drive | `act_minutes` | | | |
| `working` | running | up, draining | | none, FAIL | none | none, working | wait | lane | `silent_minutes` · heard | nudge, respawn | | |
| `limited` | limited | up, draining | | none, FAIL | none | none, working | continue-at-reset | drive | `stall_minutes` · reset | respawn | | |
| `lane-ended` | done, asleep | up, draining | | none, FAIL | none | none, working | wait | lane | `stall_minutes` · heard | nudge, respawn | | |
| `lane-lost` | exited, gone | up, draining | | none, FAIL | none | none, working | respawn | drive | `act_minutes` | | | |

- `round-start` is a fix round after FAIL, an update round, an extended round, or impl after a kill: the state
  is entered and no spawn of this round is logged yet.
- `no-change` is `drover done --no-change`: a CI-only FAIL whose head is green now becomes PASS; any other head
  is reviewed again.
- `respawn` pushes the worktree's unpushed commits to the issue branch first, then resumes in that worktree.
- `lane-ended` is #1597's case: a lane shed calls `done` with no turn report is prompted once, respawned once,
  then carded. It is never prompted for hours.

### `review` · reads lane, host, pr, verdict, ask · in flight

| id | lane | host | pr | verdict | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | | | merged | | | record-merge | drive | `act_minutes` | | | merged |
| `pr-closed` | | | closed | | | raise-card | drive | `act_minutes` | | orch | needs-human |
| `record` | | | none, open, pending, red, green, conflicting | written | | record | drive | `act_minutes` | | | ready, fix |
| `ask-open` | | | | | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | | | | | answered | deliver | drive | `act_minutes` | | | |
| `recorded-here` | | | | PASS, FAIL | none | reconcile | drive | `act_minutes` | | orch | |
| `refused` | | | | refused | none | tell-check | lane | `stall_minutes` | respawn-check | | |
| `host-offline` | | offline | | none | none | wait | drive | `host_offline_minutes` | move | | |
| `start` | none | up, draining | | none | none | spawn-check | drive | `act_minutes` | | | |
| `reviewing` | running | up, draining | | none | none | wait | lane | `check_quiet_minutes` · heard | respawn-check | | |
| `limited` | limited | up, draining | | none | none | continue-at-reset | drive | `stall_minutes` · reset | respawn-check | | |
| `no-verdict` | done, asleep | up, draining | | none | none | wait | lane | `start_minutes` · heard | tell-check, respawn-check | | |
| `lost` | exited, gone | up, draining | | none | none | respawn-check | drive | `act_minutes` | | | |

- `record` sits above the ask, host and lane rows on purpose: a written verdict is recorded from the view's
  copy whatever the lane or its host is doing (#1597, #1612, #1625).
- `refused` re-prompts the reviewer with the refusal; a reviewer that is not there fails the try and the retry
  reviews again.

### `ready` · reads pr, verdict, ask · in flight

| id | pr | verdict | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | merged | | | record-merge | drive | `act_minutes` | | | merged |
| `pr-closed` | closed | | | raise-card | drive | `act_minutes` | | orch | needs-human |
| `ask-open` | | | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | | | answered | apply-decision | drive | `act_minutes` | | | |
| `next-check` | none, open, pending, red, green, conflicting | none | none | spawn-check | drive | `act_minutes` | | | review |
| `stray-verdict` | | written, FAIL, refused | none | reconcile | drive | `act_minutes` | | orch | |
| `merge` | green | PASS | none | merge | drive | `gate_check_minutes` | | | merged, fix |
| `ci-running` | pending | PASS | none | wait | drive | `ci_stuck_minutes` | unstick-ci | | |
| `ci-not-started` | open | PASS | none | wait | drive | `ci_start_minutes` | unstick-ci, ci-fail | | |
| `ci-red` | red | PASS | none | ci-fail | drive | `act_minutes` | | | fix |
| `conflict` | conflicting | PASS | none | update | drive | `act_minutes` | | | fix |
| `no-pr` | none | PASS | none | reconcile | drive | `act_minutes` | | orch | |

- `next-check` spawns the next check stage (verify where it is due), or a fresh review when the head moved under
  the PASS. Where verify is not due it logs `verify-skipped` and the verdict then reads PASS.
- `merge` marks a draft ready, adds a missing merge label and merges integration into a branch that is only
  behind, then merges. `STAGING_WOULD_BREAK` opens a fix round; `NOT_MERGEABLE` is the `conflict` row's next
  sample; another issue's merge in progress is `Queued`.
- `conflict` merges integration in when that is clean, else spawns an update lane. `UPDATE_LIMIT` is `Final`.
- `ci-fail` records a FAIL carrying the failed check's log tail.

### `blocked` · reads pr, verdict

Old logs only. After cut-over no row leads here: a refused merge is a failed try of `ready/merge`.

| id | pr | verdict | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|
| `merged-outside` | merged | | record-merge | drive | `act_minutes` | | | merged |
| `pr-closed` | closed | | raise-card | drive | `act_minutes` | | orch | needs-human |
| `back` | none, open, pending, red, green, conflicting | none, written, PASS, FAIL, refused | reconcile | drive | `act_minutes` | | | ready, review, fix |

### `needs-human` · reads ask

| id | ask | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|
| `card-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner | |
| `decided` | answered | apply-decision | drive | `act_minutes` | | | queued, impl, fix, parked |
| `no-card` | none | raise-card | drive | `act_minutes` | | orch | |

The card itself moves through flow `ask` below: with the orchestrator first, then the owner. `apply-decision`
on [retry] returns the issue to the state it had and sets the row's tries to zero; on [one more round] it
extends a round; on [park] it parks.

### `parked` · reads nothing

| id | do | owner | timeout | to |
|---|---|---|---|---|
| `parked` | wait | owner | owner-held | the state it had, on `unpark` |

### `merged`, `closed` (file `ended`) · reads lane

| id | lane | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|
| `leftover` | running, limited, asleep, done | clean | drive | `done_close_minutes` | | | |
| `finished` | none, exited, gone | clean | drive | `done_close_minutes` | | | |

`clean` saves unpushed work to `wip/` first, closes the session, removes the worktree and build folders. With
nothing left the issue leaves the open set and has no row.

## Flow `ask`: one ask, card or question

States come from the log: `routine` (a lane's `--kind routine`), `routed` (with an orchestrator thread),
`with-owner`, `answered`, `orphaned` (its issue left the episode that raised it). `with-owner` and `answered`
read `lane`, the asking lane; the others read nothing.

| state | id | lane | do | owner | timeout | then | esc | to |
|---|---|---|---|---|---|---|---|---|
| `routine` | `default` | | wait | orchestrator | `ask_default_minutes` | take-default | | answered |
| `routed` | `in-thread` | | wait | orchestrator | `escalate_minutes` | to-owner | | with-owner, answered |
| `with-owner` | `asker-gone` | exited, gone | wait | owner | `ask_expire_hours` | expire | owner | |
| `with-owner` | `waiting` | none, running, limited, asleep, done | wait | owner | `ask_waiting_minutes` | remind | owner | answered |
| `answered` | `deliver` | running, limited, asleep, done, exited, gone | deliver | drive | `act_minutes` | | | |
| `answered` | `apply` | none | apply-decision | drive | `act_minutes` | | | |
| `orphaned` | `close` | | close-superseded | drive | `act_minutes` | | | |

- Every card starts at `routed`: needs-human, one more round, cap, respawn loop, lane asks. The thread is the
  one that filed or queued the issue, else the repo's (`orchestrator::route`). No orchestrator role, or one not
  reached, is a failed try of `in-thread` and goes on to the owner.
- These start at `with-owner`: design and ADR approval, a handoff pick, and anything naming an owner topic.
- `deliver` types `DECISION <id>: <text>` into the asking lane; for a lane that is gone it respawns the lane
  with the decision in its brief.
- `orphaned/close` is #1611: a design ask whose design was redone closes `closed: superseded`.
- `escalate = owner` on a row already with the owner means a daily reminder, never a second card.

## Flow `card`: a run card

| state | id | do | owner | timeout | then | esc |
|---|---|---|---|---|---|---|
| `posted` | `waiting` | wait | owner | `card_ttl_minutes` | expire | owner |
| `approved` | `run` | run-card | drive | the card's own `--timeout` | kill-run | |

## Flow `handoff`: one thread to another

| state | id | do | owner | timeout | then | esc |
|---|---|---|---|---|---|---|
| `pick` | `waiting` | wait | owner | `ask_waiting_minutes` | remind | owner |
| `picked` | `deliver` | deliver-handoff | drive | `act_minutes` | | |

## Flow `rollout`: a merged issue's config lines

| state | id | do | owner | timeout | then | esc |
|---|---|---|---|---|---|---|
| `open` | `draft` | tell-orchestrator | orchestrator | `escalate_minutes` | tell-orchestrator, to-owner | |
| `proposed` | `waiting` | wait | owner | `ask_waiting_minutes` | remind | owner |

## Flow `cap`: the line cap

| state | id | do | owner | timeout | then | esc |
|---|---|---|---|---|---|---|
| `near` | `propose` | tell-orchestrator | orchestrator | `escalate_minutes` | to-owner | |
| `asked` | `settle` | tell-orchestrator | orchestrator | `escalate_minutes` | to-owner | |
| `raising` | `release` | release | drive | `act_minutes` | | |

`asked` hands the orchestrator `drover cap`'s queue-wide view: it files the one raise issue or tells the lane to
trim. `release` is `Queued` behind the raise issue and unblocks the asking issue when it merges.

## Flows `pass`, `slot` and `host`: drive's own machinery

| flow | state | id | do | owner | timeout | then | esc |
|---|---|---|---|---|---|---|---|
| `pass` | `due` | `late` | wait | drive | `act_minutes` | raise-card | owner |
| `pass` | `running` | `slow` | wait | drive | `pass_max_secs` | end-pass | |
| `pass` | `paused` | `paused` | wait | owner | owner-held | | |
| `slot` | `held` | `stale` | wait | drive | `spawn_pending_max_secs` | drop-marker | |
| `host` | `holding` | `refusing` | wait | drive | `host_wait_minutes` | raise-card | orch |

- `pass/due/late` is today's `drive-stalled`: work is due and no pass of the fleet finished.
- `slot/held/stale` is #1625: a start slot or an admission lease older than its limit is dropped and logged
  `admission-dropped`; the next issue starts.
- `host/holding/refusing` is one card per host that has refused every start for `host_wait_minutes`
  (`HOST_BUSY`, `DISK_LOW`, `LOGIN_NEEDED` and the rest of the `held` class), not one per waiting issue.

## The timers' pass chart

It stays as it is. `drive::STEPS` and `timer::TICK_STEPS` are ordered step lists, not states under conditions,
and `drover diagram` already draws them from data. Only the pass's own stall moved into rows (flow `pass`).

## Today's rules, and the row that replaces each

| Today | Row |
|---|---|
| `waiting::respawns` ≥ 2 → `LANE_ENDS` | `build/lane-lost`, tries |
| `stall::stalled`: `START_FAILED`, `LANE_STALLED`, the 401 self-heal | `build/working`, `build/lane-ended`, tries |
| `stall::silent`: nudge, respawn, `LANE_SILENT` | `build/working`, `design/working` |
| `stall::stale_check`, `rev-stale-replaced` | `review/reviewing` |
| `pickup`: 3 failed pickups, one re-review, `NO_VERDICT` | `review/record`, `review/no-verdict` |
| `no_proof`, `repro_refused`, `repro_stuck` | `review/refused` |
| `turned`: `TURN_NUDGE`, `TURN_GO_ON`, `TURN_BLOCKED`, `TURN_ASK` | `build/done-no-pr`, `lane-ended`, `lane-blocked`, `need-input` |
| `done::ungated`: 3 refusals at a head | `build/lane-ended` |
| `respawn::guard` (`RESPAWN_LOOP`) and its ask | any spawn row's tries |
| `pending::stalled`, `pending::stuck`, `SPAWN_BACKOFF` | `queued/start`, `build/round-start`, tries; `host/holding` |
| `ci_red`, `gh::unstick`, `ci-missing` | `ready/ci-red`, `ci-running`, `ci-not-started` |
| `update_lane`, `land::auto_update` | `ready/conflict`, inside `merge` |
| `external`: `PR_CLOSED`, merged outside | `*/pr-closed`, `*/merged-outside` |
| `hosts::evacuate` | `*/host-offline` |
| `limit::due` | `*/limited` |
| `design::handoff` | `design/ended` |
| `promise::pass`, `promise-missed` | every drive-owned row's `act_minutes` |
| `undriven.rs`, `orphan-issue`, `ready-unmerged`, check's `stall` | every row's timeout |
| `check::expire`, the routine default, `ask-waiting` | flow `ask` |
| `stranded::drive_stalled`, `pass-slow`, `spawn-held`, `host-busy` | flows `pass`, `slot`, `host` |
