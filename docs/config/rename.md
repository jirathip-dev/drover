# Renaming a fleet (#935)

`drover fleet rename <old> <new>` is a dry run that prints every change it would make. With `--yes` it applies them.
It is owner only: the lane hook refuses it, so it runs as a run card's `--cmd`. It refuses `NOT_PAUSED` unless `<old>`
is paused, `LANE_LIVE` while a lane of an open issue in `<old>` is running, and `BAD_NAME` when `<new>` is taken or
not a valid name. First it backs up state.db, fleet.toml and the per-fleet state files. Then, in one transaction, it
moves the queue, events, asks, verdicts and secrets (their `secret-set` events) to `<new>`, renames the state files
and thread repo bindings, and renames the `[fleet.<old>*]` tables with every value unchanged. Any failure restores
the backup. It logs one `fleet-renamed` event, and old names keep resolving in `status`, `asks`, `board` and
`queue ls`. The fleet stays paused until the owner resumes it.
