# ADR: compact, a one-time fresh start for a repo's history (#1121)

## Summary

Decision: compact copies trees and never replays patches: one root before the cut, one commit per landed PR
after it, every other branch carried across with its files unchanged. The event log is not edited: one new
record per issue says where its commits live now, and older ids are answered from the private archive.
Slices: plan and dry run; write and check; archive, owner card, one all-or-nothing push; log records and their
readers; clone reset and the old-root refusal; undo; the sim and workflow page, then cooee and drover live.
Open questions: main moves in the same push? Delete the fully merged leftover branches? The line cap.

## Status

Proposed (architect lane, 2026-11-16, staging `00747d7a5`). Read `README.md` first.

## Question

How does drover replace a repo's history with a short one, once, so that the files at every tip are provably
the same, drover's own records still answer, the owner approves the one irreversible moment, and it can be put
back? And, as the issue asks: are stored ids remapped by a map plus `compacted` events or by rewriting state,
and where does this sit next to #1097 and #1099?

## Where it stands

- **The log pins commits in about 60 places.** Most compare a stored id with an open PR's live head (the gate at
  `crates/core/src/gate.rs:577`, drive, `waiting`). Two take a stored id to git for landed work: `drover why`
  (`crates/core/src/why.rs:468`, and the note readers below it) and hotspots (`crates/core/src/hotspots.rs:94`).
- **The log is a database with monthly archives** (`crates/core/src/db.rs:390`). Events of issues done more than
  14 days ago are compressed files; a saved fold and a per-process cache sit on top.
- **One in-place rewrite exists**: the fleet rename (`crates/core/src/db.rs:496`). It swaps one label for
  another, one to one, after a full copy.
- **The id map is many to one.** In drover 1,682 commits become the root; a PR's merge and its lane commits
  become one commit. Read backwards, the root "is" over three hundred landed issues.
- **Open PR branches contain catch-up merges** (update rounds merge the line in). A rebase drops merges and
  replays each lane commit on the newer base: it can conflict, and it does not keep the tree.
- **Both repos have a `main`** made of promotion merges on top of `staging`, and hundreds of leftover branches.
  Any ref left on the old history keeps all of it public and fetched by every clone.
- **Every commit on the line after the cut already reads as the issue wants**: the PR title with both
  numbers, written by `drover merge` (`crates/core/src/gate.rs:614`), authored by the PR's author. No GitHub
  lookup is needed to title them.
- **A run card may not carry a force push** (`crates/core/src/card.rs:58`), and approves a command by its sha.
- **`drover compact` already exists**, hidden: it archives old events (`crates/core/src/cli.rs:594`).
- **No cap trailer, promise or SLO record needs a map.** The landed commit's last line (`… · PASS fd6fa67`)
  names the reviewed lane head, kept by GitHub's PR page and the archive; a commit cannot name itself. The cap
  cache is keyed by tip and recomputes. A promise is pinned to (state, head) and is made again when a head moves.

## Options: how stored ids are remapped

1. **Rewrite the rows** (as the rename does). Rejected. The map loses information: after it, over 300 `merged`
   records all name the root, and "which issue landed this line" has no answer. The compressed archives would
   need rewriting too. Undo would mean restoring a copy and losing every event logged since.
2. **Translate on read, in the loader**: every reader sees new ids, no reader changes. Rejected. Same loss for
   everything before the cut; it sits in the hottest code (the cached log, the saved fold, the perf gates);
   and it would carry a PASS to a commit no reviewer ran.
