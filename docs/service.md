# `drover service` (#345, Phase C)

ADR, decided by the owner in #345's q355 (2026-11-05): "Build the full service now … Approved: the tokio dependency
and the cap raise … Shape: API-first." This lifts the "no daemon / no async" rule for one process and one
dependency, `tokio`, in its own crate (`crates/service`); every other crate stays synchronous.

## Shape

- **One process**, `drover service --api <addr> [--board <addr>]`, kept alive by launchd (`KeepAlive` after a failed exit) or systemd
  (`Restart=on-failure`). A second one refuses to start. It logs `service-started`; a stall logs `service-stalled`, which
  every notifier gets, and the process exits so the supervisor starts it again.
- **Local socket** `service.sock` beside `state.db` (mode 0600). A kick (a lane's push, stop or ask, a verdict, an
  answer, a board tap) goes there and is driven in the service's process at once, not as a detached `kick --now`.
  With no service listening, every command goes on in today's direct mode: nothing is ever stuck on it.
- **A loop per fleet** runs `drover drive <fleet>` as its own process every `fast_secs`, and at once on a wake (a
  `{"wake": <fleet>}` line, or a kick for one of its issues); a wake while its pass runs is one more pass after it.
- **GitHub** is polled by the fast job (`drive`) every 15 s inside the service, which replaced the `dev.drover.fast`
  job (#1802). The slow `tick` stays as the safety net: it skips a fleet the live service drove within `slow_secs`. #530: each fleet's open PRs, check rollups and labels come in one GraphQL query; a
  fast step first sends three conditional REST reads (open PRs latest-updated first, Actions runs, integration's
  branch) with their last ETags, and when all three answer 304 (free of rate limit) it keeps the last query's
  result. It queries anyway 5 min after the last query, or while a PR's mergeability is still UNKNOWN.
- **API v1** (the contract the board and the future cooee plugin use):
  - `GET /api/v1/health`: pid, version, busy jobs, heartbeat age.
  - `GET /api/v1/events?after=<seq>`: Server-Sent Events, the log after `seq` (the last 500), then each new event as
    it is logged; this host or a paired device's cookie only.
- **The board** runs in the same process with `--board` (the `serve` job's address); it is not grown here.
- **Links** (#1319, #1309 step 2): one thread per host, this one included, holds `drover hosts watch` open there
  (over ssh to a boat; a child process here). The watcher sends a whole report first, a new one within
  `link_poll_millis` of any outbox line, `verdict.md` or shed session change (and every `link_sample_secs`), and a
  beat every `link_beat_secs`. Each frame goes through the same intake as the fast step's poll: the view, the
  `lane-state` stamps, and each outbox line run once as its lane (a `turn done` kicks its issue at once), then
  acked on the watcher's stdin (`<worktree>\t<sent>`) so later reports leave it out. A host whose link is live is
  not polled. No frame for `link_stale_secs`, a frame out of order or a closed pipe cuts the link; it is dialled
  again after 1 s, doubling to `link_retry_max_secs`, and its first report brings the view level. `drover check`
  alerts `link-down` for a host with lanes not live for `link_down_minutes` while the service runs, `link-up` once
  it is live again; the `links` services row says how many are live. With the service stopped every host is
  polled, as before.

## Remote lanes

A lane on another host appends its commands to its worktree's outbox (#389, `DROVER_OUTBOX`); the host's link
delivers each line within `link_poll_millis` and acks it (Links, above). It holds no key, token or route to the main
host (#1322 removed the HTTP path, owner decision q2026). The main host keeps controlling other hosts over ssh
(spawn, read, kill).

## Robustness

Every job runs on a blocking thread in one of 16 slots with a timeout (a kick or poll 900 s);
a timeout logs `service-timeout` and frees its slot, so a stuck job never blocks the others. At most 64 API
connections, 64 KiB a request head or socket line. A watchdog thread exits the process when the event loop's
heartbeat is 30 s late.

A timed-out kick frees its issue's spawn slot (the marker's pid is the service's own, which outlives the kick).

Killed mid-spawn (`kill -9`), the service leaves nothing that blocks: its spawn marker and kick lock name its pid, so
they are stale at once; the CLI goes on in direct mode. The
next service starts over the dead one's socket, the next pass sweeps the marker and spawns again
(`crates/cli/tests/service.rs`).

## Install

`drover timer install` writes and loads `dev.drover.service` (`drover service --api 127.0.0.1:7779`, off the
board's 7777 and the webhooks' 7778, #1867) beside the tick, then unloads and removes the old `dev.drover.fast` job
(#1802). `drover doctor` shows `✗ service — not running` while the job is missing or its socket is dead (naming the
job or pid that holds the API port, with its fix), and `✗ fast job — replaced by the service` while both are
installed.

## Line cap

The service's budget is the planned +3,000, inside the 31,500 cap the owner set in q391 (#514).

## Deferred (q409)

The owner decided in #345's q409 (2026-11-06): "Defer (a)-(e) to a follow-up issue; list them in docs/service.md as
deferred with this decision; merge #519 as the service base." Not built here:

- (a) lane state from the runtime's event stream: built in #529 (see Lane state above);
- (b) batched GraphQL polling of GitHub with ETags: built in #530 (see GitHub above);
- (c) one kept-open SSH connection per host and one GitHub client;
- (d) the board served from memory with one live stream to each phone; the board still polls, and the future cooee
  plugin is to use `/api/v1/events`;
- (e) the week-long soak with flat memory and the measured latencies (board < 10 ms, push → review < 5 s,
  verdict → merge < 5 s).
