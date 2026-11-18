Closes #937

## For humans
drover now refuses to start a lane when the installed shed, or the running shed server, isn't the build drover pins. It says which builds it found and how to fix them.
When five lanes' prompts never land within the window, drive stops respawning, raises one needs-human alert naming the shed builds, and sends one push, instead of looping all night.
`drover shed install --yes` installs the pinned shed and restarts the shed server in one step. Without `--yes` it only shows the plan and how many terminals the server is running. Doctor then shows the installed build next to the pin.

## What changed
- `shed.rs`: `gate` reads `shed --version` and the serve hello. It refuses `SHED_VERSION` when either is off the pin (build prefix of `REV`, or semver `proto::VERSION`), or when serve's build differs from the shed on PATH ("restart shed serve"). Doctor's ✓ line adds `; shed on PATH <build>, pin <rev>`. Adds `install` (`drover shed install [--yes]`).
- `spawn.rs`: on the main host, spawn refuses `SHED_VERSION` after `SHED_MISSING`, before any worktree or terminal (skipped on `--dry-run`). Waiting rows show it as `held: SHED_VERSION`.
- `breaker.rs` + `drive.rs`: `unlanded` counts distinct lanes with an unconfirmed spawn (`runtime_down_failures` lanes in `runtime_down_minutes`, so 5 in 30 min). On a trip it holds every respawn and logs one fleet-level `needs-human` `PROMPTS_UNLANDED`, naming the builds on PATH and in serve, which pushes once. A landed spawn or `shed-installed` clears it.
- `roles/usage/30-refusals/SHED_VERSION.md`; the simulator models `shed --version`.

## Evidence (head fdb450d61f7d863319efe33b9e86a65069791456)
- Round 4 fix (CI `rust`, shard 2: `spawn::claude_role_dry_run_uses_synced_profile`): a dry-run spawn ran `shed --version` and then ignored the result. `shed::ready` now returns before the version probe when dry, so a dry run calls only `which shed`. At fdb450d61f7d863319efe33b9e86a65069791456: `cargo test -p drover --test spawn --test shed_pin` exit 0 (28 + 4 passed); CI's two clippy commands exit 0; `cargo fmt --check` exit 0; `env -u DROVER_HOST -u DROVER_LANE -u DROVER_OUTBOX -u DROVER_RULES cargo test --workspace --locked --no-fail-fast` exit 101. Every test binary passes except `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which is host-only (a real `/usr/local/bin/shed` is on the timer's PATH, as above). Doctests could not run here (`rustdoc` would not execute), and CI's nextest does not run them.
- Round 3 fix (arch-check): at 44c5e8d this PR itself pushed `cli.rs` (file and `dispatch`) and `spawn.rs:spawn` over their size baselines and added the `shed` verb with no doc line. Now the verb's argument enum lives in `shed.rs` (cli.rs only has a one-line `Shed(shed::Cmd)` variant and a one-line arm), spawn's SHED_MISSING and SHED_VERSION checks are one `shed::ready` call, and `docs/runtimes.md` documents `drover shed install` and `--yes`. `cargo run -q -p drover -- arch-check --base origin/staging`: exit 1 at 11d67712afb73c6ccc44e235d06f0fa28bad0f2b, and only `serve.rs` is reported (3591 lines vs a 3575 baseline). That file is staging's and this PR does not touch it. Locally `just gen` is also missing (`just` is not installed).
- At 11d67712afb73c6ccc44e235d06f0fa28bad0f2b: `cargo test -p drover --test shed_pin` exit 0 (4 passed); `--test shed_version` exit 0 (5 passed); `cargo test -p drover-core --lib` exit 0; CI's two clippy commands exit 0; `cargo fmt --check` exit 0. `cargo test -p drover --test shed`: exit 101. 55 tests pass. The one failure is `a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, at its doctor assertion (shed.rs:564: this host has no sysctl, no gh login and a real shed). Its spawn assertions, which run the moved SHED_MISSING check, pass.
- Earlier rounds, at 44c5e8d:
- Round 2 fix: CI's `test shards` check failed because `shed_pin` was in no shard. It is now in shard 4 of `scripts/test-shards.txt`. `scripts/test-shards.sh check`: exit 0 at 44c5e8de5bafd701232fd98da7f369e7ea745e65. Merged origin/staging (d0cba31); the only conflict was `cli.rs`'s `Stats` arm, where staging's version is kept.
- `cargo test -p drover --test shed_pin`: exit 101 at 891888d (commit 1, test only), 4 failed; exit 0 at 44c5e8de5bafd701232fd98da7f369e7ea745e65, 4 passed.
- `env -u DROVER_LANE cargo test -q --locked --no-fail-fast -p drover --test shed_pin --test shed_version --test shed --test drive --test overlap --test land`: exit 101 at 44c5e8de5bafd701232fd98da7f369e7ea745e65. Every target passes except one test, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails on a plain origin/staging checkout here. This host has a real shed in ~/.cargo/bin, and the timer's PATH finds it. Neither doctor.rs nor that test is touched here.
- `cargo test -p drover-core --lib`: exit 0. CI's two clippy commands: exit 0. `cargo fmt --check`: exit 0. All at 44c5e8de5bafd701232fd98da7f369e7ea745e65.
- The full `--workspace` run could not link the `sim` test here (ld bus error, disk at 100%). CI covers it.
- Staging's own CI at d0cba31 is red for reasons outside this PR: the sim hunt test (shard 1), a leftover fleet-test temp dir (shard 2), and `serve.rs` over its arch-check size baseline (round 2 wrongly said that was the only arch-check failure; see round 3).

