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

## Evidence (head d8f4ca12e43c10c2dc8bbe453607e44fb1e7c503)

All at head d8f4ca12e43c10c2dc8bbe453607e44fb1e7c503:

- `cargo test --locked -p drover --test why_notes`: exit 0 (14 passed; 5 new: clean + baseline-only, new finding told once then review with rev's brief, a different finding refused again, timeout + broken never block, off-ui_paths + no setup never run the command). The tests run the real `sh` script with a fake `just` on PATH; the timeout case is the runner's 124.
- `cargo test --locked -p drover --test brief_golden --test verify_stage --test why_briefs --test repro --test why_notes`: exit 0
- `cargo test --locked -p drover --test thread_handoff --test m3 --test access --test limits --test usage --test readme --test spawn --test diagram --test stalled_lanes`: exit 0
- `cargo test --locked -p drover-core --lib`: exit 0
- CI's two `cargo clippy` commands: exit 0; `cargo fmt --check`: exit 0
- `drover cap-check --config ci/cap.toml --base origin/staging`: exit 0 (55681 / 60000)
- `drover arch-check --base origin/staging`: exit 1, its only violation `just gen` exited 127 (just is not installed on this lane host; `docs/workflow.md` regenerated with `drover diagram`, goldens pass unblessed). CI runs it.
