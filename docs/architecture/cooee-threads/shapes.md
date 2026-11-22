# cooee-threads: the row and action contract (#1571)

The payload types live in `crates/server/src/api.rs` and reach `assets/types/api.d.ts` through `just types`. The row's
data without its block lives in `crates/core/src/thread/` so the terminal reads it with no server.

## Routes

| Route | Who calls | Body or answer |
|---|---|---|
| `GET /api/v1/rows?since=<seq>&n=<1..500>` | cooee | `Rows`: the rows after `since`, oldest first |
| `GET /api/v1/rows?open=1` | cooee, first connect and recovery | `Rows`: every open row, `prompt` and `push` null |
| `GET /api/v1/rows/stream` | cooee | server-sent events, `id:` the seq, `data:` one `Row`; resumes from `Last-Event-ID` |
| `GET /api/v1/thread/<id>/rows?before=<seq>&n=<k>` | cooee's import, drover's surfaces | `Rows` of one thread, `more` for older ones |
| `PUT /api/v1/thread/<id>` | cooee | `ThreadFacts`; an upsert, so open, retitle, archive, restore and move are one call |
| `POST /api/v1/thread/<id>/said` | cooee | `Said`; a repeated `id` is ignored |
| `POST /action` | cooee, drover's page | `{screen, block, action, values, by}` with an `Action`'s values; the same op as `POST /api/v1/<op>` |
| `GET /cooee/profiles` | cooee | one `ChatProfile` per repo: `all`, `none`, each fleet (unchanged shape) |

The plugin token or a paired device reads; only the plugin token may `PUT` a thread or post `said`. Every refusal is
the existing `Reply` (`ok`, `code`, `message`). `cooee-plugin.toml` names the paths in a `[threads]` table, so cooee
core holds no drover path:

```toml
[threads]
rows = "/api/v1/rows"
thread = "/api/v1/thread/{thread}"
said = "/api/v1/thread/{thread}/said"
```

## Rows

```rust
/// `GET /api/v1/rows`: `seq` the newest row's; `more`: ask again from the last row's seq.
pub struct Rows {
    pub seq: u64,
    pub more: bool,
    pub rows: Vec<Row>,
}

/// One thing a thread shows, made from one logged event (`seq`, `ts`).
pub struct Row {
    pub seq: u64,
    pub ts: u64,
    /// The row's identity: a later row with the same key replaces it (a card that settles).
    pub key: String,
    pub to: Target,
    pub kind: RowKind,
    pub fleet: String,
    pub issue: Option<u64>,
    /// One plain line: the list preview, the quote [chat] carries, the transcript.
    pub text: String,
    /// What every surface draws, in the shared blocks schema; its buttons and choices name an `Action`.
    pub block: Value,
    /// It still waits for the owner.
    pub open: bool,
    /// The line the thread's agent is told; cooee types it in, waking the agent. None: shown only.
    pub prompt: Option<String>,
    /// Set: cooee sends this notification. None: quiet.
    pub push: Option<Push>,
    /// A handoff row's item (the existing `Handoff`).
    pub handoff: Option<Handoff>,
}

#[serde(tag = "k", rename_all = "kebab-case")]
pub enum Target {
    /// A thread by id: the one that filed or queued the issue, or the one the row is about.
    Thread { id: String },
    /// The repo's desk thread; cooee resolves it.
    Desk { repo: String },
    /// No thread: a host or fleet alert, a notification only.
    Owner,
}

#[serde(rename_all = "kebab-case")]
pub enum RowKind {
    Ask,
    Decision,
    Handoff,
    RunCard,
    ConfigCard,
    SecretCard,
    System,
    Merged,
    Rollout,
    Digest,
    Alert,
    Said,
}

/// A notification's words; a newer one with the same `tag` replaces the older.
pub struct Push {
    pub title: String,
    pub body: String,
    pub tag: String,
}
```

What each kind is, and what it replaces today:

