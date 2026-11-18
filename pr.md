Closes #950

## For humans

When the daily event-log cleanup finds nothing to delete or archive, the board and the service no longer throw away their in-memory log and read it all again.
Nothing changes when the cleanup does remove events.

## What changed

- `db::compact` writes the `archived` snapshot row (which `db::version` reads) only when it deleted heartbeats or archived events, so a compaction that moves nothing keeps a held `events::shared` (same `Arc`). This is scope item 1.
- Since the issue was filed, #978 removed `events::rotate` and `db::live_bytes` and replaced them with the daily `db::compact`. That function had the same always-restamp bug, and this PR fixes it there.
- **Not done: scope item 2 and AC3 (the live-size counter).** No `SUM(LENGTH(json))` scan is left anywhere in `crates/`, so a counter would be new code that nothing reads. Decided, not asked; see the why note.

src/ net +6 (no budget given): one guard around the snapshot write.

## Evidence (head 7a282d1ae378af788e2b048c677fb6f1633978bb)

- Commit 1 (`1dae4002`, `baseline:`) adds only the test `a_compaction_that_moves_nothing_keeps_the_shared_log`. On the base, `cargo test -p drover --test compaction a_compaction_that_moves_nothing` exits 101: `left: (Some(387849), Some(1)) right: (Some(387849), None)`.
- At the head, `cargo test -p drover --test compaction` exits 0 (6 passed).
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`, the CI `--tests` clippy line and `cargo fmt --check`: exit 0.
- `cargo test --workspace --locked --no-fail-fast` (lane `DROVER_*` env unset): exit 101. The only failure is `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which also fails with the base `db.rs` (exit 101). That test depends on the host: it finds the host's real `shed`, and there is no `sysctl` or gh auth here. CI is the judge.

## Perf

Command: `cargo test -q -p drover --test compaction a_compaction_that_moves_nothing` (cargo's "finished in", 20k-event log)

| | runs | median |
|---|---|---|
| before (commit 1) | 3.17 s, 2.97 s, 3.05 s | 3.05 s |
| after (head) | 2.52 s, 2.25 s, 2.74 s | 2.52 s |

That saves about 0.53 s (-17%) per no-op compaction: the full reload of a 20k-event log is skipped. Compaction now runs daily, not every slow pass, so the gain in practice is smaller than the issue describes.
