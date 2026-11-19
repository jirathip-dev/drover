Closes #1353

## For humans
A blocked lane's "One more round?" question now closes by itself once its issue gets a new lane, so the board and status stop showing a working lane as waiting on you.
An orchestrator's typed or quoted `extend` now grants the round on a lane that is already working again, instead of refusing it.

## What changed
- `events::closes` + the asks projection (`db.rs`): an extend ask (it has no `agent`) ends `closed: lane gone` on a later `spawn` of its issue's `impl` role. Answered asks keep `answered` (lower rank), so they are never re-closed.
- `orchestrator_extend` (`lib.rs`): with an open extend ask, the needs-human gate is skipped; `--typed` answers the ask through `answer_with` → `gate::extend_ask` (#1299's path), as `--basis` already did. A parked or merged issue still refuses `NOT_NEEDS_HUMAN`.
- `src/ net +9 (budget n/a)`: two closing conditions and one branch in `orchestrator_extend`.

## Repro
Commit 1 (b9ac26fe) holds only the tests; on base `cargo test -p drover --test extend_ask --test orchestrator_reviews` fails: `a_superseding_lane_closes_the_blocked_lanes_extend_ask`, `an_update_lane_closes_the_blocked_lanes_extend_ask` ("the ask closes") and `a_typed_extend_on_a_working_lanes_open_extend_ask_grants_the_round_and_closes_it` (`REFUSED NOT_NEEDS_HUMAN: #36 is fix(1), not needs-human`).

## Evidence at head 46c2af5204ee4898d070294e426c9e829728b438
(merge of origin/staging 6ffb3812 into 3d86a21; round 1's evidence below was at 3d86a21)
- `cargo test -p drover --test drive_sim --test extend_ask --test orchestrator_reviews` → exit 0 (6+6+5 passed)
- `cargo test -q -p drover --test drive_sim a_busy_tick` ×3 → exit 0 each (CI shard 2 failed it at 3d86a21, before staging's #1375 tick.step fix; this diff does not touch tick)
- `cargo fmt --check` → exit 0
- CI's two `cargo clippy --workspace ... -D warnings` lines → exit 0

Round 1 at 3d86a2143d5c3b96600284b9aa09e47585666bde: full `cargo test --no-fail-fast` → 1744 passed, 1 host-dependent fail (`shed.rs` doctor test, macOS-only tools); `drover cap-check` → 54923 / cap 55500.

## Why (round 1)

Decided: an extend ask (no agent) ends `closed: lane gone` on a later impl `spawn` of its issue, in `events::closes` and the asks projection's spawn lookup
Because: "a later `spawn` of the issue's impl role (a first start, `--supersede`, `--update`, a drive respawn) closes it"
Rejected: writing the impl agent into the extend ask — `own()` treats a supersede of the same agent name as the ask's own lane, and `lane-killed` would end it too
Decided: `drover extend --typed|--basis` skips the needs-human gate when an open extend ask exists and answers it through `answer_with`, which calls `gate::extend_ask`
Because: "takes #1299's `gate::extend_ask` path (logs `round-extended`, closes the ask) instead of refusing `NOT_NEEDS_HUMAN`"
Check: an answered ask keeps rank `answered` (lower than lane gone), so a later spawn never re-closes it; a parked or merged issue still refuses via `gate::extend_ask`
Check: extend_ask.rs's fixture now logs the lane's respawn before the ask, since a respawn after it now closes it
Owner: a blocked lane's "one more round?" question now goes away once the issue gets a new lane, and an orchestrator's typed extend works on a working lane

<sub>note on 3d86a21 · git log --notes=why</sub>

