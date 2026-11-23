# ADR: an MCP server over the board's API (#1627)

## Summary

Decision: drover's board server gains one MCP address, over HTTP only (no stdio). Each outside agent gets a named
key the owner makes and revokes on the host; a key reads, or reads and acts. Every tool is one board API call,
logged under the agent's name. An agent is never the owner: it answers routine asks only, no owner verb is a tool.
Slices: keys; the caller's name through the board API, with three missing routes; the MCP endpoint and its tool
folder; docs and the orchestrator's rule.
Open questions: may an issue an agent queues start by itself (decided yes)? May a key ever answer owner asks (no)?

## The question

How does an agent that is not drover's own (a Teams bot, any MCP client) read the fleet and act on it, so that the
log says who acted, the owner's decisions stay the owner's, and no second copy of drover's rules appears?

## Facts

Read on staging 7c0a28c1c.

- **A caller is unpaired or a paired device, and a paired device is the owner.** `Auth` is a device or an error
  (`crates/server/src/serve.rs:129`). There is no third kind.
- **The board's acts name the actor as a literal.** An answer is logged `by: owner, via: web`
  (`crates/server/src/serve.rs:1551`); a thread message is from `owner` (`crates/server/src/serve/chat.rs:534`);
  a queue change is `via: web` with no `by`.
- **`/api/v1` lacks three of the asked operations.** It has no route for what waits (`drover waiting`), none to
  say in a thread (the page posts to its own form route, cooee to the screens' action), none to file an issue
  (drover files its own through `hygiene::gh`, in three copies: lesson, hotspots, triage).
- **Every ask says whose it is.** A lane's ask is `kind: routine` or `kind: owner`. Every ask drover itself makes
  (design, extend, respawn loop, lane hog, handoff) is `owner`. A routine ask takes its recommendation after 30
  min anyway, and one naming an owner word is refused when asked (`OWNER_TOPIC`).
- **An orchestrator cannot borrow the owner's voice.** `answer --typed` and `extend --typed` accept only a line
  whose sender is `owner`.
- **serve is hand-written HTTP on tokio and serde_json.** A request body is capped at 32 KB, 64 requests are
  handled at once, and the latency alert counts `/api/v1/` routes only.
- **The files this touches are at their size baselines.** `api` is 230 of 243 lines, `route` 173 of 184,
  `serve.rs` 3,478 of 3,594.
- **The CLI's reads are not agent-sized.** `drover status --json` is 2.2 MB on this host (2,250 issues, merged
  ones included). `/api/v1/state` holds only what is live.
- **MCP changed shape in 2026.** Revision 2026-07-28 has no handshake and no session: each POST carries its
  version and is answered alone, and `server/discover` is required. Revisions up to 2025-11-25 open with
  `initialize`. A server may serve both on one address. A server may always answer with plain JSON, never a
  stream. Authentication is the server's own choice.

## Options and choices

### 1. Transport

| Option | Verdict |
|---|---|
| stdio: a drover process the client starts | No. The callers the issue names are bots in a cloud; they cannot start a process on the host. It would also be a second server beside serve, reading the state itself, outside the per-issue job order and the rate limits |
| stdio as a thin pipe to serve | Not now. Any generic stdio-to-HTTP bridge does this for a local client; drover would add nothing |
| **Streamable HTTP on `drover serve`, one address (chosen)** | serve already owns the listener, the Host and Origin check, the job order and the caches. One route arm |

Every request gets one JSON answer. No stream, no session, no server-to-client message: drover offers tools only.

### 2. Who the caller is

| Option | Verdict |
|---|---|
| Pair the agent like a phone | No. A paired device is the owner; an MCP client cannot do the cookie-and-code dance |
| One shared token, like cooee's | No. One name for every agent, and revoking one revokes all |
| OAuth | No. An authorization server for one owner and a few bots; a static key is what these clients take today |
| A row in the devices table with a flag | No. A key sent as the device cookie would read as the owner unless every reader remembers the flag |
| **A named key per agent, in its own table (chosen)** | The proxy's lane keys already work this way: hash kept, revoked by id, logged |

