# screen-hub: the data shapes (#1318)

The wire types (`Op`, `Frame`, `Hello`, `Sync`, `Synced`) go in `crates/server/src/api.rs`, so `just types` writes
them to `assets/types/api.d.ts` and the page imports them. The rest is private to its module.

## The cache key

```rust
/// One screen as every client of its kind sees it.
pub struct Key {
    path: String,     // canonical: "/screens/queue?repo=berth" (query keys sorted, empty values dropped)
    paired: bool,     // a paired device or the plugin token sees more (lane sheets, `other in shed`)
}
```

Nothing else varies a screen today: `[later]` is logged, not per device. A new per-device input must become a key
field, never a branch inside a builder.

## What makes a key stale

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
| `/screens/lanes/<agent>` | all, with `TAIL` (that lane's tail generation) |
| `/screens/legend`, `/screens/workflow`, `/screens/proxy-token` | `CONFIG` |

A missed dependency heals in 30 s at most, as today: every screen that shows data depends on `CLOCK`.

## A cache entry

```rust
pub struct Built {
    pub rev: u64,          // +1 each time the screen's JSON changes
    pub json: Arc<[u8]>,   // what a GET sends; the next diff parses it again (0.06 to 0.4 ms)
    seen: Signals,         // the signals it was built at
    ms: u32,               // how long the build took (health)
}
struct Entry {
    built: Option<Arc<Built>>,
    ring: VecDeque<Delta>, // the last deltas, for a phone that is a little behind
    subs: u32,             // streams subscribed
    read_at: u64,
    building: bool,        // single flight: other readers wait on the hub's condvar
}
pub struct Delta { from: u64, rev: u64, ops: Arc<str> }
```

Only the JSON bytes are kept: the parsed tree is 12 times larger (Queue: 239 KB against 20 KB).

## Delta ops

Today's ops, typed, with one fix and one addition.

```rust
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Screen { screen: Value },                                     // the whole screen
    Merge { id: String, fields: Map<String, Value> },             // a block's own fields that changed
    Replace { id: String, block: Value },                         // a block lost a field, or its children reordered
    Remove { id: String },
    Insert { into: String, before: Option<String>, block: Value },
    Badge { count: u64 },                                         // the tab's count
}
```

- **Fix:** a screen that is a bare list (the cards) is diffed as `{"id": "cards", "blocks": [...]}`. Today its diff
  is always empty, so the cards stream never sends a change (`facts.md`). `GET /screens/cards` still answers the
  bare list.
- **Addition:** a container with `"log": true` (a lane sheet's transcript) is a window that slides. The diff never
  removes a child that left the head of the window. The page keeps at most 1,000 children and drops from the top.
- A block's id is stable for its life. Transcript blocks are `t<gen>-<first line number>`, never an index.

## The device stream

```
GET /stream                      Cookie `hd=…` or the plugin's bearer token; Host/Origin checked as for a GET
  event: hello
  data: {"sid":"<22 chars>","epoch":"<8 hex>","keys":16}

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

```rust
pub struct Hello { sid: String, epoch: String, keys: usize }
pub struct Frame { k: String, from: Option<u64>, rev: u64, ops: Option<Vec<Op>>, screen: Option<Value> }
pub struct Sync { open: Vec<Have>, close: Vec<String> }
pub struct Have { k: String, rev: Option<u64> }
pub struct Synced { keys: Vec<KeyState> }
pub struct KeyState { k: String, rev: Option<u64>, ops: Option<Vec<Op>>, screen: Option<Value>, gone: Option<u16> }
```

**Resume.** The server keeps nothing about a stream once its socket closes, so a reconnect, a serve restart and a
handover are one case:

1. Every connection starts with `hello`. `epoch` names this serve process's hub.
2. The page answers with one `POST` that opens every mounted key. It sends a key's `rev` only when its copy came
   from the same `epoch`.
3. Per key the reply is nothing (current), the missed ops (within the ring) or the whole screen.

**Order.** No ordering is assumed between the `POST` reply and the stream:

- a frame applies only when `from` equals the rev the page holds; `rev` at or below it is dropped;
- `from` above it is a gap: the page opens that key again with its rev;
- a `POST` reply applies when it is a `screen`, or when the page still holds the rev it sent.

`GET /screens/<x>` carries `ETag: "<epoch>.<rev>"` and answers `If-None-Match` with 304; the first paint (before
`hello`) uses it and joins the first sync with that rev.

## The lane tail

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
against the kept tail (at least 3 lines), new lines are appended, and no match is a new `gen`.

## Bounds

| What | Bound | When it is hit |
|---|---|---|
| streams per device (a paired device's hash, the plugin, else the peer address) | 4 | the oldest gets `bye evicted` |
| keys per stream | 16 | that `open` answers `gone: 429` |
| a stream's queue | 64 frames or 1 MB | the queue is dropped; each key's next frame is a whole screen |
| a key's ring | 16 deltas or 64 KB | an older phone gets the whole screen |
| keys with no subscriber | 64, each dropped 60 s after its last read | least recently read goes first |
| lane tail, watched in the last 10 min | 5,000 lines or 512 KB | the oldest lines go (`more` stays true) |
| lane tail, not watched | 200 lines | |
| all lane tails | 16 MB | unwatched lanes shrink first |
| the page's kept copies (localStorage) | 24 keys | least recently opened goes first |

There is no server-wide stream cap. The 256 connection threads stay the outer limit.
