# Monitoring

- `drover hosts` prints, per host: on or off, reachable or offline, its tags, CPU idle, memory, disk, and its lanes.
- `drover status` gets a `HOST` column once fleet.toml has `[hosts]`; #609: it shows a live lane's `where`
  (`mini · shed`, docs/serve.md), as `board` and `waiting` do.
- `GET /api/v1/host`'s `hosts` has one card per host, main first, then A→Z, each with the same rows (#234): cpu idle,
  swap-out, memory, disk, top memory, lanes and tags, the state and why its dot is amber or red in its head. Remote
  values (and top memory, with its age) come from the 5-minute `check`. An unreachable host is grey
  (`offline · last seen 33m`) in the same layout, and its lanes never page (`lane-dead` and `stall` skip them).
- #478: while a host is unreachable, a spawn onto it is refused `HOST_OFFLINE` (other hosts' spawns go on), its lanes
  show `host offline: <host>` in `status`, `board` and `waiting` (drive neither respawns nor supersedes them), and
  `check` logs `host-offline` once when it drops and `host-online` once when it is back. A supersede that could not
  close the old pane there logs it (`stale_host`, `stale_pane`); drive kills it once the host answers (wip first, as
  `kill` does) and logs `stale-pane-killed`. `drover kill|close --host <name> <agent>` picks that host's agent when
  another host has a lane of the same name.
- #1654: each host's frame carries its own load sample (percent of its CPUs idle, memory pressure). A host is `loaded` only by its
  own sample; a late frame keeps the host's last word while it or the main host is loaded; no frame for
  `host_offline_minutes` (15) is `offline` at any load, and only then are its lanes restarted on the main host
  (`lane-moved`). `drover waiting --flow` prints, under each issue's line, its flow state, each fact's value with its
  evidence, and the table row (`docs/flow.md`) that condition matches. Intake logs `verdict-seen` once per new
  `verdict.md` a frame brings; `drover verdict` logs `verdict-refused` (code, head, why) for a file it refuses.
- #938: a runtime breaker per host. `[limits] runtime_down_failures` (5) lanes there failing to start or ending
  without a push within `runtime_down_minutes` (30) make drive log one `runtime-down` (host, count, a sample reason;
  one alert). While it is open a spawn there is refused `RUNTIME_DOWN`, those lanes show `runtime down on <host> since
  <HH:MM> UTC` in `status` and `waiting`, and drive neither respawns them nor parks them needs-human. Each slow pass
  runs one canary there (a throwaway shed session echoing a computed word);
  once it passes drive logs `runtime-up`, the held issues' earlier respawns stop counting, and spawns resume paced by
  `max_parallel_spawns`.
- History (#234): `<state dir>/host-history.jsonl` keeps 24 h of `{ts, host, load_cpu, pressure, swap_out, mem_free_gb,
  disk_free_gb, lanes}`: the main host a row a minute (`drive`), a remote host a row every 5 minutes (`check`'s
  survey, no extra SSH). Rows past 24 h are dropped when it is rewritten (at most hourly), and it stays under
  about 180 KB a host. Each card row carries its 24 h series for a sparkline. Downsampling keeps the worst row of each bucket (the max; free space the
  min), so a peak is never hidden.
- Watched sampling (#352): only while a `/live?tab=host` stream is open (it stamps `<state dir>/host-watch.at`;
  seen in the last 2 min) does the main host's row also carry `procs`: the MB `ps` sees
  in five fixed groups (lanes, CI/Colima, browsers, drover, other), never per PID. With nobody watching the fast
  step never walks the process table. The file is also capped at 1,600 rows a host on rewrite.
- Each lane row shows its host name.
