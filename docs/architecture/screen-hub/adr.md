# ADR: screen-hub — each screen built once per change, one stream per device (#1318)

## Summary

Decision: build each screen whole, once per change, into one shared cache; not row by row. On the large fixture a
whole Queue build is 9 ms and every screen together 42 ms, so row-by-row would save 8 ms of a 150 ms budget and need
a map of what every row depends on. Each phone holds one stream and redraws only the rows a change names.
Slices: the shared cache; the one-stream-per-device protocol; in-place patching on the phone; the lane tail cache;
the speed, load and memory gates.
Open questions: the live Queue build is about 360 ms against 9 ms on the fixture: its own issue after slice 1?

## Status

Proposed (architect lane, 2026-11-18, staging `9ea69d9fb`). Read `usage.md` first, then this, `shapes.md`,
`boundaries.md`; `facts.md` holds the measurements.

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
  is 14 ops, 2 KB. The diff costs 0.01 to 1.2 ms. But a bare list has no diff, so the cards never get a change.
- The page throws that away: on each delta it copies the model, renders every block and rewrites localStorage.
- Sheets have no stream. A lane sheet reads up to 5,000 lines from shed on its GET.

## Options

**1. Keep a build per stream, lift the cap.** Ten screens on 3 devices are 30 streams: each event costs 30 builds,
about 100 ms of CPU and 180 MB allocated, where the cap now gives 26 of them a 503. Rejected: the cost grows with
every phone.

**2. Build once per change, share, diff (chosen).** One publisher builds each subscribed screen when something it
depends on changed, keeps the JSON, diffs it against the last build and sends the ops to every stream that holds
the screen. A GET reads the same cache.

**3. Fully incremental screens.** Each event updates only the rows it touches; nothing is built whole.

