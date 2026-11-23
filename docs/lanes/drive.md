# Drive

Every fleet is driven by drover itself. `drover drive [fleet]` does one pass, from cron after `drover check`
([setup.md](../setup.md)) or the board's **Drive now** (paired devices only). Per issue, at most one step a pass, through the same gates as the CLI:

| State | Step |
| --- | --- |
| queued, unblocked, no lane (every such item, in queue order) | `spawn impl` (start) |
| impl/fix lane idle, head pushed, PR open, not reviewed at that head | `spawn rev` (review) |
| `verdict.md` written since the rev spawn, first line `PASS` or `FAIL`, rev spawned at the PR head | `verdict --sha <head>` |
| the same, but the PR head moved since the rev spawn (its logged `sha`) | `spawn rev --supersede` at the new head (review) |
| `ready`, CI green | `merge`; CI pending: wait |
| `ready`, CI red | `verdict FAIL` at the same head (no new round), the failed check's log tail (secrets masked) as the finding (ci-red) |
| `fix(n)` after a FAIL | `spawn impl --supersede` (fix); the brief carries the findings verbatim |
| `ready` or a pushed fix, PR `CONFLICTING` or `BEHIND` the integration branch | `spawn impl --supersede --update` (conflict): the brief says merge `origin/<integration>` (no rebase, no force-push), resolve keeping both sides, run the gates, push, report the resolved files; a fresh review follows; a PASS counts toward `update_rounds`, a FAIL as a normal round; past `update_rounds` it logs `needs-human UPDATE_LIMIT` |
| `FAIL (form-only)`: every `blocking [<category>]` finding in `form_categories` | a fix round that doesn't count toward `review_rounds` (`fix(n·f)`); a blocker untagged or of another category: a plain FAIL (`form-only-rejected`); past `form_rounds` per issue they count (`form-rounds-exhausted`) |
| FAIL in round `review_rounds` | `verdict` logs `needs-human` with the findings; drive stops |
| a lane killed | `spawn impl` (respawn) |
| a live lane shed has said quiet for `stall_minutes` with no turn-end (#666, #1053) | `spawn <role> --supersede` once (respawn `lane stalled`); stalled again: `needs-human LANE_STALLED` with shed's note |
| a live lane whose pane or session shows no output 10 min after its spawn | an ACP session gets its brief again once; then `spawn <role> --supersede` once (respawn `failed start`); then `needs-human START_FAILED` |
| an ACP lane (build, check or design stage) mid-turn with no session event `silent_minutes` (10) and no tool call still running (#792) | `shed cancel` and one `continue` prompt (`nudged`); still silent 10 min later: `spawn <role> --supersede` once (`respawned-silent`); silent again within the hour: `needs-human LANE_SILENT` |
| a design or architect lane in `error`, idle, hibernated or gone with no commit of its own under its stage's folders (#915, #914) | no `design-review`, no ask; `spawn <role> --supersede` (respawn) like a gone impl lane: 30 min apart, `LANE_ENDS` after two, under `RESPAWN_LOOP` |
| a respawn refused for good (`RESPAWN_LOOP`, `DESIGN_MAIN_ONLY`, `HOST_OFFLINE`…) (#914) | `needs-human RESPAWN_REFUSED` (`RESPAWN_LOOP`: its one owner ask instead); `check` alerts `respawn-refused` once per lane and code |
| a dead, stalled, silent or released lane's respawn (#908) | admitted like any spawn: `HOST_BUSY`, `MEM_LOW` and `DISK_LOW` hold it to a later pass, and `waiting` names the hold (`held: <code> <why>`); at most `[limits] respawn_per_pass` (default 2; `respawn_per_pass` in `[fleet.<n>]` overrides it) start a pass, the rest wait `RESPAWN_PACED`, still ahead of new starts; a crash wave (#1311: 3+ lanes of one host gone by an adapter crash within `[limits] crash_wave_minutes`, default 5) is not paced: its lanes come back in one pass once admission lets them, under one `lane-wave` alert in place of each `lane-dead` push |
| a prompt drive or check sends a lane shed has asleep (`hibernated`: nudge, DECISION, turn, release, heads-up) (#908) | waking it starts a whole agent, so the host is admitted first; busy, the prompt is held and sent on a later pass |
| an ACP lane shed reports `running`, `waiting-approval` or `rate-limited`, or one drover shows `waiting-owner` (its turn ended on an open ask or `turn blocked`/`need-input`, laid over shed's `done`) (#983) | held: no nudge, no stall, no `lane-dead`; the board and `/api/v1` show shed's state and note as-is |
| a `rate-limited` lane (#983) | shown with shed's reset time; nothing until the owner's [wait]: `drover limit <fleet> <issue>` sends one `continue` at that time (`NO_RESET` when shed gave none); logged `limit-picked` `by: owner`, an orchestrator's relay `--typed` with the owner's line, `--by` names who |
| a lane the hook refused twice (`hook-failed-closed`) since its spawn | its pane closed, `needs-human HOOK_REFUSED` with the refusal |
| merged lanes left | `clean --yes` |

**`needs-human` is the owner's** (#130). It pushes to phones (`#96 needs you: review rounds ran out (4/3)`,
then the last verdict's first finding), and the page shows a needs-you card for it on the Queue tab and in the
Herd strip (the issue's horse, or a gate horse when it has no lane, wears a red `!`). A paired phone's buttons:

- **merge anyway** (PASS at the PR head, CI green, mergeable; confirmed) runs
  `drover merge <fleet> <issue> --owner-override`. The flag lifts **only** `ROUND_LIMIT`: an exact-head
  PASS, green CI and the serial gate still apply. The `merged` event records `by: owner, override: "ROUND_LIMIT"`
  and the closing comment says `Owner override: ROUND_LIMIT`.
- **one more round** logs `round-extended` (this issue's limit is `review_rounds` + 1 per extension) and
  puts the issue back in its queue place as `queued` (#1058): the tap spawns nothing, drive starts it in turn
  with `impl --supersede`, and its Queue row reads `extended · waiting: <gate>` (`PAUSED`, `HOST_BUSY`, …) until
  then. The card never comes back for a wait; a hard refusal of the start (`MERGED`, `UPDATE_LIMIT`, an error)
  is a new `needs-human` naming the code. #817: an orchestrator grants it in its repo's thread with
  `drover extend <fleet> <issue> --typed "<owner line>"` (`by: owner (typed)`) or `--basis "<spec quote>"`
  (`by: orchestrator-<thread>`, an FYI line whose [change] parks the issue for 30 min).
- **park** logs `parked` (`by`, and `drover park <fleet> <issue> --why <text>`'s reason): the issue leaves
  `needs-human` and shows as parked; `queue ls` and its Queue row show the reason.
- **unpark** (#712; a parked Queue or Lanes row's button, or `drover unpark <fleet> <issue> [--by]`, owner only)
  logs `unparked` with the state it goes back to: the state before the `needs-human` the park closed (normally
  `queued` or its round). Its queue place is kept; out of the queue it goes last; drive's next pass picks it up.
  Anything but a parked issue is `REFUSED NOT_PARKED`.

This replaces raising `review_rounds` in fleet.toml, merging and restoring it by hand.

Paused fleets are skipped (no pass, no `last pass`), but their live lanes still get delivery: the fast step runs their outbox lines from each host's report and retypes undelivered answers (#714); one fleet's error doesn't skip the others. drive never answers asks (a lane's question stays a needs-you card),
never passes `--force`, and never bypasses a refusal: `REFUSED` (e.g. `HOST_BUSY`, `CAP`,
`SERIAL_BUSY`) is printed and logged once per item an hour (cron stays quiet), and the step is retried
next pass. Starts fan out: a pass tries every startable item in queue order, each spawn sampling the host
afresh. An item-specific refusal (`SERIAL_BUSY`, `MCP_FORBIDDEN`, …) skips that item for the pass; it keeps
its queue place and status reads `skipped this pass: <CODE> (<why>)` (board lane `skip <CODE>`). `CAP` ends
the fleet's starts; `HOST_BUSY` and `LOGIN_NEEDED` end that fleet's starts in the pass. Every action logs a passive `driven` event (step, command, result).
#1320: each fleet's pass is its own process, `drover drive <fleet>`, under its own `drive-<fleet>.lock` lease (2 ×
`[limits] pass_max_secs`; taken over once its pid is gone, or once it runs out: `pass-stuck`, that pid stopped), so cron,
the service and the button never overlap on one fleet and a slow fleet never holds another. A pass ends itself at
`pass_max_secs` (60) and logs `pass-slow` naming the step it was in; it still stamps the fleet's `last pass`, so
only a pass that never ends leaves it stale. #1558: after the closed issues, a pass runs its due steps (verdicts, fix rounds, respawns) before its starts; a step that fails logs its error and the rest still run, and a host is offline to a pass only after a call in that pass failed. The fast job (`drover drive`, every `fast_secs`)
and the tick poll every host with no fresh word, then start each fleet's pass and wait for none; the service runs a
loop per fleet instead, started at once by a wake. The tick's slow chores (disk budget, roots, offline hosts, orphans,
repair, the runtime canary, stale panes, each fleet's closed issues, hotspots and backlog) run beside them. A spawn's admission places it first (host probes, no lock), then recounts that host's slots and takes its marker
under the host's short `admit-<host>.lock` (#1583): two fleets' passes never both take a host's last slot, admissions
on different hosts run at once, and a busy lock is waited for up to `[limits] admit_wait_secs` before `SPAWN_PENDING`. #1625: no in-process wait sits in front of it, so an admission whose pass hung or died holds a host's starts for at most the lock's 60 s; the next admission takes it over and logs `admission-dropped`. A fleet whose last pass is older than 2 × `pass_max_secs` + `fast_secs` gets its pass started as a process even when a service answers the wake, and `drover check` alerts `spawn-stuck` once when drive tried starts for an hour with no spawn while issues are startable. The board shows each fleet's
`⟳ driving…` and its last pass's length.

A fix lane may push back on a finding in its report with file:line proof; that is not a fix,
and the next (fresh) reviewer decides.
