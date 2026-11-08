Closes #640

## For humans
A "you've hit your limit" banner no longer takes an account out of rotation when its usage meter says it still has room (for example an Ultra plan with no 5-hour window and a week at 23%). The banner now only counts when it is Claude Code's own message, not text a lane quoted or a tool printed.
"Signed out" is now its own state: you get a card telling you to sign in or replace the token, never a fake "capped until" time. The accounts list shows why each account is in its state.

## What changed
- `access::confirm`: when the cap pattern matches, check the account's latest usage reading (status-line `usage.jsonl` row, else the `[accounts.usage]` meter). The cap holds only if (a) no reading is fresher than 10 min, or (b) a window the plan has is at 100% or more (`until` = the later of the window's reset and the banner's). A null window never counts. Otherwise `cap-banner-ignored` is logged with `5h —, 7d 23%` and the account stays in rotation. A 401 reading (`login_needed`) gives `account-signed-out`.
- **Banner matching rule** (`harness/claude.rs` `own_lines`): the pattern is matched only against the pane's **last transcript block**, and only when that block is not headed by a `⏺`/`•` reply or tool call. Prompt lines (`❯`, `>`) are dropped. So a lane quoting the pattern, a tool call or its `⎿` output, or a typed prompt never caps. The banner Claude Code prints after a failed turn (`  ⎿  You've hit your limit · resets …` under a non-`⏺` head) still does. The sign-out pattern uses the same rule.
- `account-signed-out`: a new event (webpush owner card, passive). It has no `until` and holds until `herdsman accounts reset <name>`. `check` and the fallback `switch` never log a sign-out as a cap, and never log a cap as signed out. An account-less harness still rests via `agent-unavailable` (#302).
- `herdsman accounts` state column: `ok: 5h —, 7d 23%`, `capped until Nov 7 20:06: banner 19:40 + 5h —, 7d 100%`, `signed out: usage 401 at 18:02` / `signed out: sign-in prompt at 18:02`.
- Spawn's carry-over and `waiting` treat a sign-out as a rest (`access::rests`). Docs: `docs/lanes.md`, `fleet.example.toml`.
- `tests/fallback.rs`: updated for spec item 3 (a sign-out is `account-signed-out` with no reset time; the no-chain default now reports it).

src/ net +66 (no budget stated in the issue): the meter check, the new state and the evidence line.

## Repro
Commit 1 (`a721401`) holds only the regression tests in `crates/cli/tests/access.rs`. They fail on the base: `cargo test -p herdsman --test access`. AC2 is `a_banner_the_meter_contradicts_does_not_cap_and_a_lane_still_picks_the_account`, AC3 is `a_banner_with_the_week_at_100_caps_until_the_reset`, AC4 is `signed_out_is_its_own_state_never_a_cap` + `a_banner_with_a_401_usage_reading_is_signed_out` (+ the week-at-100 test asserts no sign-out). Plus `a_banner_with_no_fresh_reading_still_caps` and `the_pattern_in_a_reply_or_tool_output_is_no_banner`.

## Evidence (head 662e4a51966429a0faf3dc13b9f9ccc89a17ae5b)
All runs used `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target`:
- `cargo fmt --check` → 0
- `cargo build --release` → 0
- `cargo clippy --all-targets -- -D warnings` → 0
- `herdsman cap-check --config ci/cap.toml` → 0 (31559 / 35500)
- `cargo test -p herdsman --test access --test fallback` → 0 (16 + 9 passed)
- `cargo test -p herdsman-core` → 0
- Full `cargo test`: this host's scratch sweep kept deleting the lane's target dir partway through a single run (binaries "never executed" / SIGKILL), so the suite was run in chunks. The pieces: everything through `chat_tab` (19 result lines, all ok), then every later test file in chunks of 12 and 5. Every chunk exit 101 was a deleted binary, and those files were re-run until they passed. Two real failures, both environmental, passed alone: `live` (SSE timing, 14/14 on re-run, exit 0) and `self_update` (it reads the lane's `HERDSMAN_LANE` env; 8/8 with it unset, exit 0). CI runs the whole suite in one go.
