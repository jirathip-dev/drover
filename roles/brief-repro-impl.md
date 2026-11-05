- This issue is queued `--repro`: commit 1 holds only the regression test, which fails on the current code. Its
  commit message names the failing command.
- Later commits fix the bug. The test changes only if the spec changes.
- No cheap test path (for example a pure visual bug): skip commit 1 and give the PR body a `## Repro` section saying
  why. The verify stage then runs for this issue and must show the bug before and after.
