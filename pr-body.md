Closes #734

## For humans

Every fleet repo can now describe its architecture in one file (which parts may use which, calls that are banned outside one place, size limits), and CI fails with the exact file and line when a change breaks it. Files and functions that are already too big are listed in a baseline, so only new or growing ones fail.
Issues can be queued as a performance fix (the first commit records a baseline measurement and the review must show before/after numbers) or as a refactor (the existing tests may only move or be renamed); the review's PASS is refused otherwise.

## What changed

- `herdsman arch-check [--config .herdsman/arch.toml] [--write-baseline]` (new `crates/core/src/arch.rs`): plain-text scan of Rust and TypeScript.
  - `[[layer]]` name, `paths`, `mods`, `may_use`: a Rust `a::b` path (and one-line `use a::{b, c}` group) or a TS `import`/`export … from`/`require` (relative ones resolved to a path) into a layer outside `may_use` fails `file:line: layer X uses Y`.
  - `[[ban]]` pattern, paths, except, reason: a match outside the exceptions fails (comment lines skipped).
  - `[[size]]` paths, `max_file_lines`, `max_fn_lines`: an offender fails unless the baseline (`.herdsman/arch-baseline.txt` by default) lists it at that size or more; `--write-baseline` records today's offenders.
  - No config: "nothing to check", exit 0. herdsman's CI runs it; `.github/workflows/arch-check.yml` is the reusable workflow for the other repos. Docs: `docs/arch-check.md`.
- `queue add N --perf`: rev's PASS refused `NO_BASELINE` unless the PR's first commit names a `baseline` and verdict.md has a `Perf:` block.
- `queue add N --refactor`: rev's PASS refused `TESTS_CHANGED` when test files gain or lose lines other than moved ones (`gh pr diff`).
- Briefs carry each playbook's impl/rev steps; `queue ls` shows the mark; the lesson role adds `[[ban]]`/`[[layer]]` entries; usage doc lists the codes.

Decided: `arch-check` is a hidden CI command like `cap-check` (keeps the `help --agent` budget); `queue set` for the playbook mark left out (not in scope). Each repo's own `arch.toml` comes from the conductor-filed issues (scope item 4), so this PR adds none.

src/ net +353 (budget not stated): one new scanner module plus two gate checks; cap total 37083 / 38028 after the #761 raise (q966).

## Evidence (head df6c7a99231facde08e62fcd9bedfd4c91239727)

- `cargo fmt --check`: exit 0
- `cargo clippy --all-targets --locked -- -D warnings`: exit 0
- `herdsman cap-check --config ci/cap.toml`: exit 0, 37083 / 38028
- `cargo test --locked --no-fail-fast`: exit 101, 1243 passed / 1 failed. The failure is `self_update::pause_and_resume_are_logged_and_pause_stops_the_job`, which reads the lane's own `HERDSMAN_LANE` (`crates/core/src/update.rs:26`). It is not touched by this PR and CI doesn't set that variable.
- `env -u HERDSMAN_LANE cargo test --locked --test self_update`: exit 0 (9 passed)
- arch_check (4), playbooks (3) and spawn tests pass in the full run above.

Fix round 1: merged origin/staging twice (#730's architect role, then #761's cap raise). Conflicts in `cli.rs` (help budget 344 + 2 = 346), `queue.rs` (`architect` and `playbook` both kept in `queue ls` JSON) and `roles/usage-tail.md` (both lines kept).
