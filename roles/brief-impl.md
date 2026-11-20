- Work on branch `{branch}` only. Commit, push it, open a PR to `{integration}` whose body says `Closes #{issue}`.
- Never merge, never push to `{integration}` or main — merging happens only via `drover merge`.
- Report: what changed, exact test commands + exit codes, head sha.
- A second build or checkout (base, staging, head) goes under `$DROVER_SCRATCH_TARGET` only (builds with
  `CARGO_TARGET_DIR=$DROVER_SCRATCH_TARGET/target`, checkouts in `$DROVER_SCRATCH_TARGET/<name>`), never `/tmp`
  or `$TMPDIR`; it is deleted when your lane ends.
- Logs and downloads (test, lint, CI output) go in your scratch folder `$DROVER_SCRATCH_TARGET`, never the
  worktree.
- #1362: `drover turn done` is a hard gate, run in your worktree at the pushed head: it refuses `DONE_CHECK` while
  `HEAD` is not the PR head ("push first") or a done check fails (the fleet's `done_checks`, else `drover arch-check`
  and `cargo fmt --check` where the repo has them), and `PR_BODY` until the PR body names your pushed head sha, has
  `## For humans` and no "Generated with" line (`gh pr edit --body` first). A refusal ends nothing and starts no
  review: fix what it names, push, then `drover turn done` again.
- #735: a non-obvious reason goes in the PR body, never in a comment: arch-check fails a new comment line but
  `// SAFETY:` over `unsafe` and one `///` line on a public item.
