# mcp-server: the data shapes (#1627)

Everything here is new unless it says "today" or "#1645's". Names are the ones `usage.md` uses. Paths under
`api/src/` are #1645's API crate, and `Viewer`, `Who`, `Id` and `Relayed` are its types after its S2 (d4863aa).

## core: a key

```rust
// crates/core/src/callers.rs
/// What an outside agent's key lets it do.
pub enum Scope {
    Read,
    Act,
}

/// One outside agent's key as kept (the `callers` table): never the key itself, only its sha256.
pub struct Key {
    pub name: String,         // [a-z0-9][a-z0-9-]{0,23}, unique for good: a revoked name is never given again
    pub hash: String,         // sha256 of the secret, lowercase hex
    pub scope: Scope,
    pub created: u64,
    pub used: u64,            // written at most hourly, off the request path, as a device's
    pub revoked: Option<u64>,
}

/// A new key's secret, `dmcp_` and 64 hex chars, returned once. `BAD_VALUE` (the name), `KEY_EXISTS`.
pub fn add(ctx: &Ctx, name: &str, scope: Scope) -> Result<String>;
/// That key stops at once. `NO_SUCH_KEY`; a second revoke is a no-op.
pub fn revoke(ctx: &Ctx, name: &str) -> Result<()>;
/// The sha256 of a `dmcp_` bearer, as the table keeps it and the edge sends it; None for any other value.
pub fn hashed(bearer: &str) -> Option<String>;
/// The live key a hash names, else why not: `no key`, `unknown key`, `revoked`, `expired`, `db error: …`.
pub fn check(ctx: &Ctx, hash: &str) -> Result<Key, String>;
pub fn list(ctx: &Ctx) -> Vec<Key>;

/// The log's `by` for a key: the only place the prefix is written.
pub fn by(name: &str) -> String;       // "mcp-<name>"
/// Whether a `by` is an outside agent's.
pub fn is_agent(by: &str) -> bool;
/// The agent holding a thread: the sender of the newest line there that is the owner's or an agent's, when it is an agent's.
pub fn asked<'a>(events: &'a [Event], thread: &str) -> Option<&'a str>;
```

```sql
CREATE TABLE IF NOT EXISTS callers (n INTEGER PRIMARY KEY, json TEXT NOT NULL);
```

The table is its own, as `devices` and `push` are: nothing that reads a device can read a key.

`asked` reads two events: a `thread-msg` whose `from` is `owner` or an agent's, and a `thread-handoff` into the
thread that carries `asked` (below). The orchestrator's own lines and `system` lines do not count. The hook's
`Policy` gains `held: Option<String>`, set from `asked` for an orchestrator's thread and for no other lane.

## core: the queue ask (q3299 (d))

An agent's queue add logs no `queue` event. It logs one ask on the issue, on the pattern of the extend ask:

```json
{"kind": "owner", "question": "mcp-grok asks to queue #1641: board: dark mode for the queue tab",
 "options": ["queue", "no"], "queue": {"op": "add", "issues": [1641], "check": true}, "asked": "mcp-grok"}
```

That is the data of a `needs-decision` event on the fleet and issue; `queue` marks it as `extend` marks an extend ask.

- No `recommend`: an owner ask never takes a default.
- It is routed to no thread, so an orchestrator's answer is `OWNER_ASK`; an agent's is too (not `routine`).
- `answer_with`, on `queue`: runs the kept op through `queue_change` with the answerer as `by`. On `no`: nothing.
- A second add of an issue whose ask is open answers with that ask's id. An add of a queued issue answers as today.

## API: a caller

The caller is #1645's `Viewer` (`Open`, `Device`, `Agent`, `Plugin`, `Local`). #1645 reserves `Agent`; this issue
gives it its scope and everything it may do. There is no second caller type.

```rust
// crates/api/src/: #1645's enum, the one variant this issue fills in
pub enum Viewer {
    /// A key the `callers` table knows: through the edge as `Drover-Key`, or on loopback as the bearer itself.
    Agent { key: String, label: String, scope: callers::Scope, ip: String },   // key: the hash; label: the key's name
}

// crates/api/src/serve/caller.rs
impl Viewer {
    pub fn by(&self) -> String;            // an agent: "mcp-<label>"; any other writer: "owner", as today
    pub fn via(&self) -> &'static str;     // an agent: "mcp"; any other: "web", as today
    /// Why this viewer may not call the route: `OWNER_ONLY` or `READ_ONLY`; None for any viewer but an agent.
    pub fn refused(&self, method: &str, route: &str) -> Option<(&'static str, String)>;
}

const AGENT_READS: &[&str] = &["state", "queue", "waiting", "needs", "repo/", "thread/", "job/"];
const AGENT_ACTS: &[&str] = &["answer", "bqueue", "say", "issue"];

/// The op's fields with the viewer's `by` and `via`, and for an agent the current `mtime`; a body's own are dropped.
pub fn stamped(f: BTreeMap<String, String>, op: &str, who: &Viewer) -> BTreeMap<String, String>;

impl Server<'_> {
    /// The agent a `/mcp` request names, by its key's hash (`callers::check`); 401's reason otherwise.
    pub(super) fn keyed(&self, ctx: &Ctx, head: &str) -> Result<Viewer, String>;
}
```

