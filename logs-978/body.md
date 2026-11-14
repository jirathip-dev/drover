Closes #978

Scope per owner decision q1651: issue/lane/ask tables kept in the append transaction + rebuild command + test; issue state and ask state read from the tables (no fold / ask-index replay); queue order, holds and waiting reasons keep reading the (now small, 14 d) hot DB; AC2–AC6 in full. Moving those to tables is a follow-up issue.

## For humans

The state database no longer grows forever. Each issue, lane and ask has one current-state row that updates with every event. Once a day, old events of finished issues move into monthly compressed archive files, heartbeat noise is deleted after 48 hours, and build-copy error text lives in the cache instead of the database. On a copy of today's live state the database went from 225 MB to 89.5 MB.

## What changed

- **AC1**: `states`, `lanes`, `asks` tables written in the same transaction as each `append` (`crates/core/src/db.rs`). #948's per-issue `states` table (merged to staging meanwhile) is now this one table, kept by the append projection; its lazy fold cursor is deleted, and its working-set log loads through the same one-snapshot read as the tables. Issue state and ask endings come from these tables; the cached log loads them in one read snapshot with its events. `drover compact --rebuild` rebuilds them from every event (hot DB + archive). Test: `crates/cli/tests/compaction.rs::the_tables_match_the_events_and_a_rebuild_gives_identical_rows` (20 seeds: tables == fresh fold, rebuild gives byte-identical rows, and the same after a compaction archives events).
- **AC2**: `drover compact` (hidden; run by `tick` once a day *after* drive, `compact.at`) moves events older than 14 days of merged/closed issues to `archive/events-YYYY-MM.jsonl.zst`, then `VACUUM` + `wal_checkpoint(TRUNCATE)`. The old checkpoint/rotation code and `rotate_mb` are deleted.
- **AC3**: `drive-pass`, `driven`, `kick` older than 48 h are deleted, not archived.
- **AC4**: `build-cloned` errors go to `cache/build-cloned/<sha256>.txt`; the event carries `errors_ref`. Existing rows are migrated on first open (`user_version` 1), which also builds the tables.
- **AC5**: `drover why` and `drover lesson digest` read `history()` = archive + hot DB.
- zstd is the host binary through the Runner (no new crate dependency).

## AC6: measured on a copy of today's live state (2026-11-14 15:30, 187,333 events)

"Before" = the installed build (`d91e726`, staging before #978) on the untouched copy; "after" = this branch's build at `988bab6a9` on the copy after one `drover compact`. Same host, same config (the six fleets), back to back.

| | before | after |
|---|---|---|
| `state.db` | 225.3 MB | 89.5 MB |
| events in hot DB | 187,333 | 133,664 |
| `build-cloned` payload in DB | 115 MB | 0 (115 MB moved to `cache/build-cloned/`) |
| archive | – | `events-2026-10.jsonl.zst` 188 KB (572 events) |
| `drover board` p50 / p95 (n=20) | 2125 / 2257 ms | 1593 / 1736 ms |
| `drover status` p50 / p95 (n=10) | 15791 / 29926 ms | 16083 / 23753 ms |

`drover compact` itself: 2.3 s real (53,097 heartbeats deleted, 572 events archived).

- **20 MB target not met today.** The live log spans only 16 days, so 14-day retention keeps nearly all of it: 59 MB of event JSON is inside the window (`verdict` 16 MB, `spawn` 9 MB, `conflict-predicted` 4 MB…), and 44.5k events belong to open issues. The DB keeps shrinking as history ages past 14 days.
- `drover status` time is dominated by work other than the state read (same ~16 s p50 on both builds; load average was 7–22 during the runs).
- One row differs between before and after: synergy #3256 is `fix(0)` in the old build and `queued` in this one. The old build folds from a stale `states` checkpoint (seq 180798); with that checkpoint deleted, the old build also shows `queued`, and `drover compact --rebuild` agrees. This branch no longer folds from checkpoints.

## Evidence at `177811ed23971f4cd6c118cb65b559095eb791da`

Merged with `origin/staging` at `2bfb337`+#948 (`e24e880`). Fleet rename (#935, from staging) now also moves the `states`, `asks` and `lanes` rows (`fleet_rename` test asserts it; it fails without the fix).

- `cargo fmt --check` → 0
- `cargo clippy --all-targets --locked -- -D warnings` → 0
- `cargo build --release --locked` → 0
- `drover arch-check --base origin/staging` → 0 (966 files, 0 violations)
- `drover cap-check --config ci/cap.toml` → 0 (44,676 / 45,400)
- `drover diagram | diff docs/workflow.md -` → 0
- `scripts/timing-check.sh` → 0
- `env -u DROVER_LANE cargo test --locked --no-fail-fast` → 0 (205 test targets). `DROVER_LANE` is unset because the lane's own value leaks into `self_update`'s actor assertion.
- `cargo test --release --locked --test perf -- waits_and_the_lanes_tab_stay_fast_on_a_30k_event_log a_hundred_board_requests the_state_read_stays_under_300ms --nocapture --test-threads=1` → 0
- `pass_budget`: its fixture writes 100k rows with raw SQL, so it now builds the tables with `db::rebuild` as appends would; the assertion is that a drive pass reads less than the whole log (it used to require a full read).
- AC6 numbers above were measured at `988bab6a9`, before the #948 merge.

`src/ net +129` against current staging (+443 −314; no budget given in the issue): #948 already removed part of the rotation code this branch deletes; the rest is the tables, rebuild and compaction.

## Not verified

- CI on Linux (pending on push).
- The 20 MB target, as above.

## Why (round 2)

Decided: one per-issue table: #948's `states` (fleet, issue, done, json) is written by #978's projection in the append transaction; #948's lazy fold cursor (snapshot 'fold', fold/folded, CHUNK) is deleted
Because: "The same transaction that appends each event updates them."
Rejected: keeping both `issues` and `states` — two tables of the same fold would drift and break "never leave two ways to do one thing"
Decided: the working log (#948 WORKING subset) loads through db::since in one read snapshot with the states and asks rows, so asks come from the table, not Log::add's ask index
Because: q1651
Decided: compact archives events whose issue row is done = 1, the column WORKING already reads
Decided: fleet rename moves the states, asks and lanes rows with the events
Because: crates/core/src/db.rs:488
Decided: pass_budget's fixture builds the tables with db::rebuild (its rows bypass append) and now asserts the pass reads less than the log
Because: "never a replay of the event log"
Check: drover lesson digest's verdict round count reads history() (hot DB + archive), not #948's hot-only query
Check: AC6 numbers were measured at 988bab6, before the #948 merge
Owner: The state database keeps one current row per issue, lane and ask and moves finished work to monthly archives; today's copy went from 225 MB to 89.5 MB.

<sub>note on 177811e · git log --notes=why</sub>

