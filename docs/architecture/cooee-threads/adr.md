# ADR: cooee owns thread chat, drover serves the rows and actions (#1571)

## Summary

Decision: drover serves a list of thread rows (asks, cards, handoffs, system lines, merges, alerts), each with what to
draw, the line to tell the agent and whether to notify. Cooee shows them in the thread or the repo's desk thread, runs
the agent and sends all notifications; it copies message text to drover, so drover's page and terminal still work.
Slices: 4 new drover issues around #1609, #1610 and #1608, and 6 cooee issues, in a fixed order.
Open questions: (1) at the cut drover's own chat loses queue, steer, stop, model picks, uploads, new and archive:
accept, relay or keep its engine? (2) keep the scene device listener the amendment names? (3) drop extra seats?

Status: proposed (architect lane, 2026-11-22). Read [README.md](README.md) first, then this, [shapes.md](shapes.md),
[boundaries.md](boundaries.md), [owners.md](owners.md).

## Question

Chat is cooee's job, yet drover holds a whole thread engine: it creates threads, stores messages and uploads, starts
and resumes the orchestrator's agent, types lines into it, picks its model and sends web push. How does cooee take
all of that while drover keeps the workflow, with cooee only ever calling drover, the contract typed and frozen, and
drover's own page, terminal and CLI still working?

## Where it stands (drover staging `d989c5beb`, cooee staging `877cd75c8`)

- Drover types into the agent in six places: the owner's message, a lane ask's routing line, a `DECISION` line, a
  handoff's text, a card's settle line, and `continue` after a usage limit. All go through `orchestrator::deliver`,
  which starts or resumes the agent in shed first.
- Cooee already runs orchestrator chats: drover serves one profile per orchestrator (`GET /cooee/profiles`: brief,
  hook settings, lane env) and cooee reports `POST /cooee/thread-started`. Lane asks are not routed to those chats,
  because nothing there can type a line into the agent, and no `DECISION` comes back.
- Cooee draws drover's cards from the `chat-cards` place: one stream of `/screens/cards`, the same cards under every
  chat of the persona, with no thread in the request. Taps come back through `POST /action`.
- Cooee can follow a plugin's server-sent stream with `Last-Event-ID` and reads one for screens today. It has no route
  by which a plugin could wake an agent. It does have plugin-to-cooee routes (`/plugin/card`, `/plugin/line`,
  `/plugin/push`); drover calls none of them, and under this issue never will.
- A cooee thread has an id, a profile fixed for its life, a title, a model and an effort, and no repo. Its agent is a
  shed session cooee names. Several agents may be orchestrators today.
- `GET /api/v1/events` already serves the log as typed rows for the Events tab, and
  `crates/cli/tests/api_v1_contract.rs` already fails on a shape change, but `BLESS=1` rewrites any change, a removal
  included.
- Drover's phone alerts are `notify()` in `events.rs` calling `webpush::send` for events with `is_push()`.

## Options

**How a workflow line reaches the agent**

1. Drover keeps typing through shed into the session cooee started. Two writers on one session, and drover cannot
   start one that sleeps.
2. Drover calls cooee (`/plugin/line` and a new wake route). The wrong direction.
3. **Cooee reads rows from drover; a row may carry a `prompt` (chosen).** One writer per session, one direction, and
   the six deliveries become one field.

**What a row carries**

1. Typed data only; cooee maps each kind to a block. Every new card (#1609's quiet rows, the digest, #1608's cap
   rows) needs a cooee change, and the mapping is workflow knowledge in cooee.
2. **The block drover already builds, in the shared blocks schema, beside a small typed envelope (chosen).** Cooee
   draws it with the renderer it has; a new card is a drover change only. The envelope has just what cooee must act
   on: where it goes, its key, the prompt, the push, whether it is open.

**How cooee learns of rows**

