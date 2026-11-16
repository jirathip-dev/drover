Closes #1086

## For humans
Opening a thread on the phone now lands on the newest message, under a bar that stays on top and shows the thread, its repo, harness, model and state.
"↑ older messages" loads the 30 before without losing your place. Images show inline and open in a viewer (zoom, swipe, save); other files are rows you can open.
Links, code [copy], thread refs and handoff chips are tappable. A long press offers copy, copy as markdown and quote. Only the messages scroll, and the composer sits on the keyboard.

## What changed
- `serve/chat.rs`: the thread screen has a sticky bar row (title · id; `waiting: config …` before repo · harness · model · state). It takes `?before=N` (the 30 before N plus every later one; the stream keeps the query) and an `older` button. Message cards carry their files (an `images` block, file rows opening `/thread/<t>/<name>`) and their link chip (`from t5`, `continued in t6`), which opens that thread via #1088's route.
- `web/`: a focus screen scrolls inside, between the header and the tab bar or keyboard (visualViewport). It opens at the end, follows the end, and keeps the card in view in place on repaint, on older loads and on back. Markdown gets plain URLs, code fences with [copy], and `t<n>` refs (thread screens only). Also an image viewer and a long-press/right-click menu (copy, copy as markdown, quote).
- CI `ui-webkit` runs the four new WebKit tests, and its change filter now includes `serve/chat.rs` and `threads_tab.rs`.
- Not done (in the problem table, not in Wanted): side-by-side at ≥ 1000 px, message times in the phone's clock.

src/ net +62 (no budget given): the screen's bar, window and file/link blocks; the rest is page TypeScript.

## Parity
Rows this PR changes in docs/parity.md (#1094), all from `open #1086` to `kept`:
- 18 message blocks (markdown, plain URLs, `t<n>` refs): new `parity_18`
- 19 fenced code [copy]: new `parity_19`
- 20 attachments and image viewer: `parity_20` un-ignored; the viewer now fits the image to the screen width
- 23 and R4 iOS keyboard, only the messages scroll: `parity_r4` un-ignored
- 28 long-press menu (copy, copy as markdown, quote): new `parity_28`
- 29 sticky header, last 30, older on scroll: `parity_29` un-ignored; reaching the top now loads the older 30 by itself
Dropped: none. Still open from this issue's table: side-by-side at ≥ 1000 px (row 24) and phone-clock times.

## Evidence (head 681ea3ed7280c93b72c3f18e08a8913da09f5db5, origin/staging cbda5640 merged cleanly)
Round 2 fixes (review of f85cc85):
- `web/src/main.ts`: a scroller that a repaint replaced ignores its late scroll event. While detached it read scrollTop 0 and loaded the next older page, which gave 71 cards instead of 60 in the older WebKit test.
- `crates/cli/tests/ui/drafts.mjs` (#1084's page_drafts): scrolls the thread's message box 400 px rather than the page, which no longer scrolls in a thread, and reads that box's scrollTop. Under the old setup the page scroll did nothing, so the view stayed at the end and followed new messages, as this issue asks.

- `cargo fmt --check`: exit 0; `cargo clippy --locked --all-targets`: exit 0, no warnings
- `drover cap-check --config ci/cap.toml`: exit 0 (src 45786 / cap 47200)
- `cargo test --locked --no-fail-fast`: exit 101. The only failures were `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` (it reads this lane's `DROVER_LANE`) and `drive_kicks::failing_retries_never_starve_another_fleets_start` (flaky under host load). `env -u DROVER_LANE cargo test --locked -q -p drover --test self_update --test drive_kicks`: exit 0
- `cargo test --locked -p drover --test threads_tab --test page_drafts --test parity_webkit`: exit 0 (the WebKit tests skip here because this host has no Playwright; CI's ui-webkit job runs them)
- `web`: `npm run check` exit 0, `npm run build` exit 0, dist committed
- `drover arch-check --base origin/staging`: 1 violation, only `just types` not found on this host (exit 127); CI runs it with `just`