`keyed` is the only place a key is read, and `/mcp` the only path that calls it (q3311 (2)):

| The request carries | The hash is | Else |
|---|---|---|
| `Authorization: Bearer <serve-token>` and `Drover-Key` | the header's value, 64 hex | no `Drover-Key`: 401. With `Drover-Device` too: 400 (#1645's rule) |
| `Authorization: Bearer dmcp_…`, from a loopback peer (the address the API's own listener saw, today's `X-Peer`) | `callers::hashed` of the bearer | another peer: 401, and the reason logged is `key off loopback` |
| `Drover-Key` beside any other token, or none | none | 400 (#1645's rule) |

`ip` is `Drover-Ip` in the first row and the peer in the second. The rate limiter's key is the hash.

## API: the tool table (q3299 (c))

```rust
// crates/api/src/serve/mcp.rs
struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    act: bool,                       // false: a read, listed for every key
    method: &'static str,            // "GET" | "POST"
    route: &'static str,             // under /api/v1/; `{arg}` is filled from the arguments
    args: &'static [Arg],
}

struct Arg {
    name: &'static str,
    kind: Kind,
    required: bool,
    description: &'static str,
}

enum Kind {
    Text,
    Number,
    Numbers,
}

const TOOLS: &[Tool] = &[/* one entry a tool, in this order */];
```

| Tool | Method and route | Arguments |
|---|---|---|
| `status` | `GET state` | |
| `queue` | `GET queue` | |
| `waiting` | `GET waiting` (new route) | |
| `asks` | `GET needs` | |
| `threads` | `GET repo/{fleet}/threads` | `fleet` |
| `thread` | `GET thread/{thread}` | `thread`; optional `before`, `n` |
| `job` | `GET job/{id}` | `id` |
| `file` | `POST issue` (new route, act) | `fleet`, `title`, `body` |
| `enqueue` | `POST bqueue` (act) | `fleet`, `issues` |
| `answer` | `POST answer` (act) | `fleet`, `id`, `choice` |
| `reply` | `POST say` (new route, act) | `thread`, `text` |

An argument named in the route fills it. The rest are the query of a `GET` and the JSON body of a `POST`. The
`inputSchema` of `tools/list` is built from `args`.

## The edge: one line (#1645's `server/src/relay.rs` and `engine.rs`; q3311 (1))

Slice 3 adds, in one PR, what #1645's `shapes.md` says arrives with this line:

```rust
// relay.rs
pub enum Who {
    Device,   // #1645's
    /// An agent: its bearer, hashed into `Drover-Key`. A cookie is dropped.
    Key,
}

Relayed { methods: &[Get, Post, Delete], path: "/mcp", stream: true, who: Key, until: None },

// engine.rs
pub enum Id {
    Device(String),   // #1645's
    /// Of the caller's bearer on a `Who::Key` line: sent as `Drover-Key`.
    Key(String),
}

pub enum Method { Get, Post, Put, Delete }   // Delete is new

const FORWARDED: [&str; 10] = [
    "accept", "accept-encoding", "content-type", "if-none-match", "last-event-id", "user-agent",
    "mcp-session-id", "mcp-protocol-version", "mcp-method", "mcp-name",
];
```

`mcp-method` and `mcp-name` are not in #1645's list of eight. Revision 2026-07-28 requires both to reach the
server that reads the body, so they come with the line.

What the API receives for an agent's `POST /mcp` with `Authorization: Bearer dmcp_5f0c…e91a`:

```
POST /mcp HTTP/1.1
Authorization: Bearer <serve-token>
Drover-Key: 77c2…0e                          64 hex: sha256 of the bearer the agent sent the edge
Drover-Ip: 100.64.0.9
MCP-Protocol-Version: 2026-07-28
Mcp-Method: tools/call
Mcp-Name: status
```

The edge hashes whatever bearer the line's caller sent; it does not know the `dmcp_` prefix, a tool or a key.
With no bearer it sends no `Drover-Key`, and the API's 401 goes back as it is. It never copies a `Drover-*` header
or a cookie from the client. Until #1645's S7 the plugin pass-through still exists; the `/mcp` line is matched
first, so a key never rides it.

The line's contract file is `mcp.json` with `mcp.keys` in #1571's frozen folder
(`crates/cli/tests/golden/contract/v1/*.keys`): the recorded answer to `tools/list` for an act key. A tool's name
and its arguments are then append-only.

## The three new `/api/v1` routes

| Route | Fields | Answer |
|---|---|---|
| `GET waiting` | none | `Waiting[]`, the type `drover waiting --json` prints today (`fleet`, `issue`, `lane`, `who`, `what`, `since`, `next_cmd`), from the waits the API already computes for the Lanes tab |
| `POST say` | `thread`, `text` | `Reply`: `sent`; the thread's `say` with the caller as sender. `NO_SUCH_THREAD`, `BAD_VALUE`, `RATE_LIMIT` |
| `POST issue` | `fleet`, `title` (one line, 1 to 200 chars), `body` | 202 and a job, as `answer`; the job ends `filed <fleet>#<n>` and logs `issue-filed`. The same `by`, fleet and title within the jobs' 15 min answers with the first number and files nothing. `BAD_VALUE`, `PUBLIC_LEAK` |

Each gets a fixture in the contract test's golden folder. `say` and `issue` are refused `UNPAIRED` for an unpaired
caller, as every write is. `issue` joins the jobs' ops, so `gh` stays off the request path.

## MCP on the wire

One JSON-RPC message a POST; one JSON object back.

```
method              era      result
initialize          legacy   {protocolVersion, capabilities: {tools: {}}, serverInfo: {name: "drover", version}, instructions}
notifications/*     legacy   202, no body
server/discover     modern   {resultType: "complete", supportedVersions, capabilities: {tools: {}}, instructions,
                              _meta: {"io.modelcontextprotocol/serverInfo": {name: "drover", version}}}
ping                both     {}
tools/list          both     {tools: [{name, title, description, inputSchema, annotations: {readOnlyHint}}]}
tools/call          both     {content: [{type: "text", text}], isError}
```

```rust
const VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];
const WAIT_SECS: u64 = 20;
const CALLS_A_MINUTE: usize = 60;
```

A request is modern when its `params._meta` carries `io.modelcontextprotocol/protocolVersion`; `initialize`
selects legacy and is answered with the client's version when it is in `VERSIONS`, else the newest legacy one.
No session id is ever sent.

| HTTP | JSON-RPC error | When |
|---|---|---|
| 403 | none | the edge: a foreign Host or Origin |
| 503 `BUSY` | none | the edge: the API is away |
| 401, `WWW-Authenticate: Bearer` | none | no key, unknown, revoked, expired, or a bearer off loopback; at most one log line a minute with the reason |
| 400 | none | `Drover-Key` beside a token that is not the edge's, or with `Drover-Device` (#1645's rule) |
| 405 | none | `GET`, `DELETE` |
| 400 | -32700, -32600 | not JSON; not a JSON-RPC message |
| 400 | -32022, with `supported` and `requested` | a version outside `VERSIONS` |
| 400 | -32020 | modern: `MCP-Protocol-Version`, `Mcp-Method` or `Mcp-Name` missing or unlike the body |
| 404 | -32601 | no such method |
| 200 | -32602 | no such tool, or one the key's scope does not list |

