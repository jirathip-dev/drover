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

## Owner cards from a chat (#1066)

A linked chat's id (`$DROVER_THREAD`) stands where a thread id does in `drover card post`, `drover config propose` and
`drover secret request`; the card is logged on the chat's fleet. `CARD_FORBIDDEN`, the 15-minute approval window and
the owner-only approve, apply and secret entry are unchanged. `GET /screens/cards` shows the open run, config and
secret cards of every thread and chat with their actions (`run-card`, `config-card` [apply]/[discard],
`secret-card` masked entry for the plugin or a paired device). How each settled is one `system` line in the chat,
read as a `note` in `GET /api/v1/thread/<chat>`: the exit code and output tail, applied or discarded,
`secret <NAME> set ✓`. The secret's value is never logged or served.

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
