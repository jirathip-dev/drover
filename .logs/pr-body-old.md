Closes #713

## For humans
When a tool herdsman calls stops answering (as the login Keychain did on 11-08), herdsman now gives up on it after a set time instead of waiting forever.
Doctor, the board, tick and check keep working through a frozen Keychain; doctor's signature row says when codesign timed out.
Nothing in herdsman can start an outside program without a time limit any more, and CI fails if new code tries.

## What changed
- `crates/core/src/runner.rs`: `limit()` returns a `Duration` for every call, matched on the program's file name, documented beside it one line per entry: default **60 s**; `security`/`codesign` **10 s**; remote `herdr --machine m … list` 20 s (#478); `herdr … --timeout <ms>` that plus 30 s; `git clone|fetch|pull|push` 10 min; `cp`/`rm` 10 min (build-folder copies and deletes); `cargo` 30 min (self-update's build). `within` lost its unbounded branch and takes `&[u8]` input. A call over its limit is killed and returns 124 with the existing `no answer in …` stderr.
- `crates/core/src/gate.rs`: premerge checks (`sh -c "cd … && <check>"`, usually a build or test run) get 30 min via `run_for`, not a table entry for all of `sh`.
- `crates/core/src/events.rs`: `gzip -dc` and the notifier run through `Real`; the notifier becomes `env -u HERMES_* sh -c` (15 s, streams still discarded inside the shell).
- `crates/core/src/webpush.rs`: the `curl` push runs through `Real::within` (15 s, past curl's own 10 s).
- `crates/core/src/doctor.rs`: the signature row reads `…: codesign timed out` when codesign exits 124.
- `crates/cli/tests/runner_limits.rs`: AC2 (`Real` cut at ~1 s with 124 by `run_for`; a `codesign` stand-in with no explicit limit cut at ~10 s; the table) and AC3 (fails on a fixture's stray `Command::new`, naming `crates/core/src/x/mail.rs:3`; exempts `runner.rs`, comments, `tests/`, `build.rs`; finds nothing on the tree). Runs in CI's `cargo test`.
- `crates/cli/tests/wedged.rs` (commit 1, the repro): `security`/`codesign` never answer and the keyring store blocks; `doctor`, `serve` (startup through `/`, `/api/v1/state`, `/api/v1/host`), `tick` and `check` must finish within 10 s of wall clock. A lane on an account with a usage meter makes serve, check and tick read `security` on Linux CI too.
- `.github/workflows/ci.yml`: the layers grep now allows process code only in `runner.rs`.
- `crates/cli/tests/host_offline.rs`: updated to the new table (a local `herdr` call gets the 60 s default, not no limit), as the spec changes it.

src/ net -3 (+88 −91 in `crates/*/src`); the issue sets no budget. Cap: 36678 / 36740 at the merge head.

## Repro
Commit 1 `fb52f6c9d` holds only `crates/cli/tests/wedged.rs`. On staging (`0ad72f46b` + commit 1):
```
cargo test --locked --test wedged
test tick_finishes_in_10_s ... FAILED
test check_finishes_in_10_s ... FAILED
test doctor_finishes_in_10_s_and_shows_the_timed_out_row ... FAILED
test serve_starts_and_serves_the_board_in_10_s ... FAILED
(each: "still running after 10 s (wedged)")
test result: FAILED. 0 passed; 4 failed; ... finished in 10.02s
```
At head:
```
cargo test --locked --test wedged
test result: ok. 4 passed; 0 failed; ... finished in 0.22s
```

## Evidence (head 83e63b2db96614f57f31a36f8811ecd3e642fe62)
- `cargo build --release --locked` exit 0
- `cargo fmt --check` exit 0
- `cargo clippy --all-targets --locked -- -D warnings` exit 0
- `herdsman cap-check --config ci/cap.toml` exit 0 (36678 / 36740)
- `cargo test --locked --test wedged --test runner_limits --test host_offline --test events_log --test webpush --test fleet_notify --test doctor --test merge --test land` exit 0 (9 binaries)
- Full `cargo test --locked` ran at `75ddaf0` (before merging staging), in pieces after the first run hit a 30-min limit: every binary passes. Two needed a second run: `live_board` (a WebKit timing failure under host load; passed alone) and `self_update` (fails only with the lane's `HERDSMAN_LANE` set; passed with it unset, as on CI).

## Not verified
- Doctor's `codesign timed out` row is only asserted on macOS (CI's Linux doctor has no codesign step).
- No run against a real frozen Keychain; the limits are proven with the fake runner and real sleeping programs.

## Why (round 1)

Decided: limit() returns a Duration for every call (60 s default, security/codesign 10 s, longer ones in a table) matched on the program's file name
Because: "`limit()` returns `Some` for every call: a default (60 s)"
Because: crates/core/src/runner.rs:252
Decided: premerge checks (`sh -c "cd wt && cargo test"`) get 30 min through run_for, not a table entry for all of sh
Because: crates/core/src/gate.rs:644
Decided: the notifier runs as `env -u HERMES_* sh -c` through Real, its streams sent to /dev/null inside the shell as before
Because: crates/core/src/events.rs:302
Rejected: an Option<Duration> kept for limit() — a None arm would let an unbounded call back in
Rejected: a sh entry in the table — every short script would then wait 30 min
Check: cargo test --test wedged fails on fb52f6c9d (all four still running after 10 s) and passes at head in under a second
Check: tests/runner_limits.rs names crates/core/src/x/mail.rs:3 in its fixture and finds nothing on the tree
Owner: When a tool like the Keychain stops answering, herdsman now gives up on it after a set time instead of hanging; doctor, the board, tick and check keep working.

<sub>note on 83e63b2 · git log --notes=why</sub>

