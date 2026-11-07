Closes #511

## For humans
Implementer lanes now say how each turn ended: done, blocked, needs input, or still working. Herdsman acts on that right away instead of guessing.
A lane that says it is done gets its review at once. If it says done but pushed nothing, it gets one nudge and then comes to you. A blocked lane brings you a card with its reason. A lane that stopped mid-task gets one "continue" prompt and is never restarted.
The waiting and status lists show the reported state and how long ago it was reported, instead of "idle without pushing (check the pane)".

## What changed
- **New verb** `herdsman turn done|blocked|need-input|working [--why <text>]` (`blocked` requires `--why`). It is run by impl-family lanes for their own issue: fleet, issue and role come from `HERDSMAN_LANE`, and check, design and conductor lanes are refused. It logs a passive `turn-end`, which does not move the issue state, and kicks the issue. It is forwarded like the other lane verbs (outbox or service), and `foreign` accepts it as the lane's own.
- **Stop hook** (`hook::turn_end`): when an impl lane's Claude `Stop` comes with no `herdsman turn` since its last `UserPromptSubmit`, the hook answers once with `{"decision":"block","reason": TURN_ASK}`. A per-lane marker file on the lane's own host (`prompt` / `turn` / `asked`), plus Claude's `stop_hook_active`, means it never loops. Lesson lanes are excluded. Dialects get `prompt` and `again` fields; Hermes has no way to make a turn go on, so it has `again: None`.
- **Drive acts on the state.** In `waiting::scan`, an idle build lane whose pane is alive and which has a `turn-end` since its spawn (or since an owner extension) gets a typed row:
  - `done` with a push at the head: `handoff` treats a `done` turn as idle at once (no 2-minute wait, no CI wait), so review spawns this pass.
  - `done` with nothing new pushed: one nudge prompt (`turn-prompted`). A second `done`, or 10 minutes without an answer, leads to `needs-human` with code `LANE_BLOCKED`.
  - `blocked`: `needs-human` with code `LANE_BLOCKED` and the lane's reason (the owner card).
  - `working`: after `stall_minutes`, one "continue" prompt per turn end. Never a respawn.
  - `need-input`: an open ask waits on the owner as before; without one, the row says so.
  - No state and no pane: #444's respawn path, unchanged.
- **Status texts**: `waiting` and `status` show `reported <state> (<why>) …`, aged from the turn end.
- **Docs**: a line in `roles/impl.md` (the brief goldens are re-blessed) and a turn-end line in `help --agent`. `HELP_AGENT_MAX` gets +5 for the command entry and that line (317 → 322 after merging staging).
- The lesson prompt code moves into a shared `tell()` helper.

src/ net +180 (the issue states no budget): a new verb, a stop hook, drive actions for four states, and status texts. Cap: 29119 / 31500.

## Evidence (head c7084ac79b15ee1d3c9b3e610d06c2ac82848f47)
Update round 2: merged `origin/staging` (#510, #505, #570) in. The only conflict was `crates/core/src/cli.rs` `HELP_AGENT_MAX`. Staging is at 317 (+4 for #510's `lesson apply`, +4 for #505's `queue unblock`) and this branch adds +5 for #511's `turn`, which makes 322. All gates were run at this head.
- `cargo fmt --all --check` → exit 0
- `cargo build --release --locked` → exit 0
- `cargo clippy --release --locked --workspace --all-targets -- -D warnings` → exit 0
- `herdsman cap-check --config ci/cap.toml` → exit 0: 29119 / cap 31500
- `cargo test --release --locked --workspace --no-fail-fast` → exit 0 (133 test results, 0 failed)
- The first fail-fast run exited 101 on `chat_tab::a_message_with_three_images_delivers_all_three_and_eleven_are_refused`. That test waits on a sleep loop and this branch does not touch it. It passed on a rerun of `-p herdsman --test chat_tab` (exit 0) and again in the full run above.
