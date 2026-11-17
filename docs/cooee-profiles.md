# Orchestrators as cooee chat profiles (#1057)

Drover serves each orchestrator to cooee as a chat profile, so the owner's `[orchestrator] instructions` file is the
one source of the orchestrator's text for both a drover thread and a cooee chat
(design: [architecture/orchestrator-chat-profile](architecture/orchestrator-chat-profile/README.md)).

## Routes (plugin token only: `Authorization: Bearer <cooee-token>`, else 401)

- `GET /cooee/profiles`: `{"profiles": [...]}`, one per orchestrator: `default` (`[orchestrator]`), then each
  `[orchestrator.<id>]`. Each request rewrites `<state dir>/chat/<id>.md` when its text changed, so an edit to the
  instructions file reaches the next chat with no restart.
- `POST /cooee/thread-started` `{"thread", "profile"}`: links the cooee thread to the orchestrator's fleet for asks
  (`orchestrator-attached`, `via: profile`), once per thread. `drover ask --thread <it>` then works; lane asks are
  never routed to it. A read-only `serve` answers 405.

## A profile's keys

| Key | Value |
|---|---|
| `name` | the orchestrator id (cooee shows `drover/<id>`) |
| `label` | its `name` |
| `agent` | the orchestrator role's ACP agent |
| `instructions` | `<state dir>/chat/<id>.md`: the role part, `roles/orchestrator/chat.md`, the usage doc, where it sits, then the instructions, memory, capability groups and skills |
| `settings` | the role's lane profile, as the provider's settings variable takes it |
| `env` | `DROVER_LANE=<fleet>:-:orchestrator`, `DROVER_ORCHESTRATOR=<id>`, `DROVER_THREAD={thread}` |
| `policy` | the fleet's `[runtime] acp_policy`, if set |
| `hidden`, `why` | the guard is not proven (run `drover profiles sync`), or the role has no ACP agent or seat |

The fleet is the first of the orchestrator's fleets with an orchestrator role. An `[orchestrator.<id>]` id is
lowercase letters, digits and `-`, and not `default`; config load refuses another.

## The persona

```toml
name = "Conductor"
role = "orchestrator"
fleets = ["cooee", "herdsman"]
avatar = "…"
profile = "drover/default"
```

## Live check

`drover profiles sync`, then start a cooee chat on that persona: its first message carries the conductor
instructions, and a `gh pr merge` in it is refused by drover's hook.
