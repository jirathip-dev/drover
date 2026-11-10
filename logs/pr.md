Closes #778

## For humans
Tapping [one more round], answering an ask whose lane is gone, or approving/revising a design no longer makes the card bounce back when the Mac is busy or no spawn slot is free. The card leaves the gate at once, the issue's row says "starting, waiting for host: <reason>", and drive starts the lane by itself once there is room.
The card only comes back if the start is really refused, or is still waiting after 30 minutes, and then it says why.

## What changed
- `crates/core/src/pending.rs` (new): each such tap logs a `pending-action` (action, role, since, last refusal, failed) after its decision event; `pending::of` folds it, and a spawn of the issue closes it.
- drive (`act`): every refused build spawn updates the record. HOST_BUSY, MEM_LOW, CAP, ALL_CAPPED, SPAWN_PENDING, SERIAL_BUSY, PAUSED and HOST_OFFLINE keep it pending; anything else ends it as failed. IN_FLIGHT and ALREADY_DONE (another pass on the same spawn, #509) neither wait nor fail.
- `extend`: records the action; a soft refusal of its inline spawn exits 0 ("waiting for host"), with no `extend-failed`. A card that came back can be tapped again; that retries the start without a second round.
- answer (lane gone) and design approve/revise record a pending start too.
- `/api/v1`: `Queued.pending` + `Queued.status`, `IssueView.pending`; the needs cards hide a pending start's card and bring one back (refused, or 30 min) naming the reason and the wait. Done in `serve/v1.rs`; `serve.rs` and web/ are untouched (#552).
- `herdsman status` / `waiting` show the same line.
- Run cards: untouched.

## Decisions
- "Started" means a `spawn` of the issue logged after the record, or after the log seq the answer read (a decision's kick can spawn before the record lands). Timestamps were rejected: a design lane and its approve log in the same second in tests.
- A hard spawn error still gets drive's existing backoff retry (#724); if that later starts the lane, the returned card goes away.
- The waiting line is shown on display surfaces only (status, waiting, API), not in the scan drive acts on, so drive's respawn wording keeps working.

## Evidence @ 393a33ef5f9841e3358243a5a6df31789d94c373
- `cargo test -p herdsman --test pending_actions --test drive --test needs_human --test design_stage --test api_v1 --test api_v1_contract` → exit 0
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- `cargo test -p herdsman-server --features ts export_bindings` (the `just types` recipe; `just` is not installed here) → exit 0, api.d.ts committed
- `cargo test --workspace --no-fail-fast -- --skip webkit` plus a rerun of the 21 targets a mid-run `cargo clean` cut short → everything passes except the items below
  - `chat_tab::the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload` and `self_update::pause_and_resume_are_logged_and_pause_stops_the_job` fail on base 4489690d too (WebKit layout; the lane's `HERDSMAN_LANE` env recorded as `by`)
  - `cloud`, `hosts`, `outbox` failed once in the full run (stale embedded sha after committing mid-build; outbox is flaky) and pass at head after a clean rebuild (exit 0; outbox 3/3)
- WebKit tests were skipped locally (a `config_card` one hung past 30 min); CI's `ui` job runs them.
- `herdsman cap-check --config ci/cap.toml`: 37595 / 38028

src/ net +203 (no budget stated): a new module for the record and its rules, plus small hooks at each tap and display site.
