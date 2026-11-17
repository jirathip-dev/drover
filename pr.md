Closes #951

## For humans
Starting a lane, the minute step and the phone page's host check no longer wait a fixed two seconds per host.
Each host is checked in one command, so a remote host costs one connection instead of one per reading.
The swap-out rate is now worked out from the previous check, so the first check after a restart shows no rate and never blocks a start on its own.

## What changed
- `host::sample` runs all its probes in one `sh -c` via the new shared `snapshot::batch` (the batching `Snap::fetch` already had, now reused there) and never sleeps.
- `host::pace` sets `swapout` (pages per 2 s) from the swap-out count's change since the host's last kept count (`swapcount` / `swapcount-<machine>` beside the state file). The first sample has no rate. Called by admission (`hosts::gate`), `survey`, the fast-step history row and serve's sampler.
- The test fakes (common `Fake`, the sim model) answer a local `sh -c` batch probe by probe.
- Test changes, deliberate:
  - `spawn_guard` swap test: seeds the kept count instead of two `vm_stat` replies, and asserts no `sleep` (it asserted `sleep 2`, which AC2 rules out).
  - `herd`'s one-sample swap rate: `0.0` → `null`, the spec's first-sample behaviour (the golden file already had `null`).
  - The sim's exact metrics: re-pinned, because samples no longer spend 2 s of virtual time (queue-to-merge 1012/1337 → 1006/1329 s).
- New `crates/cli/tests/host_sample.rs`:
  - AC2: one process per host, local `sh -c` and remote one `ssh`, zero sleeps, counted through a fake runner.
  - AC3: 600 swap-outs over 30 s gives 40 pages per 2 s; the first sample gives no rate and admits; each host keeps its own count.

src/ net +75 (budget none given): the new `pace` step, the probe list, the shared batch helper replacing fetch's inline copy, and `each_probe` so the demo answers a batch.

## Perf (`--perf`)
Command: `/usr/bin/time -p drover top --json` (one host sample), debug builds of base `01a7b5119` and head `4e04eef41`, 5 runs interleaved on this shared host (load average ~30, so the numbers are noisy):
- base: 6.47 7.16 8.29 5.29 4.32 s → median **6.47 s**
- head: 6.01 5.17 2.72 2.31 2.24 s → median **2.72 s**
- improvement: about 3.7 s per run, in line with the removed 2 s sleep plus the probes that used to run one at a time. The baseline commit records a quieter-host base median of 4.28 s.

## Evidence at head 07ae50c3cf7ccce12543d61c9ac468045fb2deb1
Merged `origin/staging` twice, last at c3d24d5d7 (sim metrics re-pinned again: 926/1249 s). The staging-side clippy errors in shed.rs/spawn.rs that turned CI red are already fixed on staging.
Fix this round: the demo's canned runner (`Canned`) now answers a host sample's `sh -c` batch probe by probe via the new `snapshot::each_probe`. Without it the demo Herd weather read `load ? · mem ?`. Golden `herd.json`: `swap 0` → `swap ?` (one sample has no rate, per the spec).
- `cargo fmt --all --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI's flags): exit 0
- `scripts/timing-check.sh`: exit 0
- `drover cap-check --config ci/cap.toml`: exit 0 (47782 / 53500)
- `drover lesson-check`: exit 0
- `scripts/test-shards.sh check`: exit 0
- `drover arch-check --base origin/staging`: exit 1, only violation `just gen` exiting 127 (`just` not installed on this lane host); serve.rs at its 3575 baseline
- `env -u DROVER_LANE cargo test --locked -p drover --no-fail-fast`: exit 101; 210 targets ok, the one failure `chat_tab::a_message_with_three_images_delivers_all_three_and_eleven_are_refused` (a chat delivery timing test, no host probe in its calls). `cargo test --locked -p drover --test chat_tab a_message_with_three_images`: exit 0 rerun alone (load ~35).
- At b67159b79 (before the last staging merge) the same full run gave exit 0. A run before that, at load ~48, had `service::a_service_killed_mid_spawn…` and `thread_handoff::a_forward_marks…` fail; both pass alone twice each.
