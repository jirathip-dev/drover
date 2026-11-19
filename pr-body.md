Closes #1308

## For humans
A drive pass no longer probes every boat once for each issue it starts. Each boat is now checked once per pass, and the boats are checked at the same time instead of one after another.
In a test where every boat call costs 0.7 s, a pass over 30 boat lanes and 10 starts went from about 96 s to about 5 s.
The slow-pass alert now names the slowest part of a fleet's pass and lists how many calls each boat took and how long they took.
Each accepted GitHub webhook delivery now gets a line in the tick log, and doctor's webhook row counts accepted deliveries next to refused ones, so you can check from the Mini that the webhook works.

## What changed
- `drive` launch: placement's host reads (shed `ls`, host sample, `df`) run once per host per fleet pass. They are memoized in launch only; the kicked spawn still reads fresh before it starts the lane.
- `hosts::each`: placement, the snapshot's per-host batch and the per-host session listing now run each host on its own thread.
- Each fleet pass's runner counts ssh calls and seconds per host. `drive-pass` logs `steps` (closed issues, spawns, PR reads/gates/steps, settle) and `remote` ({host: {calls, secs}}). `drive-slow` reads e.g. `…; slowest step: spawns (2.2 s); remote: b1 4 calls 2.8 s, …`. Docs: docs/serve.md row.
- Round 2, amendment q2010: `webhook::deliver` logs each signed (accepted) delivery to `tick.log` as `<ts> webhook <event> <repo>: <result>` (e.g. `refreshed sendmeter #7`, `pong`, `ignored: …`) and bumps a `webhook.accepted` counter beside `webhook.rejected`. Doctor's row reads `webhook — secret set · N accepted · M refused (bad signature)`. Test: crates/cli/tests/webhook.rs asserts the tick.log line and the doctor count after a real signed delivery, and asserts that 401s write no line.
- Round 3: the pass's step timer (`Laps`) and placement's probe memo moved into `drive/remote.rs`, so `drive.rs` is back at its 2185-line size baseline (arch-check). The `drive-pass` keys are unchanged; one more step, `settle`, was already there.
- Tests: crates/cli/tests/perf.rs adds a 4-boat fixture where every ssh call costs 700 ms. common/fake_shed.rs's `OverSsh` now also answers the plain `shed connect` batch line, not only `--autostart`.

## Perf (queued --perf)
Command: `cargo test -p drover --test perf a_pass_over_30_lanes -- --nocapture`
- before (base 9ea69d9f plus the baseline commit's test): 95.7 / 96.1 / 95.8 s, median **95.8 s**, 136 remote calls
- after (head c5f37130fd0b2a81302ca93b3ba80b8eca89585f): 5.4 / 5.4 / 5.5 s, median **5.4 s**, 28 remote calls
- about 18× faster (95.8 s → 5.4 s). The runs are repeatable to ±0.3 s because the fake's 700 ms sleep dominates.

## Evidence at head c5f37130fd0b2a81302ca93b3ba80b8eca89585f (round 3, staging dd475fcf merged in)
- `cargo test -p drover --test perf -- --test-threads=4`: exit 0 (20 passed). The 30-lane pass reports `drive-slow: … took 2.9 s; slowest step: spawns (2.2 s); remote: b1 4 calls 2.8 s, …`
- `cargo test -p drover --test drive --test drive_kicks --test hosts --test host_slots --test host_spared --test host_deadline --test serve_health --test outbox --test drive_sim --test lane_host_move --test webhook -- --test-threads=4`: exit 0
- `cargo fmt --check` exit 0; both CI clippy commands exit 0; `scripts/timing-check.sh` exit 0; `drover cap-check --config ci/cap.toml --base origin/staging` exit 0 (53359 / 53500)
- `drover arch-check --base origin/staging`: the size violation is gone (drive.rs 2185 lines, baseline 2185). The only violation left is that `just gen` is not installed on this host; CI runs it.

## Evidence at head abf3ab03f19f21d61abf5d034cb1a0a189b4a2c0 (round 2, staging merged in)
- `cargo test -p drover --test perf -- --nocapture`: exit 0 (20 passed). The 30-lane pass reports `took 2.9 s … remote: b1 4 calls 2.8 s, …`
- `cargo test -p drover --test webhook --test doctor --test drive --test drive_kicks --test hosts --test host_slots --test host_spared --test host_deadline --test serve_health --test outbox --test drive_sim --test lane_host_move -- --test-threads=4`: exit 0
- `cargo fmt --check` exit 0; both CI clippy commands exit 0; `scripts/timing-check.sh` exit 0; `drover cap-check` exit 0 (53300 / 53500)

## Evidence at round 1 head f23655ba304a983dc52cb9d1d040a4f66c66bcd0
- `cargo test -p drover --test perf`: exit 0 (20 passed)
- `cargo test -p drover --test drive --test drive_kicks --test hosts --test host_slots --test host_spared --test host_deadline --test serve_health --test outbox --test drive_sim --test lane_host_move -- --test-threads=4`: exit 0
- `cargo test -p drover --no-fail-fast -- --test-threads=4`: exit 101. The only failure is `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which fails identically on base 9ea69d9f on this host because `shed` is on its PATH.
- `cargo fmt --check` exit 0; both CI clippy commands exit 0; `scripts/timing-check.sh` exit 0; `drover cap-check` exit 0 (52998 / 53500)
- `drover arch-check --base HEAD~2`: the only violation is that `just gen` is not installed on this host

src/ net +192 against staging dd475fcf (no budget given): two runner wrappers (memo, per-host tally), the thread fan-out helper, the alert text and the webhook log and counter (+28).

## Decided / not done
- Not done: spawns still run inside the fleet's pass. Making them non-blocking belongs to the q2010 track's per-fleet loops step.
- Not done: one remote JSON script per host, and a per-process-tree ControlPath. The snapshot already sends one batch per host, and the measured cost was serial placement probes. The `ControlPath=none` up-probe is unchanged (once per host per pass, now in parallel).
- Not verified: AC 3, a real tick on the Mini under 3 min. Read the next `tick.log` `drive-slow` lines after this lands; they now name the host and the call counts.
