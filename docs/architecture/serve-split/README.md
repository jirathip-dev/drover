# serve-split: the page's server is a client of drover's API (#1645)

## Summary

Decision: today's server is cut in two. The part that reads state, builds screens and runs actions becomes drover's API, in its own crate and its own process (a new `api` command). What is left as `drover serve` is the page's server: it serves the page's files, checks the browser, and passes every screen, tap and stream to the API over HTTP. It holds no state and cannot call drover's code. Paired devices stay drover's; push moves nowhere (it is deleted by #1571); the chat view is just two more screens.
Slices: eight. A check that only lets the remaining core calls shrink, and the dead routes deleted; the API crate; the page's server stops calling core (the issue's end state); the device stream; the API as its own process; the page drawn by blocks; its tests on blocks' harness; the old stream route deleted.
Open questions: (1) one `drover` binary that runs as two jobs (recommended), or a second binary now? (2) a third kept-alive job, `dev.drover.api` (recommended), or the service hosts the API? (3) #1344 needs its own design stage: take it out of this root (recommended), or run the page slice with a design stage?

Status: proposed (architect lane, 2026-11-23). Read on staging `7c0a28c1c`, with the approved #1571 ADR
(`cooee-threads`, on `issue-1571`) and the #1567 ADR (`shed-split`, on `issue-1567`).

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, the open questions |
| [adr.md](adr.md) | question, where it stands, options, decision, consequences, versions, the slices, the folded issues, the rules lines, the `arch.toml` lines |
| [shapes.md](shapes.md) | the door, the headers, the new routes and their types, the page's own paths, the jobs |
| [boundaries.md](boundaries.md) | which module may call which; four statecharts |
| [gaps.md](gaps.md) | every path the page's server needs: there today, or a gap; what is deleted; where each file goes |

Three words. The **API** is everything a client may ask drover: `/api/v1/…`, the screens (`/screens/…`), `/action`
and the streams. The **edge** is the page's server, `crates/server` after the cut. The **door** is the one way the
edge reaches the API.

A path written from `api/`, `wire/`, `server/` or `cli/` is a file under `crates/` that does not exist yet; the
fenced `arch.toml` lines in [adr.md](adr.md) give full paths. The new command is `api`: `drover` followed by
`api`, as the fenced lines show it.

## The callers, sketched first

### A phone reads a screen

```
phone ── GET /screens/queue?repo=api ──▶ edge (dev.drover.serve, the address phones use)
         Cookie: hd=9c1f…                 checks Host and Origin, hashes the cookie, adds nothing else
edge  ── GET /screens/queue?repo=api ──▶ API (dev.drover.api, 127.0.0.1 only)
         Authorization: Bearer <serve token>
         Drover-Device: 5e0b…a2 (sha256 of the cookie)      Drover-Ip: 100.64.0.7
         If-None-Match: "12.48"
API   ── 304, or 200 with the screen and ETag ──▶ edge ── the same bytes ──▶ phone
```

The edge does not know whether the device is paired, what a screen holds, or which actions exist. The API decides
all three, as serve does today.

### A tap, and a stream

```
POST /action   {"screen": "/screens/queue", "block": "row-412", "action": "drive", "values": {…}}
GET  /stream                       one per device; the edge pipes the API's frames until either side closes
POST /stream/<sid>                 open and close keys
```

