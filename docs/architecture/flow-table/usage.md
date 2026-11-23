# flow-table: how the callers read (#1649)

Sketches of the code after cut-over. Names are the ones `shapes.md` defines.

## Drive: a fleet's pass

Today the pass body is 350 lines of `if w.what.starts_with(…)` over `waiting::scan`'s sentences
(`drive.rs:1237-1496`). After:

```rust
let table = flow::table();
let seen = flow::facts::Seen::load(ctx, cfg)?;
let steps: Vec<Step> = open_issues(&events, fleet)
    .map(|s| flow::decide(table, &s, &seen.cond(&s), &flow::Episode::of(&events, &s), ctx.now))
    .collect();
for step in flow::order(steps) {
    flow::act(ctx, cfg, r, out, &step)?;
}
```

- `Seen::load` reads `state.db` and the PR cache once. It calls no host.
- `decide` is pure: same inputs, same `Step`.
- `order` runs records and merges before prompts, and prompts before spawns. One issue's wait never holds
  another issue's action.
- `act` is the only function that turns an `Action` into an effect. Its `match` has no `_` arm.

## `drover waiting`

```rust
for s in open_issues(&events, fleet) {
    let step = flow::decide(table, &s, &seen.cond(&s), &flow::Episode::of(&events, &s), ctx.now);
    rows.push(step.shown());
}
```

```text
herdsman #1588  fix(2)   lane done · turn none · pr open        build/lane-ended  try 2/3
                         → drive: respawn impl (resume)          due in 4m, then orchestrator t212
herdsman #1612  review(1) verdict written · host up              review/record     try 1/3
                         → drive: record the verdict             due in 9m
synergy  #3153  queued   behind #3747 (serial db 1/1)            queued/start      queued
                         → drive: start when #3747 merges
```

Every line names its row, its owner, the next action and its deadline. There is no `→ ?`: a row with no next
action does not load.

## `drover diagram`

```rust
let chart = flow::chart::states(flow::table(), limits, counts);
```

The issue statechart's edges are each row's `to`. `docs/workflow.md` gains nothing by hand, and `flow.md` in `docs/`
(new, generated) prints every table as `tables.md` shows them.

## Adding a row

A new case is one row in one small file. No Rust list changes:

```toml
# crates/core/flow/issue/review.toml
[[row]]
id      = "quiet"
when    = { lane = ["running"], verdict = ["none"], host = ["up", "draining"] }
do      = "wait"
owner   = "lane"
timeout = "check_quiet_minutes"
since   = "heard"
then    = ["respawn-check"]
say     = "reviewing"
```

A new action is one enum variant; the compiler then demands its arm in `flow::act`. A new fact value is one enum
variant; the table test then fails for every state that reads the fact until a row names the value.

## The exhaustiveness test

```rust
#[test]
fn every_state_and_condition_has_one_row_with_an_owner_and_a_timeout() {
    let t = flow::table();
    for flow in &t.flows {
        for state in &flow.states {
            for cond in flow::facts::Cond::every() {
                assert!(state.row(&cond).is_some(), "{}/{}: no row for {cond}", flow.name, state.name);
            }
            for row in &state.rows {
                assert!(row.reachable(state), "{}: no condition reaches it", row.id);
                assert!(row.timeout.is_set(), "{}: no timeout", row.id);
            }
            for (fact, value) in state.reads_values() {
                assert!(state.names(fact, value), "{}: no row names {fact} = {value}", state.name);
            }
        }
    }
    assert_eq!(t.owner_held(), ["issue/parked/parked", "pass/paused/paused"]);
}
```

`owner` is a required enum field, so a row without one does not parse and the test fails at `flow::table()`.
The same test checks that every `timeout` is a `[limits]` key and every `to` is a state of its flow.

## The acceptance test of the engine (slice 3 on)

One fake issue walked through its life against a fake shed and a fake GitHub, asserting at each step the row
`drover waiting` names and the action drive takes:

| Step | Facts set by the fixture | Row | Drive does |
|---|---|---|---|
| queued | no lane | `queued/start` | spawns impl |
| lane ends its turn, no `drover turn` | lane `done`, turn `none`, past `stall_minutes` | `build/lane-ended` | nudges; next expiry respawns; third goes to the thread |
| `turn done` at a pushed head | turn `done`, pr `open`, verdict `none` | `build/pushed` | spawns rev |
| rev writes verdict.md on a remote host | verdict `written` | `review/record` | records it from the view; a pending spawn elsewhere does not delay it |
| the record is refused `NO_REPRO` | verdict `refused` | `review/refused` | re-prompts once, respawns once, then the thread |
| PASS, CI green | verdict `PASS`, pr `green` | `ready/merge` | merges |
| the host goes offline mid-review | host `offline` | `review/host-offline` | waits `host_offline_minutes`, then reviews elsewhere |
