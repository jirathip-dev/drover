Closes #641

## For humans
If a host's name matches two saved herdr machines, `hosts bootstrap` now refuses with `HOST_AMBIGUOUS` and lists them. It does nothing else, so no login pane opens on the Mini.
A login (setup) pane for a remote host opens only on that host. If herdr has no machine saved for it, it refuses with `HOST_UNKNOWN` instead.
Recreating a cloud box replaces its saved herdr machine instead of adding a second one, and the log records the swap.

## What changed
- `hosts::profiles` / `hosts::resolve`: list herdr machine profiles for a label (`herdr machine list --json`); refuse `HOST_AMBIGUOUS` (several, listed) or `HOST_UNKNOWN` (none), each with a fix line.
- `cloud::bootstrap`: resolves the label before any step, including `start`. "None" is allowed here because the herdr step adds it. Before `machine add`, it removes the label's old profiles and logs `host-machine-replaced`.
- `setup::start`: a remote host's setup pane requires exactly one profile, so there is no local fallback.
- Fixtures: existing setup and board tests now include the one machine profile they need. `docs/hosts.md` step 0 added.
- Scope 4 (shed machines) is not done: it depends on #637, which hasn't landed.

src/ net +47 (budget not stated): one resolver used at the two entry points, plus replace-on-add.

## Repro
Commit 1 `f7294261b` adds only the tests. `cargo test -p herdsman --test cloud` on it → exit 101 (3 failed: ambiguous, unknown, recreate).

## Evidence (head d5c1568a333861d5201a8af7b3b0482ba3fdbb30)
- `cargo fmt --all --check` → 0
- `cargo build --release` → 0
- `cargo clippy --all-targets -- -D warnings` → 0
- `herdsman cap-check --config ci/cap.toml` → 0 (32536 / 35500)
- `cargo test` → 101: one failure, `outbox::a_remote_lane_only_queues_and_drive_drains_it_once` (drive's `herdr agent list failed (1)` under suite load). It doesn't touch this change. Rerun `cargo test -p herdsman --test outbox` ×3 → 0, 0, 0. Every other test binary passed.
