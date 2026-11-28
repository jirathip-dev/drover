# Batch merge (#1787)

```toml
[fleet.app]
batch_max = 4        # unset or 1: one PR at a time, today's rule
```

- **Who.** `drover merge` for one PR (the lead) also takes up to `batch_max − 1` other `ready` PRs that pass every
  merge gate at their own head, oldest PASS first. Nothing waits for company: at low load a batch is one PR.
- **Alone.** A PR marked `--serial` or joined by its diff to a configured `[[fleet.<n>.serial]]` group (a migration
  folder is one) never rides and, as lead, takes no riders. Hot-file groups may ride. A lead that is not `ready` and
  a fleet whose integration branch rule is strict merge alone too.
- **Stack.** In the scratch worktree: integration's tip, then each head merged in order. A rider that conflicts is
  left out, still `ready`, with nothing logged; its own merge step later finds the conflict and gets its update
  round. The lead's conflict is today's `NOT_MERGEABLE`.
- **Gate.** The fleet's `premerge` checks run once, on the top. Red on a stack of two or more: once more (green then is
  `flaky`), then a binary search of the prefixes. The first red layer gets today's premerge FAIL and an uncounted fix
  round; the layers under it land, those over it stay `ready`. A tree logged green is never run again.
- **Landing.** Never a push of the stack: each PR lands by its own `gh pr merge --match-head-commit <reviewed head>`
  in stack order, with today's evidence and closing comment. Before each one after the first, integration's tip must be
  the merge before it and GitHub must call the PR mergeable within `batch_settle_secs`; after each, the tip's tree must
  be that layer's. A miss stops the landing (`batch-landed stopped`); the rest stay `ready`.
- **Log.** `batch-planned`, `batch-checked` (one per gate run, with its tree) and `batch-landed` (`landed`, `flaky`,
  `charged` or `stopped`), only for a stack of two or more; each `merged` in one carries `batch`.
- `batch_max = 0`, and `batch_max` over 1 with fast-first landing, are load errors.
