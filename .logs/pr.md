Closes #662

## For humans
When an AI lane's session dies mid-work because its account hit its usage limit or its adapter crashed, herdsman no longer gives up and hands the issue to you.
It first pushes the commits the lane already made to the issue branch, then restarts the lane in the same folder on the next free account, telling it to carry on from there.
The "lane is gone" alert and the waiting list now say why the lane ended (account limit, adapter crash, killed, or ended normally).
Only lanes that end normally without pushing count toward "lane keeps ending without a push".

## What changed
- `shed.rs`: shed's exit/kill events record a `cause` on the `gone` lane-state, worked out from the lane's last output (kept in `shed-out.json` beside the log), whether a turn was open, and the exit code. An account-limit exit also caps the lane's account (`account-capped`). New `exit_cause` and `RESUMABLE` (account limit, adapter crash).
- `check.rs`: `lane-dead` carries `cause`, and its reason names it.
- `waiting.rs`: the wait reads `lane gone (<cause>)`. Resumable respawns are not counted toward "keeps ending without a push".
- `drive.rs`: before respawning a lane gone from a resumable cause, drive pushes its local commits (`HEAD:refs/heads/<branch>`, never the integration branch, main or master; dirty files are not pushed) and logs `lane-pushed`. The repair path treats `lane gone (<cause>)` like `lane gone`.
- `spawn.rs`: a resumed lane in its own worktree, on the same machine, skips the wip save so staged changes stay staged. Its brief gets a "Resume:" note saying its commits and staged changes are kept and pushed.
- `events.rs`: `lane-pushed` is passive.

src/ net +163 (budget n/a): cause recording, push-before-resume and the respawn count each need their own small hook.

Decided: when all accounts are capped, the push still happens in drive's respawn step before spawn is refused, so the commits are safe either way.

## Repro
Commit 1 (`7e254a2d1`) holds only the regression tests.

```
cargo test -p herdsman --test shed -- acp_lane lane_dead_and
```
On base + commit 1 (`7e254a2d1`), exit 101:
```
test lane_dead_and_waiting_carry_an_acp_lanes_exit_cause ... FAILED
test an_acp_lane_that_exits_on_an_account_limit_is_pushed_and_resumed_in_its_worktree_on_another_account ... FAILED
test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 15 filtered out
```
At head: see Evidence.

## Evidence
EVIDENCE_PENDING
