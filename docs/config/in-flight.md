# In-flight cap (#1129)

`max_in_flight = N` in `[fleet.<n>]` caps the fleet's PRs in flight (an issue in impl, fix, review, verify or ready,
or one held with an open PR); unset is 8, `0` is off. At the cap drive starts no new issue but keeps reviewing, fixing
and merging; `drover spawn` of a new issue is refused `IN_FLIGHT_CAP` unless `--force`. A start drive admitted holds a
slot until its lane comes or `spawn_pending_max_secs` passes; the refusal names the issues with a lane or a PR, and
counts those starting. Drive gives the free slots to the queue in order; the kicks it launched count no cap again.

## One total for all fleets (#1801)

`[limits] max_in_flight_total = N` counts PRs in flight the same way across every fleet; default 6, `0` is off. The
per-fleet cap still applies and the stricter wins. A first start while the total is full is refused `IN_FLIGHT_CAP`,
naming the total and the issues by fleet; fix, update, review and verify rounds are never held, and nothing is killed
when over it. `[limits] fleet_priority = ["synergy", "drover"]` orders who gets a freed slot: the first listed fleet
with a startable issue, oldest queue entry first; unlisted fleets come after, oldest first. `drover status` and
`waiting` show `in flight N/6` once.
