# ADR: screen-hub — each screen built once per change, row ops on one stream per device, serve on tokio (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache, and send each phone only the rows that
changed, moved, came or went. Row-by-row builds would save 8 ms of the 150 (a whole Queue build is 9 ms on the large
fixture). The block rules, the row diff and the stream protocol are one small crate of their own; serve moves to tokio.
Slices: the crate; serve on tokio; the shared cache; one stream per device; patching in place on the phone (one patch
per 100 ms, reading position kept); lane tails; the speed, load and memory gates.
Open questions: raise the line cap by about 1,000 (414 left)? Tighten #1313's Queue build bar from 150 ms to 70 ms?

## Status

Revised 2026-11-18 for the owner's amendments q2039 (row ops required, one patch per frame, reading position kept),
q2040 (the renderer core is its own crate) and q2044 (serve on tokio). First version: `e7bf23a0f`, on staging
`9ea69d9fb`. Read `usage.md` first, then this, `shapes.md`, `boundaries.md`; `facts.md` holds the measurements.

## Question

Every open stream rebuilds its whole screen on every event, only 4 streams may exist, a navigation builds twice,
the phone redraws the whole screen on every delta, and serve sits at 716 MB against a 300 MB limit. How does serve
give every screen and sheet fresh data within 150 ms p95, to any number of phones, in bounded memory? And is the
screen rebuilt whole per change, or does each event update only its own rows?

## Where it stands

Numbers are from `facts.md` (large fixture, release).

- One stream is one thread: it polls the log every 500 ms and rebuilds its screen on any event and every 30 s. A
  Queue build takes 8.5 ms and allocates 13.5 MB to produce 20 KB. Streams are capped at 4; the fifth gets a 503.
- The server already diffs two builds into ops by block id (`merge`, `replace`, `remove`, `insert`). One new ask
  is 14 ops, 2 KB. But a row that changes place replaces its whole card, and a bare list (the cards) has no diff.
- The page throws the ops away: on each delta it copies the model, renders every block and rewrites localStorage.
- Sheets have no stream. A lane sheet reads up to 5,000 lines from shed on its GET.
- serve is one thread per connection (up to 256) around a synchronous handler.

## Options

**1. Keep a build per stream, lift the cap.** Ten screens on 3 devices are 30 streams: each event costs 30 builds,
about 100 ms of CPU and 180 MB allocated. Rejected: the cost grows with every phone.

**2. Build once per change, share, diff into row ops (chosen).** One publisher builds each held screen when
something it depends on changed, diffs it against the last build and sends the row ops to every stream that holds it.

**3. Fully incremental screens.** Each event updates only the rows it touches; nothing is built whole.

q2039 prefers option 3 if the numbers allow. Both options send the phone the same row ops; they differ only in the
server's work per event.