Both go through the same door. The edge keeps no screen and no revision: a phone that reconnects gets what it
missed from the API, exactly as after a restart (the screen-hub ADR's resume rule).

### The edge's code

```rust
match own(&req) {
    Some(Own::Shell) => shell(&facts.read(), engine.ask(&device(&req))),
    Some(Own::Asset(a)) => asset(a),
    Some(Own::Pair(step)) => pair(step, engine, &req),
    None if relayed(&req) => engine.ask(&forward(&req)),
    None => not_found(),
}
```

`engine` is the door: `ask` for one answer, `follow` for a stream. `relayed` reads one table of paths
([shapes.md](shapes.md)).

### The launcher (the `drover` binary, the only place that knows both sides)

```rust
Cmd::Serve { addr, read_only } => drover_server::run(Edge {
    listeners: serve::listeners(ctx, r, &addr)?,
    engine: Box::new(engine::Http::new(cfg.serve.api(), paths.serve_token())),
    state: ctx.state_dir(),
    themes: paths.themes(),
    successor: serve::successor(r),
    read_only,
}),
Cmd::Api { addr, read_only } => drover_api::host(ctx, r, &addr, read_only),
```

`crates/server` gets plain values, bound listeners and one function that starts its successor for a handover. It
reads no fleet.toml, no environment and no database, and starts no process.

### The terminal and the tests stay in process

```rust
let local = drover_api::Local::new(ctx, r);
let screen = local.screen(now, "/screens/queue")?;

let api = drover::serve::Server::new(&ctx, &r, None);
let got = api.respond("GET /api/v1/state HTTP/1.1\r\n\r\n", now);
```

`drover chat` and the 100-odd tests that call `Server::respond` use the API crate with no socket, as today. Only
their crate changes.

### cooee

```toml
run = "exec drover api --addr 127.0.0.1:$PORT"
attach = "http://127.0.0.1:7779"
```

cooee and the page are two clients of the same API. cooee no longer needs the page's server to be running.

### The checks

```
drover arch-check        crates/server names drover_core or drover_api: fails
cargo tree -p drover-server | grep -E '^drover-(core|api) '        CI layers: must print nothing
cargo test --test contract_v1          every path in the edge's table has a frozen shape (#1571's test)
cargo test --test edge                 the edge against recorded API answers only: no drover state at all
```

## The design on one screen

| Thing | Owner | The other side |
|---|---|---|
| The page's files: shell, script, styles, icons, service worker, themes | edge | none |
| Host and Origin check, the `hd` cookie, connection limits | edge | the API trusts only a token |
| Who is paired, pairing codes, `drover serve approve`, unpair | API (drover's database, as today) | the edge sends the cookie's hash and sets the cookie the API hands it |
| What a device may see or do, send limits, streams per device | API | the edge forwards |
| Every screen, the chat view included; every action; `/api/v1` | API (`drover-api`) | the edge relays, the page draws |
| Streams | API (the hub, the device stream) | the edge pipes bytes |
| The lifeboat page | API (it is an `/api/v1` path today) | the edge relays it |
| GitHub webhooks | API host (drover's own way in, its own listener) | none |
| The scene for a companion device on the LAN | edge listener, API screen | relayed |
| Push | drover's sender until #1571's D4 deletes it | the edge relays the subscribe call until then |
| Config the edge needs (allowed hosts, theme, hints) | API (`hello`) | the edge re-reads it when it changes |
| Port handover between builds | edge, started by the launcher | none |

One direction: phone → edge → API. The API never calls the edge.

## Open questions

1. **One binary or two.** Recommended: one `drover` binary, run as two jobs (the new `api` command and `drover serve`). The
   crate cut is what stops core calls; one file keeps one release, one install and one version. A second binary
   would be built, signed and swapped by `release.rs`, which #1567's D4 is about to replace with shed's installer.
   The edge already starts from plain values, so a second binary later is a new `main` of about 30 lines.
2. **Where the API runs.** Recommended: its own job, `dev.drover.api`, installed and revived wherever
   `dev.drover.serve` is. It is today's serve process without the browser part, so its load, its memory limit and
   its request rules stay as tested, and the page keeps working with `drover service` stopped. The alternative is
   the service hosting it: one job fewer, but screen building then shares a process with the drive loops, the
   read-only staging check needs a service that does not drive, and the page stops with the service.
3. **#1344 (the art onto swag's packs).** Its own text says a design stage comes first and the owner calls it. This
   root cannot make it fixed or moot. Recommended: take it out of the fold; it then waits only on S5. The
   alternative is to queue S5 with a design stage and do it there.
