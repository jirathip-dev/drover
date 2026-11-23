# Fast-first landing (#350)

```toml
[fleet.app.land]
mode = "fast-first"                 # default "full-first": today's rule
fast = ["lint", "unit"]             # existing CI check names that count as fast
full_first_paths = ["db/migrations/**", ".github/workflows/**"]
```

- **Landing.** A PASS at the exact head lands once every `fast` check is green there, without the `merge_label` full
  run. A PR touching `full_first_paths` lands on the full suite (`required_checks`, else all), as before.
- **Swap-if-unchanged.** It lands only while integration is still in its head (`--match-head-commit` plus an ancestor
  check). If integration moved, merge refuses `BEHIND` (no block); drive catches the PR up and its fast checks run again.
- **After landing.** Every landing logs `landed {mode, sha}`. Each drive step reads the full suite on integration's tip:
  - **Green:** logs `land-green` and resumes landings.
  - **Red:** a runner/infra red (cancelled, timed out, never started) is re-run once. Otherwise fast-first landings
    pause (`land-paused`), and the culprit is found:
    - one landing since the last green is the culprit;
    - several are bisected: each landing commit's own run, else one dispatched on a `drover-bisect-<sha>` ref.
  - **Revert.** A revert PR opens and lands once its fast checks are green (`reverted`). The issue reopens in a fix
    round, with the failed job's log as its finding.
  - **Full-first culprit.** It is never reverted: it gets the fix round only (`land-fix`).
- **Alerts.** `land-paused` and `reverted` go to every notifier of the fleet, whatever its `on` says.
- **Numbers.** `drover stats land <fleet>` prints both modes' landings, per day, median PASS → landed, reverts and check
  time, plus the minutes integration sat red.
- **Promotion.** `main` stays human-only; promote a staging commit only once its full suite is green.
