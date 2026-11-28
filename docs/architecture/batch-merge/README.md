# batch-merge: stack the ready PRs, gate once, bisect on red (#1787)

## Summary

Decision: with `batch_max` over 1, `drover merge` stacks its PR and up to `batch_max − 1` other ready PRs on
integration, runs the premerge gate once on the top, then merges each PR by today's own pinned `gh pr merge`, back
to back. Red: one retry, then a bisect; the first bad PR gets today's uncounted fix round, the PRs under it land.
Slices: one merge path (a stack of one); pick, stack and land; retry, bisect and charge; numbers, docs, rollout.
Open questions: land as N native merges in a row, not one push (one push needs a branch-protection bypass)? May
PRs that share only a hot file ride in one batch?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how `gate::merge` and the tests read after the change, the config line, what the log shows |
| `adr.md` | the question, the options, the decision on each of the issue's eight points, consequences, slices, open questions, the `.drover/arch.toml` lines |
| `shapes.md` | the types, the config key, the three events, the one new `[limits]` key |
| `boundaries.md` | which module may call which, and the batch's statechart |

## The design on one screen

```
 ready PRs (PASS at head, CI green)          scratch worktree (never pushed)            GitHub
 ─────────────────────────────────           ───────────────────────────────            ──────
 #12  ← `drover merge` was called for it     integration tip
 #15  oldest PASS                              + merge #12 head   ─ tree T1
 #19                                           + merge #15 head   ─ tree T2
 #21  conflicts with #15 ─ drops out, stays    + merge #19 head   ─ tree T3  ◀─ premerge gate, once
 #30  serial group ─ never picked                                              │
                                                                     green     ▼
                                              gh pr merge #12 --match-head-commit <its reviewed head>
                                              gh pr merge #15 …   each: tip is the last merge, tree is T(k)
                                              gh pr merge #19 …   each: `merged`, closing comment, follow-ups
```

Nothing waits for company: a batch is whatever is ready when one PR's merge step takes the fleet's merge lock.
At low load that is one PR, today's path exactly. Under load the next batch fills while the current one checks.

## Eight ready PRs, a 10-minute gate

| | Today | `batch_max = 8` |
|---|---|---|
| Premerge runs, all green | 8 | 1 |
| Premerge runs, one culprit | 8 | 5 or 6: 1 red, 1 retry, 3 bisect, 1 for the PRs after the culprit |
| Time under the merge lock, all green | 80 min | 10 min |
| Catch-up pushes in an `update_ready = "always"` fleet (each a CI run) | up to 28 | 0 |
| Pushes to integration | 8 | 8, back to back |
| Conflict update rounds, review rounds | as they are | as they are |
| Each PR's verdict, merge commit, closing comment | pinned to its head | the same |

What batching does not save: a PR that truly conflicts still needs its update round, and each PR still lands as
its own push. Whether eight pushes are eight CI runs is the repo's CI's choice: the template's one group per
branch runs the first and the last.

## What the owner decides

1. **N native merges in a row, not one push.** The issue sketches "one push of the stack, the PRs closed as merged".
   Integration requires a PR and the `drover/review` check, so a push of the stack is refused unless drover's
   token may bypass protection, and squashed PRs would show Closed, not Merged. Recommended: keep protection
   whole and land by each PR's own merge.
2. **Hot-file groups.** The issue says a PR in a serial group never batches. Each hot file is a one-slot serial
   group too, and in this repo most PRs touch one. Recommended: only configured `[[serial]]` groups and
   `--serial` marks go alone; two PRs on one hot file either stack clean (and are tested together) or one drops
   out.

Both are argued in `adr.md`.
