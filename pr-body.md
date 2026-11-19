Closes #1368

## For humans

A review that finishes without writing its verdict file no longer sits on "verdict pickup failed" for hours: drive restarts that review once by itself within three passes.
If the restarted review also ends with no verdict, you get one card naming the missing file instead of a third try.
A review that is still working is left alone, and after a restart the board shows the new review's age, not the old one's "missed" warning.

## What changed

- `waiting.rs`: when a rev lane has stopped working (it ended its turn or is gone) and left no readable verdict, it now waits on drive instead of nothing. The third failed pickup (or an exited lane with no file) restarts the review in the same round. If that round already had a heal for an earlier lane, it becomes one `NO_VERDICT` card that names the path.
- `drive/pickup.rs`: drive logs one `verdict-pickup-failed` (`no verdict.md at <path>`) per pass for a rev lane that ended its turn: idle or done, after its start grace. I removed the dedupe of identical failures, because it stopped the count from ever reaching 3. The restart is logged as `self-heal` with the spawn it replaced, and the card is emitted here as well. Verify lanes still use their existing stall respawn and card.
- `promise.rs`: a later spawn now voids an older promise, so a supersede no longer carries over the old lane's "⚠ missed" age and error.
- Root cause: a logged pickup failure turned the idle line into a drive line with no next step. That blocked both the exited-reviewer restart and the stall respawn.

src/ net +68 (no budget given in the issue). That is the counter, the once-a-round heal, the card and the promise void.

## Repro

Commit 1 (c45a3be5) is the test alone. `cargo test -p drover --test rev_no_verdict` failed 3 of 4 on the base: no restart, no card (4 rev spawns), and "⚠ missed 11d" after a supersede. All 4 pass at the head.

## Evidence (head f8c5489f6a09400b9243d4cff3d99cb917214699)

Round 2 fix: CI `rust` and every test shard were red because the new test target `rev_no_verdict` was in no shard (`scripts/test-shards.txt: in no shard: rev_no_verdict`). It now runs in shard 1. I also merged current staging into the branch.

- `scripts/test-shards.sh 1..4`: exit 0 each
- `cargo test --locked -p drover --test rev_no_verdict`: exit 0 (4 passed)
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- `cargo fmt --check`: exit 0

Round 1, at 4466fc7b: `cargo test --workspace` had one failure, `shed::a_missing_shed_…`, which also fails on base because this host has no `sysctl` or `gh auth`. cap-check exit 0. arch-check exit 1, but only from base violations.

## Why (round 1)

Decided: a rev lane that ended its turn (idle or done, past its start grace) with no verdict.md logs one failed pickup a pass; on the third the review restarts in the same round, logged `self-heal`.
Because: "after 3 failed pickups drive restarts the review once in the same round; a second time it is one owner card"
Because: crates/core/src/waiting.rs:132
Decided: a second lane of the round ending without a file is one NO_VERDICT needs-human card naming the path; the heal records the spawn it replaced, so a slow async spawn is never mistaken for the second lane.
Because: crates/core/src/waiting.rs:140
Decided: root cause of the hours-long loop: a logged pickup failure turned the idle line into a drive line with no step, so neither the reviewer-exited heal nor the stall respawn ever fired; the line now counts and heals.
Decided: a promise made before a later spawn is void, so a supersede drops the old lane's "missed" age and error.
Because: crates/core/src/promise.rs:131
Rejected: keep the dedupe of equal pickup failures — drive could never count to 3.
Rejected: apply it to verify lanes — their stall respawn-then-card is tested and unchanged (crates/core/src/drive/pickup.rs:69).
Check: crates/cli/tests/rev_no_verdict.rs covers the four AC; sim seed 9591 (rev exits before its verdict) still merges.
Owner: A review that ends without writing its verdict is restarted once by itself; if the retry also writes nothing you get one card with the file path.

<sub>note on 4466fc7 · git log --notes=why</sub>

