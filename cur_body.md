Closes #1313

## For humans
The Queue screen on the phone page now opens in a few dozen milliseconds instead of about half a second, even with thousands of cached GitHub files.
The Hosts sheet no longer scans running processes when you open it; the leftover count and token-proxy lines are refreshed in the background.
Nothing changes in what the screens show.

## What changed
- **Live cause (amendment q2042):** a Queue build re-read and re-parsed the same gh cache files about four times per issue (spec twice, PR three times, run), and for a queued issue with no spec it parsed the whole backlog file once per row. With a gh cache shaped like live (4,911 files, 139 MB) that alone took the Queue from ~6 ms to ~490 ms. Now each board GET is one build: `gh::memo` parses each cache file it touches once (`gh::cached`; `cached_spec` hands out an `Rc`).
- `Config::kept` parses fleet.toml once and keeps it until its mtime, size or inode changes (config types derive `Clone`).
- One shed ls per build: `Server::sessions` is shared by `queue_tab`, `lane_list` and `needs_count` within a request.
- SQLite: the connection cache was thread-local and serve runs a thread per request, so every request opened a fresh connection (pragmas + schema). It is now a process-wide pool (at most 8 idle per database). A pooled connection still runs the `user_version` upgrade on checkout, so a database an older binary rolled back is upgraded as before (`tests/compaction.rs` covers this).
- Hosts sheet: the leftover count (`live_on` + `check::leftovers`) and the proxy facts are read only by the host sampler loop and `Server::refresh`. Neither a GET of the sheet nor a host action (`host`, `reap`) reads them on the request; after a [Reap], the count updates on the next sample (`serve_host_secs`).

## Perf (`--perf`)
Command (median of 3 runs; each run gives the p95 of 20 GETs after one warm-up):
`cargo test --release --test perf screens_on_a_live_shaped_fixture -- --nocapture`

Fixture: 4,911 gh cache files (139 MB), 3,000 events, 150 issues, 10 lanes.

| screen | before (281691c4) | after (37381cd) | budget |
|---|---|---|---|
| /screens/queue | 486 ms (3 shed ls) | 36 ms (1 shed ls) | < 150 |
| /screens/hosts | 1 ms (1 shed ls) | 0 ms (0 shed ls) | < 50 |
| /screens/lanes | 9 ms | 8 ms | < 150 |
| /screens/threads | 1 ms | 0 ms | < 150 |

Raw runs after: queue 38/36/36, hosts 0/0/0, lanes 9/7/8, threads 0/0/0. The bench now asserts these budgets in release and runs in CI's release perf step.

Not measured: **the read-only copy of live state.** This lane host has no live state dir and no owner-approved snapshot was provided, so only the live-shaped fixture was measured. To get the live numbers, point the same bench at a copy of the live state.

## AC test
`a_queue_build_lists_shed_once_and_neither_it_nor_the_hosts_sheet_starts_a_process` (tests/perf.rs) builds a deferring `Server` with a runner that records any process started inside a request. It then GETs /screens/queue twice (at most 1 shed ls each) and /screens/hosts once (0 shed ls), and asserts no process was started. On the baseline code it fails with `/screens/queue listed shed 3 times`.

## Parity
none: no parity row changes or is dropped. The diff touches `serve.rs` and the screen builders only for speed (caching, one shed ls, pooled SQLite, the hosts sheet read from background data); no page block, test or row in docs/parity.md changes.

