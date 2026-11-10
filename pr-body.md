Closes #775. Part of #776: the runner side only. Placement skipping a hung root and `hosts eject` not hanging are still open there.

Hotfix for the 2026-11-10 NVMe hang. An earlier commit (5822f1c) is already installed on the Mini.

## For humans
When a worktree disk stops answering, the board stays up. A screenshot, design or thread file on that disk shows "disk not answering" (504) instead of freezing every page.
Herdsman no longer piles up stuck `git` calls for a lane on that disk: one call waits at most, the rest fail fast, and a call that can't be killed is given up at its time limit.

## What changed
- `crates/core/src/runner.rs`
  - `settle`: a call is watched up to its limit. At the limit its child is killed and **abandoned**: it goes on a reap list that later calls poll, and nothing waits on it, so a child stuck in kernel I/O never holds its caller.
  - One call per path at a time (a `-C` folder, a served file). A later call waits its turn within its own limit, or answers 124 at once (`Busy`) while an abandoned one still lives.
  - A `-C <dir>` on a disk that stopped answering fails at once (#776 probe). A timeout marks the whole disk hung for 5 min, shared across herdsman processes under `~/.cache/herdsman/sick-disks/`.
  - `read_file`: serve's file reads run off the request thread within `PROBE` (2 s) and return a `TimedOut` error.
- `crates/server/src/serve.rs`: `/shot/`, `/design/` and `/thread/` answer `504` for that file alone on a timeout, and `404` when the file is missing.

src/ net +278 (no budget given): the reap list, the per-path claim and the time-limited read are the three mechanisms the issue asks for.

## Acceptance tests
- AC1 `crates/cli/tests/abandoned_child.rs`: a fake child ignores SIGKILL. `settle` returns `TimedOut` within limit + 1 s, kills it once, and afterwards nothing polls it.
- AC2 `runner_limits.rs::a_hung_lane_call_is_never_started_again_while_it_lives`: a lane's call hangs for good. Across 10 ticks only one child is started, and the other 9 answer `Busy` at once.
- AC3 `verify_stage.rs::a_shot_on_a_hung_disk_answers_504_and_the_board_stays_up`: the screenshot's folder stops answering. `/shot/…` answers 504 within the limit, and `GET /` answers 200 before, during (5 requests concurrent with the shot) and after.
- Also: `hung_read.rs` (a FIFO read blocks: timeout at `PROBE`, and a second read answers at once) and `runner_limits.rs::calls_on_one_path_run_one_at_a_time_and_a_dead_child_frees_its_path`.

## Evidence (head HEADSHA)
- `cargo fmt --check` → exit 0
- `cargo clippy --all-targets --locked -- -D warnings` → exit 0
- `cargo test --locked -p herdsman --test runner_limits --test abandoned_child --test hung_read --test verify_stage` → exit 0 (15 + 1 + 1 + 17 passed)
- `cargo test --locked --workspace` → FULLRESULT

## CI note
`board-freeze` fails until #775 carries a `board-fix` line: the 504 acceptance criterion needs `serve.rs`. I filed an owner ask (Q1045) to add it.
