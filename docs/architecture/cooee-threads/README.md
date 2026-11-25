# cooee-threads: drover's threads are cut; chat lives in a client; a judge lane answers asks (#1571)

## Summary

Decision: drover cuts its thread store, its thread engine, its thread screens and `drover chat`. Chat lives only in
a client: cooee's Orchestrator agent, or a terminal session, each loading drover's orchestrator rulebook as a
profile and calling drover's CLI and `/api/v1`. Drover keeps asks, cards, the rulebook, the row and action contract,
its web push, and its board, TUI and page, which answer asks and cards with taps. A short-lived judge lane, one per
open lane ask or needs-human card, does the autonomous answering that a thread's orchestrator did. Drover never
calls a client, and neither side has a code dependency on the other: only the plugin manifest, rows and actions.
Slices: D1 (#1617) is merged. Next come #1609 (the judge lane), D2 (clients), #1610 (handoffs), D3 (the freeze and
the migration of open threads), D4 (the deletion), D5 (the manifest) and #1608, then C1 and cooee #802 on cooee's
side. #1598 closes with D2, #1610 with its own slice.
Open questions: none.

Status: revised after the owner's 2026-11-25 decisions. Read on drover staging `fd81ef5c3` (D1 included).

| File | What |
|---|---|
| this page | how the callers read, the design on one screen, the owner's decisions |
| [adr.md](adr.md) | question, options, decision, consequences, the judge lane, the contract test, the coupling list, the issues in order, the rules lines |
| [shapes.md](shapes.md) | the row, action, manifest and judge contracts as types |
| [boundaries.md](boundaries.md) | which module may call which; the migration, open-thread, judge and row statecharts |
| [owners.md](owners.md) | every function of the three files: keep, move inside drover, or delete |

## The callers, sketched first

### a client opens an Orchestrator chat (cooee #802, or a terminal)

```
GET  /api/v1/manifest                      name, api, screens, actions, profiles, threads paths (D5)
GET  /cooee/profiles                       one profile per repo: all, none, each fleet (#1057: brief, hook, lane env)
POST /api/v1/orchestrator-attach           {"fleet": "api", "thread": "t19a3f0c2b11"}
200 {"ok": true, "brief": "…", "settings": "…/settings.json", "env": {"DROVER_THREAD": "t19a3f0c2b11", …}}
```

The client starts its own agent with the brief, hook settings and env the attach returns. `fleet` is the repo picked
on [New]: a fleet, `all` or `none` (D2). Every event that agent's drover commands log carries the thread, so an ask it
files or an issue it queues comes back to that chat. A terminal session does the same through
`drover orchestrator attach <repo>` (D2), which prints the brief path, the settings path and the env lines.

### the client reads the rows

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

For a row whose `to` is a chat the client attached, it draws `block` there, types `prompt` into that chat's agent and
sends `push` to its devices. Any other row is drawn in its cards place. It never reads `kind` to decide workflow.

### the owner taps a card, on drover's board, TUI or page, or in cooee

```
POST /action    {"screen": "rows", "block": "ask-q3101", "action": "answer",
                 "values": {"fleet": "api", "id": "q3101", "choice": "1", "note": "only this once"}, "by": "phone"}
200 {"line": "api q3101: retry"}
```

Options are shown recommended first, with a short note field. The settled card is the next row with the same `key`.

### drover judges an open ask before the owner sees it (#1609)

```
needs-decision q3101 (lane ask, routine, api #412)
  → drive: spawn api-412-judge-q3101 (role judge, no worktree, the orchestrator rulebook, judge_minutes)
  → the lane: drover answer api q3101 1 --basis "> retry a flaky gate once (Amendment 2026-11-20)"
       or:    drover answer api q3101 1        (no --basis: OWNER_ASK files it to the owner, choice 1 recommended)
  → the lane exits; drive closes its pane
```

A judge's answer is a quiet `decision` row with [change] for the owner's window and a line in the daily digest. An
escalated ask is an owner card. A routine ask nobody answers still takes its recommendation after 30 minutes.

## The design on one screen

| Thing | Before (staging today) | After D4 |
|---|---|---|
| Orchestrator chat | drover threads (`thread new`, the page, `drover chat`), or a cooee chat attached | a client only: cooee's Orchestrator, or a terminal session |
| Messages, uploads, model and effort pick, running the chat agent | drover for its threads | the client |
| Role text, brief, hook, permissions | drover's | drover's, served as profiles and by `orchestrator attach` |
| Asks, cards, handoffs, system lines, merges, alerts | drover rows | the same rows, with the same keys |
| A workflow line for the chat agent | drover types it in (`deliver`) | the client types the row's `prompt` |
| Answering a lane ask or needs-human card on the owner's behalf | the routed thread's orchestrator | a judge lane per ask or card |
| Answer, approve, deny, apply, secret, [move] | drover actions | the same actions, on every surface |
| Drover's board, TUI and page | queue, lanes, events, PRs, hosts, cards, threads | queue, lanes, events, PRs, hosts, cards |
| Notifications | drover's web push | drover's web push for its page; the client's own from a row's `push` |

A device subscribes to one sender or the other, so nothing arrives twice. Every arrow is a client calling drover.

## The owner's decisions

1. Chat lives only in a client: cooee's Orchestrator agent, on drover's rulebook served as a profile, or a terminal
   session loading that rulebook. Drover's threads are cut (2026-11-25).
2. Drover keeps asks, cards and the rulebook. The board, the TUI and the page answer asks and cards with taps
   (options, recommended first, plus a short note) and keep the queue, lanes, events, PRs and hosts.
3. Autonomous answering moves to a judge lane: drover spawns a short-lived orchestrator-role lane in shed for each
   open lane ask or needs-human card. It answers with `--basis` when the spec, Amendments or a past decision settles
   it, else it files the ask to the owner, then exits. Routine asks still default after 30 minutes.
4. Drover and cooee have no code dependency or hard-wired coupling. Both speak only the blocks contract: screens JSON,
   row streams and a generic plugin manifest (name, API URL, screens, actions). Drover serves `/api/v1` and that
   manifest; which agent gets which plugin is cooee's config. Naming the other in labels, comments or docs is fine,
   and no renaming work is wanted.
5. Cooee's Chief is the general agent; drover's Orchestrator is one of cooee's agents, and Chief hands off to it
   (cooee #802). It runs as a client of drover's CLI and `/api/v1`.
6. Cooee has one Orchestrator agent. Each of its chats names a repo, `all` or `none`, picked on [New], and the thread
   list filters by repo. A `none` chat is a free chat: no cards are routed there, and it can still act on any repo.
7. Asks and cards route to the chat that filed the work, by thread id. Drover never calls a client.
8. Drover keeps its web push and devices code for its own page. Cooee sends its own alerts from drover's rows. A
   device subscribes to one or the other.
9. `crates/server/src/serve/device.rs` stays: it serves the scene to a companion device (#1497), not alerts.
10. #1609 (judgment, quiet decisions, routing by thread id, the digest) follows D1. #1610 (handoffs) and #1608 (cap
    raises) are drover's, as rows and actions. #1638 closes with #1609; #1598 and #1610 close with this ADR's slices.
11. The API contract, the row and action shapes and the contract test stay. #1617, #1645 and #1663 stand.
