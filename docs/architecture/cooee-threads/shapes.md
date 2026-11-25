# cooee-threads: the row, action, manifest and judge contracts (#1571)

The payload types live in `crates/server/src/api.rs` and reach `assets/types/api.d.ts` through `just types`. The row's
data without its block lives in `core::thread` (`crates/core/src/thread.rs`, D1), so the terminal board reads it with
no server. "A client" is any program attached through the manifest: cooee, or a terminal session.

## Routes

| Route | Who calls | Body or answer | Slice |
|---|---|---|---|
| `GET /api/v1/manifest` | a client, at attach | `Manifest` | D5 |
| `GET /api/v1/rows?since=<seq>&n=<1..500>` | a client | `Rows`: the rows after `since`, oldest first | D1, merged |
| `GET /api/v1/rows?open=1` | a client, first connect and recovery; drover's board | `Rows`: every open row, `prompt` and `push` null | D1, merged |
| `GET /api/v1/rows/stream` | a client | server-sent events, `id:` the seq, `data:` one `Row`; resumes from `Last-Event-ID` | D1, merged |
| `GET /api/v1/thread/<id>/rows?before=<seq>&n=<k>` | a client | `Rows` of one chat or old drover thread, `more` for older ones | D1, merged |
| `POST /api/v1/orchestrator-attach` | a client, the CLI attach verb | `{fleet, thread}`: a repo, `all` or `none` (D2); answers the brief, settings and env | today, D2, D3 |
| `POST /api/v1/orchestrator-detach` | a client | `{fleet, thread}`: the chat takes no more routed rows | today |
| `POST /api/v1/thread/<id>/said` | a client | `Said`; a repeated `id` is ignored | D2 |
| `POST /action` | a client, drover's page | `{screen, block, action, values, by}` with an `Action`'s values | today, plus #1610 |
| `GET /cooee/profiles` | a client | one `ChatProfile` per orchestrator seat (#1057, unchanged) | today |

D4 removes `GET /api/v1/chat`, `POST /api/v1/chat` and the `chat-about` op: they serve drover's thread tab and the
thread a lane card opened, and no `.keys` file names them. The plugin token or a paired device reads; only the plugin
token may attach or post `said`. Every refusal is the existing `Reply` (`ok`, `code`, `message`).

## The manifest (D5)

```rust
/// `GET /api/v1/manifest`: what a client needs to attach a plugin, with no path of its own.
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// The base URL the client calls; every path below is relative to it.
    pub api: String,
    /// The existing places: header strips, the cards place, the menu row, each with its screen path.
    pub screens: Vec<Place>,
    /// Every action name `POST /action` takes, and which of them are destructive.
    pub actions: Vec<String>,
    pub destructive: Vec<String>,
    /// Where a client reads profiles, and attaches, follows and writes a chat.
    pub threads: Threads,
}

pub struct Threads {
    pub profiles: String,
    pub attach: String,
    pub detach: String,
    pub rows: String,
    pub stream: String,
    pub thread: String,
    pub said: String,
}
```

`cooee-plugin.toml` is generated from it by `just gen` and listed `generated`, so the hand-kept copy goes.

## Rows

```rust
/// `GET /api/v1/rows`: `seq` the newest row's; `more`: ask again from the last row's seq.
pub struct Rows {
    pub seq: u64,
    pub more: bool,
    pub rows: Vec<Row>,
}

/// One thing a chat or the board shows, made from one logged event (`seq`, `ts`).
pub struct Row {
    pub seq: u64,
    pub ts: u64,
    /// The row's identity: a later row with the same key replaces it (a card that settles).
    pub key: String,
    pub to: Target,
    pub kind: RowKind,
    pub fleet: String,
    pub issue: Option<u64>,
    /// One plain line: the list preview, the transcript.
    pub text: String,
    /// What every surface draws, in the shared blocks schema; its buttons and choices name an `Action`.
    pub block: Value,
    /// It still waits for the owner.
    pub open: bool,
    /// The line the chat's agent is told; the client types it in. None: shown only.
    pub prompt: Option<String>,
    /// Set: a client sends this notification to its devices (always set from D2). None: quiet.
    pub push: Option<Push>,
    /// A handoff row's item (the existing `Handoff`).
    pub handoff: Option<Handoff>,
}

#[serde(tag = "k", rename_all = "kebab-case")]
pub enum Target {
    /// A chat by id: the one that filed or queued the issue, or the one the row is about.
    Thread { id: String },
    /// The repo's owner cards: drawn on drover's board, TUI and page and in a client's cards place. From D4 no thread.
    Desk { repo: String },
    /// No repo: a host alert, a notification only.
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

Every name and variant above is in contract v1 today and stays; `Desk` keeps its key and changes only what a drover
surface does with it.

| `kind` | Logged event | `to` | `prompt` | `push` |
|---|---|---|---|---|
| `ask` | `needs-decision`, `ask-owner` | the asking chat, else the issue's home | for a chat: the ask's line | an owner ask, or one a judge filed to the owner; none while a judge holds it |
| `decision` | `decision` (the ask's row again, settled; a judge's keeps [change]) | the ask's | `DECISION <id>: <text>` to the asking chat | none; a `by: default` one pushes |
| `handoff` | `thread-handoff` | the target chat, or `desk` (#1610) | the handoff text | none |
| `run-card`, `config-card`, `secret-card` | `run-card`, `config-propose`, `secret-request` and their settle events | the posting chat | the settle line | set while open |
| `system` | a `thread-msg` from `system` (a flow-back line, a refusal note, a judge's note, D3's closing row) | its chat | the line, when the agent must act on it | none |
| `merged`, `rollout` | `merged`, an open rollout | the issue's home | none | `merged` as today |
| `digest` | the daily list of judge decisions (#1609) | `desk` | none | set |
| `alert` | an event with `is_push()` and no chat (`blocked`, `stall`, `lane-dead`, `needs-human`, `host-busy`, …) | the issue's home, else `owner` | none | set |
| `said` | an owner line copied from a chat (D2) | its chat | none | none |

The issue's home is `thread::route::home` (D1): the attached chat that filed or queued the issue, unless its repo is
`none`; else `desk` of the fleet. "Attached" is `orchestrator-attached` not followed by `orchestrator-detached`; from
D4 a `thread-new` counts only as history. `to` is fixed by the log up to the row's seq, so a replay gives the same
target. Delivery is at least once: a client keeps the last seq it applied, redraws by `key`, and skips `prompt` and
`push` at or under its cursor.

## Actions

```rust
/// A tap's `action` and `values`, as `POST /action` carries them.
#[serde(tag = "action", content = "values", rename_all = "kebab-case")]
pub enum Action {
    /// An ask, a needs-human card's choice, a [change] of a judge's decision inside its window.
    Answer { fleet: String, id: String, choice: String, note: Option<String> },
    RunCard { id: String, act: CardAct, sha: String },
    ConfigCard { id: String, act: ConfigAct },
    /// `value` only with `save`; drover stores it and never logs it.
    SecretCard { thread: String, name: String, act: SecretAct, value: Option<String> },
    /// #1610, [move] on a handoff: re-target it to an attached chat; the first copy becomes forwarded.
    HandoffMove { id: u64, to: Target },
}

pub enum CardAct { Approve, Deny }
pub enum ConfigAct { Apply, Discard }
pub enum SecretAct { Save, Dismiss }
```

The first four exist today. `handoff-move` is new in #1610. The answer is `{line}` on 2xx and the refusal's line on
409. An ask's block lists its options recommended first, then a one-line note field that fills `note`.

## What a client writes

```rust
/// `POST /api/v1/thread/<id>/said` (D2): one owner line of an attached chat, copied for `answer --typed`.
pub struct Said {
    /// The client's item id: posting it twice logs it once.
    pub id: String,
    pub ts: u64,
    /// `owner`; the agent's own lines stay in the client.
    pub from: String,
    pub text: String,
}
```

A thread id is opaque to drover (`1..64` of `A-Za-z0-9-_`). A `said` for a thread that is not an attached chat is
refused `NO_SUCH_THREAD`.

## The judge (#1609)

```rust
/// What drive spawns a judge lane for; one judge per value, ever.
pub enum Judged {
    /// An open lane ask (`needs-decision` with `lane_ask`), by its id.
    Ask { fleet: String, id: String },
    /// A needs-human card, by the seq of its `needs-human` event.
    Card { fleet: String, issue: u64, seq: u64 },
}
```

| Event | Logged by | Data |
|---|---|---|
| `spawn` with `role: judge` and `judged` | drive | the `Judged` it was spawned for |
| `decision` with `by: judge` | the judge's `drover answer --basis` | as any decision; quiet, [change] for the window |
| `ask-owner` with `why` | the judge's `drover answer` with no basis (`OWNER_ASK`) | its pick as `recommend`; no `thread` |
| `judge-failed` | drive or check | `judged`, `why`: dead, timed out, refused; the ask is filed to the owner |

## The log

No new store: a row is a pure function of the event log.

| Event | Logged by | Slice |
|---|---|---|
| `orchestrator-attached`, `orchestrator-detached` | a client's attach and detach | today; D2 adds `all`, `none`; D3 adopts an archived thread id |
| `thread-msg` with `item` and `via: "client"` | `POST …/said` | D2 |
| `thread-handoff` with `moved` | the `handoff-move` action | #1610 |
| `thread-msg` from `system` with `closing: true`, then `thread-archive` with `by: migration` | D3's closing row, D4's stop | D3, D4 |

Every event an orchestrator's command logs carries `thread` (from `DROVER_THREAD`, D1); `home` reads it. An issue
filed before D1, or by a thread nobody adopted, goes to `desk`.
