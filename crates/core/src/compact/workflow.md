
## Compact: a repo's one-time fresh start

`drover compact <fleet> --cut <date|sha>` replaces the fleet's integration line with one root commit (the
cut's exact tree, message `<repo>: initial (history before <date> in <owner>/<repo>-history)`) and one commit per
landed PR after it, each with the old commit's tree, message, author and date. Every other branch and notes ref is
carried with its files unchanged: an open PR's commits are copied onto their true base; a branch that last met the
line before the cut is carried detached; a branch fully merged is deleted (the archive keeps it). Nothing is
rebased or replayed, and every new tip's tree is checked equal to its old one (`COMPACT_TREE`).

- **Dry run.** `--dry-run` mirrors origin into `<state dir>/compact/<repo>/old.git`, writes `plan.json` and
  `compact-map.tsv` (old sha, new sha, `same`/`held`/`root`) and prints the old → new commit counts, the root,
  each PR commit, the open PRs to carry, the event records to remap and the readiness checks. It pushes nothing.
  It is the only form a lane may run.
- **Card.** `--thread <id>` posts a run card for `drover compact <fleet> --push <new tip>`; `--push` runs only
  inside that approved card (`COMPACT_UNAPPROVED`). The fleet must be paused, no check lane working, every lane
  folder clean and pushed, no open PR from a fork or left detached, force pushes allowed (`COMPACT_NOT_READY`).
- **Archive.** The approved run first pushes every old branch, tag, note and PR head (as `pull/<n>`) to the
  private `<owner>/<repo>-history`, reads it back, and adds the plan and map on its `drover-compact` branch
  (`COMPACT_ARCHIVE` if it is public or misses a ref).
- **Push.** Then one atomic push moves every ref of origin, each with a lease on its old value (`COMPACT_MOVED`
  if one moved since the plan). The log is never edited: one `compacted` record per fleet and per issue with ids in
  both histories bridges them for `drover why` and hotspots; a line older than the cut is answered from the
  local archive copy. Open PR bodies get their new head ids; their old PASS does not carry, so each gets one fresh
  review. The fleet's clone and lane folders are reset (`--reset` repeats it); drive, spawn and merge refuse
  `OLD_ROOT` for a clone or PR head that is not on the new root.
- **Undo.** `drover compact <fleet> --undo --thread <id>` posts a card for `--undo --push <old tip>`, which puts
  every ref back from the scratch copy (else the archive) while all are still where compact put them, and logs
  `compact-undone`. Resuming the fleet ends that window.
