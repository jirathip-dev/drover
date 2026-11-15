Closes #971

board-fix: idle serve redraws every 15 s and rescans the log (amendment q1646)

## For humans

- The web board no longer redraws itself every 15 seconds while nothing changes; it redraws when something it shows changes.
- Redraws no longer read the whole event history for each queue row, so an idle board uses far less CPU on a big log.
- What the board and the API show is unchanged.

## What changed

- `serve.rs`: the 15 s forced redraw is gone; a redraw happens on a log/kick/drive/snapshot change, or once for a newly connected audience.
- `events.rs`: the cached log keeps per-kind and per-agent indexes, per-issue event copies and a per-fleet queue replay, all extended on append (`of_kind`, `of_agent`, `issue_log`, `with_states`).
- `lanes::join`, `queue_tab`, `undelivered`, `queue::groups`/`replay`, `land::note` (and the small readers they call) read those indexes instead of scanning the log; the board's `waits` scans only open issues and done ones whose lane it shows.

## Evidence (head 5cb661b2fd1057b17ecfa035358f4f85b42dc3e3)

All runs below are at 5cb661b2 (this branch merged with origin/staging at 55e52faa5).

- AC1: idle `drover serve` on a fresh copy of the mini's `state.db` (154049 events, taken with `sqlite3 -readonly … ".backup …"`), its last 10 min of events (112) replayed at their times: **0.41% CPU average** over 600 s (2431 ms CPU). Command: `DROVER_PERF_DB=<copy> cargo test --locked --release -p drover --test perf idle_serve_on_the_live_log_with_its_trickle_replayed -- --nocapture`, exit 0. Base was ~13% (#944).
- AC2 (per q1659: under 1.5x; the 10% bound goes to a follow-up): `cargo test --locked --release -p drover --test perf a_new_event_costs_the_same_on_a_log_twice_as_long -- --nocapture`, exit 0: 10 s idle 31 ms CPU; 10 events at 150k 540 ms, at 300k 624 ms (**1.16x**, best of 3, own process each).
- AC3: `cargo fmt --check` exit 0; `cargo clippy --all-targets --locked -- -D warnings` exit 0; `env -u DROVER_LANE cargo test --locked --no-fail-fast` exit 0 (1435 passed, 0 failed). `DROVER_LANE` is unset because `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` reads the lane's env as the actor. Goldens and API contract tests unchanged and passing.
- Merge note: `verify::served` now reads the spawn/verdict kind index instead of `of_issue`. With `of_issue`, staging's `no_board_request_starts_a_process` (its `/api/v1/issue` read on an uncached slice) bumped the global rebuild counter while `the_state_read_stays_under_300ms_on_a_150k_event_log` ran, and that test failed 1 in 3 runs. After the change it passed 3 of 3 full `--test perf` runs.

src/ net +130 (budget: none stated): the indexes on the cached log plus the readers moved onto them.
