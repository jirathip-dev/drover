Closes #835

## For humans
A remote host that drops off the network partway through a timer run now costs that run one connection timeout, not one per old lane on it, so drive's full pass still runs and starts work on the main host.
Each timer run also has a time budget of one tick interval: once it is over, nothing more is sent to other hosts, the cleanup steps after drive are skipped, and the run prints one line naming what it skipped, so the next run starts on time.

## Root cause
The live event log's 11-11 `tick-stuck` events show the tick in `drive`, waiting on `ssh … -oConnectTimeout=10 … air -- test -e <worktree>`: drive's merged-lane cleanup checks every merged spawn's worktree on the Air. The Air answered the pass's 5 s probe, then dropped. Each later call failed with ssh's own exit 255 (`connect to host … timed out`) after 10 s, and was then retried without the shared socket for another 10 s. The pass marked a host down only on exit 124, so every lane cost about 20 s.

## Change
- `hosts::Pass`: a call that fails with ssh's connect failure (255 and `connect to host`) or times out (124) marks the host down for the rest of the pass. A connect failure skips the dead-socket retry, because no shared socket is involved. The #561 retry for a dead socket (`mux_client…`) is unchanged.
- `hosts::Pass::budget` / `over`: once the deadline passes, no remote call is sent (it answers 255 `not sent`), and the calls skipped this way are counted.
- `timer::tick`: the budget is `limits.slow_secs` (one tick interval), and none is set when the config does not load. Past it, `clean`, `trim` and `lesson` are skipped, and the run prints `tick: past its N s budget; skipped: …; K remote calls not sent`. Check and drive always run; past the budget, their remote calls just fail at once.

## Repro
Commit 1 (c605fa37) holds only `crates/cli/tests/offline_tick.rs`. It fails on base: 12 calls to the Air (5 lanes × `test -e` + retry, plus the batch read), and no budget line. It passes at the head.

## Evidence (head b185401cd3e4fca3fb321d9e5b6fa8f72f49b114)
Round 2 removes only comment lines (the review's arch-check finding); no code changed.
- `herdsman arch-check --base origin/staging` → 0 (`903 files, 0 violations`; at 4cc1183 it reported 14)
- `cargo fmt` clean; `cargo clippy --locked --all-targets -- -D warnings` → 0
- `cargo test --locked -p herdsman --test offline_tick --test runner_limits --test tick_stuck` → 0 (2 + 15 + 5 passed)
- On c605fa37 (commit 1), `cargo test -p herdsman --test offline_tick` → 101 (both tests fail)
- `cargo test --locked --no-fail-fast` at this head: stopped by hand while the `*webkit*` tests waited on the host-wide UI lock, so there is no exit code. Before it stopped, 28 binaries passed and 1 failed: `chat_tab::the_thread_follows_its_end_until_you_drag…`, which fails on base too (see round 1). CI runs the UI tests.

src/ net +46 (no budget given): one predicate and the budget in the pass runner, plus the tick's skipped-steps line.

## Why (round 1)

Decided: a pass marks a host down on ssh's own connect failure (exit 255, `connect to host`), not only on exit 124, and skips the shared-socket retry then
Because: crates/core/src/hosts.rs:328
Because: "A host that fails one probe in a pass is marked offline for the rest of that pass"
Decided: root cause from the live log: 11-11 tick-stuck events wait on `ssh air -- test -e <worktree>` per merged lane; the Air passed the probe, then each call failed 255 after 10 s plus a retry, never marking it down
Decided: the tick's budget is one tick interval (`limits.slow_secs`); past it no remote call is sent and clean, trim and lesson are skipped, then one line names them
Because: "tick has a wall-clock budget; past it, it logs what it skipped and ends, so the next pass runs"
Rejected: skipping check or drive past the budget — drive's full pass must run; its remote calls fail at once instead
Rejected: per-lane offline checks in clean_merged or outbox — the pass runner is the one place every remote call goes through
Check: crates/cli/tests/offline_tick.rs fails at c605fa37 (12 calls to the Air) and passes at the head; tick_stuck.rs's dead-socket retry still passes
Check: a remote command that itself exits 255 without `connect to host` in stderr does not mark its host down
Owner: a laptop host that drops off the network now costs the timer one 10 s timeout per pass, not one per old lane, and a slow pass stops after one interval

<sub>note on 425c483 · git log --notes=why</sub>

