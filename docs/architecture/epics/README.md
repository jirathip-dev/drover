# epics: a parent issue, its children, `k/n merged`, one landed row (#1786)

## Summary

Decision: an epic is a list of child issues under a parent issue, kept as events. Drover never builds the parent,
and every view counts the children from the states it already keeps: merged, dropped (closed unmerged) or open.
When none is open and at least one merged, drover logs one pushed landed row, writes one note on the parent that
links each child's PR, and closes it.
Slices: one PR, three commits: the list with its verb and the queue flag; the views; the landing and contract keys.
Open questions: one. The last merge sends the child's push, then the epic's. Keep both (recommended) or only one?

Read on staging `c73f4b6f4`. No product code here. The screens are drawn in `docs/design/epics/`; this folder says
what feeds them.

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, what the owner decides |
| [adr.md](adr.md) | question, options, decision, consequences, where it departs from the design draft, the slices, the tests, the `arch.toml` lines |
| [shapes.md](shapes.md) | the events, the types, the contract keys, the texts, the refusal codes |
| [boundaries.md](boundaries.md) | which module may call which, the statecharts, the landing step by step |

## The callers, sketched first

### the owner or an orchestrator

```
$ drover epic acme-api 24 add 11 12 13 15 17
acme-api #24 Payments API v2: 5 children (#11 #12 #13 #15 #17)

$ drover queue acme-api add 19 --epic 24          # queue and link in one step
$ drover epic acme-api 24 rm 19                   # a mistake: unlink it (it stays queued)

$ drover waiting acme-api
epic acme-api #24 Payments API v2 · 2/4 merged · 2 in progress · 1 dropped · open #15 #17 (#13 dropped)
acme-api #15 review(1) …
acme-api #17 impl …

$ drover queue acme-api add 30 && drover queue acme-api block 30 --on 24   # starts when the whole epic has landed
```

After the last child merges, with no command from anyone:

```
thread row, pushed     acme-api #24 Payments API v2 landed: 4 issues · 1 dropped
comment on #24         Landed: 4 of 4 issues merged.
                       - #11 Refunds endpoint: PR #26
                       - #12 Currency field on /v1/payments: PR #28
                       - #15 Cursor pagination for /v1/orders: PR #33
                       - #17 Idempotency keys on POST /v1/payments: PR #35
                       Dropped (closed unmerged): #13
#24                    closed as completed
```

### the code

```rust
// cli: the verb and the flag are the only writers of the list
Cmd::Epic { fleet, parent, cmd } => writeln!(out, "{}", epic::change(ctx, &cfg, r, &fleet, parent, cmd.op())?)?,
QueueCmd::Add { epic: Some(parent), .. } => { /* epic::check first, then the queue event, then epic::change */ }

// every view: one read, one type
let epics: Vec<Epic> = epic::open(ctx, &events, &states, fleet);
for e in &epics {
    writeln!(out, "{}", e.line())?;                      // waiting, the compact board
}
card.epics = epics;                                      // GET /api/v1/queue
blocks.extend(screens_epics::section(ctx, ev, c, q));    // the Queue: page and terminal Board

// the landing: the same call after a merge and in every drive pass; a second call does nothing
epic::settle(ctx, cfg, r, out, fleet)?;
```

## The design on one screen

```
 epic add / rm ──▶ `epic` events (no issue number: the parent gets no state)
                          │ replay
                          ▼
 each child's own state ─▶ Epic { parent, title, children: Merged{pr} | Dropped | Open{state} }
 (merged, closed, live)         │                       │
                                ▼                       ▼ no child open, one or more merged
              Queue section · board line ·        epic::settle, once:
              waiting line · queue JSON             1 `epic-landed` (issue = parent, pushed)  ─▶ thread row
                                                    2 one comment on the parent
                                                    3 `gh issue close`
                                                  the parent now reads `merged`, like any landed issue
```

| Today | After |
|---|---|
| Related issues are linked by blockers and prose | A list under a parent, changed by one verb, kept as events |
| No count | `k/n merged` on the Queue, the board and `drover waiting`, from the states drover already keeps |
| The owner learns of each merge alone | One more row and push when the last one lands |
| The parent is closed by hand | Closed by drover with one comment linking each PR |
| A merged issue's PR number is gone from the log after 14 days | Kept in the issue's saved state |

## What the owner decides

1. **Two pushes or one on the last merge.** Recommended: two. The child's push is that issue's own news and is
   sent before the epic is looked at. One push is cheap if you want it: the last child's `merged` event carries a
   flag that keeps it off the phone (it still reaches the thread and other notifiers). Say so and it is in.

Decided here, each argued in `adr.md`: a dropped child leaves the count (`2/2 merged · 1 dropped`); an epic with
every child dropped stays open for the owner to close; the landed epic is not on "done today"; only the owner and
orchestrators change an epic.
