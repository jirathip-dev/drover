Closes #1152

## For humans
When the Mac is too busy to take new work and a cloud machine is already running with free room, new lanes now go to that machine right away instead of waiting about 10 minutes. A stopped cloud machine is still started only after the Macs have been busy for the configured wait.
`spawn --dry-run` now prints which machine it picked and why.

## What changed
- `hosts::place`: if no local (non-cloud) host admits with room, the first admitting host with `max_working` room is picked (a running cloud host). Its `why` reads `HOST_BUSY mini: …; cloud1 is up with room`. `overflow_after_minutes` now only gates starting a stopped cloud host (its `CLOUD_STOPPED` refusal keeps it behind). Root cause: a running cloud host waited the same overflow timer, and that timer resets whenever load dips under the limit.
- `hosts::at`: a host at its `max_working` (from the live agents it already lists) ranks after hosts with room and is never the overflow pick.
- Stickiness: a last-lane host that is busy already fell through to the general pick, so it now reaches the running cloud host too (tested).
- `spawn --dry-run` prints `on <host>: <why>`, and its JSON carries `host` and `why`.
- docs/hosts.md "Start on demand" updated. The old test `a_running_cloud_host_waits_behind_the_macs…` asserted the behaviour this issue reverses, so it was rewritten.

src/ net +20 (budget: none stated): placement rule, room rank, dry-run line.

## Evidence at 6b95a0524938387ef097b80cd008cc6a29e77654 (fix round: CI `rust` red at 9adede7)
- The red was shard 2's "no test temp dirs left" step; all of its tests passed (359/359, then 362/362 at 6b95a05). No code change was needed.
- Diagnosis: 69177b3 made that step print what it found. Shard 2 then passed with nothing left, so 6b95a05 reverts that commit (net diff = the same 4 files as before). At 6b95a05 shard 2 failed once on the plain step, then passed on `gh run rerun --failed` (run 43882301887, all jobs green). The dir shows up only intermittently, right as the tests end, so this is a timing flake, not a dir a test always leaves.
- Locally, each test of `cloud`, `spawn`, `drive_sim`, `host_slots`, `host_spared`, `tick_dead_host`, `disk` and `setup_terminal` ran in its own process, as nextest runs them, with a private `TMPDIR`: no `fleet-test-*` left after any test. `cargo test -p drover --test cloud` → 0 (32 passed).

## Evidence at 0378946ef3a7405007708fd5f398174cfef0423b (merge of origin/staging 40fcb58)
- Conflict in `crates/core/src/shed.rs`: took staging's `.ok()` form (#1111 fixed the same clippy error). Staging's `.drover/arch-baseline.txt` taken unchanged (`spawn` 501 fits: the dry-run host line lives in `print_plan`), so this PR no longer touches the baseline file.
- `cargo fmt --check` → 0; CI's two `cargo clippy` commands → 0; `drover arch-check` → 1, only `just gen` exit 127 (no `just` on this host).
- `cargo test -q --locked -p drover --test cloud --test spawn --test drive_kicks --test hosts` (lane `DROVER_*` env unset) → 0 (32 + 37 + 46 + 28 passed); no `fleet-test-*` dir left.

## Evidence at 42e044a889a1e8b2abc4c8822f79c10e462298e1 (fix round: CI red at 9adede7)
- Merged origin/staging (00747d7). **Staging build fix**: staging's own clippy errors (`shed.rs` `let _ =` on a `must_use` Result, `spawn.rs` indexing/slicing from #1140/#1146) and arch-check size baselines staging already exceeded (`tests/hosts.rs` 1791, `tests/shed.rs` 2541, `spawn` 506; same text as open #1107/#1111).
- This branch's own arch-check overage (`spawn` +3 for the dry-run host line) removed: the host/why line is now printed by `print_plan`, so `spawn` stays at staging's 506.
- Dropped the `*.log` files the supersede snapshot committed.
- `cargo fmt --check` → 0; CI's two `cargo clippy` commands → 0; `drover arch-check` → 1, its only violation `just gen` exit 127 (no `just` on this host).
- `cargo test -p drover --test cloud` → 0 (32 passed, no `fleet-test-*` left). `--test drive_kicks` (lane `DROVER_*` env unset) → 0 (37 passed), incl. CI's shard-4 failure `failing_retries_never_starve_another_fleets_start`, which did not reproduce (flaky).
- `cargo test --workspace -j 4 -- --test-threads 4` (lane env unset): 144 binaries 0 failed, then hung in `runner_limits` (unchanged from staging; real-time default limit) and was killed at 30 min; remaining 60 binaries → 101, one failure `shed::a_missing_shed_…` (no `sysctl` on this Linux host, fails on base too, as in round 1). No `fleet-test-*` dir left after that run.

## Evidence at 9adede7c42fc9e3e8227929eb4117b43929058b3 (merge of origin/staging)
- Conflicts resolved in `crates/core/src/hosts.rs` (kept the remote-with-room pick; adopted staging's `on.first()` form) and `crates/core/src/spawn.rs` (staging's `pj.put` with this branch's dry-run host/why).
- `cargo fmt --check` + CI's two `cargo clippy` commands (lib/bins `-D warnings`; tests with CI's `-A` list) → exit 0.
- `cargo test -q --locked -p drover --test cloud` (lane `DROVER_*` env unset) → exit 0 (31 passed).
- `cargo test --workspace --locked -q` → 101: only the 9 `answer_delivery` tests fail, because the lane's `DROVER_OUTBOX` env leaks into them; rerun with `DROVER_*` unset → exit 0 (9 passed).
- `drover cap-check --config ci/cap.toml` → 0 (46503 / 47200).

## Earlier evidence at d8621af782f831c3dbd709fe5e14482828d52e83
- `cargo test -q -p drover --test cloud` → exit 0 (29 passed). On base with only the tests applied: 3 new tests FAIL (running-cloud-at-once, sticky-busy-mac, dry-run-names-host).
- `cargo fmt --check` → 0; `cargo clippy -q --all-targets` → 0; `drover cap-check --config ci/cap.toml` → 0 (45581 / 47200).
- `drover arch-check` → 1: its only violation is `just types` exit 127 (no `just` on this lane host), which is environmental.
- `cargo test -q --workspace` (lane `DROVER_*` env unset) → 101: one failure, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails on base on this Linux host (no `sysctl`). Unrelated to this change.

## Why (round 1)

Decided: a running cloud host that admits with max_working room takes the lane as soon as every local host refuses; overflow_after_minutes only gates starting a stopped one
Because: "placement picks that host instead of holding the issue on the main host"
Because: crates/core/src/hosts.rs:1183
Rejected: shortening overflow_after_minutes — the held timer resets on every load dip, so a running VM still idles
Rejected: max_working as a refusal in at() — would change spawn's HOST_BUSY semantics for every host; it only ranks instead
Check: the stopped-cloud overflow test still holds HOST_BUSY on mini until the wait passes
Owner: busy Mac plus a running cloud VM with room now sends new lanes to the VM at once

<sub>note on d8621af · git log --notes=why</sub>

## Why (round 2)

Decided: merged origin/staging; kept this branch's remote-with-room placement and dry-run host/why, in staging's on.first() and pj.put forms
Because: crates/core/src/hosts.rs:1236
Rejected: none
Owner: branch brought up to date with staging; behaviour unchanged

<sub>note on 9adede7 · git log --notes=why</sub>



