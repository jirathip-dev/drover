# `herdsman service` (#345, Phase C)

ADR, decided by the owner in #345's q355 (2026-11-05): "Build the full service now … Approved: the tokio dependency
and the cap raise … Shape: API-first." This lifts the "no daemon / no async" rule for one process and one
dependency, `tokio`, in its own crate (`crates/service`); every other crate stays synchronous.

## Shape

- **One process**, `herdsman service --api <addr> [--board <addr>]`, kept alive by launchd (`KeepAlive`) or systemd
  (`Restart=always`). A second one refuses to start. It logs `service-started`; a stall logs `service-stalled`, which
  every notifier gets, and the process exits so the supervisor starts it again.
- **Local socket** `service.sock` beside `state.db` (mode 0600). A kick (a lane's push, stop or ask, a verdict, an
  answer, a board tap) goes there and is driven in the service's process at once, not as a detached `kick --now`.
  With no service listening, every command goes on in today's direct mode: nothing is ever stuck on it.
- **GitHub** is polled by the fast step (`drive --fast`) every 15 s inside the service; the `dev.herdsman.fast` job
  is no longer needed while it runs (`timer` doesn't reinstall it, `doctor` shows it "by the service"). The slow
  `tick` stays as the safety net.
- **API v1** (the contract the board and the future cooee plugin use):
  - `GET /api/v1/health`: pid, version, busy jobs, heartbeat age.
  - `GET /api/v1/events?after=<seq>`: Server-Sent Events, the log after `seq` (the last 500), then each new event as
    it is logged; this host or a paired device's cookie only.
  - `POST /api/v1/cmd` `{"token", "args", "files"}`: a remote lane's command (below).
- **The board** runs in the same process with `--board` (the `serve` job's address); it is not grown here.

## Remote lanes over HTTP

With `[service] url = "http://<main host on the tailnet>:7778"` in fleet.toml, a lane spawned on another host gets
`HERDSMAN_SERVICE` (that url) and `HERDSMAN_TOKEN` instead of `HERDSMAN_MAIN`, so it never needs ssh to the main host.
Its `ask`, `answer`, `verdict`, `done` and `kick` go to `/api/v1/cmd` with curl; a `--file` travels as its text.
The main host keeps controlling other hosts over ssh (spawn, read, kill).

**The token** is 32 random bytes, issued by spawn, stored only as its sha256 in `state.db`, and never printed or
logged. It runs a command only when:

- the command is a lane command of the token's own fleet and issue (an answer's issue is its ask's);
- a verdict comes from a check role;
- the token's spawn is still the issue's latest for its role, its lane is not closed or dead, and the issue is open.

Anything else is `REFUSED TOKEN` (HTTP 403) and runs nothing.

## Robustness

Every job runs on a blocking thread in one of 16 slots with a timeout (a kick or poll 900 s, a lane command 150 s);
a timeout logs `service-timeout` and frees its slot, so a stuck job never blocks the others. At most 64 API
connections, 4 MiB a request, 64 KiB a socket line. A watchdog thread exits the process when the event loop's
heartbeat is 30 s late.

## Install

`herdsman service --install [--api <addr>] [--board <addr>]` writes the unit (`dev.herdsman.service`) and prints the
commands that swap it in; it loads nothing itself. The API listens on `127.0.0.1:7778` by default; for remote lanes give
`--api` the main host's tailnet address (the one `[service] url` names). On macOS, for example:

```
launchctl bootout gui/$(id -u)/dev.herdsman.fast
launchctl bootout gui/$(id -u)/dev.herdsman.serve      # with --board only
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.herdsman.service.plist
```

To go back to direct mode, boot the service out and run `herdsman timer install`.

## Line cap

The service's budget is the planned +3,000, inside the 31,500 cap the owner set in q391 (#514).
