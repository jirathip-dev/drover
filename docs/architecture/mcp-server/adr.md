# ADR: an MCP server over drover's API (#1627)

## Summary

Decision: drover's API gains one MCP address over HTTP (no stdio); the page's server only passes it on. Each outside
agent gets a named key the owner makes and revokes; every act is logged under its name. An agent is never the owner:
it answers routine asks only, an issue it queues waits for the owner's tap, and its thread line gives no owner power.
Slices: keys and the rules (core, now); then, after #1645's S2 and #1571's contract folder: the caller through the
API, the MCP address with its one line in the page's server, docs.
Open questions: an orchestrator that queues or spawns because an agent asked also waits for the owner (decided yes)?

Status: revised after the owner's q3299 and q3311 (2026-11-23). What changed is marked "q3299 (a)" to "(d)" and
"q3311 (1)" to "(4)". The edge's part is #1645's as approved at d4863aa.

## The question

How does an agent that is not drover's own (a Teams bot, any MCP client) read the fleet and act on it, so that the
log says who acted, the owner's decisions stay the owner's, and no second copy of drover's rules appears?

## Facts

Read on staging 7c0a28c1c, on #1645's ADR as approved (`serve-split`, d4863aa) and on #1571's as proposed
(`cooee-threads`).

- **A caller is unpaired or a paired device, and a paired device is the owner.** `Auth` is a device or an error
  (`crates/server/src/serve.rs:129`). There is no third kind.
- **The board's acts name the actor as a literal.** An answer is logged `by: owner, via: web`
  (`crates/server/src/serve.rs:1551`); a thread message is from `owner` (`crates/server/src/serve/chat.rs:534`);
  a queue change is `via: web` with no `by`.
- **`/api/v1` lacks three of the asked operations.** No route for what waits (`drover waiting`), none to say in a
  thread, none to file an issue (drover files its own through `hygiene::gh`, in three copies).
- **Every ask says whose it is.** A lane's ask is `kind: routine` or `kind: owner`; every ask drover itself makes is
  `owner`, and `answer_with` runs what such an ask stands for (an extend, a park, a design decision).
- **An orchestrator's owner powers pass two places.** Its `answer` and `extend` end in core's
  `orchestrator_answer`, which knows the thread and sends a doubtful one to the owner (`OWNER_ASK`). Its group
  verbs and `spawn --force` pass the lane hook (`hook.rs`), which knows the thread from `DROVER_THREAD`.
- **`gh` never runs on a request.** An op that needs it is a job (`serve/jobs.rs`), answered 202 and read by id.
- **#1645 cuts the server in two.** `drover-api` (new crate, links core) holds `Server`, `route`, `v1.rs`, `chat.rs`
  and the jobs from its slice S1. `crates/server` becomes the edge: it checks Host and Origin and relays the paths
  of one table (`relay.rs`). From S2 it sends the API its own token and the caller as a hash (`Drover-Device`),
  and the API has one type for who asks, `Viewer`. The API checks neither Host nor Origin.
- **#1645 fixes how a key travels.** One `/mcp` line with `who: Key`: the edge hashes the caller's bearer into
  `Drover-Key`. It reserves `Viewer::Agent` and leaves the tools, the verbs and the key store to this ADR.
- **#1571 keeps `say` in drover** as `core::thread::say(…, from, text)`, a row cooee types into the agent.
- **MCP changed shape in 2026.** Revision 2026-07-28 has no handshake, no session and no `GET` stream: each POST
  carries its version and is answered alone, `server/discover` is required, and the headers `MCP-Protocol-Version`,
  `Mcp-Method` and `Mcp-Name` must match the body. Revisions up to 2025-11-25 open with `initialize`. A server may
  serve both on one address and may always answer with plain JSON.

## Options and choices

### 1. Transport

| Option | Verdict |
|---|---|
| stdio: a drover process the client starts | No. The callers the issue names are bots in a cloud. It would also be a second server, outside the job order and the rate limits |
| **Streamable HTTP, one address (chosen)** | One route arm in the API; one JSON answer a request, no stream, no session |

### 2. Where it sits (q3299 (a), q3311 (1) and (2))

