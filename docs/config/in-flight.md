# In-flight cap (#1129)

`max_in_flight = N` in `[fleet.<n>]` caps the fleet's PRs in flight (an issue in impl, fix, review, verify or ready,
or one held with an open PR); unset is 8, `0` is off. At the cap drive starts no new issue but keeps reviewing, fixing
and merging; `drover spawn` of a new issue is refused `IN_FLIGHT_CAP` unless `--force`. A start drive admitted holds a
slot until its lane comes or `spawn_pending_max_secs` passes; the refusal names the issues with a lane or a PR, and
counts those starting. Drive gives the free slots to the queue in order; the kicks it launched count no cap again.
