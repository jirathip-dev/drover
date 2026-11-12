Closes #844

## For humans
Every waiting time herdsman uses (ssh connects, account probes, the database's busy wait, cloud idle stop and the rest) can now be tuned in the fleet's limits section; the defaults are today's values, so nothing changes unless you edit them.
`herdsman config show` lists every limit with its value and a one-line explanation, and the config docs carry the same table.
CI now fails a pull request that hard-codes a new waiting time in the core crate.

## What changed
- `[limits]` gains `ssh_connect_secs` (10), `access_probe_secs` (30), `db_busy_secs` (60), `cloud_idle_stop_minutes` (20) and every other literal core timing (ssh quick/call, db retry, notify, gate check, shed read, proxy/service connect, disk probes, poll, tick beat, orchestrator idle/change, token refresh, ask-waiting, outbox-stale, host-offline), defaults = today's values.
- Each key (old and new) has a doc line in `config.rs` (`LIMIT_DOCS`) and in `docs/config.md`'s limits table; `herdsman config show` prints them.
- `scripts/timing-check.sh` (CI step in `ci.yml`): a literal `Duration::from_secs(`/`from_millis(`/`from_mins(` in `crates/core/src/**` outside `config.rs` and `#[cfg(test)]` fails.
- **Not done: `card_wait_secs`.** No run-card view wait exists at head; `card::settle` runs synchronously and follows the card's `--timeout` (`crates/core/src/card.rs:151`). A key nothing reads would be dead config; the change that adds such a wait (#821) should add its key.

- Raises the `help --agent` line budget by 3 for the new `config show` entry (staging sat exactly at the old cap).
- **Staging build fix:** `crates/cli/tests/needs_fold_ops.rs` (from #871) still called `swift()`, which #862 removed; it now uses `webview()`/`no_webkit()` like the other WebKit tests. Without this, staging's test build fails to compile.

src/ net +192 (no budget stated in the issue): about 35 keys, each with a field, default and doc line.

## Evidence (head `1727cce63ab18cc197474e42f1bc77c51e6d3f1a`)
- `cargo fmt --all --check` → exit 0
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0
- `scripts/timing-check.sh` → exit 0 (after the staging merge: #894's 300 s serve handover wait is now `[limits] serve_handover_secs`; #854/#855's `promise_passes`, `slo_idle_minutes`, `slo_tick_minutes` got doc lines and docs rows)
- `herdsman cap-check --config ci/cap.toml` → exit 0 (43138 / 45400)
- `herdsman arch-check --base origin/staging` → exit 0 (928 files, 0 violations)
- `env -u HERDSMAN_LANE cargo test --workspace --locked --no-fail-fast` → exit 0, including `crates/cli/tests/limits.rs`: `ssh_connect_secs = 42` in fleet.toml shows up as `-oConnectTimeout=42` in a remote spawn plan; `config show` lists every limit and matches the docs table; the timing check fails a fixture `Duration::from_secs(5)` and passes on config.rs and test code.
- `HERDSMAN_LANE` is unset because, inside a lane, `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` logs the lane, not `owner`, as the actor. That comes from the environment, not this diff. The WebKit tests skip here (no Playwright on this host); CI's ui-webkit job runs them.

## Why (round 4)

Decided: every literal core timing is a [limits] key whose default is today's value, with one doc line in LIMIT_DOCS and docs/config.md
Because: "Make each a [limits] key with today's value as default, add a doc line per key"
Decided: CI's timing check is scripts/timing-check.sh run in ci.yml; it skips config.rs and #[cfg(test)] items
Because: "outside `#[cfg(test)]` and `config.rs` fails"
Decided: no card_wait_secs key; no card view wait exists at head, a card runs for its own timeout
Because: crates/core/src/card.rs:24
Rejected: an unused card_wait_secs key — dead config nothing reads; #821 can add it with the wait it times
Decided: staging merge gives #859's six serve and SLO limits a LIMIT_DOCS line and a docs/config.md row, like every other key
Because: crates/core/src/config.rs:952
Decided: staging build fix: needs_fold_ops uses webview() since #862 removed swift()
Because: crates/cli/tests/needs_fold_ops.rs:78
Check: crates/cli/tests/limits.rs fails if any [limits] field lacks a doc line or docs table row; it passes with #859's keys
Owner: Every timing herdsman uses can now be tuned in fleet.toml's [limits], and CI blocks new hard-coded ones.

<sub>note on 1106782 · git log --notes=why</sub>

