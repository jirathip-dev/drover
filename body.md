Closes #1091

## For humans
The page's [⚙] menu gets a Hosts row. It opens a plain list of your hosts (main first, then A→Z), each with its on/off state and how many lanes run there.
You can turn a remote host off or back on, and reap leftover processes on the main host, each after a confirm.
There are no charts or gauges; host history lives in Beacon.

## What changed
- `crates/server/src/screens.rs`: `/screens/hosts` sheet (the approved design, docs/design/hosts-menu): a row per host with its name, an on/off badge, the kind (main/cloud/remote host) and the live lane count. A remote host gets a confirmed [Off]/[On] (the existing `host` op, which logs `host-off`/`host-on`). The main host gets [Reap N] while leftovers exist (the existing `reap` op). The answer redraws the sheet with a `screen` op. `host`/`reap` are page-only actions like the thread actions, so the cooee manifest is unchanged.
- Round 1's full Hosts screen (gauges, charts, sign-in, providers, sheet streams, host-watch stamp) was dropped and its files went back to staging's code. No web/ change.
- `crates/cli/tests/hosts_screen.rs` + golden `tests/golden/page/hosts.json`: a two-host fleet (mini main, cloud1 cloud host) with leftovers and a lane on cloud1. A paired phone posts off, then on (`host-off`, `host-on` logged), then reap (`kill -TERM 101 102`, `lane-reaped`). An unpaired device gets 401.
- `crates/cli/tests/setup_neutral.rs`: the design folder's byte-identical copy of the farm sprites gets the same MIT-notice exception as `assets/swag/farm/sprites.svg`.
- `docs/serve.md`: the Hosts sheet line replaces "Host tab is not ported".

src/ net +79 (budget: none stated). The screen plus two action arms, all in screens.rs.

## Parity
- 75 (turn a host on or off): `open #1091` → `page-only`, guarded by `parity_75` (the off/on/reap test in hosts_screen.rs, renamed).
- 72, 73, 74, 76, 78 (gauges, sparklines/charts, health/services, tags/spend, provider meters): `open #1091` → `dropped q1784` (the approved bare list; the issue: "No gauges, sparklines, charts, health/services, spend or provider meters").
- 77 (fleet disk/orphans/leftovers) and 79 (provider sign-in): `open #1091` → `open`; this PR doesn't bring them back (only [Reap N] on the main host's row), so the owner decides whether to port or drop them.

## Evidence (head e360bd8abd7510a4c1141b17427fb6bb3ca27f40)
- `bash scripts/parity-check.sh origin/staging`: exit 0 (117 rows, 58 tests, all listed)
- `cargo test -p drover --test hosts_screen --test setup_neutral --test parity_check`: exit 0 (7 passed)
- `cargo fmt --check`: exit 0
Earlier runs, at f04009a (e360bd8 only renames a test and edits docs/parity.md):
- `cargo test -p drover --test hosts_screen`: exit 0 (2 passed)
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` and the `--tests` run from ci.yml: exit 0
- html-lint (the justfile recipe run inline): exit 0
- `cargo test --workspace --locked --no-fail-fast` at 73462752a: exit 101. 215 binaries ok. 2 failures:
  - `setup_neutral::shipped_files_name_no_install`: fixed in f04009a. `cargo test --locked -p drover --test setup_neutral` exit 0.
  - `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`: the lane's `DROVER_LANE` env leaks into the test (`herdsman:1091:impl` vs `owner`). `env -u DROVER_LANE cargo test --locked -p drover --test self_update` exit 0; not this change.
- An earlier run also had a timing flake (`drive_now_on_the_board…`, "0.2 s" vs "0 s" under host load). Rerun alone: exit 0.

## Not verified
Owner phone check: [⚙] → Hosts, turn a host off and back on.

## Why (round 1)

Decided: the full Hosts screen of round 1 (gauges, charts, sign-in, providers, sheet streams) is dropped; the page gets the approved bare list from docs/design/hosts-menu
Because: "No gauges, sparklines, charts, health/services, spend or provider meters."
Decided: the sheet reads fleet.toml's hosts, lanes_on and the reap leftovers count only, not the heavy host tab (df, ps, services)
Because: "Each row: name, on/off state, live lane count."
Decided: [Reap N] sits on the main host's row, only while leftovers exist; the main host has no [Off] (the host op refuses it)
Because: q1784
Decided: host and reap are page-only actions like the thread actions, not added to cooee-plugin.toml
Rejected: listing them in the manifest's [permissions] — that is cooee's contract and its golden permissions fixture would change
Decided: no sheet stream and no host-watch.at stamp; an action's answer redraws the sheet with a screen op
Because: "Watched `procs` sampling (`host-watch.at`) is not restored by this issue"
Check: golden crates/cli/tests/golden/page/hosts.json: mini (main, 2 leftovers, [Reap 2]) first, then cloud1 (cloud host, 1 lane, confirmed [Off])
Check: hosts_screen.rs posts off, then on, then reap through POST /action: host-off/host-on logged, kill -TERM sent
Owner: open [⚙] → Hosts on the phone and turn a host off, then back on

<sub>note on f04009a · git log --notes=why</sub>