1. The `chat-cards` stream. It is a whole screen per persona, has no thread and no history, and cannot carry a prompt.
2. Polling `GET events` and deriving rows in cooee. Cooee would fold the log, which is engine state.
3. **A rows route with a cursor, and its stream (chosen).** A pure function of the log, so there is no new store;
   cooee keeps one number.

**Drover's own surfaces after the engine goes**

1. Workflow rows only, with a link to cooee. The conversation disappears from drover, and `--typed` cannot check the
   owner's line.
2. A full relay: every composer control becomes a command row that cooee runs, and files are copied both ways. About
   nine more contract entries for a second chat client.
3. Drover reads the agent's session through shed. Works only while the session is awake, and depends on how cooee
   names and keeps sessions.
4. **Cooee copies each message's text into drover, and drover's `say` is a row with a prompt (chosen).** Two small
   writes keep the list, the conversation, send, every card and `--typed`. What is the engine's goes (open question 1).
5. Keep drover's engine for threads drover hosts. Nothing is cut and nothing is deleted.

**Where a thread's repo lives**

1. A new repo field on cooee's thread, sent to drover. A drover concept in cooee's store.
2. **The profile is the repo (chosen).** Drover serves one profile per repo (`all`, `none`, each fleet); cooee's
   [New] picks a profile, its filter is by profile, and the profile already reaches drover.

## Decision

