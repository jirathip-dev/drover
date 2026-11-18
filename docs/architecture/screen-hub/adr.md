# ADR: screen-hub — each screen built once per change, row ops on one stream per device, serve on tokio (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache, and send each phone only the rows that
changed, moved, came or went (row-by-row builds would save 8 ms of the 150). The renderer core (typed blocks, row
diff, stream protocol, generated schema, TS and Swift types) is built in the new blocks repo; drover pins one commit.
Slices: in blocks, the crate, then the generated types and the page's stream and patch code. In drover: serve on
tokio, the shared cache, one stream per device, patching in place (one patch per 100 ms), lane tails, the gates.
Open questions: the HTML renderer and the theme move after #1318, not in it? Line cap +700? #1313's Queue bar 70 ms?

## Status

Revised 2026-11-18, three times, for the owner's amendments: q2039 (row ops required, one patch per frame, reading
position kept), q2044 (serve on tokio) and q2049 (the renderer core is built in the new shared repo
`jirathip-dev/blocks` from day one, replacing q2040's crate inside drover; q2047 and q2051 stand behind it). Earlier
versions: `e7bf23a0f`, `354e1f49f`. Read `usage.md` first, then this, `shapes.md`, `boundaries.md`; `facts.md` holds
the measurements. Only the place and the typing of the renderer core changed in this revision: the hub, the stream,
the tokio order and the numbers are as they were.

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

1. **The renderer core is the blocks repo** (q2049): `jirathip-dev/blocks`, public, MIT. Its crate `blocks` holds
   the typed blocks (a Rust type per kind), the row diff and its ops, a reference patcher, the stream's wire types
   and both ends' bookkeeping (the server's revs and ring, a client's revs). It depends on serde and serde_json, on
   no drover crate, no tokio, no clock and no socket. Its generator writes the JSON Schema, the TS types and the
   Swift types from the Rust types. Its TS package holds those types and the two new modules of the page: the
   stream client and the patcher. drover pins one commit of it. More in the next section.
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

serve moves onto tokio under all of it (q2044): the section after next.

## The blocks repo: what #1318 builds there, and what waits

`shapes.md` has the layout and the types. The choices:

- **Typed blocks, from what both apps send today.** One Rust type per kind: the 21 kinds of cooee's schema (its
  40 definitions, read at cooee staging `9c0987c9a`) and the 3 that only drover writes (`composer`,
  `header-strip`, `menu-row`). Reading is lenient, as cooee's schema is: an unknown kind passes through and a
  client skips it; an unknown field is kept. Writing is strict: a server's screen with an unknown kind, an unknown
  field or a missing one has a fault, and a fault fails a test.
- **One generator path.** Rust types → JSON Schema (schemars, behind a feature only the generator turns on) → the
  TS and the Swift types, written from the schema by the repo's own `blocks-gen`. `just gen` writes all three; CI
  fails when one is stale. Not ts-rs for TS and a second tool for Swift: three tools drift three ways. The crate
  drover links keeps two dependencies, serde and serde_json.
- **The diff stays on JSON, by id.** The typed model says which fields hold children and checks a screen; the diff
  compares fields as JSON. A typed diff would need a compare per kind and gives the phone the same ops.
- **drover's builders keep writing JSON in #1318.** The hub reads each built screen into the typed model after it
  is published, so no phone waits for the check; health lists the faults and `no_fixture_screen_has_a_fault` fails
  on one. Moving about 170 hand-written blocks onto typed constructors is a refactor (behaviour unchanged, the
  goldens hold it) and is not on the 150 ms path.
- **Everything #1318 writes new is born in blocks; nothing old moves in #1318.** The stream client and the patcher
  are new, so they are written there. The patcher draws through the `render` it is given, so drover's `blocks.ts`
  stays where it is for now.
- **One pin.** drover names one blocks commit twice: for the crate in `Cargo.toml` (a git `rev`, as `shed-proto`)
  and for the TS package in `web/package.json`. A test fails when the two differ. The commit is one merged on
  blocks' `staging`; version tags start when the owner promotes blocks to `main`.

