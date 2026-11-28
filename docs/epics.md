# Epics (#1786)

An epic is a parent issue whose child issues drover tracks, so a feature in several parts shows `k/n merged` and
lands once. The decision and its reasons are in [architecture/epics](architecture/epics/README.md).

```sh
drover epic acme 24 add 11 12 13          # the owner or an orchestrator; lanes are refused ORCHESTRATOR_ONLY
drover queue acme add 15 --epic 24        # queue and link in one step
drover epic acme 24 rm 13                 # unlink (it stays queued)
```

- The list is kept as `epic` events (no issue number), so the parent never gets a state and is never queued or
  spawned (`EPIC_PARENT`). A child is in at most one open epic (`EPIC_CHILD`); an `add` after which no child would be
  open is refused (`EPIC_DONE`). Adding a child twice, or removing one that is not there, logs nothing.
- Each child is classed by its own issue's state: merged (with its PR), dropped (closed unmerged) or open (its state,
  `queued` or `not queued`). `k` counts the merged, `n` the children not dropped: `2/2 merged · 1 dropped`.
- The Queue (page and terminal Board) shows each open epic first in its repo card: a head row with `k/n merged`, the
  children indented, and a fold link. `drover waiting` and the compact board print one `epic …` line per open epic;
  `GET /api/v1/queue` carries them as each card's `epics`.
- When no child is open and one or more merged, drover logs one pushed `epic-landed` on the parent (once, through
  the #509 action log), comments on the parent with each child's PR and the dropped ones, and closes it. It runs
  after every merge and in every drive pass. The thread row is a `merged` row keyed `epic-<fleet>-<parent>` with
  an `epic` extra; the last child's own merge push is sent too.
- An epic whose children are all dropped stays open for the owner to close. A parent closed by hand ends the epic
  with nothing said. `queue block <n> --on <parent>` waits for the whole epic.
