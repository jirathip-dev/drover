# Conductor chat (#355, steps 1 to 3)

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
account), `conductor-sleep` (why: idle, gone, usage limit, signed out); #387: `config-propose`, `config-settled`,
`secret-request`, `secret-set`, `secret-dismissed`, `secret-rm` (names only), `telegram-topic`.

## Config changes (#387)

`herdsman config propose <thread> <file>`: `<file>` is the whole new fleet.toml (the conductor edits a copy). It is
validated like fleet.toml itself, and its diff shows in the thread as a card with **[apply] [discard]**. Only the
owner applies it: a tap on a paired device, or `herdsman config apply|discard <id>` outside a lane (the lane hook
refuses both inside one, `OWNER_ONLY`). Applying keeps the old file as `fleet.toml.bak-<time>`, writes the
new one and runs `doctor`. If fleet.toml changed since the proposal, it refuses `STALE`. Settled, the card folds to the
thread's system line, e.g. `config c1 applied by owner, backup kept, doctor ✓` (the conductor gets it too).

## Accounts (#387)

The conductor uses its role's `account`/`accounts` or #302's `fallback` chain. When `check` sees the awake conductor's
usage-limit banner (or a sign-out), it rests that account (else that harness), closes the pane, and resumes the
**same session** on the next one (Claude Code `--resume <id>`). The thread shows `switched to claude:B (claude:A
resting until 18:40)`. A harness that can't resume gets the thread so far in its kick. A sign-out also adds a
`<harness> is signed out: [sign in]` line; the button opens #303's setup terminal on the main host. The conductor
never edits the account list itself: that change is a `config propose` card.

## Secrets (#387)

- `herdsman secret request <thread> <NAME> --why "…"` shows a masked-input card with [save] [dismiss], on paired
  owner devices only (elsewhere the card says so).
- [save] stores the value in the host's keychain as `herdsman/<fleet>/<NAME>` (`<fleet>` is the thread's, or `all`),
  hex-encoded on `security -i`'s stdin, never in argv. Events, the chat and lanes never see it. The thread records
  only `secret <NAME> set ✓ (by owner)`.
- A fleet's `env` refers to it as `KEY = "secret:<NAME>"`. At spawn the lane's own shell reads it
  (`security find-generic-password … -w`, the fleet's item, else `all`'s).
- `herdsman secret ls` and `herdsman secret rm <fleet|all> <NAME>` are owner-only.

## Telegram (#387, optional)

herdsman talks to the Telegram Bot API itself (no Hermes). The bot token is the keychain item
`herdsman/all/TELEGRAM_BOT_TOKEN` (a `secret request` in an `all` thread stores it there), never fleet.toml. curl
gets it on stdin only.

```toml
[telegram]
chat = -100123     # a forum supergroup the bot administers (topics on)
owner = 4242          # your Telegram user id: only your messages and taps count
# api = "https://api.telegram.org"
```

- `herdsman serve` long-polls it (it moves to Phase C, #345, later).
- **One forum topic is one thread.** A new topic starts a thread under **all repos**, unless its name starts with a
  repo's fleet name. A thread started on the board gets a new topic.
- The conductor's replies and system lines go to the topic. An ask's options become inline keyboard buttons (a tap
  answers it, `via: telegram`). Images go both ways: a photo you send is saved in the thread's `uploads/` and typed
  in with its path, and the conductor's images arrive as photos (other files as documents). A config or secret card
  says it waits on the board.
- The board stays the primary UI and shows the whole conversation. Your own messages stay where you wrote them.
- Cursor: `<state dir>/telegram.json` (the next update id, the events relayed). The first run relays nothing older.
