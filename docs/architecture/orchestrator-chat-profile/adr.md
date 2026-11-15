# ADR: each orchestrator served to cooee as a chat profile (#1057)

## Summary

Decision: drover serves one cooee profile per orchestrator, named by its orchestrator id. Its instructions file is
rebuilt on each read from the thread brief's parts, with a chat part in place of the thread part; the chat runs under
the orchestrator hook and is linked to drover when it starts.
Slices: the role text split in three parts; the profile list and the chat link in core; two cooee routes with tests.
Open questions: (1) approve a board-fix line on #1057 for three lines in the frozen board router file? (2) run cards,
config proposals and secret requests refuse a cooee chat today: a follow-up issue (my pick), or built here?

Status: proposed (architect lane, 2026-11-15). Read [README.md](README.md) first, then this, [shapes.md](shapes.md),
[boundaries.md](boundaries.md).

## Question

The orchestrator's text lives twice: drover's `[orchestrator] instructions` file and the cooee persona's own
`character` and `instructions`. q1703 made drover's file the one source. How does drover hand it to cooee as a profile,
so that (a) the text is never copied by hand, (b) a cooee chat is guarded exactly as an orchestrator lane is, and
(c) each orchestrator has a stable profile id? The issue leaves three calls to this ADR: the brief's parts, the guard
identity of a chat that is no drover thread, and the id scheme.

## Where it stands (staging `60ff82fb7`, cooee staging `9280915d6`)

- `profile()` in `crates/core/src/orchestrator.rs` builds a thread's `.brief.md`: the whole role prompt, the thread
  lines, then the owner's instructions, memory, capability groups and skills. The usage doc reaches a lane through its
  lane profile's system prompt, not the brief.
- #633's `attach()` already answers what a cooee chat needs (brief, settings, env, guarded or why) and logs
  `orchestrator-attached`, but through `POST /api/v1/orchestrator-attach`, which cooee core never calls.
- cooee fetches `GET /cooee/profiles` as its sidebar loads and again inside `POST /threads`, then reads the profile's
  `instructions` file from disk, sets `settings` in the provider's settings variable (for its one guarded provider
  that is a config folder holding `settings.json`), swaps `{thread}` in `env`, and posts `/cooee/thread-started`
  `{thread, profile}`. It refuses a `hidden` profile and unknown profile keys.
- The live persona runs Plain today: its own text, and no hook at all.
- drover answers neither `/cooee/` route; the router lives in the board-frozen `crates/server/src/serve.rs`.

## Options

**Keeping the file fresh**

1. **Write it on every `GET /cooee/profiles` (chosen).** cooee asks right before it reads the file, so an edit to the
   owner's file is in the next chat with no restart, no watcher and no new cooee field.
2. Point `instructions` at the owner's file. No role text, memory, groups or skills: not "the same parts".
3. Write it at `drover profiles sync`. Stale until someone syncs; fails the no-restart test.
4. Serve the text in the JSON. cooee's `Profile` refuses unknown keys; needs a cooee format change.

**The brief's parts**

1. Send the thread brief as it is. It tells the chat to `thread reply` and relay `--typed` lines, which a cooee thread
   is refused.
2. **Split the role prompt into three files: role, thread, chat (chosen).** A thread gets role + thread, a chat gets
   role + chat; each sentence has one home.
3. Markers inside the one file. Same result, but a parser and a way to get it wrong.

**The guard identity**

1. Lane and orchestrator id only, no thread. Nothing to hang an ask on: the chat could not ask the owner through
   drover.
2. A full #633 attach. Lane asks of that fleet would be routed to the newest cooee chat, where nothing types them in
   and no push goes out: they would sit until `ask-waiting`.
3. **A link for the chat's own asks only (chosen).** The same event, marked `via: profile`; `route()` skips it.

**The profile id**

1. **The orchestrator id (chosen):** `default`, or the `[orchestrator.<id>]` key; cooee shows `drover/<id>`.
2. One per orchestrator and fleet (`<id>-<fleet>`). More entries than personas; the issue asks for one each.
3. A slug of the name. Renaming the orchestrator would orphan the persona's `profile` line.

## Decision

- **Routes.** `GET /cooee/profiles` and `POST /cooee/thread-started`, plugin token only (401 without), in a new server
  file beside `screens.rs`; the router sends every `/cooee/` path there, so a later cooee route never edits the router
  again. Profiles never answers 404 (cooee reads that as "use the manifest's").
- **One profile per orchestrator**, `name` its id, `label` its name. Ids are checked at config load against cooee's
  name rule.