3. **Restate each record**: append a second `merged`, `verdict`, `why` with the new id. Rejected. Counts double
   (landing stats, lessons, the board's feed) and every reader must learn to prefer the newer copy.
4. **Two id spaces and a bridge (chosen).** Old ids keep their meaning, in the archive. New ids mean what git
   says, in the repo. The few hundred commits that exist in both are bridged by appended `compacted` records, one per
   issue, read by one small type. The full map is a file in the archive, for people and for undo.

Smaller choices:

| choice | chosen | not chosen |
|---|---|---|
| open PR branches | **carry**: each commit made again with the same tree and message, parents through the map | `git rebase --onto` as the issue words it: drops catch-up merges, may conflict, changes trees |
| a branch that last met the line before the cut | **detached**: files kept, git refuses to merge it | parent it on the root: a later merge takes the wrong base and reverts work without a word |
| which refs | **every** head, tag and notes ref, by one function | the line alone: `main` and leftovers would keep the old history public |
| a PASS on an open PR | **not carried**: a fresh review at the new head, as the AC says | carry it because the tree is equal: makes the gate trust a map |
| titles and authors | **from git alone**, so the same input gives the same new ids | ask GitHub per PR: slower, and the plan could change between dry run and push |
| when the archive is made | **inside the approved run**, first, verified before the push | before the card: creates a repo nobody approved |
| who posts the card | **the verb**, with the new tip in the command | a hand-written `git push --force` card: forbidden, and rightly |
| notes before the cut | **stay in the archive** | joined onto the root: one note of megabytes |
| the old hidden `compact` | **renamed** `archive-events`, its one test moved | two meanings under one verb |

## Decision

**Principle: compact moves commits, never content. Every new commit takes an existing tree; nothing is merged,
rebased or replayed.**

1. **One function places every old commit** (`shapes.md`): at or before the cut → the root; on the line after
   the cut → its own new commit, same tree; merged into the line after the cut → the commit of the PR that
   landed it; anything else (open PRs, `main`'s promotion merges, wip branches) → a copy with the same tree,
   author, date and message. A copy keeps a parent only where the base still exists with the same tree. So a
   branch that has merged the line since the cut keeps its true merge base, and one that has not is carried
   detached: its files are kept and git refuses to merge it, rather than merging it on a wrong base.
2. **The line.** The root has the cut's tree and the issue's message. Each later commit keeps its message,
   author and dates. A GitHub-style `Merge pull request #N` subject (cooee may have some) becomes the PR title
   that its body carries, plus `(#N)`, authored by the PR head's author.
3. **Every ref moves by that function**, notes refs and tags included. Notes after the cut are joined onto
   their PR's commit, in commit order, as #1097 does; a commit that already has its joined note keeps it alone.
4. **The check.** For the line, `main` and every carried or detached ref: the new tip's tree equals the old
   tip's, read from git before anything is sent. One mismatch aborts (`COMPACT_TREE`). The plan is a pure
   function of the old repo and the cut: a second run gives the same ids.
5. **The log.** Never edited. The approved run appends one `compacted` record for the run and one per issue
   that has ids in both spaces (landed after the cut, or open). `compact::Shas` folds them. `why.rs` and
   `hotspots.rs` ask it; **the gate and the fold never do**, and a ban holds that (below).
6. **`drover why` for an old line.** Blame lands on the root. drover then asks the same question in its local
   copy of the archive, at the cut, where the old ids and the old notes still match the log as they are.
   Without that copy (another host) it says the line predates the cut and names the archive.
7. **Archive, card, push.** As the README's five steps. The push is one atomic push from drover's scratch copy,
   a lease on every ref. `--push` runs only inside its own approved, running card (`COMPACT_UNAPPROVED`).
8. **Open PRs.** Carried like any ref. Their bodies' head ids are rewritten through the map, or the evidence
   check would fail on every one. Their why notes move with their commits. Their verdicts do not: each needs
   one fresh review, which drive starts as for any moved head.
9. **Clones.** The approved run resets the clone and each lane folder on every host that answers. Drive,
   spawn and merge refuse `OLD_ROOT` for a clone, folder or PR head that does not stand on the new root. For
   a clone or folder the fix is `--reset`, safe to repeat; a detached branch has none and says so.
10. **Undo.** Allowed while every ref is still where compact put it. The same push backwards, from the scratch
    copy or else the archive; then one `compact-undone` record, after which `Shas` reads the other way for
    anything logged in between.

## Order with #1097 and #1099

- **#1097 (squash merges) first.** It has landed and is the default; without it the line refills with lane
  commits. Compact reads both shapes: a true merge's lane commits from its second parent, a squash's from the
  PR's head ref.
- **#1099 (the baseline graft) is independent, and unnecessary for these two repos afterwards.** A lane in a
  compacted repo sees 211 or 132 commits with no graft: the root is the baseline. #1099 stays useful for
  fleets that are not compacted and for a later, shallower cut. Neither blocks the other and they share no
  code. One rule joins them: a `baseline` names an old commit, so compact refuses while the fleet has one
  (`COMPACT_NOT_READY`); remove the key first and set a new one afterwards if still wanted.

## Consequences

- **Each open PR costs one review round** and one CI run. 17 and 19 are open today; the issue's plan to run
  after the wave merges keeps that small. An issue already at its round limit gets its usual card.
- **Resume ends the undo window.** After new work lands, the way back is the archive, by hand.
- **The owner opens the door.** Force pushes must be allowed on `staging` and `main` for the run (and admin
  bypass, where checks are required). drover checks, names the setting, never changes it, and reports while it
  is still open afterwards.
- **GitHub.** Merged PR pages keep their commits. A link to an old merge commit on `staging` may stop
  resolving one day; the archive has it. The new commits are unsigned (GitHub signed the old merges). The push
  to `main` runs its workflows once more on an unchanged tree.
- **drover's own repo.** Installed builds name an old commit until the next self-update installs the new tip
  (same tree); hosts follow by their usual sync. cooee has no such step.
- **Old unmerged branches come across detached.** In drover that is 49 of 158 unmerged branches and none of
  the 17 open PRs. An open PR that would be detached stops the run: update it or close it first.
- **A PR from a fork cannot be carried**; compact names it and refuses.
- **Size.** About +650 lines in `src/`, of which about 150 are kept for good (`Shas`, the root check, the
  archive fallback in `why.rs`). The rest is one-time and can be deleted after both runs (open question 3).
- **Hot files** get only call sites: the verb in `cli.rs`, one line each in `drive.rs`, `spawn.rs` and
  `gate.rs`, the two readers. Everything else is the new `compact/` folder.

## Slices (commits of one PR, each green)

1. **Plan.** The old repo read once into values; the placing function, pure and unit-tested; `--dry-run`'s
   report. The hidden `compact` becomes `archive-events`.
2. **Write and check.** Commits, notes and tags written as raw objects into the scratch copy; the tree check.
3. **Archive, card, push.** The private archive and its read-back; the card; `--push` and its guard.
4. **Records and readers.** The `compacted` events, `Shas`, `why.rs` (with the archive fallback) and
   `hotspots.rs`; the ban.
5. **Clones.** The reset, the open PR bodies, `OLD_ROOT` at the three doors, the lane hook line.
6. **Undo.**
7. **Proof.** The sim of the README; the workflow page's section from `just gen`; then the cooee run and the
   drover run, each owner-approved, their before and after counts in the PR.

## Open questions for the owner

1. **`main` moves too.** The issue speaks of one branch. If `main` stayed, the whole old history would stay
   public on it and `staging` could never be promoted again. The card lists every ref. Recommend: yes, in the
   same push.
2. **Fully merged leftover branches** (about 570 in drover, 180 in cooee): they hold nothing of their own. Either
   each becomes a pointer at the commit that now holds it (mostly the root), or compact deletes them on origin;
   the archive keeps them and undo brings them back. Recommend: delete. It is one line in the placing function.
3. **The cap.** Headroom is 662 lines before the open PRs. Recommend: raise it for this issue in the usual
   way, and queue a follow-up that deletes the one-time code after both runs.

## `.drover/arch.toml` lines

No central list or generated file is converted, so there is no `frozen`, `generated` or `[[collect]]` line.
The impl lane adds this ban in slice 4, in the same PR:

```toml
[[ban]]
pattern = "Shas"
paths = ["crates/core/src/gate.rs", "crates/core/src/events.rs", "crates/core/src/db.rs"]
reason = "#1121: a verdict is pinned to the exact commit reviewed; the gate, the fold and the log never read a compact map"
```
