Closes #943

## For humans
Herdsman's event log no longer grows from huge error messages or from per-pass chatter: each event is kept small, and old routine chatter moves out of the live log (nothing is deleted).
Each herdsman command now keeps a single copy of the log in memory instead of one per reader. On a copy of the mini's database, status, drive, tick and kick now peak under 200 MB, down from about 1.1 GB.

## What changed
- `db::insert` caps every new event at 16 KB (`events::capped`): long strings keep a head and tail with `… N bytes cut …`, long arrays their first items plus `… N more`. Rows imported from an old JSONL log keep their exact text.
- `build-cloned` errors keep the first 5 lines of `cp`'s stderr (`spawn::first_lines`).
- `rotate` first sheds live rows over 16 KB: a PASSIVE kind (no fold reads it) is archived whole, any other kind is rewritten capped.
- Over `max`, `rotate` also archives `driven`, `kick`, `drive-pass`, `lane-state`, `conflict-predicted` and `intent` older than 2 days, keeping the newest of each (kind, fleet, issue, agent, step), which is what readers' `rfind`s look at. All six are PASSIVE, so the fold and the ask index never read them.
- Amendment q1574 (shared read path): `events::read` returns the process's one cached `Arc<Log>`, so callers borrow it and never clone the whole log. Test callers that need owned events use `.to_vec()`.
- `waiting::scan` no longer clones every issue's events into a map up front. It takes one issue's events at a time from the log's issue index (`events::of_issue`).
- `catch_up` keeps the fetched rows' Vec on a first load (no growth by doubling, no second buffer) and builds the indexes in place.

src/ net +98 (budget: none stated): the cap and rotation (+97) from the first round; the shared read is about net zero. cap-check 43857 / 45400.

## Evidence at b83818a0e4689b70e77d094e48e7aa54bd28d447 (merge of origin/staging, round 4)
| command | exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| `env -u DROVER_LANE -u DROVER_ACCOUNT -u DROVER_HOST -u DROVER_PORT cargo test --workspace --no-fail-fast` | 0 |
| `drover cap-check --config ci/cap.toml` | 0 (45082 / 45400) |
| `drover arch-check --base origin/staging` | 0 (932 files, 0 violations) |
| `drover diagram \| diff docs/workflow.md -` | 0 |

Round 4 fix: CI's `ui-webkit` failed at 1dcbf1f because the branch predated staging's #964 (deflake of the WebKit first-paint test: it now waits up to 5 s for the paint entry instead of reading it once). Merged origin/staging to pick that fix up. The conflicts in `lane_actions.rs`, `lanes_sheet.rs`, `live.rs` and `spawn.rs` were resolved to staging's fake-shed versions, with `read(..)` adapted to the shared `Arc<Log>` (`.to_vec()` / `.iter()`). The ui-webkit test needs WebKit and was not run locally; CI decides it.

## Evidence at 1dcbf1f14df8d6f1ea8b54a919a97283823227b6 (merge of origin/staging)
| command | exit |
|---|---|
| `cargo fmt --check` | 0 |
| `cargo clippy --all-targets --locked --keep-going -- -D warnings` | 0 |
| `env -u DROVER_LANE -u DROVER_ACCOUNT -u DROVER_HOST cargo test --locked` | 0 (197 test targets ok) |
| `drover cap-check --config ci/cap.toml` | 0 (44965 / 45400) |
| `drover arch-check --base origin/staging` | 0 (0 violations) |
| `drover diagram \| diff docs/workflow.md -` | 0 |

Merge round: staging's herdsman → drover rename and #944's shared event buffer. `events::read` still returns the one cached `Arc<Log>`, and `catch_up` uses #944's `Events` (`reserve` + `push`). Staging's new tests that called `read(..).into_iter()` now borrow and clone only the rows they filter. `self_update::pause_and_resume_…` fails only while the lane's `DROVER_LANE` is set (the acting identity). It passes with that variable unset, as in CI. The AC4 numbers below were measured at 9996db7, before this merge, and were not re-measured.

