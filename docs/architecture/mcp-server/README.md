# mcp-server: any agent drives drover through its API (#1627)

## Summary

Decision: drover's API gains one MCP address over HTTP (no stdio); the page's server only passes it on. Each outside
agent gets a named key the owner makes and revokes; every act is logged under the agent's name. An agent is never the
owner: it answers routine asks only, an issue it queues waits for the owner's tap, and its line in a thread cannot
make an orchestrator use an owner power.
Slices: keys and the rules (core, now); then, after #1645's API crate: the caller through the API, the MCP address, docs.
Open questions: an orchestrator that queues or spawns because an agent asked also waits for the owner (decided yes)?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the owner, an agent and the code read after the change, and the test |
| `adr.md` | the question, the facts, the options, the decision, the order with #1645, the slices, the open question, the `.drover/arch.toml` lines |
| `shapes.md` | the key, the caller, the tool table, the JSON-RPC messages, the three new routes, the queue ask, the events and codes |
| `boundaries.md` | which module may call which, who may reach which route, and four statecharts: a key, a call, a queue request, a thread |

## What the owner's revise changed (q3299)

| | Was | Is |
|---|---|---|
| (a) | `mcp.rs` and `caller.rs` in the page's server | in `drover-api` (#1645); the edge relays `/mcp` with the caller's own `Authorization`. #1645's S1 lands before any MCP code in the server; the core slice lands now |
| (b) | a prompt line told the orchestrator what an agent's message is | the log holds it: from an agent's line until the owner's next, that thread's orchestrator cannot answer, extend, run a group verb or `spawn --force` |
| (c) | a tool a TOML file, joined by a build step | one const table in `mcp.rs` |
| (d) | an agent's queued issue started by itself | it is an owner ask; nothing is queued before the owner's tap |

## The design on one screen

```
 an outside agent            the edge (drover serve)        drover-api                              core
 ────────────────            ───────────────────────        ──────────                              ────
        │ POST /mcp
        │ Authorization: Bearer <key>
        ▼
   Host and Origin checked ──▶ relayed whole, the ──▶ ┌─ serve/mcp.rs ─────────────┐
   (relay.rs, a keyed line)    key untouched          │ key → Caller::Agent(name)  │
                                                      │ tools/list: the const table│
                                                      │ tools/call: one route each │──▶ Server::api ──▶ answer_with,
                                                      │ a job: read to its end     │    (one guard)    queue_change,
                                                      └────────────────────────────┘                   say, hygiene::gh
                               JSON, or REFUSED <CODE> ◀──────────────────────────────                 by: mcp-<name>
```

| Today | After |
|---|---|
| A caller is unpaired or a paired device, and a paired device is the owner | A third kind: an agent, named by its key, never the owner |
| An answer from the board is logged `by: owner` whoever sent it | The log's `by` is the caller's: `owner`, or `mcp-<name>` |
| An outside agent has no way in | Seven reads and four acts as MCP tools, each one `/api/v1` route |
| `/api/v1` cannot list what waits, say in a thread or file an issue | Three new routes, with contract fixtures, for any client |

## What an agent may do

| | Tool | The route behind it | For an agent |
|---|---|---|---|
| read | `status`, `queue`, `waiting`, `asks`, `threads`, `thread`, `job` | `state`, `queue`, `waiting` (new), `needs`, `repo/<fleet>/threads`, `thread/<t>`, `job/<id>` | the unpaired view, plus its jobs |
| act | `answer` | `answer` | a routine ask; any other is `OWNER_ASK` |
| act | `file` | `issue` (new) | files the issue |
| act | `enqueue` | `bqueue` | asks the owner; the owner's tap queues it |
| act | `reply` | `say` (new) | a line from `mcp-<name>`; the thread is held until the owner writes there |
| never | merge overrides, extend, park, unpark, drive, pause, run cards, config, secrets, hosts, lane prompts and kills, pairing | no tool | the route itself refuses an agent `OWNER_ONLY` |

An agent that wants an owner-only thing says so in a thread. The thread's orchestrator turns it into an ask or a
run card, and the owner approves that on the board, as today. The orchestrator cannot do it instead.

## What the owner decides

1. In a thread an agent wrote in last, the orchestrator's own `queue add` becomes the same owner ask, and its
   `spawn` is refused. Decided: yes, since otherwise an agent starts lanes by asking the orchestrator. The other
   choice keeps to the four verbs of q3299 (b).

Two things only the owner can do, outside this issue: give the page's server an address a cloud bot can reach,
and paste the key into the bot.
