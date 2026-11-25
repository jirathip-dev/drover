# cooee-threads: the row and action contract (#1571)

The payload types live in `crates/server/src/api.rs` and reach `assets/types/api.d.ts` through `just types`. The row's
data without its block lives in `core::thread` (`crates/core/src/thread.rs`, D1) so the terminal reads it
with no server.

## Routes

| Route | Who calls | Body or answer | Slice |
|---|---|---|---|
| `GET /api/v1/rows?since=<seq>&n=<1..500>` | cooee | `Rows`: the rows after `since`, oldest first | D1, merged |
| `GET /api/v1/rows?open=1` | cooee, first connect and recovery | `Rows`: every open row, `prompt` and `push` null | D1, merged |
| `GET /api/v1/rows/stream` | cooee | server-sent events, `id:` the seq, `data:` one `Row`; resumes from `Last-Event-ID` | D1, merged |
| `GET /api/v1/thread/<id>/rows?before=<seq>&n=<k>` | cooee, drover's surfaces | `Rows` of one thread, `more` for older ones | D1, merged |
| `POST /api/v1/orchestrator-attach` | cooee | `{fleet, thread}`: `fleet` is the repo picked on [New] (`all`, `none` from D2); answers the brief, settings and env | today |
| `POST /api/v1/orchestrator-detach` | cooee | `{fleet, thread}`: the chat takes no more routed rows | today |
| `POST /api/v1/thread/<id>/said` | cooee | `Said`; a repeated `id` is ignored | D2 |
| `POST /action` | cooee, drover's page | `{screen, block, action, values, by}` with an `Action`'s values; the same op as `POST /api/v1/<op>` | today, plus D3, #1610 |
| `GET /cooee/profiles` | cooee | one `ChatProfile` per orchestrator seat (#1057, unchanged) | today |

The plugin token or a paired device reads; only the plugin token may attach or post `said`. Every refusal is the
existing `Reply` (`ok`, `code`, `message`). `cooee-plugin.toml` names the row paths in a `[threads]` table (D2), so
cooee core holds no drover path:

```toml
[threads]
rows = "/api/v1/rows"
thread = "/api/v1/thread/{thread}/rows"
said = "/api/v1/thread/{thread}/said"
attach = "/api/v1/orchestrator-attach"
detach = "/api/v1/orchestrator-detach"
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
    /// Set: cooee sends this notification to its devices (from D2, also while drover's web push sends to its own). None: quiet.
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
| `said` | an owner message typed on a drover surface, or copied from a cooee chat (D2) | its thread | the message, for a drover one | none |

The issue's home is `thread::route::home` (D1): the open thread that filed or queued the issue, unless its repo is
`none`; else `desk` of the fleet. "Open" covers both hosts: a drover thread (`thread-new`, not archived) and a cooee
chat (`orchestrator-attached`, not detached). While the repo has no desk thread (D3 makes it), a `desk` row is an owner
card as today: `open`, pushed, no `prompt`. A `none` thread gets only rows about itself. `to` is fixed by the log up
to the row's seq, so a replay gives the same target.

Who acts on `prompt` depends on the host of `to`. For a drover thread, drover delivers it, as today. For a cooee chat,
drover delivers nothing (D2) and cooee types it in. For a desk, drover delivers it, since a desk is a drover thread.

Delivery to cooee is at least once. Cooee keeps the last seq it applied; a replayed row is harmless for drawing
(`key`), and cooee skips `prompt` and `push` for a seq at or under its cursor.

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
    /// D3, [chat] on an owner card: the card's `text` quoted as an owner line in its thread (its home, else the desk).
    ChatAbout { key: String },
    /// #1610, [move] on a handoff: deliver it to another thread; the first copy becomes forwarded.
    HandoffMove { id: u64, to: Target },
}

pub enum CardAct { Approve, Deny }
pub enum ConfigAct { Apply, Discard }
pub enum SecretAct { Save, Dismiss }
```

The first four exist today as manifest actions with these values. `chat-about` exists as a v1 op and D3 makes it a
row action. `handoff-move` is new in #1610. The answer is `{line}` on 2xx and the refusal's line on 409, as cooee's
action route already expects. [chat] quoting into a cooee chat is a `said` row with a `prompt`, which cooee types in;
quoting into a drover thread is `thread::say`, delivered by drover.

## What cooee writes

```rust
/// `POST /api/v1/thread/<id>/said` (D2): one owner line of an attached chat, copied for `answer --typed`.
pub struct Said {
    /// Cooee's item id: posting it twice logs it once.
    pub id: String,
    pub ts: u64,
    /// `owner`; the agent's own lines stay in cooee.
    pub from: String,
    pub text: String,
}
```

Cooee does not post back a line that reached it as a `said` row. A thread id is opaque to drover (cooee's `1..64` of
`A-Za-z0-9-_`); drover never parses one. A `said` for a thread that is not an attached chat is refused
`NO_SUCH_THREAD`.

## The log

No new store: a row is a pure function of the event log, as the Events feed is.

| Event | Logged by | Slice |
|---|---|---|
| `orchestrator-attached`, `orchestrator-detached` | cooee's attach and detach; open and close a cooee chat in the registry | today |
| `thread-msg` with `item` (cooee's id) and `via: "cooee"` | `POST …/said` | D2 |
| `thread-facts` with `desk: true` | drover, making a repo's desk thread | D3 |
| `thread-handoff` with `moved` | the `handoff-move` action | #1610 |

Every event an orchestrator's command logs carries `thread` (from `DROVER_THREAD`, D1), whichever host runs the agent;
`home` reads it. An issue filed before D1 has no stamp and goes to the desk.
