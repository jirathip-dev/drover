# cooee-threads: cooee's Orchestrator is a drover client; drover keeps its threads (#1571)

## Summary

Decision: drover keeps its threads, their engine, its page, `drover chat` and its push, and serves every workflow
row (asks, cards, handoffs, system lines, merges, alerts) and action in `/api/v1`. Cooee runs its own Orchestrator
agent on drover's profile as a drover client: a cooee chat attaches with a thread id, so the asks and cards it raises
come back to it as drover blocks. There are two run paths, and drover never calls cooee.
Slices: D1 (#1617) is merged. Next come four drover slices (#1609, D2, D3, #1610) and #1608, then two cooee ones
(C1, cooee #802), in a fixed order. #1598 closes with D2 and #1610 with its own slice.
Open questions: none.

Status: revised after the owner's 2026-11-24 scope change. Read on drover staging `fd81ef5c3` (D1 included).

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, the owner's decisions |
| [adr.md](adr.md) | question, options, decision, consequences, the contract test, the issues in order, the rules lines |
| [shapes.md](shapes.md) | the row and action contract as types |
| [boundaries.md](boundaries.md) | which module may call which; the migration, thread and row statecharts |
| [owners.md](owners.md) | every function of the three files: keep or delete (nothing moves to cooee) |

## The callers, sketched first

### cooee's Chief hands off to the Orchestrator (cooee #802)

```
GET  /cooee/profiles                       one profile per repo: all, none, each fleet (#1057: brief, hook, lane env)
POST /api/v1/orchestrator-attach           {"fleet": "api", "thread": "t19a3f0c2b11"}
200 {"ok": true, "brief": "…", "settings": "…/settings.json", "env": {"DROVER_THREAD": "t19a3f0c2b11", …}}
```

Cooee starts its Orchestrator agent with the brief, hook settings and env the attach returns. `fleet` is the repo
picked on [New]: a fleet, `all` or `none` (D2 accepts the last two). Every
event that agent's drover commands log carries that thread, so an ask it files or an issue it queues has that chat as
its home.

### cooee reads the rows (one follower per plugin)

```
GET /api/v1/rows/stream                    Last-Event-ID: 48211
id: 48230
data: {"seq": 48230, "key": "ask-q3101", "kind": "ask", "fleet": "api", "issue": 412,
       "to": {"k": "thread", "id": "t19a3f0c2b11"}, "open": true,
       "text": "q3101 · #412: retry the flaky gate or mark it?",
       "block": {"type": "card", "id": "ask-q3101", "blocks": [ …a choice whose action is `answer`… ]},
       "prompt": "lane ask q3101 on #412 from api-412-impl (routine): … [1] retry (recommended) [2] mark",
       "push": {"title": "api #412", "body": "retry the flaky gate or mark it?", "tag": "ask-q3101"}}
```

For a row whose `to` is a chat cooee attached, cooee draws `block` there (a known `key` replaces its row), types
`prompt` into that chat's agent and sends `push` to the devices subscribed in cooee. Any other row (a drover thread,
a desk, the owner) is drawn in cooee's drover cards place, with its push. Cooee never reads `kind` to decide workflow.

### the owner taps a card, in cooee or on drover's page

```
POST /action    {"screen": "rows", "block": "ask-q3101", "action": "answer",
                 "values": {"fleet": "api", "id": "q3101", "choice": "1"}, "by": "phone"}
200 {"line": "api q3101: retry"}
```

The settled card is the next row with the same `key`. [chat] on an owner card is the action `chat-about`, [move] on a
handoff is `handoff-move`: drover blocks, the same on both surfaces.

### cooee copies the owner's lines in its chat

```
POST /api/v1/thread/t19a3f0c2b11/said   {"id": "m-8841", "ts": 1795400100, "from": "owner", "text": "yes, retry"}
```

So `answer --typed` from that chat finds the owner's exact line in drover's log.

### a drover verb emits a row; who types it in depends on who runs the thread

```rust
let to = thread::route::home(cfg, &events, fleet, issue);
thread::route::emit(ctx, cfg, Row::ask(to, &ask).prompt(&told).push(Push::needs(&ask)))?;
```

`emit` logs the row's event. For a drover thread it also delivers `prompt` through the engine, as today. For a chat
cooee attached it delivers nothing: cooee types the `prompt` in.

## The design on one screen

| Thing | Drover thread | Cooee Orchestrator chat |
|---|---|---|
| Who opens it | `thread new`, drover's page, a desk (D3) | cooee's [New] or Chief's hand-off, then `orchestrator-attach` |
| Its repo | the fleet it was opened for | the profile picked on [New]: a repo, `all` or `none` |
| Messages, uploads, model and effort pick | drover, as today | cooee |
| Running the agent (start, resume, stop, sleep, hold) | drover through shed, as today | cooee through shed |
| Role text, brief, hook, permissions | drover's | drover's, served by `GET /cooee/profiles` (#1057) |
| Asks, cards, handoffs, system lines, merges, alerts | drover rows, drawn by drover's page and terminal | the same rows, drawn by cooee |
| A workflow line for the agent | drover delivers the row's `prompt` | cooee types the row's `prompt` |
| Answer, approve, deny, apply, secret, [chat], [move] | drover actions | the same actions, posted by cooee |
| Notifications | drover's web push, to devices subscribed on its page | cooee's, from the row's `push`, to devices subscribed in cooee |

A device subscribes to one sender or the other, so nothing arrives twice. Every arrow is a client calling drover;
drover has no cooee address, client or token.

## The owner's decisions

1. Cooee's Orchestrator is a drover client. It is a cooee plugin agent that loads drover's orchestrator rulebook
   through #1057's profile and calls `/api/v1` directly. Cooee's Chief routes to it in hand-off mode, with no relay.
2. Drover stays standalone. Nothing is removed from drover's page, terminal or `drover chat`, and thread chat does not
   move out of drover.
3. Cooee runs its own Orchestrator agent. Each of its chats is tagged with a thread id, so the asks and cards it
   raises come back to that chat as drover blocks. Drover's own threads carry on separately: two run paths.
4. Cooee has one Orchestrator agent. Each of its chats names a repo, `all` or `none`, picked on [New], and the thread
   list filters by repo. A `none` chat is a free chat: no cards are routed there, and it can still act on any repo
   when the owner asks.
5. Drover routes a card to a thread by id: the thread that filed or queued the issue, else the repo's desk thread.
6. Each repo has a desk thread, and it is a drover thread. [chat] on owner cards and [move] on handoffs are drover
   blocks, so they work on drover's page and in cooee alike. #1610 stays drover's; this ADR files no cooee issue
   for them.
7. Drover keeps its web push and devices code for its own page. Cooee sends its own alerts from drover's events.
   Nothing in drover's push is removed.
8. Drover's side of the routing is #1609 (routing, judgment, quiet decisions, digest), #1610 (handoff routing) and
   #1608 (cap raises), all as API rows and actions.
9. The API contract, the row and action shapes and the contract test stay. #1617, #1645 and #1663 stand. The cooee
   side is cooee #802.
10. `crates/server/src/serve/device.rs` stays: it serves the scene to a companion device (#1497), not alerts.
