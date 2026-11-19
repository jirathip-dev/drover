Closes #1217

## For humans
A message you send on the Threads screen now shows once, from the tap until the orchestrator answers, with its photo and its ✓.
Before, a second copy of the same message (and photo) could appear right after sending and stay until the thread next refreshed.

## Parity
Row 21 (Instant send: sending…/✓, red retry on failure): kept; a sent message now stays one bubble from send to ✓. No row dropped.

## What was wrong
There is no separate pending bubble: both copies were the same logged message. After a send the page reloads the thread (300 ms later) while its live stream also announces the new message as an `insert`. When the stream's insert landed after the reload, the page added it a second time, because applying an `insert` never checked whether that block was already there.

## Fix
`web/src/ops.ts`: an `insert` whose target list exists first drops any block with the same id, so applying it twice is the same as once (the server's newer copy wins, in its place). `web/dist/board.js` rebuilt with `npm run build`.

No client nonce: the two copies already share one id (`m-<seq>`); matching on it is enough, and no composer change was needed.

## Repro
Repro: run, both in CI's ui-webkit job (real WebKit at 390; lanes may not install a browser).
Command: `cargo test --locked --test threads_tab a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390`, which ui-webkit runs in its "the thread view in WebKit" step.

Base + test only, at 50bd53653246bc23566b4d0baf7046756e3e27fc (this branch with staging's `web/src/ops.ts` and `web/dist/board.js` put back; its diff from the merge base is only `crates/cli/tests/threads_tab.rs`), run 44294361260: RED
```
test a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390 ... FAILED
thread 'a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390' panicked at crates/cli/tests/threads_tab.rs:624:5:
assertion `left == right` failed: never two bubbles, one after the echo, its ✓, its photo
  left: [2.0, 2.0, 0.0, 0.0]
 right: [1.0, 1.0, 1.0, 1.0]
```
With the fix, at 631ae17f359518570bea9641a4897dbe7fbd9a83, run 44231854711: GREEN
```
test a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390 ... ok
```
Commit 1 (54d7c0f7) holds only this test. The same test at the new head 1962faeea71933406b95d25834017f7469b42dde: CI run 44295677898, every check green, `... ok` in ui-webkit.

## Round 7 (review fix: the red run) at head 1962faeea71933406b95d25834017f7469b42dde
The finding was infra: the reviewer's host has no Playwright. This round gets the red run from the PR's own CI without a second PR or a force-push. 50bd5365 put staging's op applier back, CI ran the test in WebKit (red, above), and db9848da reverted that commit. Then I merged current `origin/staging`, with no conflicts.
- `npm run build && npm run check && npm run lint` in web/: exit 0 each; web/dist unchanged after the rebuild.
- `cargo fmt --check`: exit 0.
- `cargo test --locked -p drover --test threads_tab`: exit 0 (17 passed; WebKit tests skip on this host).
- At 50bd5365, `test (shard 4/4)` also went red on `drive_kicks::failing_retries_never_starve_another_fleets_start`. That commit touched only `web/`, so the failure isn't from this PR (green at head, run 44295677898).

## Round 6 (review fix + update onto staging) at head 631ae17f359518570bea9641a4897dbe7fbd9a83
Merged `origin/staging` (dc199874), no conflicts, no `web/` change from staging; added this `## Parity` section (review's blocking finding).
- `cargo fmt --check`: exit 0.
- `cargo test --locked -p drover --test threads_tab`: exit 0 (17 passed; WebKit tests skip on this host).
- `npm run build` in web/: exit 0, web/dist unchanged.
- ui-webkit red at bf0964d8 (run 44223610664) was `perf.rs:297` `the_first_paint_in_webkit_is_under_300ms` (first contentful paint 806 ms on the runner), not this PR's test: the same job logs `a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390 ... ok`. This PR changes only the op applier for stream inserts, not first paint. CI reruns at 631ae17f.

## Round 5 (update onto staging) at head bf0964d896c07c9ab5d99de7335dfac3fea49a04
Merged `origin/staging` (conflict in generated `web/dist/board.js` only); resolved by rebuilding it from the merged `web/src`, not by hand.
- `npm ci && npm run build && npm run check && npm run lint` in web/: exit 0.
- `cargo fmt --check`: exit 0. CI's two `cargo clippy` commands: exit 0.
- `cargo test --locked -p drover --test threads_tab`: exit 0 (17 passed).
- `cargo test --locked --workspace`: exit 101, one failure, `shed.rs` `a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`: it expects `shed` off PATH, but this lane host has it in `~/.cargo/bin`. This PR does not touch shed (its diff vs staging is threads_tab.rs, ops.ts, board.js).

## Round 4 (review fix) at head a34b663e83560b8f395de129c90ee5bd0cebaf6a
- `web/src/ops.ts`: an insert drops an earlier copy only when the block has an id, so an insert with no id leaves its id-less siblings alone (reviewer's non-blocking finding). `web/dist/board.js` rebuilt.
- `npm ci && npm run build && npm run check && npm run lint` in web/: exit 0 each. Before the change, a rebuild at 0e9efed6 left web/dist unchanged.
- `cargo test -p drover --test threads_tab`: exit 0 (16 passed; the WebKit tests skip here).
- CI at a34b663e83560b8f395de129c90ee5bd0cebaf6a: every check green. In the ui-webkit job log, `a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390 ... ok` ran in real WebKit (no skip line).

## Round 3 (CI fix) at head 0e9efed6eaf40a92a94597a3d0c2c8f2e8375ca4
The red `rust` check at 411e1a2c came from staging at that time, not from this PR's files (threads_tab.rs, ops.ts, board.js): arch-check flagged `crates/server/src/serve.rs` over its size baseline, shard 2 found a leftover `fleet-test-*` temp dir, and shard 1 hit the `sim` hunt test. This PR touches none of them. Merged current `origin/staging` (5451162a, its CI green), no conflicts.
- `npm run build && npm run check && npm run lint` in web/: exit 0; web/dist unchanged after the rebuild.
- `cargo test -p drover --test threads_tab --test cooee_screens`: exit 0 (24 + 16 passed).
- CI at 0e9efed6eaf40a92a94597a3d0c2c8f2e8375ca4: every check green (rust, test shards 1–4, web, ui, ui-webkit, evidence, actionlint).

## Round 2 (review fix) at head 411e1a2c0ca9e7bf3e5bfa9f25f0c3559437c65a
The drop now runs only after the `into` target and its list are found, so an insert into a missing target leaves the existing block in place.
- Reviewer's proof command (`node --experimental-strip-types`, insert `m1` into `gone`): now `{"id":"root","blocks":[{"id":"m1","t":"x"}]}`; insert into `root` still replaces it in place.
- `npm run build && npm run check && npm run lint` in web/: exit 0.
- `cargo test --locked -p drover --test threads_tab`: exit 0 (16 passed; WebKit tests skip locally, CI ui-webkit runs them).

## Evidence at round-1 head 5028f06218f8ea90a8b2af45142bf40f344a4b0e
- `cargo test -p drover --test threads_tab --test cooee_screens`: exit 0 (15 + 16 passed). WebKit is not installed on this host, so the WebKit tests skip locally; CI's ui-webkit job ran them at this head: `a_sent_message_with_a_photo_shows_once_from_tap_to_echo_in_webkit_at_390 ... ok`, job green.
- CI `test (shard 1/4)` is red on `sim::the_hunt_finds_a_planted_bug_sooner_than_random_seeds_and_minimises_it` (crates/cli/tests/sim/main.rs:291): the same test fails on staging's own CI at c3d24d5d, this PR's base; it does not touch the page.
- `npm run build`, `npm run check`, `npm run lint` in web/: exit 0 each.

src/ net +1 (budget n/a): one line in the page's op applier.

Not verified here: the base-commit failure of the new test in WebKit (no WebKit on this host; the reviewer's repro run), and the owner phone check.