| | Option 2: once per change | Option 3: incremental |
|---|---|---|
| Queue after one event | 8.5 ms p50, 11.9 ms p95 (9.9 and 13.6 with the event's fold) | 0.07 ms a row touched, at best |
| all 12 screens after one event | 41.7 ms p50; 61.2 ms as a sum of p95s | under 1 ms, at best |
| the same with ten times the history | Queue 12.4 ms p50, 20.5 ms p95 | the same as above |
| a GET or an open of a fresh screen | 0 builds | 0 builds |
| what it saves of the 150 ms budget | | 8 ms on the Queue, 41 ms if all 12 are open |
| what must be right | 7 coarse signals; a miss heals in 30 s | every row's inputs, per row |

Option 3's price is the last line. A Queue row reads its own events, its place in the queue and its neighbours,
its blockers and serial group (other issues), the lane list (from shed, not the log), 4 to 6 cache files (not the
log), the config and the clock (ages, the stall countdown). The counts and the badge read every row. Only one of
those six inputs is the event an incremental update would start from; a row whose other input moved stays wrong
until something else touches it. Option 2 cannot show a stale row for longer than its 30 s clock tick, and it meets
the target with room: 25 ms to notice an event and at most 61 ms to build and diff everything open. The gate is
100 ms from the event to the frame, which leaves 50 ms for the network and the phone.

## Decision

Option 2, with four parts (`shapes.md` has the types and the protocol).

1. **The hub.** A cache of built screens by `Key` (canonical path and query, paired or not). A key is stale when
   one of 7 signals it depends on moved: the log's last seq, the snapshot's stamp, the background reads, the
   config, the gh cache folder, a 30 s clock, a lane's tail. One publisher thread checks the signals every 25 ms
   (10 µs), at once after serve's own writes, and only while a stream is open. It builds each stale key that a
   stream holds, the most-held first, and publishes each as it is built. A key nobody holds is built on its next
   read, once, however many readers wait. The hub keeps JSON bytes, not trees (12 times smaller).
2. **Per-row deltas.** The existing ops, typed in `api.rs`. Two changes: a bare list gets a root id (the cards'
   fix), and a `log` container never loses its head (transcripts). The page applies an op to the one node it names
   through an index of the model by id: no copy, no full render, localStorage written at most every 2 s.
3. **One stream per device.** `GET /stream` is the only `EventSource` the page opens. `POST /stream/<sid>` opens
   and closes keys and answers with each opened key's state: nothing, the missed ops, or the whole screen. So a
   navigation is one request and no build. Every frame names its key, the rev it applies to and the rev it makes,
   which makes a lost or doubled frame harmless and a reconnect, a restart and a handover the same case. Limits:
   4 streams a device (the oldest is closed), 16 keys a stream. No server-wide cap.
4. **Lane tails.** serve keeps each lane's last lines, numbered, fed by one background thread: 200 lines every
   30 s for every lane, remote ones too; up to 5,000 lines once and then every second while its sheet is open. The
   sheet paints the phone's last copy, opens its key, and new lines arrive as `insert` ops. `[older]` pages back
   200 lines at a time out of the kept lines. A request never reads shed.

Smaller choices:

- **SSE and a POST, not a WebSocket.** serve's HTTP is hand-written; a WebSocket needs a handshake hash that is a
  new dependency, and framing. Putting the key list in the stream's URL was the other way: a reconnect per tap.
- **Nothing kept for a closed stream.** A server-side session with `Last-Event-ID` would need its own bound and
  expiry and would not survive a handover. The page's revs do the same job.
- **Coarse signals, not per-screen event filters.** When in doubt a screen depends on everything.
- **Ages stay text the server writes**, so the 30 s clock tick stays. Drawing ages on the phone would end those
  builds but changes the block contract cooee draws from: not in this issue.
- **A tab the owner opened stays subscribed while the page is visible.** Its badge is live (today only the open
  tab's is) and switching to it reads nothing. Builds are shared, so it costs one build per key, not per phone.
- **cooee's `GET /screens/<x>/stream` stays**, served from the hub as a stream with one key. The page stops using
  it. cooee moves to `/stream` in its own repo, and then it goes.
- **No new config key.** The bounds are constants in their modules.

## Consequences

- An event costs one build per open screen, whatever the number of phones: at most 42 ms of CPU and 73 MB of
  short-lived allocation on one thread, where today each stream thread does its own.
- Memory is bounded by `shapes.md`'s table. Worst case over serve's base: 26 MB of held screens (192 keys), 9 MB of
  unheld ones, 12 MB of stream queues (3 devices), 16 MB of tails, one build in flight (33 MB): under 100 MB.
- No 503 from a stream cap. A phone past its own limit loses its oldest stream and is told why.
- The cards update live for the first time. cooee's `/screens/cards/stream` starts to carry ops too.
- A thread is marked read by the hub after it builds a thread key that a paired stream holds, not inside the build.
- One publisher: a slow build delays the keys behind it. `/api/v1/health` lists each key's build p50 and p95, its
  size, rev and subscribers, so a slow screen is visible on the live instance.
- Every held screen that reads the log is rebuilt on every event. At one event a second with all 12 open, that is
  4% of a core. If a key's build is too slow, the fix is local to its builder (below), not a new architecture.
- The fixture builds the Queue 30 to 40 times faster than the live instance does. Option 2 makes navigation fresh
  on the live instance too (a GET reads the cache), but an event still waits for the Queue's build there.
- `screens.rs` is at 1,995 of its 2,000 lines. Slice 1 takes the diff and the stream loop out of it; the new code
  is in new files. Estimate: `src/` net +550 lines, `web/src` +200.
- RSS after 24 h cannot be a CI test. CI keeps the 60 s soak, moved to device streams; the 24 h number is read on
  the live instance (`/api/v1/health`'s RSS, and no `serve-mem-high` in that day).

## Slices

In this order; each carries its tests from `usage.md`.

1. **The hub.** `serve/delta.rs` (the diff, moved; the list root), `serve/hub.rs`, `Server::screen` renamed
   `build_screen` with `deps`. `GET /screens/<x>` and the old per-screen streams read the hub (ETag, 304); the cap
   of 4 and the per-stream build loop go. Per-key numbers in health. The page is unchanged.
2. **The device stream.** `serve/stream.rs`, the wire types in `api.rs`, the page's `stream.ts`. Tabs, cards, sheets
   and threads mount and unmount keys; a sheet paints its last copy first. The page's per-screen streams go.
3. **Patch in place.** The page's `patch.ts`: the id index, one node per op, `log` containers, the debounced save.
   `paint` stays for a whole `screen` only.
4. **Lane tails.** `serve/tails.rs` and its feeder; the lane sheet built from it with stable block ids; `older`.
5. **Gates and docs.** The fixture gates, 10 screens on 3 devices, the soak on device streams; `docs/serve.md` and
   `docs/performance.md`. It uses the phone-timings issue's fixture if that has merged, else a builder of the same
   size in `tests/common`.

## Open questions

1. The issue reports about 360 ms for the live Queue build; the fixture's is 9 to 11 ms. The acceptance criteria
   are on the fixture, and the lane hook refused a copy of the live state, so where the live time goes is not
   measured. Candidates from reading the code: 4 to 6 cache-file parses per row per build, `row()` run for rows that
   are not shown (111 built, 45 shown), the config loaded three times. Recommended: its own issue, queued after
   slice 1, whose health numbers show the slow part; its fix is one set of inputs read once per publisher pass.

## `.drover/arch.toml` lines

No hot file is converted. The impl lane adds these two bans, the first with slice 1 and the second with slice 2:

```toml
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
