# ADR: drover's threads are cut; chat lives in a client; a judge lane answers asks (#1571)

## Summary

Decision: drover cuts its thread store, its thread engine, its thread screens and `drover chat`. Chat lives only in
a client (cooee's Orchestrator agent or a terminal session) that loads drover's orchestrator rulebook as a profile
and calls drover's CLI and `/api/v1`. Drover keeps asks, cards, the rulebook, the rows and actions, its push, and its
board, TUI and page, which answer with taps. A short-lived judge lane per open lane ask or needs-human card does the
autonomous answering. Drover never calls a client.
Slices: D1 (#1617) is merged; then #1609, D2, #1610, D3, D4, D5 and #1608 in drover, and C1 and cooee #802 in cooee.
Open questions: none.

Status: revised after the owner's 2026-11-25 decisions, replacing the 2026-11-24 "drover keeps its threads" revision.
Read [README.md](README.md) first, then this, [shapes.md](shapes.md), [boundaries.md](boundaries.md),
[owners.md](owners.md).

## Question

The owner chats in cooee, and cooee runs its own Orchestrator agent on drover's rulebook. Drover also runs a second
chat engine for its own threads: a thread store, a session engine typing into agents through shed, thread screens and
`drover chat`, about 2.3k lines that duplicate the client's job and cause #1598 when both engines touch one session.
How does drover drop its chat while asks and cards still reach the chat that filed the work, the owner can still
answer everything from drover's own board, TUI and page, and asks still get answered on the owner's behalf when the
spec settles them?

## Where it stands (drover staging `fd81ef5c3`)

- D1 (#1617) is merged. `core::thread` makes rows from the log, and `thread::route::home` picks a row's target.
  `GET /api/v1/rows`, its stream and `thread/<id>/rows` serve them. `crates/cli/tests/golden/contract/v1/`,
  `contract_v1.rs` and `append_only` freeze their keys.
- A lane ask goes through `thread::route::route` to the repo's newest drover thread, and `lane_ask` types its routing
  line into that thread's orchestrator through `orchestrator::deliver`. The orchestrator answers with `--basis`, or
  answers without one and `answer`'s `OWNER_ASK` path files it to the owner with its pick recommended (`to_owner`).
  With no thread, a lane ask waits for the owner, and a routine one defaults after `ask_default_minutes`.
- Drover types `DECISION` lines, handoffs, card settle lines and `continue` into thread agents (`deliver`, `say`).
- Cooee attaches a chat with `POST /api/v1/orchestrator-attach`, gets the brief, settings and env with
  `DROVER_THREAD`, and D1's registry counts it as an open thread. `deliver` still starts a same-name session in shed
  for it: #1598's `NAME_TAKEN`.

## Options

**Where chat lives**

1. Drover keeps its threads, and a client chat is a second run path (the 2026-11-24 revision). Two engines, two
   stores of a conversation, and #1598's class of bug stays possible. The owner replaced this on 2026-11-25.
2. Drover's threads, and a client is a window onto them. Cooee's chat becomes a relay. Declined on 2026-11-24.
3. **Chat lives only in a client; drover keeps the workflow (chosen).** One engine per chat, run by the client that
   shows it. Drover's surfaces keep everything but the conversation.

**Who answers an ask on the owner's behalf**

1. A long-running orchestrator in a drover thread. Gone with the threads.
2. The client's Orchestrator chat. The owner's chat would be woken for every lane ask, and nothing answers while the
   client is down.
3. **A short-lived judge lane per open ask or card (chosen).** Drover spawns it like any lane, it has one question and
   the spec, and it exits. It runs with every client down.

**How a workflow line reaches a client chat's agent**

1. Drover types through shed into the client's session. Drover would call into the client's run path: #1598.
2. **The client reads rows, and a row may carry a `prompt` (chosen, D1).** One cursor per client.

**What a row carries**

1. Typed data only. Every new card would be a client change.
2. **The block drover builds, beside a small typed envelope (chosen, D1).** A new card is a drover change only.

**How a client finds drover's paths**

1. Hard-coded in the client. Coupling by code.
2. **A generic plugin manifest drover serves (chosen, D5):** name, API URL, screens, actions, profiles and the row
   paths. The client holds no drover path.

## Decision

- **No drover threads.** From D3 drover makes no new thread; from D4 it has no thread store, engine, screens,
  uploads or `drover chat`. `GET /api/v1/thread/<id>/rows` stays: rows are a function of the log, so old history
  stays readable by id.
- **Client chats.** A chat is opened by a client with `orchestrator-attach {fleet, thread}`: cooee's Orchestrator, or
  a terminal through `drover orchestrator attach <repo> [--thread <id>]` (D2), which logs the same attach and prints
  the brief path, the settings path and the env lines. `fleet` is a repo, `all` or `none`. The client runs the agent
  on the returned profile (#1057). The agent runs drover's CLI, so its asks, cards and queued issues carry its thread.
- **Rows.** D1's `Row` is the contract ([shapes.md](shapes.md)). A row whose `to` is an attached chat carries
  `prompt`; the client types it in. Drover delivers into no agent session but its own lanes'.
- **Routing.** `home(fleet, issue)` is the attached chat that filed or queued the issue, unless its repo is `none`;
  else `{"k": "desk", "repo": fleet}`, which from D4 names no thread: it is the repo's owner cards, on drover's board,
  TUI and page and in a client's cards place. Rows about a chat itself go to that chat.
- **The judge lane (#1609).** For each open lane ask and each needs-human card drive spawns one judge lane: role
  `judge`, the orchestrator role's harness and model unless `[fleet.<n>.role.judge]` sets one, no worktree, read-only
  on code, its brief the rulebook's judging part plus the ask or card, the issue's spec with its Amendments and the
  issue's past decisions. It answers with `--basis` (or `extend --basis` on a needs-human card) when those settle it,
  else it answers without one and `OWNER_ASK` files it to the owner with its pick recommended, then it exits. It runs
  once per ask id or needs-human episode (`action_id` `judge:<fleet>:<id>`), within `judge_minutes`, admitted like any
  spawn and never forced. A judge that dies, times out or is refused files the ask to the owner, logged
  `judge-failed`. A routine ask with no answer still defaults after `ask_default_minutes`.
- **Quiet decisions.** A judge's answer is logged `by: judge`; its `decision` row has no push and keeps [change] for
  `ORCHESTRATOR_CHANGE_MINUTES`. The daily `digest` row lists them (#1609).
- **Taps.** The board, the TUI and the page answer every ask and card with the `answer`, `run-card`, `config-card`,
  `secret-card` and `handoff-move` actions: options recommended first and a short note (D4 adds the note field where
  a surface lacks one).
- **Handoffs (#1610).** A handoff to a repo is an open `desk` row at once, with no pick: `land` goes. [move]
  (`handoff-move`) re-targets it to an attached chat, the first copy marked forwarded.
- **Push.** From D2 a row's `push` is always set. Drover's web push still sends from events to devices subscribed on
  its page; a client sends `push` to its own devices.
- **Open threads.** D3 freezes them and D4 stops and archives them ([boundaries.md](boundaries.md)). A client may
  attach an old thread's id to carry on: from D3 attach takes an archived drover thread's id, so the issues that
  thread filed route to the chat that adopted it. History stays in the log; nothing is copied out.
- **The manifest (D5).** `GET /api/v1/manifest` serves name, API URL, screens, actions, profiles and the `[threads]`
  paths. `cooee-plugin.toml` becomes `just gen`'s output of it, listed `generated`.

## Consequences

- One chat engine per chat. #1598 goes: drover no longer starts or types into any chat session.
- About 2.7k lines leave drover and 350 move inside it ([owners.md](owners.md)): the thread engine and store in
  `orchestrator.rs`, `serve/chat.rs`'s thread screens and composer, `chat/threads.rs` and `threads/view.rs`, and the
  `thread` verbs.
- With every client down the owner still answers every ask and card on drover's board, TUI and page, and judge lanes
  still answer what the spec settles. There is no chat until a client is back or a terminal session starts.
- An ask from a chat that has detached routes to the repo's cards, not to a dead chat.
- Judge lanes cost a short agent run per ask. `judge_max` (default 2) caps them per fleet; an ask over the cap waits
  for the next pass, and its routine default still runs.
- A client chat's messages live in the client. Drover holds only the owner lines a client copies through `said` (D2),
  for `answer --typed`.
- `all` and `none` become reserved fleet names, refused at config load (D2).
- The thread uploads folder stays on disk after D4. Deleting it is data deletion: an owner run card, not a slice.

## The judge lane

| Item | Value |
|---|---|
| Trigger | a drive pass: an open `needs-decision` with `lane_ask`, or a `needs-human` issue, with no `judge` spawn for that id or episode |
| Key | `<fleet>-<issue>-judge` agent, `action_id` `judge:<fleet>:<ask id>` or `judge:<fleet>:nh<seq>` |
| Brief | `roles/orchestrator/judge.md` (the rulebook's judging part, out of `role.md`), the ask or card, `drover brief`'s spec, the issue's decisions |
| May run | `drover brief`, `drover asks`, `drover why`, `drover answer --basis`, `drover answer` (to the owner), `drover extend --basis`, `drover turn done` |
| Must not | edit files, spawn, merge, queue, amend, ask a new question (the lane hook's `judge` role list) |
| Ends | after its one answer; drive closes its pane at `turn done` or at `judge_minutes` |
| Fails | dead, timed out or refused: the ask goes to the owner (`to_owner`, why `judge not reached`), logged `judge-failed` |
| Limits | `judge_minutes` (default 10), `judge_max` per fleet (default 2), both `[limits]` keys |

An owner-kind ask from a lane may be answered only with `--basis`, as today. An orchestrator chat's own ask (one with
`thread`) is never judged: it is the owner's.

## The contract test

Drover (D1, merged):

- `crates/cli/tests/golden/contract/v1/` holds `<name>.json` and `<name>.keys` per route and row kind.
- `crates/cli/tests/contract_v1.rs` drives the real server and fails on any difference. `BLESS=1` adds new lines only.
- `append_only` fails a PR that removes or changes a line of a `.keys` file; a breaking change is `/api/v2`.
- `to.k = "desk"` and `to.k = "thread"` stay: D4 changes what `desk` means to a drover surface, not its shape.
- D2 adds the `.keys` of `said` and `orchestrator-attach`'s answer, #1610 `handoff-move`, D5 `manifest`.

Cooee (C1): `crates/cooee/tests/fixtures/drover-contract/` is a `generated` copy of that folder at the pinned drover
commit, and `crates/cooee/tests/drover_contract.rs` parses every sample with cooee's own types and fails on a key it
reads that is missing. A drover bump that would break cooee fails in cooee's CI at the bump.

## Real coupling, and where each piece goes

| Coupling | Where it goes |
|---|---|
| `orchestrator::deliver` starting or typing into a session of a chat the client runs (#1598) | D2 guards it; D4 deletes the engine |
| `crates/server/src/cooee.rs`: `GET /cooee/profiles`, `POST /cooee/thread-started` | stay as routes; D5 lists them in the manifest, so a client holds no path |
| `cooee-plugin.toml`, written by hand in cooee's format | D5: generated from the manifest drover serves |
| cooee's `api/v1` call in its machines screen | cooee's ban exception, read from the manifest in C1 |
| cooee's drover-cards place reading drover's screens | stays: it is the manifest's screens |

Nothing in drover imports, calls or holds an address of cooee; the `/plugin/` ban keeps it so.

## The issues to file, in order

| # | Repo | Issue | Needs | Closes |
|---|---|---|---|---|
| D1 | drover | #1617, merged: rows from the log, `thread::route`, the rows routes, contract v1 and its test | this ADR | |
| #1609 | drover | the judge lane, `[limits] judge_minutes`, `judge_max`, `roles/orchestrator/judge.md`, the `judge` hook role; quiet decisions; the digest; routing by thread id, else `desk` | D1 | #1638 |
| D2 | drover | clients: `deliver` skips an attached chat; attach takes `all`, `none`; `drover orchestrator attach`; `said` and `--typed` reading it; `push` always set; the `.keys` of `said` and attach | D1 | #1598 |
| #1610 | drover | handoffs to a repo are an open `desk` row at once, `land` deleted; [move] as `handoff-move` | D2 | #1610 |
| D3 | drover | the freeze: `thread new` and [New] refused `THREADS_CUT`; lane asks routed to the judge, not to threads; attach adopts an archived thread id; each open thread's closing row | #1609, #1610, C1 released | |
| D4 | drover | the cut: stop and archive every open thread, delete the store, engine, screens, uploads code, `drover chat` and the `thread` verbs but `handoff`; the note field on taps; the `thread-new` ban | D3 | |
| D5 | drover | `GET /api/v1/manifest` and its `.keys`; `cooee-plugin.toml` generated from it | D2 | |
| #1608 | drover | cap raises within 5% are the judge's, a quiet row | #1609 | |
| C1 | cooee | read a plugin's rows: the follower and its cursor, draw by key in an attached chat or the cards place, type `prompt`, send `push`, post actions and `said`; the contract copy and its test | D2 | |
| cooee #802 | cooee | Chief's hand-off to the Orchestrator agent; [New] picks a repo, `all` or `none`, attaches, and may adopt an old thread id; the repo filter | C1 | |

#1645 and #1663 stand: they move the same routes into serve's own process and change no shape. D3 waits for C1 to be
released, so the owner has a chat before drover's stop taking new threads.

## The rules lines

Both files are at their 30-line limit, so each line widens one.

Drover's `.drover/rules.md`, line 13 (D1's contract line), widened by D2:

```
- don't let assets/types/ drift from `just types`, hand-copy a generated type, remove or retype a key of crates/cli/tests/golden/contract/v1/, call a client, or type into a chat a client runs: a breaking change is /api/v2, and a line for a client's chat is a row's prompt (enforced by: CI TS types, `just types-lint`, CI arch-check `append_only` and the `/plugin/` ban, crates/cli/tests/contract_v1.rs, D2's test that a lane ask to an attached chat starts no shed session)
```

D4 widens it once more, adding "or make a drover thread" and the `thread-new` ban to its enforcers.

Cooee's `.drover/rules.md`. C1 replaces its last line, "Don't put workflow logic … (enforced by: review)", with:

```
- Don't put workflow logic (issues, queues, lanes, reviews, merges, where a card goes, when to notify) in cooee, or name a plugin route or event kind outside its manifest: draw a row's block, type its prompt, send its push, post its action (enforced by: CI arch-check `api/v1` and `needs-decision` bans (.drover/arch/boundaries.toml), crates/cooee/tests/drover_contract.rs)
```

Cooee's `.drover/arch/boundaries.toml` gains two bans in C1:

```toml
[[ban]]
pattern = "api/v1"
paths = ["crates/*/src/**"]
except = ["crates/cooee-server/src/screens/settings.rs"]
reason = "drover#1571: cooee reads a plugin's rows at the paths its manifest names; it holds no plugin route"

[[ban]]
pattern = "needs-decision"
paths = ["crates/*/src/**"]
except = []
reason = "drover#1571: cooee draws a row's block and never reads a workflow event kind"
```

## `.drover/arch.toml`

This PR adds none: the line this design ends with bans code that still exists, so it lands with D4.

```toml
[[ban]]
pattern = "thread-new"
paths = ["crates/*/src/**"]
except = ["crates/core/src/thread.rs"]
reason = "#1571: drover makes no thread; chat lives in a client, and thread.rs only reads old events"
```

D1's `append_only` and `/plugin/` ban stay.

## Hot files

| File | Here |
|---|---|
| `crates/core/src/orchestrator.rs` | D2 adds a guard in `deliver`; D4 deletes about 900 lines ([owners.md](owners.md)) |
| `crates/core/src/lib.rs` | #1609 and D3: a lane ask goes to the judge; `to_owner` takes no thread |
| `crates/core/src/drive.rs`, `spawn.rs` | #1609: the judge spawn |
| `crates/server/src/serve/chat.rs` | D4: the card builders move to `serve/cards.rs`, the rest is deleted |
| `cooee-plugin.toml` | D5: generated |
| `.drover/rules.md` | line 13 widened by D2 and D4, none added |
| `docs/rows.md` | the contract's page: D2, #1610 and D5 add their routes and actions |
| `assets/types/api.d.ts` | `just types` |
