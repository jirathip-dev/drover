# mcp-server: the data shapes (#1627)

Everything here is new unless it says "today". Names are the ones `usage.md` uses.

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
    pub used: u64,            // written at most hourly, as a device's
    pub revoked: Option<u64>,
}

/// A new key's secret, `dmcp_` and 64 hex chars, returned once. `BAD_VALUE` (the name), `KEY_EXISTS`.
pub fn add(ctx: &Ctx, name: &str, scope: Scope) -> Result<String>;
/// That key stops at once. `NO_SUCH_KEY`; a second revoke is a no-op.
pub fn revoke(ctx: &Ctx, name: &str) -> Result<()>;
/// The live key a bearer value names, else why not: `no key`, `unknown key`, `revoked`, `expired`, `db error: …`.
pub fn check(ctx: &Ctx, bearer: &str) -> Result<Key, String>;
pub fn list(ctx: &Ctx) -> Vec<Key>;

/// The log's `by` for a key: the only place the prefix is written.
pub fn by(name: &str) -> String;       // "mcp-<name>"
/// Whether a `by` is an outside agent's.
pub fn is_agent(by: &str) -> bool;
```

```sql
CREATE TABLE IF NOT EXISTS callers (n INTEGER PRIMARY KEY, json TEXT NOT NULL);
```

The table is its own, as `devices` and `push` are: nothing that reads a device can read a key.

## serve: a caller

```rust
// crates/server/src/serve/caller.rs
/// Who a request is from: what it may reach, and what the log's `by` says.
#[derive(Clone)]
pub enum Caller {
    Device(devices::Device),   // a paired device: the owner's hand
    Agent(callers::Key),       // an MCP key: an outside agent, never the owner
}

impl Caller {
    pub fn by(&self) -> String;            // "owner" | "mcp-<name>"
    pub fn via(&self) -> &'static str;     // "web"   | "mcp"
    pub fn label(&self) -> &str;           // the device's label | the key's name
    pub fn hash(&self) -> &str;            // the rate limiter's key
    pub fn owner(&self) -> bool;           // a device: the routes kept for a paired device
    /// Why this caller may not call the route: `OWNER_ONLY` or `READ_ONLY`; None for a device.
    pub fn refused(&self, method: &str, route: &str) -> Option<(&'static str, String)>;
}

const AGENT_READS: &[&str] = &["state", "queue", "waiting", "needs", "repo/", "thread/", "job/"];
const AGENT_ACTS: &[&str] = &["answer", "bqueue", "say", "issue"];

/// The op's fields with the caller's `by` and `via`, and for an agent the current `mtime`; a body's own are dropped.
pub fn stamped(f: BTreeMap<String, String>, op: &str, who: Option<&Caller>) -> BTreeMap<String, String>;
```

```rust
// crates/server/src/serve.rs, today: type Auth = Result<devices::Device, String>;
type Auth = std::result::Result<Caller, String>;
```

The cookie yields only `Caller::Device`; the bearer header is read only at `/mcp` and yields only
`Caller::Agent`. An agent has no rendered page to be stale against, so its ops get the current `mtime`, as a
thread's answer already does.

## serve: a tool

```rust
// crates/server/src/serve/mcp.rs
#[derive(Deserialize)]
struct Tool {
    name: String,                    // the file's stem
    title: String,
    description: String,
    #[serde(default)]
    act: bool,                       // false: a read; listed for every key
    method: String,                  // "GET" | "POST"
    route: String,                   // under /api/v1/; `{arg}` is filled from the arguments
    #[serde(default)]
    args: BTreeMap<String, Arg>,
    #[serde(default)]
    optional: Vec<String>,           // every other arg is required
}

#[derive(Deserialize)]
struct Arg {
    r#type: String,                  // "string" | "integer" | "integers"
    description: String,
}
```

An argument named in the route fills it. The rest are the query of a `GET` and the JSON body of a `POST`.

```
crates/server/mcp/status.toml     GET  state
crates/server/mcp/queue.toml      GET  queue
crates/server/mcp/waiting.toml    GET  waiting                    new route
crates/server/mcp/asks.toml       GET  needs
crates/server/mcp/threads.toml    GET  repo/{fleet}/threads
crates/server/mcp/thread.toml     GET  thread/{thread}            optional: before, n
crates/server/mcp/job.toml        GET  job/{id}
crates/server/mcp/file.toml       POST issue                      new route · act
crates/server/mcp/enqueue.toml    POST bqueue                     act
crates/server/mcp/answer.toml     POST answer                     act
crates/server/mcp/reply.toml      POST say                        new route · act
```

```rust
// crates/server/build.rs: the folder, joined in name order, as core's build.rs joins the usage parts
// OUT_DIR/mcp-tools.toml, one [[tool]] a file; mcp.rs reads it with include_str! once
```

## The three new `/api/v1` routes

| Route | Fields | Answer |
|---|---|---|
| `GET waiting` | none | `Waiting[]`, the type `drover waiting --json` prints today (`fleet`, `issue`, `lane`, `who`, `what`, `since`, `next_cmd`), from the waits serve already computes for the Lanes tab |
| `POST say` | `thread`, `text` | `Reply`: `sent`; the page's thread `say`, with the caller as sender. `NO_SUCH_THREAD`, `BAD_VALUE`, `RATE_LIMIT` |
| `POST issue` | `fleet`, `title` (one line, 1 to 200 chars), `body` | `Reply`: `filed <fleet>#<n>`; logs `issue-filed`. The same `by`, fleet and title within 15 min answers with the first number and files nothing. `BAD_VALUE`, `PUBLIC_LEAK` |

Each gets a fixture in the contract test's golden folder. `say` and `issue` are refused `UNPAIRED` for an unpaired
caller, as every write is.

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
| 401, `WWW-Authenticate: Bearer` | none | no key, unknown, revoked or expired; one `serve.log` line with the reason |
| 403 | none | a foreign Host or Origin |
| 405 | none | `GET` or `DELETE` |
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
| `decision` (today) | | `by: mcp-<name>`, `via: mcp` |
| `queue` (today) | | `op: add`, `check: true`, `by: mcp-<name>`, `via: mcp` |
| `thread-msg` (today) | | `from: mcp-<name>` |

A read logs nothing. None of these pushes an alert: an orchestrator's decision does not either.

## Codes

| Code | New here | When |
|---|---|---|
| `OWNER_ASK` | for an agent | its answer to an ask that is not `routine` |
| `OWNER_ONLY` | as an API code | an agent on a route outside its list |
| `READ_ONLY` | for a key | a read key on an act |
| `KEY_EXISTS` | yes | `keys add` with a name ever used |
| `NO_SUCH_KEY` | yes | `keys revoke` of an unknown name |
| `RATE_LIMIT` | for a key | over 60 tool calls a minute, or 10 acts |
