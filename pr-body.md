Closes #1280

## For humans
Idle agent panes no longer use up a host's "working" slots. A slot now counts only while an agent is actually in the middle of a turn, so hosts full of idle lanes accept new ones again.
The working N/cap shown by the hosts list, status and the board now uses this same rule, so it matches what is really running.

## What changed
- `hosts::held_slots`: an ACP (shed-acp) pane holds a `max_working` slot only while `Session::busy()` (shed reports `working`/`running`, i.e. a prompt is running). Between turns (`idle`, `done`, `hibernated`, unreported) it holds none. Non-ACP panes keep the old rule. `slot_before`, `working_full`, placement and `slots_line` (hosts/status/board) all go through `held_slots`, so they all use the new rule.
- The simulator count is unchanged (a sim lane keeps its simulator between turns). The OOM guard stays: a mid-turn lane still holds its slot.

src/ net +1 (budget n/a): a single branch in `held_slots`.

## Repro
Commit 1 (f7f4a0bd) adds only the tests. `cargo test -p drover --test host_slots acp` fails at the base: `mini: working 5/4` (10 ended + 1 mid-turn) and `waiting for a slot: 6/4 working on mini` (4 mid-turn + idle). The fix commit adds `max_in_flight = 0` to the first test's fleet, because its 11 fixture lanes otherwise hit the unrelated in-flight PR cap once the slot check passes. The assertions are unchanged.

## Evidence (head bd3e513bd2aca37932d355015f41b7eb320f65ff)
- `cargo test -p drover --test host_slots` → exit 0 (10 passed)
- `cargo fmt --check` → exit 0
- CI's two `cargo clippy` commands (lib/bins, tests) → exit 0
- `cargo test --workspace --no-fail-fast` → exit 101: the only failure is `shed.rs::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails on base dc199874 on this Linux host (no `sysctl`/`vm_stat`). It is environmental and unrelated.

Not verified: the live-fleet AC (`working N/4` on each boat matching the panes that use CPU). That needs a deploy.
