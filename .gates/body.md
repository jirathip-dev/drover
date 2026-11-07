Closes #509

## For humans
A crash or a racing pass can no longer start the same lane twice or merge the same PR twice. Before it acts, each spawn or merge now writes down what it is about to do. A second pass trying the same step skips it and says so. After a crash, the next pass checks GitHub or the panes before doing anything, so a merge that went through is recorded and never repeated. Kicks from lane hooks, verdicts and answers carry an id, so the same trigger is driven once.

## What changed
- `crates/core/src/action.rs` (new): `begin` writes an `intent` with a stable `action_id` (`<fleet>#<issue>:<step>@<key>`) in one SQLite transaction (`db::claim`) that first reads that id's latest outcome:
  - `done`: skipped, `ALREADY_DONE`.
  - an intent that is not yet timed out and whose process is still alive: skipped, `IN_FLIGHT`.
  - otherwise the intent is recovered: `probe` checks the real world. If the step happened, it records `done` (`recovered: true`). If not, it records `failed`, and this caller logs a new intent and acts. That failed-plus-new-intent pair is written atomically, so two recoverers can't both act.
  - The `Intent` guard logs `done` explicitly. If dropped on an error it logs `failed`. A panic leaves it open, the same as a crash.
- **spawn / supersede** (`spawn.rs`): the intent is written after every gate and before the first side effect. Its key is `role@<seq of the issue's last spawn, as this caller saw it>`, so two passes acting on the same state share an id. Recovery counts the step as done if a `spawn` was logged after the intent, or if the lane's agent is up. `done` is written just before the `spawn` event; a crash between the two leaves a lane the existing orphan path adopts.
- **merge** (`gate.rs`): the intent (key: head sha) wraps `gh pr merge`, and `done` follows it. Everything after the merge (the `merged` log, landing, evidence comment, issue close, teardown) moved into `merged()`. Before the PR lookup, merge checks for a merge action that has no `merged` log after it:
  - in flight: `IN_FLIGHT`.
  - otherwise it reads `gh pr view --json state`. If the PR is MERGED, merge records `done` and runs `merged()`, with no second `gh pr merge`. If not, it records `failed` and carries on as before.
- **kicks** (#304): `herdsman kick --id <id>`, also carried through the service socket. A new `processed` table in state.db records `<fleet>#<n>:<why>:<id>`, and a repeated id prints `already processed` and drives nothing. These callers pass an id:
  - verdict: `<sha>-<PASS|FAIL>`
  - answer: `<ask id>-<by>`
  - a lane hook's push: the hook's `tool_use_id`
- `intent`/`done`/`failed` are in `PASSIVE`, so they don't change any issue's state or its last activity. Drive words: "skipped: another pass is on it or did it".
- Phase C (#345): each step derives its own id from its inputs, so a per-issue worker gets the same ids as drive and kicks. This is documented in docs/architecture.md.

Decided, not done: **label**, **verdict record** and the separate **close** get no intent of their own.
- `sync_label` reads the PR's labels before acting, so a retry is harmless. A stable id there would wrongly skip a toggle back at the same head.
- A verdict record is a single log write and already dedupes per sha.
- The issue close is part of the merge action's `merged()` tail, so it is recovered with the merge.

## Tests (crates/cli/tests/action_ids.rs)
- `two_passes_racing_the_same_spawn_start_it_once_and_the_other_skips`: drive pass A is stopped at its `herdr worktree` call (intent logged, nothing created yet). On a second thread, a `kick --now` pass drives the same issue and gets `REFUSED IN_FLIGHT: skipped: sendmeter#12:spawn@impl@0 in flight`. Result: one `herdr agent start`, one `spawn`, one intent/done pair, and a `driven` with code IN_FLIGHT.
- `a_crash_between_merge_and_its_log_is_recovered_from_the_merged_pr_without_merging_again`: the runner panics right after `gh pr merge` succeeds, which leaves only the intent. Within the timeout the next pass is `IN_FLIGHT`. Past it, the pass reads the PR as MERGED and logs `done` (`recovered`) and `merged` (m3rg3). `gh pr merge` was called exactly once.
- `a_timed_out_merge_intent_whose_pr_never_merged_fails_and_merges_once`: a stale intent whose PR is still OPEN gets `failed`, a new intent, one merge, and `done`.
- `the_same_kick_id_is_driven_once`: the same `--id` twice gives `already processed` the second time; a new id is driven. Two `kick` passes in total.
- Existing tests adjusted for the new bookkeeping events: fallback.rs and spawn_guard.rs filter out `intent`/`failed`; service.rs passes the new `id` argument.

## Fix round 1
- The layers check failed because `action::dead` called `std::process::Command` (`kill -0`) inside core. It now goes through the `Runner` (`r.run("kill", ["-0", pid])`), and `begin` takes the runner; gate.rs and spawn.rs pass theirs in. The layers grep (ci.yml:44-45) is now empty.
- action_ids.rs fakes now answer `kill -0`. The Fake exits 1 on any command it has no reply for, which would read the live test process's intent as dead.
- Merged origin/staging. The `crates/core/src/lib.rs` conflict was resolved by keeping both `pub mod action;` and `pub mod card;`.

## Evidence @ b99891ddeb42940094cd29d0dbca9341c8614beb
Head b99891d is 17727c3 with `origin/staging` (e8491e1f) merged in cleanly; no other changes. All runs used `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target`.
- `cargo build --release`: exit 0
- `cargo clippy --all-targets -- -D warnings`: exit 0
- `cargo fmt --check`: exit 0
- `herdsman cap-check --config ci/cap.toml`: exit 0, 29270 / 31500
- `cargo test -p herdsman --test action_ids --test spawn_guard --test spawn --test service --test fallback --test asks`: exit 0
- `cargo test --no-fail-fast`: exit 101, but no test failed. 61 targets passed (`test result: ok`), with no `FAILED` and no panics. The other 68 test binaries never ran: the host reclaimed the scratch target in the middle of the run (`could not execute process ... No such file or directory`). CI's `rust` job runs the full suite at this head.

src/ net +292 (budget: none stated). That covers the new action module, the atomic claim plus the processed table, merge recovery, and the kick id plumbing.


