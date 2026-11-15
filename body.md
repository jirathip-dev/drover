Closes #997

## For humans
`drover doctor` now checks shed-only remote hosts through shed, as the host survey does, and no longer reports them as broken because they have no herdr.
If an old cloud recipe file in your config folder lacks the ssh identity the built-in recipe has, doctor now names that file and says how to fix it.
When bootstrap can't reach a host, its error now names the recipe override file in use.

## What changed
- `hosts.rs`: after #721, `hosts::health` already lists a host's agents through shed (`host::agents`, the call `hosts::survey` makes) and reports a failure as a `! shed:` row. But a `runtime = "shed-acp"` host returned early through `acp_health` and was never listed. Its health now adds the same row, through one `shed_row` helper both paths share. `doctor.rs` is unchanged from staging: its ✓ host row already names shed in place of herdr, and herdr is gone everywhere.
- `cloud.rs`: `stale_override` returns the user's `recipes/<c>.toml` when it has no `identity` but the built-in recipe has one. Doctor adds a ✗ row naming that file, with the fix: delete it or add `identity`. Bootstrap's `can't reach` error names the override file when one is in use.
- Tests: `remote_acp::doctor_checks_a_shed_acp_host_through_shed_as_the_survey_does` covers a shed-acp host with a working shed (✓, shed contacted, no herdr) and one whose shed fails (`! host boat1 shed:`, no ✓). It fails on staging's `hosts.rs` because doctor never contacts that host's shed there. `boat::bootstrap_that_cannot_reach_a_host_names_the_recipe_override_in_use`, plus the stale-override ✗ row asserted in `doctor_flags_a_recipe_subcommand…` and its absence asserted in `the_rates_are_boats…`. Both boat override tests fail without the `cloud.rs` change.
- Staging is merged in (after #721, #966, #1070 and #1023), never rebased: the branch is pushed, and lanes never force-push.

src/ net +33 (budget: none stated). One shared shed-row helper and one small recipe helper.

## Evidence (head 055e4babd63230fc0a12d8d9a19da351940c7d9b)
- Staging merged again (#1023 renamed shed's agent list to `session::live_on`); the `hosts.rs` conflict is resolved by pointing the shared `shed_row` helper at it.
- `cargo fmt --check`: exit 0
- `cargo build --workspace --all-targets`: exit 0
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
- `cargo test --workspace`: exit 101 in the lane, only `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` failing because it reads the lane's own `DROVER_LANE` (`left: Some("herdsman:997:impl")`). `env -u DROVER_LANE cargo test --workspace`: exit 0 (203 of 203 test results ok).
- `drover cap-check --config ci/cap.toml`: exit 0 (44384 / 45400)
- `drover arch-check`: exit 0 (0 violations)

## Why (round 1)

Decided: after #721 removed herdr, hosts::health already lists a host's agents through shed (host::agents, the survey's call) but skipped it for shed-acp hosts; the shed-acp path now adds the same ! shed row
Because: crates/core/src/hosts.rs:899
Decided: doctor.rs is left as staging has it: its ✓ host row already names shed in place of herdr, and it reads health's rows
Because: "The ✓ row lists shed in place of herdr."
Rejected: a separate ✗ shed list check in doctor — it duplicated health's ! shed row and broke hosts' doctor_names_the_failed_host_check test
Decided: the stale-override row sits with the other cloud rows in cloud::doctor; bootstrap's can't-reach names any override file in use
Because: crates/core/src/cloud.rs:373
Rejected: rebasing onto staging as asked — the branch is pushed and lanes never force-push, so staging is merged in
Check: remote_acp doctor_checks_a_shed_acp_host_through_shed_as_the_survey_does fails on staging's hosts.rs (no shed connect to boat1); boat's two override tests fail without cloud.rs
Owner: Up to date with staging after #721, #966 and #1070; doctor checks shed-only hosts through shed and points at an old recipe file with no ssh identity.

<sub>note on b0c7eb2 · git log --notes=why</sub>

