# Orchestrator chat (#355, steps 1 to 3)

Talk to a **orchestrator** through drover itself, with no Hermes needed. A **thread** belongs to one fleet's repo, or
to `all` for cross-cutting work. Each thread is talked through by one orchestrator lane.

```sh
drover thread new acme "Grill #12"          # prints t1
drover thread say t1 "what should #12 cover?"
drover thread show t1 [--json]              # the conversation; marks it read
drover thread ls [FLEET] [--json]           # per repo: title, last message, unread •, awake/asleep
```

## The orchestrator lane

- **Role `orchestrator`, any harness:** `[fleet.<n>.role.orchestrator]` takes the usual role keys (`harness`, `model`,
  `effort`, `mcp`). An `all` thread uses the first fleet that defines one. Without
  one (#401), a fleet's orchestrator is its `impl` role on profile `lane-orchestrator` (which `profiles sync` writes; a
  Hermes impl needs `hermes profile create lane-orchestrator`); only a fleet with neither refuses `NO_ORCHESTRATOR`.
  `drover spawn … orchestrator` refuses `BAD_VALUE`.
- **[config] → Orchestrator** shows the role in effect (`default: impl` or `set`) with tappable harness, model (the
  models the fleet's roles use) and effort choices. A tap from a paired device writes `[fleet.<n>.role.orchestrator]` in
  place (the first one copies the impl role's keys), after saving the old file as `fleet.toml.bak`; it is refused
  `DOCTOR` when `drover doctor` with the new file shows a ✗ it did not show before. The next orchestrator start uses it.
- **Harness, model and effort (#404):** the orchestrator role keeps its powers; the owner picks only these. `[chat]`
  sets new threads' defaults over the role (each key unset: the role's):

  ```toml
  [chat]
  harness = "claude"   # claude | hermes | dsh; another harness than the role's drops its model
  model = "opus"
  effort = "high"
  ```

  A thread keeps the harness it starts on for good (`drover thread new … --harness <h>`, or the empty thread's
  harness chip; a thread from before #404 has the default). Its composer's model and effort chips offer the models
  that roles (or `[chat]`) on its harness use and the harness's efforts (no model chip where a profile owns the
  model: Hermes, dsh). A pick logs `thread-model`, shows `model sonnet · effort low from the next turn`, and the
  next message resumes the orchestrator's session on it. The header reads `repo · harness · model`.
- **Model list (#860):** the model chip also offers fleet.toml's top-level `models = [...]`; add a model there (via
  `drover config propose`) and the chip offers it. A send on a pick no longer offered is refused `BAD_VALUE` with a
  system line naming the model and the list (the message stays); a model the CLI rejects at start is one system line
  with its error, not retried.
- **No worktree:** it runs in `<state dir>/orchestrator/<thread>/` (or `<[orchestrator] dir>/<thread>/`) in its own shed terminal (an ACP session on a `shed-acp` fleet), as agent
  `orchestrator-<thread>`, with `DROVER_LANE=<fleet>:-:orchestrator` (so the lane hook guards it) and
  `DROVER_THREAD=<thread>`.
- **Its brief** (`.brief.md` there, rewritten at each start) is `roles/orchestrator.md`, the thread and repo, then the
  `[orchestrator]` profile:

  ```toml
  [orchestrator]
  dir = "~/orchestrator"                            # optional: the threads' folders' root (default <state dir>/orchestrator)
  instructions = "~/notes/orchestrator/AGENTS.md"   # inlined
  memory = "~/notes/orchestrator/memory"            # each *.md inlined
  skills = "~/notes/orchestrator/skills"            # listed, read on demand
  allow = ["design", "clean", "hosts"]           # #538: capability groups (default: all but pause and set)
  name = "Conductor"                               # #780: what you call it (default "Orchestrator"), labels only

  [orchestrator.ada]                               # #780: more orchestrators, each a name and its fleets
  name = "Ada"
  fleets = ["acme-api", "all"]                     # overlap allowed; `all` covers the cross-repo threads
  ```

- **Role and name (#780):** code keys off the role `orchestrator` and an orchestrator's id; its `name` is free text
  that `thread show`, `thread ls` and `why` print, so renaming it touches that one field. A thread talks to one
  orchestrator: `thread new <repo> <title> --orchestrator <id>`, else the first `[orchestrator.<id>]` whose `fleets`
  cover the repo, else the default (`[orchestrator]` itself). Its lane gets `DROVER_ORCHESTRATOR=<id>`, and every
  event it logs carries `orchestrator: <id>`. Two orchestrators on one fleet go through the same queue, spawn and
  merge gates as anyone.
- **Old names (#780, one release):** `[conductor]`, `role.conductor`, `--by conductor-<thread>`, a lane's
  `DROVER_LANE` role `conductor` and the old `<state dir>/conductor/` folder are read as the new names, and so are
  the log's old rows: `conductor-start|sleep|attached|detached` as `orchestrator-…`, `from: conductor` and
  `by: conductor-<thread>`. New writes use the new names only. `doctor` prints one deprecation line while fleet.toml
  has the old keys.

- **Sleep and resume:** `check` closes an awake orchestrator whose thread has been quiet for 30 min and that is not
  working (`orchestrator-sleep`). The next message starts it again with the same session id. Claude Code is started
  with `--session-id <uuid>` and resumed with `--resume <uuid>`, so the full conversation carries over. A harness
  that can't resume gets the thread so far in its kick instead.

## Messages and asks

- `thread say` logs `thread-msg` (from `owner`), then types `[<thread>] owner: <text>` into the orchestrator (waking it
  first). The orchestrator answers with `drover thread reply <thread> "<text>"`.
- **Choices:** the orchestrator runs `drover ask <fleet> - "<question>" --thread <thread> --option … --recommend N`.
  The thread shows the ask with its options, the recommended one first. `drover answer <fleet> <id> <n>` (the
  tap later) logs the decision, adds `DECISION <id>: <text>` to the thread and types it into the orchestrator.
  `--thread` with an unknown thread refuses `NO_SUCH_THREAD`.
- **Lane asks (#541):** an `ask` from an impl, rev, fix or design lane (no `--thread`) goes to its repo's orchestrator
  thread first: `[fleet.<n>] ask_thread = "t6"`, else the repo's most recently active thread (archived ones never).
  It is logged `routed` (no push) and typed into that orchestrator (woken as by `say`) as a system line the board hides;
  it stays open on the board and the issue card. No thread (or the orchestrator not reached: `ask-owner`), the owner
  gets it as before. The orchestrator answers with `drover answer <fleet> <id> <n> --basis "<quote | q<id>>"`, logged
  `by: orchestrator-<thread>` with its `basis`, and only if the ask is a lane ask on its repo, its question and choice
  name no owner topic (the `OWNER_TOPIC` words), and the basis is 3+ words found verbatim in the issue's spec
  (Amendments included) or an answered decision id. Else `REFUSED OWNER_ASK: <why>; ask the owner`, and the ask is
  logged `ask-owner` in the thread: a choice row (the orchestrator's pick recommended) that pushes like an ask. Each
  orchestrator answer posts `answered <id> for #<n>: <choice>, because <basis>` with [change] for 30 min (no push); the
  owner's pick there re-sends `DECISION <id>: <choice>` to the lane, `by: owner`, replacing it.
- **Amendments (#574):** `drover amend <fleet> <issue> --basis q<id> "<text>"` (orchestrators and the owner; the lane
  hook refuses it for impl, rev and design lanes) writes an owner's decision into the spec: it appends
  `- **Amendment (q<id>)**: <text>` to the issue body's `## Amendments` section (made once, at the end), logs `amend`
  `{id, text}` and re-runs the open PR's failed spec-reading checks (board-freeze). `q<id>` must be an ask on that
  issue answered `by: owner` (or a routine ask's `by: default`), else `REFUSED NO_DECISION` before any GitHub call.
  An orchestrator runs it as soon as an owner decision names an amendment; the thread shows it as a tool row.

- **Files:** `drover thread reply <thread> "<text>" --file <path>` (repeatable) shows the files with the message:
  images inline, any other file as a row. Only a file in the thread's orchestrator folder is taken (anything else is
  refused: copy it in first), and the board serves only those. Files the owner attaches on the board (#405: several, up to 10 a message,
  each at most 10 MB; picked at once or added with `[+]`, waiting above the box as thumbnails with `[×]`) or with
  `drover thread say <thread> "<text>" --file <path>` (repeatable) are saved in the orchestrator's folder under
  `uploads/` and each path is typed in with the message (`(attached: <path>)`), so the orchestrator can read them.
- **Handoff (#413):** `drover thread handoff <from> <repo|thread> <title> <text>` starts a thread titled `<title>`
  in a repo (or `all`), or reuses an existing thread (`t6`; `<title>` is the handoff row's subject). It logs
  `thread-handoff` `{from, to, title, text}`, then types `[<to>] <from>: <text>` into the target's orchestrator (started or
  resumed, as with `say`). The source shows `continued in <to>` and the target `from <from>` with the text. While the
  link exists, the target's filed issues (`gh issue create`: `filed #<n>: <title>`, or the title alone) and answered
  thread asks (`answered <id>: <question> → <answer>`) each post one line back into the source. Any orchestrator may
  hand off to any repo; an orchestrator's plain `thread new` refuses `USE_HANDOFF`, so every thread an orchestrator starts is
  logged with its source. The owner's `thread new` (CLI or board) is unchanged.
- **Commands:** the lane hook logs each `drover` or `gh` command the orchestrator runs (`thread-cmd`, not `thread
  reply`); the board shows them as closed tool rows.

## The Threads tab (#1072)

**Threads** is the last tab of the standalone page only (`[queue] [events] [lanes] [threads·N]`; `cooee-plugin.toml`
has no Threads place, so the Cooee plugin never shows it), N the threads where a choice waits. It is a JSON screen
like the others (`docs/design/threads-tab/`): `/screens/threads` (the list), `/screens/threads/<id>` (a thread, opened
in the same tab, its cards place hidden) and `/screens/threads/new/<repo>`. Its taps post `/action` `thread-new`,
`thread-say`, `thread-retry`, `thread-archive`, `thread-restore` (the `POST /thread` ops) and `answer`. Files, queued
messages, config and secret cards come with #1086–#1088.

- **Archive (#405):** the thread's orchestrator stops (its pane closes; the folder, uploads and log stay) and the
  thread folds under "N archived"; `restore` brings it back and its next message resumes the same session. CLI:
  `drover thread archive|restore <thread>` (logged `thread-archive` / `thread-restore`). Nothing is deleted.
- **Writes** are `POST /thread` (`op` = `new`, `say` with an optional base64 file of at most 10 MB, `answer`,
  `read`, `retry`; #407 `queue`, `unqueue`, `steer`, `stop`) from a paired device, 10 a minute. #403: `new` and `say`
  answer once the thread or message is logged; starting or resuming the orchestrator and typing the line run after
  the reply. A failed delivery is a red system line with `[retry]` (`retry` types the same line again, logging no copy).

**Push** (a `webpush` notifier, default `on`): an orchestrator's reply is `<repo> · <thread title>: <its first line>`, a
choice `<repo> · <thread title> asks: <question>`. The owner's messages, commands and system lines never push.

Events: `thread-new`, `thread-msg` (`files`; #413 `via`: a flow-back line), `thread-handoff`, `thread-cmd`, `thread-read`, #407 `thread-queue` (`text`, `files`, `q`: the one an edit replaces) and `thread-unqueue` (`q`, why: sent, removed), `orchestrator-start` (session, resumed),
`orchestrator-sleep` (why: idle, gone, signed out); #387: `config-propose`, `config-settled`,
`secret-request`, `secret-set`, `secret-dismissed`, `secret-rm` (names only).

## Config changes (#387)

`drover config propose <thread> <file>`: `<file>` is the whole new fleet.toml (the orchestrator edits a copy). It is
validated like fleet.toml itself, and its diff shows in the thread as a card with **[apply] [discard]**. Only the
owner applies it: a tap on a paired device, or `drover config apply|discard <id>` outside a lane (the lane hook
refuses both inside one, `OWNER_ONLY`). Applying keeps the old file as `fleet.toml.bak-<time>`, writes the
new one and runs `doctor`. A failing `doctor` does not roll back: the line says `doctor ✗` and the backup is there
to restore. If fleet.toml changed since the proposal, it refuses `STALE`, and the card shows `stale: fleet.toml
changed since` instead of its buttons (#460). #1028: the proposed file is kept whole beside the event log
(`proposals/<id>.toml`, its sha256 in the event); a proposal not stored whole refuses `PROPOSAL_TRUNCATED`: propose it again. Settled, the card folds to the thread's system line, e.g. `config c1
applied by owner, backup kept, doctor ✓` (the orchestrator gets it too). #460: while a card waits, the thread's bar and
its row in the list say `config c1 waiting` (a tap on the bar's scrolls to the card), and the thread's first page
always carries it: past the last 30 messages it stays right under the older-messages marker.

## Capability groups and run cards (#538)

- **Groups:** `[orchestrator] allow = [...]` switches on owner-level drover verbs for orchestrators; the lane hook allows
  a group's verbs from an orchestrator and refuses them from every other lane (`OWNER_ONLY`), and refuses an off group's
  `GROUP_OFF`. Unset, every group but `pause` and `set` is on. The groups: `design` (`spawn … design`), `clean`
  (`clean --yes`, `--orphans`, `--lane`), `hosts` (`hosts on|off|refresh`), `reclaim`, `runners` (#537's `runners …`), `pause` (`pause`, `resume`), `set`. The brief lists each, on or off. The
  orchestrator never edits `allow`: it proposes the change as a config card the owner applies. Its group verbs show in
  the thread as tool rows with their group (`thread-cmd`'s `group`).
- **Run cards:** `drover card post <thread> --cmd "<command>" --why "…" --blast "…" --dir <folder> [--timeout
  600]` posts the exact command, why, what it touches and where it runs (the main host, that folder) as card
  `r<n>` (`run-card`, with the command's sha). The owner settles it: `drover card approve <id> [--sha <prefix>]`
  or `card deny <id>` (owner-only, `OWNER_ONLY` in any lane), or the card's Telegram buttons. An approval covers the
  exact text (`CARD_CHANGED` if the sha the owner saw differs), runs once (`ALREADY_SETTLED`) and only within 15
  min of the post (`EXPIRED`). drover runs it with `sh` in the folder under the timeout, then posts `run r1
  approved by owner, ran in <dir>: exit <code>` and the output's last 20 lines (secrets masked) to the thread.
  Events: `run-card`, `run-settled` (`by: owner`, id, sha, approved), `run-done` (code, tail).
- **Hard nos** (`CARD_FORBIDDEN`, at posting and, from an orchestrator's own shell, in the hook): printing or sending a
  secret's value, deleting production data, force-pushing or rewriting history, turning off branch protection or
  drover's gates. It is a pattern guard over the command's words, best effort like `deny_commands`.

## Sign-in (#923)

The orchestrator runs on the host's own login, like every lane ([accounts](accounts.md)). When the awake orchestrator
shows a sign-in prompt, it sleeps and the thread gets a `<harness> is signed out: [sign in]` line; the button opens
#303's setup terminal on the main host.

## Secrets (#387)

- `drover secret request <thread> <NAME> --why "…"` shows a masked-input card with [save] [dismiss], on paired
  owner devices only (elsewhere the card says so).
- [save] writes the value to the main host's 0600 file `<config dir>/secrets/<NAME>` (#707, docs/providers.md), never
  in argv. Events, the chat and lanes never see it. The thread records
  only `secret <NAME> set ✓ (by owner)`.
- One file per name, shared by every fleet: there are no per-fleet items (#707).
- A fleet's `env` refers to it as `KEY = "secret:<NAME>"`. At spawn drover delivers it through a 0600 file the
  pane's shell sources, deleted after the start (docs/providers.md).
- `drover secret ls` and `drover secret rm <fleet|all> <NAME>` are owner-only.

## Telegram

There is no Telegram bridge: the page and web push are the owner UI. A `[telegram]` table in fleet.toml loads with a
warning (`drover doctor` shows a `!` row); drop it.

