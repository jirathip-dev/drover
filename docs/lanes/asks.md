# Asks

A lane that needs a decision runs `drover ask <fleet> <issue> "<question>" --option … --recommend <n> --kind routine|owner`
and waits for a `DECISION <id>: <text>` prompt.

- **owner** (the default; an unmarked ask is one) always waits for a person. Scope or feature changes, spend,
  security or secret trade-offs, anything touching main or production, data deletion, a new dependency. The
  phone gets a push for each one.
- **routine** needs `--recommend` (else `NO_RECOMMENDATION`). Fix rounds, ordering, size or cap targets already set
  by an issue, already-decided specs, test or tooling choices inside the issue's scope. If nobody answers within
  `[limits] ask_default_minutes` (30), `drover check` takes the recommended option through the same path as
  `drover answer`: a `decision` with `by: "default"`, and the lane gets
  `DECISION <id>: <option> (default after 30 min, nobody replied)`. The phone hears of the default taken, never of
  the routine ask merely waiting, and a routine ask raises no `ask-waiting`.
- A routine ask whose question or options use an owner word (`main`, `production`/`prod`, `secret`, `token`,
  `delete`, `spend`, `cost`, `$`, `dependency`, `scope`, or a plural or inflection such as `secrets`, `deleting`) is
  refused `OWNER_TOPIC`. The list is a backstop, not the definition.

The needs-you card tags each ask `routine · defaults to 2 in 24m` or `owner · waits for you`. A defaulted
decision shows on its issue row as `decided by default: <option>` with **[change]**: answering again (also
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
