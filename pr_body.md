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

## Evidence (head 8dabfd1485e84e8d735ea2206bc8bc66d0a2be27, origin/staging e4df8b90 merged cleanly)
Round 3 fixes (review of 681ea3e):
- `web/src/board.css`: in a thread, the loading/last-update strip floats over the top of the messages. It used to sit in the page's flow, so when it appeared or went away the message box's top moved by about 24 px and the card on screen moved with it. That was the drafts test's 417.5 vs 393 reading, both live and on the 30 s tick.
- `threads_tab.rs` older test: it now scrolls 200 px up from the end and taps the marker once. Before, it scrolled to the top first, which already loads the older 30 by itself (parity_29, `older on scroll`). On a local server that load finished within the test's frame wait, so its tap then hit the *new* marker and loaded the thread's start: 30 + 30 + 11 = 71. One tap loads one page.
- Pushback: `parity_2` (events tab) is `#[ignore = "red until #1084: a live update repaints every node"]`. It runs only in CI's soft `--ignored` step, and it is red on other branches too (issue-809 c27b5321, main 18bed205: `[0, 1, 0]`). This PR doesn't touch the repaint of non-thread screens.
- Removed `pr_body.md`, which the previous lane's wip snapshot committed by mistake.

- CI at a7ddbaa and at 8dabfd1: `ui-webkit` pass (threads_tab webkit 4 passed; page_drafts passed; parity_webkit 9 passed, with parity_2 red only in the soft step), `rust` pass, `ui` pass, `web` pass
- `cargo fmt --check`: exit 0; `cargo clippy --locked --all-targets`: exit 0, no warnings
- `drover cap-check --config ci/cap.toml`: exit 0 (src 45600 / cap 47200)
- `env -u DROVER_LANE cargo test --locked --no-fail-fast`: exit 101, the only failure `drive::drive_now_on_the_board_is_paired_only_and_toasts_its_line` (timing: `0.1 s` vs `0 s` under host load); rerun alone `cargo test --locked -q -p drover --test drive drive_now_on_the_board`: exit 0
- `cargo test --locked -p drover --test threads_tab --test page_drafts --test parity_webkit`: exit 0 (WebKit skipped locally, no Playwright here)
- `web`: `npm run check && npm run build`: exit 0, dist committed
