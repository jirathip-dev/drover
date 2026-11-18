# screen-hub: how the callers read (#1318)

Sketches, not code to copy. The names are the ones `shapes.md` and `boundaries.md` use. Everything here is drover's
code; where it calls the blocks crate or package, the call is marked with the slice that brings the pin.

## The server, on the runtime (slice 1)

One task per connection. It takes a blocking thread only for today's handler or a build.

```rust
async fn conn(self: Arc<Self>, mut c: TcpStream) {
    let Ok(_conn) = self.conns.clone().try_acquire_owned() else { return busy(&mut c).await };   // 256: 503, as today
    let Ok(Ok(req)) = timeout(READ, request(&mut c)).await else { return };                      // 5 s, today's size limits
    match route(&req) {
        Route::Stream => self.streams.attach(&self, &req, c).await,                 // slice 5: a task until the socket ends
        Route::Sync(sid) => reply(c, self.streams.sync(&self.hub, sid, &req).await).await,      // slice 5
        Route::Screen(key) => reply(c, cached(self.hub.read(&self, &key).await, &req)).await,   // slice 2: 200 with ETag, or 304
        Route::Other => {
            let _slot = self.reqs.acquire().await;                                  // 64 at a time; the rest wait here
            let me = self.clone();
            reply(c, spawn_blocking(move || me.respond(&req)).await).await          // today's handler, whole
        }
    }
}
```

Two entries with the same arguments, owned as `service::run`'s are. `drover service` awaits the first on its own
runtime; `drover serve` and the tests call the second, which builds a runtime (2 workers) and blocks on the first.

```rust
pub async fn run(ctx: Ctx, r: &'static dyn Runner, l: TcpListener, read_only: bool, ho: Option<Arc<Handover>>) -> Result<()>
pub fn serve_with(ctx: Ctx, r: &'static dyn Runner, l: TcpListener, read_only: bool, ho: Option<Arc<Handover>>) -> Result<()>
```

## The hub (slice 2)

A read builds only when the key is stale and nobody else is building it.

```rust
pub async fn read(&self, s: &Arc<Server>, k: &Key) -> Option<Arc<Kept>> {
    loop {
        match self.look(k) {
            Look::Fresh(kept) => return Some(kept),
            Look::Building(mut done) => drop(done.changed().await),                 // someone else's build: await it
            Look::Mine(flight) => return self.build(s, k, flight).await,            // a permit, then `spawn_blocking`
        }
    }
}
```

One task publishes. It is the only place a held screen is built.

```rust
tokio::spawn(async move {
    loop {
        hub.held().await;                                             // asleep while no stream holds a key
        let (h, s) = (hub.clone(), srv.clone());
        drop(spawn_blocking(move || h.pass(&s, crate::now())).await); // the signals; build, diff, publish each stale held key
        drop(timeout(POLL, hub.kick.notified()).await);               // 25 ms, or at once on `hub.kick()`
    }
});
```

An action that wrote something wakes it, so the phone does not wait for the poll. `kick` is callable from a
blocking thread.

```rust
let res = self.action(ctx, name, &a);
self.hub.kick();
```

A screen builder is what `Server::screen` is today, renamed, plus what it depends on. It stays synchronous.

```rust
pub(super) fn build_screen(&self, ctx: &Ctx, k: &Key) -> Option<Value>     // pure: no socket, no mark-read
pub(super) fn deps(path: &str) -> Deps                                      // when in doubt: Deps::ALL
```

Today's per-screen stream stays, as a task with one fixed key, in its `{seq, ops}` framing. In slices 2 and 3 its
ops come from the diff `screens.rs` has today.

```rust
Route::Old(key) => self.streams.single(&self.hub, key, c).await,
```

## The lane sheet (slice 3)

It reads its transcript from the tail cache and never from shed.