- **Rows.** `GET /api/v1/rows` (and `/stream`, and `thread/<id>/rows`) serves `Row`s: `key`, `to` (a thread id, a
  repo's desk, or the owner), `kind`, `text`, `block`, `open`, `prompt`, `push` ([shapes.md](shapes.md)). A row is
  built from one logged event; a settled card is the same key again.
- **Actions.** `answer`, `run-card`, `config-card`, `secret-card` and `handoff-move`, through `POST /action`, each the
  same op `POST /api/v1/<op>` runs for a paired device.
- **What cooee writes.** `PUT /api/v1/thread/<id>` (profile, title, state, archived, desk, the drover thread it
  continues) and `POST /api/v1/thread/<id>/said` (one message's text, file names and sizes).
- **Drover never delivers.** Every workflow verb emits a row through `core::thread::route`; a line for the agent is the
  row's `prompt`. Drover starts, resumes, stops and types into no thread agent.
- **Routing.** `home(fleet, issue)`: the open thread whose orchestrator filed or queued the issue, unless its profile
  is `none`; else the repo's desk. Rows about a thread itself (its own asks and cards) go to that thread.
- **Role, brief, permissions.** Stay drover's, served by `GET /cooee/profiles`, one profile per repo. The thread brief
  goes; the chat brief gains the handoff and `--typed` lines.
- **Push.** A row's `push` is the notification. While fleet.toml has a web push notifier drover sends and `push` is
  null; the owner's config card that removes it switches to cooee. `[[notify]]` command notifiers stay: they are how
  drover hands an event to a script, and how `rows-stale` is heard when cooee is down.
- **Drover's surfaces.** The page, `drover chat`, `thread show|ls|say` and the board read `core::thread`, the same
  rows the routes serve. `chat`, `thread/<t>` and `repo/<fleet>/threads` keep their shapes for the page.
- **Migration.** Five running states, then the cut ([boundaries.md](boundaries.md)). History is imported by cooee from
  `thread/<id>/rows`; the old thread turns read-only and its open rows follow the new id.

## Consequences

- One direction holds by construction: drover has no cooee address, client or token, and a ban keeps `/plugin/` out.
- With cooee down, rows wait in the log. Nothing is typed or pushed until it reads again; the board and the command
  notifiers say so after `rows_stale_minutes`, and the lifeboat still answers asks and run cards.
- A message's text is in two places. Cooee's is the truth; drover's copy is for its surfaces and `--typed`. File bytes
  are only in cooee, so drover shows a chat file's name and size.
- Lane asks, `DECISION` lines and handoffs reach a cooee chat, which they do not today.
- A cooee chat's cards appear once, in their thread, not under every chat. C1 hides the `chat-cards` deck for a plugin
  that has a `[threads]` table; D4 removes the place.
- `all` and `none` become reserved fleet names (refused at config load).
- A push reads as drover's plain event line; the per-phone theme words go with drover's sender.
- Older cooee builds refuse a manifest with an unknown table, so `[threads]` enters `cooee-plugin.toml` only in D2,
  after C1 is released, with `min_cooee` raised.
- About 1,450 lines leave drover and about 350 are added ([owners.md](owners.md)). The rendering stays because the
  surfaces stay.
- #1567 lists `orchestrator.rs` `start()` as a caller of shed's account parameter. After D4 that caller is gone;
  the account of an orchestrator chat is cooee's.

## The contract test

Drover, new in D1:

- `crates/cli/tests/golden/contract/v1/`: one `<name>.json` (a recorded answer or body) and one `<name>.keys` per route,
  row kind and action. A `.keys` file is the flattened shape, one sorted line each: `rows[].to.k = "desk"`,
  `rows[].push.tag: string`.
- `crates/cli/tests/contract_v1.rs` drives the real server, flattens each answer and compares it with the `.keys`
  file. A difference fails. `BLESS=1` writes added lines only.
- `drover arch-check` gains `append_only`: a PR that removes or changes a line of a listed file fails. So a key, a
  kind or an action can be added by a lane and never removed or retyped; that takes `/api/v2`.

Cooee, new in C1:

- `crates/cooee/tests/fixtures/drover-contract/` is a copy of that folder at the drover commit cooee's plugin index
  pins, written by `just drover-contract` and listed `generated`, so it is never edited by hand.
- `crates/cooee/tests/drover_contract.rs` parses every sample with cooee's own row, target and action types and fails
  on a key it reads that is missing. A drover bump that would break cooee fails in cooee's CI at the bump.

## The issues to file, in order

| # | Repo | Issue | Needs |
|---|---|---|---|
| D1 | drover | rows: `core::thread` rows from the log, the three read routes, the contract folder, its test and `append_only` | this ADR |
| C1 | cooee | read a plugin's rows: the `[threads]` manifest table, the follower and its cursor, draw by key, type `prompt`, send `push`, post actions; the contract copy and its test | D1 |
| D2 | drover | threads are cooee's to open: `PUT thread`, `said`, `say` as a row, a profile per repo, the `thread` stamp, emitters set `prompt` instead of delivering to a cooee thread, `rows-stale`; the manifest generated, with `[threads]` | D1, C1 released |
| C2 | cooee | one Orchestrator: [New] picks a repo, all or none (the profile), the repo filter, `PUT thread` and `said` | D2 |
| C3 | cooee | the desk thread: one per repo, made on first use, where `to: desk` rows land | C2 |
| #1609 | drover | cards to the orchestrator first, by thread id or desk; judgment; quiet decisions; the digest (all as rows) | D2, C3 |
| C4 | cooee | [chat] on an owner card: opens the card's thread with the row's `text` quoted | C3 |
| #1610 | drover | handoffs go to the desk at once; the `handoff-move` action | #1609 |
| C5 | cooee | [move] on a handoff row: posts `handoff-move` | #1610 |
| #1608 | drover | cap raises within 5% are the orchestrator's, a quiet row | #1609 |
| — | owner | the config card that removes the web push notifier (cooee now notifies) | C3 live |
| C6 | cooee | import: each open drover thread into a cooee thread, then `PUT` with `from` | C2 |
| D3 | drover | no new drover-hosted thread: `thread new`, the page's new thread and `chat-about` send to the repo's desk | C6 run |
| D4 | drover | the cut: delete the engine, web push, the `[chat]` table, the old attach routes and the `chat-cards` place; the bans | D3, open question 1 answered |

## The rules lines

Both files are at their 30-line limit, so each line replaces or widens one.

Drover's `.drover/rules.md`. The first replaces the `assets/types/` line; the second is #1568's "no new chat or thread
UI" line, widened (D1 and D4 add them):

```
- don't let assets/types/ drift from `just types`, hand-copy a generated type, or remove or retype a key of crates/cli/tests/golden/contract/v1/: a breaking change is /api/v2 (enforced by: CI TS types, `just types-lint`, CI arch-check `append_only`, crates/cli/tests/contract_v1.rs)
- don't call cooee, start or type into a thread's agent, send a notification, or add chat or thread UI in drover: emit a row or serve an action in /api/v1 (enforced by: CI arch-check bans `/plugin/`, `webpush`, `term::` and `shed::` in crates/core/src/thread/; UI by review)
```

Cooee's `.drover/rules.md`. It replaces the last line, "Don't put workflow logic … (enforced by: review)"; C1 adds
it:

```
- Don't put workflow logic (issues, queues, lanes, reviews, merges, where a card goes, when to notify) in cooee, or name a drover route or event kind outside the plugin manifest: draw a row's block, type its prompt, send its push, post its action (enforced by: CI arch-check `api/v1` and `needs-decision` bans (.drover/arch/boundaries.toml), crates/cooee/tests/drover_contract.rs)
```

with, in cooee's `.drover/arch/boundaries.toml` (the one existing call, the machines screen's host tags, is excepted):

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

## Hot files

Files this design touches that 5 or more merged PRs touched in 14 days:

| File | PRs | Class | Here |
|---|---|---|---|
| `crates/core/src/orchestrator.rs` | 26 | large module | splits: `core/src/thread/` (registry, rows, route) is new, the engine is deleted, about 300 lines stay |
| `cooee-plugin.toml` | 11 | central list (the actions line) | generated in D2 from `templates/cooee-plugin.toml` and the server's action table |
| `crates/server/src/screens.rs` | 62 | central list (the action match) | +1 arm once in D1: thread actions go to `serve/rows.rs`'s own table, so #1609 and #1610 add none here |
| `docs/serve.md`, `docs/orchestrator.md` | 55, 15 | docs per area | the contract is a new `docs/rows.md`; the two lose their thread routes and engine sections, and gain one link |
| `.drover/rules.md` | 9 | central list, capped at 30 lines | two lines replaced, none added |
| `crates/cli/tests/api_v1_contract.rs` | 12 | tests | untouched: the contract has its own test file and a folder of one file per route |
| `serve.rs`, `serve/v1.rs`, `serve/chat.rs`, `api.rs`, `events.rs`, `lib.rs`, `cli.rs` | 24 to 88 | already `shared` | new routes and types go in `serve/rows.rs` and `core/src/thread/`; `v1.rs` gets one arm |
| `assets/types/api.d.ts` | 27 | already `generated` | `just types` |

`.drover/arch.toml`, in D1 (the PR that adds the contract folder):

```toml
append_only = ["crates/cli/tests/golden/contract/v1/*.keys"]

[[ban]]
pattern = "/plugin/"
paths = ["crates/*/src/**"]
reason = "#1571: drover never calls cooee; it serves rows and actions in /api/v1"
```

In D2 (the PR that converts the manifest), with one line added to `just gen`,
`BLESS=1 cargo test -q --locked -p drover --test cooee_manifest`:

```toml
generated = ["cooee-plugin.toml"]   # appended to the list above
```

In D4 (the cut):

```toml
[[ban]]
pattern = "webpush"
paths = ["crates/*/src/**", "web/src/**"]
reason = "#1571: cooee sends every notification; a row's push says when"

[[ban]]
pattern = "term::"
paths = ["crates/core/src/thread/**", "crates/core/src/orchestrator.rs", "crates/core/src/orchestrator/**"]
reason = "#1571: cooee runs a thread's agent; a line for it is a row's prompt"

[[ban]]
pattern = "shed::"
paths = ["crates/core/src/thread/**", "crates/core/src/orchestrator.rs", "crates/core/src/orchestrator/**"]
reason = "#1571: cooee runs a thread's agent; a line for it is a row's prompt"
```
