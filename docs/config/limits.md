# Limits (#844)

Every timing and cap drover core uses is a `[limits]` key; a fleet.toml without one keeps the default below.
`drover config show` prints each key with its current value and this doc line, and CI's timing check
(`scripts/timing-check.sh`) fails a hard-coded `Duration` in `crates/core/src` outside `config.rs` and tests.
This file is `drover config show --md`'s output: `just gen` writes it.

| key | default | what it sets |
| --- | --- | --- |
| `review_rounds` | `3` | review rounds before an issue goes needs-human (ROUND_LIMIT) |
| `update_rounds` | `3` | PASSed update rounds before `spawn --update` refuses UPDATE_LIMIT |
| `stall_minutes` | `30` | a working lane with no event this long is stalled |
| `silent_minutes` | `10` | an ACP lane's open turn with no session event this long is cancelled and nudged |
| `ci_start_minutes` | `10` | a PR head with no checks this long after its push alerts `ci-missing` |
| `ci_stuck_minutes` | `30` | a ready PR's check still pending this long after its run completed is re-run once, then alerts `ci-stuck` |
| `min_cpu_idle` | `15.0` | spawn refuses HOST_BUSY under this percent of the CPUs idle over a 1 s sample; `[hosts.<n>]` may set its own |
| `overload_minutes` | `10` | CPU idle under `min_cpu_idle` this long alerts `host-overloaded` once, with the 5 busiest processes |
| `swapout_rate` | `2000.0` | pages swapped out per 2 s sample that count as swapping; `[hosts.<n>]` may set its own |
| `min_free_gb` | `10.0` | spawn refuses DISK_LOW under this many GB free on `/` |
| `worktree_budget_gb` | `25.0` | over this many GB of worktrees drive frees build folders, then spawn refuses DISK_BUDGET |
| `build_budget_gb` | `20.0` | over this many GB of lane build folders drive trims their unlocked profiles |
| `sweep_remote` | `true` | the scratch sweep runs on every enabled remote host too |
| `stray_roots` | `["/tmp", "$TMPDIR"]` | where `drover clean` looks for stray build folders and temp dirs |
| `min_free_mem_gb` | `2.0` | spawn refuses MEM_LOW under this many GB of free + inactive memory; `[hosts.<n>]` may set its own |
| `effort` | `unset` | default reasoning effort for roles that set none |
| `ask_default_minutes` | `30` | a routine ask unanswered this long takes its recommended option |
| `ask_expire_hours` | `24` | an ask no live lane waits on expires after this many hours |
| `thread_resend_minutes` | `5` | a new-thread send repeating a request id this soon gets the thread it made |
| `idle_reap_minutes` | `10` | a lane done or idle this long loses the processes its agent started |
| `done_close_minutes` | `5` | a done lane handed off this long closes its session |
| `idle_merged_minutes` | `15` | a merged or closed issue's idle lane has its pane closed after this long |
| `transcript_days` | `30` | a lane's agent keeps its transcripts this many days |
| `deny_commands` | `assets/deny_commands.toml` | commands the lane hook refuses (default: containers) |
| `fast_secs` | `60` | the fast job (`drive`) starts every fleet's pass every this many seconds |
| `slow_secs` | `300` | the slow tick runs every this many seconds |
| `form_categories` | `["trailer", "stale-baseline", "docs-fresh", "migration-redate", "infra"]` | the finding tags a `FAIL (form-only)` may carry |
| `form_rounds` | `3` | form-only FAILs per issue that don't count as a review round |
| `overflow_after_minutes` | `10` | every local host refused this long lets placement start a stopped cloud host |
| `import_max_age_hours` | `24` | a remote host's asks older than this are never imported |
| `max_pushes_per_pass` | `3` | push alerts `check` sends a pass before a summary |
| `max_parallel_spawns` | `6` | concurrent detached spawn kicks on the main host (0: in the pass) |
| `start_max` | `2` | lanes starting at once on a remote host that sets no `start_max` |
| `start_release` | `"started"` | a start frees its slot once its harness is up (`started`) or its prompt is confirmed (`confirmed`) |
| `spawn_pending_max_secs` | `300` | a spawn marker older than this is stale |
| `admit_wait_secs` | `5` | a spawn's admission waits this long for another admission on its host before `SPAWN_PENDING` |
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
| `pass_max_secs` | `60` | a fleet's pass ends itself here, `pass-slow` naming its step; a lock held past 2 × it logs `pass-stuck` and is taken over |
| `ssh_connect_secs` | `10` | every host ssh's `-oConnectTimeout`; the whole `ssh <host> true` probe's retry and `hosts refresh`'s probe |
| `ssh_quick_secs` | `5` | a maintenance ssh's connect timeout and the `ssh <host> true` probe's; the whole first probe |
| `ssh_call_secs` | `20` | a remote call in a pass is killed after this long |
| `shed_call_secs` | `30` | a call to this host's shed server with no answer this long (past a `wait`'s own timeout) fails instead of blocking |
| `ssh_sessions` | `8` | remote calls open at once to one host over its shared connection; more wait their turn (under sshd's `MaxSessions`) |
| `ssh_retry_secs` | `1` | a call the shared connection refused (`Session open refused by peer`) runs again on it after this long, up to 3 times |
| `link_beat_secs` | `5` | a host's watcher with nothing to say writes a beat this often |
| `link_stale_secs` | `20` | no frame from a host's link for this long: it is cut and dialled again |
| `link_retry_max_secs` | `30` | the longest wait between a link's dials (1 s, doubling) |
| `link_poll_millis` | `500` | how often a watcher looks at each outbox, verdict.md and shed session for a change |
| `link_sample_secs` | `30` | a watcher sends a whole report this often, changed or not |
| `link_down_minutes` | `5` | a host with lanes whose link is not live this long alerts `link-down` |
| `db_busy_secs` | `60` | the SQLite busy timeout |
| `db_retry_millis` | `10` | the pause between a new database's WAL switch retries |
| `cloud_idle_stop_minutes` | `20` | a cloud host's `idle_stop_minutes` when it sets none |
| `notify_secs` | `15` | a notifier command or web push send is killed after this long |
| `gate_check_minutes` | `30` | a premerge or gate check (a build or test run) is killed after this long |
| `shed_read_millis` | `300` | how long drive's intake waits for shed's state changes |
| `proxy_connect_secs` | `1` | the token proxy's liveness connect timeout |
| `proxy_retry_secs` | `300` | a proxy lane's sign-out holds its host only if no call gets through the token proxy this long after it |
| `service_call_secs` | `2` | a call to the running service's socket times out after this long |
| `disk_probe_secs` | `2` | a folder listing slower than this on a host over `min_cpu_idle` marks its disk as not answering |
| `disk_hung_secs` | `6` | under `min_cpu_idle` a call waits this long for a slow folder listing, then the disk is not answering |
| `disk_sick_secs` | `300` | a disk that let a call run out of time stays hung this long |
| `disk_trust_secs` | `60` | a folder that answered is trusted this long before it is probed again |
| `lane_read_secs` | `5` | a lane worktree read slower than this skips the lane for the pass (`root-hung`) |
| `guard_secs` | `180` | `turn done`'s UI check (`just guard`) runs this long at most; past it the turn ends and `ui-check` logs a timeout |
| `poll_millis` | `20` | the pause between polls of a child process or disk probe |
| `hold_sample_secs` | `10` | `drover hold` at the front of the heavy line samples the host's admission this often |
| `tick_beat_secs` | `60` | a running tick writes `tick.beat` this often |
| `orchestrator_idle_minutes` | `30` | an orchestrator whose thread is quiet this long closes on the next `check` |
| `orchestrator_reply_minutes` | `10` | an owner message with no orchestrator reply this long gets a `no reply yet` row |
| `orchestrator_change_minutes` | `30` | an orchestrator's answer may not change one this long after it |
| `token_refresh_minutes` | `30` | `check` pushes a fresh runner registration token to every VM this often |
| `ask_waiting_minutes` | `30` | an owner ask open this long alerts `ask-waiting` |
| `outbox_stale_minutes` | `5` | a remote lane's outbox line undelivered this long alerts `outbox-stale` |
| `host_offline_minutes` | `15` | a remote host unreachable this long has its open lanes restarted on the main host |
| `host_down_secs` | `60` | a host a pass could not reach gets no ssh from any drover command this long |
| `kick_max_secs` | `300` | a kick running this long is ended by the next tick and logged `kick-killed` (a merge's: `gate_check_minutes`) |
| `job_max` | `4` | issue jobs of one fleet running at once |
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
| `slo_tick_minutes` | `10` | a tick with no progress (no step or call) for this long breaches `slo-tick`; the next fast job stops it |
| `slo_pass_minutes` | `5` | no finished pass of a fleet for this long stops that fleet's pass (never another's) and shows on the board's drive-health line |
| `stall_restart_hours` | `2` | an issue with no push, verdict or lane event and no named wait this long restarts once (a load wait moves host); again: a stuck card (a fleet may set its own) |
| `self_update_wait_minutes` | `45` | a head with no release build this long alerts `self-update-waiting` |
| `self_update_stage_secs` | `300` | one host's download and gate of a release build |
| `act_minutes` | `10` | a flow row drive owns must have acted within this long (#1649) |
| `escalate_minutes` | `30` | a flow row's card stays with the orchestrator thread this long before the owner gets it (#1649) |
| `start_minutes` | `10` | a flow row's check lane that ended with no verdict gets this long (#1649) |
| `check_quiet_minutes` | `120` | a flow row's quiet check lane gets this long (#1649) |
| `host_wait_minutes` | `30` | a flow row's loaded host, or one refusing every start, gets this long (#1649) |
| `card_ttl_minutes` | `15` | a flow row's run card waits this long for approval (#1649) |