```rust
let tail = self.tails.tail(agent, 200);           // the last 200 lines, each with its number
let said = blocks(&tail).map(|b| said(&b));       // a block's id is `t<gen>-<its first line's number>`
json!({"type": "section", "id": "transcript", "log": true, "blocks": said})
```

## The crate, from the hub (slice 4)

The pin lands and the publisher's pass hands the crate screens and gets frames and key states back. drover's own
diff and the hub's own rev bookkeeping are deleted in the same PR.

```rust
use blocks::{Built, Shelf, wire::{Frame, Sync, Synced}};

let new = Built::new(self.build_screen(ctx, &key)?);           // never fails: the builders still write JSON
match shelf.put(&key, &key.path, &new) {
    Put::Changed(delta) => holders.for_each(|q| q.push(&delta.frame)),   // one serialized frame for every stream
    Put::First | Put::Same => {}
}
meta.faults = new.check();                                     // after publishing, for health
let state = shelf.state(&key, &key.path, have.rev);            // current, the missed ops, or the whole screen
```

The per-screen stream now sends each delta through `wire::old`: the crate's ops as today's.

## The device stream (slice 5)

A stream's writer is a task: it waits on its queue, not on a poll.

```rust
async fn run(self: Arc<Stream>, mut c: TcpStream) -> io::Result<()> {
    write(&mut c, &self.hello()).await?;
    loop {
        match timeout(self.beat, self.queue.next()).await {
            Ok(Next::Frame(f)) => timeout(2 * self.beat, c.write_all(f.as_bytes())).await??,   // a phone that stopped reading
            Ok(Next::Bye(why)) => return write(&mut c, &bye(why)).await,                      // evicted, unpaired, handover
            Err(_) => write(&mut c, PING).await?,
        }
    }
}
```

Its tests need no page: the crate's `Mirror` is the phone.

```rust
let mut phone = Mirror::default();
phone.hello(&hello);
phone.synced(&sent, post(sid, &Sync { open: phone.have(), close: vec![] }));
assert_eq!(phone.frame(next_frame(&mut sse)), Took::Patched);
```

## The page (slice 6)

One stream for the page. A view (a tab, the cards, a sheet, a thread) opens its key when it mounts and closes it
when it goes. The stream client and the patcher come from the blocks package; the page hands the patcher its own
`render`.

```ts
import { patcher } from '@jirathip-dev/blocks/patch';
import { stream } from '@jirathip-dev/blocks/stream';
import { render } from './blocks';

const live = stream({ url: R + 'stream', state: sync });     // the one EventSource; the sync dot follows it

