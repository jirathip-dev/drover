# flow-table: one transition table drives every flow (#1649)

## Summary

Decision: one table, kept as small data files, says for every state and every observed condition what happens next, who must do it, by when, and what happens when it does not. Drive only runs what a row says; `drover waiting` and `drover diagram` print the same rows. A test fails any state and condition with no row.
Slices: 1 the table and its test; 2 one reader per fact; 3 the engine in shadow beside today's drive, with a disagreement report; 4 the escalation ladder (two retries, the orchestrator thread, one owner card); 5 cut-over, the side flows and the five folded bugs; 6 delete the old paths: the stall detector becomes row timeouts and the state list is generated.
Open questions: (1) accept one added fact (the lane's own turn report) and five added values? (2) parked and paused rows run no clock: accept? (3) two #1597 fixes the table does not cover ride in slice 5: accept? (4) six slices as six issues, filed on approve?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read after the change: drive, waiting, diagram, a new row, the test |
| `adr.md` | the question, options, decision, consequences, shadow week, cut-over, slices, the folded bugs, open questions, the `.drover/arch.toml` lines |
| `facts.md` | the closed fact list: each value, its one source, how it is sampled |
| `shapes.md` | the row format, the types, the new events and `[limits]` keys |
| `tables.md` | every flow's full table |
| `boundaries.md` | which module may call which, and three statecharts: a row's life, the escalation ladder, shadow to cut-over |

## The design on one screen

```
 one source per fact                           the table (data)                 one place that acts
 ───────────────────                           ────────────────                 ───────────────────
 lane   ◀ shed, through the link's view   ┐
 host   ◀ the link's view, [hosts]        │    crates/core/flow/
 pr     ◀ GitHub, through the PR cache    ├─▶  sample ─▶ (state, condition) ─▶ row ─▶ act ─▶ the log
 verdict◀ verdict.md in the view, the log │              one row matches,           do · owner · timeout
 ask    ◀ the log                         │              or CI is red               retries ≤ 2
 turn   ◀ the lane's `drover turn`        ┘                                         ▼ then
                                                                          orchestrator thread ─▶ one owner card
                                                                                                 [retry] [park]
```

| Today | After |
|---|---|
| `waiting.rs` builds a sentence per issue; `drive.rs` matches on the sentence's words to pick a step | both read the same row; no decision reads a display line |
| `STATES` lists the ways out of a state, for the chart only | the chart, the docs and `STATES` are generated from the rows drive runs |
| Seven retry counters, each its own rule (respawns, pickups, nudges, heals, re-prompts, done refusals, start failures) | one counter per row and episode: a first try and two retries |
| Nine kinds of card reach the owner directly | every card goes to the issue's orchestrator thread first, then one owner card naming the row |
| A state and a condition nobody wrote a branch for waits forever; `undriven.rs` notices after 30 min | the build fails until the row exists; every row has an owner and a timeout |
| A lane shed calls `done` counts as live in some paths and ended in others | "live" is the lane fact, read in one place |

## Open questions

1. **Facts.** The issue's list has five facts. Today's drive also decides on the lane's own turn report (`drover turn done|blocked|need-input|working`), so I added it as a sixth, and five values the code already acts on: lane `none` and `limited`, PR `closed` and `merged` (outside drover), ask `none`. Recommended: accept. Without them those paths have no row.
2. **No clock on two rows.** The AC wants a timeout on every row. `parked` and a paused fleet are the owner's own stop. Recommended: those two rows carry the value `owner-held` in place of a clock, and the test pins the exception to exactly them. The alternative is a reminder after N days.
3. **#1597's two fixes outside the table.** A remote worktree's branch tracking `origin/issue-N`, and placement skipping a host under `min_free_gb`, are plain bugs no row covers. Recommended: slice 5 carries them as two commits, since #1597 closes with it.
4. **Six issues.** A shadow week sits between slices 3 and 5, so this cannot be one PR. Recommended: slice 1 is #1649's own PR on this branch; the orchestrator files slices 2 to 6 as issues on approve, each blocked on the one before. The AC line "the PR body names each of #1545, #1597, #1625, #1593, #1611 fixed or moot" then moves to slice 5's issue by amendment, and those five are blocked on it.
