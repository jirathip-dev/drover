# Conductor chat (#355, steps 1 and 2)

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
  one (#401), a fleet's conductor is its `impl` role on profile `lane-conductor` (which `profiles sync` writes; a
  Hermes impl needs `hermes profile create lane-conductor`); only a fleet with neither refuses `NO_CONDUCTOR`.
  `herdsman spawn … conductor` refuses `BAD_VALUE`.
- **[config] → Conductor** shows the role in effect (`default: impl` or `set`) with tappable harness, model (the
  models the fleet's roles use) and effort choices. A tap from a paired device writes `[fleet.<n>.role.conductor]` in
  place (the first one copies the impl role's keys), after saving the old file as `fleet.toml.bak`; it is refused
  `DOCTOR` when `herdsman doctor` with the new file shows a ✗ it did not show before. The next conductor start uses it.
- **No worktree:** it runs in `<state dir>/conductor/<thread>/` (or `<[conductor] dir>/<thread>/`) in its own herdr workspace, as agent
  `conductor-<thread>`, with `HERDSMAN_LANE=<fleet>:-:conductor` (so the lane hook guards it) and
  `HERDSMAN_THREAD=<thread>`.
- **Its brief** (`.brief.md` there, rewritten at each start) is `roles/conductor.md`, the thread and repo, then the
  `[conductor]` profile:

  ```toml
  [conductor]
  dir = "~/conductor"                            # optional: the threads' folders' root (default <state dir>/conductor)
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

- **Files:** `herdsman thread reply <thread> "<text>" --file <path>` (repeatable) shows the files with the message:
  images inline, any other file as a row. Only a file in the thread's conductor folder is taken (anything else is
  refused: copy it in first), and the board serves only those. A file the owner attaches on the board is saved in the
  conductor's folder under `uploads/` and its path is typed in with the message (`(attached: <path>)`), so the
  conductor can read it.
- **Commands:** the lane hook logs each `herdsman` or `gh` command the conductor runs (`thread-cmd`, not `thread
  reply`); the board shows them as closed tool rows.

## The [chat] tab (#386)

Built from the approved mockup (`docs/design/conductor-chat/`): the middle tab, `[queue] [lanes] [chat] [host]
[events]`, reading `[chat·N]` with N the threads where a choice waits.

- **The list:** "all repos", then each fleet by name, a card each with its 3 newest threads (the rest fold under
  "N older threads") and `[+ thread]`, which takes a title and starts the thread and its conductor (the title is the
  first message). A row has the unread dot, the title, the age, then `! choose`, `✻ working` or `☾ asleep` and the
  last message.
- **A thread** is the lane sheet's chat blocks: your messages in the accent colour, the conductor's purple, its
  commands as closed tool rows, system lines (resumed, asleep, account switch), images and file rows
  (`GET /thread/<thread>/<name>`), plain links. An open ask shows its choice rows (recommended first) and an "other"
  box, with an amber edge; a tap answers it (`DECISION` typed into the conductor); answered, a green `✓` line with
  who and when stays. Thread asks never join the red needs-you stack. Below: the live line and the composer (`[+]`
  attach, the text, `[send]`). Opening a thread marks it read.
- **At 1000 px and up** the list is a 400 px column with the open thread beside it.
- **Not a paired owner device:** the thread reads the same; the choice rows, `[+ thread]` and the composer are off.
- **Writes** are `POST /thread` (`op` = `new`, `say` with an optional base64 file of at most 10 MB, `answer`,
  `read`) from a paired device, 10 a minute.

**Push** (a `webpush` notifier, default `on`): a conductor's reply is `<repo> · <thread title>: <its first line>`, a
choice `<repo> · <thread title> asks: <question>`; a tap opens the thread. The owner's messages, commands and system
lines never push.

Events: `thread-new`, `thread-msg` (`files`), `thread-cmd`, `thread-read`, `conductor-start` (session, resumed,
account), `conductor-sleep`.

Still to come: the `config propose` cards, secrets and the Telegram bridge (step 3).
