Closes #1080

## For humans
When the owner types an approval on a design or architect ask ("approve", "approved", "ok", optionally with the issue's number, like "#213 approve"), drover now records it as an approval and starts the implementation, instead of sending the design lane a pointless "revise".
A line that names a different issue number is still treated as a revise note, so it can't approve the wrong design.

## What changed
- `design::is_approval`: an answer on a design/architect ask whose words are `approve`/`approved`/`ok` (case-insensitive, trailing `.`/`!` dropped), with at most one `#N` equal to the ask's issue.
- `chosen` (shared by `answer`, `answer --typed` and orchestrator answers) maps such a line to `approve`, so `design::decide` logs `design-approved` and the `decision` event's choice is `approve`.
- Regression test (commit 1, `da2306991`): the q1305 line `#213 approve` on #213's ask, plus `approved`, `OK`, `#213 ok`; `#214 approve` on #213's ask stays `design-revise`. The existing free-text test (`make it blue` → revise) still passes.

## Repro
Commit 1 `da2306991` (test only): `cargo test -p drover --test typed_choice` → exit 101 (`#213 approve is no revise`). Head: exit 0.

## Evidence at head 1a750308fa607d27cff63d93e96575448c024f48 (origin/staging merged in again)
- `cargo fmt --check` → 0
- `cargo clippy --workspace --all-targets -- -D warnings` → 0
- `cargo test -p drover --test typed_choice --test drive_kicks` → 0
- Round-1 CI red, `drive_kicks::failing_retries_never_starve_another_fleets_start`: this diff doesn't touch it (it's about drive spawn retries). Rerun alone 4 times locally → 0 each time.
- `env -u DROVER_LANE cargo test --workspace --no-fail-fast` → 101. Under full-suite load, `chat_tab`, `outbox` and `shed` each had one timing failure. Rerun alone, `env -u DROVER_LANE cargo test -p drover --test chat_tab --test outbox --test shed` → 0. An earlier `cloud`/`hosts` failure came from a stale compiled-in sha in the local build; after a rebuild, `cargo test -p drover --test cloud --test hosts` → 0. `DROVER_LANE` is unset because the lane's value leaks into `self_update`'s `by` assertion.
- `pr.md` (a PR-body draft committed by an earlier wip snapshot) is removed from the tree.

src/ net +12 (budget: none given): one predicate plus the call that uses it.
