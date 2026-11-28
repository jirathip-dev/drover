# ADR: batch-merge — stack the ready PRs, gate once, bisect on red (#1787)

## Summary

Decision: with `batch_max` over 1, `drover merge` stacks its PR and up to `batch_max − 1` other ready PRs on
integration, runs the premerge gate once on the top, then merges each PR by today's own pinned `gh pr merge`, back
to back. Red: one retry, then a bisect; the first bad PR gets today's uncounted fix round, the PRs under it land.
Slices: one merge path (a stack of one); pick, stack and land; retry, bisect and charge; numbers, docs, rollout.
Open questions: land as N native merges in a row, not one push (one push needs a branch-protection bypass)? May
PRs that share only a hot file ride in one batch?

## Status

Proposed (architect lane, 2026-11-28, staging `41059a8ae`). Read `usage.md` first, then this, `shapes.md`,
`boundaries.md`.

## Question

Eight PRs are ready. Each merge runs the premerge gate alone under the fleet's one merge lock, so the eighth
waits for seven gate runs. How do several ready PRs land on one gate run, with a red run still charged to the
one PR that caused it, each review still pinned to its head, and branch protection untouched?

## Where it stands (read on `41059a8ae`)

- `gate::merge` takes `merge-<fleet>.lock`: one merge at a time. It merges the head onto integration's tip in
  the scratch worktree `premerge-<fleet>`, runs the fleet's `premerge` checks, and re-reads the tip (moved
  twice: `STAGING_MOVED`). Red has no retry: `premerge_failed` writes a drive FAIL at the head
  (`STAGING_WOULD_BREAK` or the check's own code), an uncounted fix round.
- Those checks already go through `Heavy::check`: a command of a heavy kind with a remote rule runs on runners
  and falls back to the Mini (#1708, #1778, both merged).
- A PR lands by `gh pr merge --squash --delete-branch --match-head-commit <head>` (or `--merge`), inside a
  retry-safe action `fleet#issue:merge@head` (#509). `merged` then logs the evidence and closes the issue.
- `drover protection` asks integration for required status checks, a required PR and `drover/review` as a
  required check. drover never pushes to integration; it only merges PRs.
- After a merge, `land::auto_update` pushes integration into the other ready PRs only for
  `update_ready = "always"`, a strict branch rule or fast-first. A real conflict gets an `--update` lane.
- Fast-first (`land.rs`) lands before the full suite and bisects on integration afterwards, with reverts.
- The `integration` job (#1785) runs the same gate on integration's head and files one issue when it is red.

## Options

### 1. How the stack lands

| Option | Why not |
|---|---|
| B. One `git push` of the stack to integration | protection refuses it: no PR, and the top has no `drover/review` or CI result. It needs a bypass for drover's token, which is what `drover protection` warns about. Squashed PRs would show Closed, not Merged, and `merged` would have no merge commit GitHub made |
| C. A batch PR: push the stack to `batch/<id>`, open a PR, let CI run, merge it | atomic, and CI's runners do the gate; but drover would post `drover/review` on a head no reviewer saw, history turns into merges of merges (squash is the default mode), member PRs show Merged only if their commits are kept, a full CI run is added per batch, and every reader of "PR state is MERGED" and "first-parent subject ends `(#N)`" needs a second path |
| D. GitHub's merge queue | batching and bisecting for free, but not on every plan or account type, CI must run on `merge_group`, and drover loses the pinned head, its merge message and its ordering |
| **A. Check the stack locally, then land each PR by its own pinned merge, back to back (chosen)** | — |

A changes nothing GitHub sees: the same merge call per PR, the same checks at each PR's own head, the same
squash commit, message and closing comment. What it gives up is atomicity, and it buys that back with two cheap
checks per landing (decision 8).

### 2. Who is in a batch

| Option | Why not |
|---|---|
| Queue order | the queue orders starts; by `ready` its work is done, and a PR that started late but passed early would wait for no reason |
| A timer that fills a batch | holds a ready PR to wait for company; adds a clock and a state |
| A fleet-level batch step in drive's pass | merges already run in issue jobs under one lock; a second planner in `drive.rs` (2,163 lines) would race the per-issue `merge` step |
| **The PR `merge` was called for leads; riders are the other ready PRs, oldest PASS first (chosen)** | — |

### 3. Red

| Option | Why not |
|---|---|
| Fall back to one at a time | 8 more runs after the red one; today's cost plus one |
| Drop the newest half and retry | lands faster on average but can charge nobody for several rounds |
| Test every prefix | 8 runs |
| **Retry the top once, then binary-search the prefixes (chosen)** | — |

## Decision, point by point

**1. Batch size and pick.** `batch_max` per fleet, unset or 1 = off. The lead is the issue `drover merge` names;
it is the bottom of the stack, so the command's result is the lead's result, as today. Riders are the fleet's
other `ready` issues that pass `gate::eligible` and do not go alone (point 5), ordered by the time of the PASS at
their head, oldest first, at most `batch_max − 1`. No waiting: at low load a batch is one PR.

**2. Who runs the gate, and when.** The fleet's `premerge` checks, through `gate::run_checks` and
`Heavy::check`, unchanged: on runners where the fleet routes that kind, else on the Mini in a heavy slot. Once
per stack, on its top, in the lead's job, under the merge lock, as soon as the lead's merge step is due. A
fleet with `batch_max > 1` and no `premerge` still stacks: the stack then only finds conflicts.

**3. The stack.** In the scratch worktree: integration's tip, then `git merge --no-ff <head>` per member, in
order. The lead's conflict is today's `NOT_MERGEABLE` and its update round. A rider's conflict aborts that one
merge and leaves the rider out, with nothing logged on its issue: it may conflict only with a PR under it. Once
the batch has landed, the rider's own merge step finds the conflict against integration and gets today's update
round, with something real to resolve.

**4. Red.** A red top on a stack of two or more is run once more; green then is logged `flaky` and the batch
lands. Red twice: binary search for the first red prefix (3 runs for 8). That layer is the culprit, convicted
by a red run of its own prefix over a green one. It gets today's `premerge_failed`: a drive FAIL at its head
naming the PRs it was stacked on, the same code, an uncounted fix round. The layers under it land at once
(their prefix is green). The layers over it stay `ready` and see nothing; the next merge step leads them. A
stack of one keeps today's rule: no retry.

**5. Alone.** Never a rider, and as lead takes no riders: an issue marked `--serial` or joined by its diff to a
configured `[[fleet.<n>.serial]]` group (a migration is exactly this: `paths = ["db/migrations/**"]`), and a
lead that is not `ready` (an owner override). A PR whose diff touches a serial group it never joined is still
refused `SERIAL_UNMARKED` by `eligible`. A fleet under a strict branch rule never batches (GitHub refuses a
behind PR; `doctor` says so), and `batch_max` with fast-first landing is a load error: fast-first has its own
bisect, after landing.

**6. Update rounds.** Saved: every catch-up push between batch members (`auto-updated`, each a CI run), which
is most of them in an `update_ready = "always"` fleet. `auto_update` pushes nothing
while the merge lock is held. Not saved: a real conflict's update round, and a culprit's fix round. In the
default `needed` mode a behind PR was never updated, so the saving there is gate runs and waiting, not rounds.

**7. Verdicts and evidence.** Unchanged, because each PR still lands by `gate::land_one`: the merge is pinned
with `--match-head-commit` to the head `eligible` passed, under its own action id, and `merged` logs
`verdict_sha` and `merge_sha`, writes the closing comment, files follow-ups and opens the rollout. `merged`
gains only `batch`. The stack's merge commits are local and thrown away.

**8. Push.** There is no push of the stack. The PRs land by their own merges in stack order. Before each one
after the first, `land` waits up to `batch_settle_secs` for GitHub to say the PR is mergeable, and checks that
integration's tip is the merge before it. After each, it checks that the new tip's tree is that layer's prefix
tree. Any miss stops the landing: what landed is a prefix whose tree was in the stack, and the rest stay
`ready`. Where their next stack comes to a tree already logged green, it lands with no new run.

## Consequences

- Green path: one gate run and one lock hold per batch. `batch_max = 1` is today's merge, proved by `merge.rs`
  passing unedited.
- A culprit among 8 costs 5 or 6 runs against 8. A flaky gate costs one extra run and no longer charges anyone
  in a batch.
- A landing that stops leaves integration at a prefix the gate saw only as part of the whole. The same is true
  today of any tip between two merges; the `integration` job is the net for both.
- A culprit can be a PR that fails only together with one under it. Those have landed by the time its fix
  round starts, so the lane reproduces it on integration.
- A bisect run that flakes can charge an innocent PR an uncounted round; its lane answers with
  `drover done --no-change`. Today's single run has the same exposure on every merge.
- A red integration charges the lead, as it does today.
- Pushes to integration stay one per PR, now seconds apart. A CI with one concurrency group per branch (the
  template's) runs the first and the newest; one without runs all of them, as today.
- `gate.rs` shrinks (`premerge` and the landing tail move out); `drive.rs` is untouched.

## Slices

One PR, four commits, each green.

1. **One path.** Lift `eligible` and `land_one` out of `gate::merge`; move `premerge` into `batch::run` as a
   stack of one. No behaviour change; no test edited.
2. **Pick, stack, land.** `batch_max` and its load errors, `plan`, `Stack::build`, `land` with its two checks,
   `batch_settle_secs`, the three events, `auto_update`'s early return, the lease renewal and `job::limit`.
3. **Red.** The retry, the run cache by tree, the bisect, `charge`.
4. **Numbers and docs.** the replay's numbers (in the PR body), `docs/config/batch.md`, the
   example config line, `doctor`'s strict-rule line, the PR body's numbers and `## Rollout`.

## The AC's numbers

"CI runs per merged PR" over one day of one fleet, counted the same way before and after:

- CI runs: `gh run list -R <repo> --workflow <ci> --created <day>` by event (`pull_request`, `push`), over the
  day's `merged` count.
- Gate runs: `merged` events with `premerge_secs` and no `batch`, plus `batch-checked`, over the same count.

Batching is off until the owner turns it on, so the PR cannot show a live "after". A replay gives it
from the same day's log: it re-forms the batches from each PR's PASS time, the day's merge order and the
median gate time, and counts gate runs, catch-up pushes that fall inside a batch, and time under the lock.
Built (#1787): the replay ran once over the log for the PR body, not as a `stats` flag; the rollout asks the
owner to count the same way a day after and compare.

The `## Rollout` lines, for the fleet of this repo first:

```toml
[fleet.<name>]
batch_max = 4
```

Four first: a culprit costs 4 or 5 runs and a bad day touches four PRs. Raise to the fleet's `max_in_flight`
(8) after a week with no `batch-landed stopped` on a tree mismatch.

## Open questions for the owner

1. **N merges in a row, not one push?** Recommended: yes (option A). One push (B) needs drover's token to
   bypass integration's protection and ends the per-PR merge commit; a batch PR (C) keeps protection but puts
   `drover/review` on an unreviewed head and changes the history's shape.
2. **Hot-file groups.** Recommended: only configured serial groups and `--serial` marks go alone. Hot-file
   groups are one-slot serial groups joined by diff; counting them would send most of this repo's PRs alone.
   Two PRs on one hot file either stack clean and are gated together, or the later one drops out.

## `.drover/arch.toml` lines

No hot file is converted: the new code is a new file, `docs/config/batch.md` lands in a collected folder, and
`fleet.example.toml` and `docs/config/limits.md` come from `just gen`. Slice 2 adds the two bans that hold the
boundaries:

```toml
[[ban]]
pattern = "batch::run"
paths = ["crates/*/src/**"]
except = ["crates/core/src/gate.rs", "crates/core/src/batch.rs"]
reason = "#1787: gate::merge is the one door to a merge; a stack is built, checked and landed only under its merge lock"

[[ban]]
pattern = "\"push\""
paths = ["crates/core/src/batch.rs"]
reason = "#1787: a stack is never pushed; each PR lands by its own merge pinned to its reviewed head"
```
