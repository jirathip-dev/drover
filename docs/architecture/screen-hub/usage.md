# screen-hub: how the callers read (#1318)

Sketches, not code to copy. The names are the ones `shapes.md` and `boundaries.md` use.

## The crate, from serve

The crate lives in the blocks repo and knows nothing of serve. drover names one commit of it, twice:

```toml
blocks = { git = "https://github.com/jirathip-dev/blocks", rev = "<40 hex>" }      # Cargo.toml
```
```json
"@jirathip-dev/blocks": "github:jirathip-dev/blocks#<the same 40 hex>"
```

serve hands the crate screens and gets ops, frames and key states back.

```rust
use blocks::{diff::ops, patch::apply, Built, Shelf, wire::{Frame, Sync, Synced}};

let new = Built::new(self.build_screen(ctx, &key)?);           // never fails: the builders still write JSON
match shelf.put(&key, &key.path, &new) {
    Put::Changed(delta) => holders.for_each(|q| q.push(&delta.frame)),   // one serialized frame for every stream
    Put::First | Put::Same => {}
}
meta.faults = new.check();                                     // after publishing: read into the typed blocks, for health
let state = shelf.state(&key, &key.path, have.rev);            // current, the missed ops, or the whole screen
```

Its main test needs no server at all:

```rust
let mut phone = a.clone();
apply(&mut phone, &ops(&a, &b));
assert_eq!(phone, b);                                          // for every pair of fixture screens, and each reorder
```

## The server, on the runtime

One task per connection. It takes a blocking thread only for today's handler or a build.

```rust
async fn conn(self: Arc<Self>, mut c: TcpStream) {
    let Ok(_conn) = self.conns.clone().try_acquire_owned() else { return busy(&mut c).await };   // 256: 503, as today
    let Ok(Ok(req)) = timeout(READ, request(&mut c)).await else { return };                      // 5 s, today's size limits
    match route(&req) {
        Route::Stream => self.streams.attach(&self, &req, c).await,                 // a task until the socket ends
        Route::Sync(sid) => reply(c, self.streams.sync(&self.hub, sid, &req).await).await,
        Route::Screen(key) => reply(c, cached(self.hub.read(&self, &key).await, &req)).await,   // 200 with ETag, or 304
        Route::Other => {
            let _slot = self.reqs.acquire().await;                                  // 64 at a time; the rest wait here
            let me = self.clone();
            reply(c, spawn_blocking(move || me.respond(&req)).await).await          // today's handler, whole
        }
    }
}
```

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

cooee's per-screen stream stays, as a stream with one fixed key, in the old `{seq, ops}` framing and the old ops.

```rust
Route::Old(key) => self.streams.single(&self.hub, key, c).await,       // each delta through `wire::old`
```

A screen builder is what `Server::screen` is today, renamed, plus what it depends on. It stays synchronous.

```rust
pub(super) fn build_screen(&self, ctx: &Ctx, k: &Key) -> Option<Value>     // pure: no socket, no mark-read
pub(super) fn deps(path: &str) -> Deps                                      // when in doubt: Deps::ALL
```

The lane sheet reads its transcript from the tail cache and never from shed.

