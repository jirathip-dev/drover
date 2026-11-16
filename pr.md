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

## Evidence at head 77511eb51253d0f325b68a432f1e0704105b85ab (origin/staging merged in after #973)
- `cargo fmt --check` → 0
- `cargo clippy --workspace --all-targets -- -D warnings` → 0
- `cargo test -p drover --test typed_choice` → 0
- `env -u DROVER_LANE cargo test --workspace --no-fail-fast`: no failures except `drive::drive_now_on_the_board_is_paired_only_and_toasts_its_line`, a timing flake (it read `0.1 s` under load where it expects `0 s`); rerun alone, `cargo test -p drover --test drive drive_now_on_the_board` → 0. `DROVER_LANE` is unset because the lane's value leaks into `self_update`'s `by` assertion.

src/ net +12 (budget: none given): one predicate plus the call that uses it.