function mount(v: View) {
  const c = cached(v.path);                                  // the phone's last copy, painted at once
  if (c) show(v, c.v);
  v.patch = patcher({ root: v.root, model: () => v.model, draw: b => render(b, v.ctx), saved: () => save(v) });
  live.open(v.path, c?.rev, f => (f.screen ? show(v, f.screen) : v.patch.queue(f.ops)));
}
function unmount(v: View) { live.close(v.path); v.patch.drop(); }
```

The patcher queues the ops, patches at most once per 100 ms and keeps the reading position (q2039; `shapes.md` lists
what the page relies on). The page saves after a patch: localStorage, at most once every 2 s and on `pagehide`,
where it also calls `flush`, so the saved copy and its rev agree. A whole screen (`show`) drops the view's queued
ops: it is newer than all of them.

A lane sheet's `[older]` asks for the page before its first block and puts it on top; the patcher's anchor keeps
the place.

```ts
const page = await get(`${v.path}/older?before=${first(v, 'transcript')}`);
v.patch.queue(page.blocks.map(block => ({ op: 'insert', into: 'transcript', before: first(v, 'transcript'), block })));
```

## The tests

All in drover. Each drives `serve_on` on a socket; from slice 2 on, on #1312's large board (`common::large`: at
least 1,500 events, 150 issues, 10 lanes) in release. The diff's, the protocol's and the patcher's own cases
(`apply(a, ops(a, b)) == b`, the order rules, the anchor on a stand-in page) are tests in blocks, not here.

| Slice | Test | Asserts |
|---|---|---|
| 1 | every existing socket test | unchanged but for its `serve_on` call |
| 1 | `a_slow_request_does_not_hold_up_a_fast_one` | one handler held in its blocking thread; a screen GET still answers |
| 1 | `the_65th_request_waits_and_gets_its_answer` | 80 at once: 80 answers, no 503 |
| 1 | `a_client_that_sends_half_a_request_is_dropped_after_5s` | and holds no blocking thread meanwhile |
| 2 | `one_change_is_one_build_for_every_reader` | 30 streams and 30 GETs of one key, one event: the build count goes up by 1 |
| 2 | `fresh_data_is_under_150ms_p95_on_every_screen_and_sheet` | a GET of each screen and sheet; from slice 5 a stream `open` of each too |
| 2 | `ten_screens_across_three_devices_get_no_503` | 30 per-screen streams, 200 events: every delta arrives, no 503; from slice 5 on 3 device streams, each mirror equal to the newest build |
| 2 | `a_screen_get_answers_304_to_its_etag` | and 200 with a new ETag after a change |
| 3 | `a_lane_sheet_opens_without_a_shed_call` | the call-count probe, a local and a remote lane; block ids keep when a line arrives |
| 3 | `older_pages_back_200_lines_out_of_the_kept_tail` | no shed call; `more` is false at the first line |
| 3 | `the_tails_stay_under_their_bound` | 10 lanes at 5,000 lines: 16 MB at most, unwatched lanes shrink first |
| 4 | `no_fixture_screen_has_a_fault` | every screen and sheet: every row and card has an id, no id twice, no unknown kind, field or shape |
| 4 | `a_row_change_travels_as_row_ops` | a new ask, a merge, a queue move: no whole screen, no `replace` of a block that has children |
| 4 | `the_cards_get_a_change` | a new ask is an `insert` on the cards key (today: no op at all) |
| 4 | `cooees_stream_keeps_its_old_ops` | the golden cooee fixtures: no `move`, a reorder is a `replace` |
| 5 | `an_event_reaches_every_subscribed_phone_within_100ms_p95` | append an event, read each key's frame on 3 streams; one build per key, counted |
| 5 | `a_dropped_stream_catches_up_from_its_revs` | close the socket, append, reconnect: ops for a small gap, a whole screen past the ring |
| 5 | `a_fifth_stream_of_a_device_closes_its_oldest` | `bye evicted` on the oldest; other devices untouched |
| 5 | `a_lane_tail_streams_in_as_inserts` | new lines on an open lane sheet key: `insert` ops, no `remove` of the head |
| 5 | `serve_holds_its_budget_and_stops_growing_under_load` (`soak.rs`, changed) | 3 device streams with 10 keys each in place of 8 `/live` streams; the same 300 MB bound |
| 6 | `the_page_and_the_server_pin_one_blocks_commit` | the rev in `Cargo.lock` is the one in `web/package-lock.json` |
| 6 | `the_page_opens_one_event_source` | 3 tabs, a sheet and a thread open: one `/stream`, no per-screen stream |
| 6 | `the_queue_keeps_its_place_when_a_row_arrives_above` | scrolled into the Queue, a new ask on top: the row being read moves under 1 px |
| 6 | `a_queue_row_that_moves_keeps_its_node` | a queue move: one node moved, every other row's node untouched |
| 6 | `a_burst_of_events_is_at_most_one_patch_per_100ms` | 10 events in 50 ms: at most 2 patches; the page equals the newest screen |
| 6 | `a_lane_sheet_at_its_end_follows_new_lines_and_older_keeps_the_place` | the tail streaming in; `[older]` on top |
| 6 | `a_typed_draft_survives_a_patch` | a Send line with text in it, its row merged |
| 7 | `phone_timings.rs` (#1312's, extended) | event to painted under 150 ms p95 on the large board, as the page times it |
| 7 | `a_held_screen_builds_within_70ms_p95_on_the_live_shaped_fixture` | the Queue and each screen a tab keeps open, on #1313's fixture (`perf.rs`) |

Slice 6's run in WebKit at 390 px through `crates/cli/tests/support/webview.mjs`, in the ui-webkit job, on the real
page with drover's renderer and data.
