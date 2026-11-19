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
`db`). Giving both forms in one fleet is a config error; `drover doctor` suggests the new form.

## Joining

An issue is in a group when

- it is **marked**: `drover queue add N --serial ci`, or `queue set N serial ci,db` (replaces its marks). A plain
  `--serial` means the fleet's first group, as before; or
- its **pushed diff** touches one of the group's paths: drive checks each open PR and logs
  `serial-joined {group, by: "diff"}`.

`drover merge` still refuses `SERIAL_UNMARKED ci: PR touches scripts/check-x.mjs` for a PR touching a group it never
joined (belt and braces).

## Slots

An issue holds a slot in a group from joining until it is merged, closed or parked (in flight: an impl/fix lane, at
review or ready).

- A slot gates only new impl work: a marked issue with no open PR starts only with a free slot:
  `SERIAL_BUSY ci: #2968 holds 1/1`. A blocked issue holds no slot.
- A pushed head is never held (#1136): an open PR whose diff joins a full group is reviewed, verified, fixed and merged
  like any other, and counts toward the group's use. A conflict at merge gets an update round as usual.
- Issues already in flight over a limit when a group is added keep going (grandfathered).

`status` and `board` show each group's use: `serial: db 3/10 · ci 1/1`.

Hot files (#891): each file the last stored hotspots list names is its own one-slot group, `hot <file>`, joined
only by a PR's diff (never a `--serial` mark). Drive drops a file from it on its next pass once
`.drover/arch.toml` on the integration branch lists it `frozen`, `generated` or `shared` (#1147; `hot-files {files}`
logs each change).

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
    - several are bisected: each landing commit's own run, else one dispatched on a `drover-bisect-<sha>` ref.
  - **Revert.** A revert PR opens and lands once its fast checks are green (`reverted`). The issue reopens in a fix
    round, with the failed job's log as its finding.
  - **Full-first culprit.** It is never reverted: it gets the fix round only (`land-fix`).
- **Alerts.** `land-paused` and `reverted` go to every notifier of the fleet, whatever its `on` says.
- **Numbers.** `drover stats land <fleet>` prints both modes' landings, per day, median PASS → landed, reverts and check
  time, plus the minutes integration sat red.
- **Promotion.** `main` stays human-only; promote a staging commit only once its full suite is green.

## Renaming a fleet (#935)

`drover fleet rename <old> <new>` is a dry run that prints every change it would make. With `--yes` it applies them.
It is owner only: the lane hook refuses it, so it runs as a run card's `--cmd`. It refuses `NOT_PAUSED` unless `<old>`
is paused, `LANE_LIVE` while a lane of an open issue in `<old>` is running, and `BAD_NAME` when `<new>` is taken or
not a valid name. First it backs up state.db, fleet.toml and the per-fleet state files. Then, in one transaction, it
moves the queue, events, asks, verdicts and secrets (their `secret-set` events) to `<new>`, renames the state files
and thread repo bindings, and renames the `[fleet.<old>*]` tables with every value unchanged. Any failure restores
the backup. It logs one `fleet-renamed` event, and old names keep resolving in `status`, `asks`, `board` and
`queue ls`. The fleet stays paused until the owner resumes it.

## Merge label (#947)

`merge_label` (unset by default) is the PR label the full CI needs. drover adds it on a PASS at the head and removes it
when the head moves past that PASS. `drover setup` creates it in the repo, and `drover doctor` checks that it exists.
New fleets set `merge_label = "merge-ready"` with the affected-only CI recipe in [ci.md](ci.md).

## Hotspots (#889)

`drover hotspots [FLEET]` (every fleet when none is named) lists the files 1 in 20 of the merged PRs (at least 5)
touched in the last 14 days on the integration branch; a merge touching more than 40 files is a sweep and not counted
(#1147), and the report's header says how many were skipped, any language, each with the distinct PR pairs it was predicted to conflict in
(#973: pairs, not one per push) and the update rounds of the issues that touched it (drover's log, every fleet on the
same repo). A file no longer at the integration tip, or one `.drover/arch.toml` lists frozen, generated or shared, is
left out. It keeps the list in the state folder as
`hotspots-<fleet>.json`. With any hot file it opens `hotspots: <fleet> week of <monday>`, or updates that issue while it
is open, and queues it `--architect`. `drover tick` stores each fleet's list daily and files its issue weekly; `hotspots = "off"` in `[fleet.<n>]` opts
out.

## History baseline (#1099)

`drover baseline <fleet> [<sha>]` (the owner's; default the integration tip) writes `baseline = "<sha>"` in
`[fleet.<n>]` and logs `baseline-set`; a sha not on the integration branch is `REFUSED BASELINE_OFF_INTEGRATION`.
Lanes whose branch contains it see `git log` and `git blame` only since it, by local grafts; nothing is rewritten.
Remove the key to undo it. How it works and the other undos: `docs/workflow.md`, "History baseline"; the design:
`docs/architecture/baseline/`.

## In-flight cap (#1129)

`max_in_flight = N` in `[fleet.<n>]` caps the fleet's PRs in flight (an issue in impl, fix, review, verify or ready,
or one held with an open PR); unset is 8, `0` is off. At the cap drive starts no new issue but keeps reviewing, fixing
and merging; `drover spawn` of a new issue is refused `IN_FLIGHT_CAP` unless `--force`.

## Hygiene (#1101)

One history and issue convention is on for every fleet with no config. `[hygiene]` sets it for all fleets,
`[fleet.<n>.hygiene]` per fleet (its keys win); `"off"` opts a key out.

| key | default | what |
|---|---|---|
| `merge` | `"squash"` | the merge method `[fleet.<n>] merge` and the repo should use; also `"merge"` |
| `comments` | `"strict"` | `.drover/arch.toml`'s `[comments] policy`; the rule ships in `rules/shared.md` |
| `baseline` | `"setup"` | the arch-check baseline, set at fleet setup, redone on the owner's ask |
| `triage` | `"weekly"` | a weekly triage lane |
| `docs_trust` | `["docs/archive/"]` | paths a new lane worktree leaves out (sparse checkout); `[]` = off |
| `issue_spec` | `"triage"` | triage folds amendments into `## Spec (current)` |
| `docs` | `"on"` | `.drover/arch.toml` has the #735 `[docs]` stale-doc check |

`drover setup` applies the set to each fleet's local clone, writing only what is missing: `[comments] policy = "strict"`
and a `[docs]` section (paths `docs/**/*.md` and `README.md`, the repo's `cli`, `config` and `api` files as `watch`) in
`.drover/arch.toml`, then the first arch-check baseline when `baseline = "setup"` and none exists. Commit what it wrote.

`drover doctor` prints each fleet's `hygiene` line and a red `! hygiene` row for each drift: the fleet's own
`merge` mode (#1097) differing from it, the repo on GitHub not allowing that method, or `comments = "strict"` with integration's `.drover/arch.toml` saying otherwise, or `docs = "on"` with no `[docs]` there.

## Limits (#844)

Every timing and cap drover core uses is a `[limits]` key; a fleet.toml without one keeps the default below.
`drover config show` prints each key with its current value and this doc line, and CI's timing check
(`scripts/timing-check.sh`) fails a hard-coded `Duration` in `crates/core/src` outside `config.rs` and tests.

| key | default | what it sets |
| --- | --- | --- |
| `review_rounds` | `3` | review rounds before an issue goes needs-human (ROUND_LIMIT) |
| `update_rounds` | `3` | PASSed update rounds before `spawn --update` refuses UPDATE_LIMIT |
| `stall_minutes` | `30` | a working lane with no event this long is stalled |
| `silent_minutes` | `10` | an ACP lane's open turn with no session event this long is cancelled and nudged |
| `ci_start_minutes` | `10` | a PR head with no checks this long after its push alerts `ci-missing` |
| `load_per_cpu` | `2.5` | spawn refuses HOST_BUSY over this load per CPU |
| `overload_minutes` | `10` | load per CPU over `load_per_cpu` this long alerts `host-overloaded` once, with the 5 busiest processes |
| `swapout_rate` | `2000.0` | pages swapped out per 2 s sample that count as swapping |
| `min_free_gb` | `10.0` | spawn refuses DISK_LOW under this many GB free on `/` |
| `worktree_budget_gb` | `25.0` | over this many GB of worktrees drive frees build folders, then spawn refuses DISK_BUDGET |
| `build_budget_gb` | `20.0` | over this many GB of lane build folders drive trims their unlocked profiles |
| `sweep_remote` | `true` | the scratch sweep runs on every enabled remote host too |
| `stray_roots` | `["/tmp", "$TMPDIR"]` | where `drover clean` looks for stray build folders and temp dirs |
| `min_free_mem_gb` | `2.0` | spawn refuses MEM_LOW under this many GB of free + inactive memory |
| `effort` | `unset` | default reasoning effort for roles that set none |
| `ask_default_minutes` | `30` | a routine ask unanswered this long takes its recommended option |
| `ask_expire_hours` | `24` | an ask no live lane waits on expires after this many hours |
| `idle_reap_minutes` | `10` | a lane done or idle this long loses the processes its agent started |
| `done_close_minutes` | `5` | a done lane handed off this long closes its session |
| `idle_merged_minutes` | `15` | a merged or closed issue's idle lane has its pane closed after this long |
| `transcript_days` | `30` | a lane's agent keeps its transcripts this many days |
| `deny_commands` | `assets/deny_commands.toml` | commands the lane hook refuses (default: containers) |
| `fast_secs` | `60` | the fast step (`drive --fast`) runs every this many seconds |
| `slow_secs` | `300` | the slow tick runs every this many seconds |
| `form_categories` | `["trailer", "pr-body", "stale-baseline", "format", "docs-fresh", "migration-redate", "infra"]` | the finding tags a `FAIL (form-only)` may carry |
| `form_rounds` | `3` | form-only FAILs per issue that don't count as a review round |
| `overflow_after_minutes` | `10` | every local host refused this long lets placement start a stopped cloud host |
| `import_max_age_hours` | `24` | a remote host's asks older than this are never imported |
| `max_pushes_per_pass` | `3` | push alerts `check` sends a pass before a summary |
| `max_parallel_spawns` | `6` | concurrent detached spawn kicks on the main host (0: in the pass) |
| `start_max` | `2` | lanes starting at once on a remote host that sets no `start_max` |
| `start_release` | `"started"` | a start frees its slot once its harness is up (`started`) or its prompt is confirmed (`confirmed`) |
| `spawn_pending_max_secs` | `300` | a spawn marker or kick lock older than this is stale |
| `acp_start_minutes` | `3` | an ACP lane with no event back this long after its first turn is a failed start |
| `webhook_poll_minutes` | `10` | with GitHub webhooks on, the fast step re-reads PRs only this often |
| `keychain_readers` | `3` | more Keychain readers up over 60 s alert `keychain-jammed` |
| `reap_keychain_readers` | `false` | kill those stuck Keychain readers too |
| `respawn_max` | `3` | failed starts of an issue's role in the window before spawn refuses RESPAWN_LOOP |
| `respawn_window_minutes` | `60` | the window `respawn_max` counts failed starts in |
| `respawn_per_pass` | `2` | respawns a drive pass starts, each through admission (a fleet may set its own) |
| `crash_wave_minutes` | `5` | 3+ lanes of one host dying by an adapter crash within it are one `lane-wave`: respawned together, one alert |
| `runtime_down_failures` | `5` | lanes on one host failing to start or ending without a push in the window before drive trips `runtime-down` there |
| `runtime_down_minutes` | `30` | the window `runtime_down_failures` counts in |
| `pass_max_secs` | `120` | a drive pass longer than this logs `pass-slow`; past 3 × it logs `pass-stuck` and frees its lock |
| `ssh_connect_secs` | `10` | every host ssh's `-oConnectTimeout` |
| `ssh_quick_secs` | `5` | a maintenance ssh's connect timeout and each pass's `ssh <host> true` probe |
| `ssh_call_secs` | `20` | a remote call in a pass is killed after this long |
| `db_busy_secs` | `60` | the SQLite busy timeout |
| `db_retry_millis` | `10` | the pause between a new database's WAL switch retries |
| `cloud_idle_stop_minutes` | `20` | a cloud host's `idle_stop_minutes` when it sets none |
| `notify_secs` | `15` | a notifier command or web push send is killed after this long |
| `gate_check_minutes` | `30` | a premerge or gate check (a build or test run) is killed after this long |
| `shed_read_millis` | `300` | how long drive's intake waits for shed's state changes |
| `proxy_connect_secs` | `1` | the token proxy's liveness connect timeout |
| `proxy_retry_secs` | `300` | a proxy lane's sign-out holds its host only if no call gets through the token proxy this long after it |
| `service_call_secs` | `2` | a call to the running service's socket times out after this long |
| `disk_probe_secs` | `2` | a folder listing slower than this marks its disk as not answering |
| `disk_sick_secs` | `300` | a disk that let a call run out of time stays hung this long |
| `disk_trust_secs` | `60` | a folder that answered is trusted this long before it is probed again |
| `lane_read_secs` | `5` | a lane worktree read slower than this skips the lane for the pass (`root-hung`) |
| `poll_millis` | `20` | the pause between polls of a child process or disk probe |
| `tick_beat_secs` | `60` | a running tick writes `tick.beat` this often |
| `orchestrator_idle_minutes` | `30` | an orchestrator whose thread is quiet this long closes on the next `check` |
| `orchestrator_reply_minutes` | `10` | an owner message with no orchestrator reply this long gets a `no reply yet` row |
| `orchestrator_change_minutes` | `30` | an orchestrator's answer may not change one this long after it |
| `token_refresh_minutes` | `30` | `check` pushes a fresh runner registration token to every VM this often |
| `ask_waiting_minutes` | `30` | an owner ask open this long alerts `ask-waiting` |
| `outbox_stale_minutes` | `5` | a remote lane's outbox line undelivered this long alerts `outbox-stale` |
| `host_offline_minutes` | `15` | a remote host unreachable this long has its open lanes restarted on the main host |
| `promise_passes` | `{}` | drive passes a promise of each kind may stay open before `promise-missed` (default 2) |
| `serve_shed_secs` | `5` | serve re-reads shed's lanes in the background this often |
| `serve_remote_secs` | `10` | serve re-reads the remote hosts (ssh) in the background this often |
| `serve_host_secs` | `10` | serve re-samples this host's load in the background this often |
| `serve_heartbeat_secs` | `15` | `/live` pings this often; a stream whose last 2 pings are still unsent is dropped |
| `serve_handover_secs` | `300` | how long serve waits for a held port's serve to answer, hand over, or let go of the port (#1244) |
| `serve_mem_mb` | `300` | serve over this much RSS logs `serve-mem-high` once and hands over to a fresh copy of itself |
| `slo_api_p95_ms` | `1000` | `/api/v1` p95 over this many ms for `slo_api_minutes` alerts `slo-api` once |
| `slo_api_minutes` | `10` | how long `/api/v1` p95 must stay over `slo_api_p95_ms` before `slo-api` |
| `slo_idle_minutes` | `20` | no lane working this long while the queue has spawnable work breaches `slo-idle` |
| `slo_tick_minutes` | `10` | a tick with no progress (no step or call) for this long breaches `slo-tick`; the next fast step stops it |
| `slo_pass_minutes` | `30` | no slow drive pass of a fleet for this long shows the owner's `Drive is stuck` card |
| `stall_restart_hours` | `2` | an issue with no push, verdict or lane event and no named wait this long restarts once (a load wait moves host); again: a stuck card (a fleet may set its own) |
| `self_update_wait_minutes` | `45` | a head with no release build this long alerts `self-update-waiting` |
| `self_update_lag_minutes` | `30` | how long a rollout waits for an enabled host that is not staged before the rest swap |
| `self_update_stage_secs` | `300` | one host's download and gate of a release build |
| `self_update_swap_secs` | `120` | how long a `self-update-swapping` mark holds remote placement when no `self-updated` follows |
