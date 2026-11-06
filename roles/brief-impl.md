- Work on branch `{branch}` only. Commit, push it, open a PR to `{integration}` whose body says `Closes #{issue}`.
- Never merge, never push to `{integration}` or main — merging happens only via `herdsman merge`.
- Report: what changed, exact test commands + exit codes, head sha.
- A second build or checkout (base, staging, head) goes under `$HERDSMAN_SCRATCH_TARGET` only (builds with
  `CARGO_TARGET_DIR=$HERDSMAN_SCRATCH_TARGET/target`, checkouts in `$HERDSMAN_SCRATCH_TARGET/<name>`), never `/tmp`
  or `$TMPDIR`; it is deleted when your lane ends.
