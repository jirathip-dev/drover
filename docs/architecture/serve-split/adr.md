# ADR: the page's server is a client of drover's API (#1645)

## Summary

Decision: today's server is cut in two. The part that reads state, builds screens and runs actions becomes drover's API, in its own crate and its own process (a new `api` command). What is left as `drover serve` is the page's server: it serves the page's files, checks the browser, and passes every screen, tap and stream to the API over HTTP. It holds no state and cannot call drover's code. Paired devices stay drover's; push moves nowhere (it is deleted by #1571); the chat view is just two more screens.
Slices: eight. A check that only lets the remaining core calls shrink, and the dead routes deleted; the API crate; the page's server stops calling core (the issue's end state); the device stream; the API as its own process; the page drawn by blocks; its tests on blocks' harness; the old stream route deleted.
Open questions: (1) one `drover` binary that runs as two jobs (recommended), or a second binary now? (2) a third kept-alive job, `dev.drover.api` (recommended), or the service hosts the API? (3) #1344 needs its own design stage: take it out of this root (recommended), or run the page slice with a design stage?

Status: proposed (architect lane, 2026-11-23). Read [README.md](README.md) first, then this, [shapes.md](shapes.md),
[boundaries.md](boundaries.md), [gaps.md](gaps.md).

## Question

`crates/server` is a crate in name only: it re-exports all of core and calls it everywhere. How does the page's
server become a process that reaches drover only through `/api/v1`, with a check that keeps it so, without
breaking the page, cooee, the terminal or the tests on the way, and in step with #1571 (cooee runs the chat) and
#1567 (the shed move)?

## Where it stands (staging `7c0a28c1c`)

- `crates/server/src/lib.rs` starts with `pub use drover_core::*;`. A grep of module paths finds 45 core modules
  at about 500 call sites in `crates/server/src` (the issue counts 57). No check sees them: `arch.toml` has no
  layer entry, and CI's layers step only keeps core from depending on the server.
- The crate is 12,800 lines. By function, about 10,700 read state, build a view or run a verb (the screens, `v1.rs`,
  `chat.rs`, the hub, the tails, the jobs, the types, most of `serve.rs`), and about 2,000 are the HTTP edge (the
  listener, the page shell, the assets, the themes, pairing cookies, the Host and Origin check, the handover).
- There is already one door. `Server::respond(request text) -> response bytes` answers every request; the
  listener, the terminal's `Local` and 104 test files all call it. The split is where that call crosses a process.
- The page (`web/src`) calls only: `screens/…` and their streams, `action`, `pair/status|request|claim`,
  `push/subscribe`, `api/v1/timings`, its own files, and the image links a screen carries (`/thread/`, `/design/`,
  `/shot/`). It calls no other `/api/v1` route.
- Nobody calls `/live`, `/lane`, the form `POST /`, `/answer`, `/setup`, `POST /thread`, `/report`, `/pair/cancel`
  or `/push/unsubscribe`. Tests still do.
- Auth today: a paired device is the `hd` cookie, whose hash is a row of the `devices` table; `drover serve approve`
  (core) approves a code. cooee sends a bearer token (`cooee-token`), honoured on the screens, `/action` and
  `/cooee/`, not on `/api/v1`. `POST /api/v1/mv|pause|resume|set` need only the Host check: an unpaired write.
