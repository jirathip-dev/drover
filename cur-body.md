Closes #1361

Builds to the guard hook contract in jirathip-dev/blocks#79's ADR (merged as jirathip-dev/blocks#121): `docs/architecture/guard/hook.md` and `shapes.md` at 289eaf9.

## For humans

When an agent finishing work on screens says it is done, drover now first runs the app's own screen check.
If the check finds new problems (for example a screen that overflows on a phone), the agent is told each one with its fix and must deal with them before its turn ends.
Repos that have not set the check up, and changes that touch no screen files, behave exactly as before; a check that hangs or is broken never holds an agent up.

## What changed

- `crates/core/src/guard.rs` (new): on `turn done` of an impl/fix/update lane, if the worktree has `.blocks/guard.json` and the PR's files touch `ui_paths`, `.blocks/` or a `screens` folder, it runs `just guard` (the contract's recommended recipe) in the lane's worktree on the lane's host, with `INTEGRATION` and `GUARD_OUT` set; the `Outcome` comes back on stdout so a remote host needs no second read.
- New findings: `REFUSED UI_CHECK` with each finding as `<rule> <file>[:line] <at>: <message>` + `  fix: <fix>` (first 20, then a count), no `turn-end`, a passive `ui-check` event with the count and seconds taken.
- A second `done` at the same findings (same `rule file at` keys since the lane's spawn) ends the turn and goes to review; rev's brief carries the findings (contract's recommendation).
- Clean / baseline only: `turn-end` as today with `ui_check: {result: clean, known: N, secs}`. Exit 124 (timeout) or anything else unparseable (exit 2, can't start): logged `timeout`/`broken`, the turn ends.
- `[limits] guard_secs = 180` (the contract: "180 s is safe"); `verify::ui` is the one `ui_paths` test, shared with the verify stage.
- `UI_CHECK` in `roles/usage/30-refusals/` and the issue-states paragraph, so `drover help --agent` lists it.

## Decisions

- The command is `just guard` (the ADR: "recommended: a `guard` recipe in its justfile, so the hook and CI share it"); no fleet config key for it.
- Finding lines add `:line` after the file when the finding has one (the issue asks for rule, file, line, what).
- src/ net +184 (budget: none stated): one new module for the hook, its limit key and two call sites.

## Evidence (head ee9a87e52f83a0ee9382511e918cac411ef966c9)

Round 3: staging build fix. CI `rust` arch-check was red because staging's #1380 grew `crates/cli/tests/shed.rs` to 2560 lines, over its 2557 baseline. Trimmed that test (a duplicate assert, a shorter call) to 2556 lines; no other change. Merged `origin/staging` again. At head ee9a87e52f83a0ee9382511e918cac411ef966c9:

- `cargo test --locked -p drover --test shed an_idle_impl_agent_holding`: exit 0
- `cargo fmt --check && cargo test --locked -p drover --test why_notes`: exit 0 (14 passed)
- `drover arch-check --base origin/staging`: exit 1, its only violation `just gen` exited 127 (no `just` on this lane host); the shed.rs size violation is gone. CI runs it.

### Round 2 evidence (head a885ed17c5e8eded82981a1b12e5acfa64ad9302)

Round 2 (fix for CI red at d8f4ca1: `test (shard 3/4)` failed `setup_neutral::shipped_files_name_no_install`, since a `///` line in `crates/core/src/guard.rs` named the org; it now says `blocks#79`). Merged `origin/staging`. All at head a885ed17c5e8eded82981a1b12e5acfa64ad9302:

- `cargo test -p drover --test why_notes --test verify_stage --test why_briefs --test brief_golden --test repro --test setup_neutral`: exit 0 (why_notes 14 passed, 5 of them new for this issue; setup_neutral 2 passed)
- CI's two `cargo clippy` commands: exit 0; `cargo fmt --check`: exit 0
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0 (55767 / 60000)

Round 1 (at d8f4ca1, unchanged since): `cargo test --locked -p drover --test thread_handoff --test m3 --test access --test limits --test usage --test readme --test spawn --test diagram --test stalled_lanes` exit 0; `cargo test --locked -p drover-core --lib` exit 0; `drover arch-check` locally exit 1, and its only violation is `just gen` exiting 127 (`just` is not installed on this lane host). CI runs arch-check. The why_notes tests run the real `sh` script with a fake `just` on PATH; the timeout case is the runner's 124.

## Why (round 1)

Decided: run `just guard` in the lane's worktree on its host before turn-end; the Outcome comes back on stdout
Because: crates/core/src/guard.rs:17
Rejected: a fleet config key naming the command — the ADR names a guard recipe in the app's justfile, shared with CI
Decided: new findings refuse UI_CHECK with no turn-end; the same findings a second time go to review
Because: "A second `done` with the same findings follows what the contract says"
Because: crates/core/src/guard.rs:121
Decided: a timeout (guard_secs, 180 s) or an unparseable outcome logs ui-check and ends the turn
Because: "a check that times out or cannot start does not block the turn"
Check: no turn-end on a refusal, and the fake just runs only when guard.json exists and ui_paths are touched
Owner: agents on screen work now get the app's own screen check before their turn ends; nothing changes elsewhere

## Why (round 2)

Decided: run `just guard` in the lane's worktree on its host before turn-end; the Outcome comes back on stdout
Because: crates/core/src/guard.rs:17
Rejected: a fleet config key naming the command — the ADR names a guard recipe in the app's justfile, shared with CI
Decided: new findings refuse UI_CHECK with no turn-end; the same findings a second time go to review
Because: "A second `done` with the same findings follows what the contract says"
Decided: a timeout (guard_secs, 180 s) or an unparseable outcome logs ui-check and ends the turn
Because: "a check that times out or cannot start does not block the turn"
Decided: round 2 drops the org name from the guard.rs doc line; setup_neutral bans setup-specific names in shipped files
Because: crates/cli/tests/setup_neutral.rs:119
Check: no turn-end on a refusal, and the fake just runs only when guard.json exists and ui_paths are touched
Owner: agents on screen work now get the app's own screen check before their turn ends; nothing changes elsewhere

<sub>note on a885ed1 · git log --notes=why</sub>

