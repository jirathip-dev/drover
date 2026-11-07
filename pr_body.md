Closes #584

## For humans
A question in a chat thread now stops showing the red "choose" badge once it's really settled: when the owner answered it in words, the conductor replaced it, a handoff went out (or failed), or nobody has been waiting on it for a day.
Questions a running lane is still waiting on never expire; instead the reminder repeats once a day.
On the first check after this ships, today's stale questions are cleaned up in one pass.

## What changed
- `events::closed` knows every ending: `answered`, `withdrawn`, `delivered`, `failed`, `expired`, `closed: merged`, `closed: lane gone` (one function; `open_asks`, the chat `choose` state, `herdsman asks` and the needs cards all read it). `herdsman asks --all` rows read `[withdrawn → q<new>]`, `[delivered to t<n>]`, `[failed]`, `[expired]`.
- `herdsman answer <fleet> <id> --typed "<line>"`: accepted only when the line is an owner `thread-msg` in the ask's thread (or the thread its lane ask was put to the owner in) after the ask; logs `decision` `by: owner (typed)`; else `REFUSED NOT_TYPED` (asked in no thread / no owner message says that / the line is in tN not the ask's thread / it came before the ask). Runs for any conductor (before the `HERDSMAN_THREAD` → `--basis` path), so it is no `OWNER_ASK`.
- `herdsman ask withdraw <fleet> <id> --by conductor-<t> [--for <new id>]`: only the ask's own thread's conductor; logs `ask-withdrawn`.
- Handoff cards: the tap's decision now carries `to` (closed as `delivered`); a send that fails logs `ask-failed`.
- `check`: an ask open over `[limits].ask_expire_hours` (default 24) that nobody waits on (thread ask, handoff card, or an ask whose lane is no live agent) → `ask-expired`. A live lane's ask never expires; its `ask-waiting` key is `q<id>@<day>d` after day 0, so it re-alerts once per 24 h. Unknown liveness (herdr not answering, the ask's host offline) counts as live.
- One-time sweep: the first `check` on a log with asks closes handoff cards whose `thread-handoff` went out (`ask-delivered`, no decision was logged before) plus the expiries, then logs one `asks-swept {count}`; never again.
- Rule in `roles/conductor.md` (shipped brief): "When the owner answers an open ask in words, run `herdsman answer <fleet> <id> --typed "<their exact line>"` before acting on it; withdraw an ask you replace." `NOT_TYPED` in the usage doc; docs/lanes.md and fleet.example.toml.

Decisions: the board UI files (serve/*, web/*) are frozen (#552, no `board-fix` in this issue), so no UI file changes: the thread's `! choose` goes because `chat::open_ask` already reads `closed()`; the "withdrawn → q<new>" / "delivered to t<n>" / "expired" wording is in `herdsman asks` rows (and its JSON `closed`). An ask with no lane and no thread (an owner's own) does not expire: the spec lists only thread asks, handoffs and gone lanes.

src/ net +239 (budget not stated in the issue); five new behaviours plus two CLI verbs. Cap: 29894 / 31500 after merge.

## Evidence (head 17ac4ef50d9070c6edf036957e5f5217951f58fe)
- Repro, commit 1 (53533c12): `cargo test --locked --test ask_endings` → exit 101, 7 failed (on the base code).
- Head: `cargo test --locked --test ask_endings` → exit 0, 7 passed (also in an isolated target dir after the staging merge).
- `cargo fmt --check` → 0; `cargo clippy --all-targets --locked -- -D warnings` → 0; `herdsman cap-check --config ci/cap.toml` → 0 (29894/31500); `herdsman lesson-check` → 0.
- Full `cargo test --locked`: every binary passed on the fix before the staging merge; after the merge, the host's shared target dir was being deleted and overwritten by other lanes mid-run (`never executed`, and issue-565's `ACP_UNGUARDED` code linked into `shed`), so the post-merge full run is CI's: all checks green at 17ac4ef (rust incl. `cargo test --locked`, web, ui, ui-webkit, board-freeze, evidence). `shed` passes in an isolated target. One WebKit timing test (`chat_tab the_thread_follows_its_end…`) flaked once and passed twice on rerun.

Not verified: AC3's 390 px screenshot (no Chrome on this lane; the verify stage covers it); AC4 is live, the day after merge.
