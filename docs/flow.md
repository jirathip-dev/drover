# Flow table

Generated from `crates/core/flow/` (#1649) by `just gen`; CI fails when this file differs. Rows are tried top to bottom and the first match is the row; a blank fact cell is any value. The design is `docs/architecture/flow-table/`.

## Flow `ask`

### `answered` (file `answered`) · reads lane

| id | lane | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `deliver` | running, limited, asleep, done, exited, gone | deliver | drive | `act_minutes` |  |  |  | deliver the answer to the lane |
| `apply` | none | apply-decision | drive | `act_minutes` |  |  |  | apply the answer |

### `orphaned` (file `orphaned`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `close` | close-superseded | drive | `act_minutes` |  |  |  | its episode ended: close it superseded |

### `routed` (file `routed`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `in-thread` | wait | orchestrator | `escalate_minutes` | to-owner |  | with-owner, answered | with the orchestrator thread |

### `routine` (file `routine`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `default` | wait | orchestrator | `ask_default_minutes` | take-default |  | answered | a routine ask: the default is taken |

### `with-owner` (file `with-owner`) · reads lane

| id | lane | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `asker-gone` | exited, gone | wait | owner | `ask_expire_hours` | expire | owner |  | the asking lane is gone |
| `waiting` | none, running, limited, asleep, done | wait | owner | `ask_waiting_minutes` | remind | owner | answered | with the owner |

## Flow `cap`

### `asked` (file `asked`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `settle` | tell-orchestrator | orchestrator | `escalate_minutes` | to-owner |  |  | a cap ask: raise or trim |

### `near` (file `near`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `propose` | tell-orchestrator | orchestrator | `escalate_minutes` | to-owner |  |  | near the line cap: propose a raise |

### `raising` (file `raising`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `release` | release | drive | `act_minutes` |  |  |  | the raise merged: release the asking issue |

## Flow `card`

### `approved` (file `approved`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `run` | run-card | drive | `subject` | kill-run |  |  | run the approved card |

### `posted` (file `posted`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `waiting` | wait | owner | `card_ttl_minutes` | expire | owner |  | the run card waits on the owner |

## Flow `handoff`

### `pick` (file `pick`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `waiting` | wait | owner | `ask_waiting_minutes` | remind | owner |  | the owner picks a thread |

### `picked` (file `picked`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `deliver` | deliver-handoff | drive | `act_minutes` |  |  |  | deliver the hand-off |

## Flow `host`

### `holding` (file `holding`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `refusing` | wait | drive | `host_wait_minutes` | raise-card | orch |  | a host refuses every start |

## Flow `issue`

### `blocked` (file `blocked`) · reads pr, verdict

| id | pr | verdict | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | merged | | record-merge | drive | `act_minutes` |  |  | merged | merged outside drover: record it |
| `pr-closed` | closed | | raise-card | drive | `act_minutes` |  | orch | needs-human | PR closed outside drover: a card |
| `back` | none, open, pending, red, green, conflicting | none, written, PASS, FAIL, refused | reconcile | drive | `act_minutes` |  |  | ready, review, fix | an old blocked state: reconcile |

### `impl`, `fix` (file `build`) · reads lane, host, pr, verdict, ask, turn · in flight

| id | lane | host | pr | verdict | ask | turn | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | | | merged | | | | record-merge | drive | `act_minutes` |  |  | merged | merged outside drover: record it |
| `pr-closed` | | | closed | | | | raise-card | drive | `act_minutes` |  | orch | needs-human | PR closed outside drover: a card |
| `ask-open` | | | | | open | | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | | | | | answered | | deliver | drive | `act_minutes` |  |  |  | apply the answer |
| `reviewed-here` | | | | written, PASS, refused | none | | reconcile | drive | `act_minutes` |  | orch |  | a verdict at a build state: reconcile |
| `pushed` | | | open, pending, red, green, conflicting | none | none | done | spawn-check | drive | `act_minutes` |  |  | review | pushed: review starts |
| `no-change` | | | | FAIL | none | done | no-change | drive | `act_minutes` |  |  | review, ready | done with no change: review the same head |
| `lane-blocked` | | | | none, FAIL | none | blocked | raise-card | drive | `act_minutes` |  | orch | needs-human | the lane is blocked: a card |
| `host-offline` | | offline | | none, FAIL | none | | wait | drive | `host_offline_minutes` · host heard | move |  |  | the lane's host is offline |
| `done-no-pr` | | up, loaded, draining | none | none | none | done | nudge | lane | `stall_minutes` |  |  |  | turn done with no PR: open one |
| `need-input` | | up, loaded, draining | | none, FAIL | none | need-input | nudge | lane | `stall_minutes` |  |  |  | the lane needs input with no ask: file one |
| `round-start` | none | up, loaded, draining | | none, FAIL | none | none, working | spawn-build | drive | `act_minutes` |  |  |  | start this round's lane |
| `working` | running | up, draining | | none, FAIL | none | none, working | wait | lane | `silent_minutes` · heard | nudge, respawn |  |  | the lane is working |
| `working-loaded` | running | loaded | | none, FAIL | none | none, working | wait | lane | `host_wait_minutes` · heard | move |  |  | the lane is working on a loaded host |
| `limited` | limited | up, loaded, draining | | none, FAIL | none | none, working | continue-at-reset | drive | `stall_minutes` · reset | respawn |  |  | rate-limited: continue at the reset |
| `lane-ended` | done, asleep | up, loaded, draining | | none, FAIL | none | none, working | wait | lane | `stall_minutes` · heard | nudge, respawn |  |  | the lane ended its turn with no report |
| `lane-lost` | exited, gone | up, loaded, draining | | none, FAIL | none | none, working | respawn | drive | `act_minutes` |  |  |  | the lane is gone: respawn it |

### `design-review` (file `design-review`) · reads ask

| id | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `ask-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | answered | apply-decision | drive | `act_minutes` |  |  | queued, design, needs-human | apply the answer |
| `no-ask` | none | hand-off | drive | `act_minutes` |  |  |  | no review ask: raise it |

### `design` (file `design`) · reads lane, host, ask

| id | lane | host | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|---|---|
| `ask-open` | | | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | | | answered | deliver | drive | `act_minutes` |  |  |  | apply the answer |
| `host-offline` | | offline | none | wait | drive | `host_offline_minutes` · host heard | move |  |  | the lane's host is offline |
| `no-lane` | none | up, loaded, draining | none | reconcile | drive | `act_minutes` |  | orch |  | no design lane: reconcile |
| `working` | running | up, draining | none | wait | lane | `silent_minutes` · heard | nudge, respawn |  |  | the lane is working |
| `working-loaded` | running | loaded | none | wait | lane | `host_wait_minutes` · heard | move |  |  | the lane is working on a loaded host |
| `limited` | limited | up, loaded, draining | none | continue-at-reset | drive | `stall_minutes` · reset | respawn |  |  | rate-limited: continue at the reset |
| `ended` | done, asleep | up, loaded, draining | none | hand-off | drive | `act_minutes` | nudge, respawn |  | design-review | the design lane ended: hand off for review |
| `lost` | exited, gone | up, loaded, draining | none | respawn | drive | `act_minutes` |  |  |  | the design lane is gone: respawn it |

### `merged`, `closed` (file `ended`) · reads lane

| id | lane | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `leftover` | running, limited, asleep, done | clean | drive | `done_close_minutes` |  |  |  | ended: close its lane and clean up |
| `finished` | none, exited, gone | clean | drive | `done_close_minutes` |  |  |  | ended: clean up |

### `needs-human` (file `needs-human`) · reads ask

| id | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `card-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | the card waits on the owner |
| `decided` | answered | apply-decision | drive | `act_minutes` |  |  | queued, impl, fix, parked | apply the owner's decision |
| `no-card` | none | raise-card | drive | `act_minutes` |  | orch |  | no card: raise one |

### `parked` (file `parked`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `parked` | wait | owner | `owner-held` |  |  | needs-human | parked: unpark returns it to the state it had |

### `queued` (file `queued`) · reads ask

| id | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|
| `ask-open` | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | answered | apply-decision | drive | `act_minutes` |  |  |  | apply the answer |
| `start` | none | start | drive | `act_minutes` |  |  | design, impl | start the first lane |

### `ready` (file `ready`) · reads pr, verdict, ask · in flight

| id | pr | verdict | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | merged | | | record-merge | drive | `act_minutes` |  |  | merged | merged outside drover: record it |
| `pr-closed` | closed | | | raise-card | drive | `act_minutes` |  | orch | needs-human | PR closed outside drover: a card |
| `ask-open` | | | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | | | answered | apply-decision | drive | `act_minutes` |  |  |  | apply the answer |
| `next-check` | none, open, pending, red, green, conflicting | none | none | spawn-check | drive | `act_minutes` |  |  | review | start the next check |
| `stray-verdict` | | written, FAIL, refused | none | reconcile | drive | `act_minutes` |  | orch |  | a stray verdict at ready: reconcile |
| `merge` | green | PASS | none | merge | drive | `gate_check_minutes` |  |  | merged, fix | PASS and CI green: merge |
| `ci-running` | pending | PASS | none | wait | drive | `ci_stuck_minutes` | unstick-ci |  |  | CI is running |
| `ci-not-started` | open | PASS | none | wait | drive | `ci_start_minutes` | unstick-ci, ci-fail |  |  | CI has not started |
| `ci-red` | red | PASS | none | ci-fail | drive | `act_minutes` |  |  | fix | CI is red: a fix round |
| `conflict` | conflicting | PASS | none | update | drive | `act_minutes` |  |  | fix | the PR conflicts: update it |
| `no-pr` | none | PASS | none | reconcile | drive | `act_minutes` |  | orch |  | PASS with no PR: reconcile |

### `review` (file `review`) · reads lane, host, pr, verdict, ask · in flight

| id | lane | host | pr | verdict | ask | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `merged-outside` | | | merged | | | record-merge | drive | `act_minutes` |  |  | merged | merged outside drover: record it |
| `pr-closed` | | | closed | | | raise-card | drive | `act_minutes` |  | orch | needs-human | PR closed outside drover: a card |
| `record` | | | none, open, pending, red, green, conflicting | written | | record | drive | `act_minutes` |  |  | ready, fix | record the written verdict |
| `ask-open` | | | | | open | wait | owner | `ask_waiting_minutes` | remind | owner |  | waiting on the owner's answer |
| `decided` | | | | | answered | deliver | drive | `act_minutes` |  |  |  | apply the answer |
| `recorded-here` | | | | PASS, FAIL | none | reconcile | drive | `act_minutes` |  | orch |  | a recorded verdict at review: reconcile |
| `refused` | | | | refused | none | tell-check | lane | `stall_minutes` | respawn-check |  |  | the verdict was refused: the check fixes it |
| `host-offline` | | offline | | none | none | wait | drive | `host_offline_minutes` · host heard | move |  |  | the check's host is offline |
| `start` | none | up, loaded, draining | | none | none | spawn-check | drive | `act_minutes` |  |  |  | start the check lane |
| `reviewing` | running | up, loaded, draining | | none | none | wait | lane | `check_quiet_minutes` · heard | respawn-check |  |  | the check is working |
| `limited` | limited | up, loaded, draining | | none | none | continue-at-reset | drive | `stall_minutes` · reset | respawn-check |  |  | rate-limited: continue at the reset |
| `no-verdict` | done, asleep | up, loaded, draining | | none | none | wait | lane | `start_minutes` · heard | tell-check, respawn-check |  |  | the check ended with no verdict |
| `lost` | exited, gone | up, loaded, draining | | none | none | respawn-check | drive | `act_minutes` |  |  |  | the check is gone: respawn it |

## Flow `pass`

### `due` (file `due`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `late` | wait | drive | `act_minutes` | raise-card | owner |  | work is due and no pass finished |

### `paused` (file `paused`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `paused` | wait | owner | `owner-held` |  |  |  | the fleet is paused |

### `running` (file `running`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `slow` | wait | drive | `pass_max_secs` | end-pass |  |  | a pass is running |

## Flow `rollout`

### `open` (file `open`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `draft` | tell-orchestrator | orchestrator | `escalate_minutes` | tell-orchestrator, to-owner |  |  | the orchestrator drafts the rollout |

### `proposed` (file `proposed`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `waiting` | wait | owner | `ask_waiting_minutes` | remind | owner |  | the proposal waits on the owner |

## Flow `slot`

### `held` (file `held`) · reads nothing

| id | do | owner | timeout | then | esc | to | say |
|---|---|---|---|---|---|---|---|
| `stale` | wait | drive | `spawn_pending_max_secs` | drop-marker |  |  | a start slot is held |
