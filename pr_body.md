Closes #1087

## For humans
The Threads screen on the phone page now has a full message box. It grows as you type, you can attach photos and files, and you can see each message being sent, sent, or failed with a retry button that keeps your text and files.
While the orchestrator is working, a pill shows how long it has been working with a [stop] button. New messages queue until the turn ends and can be sent now, edited or removed. A long press on [queue] offers [steer].
Model and effort chips sit under the box, and a new thread lets you pick its harness. Typed text is kept as a draft per thread, as the other boxes on the page already are.

## What changed
- `crates/server/src/serve/chat.rs`: the thread screen's composer is now a page-only `composer` block. It carries the thread, `working.since`, the queued list, the model/effort chips and the op names. A new thread gets a harness chip instead. A sent message's photos and files show through #1086's message card (merged from staging; this branch's own renderer for them was dropped in the merge).
- `crates/server/src/screens.rs`: `/action` now accepts `thread-queue`, `thread-steer`, `thread-unqueue`, `thread-stop` and `thread-model` (beside #1086's `thread-secret`, `thread-config` and `thread-signin`), and passes `name`, `data`, `q`, `harness`, `model` and `effort` through to the existing `/thread` ops.
- `web/src/blocks.ts`: the `composer` renderer.
  - A growing textarea: Enter makes a new line; [send] or Cmd/Ctrl-Enter sends.
  - [+] attaches up to 10 files, with thumbnails and [×].
  - Send states: sending… / ✓ sent / not sent with [retry], keeping text and files on failure. The red toast's [retry] works too.
  - The working pill with timer and [stop], then stopping…, or offline · tap to retry.
  - [queue] during a turn, [steer] on long press, and the queued list with [send now] / edit / remove.
  - The chips.
  - The generic `form` keeps its `submit` path: #1086's secret card uses it.
- `web/src/main.ts`: #1084's repaint keeps a focused `textarea` block in place, as it already did for `input` and `select`. #1086's long-press [quote] writes into the composer's box.
- Drafts (#1084, merged): the composer's text uses #1084's stored draft under the same key (`draft:thread-<id>:say:text`), so it survives a reload. Attached files and chip picks are kept in page memory across repaints.
- `crates/cli/tests/ui/drafts.mjs` (#1084's WebKit test): it now drives `.cmp textarea` and sends with Ctrl+Enter, since Enter makes a new line.
- #1086's WebKit tests (`threads_tab`, `parity_webkit`) find the thread's box as `.cmp textarea` instead of the old one-line form; `board.css` sticks `.cmp` to the bottom of a focus screen as #1086 did the old form.
- CI: #1086's `threads_tab webkit` step in `ui-webkit` now also runs this issue's two WebKit tests (their names end in `_in_webkit`), and the job also triggers on `serve/chat.rs` and `threads_tab.rs`. #1084's and #1085's steps are kept.

Not done: "reconnecting…" on the pill, and the "no push for the open thread" row from the issue's table, which isn't in Wanted.

## Parity
Rows in `docs/parity.md` (#1094) that named #1087:
- **21** Instant send: sending…/✓, red retry on failure → `page-only`, test `parity_21_two_photos_go_with_the_message_and_a_failed_send_keeps_them_in_webkit`.
- **26** Queue or steer during a turn; edit or remove queued → `page-only`, test `parity_26_a_message_sent_while_working_queues_and_send_now_steers_it_in_webkit`. Edit and remove are covered by `parity_34`'s ops.
- **34** Harness pick at thread start; model/effort chip in composer → `page-only`, test `parity_34_each_composer_op_reaches_its_thread_op_and_the_screen_shows_it`.
- **22** Growing composer, pinned send, smart follow-scroll → `open` (was `open #1087`). The growing box and the pinned send landed; smart follow-scroll is not in #1087's Wanted and has no test, so the row waits for the owner.
- **25** Working pill with timer and [stop]; reconnecting/offline states → `open` (was `open #1087`). The pill, timer, [stop], stopping… and offline landed; "reconnecting…" did not, so the row stays open.

Dropped: none.

src/ net +22 vs staging (budget: none stated): the five thread ops' plumbing and the composer block, which now merges its extra fields without JSON indexing (#809); the UI itself is in web/src (TypeScript, soft budget).

## Evidence (head 71a4444df58f84500f472e5d840de870c9449fc9)
Round 2 fixes the red `ui` check at b867cab: the UI audit's `thread` step waited for the old `.form.say` box, so it printed nothing. `assets/ui-audit.js` now waits for `.cmp`. The composer's chip rows were 8 px apart, so their 44 px tap areas overlapped [+] and each other; they are now 16 px apart (`--s4`, as `.chips` everywhere else). The dead `.form.say` CSS is gone. Staging merged in twice, the second time resolving a `web/dist/board.js` conflict by rebuilding. #809's stricter types and lints, new on staging, are applied to the composer: typed reads in `blocks.ts`, and no `value["key"] =` indexing in `chat.rs`.
- `scripts/ui-audit.sh <site> <shots>` with `UI_AUDIT_VIEWS=thread`, on a fresh `serve --export --demo`: the thread view now has 13 controls and no tap-area outliers. Exit 1 only from the two outliers caused by this host's Chrome (`viewport 500 px wide`, smoothed sprite pixels). This host has new-headless Chrome, not chrome-headless-shell, and staging's `threads` view shows the same two here. CI's `ui` job is the real gate.
- `cargo test --locked --workspace --no-fail-fast`, run with this lane's `DROVER_*` variables unset (with them set, `answer_delivery` takes the remote-lane path): exit 101, from one failure, `drive_now_on_the_board_is_paired_only_and_toasts_its_line` (a timing line, `0.2 s` vs `0 s`, on a loaded debug build). Rerun alone (`cargo test --locked --test drive drive_now_on_the_board`): exit 0.
- `cargo test --locked --test threads_tab --test parity_webkit`: exit 0 (12 + 9 passed, 1 ignored). The WebKit tests return early here (no Playwright) and run in CI's `ui-webkit`.
- `cargo fmt --check`: exit 0. CI's two clippy commands (`--workspace --lib --bins -D warnings`; `--workspace --tests` with CI's `-A` list): exit 0 and 0.
- `npm run check` and `npm run build` in `web/`: exit 0; `web/dist` is the build. `npm run lint` (#809's banned TypeScript): exit 0, 0 violations; the working pill now uses `nodes()` like the other blocks, not `innerHTML`. `scripts/lint-fixtures.sh ts`: exit 0.
- Staging merged again at this head (#1120 docks the composer below a `.msgs` scroller). Resolution: the `.msgs` wrapper and #1120's static-in-focus rule target `.cmp`; `.form.say` and `.card .form.say` CSS kept for #1086's secret card; `threads_tab`'s `open()` waits for `.cmp textarea` and returns `.msgs`; #1120's dock test drives the composer's textarea and its real growth (the input's `minHeight` stand-in is dropped); `drafts.mjs` scrolls `.msgs`. After the merge: `npm run check`, `npm run lint`, `npm run build`: exit 0. `cargo fmt --check`: exit 0. `cargo test --locked --test threads_tab --test parity_webkit --test page_drafts --test cooee_screens`: exit 0 (13 + 1 + 9 + 13 passed, 1 ignored; WebKit tests run in CI). Thread audit: 13 controls, no tap-area outlier (only this host's 500 px viewport). `cap-check`: exit 0, 46343 / 47200.
- CI at 71a4444df58f84500f472e5d840de870c9449fc9 (this head): every job green, `ui-webkit` (the adapted #1120 dock test and this issue's WebKit tests), `ui`, `web`, `rust` and all four test shards.
- CI at 4ff50219e462a19e5ba1fed9dfc9fcccbf7f0fb2 (the head before this merge): every job green (`web`, `ui`, `ui-webkit`, `rust`, four test shards, `actionlint`, `evidence`).
- CI at b6206d26 (the head before this lint fix): `ui`, `ui-webkit`, `rust` and all four test shards succeeded; only `web`'s new ts lint failed, fixed at this head.
- `drover cap-check --config ci/cap.toml`: exit 0, 46332 / 47200. `scripts/parity-check.sh HEAD^1`: exit 0 (117 rows, 57 tests). `scripts/timing-check.sh`: exit 0. `drover lesson-check`: exit 0.
- `drover arch-check`: exit 1, its one violation `just types` exit 127 (no `just` on this host); no generated type changed in this round.

Not verified here: the owner's phone check (attach a screenshot from Photos and send it).

## Why (round 1)

Decided: on merging staging, a sent message's files render only through #1086's msg_card; this branch's own files() renderer is deleted.
Because: crates/server/src/serve/chat.rs:740
Decided: the generic form keeps its submit path, since #1086's secret card posts its masked value through it.
Because: crates/server/src/serve/chat.rs:899
Decided: #1086's tests, quote and focus-screen CSS target the composer's .cmp textarea, the thread's only box now.
Because: "A `textarea` that grows to 6 lines; Enter makes a new line, [send] sends"
Rejected: keeping both renderers — every attachment showed twice in the thread golden.
Check: thread.json and thread-cards.json differ from staging only by the composer block; the ui-webkit job runs the threads_tab webkit filter, which matches the _in_webkit tests.
Owner: Please attach a screenshot from Photos on your phone and send it; that check can only be done by hand.

<sub>note on b867cab · git log --notes=why</sub>

