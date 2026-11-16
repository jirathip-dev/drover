Closes #1160

## For humans
In the Threads tab, a message handed over from another thread now says which thread it came from (`from t1`) and has a chip back to it, where it used to read "orchestrator".
When you pick a thread for a handoff, the card then shows a chip to the thread it went to. If delivery failed, the card turns red and shows the error.
Tapping a pick twice still delivers it only once.

## What changed
- `crates/server/src/serve/chat.rs` (drawing only, in the existing Threads card blocks):
  - A received message (`thread-msg` from a thread id) is titled with its link (`from t<n>`). Its existing chip opens the source thread.
  - A pending repo handoff (the `needs-decision` with `handoff`) keeps the existing choice row (the repo's threads with title and last topic, plus `new thread: …`). Its values now carry the card's `fleet`, so a repeat tap from a stale row reaches `land` (#621) and delivers nothing more, instead of failing with "no open ask".
  - Once delivered, the card gets a `continued in t<n>` chip to where it went. If delivery failed, the card is `tone: bad` with `✗ not delivered: <error>`.
  - These states come from API v1's existing handoff fold (`v1::handoff`, now `pub(super)`). There are no new endpoints and no new event kinds.
- Already drawn, and now guarded by the test: the sending row (`<title> → <target title> · t<n>` with its `continued in t<n>` chip), the `(forwarded)` mark, and flow-back one-liners (`filed #52: …` with `from t<n>`).
- `docs/parity.md` row 35 is now `parity_35 | kept`.
- Golden `cooee/thread.json`: one card title changes from `orchestrator` to `from t3`. That message came from thread t3, so the change is intended.

## Parity
- Row 35: `open` → `kept`, guarded by `parity_35_handoff_chips_flow_back_lines_and_a_pick_a_thread_tap_that_delivers_once` (WebKit, `crates/cli/tests/parity_webkit.rs`). It covers the sent row's chip, the flow-back row's chip, a failed card's error, the pick rows, a tap on `new thread` landing in t3 with its chip, a second tap on the same (now stale) button, the target thread's `from t1` card with its chip, and in the log exactly one `thread-handoff`, one new thread and one decision.

## Evidence (head 4c6405ce0681d19c9bfa417e678f4895ee7248fa)
The lane's own `DROVER_OUTBOX`/`DROVER_HOST`/`DROVER_LANE`/`DROVER_RULES` were unset for tests, because they redirect the CLI into the lane's outbox.
- `cargo fmt --check` → exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` → exit 0
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI's flags) → exit 0
- `cargo test -p drover --test parity_webkit` → exit 0 (10 passed, 1 ignored). **No Playwright/WebKit on this host, so the WebKit tests skip here.** CI's ui-webkit job is the real run of `parity_35`.
- `cargo test --workspace --no-fail-fast` → exit 101, with one failure: `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`. It fails the same way on the base f6e8437e (it needs macOS `sysctl`/`vm_stat`), so it is unrelated. The other 211 test binaries are ok, including `threads_tab` (goldens) and `thread_handoff`.
- `bash scripts/parity-check.sh` → exit 0 (117 rows, 58 tests)
- `drover cap-check --config ci/cap.toml` → exit 0 (47149 / 47200)
- `drover arch-check` → exit 1, but only because `just` is not installed here (`just gen` exited 127). This PR changes no generated file.

src/ net +33 (budget: none stated). Small because the backend (delivery, flow-back, the v1 handoff fold) already existed; only the drawing changed.

Decided: kept the existing card kinds and choice block (no new CSS, no new block type), so nothing outside these rows draws differently.
