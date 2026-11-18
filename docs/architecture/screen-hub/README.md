# screen-hub: each screen built once per change, row ops on one stream per device, serve on tokio (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache, and send each phone only the rows that
changed, moved, came or went (row-by-row builds would save 8 ms of the 150). The renderer core (typed blocks, row
diff, stream protocol, generated schema, TS and Swift types) is built in the new blocks repo; drover pins one commit.
Slices: in blocks, the crate, then the generated types and the page's stream and patch code. In drover: serve on
tokio, the shared cache, one stream per device, patching in place (one patch per 100 ms), lane tails, the gates.
Open questions: the HTML renderer and the theme move after #1318, not in it? Line cap +700? #1313's Queue bar 70 ms?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the pinned crate, the server and the page read after the change, and the tests, per repo |
| `adr.md` | the question, the options with their numbers, the decision, the blocks repo (what #1318 builds there, what waits), the tokio order, what topcoat's shards taught, consequences, slices, the `.drover/arch.toml` lines |
| `shapes.md` | the blocks repo's layout and pin, its types (typed blocks, ops, the device-stream protocol and its resume), the cache key, what makes a key stale, the lane tail, every bound |
| `boundaries.md` | which repo, crate and module may call which, what runs on the runtime, and five statecharts: a cache entry, the stream on the page, a view's patches, the stream on the server, a lane's tail |
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
jirathip-dev/blocks, one commit pinned: the typed blocks, the diff, the ops, the protocol; the generated schema, TS
and Swift types; the page's stream client and patcher. Nothing of drover's in it.
serve: tasks on tokio for sockets and streams; builds and today's handler in spawn_blocking.
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
| The diff and the protocol | inside `screens.rs` | a crate in the blocks repo, pinned; cooee can take the same one |
| A block's shape | hand-written JSON in drover, a hand-written schema in cooee, hand-written Swift | one Rust type per kind; the schema, TS and Swift types generated from it; a server's drift fails a test |
| Memory | a build per stream thread, per event | every cache bounded; under 130 MB over serve's base at worst |

## Why not fully incremental

q2039 prefers it if the numbers allow. Both ways send the phone the same row ops; the difference is the server's
work. The fixture's whole Queue build is 8.5 ms (p95 11.9); one row is 0.07 ms. Incremental would save about 8 ms
an event on the Queue, and 41 ms if all 12 screens were open, out of 150. It would cost a dependency list for
every row: its events, its neighbours in the queue, its blockers, the lane list, 4 to 6 cache files, the config
and the clock. A missed entry is a row that stays wrong. Built whole, a screen is wrong for 30 s at most.

The live Queue build (about 360 ms) is #1313's job: it is repeated reads, which a row built alone would repeat too.

## What #1318 builds in blocks, and what waits

Built there, because #1318 writes it new: the crate, the generator, the stream client and the patcher. Left for
their own issues after #1318: the one HTML renderer (drover's `blocks.ts`, which draws the theme's figures and
words and so moves with the theme), the shared theme package, cooee's switch to the repo, and drover's builders
moving from hand-written JSON to the typed constructors. None of the four is on the 150 ms path.

## Open questions

1. **What waits.** Recommended: the four above are issues after #1318, not part of it. If the HTML renderer should
   move inside #1318, it goes before the device stream and takes the theme's figures and words with it.
2. **The line cap.** drover gains about +620 lines of Rust against 414 of headroom (4 once the open PRs land).
   Recommended: one raise of about 700, filed by the orchestrator, before slice 1. (About 1,000 before q2049.)
3. **#1313's bar for the Queue.** Event to painted in 150 ms leaves a held screen's build 70 ms p95 (25 ms to
   notice, 5 ms to diff and write, about 50 ms for the network and the phone). #1313's gate is 150 ms a build.
   Recommended: #1313 holds the Queue, and each screen a tab keeps open, to 70 ms p95 on the live copy.
