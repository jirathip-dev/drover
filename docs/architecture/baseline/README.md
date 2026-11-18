# baseline: lanes see history only since a chosen commit (#1099)

## Summary

Decision: graft, never shallow. Each fleet clone keeps every commit; drover marks the baseline and the fork points of older branches merged after it as "no parents", and switches that view on per lane worktree, only where the branch contains the baseline. Everywhere else (the owner's checkout, drover's own git calls, branches cut before the baseline) history stays whole.
Slices: 1 drover's own git never looks through a graft, 2 the `baseline` key and the owner's verb that sets it, 3 the grafts and the per-worktree switch at spawn, 4 the sim with a baseline set and the workflow page.
Open questions: (1) a branch cut before the baseline: I refuse it the graft, not the lane (it keeps full history, spawn says why); refusing the lane would stop every open PR on re-baseline day. (2) Orchestrator threads read the owner's checkout, so they keep full history; say if they should be cut too.

Read this file, then `adr.md`. `shapes.md`, `boundaries.md` and `facts.md` are for the impl lane and the reviewer.

## Why one graft is not enough

The issue asks for `git replace --graft <baseline>`. Two things in this repo break that as written (both measured,
`facts.md`):

1. **Replace refs are shared by every worktree of a clone**, the owner's checkout and drover's own `merge-tree`
   and `blame` included. A branch cut before the baseline, seen through the graft, has no common ancestor with the
   integration branch: `git merge` answers "refusing to merge unrelated histories". On the day a baseline is set,
   that is every open PR.
2. **PRs land as true merges.** The first older branch that merges after the baseline brings its whole ancestry
   back through the merge's second parent, and every lane cut after that sees all 2,867 commits again.

So the design has two parts: *which commits are cut* (the baseline and each older fork point that integration has
since merged: 15 commits for a baseline 50 merges back) and *who looks through the cut* (a per-worktree git
setting, off by default).

## The design on one screen

| Who | Sees | How |
|---|---|---|
| A lane whose branch contains the baseline | history since the baseline; old shas still open with `git show` | its worktree's own config turns replace refs on |
| A lane on a branch cut before it | full history, and a line from spawn saying why | the switch stays off; an update round that merges integration in makes it eligible at its next spawn |
| The owner's checkout, orchestrator threads | full history | the clone's shared config turns replace refs off |
| drover itself (gate, merge preview, update, `why`, hotspots, lessons) | full history, always | the runner adds `--no-replace-objects` to every `git` it starts |
| GitHub, every sha, `refs/notes/why`, pushes | unchanged | replace refs are local and never transferred |

"Since the baseline" is exact: the baseline, every commit that is not its ancestor, and one parentless fork point
per older branch merged later. On today's history with the baseline 50 merges back: 363 commits of 2,867.

## How the callers read

The owner sets it, and re-sets it after a big cleanup:

```console
$ drover baseline drover
drover baseline: none -> 6e618cd22a43 (363 of 2867 commits; branches cut before it keep full history)
$ drover baseline drover 0123abc
REFUSED BASELINE_OFF_INTEGRATION: 0123abc is not on staging; a lane's branch could never contain it
```

fleet.toml gains one line, and removing it undoes the baseline for the fleet:

```toml
[fleet.drover]
baseline = "6e618cd22a43225e18a773b2dc5bd9ab24a9088b"
```

Spawn, once the worktree exists (new or reopened), on the lane's host:

```rust
let view = crate::baseline::view(r, f, &p.worktree, &heads);   // heads: the branch, and the saved wip it is told to merge
writeln!(out, "{}", view.line(&p.branch))?;                    // "history: since 6e618cd22a43" | "history: full (issue-1042 was cut before 6e618cd22a43)"
```

The drive pass, right after the fetch it already does in the main checkout:

```rust
git(r, &dir, &["fetch", "-q", "origin"])?;
let _ = crate::baseline::grafts(r, f, &dir);
```

Nothing else changes at a call site. `overlap`, `gate`, `land`, `update`, `why`, `hotspots` and `lesson` keep their
git calls as they are; the runner makes them graft-blind:

```rust
// runner.rs: the local process seam, and the line hosts::On quotes for ssh
let args = plain("git", args);        // ["--no-replace-objects", "-C", dir, "merge-tree", …]
```

A lane's brief gains one line when its view is on:

```
History: `git log` and `git blame` here start at 6e618cd22a43, the fleet's baseline. Older commits still open by sha;
`drover why <file:line>` and `git --no-replace-objects log` see through it.
```

## Undo, at four sizes

```console
$ git --no-replace-objects log                         # one command
$ git config --worktree core.useReplaceRefs false      # one worktree, until that lane's next spawn
$ git replace -d $(git replace -l)                     # one clone, until drover's next spawn or pass there
```

The fleet: remove `baseline` from `[fleet.<f>]`. Each clone drops its grafts at its next spawn (the main host: at
the next pass).

## The AC, mapped

| AC | Met by |
|---|---|
| `git log --oneline \| wc -l` counts only commits since the baseline; `git show <old sha>` works | slice 3; the count equals `git --no-replace-objects rev-list --boundary HEAD origin/<integration> ^<baseline>` |
| `merge-base` with a branch cut after the baseline is unchanged | holds for any two commits that both contain the baseline (`adr.md`, "Why merge-base holds") |
| a branch cut before it is refused with a clear message | the graft is refused for its worktree, in spawn's output (open question 1); a baseline that is not on integration is `REFUSED BASELINE_OFF_INTEGRATION` |
| the drive sim passes with a baseline set | slice 4 |
| `docs/workflow.md` explains the baseline and its undo | slice 4: the page is `drover diagram`'s output, so the section comes from the code |

## Slices, in order

1. **drover is graft-blind** (`runner`, `hosts::On`): `--no-replace-objects` on every `git` the runner starts, here
   and over ssh; a guard test beside the runner-limits test. Lands first, changes nothing while no graft exists.
2. **The key and the verb** (`config`, `cli`, `hook`, the usage doc, the config docs, the example fleet file):
   `baseline` as a typed 40-hex sha; the owner-only verb with its refusal; a `baseline-set` event.
3. **Grafts and view** (new `core::baseline`; one call in `spawn`, one in `overlap::preview`; the brief line): a
   real-git test for AC 1 and 2, including a true merge of an older branch, a re-baseline and the key removed.
4. **Sim and docs** (the sim's scenario config, `diagram`): the fixed seeds run with a baseline set; the
   workflow page gains a "History baseline" section with the worktree statechart and the four undos.

One PR, four commits. No slice touches a hot file by more than a line or two; the new code is one new module.