| Option | Verdict |
|---|---|
| In the page's server | No. After #1645 it holds no state and may not call drover's code |
| A third listener | No. One more port, one more Host rule, for one route |
| **In `drover-api`, relayed by the edge (chosen)** | `mcp.rs`, `caller.rs` and the key check sit behind `Server::route`. The edge's relay table gets #1645's one `/mcp` line |

How the key reaches the API:

| Option | Verdict |
|---|---|
| The edge passes the caller's `Authorization` on whole (the first revision) | No. The API would answer a second kind of bearer from the edge, and #1645's S7 ends every passed-on bearer |
| **The edge hashes it into `Drover-Key` (chosen; #1645's rule)** | Beside the edge's token and `Drover-Ip`, as a cookie becomes `Drover-Device`. The client's `Authorization` is dropped. The API looks the hash up |

### 3. Who the caller is

| Option | Verdict |
|---|---|
| Pair the agent like a phone | No. A paired device is the owner |
| One shared token, like cooee's | No. One name for every agent, and revoking one revokes all |
| OAuth | No. An authorization server for one owner and a few bots |
| **A named key per agent, in its own table (chosen)** | The proxy's lane keys already work this way: hash kept, revoked by id, logged |

The owner makes a key on the host with a name and a scope, `read` or `act`. The log's `by` is `mcp-<name>`, built in
one place, so it can never read `owner`, `default` or an orchestrator's.

### 4. What an agent may do

| Act | Rule for an agent |
|---|---|
| answer an ask | a `routine` ask only; any other is refused `OWNER_ASK` and stays open for the owner |
| reply in a thread | a message into an existing thread, from `mcp-<name>`; see 5 |
| file an issue | through `hygiene::gh`, so `PUBLIC_LEAK` holds; a job, since it calls `gh` |
| queue it | **q3299 (d): starting lanes is spend, so it is the owner's.** The agent's queue request becomes an owner ask on that issue, [queue] or [no]. Nothing is queued until the owner taps; the queue event is then `by: owner` |

Owner-only verbs have no tool, and the API refuses an agent on every route outside its list.

### 5. An agent's line in a thread (q3299 (b))

A line from `mcp-<name>` reaches an orchestrator, which holds owner-level powers. A prompt rule alone would be a
rule nothing checks.

| Option | Verdict |
|---|---|
| The orchestrator's prompt says so | Not alone: a crafted line argues with a prompt |
| Track the orchestrator's turn | No. After #1571 cooee runs the agent; drover no longer sees a turn |
| **The thread has a state, read from the log (chosen)** | From an agent's line until the owner's next line there, the thread is *held* |

In a held thread, its orchestrator's `answer` and `extend` go to the owner as the ask they are (`OWNER_ASK`,
today's path), and a capability-group verb or a `spawn --force` is refused `OWNER_ONLY`. The ways through are the
existing ones: the owner writes in the thread, answers the ask, or approves a run card. A handoff out of a held
thread carries the agent's name, so the target thread is held too.

Decided, and the one open question: in a held thread the orchestrator's `queue add` becomes the same owner ask as
in 4, and any `spawn` is refused. Otherwise (d) is one message away from void.

### 6. What a tool maps onto

| Option | Verdict |
|---|---|
| The CLI's verbs, run in-process | No. Its reads are unbounded (`status --json` is 2.2 MB here), and several start `gh` or `git` |
| Logic of its own behind each tool | No. A second copy of the rules |
| **One `/api/v1` route per tool (chosen)** | The reads are bounded and cached; the acts carry the refusals. Three routes are added to `/api/v1` first, with fixtures |

**q3299 (c):** the tools are one const table in `mcp.rs`, a line a tool: name, description, arguments, method,
route. No TOML folder, no build step. `mcp.rs` fills the route, calls `Server::api` and maps the answer.

### 7. MCP revisions, and no dependency

Both eras on the one address: 2026-07-28 and the handshake of 2025-11-25 and 2025-06-18. The surface is five
methods over JSON the API already parses, so no SDK crate.

## Decision

1. **`POST /mcp` in `drover-api`** speaks MCP over HTTP, statelessly, to both eras. `GET` and `DELETE` answer 405:
   there is no stream to open and no session to end.
2. **The edge relays it on one line** (q3311 (1)), #1645's:
   `Relayed { methods: &[Get, Post, Delete], path: "/mcp", stream: true, who: Key, until: None }`. After its Host
   and Origin check the edge hashes the caller's bearer into `Drover-Key`, sends it beside its own `serve-token`
   and `Drover-Ip`, and drops the client's `Authorization`, the cookie and any `Drover-*` the client sent. It
   pipes the answer and reads none of it. The line is wider than what the API answers today, so a later stream
   needs no change in the edge.
   With the line the edge forwards four `mcp-` headers: #1645's `mcp-protocol-version` and `mcp-session-id`,
   and `mcp-method` and `mcp-name`, which revision 2026-07-28 requires and #1645's list lacks.
3. **A key per agent.** `serve keys add|revoke` and the list, owner only. Name, scope, hash; the secret is shown
   once. The API looks the hash up on every request, so a revoke stops the next call (q3311 (2)):
   - `Drover-Key` counts only beside the edge's `serve-token`; beside another token or with `Drover-Device` it is
     400, as #1645 has it;
   - the `dmcp_` bearer itself counts only from a loopback peer, straight at the API's own listener (from S4);
   - both are read at `/mcp` only. On any other path a key is an unknown token.
4. **A caller is #1645's `Viewer`**; this issue fills in its `Agent`. An agent's `by` is `mcp-<name>`, its `via`
   is `mcp`, stamped on an op's fields at the route (`caller.rs`); a request body cannot set them.
5. **One guard in `api()`** refuses an agent on any route outside its list (`OWNER_ONLY`) and a read key on an act
   (`READ_ONLY`). An agent reads what an unpaired browser reads, plus its jobs.
6. **Three rules in core**, where every caller passes:
   - an agent's answer to an ask that is not `routine` is `OWNER_ASK`;
   - an agent's queue add is an owner ask, and that ask's [queue] runs the add;
   - a held thread's orchestrator has no owner power (choice 5): `orchestrator_answer` and the hook read one
     function, `callers::asked`.
7. **Three routes join `/api/v1`**: `GET waiting`, `POST say`, `POST issue` (a job). The three copies of "file an
   issue and read its number" become one helper.
8. **A key is limited** to 60 tool calls a minute, 10 of them acts, and a call waits at most 20 s for its job.
9. **The board names the agent** in the Events line, the ask and the thread message. Words in rows that exist; no
   screen changes, so no design stage.

## Order with #1645 (q3299 (a), q3311 (3))

| Order | Slice | Waits for |
|---|---|---|
| first | **1. Keys and the rules.** All in `crates/core` and the CLI, files #1645 never moves | nothing; it lands now, the guards before the door |
| then | #1645's S0, S1 and **S2** (the API crate; the edge's relay table, `Who`, `Viewer` and the token beside an identity header); #1571's **D1** (the frozen contract folder and its test) | their own issues |
| then | **2. The caller through the API**, **3. the MCP address**, **4. words** | S2 for all three; slice 3 also D1 |

Slice 3 is one PR (q3311 (3)): `mcp.rs`, the `/mcp` relay line, `Who::Key` and `Id::Key`, the line's headers and
methods in the edge, and `/mcp`'s contract file. None of that is left to #1645's S4.

#1645 also allows the MCP PR before S2, with the bearer checked in the one server and S2 moving it behind the
header. Not taken: the key check would be built twice. What #1645's later slices then do:

- **S4** puts the door on a socket. The line and the request text are the same, so nothing of MCP changes. The
  API gets its own loopback listener there, which is when the bearer's direct way (decision 3) can first be reached.
- **S7** deletes the plugin pass-through. `/mcp` never used it: its line is matched first. The `Who::Key` half of
  S7's `edge.rs` check lands with slice 3.

The orchestrator files slice 1 as its own issue; #1627 keeps slices 2 to 4, blocked on S2's issue and on D1's.

## Consequences

- An agent with an act key can ask for work, never start it: one owner tap an issue. It can still file issues
  and write in threads at will; the limits, the scope and the revoke are the levers.
- An agent's line holds its thread: that thread's orchestrator sends its lane asks to the owner until the owner
  writes there. Safe, and slower. An agent belongs in a thread of its own, not a repo's working thread.
- A cloud bot reaches the page's server only at a public address. That is a host change, the owner's, and no part
  of this issue. There the key is the only gate: 256 random bits, never logged, kept and passed on only as its
  hash. On the host, a local client may call the API's loopback address directly with its key, from S4.
- The edge holds no key and no list. A revoke is one row in the API's table, and the edge's next relayed call is
  answered 401.
- MCP in the server waits for three #1645 slices and one of #1571's. Until then slice 1's rules have no caller
  but the CLI's `--by`.
- Clients that accept only OAuth cannot connect, and a stdio-only client needs a bridge.
- Reads are the board's payloads. One too big for an agent gets a slimmer `/api/v1` route; `mcp.rs` never trims.
- The tool table is a list in one file. With eleven tools and a new one rare, it is not split (q3299 (c)).
- `reply` is one route over the existing `say`; no thread screen is added, so #1571's freeze holds. After #1571
  the same route calls `core::thread::say` with the agent as `from`.
- An agent's answer or filed issue holds one request slot for up to 20 s; the act limit caps that at ten a key.
- About +520 lines of `src/`, the tool table included. Headroom today is 1,572 with 1,374 left after open PRs.

## Slices

1. **Keys and the rules.** `core::callers` (table, `add`, `revoke`, `check`, `asked`), `serve keys`, the two key
   events, the three core rules, the hook's lines. Proved by: add, list, revoke, a reused name refused; a
   `--by mcp-x` answer to an owner ask refused; an agent's add leaves the queue as it was and one ask open, whose
   [queue] queues; in a held thread an orchestrator's answer goes to the owner and `hosts off` is refused, and
   both work again after an owner line.
2. **The caller through the API.** `Viewer::Agent` and `keyed` (`caller.rs`), the stamp, the guard, the three
   routes with their fixtures, the one issue-filing helper, the board's words. Proved by: every POST fixture
   called as an agent with an act key is `OWNER_ONLY` but the four; a `Drover-Key` beside another token is 400,
   and a bearer key off loopback 401.
3. **The MCP address.** `mcp.rs` with its table, both eras, the job wait; the relay line, `Who::Key` and the
   contract file. Proved by the test in `usage.md` (the issue's AC, line by line) and the `Who::Key` cases in
   #1645's `edge.rs`.
4. **Words.** The serve docs' new section, the refusal lines, the orchestrator prompt's line on `mcp-<name>`.

## Open question

1. **Does the owner's hold on queueing reach through an orchestrator?** Decided: yes. In a held thread its
   `queue add` becomes the owner ask and its `spawn` is refused. The other choice keeps to the four of q3299 (b)
   and trusts the orchestrator's judgement on an agent's request to queue; it removes about 15 lines.

## Hot files and `.drover/arch.toml` lines

| File | Class | Here |
|---|---|---|
| the tool table in `mcp.rs` (new) | central list | kept as one const by q3299 (c); a tool is one line |
| the edge's `relay.rs` (#1645's) | central list of prefixes | one line, once |
| `crates/cli/tests/golden/contract/v1/*.keys` (#1571's) | collected folder, `append_only` | new files (`mcp.json`, `mcp.keys`), none edited |
| `serve.rs`, `v1.rs` (`shared`, in `drover-api` after S1) | large modules | single-line arms into `caller.rs` and `mcp.rs` |
| `docs/serve/*.md` | docs per area | one new file, `mcp.md` |

Slice 3 adds, in the PR that creates `mcp.rs`:

```toml
[[ban]]
pattern = "crate::"
paths = ["crates/api/src/serve/mcp.rs"]
reason = "#1627: an MCP tool is an /api/v1 route called through Server::api; the module holds no drover logic"

[[ban]]
pattern = "drover_core::"
paths = ["crates/api/src/serve/mcp.rs"]
reason = "#1627: an MCP tool is an /api/v1 route called through Server::api; the module holds no drover logic"
```

No `[[collect]]`, `frozen` or `generated` line: there is no folder, no old list and no generated file. #1571's
`append_only` glob already covers `mcp.keys`.
