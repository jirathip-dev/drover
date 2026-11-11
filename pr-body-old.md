Closes #833

## For humans
The orchestrator can now show a design image or screenshot from a lane's worktree, or from the repo checkout, in its thread reply. The file is copied into the thread for you, with no run card per image.
Files from anywhere else are still refused. Copying straight into the orchestrator's own thread folder is now allowed; other threads, the event log and config stay off limits.

## What changed
- `thread reply --file`: a file outside the thread folder that lies under the fleet's worktree folder (`<worktrees>/<repo>/…`) or the fleet's `local` checkout is copied into `<thread folder>/files/<name>` and the reply logs that copy. Any existing file at that name is removed first, so a planted symlink is never written through. Other paths are refused as before.
- Hook: writes to a file *inside* the own thread folder were already allowed (#504). The repro's `cp … <thread folder>/`, with the folder itself as the target, was refused. Now the folder itself is allowed only as the last argument (the destination) of `cp`/`mv`/`install`/`rsync`/`ditto`. `rm -rf <folder>`, other threads' folders and the event log stay refused (tested).

## Repro
Commit 1 (331aa5d) holds only the tests. Both fail at base:
- `cargo test --test chat_tab a_reply_file_from_a_lane_worktree_is_copied_into_the_thread_folder` → "not in the thread's folder … copy it there first"
- `cargo test --test hook a_orchestrators_shell_writes_its_own_thread_folder` → `cp /w/docs/design/a.png /w/docs/design/b.png <state>/orchestrator/t6/` denied

## Tests (head 39148d6b866e1af204b881fdcf7e69b92ded26e8, origin/staging b47fd12 merged in)
- `cargo fmt --check` → 0
- `cargo build --locked` → 0
- `cargo clippy --locked --all-targets -- -D warnings` → 0
- `herdsman cap-check --config ci/cap.toml` → 0 (40999 / 41000)
- `herdsman arch-check` → 0 (0 violations)
- `cargo test --locked --test hook` → 0 (42 passed)
- `cargo test --locked --test chat_tab a_reply_file_from_a_lane` → 0
- Full `cargo test --locked` → 101: the only failure is `the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload` (a WebKit scroll test). It fails the same way at origin/staging b47fd12 on this host (exit 101), so this change did not cause it. All other test binaries pass.

Fix round 4: trimmed the copy-in helper (a one-line refusal message, `map_or_else` instead of a `match`) so the repo fits the line cap on current staging.

src/ net +18 (no budget in the issue): one copy helper plus a destination-folder exception in the hook.

## Why (round 1)

Decided: reply --file copies a file from the fleet's worktree folder or its repo checkout into the thread folder's files/ and logs that copy
Because: "copies it into the thread's folder itself. Paths outside those roots are still refused"
Decided: the hook already allowed writes to a file inside the own folder (#504); the repro's cp into the folder itself was the refused case, now allowed only as a copy's last argument
Because: crates/core/src/hook.rs:284
Rejected: allowing the own folder path for every command — rm -rf on it would delete herdsman's .brief.md and .thread.md
Decided: an existing file at the copy's destination is removed first, so a planted symlink is never written through
Because: crates/core/src/orchestrator.rs:649
Check: at 331aa5d both new tests fail (reply refused, cp into the folder refused); at head they pass, and other threads, the event log and rm -rf on the folder stay refused
Owner: The orchestrator can now show a design image or screenshot from a lane's worktree in a thread reply, with no run card.

<sub>note on d33a771 · git log --notes=why</sub>

## Why (round 3)

Decided: no code change this round; the evidence FAIL was a stale run from a body edit, the next run at b68de16 passed
Because: scripts/evidence-check.sh:10
Decided: merged origin/staging so every check re-runs at one new head, and the body names that head
Because: scripts/evidence-check.sh:3
Rejected: re-running the failed evidence run — a re-run replays the old event body and fails again
Check: at 97ccf0e the evidence, rust and ui checks are green with no older failed evidence run on that head
Owner: The orchestrator can now show a design image or screenshot from a lane's worktree in a thread reply, with no run card.

<sub>note on 97ccf0e · git log --notes=why</sub>

## Why (round 4)

Decided: trimmed copy_in to fit the line cap on current staging: a one-line refusal and map_or_else instead of a match
Because: crates/core/src/orchestrator.rs:625
Rejected: filing a cap ask — trimming 5 lines with no change in behaviour was enough
Decided: merged origin/staging b47fd12 so the cap-check counts against the current integration total
Because: "`cargo build`, `clippy -D warnings` and `cargo test` exit 0, and `ci/cap.toml` holds"
Check: cap-check at 39148d6 reports 40999 / 41000; the reply --file and hook tests still pass
Owner: The orchestrator can now show a design image or screenshot from a lane's worktree in a thread reply, with no run card.

<sub>note on 39148d6 · git log --notes=why</sub>

