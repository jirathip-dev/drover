# ADR: screen-hub — each screen built once per change, row ops on one stream per device, serve on tokio (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache, and send each phone only the rows that
changed, moved, came or went (row-by-row builds would save 8 ms of the 150). #1318 is drover's side only (q2071):
the diff, the stream protocol and the page's patch code come pinned from the blocks repo; #1318 builds none of them.
Slices: seven, all in drover. Serve on tokio, the shared cache and the lane tail cache start now. The pin and the
device stream wait for blocks#4; the page's one stream and its patching wait for blocks' renderer; then the gates.
Open questions: keep drover's own drawing code until blocks' renderer replaces it? Count #1318 in the cap raise?

## Status

Revised 2026-11-18, four times, for the owner's amendments: q2039 (row ops required, one patch per frame, reading
position kept), q2044 (serve on tokio), q2049 (the renderer core lives in `<org>/blocks`) and q2071 (#1318
builds nothing there: it consumes blocks#4, blocks#5 and blocks#2's slices, pinned). Earlier versions: `e7bf23a0f`,
`354e1f49f`, `010f53fe2`. Read `usage.md` first, then this, `shapes.md`, `boundaries.md`; `facts.md` holds the
measurements. This revision drops the two slices that built in blocks, says what drover needs from blocks and which
issue brings it, and reorders the drover slices so that three of them wait for nothing. The hub, the stream, the
tokio order and the numbers are as they were; nothing was measured again.

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

**The live instance.** Its Queue build was about 360 ms, not 9: repeated reads (each cache file parsed 4 times, the
config 5 times, shed listed 3 times, a new SQLite connection per request). That is #1313's job, and a row rebuilt
alone would repeat the same reads. #1313's PR (in review, not merged) measures the Queue at 36 ms p95 on a fixture
shaped like live (4,911 cache files, 139 MB), down from 486. This design needs a held screen to build within 70 ms
p95 on live data (25 ms to notice an event, 5 ms to diff and write, about 50 ms for the network and the phone, out
of 150). 36 ms is inside that; slice 7 makes 70 ms its own gate on #1313's fixture.

## Decision

Option 2, in six parts (`shapes.md` has the types and the protocol).

1. **The diff, the protocol and the patcher are taken from blocks** (q2071). drover pins one commit of
   `<org>/blocks` and writes no diff, op, frame or patch code of its own. The next section says what drover
   needs from it and which blocks issue brings each part.
2. **The hub.** A cache of built screens by `Key` (canonical path and query, paired or not). A key is stale when
   one of 7 signals it depends on moved: the log's last seq, the snapshot's stamp, the background reads, the
   config, the gh cache folder, a 30 s clock, a lane's tail. One publisher checks the signals every 25 ms, at once
   after serve's own writes, and only while a stream is open. It builds each stale held key, the most-held first,
   and publishes each as it is built. A key nobody holds is built on its next read, once, however many readers
   wait. The hub keeps JSON bytes, not trees (12 times smaller).
3. **Row ops** (q2039), from the crate's diff: `merge` (fields changed), `insert`, `remove` and `move` (a row
   changed place; today that replaces the whole card), a root for a bare list (the cards' fix), and a `log`
   container that never loses its head (transcripts). drover's part is that every row and card its builders write
   carries an id: a screen that breaks that is listed in health and fails a test.
4. **One patch per frame, reading position kept** (q2039). The page hands each key's ops to the blocks patcher,
   which patches at most once per 100 ms, on an animation frame, touches only the nodes the ops name, and keeps the
   first row in view where it was, so nothing above the viewport pushes the reading down. That behaviour is what
   drover needs of the patcher; drover's own tests hold it on the real page with real screens (`usage.md`).
5. **One stream per device.** `GET /stream` is the only `EventSource` the page opens. `POST /stream/<sid>` opens
   and closes keys and answers with each opened key's state: nothing, the missed ops, or the whole screen. Every
   frame names its key, the rev it applies to and the rev it makes, which makes a lost or doubled frame harmless
   and a reconnect, a restart and a handover the same case. Limits: 4 streams a device (the oldest is closed), 16
   keys a stream. No server-wide stream cap. The routes, the sessions and the limits are serve's; the frames are
   the crate's.
6. **Lane tails.** serve keeps each lane's last lines, numbered, fed by one background thread: 200 lines every
   30 s for every lane, remote ones too; up to 5,000 lines once and then every second while its sheet is open. The
   sheet paints the phone's last copy, opens its key, and new lines arrive as `insert` ops. `[older]` pages back
   200 lines at a time out of the kept lines. A request never reads shed.

serve moves onto tokio under all of it (q2044): the section after next.

## What #1318 takes from blocks

| | drover needs | comes from | the slice that waits |
|---|---|---|---|
| N1 | the crate: the row diff (part 3's ops), the check that names a screen's faults, the stream's frame types, both ends' bookkeeping, today's ops for cooee's old stream | blocks#4 | 4 and, through it, 5 |
| N2 | the page's half: the generated TS types, the stream client, the patcher of part 4 | blocks#4 (the types); a blocks#2 slice (the rest) | 6 |

N1 is what the hub publishes with, what the device stream answers with, and what plays the phone in serve's tests
(the server's revs and ring, a client's revs). N2 is what the page mounts keys on and patches through; its patcher
draws through a function it is handed. The stream client and the patcher are the op half of blocks' HTML renderer:
B2 in blocks#2's draft, with no issue number yet.

- **Blocked by issue, not by guess.** Each drover slice is its own issue. The orchestrator blocks slice 4 with
  `drover queue herdsman block --on blocks#4 <issue>`, and slice 6 on the blocks issue that brings N2 once blocks#2's
  ADR is approved and its slices are filed. blocks#4 as filed says "typed blocks + ops"; blocks#2's draft puts the
  diff and the bookkeeping in that same first slice. If its approved ADR cuts them into another issue, slice 4's
  block moves there.
- **Nothing is patched around.** If the pin lacks something in the table, the fix is a blocks issue and the drover
  slice stays blocked. `api.rs`, `screens.rs` and `web/src` never define an op, a frame or a patch.
- **blocks#2's ADR owns the shapes.** `shapes.md` records them as drover calls them. blocks#2 is asked to reuse them
  and its draft does, field for field, adding what is its own to decide: `after` on an insert, what a client does
  with an op it does not know, the schema version in `hello`, an app's own kinds in the check. Where the pinned
  crate differs from `shapes.md`, the crate is right and the slice follows it.
- **One pin.** One blocks commit, named in `Cargo.toml` from slice 4 (a git `rev`, as `shed-proto`) and in
  `web/package.json` from slice 6. A test fails when the two differ.

Not #1318's, whatever blocks ships meanwhile; each is an issue in blocks#2's migration: the page drawing through
blocks' HTML renderer (open question 1), the theme, the shell and `layout.json`, drover's builders on typed
constructors in place of `json!` (q2063), compatibility across schema versions (q2064), cooee's move to `/stream`
and the end of the old stream route.

## serve on tokio: what moves, in what order

After the move: accepting, reading a request, every stream's writer and the hub's bookkeeping are tasks on the
runtime. Anything that blocks (SQLite, shed, a subprocess, a file, a build) runs in `spawn_blocking`, or on a
thread of its own when the work never stops (the background readers, the tail feeder). core stays synchronous;
tokio is in `crates/server` and `crates/service` only. No new tokio feature and no HTTP crate: the request reader
stays hand-written, and waits are `timeout(…, notified())` as in `drover service` (no `select!`).

1. **The transport only (slice 1).** serve builds a runtime (2 workers), or runs on its caller's (`drover
   service`). Accept is async; one task per connection reads the request with today's time and size limits, then
   runs today's `Server::respond`, whole, in `spawn_blocking`: at most 64 at a time, the rest wait instead of
   getting a 503. `Server` goes into an `Arc`, so the entry takes an owned `Ctx` and a `&'static dyn Runner`, as
   `service::run` does. The background loops stay threads. The long-lived sockets of today (`/live`, the
   per-screen streams) are handed back as blocking sockets to today's code, unchanged. The webhook listener uses
   the same accept and reader. Gate: every socket test passes with only its `serve_on` call changed; handover, the
   soak and `serve_load.rs` pass.
2. **The hub (slice 2)** is written for the runtime: the publisher is a task whose pass (signals, builds, diffs)
   is one `spawn_blocking` call; `GET /screens/<x>` is answered on the runtime from the cache and takes a blocking
   thread only to build. The per-screen streams become tasks on the hub; the cap of 4 goes.
3. **The device stream (slice 5)** is a task per stream with a bounded queue. `/live` moves onto the same writer.
   From here no socket is held by a thread.
4. Everything after slice 1 is built and measured on the async serve, so the gates (150 ms p95, 10 screens on 3
   devices, RSS under 300 MB) are gates on it.

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
  The one HTML renderer of q2047 does not change this: it is a client that draws the JSON blocks in a web view.

## Smaller choices

- **The hub comes before the pin.** Slice 2 publishes with the diff `screens.rs` has today, to today's per-screen
  streams, and keeps each key's rev and JSON itself. Slice 4 swaps that diff and that bookkeeping for the crate's
  (about 30 lines rewritten) and deletes drover's diff. The price is small, and build-once, the end of the 503 and
  the memory bound do not wait for blocks.
- **SSE and a POST, not a WebSocket.** A WebSocket needs a handshake hash and framing by hand or a new dependency.
- **Nothing kept for a closed stream.** The page's revs do the job of a server-side session with no bound to keep.
- **Coarse signals, not per-screen event filters.** When in doubt a screen depends on everything.
- **Ages stay text the server writes**, so the 30 s clock tick stays; changing that changes what cooee draws.
- **A tab the owner opened stays subscribed while the page is visible**: its badge is live, switching reads nothing.
- **Batching is on the phone, not the server.** The publisher already folds a burst into one delta per build.
- **cooee's `GET /screens/<x>/stream` stays**, served from the hub in its old framing and old ops (from slice 4 a
  `move` goes out as today's `replace` of the card, a changed card list as a whole `screen`). cooee moves to
  `/stream` in its own repo, and then the route and that translation go.
- **No new config key.** The bounds are constants in their modules.

## Consequences

- **What lands before blocks does.** blocks#4 is queued behind blocks#1 (in impl) and blocks#2 (its ADR in
  revision). Slices 1 to 3 need neither and give: one build per change for all phones, no stream cap and no 503,
  serve on tokio, every cache bounded, a lane sheet that opens from the cache. Row patches on the phone, one stream
  per device, live cards and a lane's tail streaming in arrive with slices 4 to 6, when blocks brings N1 and N2.
- An event costs one build per open screen, whatever the number of phones: at most 42 ms of CPU and 73 MB of
  short-lived allocation, two builds at a time at most, where today each stream thread does its own.
- Memory is bounded by `shapes.md`'s table. Worst case over serve's base: 26 MB of held screens (192 keys), 9 MB of
  unheld ones, 12 MB of stream queues (3 devices), 16 MB of tails, two builds in flight (66 MB): under 130 MB.
  Connection threads go from 256 to at most 64 blocking threads, idle ones ending after 10 s.
- No 503 from a stream cap. From slice 5 a phone past its own limit loses its oldest stream and is told why;
  between slices 2 and 5 the per-screen streams are bounded only by the 256 connections.
- A row that changes place is one `move`, and the cards update live for the first time (slice 4 on).
- `serve_on`'s signature changes (an owned `Ctx`, a `'static` runner): about 25 test call sites, mechanically.
- `docs/serve.md`'s dependency list and CI's deps gate gain tokio in `crates/server` (slice 1) and `blocks` (slice
  4); `deny.toml`'s `allow-git` gains the blocks repo.
- A change drover needs to an op, a frame or the patcher is two PRs: an issue in blocks, then the pin in drover
  (the lock files and `web/dist` with it).
- drover's page takes its first npm dependency that ends up in the bundle (slice 6). `web/dist` stays committed, so
  building the binary still needs no Node.
- cooee is not touched by #1318.
- A thread is marked read by the hub after it builds a thread key that a paired stream holds, not inside the build.
- One publisher: a slow build delays the keys behind it. `/api/v1/health` lists each key's build p50 and p95, its
  size, rev, holders and (from slice 4) faults, so a slow or unaddressable screen is visible on the live instance.
- #1314 (gzip, keep-alive; its PR is in a fix round) edits the connection code that slice 1 replaces. It lands
  first, and slice 1 carries both into `serve/conn.rs`; #1314's socket tests are the check.
- `screens.rs` is at 1,996 of its 2,000 lines and `serve.rs` at 3,380. The stream loop (slice 2), the
  diff (slice 4) and the connection handler (slice 1) leave them; all new code is in new files.
- Estimate: Rust `src/` net about +620 lines (the hub 260, the stream 230, the tails 220, the connection code 60,
  less 155 that leave `screens.rs`), about 500 of it in the three slices that start now. `web/src` about level
  (`ops.ts` and the per-screen streams go, the mounts come). Open question 2.
- RSS after 24 h cannot be a CI test. CI keeps the 60 s soak, moved to device streams in slice 5; the 24 h number
  is read on the live instance (`/api/v1/health`'s RSS, and no `serve-mem-high` in that day).

## Slices

Seven, all in drover (fleet `herdsman`). Each is one issue and one PR, filed by the orchestrator with a link to
this folder, and carries its tests from `usage.md`. Slices 1 to 3 need nothing from blocks and run in that order.

1. **serve on tokio.** `serve/conn.rs`: accept, the request reader, the routes' split. As step 1 of its section.
2. **The hub** (after 1). `serve/hub.rs`, `Server::screen` renamed `build_screen` with `deps`. `GET /screens/<x>`
   (ETag, 304) and the per-screen streams read the hub; the cap of 4 and the per-stream build loop go. Health's key
   table. The diff is still `screens.rs`'s.
3. **Lane tails, the server's half** (after 2). `serve/tails.rs` and its feeder, remote lanes too; the lane sheet
   built from it with stable block ids, never from shed; the `older` route.
4. **The pin** (after 2; blocked on blocks#4, N1). `Cargo.toml`, `deny.toml`, the deps gate, `docs/serve.md`. The
   hub publishes through the crate; drover's diff is deleted; the per-screen streams send the crate's ops as
   today's. Every fixture screen passes the crate's check, and health lists faults. One change shows: cooee's cards
   stream sends the list when it changes (today it sends nothing).
5. **The device stream, the server's half** (after 4). `serve/stream.rs`: `GET /stream`, `POST /stream/<sid>`, the
   per-device limit; `/live` joins the writer. Its tests play the phone with the crate's client bookkeeping, so it
   needs no page. The soak moves to device streams.
6. **The page: one stream, patched in place** (after 5; blocked on the blocks issue that brings N2). The pin gains
   the TS package. Tabs, cards, sheets and threads mount and unmount keys on its stream client and hand its patcher
   drover's `render`; a sheet paints its last copy first; a lane's tail streams in and `[older]` pages back.
   `ops.ts` and the page's per-screen streams go.
7. **Gates and docs** (after 6). On #1312's large board (`common::large`, merged): fresh data and event to painted
   under 150 ms p95 by `phone_timings.rs`'s numbers, 10 screens on 3 devices. On #1313's live-shaped fixture: a held
   screen's build within 70 ms p95. `docs/serve.md` and `docs/performance.md`.

## Open questions

1. **Whose code draws a block after slice 6.** q2071 says the phone patches "through the blocks renderer". Read
   here as: the page takes the renderer's stream client and patcher (N2) and keeps drover's own drawing code
   (`web/src/blocks.ts`), handed to the patcher, until blocks#2's migration replaces it in an issue of its own.
   Recommended: yes. Drawing through blocks' renderer needs its theme as well, changes pixels in an issue that
   should change none, and would put slice 6 behind two more blocks slices. If the owner wants the page to draw
   through blocks' renderer inside #1318, slice 6 is blocked on blocks' HTML renderer and theme too.
2. **The line cap.** `drover cap` today: 53,359 of 53,500, and the open PRs already project 210 over, so a raise
   is coming whatever #1318 does. #1318 adds about 620 across its slices. Recommended: the one raise the
   orchestrator files counts #1318 in (to about 54,900), before slice 1 starts.

## `.drover/arch.toml` lines

No hot file is converted. The impl lanes add these bans: the first with slice 1, the second with slice 2 and the
last with slice 6.

```toml
[[ban]]
pattern = "tokio"
paths = ["crates/core/src/**", "crates/sys/src/**"]
reason = "#1318 (q2044): tokio is serve's and service's transport; core and sys stay synchronous"

[[ban]]
pattern = ".build_screen("
paths = ["crates/server/src/**"]
except = ["crates/server/src/serve/hub.rs", "crates/server/src/screens.rs"]
reason = "#1318: a screen is built once per change by the hub (serve/hub.rs); a request and a stream read its cache"

[[ban]]
pattern = "new EventSource"
paths = ["web/src/**"]
reason = "#1318: the page holds one stream per device, opened by the blocks package; a view opens a key on it"
```
