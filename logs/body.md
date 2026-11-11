Closes #824

## For humans
When drive tries to catch a passed pull request up with staging and is refused, the status and the board now say why it is held (for example "held: HOST_BUSY …" or "held: LANE_LIVE …") instead of promising it will catch up next pass.
A passed pull request whose own lane is idle or hibernated already gets its catch-up round in one pass; a new test pins that.

## What changed
- `crates/core/src/waiting.rs`: a conflicting/behind PASS whose last update spawn drive tried was refused shows `PASS · conflicts with staging · held: <CODE> <why>`; the "catching up next pass (≤ 5 min)" promise is dropped (before, the held reason was appended after the promise).
- `crates/cli/tests/drive.rs`: two tests.
  - `a_conflicting_pass_with_an_idle_impl_lane_gets_its_update_in_one_pass`: ready PASS, CONFLICTING PR, `blocked NOT_MERGEABLE`, impl agent `hibernated` → one drive pass logs `conflict` and the state is `fix(1)+u1` (AC1, AC3).
  - `a_refused_update_round_shows_its_reason_not_catching_up`: the update spawn refused HOST_BUSY → `waiting` shows `held: HOST_BUSY`, never "catching up next pass" (AC2).

## Decided
- No drive change for AC1: drive's update spawn already passes `--supersede` (`crates/core/src/drive.rs:1579`, and the waiting next_cmd), so the issue's own idle/hibernated lane is superseded with its wip saved. The hibernated-lane test passes on base as well; it pins the behaviour.
- The cooee #139/#142 stall (2026-11-11 07:20–08:01): the event log has no LANE_LIVE refusal for either. Only fast passes (12–15 min each, which never spawn update rounds) and kicks for other issues ran for cooee in that window; no full pass ran to start the update. That starvation is outside this issue and worth its own issue.

## Repro
Commit 1 (`dd329870`) holds only the tests: `cargo test -p herdsman --test drive -- a_refused_update_round a_conflicting_pass_with` → exit 101 (`a_refused_update_round_shows_its_reason_not_catching_up` fails; the hibernated-lane one passes on base).

## Evidence @ d26c29ec903abbf3e4567931e6159aa0f1760ad6
- `cargo test -p herdsman --test drive` → exit 0 (81 passed)
- `cargo test --locked -p herdsman-core` → exit 0
- `cargo test --locked -p herdsman --no-fail-fast --test drive --test drive_kicks --test waiting --test blocked_respawn --test serial_groups --test merge --test needs_human --test stalled_lanes --test orphan_issue --test pending_actions --test gate_unit --test live_board --test repro --test turn --test land --test overlap` → exit 101: 15 suites ok; the one failure is `live_board_chat_and_ascii_in_webkit`, which also fails on base on this host (below)
- `cargo fmt --check` → 0 · `cargo clippy --all-targets --locked -- -D warnings` → 0
- `herdsman arch-check --base origin/staging` → 0 · `herdsman cap-check --config ci/cap.toml` → 0 (39369 / 41000)
- Not verified locally: full `cargo test --locked` timed out after 30 min in the WebKit suites. `chat_tab::the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload` and `live_board::live_board_chat_and_ascii_in_webkit` fail on this host on base source too (exit 101 both), so they are not this change; CI is the judge.

src/ net +4 (budget: none stated) — one constant and the suffix swap.
