/// Every `[limits]` key and what it sets, in docs/config/limits.md's order; `drover config show` prints them.
pub const LIMIT_DOCS: &[(&str, &str)] = &[
    ("stall_minutes", "a working lane with no event this long is stalled"),
    (
        "silent_minutes",
        "an ACP lane's open turn with no session event this long is cancelled and nudged",
    ),
    (
        "ci_start_minutes",
        "a PR head with no checks this long after its push alerts `ci-missing`",
    ),
    (
        "ci_stuck_minutes",
        "a ready PR's check still pending this long after its run completed is re-run once, then alerts `ci-stuck`",
    ),
    (
        "ci_flaky_retries",
        "re-runs of a failed check's jobs a head gets before its red counts, `ci-rerun` why failed (0 = off; a fleet may set its own)",
    ),
    (
        "min_cpu_idle",
        "spawn refuses HOST_BUSY under this percent of the CPUs idle over a 1 s sample; `[hosts.<n>]` may set its own",
    ),
    (
        "overload_minutes",
        "CPU idle under `min_cpu_idle` this long alerts `host-overloaded` once, with the 5 busiest processes",
    ),
    (
        "swapout_rate",
        "pages swapped out per 2 s sample that count as swapping; `[hosts.<n>]` may set its own",
    ),
    ("min_free_gb", "spawn refuses DISK_LOW under this many GB free on `/`"),
    (
        "worktree_budget_gb",
        "over this many GB of worktrees drive frees build folders, then spawn refuses DISK_BUDGET",
    ),
    (
        "build_budget_gb",
        "over this many GB of lane build folders drive trims their unlocked profiles",
    ),
    (
        "sweep_remote",
        "the scratch sweep runs on every enabled remote host too",
    ),
    (
        "stray_roots",
        "where `drover clean` looks for stray build folders and temp dirs",
    ),
    (
        "min_free_mem_gb",
        "spawn refuses MEM_LOW under this many GB of free + inactive memory; `[hosts.<n>]` may set its own",
    ),
    ("effort", "default reasoning effort for roles that set none"),
    (
        "ask_default_minutes",
        "a routine ask unanswered this long takes its recommended option",
    ),
    (
        "ask_expire_hours",
        "an ask no live lane waits on expires after this many hours",
    ),
    (
        "thread_resend_minutes",
        "a new-thread send repeating a request id this soon gets the thread it made",
    ),
    (
        "idle_reap_minutes",
        "a lane done or idle this long loses the processes its agent started",
    ),
    (
        "done_close_minutes",
        "a done lane handed off this long closes its session",
    ),
    (
        "idle_merged_minutes",
        "a merged or closed issue's idle lane has its pane closed after this long",
    ),
    ("transcript_days", "a lane's agent keeps its transcripts this many days"),
    ("deny_commands", "commands the lane hook refuses (default: containers)"),
    (
        "fast_secs",
        "the fast job (`drive`) starts every fleet's pass every this many seconds",
    ),
    ("slow_secs", "the slow tick runs every this many seconds"),
    ("form_categories", "the finding tags a `FAIL (form-only)` may carry"),
    (
        "form_rounds",
        "form-only FAILs per issue that don't count as a review round",
    ),
    (
        "import_max_age_hours",
        "a remote host's asks older than this are never imported",
    ),
    (
        "max_pushes_per_pass",
        "push alerts `check` sends a pass before a summary",
    ),
    (
        "max_parallel_spawns",
        "concurrent detached spawn kicks on the main host (0: in the pass)",
    ),
    (
        "max_in_flight_total",
        "PRs in flight across every fleet before a first start is refused `IN_FLIGHT_CAP` (0: off)",
    ),
    (
        "fleet_priority",
        "fleets in the order a freed `max_in_flight_total` slot goes to, oldest queue entry first; unlisted fleets after",
    ),
    (
        "start_max",
        "lanes starting at once on a remote host that sets no `start_max`",
    ),
    (
        "start_release",
        "a start frees its slot once its harness is up (`started`) or its prompt is confirmed (`confirmed`)",
    ),
    (
        "spawn_pending_max_secs",
        "a spawn marker older than this is stale",
    ),
    (
        "acp_start_minutes",
        "an ACP lane with no event back this long after its first turn is a failed start",
    ),
    (
        "webhook_poll_minutes",
        "with GitHub webhooks on, the fast step re-reads PRs only this often",
    ),
    (
        "keychain_readers",
        "more Keychain readers up over 60 s alert `keychain-jammed`",
    ),
    ("reap_keychain_readers", "kill those stuck Keychain readers too"),
    (
        "respawn_max",
        "failed starts of an issue's role in the window before spawn refuses RESPAWN_LOOP",
    ),
    (
        "respawn_window_minutes",
        "the window `respawn_max` counts failed starts in",
    ),
    (
        "respawn_per_pass",
        "respawns a drive pass starts, each through admission (a fleet may set its own)",
    ),
    (
        "crash_wave_minutes",
        "3+ lanes of one host dying by an adapter crash within it are one `lane-wave`: respawned together, one alert",
    ),
    (
        "runtime_down_failures",
        "lanes on one host failing to start or ending without a push in the window before drive trips `runtime-down` there",
    ),
    (
        "runtime_down_minutes",
        "the window `runtime_down_failures` counts in",
    ),
    (
        "pass_max_secs",
        "a fleet's pass ends itself here, `pass-slow` naming its step; a lock held past 2 × it logs `pass-stuck` and is taken over",
    ),
    (
        "ssh_connect_secs",
        "every host ssh's `-oConnectTimeout`; the whole `ssh <host> true` probe's retry and `hosts refresh`'s probe",
    ),
    (
        "ssh_quick_secs",
        "a maintenance ssh's connect timeout and the `ssh <host> true` probe's; the whole first probe",
    ),
    ("ssh_call_secs", "a remote call in a pass is killed after this long"),
    (
        "shed_call_secs",
        "a call to this host's shed server with no answer this long (past a `wait`'s own timeout) fails instead of blocking",
    ),
    (
        "ssh_sessions",
        "remote calls open at once to one host over its shared connection; more wait their turn (under sshd's `MaxSessions`)",
    ),
    (
        "ssh_retry_secs",
        "a call the shared connection refused (`Session open refused by peer`) runs again on it after this long, up to 3 times",
    ),
    ("link_beat_secs", "a host's watcher with nothing to say writes a beat this often"),
    ("link_stale_secs", "no frame from a host's link for this long: it is cut and dialled again"),
    ("link_retry_max_secs", "the longest wait between a link's dials (1 s, doubling)"),
    ("link_poll_millis", "how often a watcher looks at each outbox, verdict.md and shed session for a change"),
    ("link_sample_secs", "a watcher sends a whole report this often, changed or not"),
    ("link_down_minutes", "a host with lanes whose link is not live this long alerts `link-down`"),
    ("db_busy_secs", "the SQLite busy timeout"),
    (
        "db_retry_millis",
        "the pause between a new database's WAL switch retries",
    ),
    (
        "notify_secs",
        "a notifier command or web push send is killed after this long",
    ),
    (
        "gate_check_minutes",
        "a premerge or gate check (a build or test run) is killed after this long",
    ),
    (
        "shed_read_millis",
        "how long drive's intake waits for shed's state changes",
    ),
    (
        "service_call_secs",
        "a call to the running service's socket times out after this long",
    ),
    (
        "disk_probe_secs",
        "a folder listing slower than this on a host over `min_cpu_idle` marks its disk as not answering",
    ),
    (
        "disk_hung_secs",
        "under `min_cpu_idle` a call waits this long for a slow folder listing, then the disk is not answering",
    ),
    (
        "disk_sick_secs",
        "a disk that let a call run out of time stays hung this long",
    ),
    (
        "disk_trust_secs",
        "a folder that answered is trusted this long before it is probed again",
    ),
    (
        "lane_read_secs",
        "a lane worktree read slower than this skips the lane for the pass (`root-hung`)",
    ),
    (
        "guard_secs",
        "`turn done`'s UI check (`just guard`) runs this long at most; past it the turn ends and `ui-check` logs a timeout",
    ),
    (
        "poll_millis",
        "the pause between polls of a child process or disk probe",
    ),
    (
        "hold_sample_secs",
        "`drover hold` at the front of the heavy line samples the host's admission this often",
    ),
    ("tick_beat_secs", "a running tick writes `tick.beat` this often"),
    (
        "orchestrator_idle_minutes",
        "an orchestrator whose thread is quiet this long closes on the next `check`",
    ),
    (
        "orchestrator_reply_minutes",
        "an owner message with no orchestrator reply this long gets a `no reply yet` row",
    ),
    (
        "orchestrator_change_minutes",
        "an orchestrator's answer may not change one this long after it",
    ),
    (
        "ask_waiting_minutes",
        "an owner ask open this long alerts `ask-waiting`",
    ),
    (
        "outbox_stale_minutes",
        "a remote lane's outbox line undelivered this long alerts `outbox-stale`",
    ),
    (
        "host_offline_minutes",
        "a remote host unreachable this long has its open lanes restarted on the main host",
    ),
    (
        "host_down_secs",
        "a host a pass could not reach gets no ssh from any drover command this long",
    ),
    (
        "kick_max_secs",
        "a kick running this long is ended by the next tick and logged `kick-killed` (a merge's: `gate_check_minutes`)",
    ),
    ("job_max", "issue jobs of one fleet running at once"),
    (
        "promise_passes",
        "drive passes a promise of each kind may stay open before `promise-missed` (default 2)",
    ),
    (
        "serve_shed_secs",
        "serve re-reads shed's lanes in the background this often",
    ),
    (
        "serve_remote_secs",
        "serve re-reads the remote hosts (ssh) in the background this often",
    ),
    (
        "serve_host_secs",
        "serve re-samples this host's load in the background this often",
    ),
    (
        "serve_heartbeat_secs",
        "`/live` pings this often; a stream whose last 2 pings are still unsent is dropped",
    ),
    (
        "serve_handover_secs",
        "how long serve waits for a held port's serve to answer, hand over, or let go of the port (#1244)",
    ),
    (
        "serve_mem_mb",
        "serve over this much RSS logs `serve-mem-high` once and hands over to a fresh copy of itself",
    ),
    (
        "slo_api_p95_ms",
        "`/api/v1` p95 over this many ms for `slo_api_minutes` alerts `slo-api` once",
    ),
    (
        "slo_api_minutes",
        "how long `/api/v1` p95 must stay over `slo_api_p95_ms` before `slo-api`",
    ),
    (
        "slo_idle_minutes",
        "no lane working this long while the queue has spawnable work breaches `slo-idle`",
    ),
    (
        "slo_tick_minutes",
        "a tick with no progress (no step or call) for this long breaches `slo-tick`; the next fast job stops it",
    ),
    (
        "slo_pass_minutes",
        "no finished pass of a fleet for this long stops that fleet's pass (never another's) and shows on the board's drive-health line",
    ),
    (
        "stall_restart_hours",
        "an issue with no push, verdict or lane event and no named wait this long restarts once (a load wait moves host); again: a stuck card (a fleet may set its own)",
    ),
    (
        "self_update_wait_minutes",
        "a head with no release build this long alerts `self-update-waiting`",
    ),
    ("self_update_stage_secs", "one host's download and gate of a release build"),
    ("act_minutes", "a flow row drive owns must have acted within this long (#1649)"),
    (
        "escalate_minutes",
        "a flow row's card stays with the orchestrator thread this long before the owner gets it (#1649)",
    ),
    ("start_minutes", "a flow row's check lane that ended with no verdict gets this long (#1649)"),
    ("check_quiet_minutes", "a flow row's quiet check lane gets this long (#1649)"),
    (
        "host_wait_minutes",
        "a flow row's loaded host, or one refusing every start, gets this long (#1649)",
    ),
    ("card_ttl_minutes", "a flow row's run card waits this long for approval (#1649)"),
];
