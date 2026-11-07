Closes #605

## For humans
Panes of lanes whose issue is merged or closed no longer pile up. Once such an agent has sat idle for 15 minutes (`idle_merged_minutes`), each drive pass saves any unpushed work to a wip branch, closes the pane and cleans up its worktree. A working agent and an open issue's lane are never touched. If a pane's work holds something that looks like a secret, the pane stays open and this is reported once.

## What changed
- `clean` (run by drive on every pass, and by hand): a merged/closed issue's own lane pane that has not been `working` (herdr `idle`/`done`, or a shed lane's reported state) for `[limits] idle_merged_minutes` (default 15) is closed through `kill --yes`'s path. Unpushed or dirty work goes to `wip/<agent>-<stamp>` first, then the pane closes and the event is logged as `lane-closed` with `why: merged-idle`. After that the worktree is removed the usual way. The dry run prints `would close <agent>'s idle pane …`.
- The idle-since time is kept per agent in the state db (`idle-merged`). Seeing the agent `working` again resets it.
- `SECRET_IN_WIP` or a failed snapshot keeps the pane. It is logged once as `lane-kept-dirty` (keyed by lane) and never retried.
- Only the lane's own agent on its own host is considered. Conductors, other agents in the folder and open issues' lanes stay `keep …: live agent`. Remote hosts match on the agent's `host`.
- `kill` keeps its signature. A private `kill_as` takes an optional `why`, and with one it logs `lane-closed` instead of `lane-killed`. The board file `serve.rs` stays untouched (board-freeze). Its wip snapshot now runs on the lane's host (`On`), not just the local runner, so remote lanes are snapshotted where they live.

src/ net +60 (no budget stated): the close sits inside `clean`'s existing per-lane loop and reuses `kill`.

## Repro
Commit 1 (829d0e9a3) holds only the regression tests. It fails with `cargo test -p herdsman --test drive -- a_merged_issues_idle_lane_pane_closes_after_the_limit_with_its_work_on_wip`.

## Evidence (head 83c003ea5715ea853be3c30691712e40e9dc9c72, origin/staging 7e85bc16c merged in)
- Fix round: CI's red `thread_handoff` test (`a_failed_handoff_tap_shows_its_error_and_its_retry_is_the_one_delivery`) was staging's, fixed there by d08607cf2; merging origin/staging brings that fix in. No code change in this PR for it.
- `cargo fmt --all --check && cargo clippy --all-targets -q -- -D warnings && cargo test -q --workspace --no-fail-fast` → fmt and clippy passed; every test binary that ran passed (0 `FAILED`, drive 76/76 included). Exit 101 only because the host's build sweeper deleted my scratch target partway through ("never executed" on the remaining binaries).
- `cargo test -q -p herdsman` with `--test` for each of those 60-odd remaining binaries (lesson … webpush, thread_handoff included) → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31406 / 31500)
