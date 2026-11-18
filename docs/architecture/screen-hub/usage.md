# screen-hub: how the callers read (#1318)

Sketches, not code to copy. The names are the ones `shapes.md` and `boundaries.md` use.

## The server

A GET reads the cache. It builds only when the key is stale and nobody else is building it.

```rust
("GET", p) => match self.hub.read(ctx, &Key::of(p, query, paired)) {
    Some(b) => cached(&b, head),                  // 200 with b.json and its ETag, or 304
    None => line("404 Not Found", "no such screen"),
},
```

One thread publishes. It is the only place a subscribed screen is built.

```rust
sc.spawn(move || loop {
    hub.pass(&at(crate::now()));                  // read the signals; build, diff and publish each stale subscribed key
    hub.wait(POLL);                               // 25 ms, or at once on `hub.kick()`
});
```

An action that wrote something wakes it, so the phone does not wait for the poll.

```rust
let res = self.action(ctx, name, &a);
self.hub.kick();
```

The device stream is two routes. Neither builds a screen.

```rust
("GET", "/stream") => {
    let s = self.streams.attach(&self.hub, who, paired);      // past 4 for this device: its oldest gets `bye`
    s.run(&mut socket)                                        // `hello`, then frames until the socket or session ends
}
("POST", p) if p.starts_with("/stream/") => {
    let sync: Sync = serde_json::from_str(body)?;
    out("200 OK", &self.streams.sync(&self.hub, sid(p), who, &sync)?)   // opens, closes; each opened key's state
}
```

cooee's per-screen stream stays, as a stream with one fixed key, in the old `{seq, ops}` framing.

```rust
("GET", p) if p.ends_with("/stream") => self.streams.single(&self.hub, Key::of(base(p), query, paired), &mut socket),
```

A screen builder is what `Server::screen` is today, renamed, plus what it depends on.

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

```rust
("GET", p) if older(p) => out("200 OK", &self.tails.page(agent, before, 200)),   // from the kept lines only
```

## The page

One stream for the page. A view (a tab, the cards, a sheet, a thread) opens its key when it mounts and closes it
when it goes.

```ts
const live = stream({ state: sync });                        // the one EventSource; the sync dot follows it

function mount(v: View) {
  const c = cached(v.path);                                  // the phone's last copy, painted at once
  if (c) show(v, c.v);
  live.open(v.path, c?.rev, f => (f.screen ? show(v, f.screen) : patch(v, f.ops)));
}
function unmount(v: View) { live.close(v.path); }
```

A frame patches the nodes it names. Nothing is cloned and the screen is not rendered again.

```ts
export function patch(v: View, ops: Op[]): void {
  for (const op of ops) {
    if (op.op === 'merge') redraw(v, op.id, Object.assign(v.byId(op.id), op.fields));   // one node
    else if (op.op === 'insert') place(v, op.into, op.before, render(op.block, v.ctx)); // one new node
    else if (op.op === 'remove') v.node(op.id)?.remove();
    …
  }
  save(v);                                                   // localStorage, at most once every 2 s and on pagehide
}
```

A lane sheet's `[older]` asks for the page before its first block and puts it on top.

```ts
const page = await get(`${v.path}/older?before=${first(v, 'transcript')}`);
patch(v, page.blocks.map(block => ({ op: 'insert', into: 'transcript', before: first(v, 'transcript'), block })));
```

## The acceptance tests (slice 5)

Each drives `serve_on` on a socket, on the large fixture (at least 1,500 events, 150 issues, 10 lanes), in release.

| Test | Asserts |
|---|---|
| `fresh_data_is_under_150ms_p95_on_every_screen_and_sheet` | a GET and a stream `open` of each screen and sheet |
| `an_event_reaches_every_subscribed_phone_within_100ms_p95` | append an event, read each key's frame on 3 streams; one build per key, counted |
| `ten_screens_across_three_devices_get_no_503` | 3 streams, 10 keys, 200 events: every frame arrives, in order, no 503 |
| `a_dropped_stream_catches_up_from_its_revs` | close the socket, append, reconnect: ops for a small gap, a whole screen past the ring |
| `a_fifth_stream_of_a_device_closes_its_oldest` | `bye evicted` on the oldest; other devices untouched |
| `the_cards_get_a_change` | a new ask is an `insert` on the cards key (today: no op at all) |
| `a_lane_sheet_opens_without_a_shed_call_and_its_tail_streams_in` | the call-count probe; new lines arrive as `insert` ops |
| `serve_holds_its_budget_and_stops_growing_under_load` (`soak.rs`, changed) | 3 device streams with 10 keys each in place of 8 `/live` streams; the same 300 MB bound |