- **The seat.** A chat's lane identity is `DROVER_LANE=<fleet>:-:orchestrator`, `DROVER_ORCHESTRATOR=<id>`,
  `DROVER_THREAD={thread}`. The fleet is the first of the orchestrator's fleets with an orchestrator role, by the rule
  an all-repos thread already uses. The hook, its rules and `acting()` read these exactly as for a lane: no new hook
  code.
- **The guard.** `settings` is the lane fleet's orchestrator lane profile, shaped for cooee by a new provider method.
  The same `unguarded()` proof `attach()` runs decides `hidden` and `why`: an unsynced profile, or a provider with no
  ACP agent, is listed but cannot start. `policy` is the fleet's `acp_policy`, as for drover's own ACP lanes.
- **The text.** `<state dir>/chat/<id>.md`, rewritten on each profiles request when it changed: role part, chat part,
  usage doc, seat lines, then `standing(cfg)`, the same call the thread brief ends with. The thread brief's text does
  not change.
- **The link.** `thread-started` logs `orchestrator-attached` on the lane fleet with the orchestrator id and
  `via: profile`, once per thread. It makes `ask --thread`, `answer --basis` and `extend --basis` work for the chat;
  lane asks keep going where they go today.
- **Nothing else is served**: no account, folder, avatar, MCP, fleet env or port ([shapes.md](shapes.md)).

## Consequences

- One source: the owner edits one file; the next thread brief and the next cooee chat both carry it. The persona
  keeps name, avatar and fleets, and names `drover/<id>`.
- A cooee chat has an orchestrator lane's refusals from its first tool call (the env alone arms the hook) and fewer
  verbs than a drover thread, never more: `thread reply|title|show|handoff`, `--typed`, `card post`,
  `config propose` and `secret request` refuse a cooee thread (`NO_SUCH_THREAD`, `NOT_TYPED`), and the chat part
  says so.
- No `DECISION` line reaches the chat: the owner answers on the ask's card, and the chat reads the answer when the
  owner next writes. Typing it in needs a cooee route drover does not have.
- The default orchestrator sits on the first fleet with an orchestrator role; to choose, define an
  `[orchestrator.<id>]` whose `fleets` starts with the fleet wanted.
- For the provider cooee guards today, `settings` is used as the agent's config folder in place of cooee's account,
  so that folder needs its own sign-in once. Unverified here; the PR's live check proves it.
- A profiles request writes files. They are derived, in drover's state folder, and written only on change.
- `POST /api/v1/orchestrator-attach` is left as it is (it shares `link()`); with no caller in cooee core, removing it
  and its fixtures is a follow-up, not this PR.
- The live check needs cooee's paired issue (a persona takes its text from the profile it names) and
  `drover profiles sync` first.

## Open questions for the owner

1. **board-fix.** CI's board-freeze fails any PR that touches `crates/server/src/serve.rs` without a `board-fix`
   line in the issue. This design needs three lines there (the module and one router arm) and no board UI change.
   Recommend: approve, and the orchestrator runs `drover amend` with "board-fix: the router sends /cooee/ paths to
   the plugin backend". There is no path around it: cooee fixes the route's name.
2. **Thread cards in a chat.** Run cards, config proposals and secret requests take only a drover thread today, and
   cooee's cards screen shows no config or secret card. Recommend: a follow-up issue (with the `DECISION` line back),
   so this one stays the profile.

## Hot files

Files this issue touches that 5 or more merged PRs touched in 14 days, and what the design does to each:

| File | PRs | Class | Here |
|---|---|---|---|
| `crates/server/src/serve.rs` | 79 | central list (the router), frozen by board-freeze | +3 once; all `/cooee/` routes then live in the new file |
| `crates/server/src/screens.rs` | 10 | central list (screens, actions) | no entry added; one visibility word |
| `crates/core/src/orchestrator.rs` | 13 | large module | shrinks: the brief moves to its own file, the chat code is a new file |
| `crates/core/src/config.rs` | 83 | validation list | +5 beside the existing orchestrator check |
| `crates/core/src/spawn.rs` | 88 | large module | one match arm changed |
| `roles/orchestrator/` (the old single file) | 5 | docs split per area | becomes a folder of three parts; the file is deleted |
| `docs/serve.md` | 66 | docs split per area | untouched: the routes get their own docs file |
| `crates/cli/tests/cooee_screens.rs` | 8 | tests | untouched: the tests get their own file |

`.drover/arch.toml`: no lines to add. Nothing here is generated, and the one converted file is deleted rather than
kept as a list, so there is nothing to freeze or collect.
