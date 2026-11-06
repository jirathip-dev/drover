- This issue is queued `--repro`: a PASS verdict.md carries one of these blocks, else `herdsman verdict` refuses it
  (`NO_REPRO`):
  - `Repro: run`, then the test command fenced and its failing output fenced, run on the PR base plus the branch's
    commit 1 (checked out and built under `$HERDSMAN_SCRATCH_TARGET`); then the same command fenced and its passing
    output fenced, at this head.
  - `Repro: verify`, only when the PR body has a `## Repro` section: quote its reason on a `>` line.
- A repro test that passes on the base plus commit 1, or a fix that changes the test without a spec change, is a FAIL.
