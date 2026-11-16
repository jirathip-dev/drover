Closes #1146

## For humans
A finished verify lane whose folder holds only its own screenshots now closes on time instead of staying open as "dirty".
When the next verify round needs the same lane name and the old lane already gave its verdict, drive replaces it (logged, unsaved work saved first) instead of waiting on "lane live" until someone steps in by hand.
A lane that hasn't given its verdict yet is still protected.

## What changed
- `SHOT_DIRS` gains `shots/` (roles/verify.md writes its screenshots there), so `changes()`/`dirty()` skip it and `clean` removes it.
- spawn's "finished lane" test: a live rev-family lane that has a verdict of its own stage after its spawn counts as finished whatever its pane status says, so spawn reports `finished (...)` and drive's existing retry passes `--supersede`. Before, only a done/idle pane counted; #459's verify pane still reported working, so drive got "live agent(s)" and held.

## Repro
Commit 1 (3585fc24) holds only the tests; on the base it fails:
`cargo test --no-fail-fast -p drover --test done_close --test verify_stage -- verify_lane own_outputs` → exit 101 (`changes()` returned 1 for `?? shots/`; drive held on `REFUSED LANE_LIVE: live agent(s) ["sendmeter-verify-12-r1"]`).
The fix commit also points the test's fake git `rev-parse HEAD` at head B (fixture only, no assertion changed) so the new spawn's sha can be asserted.

## Evidence at head fd6fa679c277f5a9eb1745203ee67c3e0ea5d1d8
- `cargo test --no-fail-fast -p drover --test verify_stage --test done_close -- verify_lane own_outputs` → exit 0 (4 tests)
- `cargo fmt --check` → 0; `cargo clippy --all-targets --locked -- -D warnings` → 0
- `drover cap-check --config ci/cap.toml` → 0 (45566 / 47200); `scripts/timing-check.sh` → 0
- `cargo test --workspace --locked --no-fail-fast` (lane env vars DROVER_LANE/OUTBOX/HOST/RULES unset, else the CLI queues to the outbox) → 101: one failure, `shed.rs a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`. It expects no shed on PATH, but this host has one; this diff doesn't touch shed or doctor.
- `drover arch-check` → 1, only because `just` isn't installed here (`just types` exited 127). CI runs it.

src/ net +5 (budget: none stated); one const entry and one predicate.
