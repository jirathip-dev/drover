Closes #572

## For humans
When drive skips a queued issue because its one-at-a-time group is full, status and the board now list who holds the group right now, not who held it when the skip was logged. An issue that has since merged no longer shows as a holder.
When a slot has opened up, the line says the slot is free and the issue starts on the next pass.
Other skip reasons show how old they are, and a reason older than the issue's last move is no longer shown.

## What changed
- `queue::skipped` now takes the fleet config and returns (board, status) text:
  - `SERIAL_BUSY`: status shows `waiting for a db slot (#4 holds 1/1)`, built from `queue::holders` at read time. The board still shows `skip SERIAL_BUSY`. When the group has a free slot, status shows `db slot free; starts next pass` and the board shows `db slot free`.
  - Any code: the skip is dropped if it is older than the issue's last state change (`last_ts`, where `driven` is passive).
  - Other codes: the skip is dropped once an issue in one of the skipped issue's serial groups merges after it; otherwise the line reads `CAP (why) · 12m ago`.
- `status` and `board` pass the fleet config through.

## Tests
New, in `crates/cli/tests/drive.rs`:
- `a_serial_busy_skip_shows_the_live_holders`: the stored text names #9. After #9 merges, status reads `db slot free` and the board reads `db slot free`. After #4 takes the slot, status reads `(#4 holds 1/1)` and no longer names #9.
- `an_old_skip_shows_its_age_until_the_issue_moves`: a CAP skip shows `· 12m ago`. After a spawn and `lane-killed` put the issue back in the queue, it is not shown.
- `a_serial_busy_item_is_skipped_and_keeps_its_place` is updated for the new text.

## Evidence (head 9719303, after merging origin/staging)
- `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings`: exit 0
- `cargo test --locked --no-fail-fast` with `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target` and the lane's `HERDSMAN_LANE/HOST/OUTBOX/ACCOUNT` unset:
  - 117 binaries reported `ok` and none failed, then the run was killed by a signal (exit 144, cause not found) inside `ui_audit`.
  - The rest (`--test ui_audit --test ui_tokens --test usage --test verify_stage --test waiting --test webpush`) re-ran in the foreground: exit 0.
- `herdsman cap-check --config ci/cap.toml`: exit 0, 28889 / 31500.

Note: with the lane's `HERDSMAN_*` env set, `answer_delivery` fails locally because the CLI queues to the lane outbox. That is environment only; it passes with the variables unset.

src/ net +41 (budget +30): the free-slot branch and the serial-group merge drop each need their own lines.

## Not verified
- The live readback on synergy (`herdsman status synergy` after a db holder merges) needs this build deployed.
