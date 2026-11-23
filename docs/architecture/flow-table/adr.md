# ADR: flow-table — one transition table drives every flow (#1649)

## Summary

Decision: one table, kept as small data files, says for every state and every observed condition what happens next, who must do it, by when, and what happens when it does not. Drive only runs what a row says; `drover waiting` and `drover diagram` print the same rows. A test fails any state and condition with no row.
Slices: 1 the table and its test; 2 one reader per fact; 3 the engine in shadow beside today's drive, with a disagreement report; 4 the escalation ladder (two retries, the orchestrator thread, one owner card); 5 cut-over, the side flows and the five folded bugs; 6 delete the old paths: the stall detector becomes row timeouts and the state list is generated.
Open questions: (1) accept one added fact (the lane's own turn report) and five added values? (2) parked and paused rows run no clock: accept? (3) two #1597 fixes the table does not cover ride in slice 5: accept? (4) six slices as six issues, filed on approve?

## Status

Proposed (architect lane, 2026-11-23, staging `7c0a28c1c`). Read `usage.md` first, then this, `facts.md`,
`shapes.md`, `tables.md`, `boundaries.md`.

## Question

About 26 of the 167 issues filed since 2026-11-19 are a step with no next step. How is drive built so that a
state plus an observed condition can never be without an actor, a deadline and a way up to a person, and so that
what drive does, what `drover waiting` says and what `drover diagram` draws cannot differ?

## Where it stands

- **The decision is spread over three places that agree by string.** `waiting::waits` and `scan_where`
  (`waiting.rs:345-1002`) build one sentence and one command string per issue. `drive::pass`
  (`drive.rs:1237-1496`) then picks its step by `w.what.starts_with(…)`, `w.what.contains(NO_PUSH)` and
  `cmd.ends_with("--update")`. `check` holds a third copy of the stall and ready rules (`check.rs:306-468`).
  A sentence nobody matches is a step nobody takes.
- **`STATES` (`events.rs:1100`) is drawn, not run.** It has ways out but no conditions, so "lane done", "host
  offline" or "verdict written on another host" appear nowhere in it.
- **Seven retry counters, seven rules.** Respawns (`waiting::respawns`, 2 in 30 min), verdict pickups
  (`PICKUP_TRIES` 3), turn nudges (once, 10 min), reviewer heals (once a round), refusal re-prompts (once), done
  refusals (3 at a head), failed starts (`respawn_max` in a window). Each ends in its own `needs-human` code or
  its own ask, or in nothing: `TURN_GO_ON` prompts a lane once and then waits for ever (#1597).
- **"Live" has several meanings.** `Sessions::running` counts a lane shed calls `done` as running; `busy`
  does not. Build trim keeps the folders of the first (`check::live_lanes`); the stall rule reads the second.
- **The safety net works after the fact.** `undriven.rs` raises a stall once an issue has had no action for 30
  min and two passes. It says that nothing drives the issue, not what should.
- **Cards go straight to the owner** from fourteen `needs-human` emitters in ten files (`drive.rs`, `stall.rs`,
  `pickup.rs`, `pending.rs`, `spawn.rs`, `gate.rs`, `done.rs`, `design.rs`, `breaker.rs`) and `respawn.rs`'s ask;
  only lane asks are routed to a thread first.

## Options

| Option | Why not |
|---|---|
| 1. Keep the code paths; widen the stall detector and add a test per reported gap (what #1580, #1612 and #1594 did) | each fix adds a branch and a counter; the next unmatched sentence is found by the owner again |
| 2. A typed state machine: `match (state, cond)` in Rust, the compiler checks every arm | total, but it is a code path: `waiting` and `diagram` would need a second list kept in step by hand, every change lands in one large `match`, and the owner cannot read it |
| 3. The rows as Rust `const` tables | typed at compile time, but needs a module list (a new central file), counts as `src/` against the line cap, and is still read by programmers only |
| **4. The rows as data files in a folder, loaded into closed Rust enums, checked by one test (chosen)** | |
| 5. A workflow-engine crate | a new dependency for what is one lookup and one counter |

Smaller choices:

- **First match, per file.** Rows in one state's file are tried top to bottom. Disjoint rows would need no order
  but triple each row's length. Order is local to a file of at most 16 rows, and the test fails a row nothing reaches.
- **Tries counted per episode, not per stay.** An issue that flips between two rows (a lane dies, is respawned,
  dies) would otherwise restart its count each time: a silent loop. An episode ends only on real progress
  (`shapes.md`).
- **Limits, blockers and room are outcomes, not facts.** A round limit, a serial group, a full host come back
  from the action as `Final`, `Queued` or `Held`. Making them facts would square the table for no new behaviour.
- **States are today's fold states.** Shadow can then compare like with like, and `STATES` can be generated.
  `blocked` gets rows for old logs, and no row leads to it after cut-over.

## Decision

1. **The table.** `crates/core/flow/<flow>/<state>.toml`, one file per state, joined by `build.rs` and embedded.
   A row is `when` (a value set per fact) → `do`, `owner`, `timeout`, optional `then`, `since`, `escalate`,
   `to`, and the line to show (`shapes.md`). The content is `tables.md`: twelve lifecycle states in ten files,
   and eight side flows (ask, card, handoff, rollout, cap, pass, slot, host).
2. **Facts.** Six closed enums, each with one reader and one source: lane (shed, through the link's view), host
   (the view and `[hosts]`), pr (the PR cache), verdict (the log and the view's `verdict.md`), ask (the log),
   turn (the lane's `drover turn`). One sample per pass, from `state.db`, the PR cache and the log only; no
   host is called to decide (`facts.md`).
3. **The engine.** `flow::decide(table, state, cond, episode, now)` is pure and returns a `Step`. `flow::act`
   is the only code that turns an `Action` into an effect, in one `match` with no wildcard arm, and logs
   `row-entered` and `row-tried`. Drive's pass is sample, decide, order, act. Records and merges run before
   prompts, prompts before spawns, and one issue's wait never holds another's action. `waiting` prints the
   `Step`. `diagram` draws the rows' `to` edges (`usage.md`, `boundaries.md`).
4. **Escalation.** Every row gets a first try and two retries. Then the issue's orchestrator thread gets one
   ask and acts if a spec line or a past decision settles it. After `escalate_minutes`, or when it passes, the
   owner gets one card that names the row, with [retry] and [park]. Every card kind takes this path:
   needs-human, one more round, cap, respawn loop, lane ask. Design and ADR approval and owner topics go to the
   owner directly. `flow::escalate` is the only code that raises a card.
5. **The exhaustiveness test.** One test in `crates/core` walks every state of every flow against every
   condition (12,600 per lifecycle state) and fails when no row matches, when a row is unreachable, when a
   state reads a fact and no row names one of its values, or when a timeout is not a `[limits]` key. `owner`
   and `timeout` are required fields: a row without one does not load. Exactly two rows may carry `owner-held`
   in place of a clock (`usage.md`).
6. **Shadow mode.** From slice 3 each pass also runs `decide` and compares it with what today's drive did for
   the same issue: the owner, the action and the role. A difference is one passive `flow-disagree` event.
   the new verb `flow report [fleet] [--days 7]` groups them by row and old step, with a count, first and last seen
   and three example issues.
7. **Cut-over.** After 7 days, slice 5's PR body lists every disagreement class as `row fixed` (with the commit)
   or `today's bug` (with the issue it explains). None may be left unmarked. Slice 5 then makes `on` the
   default; `[fleet.<n>] flow = "legacy"` puts one fleet back. After 7 more days with no fleet on `legacy`,
   slice 6 deletes the old paths and the key.

Disagreements the table makes on purpose, marked `today's bug` in advance:

| Table | Today |
|---|---|
| a lane shed calls `done` with no turn report is nudged once, respawned once, then carded | prompted once, then nothing (#1597) |
| a written verdict is recorded whatever spawns are pending | waits behind admission (#1612, #1625) |
| a start slot older than its limit is dropped | holds every start (#1625) |
| a same-name session that is done or exited is replaced by the new round | the round waits at `starting` (#1545) |
| a merged issue holds no serial slot | it holds one for good (#1593) |
| an ask whose episode ended closes | it stays an owner card (#1611) |
| every card goes to the thread first | most go to the owner |
| three tries, everywhere | two to five, by path |

## Consequences

- **No `→ ?`.** Every open issue shows its row, owner, next action and deadline. A gap is a red build, not a
  stalled issue.
- **Behaviour that changes:** the table above; `blocked` is no longer entered; `needs-human` codes become row
  ids (the old `code` stays in the event); the waiting lines are the rows' `say` text, so tests that assert the
  old sentences are rewritten in slice 5; alerts `stall`, `orphan-issue`, `ready-unmerged`, `promise-missed`,
  `spawn-held`, `host-busy` and `drive-stalled` are replaced by `row-escalated`.
- **API v1 keeps `Waiting`'s shape** and gains `row`, `try`, `deadline` and `level`. `who` is mapped from the
  row's owner.
- **Six new `[limits]` keys, three constants and `promise_passes` gone** (`shapes.md`).
- **`src/` first grows, then shrinks.** Slices 1 to 5 add about 1,250 lines (`flow/`, with shadow) while the old
  paths still stand. Slice 6 removes about 2,000 (`waiting.rs`'s ladder, `drive.rs`'s pass body, `stall.rs`,
  `pending.rs`, `promise.rs`, `respawn.rs`, `undriven.rs`, check's issue rules, `flow::shadow`). The rows are
  outside `src/`. `drover cap` will show whether one raise is needed before slice 3.
- **Adding behaviour is adding a row.** Two PRs that touch different states touch different files.
- **The orchestrator thread becomes a required hop.** A fleet with no orchestrator role skips it: the try fails
  and the card goes to the owner. #1609 builds on the level this logs (in-scope judgment, quiet decisions, the
  digest, the board's `with orchestrator`).
- **Risk: a wrong row acts wrongly everywhere at once.** That is what the shadow week and `flow = "legacy"` are
  for. `decide` being pure also lets every reported case become a three-line test: state, condition, expected row.
- **The shed move (#1567) changes two readers**, `facts::lane` and `facts::host`, and no row.

## Slices

Each is one PR, green alone. A shadow week sits between 3 and 5.

1. **The table and its test.** `crates/core/flow/*`, `flow::table`, the six enums, the exhaustiveness test,
   `flow.md` in `docs/` generated, `drover diagram`'s statechart from the rows. A test proves the rows' `to` edges
   cover every edge of today's `STATES`. No behaviour change. This branch.
2. **Facts.** `flow::facts` with `Seen::load` and one reader per fact; `flow::episode`; `waiting --flow`
   prints state, condition and row beside today's line. No behaviour change.
3. **Shadow.** `flow::decide`, `flow::shadow`, `flow-disagree`, `flow report`, `[fleet.<n>] flow`
   defaulting to `shadow`, the six `[limits]` keys. The acceptance walk of `usage.md` runs against `decide`.
4. **Escalation.** `flow::escalate`: the ladder, the thread routing, the owner card with [retry] and [park],
   `row-escalated`, `row-retry`. Used by `legacy` cards at once, so the routing is live before cut-over.
5. **Cut-over.** `flow::act` and `flow::order`; drive's pass, `waiting` and `check` read the `Step`; the side
   flows; `outcomes.toml` replaces the refusal lists; slot holders counted from `in_flight`; `on` the default.
   The PR body carries the shadow report and names #1545, #1597, #1625, #1593 and #1611 fixed or moot.
6. **Delete.** The old pass body, `waiting.rs`'s ladder, `stall.rs`, `pending.rs`, `promise.rs`, `respawn.rs`,
   `flow::shadow`, the `flow` key and `promise_passes`. **`undriven.rs`'s after-the-fact stall is replaced by row
   timeouts, and `STATES` is generated from the table**: `events::STATES` is deleted, the fold's check and
   `diagram` read `flow::chart::states()`.

## The folded symptoms

| Issue | What fixes it | Expected |
|---|---|---|
| #1545 an extended round waits behind a same-name lane in shed state done | every spawn action replaces a session whose lane fact is `done`, `asleep`, `exited` or `gone`; `queued/start`, `build/round-start` | fixed, slice 5 |
| #1597 lanes in shed state done count as live | `build/lane-ended`, `review/record`; trim and clean read the lane fact, so `done` is not live; two commits outside the table (open question 3) | fixed, slice 5 |
| #1625 admission stuck 10 h; ready PRs and verdicts wait too | `slot/held/stale` drops the lease; `flow::order` runs records and merges first; `pass/due/late`; `host/holding/refusing` is its `spawn-stuck` alert. The stale `drive.at` and `drive.lock.step` are files of the pre-#1320 single pass: deleted | fixed, slice 5 |
| #1593 merged issues hold a serial group's slots | holders are the issues whose state file says `in_flight`; the gate and `waiting` call the same function | fixed, slice 5 |
| #1611 a superseded design ask stays an owner card | `ask/orphaned/close` | fixed, slice 5 |

#1609 is not folded here: slice 4 gives it the routing it builds on.

## Open questions for the owner

1. **Facts.** One fact and five values beyond the issue's list: `turn`; lane `none` and `limited`; pr `closed`
   and `merged`; ask `none`. Each is something drive branches on today (`waiting::turned`, `limit::due`,
   `drive::external`). Recommend: accept.
2. **`owner-held`.** `parked` and a paused fleet have no clock, pinned by the test to those two rows. Recommend:
   accept. The alternative is a reminder key, which is a new feature.
3. **#1597's two bugs outside the table** (a remote branch tracking `origin/issue-N`; placement skipping a host
   under `min_free_gb`). Recommend: two commits in slice 5, so #1597 closes with it.
4. **Slices as issues.** Recommend: slice 1 is #1649's own PR on this branch; the orchestrator files slices 2
   to 6 on approve, each blocked on the one before, and the "names each symptom fixed or moot" line moves to
   slice 5's issue by amendment, with #1545, #1597, #1625, #1593 and #1611 re-blocked on it.

## `.drover/arch.toml` lines

The table is a central list, so it is a folder from the first PR. Slice 1 adds:

```toml
[[collect]]
dir = "crates/core/flow"
glob = "**/*.toml"
```

and appends `"docs/flow.md"` to `generated` (the generator is `just gen`, already listed as `generate`).

The old lists are `events::STATES` and the ladders in `waiting.rs`, `stall.rs`, `pending.rs`, `promise.rs` and
`respawn.rs`. Slice 5 freezes them until slice 6 deletes them, and takes `waiting.rs` out of `shared`:

```toml
frozen = [
    "crates/core/src/waiting.rs",
    "crates/core/src/stall.rs",
    "crates/core/src/pending.rs",
    "crates/core/src/promise.rs",
    "crates/core/src/respawn.rs",
    "crates/core/src/drive/undriven.rs",
]
```

Slice 5 also adds two bans; slice 6 drops their `except` lines with the files they name:

```toml
[[ban]]
pattern = "next_cmd.as_deref()"
paths = ["crates/core/src/**"]
except = ["crates/core/src/flow/shadow.rs", "crates/core/src/waiting.rs", "crates/core/src/drive.rs", "crates/core/src/drive/**"]
reason = "#1649: no decision reads a display line or a command string; the next action is a flow::Step"

[[ban]]
pattern = "what.starts_with("
paths = ["crates/core/src/**"]
except = ["crates/core/src/flow/shadow.rs", "crates/core/src/waiting.rs", "crates/core/src/drive.rs", "crates/core/src/drive/**"]
reason = "#1649: no decision reads a display line; add a row in crates/core/flow/"
```
