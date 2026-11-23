# mcp-server: any agent drives drover through the board's API (#1627)

## Summary

Decision: drover's board server gains one MCP address, over HTTP only (no stdio). Each outside agent gets a named
key the owner makes and revokes on the host; a key reads, or reads and acts. Every tool is one board API call,
logged under the agent's name. An agent is never the owner: it answers routine asks only, no owner verb is a tool.
Slices: keys; the caller's name through the board API, with three missing routes; the MCP endpoint and its tool
folder; docs and the orchestrator's rule.
Open questions: may an issue an agent queues start by itself (decided yes)? May a key ever answer owner asks (no)?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the owner, an agent and the code read after the change, and the test every slice shares |
| `adr.md` | the question, the facts, the options, the decision, the consequences, the slices, the open questions, the `.drover/arch.toml` lines |
| `shapes.md` | the key, the caller, a tool file, the JSON-RPC messages, the three new routes, the events and codes |
| `boundaries.md` | which module may call which, who may reach which route, and two statecharts: a key, a call |

## The design on one screen

```
 an outside agent                         drover serve                                   core
 ────────────────                         ────────────                                   ────
 (a Teams bot, any MCP client)
        │  POST /mcp
        │  Authorization: Bearer <key>
        ▼
   ┌─ serve/mcp.rs ──────────────┐   the same call a screen makes
   │ key → Caller::Agent(name)   │ ─────────────────────────────▶  Server::api  ─▶  answer, queue_change,
   │ tools/list: the tool folder │      (method, /api/v1 route,     (one guard:      orchestrator::post,
   │ tools/call: one route each  │       fields, the caller)         what this        hygiene::gh
   │ a tap's job: read to its end│ ◀─────────────────────────────    caller may       each logs `by: mcp-<name>`
   └─────────────────────────────┘      JSON, or REFUSED <CODE>      reach)
```

| Today | After |
|---|---|
| A caller is unpaired or a paired device, and a paired device is the owner | A third kind: an agent, named by its key, never the owner |
| An answer from the board is logged `by: owner` whoever sent it | The log's `by` is the caller's: `owner`, or `mcp-<name>` |
| An outside agent has no way in | Seven reads and four acts as MCP tools, each one `/api/v1` route |
| `/api/v1` cannot list what waits, say in a thread or file an issue | Three new routes, with contract fixtures, for any client |

## What an agent may do

| | Tool | The route behind it |
|---|---|---|
| read | `status`, `queue`, `waiting`, `asks`, `threads`, `thread`, `job` | `state`, `queue`, `waiting` (new), `needs`, `repo/<fleet>/threads`, `thread/<t>`, `job/<id>` |
| act | `file`, `enqueue`, `answer`, `reply` | `issue` (new), `bqueue`, `answer`, `say` (new) |
| never | merge overrides, extend, park, unpark, drive, pause, run cards, config, secrets, hosts, lane prompts and kills, pairing | no tool; the route itself refuses an agent `OWNER_ONLY` |

An agent that wants an owner-only thing says so in a thread. The thread's orchestrator turns it into an ask or a
run card, and the owner approves that on the board, as today.

## What the owner decides

1. An act key can start lanes: it files an issue and queues it. Decided: allowed, since the issue asks for both
   acts; the row shows `needs-check` and the Events tab names the agent. The other choice holds each such issue
   behind one tap.
2. A key never answers an ask that waits for the owner, even when you tell the bot to in Teams: drover cannot see
   that it was you. Decided: refused `OWNER_ASK`; you answer on the board.

Two things only the owner can do, outside this issue: give serve an address a cloud bot can reach, and paste the
key into the bot.
