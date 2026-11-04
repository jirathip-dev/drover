# fleet.toml: serial groups (#294)

A serial group is a set of paths only `max` issues at a time may change. Each fleet may have any number; each group has
its own limit and its own Queue mark (🔒name).

```toml
[[fleet.app.serial]]
name = "db"                    # ≤ 8 chars, also the Queue mark
paths = ["supabase/migrations/**"]
max = 10                       # default 1

[[fleet.app.serial]]
name = "ci"
paths = [".github/workflows/ci.yml", "scripts/lib/ci*", "scripts/check-*"]
max = 1
```

The old form, `serial_paths` / `serial_max` / `serial_label`, still works: it is one group named `serial_label` (unset:
`db`). Giving both forms in one fleet is a config error; `herdsman doctor` suggests the new form.

## Joining

An issue is in a group when

- it is **marked**: `herdsman queue add N --serial ci`, or `queue set N serial ci,db` (replaces its marks). A plain
  `--serial` means the fleet's first group, as before; or
- its **pushed diff** touches one of the group's paths: drive checks each open PR and logs
  `serial-joined {group, by: "diff"}`.

`herdsman merge` still refuses `SERIAL_UNMARKED ci: PR touches scripts/check-x.mjs` for a PR touching a group it never
joined (belt and braces).

## Slots

An issue holds a slot in a group from joining until it is merged, closed or parked (in flight: an impl/fix lane, at
review or ready).

- A marked issue starts only with a free slot: `SERIAL_BUSY ci: #2968 holds 1/1`.
- An issue whose diff joins a full group **holds** (`serial-held {group, holder}`): its lane is told
  `stop: ci is held by #2968; you'll get an update round when it merges`, its worktree stays and no review is spawned.
  Its card reads `waits: ci held by #2968 (1/1)`.
- When a slot frees, the first held issue in queue order takes it (`serial-slot {group}`) and gets an update round
  (merge `origin/<integration>`), then carries on.
- An issue in two or more groups takes their slots in name order and needs all of them before it continues, so two
  issues never wait on each other.
- Issues already in flight over a limit when a group is added keep going (grandfathered); new joins wait.

Board, status and the Host tab show each group's use: `serial: db 3/10 · ci 1/1`.

## Fast-first landing (#350)

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
    - several are bisected: each landing commit's own run, else one dispatched on a `herdsman-bisect-<sha>` ref.
  - **Revert.** A revert PR opens and lands once its fast checks are green (`reverted`). The issue reopens in a fix
    round, with the failed job's log as its finding.
  - **Full-first culprit.** It is never reverted: it gets the fix round only (`land-fix`).
- **Alerts.** `land-paused` and `reverted` go to every notifier of the fleet, whatever its `on` says.
- **Numbers.** `herdsman stats land <fleet>` prints both modes' landings, per day, median PASS → landed, reverts and check
  time, plus the minutes integration sat red.
- **Promotion.** `main` stays human-only; promote a staging commit only once its full suite is green.
