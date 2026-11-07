Closes #573

## For humans
When the main host runs herdsman through a link (like `~/.local/bin/herdsman`), the remote auto-update now builds into the real install folder instead of next to the link. The link on the remote host stays a link to the fresh build. The host health check reads the version from the real binary, so an old real binary shows as out of date.

## What changed
- `hosts.rs`: a new `real_exe` canonicalizes `ctx.exe` before rehoming. `update` takes its `--root` from it (`~/.local/share/herdsman`, not `~/.local`) and runs `profiles sync` with it. `health` reads the version back from it. `cargo install` now writes only the real root's `bin/`, so the remote symlink is never replaced (fix point 2 follows from point 1).
- Test `a_symlinked_herdsman_updates_and_reads_back_its_real_install_root` (crates/cli/tests/hosts.rs): a real tempdir symlink and the fake runner. The current symlinked path plus a stale real binary is reported as a mismatch, and the update argv's `--root` is the real root.
- `src/ net +6 (budget: none set)`: one 4-line helper and its doc comment, used in two places.

## Repro
- Commit 1 (47ce292d) is the test only: `cargo test -p herdsman --test hosts a_symlinked` → exit 101 (`assertion left == right failed: [] left: 0 right: 1`: the stale real binary was not reported).

## Evidence (head 723651fc79a94c910e0264e7332245977739d612)
- CI at this head: `rust` (fmt, `cargo clippy --all-targets --locked -- -D warnings`, `cargo build --release`, `cargo test`, `cap-check`) SUCCESS. Cap: src lines 29481 / cap 31500.
- Local at this head: `cargo build --release` exit 0, `cargo clippy --all-targets --locked -- -D warnings` exit 0, `env -u HERDSMAN_OUTBOX cargo test -p herdsman --test hosts` exit 0 (33 passed). The lane sets `HERDSMAN_OUTBOX`, which changes `a_remote_lane_knows_the_main_host_and_answers_reach_it`'s output, so it is unset for local tests. A full local `cargo test` exited 101 only because the host deleted the build folder mid-run (disk nearly full; `hook.rs:51` spawn NotFound). CI's full run is the evidence.
- Commit 3 fixes the test's own assertion: the fake runner's `called` matches a prefix, so the `--root` check now uses `contains`. The spec and assertions are unchanged.
