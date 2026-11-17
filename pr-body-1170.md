Closes #1170

## For humans
In an open thread, or any screen with a back button, you can now swipe in from the left edge of the phone to go back. The screen follows your finger. Let go past about 40% of the width and you go back to where you were in the list. Let go earlier and it slides back into place.

## What changed
- `web/src/main.ts`: the old swipe, which only worked in the installed app and only when you lifted your finger, is replaced with touch tracking:
  - It starts on a touch at x<20 on a pushed screen, with no sheet open, not in the two-pane layout (#1118), and not inside the image viewer, an image strip or a code block.
  - The screen's blocks move with the finger (`translateX`). A swipe that turns out to be vertical is dropped.
  - On release past 40% of the width it calls `history.back()`. Otherwise it eases back.
  - `go()` saves the scroll position of the tab on screen per path and restores it on back, so the list comes back where it was (for the back button too).
- `crates/cli/tests/parity_webkit.rs`: new `parity_24_an_edge_swipe_…`. At 390 px with 40 threads: open the older-threads fold, scroll the list, open thread t1, drag to 30% (the view follows, it stays in the thread, the offset is cleared), drag to 60% (back to the list, offset cleared, `scrollY` the same as before).
- `docs/parity.md` row 24: `kept` with parity_24. The swipe comes from #1170 and the side-by-side half from #1118, which is now on staging.

## Fix round 1
- ui-webkit failed because the list never scrolled: only 3 threads show and the rest sit in a closed "older threads" fold. The test now opens it.
- After merging staging, the composer is `.cmp` (#1087), not `.form.say`, so the test now waits on `.cmp` and opens the seeded thread t1.
- Reviewer note: `go()` now saves scroll only for the tab on screen.

## Parity
Row 24: kept (parity_24). The swipe half is from this PR; the side-by-side half came with #1118.

## Evidence (head a73862c58837cc0ee24da79e021194665c59ca75)
- CI `ui-webkit` at a73862c: pass. It runs `cargo test --locked --test parity_webkit`, the new swipe test included: https://github.com/jirathip-dev/drover/actions/runs/43933824116/job/133533115009
- CI `rust`, `test` shards 1–4, `ui`, `web`, `actionlint` at a73862c: pass
- `cd web && npm ci && npm run build && npm run check && npm run lint` → 0 (0 violations; dist committed)
- `bash scripts/parity-check.sh` → 0 (117 rows, 57 tests, all listed)
- `cargo fmt --check` → 0
- `cargo test --locked --test threads_tab` → 0 (15 passed)
- `cargo test --locked --test parity_webkit` locally, with Chromium standing in for WebKit (not committed): 0 (10 passed, 1 ignored). This host has no Playwright WebKit.

src/ net +0 (budget n/a): only `web/`, tests and docs changed; no Rust in src/.