What waits for its own issue, after #1318 (open question 1):

| What | Where | Why not now |
|---|---|---|
| the one HTML renderer (q2047): drover's `blocks.ts` and its CSS, moved | blocks | it draws the theme's figures and words, so it moves with the theme; not on the 150 ms path |
| the shared theme package (q2047) | blocks | #1318 changes no theme |
| cooee adopts: its schema file generated, the Swift types, `/stream`, the embedded renderer | cooee | cooee's own queue; until then its stream keeps today's ops (below) |
| drover's builders onto the typed constructors | drover, `--refactor` | the check already holds them to the model |

## serve on tokio: what moves, in what order

After the move: accepting, reading a request, every stream's writer and the hub's bookkeeping are tasks on the
runtime. Anything that blocks (SQLite, shed, a subprocess, a file, a build) runs in `spawn_blocking`, or on a
thread of its own when the work never stops (the background readers, the tail feeder). core and the `blocks` crate
stay synchronous; tokio is in `crates/server` and `crates/service` only. No new tokio feature and no HTTP crate: the
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
  The one HTML renderer of q2047 does not change this: it is a client that draws the JSON blocks in a web view.

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
- **A git pin, not a registry.** drover already pins `shed-proto` this way; a registry adds a publish step to every
  change while the protocol is still moving.
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
- `docs/serve.md`'s dependency list and CI's deps gate gain `blocks` and tokio in `crates/server`; `deny.toml`'s
  `allow-git` gains the blocks repo; one CI line checks that `blocks` brings in nothing but serde and serde_json.
- A change to an op, a block or the protocol is two PRs: one in blocks, then the pin in drover (the lock files and
  `web/dist` with it). That is the price of q2049 while the protocol still moves; slices 1 and 4 each start with it.
- drover's page takes its first npm dependency that ends up in the bundle. `web/dist` stays committed, so building
  the binary still needs no Node.
- cooee is not touched. Its goldens in drover's tests and its schema's fixtures are parsed by the crate's tests, so
  the typed model accepts what cooee draws today.
- A thread is marked read by the hub after it builds a thread key that a paired stream holds, not inside the build.
- One publisher: a slow build delays the keys behind it. `/api/v1/health` lists each key's build p50 and p95, its
  size, rev, holders and faults, so a slow or unaddressable screen is visible on the live instance.
- #1314 (gzip, keep-alive) edits the connection code that slice 2 replaces. Whichever lands second carries the
  other; its socket tests are the check. If #1314 is still open when slice 2 starts, block it on #1318.
- `screens.rs` is at 1,995 of its 2,000 lines and `serve.rs` at 3,378. The diff, the stream loop and the
  connection handler leave them; all new code is in new files.
- Estimate, drover: Rust `src/` net about +620 lines (the hub 260, the stream 230, the tails 220, the connection
  code 60, less 155 that leave `screens.rs`); `web/src` about level (`ops.ts` and the per-screen streams go, the
  mounts come). The cap's headroom is 414, and 4 once the open PRs land. Estimate, blocks (no cap there): the crate
  about 900 lines, the generator 300, the TS package 450.
- RSS after 24 h cannot be a CI test. CI keeps the 60 s soak, moved to device streams; the 24 h number is read on
  the live instance (`/api/v1/health`'s RSS, and no `serve-mem-high` in that day).

## Slices

Two repos. A blocks slice is an issue in `jirathip-dev/blocks` (fleet `blocks`), filed by the orchestrator with a
link to this folder. The drover slice that needs it is blocked on it (`drover queue herdsman block --on blocks#N`)
and starts by moving the pin. Each slice carries its tests from `usage.md`.

In blocks:

- **B1. The crate.** The workspace and its CI: fmt, clippy with drover's lint table, tests, deny, and a line that
  the crate's dependencies are serde and serde_json. `blocks`: the typed blocks, `Built` and its faults, the diff
  (with `move` and the list root), the patcher, the wire types, `Shelf`, `Mirror`, `wire::old`. `fixtures/`:
  cooee's and drover's goldens, each parsed with no fault.
- **B2. The generated types and the page's two modules.** `blocks-gen` and `just gen` (the schema, `types.ts`,
  `Blocks.swift`) with the stale check; the Swift file builds and decodes the fixtures. The TS package:
  `stream.ts` and `patch.ts` with their tests (the protocol cases `Mirror` runs too; WebKit for the patcher).

