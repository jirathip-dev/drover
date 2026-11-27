# fleet.toml: the flow table's shadow week (#1649, #1655)

`[fleet.<n>] flow` picks how the flow table (`docs/flow.md`) runs beside today's drive. Until the cut-over (slice 5)
only today's drive acts, asks and raises cards in every mode.

| Value | What it does |
|---|---|
| `"shadow"` (default) | after each pass or kick, under its lease, the table decides every open issue of the pass and logs passive events marked `"shadow": true`: `row-entered`, `row-tried` (a deadline passed, or today's drive ran the same action and it failed, classed by `crates/core/flow/outcomes.toml`) and `row-escalated` (the level the ladder would be at: three tries, then the orchestrator, after `escalate_minutes` the owner). Each difference with today's drive is one `flow-disagree`: `action` (owner or action in one pass) or `escalation` (one side reached a person and the other did not, or both more than `act_minutes` apart or at another level), once per issue, row and episode |
| `"on"` | logs as `"shadow"` until slice 5 builds the acting engine |
| `"legacy"` | no shadow run |

`drover flow report [FLEET] [--days 7]` groups the disagreements by kind, row and today's step or `needs-human` code:
count, first and last seen, the median lead or lag of the table's escalation, and three example issues. The shadow
week's report goes into slice 5's issue, where every group gets a class (`row fixed` or `today's bug`).
