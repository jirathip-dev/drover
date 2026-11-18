Closes #1276

## For humans
When a merge is refused at an approved head for a reason another review can't fix (missing repro proof, unmarked serial paths, changed tests, missing perf baseline), drive no longer starts paid re-reviews of that same head. It raises one needs-human card per head, with the refusal as its reason.
A reviewer that finished and left its verdict is recorded before drive starts that review role again, so the verdict is no longer lost.
The verify stage is forced by a "Repro: verify" review only when that is the latest review pass at the current head.

## What changed
- `gate::merge`: `NO_REPRO`, `TESTS_CHANGED`, `NO_BASELINE` and `SERIAL_UNMARKED` log `needs-human` with `sha` and the reason `merge refused <CODE>: …`, once per (code, head). They used to log `blocked`, which `waiting` turned into "review again", hence the rev loop. `STALE_VERDICT`/`VERDICT_FAIL` keep today's flow.
- `drive/pickup.rs` `ingest`: before drive runs a `spawn` for an issue, a check lane's verdict.md that was written since that lane's spawn, is not yet recorded and names the PR head is recorded with `drover verdict`. If that succeeds, the spawn is skipped. It runs after the remote pickup path, so a remote lane gets at most one read per pass.
- `gate::repro_verify`: one rule (the latest rev PASS at the head asked `Repro: verify`), used by `gate::unverified` and `verify::why`. `verify::why` now takes the head.
- Spawn markers: no code change. Staging already drops a marker past `spawn_pending_max_secs`, or one whose pid is dead, and logs `stale-marker` once (`crates/core/src/drive.rs:133`, #812). A new test pins the AC.
- `roles/usage/30-refusals/{NO_REPRO,SERIAL_UNMARKED}.md` say the refusal goes needs-human once per head.

src/ net +97 (no budget given): the verdict ingest (+58) is most of it.

## Repro
Commit 1 (d86e2c50) holds only the tests. `cargo test -p drover --test repro --test drive_kicks` on it: exit 101, with 3 failures:
- `a_merge_gate_refusal_at_a_passed_head_is_needs_human_once_and_no_rev_rerun` (state `blocked`, not `needs-human`)
- `a_finished_rev_lanes_verdict_is_recorded_before_drive_supersedes_it` (0 verdicts; drive spawned `rev-12-r1`)
- `a_later_run_pass_at_the_head_does_not_force_the_verify_stage` (a verify lane was spawned)

`a_laneless_marker_past_the_limit_is_dropped_and_logged_once` already passes there (see above).

## Evidence (head 86816082756fc509f1e83b61ee6c730ec8698cac)
- `cargo fmt --check` + both CI clippy commands (CI's exact flags): exit 0
- `cargo test -p drover --test repro --test drive_kicks --test verify_stage --test drive --test remote_verdict`: exit 0
- `cargo test --workspace --locked`: exit 101. The one failure is `drive::drive_now_on_the_board_is_paired_only_and_toasts_its_line`, a wall-clock assert (`0 s` vs `0.1 s` pass time) that fails only under host load. Alone at this head it passed 6 of 6 (3 runs of the test, 3 of the whole drive suite); on staging it passed 3 of 3. An earlier full run also failed `perf::a_step_makes_one_remote_call_per_host_and_requests_make_none` on its 150 ms page target in a debug build; it passes in release, as CI runs it (exit 0).
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0 (52284 / 53500)
- `drover arch-check --base origin/staging`: only `just gen` fails (`just` is not installed here). By hand: `drover diagram` output matches `docs/workflow.md`, and `cargo test -p drover --test brief_golden` exits 0.
- `scripts/timing-check.sh`: exit 0

## Why (round 1)

Decided: merge refusals no review can change (NO_REPRO, TESTS_CHANGED, NO_BASELINE, SERIAL_UNMARKED) log needs-human with the code and head sha, once per head, in gate::merge
Because: "Others (`NO_REPRO`, `TESTS_CHANGED`, `NO_BASELINE`, `SERIAL_UNMARKED`) make the issue needs-human with the refusal as the card's reason, once per head"
Rejected: a drive-side check in waiting's stale branch — a blocked event from any merge caller would still read as review again; the event kind is the root
Decided: drive/pickup.rs ingest records a check lane's fresh, unrecorded verdict.md at the PR head before drive runs a spawn, after the remote pickup path
Because: "Drive ingests a finished lane's verdict.md before it supersedes or cleans that lane"
Decided: gate::repro_verify is the one latest-rev-PASS-at-head rule; unverified and verify::why both call it
Because: "`verify::why` reads the repro mode from the latest rev PASS at the head, the same rule as `gate::unverified` (one shared helper)"
Decided: no code for stale spawn markers: drive::holds already drops one past spawn_pending_max_secs or with a dead pid and logs stale-marker once; a test pins the AC
Because: crates/core/src/drive.rs:133
Check: repro.rs's three new tests fail on d86e2c50 and pass at the head
Owner: drive stops paying for re-reviews of an approved head when merge refuses it for a reason a review cannot fix; you get one card instead

<sub>note on 8681608 · git log --notes=why</sub>

