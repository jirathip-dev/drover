Closes #827

## For humans
Orchestrator threads, including ones started before the conductor→orchestrator rename, no longer get a demand to run "herdsman turn" at the end of every turn, so there is no more blocking error and no more wasted extra turn.
Review and verify lanes are never asked either. Implementation lanes are still asked once per prompt, including on hosts whose state folder did not exist yet.

## What changed
- `Fleet::family`: the old role name `conductor` maps to `orchestrator` (`orchestrator::is_role`), and `verify` is always a check (`rev`). Before, both fell through to `impl`, so the Stop hook's `builds` was true for a pre-#780 thread (`HERDSMAN_LANE=<fleet>:-:conductor`). It asked for `herdsman turn`, which refuses there.
- `hook::turn_end` creates the marker's folder before writing, so the "ask once" marker holds where the state folder is missing.
- Test `crates/cli/tests/hook.rs::the_stop_hook_asks_for_a_turn_only_in_impl_type_lanes` runs the real binary. It covers no lane env, orchestrator and conductor lanes, and rev, verify, design and architect lanes, all ending with no block. An impl lane is blocked once with TURN_ASK and not asked again.

`src/ net +1 (budget n/a)`: two match arms changed and one line added.

## Repro
Commit 1 (`8278768`) holds only the test. On it, `cargo test --test hook the_stop_hook_asks` fails, exit 101: `sendmeter:-:conductor` gets the TURN_ASK block.

## Evidence (head c47ae486c68d5a92947ecd142c8c6c0f11956bc9)
- `cargo test --locked --test hook`: exit 0 (42 passed)
- `cargo fmt --check`: exit 0
- `cargo clippy --all-targets --locked -- -D warnings`: exit 0
- `cargo test --locked --no-fail-fast` (three WebKit tests skipped, they hang on this host): running
