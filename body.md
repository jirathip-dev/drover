Closes #1129

## For humans
Drive now stops starting new issues once a fleet has 8 pull requests in flight (built, in review, being fixed or waiting to merge), and spends its passes landing them instead.
Set `max_in_flight` per fleet to change the number, or 0 to turn it off; work already in flight is never stopped.
Status, board, waiting and the Queue screen show "in flight 7/8", and a held queued issue says it waits for the cap. An owner or orchestrator can push one start past it with `--force`, which is logged.

## Parity
none: the Queue screen's stat block gains an `in flight` item (page and cooee, checked by `cooee_screens`); no row of `docs/parity.md` changes or drops.

## What changed
- `[fleet.<n>] max_in_flight` (unset 8, `0` off). In flight = state impl, fix, review/verify or ready, plus queued/blocked/needs-human issues whose cached PR is open (`queue::flight`).
- The check lives in `spawn::lane_admit`, the gate both drive's launch and `drover spawn` use. It applies only to an impl-family start of a queued issue with no open PR, so reviews, fix, update and verify rounds and merges go on as before. It refuses `IN_FLIGHT_CAP` with the count and the issue list; `--force` logs `override` with `code` and `by`.
- Drive stops trying further starts in a pass after `IN_FLIGHT_CAP` (like `CAP`). The code is soft in `pending::SOFT`.
- `status`, `waiting` and `board` show `<fleet>: in flight N/M` (merged with the serial line, now `queue::fleet_use`). A held queued issue says `waits: in-flight cap N/M` (a `promise::Say::InFlightCap`; the board's lane column says `in-flight cap N/M`).
- The Queue screen's stat block gets an `in flight` item (one per fleet on "all"), on the page and in cooee.
- Usage doc: `roles/usage/30-refusals/IN_FLIGHT_CAP.md`.
- Updated tests whose expected text or issue counts the default of 8 now changes: `m3`, `fleet_cards`, `synergy`, `serial_groups` (header text), and `drive_kicks` and `synergy`'s no-cap test (set `max_in_flight = 0`, since they start 10 / 100 issues).

src/ net +64 (budget none stated): the gate, the in-flight fold and the views; `cli.rs`, `drive.rs` and `serve.rs` stay at their arch-baseline sizes.

## Tests (at head a261b1ed6eccacbe25dd62432d629efc767403f0)
Fix round: a261b1ed adds `2 in_flight` to scripts/test-shards.txt (the new target was in no shard, so CI's shard check failed). At a261b1ed: `scripts/test-shards.sh check` → exit 0; `cargo test -p drover --test in_flight -j 4` → exit 0 (4 passed). The source is unchanged from d0aef678, so the runs below still hold.

This head merges origin/staging (ebec8a2f) into 783b50e2. In the conflicts, `queue::skipped` takes `(n, ctx)` plus staging's `slots`. The in-flight item is now added after staging's new Queue stat items (index 6, `cooee_screens` updated). `IN_FLIGHT_CAP` joined staging's `starts::stops`.

New `crates/cli/tests/in_flight.rs` covers each AC 8 case: the cap holds a new start and every view says so; a review spawn isn't held, and a merge frees a slot for the next issue in queue order; default 8 (8 refuse, 7 admit); `0` off; a held issue with an open PR counts; the refusal and its `--force` override are logged. `cooee_screens` checks the Queue screen stat.

At d0aef678:
- `env -u DROVER_LANE cargo test -q -p drover -j 4 --test in_flight --test cooee_screens --test serial_groups --test synergy --test m3 --test fleet_cards --test drive_kicks -- --test-threads=4` → exit 0 (24+46+3+4+10+14+15 passed)
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` and CI's `--tests` clippy line → exit 0
- `cargo fmt --check` → exit 0
- `drover cap-check --config ci/cap.toml` → exit 0

Not verified at d0aef678: the full `cargo test --workspace` run exited 101 because the host disk filled (73 MB free on `/`, `cc` link failed with no space). That is not a test failure; CI runs the full suite. At 783b50e2 the full suite was exit 0, except that `self_update`'s pause test needs `DROVER_LANE` unset in a lane.

## Why (round 1)

Decided: the cap is checked in lane_admit, the one gate drive's launch and drover spawn share, only for an impl-family start of a queued issue with no open PR
Because: "With the cap reached, drive starts no new impl lane (first start of a queued issue)"
Decided: in flight is the fold's in_flight states plus queued, blocked or needs-human issues whose cached PR is open; the cache is a file read, safe on serve's request path
Because: crates/core/src/queue.rs:389
Decided: --force logs an `override` with code IN_FLIGHT_CAP and `by`, the same shape HOST_BUSY's override has
Because: "`--force` overrides, logged `override` with `by`"
Decided: the Queue screen gets an `in flight` item in its stat block (one per fleet on "all", plain `in flight` with one repo picked); no new QueueCard field, so no type regen
Because: "a block like the other counts, on the page and in cooee"
Rejected: a QueueCard field read by the card header — cli.rs, drive.rs and serve.rs sit at their arch-baseline sizes, and the stat block already reaches both page and cooee
Check: the board header can pass 60 cols with serial groups too; in flight is put first so the cut drops serial, not it
Owner: Drive now stops starting new issues once a fleet has 8 PRs in flight (set max_in_flight to change it, 0 for off) and spends its passes landing them.

<sub>note on 783b50e · git log --notes=why</sub>



