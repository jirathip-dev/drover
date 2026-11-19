Closes #1310

## For humans
The phone page no longer refreshes when nothing on the screen changed, and typing in a thread keeps the keyboard up while the thread updates.
Times like "as of 3m ago" and message ages now count up on the phone itself instead of being re-sent by the server.

## What changed
- Server: the snapshot age, thread message ages and the threads list's ages are sent as `{since, label}` (`crates/server/src/screens.rs` `ticking`), so a screen rebuilt a minute later is identical and the stream sends nothing but pings. The events screen's "earlier days" link rounds up to the hour (it changed every second).
- Page: `value()` renders `{since, label}` / `{until, label}`; a 1 s ticker updates only those spans. The 30 s whole-screen repaint is gone (only the stale strip's `sync()` stays on a timer). A stream delta that leaves the model unchanged does not paint.
- Composer: its row, textarea and send button are built once; a block change (working on/off, queued, chips) is patched in place via a `patch` hook that `morph` calls instead of replacing the node.
- `thread-read`: with no ages re-sent, it changes (and sends data to) only the threads list (its unread dot).

## Repro (`--repro`)
Commit 1 `0c15e04e` holds only the tests; on staging 9ea69d9 they fail:
- `cargo test --locked --test keyboard_webkit`: `{"blurs":1,"focus":false,"same":false,...}` (textarea replaced, focus lost)
- `cargo test --locked --test stream_quiet a_screen_built_a_minute_later_is_the_same_screen_its_ages_tick_on_the_page`: `/screens/queue` differs (`as of 100s ago` vs `as of 2m ago`)

## Evidence at head ef8f5f5e37f3c6af67af7a871c845bb25b63736c (merge of origin/staging 4b1a59b; fix round for red CI at e799151)
- The red at e799151 was two timing flakes outside this diff: `drive_now_on_the_board_is_paired_only_and_toasts_its_line` (a real-time pass took 0.1 s vs the expected 0 s; drive code untouched here) and WebKit first paint 1308 ms warm / 132 ms cold (this branch's earlier runs: 15–18 ms warm; this issue's own `keyboard_webkit` step passed in that same job).
- `cargo test -p drover --test drive drive_now_on_the_board` → exit 0
- `cargo test -p drover --test stream_quiet --test keyboard_webkit --test perf` → exit 0 (the WebKit gates skip locally, no Playwright; CI's `ui-webkit` is the real run)
- CI run 44346165292 at ef8f5f5: all jobs green (ui-webkit, shards 2–4 included) except `test (shard 1/4)`: `a_service_killed_mid_spawn_leaves_no_stale_state_and_a_new_one_resumes` (crates/cli/tests/service.rs:290, a kill-timing race; nothing under crates/core, crates/service or that test is in this diff). `cargo test -p drover --test service a_service_killed_mid_spawn` → exit 0 twice locally. A re-run of that job should clear it.

## Evidence at head e79915105ba05cff0fd359c6fe2ae0643c589b1e (fix round: an unchanged delta is still timed)
- The skip on an unchanged delta returned before the phone timing was recorded, so #1312's gate (`phone_timings.rs:76`) saw no `delta`. The page now records the timing, then skips the repaint.
- `cd web && npm run build` exit 0 (web/dist regenerated), `npm run check` exit 0, `npm run lint` exit 0.
- `cargo test --locked --test phone_timings` exit 0 and `cargo test --locked --test keyboard_webkit` exit 0 here, but both skip locally (no Playwright module on this host); CI's `ui-webkit` job is the real run.

## Evidence at head a6c8751461b5450a62bf0b759123bd48eb1a7cf2 (merge of origin/staging; conflicts in the CI WebKit filter and main.ts's delta handler kept both sides; web/dist rebuilt)
- `npm run build` / `npm run check` / `npm run lint` → exit 0
- `cargo fmt --check` → exit 0; both CI `cargo clippy` invocations → exit 0
- `cargo test --locked --test stream_quiet --test board_fresh --test threads_tab --test cooee_screens --test serve_health` → exit 0
- `scripts/test-shards.sh check` → exit 0

## Evidence at fb7c50e9e7c0dbb2518d78643bf8ff8296c2c97c (fix round: test shards)
- `scripts/test-shards.sh check` → exit 0 (`keyboard_webkit` in shard 3, `stream_quiet` in shard 1; it failed with "in no shard" for both before)
- Only `scripts/test-shards.txt` changed since 17eb960, so the evidence below still holds for the code

## Evidence at 17eb960230b9beadce148b3668feebea2bde6fff (merge of origin/staging; web/dist rebuilt with npm run build)
- `cargo test --locked --test stream_quiet --test board_fresh --test threads_tab --test cooee_screens` → exit 0
- `cargo fmt --check` → exit 0; both CI `cargo clippy` invocations → exit 0; `npm run build` / `npm run check` / `npm run lint` → exit 0

## Earlier evidence at 3202c7f534fea5f3408033e347f9a703fd792676
- `cargo test --locked --test stream_quiet --test board_fresh --test threads_tab` → exit 0
- `cargo test --locked --test cooee_screens` → exit 0
- `cargo test --locked --no-fail-fast --test keyboard_webkit --test stream_quiet --test page_drafts` → exit 0 (**Chromium** locally: Playwright WebKit can't start on this host (missing system libs), so the scripts were run with `webkit` swapped to `chromium`; CI's ui-webkit job runs the real WebKit, and the new `keyboard_webkit` step is added to it)
- threads_tab / parity_webkit / url_links / issue_sheet_webkit / herd under Chromium the same way → all pass
- `cargo clippy` (both CI invocations) → exit 0; `cargo fmt --check` → exit 0; `npm run check` / `npm run lint` / `npm run build` → exit 0
- full `cargo test --workspace --no-fail-fast`: the only other failure, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`, comes from this host (no sysctl, no gh auth); this PR doesn't touch it
- `drover arch-check` locally: only `just gen` missing (no `just` here); web/dist was built with the pinned esbuild 0.28.2

## Notes
- Goldens `thread.json`, `thread-cards.json`, `threads.json` and `board_fresh` updated for the new `{since, label}` shape (a spec change). After commit 1, `stream_quiet` was anchored to the hour so it can't straddle the hourly link change.
- The new WebKit test has its own binary: serve allows 4 streams per process, and the page_drafts browsers already use them up.
- Not converted: lane ages on the Lanes/Queue screens and the answered/applied lines inside ask, config and run cards still change once a minute (they're not on the thread composer's path); the shared screen cache is the architect issue.
- cooee's native renderer has to draw `{since, label}` in row value, subtitle text, card meta and stat value; I couldn't check that here.

src/ net +2 Rust, web/src +22 (budget: none stated): the composer reshuffle accounts for most of it.

## Parity
- Row 3 (loading strip "as of Xm ago", `parity_3`, kept): the age is now sent as a timestamp and counts up on the page; the row's behaviour and test are unchanged.
- Row 23 / R4 (iOS keyboard, `parity_r4`, kept): the composer and its textarea are patched in place, never replaced, so focus and the keyboard stay; the new `keyboard_webkit` test guards it alongside `parity_r4`.
- Rows 21, 26, 34 (send state, queue/steer, model chip, page-only): same behaviour, now patched into the existing composer instead of rebuilding it.
- Dropped: none.

## Why (round 1)

Decided: ages go out as {since, label} and tick on the page, so a rebuilt screen equals the last one and the stream sends nothing
Because: "A ticking value (`as of Ns ago`, `working 12s`) is computed on the client from a timestamp, not re-sent by the server."
Decided: the composer registers a patch; morph calls it instead of replacing the node, and draw never detaches the textarea
Because: "The composer element and its textarea are never replaced while they exist"
Decided: no rebuild filter per event kind; the stream already diffs and sends nothing on an empty diff
Rejected: per-screen event routing — the shared screen cache is the separate architect issue
Decided: the events screen's earlier-days link rounds up to the hour so it stops changing every second
Rejected: keeping the 30 s repaint for ages — the 1 s ticker updates only the age spans
Check: crates/cli/tests/keyboard_webkit.rs runs in its own binary: serve allows 4 streams per process and page_drafts' browsers fill them
Check: cooee's native renderer must draw the {since, label} shape in row value, subtitle text, card meta and stat value
Owner: the phone page stops refreshing when nothing changed, and typing in a thread no longer drops the keyboard

<sub>note on 3202c7f · git log --notes=why</sub>

## Why (round 3)

Decided: the thread composer is patched in place (same textarea), and ticking ages go to the page as a timestamp it counts up itself
Because: "The composer element and its textarea are never replaced while they exist"
Because: "A ticking value (`as of Ns ago`, `working 12s`) is computed on the client from a timestamp"
Rejected: a server-side diff of every row — the shared screen cache is the separate architect issue
Decided: the staging merge keeps both the delta timing span and the skip of a delta that changes nothing
Because: web/src/main.ts:171
Check: the CI rust job's shard check and the ui-webkit keyboard_webkit step are green at the head
Owner: typing on the phone no longer loses the keyboard, and an idle page stops refreshing

<sub>note on a6c8751 · git log --notes=why</sub>

## Why (round 4)

Decided: an unchanged stream delta is still timed (painted) before the page skips its repaint
Because: web/src/main.ts:175
Rejected: dropping or loosening the #1312 delta gate — the review asked to keep it
Check: ui-webkit is green at this head, with phone_timings seeing a delta route
Owner: the phone speed check counts live updates again, even the ones that change nothing on screen

<sub>note on e799151 · git log --notes=why</sub>