The owner makes a key on the host with a name and a scope, `read` or `act`. The key's name is the identity:
the log's `by` is `mcp-<name>`, built in one place, so it can never read `owner`, `default` or an orchestrator's.
A scope is the one option kept: an agent picks its own tools, so a bot that only reports must not hold an act key.

### 3. What an agent may do

The issue's list, and nothing else:

| Act | Rule for an agent |
|---|---|
| answer an ask | a `routine` ask only; any other is refused `OWNER_ASK` and stays open for the owner. The owner may change an agent's answer within the window an orchestrator's answer has |
| reply in a thread | a message into an existing thread, from `mcp-<name>`; its orchestrator reads it as a request |
| file an issue | through `hygiene::gh`, so `PUBLIC_LEAK` holds; the same title from the same key within 15 min returns the first |
| queue it | `bqueue`, which marks the row `needs-check`; the queue event carries `by` |

Considered and left out: an agent's answer with a `--basis` quote, as an orchestrator gives. That rule leans on
the orchestrator's thread and repo; `routine` is one field on the ask and needs no judgement.

Owner-only verbs have no tool, and the route guard refuses an agent on every route outside its list, so a tool
file added by mistake cannot widen it. The path to an owner decision is the existing one: the agent says what it
wants in a thread, the orchestrator files the ask or the run card, the owner taps.

### 4. What a tool maps onto

| Option | Verdict |
|---|---|
| The CLI's verbs, run in-process | No. Its reads are unbounded (2.2 MB), and several start `gh` or `git`, which a board read must never do |
| Logic of its own behind each tool | No. A second copy of the rules |
| **One `/api/v1` route per tool (chosen)** | The reads are bounded and cached; the acts already carry the refusals. Three routes are missing and are added to `/api/v1` first, with fixtures, as that API's own rule says |

A tool is data: a name, a description, its arguments, a method and a route. The MCP module fills the route, calls
`Server::api` as the HTTP layer does, and maps the answer. It awaits a tap's job so a refusal comes back in the
same call; that is the board's toast, not a rule.

### 5. Which MCP revisions

Both eras, on the one address: 2026-07-28 (per-request version, `server/discover`, the mirrored headers checked)
and the handshake of 2025-11-25 and 2025-06-18. With no session and tools only, the old era costs one more method.
Supporting only the new one would lock out every client that has not moved.

### 6. A dependency

None. The surface is five methods over JSON that serve already parses. An SDK crate would bring its own HTTP
stack beside the hand-written one, and a new dependency is an owner call the design does not need.

## Decision

1. **`POST /mcp` on `drover serve`** speaks MCP over streamable HTTP, statelessly, to both protocol eras. `GET`
   and `DELETE` answer 405. The Host and Origin check is the API's.
2. **A key per agent.** A new `serve keys` verb adds, lists and revokes; owner only. A key has a name, a scope and a
   hash; the secret is shown once. Every request is checked against the table, so a revoke stops the next call.
   A key unused for a year expires, as a device does.
3. **A caller is a device or an agent**, one type in serve. An agent's `by` is `mcp-<name>` and its `via` is
   `mcp`. The caller is stamped on an op's fields at the HTTP edge; a request body cannot set them.
4. **One guard in `api`** refuses an agent on any route outside its list (`OWNER_ONLY`) and a read key on an act
   (`READ_ONLY`). An agent reads what an unpaired browser reads, plus its jobs; never a lane's transcript.
5. **One rule in core**: an agent's answer to an ask that is not `routine` is `OWNER_ASK`. It sits where every
   answer passes, so the CLI's `--by mcp-…` gets it too. The owner may change an agent's answer for as long as
   an orchestrator's (`orchestrator_change_minutes`).
