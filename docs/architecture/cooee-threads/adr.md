# ADR: cooee's Orchestrator is a drover client; drover keeps its threads (#1571)

## Summary

Decision: drover keeps its threads, their engine, its page, `drover chat` and its push, and serves every workflow
row and action in `/api/v1`. Cooee runs its own Orchestrator agent on drover's profile as a drover client: a cooee
chat attaches with a thread id, so the asks and cards it raises come back to it as drover blocks. There are two run
paths, and drover never calls cooee.
Slices: D1 (#1617) is merged. Next come four drover slices (#1609, D2, D3, #1610) and #1608, then two cooee ones
(C1, cooee #802), in a fixed order. #1598 closes with D2 and #1610 with its own slice.
Open questions: none.

Status: revised after the owner's 2026-11-24 scope change. Read [README.md](README.md) first, then this,
[shapes.md](shapes.md), [boundaries.md](boundaries.md), [owners.md](owners.md).

## Question

The owner works in cooee chat, and cooee already has orchestrator agents with drover attached. Yet a lane ask, a
`DECISION` line or a handoff never reaches a cooee chat, and cooee draws drover's cards as one deck under every chat.
How does a cooee Orchestrator chat get its own workflow rows, with cooee only ever calling drover, the contract typed
and frozen, and drover still a whole product on its own?

## Where it stands (drover staging `fd81ef5c3`)

- D1 (#1617) is merged. `core::thread` makes rows from the log, and `thread::route::home` picks a row's target.
  `GET /api/v1/rows`, its stream and `thread/<id>/rows` serve the rows. The contract folder
  `crates/cli/tests/golden/contract/v1/`, `contract_v1.rs` and `append_only` freeze their keys. `docs/rows.md`
  documents them.
- Drover types into a thread's agent through `orchestrator::deliver`, which starts or resumes it in shed first: the
  owner's message, a lane ask's routing line, a `DECISION` line, a handoff, a card's settle line, and `continue`
  after a usage limit.
- Cooee attaches a chat with `POST /api/v1/orchestrator-attach {fleet, thread}`. It gets back the brief, the hook
  settings and the lane env with `DROVER_THREAD`, so its agent's events carry the thread. D1's registry already
  counts such a chat as an open thread. But `deliver` treats it like a drover thread: it starts a session of the same
  name in shed, which cooee already runs. That is #1598's `NAME_TAKEN`.
- A row's `push` is null while fleet.toml has a web push notifier. Cooee draws drover's cards from the `chat-cards`
  place, the same deck under every chat.
- #1645 splits serve into its own process, an `/api/v1` client. #1663 moves cooee's attach onto that API.

## Options

**Who runs a cooee chat's Orchestrator agent**

1. Drover runs it, and cooee is a window onto drover's thread. One run path, but cooee's chat becomes a relay of a
   drover session. The owner declined this.
2. **Cooee runs it on drover's profile, as a drover client (chosen).** Two run paths: drover runs its own threads,
   cooee runs its chats. Each side types only into the sessions it started.

**How a workflow line reaches a cooee chat's agent**

1. Drover types through shed into cooee's session. Two writers on one session, and it is #1598.
2. Drover calls cooee. The wrong direction.
3. **Cooee reads rows from drover, and a row may carry a `prompt` (chosen).** It is D1's row. Cooee keeps one cursor.

**What a row carries**

1. Typed data only, with cooee mapping each kind to a block. Every new card would need a cooee change.
2. **The block drover already builds, beside a small typed envelope (chosen, D1).** A new card is a drover change
   only.

**Where [chat] and [move] live**

1. Cooee buttons. They work in cooee only, and cooee would need to know handoffs and owner cards.
2. **Drover blocks with drover actions (chosen).** They work on drover's page and in cooee alike.

**Push**

1. One sender, cooee. Drover's page would lose its alerts.
2. **Both, from the same rows; a device subscribes to one (chosen).** Drover's web push serves drover's page. Cooee
   sends a row's `push` to its own devices.

## Decision

- **Two run paths.** A thread is either a drover thread (`thread-new`: drover runs its agent, as today) or a cooee
  chat (`orchestrator-attached`: cooee runs its agent). A thread keeps its host for life.
- **Cooee's Orchestrator.** It is one cooee plugin agent. Cooee's Chief hands off to it (cooee #802), and [New] picks
  its repo: a fleet, `all` or `none`. Cooee attaches the chat with that repo as `fleet` and starts the agent with the
  brief, settings and env the attach returns (#1057's profile). The agent runs drover's CLI, so its asks, cards and
  queued issues carry its thread.
- **Rows.** D1's `Row` is the contract ([shapes.md](shapes.md)). A row whose `to` is a cooee chat carries `prompt`.
  Drover delivers nothing into a cooee chat; cooee types the `prompt` in. A row whose `to` is a drover thread is
  delivered by drover, as today.
- **Routing.** `home(fleet, issue)` is the open thread that filed or queued the issue, unless its repo is `none`, else
  the repo's desk (D1). Rows about a thread itself (its asks, its cards) go to that thread.
- **The desk.** Each repo has one desk thread, a drover thread that D3 makes when a row first needs it. Until then a
  `to: desk` row is an owner card as today: open, pushed, on the board.
- **Actions.** `answer`, `run-card`, `config-card`, `secret-card`, `chat-about` ([chat]) and `handoff-move` ([move])
  go through `POST /action`, each the same op as `POST /api/v1/<op>`.
- **What cooee writes.** It writes `orchestrator-attach` and `-detach`, already served, which D2 lets take `all` and
  `none`. It also writes `POST /api/v1/thread/<id>/said`, a copy of one owner line, so `answer --typed` works from a
  cooee chat.
- **Push.** From D2 a row's `push` is always set. Drover's web push still sends from events to devices subscribed on
  its page, and cooee sends `push` to devices subscribed in cooee.
- **Drover's surfaces.** Drover's page, `drover chat`, `thread show|ls|say` and the board stay as they are. They show
  drover's threads and every open card wherever it routed.
- **History.** None moves. Drover's threads keep their history in drover's log. A cooee chat starts with its attach
  and reads its own rows through `thread/<id>/rows`.

## Consequences

- One direction holds by construction: drover has no cooee address, client or token, and the `/plugin/` ban holds it.
- Lane asks, `DECISION` lines and handoffs reach a cooee chat, which they do not today. #1598 goes, because drover no
  longer starts a session in a chat cooee runs.
- Two engines run orchestrator agents. That is the owner's call: drover stays standalone, and cooee's chat stays
  cooee's.
- With cooee down, a cooee chat's rows wait in the log, and its open cards still show on drover's board and page.
  Drover's own threads are unaffected.
- A cooee chat's messages live in cooee. Drover holds only the owner lines cooee copies through `said`, for
  `--typed`, and does not list a cooee chat's conversation.
- `all` and `none` become reserved fleet names, refused at config load (D2).
- Nothing is deleted from drover but `land`, the handoff pick that #1610 replaces with the desk and [move]
  ([owners.md](owners.md)).

## The contract test

Drover (D1, merged):

- `crates/cli/tests/golden/contract/v1/` holds `<name>.json` (a recorded answer) and `<name>.keys` (its flattened
  shape, one sorted line each) per route and row kind.
- `crates/cli/tests/contract_v1.rs` drives the real server and fails on any difference. `BLESS=1` adds new lines only.
- `drover arch-check`'s `append_only` fails a PR that removes or changes a line of a `.keys` file. A key, a kind or an
  action can be added and never removed or retyped; that takes `/api/v2`.
- D2, D3 and #1610 each add the `.keys` of what they add (`said`, `chat-about`, `handoff-move`).

Cooee (C1):

- `<cooee>/crates/cooee/tests/fixtures/drover-contract/` is a copy of that folder at the drover commit cooee's plugin
  index pins. `just drover-contract` writes it and it is listed `generated`, so it is never edited by hand.
- `<cooee>/crates/cooee/tests/drover_contract.rs` parses every sample with cooee's own row, target and action types,
  and fails on a key it reads that is missing. A drover bump that would break cooee fails in cooee's CI at the bump.

## The issues to file, in order

| # | Repo | Issue | Needs | Closes |
|---|---|---|---|---|
| D1 | drover | #1617, merged: rows from the log, `thread::route`, the rows routes, contract v1 and its test | this ADR | |
| #1609 | drover | cards to the orchestrator first, by thread id, else the desk (an owner card until the desk exists); judgment; quiet decisions; the digest | D1 | #1638 |
| D2 | drover | cooee chats as clients: `deliver` skips a chat cooee attached, so its row's `prompt` is the only way in; attach takes `all` and `none`, both reserved fleet names; `POST thread/<id>/said` and `--typed` reading it; `push` always set; `[threads]` in `cooee-plugin.toml`; the `.keys` of `said` | D1 | #1598 |
| D3 | drover | the desk: one drover thread per repo, made on a row's first need, logged `thread-facts` with `desk`; [chat] on owner cards as the `chat-about` action, quoting the card into its thread | D2 | |
| #1610 | drover | handoffs go to the desk at once, with no pick; [move] as the `handoff-move` action | D3 | #1610 |
| #1608 | drover | cap raises within 5% are the orchestrator's, a quiet row | #1609 | |
| C1 | cooee | read a plugin's rows: the `[threads]` table, the follower and its cursor, draw by key in an attached chat or the drover cards place, type `prompt`, send `push`, post actions; post `said`; the contract copy and its test | D2 | |
| cooee #802 | cooee | Chief's hand-off to the Orchestrator plugin agent; [New] picks a repo, `all` or `none`, and attaches; the repo filter | C1 | |

#1645 and #1663 stand. They move the same routes into serve's own process and change no shape, so the contract test
covers them. #1609 lands before D2: it reads only D1's rows and `home`.

## The rules lines

Both files are at their 30-line limit, so each line widens one.

Drover's `.drover/rules.md`, line 13 (D1's contract line), widened by D2:

```
- don't let assets/types/ drift from `just types`, hand-copy a generated type, remove or retype a key of crates/cli/tests/golden/contract/v1/, call cooee, or type into a thread cooee runs: a breaking change is /api/v2, and a line for a cooee chat is a row's prompt (enforced by: CI TS types, `just types-lint`, CI arch-check `append_only` and the `/plugin/` ban, crates/cli/tests/contract_v1.rs, D2's test that a lane ask to an attached chat starts no shed session)
```

Cooee's `.drover/rules.md`. C1 replaces its last line, "Don't put workflow logic … (enforced by: review)", with:

```
- Don't put workflow logic (issues, queues, lanes, reviews, merges, where a card goes, when to notify) in cooee, or name a drover route or event kind outside the plugin manifest: draw a row's block, type its prompt, send its push, post its action (enforced by: CI arch-check `api/v1` and `needs-decision` bans (.drover/arch/boundaries.toml), crates/cooee/tests/drover_contract.rs)
```

Cooee's `.drover/arch/boundaries.toml` gains two bans. The one existing call, the machines screen's host tags, is
excepted:

```toml
[[ban]]
pattern = "api/v1"
paths = ["crates/*/src/**"]
except = ["crates/cooee-server/src/screens/settings.rs"]
reason = "drover#1571: cooee reads a plugin's rows at the paths its manifest names; it holds no drover route"

[[ban]]
pattern = "needs-decision"
paths = ["crates/*/src/**"]
except = []
reason = "drover#1571: cooee draws a row's block and never reads a workflow event kind"
```

## `.drover/arch.toml`

D1 added both lines this design needs (`append_only` on the contract keys, the `/plugin/` ban). This revision adds
none: nothing is deleted, so the earlier `webpush`, `term::` and `shed::` bans are dropped.

## Hot files

| File | Here |
|---|---|
| `crates/core/src/orchestrator.rs` | D2 adds one guard in `deliver`, and D3 a desk maker; everything else stays |
| `crates/server/src/screens.rs` | no new arm: thread actions go through `serve/rows.rs`'s table (D1) |
| `cooee-plugin.toml` | D2 adds the `[threads]` table and `min_cooee`, after C1's reader is released |
| `.drover/rules.md` | one line widened, none added |
| `docs/rows.md` | the contract's page: D2, D3 and #1610 each add their routes and actions |
| `assets/types/api.d.ts` | `just types` |
