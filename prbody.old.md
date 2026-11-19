1390
Closes #1363

## For humans
- When a fleet's local clone loses git objects, drover now repairs it on its own at the next fetch, and merges carry on.
- If the repair can't finish (GitHub unreachable), you get one alert, and the repair is retried until it succeeds.
- Fetches of one clone now take turns. `drover doctor` shows each clone's health and the commands that fix it. `drover clean` removes the temp pack files that killed fetches leave behind.

## What changed
- New `crates/core/src/clone.rs`. Every fleet-clone fetch now goes through `clone::fetch`: in baseline, gate, gh, land, overlap, check (wip prune), spawn (local and remote hosts) and why.
  - **Lock:** a flock on `<clone>/.git/drover-fetch.lock`. A second process polls every second through `Runner::sleep`, so the sim scheduler keeps running. Its wait is capped by git's own fetch limit (`runner::limit`).
  - **Heal:** triggered by `bad object`, `did not send all necessary objects`, `unresolved deltas` or `invalid index-pack output`. It runs `ls-remote` first, so nothing is dropped while origin is down. Then it drops only the remote-tracking refs that point at a missing object, runs `fetch --refetch`, then `fsck --connectivity-only`, and logs `clone-healed {clone, dropped, secs}`.
  - **Failed heal:** stays due in the state db (`clone-heal:<dir>`), so later fetches retry it. `clone-broken` is pushed once per state (added to the always-alert kinds).
- `doctor`: one row per clone. It shows `✓ clone <fleet> — <dir> connectivity ok`, or `✗` with the missing object and the `update-ref -d …` / `fetch --refetch` heal commands.
- `clean`: removes `tmp_pack_*` files older than a day from each fleet clone (a dry run lists them).
- The race (why.md): the `drive-<fleet>` and `drive-fast-<fleet>` leases are separate, so a slow pass and a fast pass, plus `merge`, `check` and `spawn`, could all fetch one clone at once.
- `sim` known-fleet metrics re-pinned (`lock_wait` 5 → 6 s, merges at 928 s and 1250 s). Overlapping passes now wait for the clone instead of running git in it together.
- `gate::verdict`: recording the same `verdict.md` at the same sha a second time for one reviewer spawn is now a no-op (exit 0, "skipped: … is recorded already"). The clone lock's timing let the reviewer's kick and a fast drive pass both record one verdict, so the sim fixture replay counted 5 verdicts against the real 4.
- `clone_heal` test: the test clone sets `fetch.prune false`. This host's `~/.gitconfig` now has `fetch.prune=true`, which pruned the broken ref so the fetch never failed and the test went red locally.
- docs/troubleshooting.md §9.

src/ net +207 (no budget given): one new module, `ctx` passed into four fetch helpers so the heal can log, and the verdict dedupe.