src/ net +163 (budget not stated): the gate, the install verb and the breaker. The cap was raised in #1201 (q1812).

## Not verified
- A real `shed install --force` restart against a live serve, and whether it ends running lanes (shed#75). The verb's dry run shows the terminal count so the owner can decide first.
- Remote hosts are not gated at spawn; they keep the bootstrap pin from `cloud.rs`.

## Why (round 1)

Decided: a shed counts as pinned when its build is a prefix of REV or its semver is proto::VERSION; serve must also run the same build as the shed on PATH
Because: "are the build it pins, or speak its proto version"
Decided: spawn gates on the main host only, after SHED_MISSING and before any worktree; remote hosts keep their bootstrap pin
Because: crates/core/src/spawn.rs:1003
Decided: an unreadable `shed --version` or a serve that does not answer is not a refusal; only a read mismatch refuses
Because: crates/core/src/shed.rs:140
Decided: the breaker counts distinct lanes with an unconfirmed spawn, reusing runtime_down_failures and runtime_down_minutes; a landed spawn or `shed-installed` clears it
Because: crates/core/src/breaker.rs:119
Rejected: new [limits] keys for N and M — the existing 5 lanes / 30 min covers the 5-in-15 case
Decided: install is `drover shed install`, a dry run naming serve's live terminals unless --yes, then cargo install --rev REV and `shed install --force`, read back as doctor's line
Because: crates/core/src/shed.rs:156
Decided: the line cap is raised in one orchestrator-filed issue; this PR waits for it
Because: q1812
Check: cargo test -p drover --test shed_pin fails at 891888d (commit 1 on staging) and passes at the head
Owner: drover now refuses to start lanes on a shed build that doesn't match its pin, stops respawning when prompts stop landing fleet-wide, and has one command that installs shed and restarts it.

<sub>note on d95dc29 · git log --notes=why</sub>

## Why (round 2)

Decided: shed_pin goes in shard 4 of scripts/test-shards.txt, next to the other shed test targets
Because: scripts/test-shards.txt:165
Decided: in the merge, staging's `Cmd::Stats { cmd }` dispatch replaces this branch's old `StatsCmd::Land` arm; the `shed install` arm stays
Because: crates/core/src/cli.rs:1984
Rejected: fixing staging's other red checks (sim hunt, temp-dir leak, serve.rs size) here — outside this issue's files and not caused by it
Check: scripts/test-shards.sh check exits 0 and CI's rust job passes the test shards step
Owner: The CI failure was a missing test-shard entry; it is added and the branch is merged with current staging.

<sub>note on 44c5e8d · git log --notes=why</sub>

## Why (round 3)

Decided: the shed verb's clap enum lives in shed.rs; cli.rs keeps a one-line Shed(shed::Cmd) variant so cli.rs and dispatch stay within their arch baselines
Because: crates/core/src/cli.rs:511
Decided: spawn's SHED_MISSING and SHED_VERSION checks move into one shed::ready call so spawn's function shrinks back under its baseline
Because: crates/core/src/spawn.rs:1002
Decided: docs/runtimes.md's setup step documents `drover shed install` and its --yes flag
Because: docs/runtimes.md:201
Rejected: bumping .drover/arch-baseline.txt — the baselines are meant to shrink, and a trim was possible
Check: cargo run -q -p drover -- arch-check --base origin/staging reports only staging's serve.rs
Owner: The CI layout check now passes for this change; only the shed server file that staging already had over its limit is still flagged.

<sub>note on 11d6771 · git log --notes=why</sub>