A known tool always answers 200 with a result; drover's refusal is in the result, not a protocol error, so the
model reads it. A modern result also carries `resultType: "complete"`.

| The route's answer | The tool's text | `isError` |
|---|---|---|
| 200 with a payload | the payload, as JSON | false |
| 200 `Reply` | its `message` | false |
| 202 `Reply` with a `job` | the job read until it ends: its `message` when done; `REFUSED <CODE>: <why>` when refused | by the end |
| the same, still running after `WAIT_SECS` | `ACCEPTED: job <id> is still running; read it with job` | false |
| any other `Reply` | `REFUSED <CODE>: <why>` (`BUSY` and `RATE_LIMIT` too); `error: <message>` for code `ERROR` | true |
| none: a required argument is missing or of the wrong type | `REFUSED BAD_VALUE: <tool> needs <arg> (<type>)` | true |

The refusal text is byte for byte what the CLI prints for that refusal.

## Events

| Kind | Fleet | Data |
|---|---|---|
| `mcp-key-added` | `drover` | `name`, `scope`, `by: owner` |
| `mcp-key-revoked` | `drover` | `name`, `by: owner` |
| `issue-filed` | the fleet, with the issue | `title`, `by`, `via` |
| `needs-decision` (today) | the fleet, with the issue | the queue ask: `kind: owner`, `queue`, `asked` |
| `decision` (today) | | `by: mcp-<name>`, `via: mcp` on a routine ask; `by: owner` on a queue ask |
| `queue` (today) | | `op: add`, `check: true`, `by: owner`, `asked: mcp-<name>`: logged by the queue ask's [queue] |
| `thread-msg` (today) | | `from: mcp-<name>` |
| `thread-handoff` (today) | | `asked: mcp-<name>` when it leaves a held thread |

A read logs nothing. The queue ask alerts as any owner ask does; no other line here pushes an alert.

## Codes

| Code | New here | When |
|---|---|---|
| `OWNER_ASK` | for an agent | its answer to an ask that is not `routine`; an orchestrator's `answer` or `extend` in a held thread (the ask goes to the owner) |
| `OWNER_ONLY` | as an API code; for a held thread | an agent on a route outside its list; a held thread's orchestrator running a group verb or a `spawn` |
| `READ_ONLY` | for a key | a read key on an act |
| `KEY_EXISTS` | yes | `keys add` with a name ever used |
| `NO_SUCH_KEY` | yes | `keys revoke` of an unknown name |
| `RATE_LIMIT` | for a key | over 60 tool calls a minute, or 10 acts |
