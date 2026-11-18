# screen-hub: each screen built once per change, one stream per device (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache; not row by row. On the large fixture a
whole Queue build is 9 ms and every screen together 42 ms, so row-by-row would save 8 ms of a 150 ms budget and need
a map of what every row depends on. Each phone holds one stream and redraws only the rows a change names.
Slices: the shared cache; the one-stream-per-device protocol; in-place patching on the phone; the lane tail cache;
the speed, load and memory gates.
Open questions: the live Queue build is about 360 ms against 9 ms on the fixture: its own issue after slice 1?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the server and the page read after the change, and the acceptance tests |
| `adr.md` | the question, the options with their numbers, the decision, consequences, slices, the `.drover/arch.toml` lines |
| `shapes.md` | the cache key, what makes a key stale, the delta ops, the device-stream protocol and its resume, the lane tail, every bound |
| `boundaries.md` | which module may call which, and four statecharts: a cache entry, the stream on the page, the stream on the server, a lane's tail |
| `facts.md` | what was measured, how, and what the code does today |

## The design on one screen

```
the log, the snapshot, background reads,          one publisher thread            one stream per device
config, gh cache files, the clock, lane tails  ──>  builds each stale screen  ──>  GET /stream: frames {key, from, rev, ops}
        (7 signals, checked every 25 ms)            once, keeps its JSON,          POST /stream/<sid>: open and close keys,
                                                    diffs it into ops              answered with each key's state
                                                         │
                                          GET /screens/<x> reads the same JSON     the page patches the named nodes
```

| | Today | After |
|---|---|---|
| A new event | every stream rebuilds its own screen (Queue: 8.5 ms, 13.5 MB allocated) | one build per open screen, for all phones (all 12 screens: 42 ms) |
| Noticing it | each stream polls every 500 ms | one check every 25 ms; at once for serve's own writes |
| Streams | 4 for the whole server, then 503 | 1 per page, 4 per device, no server-wide cap |
| A navigation | 2 builds: the GET and the stream's first message | 1 request, 0 builds when the screen is fresh; the last copy paints first |
| A delta on the phone | copy the model, render every block, rewrite localStorage | redraw the nodes the ops name |
| The cards | never get a change (their diff is always empty) | `insert`, `merge` and `remove` like any screen |
| Sheets | fetched once, no stream | subscribe like a tab |
| A lane sheet | a GET that reads up to 5,000 lines from shed | opens from the phone's copy; the tail cache streams new lines; `[older]` pages back |
| A dropped stream | the whole screen again | the missed ops, by the revs the page holds |
| Memory | a build per stream thread, per event | every cache bounded; under 100 MB over serve's base at worst |

## Why not fully incremental

The fixture's whole Queue build is 8.5 ms (p95 11.9); one row is 0.07 ms. Incremental would save about 8 ms an
event on the Queue, and 41 ms if all 12 screens were open, out of 150. It would cost a dependency list for every
row: its events, its neighbours in the queue, its blockers, the lane list, 4 to 6 cache files, the config and the
clock. A missed entry is a row that stays wrong. Built whole, a screen is wrong for 30 s at most.

## Open questions

1. The live instance builds the Queue in about 360 ms (the issue's number); the fixture in 9 to 11 ms. The
   acceptance criteria are on the fixture, and the live cause is not measured (the lane hook refused a copy of the
   live state). Recommended: its own issue, queued after slice 1, whose per-screen build times in
   `/api/v1/health` show the slow part.
