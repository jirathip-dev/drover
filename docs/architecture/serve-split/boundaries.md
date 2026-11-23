# serve-split: module boundaries and statecharts (#1645)

## Who may call whom

```
phone, browser ──HTTP──▶ server (the edge) ──Engine──▶ the door ──HTTP, serve-token──▶ api::host
cooee ──HTTP, cooee-token────────────────────────────────────────────────────────────▶ api::host
GitHub ──POST /hooks/github (its own listener)───────────────────────────────────────▶ api::host
drover chat, the tests ──in process──▶ api::Local, api::serve::Server::respond
                                                         │
                                                         ▼
                                   api (routes, screens, hub, tails, jobs) ──▶ core ──▶ sys
cli (the launcher) ──▶ server::run(Edge)      cli ──▶ api::host      cli ──▶ core
server, api ──▶ wire (HTTP only)              server ──▶ sys (the listeners' file descriptors)
```

| Crate or module | Owns | May call | Must not |
|---|---|---|---|
| `server` (the edge) | the page's listener and the device listener; the shell and the embedded files; themes; the Host and Origin check; the `hd` cookie; the relay; the handover; its own `/health` | `wire`, `sys`, `blocks` and `swag` (theme rules), the `Engine` it was given | link `core` or `api`; read fleet.toml, the environment or the database; start a process; decide who is paired or what a device may do; keep a screen, a revision or a device list; parse a relayed body |
| `server::engine` | the `Engine` trait, `Request`, `Reply`; from S4 the `Http` door | `wire`'s client | name an API path outside `relay.rs` and the four the edge asks itself (`hello`, `layout`, `device`, `pair/*`) |
| `api` | every route a client may call; `Viewer` from the token and headers; paired-only and read-only refusals; send and stream limits; screens, the chat screens, actions; the hub and the device stream; the tails; the jobs; the lifeboat's HTML; the webhook listener; `Local`; the payload types | `core`, `wire`, `blocks` | build the page shell or serve a page file; check Host or Origin; call the edge; trust a `Drover-*` header that came without the edge's token |
| `api::host` | the `api` command: the loopback listener, the tokens, the background readers, the sampler, the memory limit | `api`, `core` | bind a non-loopback address |
| `wire` (S4) | accept, read a request, write an answer, gzip, a stream's frames; the loopback client | `tokio`, `flate2` | link any drover crate; know a path |
| `core` | state, gates, verbs, `devices`, `thread` (#1571) | `sys` | link `api`, `server` or `cli`; name a screen |
| `cli` | the `drover` binary: the `serve` launcher (config to `Edge`, the held-port rule, the successor), `api`, `serve approve|pending|devices`, `serve --export` (both crates' files) | everything | hold route or screen logic |
| `chat` (the terminal) | the Board and Threads tabs | `api::Local`, `core` | open a socket to drover; link `server` |
| `service` | the drive loops, the links, the kicks | `core` | link `api` or `server`; host a board |
| `web/src` (the page) | from S5: the `mount` call, `layout.json`'s use, the scene, pairing, alerts until #1571's D4, timings | the blocks package; the edge's paths | name an `/api/v1` read; define an op, a frame or a patch |

The bans and layer rules in [adr.md](adr.md) hold the last column wherever a pattern can. The rest is the `edge`
test: it runs the edge against a door that only replays the contract folder's recorded answers, so a need outside
the contract has nothing to answer it.

## The migration

```mermaid
stateDiagram-v2
    [*] --> Entangled: today, crates/server re-exports all of core
    Entangled --> Ratcheted: S0 merged (the core list is frozen, dead routes gone)
    Ratcheted --> Carved: S1 merged (crates/api holds the Server, the edge calls the door in process)
    Carved --> CoreFree: S2 merged (no core in crates/server, the layer rule on)
    CoreFree --> TwoProcesses: S4 merged, dev.drover.api installed (the door is HTTP)
    TwoProcesses --> BlocksPage: S5 and S6 merged (the page is one mount)
    BlocksPage --> Done: S7 merged (the old stream route gone, #1645 closes)
    Done --> [*]
```

Every state runs and ships. Up to `CoreFree` it is still one process: going back is a revert of one PR. S3 (the
device stream) lands anywhere after `Carved` and before `BlocksPage`. In `TwoProcesses`, a host whose
`dev.drover.api` is not installed yet has an edge that answers `BUSY`: S4's rollout note is `drover timer install`,
and `drover check` revives the job like serve's.

## The edge's link to the API

```mermaid
stateDiagram-v2
    [*] --> Starting: run(Edge)
    Starting --> Up: hello answered
    Starting --> Away: no answer (the listener is open, the shell is served)
    Up --> Away: a call or the heartbeat's hello fails
    Away --> Up: hello answered
    Up --> Up: hello.config moved (allowed hosts, theme and hints re-read)
    Up --> Skew: hello.sha is not the edge's build
    Skew --> Up: either job restarted onto the same build
    Skew --> Away: a call fails
```

`Away`: a relayed request answers 503 `BUSY`, a stream ends with a retry hint, the shell and the page's files are
still served, and the page paints its last copy. `Skew`: everything works (the contract only grows); `/health` and
doctor show both builds. The edge never restarts itself over a build difference: self-update restarts the jobs,
API first.

## A relayed stream

```mermaid
stateDiagram-v2
    [*] --> Checked: GET /stream (Host, Origin, the cookie hashed)
    Checked --> Piping: the API answered 200, its first bytes passed on
    Checked --> Refused: the API answered 401, 429 or 503 (passed on as it is)
    Piping --> Piping: a frame or a ping from the API, written as it came
    Piping --> ClosedDown: the phone went away (the upstream socket is closed within a second)
    Piping --> ClosedUp: the API closed or restarted (the phone is sent a retry hint and reconnects)
    Refused --> [*]
    ClosedDown --> [*]
    ClosedUp --> [*]
```

The edge holds one upstream socket per stream and nothing else. Which keys are open, the revisions, the queue's
bounds and the four-streams-a-device rule are the API's, keyed by the `Viewer`. A reconnect after `ClosedUp`
resumes by revision, as after any restart.

## A device

```mermaid
stateDiagram-v2
    [*] --> NoCookie
    NoCookie --> Requested: POST /pair/request (edge) → pair/request (API): a code, a 10 minute cookie
    Requested --> NoCookie: the code ran out, or a new request replaced it
    Requested --> Approved: the owner approves the code at the terminal (the CLI, in core)
    Approved --> Paired: POST /pair/claim (edge) → pair/claim (API): the year-long cookie
    Paired --> Paired: any request (the API refreshes `used`, at most hourly)
    Paired --> NoCookie: unpair (an action), or a year unused
```

The states live in drover's `devices` table, as today. The edge's only part is to carry the cookie: it sets the
value the API hands it and sends its hash back. `Open` in the API's `Viewer` is every state but `Paired`.
