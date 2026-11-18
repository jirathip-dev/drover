Closes #1207

## For humans
A lane working on a "cap:" issue may now raise the line cap on any host, including a remote one.
Before, only the main host knew the issue's title, so a remote lane for a cap raise was wrongly refused and needed the owner.
Every other lane is still refused, and a lane whose title is unknown stays refused.

## What changed
- `hook::Rules` gets `cap_raise` (serde default `false`, omitted when false, so other rules files stay byte-identical). Spawn sets it from the main host's title cache when it freezes a remote lane's rules file; the lane can't edit that file (existing `the_rules_file_holds_no_secrets_and_the_lane_may_not_edit_it`).
- The hook's `cap` check also honours `rules.cap_raise`; a local lane still uses the title cache. With no flag and no cache, `cap` stays on (fails closed).
- `rules_stale` carries the recorded `cap_raise` over, so a cap lane is not shown as "rules stale".

## Repro
Commit 1 (ebf83990) holds only the test `a_remote_cap_lane_may_write_the_cap_file_and_no_other_lane_may` (crates/cli/tests/remote_rules.rs). It spawns issue 12 on remote `bazzite` with a `cap:` title and with a plain title, then runs the hook with the remote env (no `state/cache`) on `printf 'line_cap = 53500' > ci/cap.d/x.toml`.
- At commit 1 (re-run 2026-11-17 in a fresh `git worktree add --detach … ebf83990` with its own `CARGO_TARGET_DIR`, no `cap_raise` anywhere in crates/): `cargo test -q -p drover --test remote_rules a_remote_cap_lane` → exit 101, three runs out of three, in a clean checkout of ebf83990 with its own target dir. Failure: `remote_rules.rs:309:9 ... cap: raise the src/ line cap to 53,500: ... REFUSED CAP_FILE`, so the `cap:` lane is refused on the old code.
- At head 9d68f9e8cb90f773611fb1404d0013fa660987af: same command → exit 0

- Commit 3 adds a third row to the same test: a remote lane whose issue title was never cached is still refused `CAP_FILE` (the AC's fail-closed case).

## Evidence at head 9d68f9e8cb90f773611fb1404d0013fa660987af
- `env -u DROVER_LANE -u DROVER_RULES -u DROVER_HOST -u DROVER_OUTBOX -u DROVER_PORT cargo test -q -p drover --test hook --test remote_rules` → exit 0 (47 + 6 passed). The `env -u` is needed because the lane's own hook env otherwise leaks into the hook tests.
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` and CI's `--tests` clippy line → exit 0
- `cargo fmt --check` → exit 0
- `cargo test -q --workspace --no-fail-fast` (same `env -u`) → exit 101. Every test passed except `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails on base e644c4c6 on this host because it has `/usr/local/bin/shed`. The doctests also failed because `rustdoc` couldn't be executed on this host. Both are host-only failures; CI runs the full suite.

src/ net +18 (budget not stated): one serde field, a spawn-time setter and the stale-check carry-over.

