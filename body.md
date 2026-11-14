Closes #968

## For humans

Every way of installing drover (the reinstall line printed after a merge, the doctor fix hint, and the manual and
"Coming from herdsman" steps in the setup guide) now signs a copy beside the installed program and swaps it in by
rename. Nothing re-signs the program a running board server is using. Self-update already worked this way; a test now pins it.

## What changed

- `crates/core/src/gate.rs`: the dogfood reinstall line is `cargo build --release … && sh <src>/scripts/install-gate.sh <src>/target/release/drover <root>/bin/drover && ln -sf …` (was `cargo install --force && signing-identity.sh sign <root>/bin/drover`).
- `crates/core/src/doctor.rs`: the `signature` fix says `sh scripts/install-gate.sh <exe> <exe>` (copies to `<exe>.new`, signs, mv), not `signing-identity.sh sign <exe>`.
- `docs/setup.md`: step 1 and the herdsman cut-over copy the build to `bin/drover.new`, sign it there, `mv` it over. They don't use `install-gate.sh` because its `doctor --config-only` gate needs a config, which doesn't exist yet at step 1. Step 1 now names `install-gate.sh` for later reinstalls.
- Self-update (`crates/core/src/update.rs`) was already safe (signs `live.new`, `rename`), so it is unchanged.

AC1: `git grep -nE "signing-identity.sh sign|codesign --force"` now hits only `docs/setup.md:21` and `:332` (`"$b/drover.new"`) and `scripts/signing-identity.sh` itself.

## Repro

Commit 1 (`9b3f5788c`) holds only tests. On the base it fails `cargo test --no-fail-fast -p drover --test merge --test doctor`:
`merge_into_fleet_repo_prints_reinstall_command_last` and `each_check_fails_with_its_fix_and_exit_one` (the in-place sign line).
AC2's test, `self_update.rs::green_is_built_signed_gated_swapped_synced_and_restarted`, hard-links the live file first. It then asserts the inode changed and the hard link still reads the old contents. It passes on the base too, because self-update already renamed.

## Evidence @ 8ae5bd754506b4c655c8bb36aa5854a31367db4b

Round 2: merged `origin/staging` (CI's red `diagram` test was staging's stale `docs/workflow.md`); `cargo test -p drover --test diagram` → exit 0.


- `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings` → exit 0
- `env -u DROVER_LANE cargo test --locked` → exit 0. The lane's `DROVER_LANE` leaks into `self_update.rs::pause_and_resume…` (it records `by` as the lane), so it is unset here; CI has no such variable.
- `drover cap-check --config ci/cap.toml` → exit 0 (44869 / 45400)

`src/ net -2 (budget n/a)`: the reinstall line moved to named format args.

## AC3: not verified live

A self-update on the mini replaces the live binary and restarts the live board, which a lane may not do. That before/after is the owner's run.
Before (read-only, now): `launchctl print gui/$UID/dev.drover.serve` → `state = running`, `pid = 84598`, `runs = 1`, `last exit code = (never exited)`.

Scratch run on this host (Darwin 27.2), a `drover serve --read-only` from a scratch copy:
- in-place `signing-identity.sh sign` on the running file: inode 417007528 → 417012953, serve pid alive
- `install-gate.sh`: inode 417017029 → 417022789, serve pid alive
- `cargo install --force` over the running file: inode 417054483 → 417059813, serve pid alive

So here `codesign --force` and `cargo install` both already replace the file by rename. The `OS_REASON_CODESIGNING` kill did not reproduce, and `log show --last 7d` has no CODESIGNING entry. This PR removes every in-place path the issue names, but the -9 may have had another cause.
