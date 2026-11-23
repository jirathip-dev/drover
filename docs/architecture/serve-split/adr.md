# ADR: the page's server is a client of drover's API (#1645)

## Summary

Decision: today's server is cut in two. The part that reads state, builds screens and runs actions becomes drover's API: its own crate and its own kept-alive job (a new `api` command). What is left as `drover serve` is the page's server: it serves the page's files, checks the browser, and passes every screen, tap and stream to the API over HTTP. It holds no state and cannot call drover's code. Paired devices stay drover's; push moves nowhere (#1571 deletes it); the chat view is two more screens.
Revised per q3302: the MCP endpoint (#1627) belongs to the API, behind one line of the page's server, and an agent's key reaches it in its own header, never through the plugin pass-through that the last slice deletes. cooee only attaches to the one API job and can no longer start a copy; that job alone takes GitHub's webhooks. The split fixes no speed problem (#1651 does); S4 measures that it adds none. #1344 is moot here: it waits for #1628's design.
Slices: nine. A check that only lets the remaining core calls shrink, and the dead routes deleted; the API crate; the page's server stops calling core (the issue's end state); the device stream; the API as its own process, measured; cooee attached to it; the page drawn by blocks; its tests on blocks' harness; the old stream route and the pass-through deleted.
Open questions: one left. A single `drover` binary that runs as two jobs (recommended, and what the slices assume), or a second binary now? q3302 answered the other two.

Status: proposed, revised once (q3302, 2026-11-23). Read [README.md](README.md) first, then this,
[shapes.md](shapes.md), [boundaries.md](boundaries.md), [gaps.md](gaps.md).

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
- cooee's manifest names a backend to attach to (`http://127.0.0.1:7777`, the page's port) and a `run` line that
  starts `drover serve` on that port. cooee runs it whenever the port does not answer, and its manifest reader
  requires the line. A serve started that way binds the webhook listener too.
- The webhook listener (`[serve] hooks_addr`) is bound by any `drover serve` that starts writable. A second one
  logs "no webhooks" and carries on.
