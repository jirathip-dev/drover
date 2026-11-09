Closes #688

## For humans
The nightly simulation now also runs a 10-minute "hunt". Instead of only trying random scenarios, it keeps the scenarios that reach something new and tweaks them, which finds rare bugs sooner.
Each bug it finds is cut down to a small scenario and printed as one line you can paste into the fixed seed list or replay. The nightly job uploads the hunt's scenarios and any failures as an artifact.

## What changed
- `crates/cli/tests/sim/hunt.rs` (new): `Plan` holds a run's whole input: scheduler seed, limits, the agenda of timers, owner verbs and faults, and the lanes' queued scripts. It prints as one line and parses back. `Plan::of(seed)` is the seed's own scenario, so `run(seed)` is unchanged. `mutate` changes fault timing (shift, add or drop a load spike or crash), lane scripts (insert or replace) and ordering (scheduler seed). `coverage` reads the sim trace (the events as the yield hook saw them): one `pass code kind step` per event and per drive-pass action, plus metrics (supersedes, most spawns of one lane, worst clock lag). `hunt` keeps a mutant when it reaches a new combination or makes a metric worse. `minimise` drops faults and scripts while the plan still fails the same invariant, and bounds `steps` to the failing step. No compiler instrumentation, no new dependency.
- `main.rs`: `run_plan`, which runs a plan through staging's `run_world` (#685). `Outcome` now carries steps, coverage and the hunt's `peaks`, beside #685's `metrics`. `fixed_seeds_hold_every_invariant` also runs `seeds::PLANS` (new, empty), and `HERDSMAN_SIM_PLAN='…'` replays one printed plan. New `hunt` test (ignored) behind `just sim --hunt [minutes]`: it writes `corpus.txt` and `failing.txt` to `target/sim-corpus` and fails if anything failed.
- `Sabotage::Planted` (test-only): I1 breaks only at a lane's fourth start. Random seeds first trip it at seed 50 (2..130 tried: 50, 68, 72).
- justfile: `sim *args`. `just sim [n]` and #685's `just sim --metrics` work as before, `just sim --hunt [minutes]` is new. `sim.yml`: after the seed batch it runs `just sim --hunt 10` and uploads `target/sim-corpus/` (`if: !cancelled()`).
- `scenario::random` and `seeds::incident` return the config tuple instead of the TOML text, so a plan can carry it.

## Tests (AC)
- AC 2 and 3: `the_hunt_finds_a_planted_bug_sooner_than_random_seeds_and_minimises_it`. The hunt (rng 5, from seeds 2–5) finds the planted I1 within 40 rounds. Random seeds 2..2+round run with the same sabotage do not trip it. The minimised plan has fewer faults and scripts and fewer steps than the original. Its printed seeds.rs line parses and replays to the same I1 failure at the same step.
- A 1-minute real hunt (no sabotage) found and minimised three real failures in 191 rounds: I4 merged-sha (the parked seed 16 class), I9 churn, and an I7 clock lag after a crashed drive (minimised 12 to 3 faults, 5000 to 728 steps, replayed with `HERDSMAN_SIM_PLAN`). These are left for the nightly to report. Fixing them is out of scope here.

## Evidence (head 698dd9a2ad67916fd3d9f4e8969a07268c0e891a, after merging origin/staging b482d843b)
- `cargo fmt --all`: no changes afterwards
- `cargo clippy --locked --all-targets -q -- -D warnings` exit 0
- `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target cargo test --locked --no-fail-fast`: hit the 30-minute limit and was stopped inside `ui_audit`, which drives a browser on fixed port 127.0.0.1:7777 and hangs on this shared host. No web or UI file differs from staging. Of the 134 binaries that finished, the only failure was `self_update::pause_and_resume…`, which picks up this lane's own `HERDSMAN_LANE` (`env -u HERDSMAN_LANE cargo test --locked -p herdsman --test self_update` exit 0). `sim` 7 passed, 4 ignored (incl. the hunt test and #685's metrics tests). The binaries after `ui_audit` (`cargo test --locked --no-fail-fast -p herdsman --test ui_audit_hang --test ui_lock --test ui_tokens --test usage --test verify_stage --test waiting --test webhook --test webpush`) exit 0.
- On the shared lane target dir, `api_v1`, `api_v1_contract` and `brief_golden` failed against goldens from another lane's build. They pass on this lane's own target dir and on a clean staging checkout.
- `herdsman cap-check --config ci/cap.toml` exit 0: src 33922 / 35500.
- `just` is not installed on this host, so the merged `sim` recipe was not run here.

src/ net +0 (budget n/a): all changes are under tests/, the justfile and the workflow.

