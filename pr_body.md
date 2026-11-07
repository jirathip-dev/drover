Closes #560

## For humans

When the Mac is short of room, drive now gives it to merges, then reviews of pushed work, then fix rounds, and only then to new issues. A busy Mac starts nothing at all in that pass, so a review can no longer lose its turn to a new issue. The lane list says where a held review stands ("review waits behind 2 earlier reviews").
UI audits (the Chrome audit script and the WebKit tests) now run one at a time on the host. A lane that has to wait logs "waiting for the UI audit lock (held by <lane>)".
The board server no longer re-reads the whole event log on every request, and it skips rendering when nobody is watching. On a 30,000-event store its CPU per request drops by about two thirds.

## What changed

- **Reviews first** (`crates/core/src/drive.rs`):
  - A pass sorts its due steps: verdicts, then merges, then reviews (rev/verify spawns), then fix/update rounds and respawns. New starts still follow every fleet's pass.
  - Before launching any spawn kick, drive looks at this host's admission once per pass (`room`, using `hosts::gate`). A refusal (`HOST_BUSY`/`MEM_LOW`/`DISK_LOW`) holds that spawn and, through `go`, every start in the pass. The old code launched the review's kick and the starts' kicks together, so a start whose kick sampled a quieter moment could take the room.
  - The host look is skipped when an enabled remote host exists, because placement can go elsewhere and each kick still admits its own spawn.
  - `MEM_LOW` now also stops starts.
- **`waiting` names the order** (`crates/core/src/waiting.rs`): a review whose last try was refused for room (`SPAWN_PENDING` or a host wait) reads "review waits behind N earlier review(s)", counted in drive's order.
- **One UI audit at a time** (`scripts/ui-lock.sh`, new):
  - `scripts/ui-audit.sh` re-execs under it, and the WebKit tests' `common::swift()` runs `swift` under it.
  - It takes a kernel `flock` on `~/.cache/herdsman/ui-audit.lock` (perl, kept across `exec`). A dead holder lets go at once; a lock held 10 min is stale and run past.
  - The home comes from the password database, not `$HOME`: `live_board.rs` points `$HOME` at a temp dir that is later deleted.
- **Serve CPU**:
  - Profiled with `sample` on a 30k-event store. Most of the time went to `db::events` deserializing the whole log, about 16 ms per read and several reads per request and per watcher render. Callers were serve's handlers plus `timer::services` and `check::leftovers` on the Host tab.
  - New `events::shared`: the live log is kept in-process and each call reads only the rows after the last seq. A rotation (`db::archive` stamps an `archived` snapshot row) or a new database file reads it all again. Serve's handlers, `timer::services` and `check::leftovers` use it.
  - `Server::waits` keeps the waits it worked out for one log + lanes key (at most 30 s), so a log that moved since the step's snapshot isn't rescanned per request.
  - The watcher tick renders the Host/Queue/needs/lanes/chat parts only when a `/live` client listens, and only for the audiences (paired/unpaired) that listen.
- Docs: `docs/performance.md` (new budget row) and `docs/ui.md` (the lock). CI's release perf step also runs the new budget test.

Decided myself: the CPU budget is 2 s of CPU per 100 requests in release (about 1.0–1.3 s on the mini at this head, about 3.4 s before; headroom for the slower ubuntu runner). The 30k fixture uses 20 lanes and a new event every 10th request.

`src/ net +87` (the issue states no line budget): the drive order and host look, the waiting line, `events::shared`, and the serve cache and tick changes. The cap holds: 31377 / 31500 at head `aef9fef2`.

## Evidence (head `aef9fef2ff5ee6a39a16a230223a033b43db451e`)

