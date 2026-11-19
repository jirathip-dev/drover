Closes #1350

## For humans
After a self-update, drover now checks that the board really came back. If it didn't, drover tries once more, then sends one alert that includes the launchctl error.
Restarting a job now waits until launchd has fully removed it, and retries a failed start, so the "Bootstrap failed: 5" race no longer leaves the board down.
Every `check` pass also restarts the board job if it is installed but not loaded, so the board is never left down until someone notices.

## What changed
- `timer::rebootstrap`: after bootout, polls the job's probe (`launchctl print` / `systemctl is-active`) up to 10 × 1 s until the job is gone, then bootstraps with 3 tries (1 s, 2 s backoff). The interval jobs (`heal`) use the same path.
- `update::restart_serve`: after the restart, waits up to 10 s for a pid and a TCP connect on `[serve] addr`. If neither comes, it re-bootstraps once; if serve is still down, it logs `serve-down` (push) with the launchctl errors instead of `self-update-failed`, so one alert goes out.
- `check`: `timer::revive` bootstraps an installed serve plist/unit whose job is not loaded and logs `serve-revived`.
- docs/releases.md: one paragraph on the above.

## Repro (queued --repro)
Commit 1 `b33a8823` holds only the tests. On staging b9f15b2c, `cargo test -p drover --test self_update --test timer` fails on all three:
- `a_serve_bootstrap_hitting_error_5_after_bootout_is_retried_until_serve_is_up` (today: `✗ … failed (5): Bootstrap failed: 5`, exit 1)
- `a_serve_restart_that_keeps_failing_alerts_serve_down_once_with_the_error` (today: no `serve-down`)
- `check_bootstraps_an_unloaded_serve_job_once` (today: 0 `serve-revived`)

## Evidence at head ed355194a8c3299cb273ede8654b285d7ab9299d
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` plus the CI `--tests` invocation: exit 0
- `scripts/timing-check.sh`: exit 0
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0 (53741 / 55500)
- `drover arch-check --base HEAD~3`: 1 violation, `just gen` exited 127 because `just` is not installed on this host. No generated files are touched; CI runs it.
- `cargo test --workspace --locked --no-fail-fast`: exit 101. Every binary passes except `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which fails the same way on untouched `origin/staging` on this host (host-dependent, not this change); CI is the judge.
- Existing self-update fixtures (`self_update.rs`, `host_sync.rs`) now reply a pid and bind a real listener for `[serve] addr`, since the restart is confirmed.

src/ net +78 (budget: none stated): retry/wait loop, serve confirmation with the one-alert error type, and the check-pass revive.
