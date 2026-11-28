# Asks

#1813: a lane decides every in-scope choice itself (fix rounds, ordering, already-decided specs, test or tooling
choices inside the issue's scope) and lists it in its PR body under `## Decisions`. `drover ask` is for owner topics
only: scope or feature changes, spend, security or secret trade-offs, anything touching main or production, data
deletion, a new dependency. A lane runs `drover ask <fleet> <issue> "<question>" --option … --recommend <n>` and waits
for a `DECISION <id>: <text>` prompt; the ask goes straight to the owner as a card (the phone gets a push), never to
an orchestrator thread.

- A lane's ask whose question and options use no owner word (`main`, `production`/`prod`, `secret`, `token`,
  `delete`, `spend`, `cost`, `$`, `dependency`, `scope`, or a plural or inflection such as `secrets`, `deleting`) is
  refused `IN_SCOPE` and nothing is logged: decide it, note it under `## Decisions`, carry on. Asks from outside a
  lane (the owner, an orchestrator) are not checked.
- There is no `--kind` any more: `--kind routine` is an unknown option. A routine ask filed before #1813 and still
  open defaults once as it did: after `[limits] ask_default_minutes` (30) `drover check` takes its recommendation,
  `by: "default"`, and the lane gets `DECISION <id>: <option> (default after 30 min, nobody replied)`.
- The #1809 stuck card (the same blocking finding through 2 fix rounds, or CI red on the same check twice) goes to
  the owner as a card, never into a thread.

A defaulted decision shows on its issue row as `decided by default: <option>` with **[change]**: answering again (also
`drover answer`) logs a new `decision` and prompts the lane, or the next lane's brief carries it. A decision
someone made is final (`ALREADY_ANSWERED`).

#584: an ask closes on every real ending, and `drover asks --all` says which: `answered` (a tap, `drover answer`,
or `answer <fleet> <id> --typed "<line>"`, which an orchestrator runs with the owner's exact line from the ask's thread,
logged `by: owner (typed)`; any other line is `NOT_TYPED`; #838: `answer <fleet> <id> <choice> --typed "<line>" --note
"<note>"` names the owner's pick, the line is then evidence only, and an approve's note also goes under the issue's
`## Amendments`; a typed answer is an `amend --basis`), `withdrawn → q<new>` (`drover ask withdraw`, by its own
thread's orchestrator), `delivered to t<n>` or `failed` (a handoff card), `expired`, `closed: merged`, `closed: lane
gone`. `drover check` expires an ask open over `[limits] ask_expire_hours` (24) that nobody waits on: a thread ask
with no lane, a handoff card, an ask whose lane is no live agent. A live lane's ask never expires; `ask-waiting`
repeats for it once a day. The first `check` after #584 sweeps the log once (`asks-swept`, with the count).