| | Option 2: once per change | Option 3: incremental |
|---|---|---|
| what the phone gets for one new ask | 14 row ops, 2 KB | the same |
| Queue after one event | 8.5 ms p50, 11.9 ms p95 (9.9 and 13.6 with the event's fold) | 0.07 ms a row touched, at best |
| all 12 screens after one event | 41.7 ms p50; 61.2 ms as a sum of p95s | under 1 ms, at best |
| the same with ten times the history | Queue 12.4 ms p50, 20.5 ms p95 | the same as above |
| what it saves of the 150 ms budget | | 8 ms on the Queue, 41 ms if all 12 are open |
| what must be right | 7 coarse signals; a miss heals in 30 s | every row's inputs, per row |

Option 3's price is the last line. A Queue row reads its own events, its place in the queue and its neighbours,
its blockers and serial group (other issues), the lane list (from shed, not the log), 4 to 6 cache files (not the
log), the config and the clock. The counts and the badge read every row. Only one of those inputs is the event an
incremental update would start from; a row whose other input moved stays wrong until something else touches it.
The numbers do not ask for that risk: option 2 meets the target with room (25 ms to notice an event, at most 61 ms
to build and diff everything open) and cannot show a stale row for longer than its 30 s clock tick.

**The live instance.** Its Queue build is about 360 ms, not 9. That is #1313's job (in impl): the config parsed 5
times a build, shed listed 3 times, each cache file parsed 4 times, a new SQLite connection per request. A row
rebuilt alone reads the same inputs, so option 3 would need #1313's fix first as well. The 150 ms gate must hold on
live data after #1313, and that sets a bar this design depends on: see the open questions.

## Decision

Option 2, in six parts (`shapes.md` has the types and the protocol).

1. **The renderer core is a crate** (q2040): `drover-blocks`, in a new `blocks` folder beside the other crates.
   It holds the block rules (what a screen must be for a row to be addressed), the row diff and its ops, a
   reference patcher, the stream's wire types and both ends' bookkeeping (the server's revs and ring, a client's
   revs). It depends on serde and serde_json, on no drover crate, no tokio, no clock and no socket. It draws
   nothing: not a UI framework.
2. **The hub.** A cache of built screens by `Key` (canonical path and query, paired or not). A key is stale when
   one of 7 signals it depends on moved: the log's last seq, the snapshot's stamp, the background reads, the
   config, the gh cache folder, a 30 s clock, a lane's tail. One publisher checks the signals every 25 ms, at once
   after serve's own writes, and only while a stream is open. It builds each stale held key, the most-held first,
   and publishes each as it is built. A key nobody holds is built on its next read, once, however many readers
   wait. The hub keeps JSON bytes, not trees (12 times smaller).
3. **Row ops** (q2039). The diff names rows: `merge` (fields changed), `insert`, `remove`, and the new `move` (a row
   changed place; today that replaces the whole card). A bare list gets a root id (the cards' fix); a `log`
   container never loses its head (transcripts). Every row and card must carry an id; a screen that breaks that is
   listed in health and fails a test. The page applies each op to the one node it names.
4. **One patch per frame, reading position kept** (q2039). The page queues ops and patches at most once per 100 ms,
   on an animation frame; a lone frame is not held back. Before a patch it notes the first row in view that the
   patch does not move or remove, and afterwards scrolls by however far that row shifted, so nothing above the
   viewport pushes the reading down. At the very top a new row shows at the top; a log at its end stays at its end.
5. **One stream per device.** `GET /stream` is the only `EventSource` the page opens. `POST /stream/<sid>` opens
   and closes keys and answers with each opened key's state: nothing, the missed ops, or the whole screen. Every
   frame names its key, the rev it applies to and the rev it makes, which makes a lost or doubled frame harmless
   and a reconnect, a restart and a handover the same case. Limits: 4 streams a device (the oldest is closed), 16
   keys a stream. No server-wide stream cap.
6. **Lane tails.** serve keeps each lane's last lines, numbered, fed by one background thread: 200 lines every
   30 s for every lane, remote ones too; up to 5,000 lines once and then every second while its sheet is open. The
   sheet paints the phone's last copy, opens its key, and new lines arrive as `insert` ops. `[older]` pages back
   200 lines at a time out of the kept lines. A request never reads shed.

serve moves onto tokio under all of it (q2044): next section.

## serve on tokio: what moves, in what order

After the move: accepting, reading a request, every stream's writer and the hub's bookkeeping are tasks on the
runtime. Anything that blocks (SQLite, shed, a subprocess, a file, a build) runs in `spawn_blocking`, or on a
thread of its own when the work never stops (the background readers, the tail feeder). core and the new crate stay
synchronous; tokio is in `crates/server` and `crates/service` only. No new tokio feature and no HTTP crate: the
request reader stays hand-written, and waits are `timeout(…, notified())` as in `drover service` (no `select!`).

1. **The transport only (slice 2).** serve builds a runtime (2 workers), or runs on its caller's (`drover
   service`). Accept is async; one task per connection reads the request with today's time and size limits, then
   runs today's `Server::respond`, whole, in `spawn_blocking`: at most 64 at a time, the rest wait instead of
   getting a 503. `Server` goes into an `Arc`, so the entry takes an owned `Ctx` and a `&'static dyn Runner`, as
   `service::run` does. The background loops stay threads. The long-lived sockets of today (`/live`, cooee's
   per-screen stream) are handed back as blocking sockets to today's code, unchanged. The webhook listener uses the
   same accept and reader. Gate: every socket test passes with only its `serve_on` call changed; handover, the
   soak and `serve_load.rs` pass.
2. **The hub (slice 3)** is written for the runtime: the publisher is a task whose pass (signals, builds, diffs)
   is one `spawn_blocking` call; `GET /screens/<x>` is answered on the runtime from the cache and takes a blocking
   thread only to build. cooee's per-screen stream becomes a task on the hub; the cap of 4 goes.
3. **The device stream (slice 4)** is a task per stream with a bounded queue. `/live` moves onto the same writer.
   From here no socket is held by a thread.
4. Everything after is built and measured on the async serve, so the gates (150 ms p95, 10 screens on 3 devices,
   RSS under 300 MB) are gates on it.

Transport first, because the hub and the streams are then written once, for the runtime, and the first step
changes no behaviour that a test or the owner can see.

## What topcoat's shards taught, and why the blocks stay JSON

Read: topcoat 0.11 (the version cooee pins), its runtime guide and the browser's shard code. A shard is a named
piece of a page with its own endpoint. The server renders it again when an input it read changes, and the browser
morphs the new markup into its place, keeping focus, scroll and typed text. A list's items need stable ids.

Taken:

- **Re-render only the piece whose inputs changed**, at two grains. A screen is rebuilt only when a signal it
  declares moved (the legend never rebuilds for a log event). Within a screen only the rows whose data changed
  travel and are redrawn. The numbers put the rebuild grain at the screen and the repaint grain at the row.
- **Inputs are declared, and coarse where reads cannot be tracked.** topcoat tracks a shard's reads by itself; a
  screen build reads files, shed and SQLite that nothing tracks, so each screen names its signals and the clock
  heals a miss.
- **Stable identity.** A block's id lasts its life, a `move` lets a row's node follow its row, and an unnamed row
  is a fault the tests catch.
- **Patch in place.** A node no op names is never touched, so focus, drafts and the reading position survive.
- **One connection for every piece.** Connected shards share the document's connection; the keys share one stream.

Not taken:

- **A render per browser.** A shard runs for each client, with that client's values. A screen has no per-client
  input, so one build serves every phone; that is the hub.
- **State held by the client and sent back.** The phone sends keys and revs, nothing a build reads.
- **HTML fragments.** cooee draws the same blocks natively; HTML would need a web view there, or a second
  definition of every screen. A JSON diff is by field (one new ask: 14 ops, 2 KB), where a fragment is its region
  whole and the client must morph it to find the change. The phone keeps its last copy as data and paints it
  before the network answers. And the repo already bans HTML built in the server and `innerHTML` in the page.

## Smaller choices

- **SSE and a POST, not a WebSocket.** A WebSocket needs a handshake hash and framing by hand or a new dependency.
- **Nothing kept for a closed stream.** The page's revs do the job of a server-side session with no bound to keep.
- **Coarse signals, not per-screen event filters.** When in doubt a screen depends on everything.
- **Ages stay text the server writes**, so the 30 s clock tick stays; changing that changes what cooee draws.
- **A tab the owner opened stays subscribed while the page is visible**: its badge is live, switching reads nothing.
- **Batching is on the phone, not the server.** The publisher already folds a burst into one delta per build.
- **cooee's `GET /screens/<x>/stream` stays**, served from the hub in its old framing and old ops (a `move` goes
  out as today's `replace` of the card, a changed card list as a whole `screen`). cooee moves to `/stream` in its
  own repo, and then the route and that translation go.
- **The block kinds are not typed per field.** The builders keep writing JSON; the crate checks structure only
  (ids, kinds). Typing every kind's fields is cooee's vocabulary and a rewrite of every builder: not this issue.
- **No new config key.** The bounds are constants in their modules.

## Consequences

- An event costs one build per open screen, whatever the number of phones: at most 42 ms of CPU and 73 MB of
  short-lived allocation, two builds at a time at most, where today each stream thread does its own.
- Memory is bounded by `shapes.md`'s table. Worst case over serve's base: 26 MB of held screens (192 keys), 9 MB of
  unheld ones, 12 MB of stream queues (3 devices), 16 MB of tails, two builds in flight (66 MB): under 130 MB.
  Connection threads go from 256 to at most 64 blocking threads, idle ones ending after 10 s.
- No 503 from a stream cap. A phone past its own limit loses its oldest stream and is told why.
- A row that changes place is one `move`, and the cards update live for the first time.
- `serve_on`'s signature changes (an owned `Ctx`, a `'static` runner): about 25 test call sites, mechanically.
- `docs/serve.md`'s dependency list and CI's deps gate gain `drover-blocks` and tokio in `crates/server`; CI's
  layers step checks that `drover-blocks` pulls in no drover crate; ui-webkit's path filter gains the new files.
- A thread is marked read by the hub after it builds a thread key that a paired stream holds, not inside the build.
- One publisher: a slow build delays the keys behind it. `/api/v1/health` lists each key's build p50 and p95, its
  size, rev, holders and faults, so a slow or unaddressable screen is visible on the live instance.
- #1314 (gzip, keep-alive) edits the connection code that slice 2 replaces. Whichever lands second carries the
  other; its socket tests are the check. If #1314 is still open when slice 2 starts, block it on #1318.
- `screens.rs` is at 1,995 of its 2,000 lines and `serve.rs` at 3,378. The diff, the stream loop and the
  connection handler leave them; all new code is in new files.
- Estimate: Rust `src/` net about +1,100 lines (the crate 500, the hub 260, the stream 230, the tails 220, the
  connection code 60, less 155 that leave `screens.rs`); `web/src` about +350. The cap's headroom is 414.
- RSS after 24 h cannot be a CI test. CI keeps the 60 s soak, moved to device streams; the 24 h number is read on
  the live instance (`/api/v1/health`'s RSS, and no `serve-mem-high` in that day).

## Slices

In this order; each carries its tests from `usage.md`.

1. **The crate.** `drover-blocks`: the block rules, the diff (moved out of `screens.rs`, with `move` and the list
   root), the patcher, the wire types, `Shelf` and `Mirror`. `api.rs` re-exports the wire types; `just types`
   writes them. serve's old streams call the crate's diff through the old-ops translation. One change shows:
   cooee's cards stream sends the list when it changes (today it sends nothing).
2. **serve on tokio.** `serve/conn.rs`: accept, the request reader, the routes' split. As step 1 above.
3. **The hub.** `serve/hub.rs`, `Server::screen` renamed `build_screen` with `deps`. `GET /screens/<x>` and the old
   per-screen streams read the hub (ETag, 304); the cap of 4 and the per-stream build loop go. Health's key table.
4. **The device stream.** `serve/stream.rs`, the page's `stream.ts`. Tabs, cards, sheets and threads mount and
   unmount keys; a sheet paints its last copy first. The page's per-screen streams go; `/live` joins the writer.
5. **Patch in place.** The page's `patch.ts`: the id index, one node per op, the 100 ms queue, the anchor, `log`
   containers, the debounced save. `paint` stays for a whole `screen` only.
6. **Lane tails.** `serve/tails.rs` and its feeder; the lane sheet built from it with stable block ids; `older`.
7. **Gates and docs.** The fixture gates, 10 screens on 3 devices, the soak on device streams; `docs/serve.md` and
   `docs/performance.md`. It uses #1312's fixture (phone timings) if that has merged, else a builder of the same
   size in `tests/common`.

## Open questions

1. **The line cap.** This issue adds about 1,100 lines of Rust; the cap has 414 left, 187 once the open PRs land.
   Recommended: one raise of about 1,000, filed by the orchestrator, before slice 1 starts.
2. **#1313's bar for the Queue.** An event must be painted within 150 ms: 25 ms to notice it, the build, 5 ms to
   diff and write, and about 50 ms for the network and the phone. That leaves a held screen's build 70 ms p95.
   #1313's own gate is 150 ms a build, on the live copy and the live-shaped fixture. Recommended: #1313 holds the
   Queue (and each screen a tab keeps open) to 70 ms p95 there. If it lands between 70 and 150 ms, this issue's
   event gate passes on the fixture and fails on live data, and the fix is then inside the Queue's builder (keep
   each fleet card's rows until one of that card's inputs moves), not a new architecture.

## `.drover/arch.toml` lines

No hot file is converted. The impl lane adds these bans: the first two with slice 1, the third with slice 3 and
the last with slice 4.

```toml
[[ban]]
pattern = "drover_"
paths = ["crates/blocks/src/**"]
reason = "#1318 (q2040): the renderer core imports nothing from drover's crates, so it can be extracted"

[[ban]]
pattern = "tokio"
paths = ["crates/blocks/src/**", "crates/core/src/**", "crates/sys/src/**"]
reason = "#1318 (q2044): tokio is serve's and service's transport; core, sys and the renderer core stay synchronous"

[[ban]]
pattern = ".build_screen("
paths = ["crates/server/src/**"]
except = ["crates/server/src/serve/hub.rs", "crates/server/src/screens.rs"]
reason = "#1318: a screen is built once per change by the hub (serve/hub.rs); a request and a stream read its cache"

[[ban]]
pattern = "new EventSource"
paths = ["web/src/**"]
except = ["web/src/stream.ts"]
reason = "#1318: the page holds one stream per device (web/src/stream.ts); a view opens a key on it"
```
