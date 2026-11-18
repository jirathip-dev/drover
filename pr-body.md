Closes #1225

## For humans
A lane that moved to another host no longer flips between "gone" and "working/hibernated" on every pass.
drover now reads a lane's state only from the host it was last started on, and tidies away the finished
copy left on the old host. Running copies are never removed. This stops the false crash-resumes and the
thousands of extra log rows an hour.

## What changed
- `session/stamp.rs` `observe`/`look`: each session is judged against the machine of its lane's latest spawn; a session of the same name on any other host is ignored for `lane-state`, and if it has exited it is removed there (`shed rm`) once that host answers. A running one is left alone (still a `lane-dup`).
- `on_event` (shed's local event stream): events for a lane whose spawn names a remote machine are ignored for `lane-state`, pulse and exits.
- `check` and the board snapshot pass their runner to `observe` so it can reach the old host.

Decided: no extra "flip-back" guard in `stamp` (Wanted 3). After the host filter, no other-host row reaches `stamp`, so the guard would never fire on new data. On log rows written before this change (which carry no host), it could block the live host's correct state and leave a running lane stuck at `gone`.

## Repro (queued --repro)
Commit 1 `39b7b4ec` (+ fmt `a55e819a`) adds `crates/cli/tests/lane_host_move.rs` only.
`cargo test -q --locked -p drover --test lane_host_move`: exit 101 at `a55e819a` (3 of 4 fail: gone/hibernated rows every sweep, old exited terminal kept, stream exit stamps `gone`); exit 0 at head `4a3202f4e58fcf341b87553bef053aa288b61f34` (4 passed).

## Staging build fix (round 2)
The review FAIL was the `sim` hunt test (`the_hunt_finds_a_planted_bug_sooner_than_random_seeds_and_minimises_it`). It is red on staging itself: CI failed at c3d24d5d, 5347b56a and d0cba316 and passed at 873990b1. Locally `cargo test -p drover --test sim the_hunt` gives exit 101 with the same panic (random seed 1003, impl-104) at d0cba316, 5347b56a and c3d24d5d, none of which contain this PR's change.
Cause: since #1195 a fix round starts first when one start slot is free. Fix rounds reuse the impl lane's name, so three starts of an impl lane is now normal and random seed 1003 reaches the planted bug. Without sabotage, seeds 1000–1004 pass (`DROVER_SIM_FROM=1000 DROVER_SIM_N=5 cargo test -p drover --test sim random_seeds -- --ignored`: exit 0).
Fix (test only, `crates/cli/tests/sim/model.rs`): the planted bug now trips only on a rev lane's third start, which is still unusual. Rejected: raising it to the fourth start, because the hunt then misses it within 40 rounds (tried, exit 101).

## Staging build fix (round 3)
The review FAIL was `arch-check`: `crates/server/src/serve.rs` is 3591 lines, over its 3575 baseline. That growth is staging's own (#1108, d0cba316, added 16 lines to serve.rs); this PR never touches the file. Fix: `.drover/arch-baseline.txt` records serve.rs at 3591. The six log files the wip snapshot committed are removed.

## Evidence at head 4a3202f4e58fcf341b87553bef053aa288b61f34
- `drover arch-check --base origin/staging` (built from this head): exit 1, one violation: `just gen` exited 127 (`just` isn't installed on this host; CI has it). The serve.rs size violation is gone.
- `cargo test -q --locked -p drover --test lane_host_move`: exit 0 (4 passed)
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- From round 2 at 25d8d33b (src/ unchanged since): `cargo test -p drover --test sim`: exit 0 (10 passed, 6 ignored); the `--tests` clippy line from ci.yml: exit 0
- From round 1 at 1feaf1ab (src/ unchanged since): `scripts/test-shards.sh check`, `drover cap-check --config ci/cap.toml` (47739 / 53500), `drover lesson-check`: exit 0
- `cargo test --locked --workspace`: not run locally; CI is the evidence for the full suite.

src/ net +16 (budget: none stated): host filter, old-host removal and the runner passed through to `observe`.