- There is no MCP endpoint. #1627 (open, design first) asks for one over the operations `/api/v1` has.
- Speed: every screen request costs seconds and serve spins a core when idle (#1622, #1588). Both are folded under
  #1651 (settled state kept as a snapshot).
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

**Where the API runs**: its own job (chosen; q3302 settled it) or the service.

**One binary or two**: one (chosen). The open question in [README.md](README.md).

**An agent's key (MCP, #1627)**

1. It rides the plugin pass-through: the edge passes any bearer on whole. S7 deletes the pass-through, and the API
   could not trust the address such a request claims to come from.
2. The edge keeps the keys and decides. A second store, and a revoke would have to reach the edge.
3. **The edge carries it, the API decides (chosen).** On the one `/mcp` line the edge hashes the caller's bearer
   into `Drover-Key`, beside its own token, exactly as it hashes the cookie into `Drover-Device`.

**What cooee does when the API is down**

1. Its `run` line starts an API of its own (the first draft). Two hosts can then exist, one outside launchd, and
   both want the webhook port.
2. A `run` line that only waits, or wakes the job. A process whose one purpose is to fill a field; cooee would show
   it as running.
3. **No `run` line (chosen).** cooee learns an attach-only plugin (a small cooee change) and shows the plugin as
   down until the job is back. The API host also refuses to be the second writable host of one state folder.

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
  cooee still attached to the page's port keeps working until its manifest names the API (S4b). S7 deletes that
  pass-through.
- **MCP (#1627).** `/mcp` is an API route in `drover-api`; its tools, verbs and key store are #1627's ADR to
  decide. The edge gets exactly one line for it in the relay table: every method of the path, piped like a stream.
  On that line, and no other, the edge reads the caller's `Authorization: Bearer <key>` and sends
  `Drover-Key: <sha256 of the key>` with its own `serve-token` and `Drover-Ip`. The API looks the hash up (it
  stores hashes only), names the caller as the event's `by`, and answers 401 for an unknown or revoked key, which
  the edge passes on. An agent on the host itself may call the API's loopback address with its key as the bearer:
  the API hashes it and does the same lookup.
  A key therefore never uses the plugin pass-through. While the pass-through exists the `/mcp` line is matched
  first; S7's deletion changes nothing for it. After S7 the edge sends the API one credential (`serve-token`) and
  at most one identity (`Drover-Device` or `Drover-Key`), and drops a client's `Authorization` header on every
  other line. #1627's impl adds the line with its contract file; if it lands before S2, its bearer check sits in
  the one server and S2 moves it behind the header.
- **cooee.** Its manifest names the API's address and has no `run` line (S4b). cooee attaches to
  `dev.drover.api` or shows the plugin as down; it never starts a drover process. This needs cooee's attach-only
  change released first; `min_cooee` in the manifest names that release.
- **One writable host.** `api::host` holds `api.lock` in the state folder for its life. A second writable start on
  the same state exits 1 naming the holder; when the starter is the job and the holder is not, it stops the holder
  first (serve's rule today). `--read-only` takes no lock, binds no webhook listener and runs no jobs.
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
- **Webhooks** are drover's way in, not the page's. Exactly one process binds `hooks_addr`: the writable API host,
  under the lock above. The edge never binds it; from S4 the launcher closes a webhook socket an older build hands
  over ([boundaries.md](boundaries.md), the swap). The host binds its listeners before it reads state, so a
  delivery during a restart waits in the backlog.
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
- One more hop on loopback per request: "Speed and CPU" below.
- The webhook socket is no longer handed from build to build. A delivery that arrives between the old host's exit
  and the new one's bind is refused; the timer's next pass picks the change up, as for any missed delivery.
- cooee can no longer bring drover up by itself. A host where cooee draws drover's screens needs the API job
  installed (`setup` does it), and cooee needs its attach-only release before S4b.
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

## Speed and CPU

Making serve fast is #1651's work (#1588: the idle spin and the webhook pile-up; #1622: the fixed cost of every
request and the chat payload). This issue fixes none of it, and its PRs name neither as fixed. Its duty is to add
nothing:

- The edge has one timer, the `hello` heartbeat, and `hello` is answered from memory (no database read, no fold).
  A stream is two blocking sockets. The edge polls nothing and watches no file.
- A relayed body is passed as it comes (gzip and `ETag` included), never decoded and encoded again. A relayed
  request makes one API call. The shell asks `device` once; `hello` and `layout` are kept.
- The serve gates (150 ms for `GET /`, the bench's p95 per route group, the request-path rule, `serve_mem_mb`,
  `serve-slow`) stay with the code they test, in the API. The phone's own timings (`phone-slow`) include the hop.

S4 proves it. It is queued `--perf`, so its baseline commit and rev's before and after are gated (`NO_BASELINE`):

| Measure | How (release, `crates/cli/tests/perf.rs`, the `big` fixture) | CI perf gate, from S4 on | In the PR's before and after |
|---|---|---|---|
| p95 through the edge | 200 requests per route group (a screen, a tap, the shell), at the API's address and then at the edge's, in one run | per group, the edge's p95 is at most the API's plus 5 ms | each group's p95: one process, then through the edge |
| idle CPU | both processes up, one device stream open through the edge, no request for 60 s (#1588's own measure) | the edge under 1 % of a core | the two together at most the one process plus 1 % |

The baseline commit adds the two measures on the one-process build and records their numbers. A number over
budget fails S4, whatever #1651 has landed by then.

#1651's open PRs edit files S1 moves. Whichever lands second follows the move. Only `serve.rs`'s carve is a real
conflict, so S1 starts after #1588's and #1622's PRs land or close.

## The slices

Each is one issue and one PR. "Core list" is `server/src/core_uses.rs`, the frozen file S0 makes.

| # | Slice | Removes | The check that fails a new core call | Needs |
|---|---|---|---|---|
| S0 | The ratchet. `pub use drover_core::*` becomes the core list: one `pub use drover_core::<module>;` a line. The routes nobody calls are deleted ([gaps.md](gaps.md)) | the glob; `/live`, its fan-out and the dead route arms | the core list is `frozen` (a PR may delete a line, never add one); `drover_core` is banned in every other file of the crate | this ADR |
| S1 | The API crate. The view and action files move to `drover-api`; `serve.rs` is carved; the edge calls the door; `chat`, `service` and the tests name `drover_api` (through `drover::serve`, so no test changes) | every line of the core list but the edge's own (about eight of 45: devices, webpush, config, events, runner, clock, update, timer) | the same; plus `drover_api` banned in `crates/server` | S0 |
| S2 | The edge stops calling core. `Drover-Device`; `hello`, `device`, `pair/*`, `layout` and the leaving push routes in the API; the launcher in `crates/cli`; the edge's own clock | the rest of the core list, the file itself, and `drover-core` in `crates/server/Cargo.toml` | the layer rule and the Cargo bans below; CI layers greps `cargo tree`. **The issue's end state** | S1 |
| S3 | The device stream, in `drover-api`: the hub on blocks' diff and revisions (#1441), `GET /stream` and `POST /stream/<sid>` (#1440); the door's `follow` pipes it | drover's own diff; `/live` is already gone | unchanged | S1 |
| S4 | The API as its own process, queued `--perf`. `drover-wire`; the `api` command; the tokens; `api.lock` and the one webhook binder; `dev.drover.api` in setup, timer, check, doctor and self-update; the edge's HTTP door; a contract file per path of the edge's table; the staging job; `--board` deleted; the two measures above | the in-process door; the service's dependency on the server; the webhook listener in `drover serve` | `contract_v1` covers the edge's table; `edge` tests run it on recorded answers alone; `perf.rs`'s two budgets; `api_host.rs`: a second writable host exits 1 and binds nothing | S2; #1571's D1 (the frozen folder and its test) |
| S4b | cooee attaches to the API. The manifest's template: `attach` is `[serve] api`, `min_cooee` set, no `run` | the manifest's `run` line: cooee can start no drover process | `cooee_manifest.rs`: the manifest has no `run`, and its `attach` is the API's address | S4; cooee's attach-only change released |
| S5 | The page drawn by blocks: its renderer, `choose` and scheme CSS from the package (#1340, #1399); the shell is one `mount` with `layout.json`, reading the device stream (#1341) | `web/src/blocks.ts`, `ops.ts`, the generic rules of `board.css`, the inlined places | the web lint | S3 |
| S6 | The WebKit and verify tests of blocks screens on `blocks-shot` (#1345) | `webview.mjs` for those screens | unchanged | S5 |
| S7 | The per-screen stream route and `wire::old` deleted (#1343); the plugin-bearer pass-through deleted. This is #1645's own PR: its body names each folded issue with its commit | the route, `wire::old`, the pass-through, `Viewer.bearer` | `edge.rs`: a client's `Authorization` reaches the API on no line but `/mcp`'s, and there only as `Drover-Key` | S5, S4b; cooee's app half of C5, and cooee's pin of drover's plugin at S4b or later |

S0, S1, S2, S4 and S4b are one chain. S3 can run beside S2, and S5 and S6 beside S4. Where #1556 (screens one file an
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
| #1344 | the art onto swag's packs, design first | moot here (q3302): Blocks art waits for #1628's design (design lanes in a Blocks repo draw on Blocks). No slice draws it or waits for it; S5 keeps today's art |

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
| `cooee-plugin.toml` | `generated` from #1571's D2 | S4b changes its template: `attach` and `min_cooee` set, `run` gone |
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
