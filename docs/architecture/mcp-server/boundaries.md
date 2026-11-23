# mcp-server: module boundaries and statecharts (#1627)

## Who may call whom

```
 HTTP ─▶ serve.rs route() ─┬─ cookie ─▶ Caller::Device ─┐
                           │                            ├─▶ v1.rs api() ─▶ jobs, write(), chat::post ─▶ core
                           └─ /mcp ─▶ mcp.rs ───────────┘     (the guard)     (the caller's by and via)
                                       │  bearer ─▶ Caller::Agent
                                       └─ the tool folder (data)
```

New code goes in new files; the `shared` files gain single-line arms and stay under their baselines.

```
crates/core/src/callers.rs           Key, Scope: the table, add, revoke, check, the `mcp-` prefix
crates/server/src/serve/caller.rs    Caller, the agent's route list, the stamp
crates/server/src/serve/mcp.rs       JSON-RPC framing, both eras, the tool table, the job wait
crates/server/mcp/*.toml             one tool a file
crates/server/build.rs               joins the folder into the build's output
crates/cli/tests/mcp.rs              the AC, through Server::respond
```

| Module | Owns | May call | Must not |
|---|---|---|---|
| `core::callers` (new) | `Key`, `Scope`, the `callers` table, the prefix of an agent's `by` | the database, `devices`' random and sha256 helpers, the events | know MCP, a route or a tool |
| `core` `answer_with` (kept, one rule more) | which asks an agent may answer | `callers::is_agent` | be skipped: every answer path ends here |
| `serve::caller` (new) | `Caller`, `AGENT_READS`, `AGENT_ACTS`, `stamped` | `devices`, `callers` | read a tool file: the route list is code, reviewed as code |
| `serve::v1` `api` (kept) | every `/api/v1` route, the guard | as today | name an actor as a literal: `by` and `via` come from the stamp |
| `serve` `write`, `serve::chat` `post` (kept) | the ops | as today | the same |
| `serve::mcp` (new) | the wire: parse, answer, list, call, await a job, map the answer | `Server::api`, `Server::allowed`, `Server::keyed`, `Server::job`, serde, toml | call core, write an event, branch on a tool's name |
| the tool folder (new) | what MCP offers | nothing: data | name a route outside the agent's list (a test fails) |
| `crates/cli` | the `serve keys` verb | `core::callers` | — |
| the lane hook (kept) | `OWNER_ONLY` for a lane running `serve keys` | — | — |

Two tests hold the edges. Every tool's route is in the agent's list and has a contract fixture. Every `/api/v1`
POST fixture, called with an act key, is `OWNER_ONLY` unless it is one of the four acts.

## Who may reach which route

| Route | No key, no pairing | Read key | Act key | Paired device |
|---|---|---|---|---|
| `/mcp` itself | 401 | yes | yes | 401 (a cookie is not a key) |
| `state`, `queue`, `waiting`, `needs`, `repo/…/threads`, `thread/…` | as today, from the board | yes, the unpaired view | yes, the unpaired view | as today |
| `job/<id>` | `UNPAIRED` | yes | yes | yes |
| `answer` | `UNPAIRED` | `READ_ONLY` | a `routine` ask; else `OWNER_ASK` | yes, as the owner |
| `bqueue`, `say`, `issue` | `UNPAIRED` | `READ_ONLY` | yes | yes, as the owner |
| a lane's transcript, `shed`, `config`, every other POST | as today | `OWNER_ONLY` | `OWNER_ONLY` | as today |

A key is read only at `/mcp`. Sent to `/api/v1` directly, or as the device cookie, it is no credential.

## A key

```text
 (none)
   │ serve keys add <name> [--act]          logs mcp-key-added; the secret is printed once
   ▼
 [live] ── each call: the hash matches ──▶ [live]              `used` moves at most hourly
   │
   ├─ serve keys revoke <name> ──▶ [revoked]                    logs mcp-key-revoked; final
   └─ unused for a year ────────▶ [expired]                    final; revoke it, add a new name

 [revoked], [expired]: every call is 401, and one serve.log line says which
 add with a name in any state: REFUSED KEY_EXISTS
```

There is no cache between a call and the table, and no session to end: the call after a revoke is refused.

## A call

```text
 POST /mcp
   │
   ├─ a foreign Host or Origin ────────────────────────────▶ 403
   ├─ no live key ─────────────────────────────────────────▶ 401
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
   ├─ an op that runs at once (bqueue, say, issue) ────────▶ result: its line | REFUSED <CODE>
   └─ a tap (answer) ──▶ [job running]
                           ├─ done ────────────────────────▶ result: its line
                           ├─ refused ─────────────────────▶ result: REFUSED <CODE>
                           └─ still running after 20 s ────▶ result: ACCEPTED, the job's id
                                                               (the job goes on; the job tool reads its end)
```

A call that reaches `[routed]` is the same call a screen makes: the same job order, the same refusals, the same
events, with the caller's name in place of the owner's.
