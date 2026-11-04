# Conductor chat (#355, step 1)

Talk to a **conductor** through herdsman itself, with no Hermes needed. A **thread** belongs to one fleet's repo, or
to `all` for cross-cutting work. Each thread is talked through by one conductor lane.

```sh
herdsman thread new acme "Grill #12"          # prints t1
herdsman thread say t1 "what should #12 cover?"
herdsman thread show t1 [--json]              # the conversation; marks it read
herdsman thread ls [FLEET] [--json]           # per repo: title, last message, unread •, awake/asleep
```

## The conductor lane

- **Role `conductor`, any harness:** `[fleet.<n>.role.conductor]` takes the usual role keys (`harness`, `model`,
  `effort`, `account`/`accounts`, `fallback`, `mcp`). An `all` thread uses the first fleet that defines one. Without
  one, `say` refuses `NO_CONDUCTOR`. `herdsman spawn … conductor` refuses `BAD_VALUE`.
- **No worktree:** it runs in `<state dir>/conductor/<thread>/` in its own herdr workspace, as agent
  `conductor-<thread>`, with `HERDSMAN_LANE=<fleet>:-:conductor` (so the lane hook guards it) and
  `HERDSMAN_THREAD=<thread>`.
- **Its brief** (`.brief.md` there, rewritten at each start) is `roles/conductor.md`, the thread and repo, then the
  `[conductor]` profile:

  ```toml
  [conductor]
  instructions = "~/notes/conductor/AGENTS.md"   # inlined
  memory = "~/notes/conductor/memory"            # each *.md inlined
  skills = "~/notes/conductor/skills"            # listed, read on demand
  ```

- **Sleep and resume:** `check` closes an awake conductor whose thread has been quiet for 30 min and that is not
  working (`conductor-sleep`). The next message starts it again with the same session id. Claude Code is started
  with `--session-id <uuid>` and resumed with `--resume <uuid>`, so the full conversation carries over. A harness
  that can't resume gets the thread so far in its kick instead.

## Messages and asks

- `thread say` logs `thread-msg` (from `owner`), then types `[<thread>] owner: <text>` into the conductor (waking it
  first). The conductor answers with `herdsman thread reply <thread> "<text>"`.
- **Choices:** the conductor runs `herdsman ask <fleet> - "<question>" --thread <thread> --option … --recommend N`.
  The thread shows the ask with its options, the recommended one first. `herdsman answer <fleet> <id> <n>` (the
  tap later) logs the decision, adds `DECISION <id>: <text>` to the thread and types it into the conductor.
  `--thread` with an unknown thread refuses `NO_SUCH_THREAD`.

Events: `thread-new`, `thread-msg`, `thread-read`, `conductor-start` (session, resumed, account), `conductor-sleep`.

Still to come in later steps: the `[chat]` tab (from the approved mockup), images, push for replies, the
`config propose` cards and the Telegram bridge.
