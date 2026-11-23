# mcp-server: module boundaries and statecharts (#1627)

## Who may call whom

```
 agent ─/mcp, its key─▶ server (the edge) ── relay.rs, the Who::Key line ──▶ the door ──▶ api: Server::route()
                         Host, Origin; the key hashed into Drover-Key,                      │
                         beside serve-token and Drover-Ip                                   │
 an agent on the host ─/mcp, its key as the bearer, the API's loopback address (S4) ───────▶│
                                                                                            ├─ Drover-Device ─▶ Viewer::Device ─┐
                                                                                            └─ /mcp ─▶ mcp.rs ─▶ Viewer::Agent ─┤
                                                                                                         └─ TOOLS (a const)     ▼
                                                                  v1.rs api() (the guard) ─▶ jobs, write(), chat::post ─▶ core
 the orchestrator's shell ─▶ the lane hook ─▶ callers::asked ◀─ orchestrator_answer              (answer_with, queue_change)
```

New code goes in new files; the `shared` files gain single-line arms and stay under their baselines.

```
crates/core/src/callers.rs            Key, Scope: the table, add, revoke, hashed, check, the `mcp-` prefix, asked
crates/api/src/serve/caller.rs        what a Viewer::Agent may reach, the stamp, keyed
crates/api/src/serve/mcp.rs           JSON-RPC framing, both eras, the tool table, the job wait
crates/server/src/relay.rs            one line and Who::Key (#1645's file); engine.rs: Id::Key, Delete, four headers
crates/cli/tests/mcp.rs               the AC, through Server::respond
crates/cli/tests/golden/contract/v1/  mcp.json, mcp.keys (#1571's folder)
```

