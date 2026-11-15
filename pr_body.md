Closes #1053

## For humans

Drover now learns everything about a lane (working, blocked, idle, done, stalled, dead, orphaned, and its transcript) from shed itself, instead of reading the terminal screen. So a lane can no longer show the wrong state the way #1033 did.
What changes for you: a lane's subtitle is shed's own note, a blocked card no longer has menu buttons (the reply box still works), and a stall alert means shed has said "quiet" for the stall time with no turn-end. Usage-limit and sign-out alerts come from shed's `rate-limited` / `error auth:` reports.

## What changed (per the approved ADR, `docs/architecture/shed-native/`)

- **`crates/core/src/session/`** (new): `Session`, `Life`, `Status`, `Cause`, `Exit`, `Sessions`, and the one mapping from shed's `TerminalInfo` (`Status::of`). `list.rs` (`all`, `on`, `live_on`, `get`), `stamp.rs` (the `lane-state` stamp with `since`: `on_event` for the stream, `observe` for drive's pass and `drover check`; `intake` moved here from `shed.rs`), `pulse.rs` (the typed ACP pulse that was untyped `shed-out.json`, its 500-char reply tail dropped), `transcript.rs` (`read`/`of`/`page`, plus `archive` at close and `archived` after it, and `said` for spawn's handover).
- **Rules in one place**: `lanes::health` (Fine / NeverStarted / Stalled / Dead / Closed / Unknown) and `lanes::orphans`, used by `check` (alerts) and `stall` (drive's acts). Stall means shed says quiet since the stamp, `stall_minutes` have passed and there is no turn-end. Never-started means no report `START_SECS` after the spawn. Dead means exited or not listed on a host that answered. Unreachable means no judgement. Orphan means a labelled session that no spawn names.
- **Gone**: `term::agent_of/agents/list/agent_list/read/recent/harness_in/pane_get_argv`, `host::agents/live_in`, `hosts::remote_agents/all_agents`, `spawn::shaped`, `stall::changed` + `pane-seen.json`, `shed::{log,session,silent,exit_cause,RESUMABLE,others}`, every screen reader in `lanes.rs` (`chat`, `body`, `roll`, `activity`, `spin`, `now`, `said`, `prompt`, `Tail`/`read_tail`, …), nine `Provider` screen methods (`cap_banner`, `usage_limited`, `signed_out`, `status_word`, `now_line`, `tool_line`, `pending_menu`, `answer_keys`, `transcript`) and serve's menu `pick` (`POST /answer` / v1 `menu` now answer `BAD_VALUE: no menu`).
- **`.drover/arch.toml`**: the ADR's four `[[ban]]` lines (`agent_status`, `pane_id`, `.reported`, `read_argv`).
- **Docs**: `runtimes.md`, `providers.md`, `lanes.md`, `serve.md`.

## Decisions (routine, inside the ADR)

- The ADR's open question 1 (`board-fix`) is moot: #927 merged and removed the old board and its freeze rule. Open question 2 (PTY lanes lose screen-parsed alerts and menus) was approved with the ADR.
- The stall clock starts at drover's stamp of shed's status (ADR `shapes.md`: "`since` is this event's time when the Status differs…"). The first poll of an already-quiet lane starts the clock, so the old "idle since the issue's last event" tests now advance time once.
- Any turn-end means no stall (ADR `boundaries.md`), and a gone lane is dead only, with no stall alert beside it.
- `drover kill` no longer deletes the lane's log. The archive now goes only with its worktree (`clean`), so a respawn's handover can still read it.
- `check`'s `orphan-lane` fires only when the labelled session's issue is untracked. drive adopts the rest by labels, as before.
- Test infra: the fake shed's `exit()` is now a normal end (code 0, last `done`). The sim's planted bug fires on the 4th start (was 3rd): never-started now comes from shed's silence, so restarts happen more often in the sim. The `the_known_fleet` lock-wait metric moved 12 → 11.

`src/ net −70` (budget: the ADR estimated about −300; staging is at 44026 / 45400). The new `session/` module (~730 lines) absorbs what left `term.rs`, `shed.rs`, `lanes.rs`, `stall.rs`, `check.rs` and `serve.rs`.

## Evidence

Every row below ran at head `0975e05414ce3e105e915b71ae653ac073d63e35`: this branch merged with `origin/staging` again (update round; clean merge, staging brought `crates/core/src/drive.rs` and `crates/cli/tests/serial_groups.rs`, no conflicts). The previous round's fix (stamping lane states inside `snapshot::take`, at `99aa1b3`) is unchanged.

| Command | Exit |
|---|---|
| `cargo fmt --all --check && cargo clippy --workspace --all-targets -q -- -D warnings` | 0 |
| `env -u DROVER_LANE -u DROVER_HOST cargo test --workspace -q --no-fail-fast` (includes `shed_native`, #1033's `herd a_shed_lane_reported_running_is_working_with_its_note`, `perf`, `serial_groups`) | 0 |
| `cargo test --workspace` inside the lane env | 101: only `self_update::pause_and_resume…`, whose logged `by` picks up the lane's own `DROVER_LANE`; passes without it |
| `drover arch-check --config .drover/arch.toml --base origin/staging` | 0 (814 files, 0 violations) |

## Not verified

- A real shed session's `rate-limited` / `error auth:` reports. The tests drive the fake shed with those words, as shed's protocol at the pinned rev `9b88eb5` documents them.
- Release-build perf (`--release` perf tests) and the web/UI jobs. CI runs them.

## Why (round 1)

Decided: drive's look at shed sessions (lane-state stamps) runs inside the step's snapshot take, before its wait key, not in each fleet's pass
Because: crates/core/src/drive.rs:774
Rejected: observe before the grew baseline in drive — the saved wait key then lags the stamps and the board recomputes waits (perf waits_and_the_lanes_tab test)
Decided: the ADR docs' example lane is myapp-impl-12, an install-neutral name
Because: "Lane status, stall, dead, orphan and transcript read only shed (plus drover's turn-end); no herdr-shaped mapping or pane scrape remains."
Check: cargo test -p drover --test perf passes both the one-remote-call-per-step and the snapshot-hit tests
Owner: a drive step again makes one remote call per host; staging's #966 is merged in

<sub>note on 99aa1b3 · git log --notes=why</sub>

