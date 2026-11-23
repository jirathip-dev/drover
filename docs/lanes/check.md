# Check and notifiers

#854: every waiting or next-step line drive prints (`merging next pass`, `waiting for drive (next pass ≤ 5 min) to …`,
`waiting for a spawn slot`) is a promise with a deadline: `[limits] promise_passes` drive passes for its kind
(`next-pass`, `merging`, `spawn-pending`, `waiting-on`; default 2 each, `slow_secs` per pass). Each pass logs a new
one as `promise`; one past its deadline whose issue kept its state and head is logged once as `promise-missed` (a
notifier can subscribe to it) with the reason drive sees then, and `waiting`, `status` and the board show
`⚠ missed <age>: <reason>` in place of the stale line.

#855: the fast step also checks two fleet SLOs: `slo-idle` (no lane working for `[limits] slo_idle_minutes` (20)
while the queue has spawnable work) and `slo-tick` (#1307: a tick with no step or call for `slo_tick_minutes` (10), however long it has run; replacing
`tick-stuck`). On a breach drive first tries the known heal and logs `self-heal`: a drover
tick pid it can name, or the stuck pass of the one fleet with no finished pass for `slo_pass_minutes`, gets SIGTERM,
then SIGKILL after 10 s (never another fleet's); a DISK_BUDGET hold is recounted and the
queue's first spawn retried; drive's own pass already respawns a silent lane and marks a PASSed draft ready.
#1183: a `stuck` (pushed once per fleet, and the owner's one `Drive is stuck` card per fleet) means drive itself is
broken: a tick still hung a pass (`slow_secs`) after its heal, or an issue's spawn failing the same way
twice on the same head. #1502: drive's own health (no finished pass of the fleet for `[limits] slo_pass_minutes`
(5), `slo-idle`, drive's last pass of the fleet failing) is never a card or a push: it is one line at the top of
the board (`drive late: 5 fleets · 34m`, idle and pass-error counts beside it), hidden when every fleet is healthy;
tapping it lists each fleet's reason and age with [Drive now]. A wait (a hold, CI, a spawn slot, HOST_BUSY, `slo-idle`, a missed promise) never makes one:
its row names the wait. `slo-ok` ends the episode once it recovers, and the card clears; an issue's line goes as
soon as its lane starts. The card's hint is `drover waiting <fleet>`, and each issue on it has a [force] that runs
its `drover waiting` next command (a spawn) once with `--force`, past the load limit, logged `override` `by: owner`.

#1216: an in-flight issue with no push, verdict or lane event (`spawn`, `turn-end`, `pushed`) for
`[limits] stall_restart_hours` (2; `stall_restart_hours` in `[fleet.<n>]` overrides it) is restarted once:
`stall-restart` (`by: drover`), then `spawn <role> --supersede` (its wip saved to `wip/`, its lane cleaned, the same
role respawned). A load wait (`HOST_BUSY`, `MEM_LOW`, `DISK_LOW`) that long logs `stall-move` instead: the next
spawn skips that host, whatever its worktree holds, and goes to another enabled host under its limits that fits the
tags, its wip saved first. A named wait never counts: held until #N, CI running or red, a spawn slot, an open ask,
or a `turn working` younger than the window; nor does an issue drive is on (its pass holds the issue, or its
last step since was refused or failed for another reason than load: that refusal or the same-failure card names it). A second stall on the same step, a failed restart, or a move with no
free host is a stuck card. Only changes to tracked files and commits not on origin pin a lane to its host;
untracked files never do (drover's scratch, `body.md` among `OWN_FILES`, is never saved; the rest travel in `wip/`).

`drover check` alerts once per state on `stall`, `lane-dead` (a parked lane, clean and pushed with no
pane, gets one `lane-dead` and never a `stall`), `ready-unmerged`, `ci-missing` (a PASSed PR head with no
checks `[limits] ci_start_minutes` (10) after its push: `CI broken: <workflow> did not start` when a failed run has
no jobs, else `CI missing`; once per head, and drive sends the lane a fix round as for red CI), `respawn-refused`
(#914: a lane respawn refused for good, once per lane and code) and
`usage-limit` / `signed-out` (#923). A limit banner counts only on the harness's own lines (the pane's last block,
unless a `⏺` reply or tool call heads it; prompt lines never); a sign-in prompt is `signed-out`. Either alerts once,
naming the lane, its host and the login command, and holds new lanes on that host (`LOGIN_NEEDED`) until the owner
signs in there and turns the host back on ([accounts](../accounts.md)). It is idempotent and exits 0, so it can run from cron:

```cron
*/5 * * * * drover check >/dev/null 2>>"$HOME/Library/Logs/drover-check.log"
```

Each `[[notify]]` entry gets one plain line on stdin for the events in its `on` list, or
the raw event JSON with `format = "json"`. A fleet's own `[[fleet.<name>.notify]]`
replaces the global list for its events; `notify_global = true` sends to both. Notifiers
run without the caller's `HERMES_*` environment.
