Closes #1126

## For humans
Bringing up a new cloud host now sets up the lane guard there during bootstrap.
The host passes its health check right away and starts getting lanes, with no manual sync step and no spawn needed first.

## What changed
- `hosts bootstrap` on a shed-acp host runs `profiles sync --profile P --role F` there, once per ACP profile of every fleet the host fits. It uses the real drover path #1124 gave bootstrap, the same one the guard checks.
- `hosts::acp_profiles` is now the one list both the health check (`acp_health`) and bootstrap use. Side effect: the health rows list every `✗ git` line first, then the `✗ acp guard` lines.
- New test `bootstrap_syncs_each_fitting_fleets_acp_profile_there_so_the_next_survey_passes_the_guard` (crates/cli/tests/remote_acp.rs): two fitting fleets (`fleet-impl`, `cargo-impl`). It asserts one sync per profile there and no `✗ acp guard` row in the next health survey. The test's fake `profiles sync` now writes the hook for the `--role` it is given.
- Merged `origin/staging` after #1124, then again after #1125 (no conflicts either time; #1125 touched only `cloud::start` and boat.rs).

src/ net +13 (budget: none stated). It is a 10-line loop in bootstrap plus the shared helper.

## Evidence (head 5c66c4fb0d1baa1482c714302d3dd5cbdac2dfa2)
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --all-targets -q`: exit 0
- `cargo test -p drover --test cloud`: exit 0 (24 passed). Before that I had to touch the build script: a stale build-time sha made two cloud tests fail.
- `env -u DROVER_LANE cargo test --workspace -q`: exit 101. The one failure was `shed::a_shed_acp_fleets_thread_runs_its_orchestrator_as_an_acp_session_that_sleeps_and_restarts` (shed.rs:1286). That file is not in this diff. Run alone it passed 3/3 (exit 0), and `env -u DROVER_LANE cargo test -p drover --test shed` passed with exit 0, so I take it as a timing flake under full-suite load.
- `DROVER_LANE` is unset because otherwise `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` reads this lane's identity (exit 101 with it set, exit 0 without).
- The new test with bootstrap's loop removed (at 6b52dc3): exit 101, `✗ acp guard` rows

## Why (round 1)

Decided: bootstrap runs `profiles sync --profile P --role F` there for each profile hosts::acp_profiles lists, the same list acp_health checks
Because: "syncs, on the VM, each profile `acp_health` would check"
Decided: acp_profiles is the one list of (profile, family, adapter) both acp_health and bootstrap use, so they cannot drift
Because: crates/core/src/hosts.rs:983
Decided: the syncs run through bootstrap's exe, which #1124 made the real path the guard checks
Because: crates/core/src/cloud.rs:393
Decided: the per-profile syncs run only for a shed-acp host; the plain `profiles sync` step stays as it was
Because: crates/core/src/cloud.rs:473
Rejected: keeping the main config on the VM so the plain `profiles sync` runs — it needs the main host's fleet.toml there, which bootstrap removes on purpose
Rejected: syncing in spawn only — spawn never places on the unhealthy host, the deadlock itself
Decided: the test's fake `profiles sync` writes the hook for the `--role` it is given, so a lane-orchestrator profile gets its own family
Because: crates/cli/tests/remote_acp.rs:513
Check: with cloud.rs's loop removed the new test fails on `✗ acp guard cargo-impl`, `lane-orchestrator` and `fleet-impl` rows; at head it passes
Owner: Bringing up a new cloud host now sets up the lane guard there, so the host passes its health check and gets lanes without a manual step.

<sub>note on 2bf6901 · git log --notes=why</sub>

