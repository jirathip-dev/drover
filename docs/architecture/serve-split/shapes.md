# serve-split: the data shapes (#1645)

The edge's types live in `crates/server/src/`. The API's payload types stay in `api.rs` (in `api/src/`
from S1) and reach `assets/types/api.d.ts` through `just types`.

## The door (`server/src/engine.rs`)

```rust
/// The one way the page's server reaches drover.
pub trait Engine: Send + Sync {
    /// One request, one whole answer. A door that cannot reach the API answers 503 `BUSY`, which the page already retries.
    fn ask(&self, req: &Request) -> Reply;
    /// A stream: the API's bytes go to `to` until either side closes.
    fn follow(&self, req: &Request, to: &mut dyn Write) -> std::io::Result<()>;
}

pub struct Request {
    pub method: Method,
    /// Path and query, as the browser sent them.
    pub target: String,
    /// Only the headers of `FORWARDED`; never a `Drover-*`, a cookie or an `Authorization`.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub viewer: Viewer,
}

/// What the edge knows about who asked. It decides nothing from it.
pub struct Viewer {
    pub id: Option<Id>,
    pub ip: String,
    /// A plugin's own bearer token, passed on in place of the edge's. S7 deletes the field.
    pub bearer: Option<String>,
}

/// One identity a request, by its line's `Who`. Each is a sha256 in hex: the edge passes on no secret.
pub enum Id {
    /// Of the `hd` cookie: sent as `Drover-Device`.
    Device(String),
    /// Of the caller's bearer on a `Who::Key` line: sent as `Drover-Key` (#1627).
    Key(String),
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub enum Method { Get, Post, Put, Delete }

const FORWARDED: [&str; 8] = [
    "accept", "accept-encoding", "content-type", "if-none-match", "last-event-id", "user-agent",
    "mcp-session-id", "mcp-protocol-version",
];
```

`Id::Key`, `Method::Delete`, `accept` and the two `mcp-` headers arrive with #1627's line, not before.

Two doors implement it. `drover::serve::InProcess` (in `crates/cli`, S1 to S3) writes the request as text and
calls `Server::respond`. `drover_server::engine::Http` (S4) writes the same text to the API's socket. The request
text is the same in both, so S4 changes no handler.

## On the wire, edge to API

```
GET /screens/queue?repo=api HTTP/1.1
Authorization: Bearer <serve-token>          until S7 also: the plugin's own bearer, passed on
Drover-Device: 5e0b…a2                       64 hex; absent: no cookie
Drover-Ip: 100.64.0.7
If-None-Match: "12.48"
```

