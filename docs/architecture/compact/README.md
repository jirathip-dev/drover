# compact: a one-time fresh start for a repo's history (#1121)

## Summary

Decision: compact copies trees and never replays patches: one root before the cut, one commit per landed PR
after it, every other branch carried across with its files unchanged. The event log is not edited: one new
record per issue says where its commits live now, and older ids are answered from the private archive.
Slices: plan and dry run; write and check; archive, owner card, one all-or-nothing push; log records and their
readers; clone reset and the old-root refusal; undo; the sim and workflow page, then cooee and drover live.
Open questions: main moves in the same push? Delete the fully merged leftover branches? The line cap.

Read this page first, then `adr.md` (why), `shapes.md` (types, files, events) and `boundaries.md` (who calls
whom, the statecharts). The three open questions are at the end of `adr.md`.

## The numbers today (2026-11-16, cut "before 2026-11-09")

| repo | line that is cut | commits now | after | other branches | open PRs |
|---|---|---|---|---|---|
| drover | `staging` | 2,960 | 1 root + 210 | 724 (about 570 fully merged) | 17 |
| cooee | `staging` | 1,081 | 1 root + 131 | 360 (about 180 fully merged) | 19 |

Two facts differ from the issue text:

- cooee's fleet lands on `staging`, not `main`, so the cut runs along `staging` there too. The issue's "about 32"
  counted main's own first-parent line. `main` follows in both repos (open question 1).
- A date is read as git reads it, on the main host's clock: the last commit on the line before that midnight.
  The dry run prints the commit it found; from then on only that sha is used.

## How it is used

The owner (or an orchestrator in a thread) runs three commands. Nothing leaves the machine before the card.

```text
$ drover pause cooee
$ drover compact cooee --cut 2026-11-09 --dry-run
compact <owner>/cooee · line staging · cut 89570c966 2026-11-08 23:46 "<subject>"
commits   staging 1081 → 132 (1 root + 131) · main 736 → 82
root      tree 5c1d0e7a… "cooee: initial (history before 2026-11-09 in <owner>/cooee-history)"
chain     131: 127 PRs, 4 direct pushes
  0a1b2c3 ← 9f8e7d6  board: one sheet for every card (#310) (#322)      holds 14 old commits
  …
refs      361: line 1 · same tree 9 · carried 141 (19 open PRs) · detached 28 · fully merged 182
open PRs  19, each costs one fresh review
  #331 issue-325  4 commits  5d6e7f8 → 7e8f9a0  ready
  …
events    131 landed issues (412 ids) · 19 open issues (57 ids)
  #310  merged 9f8e7d6 → 0a1b2c3, 3 lane heads held
  …
checks    tip trees equal ✓ · paused ✓ · lanes quiet ✓ · no baseline ✓ · force push allowed: staging ✗ main ✗
new tip   3d4e5f6a7b8c… · nothing pushed

$ drover compact cooee --cut 2026-11-09 --thread <thread>
plan 3d4e5f6a7b8c written · run card r12 posted

$ drover card approve r12 --sha 51c0ffee12ab        # the owner; the card runs:
                                                    #   drover compact cooee --push 3d4e5f6a7b8c…
```

The card's command is drover's own verb with the new tip in it, so the approval covers exactly the history the
dry run showed. The approved run does, in this order, each step safe to repeat:

1. **Check again.** Fleet paused, no lane working, every lane folder clean and pushed, no landing waiting on
   its suite, every ref on origin still where the plan saw it.
2. **Archive.** Create the private `<repo>-history` if missing, push every old ref and note there, read it
   back and compare, add the map. A public or different archive stops the run.
3. **Push.** One all-or-nothing push of every ref, each with a lease on its old value. Either all of origin
   moved or none of it did.
4. **Log.** Append the `compacted` records, in one transaction.
5. **Tidy.** Rewrite the head ids named in open PR bodies, reset the clone and every lane folder on every
   host that answers. Files on disk do not change (the trees are the same), so builds stay warm.

Then the owner looks (CI on the new tips, a few `drover why` lines, GitHub's commit list) and resumes the
fleet. **Resume is the end of the undo window**: undo restores origin exactly, so it refuses once a ref has
moved again.

```text
$ drover compact cooee --undo --dry-run
$ drover compact cooee --undo --thread <thread>      # card: drover compact cooee --undo --push <old tip>
```

A clone that missed the reset (a host that was off) is refused work until it is reset:

```text
$ drover spawn cooee 340 impl
REFUSED OLD_ROOT: host-2 still has cooee's history from before the compact → drover compact cooee --reset
```

## How the code that uses it reads

Only code that takes a stored id to git asks where it lives now. There are two such places.

```rust
// why.rs: blame gave `sha` in today's repo; find the issue that landed it
let shas = compact::Shas::of(&events);
if let Some(run) = shas.root(&sha) {
    return before_cut(ctx, r, out, run, &blame);        // the same question, asked in the archive
}
let landed = |m: &str| events.iter().find(|e| e.kind == "merged" && e.str("merge_sha").map(|s| shas.now(s)) == Some(m));

// why.rs: a note recorded on a lane head; hotspots.rs: a landing's update rounds
let text = note(r, &local, shas.now(e.str("sha")?));
```

The gate does not ask. A verdict stays pinned to the exact commit a reviewer ran:

```rust
// gate.rs, unchanged: no PASS at the new head → STALE_VERDICT → a fresh review
let at = |x: &&(String, String, String)| x.0.eq_ignore_ascii_case(&head);
```

Three doors check the root, with one function:

```rust
// drive (the fleet's clone), spawn (the lane's folder), merge (the PR head)
compact::rooted(ctx, r, &events, f, &dir, rev)?;        // Err(OLD_ROOT) names the host and the reset command
```

## What the sim proves, and what only cooee can

The sim (slice 7) builds a repo with true merges, squash merges, catch-up merges, a direct push, why notes,
a PASS record, a `main` with promotion merges and an open PR with a catch-up merge in it. It asserts the four
acceptance lines of the issue, plus: the plan is the same on a second run, and a run killed after the push
finishes on the next call.

Three things are GitHub's behaviour and cannot be simulated. The cooee run checks them, with undo as the way
back: an all-or-nothing push that includes protected branches; open PRs staying open with the same diff when
base and head move together; the archive taking PR heads as branches.
