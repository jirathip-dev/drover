# cooee-threads: cooee owns thread chat, drover serves the rows and actions (#1571)

## Summary

Decision: drover serves a list of thread rows (asks, cards, handoffs, system lines, merges, alerts), each with what to
draw, the line to tell the agent and whether to notify. Cooee shows them in the thread or the repo's desk thread, runs
the agent and sends all notifications; it copies message text to drover, so drover's page and terminal still work.
Slices: 4 new drover issues and 6 cooee issues around #1609, #1610 and #1608, in a fixed order. #1609 follows the
first drover slice, before any cooee work; only its desk part waits, and turns on when cooee's desk thread exists.
Open questions: none (the owner settled the three in q3122).

Status: revised after the owner's q3122 (architect lane, 2026-11-22). Read on drover staging `d989c5beb`, cooee
staging `877cd75c8`.

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, what the owner settled |
| [adr.md](adr.md) | question, options, decision, consequences, the contract test, the issues in order, the rules lines, the `arch.toml` lines |
| [shapes.md](shapes.md) | the row and action contract as types |
| [boundaries.md](boundaries.md) | which module may call which; the migration, thread and row statecharts |
| [owners.md](owners.md) | every function of the three files: keep, move to cooee, delete |

## The callers, sketched first

### cooee reads the rows (one follower per plugin, on its server)

```
GET /api/v1/rows?open=1                    Authorization: Bearer <plugin token>
200 {"seq": 48211, "more": false, "rows": [ …the rows still open, no prompt, no push… ]}

GET /api/v1/rows/stream                    Last-Event-ID: 48211
id: 48230
data: {"seq": 48230, "ts": 1795400000, "key": "ask-q3101", "kind": "ask", "fleet": "api", "issue": 412,
       "to": {"k": "thread", "id": "t19a3f0c2b11"}, "open": true,
       "text": "q3101 · #412: retry the flaky gate or mark it?",
       "block": {"type": "card", "id": "ask-q3101", "blocks": [ …a choice whose action is `answer`… ]},
       "prompt": "lane ask q3101 on #412 from api-412-impl (routine): … [1] retry (recommended) [2] mark",
       "push": null}
```

For each row cooee does three things and nothing else: draw `block` in the thread `to` names (a row with a `key` it has
replaces that row), type `prompt` into that thread's agent if there is one, and send a notification if `push` is set.
`{"k": "desk", "repo": "api"}` is the repo's desk thread, which cooee resolves. Cooee never reads `kind` to decide
workflow; it is there for the filter chips and the [chat] and [move] buttons.

### the owner taps a card in cooee

```
POST /action    {"screen": "rows", "block": "ask-q3101", "action": "answer",
                 "values": {"fleet": "api", "id": "q3101", "choice": "1"}, "by": "phone"}
200 {"line": "api q3101: retry"}
```

The settled card arrives as the next row with the same `key` (`open: false`, no choice, a `✓ retry · owner` line).

### cooee tells drover about its threads and messages

```
PUT  /api/v1/thread/t19a3f0c2b11   {"profile": "api", "title": "gate flakes", "state": "working",
                                    "archived": false, "desk": false, "from": null}
POST /api/v1/thread/t19a3f0c2b11/said
     {"id": "m-8841", "ts": 1795400100, "from": "orchestrator", "text": "Retried; green now.",
      "files": [{"name": "run.png", "size": 48211}]}
```

`profile` is the thread's repo: a fleet, `all` or `none`. `from` names the drover thread it continues (a moved one).

### a drover verb emits a row (it never types into an agent again)

```rust
let to = thread::route::home(cfg, &events, fleet, issue);
thread::route::emit(ctx, cfg, Row::system(to, fleet, &line).prompt(&line))?;

thread::route::emit(ctx, cfg, Row::ask(to, &ask).prompt(&told).push(Push::needs(&ask)))?;
```

### drover's own page, terminal and CLI read the same rows

```rust
let rows = thread::rows(&events, id);
let list = thread::list(&events, cfg);
thread::say(ctx, cfg, id, "owner", text)?;
```

`say` logs a `said` row with a `prompt`; cooee shows it as the owner's message and types it into the agent.

### the contract test

```rust
for route in contract::ROUTES {
    let got = keys(&shape(&recorded(route)));
    assert_eq!(got, golden(route), "BLESS=1 records an added key; a removed or retyped one is /api/v2");
}
```

`drover arch-check` fails a PR that removes or changes a line of a `contract/v1/*.keys` file ([adr.md](adr.md)).

## The design on one screen

| Thing | Owner | The other side |
|---|---|---|
| The thread, its messages, uploads, title, archive, read state | cooee | drover holds a text copy for its own surfaces and for `--typed` |
| Running the agent (start, resume, stop, queue, steer, idle sleep, rate-limit hold, sign-in) | cooee, through shed | drover runs no thread agent |
| Model and effort pick, harness | cooee | drover keeps the role's harness in fleet.toml: it decides the profile's agent |
| The orchestrator's role text, brief, hook and permissions | drover (`GET /cooee/profiles`, one profile per repo) | cooee starts the agent with them |
| Asks, cards, handoffs, system lines, merge and rollout rows, decisions, digest | drover (rows) | cooee draws the block |
| Answer, approve or deny, apply or discard, secret, handoff move | drover (actions) | cooee posts the tap |
| Which thread a row goes to | drover names a thread id or a repo's desk | cooee owns the desk thread; until it has `PUT` one, a desk row is an owner card as today |
| Waking the agent with a workflow line | drover sets `prompt` | cooee types it in |
| Notifications | drover sets `push` | cooee sends; drover's web push is deleted |

One direction: every arrow is cooee calling drover. Drover has no cooee address, client or token.

## Settled by the owner (q3122)

1. **#1609 lands early.** Its judgment, quiet decisions and routing to the filing thread need only D1, so it follows
   D1 and lands before C1. Only its desk fallback waits for C3: until cooee has `PUT` a repo's desk thread, a card
   with no filing thread is an owner card as today, and the first `PUT` turns orchestrator first on for that repo.
2. **The cuts at D4 are accepted.** With cooee running the agent, drover's page and its terminal chat keep the thread
   list, the conversation as text, send, and every card with its buttons. They lose what is the engine's: queue, steer,
   stop, the model, effort and harness chips, file upload and inline chat images, new thread (it becomes a message to
   the repo's desk thread), archive and restore, and the [chat] defaults in the config sheet. Until D4 nothing is cut.
3. **The devices file stays.** `crates/server/src/serve/device.rs` is the LAN listener that serves the scene to a
   companion device (#1497), not alert devices. The alert code that goes is `webpush.rs`, the `/push/*` routes, the
   alert devices list on the hosts view and `drover push test`.
4. **Extra orchestrator seats go.** Profiles become one per repo for the one Orchestrator. The `[orchestrator.<id>]`
   tables beyond the default stop being served to cooee at D4, with a config warning.

