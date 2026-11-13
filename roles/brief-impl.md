- Work on branch `{branch}` only. Commit, push it, open a PR to `{integration}` whose body says `Closes #{issue}`.
- Never merge, never push to `{integration}` or main — merging happens only via `drover merge`.
- Report: what changed, exact test commands + exit codes, head sha.
- A second build or checkout (base, staging, head) goes under `$DROVER_SCRATCH_TARGET` only (builds with
  `CARGO_TARGET_DIR=$DROVER_SCRATCH_TARGET/target`, checkouts in `$DROVER_SCRATCH_TARGET/<name>`), never `/tmp`
  or `$TMPDIR`; it is deleted when your lane ends.
- #677: before `drover turn done`, write `why.md` in the worktree root (never commit it: drover puts it on your
  pushed head as a git note and copies it into the PR body). At most 15 `Key: text` lines, `Owner:` once and last:
  `Decided: …`, `Because: "spec quote" | q<id> | file:line`, `Rejected: alternative — why`,
  `Check: what the reviewer should verify`, `Owner: one plain line`. Every `Because:` cites a verbatim spec quote
  (12+ chars), an answered ask or a `file:line` at your head; no secrets. A bad note is refused `WHY_INVALID`.
- #925: `turn done` refuses `PR_BODY` until the PR body names your pushed head sha, has `## For humans` and no
  "Generated with" line: `gh pr edit --body` first.
- #735: a non-obvious reason goes in that why note (`drover why`), never in a comment: arch-check fails a new
  comment line but `// SAFETY:` over `unsafe` and one `///` line on a public item.
