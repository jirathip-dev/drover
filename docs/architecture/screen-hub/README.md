# screen-hub: each screen built once per change, row ops on one stream per device, serve on tokio (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache, and send each phone only the rows that
changed, moved, came or went (row-by-row builds would save 8 ms of the 150). #1318 is drover's side only (q2071):
the diff, the stream protocol and the page's patch code come pinned from the blocks repo; #1318 builds none of them.
Slices: seven, all in drover. Serve on tokio, the shared cache and the lane tail cache start now. The pin and the
device stream wait for blocks#4; the page's one stream and its patching wait for blocks' renderer; then the gates.
Open questions: keep drover's own drawing code until blocks' renderer replaces it? Count #1318 in the cap raise?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how serve and the page read after the change, slice by slice, and every test with its slice |
| `adr.md` | the question, the options with their numbers, the decision, what #1318 takes from blocks and which issue brings it, the tokio order, what topcoat's shards taught, consequences, slices, the `.drover/arch.toml` lines |
| `shapes.md` | what serve builds (the cache key, what makes a key stale, the hub, the device-stream protocol and its resume, the lane tail, every bound), then what it calls from blocks (the ops, the frames, both ends' bookkeeping) |
| `boundaries.md` | which module may call which, what runs on the runtime, and five statecharts: a cache entry, the stream on the page, a view's patches, the stream on the server, a lane's tail |
| `facts.md` | what was measured, how, and what the code does today |

## The design on one screen

```
the log, the snapshot, background reads,          one publisher                   one stream per device
config, gh cache files, the clock, lane tails  ──>  builds each stale screen  ──>  GET /stream: frames {key, from, rev, ops}
        (7 signals, checked every 25 ms)            once, keeps its JSON,          POST /stream/<sid>: open and close keys,
                                                    diffs it into row ops          answered with each key's state
                                                         │
                                          GET /screens/<x> reads the same JSON     the page patches the named rows,
                                                                                   once per 100 ms, reading position kept
serve: tasks on tokio for sockets and streams; builds and today's handler in spawn_blocking.
from <org>/blocks, one commit pinned, built there by its own issues: the diff, the ops, the frames, both
ends' bookkeeping, the page's stream client and patcher.
```

| | Today | After |
|---|---|---|
| A new event | every stream rebuilds its own screen (Queue: 8.5 ms, 13.5 MB allocated) | one build per open screen, for all phones (all 12 screens: 42 ms) |
| Noticing it | each stream polls every 500 ms | one check every 25 ms; at once for serve's own writes |
| Streams | 4 for the whole server, then 503 | 1 per page, 4 per device, no server-wide cap |
| A navigation | 2 builds: the GET and the stream's first message | 1 request, 0 builds when the screen is fresh; the last copy paints first |
| A delta on the phone | copy the model, render every block, rewrite localStorage | patch the rows the ops name, at most once per 100 ms |
| A row that changes place | its whole card is replaced | one `move`; the row's node follows it |
| Rows arriving above what is being read | the screen is redrawn | the reading position stays |
| The cards | never get a change (their diff is always empty) | `insert`, `merge` and `remove` like any screen |
| Sheets | fetched once, no stream | subscribe like a tab |
| A lane sheet | a GET that reads up to 5,000 lines from shed | opens from the phone's copy; the tail cache streams new lines; `[older]` pages back |
| A dropped stream | the whole screen again | the missed ops, by the revs the page holds |
| serve | a thread per connection, up to 256 | tasks on tokio; at most 64 blocking threads |
| The diff and the protocol | inside `screens.rs` | the blocks crate, pinned; drover keeps no copy |
| Memory | a build per stream thread, per event | every cache bounded; under 130 MB over serve's base at worst |

## Why not fully incremental

q2039 prefers it if the numbers allow. Both ways send the phone the same row ops; the difference is the server's
work. The fixture's whole Queue build is 8.5 ms (p95 11.9); one row is 0.07 ms. Incremental would save about 8 ms
an event on the Queue, and 41 ms if all 12 screens were open, out of 150. It would cost a dependency list for
every row: its events, its neighbours in the queue, its blockers, the lane list, 4 to 6 cache files, the config
and the clock. A missed entry is a row that stays wrong. Built whole, a screen is wrong for 30 s at most.

The live Queue build (about 360 ms) is #1313's job: it is repeated reads, which a row built alone would repeat too.
#1313's PR, in review, measures 36 ms p95 on a fixture shaped like live; this design needs 70 ms or less.

## What waits for blocks, and what does not

| Slice | Needs from blocks | The owner gets |
|---|---|---|
| 1 serve on tokio | nothing | no visible change; at most 64 blocking threads |
| 2 the hub | nothing | one build per change for every phone; no stream cap, no 503; a fresh screen from the cache |
| 3 lane tails, the server's half | nothing | a lane sheet opens from the cache, remote lanes too |
| 4 the pin | blocks#4: the crate | rows that move as one op; live cards |
| 5 the device stream, the server's half | (slice 4) | the server ready for one stream per device |
| 6 the page: one stream, patched in place | blocks' stream client and patcher (a blocks#2 slice, no issue yet) | row patches, the reading position kept, sheets and a lane's tail live |
| 7 gates and docs | (slice 6) | the 150 ms numbers held in CI |

blocks#4 is queued behind blocks#1 (in impl) and blocks#2 (its ADR in revision). #1318 opens no PR in blocks and
writes no diff, frame or patch code of its own: if the pin lacks something, the fix is a blocks issue.

## Open questions

1. **Whose code draws a block after slice 6.** Recommended: the page takes blocks' stream client and patcher and
   keeps drover's own drawing code (`web/src/blocks.ts`) until blocks#2's migration replaces it in its own issue.
   Drawing through blocks' renderer inside #1318 would put slice 6 behind blocks' HTML renderer and theme as well,
   and change pixels in an issue that should change none.
2. **The line cap.** 53,359 of 53,500 today, and the open PRs already project 210 over. #1318 adds about 620.
   Recommended: the one raise the orchestrator files counts #1318 in (to about 54,900), before slice 1 starts.