and on the `/mcp` line (#1627), in place of `Drover-Device`:

```
POST /mcp HTTP/1.1
Authorization: Bearer <serve-token>
Drover-Key: 77c2…0e                          64 hex: sha256 of the bearer the agent sent the edge
Drover-Ip: 100.64.0.9
Mcp-Session-Id: …
```

The API reads `Drover-Device`, `Drover-Key` and `Drover-Ip` only beside the `serve-token`. With both identity
headers, or either beside another token, it answers 400.

What the API makes of it (`drover-api`; today's `Who` plus the unpaired case):

```rust
pub enum Viewer {
    /// The edge's token and no known device: an unpaired browser. Reads the open variant; no write.
    Open { ip: String },
    /// The edge's token and a device hash the `devices` table approves.
    Device { hash: String, label: String, ip: String },
    /// A key #1627's store knows: through the edge as `Drover-Key`, or on loopback as the bearer itself.
    Agent { key: String, label: String, ip: String },
    /// cooee's token.
    Plugin,
    /// In process: the terminal, a test.
    Local,
}
```

No token, or an unknown one: 401 `UNPAIRED`, whatever the path. The API binds loopback only. `Agent` and what it
may do are #1627's; only where its key travels is fixed here.

| Credential | Kept | Made by | Sent by |
|---|---|---|---|
| `cooee-token` | a file beside fleet.toml, mode 0600 | the API host at its first start (today: serve) | cooee, through the manifest's `attach_token`, to the API (to the edge until S4b) |
| `serve-token` | a file beside fleet.toml, mode 0600 | the API host at its first start (S4) | the edge; the launcher hands it the path |
| an agent's key (#1627) | its hash, in drover's database | #1627's verb; revoked there | the agent, as its bearer: to the edge on `/mcp`, or to the API on loopback |

## The edge's own paths

Everything else the edge answers is relayed or 404.

| Path | What | Asks the API |
|---|---|---|
| `GET /` | the page shell | `hello` (kept), `layout` (kept), `device` when a cookie came |
| `GET /a/<hash>/<file>`, `/sw.js`, the manifest and icons | the embedded files | nothing |
| `GET /themes/<name>/<file>` | an embedded theme, else a file of the themes folder the launcher named | nothing |
| `GET /pair/status` | `{"paired": bool}` | `device` |
| `POST /pair/request` | the code and the hint; sets the short `hd` cookie | `pair/request` |
| `POST /pair/claim` | sets the year-long `hd` cookie once approved | `pair/claim` |
| `POST /push/subscribe` | leaving (#1571's D4) | `push/subscribe` |
| `GET /health` | the edge's own build, pid, and its link to the API | nothing (it reports the last `hello`) |
| the device listener (`[serve] device_addr`): `GET /screens/scene` | the scene for a companion device | relayed |

## The relay table (`server/src/relay.rs`)

```rust
pub struct Relayed {
    pub methods: &'static [Method],
    /// A whole path, or a prefix ending in `/`.
    pub path: &'static str,
    /// Piped with `follow`; else one whole answer with `ask`.
    pub stream: bool,
    pub who: Who,
    /// The slice or issue that deletes this line; none: it is in the frozen contract.
    pub until: Option<&'static str>,
}

/// Whose identity a line carries to the API.
pub enum Who {
    /// A browser: the `hd` cookie, hashed into `Drover-Device`. A client's `Authorization` is dropped.
    Device,
    /// An agent: its bearer, hashed into `Drover-Key`. A cookie is dropped.
    Key,
}

pub const RELAYED: &[Relayed] = &[
    Relayed { methods: &[Get],  path: "/screens/",        stream: false, who: Device, until: None },
    Relayed { methods: &[Post], path: "/action",          stream: false, who: Device, until: None },
    Relayed { methods: &[Get],  path: "/stream",          stream: true,  who: Device, until: None },
    Relayed { methods: &[Post], path: "/stream/",         stream: false, who: Device, until: None },
    Relayed { methods: &[Get],  path: "/design/",         stream: false, who: Device, until: None },
    Relayed { methods: &[Get],  path: "/shot/",           stream: false, who: Device, until: None },
    Relayed { methods: &[Post], path: "/api/v1/timings",  stream: false, who: Device, until: None },
    Relayed { methods: &[Get, Post], path: "/api/v1/lifeboat", stream: false, who: Device, until: None },
    Relayed { methods: &[Get],  path: "/thread/",         stream: false, who: Device, until: Some("#1571 D4") },
    Relayed { methods: &[Get, Post, Delete], path: "/mcp", stream: true, who: Key, until: None },
];
```

The last line is #1627's: its impl adds it, `Who::Key` and the line's contract file in one PR. It is the edge's
whole share of MCP. Piped, it carries a JSON answer and an event stream alike, so the edge reads neither.

A `GET /screens/…/stream` is the same first line with a stream answer, until S7. The edge checks Host and Origin
on every relayed request and every POST (a request with no `Origin`, as an agent sends, passes on its Host), and
refuses a non-GET with 405 `READ_ONLY` when it runs read-only. Until S7 a request carrying a bearer on any line
but a `Who::Key` one skips the table and is passed on whole (the plugin pass-through); the `/mcp` line is matched
first. From S7 that bearer is dropped.

`cli/tests/edge.rs` fails when a line with `until: None` has no file in the frozen contract folder.

## The new API routes (`api/src/serve/edge.rs`)

| Route | Who may call | Answer |
|---|---|---|
| `GET /api/v1/hello` | any token | `Hello` |
| `GET /api/v1/layout` | any token | drover's `layout.json`: the manifest's places plus the page's own (Threads, Hosts), in blocks' layout model |
| `GET /api/v1/device` | the edge's token | `Device` for the request's `Drover-Device`; 503 `BUSY` when the database is |
| `POST /api/v1/pair/request` | the edge's token | body `{"label": "iPhone"}`; `Pairing`, or 429 past today's limits (4 open requests, 2 an address) |
| `POST /api/v1/pair/claim` | the edge's token | `Device`; `paired` turns true once `drover serve approve <code>` ran |
| `GET /api/v1/push/key`, `POST /api/v1/push/subscribe` | the edge's token | leaving: today's key and subscribe body; deleted by #1571's D4 |
| `GET /stream`, `POST /stream/<sid>` | any token | the device stream, as the screen-hub ADR's `shapes.md` has it; its "device" is the `Viewer` |
| `/mcp` (reserved; #1627 builds it, in `api/src/serve/mcp.rs`) | an agent's key | #1627's ADR |

`hello` is answered from memory: no database read and no fold, since the edge calls it every heartbeat.

```rust
/// `GET /api/v1/hello`: who answers, and what the page's server needs of fleet.toml.
pub struct Hello {
    /// The API's build.
    pub sha: String,
    pub version: String,
    /// fleet.toml's mtime: `serve` below changed when this moves.
    pub config: u64,
    pub read_only: bool,
    pub serve: ServeFacts,
}

pub struct ServeFacts {
    /// Host names the edge accepts beside its bind address (`[serve] allowed_hosts`).
    pub allowed_hosts: Vec<String>,
    /// The same for the device listener (`[serve] device_hosts`).
    pub device_hosts: Vec<String>,
    pub theme: String,
    /// The line under a pairing code (`[serve] approve_hint`).
    pub approve_hint: String,
    pub heartbeat_secs: u64,
}

/// `GET /api/v1/device`, `POST /api/v1/pair/claim`.
pub struct Device {
    pub paired: bool,
    pub label: Option<String>,
    /// Seconds the cookie should live; set by a claim that succeeded.
    pub max_age: Option<u64>,
}

/// `POST /api/v1/pair/request`.
pub struct Pairing {
    /// The cookie's value. The API stores only its hash.
    pub token: String,
    pub code: String,
    pub hint: String,
    pub max_age: u64,
}
```

Each gets a recorded answer and a `.keys` file in `crates/cli/tests/golden/contract/v1/*.keys` (#1571's folder), except
the two leaving push routes. `pair-request` and `pair-approve` are logged by the API, as today.

## What the launcher hands the edge (`crates/server/src/lib.rs`)

```rust
/// Everything the page's server is started with. It reads nothing else from the host.
pub struct Edge {
    pub listeners: Listeners,
    pub engine: Box<dyn Engine>,
    /// The state folder: the handover's control socket, `serve-starts`, `serve.log`.
    pub state: PathBuf,
    /// `<config dir>/themes`.
    pub themes: PathBuf,
    pub read_only: bool,
    /// This build's sha, for `/health` and the handover's same-build test.
    pub build: String,
    /// The CLI's one writer; the edge prints nothing itself.
    pub log: fn(&str),
    /// Starts the next build with these listeners kept open, and returns its pid.
    pub successor: Box<dyn Fn(&[RawFd]) -> std::io::Result<u32> + Send + Sync>,
}

pub struct Listeners {
    /// The page's listener, then the device listener when `[serve] device_addr` is set.
    pub list: Vec<TcpListener>,
    /// A predecessor passed them down: this start is a handover, not a restart.
    pub inherited: bool,
}

pub fn run(edge: Edge) -> anyhow::Result<()>;
```

From S4 the launcher builds `engine` with `engine::Http::new(addr, token_file)`.

## Config and jobs

| Key | Default | What |
|---|---|---|
| `[serve] api` | `127.0.0.1:7779` | where the API host listens and the edge calls; loopback only (config load refuses another address) |

No `[limits]` key is added. `serve_heartbeat_secs` also paces the edge's `hello`. `serve_mem_mb`,
`serve_host_secs`, `serve_remote_secs` and the stream bounds are read by the API.

| Job | Command, after `drover` | Change |
|---|---|---|
| `dev.drover.api` | `api --addr <[serve] api>` | new in S4; kept alive like serve; installed, revived and restarted with it, and first. The one writable host: it holds `api.lock` and binds `[serve] hooks_addr` |
| `dev.drover.serve` | `serve --addr <[serve] addr>` | the same label and address; now the edge. Binds the page's listener and the device listener, never the webhook one |
| `dev.drover.serve-staging` | the staged build's `api --read-only --addr <staging_addr>` | the same label; the staged build's views on the live state. No lock, no webhook listener, no jobs |
| `dev.drover.service` | `service` | loses `--board`, then its own listener |

## The API host (`api/src/host.rs`)

```rust
/// What a start of the `api` command became.
pub enum Hosting {
    /// Holds `api.lock`; bound the API's address and, when set, the webhook address.
    Writable { lock: File, api: TcpListener, hooks: Option<TcpListener> },
    /// `--read-only`: the API's address alone.
    ReadOnly { api: TcpListener },
}

/// A writable start that found the lock held: printed as `drover api: pid <pid> hosts this state on <addr>`, exit 1.
pub struct Held { pub pid: u32, pub addr: String }
```

`api.lock` is a file in the state folder holding the holder's pid and address, locked for the process's life (the
OS drops the lock when it exits, however it exits). A webhook listener exists only inside `Writable`, so no code
path binds it without the lock.

cooee's manifest from S4b (generated; the places are unchanged):

```toml
attach = "http://127.0.0.1:7779"
attach_token = "~/.config/drover/cooee-token"
min_cooee = "<cooee's attach-only release>"
```
