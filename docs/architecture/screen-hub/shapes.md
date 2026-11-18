# screen-hub: the data shapes (#1318)

Two parts. First what #1318 builds, all in serve: the key, the signals, the hub, the device stream's routes, the
lane tails, every bound. Then what it takes from the blocks repo, as drover calls it (q2071): the ops, the frames,
both ends' bookkeeping and the page's two functions. blocks#2's ADR owns that second part; where the pinned crate
differs from it, the crate is right.

## serve: the cache key

```rust
/// One screen as every client of its kind sees it.
pub struct Key {
    path: String,     // canonical: "/screens/queue?repo=berth" (query keys sorted, empty values dropped)
    paired: bool,     // a paired device or the plugin token sees more (lane sheets, `other in shed`)
}
```

Nothing else varies a screen today: `[later]` is logged, not per device. A new per-device input must become a key
field, never a branch inside a builder. The wire's `k` is the path; `paired` is the stream's own.

## serve: what makes a key stale

```rust
/// Counters the publisher reads each pass. A key is stale when one it depends on moved since its build.
pub struct Signals {
    log: u64,        // db::last_seq
    snapshot: u64,   // db::stamp("board")
    reads: u64,      // Fresh: bumped when a background read's output changes (shed's list, a host's reads)
    config: u64,     // fleet.toml's mtime
    files: u64,      // the gh cache folder's mtime (PR, issue, backlog and cap files are renamed into it)
    clock: u64,      // now / 30: ages and countdowns are text the server writes
}
pub struct Deps(u8); // LOG | SNAPSHOT | READS | CONFIG | FILES | CLOCK | TAIL
```

