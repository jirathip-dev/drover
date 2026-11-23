# Asks, answers and verdicts from another host (#275, #389)

A lane on another host never connects to the main host: it holds no key, token or ssh route to it. Spawn exports
`DROVER_OUTBOX=1`, and its `drover ask`, `answer`, `verdict`, `done` and its own events (`hook-denied`,
`coauthor-trailer`) are appended to `<worktree>/.drover/outbox.jsonl` (exit 0: `queued for the main host: drive
delivers it within 1 min`). #1309: each drive step makes one call per host, `drover hosts report` over main → host
ssh (the main host's own in this process): its shed sessions and each live lane worktree's head, uncommitted state,
verdict.md and outbox lines. The main host keeps the last report as the view in `state.db`; drive, check, `waiting`,
`status` and the board read lane facts from it, never over ssh (docs/architecture/drive-push/). Each outbox line runs
as that lane (from its spawn event, never the line), once per lane and timestamp; a line for another issue or role, or
a verb a lane doesn't run itself, is refused and logged `outbox`. `check` raises `outbox-stale` once when a reported
line has not run for over 5 min. So the main host's log is the only one written and only the main host pushes.
That host's own log (#344: its own `state.db`) is never written by a lane. Each
`check` imports the asks a lane left in a remote host's own log (lanes started before this; read through its
`drover events --jsonl`, or its `events.jsonl` on an older build) under a new id, with `from` naming the old one.

An answer reaches a remote lane through that host's shed (`send`, then Enter).
