Closes #714

## For humans
Pausing a fleet no longer cuts off its lanes that are still running on other machines: their questions reach your asks list, their end-of-turn reports land, and your answers get typed into them on the next fast step.
Pause still starts no new lanes and runs no review, fix or merge passes for that fleet.

## What changed
- `crates/core/src/drive.rs`: the pass keeps two lists. `live` holds every fleet that matches `only`, paused or not; the outbox drain (#389) and the undelivered-answer retry (#325) use it. `fleets` (unpaused) still drives orphans, repair, passes, starts, `drive-pass` and `last pass`. A fast step takes a fresh look whenever there is a fleet to drain, even if every fleet is paused.
- `docs/lanes.md` (the paused-fleets line): outbox delivery and answers continue while a fleet is paused.
- `crates/cli/tests/paused_delivery.rs` (commit 1, the regression test): a paused fleet with a live remote lane on `air` whose outbox holds an `ask` and a `turn` line. After one `drive --fast` the ask shows in `herdsman asks`, `turn-end` is logged and the outbox is gone. An answer that couldn't be delivered is retyped (`herdsman --machine air agent prompt … DECISION q1: a`) on the next fast step. A slow `drive` then logs no spawn for queued #13, no `drive-pass`, and no `last pass`.

src/ net +3 (budget: none stated): one list becomes two; the cap holds (35879 + 3 of 36740).

## Repro
Commit 1 (`0d0e851`) on staging: `cargo test --test paused_delivery` exits 101 at `paused_delivery.rs:62` (`left: 0, right: 1`: no `needs-decision` after the fast step). At head it exits 0.

## Evidence (head 17c47b74c4b8ba5e5f7b4256505be0a2fbcf2818)
- `cargo build --release`: exit 0
- `cargo clippy --all-targets -- -D warnings`: exit 0
- `cargo test --test paused_delivery`: exit 0
- `env -u HERDSMAN_LANE cargo test --no-fail-fast`: every binary through `turn.rs` ok (138 results, 0 failed). The run hit my 30-minute background limit inside `ui_audit`, a browser test slowed down by other lanes' browser tests on this host. The remaining cli tests (`ui_audit` … `why_notes`), plus core/server/service and the doctests, were run separately: all exit 0 (cli tail 82 passed, 0 failed).
- Local notes: `HERDSMAN_LANE` is unset for the suite because `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` reads the lane's own env var (CI never sets it). `cloud::bootstrap_…` failed once because an incremental build cached an old commit sha in `HERDSMAN_SHA`; after `cargo clean -p herdsman -p herdsman-core` it passes. Neither is related to this change.