| Screens | Deps |
|---|---|
| every screen not named below | all but `TAIL` |
| `/screens/threads`, `/screens/threads/<id>` | `LOG`, `READS`, `CONFIG`, `CLOCK` |
| `/screens/lanes/<agent>` | all, with `TAIL` (that lane's tail generation; from slice 3) |
| `/screens/legend`, `/screens/workflow`, `/screens/proxy-token` | `CONFIG` |

A missed dependency heals in 30 s at most, as today: every screen that shows data depends on `CLOCK`.

## serve: the hub and a stream's queue

```rust
pub struct Hub {
    shelf: Mutex<Shelf<Key>>,  // the crate's, from slice 4; in slices 2 and 3 a map of Key to (rev, JSON bytes)
    meta: Mutex<BTreeMap<Key, Meta>>,
    kick: Notify,              // serve's own writes and a tail's new lines wake the publisher
    builds: Semaphore,         // 2: the publisher's and one reader's
    epoch: String,
}
struct Meta {
    seen: Signals,             // the signals it was built at
    holders: Vec<Weak<Queue>>, // the streams subscribed
    read_at: u64,
    building: Option<watch::Receiver<()>>,   // single flight: other readers await it
    ms: [u32; 32],             // its last build times (health)
    faults: Vec<Fault>,        // the crate's check, run after publishing (health; from slice 4)
}
/// One stream's outbound frames; the hub pushes, the stream's task writes.
struct Queue { q: Mutex<Out>, wake: Notify }
struct Out { frames: VecDeque<Arc<str>>, bytes: usize, lag: bool, bye: Option<Why> }
```

The hub keeps JSON bytes only: a screen's parsed tree is 12 times larger (`facts.md`).

## serve: the device stream

The routes, the cookie check, the sessions and the limits are serve's. The frames' types are the crate's (below).

```
GET /stream                      Cookie `hd=…` or the plugin's bearer token; Host/Origin checked as for a GET
  event: hello
  data: {"v":1,"sid":"<22 chars>","epoch":"<8 hex>","keys":16}

  data: {"k":"/screens/queue","from":7,"rev":8,"ops":[…]}        a delta: apply only on rev 7
  data: {"k":"/screens/lanes/berth-impl-12","rev":3,"screen":{…}} no `from`: replaces the phone's copy

  event: bye
  data: {"why":"evicted"}                                         evicted | unpaired | handover
  : ping                                                          every [limits] serve_heartbeat_secs

POST /stream/<sid>   {"open":[{"k":"/screens/queue","rev":7}],"close":["/screens/issue?issue=12&repo=berth"]}
  200 {"keys":[{"k":"/screens/queue","rev":8,"ops":[…]},          7 to 8, from the ring
               {"k":"…","rev":8,"screen":{…}},                    no rev sent, or too old for the ring
               {"k":"…","rev":7},                                 current
               {"k":"…","gone":404}]}                             404 no such screen · 401 pair first · 429 over 16 keys
  404 {"line":"no such stream"}                                   not this device's live stream: reconnect
```

**Resume.** The server keeps nothing about a stream once its socket closes, so a reconnect, a serve restart and a
handover are one case:

1. Every connection starts with `hello`. `epoch` names this serve process's hub.
2. The client answers with one `POST` that opens every mounted key. It sends a key's `rev` only when its copy came
   from the same `epoch`.
3. Per key the reply is nothing (current), the missed ops (within the ring) or the whole screen.

**Order.** No ordering is assumed between the `POST` reply and the stream:

- a frame applies only when `from` equals the rev the client holds; `rev` at or below it is dropped;
- `from` above it is a gap: the client opens that key again with its rev;
- a `POST` reply applies when it is a `screen`, or when the client still holds the rev it sent.

`GET /screens/<x>` carries `ETag: "<epoch>.<rev>"` and answers `If-None-Match` with 304 (slice 2); the first paint
(before `hello`) uses it and joins the first sync with that rev.

## serve: the lane tail

```rust
pub struct Tails { lanes: Mutex<BTreeMap<String, Tail>> }
struct Tail {
    gen: u32,                 // +1 when a read no longer lines up with the kept lines (the terminal was cleared)
    first: u64,               // the number of lines[0]
    lines: VecDeque<String>,  // masked; a line's number is first + its index
    watched_at: u64,          // a lane sheet was open on it
    read_at: u64,
    more: bool,               // shed has lines before `first`
}
pub struct TailPage { blocks: Vec<Value>, more: bool }   // `GET /screens/lanes/<agent>/older?before=t<gen>-<n>`
```

An ACP lane's line numbers are shed's event seqs. A PTY lane's are counted by serve: each read's head is matched
against the kept tail (at least 3 lines), new lines are appended, and no match is a new `gen`. A transcript block's
id is `t<gen>-<its first line's number>`, so a line that arrives shifts no other block's id; its section carries
`log: true`.

## Bounds

| What | Bound | When it is hit |
|---|---|---|
| connections | 256 | 503, as today |
| requests in `spawn_blocking` | 64 | the next waits for a slot (today: a thread each, up to 256) |
| builds at once | 2 | the next waits; a second reader of one key awaits the first build |
| streams per device (a paired device's hash, the plugin, else the peer address) | 4 | the oldest gets `bye evicted` |
| keys per stream | 16 | that `open` answers `gone: 429` |
| a stream's queue | 64 frames or 1 MB | the queue is dropped; each key's next frame is a whole screen |
| a key's ring | 16 deltas or 64 KB | an older phone gets the whole screen |
| keys with no holder | 64, each dropped 60 s after its last read | least recently read goes first |
| lane tail, watched in the last 10 min | 5,000 lines or 512 KB | the oldest lines go (`more` stays true) |
| lane tail, not watched | 200 lines | |
| all lane tails | 16 MB | unwatched lanes shrink first |
| the page's queued ops (a hidden page) | 500 | the key is opened again without its rev: one whole screen |
| the page's patch rate | one per 100 ms per view | later ops wait for the next patch |
| the page's kept copies (localStorage) | 24 keys | least recently opened goes first |

There is no server-wide stream cap. The ring's bound is the crate's `Shelf`'s and the page's two patch bounds are
the patcher's; the rest are constants in serve's modules.

## From blocks: what drover calls

Not built by #1318. These are the shapes the earlier versions of this ADR drew and blocks#2's draft reuses; they
are kept here because serve's and the page's code is written against them. blocks#2's draft adds to them: `after`
on an insert, a `screen` op for an action's answer, a rule for an op the client does not know, the schema version
as `hello.v`, and an app's own kinds as an argument to the check.

**The pin.** One commit, merged on blocks' `staging`, named twice in drover; a test fails when the two differ.

```toml
blocks = { git = "https://github.com/jirathip-dev/blocks", rev = "<40 hex>" }     # Cargo.toml, slice 4; deny.toml allows the source
```
```json
"@jirathip-dev/blocks": "github:jirathip-dev/blocks#<the same 40 hex>"
```

The second line comes with slice 6. drover's esbuild bundles the package into `web/dist`, and its `tsc --strict`
reads it.

### A built screen and its faults (N1)

```rust
/// A built screen as the diff reads it. Always constructible; its faults say what is wrong with it.
pub struct Built { /* the JSON, and the faults one walk finds */ }
pub enum Fault {
    Twice { id: String },                    // two blocks with one id: an op would be ambiguous
    Unnamed { kind: String, under: String }, // a row or card without an id: its change would replace its parent
    Unknown { kind: String },                // a kind the typed model lacks
    Shape { id: String, why: String },       // a known kind that does not read: a field missing or of another type
    Extra { id: String, field: String },     // a field the typed model lacks
}
impl Built {
    pub fn new(v: Value) -> Built;           // the first two faults, on every build
    pub fn check(&self) -> Vec<Fault>;       // the last three: reads the screen into the typed blocks
}
```

drover's builders keep writing JSON in #1318. The hub runs `check` after a build is published, so no phone waits
for it; health lists each key's faults and `no_fixture_screen_has_a_fault` fails on one. That is how "row ops are
required" is enforced on drover's side: a row without an id is a fault. A screen with a `Twice` fault is sent whole
on every change; the others still diff.

### Ops (N1)

```rust
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Merge { id: String, fields: Map<String, Value> },             // a block's own fields that changed
    Insert { into: String, before: Option<String>, block: Value },
    Move { id: String, into: String, before: Option<String> },    // a block changed place among its siblings
    Remove { id: String },
    Replace { id: String, block: Value },                         // a block lost a field, or has unnamed children
    Badge { count: u64 },                                         // the tab's count
    Dot { on: bool },                                             // cooee's unread dot; the diff never writes it
}
```

What serve relies on from the diff:

- Removes come first. Then each container's `insert` and `move` ops, last child first, each `before` its next
  sibling in the new order: every `before` names a node already in its place.
- Only rows outside the longest run that kept its order move. One row sent to the top of 45 is one `move`.
- A bare list (the cards) diffs under a root, `{"id": "root", "blocks": [...]}`, on both ends; its JSON on the wire
  stays the bare list. Today its diff is always empty (`facts.md`).
- `log: true` on a container: the diff never removes a child that left its head (a transcript's window slides).
- A whole screen is not an op: it is a frame with `screen`.
- `wire::old(ops, new)` gives the per-screen stream today's ops: a `move` becomes a `replace` of its parent, a
  changed list root a whole `screen`. It goes with that route.

### Frames and both ends' bookkeeping (N1)

```rust
pub struct Hello { v: u8, sid: String, epoch: String, keys: usize }
pub struct Frame { k: String, from: Option<u64>, rev: u64, ops: Option<Vec<Op>>, screen: Option<Value> }
pub struct Bye { why: Why }                                       // Evicted | Unpaired | Handover
pub struct Sync { open: Vec<Have>, close: Vec<String> }
pub struct Have { k: String, rev: Option<u64> }
pub struct Synced { keys: Vec<KeyState> }
pub struct KeyState { k: String, rev: Option<u64>, ops: Option<Vec<Op>>, screen: Option<Value>, gone: Option<u16> }

/// The server's half: one entry per key, JSON bytes only.
pub struct Shelf<K> { /* per key: rev, JSON, a ring of deltas (16 deltas or 64 KB) */ }
pub struct Delta { pub from: u64, pub rev: u64, pub frame: Arc<str> }   // serialized once, for every stream
pub enum Put { Same, First, Changed(Delta) }
impl<K: Ord + Clone> Shelf<K> {
    pub fn put(&mut self, k: &K, wire: &str, new: &Built) -> Put;        // parses the kept JSON (0.06 to 0.4 ms), diffs
    pub fn state(&self, k: &K, wire: &str, have: Option<u64>) -> KeyState; // current, the ring's ops, or the screen
    pub fn forget(&mut self, k: &K);
}

/// A client's half. serve's tests play the phone with it; the page's stream client follows the same rules.
pub struct Mirror { /* the epoch, and per key the rev held and the screen */ }
pub enum Took { Patched, Replaced, Old, Gap }                     // Gap: open the key again with the rev held
impl Mirror {
    pub fn hello(&mut self, h: &Hello);                           // a new epoch forgets every rev
    pub fn have(&self) -> Vec<Have>;
    pub fn frame(&mut self, f: Frame) -> Took;
    pub fn synced(&mut self, sent: &[Have], s: Synced);
}
```

`api.rs` holds no copy of these and `just types` does not write them: the page imports the package's generated types.

### The page's two functions (N2)

```ts
export type State = 'off' | 'connecting' | 'syncing' | 'live' | 'waiting' | 'parked';
export function stream(o: { url: string; state?: (s: State) => void }): {
  open(k: string, rev: number | undefined, on: (f: Frame) => void): void;
  close(k: string): void;
};
export type Draw = (block: Block) => HTMLElement | null;
export function patcher(o: { root: HTMLElement; model: () => Block; draw: Draw; saved?: () => void }): {
  queue(ops: Op[]): void;      // patched at most once per 100 ms, on an animation frame
  flush(): void;               // now: on `pagehide`
  drop(): void;                // a whole screen arrived, or the view went
};
```

What the page relies on from the patcher (q2039), and what drover's WebKit tests check on the real page:

- one patch per 100 ms per view at most; a lone frame waits only for the next animation frame;
- a patch touches the nodes its ops name and no other;
- the first row in view that the patch neither moves nor removes stays where it was; at the very top a new row
  shows at the top; a `log` container at its end stays at its end;
- it draws nothing itself: `draw` gives one node for one block. Until blocks' renderer replaces it (`adr.md`, open
  question 1), `draw` is drover's `blocks.ts`.
