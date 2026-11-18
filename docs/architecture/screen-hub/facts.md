# screen-hub: what was measured (#1318)

## How

A throwaway test (not committed: an architect writes no product code) built a fixture, served it with
`Server::respond` and timed each screen in release, on staging `9ea69d9fb`. It added timers around the Queue's row
loops and a counting allocator; both were reverted. Slice 7 commits the gates that replace it.

```
cargo test --release -p drover --test adr1318 -- --nocapture --test-threads 1
```

The phone-timings issue (#1312) defines the large fixture, and it is not on staging yet. The stand-in: 1,656 events, 201
issues over 3 fleets (90 queued, 90 merged, 11 in review or ready, 10 with a live lane), 10 lanes with 400-line screens, 6 open
asks, 3 threads of 40 messages, an issue and a PR cache file per issue. The host was busy (load average 14 to 40,
other lanes building), so each p95 includes waiting for a CPU.

## Every screen, built whole (100 builds each)

| Screen | JSON bytes | rows + cards | build p50 ms | p95 ms | allocated per build | parsed tree | diff ms |
|---|---|---|---|---|---|---|---|
| `/screens/queue` | 20,077 | 45 | 8.5 | 11.9 | 13.5 MB | 239 KB | 0.34 |
| `/screens/queue?repo=sendmeter` | 7,612 | 15 | 7.8 | 10.4 | 11.2 MB | 91 KB | 0.12 |
| `/screens/queue?more=…` (every row shown) | 70,808 | 126 | 9.5 | 12.8 | 16.4 MB | 889 KB | 1.16 |
| `/screens/cards` | 4,219 | 6 | 1.1 | 1.8 | 4.6 MB | 42 KB | (none: below) |
| `/screens/lanes` | 2,364 | 11 | 0.9 | 1.9 | 2.5 MB | 25 KB | 0.02 |
| `/screens/events` | 46,876 | 186 | 3.3 | 5.1 | 4.4 MB | 496 KB | 0.55 |
| `/screens/threads` | 1,943 | 3 | 0.6 | 1.2 | 2.3 MB | 25 KB | 0.02 |
| `/screens/threads/t1` | 20,165 | 31 | 0.5 | 1.1 | 2.7 MB | 68 KB | 0.06 |
| `/screens/issue?repo=…&issue=…` | 2,414 | 14 | 5.6 | 7.4 | 5.8 MB | 28 KB | 0.03 |
| `/screens/lanes/<agent>` (lane sheet) | 9,581 | 6 | 1.4 | 2.8 | 5.0 MB | 24 KB | 0.01 |
| `/screens/herd` | 17,324 | 101 | 2.3 | 4.6 | 3.7 MB | 160 KB | 0.09 |
| `/screens/fleets` | 1,977 | 0 | 0.2 | 0.2 | 0.6 MB | 16 KB | 0.01 |
| **all 12** | **205 KB** | | **41.7** | **61.2** | **72.7 MB** | **2.1 MB** | **2.4** |

- A build right after a new event (the log folds only that event) costs the same: Queue 9.9 ms p50, 13.6 ms p95.
- A second run under heavier load: Queue 11.4 ms p50, 14.1 ms p95.
- `db::last_seq`, the change check: 7 to 11 µs a call.
- One new ask changes the Queue by 14 ops, 2,150 bytes, against 20,077 for the whole screen.

## Inside one Queue build (mean of 20, 13.6 ms)

| Part | ms | Note |
|---|---|---|
| `queue_tab`: the rows' data | 7.2 | 111 rows, 0.049 ms a row; each row reads and parses 4 to 6 cache files |
| `fleet_card`: the rows' blocks | 2.8 | `row()` runs for all 111 rows (0.022 ms each); 45 are shown |
| the rest: lane list, stats, the badge, JSON | 3.6 | the config is loaded three times |

So one row costs 0.07 ms to rebuild. That is the floor for a fully incremental Queue.

## Two more fixtures (40 builds each)

| Fixture | Queue p50 ms | p95 ms | allocated per build |
|---|---|---|---|
| ten times the history: 19,362 events, 1,563 issues, the same open work | 12.4 | 20.5 | 13.6 MB |
| each cache file 14 KB larger | 11.3 | 14.2 | 32.0 MB |

In the first of these, other screens' p95 reached 139 ms while their p50 stayed under 26 ms: the tail is the busy
host, not the build.

## What the code does today (read, and checked where a number is given)

- Each stream is a thread that polls the log every 500 ms and rebuilds its whole screen on any new event, and
  every 30 s (`screens.rs`, `stream`). At most 4 streams exist; the fifth gets 503 `no stream`.
- A navigation builds the screen twice: the GET, then the stream's first `screen` op.
- The cards stream never sends a change. The cards are a bare list, and the diff of two bare lists is empty:
  `ops([card a], [card a changed, card b])` returned 0 ops in the test. The cards only change on a reload.
- The page, on each delta: copies the whole model through JSON, applies the ops to the copy, writes the whole
  screen to localStorage, renders every block again and morphs the result into the page (`main.ts`, `stream` and
  `paint`). Each block rendered is turned to JSON once more as its redraw key (`blocks.ts`, `render`).
- A sheet (issue, lane, ask) is fetched once and has no stream. A lane sheet's GET reads up to 5,000 lines from
  shed and windows them; its blocks are numbered by position, so every block's id shifts when a line arrives.
- A thread screen's build marks the thread read, so a shared build cannot keep that side effect inside it.

## The live instance

Read-only counts from the live database: 448,066 events (376,597 not archived), 1,502 issues (117 open), a 518 KB
board snapshot, 4,963 cache files (141 MB, 28 KB a file on average). The issue reports about 360 ms for its Queue
build. The fixture's is 9 to 11 ms, 30 to 40 times less. Copying the live state to measure where that time goes was
refused by the lane hook, so it is not measured here. #1313 (in impl, amendment q2042) finds and fixes the live
cause on an owner-approved copy and on a fixture shaped like live; the hub's per-key build times in
`/api/v1/health` (slice 3) then show it on the instance itself.

## Read for the amendments (q2039, q2040, q2044): not measured

- **A row that changes place.** The diff gives up on a container whose shared children changed order and emits a
  `replace` of that container (`screens.rs`, `children` and `walk`). A Queue row that moves replaces its card.
- **The reading position.** The page keeps an anchor only for a chat's scroller (`main.ts`, `spot` and `land`);
  a tab is morphed whole and the window's scroll offset is put back as a number.
- **Blocks.** Every builder writes `json!` by hand; there is no Rust type for a block. 22 kinds are in use.
- **serve's threads.** One scoped thread per connection, up to 256, then a 503 (`serve.rs`, `serve_with`); `Server`
  borrows its `Ctx` and runner for the scope. `/live` spawns a thread per client. The background loops (the host
  sampler, one per background read, the SLO pass, the tick, the tap jobs) are scoped threads too.
- **tokio in the repo.** `drover service` runs on it with `Arc<Service>`, an owned `Ctx`, a `&'static dyn Runner`,
  `spawn_blocking` around every SQLite read and a hand-written request reader. The workspace's tokio features
  (`rt-multi-thread`, `net`, `io-util`, `time`, `sync`) have no `macros`, so no `select!`.
- **topcoat.** cooee pins 0.11.0. Its runtime guide and `browser/src/render/shard.ts` were read; nothing was run.
- **The line cap.** `drover cap`: 53,086 of 53,500 at staging `486f415`, headroom 414; 187 with the open PRs.
- **Neighbours.** #1312 (phone timings, in impl) defines the fixture and the tap-to-painted numbers. #1313 (in
  impl) gates each build at 150 ms p95. #1314 (queued) adds gzip and keep-alive to the connection code.

Nothing was run on an async serve: an architect writes no product code. Slice 2's gate is the existing socket
tests, and every later gate is measured on the async serve because it lands first.
