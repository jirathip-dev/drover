
## Compact: a repo's fresh start

Compact is a feature for any fleet: the repo, its integration branch, `main` and the archive
(`<repo>-history`) all come from the fleet. `drover compact <fleet> --cut <date|sha>` replaces the fleet's integration line with one root commit (the
cut's exact tree, message `<repo>: initial (history before <date> in <owner>/<repo>-history)`) and one commit per
landed PR after it, each with the old commit's tree, message, author and date. Every other branch and notes ref is
carried with its files unchanged: an open PR's commits are copied onto their true base; a branch that last met the
line before the cut is carried detached; a branch fully merged is deleted (the archive keeps it). Nothing is
rebased or replayed, and every new tip's tree is checked equal to its old one (`COMPACT_TREE`).

- **Dry run.** `--dry-run` mirrors origin into `<state dir>/compact/<repo>/old.git`, writes `plan.json` and
  `compact-map.tsv` (old sha, new sha, `same`/`held`/`root`) and prints the old → new commit counts, the root,
  each PR commit, the open PRs to carry, the event records to remap and the readiness checks. It pushes nothing.
  It is the only form a lane may run.
- **Cards.** `--thread <id>` posts a run card for `drover compact <fleet> --push <new tip>` (every ref but
  `main`) and, when `main` moves, a second card for `drover compact <fleet> --main --push <main's new tip>`. Each is
  approved on its own (`--sha`); `--push` runs only inside its approved card (`COMPACT_UNAPPROVED`), so the line's
  card never pushes `main`. The fleet must be paused, no check lane working, every lane
  folder clean and pushed, no open PR from a fork or left detached, force pushes allowed (`COMPACT_NOT_READY`).
- **Main.** `main` is rewritten like any branch, but its new tip's tree must equal its old one and it is never
  deleted, else the cut aborts before any push (`COMPACT_TREE`). Its card runs after the line's (`COMPACT_NOT_READY`
  before), pushes `main` alone with `--force-with-lease` on its old tip, and its blast says the push may fire the
  repo's main-merge workflows (a prod deploy, migrations).
- **Archive.** The approved run first pushes every old branch, tag, note and PR head (as `pull/<n>`) to the
  private `<owner>/<repo>-history`, reads it back, and adds the plan and map on its `drover-compact` branch
  (`COMPACT_ARCHIVE` if it is public or misses a ref).
- **Push.** Then one atomic push moves every ref of origin but `main`, each with a lease on its old value (`COMPACT_MOVED`
  if one moved since the plan). The log is never edited: one `compacted` record per fleet and per issue with ids in
  both histories bridges them for `drover why` and hotspots; a line older than the cut is answered from the
  local archive copy. Open PR bodies get their new head ids; their old PASS does not carry, so each gets one fresh
  review. The fleet's clone and lane folders are reset (`--reset` repeats it); drive, spawn and merge refuse
  `OLD_ROOT` for a clone or PR head that is not on the new root.
- **Undo.** `drover compact <fleet> --undo --thread <id>` posts a card for `--undo --push <old tip>`, which puts
  every ref but `main` back from the scratch copy (else the archive) while all are still where compact put them,
  and logs `compact-undone`; and, when `main` moved, its own card for `--undo --main --push <main's old tip>`.
  Resuming the fleet ends that window.
- **Guard.** Compact remaps stored ids through one list, `compact::SAVED` (record kind, field); a sha-named field
  it leaves alone is in `compact::KEPT` with why. CI's `every_saved_sha_field_is_in_compacts_list` fails a PR that
  writes a new `*_sha` field in neither, and the sim fixture (merges, why notes, a PASS, an open PR, `main`; compact,
  then undo) runs in the normal test step (crates/cli/tests/compact.rs).
