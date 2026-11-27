# Orchestrator chat (#355, #1571)

Drover runs no chat of its own (#1773, the cut of `docs/architecture/cooee-threads/`). A chat lives in a client:
cooee's Orchestrator agent, or a terminal session. The client attaches the chat to a repo, `all` or `none`, runs the
agent on the returned profile, and the agent drives drover through its CLI, so its asks, cards and queued issues
carry its chat id (`DROVER_THREAD`).

```sh
drover attach acme [ID]          # a terminal chat: logs the attach, prints the brief path, settings path and env lines
drover thread handoff <from> <repo|chat> <title> <text>
```

`POST /api/v1/orchestrator-attach {fleet, thread}` does the same for a client; `-detach` ends the link. The owner
answers every ask and card on the board, `drover board` and the page, chat or no chat.

## Old drover threads

The threads drover ran before #1773 stay in the log as history. `drover check` gives each still-open one a closing
`system` row (`closing: true`: attach its id in a client to carry on), ends its orchestrator's session once and logs
`thread-archive` with `by: migration`; an archived thread is skipped, so it runs once. `GET
/api/v1/thread/<id>/rows` still serves a thread's rows by id. Attach takes an archived thread's id: the chat that
adopts it gets the issues that thread filed or queued (their rows route to it). The thread uploads folder stays on
disk; deleting it is an owner run card. The Lanes tab shows no orchestrator rows.

## The orchestrator role

- **Role `orchestrator`, any harness:** `[fleet.<n>.role.orchestrator]` takes the usual role keys (`harness`, `model`,
  `effort`, `mcp`). An `all` chat uses the first fleet that defines one. Without one (#401), a fleet's orchestrator is
  its `impl` role on profile `lane-orchestrator` (which `profiles sync` writes); only a fleet with neither refuses
  `NO_ORCHESTRATOR`. `drover spawn … orchestrator` refuses `BAD_VALUE`.
- **[config] → Orchestrator** shows the role in effect (`default: impl` or `set`) with tappable harness, model and
  effort choices. A tap from a paired device writes `[fleet.<n>.role.orchestrator]` in place (the first one copies the
  impl role's keys), after saving the old file as `fleet.toml.bak`; it is refused `DOCTOR` when `drover doctor` with
  the new file shows a ✗ it did not show before. `[chat]` is read by nothing since #1773 (a client picks its own model).
- **Its folder:** `<state dir>/orchestrator/<chat>/` (or `<[orchestrator] dir>/<chat>/`): `drover attach` writes the
  brief there, and the lane hook lets the agent write in it.
- **Its brief** is `roles/orchestrator/role.md` and `chat.md`, the chat and repo, then the `[orchestrator]` profile:

  ```toml
  [orchestrator]
  dir = "~/orchestrator"                            # optional: the chats' folders' root (default <state dir>/orchestrator)
  instructions = "~/notes/orchestrator/AGENTS.md"   # inlined
  memory = "~/notes/orchestrator/memory"            # each *.md inlined
  skills = "~/notes/orchestrator/skills"            # listed, read on demand
  allow = ["design", "clean", "hosts"]           # #538: capability groups (default: all but pause and set)
  name = "Conductor"                               # #780: what you call it (default "Orchestrator"), labels only

  [orchestrator.ada]                               # #780: more orchestrators, each a name and its fleets
  name = "Ada"
  fleets = ["acme-api", "all"]
  ```

- **Old names (#780, one release):** `[conductor]`, `role.conductor`, `--by conductor-<thread>`, a lane's
  `DROVER_LANE` role `conductor` and the old `<state dir>/conductor/` folder are read as the new names, and so are
  the log's old rows. New writes use the new names only.

## Asks and answers

- **Choices:** the agent runs `drover ask <fleet> - "<question>" --thread <chat> --option … --recommend N`. Every
  surface shows it with its options, the recommended one first, and an "other" box. The decision row carries
  `DECISION <id>: <text>` as its prompt, which the client types in. `--thread` with an unknown chat refuses
  `NO_SUCH_THREAD`.
- **Lane asks (#541):** an `ask` from an impl, rev, fix or design lane (no `--thread`) goes to its repo's attached
  terminal chat (`[fleet.<n>] ask_thread`, else the most recently active one), logged `routed` (no push) with a
  `system` row whose prompt the client types in; it stays open on the board. With no chat, the owner gets it. The
  agent answers with `drover answer <fleet> <id> <n> --basis "<quote | q<id>>"`, logged `by: orchestrator-<chat>`,
  only if the ask is a lane ask on its repo, names no owner topic, and the basis is 3+ words found verbatim in the
  issue's spec (Amendments included) or an answered decision id; else `REFUSED OWNER_ASK` and the ask goes to the
  owner as a choice row (`ask-owner`) with the agent's pick recommended. Each such answer posts `answered <id> for
  #<n>: <choice>, because <basis>` with [change] for 30 min (no push).
- **Amendments (#574):** the spec-amending verb (orchestrators and the owner, `--basis q<id>`) appends
  `- **Amendment (<YYYY-MM-DD>)**: <text>` to the issue body's `## Amendments` section and re-runs the open PR's
  failed spec-reading checks. `q<id>` must be an ask on that issue answered `by: owner` (or a routine ask's
  `by: default`), else `REFUSED NO_DECISION`.
- **Handoff (#413, #1610):** `drover thread handoff <from> <repo|chat> <title> <text>` from an attached chat: to
  another attached chat, a row whose prompt is `[<to>] <from>: <text>`; to a repo (or `all`), an open card on that
  repo's desk, with no thread made. [move] (`POST handoff-move`) sends it on to an attached chat and marks the first
  one forwarded. While the link exists, the target's filed issues and answered asks each post one line back to the
  source.

Events: `thread-msg` (an owner line a client copies through `said`, a `system` row), `thread-handoff` (`desk`),
`orchestrator-attached`, `orchestrator-detached`; history only: `thread-new`, `thread-archive`, `orchestrator-start`,
`orchestrator-sleep`; #387: `config-propose`, `config-settled`, `secret-request`, `secret-set`, `secret-dismissed`,
`secret-rm` (names only).

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

## Rollouts (#1479)

A feature that adds a fleet.toml key does nothing until the config turns it on. Its PR body carries a `## Rollout`
section: the exact fleet.toml lines in a fenced block and which fleets they go in (`none`, or no section, rolls out
nothing). `drover merge` of such a PR logs `rollout-open` once and posts one row to the chat whose orchestrator
filed the issue (the lane hook logs `issue-filed` on its `gh issue create`), else the repo's newest attached chat:
`#<n> merged with a rollout, not applied yet:` with the snippet. That wakes the thread's orchestrator, which drafts the
card at once with `drover config propose <thread> <file> --rollout <n>` (refused `NO_SUCH_ROLLOUT` unless #n's
rollout is open), so the owner only taps [apply]. The apply logs `rollout-done` for #n and its line ends
`, rollout #<n> done`. The owner may instead run `drover rollout dismiss <fleet> <n> [--by <who>]` (owner only; the
lane hook refuses it, `OWNER_ONLY`), logged `rollout-dismissed` with `by`. Until either, `drover waiting` (and
`--json`) lists it as the owner's: `<fleet> #<n> owner: rollout not applied (thread <t>) (<age>)`, its lane the
thread's orchestrator. Nothing is ever applied by itself.

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

