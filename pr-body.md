Closes #1244

## For humans
The board's background job is now always the one serving the board port. If some other copy of serve holds the port, the job's copy stops it and takes over, so it no longer exits and restarts in a loop.
Updates restart serve through the launchd or systemd job and never start a copy outside it.
`drover doctor` shows a red "serve job" row when the process on the port isn't the job's own process, along with the restart command that fixes it.

## What changed
- `crates/server/src/hooks.rs`: when the port is held, serve asks the job manager for the job's pid (`timer::job_pid`). If that pid is this process, it stops the port's listeners (`lsof -t`, `kill`) and binds again within `serve_handover_secs`, then serves. It does not exit 0. A serve started any other way keeps the #1081 behaviour (exit 0 on the same build, handover on an older one).
- `crates/core/src/update.rs`: self-update's serve restart is only `timer::restart` (`launchctl kickstart -k gui/<uid>/<label>` / `systemctl --user restart <label>.service`). The handover ask is gone from self-update.
- `crates/core/src/timer.rs`: added `job_pid` (`launchctl print` `pid = N` / `systemctl --user show -p MainPID -p LoadState`), `listeners` (`lsof -nP -t`) and `restart_cmd` (shared with `restart`).
- `crates/core/src/doctor.rs`: new `serve job` row, ✗ when a listener on `[serve] addr` is not the job's pid. The serve rows moved into `serve_rows` so `checks` stays under its size baseline.
- Docs: serve.md, setup.md and config.md (plus the `serve_handover_secs` text in config.rs) describe the new behaviour.

Decided: the job's serve stops the foreign holder instead of asking it for a handover. A #894 handover leaves the listener in a bridge process outside the job, which is the state this issue reports.

## Repro (queued `--repro`)
Commit 1 `dccbd0a3` adds only the three tests. Each one fails on the base:
`cargo test --no-fail-fast -p drover --test doctor --test self_update --test handover -- a_serve_on_the_port_the_job_does_not_own_is_red the_serve_restart_is_the_jobs_own_restart a_job_started_serve_takes` → exit 101 at dccbd0a3. Failures: the job's serve exits 0 with `already serves 09bc29d`, the handover socket was asked, and doctor has no ✗ row.

## Evidence at head e0032ec21f1c593097b267a6ec7478a8f780f206 (origin/staging merged in; the conflict with #1243 kept both: `restart` re-bootstraps a refused job, then runs `restart_cmd`; self-update runs `restart_serve` then `timer::heal`)
Tests were run with the lane's `DROVER_*` env vars unset: `env -u DROVER_LANE -u DROVER_OUTBOX -u DROVER_HOST -u DROVER_RULES -u DROVER_PORT`.
- `cargo test --no-fail-fast -p drover --test doctor --test self_update --test handover` → exit 0 (17 + 7 + 11 passed)
- `cargo test --workspace --locked --no-fail-fast` → exit 101. The only failure is `shed.rs` `a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which depends on the host: this lane host has a real `shed` in `/usr/local/bin` on the timer's PATH. Nothing in this diff touches it.
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` plus the CI `--tests` line → exit 0
- `cargo fmt --check` → exit 0; `scripts/timing-check.sh` → exit 0; `drover cap-check --config ci/cap.toml` → exit 0 (48590 / 53500)
- `drover arch-check --base origin/staging` → exit 1. The one violation is ``just gen` exited 127`` (`just` is not installed on the lane host); CI runs it.

src/ net +58 (budget: none stated). It adds pid lookup, takeover and a doctor row, and removes the self-update handover path.

AC4 (the Mini) can only be checked once this ships.
