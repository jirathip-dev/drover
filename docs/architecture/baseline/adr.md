# ADR: a per-fleet history baseline by graft (#1099)

## Summary

Decision: graft, never shallow. Each fleet clone keeps every commit; drover marks the baseline and the fork points of older branches merged after it as "no parents", and switches that view on per lane worktree, only where the branch contains the baseline. Everywhere else (the owner's checkout, drover's own git calls, branches cut before the baseline) history stays whole.
Slices: 1 drover's own git never looks through a graft, 2 the `baseline` key and the owner's verb that sets it, 3 the grafts and the per-worktree switch at spawn, 4 the sim with a baseline set and the workflow page.
Open questions: (1) a branch cut before the baseline: I refuse it the graft, not the lane (it keeps full history, spawn says why); refusing the lane would stop every open PR on re-baseline day. (2) Orchestrator threads read the owner's checkout, so they keep full history; say if they should be cut too.

Status: proposed (architect lane, 2026-11-16). Read `README.md` first.

## Question

Lanes should see history only since a commit the owner picks, with nothing rewritten. Shallow fetch or graft? And
how does either live with what this repo does all day: many worktrees on one clone, true-merge PRs, `merge-tree`
previews, update rounds, `drover why`'s blame?

## Where it stands

- Every lane on a host is a worktree of one clone, `[fleet.<f>] local` (`crates/core/src/spawn.rs:651`); on the
  main host that clone is the owner's checkout. drover's own git runs in the same clone: the conflict preview's
  `merge-tree` (`crates/core/src/overlap.rs:28`), the pure-update check (`crates/core/src/overlap.rs:140`), the
  update and premerge worktrees (`crates/core/src/drive.rs:2154`, `crates/core/src/gate.rs:640`).
- `drover why` blames in the lane's own worktree (`crates/core/src/why.rs:412`) and then walks merges up to
  integration to find the issue.
- PRs land with `gh pr merge --merge` (`crates/core/src/gate.rs:905`): integration has 1,474 merge commits in 2,867.
- A fix or update lane merges integration into its branch itself; a respawn may be told to merge a saved `wip/`
  branch first (`crates/core/src/spawn.rs:1290`).
- `origin` has 693 branches; 607 do not contain the commit 50 merges back.
- Every process starts in the runner (`crates/core/src/runner.rs:125`) or, on another host, in the line
  `hosts::On` quotes for ssh (`crates/core/src/hosts.rs:82`).

## Options

1. **Shallow (`--shallow-since`, `--depth`).** Git computes the cut itself, merges included. But the shallow list is
   one file for the whole clone with no per-worktree or per-command off switch: drover's `merge-tree`, its
   ancestor checks and `why`'s blame would all run cut. A fresh shallow clone does not hold the old objects, so
   `git show <old sha>` fails (AC 1). The cut is a date, not the sha the owner picked. A fetch of an older branch
   moves the boundary. Undo is a network `fetch --unshallow`. Rejected.
2. **One graft on the baseline, as the issue words it.** Shared by every worktree, so branches cut before it lose
   their merge base (`facts.md` 5), and undone in practice by the first older branch merged after it
   (`facts.md` 6). Rejected as written; it is the core of option 4.
3. **One graft, plus a rule that a PR must be rebased or squashed past the baseline before it merges.** Keeps the
   cut clean, but rewrites lane shas that verdicts, why notes and the event log pin: the thing the issue rules out.
   Rejected.
4. **Boundary grafts, seen per worktree, never by drover (chosen).** Below.
5. **The same grafts under a private ref namespace, switched on by an environment variable in the lane's shell.**
   No shared config, but the view then depends on every child process inheriting the variable, a plain `git log`
   in the worktree cannot test it, and the issue's `git replace` commands stop working. Rejected.

## Decision

**What is cut.** For a baseline `T`, drover grafts every boundary commit of "integration and this lane's head, minus
`T`'s history": `T` itself and the fork point of each older branch that has been merged since. Each becomes
parentless through an ordinary replace ref. The set only grows while `T` stands, the replacement objects are
deterministic (the same sha in every clone), and making them is idempotent. The exact commands are in `shapes.md`.