| Module | Owns | May call | Must not |
|---|---|---|---|
| `core::callers` (new) | `Key`, `Scope`, the `callers` table, the prefix of an agent's `by`, which thread an agent holds | the database, `devices`' random and sha256 helpers, the events | know MCP, a route or a tool |
| `core` `answer_with` (kept, two rules more) | which asks an agent may answer; what a queue ask's [queue] runs | `callers::is_agent`, `queue_change` | be skipped: every answer path ends here |
| `core` `queue_change` (kept, one rule more) | an agent's add becomes the queue ask | `callers::is_agent`, the ask's writer | queue for an agent |
| `core` `orchestrator_answer` (kept, one rule more) | an orchestrator's `answer` and `extend` in a held thread go to the owner | `callers::asked`, `to_owner` | accept a `--basis` there |
| the lane hook (kept, lines more) | `OWNER_ONLY` for a lane running `serve keys`; for a held thread's orchestrator running a group verb or a `spawn` | `callers::asked`, once, only for an orchestrator's drover verb | read the log for any other call |
| `core::cli` `queue add` (kept) | an orchestrator's add from a held thread is stamped the agent's | `callers::asked` | — |
| `api` `serve::caller` (new) | what `Viewer::Agent` may reach: `AGENT_READS`, `AGENT_ACTS`, `refused`, `stamped`; `keyed`, the one reader of a key | `callers::check`, `callers::hashed` | read the tool table: the route list is its own, reviewed as code. Read `Drover-Key` beside any token but the edge's, a bearer key from a peer off loopback, or either on a path other than `/mcp` |
| `api` `serve::v1` `api()` (kept) | every `/api/v1` route, the guard | as today | name an actor as a literal: `by` and `via` come from the stamp |
| `api` `write`, `chat::post` (kept) | the ops | as today | the same |
| `api` `serve::mcp` (new) | the wire: parse, answer, list, call, await a job, map the answer; `TOOLS` | `Server::api`, `Server::keyed`, `Server::job`, serde | call core, write an event, branch on a tool's name, check Host or Origin |
| `server` `relay.rs`, `engine.rs` (#1645's) | that `/mcp` is relayed, and its caller's bearer hashed into `Drover-Key` | the door, its own sha256 | keep a key or a list of them, parse the body, pass a client's `Authorization`, cookie or `Drover-*` on, know a tool |
| `crates/cli` | the `serve keys` verb | `core::callers` | — |

Three tests hold the edges. Every tool's route is in the agent's list and has a contract fixture. Every `/api/v1`
POST fixture, called with an act key, is `OWNER_ONLY` unless it is one of the four acts. And #1645's `edge.rs`,
its `Who::Key` half: a client's `Authorization` reaches the door only as `Drover-Key`, on `/mcp` alone.

## Who may reach which route

| Route | No key, no pairing | Read key | Act key | Paired device |
|---|---|---|---|---|
| `/mcp` itself | 401 | yes | yes | 401 (a cookie is not a key) |
| `state`, `queue`, `waiting`, `needs`, `repo/…/threads`, `thread/…` | as today | yes, the unpaired view | yes, the unpaired view | as today |
| `job/<id>` | `UNPAIRED` | yes | yes | yes |
| `answer` | `UNPAIRED` | `READ_ONLY` | a `routine` ask; else `OWNER_ASK` | yes, as the owner |
| `bqueue` | `UNPAIRED` | `READ_ONLY` | an owner ask is made | yes, as the owner: queued |
| `say`, `issue` | `UNPAIRED` | `READ_ONLY` | yes | yes, as the owner |
| a lane's transcript, `shed`, `config`, every other POST | as today | `OWNER_ONLY` | `OWNER_ONLY` | as today |

A key is read only at `/mcp`: as `Drover-Key` beside the edge's token, or as the bearer from a loopback peer.
Sent to any other path, or as the device cookie, it is no credential.

## A key

```text
 (none)
   │ serve keys add <name> [--act]          logs mcp-key-added; the secret is printed once
   ▼
 [live] ── each call: the hash matches ──▶ [live]              `used` moves at most hourly
   │
   ├─ serve keys revoke <name> ──▶ [revoked]                    logs mcp-key-revoked; final
   └─ unused for a year ────────▶ [expired]                    final; revoke it, add a new name

 [revoked], [expired]: every call is 401
 add with a name in any state: REFUSED KEY_EXISTS
```

The edge knows no key: it hashes and passes on. There is no cache between a call and the table and no session
to end, so the call after a revoke is refused.

## A call

```text
 POST /mcp at the edge
   ├─ a foreign Host or Origin ────────────────────────────▶ 403
   ├─ the API is away ─────────────────────────────────────▶ 503 BUSY
   ▼ the bearer hashed into Drover-Key, beside serve-token and Drover-Ip; the client's Authorization dropped
 POST /mcp at the API (straight there: the bearer itself, from a loopback peer)
   ├─ GET, DELETE ─────────────────────────────────────────▶ 405
   ├─ Drover-Key beside another token, or with Drover-Device ▶ 400
   ├─ no live key for the hash · a bearer off loopback ────▶ 401
   ├─ not a message · a bad version · headers unlike body ─▶ 400 with its JSON-RPC error
   ▼
 [framed] ─ initialize · server/discover · ping · tools/list ─▶ answered from the table alone
   │ tools/call
   ├─ no such tool for this key ───────────────────────────▶ error -32602
   ├─ over the key's limit ────────────────────────────────▶ result: REFUSED RATE_LIMIT
   ├─ an argument missing or mistyped ─────────────────────▶ result: REFUSED BAD_VALUE
   ▼
 [routed] ─ Server::api(method, route, fields, the caller)
   │
   ├─ the guard ───────────────────────────────────────────▶ result: REFUSED OWNER_ONLY | READ_ONLY
   ├─ a read ──────────────────────────────────────────────▶ result: the JSON
   ├─ an op that runs at once (bqueue, say) ───────────────▶ result: its line | REFUSED <CODE>
   └─ a job (answer, issue) ──▶ [job running]
                           ├─ done ────────────────────────▶ result: its line
                           ├─ refused ─────────────────────▶ result: REFUSED <CODE>
                           └─ still running after 20 s ────▶ result: ACCEPTED, the job's id
                                                               (the job goes on; the job tool reads its end)
```

A call that reaches `[routed]` is the same call a screen makes: the same job order, the same refusals, the same
events, with the caller's name in place of the owner's.

## A queue request (q3299 (d))

```text
 an issue, not queued
   │ an agent's enqueue · an orchestrator's `queue add` from a held thread
   ▼
 [asked]  one owner ask on the issue, [queue] [no]; the queue is as it was; no lane can start
   │        the same request again ──▶ [asked], the same ask's id
   ├─ the owner taps [queue] ──▶ [queued]   the kept add runs, `by: owner`, `asked: mcp-<name>`; drive goes on as today
   └─ the owner taps [no] ─────▶ not queued; a later request asks again
```

Only the owner leaves `[asked]`: the ask is `kind: owner` and routed to no thread, so an agent's or an
orchestrator's answer to it is `OWNER_ASK`.

## A thread (q3299 (b))

```text
 [free] ── a line from mcp-<name> (say), or a handoff out of a held thread ──▶ [held by mcp-<name>]
 [held] ── a line from the owner in the thread ─────────────────────────────▶ [free]
 [held] ── another agent's line ────────────────────────────────────────────▶ [held by that agent]
```

The state is `callers::asked` over the log: no flag is stored, so a replay gives the same answer. The
orchestrator's own replies and `system` lines change nothing.

| The thread's orchestrator runs | In `[free]` | In `[held]` |
|---|---|---|
| `answer --basis`, `extend --basis` | as today | the ask goes to the owner; `OWNER_ASK`, naming the agent |
| `answer --typed`, `extend --typed` | as today | as today: the line is the owner's own words about that ask |
| a capability-group verb, `spawn --force` | as today | `OWNER_ONLY`, naming the agent; a run card or the owner's line is the way |
| `queue add`, any other `spawn` (the open question) | as today | the queue ask; `OWNER_ONLY` |
| `thread reply`, `ask`, `card post`, `config propose`, reads | as today | as today |
| `thread handoff` | as today | the target thread is held by the same agent |
