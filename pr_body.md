Closes #821

## For humans
- Approving a run card now answers "running…" in the thread at once instead of waiting.
- The command runs on its own on the main host for up to the card's timeout; the thread then gets its real exit code and last output lines.
- A command that runs past its timeout is stopped and reported as a timeout, with its last output.
- Restarting herdsman or the board while a card runs no longer loses its result.

## What changed
- Root cause of `exit 124 … no answer in 60s`: shed's runner (`[runtime] kind = "shed"`) did not forward `run_for`, so the card's `--timeout` fell back to the 60 s default. It now forwards it (`crates/core/src/shed.rs`).
- `card approve` logs `run-settled`, posts `run rN approved by owner, running in <dir>…` and detaches a hidden `herdsman card run rN` (own session) on the main host.
- `card run` (once per card: `run-started` / `run-done` in the event log) runs the exact command in its folder with its output in `run-<id>.out` beside the state, so a run killed at `--timeout` still reports its tail; it logs `run-done` and posts `exit N` with the masked tail. Past the timeout it reports `exit 124 … killed after Ns (its --timeout)` and kills its own process group.
- The lane hook refuses `card run` from lanes, like approve/deny.
- The existing approve tests now approve, then run (the spec moved the run out of approve).

src/ net +57 (budget: none stated) — a detached runner entry point plus the shed runner fix.

## Repro
Commit 1 (`63246559`) holds only the regression tests. `cargo test -p herdsman --test run_card` at commit 1 exits 101: approve answers `run r1 approved by owner, ran in /srv/app: exit 124 / no answer in 60s` for `sleep 90` with `--timeout 300`.

## Evidence
All run at head `615212b40e8e71df4a2f63f682f71bbfb2b0d86d` (615212b: e644116 + a clean merge of origin/staging 8c97451), macOS host.

- `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0
- `cargo test --locked -p herdsman --test run_card` (the issue's test) → exit 0, 8 passed
- `cargo test --workspace --locked --no-fail-fast` → exit 101, 168 test binaries ok; 4 failed, none from this PR (this PR touches no UI code and none of these files):
  - `chat_tab` `the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload`: fails the same way on plain staging 8c97451 (`chat_tab.rs:1602`)
  - `config_card` `the_card_lands_live_in_view_and_stays_reachable_in_webkit`: hangs in WebKit on plain staging 8c97451 too (and in every other lane on this host); killed
  - `self_update` `pause_and_resume_are_logged_and_pause_stops_the_job`: fails the same way on plain staging 8c97451; it reads the lane's own identity (`herdsman:821:impl`) from the environment instead of `owner`
  - `live_board` (2 WebKit tests): host WebKit trouble, like the above; not rerun on staging