## Evidence (head f5d6c884b52889fa74bf3dd16043824ba80c69e7)
- Round 3 at f5d6c884b52889fa74bf3dd16043824ba80c69e7 (staging 70bc331 merged in). CI's `rust` red at 7858f5f was arch-check's `crates/cli/tests/shed.rs` size (2560 > 2557), which came from an older staging; it is 2556 after the merge. The `test` red was the two `sim` tests.
  - `cargo test --locked --test sim`: exit 0, 13 passed / 6 ignored.
  - `cargo test --locked --test clone_heal`: exit 0, 6/6. `cargo test --locked --test overlap`: exit 0, 31/31.
  - Every other test file that calls `drover verdict` (63 binaries): exit 0, except `why_notes::a_clean_ui_check_ends_the_turn_with_its_result`, which also fails on untouched origin/staging on this host.
  - `cargo test --locked --workspace --tests --no-fail-fast` (run before the last gate.rs/clone_heal commit): exit 101. Failures that also fail on origin/staging here: doctor `each_check_fails_with_its_fix_and_exit_one`, handover, lanes_sheet, 4 in self_update, why_notes. Failures that passed on rerun (load): drive, limits, service, thread_handoff, runner_limits. Branch-only failures, both fixed in this round: `overlap::a_verdict_at_the_head…` and the two `clone_heal` tests.
  - `cargo fmt --check` 0; both CI clippy commands 0; `scripts/test-shards.sh check` 0; `scripts/timing-check.sh` 0; `drover cap-check --base origin/staging` 0; `drover lesson-check` 0; `drover arch-check --base origin/staging`: the only violation is `just gen` exit 127 (just isn't installed on this host).
- Round 2 at 7858f5f1e19d964d05816d7655d521a24c564081 (staging merged in, `clone_heal` added to shard 1 of scripts/test-shards.txt, which was the `rust` CI failure): `scripts/test-shards.sh 1..4` exit 0 with no error; `cargo test --locked --test clone_heal` exit 0, 6/6; `cargo fmt --check` 0; both CI clippy commands 0.
- Round 1 lines below were produced at 429020d.
- Repro: commit 1 (`65afa09`) adds only `crates/cli/tests/clone_heal.rs`. `cargo test --test clone_heal` on base + commit 1: exit 101, 0/6 passed (the merge test fails with `fatal: bad object refs/remotes/origin/issue-9`). At head: exit 0, 6/6.
- `cargo test --locked --workspace --tests --no-fail-fast` at 429020d: exit 101. 237 test binaries ok; the only failure is `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails on untouched origin/staging on this host (a real `shed` is on PATH).
- `cargo test --test why_notes two_lanes` run 5×: exit 0 every time.
- `cargo fmt --check` 0; both CI clippy commands 0; `scripts/timing-check.sh` 0; `drover cap-check` 0 (55159 / 55500); `drover lesson-check` 0.
- `drover arch-check --base origin/staging`: one violation, `just gen` exited 127 (just isn't installed on the lane host). No generated file is touched.
- Not run here: doctests (no `rustdoc` on this host).

## Why (round 1)

Decided: every fleet-clone fetch goes through one function, crate::clone::fetch, which holds a flock on <clone>/.git/drover-fetch.lock for the fetch and heals on a missing or bad object.
Because: "one fetch of a clone at a time"
Because: crates/core/src/drive.rs:898 takes drive-<fleet>.lock and drive-fast-<fleet>.lock separately, so a slow and a fast pass of one fleet (plus drover merge, check, spawn) fetched the same clone at once.
Decided: the heal asks origin first (ls-remote), then drops only remote-tracking refs to missing objects, runs fetch --refetch and fsck --connectivity-only.
Because: "Never deletes a local branch, a worktree or anything not on origin: only remote-tracking refs and git's own leftover temp packs."
Decided: a failed heal is kept as a due heal in the state db (clone-heal:<dir>), so every later fetch retries it and clone-broken alerts only the first time.
Because: crates/core/src/clone.rs:22 reads the due heal; once a broken ref is gone a plain fetch exits 0 while a tree is still missing, so fetch success alone cannot clear it.
Rejected: a heal step at the start of each drive pass — an extra network fetch every pass, and merges run outside the pass too.
Rejected: try_lock and skip — a skipped fetch fails the merge for that pass; waiting is bounded by git's own 10 min fetch limit.
Rejected: gc.autoDetach=false on each fetch — it changes every fetch's argv; git's detached auto-gc takes its own gc.pid lock.
Check: crates/cli/tests/clone_heal.rs builds a clone missing staging's tree with a ref to a missing commit; one drive pass heals and merges.
Check: the remote host's clone heals through the same function because spawn passes its ssh runner (a_remote_hosts_clone_gets_the_same_heal).
Owner: a fleet clone that loses objects now fixes itself on the next fetch, and you get one alert only if origin cannot be reached.

<sub>note on 429020d · git log --notes=why</sub>

## Why (round 3)

Decided: every fleet-clone fetch goes through crate::clone::fetch, which holds a flock on <clone>/.git/drover-fetch.lock for the fetch and heals on a missing or bad object.
Because: "one fetch of a clone at a time"
Because: crates/core/src/drive.rs:695 runs a fast pass under drive-fast.lock apart from the slow pass, so both (plus merge, check, spawn) fetched one clone at once.
Decided: the heal asks origin first (ls-remote), drops only remote-tracking refs to missing objects, runs fetch --refetch and fsck --connectivity-only.
Because: "Never deletes a local branch, a worktree or anything not on origin: only remote-tracking refs and git's own leftover temp packs."
Decided: a failed heal stays due in the state db (clone-heal:<dir>), so later fetches retry it and clone-broken alerts only once.
Because: crates/core/src/clone.rs:21
Decided: round 3 records a verdict.md at one sha once per reviewer spawn; a second record of it is a no-op that exits 0.
Because: crates/core/src/gate.rs:264 recorded; the sim fixture replay logged 5 verdicts against the real 4 once the clone lock let kick and drive --fast both record one verdict.md.
Rejected: refusing the repeat with ALREADY_DONE — overlap's a_verdict_at_the_head_a_pure_merge_moved_from_records_and_merges re-runs drover verdict and expects exit 0.
Decided: clone_heal's local clone sets fetch.prune false, so a host-wide fetch.prune=true cannot prune the broken ref before the heal is tested.
Check: crates/cli/tests/clone_heal.rs builds a clone missing staging's tree with a ref to a missing commit; one drive pass heals and merges.
Check: cargo test --test sim passes: the fixture's verdict count matches the real log; the known fleet's metrics are re-pinned (lock_wait 6 s).
Owner: a fleet clone that loses objects now fixes itself on the next fetch, and you get one alert only if origin cannot be reached.

<sub>note on f5d6c88 · git log --notes=why</sub>