## Evidence at d462050cf2abb3316cab65f78e52245a0defe9f5 (merge of origin/staging 818280c6)
- Fix round 7 (no code change): at d462050 CI was red on `ui-webkit` (apt mirrors unreachable in `playwright install --with-deps`) and `test (shard 3/4)` (`drive_now_on_the_board_is_paired_only_and_toasts_its_line`, a wall-clock flake: pass took 0.1 s vs 0 s; staging's test, untouched here). Local `cargo test -q -p drover --test drive drive_now_on_the_board` → exit 0 three times. Re-ran the failed jobs (run 44521197426): all 13 checks green at d462050cf2abb3316cab65f78e52245a0defe9f5.
- Fix round: the FAIL at e60aeebd was CI `ui-webkit` **cancelled** at its job timeout inside `playwright install --with-deps webkit` (apt mirror stalled on libflite1/libplacebo); no UI test ran. CI infra, not this diff. Merged staging 818280c6 (clean) for a fresh run.
- `drover cap-check --config ci/cap.toml` → exit 0 (55963 / 60000); `cargo fmt --check` → exit 0
- both CI clippy commands → exit 0
- CI perf command (same as below) → exit 0 (6 passed): /screens/queue 52 ms (1 shed ls) · /screens/hosts 0 ms (0 shed ls) · /screens/lanes 13 ms · /screens/threads 0 ms p95; drive --fast 49 ms CPU. (A first run right after the release build hit host load: drive --fast 163 ms, exit 101; two isolated re-runs 51 / 47 ms, then the full command passed.)
- `cargo test --locked -p drover --test serve_health --test hosts_screen --test screen_pages --test serve_wire` → exit 0

## Earlier evidence at e60aeebd358e7516106c428ffca2769ad6450c7e (merge of origin/staging 70bc331d)
- Fix round: CI `rust` at 780747bf failed only arch-check's size baseline for `crates/cli/tests/shed.rs` (2560 > 2557). This PR never edits that file; the merge of staging 70bc331d brings it to staging's 2556 lines.
- `drover arch-check --base origin/staging` → exit 1, only violation `just gen` (just not installed on this host; no generated inputs changed)
- `drover cap-check --config ci/cap.toml --base origin/staging` → exit 0 (55879 / 60000); `cargo fmt --check` → exit 0
- both CI clippy commands → exit 0
- CI perf command (`cargo test --release --locked --test perf -- waits_and_the_lanes_tab_stay_fast_on_a_30k_event_log a_hundred_board_requests the_state_read_stays_under_300ms a_pass_over_30k_events_and_150_done_issues_stays_fast stats_autonomy_on_a_150k screens_on_a_live_shaped_fixture --nocapture --test-threads=1`) → exit 0 (6 passed): /screens/queue 39 ms (1 shed ls) · /screens/hosts 12 ms (0 shed ls) · /screens/lanes 13 ms · /screens/threads 0 ms p95
- `cargo test --locked --test hosts_screen --test serve_health --test shed --test compaction` → exit 0

## Earlier evidence at 780747bfe4c37581176181547f2130db517039ff (clean merge of origin/staging a96f2973, which raises the cap to 60000 via ci/cap.d/2026-11-19-60000.toml)
- Fix round: CI `rust` at 88c3e984 failed only on the line cap (55576 / 55500); `drover cap-check --config ci/cap.toml --base origin/staging` → exit 0 (55685 / 60000)
- `cargo test --release --locked --test perf -- --test-threads=1` → exit 0 (24 passed, incl. the AC test)
- `cargo test --release --locked --test perf screens_on_a_live_shaped_fixture -- --nocapture` ×3 → exit 0, 0, 0. /screens/queue 40/38/47 ms (1 shed ls) · /screens/hosts 0/0/0 ms (0 shed ls) · /screens/lanes 9/7/8 ms · /screens/threads 0/0/0 ms
- `cargo test --locked --test serve_health` → exit 0 (9 passed); `cargo fmt --all --check` → exit 0; `cargo clippy --workspace --lib --bins --locked -- -D warnings` → exit 0
- ui-webkit's `the_model_and_effort_picks_are_one_row_of_dropdowns_and_a_model_pick_reaches_the_bar_in_webkit` (red at 88c3e984): with Playwright WebKit, `cargo test --locked -p drover --test threads_tab` passes it in 3 of 3 runs here. In those same runs `parity_26_a_message_sent_while_working_queues_and_send_now_steers_it_in_webkit` fails, and it fails identically on plain origin/staging a96f2973 on this host (2 of 2), so it is a host issue, not this PR.

## Evidence at 88c3e984064e1356675ca1101039e2473be45f10
Fix round for the review at f8437da1 (blocking: `host_op` re-read shed and leftovers on the request):
- `host_op` no longer calls `read_hosts`; it returns the sheet from the sampled data (screens_hosts.rs).
- The AC test now also POSTs a host action (`host` b1 off) and asserts it lists shed 0 times. Without the fix it fails: `a host action listed shed` (exit 101); with it, it passes.
- `tests/hosts_screen.rs` parity_75 depended on the action re-reading leftovers; it now calls `refresh` first, as the sampler does.
- Merged origin/staging (conflict in serve.rs `sessions`: staging's `link::view::View` read, kept inside this branch's one-listing-per-build memo).
- This PR's benches moved into `crates/cli/tests/perf/screens.rs` (a child module of perf.rs), so perf.rs stays under its 1500-line size rule.

| command | exit |
|---|---|
| `cargo test --release --locked --test perf screens_on_a_live_shaped_fixture -- --nocapture` | 0: queue 40 ms (1 shed ls), hosts 0 ms (0), lanes 9 ms, threads 0 ms |
| `cargo test --locked --test perf a_queue_build_lists_shed_once` | 0 |
| `cargo test --locked --test hosts_screen` | 0 |
| `cargo clippy` (CI's two invocations) | 0 |
| `cargo fmt --all --check` | 0 |
| `drover arch-check --base origin/staging` | 1: only `just gen` (no `just` on the lane host) |
| `drover cap-check --config ci/cap.toml --base origin/staging` | 1: **55549 / 55500**, see below |
| `cargo test --workspace --locked --no-fail-fast` | 101: 236 suites ok, 1 test failed: `shed.rs` a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it, which fails the same on plain origin/staging on this lane host (a real `shed` is on PATH there); unrelated |

**Line cap:** staging is at 55461 (39 headroom); this PR is `src/ net +88` (budget: none given), the five wanted parts. Over the cap by 49 after trimming; owner ask filed (raise in one orchestrator-filed issue, recommended, or drop a part).

## Earlier evidence at 37381cdb7e36fc7971f3f685879c0a397b792c45
- `cargo test --release --locked --test perf -- waits_and_the_lanes_tab_stay_fast_on_a_30k_event_log a_hundred_board_requests the_state_read_stays_under_300ms a_pass_over_30k_events_and_150_done_issues_stays_fast stats_autonomy_on_a_150k screens_on_a_live_shaped_fixture --nocapture --test-threads=1` → exit 0 (6 passed)
- `cargo test --test perf --test hosts_screen --test compaction` → exit 0
- `cargo test --workspace --no-fail-fast` (at 10b88388; the only later change removes one doc line) → exit 101. The one failure is `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, which fails the same way on the untouched baseline because `shed` is on this host's PATH.
- both CI clippy commands → exit 0; `cargo fmt --check` → exit 0; `scripts/timing-check.sh` → exit 0
- `drover cap-check --config ci/cap.toml --base 55f8bb2d` → exit 0 (53,182 / 53,500)
- `drover arch-check --base 55f8bb2d` → 1 violation: `just gen` is not installed on this host. No generated inputs changed.

src/ net +96 (no budget given in the issue): five targeted caches/pools, plus `Clone` derives edited in place.

## Why (round 2)

Decided: each board GET is one build: gh cache files parsed once (gh::memo), shed listed once, fleet.toml parsed once per file stamp
Because: "Each gh cache file parsed once per build."
Because: q2042
Decided: the live Queue cause is the gh cache, re-parsed ~4x per issue plus the backlog per spec-less row; 486 -> 36 ms on a 139 MB fixture
Decided: SQLite connections pool process-wide; a pooled one still runs the user_version upgrade on checkout
Because: crates/core/src/db.rs:90
Rejected: keep thread-local connections — serve spawns a thread per request, so each request reopened the database
Decided: hosts sheet reads leftovers and proxy facts kept by the host sampler, refresh and actions
Because: "never from shed or leftovers on the request"
Check: the live-state snapshot numbers were not taken: no snapshot exists on this lane host
Owner: Queue opens about 13x faster on a live-shaped cache; live-state numbers still need the approved snapshot

<sub>note on b641704 · git log --notes=why</sub>

## Why (round 3)

Decided: merge origin/staging 49f95b77; serve.rs sessions() keeps this branch's once-per-build shed memo and fills staging's listed snapshot inside it
Because: "One shed ls per build, shared by all its parts."
Rejected: none
Owner: Brought the speed-up up to date with staging; screens stay under 150 ms on the live-shaped fixture

<sub>note on f8437da · git log --notes=why</sub>

## Why (round 5)

Decided: a host action returns the sheet from the sampled leftovers and proxy facts; only the sampler and refresh read them
Because: "The hosts sheet is built only from data refreshed in the background, never from shed or leftovers on the request."
Rejected: kicking the sampler after an action — no wake channel exists; the count catches up within serve_host_secs
Decided: this PR's benches live in crates/cli/tests/perf/screens.rs, a child module of perf.rs
Because: .drover/arch.toml:197
Rejected: a new test target — it would copy perf.rs's fixtures and need a shard line
Check: the AC test POSTs a host action and asserts zero shed ls; it fails with the old host_op
Owner: the line cap raise (60000) is on staging; this PR now fits under it

<sub>note on 780747b · git log --notes=why</sub>

## Why (round 6)

Decided: merge origin/staging 70bc331d; crates/cli/tests/shed.rs returns to staging's 2556 lines, under its 2557 size baseline
Because: crates/cli/tests/shed.rs:1
Rejected: merging the wip snapshot — it holds only local test logs, and build output never goes in git
Check: arch-check's only remaining local violation is `just gen` missing on this host
Owner: Brought the speed-up up to date with staging again; screens stay under 150 ms on the live-shaped fixture

<sub>note on e60aeeb · git log --notes=why</sub>


