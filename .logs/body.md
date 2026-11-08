Closes #647

## For humans
- Bootstrapping a host no longer gets stuck at the profile sync step: a settings file an old bootstrap left on the host is removed first, and the sync runs with its errors visible.
- When any bootstrap step fails, you see the command, its exit code and the last 20 lines of output, and the same goes into the host's event log.
- `herdsman doctor` now says which host check failed (ssh, probe, herdr agent list, or a shed warning) instead of a bare "unreachable"; "unreachable" means ssh itself failed.

## What changed
- `crates/core/src/cloud.rs` bootstrap: no more `>/dev/null`. A failed step (install or check) prints `` `cmd` exit N `` with the last 20 output lines and logs a `host-bootstrap` event `{step, cmd, code, tail}`. Repair: a remote fleet.toml matching an old bootstrap's content (only `[fleet]` plus `[hosts.<this host>]`) is removed before profiles sync. Bootstrap still writes no fleet.toml (#592), so the role account pins never reach the remote.
- `crates/core/src/doctor.rs` (Amendment A): probe, then the herdr agent list, are checked separately; shed's failure goes through `hosts::health` as a `! shed` row; `unreachable (ssh: …)` only when `ssh true` fails.
- docs/hosts.md updated.

## Repro
Commit 1 (08a37ad7c) holds only the tests; fails on base with
`cargo test -p herdsman --test cloud --test hosts -- bootstrap doctor_names`.
Tests: `crates/cli/tests/cloud.rs` (a fake remote with an old bootstrap's fleet.toml whose roles pin accounts: bootstrap removes it and profiles sync passes; a failing step's error reaches the output and the event), `crates/cli/tests/hosts.rs` (ssh and herdr answer, shed fails → `! shed` row, not `✗ unreachable`).

## Evidence (head 184d99869d52ed26144765de4b5746855af6e39f)
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (src lines 32226 / cap 35500)
- `cargo test -p herdsman --test cloud --test hosts --test shed` → exit 0 (16 + 37 + 22 passed)
- `cargo test --workspace --exclude herdsman` → exit 0; `cargo test -p herdsman --lib --bins` → exit 0
- Full local `cargo test` → exit 101, for a host reason, not a test: the build folder (shared target and scratch alike) was deleted mid-run four times (`could not execute process … No such file or directory`, `SIGKILL`); no test reported FAILED. CI (7 checks, full `cargo test` included) is green at 184d99869d52ed26144765de4b5746855af6e39f.

src/ net +6 (budget: none stated): the failed-step reporter and the repair check, offset by doctor's shorter rows.
