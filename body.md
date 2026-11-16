Closes #1088

## For humans
Tapping an ask, a lane link or an event that points at a thread now opens that thread in the phone page's Threads tab, and a push for a thread opens it straight away.
The open thread stays in the address, so a reload or reopening the Home Screen app keeps it, and back (or a swipe from the left edge in the app) returns to where you were.
Config proposals, secret requests, run cards, orchestrator answers you may still change, and sign-in notes now show their buttons inside the thread, so you can apply, save, approve, deny, change or sign in from there.

## What changed
- `web/src/main.ts`: a `{thread}` tap opens `#threads/<id>`. A lane name opens that lane's sheet, because a lane has no conductor thread. The `#<tab>/<sub>` route is read on load and on `hashchange`. Opening a sub-screen pushes a history entry. The in-page back button and a left-edge swipe (Home Screen app only) call `history.back()`. A reload of `#threads/<id>` seeds `#threads` under it. A `{copy}` tap copies to the clipboard.
- `crates/core/src/webpush.rs`: a thread push links `/#threads/<t>` (was `?thread=<t>#chat`).
- `crates/server/src/serve/chat.rs` `msg_card`:
  - config cards: the diff, [apply]/[discard], and a stale card shows only [discard].
  - secret cards: a masked field with [save]/[dismiss], for paired devices only.
  - run cards: command, why, touches, host and expiry with approve/deny. Once settled: outcome, who, exit code, output tail and [copy].
  - handoff and "from tN" chips that open the linked thread.
  - [change] on an orchestrator-answered ask while its change window is open, and [sign in] on a sign-in note.
- `crates/server/src/screens.rs`: new thread actions `thread-config`, `thread-secret` and `thread-signin`, on the standalone page only. They use the existing settle and setup paths. `answer` falls back to the ask's fleet when the ask is no longer open, which is the [change] path; core still enforces the window.
- `web/src/board.css`: a form inside a card is not sticky.
- Tests:
  - golden `thread-cards.json`, with every card kind in a thread.
  - an action test that applies config c1, saves a secret (the value never lands in the log), denies a run card, changes q9 and starts a sign-in.
  - a WebKit test: a Queue row with an ask opens t9, back goes to `#queue`, `/#threads/t9` opens t9, and back goes to the list. It is added to the `ui-webkit` job.

src/ net +119 (no budget given): four new card kinds, three action arms and the waiting line on the thread bar.

## Parity
- Now `kept`:
  - 9: a push opens its thread, `parity_9`.
  - 30: choice rows, the "other" box and the answered ✓ line, `parity_15`.
  - 31: [change] for 30 min; 32: config card with diff, apply/discard and stale; 33: masked secret entry. All three are guarded by `parity_32_the_owner_settles_each_card_from_inside_its_thread`. For row 32's "waiting bar", the thread's header row now reads `waiting: config c1`.
  - 46: a Queue row with an ask links to its thread, `parity_46_…` (WebKit).
- Changed but still open: 24. `parity_24` is no longer `#[ignore]`d, because back and the edge swipe work. The row goes from `open #1088` to `open`: its side-by-side layout at 1000 px and up is not in #1088's spec.
- Unchanged and still open, `open #1088` → `open`: 35 (this PR draws handoff and "from tN" chips; the pick-a-thread choice row for a pending repo handoff is not in the spec) and 52 (orchestrator sessions listed as lanes is not in the spec).
- Dropped: none.

## Evidence (head 236e1fcd3da06fc233c9d882953743e294d237ea)
Fix round 2: `page_drafts` (#1084) expected a reload to land on the threads list. Because this PR keeps the open thread in the URL, the reload now lands on `#threads/t2`. The test now waits for the thread's composer after the reload and asserts the hash `#threads/t2`. Then merged origin/staging again (no conflicts, nothing under web/).
- Merged origin/staging again (#1124, #1125). No conflicts, nothing under web/. At 236e1fcd3da06fc233c9d882953743e294d237ea: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` → 0; `env -u DROVER_LANE cargo test -p drover --test threads_tab --test cooee_screens --test webpush --test page_drafts --test boat --test cloud` → 0. The lines below were run at a8560a08c.
- CI `rust` at a8560a08c failed only `drive_now_on_the_board…`. It asserts a drive pass's wall time rounds to "0 s"; CI took 0.2 s. Locally 3 runs: 2 passed, 1 failed. Staging passed 3 of 3. This PR's only `crates/core` change is the push URL string, so it cannot slow that pass. `ui-webkit` passed at a8560a08c.
- `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` → 0
- `cargo test -p drover --test page_drafts` → 0 (skips locally without Playwright; CI `ui-webkit` runs it)
- `cargo test --workspace --no-fail-fast` → 101. Two failures, neither in this PR's code:
  - `drive_now_on_the_board…` is a load-timing check ("0.1 s" vs "0 s"). Rerun alone (`cargo test -p drover --test drive drive_now_on_the_board`) → 0.
  - `self_update` reads the lane's `DROVER_LANE`. `env -u DROVER_LANE cargo test -p drover --test self_update` → 0.
- `bash scripts/parity-check.sh origin/staging` → 0 (117 rows, 51 tests, all listed)
- `drover cap-check --config ci/cap.toml` → 0
- The WebKit tests (`page_drafts`, `parity_46_…`, `parity_24`) skip locally (no Playwright); CI's `ui-webkit` job runs them.
- Not verified: the owner phone check (apply a config proposal from inside its thread).