In drover, in this order (slice 2 needs nothing from blocks and can start at once):

1. **The pin** (needs B1). `Cargo.toml`, `deny.toml`, the deps gate, `docs/serve.md`. The diff leaves `screens.rs`:
   serve's old streams call the crate's through `wire::old`. One change shows: cooee's cards stream sends the list
   when it changes (today it sends nothing).
2. **serve on tokio.** `serve/conn.rs`: accept, the request reader, the routes' split. As step 1 of its section.
3. **The hub.** `serve/hub.rs`, `Server::screen` renamed `build_screen` with `deps`. `GET /screens/<x>` and the old
   per-screen streams read the hub (ETag, 304); the cap of 4 and the per-stream build loop go. Health's key table,
   with each key's faults from the typed check.
4. **The device stream** (needs B2). `serve/stream.rs`; the pin gains the TS package. Tabs, cards, sheets and
   threads mount and unmount keys on the package's stream; a sheet paints its last copy first. The page's
   per-screen streams go; `/live` joins the writer.
5. **Patch in place.** The page hands its `render` to the package's patcher: one node per op, the 100 ms queue, the
   anchor, `log` containers, the debounced save. `ops.ts` goes; `paint` stays for a whole `screen` only.
6. **Lane tails.** `serve/tails.rs` and its feeder; the lane sheet built from it with stable block ids; `older`.
7. **Gates and docs.** The fixture gates, 10 screens on 3 devices, the soak on device streams; `docs/serve.md` and
   `docs/performance.md`. It uses #1312's fixture (phone timings) if that has merged, else a builder of the same
   size in `tests/common`.

## Open questions

1. **What waits.** The amendment puts the HTML renderer and the theme package in the blocks repo; it does not say
   #1318 builds them. Recommended: their own issues after #1318, with cooee's switch and the builders' move to
   typed constructors (the table above). #1318 then builds in blocks only what its gates need. If the renderer
   should move inside #1318 instead, it goes between B2 and slice 4 and takes the theme's figures and words with it.
2. **The line cap.** drover gains about 620 lines of Rust; the cap has 414 left, 4 once the open PRs land.
   Recommended: one raise of about 700, filed by the orchestrator, before slice 1 starts. (It was about 1,000
   before q2049: the crate's lines are now in blocks.)
3. **#1313's bar for the Queue.** An event must be painted within 150 ms: 25 ms to notice it, the build, 5 ms to
   diff and write, and about 50 ms for the network and the phone. That leaves a held screen's build 70 ms p95.
   #1313's own gate is 150 ms a build, on the live copy and the live-shaped fixture. Recommended: #1313 holds the
   Queue (and each screen a tab keeps open) to 70 ms p95 there. If it lands between 70 and 150 ms, this issue's
   event gate passes on the fixture and fails on live data, and the fix is then inside the Queue's builder (keep
   each fleet card's rows until one of that card's inputs moves), not a new architecture.

## `.drover/arch.toml` lines

No hot file is converted. In drover the impl lane adds these bans: the first with slice 2, the second with slice 3
and the last with slice 4.

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
reason = "#1318: the page holds one stream per device, opened by the blocks package's stream.ts; a view opens a key on it"
```

In blocks, B2 adds its generated files, so they are regenerated and never hand-merged:

```toml
generated = ["schema/blocks.schema.json", "ts/src/types.ts", "swift/Sources/Blocks/Blocks.swift"]
generate = "just gen"
```
