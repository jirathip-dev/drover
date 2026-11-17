# Boundaries and state

## Modules

Everything new is `core::compact`, one folder, inside `crates/core`. Core gains no layer edge.

| file | owns | may call | never |
|---|---|---|---|
| `compact.rs` | the verb: which step, the readiness check, the dry run's text, the refusals | the files below, `card`, `config`, the events | git or `gh` itself |
| `compact/plan.rs` | `Old` → `Shape`: the placing function, ref classes, the issue records | nothing: values in, values out | the runner, the clock, the log |
| `compact/write.rs` | reading `Old`, writing commits, notes and tags into the scratch copy, the tree check | the runner (git, one `gh pr list`) | origin, the fleet's clone |
| `compact/push.rs` | the archive and its read-back, the atomic push, undo's push, open PR bodies | the runner (git, `gh`) | the fleet's clone, lane folders |
| `compact/reset.rs` | the clone and lane folders on each host | `hosts::On`, `spawn::dirty`, `gh::forget_pr` | origin |
| `compact/shas.rs` | `Shas`, `Run`, `rooted` | the events, the runner (one ancestor check), the state db (a stamp) | everything else in the folder |

Who calls in:

| caller | calls | for |
|---|---|---|
| `cli.rs` | `compact::run` | the verb |
| `why.rs`, `hotspots.rs` | `Shas::of`, `now`, `root` | a stored id on its way to git |
| `drive.rs`, `spawn.rs`, `gate.rs` (merge) | `compact::rooted` | `OLD_ROOT` |
| `compact.rs` | `card::post`, and a new `card::running(events, cmd)` | the card and the `--push` guard |
| `compact.rs` | a new batch append in `db.rs` | the records, one transaction |
| the lane hook | a deny line | a lane may run only `--dry-run` |

Rules that keep this honest:

- `shas.rs` is the only part other modules import. The rest of the folder can be deleted after both runs
  without touching a caller.
- `gate.rs`, `events.rs` and `db.rs` never name `Shas` (the ban in `adr.md`).
- Only `push.rs` writes to GitHub, and only from the approved run. The dry run and the plan step write the
  scratch copy and the plan file, nothing else.
- `card::forbidden` is not loosened. No card text ever contains a force push; the card runs drover's verb.

## A repo, as a statechart

Read from origin's refs against the plan file, never from memory, so a killed run is seen for what it is.

```text
  [never]  no plan
     │  --cut --thread: mirror, plan, tree check, card
     ▼
  [planned]  plan.json; every origin ref at the plan's old value
     │  card approved → --push <tip>            card denied or expired: stays [planned];
     │                                          the same command posts a new card
     ▼
  [archived]  <repo>-history holds every old ref, read back equal, private
     │  one atomic push, a lease on every ref   refused by GitHub: nothing moved, stays [archived]
     ▼
  [pushed]  every origin ref at the plan's new value; log not yet told
     │  append `compacted` records (one transaction)
     ▼
  [compacted]  ──  PR bodies, clone and lane folders reset (each safe to repeat; --reset for stragglers)
     │                                          the owner checks, then `drover resume`
     │  --undo --thread → card → --undo --push <old tip>
     │  only while every ref is still at the plan's new value
     ▼
  [undone]  origin as before; `compact-undone` logged; clones reset back; the archive stays

  any state, a ref at neither value  →  COMPACT_MOVED, nothing done
```

`--cut` on a repo in `[pushed]` finishes it (records, reset) without a new card: nothing remote is left to do.
`--cut` on a repo in `[compacted]` prints the result and exits 0.

## A ref, by class

```text
  line        the integration branch                     → the new chain's tip            tree checked
  same tree   tip is a line commit after the cut          → that commit's new one          tree checked
              (release branches)
  carried     has commits of its own, and has met the     → a copy on its true base        tree checked
              line since the cut (open PRs, main, wip)
  detached    has commits of its own, last met the line   → a copy with no base: files      tree checked
              before the cut (old wip)                       kept, git refuses to merge it
  merged      tip is merged history, off the line         → its holder, or deleted (question 2)
              or before the cut
  notes       refs/notes/*                                → rebuilt: notes after the cut on their new commits
```

## An open issue across the compact

Compact adds no issue state. To drive, a carried PR is a PR whose head moved.

```text
  ready, PASS at old head      → no PASS at the new head → the check lane spawns → PASS → merge
  impl or fix, lane mid-work   → its folder is reset to the same files on the new head; it goes on
  review or verify in flight   → not possible: compact refuses while a lane works
  design-review, blocked,      → unchanged; the branch moved under it
  needs-human, parked
```