| `kind` | Logged event | `to` | `prompt` | `push` |
|---|---|---|---|---|
| `ask` | `needs-decision`, `ask-owner` | the ask's thread, else the issue's home | a lane ask's routing line | an owner ask; none for a routine or routed one |
| `decision` | `decision` (the ask's row again, settled; an orchestrator's keeps [change], #1609) | the ask's | `DECISION <id>: <text>` to the asking thread | none; a `by: default` one pushes |
| `handoff` | `thread-handoff` | the target thread, or `desk` (#1610) | the handoff text | none |
| `run-card`, `config-card`, `secret-card` | `run-card`, `config-propose`, `secret-request` and their settle events | the posting thread | the settle line (`exit 0`, applied, `secret X set ✓`) | set while open |
| `system` | a `thread-msg` from `system` (a flow-back line, a refusal note) | its thread | the line, when the agent must act on it | none |
| `merged`, `rollout` | `merged`, an open rollout | the issue's home | none | `merged` as today |
| `digest` | the daily list of orchestrator decisions (#1609) | `desk` | none | set |
| `alert` | an event with `is_push()` and no thread (`blocked`, `stall`, `lane-dead`, `needs-human`, `host-busy`, …) | the issue's home, else `owner` | none | set |
| `said` | an owner message typed on a drover surface | its thread | the message | none |

The issue's home is `thread::route::home`: the thread whose orchestrator filed or queued the issue, when that thread is
open and its profile is not `none`; else `desk` of the fleet. A `none` thread gets only rows about itself (its own
asks, cards and their settle lines). `to` is fixed by the log up to the row's seq, so a replay gives the same target.

Delivery is at least once. Cooee keeps the last seq it applied; a replayed row is harmless for drawing (`key`), and
cooee skips `prompt` and `push` for a seq at or under its cursor.

## Actions

```rust
/// A tap's `action` and `values`, as `POST /action` carries them.
#[serde(tag = "action", content = "values", rename_all = "kebab-case")]
pub enum Action {
    /// An ask, a [change] of an orchestrator's decision inside its window, a pending handoff's pick.
    Answer { fleet: String, id: String, choice: String, note: Option<String> },
    RunCard { id: String, act: CardAct, sha: String },
    ConfigCard { id: String, act: ConfigAct },
    /// `value` only with `save`; drover stores it and never logs it.
    SecretCard { thread: String, name: String, act: SecretAct, value: Option<String> },
    /// #1610: deliver a handoff to another thread; the first copy becomes forwarded.
    HandoffMove { id: u64, to: Target },
}

pub enum CardAct { Approve, Deny }
pub enum ConfigAct { Apply, Discard }
pub enum SecretAct { Save, Dismiss }
```

The first four exist today as manifest actions with these values; only `handoff-move` is new. The answer is `{line}`
on 2xx and the refusal's line on 409, as cooee's action route already expects.

## What cooee writes

```rust
/// `PUT /api/v1/thread/<id>`: the facts drover needs to route to a thread and list it.
pub struct ThreadFacts {
    /// The thread's repo: a fleet, `all` or `none` (the drover profile it started with).
    pub profile: String,
    pub title: String,
    /// `working`, `idle` or `asleep`: the figure on drover's own list.
    pub state: String,
    pub archived: bool,
    /// The repo's desk thread.
    pub desk: bool,
    /// The drover thread this one continues (a move); set once.
    pub from: Option<String>,
}

/// `POST /api/v1/thread/<id>/said`: one chat message, copied for drover's surfaces and for `--typed`.
pub struct Said {
    /// Cooee's item id: posting it twice logs it once.
    pub id: String,
    pub ts: u64,
    /// `owner` or `orchestrator`.
    pub from: String,
    pub text: String,
    /// Names and sizes only; the bytes stay in cooee.
    pub files: Vec<FileRef>,
}
```

Cooee does not post back a message that reached it as a `said` row. A thread id is opaque to drover (cooee's
`1..64` of `A-Za-z0-9-_`); drover never parses one.

## The log

No new store: a row is a pure function of the event log, as the Events feed is today.

| Event | Logged by | Replaces |
|---|---|---|
| `thread-facts` (`thread`, the `ThreadFacts` fields) | `PUT thread/<id>` | `thread-new`, `thread-title`, `thread-archive`, `thread-restore`, `orchestrator-attached`, `orchestrator-detached` |
| `thread-msg` with `item` (cooee's id) | `POST …/said` | the same event written by `say` and `thread reply` |
| `thread-msg` with `via: "drover"` | `thread::say` | the same, delivered by drover |

Every event an orchestrator's command logs carries `thread` (from `DROVER_THREAD`) beside today's `orchestrator`
stamp; `home` reads it. Old events stay readable: a `thread-new` thread is a drover-hosted one, an
`orchestrator-attached` one is a cooee thread opened before this contract.

## `[limits]`

| Key | Default | What it times |
|---|---|---|
| `rows_stale_minutes` | 10 | a row with `push` older than this past the reader's cursor: `check` logs `rows-stale` once, the board shows `cooee is not reading rows`, and `[[notify]]` commands get it |