**Who sees it.** A clone with a baseline has `core.useReplaceRefs = false` in its shared config, written before the
first graft, so nothing sees the cut by default. A lane worktree gets `core.useReplaceRefs = true` in its own
config (git's `extensions.worktreeConfig`) when, and only when, the branch it has checked out and every head its
brief tells it to merge contain `T`. Spawn decides again on every spawn, a reopen included.

**drover never sees it.** The runner adds `--no-replace-objects` to every `git` it starts, at its local seam and in
the ssh line. Gate, preview, update, land, `why`, hotspots and lessons therefore behave exactly as today, whatever
a worktree's switch says. This is the one line that makes "the gate and the event log are unaffected" true rather
than hoped for; it lands first.

**Setting it.** The owner's verb resolves the sha (default: the integration tip, after a fetch), refuses one that is
not an ancestor of integration, writes the key the way `pause` writes `paused`, and logs `baseline-set` with the
old and new sha. It is owner-only in the hook. It does not touch any clone; the next spawn or pass does.

**Changing or removing it.** Each clone remembers the baseline its grafts were built for (`drover.baseline` in its
git config). When that differs from fleet.toml, drover first turns the view off in every worktree of the clone,
then deletes the replace refs, then builds the new set; lanes get the view back at their next spawn if eligible.
With the key gone it stops after the delete and unsets its config lines.

**Failing.** Any error (a baseline hand-edited to a sha that is not on integration, a graft git refuses) leaves the
worktree on full history and logs `baseline-failed`. Full history is the safe state; a lane is never refused or
delayed for the baseline.

**Clones stay full.** `clone_sh` is unchanged. Nothing fetches shallow.

### Why merge-base holds

Take two commits that both contain `T`. Every real merge base of the two is `T` or a commit that is not an
ancestor of `T` (anything older is dominated by `T`). A path from either commit to such a base never passes
through a strict ancestor of `T`, and grafts only cut edges that leave `T` or its strict ancestors. So every real
merge base is still reachable and still a best common ancestor: the answer is unchanged (`facts.md` 4, 10).
Integration always contains `T` (the verb checks it); the view is on only where the lane's heads do.

### Where it differs from the issue

- Several grafts, not one, and behind a per-worktree switch: both forced by the two facts above.
- `git replace -d <baseline>` still works but is re-made at drover's next spawn in that clone; the durable undos
  are the worktree switch and the key (`README.md`, "Undo").
- "A branch cut before it is refused": the graft is refused, the lane is not (open question 1).

## Consequences

- A lane on a branch cut before the baseline keeps full history until its PR merges. Once an update round has
  merged integration in, its next spawn turns the view on, and its own fork point is grafted with it.
- The day a baseline is set, open PRs are all "cut before". The benefit arrives with the lanes spawned after it.
- Seen through the view, `git show <baseline>` and the tail of `git log -p` print the baseline's whole tree as
  added (it is a root), and blame marks older lines as boundary lines of the baseline. The brief line tells the
  lane how to look past it.
- Between an older branch merging and drover's next spawn or pass in a clone, a lane that merges integration can
  see that branch's ancestry. It is a view leak, not a correctness one, and it closes by itself.
- drover owns `refs/replace/` in a fleet clone while a baseline is set: a replace ref made there by hand is
  deleted on a re-baseline.
- Git skips its commit-graph file while replace refs are in use; at this size it does not show.
- On another host a changed or removed baseline arrives with the next spawn there. Until then its live lanes
  keep the old view, which stays correct for them.
- Fakes in tests that match a remote `git` line gain the flag; local fakes sit above the seam and do not change.
- Hot files take a line or two each (`cli.rs`, `config.rs`, `spawn.rs`, `hook.rs`, `overlap.rs`); the code is one
  new module. No file is converted, so nothing is added to `frozen`, `[[collect]]` or `generated`.

## `.drover/arch.toml` lines (same PR)

They keep the boundary: one module knows the graft, one knows the flag.

```toml
[[ban]]
pattern = "useReplaceRefs"
paths = ["crates/*/src/**"]
except = ["crates/core/src/baseline.rs"]
reason = "#1099: only core::baseline switches a worktree's history view"

[[ban]]
pattern = "refs/replace"
paths = ["crates/*/src/**"]
except = ["crates/core/src/baseline.rs"]
reason = "#1099: only core::baseline makes or deletes a graft"

[[ban]]
pattern = "no-replace-objects"
paths = ["crates/*/src/**"]
except = ["crates/core/src/runner.rs", "crates/core/src/baseline.rs"]
reason = "#1099: the runner makes every git call graft-blind; no call site adds or drops the flag (baseline.rs names it in the brief line)"
```