Since `ebf13e6e`: a clean merge of staging (#653, #657). No conflicts, no change to this PR's own code.

All runs at `aef9fef2`, scratch `CARGO_TARGET_DIR`:
- `cargo fmt --all -- --check` → exit 0
- `cargo build --release --locked` → exit 0
- `cargo clippy --locked --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31377 / 31500)
- `cargo test --locked --no-fail-fast` → exit 101: 141 `test result: ok`; the only failure is `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`, whose `by` reads the lane's `HERDSMAN_LANE` (unset in CI)
- `env -u HERDSMAN_LANE cargo test --locked -p herdsman --test self_update` → exit 0 (8 passed)

## Earlier evidence (head `ebf13e6e47d99568383dea0e03960525f8ba52f3`)

Since `73d514fd`: a staging merge (#606, #628), `chat_scale` counting fold walks through `events::shared`, and a **staging build fix**: #628 (#623) retries a failed conductor delivery once, which broke #621's `a_failed_handoff_tap_shows_its_error_and_its_retry_is_the_one_delivery` on staging (staging `ci` red at `55269e10`; same failure locally on a staging checkout). The test now fails both tries of the tap (3 `agent start` calls in all). Test-only change.

All runs use the scratch `CARGO_TARGET_DIR` with `HERDSMAN_LANE` unset, as in CI (`self_update`'s `by` check reads it).
- `cargo fmt --all -- --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (31161 / 31500)
- `cargo test -p herdsman --test thread_handoff` → exit 0 (13 passed)
- `cargo test --no-fail-fast` → exit 101: 141 `test result: ok`, the only failure `live_board_chat_and_ascii_in_webkit` (`/live lanes` 0) at load avg 50+. That's the known load flake noted below, which also hits staging. It passed in the full run one commit earlier (`42e7b15c`, where the only failure was the thread_handoff test fixed here).
- CI at this head: `ci` (rust, ui, ui-webkit, web) success, `board-freeze` success.

## Earlier evidence (head `73d514fd83c0b7375885379c10bc543829083dd6`, merge of staging after #584)

- Clean merge (no conflicts).
- `cargo fmt --all -- --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (30691 / 31500)
- `cargo test --no-fail-fast` (scratch `CARGO_TARGET_DIR`, a lock held at the scratch root so the host's build sweep keeps it) → exit 0, 137 `test result: ok`. An earlier run in the shared target dir lost that dir to the build sweep mid-run (SIGKILL, "never executed"); not a test failure.
- `board-freeze` stays red until the owner's `**Amendment (q573)**: board-fix: …` comment is on issue #560 (decisions q573/q599/q616). Issue #560 still had zero comments when this push went up, and lanes cannot post it.

## Earlier evidence (head `c6b8a54bb0a733b40fd882fefbd5deaa9716645e`, merge of staging after #576)

- One conflict, `serve/v1.rs` imports: took staging's (#576) list with `shared` for `read`. #576 added three new full event-log reads in `serve/v1.rs` (`marked`, `issue`, `lessons`); they now read the shared snapshot like the other routes, so scope 3 still holds.
- `cargo fmt --all -- --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (30280 / 31500)
- `cargo test --no-fail-fast` (scratch `CARGO_TARGET_DIR`) → exit 101. 135 `test result: ok`, no failing test. The one red binary, `system_sheet` (WebKit), was killed by SIGTERM mid-run; `cargo test -p herdsman --test system_sheet` → exit 0 (3 passed) on rerun.
- Host note, not this PR: the build sweep (`check.rs` `sweep_scratch`/`trim_builds`) deleted the lane's scratch target twice while tests ran, because cargo drops its `.cargo-lock` while running test binaries. The passing run held a lock at the scratch root to keep it.
- `board-freeze` stays red until the owner's `**Amendment (q573)**: board-fix: …` comment is on issue #560 (decisions q573/q599/q616). Issue #560 still had zero comments when this push went up, and lanes cannot post it.

## Earlier evidence (head `ff2b788e8be13f2d5c7b06cd175dacf61e7a342c`, merge of staging after #572)

- Clean merge (no conflicts).
- `cargo fmt --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (29067 / 31500)
- `cargo test --no-fail-fast` → exit 101. Every test binary passed (132 `test result: ok`); the only failure was the `herdsman-server` doctest, which couldn't resolve `events::shared` against a stale `herdsman_core` rlib in the shared target dir. `cargo test -p herdsman-server --doc` → exit 0 on rerun. (Two earlier runs lost their scratch target dir mid-run to the host's build-folder reclaim, so this run used the lane's default target dir.)
- `board-freeze` stays red until the owner's `**Amendment (q573)**: board-fix: …` comment is on issue #560 (decisions q573/q599/q616). Issue #560 still had zero comments when this push went up, and lanes cannot post it.

## Earlier evidence (head `6ad231e538b84cf800ec00ef57b2284ffe23643a`, merge of staging after #538, scratch `CARGO_TARGET_DIR`)

- Clean merge (no conflicts).
- `cargo fmt --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (28725 / 31500)
- `cargo test -p herdsman --test drive_kicks --test drive --test waiting --test ui_lock --test compaction --test serve_web --test api_v1 --test run_card` → exit 0
- `cargo test --release -p herdsman --test perf -- --test-threads=1` → exit 0 (8 passed)
- `board-freeze` stays red until the owner's `**Amendment (q573)**: board-fix: …` comment is on issue #560 (decisions q573/q599/q616); lanes cannot post it.

## Earlier evidence (head `75d857f1233536778a76bf98abbfe76656258a8c`, merge of staging after #555)

- Conflict in `crates/server/src/serve/v1.rs`: both sides' imports kept. #555's new `roles` (GET) and `close` routes read through `shared` instead of the full `read` (scope 3).
- `cargo fmt --check` → exit 0
- `cargo build --release` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0 (28371 / 31500)
- `cargo test -p herdsman --test drive_kicks --test drive --test waiting --test ui_lock --test compaction --test serve_web --test live --test api_v1 --test host_tab --test queue_tab` → exit 0
- `cargo test --release -p herdsman --test perf -- waits_and_the_lanes_tab a_hundred --nocapture` → exit 101: `100 board requests 1397 ms of CPU` (passed); #447's wall-clock `snapshot::waits took 423 ms (target < 200)` at load 29 on 12 CPUs. Re-run alone: `cargo test --release -p herdsman --test perf -- waits_and_the_lanes_tab --nocapture` → exit 0, waits 35 ms, lanes 48 ms.

## Earlier evidence (head `a034e8d0d0484e797b774f814f423bf46fa99752`, private `CARGO_TARGET_DIR`)

- `cargo fmt --check` → exit 0
- `cargo build --release --locked` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `./target/release/herdsman cap-check --config ci/cap.toml` → exit 0 (27851 / 31500)
- CI's perf step as now written: `cargo test --release --locked --test perf -- waits_and_the_lanes_tab_stay_fast_on_a_30k_event_log a_hundred_board_requests --nocapture --test-threads=1` → exit 0: `100 board requests 1169 ms of CPU`; #447's waits 138 ms, lanes 42 ms
- `cargo test --locked --no-fail-fast`: something outside the tests killed this run twice partway (exit 144, at different binaries). Every target ran and passed across that run and the next steps: 112 `herdsman` test binaries in that run with no failure, then `cargo test --locked --no-fail-fast -p herdsman --test transcript_tail --test ui_audit --test ui_lock --test ui_tokens --test usage --test verify_stage --test waiting --test webpush` → exit 0, `cargo test --locked --no-fail-fast --workspace --exclude herdsman` → exit 0, `cargo test --locked -p herdsman --doc` → exit 0
- `cargo test --locked -p herdsman --test live_board` at load 13–34: 2/3 runs pass, 1 fails on `live_board_chat_and_ascii_in_webkit` (`/live lanes` 0); passes behind a 20 s outside lock hold (waits, "held by HOLDER"). Staging (`362fb7f4`) fails that same check 2 of 4 runs at the same load: a load flake that predates this PR.
- The new tests fail on base: with `crates/core/src` and `crates/server/src` reverted, `room_for_one_spawn_goes_to_the_review`, `a_busy_host_holds_every_spawn_behind_the_review` and `waiting_names_the_reviews_ahead` → 3 FAILED (exit 101). The CPU budget measured about 3.4 s before the change (fixture with a new event every 5th request).

### Fix round (CI `rust` red at `78b7c77e`)

- `board-freeze` (#569): this PR changes `serve.rs` and `serve/chat.rs` (scope 3, the board server's CPU). Owner decision q573: #560 takes a `board-fix` amendment, since scope 3 is a performance bug fix.

- CI's perf step passed two test names before `--`. Cargo takes one filter there, so it exited 1 on the second name. Both filters now go after `--`, where the test runner takes several. It also runs them one at a time (`--test-threads=1`): the CPU budget reads `getrusage(RUSAGE_SELF)`, which counts the other test's 30k-event fixture work too (1245 ms side by side vs 557 ms alone).
- The full run then showed `live_board` breaking under the lock, which serialized its two WebKit tests:
  - With the lock, the herd test's WebKit always ran after the chat test had deleted the temp `$HOME` it set for the whole process. WebKit under a missing home times out every time (`HOME=/nonexistent` → 2/2 timeout). `common::swift()` now gives WebKit the user's home from the password database; the same run then passes.
  - The chat test fires a `/live` update 2.5 s after `page()` starts. A lock wait inside `page()` put that update before the page loaded. Each `live_board` test now holds the host's UI audit lock for its whole run (`common::ui_lock()`, the same file and `flock` as `ui-lock.sh`), so its clock starts once WebKit is free.

New tests:
- `drive_kicks.rs`:
  - `room_for_one_spawn_goes_to_the_review`: one slot, with a fix round, a pushed head and a queued issue due → the review's kick launches.
  - `a_busy_host_holds_every_spawn_behind_the_review`: a busy host launches nothing and tries no start; once the room is back the review launches before the start.
  - `waiting_names_the_reviews_ahead`
- `ui_lock.rs`:
  - a second lane waits with the holder named, then runs once the first ends;
  - a dead holder lets go at once;
  - a lock held 10 min is run past;
  - the WebKit `swift` waits on the same lock.
- `compaction.rs`: `the_shared_read_follows_appends_and_rotations`
- `perf.rs`: `a_hundred_board_requests_on_a_30k_event_store_stay_under_the_cpu_budget`

## Not verified

- AC3 (live over a day: no review more than one pass behind a new start, serve under 20% CPU on average) needs the install and a day of the real fleet.
- A conflicted merge's `--update` lane is still spawned inside the merge step, so it runs before reviews later in the same pass. Every other update round is ordered after reviews.






