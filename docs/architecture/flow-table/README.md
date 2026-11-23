# flow-table: one transition table drives every flow (#1649)

## Summary

Decision: one table, kept as small data files, says for every state and every observed condition what happens next, who must do it, by when, and what happens when it does not. Drive only runs what a row says; `drover waiting` and `drover diagram` print the same rows. A test fails any state and condition with no row. Only a drive pass acts on a row; the API process of #1645 records the owner's taps and never acts. Each fact has one source. Host load is its own fact: only a host's own load makes it loaded, a late answer from it reads loaded and not offline, and a host silent past its limit is offline whatever the load. The orchestrator answers a row once; the next time, the owner gets the card.
Slices: 1 the table and its test; 2 one reader per fact, the hosts' load sample, #1652 fixed; 3 the engine in shadow beside today's drive (actions, tries, deadlines and escalation timing compared), with a disagreement report; 4 the escalation ladder, built but off until cut-over; 5 cut-over, the side flows and the five folded bugs; 6 delete the old paths: the stall detector becomes row timeouts and the state list is generated.
Open questions: (1) accept one added fact (the lane's own turn report) and five added values? (2) parked and paused rows run no clock: accept? (3) two #1597 fixes the table does not cover ride in slice 5: accept? (4) six slices as six issues, filed on approve? (5) rows are acted on in the service's drive pass, not in the API process, whose lock guards outside writes only: accept?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read after the change: drive, waiting, diagram, a new row, the test |
| `adr.md` | what the revision changed, the question, options, decision, consequences, shadow week, cut-over, slices, the folded bugs, open questions, the `.drover/arch.toml` lines |
| `facts.md` | the closed fact list: each fact's one source, each value, how it is sampled, what #1567 and #1645 change |
| `shapes.md` | the row format, the types (with `Pass`, who may act), the clocks, the new events and `[limits]` keys |
| `tables.md` | every flow's full table |
| `boundaries.md` | which module may call which; which process runs what, today and under #1645; three statecharts: a row's life, the escalation ladder, shadow to cut-over; who classes a shadow disagreement |

## The design on one screen

```
 one source per fact                           the table (data)                 one place that acts
 ───────────────────                           ────────────────                 ───────────────────
 lane   ◀ shed, as the view holds it      ┐                                         (in a drive pass or kick,
 host   ◀ the host's frames (view row)    │    crates/core/flow/                     under its lease)
 pr     ◀ GitHub, through the PR cache    ├─▶  sample ─▶ (state, condition) ─▶ row ─▶ act ─▶ the log
 verdict◀ the log                         │              one row matches,           do · owner · timeout
 ask    ◀ the log                         │              or CI is red               retries ≤ 2
 turn   ◀ the log (`drover turn`)         ┘                                         ▼ then
                                                                  orchestrator thread ─▶ one owner card
                                                                  (once per row)         [retry] [park]

 host: up · loaded · draining · offline. Loaded is the host's own sample. Late from a loaded host is loaded,
 not offline; silent for host_offline_minutes is offline, whatever the load.
```

| Today | After |
|---|---|
| `waiting.rs` builds a sentence per issue; `drive.rs` matches on the sentence's words to pick a step | both read the same row; no decision reads a display line |
| `STATES` lists the ways out of a state, for the chart only | the chart, the docs and `STATES` are generated from the rows drive runs |
| Seven retry counters, each its own rule (respawns, pickups, nudges, heals, re-prompts, done refusals, start failures) | one counter per row and episode: a first try and two retries |
| Nine kinds of card reach the owner directly | every card goes to the issue's orchestrator thread first, which answers a row once, then one owner card naming the row |
| One slow probe under load marks a disk "not answering" or a host offline | load is a fact of its own; "gone" needs evidence of absence, and a late answer keeps the last value, for at most `host_offline_minutes` |
| `answer` types the decision and `extend` spawns, from whichever process ran the verb | a row is acted on only in a drive pass or kick, under its lease; a verb or a tap (the API host's, under #1645) logs its event and kicks |
| A state and a condition nobody wrote a branch for waits forever; `undriven.rs` notices after 30 min | the build fails until the row exists; every row has an owner and a timeout |
| A lane shed calls `done` counts as live in some paths and ended in others | "live" is the lane fact, read in one place |

## Open questions

1. **Facts.** The issue's list has five facts (with `loaded` now among the host's values). Today's drive also decides on the lane's own turn report (`drover turn done|blocked|need-input|working`), so I added it as a sixth, and five values the code already acts on: lane `none` and `limited`, PR `closed` and `merged` (outside drover), ask `none`. Recommended: accept. Without them those paths have no row.
2. **No clock on two rows.** The AC wants a timeout on every row. `parked` and a paused fleet are the owner's own stop. Recommended: those two rows carry the value `owner-held` in place of a clock, and the test pins the exception to exactly them. The alternative is a reminder after N days.
3. **#1597's two fixes outside the table.** A remote worktree's branch tracking `origin/issue-N`, and placement skipping a host under `min_free_gb`, are plain bugs no row covers. Recommended: slice 5 carries them as two commits, since #1597 closes with it.
4. **Six issues.** A shadow week sits between slices 3 and 5, so this cannot be one PR. Recommended: slice 1 is #1649's own PR on this branch; the orchestrator files slices 2 to 4 as issues on approve, and slices 5 and 6 at the end of the shadow week, each blocked on the one before. The AC lines "the PR body names … fixed or moot" then move by amendment: #1652's to slice 2's issue, and those of #1545, #1597, #1625, #1593 and #1611 to slice 5's; each symptom is blocked on its slice.
5. **Which process acts, under #1645.** Its ADR's `api.lock` admits one writable API host: the one process that takes taps and webhook deliveries. It does not guard the log, and its `service` keeps the drive loops with a core link. So `flow::act`, `flow::escalate` and shadow's events run in the service's drive pass and kicks, under drive's own leases, and the API host only logs a tap and kicks (`boundaries.md`). Recommended: accept. If you want every write to go through drover-api, that changes #1645's ADR; here only the place `Pass` is built would move.
