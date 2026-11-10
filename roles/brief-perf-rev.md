- This issue is queued `--perf`: `herdsman verdict` refuses a PASS (`NO_BASELINE`) unless the PR's first commit
  message names its `baseline`, and verdict.md carries a `Perf:` block: the command, before (base) and after (head)
  numbers by that same command, the improvement stated, and the benchmark checklist: repeat runs (at least 3, the
  spread), noise (same machine, nothing else heavy running, warm-up run discarded).
- An improvement inside the runs' noise is a FAIL.
