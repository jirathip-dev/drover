# drive-push: boats send, fleets loop alone, slow work leaves the pass (#1309)

## Summary

Decision: each boat sends every lane change to the Mini as it happens, over the Mini's own ssh connection kept
open; lane commands wait in today's outbox until the Mini confirms them. Drive reads only what has arrived, each
fleet drives alone, and slow work (spawns, merges) runs beside a pass, never inside it.
Slices: #1308 first; a local view of every host; the watcher and the link; a loop per fleet; passes that only plan.
Open questions: load the service job on the Mini? Accept that the Mini dials the boat? Raise the line cap by
about 600? Delete the unused lanes-over-HTTP path?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read after the change, and the test fixture every step shares |
| `adr.md` | the question, options, decision, what happens when things break, self-heal and SLOs, the steps, open questions, the `.drover/arch.toml` lines |
| `shapes.md` | the frame, ids and dedup, the view's tables, events, leases, `[limits]` keys |
| `boundaries.md` | which module may call which, and five statecharts: link, outbox line, fleet loop, job, start slot |

## The design on one screen

```
 boat                                          Mini
 ────                                          ────
 lane ─▶ its outbox (durable) ─┐
 shed's events ────────────────┼─▶ watcher ══ one ssh call, kept open ══▶ link ─▶ intake ─▶ view + log
 worktree head, verdict.md ────┘   (ends with the call;                 (one per host,        │
                                    a whole report first)                redials)             ▼ wake
                                                                              fleet A: pass ─▶ jobs
                                                                              fleet B: pass ─▶ jobs
                                                                              fleet C: pass ─▶ jobs
```

| Today | After |
|---|---|
| Three pollers call each boat, a fact a call, 0.7 s each | One watcher per boat sends changes; one reader on the Mini |
| A lane's command waits up to a minute for a drain | It arrives in about a second and is acked |
| A pass reads boats while it holds its lock | A pass reads `state.db` only |
| One pass drives every fleet and ends when the slowest does | A pass per fleet, on its own lock and clock |
| An issue's merge build runs inside the pass | It runs in the issue's job; the pass only plans |
| A stuck tick is stopped whole | Each pass, job and link heals alone |
| With the Mini asleep, lines wait in outboxes | The same, and the first frame after waking brings it level |

## Targets

| Target | Where it is proved |
|---|---|
| A lane change is acted on within about 5 s (its job has started), p95 | steps 2 and 5, on a fake remote that costs 0.7 s a call |
| No fleet's pass is over 60 s; it ends itself there | steps 3 and 4 |
| A pass makes no remote read | step 1 on, on a runner that fails one |
| One fleet's slow pass adds no delay to another's | step 3 |
| Nothing lost, nothing run twice across a sleep, a reboot and a split | step 2 |

## What the owner decides

1. Load `drover service` on the Mini at step 2 (a launchd change; without it the design runs at poll speed).
2. The Mini dials and the boat sends, in place of the three transports the issue names.
3. One line-cap raise of about 600 before step 2.
4. Delete lanes over HTTP in step 5.

Each is argued in `adr.md`, with a recommendation.
