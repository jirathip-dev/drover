# Serve health (#898)

Each minute serve writes its own numbers to `serve-health.json` in the state dir, and `GET /api/v1/health` returns
them as `serve`: `sha` (the build it runs, #1081), `rss_mb`, `groups` (the last minute's `/live` first bytes, `/api/v1 reads` and `writes`, each with
`count`, `p50`, `p95` and `max` ms), `busy` (reads answered 503 in that minute), `clients` (live streams), `queued`
(taps waiting for a worker), `started` and `slow` (set while `/api/v1` p95 has been over 200 ms for 5 min). Each start
appends `<ts> cold|handover` to `serve-starts` (the last day kept); a start that fails appends `<ts> failed`.
Its `schemes` lists each colour scheme blocks' gate refused on load, with why (#1339, docs/themes.md).
`keys` lists the screens the hub keeps (#1372): each `key` (path and query), `paired`, `bytes`, `rev`, `holders` (open
streams) and `builds`.

#1312: the page times what the owner feels: each tap, sheet open, tab switch, send and answer from the input event to
the frame after the DOM changed (split into `network`, `parse` and `render`), and each stream delta from receipt to
painted. It batches them into one `POST /api/v1/timings` (paired only; `[[kind, ms, network, parse, render], …]`) at
most every 30 s, dropped while offline. `phone` in the health is the last minute by kind (`tap`, `sheet`, `tab`, `send`,
`answer`, `delta`, and each `<kind>/network|parse|render`) with `count`, `p50`, `p95` and `max`; `phone_slow` is set
while a kind's p95 has been over 150 ms for `slo_api_minutes`.

#1081: a `serve` whose port is held asks that port's health which build holds it: its own build, it exits 0 (launchd and
systemd restart it only after a failed exit); an older one, it asks that serve over its control socket to hand over (its
bridge runs the installed binary), then exits 0; nothing answers, it fails.
#1244: a serve the launchd (or systemd) job started never exits 0 on a held port: it stops the serve on the port and
listens itself, so the job's own pid is the one listener. `self-update` restarts serve only through the job
(`launchctl kickstart -k`, `systemctl --user restart`), and `drover doctor`'s `serve job` row is ✗ while the port's
listener is not the job's pid.

`drover check` alerts once per state, then `<kind>-ok` when it clears:

| Alert | When |
|---|---|
| `serve-restarts` | more than 3 starts in 24 h that were not handovers |
| `serve-mem` | RSS over 300 MB |
| `serve-stale` | the serve on the port runs another build than the installed `drover` |
| `serve-loop` | more than 3 failed serve starts in 1 h (launchd's serve exiting non-zero) |
| `serve-slow` | `/api/v1` p95 over 200 ms for 5 min |
| `phone-slow` | a phone kind's p95 (tap, sheet, tab, send, answer, delta) over 150 ms for `slo_api_minutes` |
| `serve-503` | any read answered 503 in the last minute |
| `drive-slow` | a fleet's last pass over `pass_max_secs` (60 s); names the fleet, its slowest step (spawns, PR reads) and each host's remote-call count and seconds |

CI's gates are `crates/cli/tests/serve_health.rs`: the request-path guard (every read answers with the db write-locked,
and none calls git, gh or ssh) and the bench (each route group's p95 within 50 ms on the fixture state).
