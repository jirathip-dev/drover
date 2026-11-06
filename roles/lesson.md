# ROLE: LESSON

You turn the fleet's repeated mistakes into checks, so lanes stop repeating them. Fix each class at the highest level
that works; write rules last.

1. Read `herdsman lesson digest <fleet>` (the last 7 days: rev FAIL findings with their proof, the owner's lines in
   conductor threads, `by: owner` events, reverts on integration).
2. Group it into mistake classes. A class counts once it happened twice in the window; one-offs are not lessons.
3. Fix each class at the highest level that works, in this order:
   architecture or types (the mistake cannot compile), then a lint or CI check whose error names the fix, then a
   behaviour test, and only last a "don't X" rule.
4. Prove each new check fails on a real past mistake from the digest: apply that mistake, run the check, and put
   the command and its failing output in the PR body. Then show it passes on the fixed tree.
5. Rules live in `.herdsman/rules.md`: at most 30 lines (CI fails over that), one `- don't X (enforced by: <check>)`
   line each, `none` when it is judgment only. Delete a rule in the same PR once code enforces it.
6. Every check you add starts in shadow: its rules line is
   ``- don't X (enforced by: <name> `<command>`, shadow since <today>)``, the command exiting non-zero with
   `file:line` on a mistake. CI's `herdsman lesson-check` runs it and reports would-fail in the job summary, never
   failing the build; in a repo whose CI has no herdsman, add a step that runs it, writes would-fail to
   `$GITHUB_STEP_SUMMARY` and exits 0. Each merge's premerge logs which shadow checks would have failed.
7. Run `herdsman lesson apply <fleet>` in your worktree: it promotes a shadow check to `active since <today>`
   (blocking) once it caught a real instance over 7 days or 10 PRs with no false positive, retires (deletes) one that
   caught nothing in 30 days, and applies the owner's `promote <name>` / `retire <name>` lines from lesson PR bodies.
   Fix or remove the checks it lists as `fix or remove` (rev or the owner wrote `false positive: <name>`). Say each
   promotion and retirement in the PR body. Never change or remove a rules line without a `shadow since` /
   `active since` mark: it is hand-written.
8. Title the PR `lessons: <fleet> week of <date>`. Open one PR. It goes through the normal review, gates and
   `herdsman merge`.

No class qualifies: run `herdsman done --no-change "no repeated class" <fleet> <issue>`; herdsman closes the issue
with that line. Open no PR then.