## Evidence at 9996db7ee81498be560aaa3d7234aaff169fff50
| command | exit |
|---|---|
| `cargo fmt --check` | 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | 0 |
| `cargo build --release --locked -p herdsman` | 0 |
| `herdsman cap-check --config ci/cap.toml` | 0 |
| `herdsman arch-check --config .herdsman/arch.toml --base origin/staging` | 0 (0 violations) |
| `cargo test --locked --no-fail-fast` | 101: 1437 passed, 3 failed |

The 3 failures (`cloud::bootstrap_installs_…`, `hosts::an_older_herdsman_there_warns_…`, `self_update::pause_and_resume_…`) also fail on base `1534b233d` in this lane. They compare the build's sha stamp and the acting identity, and this lane's environment (a stale `HERDSMAN_SHA`, the lane identity env) changes both. CI should judge them.

## Repro
Commit 1 (`9450634cb`) adds the two tests alone, and `cargo test -p herdsman --test compaction` fails on it: the 5 MB `build-cloned` is stored at 5,242,980 bytes, and rotation of the 15-day log archives 0 of 1,755 old noise events.

## Measure (AC4): a copy of the mini's `state.db` (164 MB of live JSON, 163,354 events), sandboxed
Sandbox: every fleet paused, fake repos, no notifiers, `gh`/`git`/`herdr`/`ssh` stubbed to fail; `/usr/bin/time -l`. `tick` exits 1 in every run (stubbed `herdr status`) after reading the log. "After rotate" first ran one slow `drive` pass (checkpoint + rotate), which left a live log of 109,412 rows / 49 MB and 53,947 archived, with no live row over 16,379 bytes. The "shared read" column is the release build of this head's source, on a fresh copy of that rotated db.

| command | before: peak RSS / wall | after rotate only (ceee148) | after rotate + shared read (this head) |
|---|---|---|---|
| `status herdsman` | 1073 MB / 4.13 s | 499 MB / 3.02 s | **196 MB** / 3.41 s |
| `drive --fast` | 1094 MB / 3.76 s | 524 MB / 3.35 s | **189 MB** / 2.58 s |
| `tick` | 1085 MB / 0.55 s | 520 MB / 0.45 s | **185 MB** / 0.15 s |
| `kick herdsman 943 --now` | 869 MB / 10.49 s | 523 MB / 10.48 s | **189 MB** / 10.14 s |

AC4 is met: all four commands peak under 200 MB. `status` has the least room (196 MB) because the shared log itself is about 185 MB at this size. Moving callers to targeted SQLite queries is the follow-up the owner chose (q1574).

## Why (round 2)

Decided: events::read returns the one cached Arc<Log>; callers borrow it instead of cloning the whole log.
Because: q1574
Because: "the event log is loaded once per process and every events::read caller shares that one copy"
Decided: waiting::scan takes each issue's events from the log's issue index, one issue at a time, instead of cloning every issue event into a map up front.
Because: crates/core/src/waiting.rs:626
Decided: catch_up keeps the fetched rows' Vec on a first load and indexes in place, so no doubling copy is made.
Because: crates/core/src/events.rs:772
Rejected: targeted SQLite queries per caller — the owner made this a follow-up (q1574).
Check: status, drive --fast, tick and kick peak under 200 MB on the rotated copy of the mini's state.db (PR body table).
Owner: Herdsman now keeps one copy of its event log in memory per process, so everyday commands use under 200 MB.

<sub>note on 9996db7 · git log --notes=why</sub>

## Why (round 3)

Decided: merged origin/staging; events::read keeps returning the cached Arc<Log>, and catch_up takes #944's Events reserve-and-push in place of this branch's Vec swap.
Because: q1574
Rejected: none
Owner: The branch now sits on top of the latest staging, including the rename to drover, with no change in behaviour.

<sub>note on 1dcbf1f · git log --notes=why</sub>