6. **Three routes join `/api/v1`**: `GET waiting`, `POST say`, `POST issue`. The three copies of "file an issue
   and read its number" become one helper, used by all four callers.
7. **Tools are files** in one collected folder, joined by a build step. A test holds each to a route with a
   contract fixture and to the agent's list.
8. **A key is limited** to 60 tool calls a minute, 10 of them acts (the limit a device's sends have), and an
   answer waits at most 20 s for its job.
9. **The board names the agent**: the Events line and the thread message say `mcp-<name>` where they say `you`
   or `owner` today. Words in rows that exist; no screen changes, so no design stage.

## Consequences

- An agent with an act key can start work on the owner's hosts: it files an issue, queues it, and lanes build
  it. It can also ask an orchestrator, which can queue and spawn. This is what the issue asks for; the key is
  the trust, the scope and the revoke are the levers, and the log says who queued it.
- A cloud bot reaches serve only at a public address. That is a host change, the owner's, and no part of this
  issue. At such an address the key is the only gate: 256 random bits, hashed at rest, compared in constant
  time, never logged; each refused request is one line in `serve.log`.
- Clients that accept only OAuth cannot connect. A later issue, if one matters.
- No stdio: a local stdio-only client needs a bridge.
- Reads are the board's payloads, presentation fields included. One too big for an agent gets a slimmer
  `/api/v1` route first; the MCP module never trims.
- An agent's answer holds one of serve's 64 request slots for up to 20 s; the act limit caps that at ten a key.
- A name is never reused, so the keys table only grows, by one small row a key.
- The old handshake stays until clients have moved; deleting it is one arm.
- About +450 lines of `src/`. The cap has 1,572 lines of headroom and the queue is promised about 1,700, so
  this needs room in the next raise. The tool files and the build step are not `src/`.
- `api`, `route` and `serve.rs` stay under their baselines: each gains single-line arms that call into new
  files.

## Slices

In this order. Each is green alone: one PR's commits, or four issues if the orchestrator splits them.

1. **Keys.** `core::callers`, its table, `serve keys add|revoke` and the list, the two events, the hook's
   `OWNER_ONLY` for a lane. Proved by: add, list, revoke, a reused name refused.
2. **The caller through the API.** `Caller`, the stamp, the guard, the core rule, the three routes with their
   fixtures, the one issue-filing helper, the board's words. Proved by: an agent caller in `api_v1`'s tests;
   every POST fixture refused `OWNER_ONLY` but the four.
3. **The endpoint.** `serve/mcp.rs`, the tool folder and its build step, both eras, the job wait. Proved by the
   test in `usage.md`: the issue's AC, line by line.
4. **Words.** The serve docs' new section, the refusal lines, the orchestrator prompt's rule for a message from
   `mcp-<name>`.

## Open questions

1. **May an issue an agent queues start by itself?** Decided: yes, marked `needs-check`, the agent named in
   Events. The other choice: the agent's queue request becomes an owner ask, one tap a queued issue; about 20
   lines more, on the extend ask's pattern.
2. **May a key ever answer an ask that waits for the owner?** Decided: no. drover cannot tell that a line in
   Teams was the owner's, and an owner ask is where scope, spend and secrets are decided.

## `.drover/arch.toml` lines

Slice 3 adds, in the PR that creates the folder:

```toml
[[collect]]
dir = "crates/server/mcp"
glob = "*.toml"

[[ban]]
pattern = "crate::"
paths = ["crates/server/src/serve/mcp.rs"]
reason = "#1627: an MCP tool is an /api/v1 route called through Server::api; the module holds no drover logic"

[[ban]]
pattern = "drover_core::"
paths = ["crates/server/src/serve/mcp.rs"]
reason = "#1627: an MCP tool is an /api/v1 route called through Server::api; the module holds no drover logic"
```

No `frozen` line: there is no old list. No `generated` line: the joined table is written to the build's own
output folder, never committed.