- Streams: `/screens/<x>/stream` from the hub (one build per change, #1372), 16 a device. The device stream
  (`GET /stream`, #1440) is designed and not built; cooee's side of it is (cooee#615).
- Deployment: `dev.drover.serve` is its own job. `drover service --board` can host the same board in the service's
  process. The service has a listener of its own (`--api`, default `127.0.0.1:7778`) with two routes, `health` and
  an events stream. `dev.drover.serve-staging` runs a staged build read-only before a swap. Self-update restarts
  the job; the handover passes the listening sockets to a successor when a start finds the port held.
- blocks has shipped what the page migration waited for: the HTML renderer, the shell (`mount`), the layout check
  and the headless harness (blocks#15, #16, #14, #17, all closed).

## Options

**Where the cut goes**

1. Move everything to a new crate and leave a shell called `server`. The AC holds and nothing has changed.
2. The page's server reads drover's database itself and builds its own views. Two readers of the state, and the
   page's needs stay in core's tables.
3. **Cut at `respond`, the door that exists (chosen).** Behind it: the API, which links core. In front: the edge,
   which holds a socket and files. Everything that crosses is an HTTP request with a frozen shape.

**How the edge reaches the API**

1. A Rust client crate with one typed function per route. Every new screen field becomes a client release, and the
   edge would parse what it only passes on.
2. **A relay (chosen).** The edge forwards the request and pipes the answer. It parses only the three answers it
   uses itself (`hello`, `device`, a pairing). A new screen or action needs no edge change.

**Who knows a device**

1. The edge keeps its own device file. `drover serve approve`, the hosts view and doctor would then call the edge,
   which is the wrong direction, and a second store appears.
2. The edge asks the API whether a cookie is paired and caches the answer. Two places decide, and an unpair waits
   for the cache.
3. **The edge sends the cookie's hash with every request; the API decides, as today (chosen).** No store and no
   cache in the edge; an unpair holds at once; `serve approve` is untouched.

**Streams**

1. The edge keeps one stream to the API and fans frames out to phones from its own copy of each screen. Phones
   stay connected while the API restarts, at the price of a second hub.
2. **One upstream per downstream, bytes piped (chosen).** The API's limits and resume rule do all the work. An API
   restart looks to a phone like a reconnect, which the stream already handles.

**Where the API runs**: its own job (chosen) or the service. Open question 2 in [README.md](README.md).

**One binary or two**: one (chosen). Open question 1.

**Push**

1. Move web push into the edge. #1571 deletes it in its D4; this would build it a new home first.
2. **Leave it (chosen).** drover's sender stays where it is until D4. The edge relays the page's subscribe call to
   an API route that is marked as leaving and deleted with it.

## Decision

- **Three crates where there was one.** `drover-api` (new, in an `api` folder beside the other crates; links
  core): the `Server` and its `respond`, every `/api/v1` route, the screens, the chat screens, `/action`, the hub,
  the tails, the jobs, the lifeboat, the webhook listener, the types, `Local`. `crates/server` (`drover-server`):
  the edge. `drover-wire` (new in S4, in a `wire` folder): the HTTP code both listen with, and the loopback
  client. The moves are whole files; `serve.rs` is the one file that is carved ([gaps.md](gaps.md)).
- **The door.** The edge has one trait, `Engine`, with `ask` and `follow` ([shapes.md](shapes.md)). Until S4 the
  launcher gives it an in-process door (a call to `respond`); from S4 the edge's own HTTP door. The edge never
  depends on `drover-api`.
- **The launcher.** `crates/cli` is the only crate that links both sides. It reads `[serve]`, binds or inherits the
  listeners (the held-port and handover logic of `hooks.rs` moves there, since it needs core's runner and timer),
  and calls `drover_server::run` with plain values.
- **Auth.** The API listens on loopback and answers only a bearer token: `cooee-token` as today, and a new
  `serve-token` beside it. The edge checks Host and Origin, hashes the `hd` cookie and sends `Drover-Device` and
  `Drover-Ip`; it never copies such a header from a browser. The API maps the hash to a device and decides paired,
  refused or limited. Pairing (`pair/request`, `pair/claim`) and `device` become API routes; the edge sets the
  cookie the API hands back. A request that arrives with a plugin's own bearer is passed on with that bearer, so a
  cooee still attached to the page's port keeps working until its manifest names the API.
- **Screens.** The API builds them; the page draws them. From S5 the page is blocks' `mount` with drover's
  `layout.json` (`GET /api/v1/layout`), so the edge serves a shell and a bundle and has no drawing code.
- **Streams.** The hub and the device stream are the API's. The edge pipes. `/live` is deleted. #1571's
  `rows/stream` is the API's too; the edge does not read rows, because its thread view is a screen.
- **Chat.** Per #1571 the page keeps the thread list, the text, send and the cards. Those are the
  `/screens/threads` screens, built in `drover-api` from `core::thread`. The edge has no chat code at all.
  #1571's files named under `crates/server/src/serve/` (`rows.rs`, `chat.rs`) are the same files under
  `api/src/serve/`; whichever lands second rebases a path.
- **Push.** Nothing moves. `push/subscribe` and the push key are API routes marked leaving, deleted by #1571's D4.
- **The lifeboat** stays at `/api/v1/lifeboat`, built by the API, relayed by the edge. It needs no new route.
- **Webhooks** are drover's way in, not the page's: the `hooks_addr` listener moves to the API host unchanged.
- **Jobs.** `dev.drover.api` runs the new `api` command on `[serve] api` (default `127.0.0.1:7779`).
  `dev.drover.serve` keeps its label and address. `drover service --board` is deleted; the service's own
  two-route listener goes with it once no caller is found (its events stream is #1571's `rows/stream`).
  `dev.drover.serve-staging` keeps its label and runs the staged build's `api --read-only`: what it proves is
  that the new build's views answer on the live state.
- **The contract.** Every path in the edge's table gets a shape file in #1571's frozen folder
  (`crates/cli/tests/golden/contract/v1/*.keys`). The old `/api/v1` reads the page does not call stay in the old,
  re-recordable fixtures and are not frozen by this issue.

## Versions

One release carries both, so both jobs run the same build except for the seconds between their restarts.

- Self-update restarts `dev.drover.api` first, then `dev.drover.serve`. An edge is therefore never newer than its
  API for longer than a restart.
- The contract only grows, so an older edge works against a newer API.
- The edge reads `GET /api/v1/hello` at start and every `serve_heartbeat_secs`. It carries the API's build. The
  edge's own `GET /health` reports both builds and the link's state ([boundaries.md](boundaries.md)); `drover
  doctor` shows them on the serve row. No new alert.
- A path the API does not know answers its normal `NOT_FOUND`; the edge passes it on. Nothing in the edge branches
  on a version.
- If the edge ever ships apart from drover (open question 1), it needs a number to compare. That number is the line
  count of the frozen `.keys` files, which can only rise. It is not built now.

## Consequences

- The issue's end state arrives at S2, before any process is split: `crates/server` has no `drover-core`
  dependency and a layer rule holds it. S4 then moves the door onto a socket.
- A core change can still break `drover-api` at compile time. It cannot break the edge, and it cannot change what
  the page receives without a contract file changing.
- The unpaired writes close: every API request needs a token, and every write a paired device, the plugin or the
  terminal.
- One more hop on loopback per request. Bodies are passed as they come (gzip and `ETag` included), never decoded
  and encoded again. The existing serve gates (150 ms p95, the request-path rule, `serve_mem_mb`) become the
  API's and are measured through the edge in S4.
- One more kept-alive job. `setup`, `timer install`, `check`'s revive, doctor and the Services rows each gain a
  row for it. With the API down the edge still serves the shell, and the page paints its last copy with the red dot.
- The page no longer works by starting `drover serve` alone: it needs the API host. `drover serve --export` is the
  launcher calling both crates.
- The memory watch and the hand-over-on-high-memory leave the edge (it caches nothing). The API exits past
  `serve_mem_mb` and its job restarts it; clients resume by revision.
- The four tests on `/live` are rewritten on the screen streams. `docs/parity.md` rows that name a deleted route
  cite this ADR's approval.
- `just types`, `just html-lint` and CI's TS types step name `drover-api` where they named `drover-server`. CI's
  deps gate lists the two new workspace crates. No third-party crate is added.
- Lines, as an estimate from today's files: about 700 new (the door, the relay, the launcher's share, the new
  routes, the job) against about 500 deleted (`/live` and its fan-out, the dead routes, `--board`, the doubled
  auth), before S5 shrinks `web/src`.
- #1567's ADR keeps "the serve handover" in drover. It still does: the handover is the edge's, started by the
  launcher.

## The slices

Each is one issue and one PR. "Core list" is `server/src/core_uses.rs`, the frozen file S0 makes.

| # | Slice | Removes | The check that fails a new core call | Needs |
|---|---|---|---|---|
| S0 | The ratchet. `pub use drover_core::*` becomes the core list: one `pub use drover_core::<module>;` a line. The routes nobody calls are deleted ([gaps.md](gaps.md)) | the glob; `/live`, its fan-out and the dead route arms | the core list is `frozen` (a PR may delete a line, never add one); `drover_core` is banned in every other file of the crate | this ADR |
| S1 | The API crate. The view and action files move to `drover-api`; `serve.rs` is carved; the edge calls the door; `chat`, `service` and the tests name `drover_api` (through `drover::serve`, so no test changes) | every line of the core list but the edge's own (about eight of 45: devices, webpush, config, events, runner, clock, update, timer) | the same; plus `drover_api` banned in `crates/server` | S0 |
| S2 | The edge stops calling core. `Drover-Device`; `hello`, `device`, `pair/*`, `layout` and the leaving push routes in the API; the launcher in `crates/cli`; the edge's own clock | the rest of the core list, the file itself, and `drover-core` in `crates/server/Cargo.toml` | the layer rule and the Cargo bans below; CI layers greps `cargo tree`. **The issue's end state** | S1 |
| S3 | The device stream, in `drover-api`: the hub on blocks' diff and revisions (#1441), `GET /stream` and `POST /stream/<sid>` (#1440); the door's `follow` pipes it | drover's own diff; `/live` is already gone | unchanged | S1 |
| S4 | The API as its own process. `drover-wire`; the `api` command; the tokens; `dev.drover.api` in setup, timer, check, doctor and self-update; the edge's HTTP door; a contract file per path of the edge's table; the staging job; cooee's manifest; `--board` deleted | the in-process door; the service's dependency on the server | `contract_v1` covers the edge's table; `edge` tests run it on recorded answers alone | S2; #1571's D1 (the frozen folder and its test) |
| S5 | The page drawn by blocks: its renderer, `choose` and scheme CSS from the package (#1340, #1399); the shell is one `mount` with `layout.json`, reading the device stream (#1341) | `web/src/blocks.ts`, `ops.ts`, the generic rules of `board.css`, the inlined places | the web lint | S3 |
| S6 | The WebKit and verify tests of blocks screens on `blocks-shot` (#1345) | `webview.mjs` for those screens | unchanged | S5 |
| S7 | The per-screen stream route and `wire::old` deleted (#1343); the plugin-bearer pass-through deleted. This is #1645's own PR: its body names each folded issue with its commit | the route, `wire::old`, the pass-through | unchanged | S5, S4; cooee's app half of C5 and its manifest on the API |

S0, S1, S2 and S4 are one chain. S3 can run beside S2, and S5 and S6 beside S4. Where #1556 (screens one file an
area) lands first, S1 moves its folder; where S1 lands first, #1556 splits the file in `drover-api`.

If cooee's app half of C5 is not released when S6 merges, #1343 is the only thing holding #1645 open. The
orchestrator then asks whether to take #1343 out of the fold.

## The folded issues

| Issue | What it wants | Outcome |
|---|---|---|
| #1441 | the hub's diff and revisions from the blocks crate | fixed in S3 |
| #1440 | one stream per device; `/live` on the same writer | fixed in S3; the `/live` half is moot, S0 deletes `/live` |
| #1340 | the page draws through blocks' renderer | fixed in S5 |
| #1399 | `choose` and the scheme CSS from the blocks package | fixed in S5 |
| #1341 | the shell is one blocks mount; the places are `layout.json` | fixed in S5 |
| #1345 | blocks screens tested with `blocks-shot` | fixed in S6 |
| #1343 | delete the per-screen stream and `wire::old` | fixed in S7 |
| #1344 | the art onto swag's packs, design first | neither: open question 3 |

## The rules lines

`.drover/rules.md` is at its limit, so each line replaces one. S2 replaces the layers line and the HTML line; S4
rewords the request-path line.

```
- don't make core depend on api, server or cli, api on server or cli, or server on core or api; don't spawn a process outside core's runner.rs (enforced by: CI layers, CI arch-check `[[layer]]` and the Cargo bans, tests/runner_limits.rs)
- don't build HTML outside crates/server's page shell and crates/api's lifeboat (enforced by: CI `just html-lint`)
- don't call git, gh, ssh or a db write on the API's request path (enforced by: the request-path guard test, crates/cli/tests/serve_health.rs)
```

## Hot files

Files this design touches that are hot, or would become so:

| File | Class | Here |
|---|---|---|
| `crates/server/src/serve.rs` (3,478 lines, `shared`) | large module | carved once in S1: the edge's part becomes six small files in `crates/server/src/`, the rest moves to `api/src/serve.rs` |
| `crates/server/src/serve/v1.rs`, `serve/chat.rs`, `api.rs` (`shared`) | large modules | moved whole; the new routes go in one new file, `api/src/serve/edge.rs`, with its own table, so `v1.rs` gets one arm |
| `crates/server/src/screens.rs` | central list (the action match) | untouched here; #1556 owns it |
| `server/src/core_uses.rs` (new) | central list | `frozen` from the PR that creates it; deleted in S2 |
| the edge's relay table (new, `server/src/relay.rs`) | central list | prefixes, not routes: a new screen or action adds no line. It gets a line only when the page needs a new kind of path |
| `crates/cli/tests/golden/contract/v1/*.keys` | collected folder (#1571) | one file per path; S4 adds files, edits none |
| `cooee-plugin.toml` | `generated` from #1571's D2 | S4 changes its template's `run` and `attach` |
| `docs/serve/*.md`, `docs/service.md` | docs per area | a new `process.md` in `docs/serve/` (the two jobs, the tokens, the versions); `api-v1.md` gains the new routes; `service.md` loses `--board` |
| `web/src/main.ts` (`shared`) | large module | S5 makes it the `mount` call |
| `.drover/rules.md` | central list, capped | three lines replaced, none added |

`.drover/arch.toml`, in S0 (the PR that makes the core list):

```toml
frozen = ["crates/server/src/core_uses.rs"]   # appended to the list

[[ban]]
pattern = "drover_core"
paths = ["crates/server/src/**"]
except = ["crates/server/src/core_uses.rs"]
reason = "#1645: the page's server reaches drover through the API; core_uses.rs lists what is left and only shrinks"
```

In S1 (the PR that moves the files). The `shared`, `[docs] watch` and ban paths that name a moved file follow it
(`crates/server/src/api.rs` becomes `api/src/api.rs`, and so on for `serve.rs`, `serve/chat.rs`,
`serve/v1.rs`, the `json!` ban and the `.build_screen(` ban); the `crates/chat` bans say `api::Local`. New:

```toml
[[ban]]
pattern = "drover_api"
paths = ["crates/server/src/**"]
reason = "#1645: the page's server calls the door (Engine), never the API crate"
```

In S2 (the PR that deletes the core list). The S0 lines go, and:

```toml
[[layer]]
name = "core"
paths = ["crates/core/src/**"]
mods = ["drover_core"]
may_use = []

[[layer]]
name = "api"
paths = ["crates/api/src/**"]
mods = ["drover_api"]
may_use = ["core"]

[[layer]]
name = "server"
paths = ["crates/server/src/**"]
mods = ["drover_server"]
may_use = []

[[ban]]
pattern = "drover-core"
paths = ["crates/server/Cargo.toml"]
reason = "#1645: the page's server never links core"

[[ban]]
pattern = "drover-api"
paths = ["crates/server/Cargo.toml"]
reason = "#1645: the page's server never links the API crate"
```

and one line in CI's layers step, which also covers what a dependency brings in:

```
! cargo tree --locked -p drover-server -e normal,build --prefix none | grep -E '^drover-(core|api) v'
```

S4 adds no line: `drover-wire` links no drover crate, and the `cargo tree` line above fails if it ever does.

The time and process bans (#809) gain `server/src/clock.rs` as an exception for `SystemTime::now`,
`Instant::now` and `thread::sleep` in S2: the edge cannot reach core's clock. It gains none for `Command::new` or
`env::var`: the launcher reads the environment and hands the edge a function that starts its successor.