```rust
let tail = self.tails.tail(agent, 200);           // the last 200 lines, each with its number
let said = blocks(&tail).map(|b| said(&b));       // a block's id is `t<gen>-<its first line's number>`
json!({"type": "section", "id": "transcript", "log": true, "blocks": said})
```

Two entries with the same arguments, owned as `service::run`'s are. `drover service` awaits the first on its own
runtime; `drover serve` and the tests call the second, which builds a runtime (2 workers) and blocks on the first.

```rust
pub async fn run(ctx: Ctx, r: &'static dyn Runner, l: TcpListener, read_only: bool, ho: Option<Arc<Handover>>) -> Result<()>
pub fn serve_with(ctx: Ctx, r: &'static dyn Runner, l: TcpListener, read_only: bool, ho: Option<Arc<Handover>>) -> Result<()>
```

## The page

One stream for the page. A view (a tab, the cards, a sheet, a thread) opens its key when it mounts and closes it
when it goes. The stream and the patcher come from the blocks package; the page hands the patcher its own `render`.

```ts
import { patcher } from '@jirathip-dev/blocks/patch';
import { stream } from '@jirathip-dev/blocks/stream';
import type { Frame, Op } from '@jirathip-dev/blocks/types';
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

The rest of this section is the inside of the package's `patch.ts` (`v` is one patcher's state). Ops are queued and patched at most once per
100 ms, on an animation frame (q2039). A lone frame waits for nothing but the next animation frame.

```ts
const FRAME = 100;
export function queue(v: View, ops: Op[]): void {
  v.todo.push(...ops);
  if (v.due) return;                                              // a patch is already planned
  const wait = Math.max(0, v.last + FRAME - performance.now());
  v.due = setTimeout(() => requestAnimationFrame(() => flush(v)), wait);
}
```

A patch touches the nodes its ops name and keeps the reading position (q2039).

```ts
function flush(v: View): void {
  const ops = v.todo.splice(0);
  const hold = anchor(v, ops);                 // the first row in view these ops neither move nor remove, and its top
  for (const op of ops) {
    if (op.op === 'merge') redraw(v, op.id, Object.assign(v.byId(op.id), op.fields));    // one node
    else if (op.op === 'insert') place(v, op.into, op.before, v.draw(op.block));         // one new node
    else if (op.op === 'move') place(v, op.into, op.before, v.node(op.id));              // the same node, elsewhere
    else if (op.op === 'remove') v.node(op.id)?.remove();
    …
  }
  settle(v, hold);                             // scrollTop += how far that row shifted; a log at its end stays there
  [v.due, v.last] = [0, performance.now()];
  v.saved?.();                                 // the page saves: localStorage, at most once every 2 s and on pagehide
}
```

A whole screen (`show`) drops the view's queued ops: it is newer than all of them. `pagehide` patches at once, so
the saved copy and its rev agree.

A lane sheet's `[older]` asks for the page before its first block and puts it on top; the anchor keeps the place.

```ts
const page = await get(`${v.path}/older?before=${first(v, 'transcript')}`);
v.patch.queue(page.blocks.map(block => ({ op: 'insert', into: 'transcript', before: first(v, 'transcript'), block })));
```

## The tests

In the blocks repo, the crate's (B1; no server):

| Test | Asserts |
|---|---|
| `ops_then_apply_gives_the_new_screen` | for each pair of screens in `fixtures/`, and each reorder of a list: `apply(a, ops(a, b)) == b` |
| `a_changed_row_is_one_merge_of_its_changed_fields` | no `replace`, no whole screen |
| `a_row_that_changes_place_is_a_move` | one row to the top of 45: one `move`; two lists swapped: the fewest moves |
| `a_log_never_loses_its_head` | lines that left the window: no `remove` |
| `a_bare_list_diffs_under_its_root` | a new card is an `insert` into `root` |
| `a_mirror_drops_an_old_frame_and_asks_again_on_a_gap` | `fixtures/protocol/`: the order rules; a new epoch forgets every rev |
| `a_shelf_answers_current_ops_or_screen` | by the rev sent and the ring's bounds |
| `every_fixture_reads_into_the_typed_blocks` | cooee's and drover's goldens: no fault; written back, the same JSON |
| `an_unknown_kind_or_field_is_kept_and_is_a_fault_for_its_server` | read and written back unchanged; `check` names it |

In the blocks repo, the generator's and the package's (B2; the patcher's run in WebKit at 390 px on a stand-in
`draw`):

| Test | Asserts |
|---|---|
| `the_generated_files_are_current` | `just gen` changes nothing; `Blocks.swift` builds and decodes every fixture |
| `the_stream_follows_the_mirrors_cases` | `stream.ts` on `fixtures/protocol/`: the same answers as `Mirror` |
| `a_burst_of_frames_is_one_patch_per_100ms` | 10 frames in 50 ms: at most 2 patches; the page equals the newest screen |
| `a_lone_frame_paints_on_the_next_animation_frame` | no 100 ms wait after a quiet spell |
| `a_patch_touches_only_the_nodes_its_ops_name` | a mutation observer: every other row keeps its node, untouched |
| `an_insert_above_the_viewport_keeps_the_reading_position` | scrolled to a row; 3 rows arrive above it: its top moves under 1 px |
| `a_move_across_the_viewport_keeps_the_reading_position` | a row from below to above, and from above to below |
| `a_removed_anchor_falls_to_the_next_row_in_view` | the first row in view is removed: the next one holds |
| `at_the_top_a_new_row_shows_at_the_top` | no scroll is added when nothing is above the viewport |
| `a_log_at_its_end_stays_at_its_end_and_older_lines_keep_the_place` | new lines at the end; a page of older ones on top |

In drover, the pin (slice 1) and serve on tokio (slice 2): every existing socket test, unchanged but for its
`serve_on` call, plus:

| Test | Asserts |
|---|---|
| `the_page_and_the_server_pin_one_blocks_commit` | the rev in `Cargo.lock` is the one in `web/package-lock.json` |
| `a_slow_request_does_not_hold_up_a_fast_one` | one handler held in its blocking thread; a screen GET still answers |
| `the_65th_request_waits_and_gets_its_answer` | 80 at once: 80 answers, no 503 |
| `a_client_that_sends_half_a_request_is_dropped_after_5s` | and holds no blocking thread meanwhile |

Each of these drives `serve_on` on a socket, on the large fixture (at least 1,500 events, 150 issues, 10 lanes), in
release, with `blocks::Mirror` as the phone (slices 3 to 7):

| Test | Asserts |
|---|---|
| `fresh_data_is_under_150ms_p95_on_every_screen_and_sheet` | a GET and a stream `open` of each screen and sheet |
| `an_event_reaches_every_subscribed_phone_within_100ms_p95` | append an event, read each key's frame on 3 streams; one build per key, counted |
| `ten_screens_across_three_devices_get_no_503` | 3 streams, 10 keys, 200 events: every frame arrives, in order, no 503; each mirror equals the newest build |
| `no_fixture_screen_has_a_fault` | every screen and sheet: every row and card has an id, no id twice, and `check` finds no unknown kind, field or shape |
| `a_row_change_travels_as_row_ops` | a new ask, a merge, a queue move: no whole screen, no `replace` of a block that has children |
| `a_dropped_stream_catches_up_from_its_revs` | close the socket, append, reconnect: ops for a small gap, a whole screen past the ring |
| `a_fifth_stream_of_a_device_closes_its_oldest` | `bye evicted` on the oldest; other devices untouched |
| `the_cards_get_a_change` | a new ask is an `insert` on the cards key (today: no op at all) |
| `cooees_stream_keeps_its_old_ops` | the golden cooee fixtures: no `move`, a reorder is a `replace` |
| `a_lane_sheet_opens_without_a_shed_call_and_its_tail_streams_in` | the call-count probe; new lines arrive as `insert` ops |
| `serve_holds_its_budget_and_stops_growing_under_load` (`soak.rs`, changed) | 3 device streams with 10 keys each in place of 8 `/live` streams; the same 300 MB bound |

In drover, the real page (slice 5; WebKit at 390 px through `crates/cli/tests/support/webview.mjs`, in the ui-webkit
job). The patcher's own cases are the package's; these check it with drover's renderer and data:

| Test | Asserts |
|---|---|
| `the_queue_keeps_its_place_when_a_row_arrives_above` | scrolled into the Queue, a new ask on top: the row being read moves under 1 px |
| `a_queue_row_that_moves_keeps_its_node` | a queue move: one node moved, every other row's node untouched |
| `a_lane_sheet_at_its_end_follows_new_lines_and_older_keeps_the_place` | the tail streaming in; `[older]` on top |
| `a_typed_draft_survives_a_patch` | a Send line with text in it, its row merged |

The event-to-painted number itself is #1312's (phone timings): this issue's gate reads it on the fixture, under
150 ms p95, and on live data once #1313 has landed (`adr.md`, open question 3).
