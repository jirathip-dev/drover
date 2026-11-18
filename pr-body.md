Closes #955

## For humans

Checking whether an automated step already ran is now a quick index lookup, even on a large event log. The database also stops listing its folder on every call; it does that once per process. Crash safety is the same as before.

## What changed (crates/core/src/db.rs)

1. A partial expression index `events_action` on `json_extract(json, '$.action_id')` covering `intent`/`done`/`failed`, so `claim` (its SQL is now the `CLAIM` const) is an index search. It is built by the existing `CREATE INDEX IF NOT EXISTS` when the db opens, with no migration.
2. `legacy` lists the state directory once per process per directory. It lists again only when `events.jsonl` exists or the last listing found old logs.
3. `prepare_cached` on insert, the states/lanes projection, `last`, `rows`, `claim`, `since`, `asks_since` and snapshot `get`/`stamp`.
4. Decided: `synchronous` stays FULL. With NORMAL, a lost tail event could be an owner's answer, a verdict or a `merged` event, and no later pass can rebuild those.

src/ net +58 (budget: none stated). Most of it is `prepare_cached` call chains split across lines by rustfmt, plus the listing cache.

## Perf (`--perf`)

Command (the same one as the baseline commit 86d35c1c):

    cargo test --release -p drover --test state_db hot_db_calls -- --ignored --nocapture

| | 1000 claims (new ids, 20k claim-event log) | 1000 last_seq |
|---|---|---|
| before (median of 3) | 14551 ms | 9 ms |
| after (8 / 11 / 13 ms runs, median) | 11 ms | 2 ms |

Claims are about 1300× faster and last_seq about 4.5× faster.

## Evidence at 7e354278b1f2e58bd7f1284071dcbab851f0ffdb

All run at 7e354278b1f2e58bd7f1284071dcbab851f0ffdb (staging d0cba316 merged in):

- `cargo fmt --check`: exit 0
- `cargo clippy` (both CI invocations): exit 0
- `cargo test --release --locked -p drover --test state_db`: exit 0 (7 passed; includes `the_claim_lookup_is_an_index_search`, where EXPLAIN QUERY PLAN names `events_action`, and `a_thousand_last_seq_calls_list_the_state_directory_once`; the lease scenario `a_lease_is_held_until_it_expires…` passes unchanged)
- `cargo test --release --locked -p drover-core --lib`: exit 0
- `drover cap-check --config ci/cap.toml`: exit 0 (47753 / 53500)
- `scripts/timing-check.sh`: exit 0; `scripts/test-shards.sh check`: exit 0; `drover lesson-check`: exit 0
- `cargo test --release --locked -p drover --test sim` and `--test drive_sim`: exit 101. One test fails in each (`the_hunt_finds_a_planted_bug…`, `a_silent_reviewer_is_superseded…`). Both fail the same way on plain staging d0cba316 (exit 101, built in a scratch target), so this change did not cause them.
- `drover arch-check --base origin/staging`: exit 1. Neither violation comes from this diff: `serve.rs` is over its size baseline on staging, and `just` is not installed on this host. This PR touches only db.rs and state_db.rs.

