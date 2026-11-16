# The phone page

`drover serve --addr <host:port>` is one phone page (Home Screen app, works offline with a `last update hh:mm`
line). `--read-only` serves GET only.

#927: the page is a renderer of Drover's screens, the same JSON blocks cooee draws (below, "cooee's native
screens"): one definition per screen in `crates/server/src/screens.rs`, two renderers, cooee's native one and this
page (`web/src`, built to `web/dist/board.js` and `board.css`). The page shell (`GET /`) carries `cooee-plugin.toml`'s
`[[places]]`: each `header-strip` place is a tab (Queue, Events, Lanes, in the manifest's order), each `menu-row` place
opens from `[⚙]` (the Fleets sheet), and the `chat-cards` place is the cards above every tab (open asks, design
reviews, needs-human issues, run cards and alerts, each folded to its one line and main button until tapped). A new
screen in the manifest is a new tab with no change to the page. The Threads tab is the standalone page's own
([orchestrator.md](orchestrator.md)); asks and run cards show as cards. `drover doctor` and `GET /api/v1/host` give the
hosts view. The plain
lifeboat ([lifeboat.md](lifeboat.md)) stays the last resort when this page is broken too.

The page fetches a screen (`GET /screens/<x>`), draws it block by block, one renderer per block type (`screen`,
`section`, `list`, `row`, `card`, `text`, `stat`, `chips`, `choice`, `button`, `run`, `images`, `form` with its
`toggle`, `text`, `picker` and `secret` fields, `sheet`), follows its `stream` (`GET /screens/<x>/stream`: the open
tab's and the cards', at most two at once) and applies each `{seq, ops}` patch in place, flashing the rows that
changed. A block's `action` POSTs `/action` `{screen, block, action, values}` (a `confirm` asks first in the page's
own dialog); the reply's `toast` shows under the header, its `ops` patch the screen, `close` closes the sheet. A
row's `tap` opens a sheet (`sheet`), another screen in the tab with `[‹ back]` (`open`) or a link (`url`). A sheet's
`done` posts its fields' values. A row's `figure` (`state`, `tool`, `coat`) is the theme's sprite, as before: a
lane's avatar in its coat, else the 24 px row sprite, else the state glyph (terminal).

The header has the sync dot (green: the open tab's stream is live, pulsing on a patch; amber: connecting; red:
offline; blue in the demo), `[↻]` (read the tab and the cards again) and `[⚙]` (the menu-row screens, the theme and
colours, and in the Home Screen app "enable alerts"). Each screen's last copy is kept on the phone (localStorage) and
painted first, offline too, with `last update hh:mm · Nm ago` on the strip until a fresh read or a live stream
replaces it. A read over 300 ms shows `↻ loading` on the strip. `crates/cli/tests/page.rs` taps through it in Chrome.

#895: every `[limits] serve_host_secs` serve hands its freed heap back to the OS and reads its own RSS; past
`[limits] serve_mem_mb` (300) it logs `serve-mem-high` once and hands over to a fresh copy of itself (#894's handover,
no client dropped), unless it was over that already at its first reading (a fresh copy would be no smaller).
`scripts/serve-soak.sh` measures it on a copy of a real database.

Snapshot (#337): the fast step (every minute) and every kick (a lane push, a verdict, an ask, a board tap) take one
look at everything the board and `status` show and save it as the `board` row of `state.db`'s snapshots (#344): each read's output by
command line. Every remote host's reads (verdict files, `git` facts, its shed's agent list and pane reads) go in one
ssh call per host per step, a small script that prints each output and exit code; drover's ssh keeps one connection
per host for 10 minutes (`ControlMaster=auto`, `ControlPersist=10m`, whatever `~/.ssh/config` says). Only what decides
something is read: a verdict file only until its verdict is recorded, `git log` only for an impl/fix not seen pushed
(or a review with no spawn sha), never a merged or closed lane. A request (`/screens/…`, `/api/v1/…`, `/live`) and
`status`/`hosts` replay the snapshot: a remote read it lacks fails, it never runs on a request; only a lane's chat
(`/lane`) reads that one session's log from shed live (#1053: `session::transcript`, or the copy kept at close). Every screen shows the snapshot's age (#977: `as of 3m ago`, `as of: not yet known` before the first; amber past 10 min), and `status` ends with it. With no snapshot saved (a new install), reads run live as before; the
watcher pushes on each new snapshot instead of polling shed. `tests/perf.rs` is the gate (budgets: [performance.md](performance.md)): 30 lanes on 2 hosts, +300 ms
per remote call, at most one remote call per host per step, `GET /` < 150 ms (< 40 KB),
`status` < 300 ms; a miss fails CI and the numbers go to the job summary.

Speed (#57): a background thread samples the host every 10 s (the 2 s `vm_stat` delta happens there),
so a request never waits on it. Requests share the watcher's cache of pane reads and the
per-issue `git log`; the watcher re-reads at a third of a request's ttl, so a request finds them warm. The icons and
manifest are `public, max-age=86400`. The page is HTML only (`no-store`); its script and style are
`/a/<hash8>/board.js` and `board.css`, with the built-in themes (`themes/…`; `<hash8>` is FNV-1a over their content
at build time) `cache-control: public, max-age=31536000, immutable`, precached by the service worker. The SW serves
the page stale-while-revalidate and never caches `screens/`, `action` or `api/`.

A POST must name the bind address (or a `[serve].allowed_hosts` name, e.g. the `tailscale serve` `*.ts.net` host) in
`Host`/`Origin`. `/live` (the old board's stream of API v1 parts) still answers until #746 removes it.

A device must be paired once (#59, docs/setup.md): the header's **[pair]** (shown to an unpaired browser on the live page) POSTs `/pair/request`, which shows a 6-digit code and sets
an HttpOnly, Secure, SameSite=Strict `hd=` cookie with a random token for 10 minutes. The token pairs
nothing until `drover serve approve <code>` on the Mac (`NO_SUCH_CODE` / `EXPIRED` otherwise, single
use; `serve pending` lists requests). The approval is logged, so the waiting page's live stream
sends its `ev` (and the page's 2 s claim poll, #288, sees it) and the page POSTs `/pair/claim`, which keeps the same cookie for a year;
each page load claims again, renewing it, and a device unused for a year (365 days) no longer counts as paired
(`drover serve devices` lists each paired device's days left). Opened on the host itself (a loopback peer whose
`Host` is `127.0.0.1`/`localhost`, no `X-Forwarded-For` or `Tailscale-User-Login`), the panel also shows the
exact `drover serve approve <code>` line with a `[copy]` button: paste it in Terminal (a browser can't run a
command; there is no `drover://` link). Local doesn't mean trusted: lanes run on the host too, so the lane
hook refuses `serve approve`, `serve unpair` and `serve devices --revoke` (`REFUSED OWNER_ONLY`). At most 4 requests
are open, 2 per peer address (behind `tailscale serve`, a loopback peer, the `X-Forwarded-For` address or
`Tailscale-User-Login`; trusted from loopback only), and a new code never repeats an open one. A re-tap of
Pair replaces the browser's own open request, and the panel's `[cancel]` (`/pair/cancel`) drops it. Only the token's SHA-256 is kept, in `<state>/serve/devices.json`
(0600); `drover serve devices` lists them and `drover serve unpair` removes one. Approving a browser that pairs again
(same device label but its hash suffix, same address) replaces its older row (#501).
A 401 never latches the page unpaired (#501): it asks `GET /pair/status` (`{"paired": bool}`, no secrets) and only
a "no" shows the Pair UI; any later answer to a paired device (its `X-Paired: 1` header) takes it away again. A
database error reading the devices answers 503 `board busy, retrying` (API v1: `BUSY`), never 401: the page says why in its toast. Each 401 and 503 is one `serve.log` line, `auth <unix secs> <status>
<method> <route>: <reason>` (`no cookie`, `unknown token`, `expired`, `db error: <kind>`; never the token), and
`drover doctor` shows the last 24 h's counts.
#896: up to 256 requests run at once (reads off the snapshot are cheap), of which at most 16 writes (any non-GET); past
either bound the reply is 503 with `Retry-After: 1`. A tap's job (#977) runs on a worker per key: the fleet for a drive
press, the fleet and issue for the rest, a card or config by its id. Different keys run at once, one key's jobs run in the
order they came. Each ends in a toast.
The panel reads `Waiting for approval on the Mac: drover serve approve <code>`. An unpaired page,
while any device is paired, adds "Safari and the home-screen app pair separately" (iOS keeps their cookies apart). An optional `[serve] approve_hint`
(a string, `{code}` replaced by the code) is appended after ` · `, e.g.
`approve_hint = "or send 'pair {code}' to the owner"`; `/pair/request` returns it as a second line and
the page sets it as text, never HTML.
Secure cookies need https (`tailscale serve`) or localhost.

The Home Screen icon is Corral's Palomino app icon (jirathip-dev/corral, `ios/tools/herd-art/appicon-masters/treatment-a/palomino-1024.png`, MIT © jirathip-dev).

### Themes and colour schemes (#172, #241, #244)

The page's look is data, no code: see [themes.md](themes.md) for the contract (per-role actions, looks, load scene,
rail, moments, row sprites, icons, ASCII words, Lanes modes), the per-phone pick under `[⚙]`, `[serve] theme` (its
default) and the built-in `farm`, `kitchen`, `garden` (pinned from the shared theme repo, #928) and `terminal`. `serve` serves `/themes/<name>/<file>`; `--export`
writes them to `themes/`. Colours are a separate per-phone pick in the same sheet: 8 built-in schemes plus your own
`<config dir>/schemes/<name>.json`, each contrast-gated ([themes.md](themes.md#colour-schemes-244)).

## Report mode (#363)

`POST /report` (a paired device) keeps a bundle as `state/reports/<id>/bundle.json` and emits a `report` event;
`drover report` lists them. The old board's report mode that posted it went with the board (#927).

## Alerts (web push)

`[⚙]`'s **enable alerts on this phone** shows only in the Home Screen app, to a paired device (iOS allows web push
there alone). Tapping it asks for notification permission, subscribes with the server's VAPID
public key and `POST /push/subscribe`s the subscription; that and `POST /push/unsubscribe` need the same Host/Origin check and a paired device (401
otherwise). Delivery goes Mac → Apple/Google push service → phone, outbound only; the page stays
tailnet-only.

#370: the subscription carries the phone's theme words (`w_merged`, `w_needs`), so a merge reads `Order up: #2957
merged` (kitchen) or `Over the fence: #2957 merged` (farm), and needs-human / an owner ask `The bell: api #2961 needs
you` / `At the gate: …`; terminal sends none and keeps today's titles. A theme switch re-sends them with `quiet=1`
(saved, no test push). Only the title changes; the body stays.

```toml
[[notify]]
type = "webpush"
sub = "mailto:you@example.com"   # the VAPID `sub` claim; required
# on = [...]   # default: blocked, stall, lane-dead, ready-unmerged, merged, needs-decision, needs-human, ask-waiting, answer-undelivered, state-write-failed
```

It works per fleet too (`[[fleet.<name>.notify]]`), next to any command notifier, which is
unchanged. Each event is encrypted per device (RFC 8291 aes128gcm), signed with an ES256 VAPID
JWT (12 h) and sent once with `curl -sS --max-time 10 -X POST`, the body on stdin. A 404 or 410
prunes that device; any other failure logs one `notify-failed` (no retry). Tapping an alert
opens the page on its tab with the issue's row open. `drover push test` sends a test alert to
every subscribed device.

The subscribe tells the truth (#87): with a webpush notifier it pushes one "alerts are on" test to
that device alone and answers `alerts on` (or why the test push failed); without one it still keeps
the subscription but answers, under the button, "alerts are saved but off: add `[[notify]] type =
"webpush"` to fleet.toml (docs/serve.md)". API v1's `host` `push` row reads `on · 1 device · last
sent 3m ago`, `off: no webpush notifier`, or `on · last failure: <reason>` while a webpush
`notify-failed` is newer than the last delivery. Each delivery logs one passive `push-sent` with the
device count only (no endpoint, no key). `drover check` logs one `alerts-off` (again only when the
device count changes) while `subs.json` has devices but fleet.toml has no webpush notifier.

Secrets: the VAPID key (`<state>/webpush/vapid.key`) and the subscriptions
(`<state>/webpush/subs.json`, endpoint, keys, device label, creation time) are 0600 and never
logged, printed or served; the page gets only the public key and each device's label.

Dependencies: the allowed crates are clap, serde, serde_json, toml and anyhow/thiserror, plus, for web
push only, the pure-Rust RustCrypto set p256, aes-gcm, hkdf, sha2 and base64 (no async, no HTTP
client), ts-rs behind the `ts` feature for the board's TS types (#375, never in the release build), and, for the
shed runtime only, shed-proto (shed's serde wire types, no I/O; docs/runtimes.md), and rusqlite (bundled SQLite)
for the state database (#344, docs/architecture.md), keyring for the main host's secret store
(#441, owner-approved; docs/providers.md), and tokio in `crates/service` only (#345, docs/service.md). CI's deps gate
fails on any other direct dependency.

## GitHub webhooks (#626)

serve also listens on `[serve] hooks_addr` (default `127.0.0.1:7778`, `""` turns it off; a `--read-only` serve has
none). That listener answers `POST /hooks/github` only: every other path is a bare 404, so publishing it never
publishes the board. It is the one path on Tailscale Funnel; the board stays tailnet-only on `tailscale serve`. Funnel
opens a whole HTTPS port, so give it one the board does not use (443, 8443 or 10000):

```
tailscale funnel --bg --https=10000 --set-path /hooks/github http://127.0.0.1:7778/hooks/github
```

In the repo's GitHub settings add a webhook: payload URL `https://<mac>.<tailnet>.ts.net:10000/hooks/github`,
content type `application/json`, the secret, and the events *Pull requests*, *Check suites*, *Check runs*,
*Statuses* and *Pushes*. The secret is the 0600 file `[serve] webhook_secret_file` names (#707; one for every fleet, refused
`SECRET_FILE_OPEN` if others can read it); without it the endpoint answers 503 and doctor shows
`! webhook — no secret`.

Each delivery's `X-Hub-Signature-256` (HMAC-SHA256 of the raw body) is checked in constant time; a missing or bad one
gets 401 and is only counted (doctor's `webhook` row). A body over GitHub's 25 MB is refused (413) before it is read.
`ping` answers 200. A `pull_request`, `check_suite`, `check_run`, `status` or `push` event naming an open fleet
issue's branch or PR (a merge push: its `(#N)`) re-reads that PR into the cache `drover pr` reads and kicks drive
for the issue at once (200); anything else is ignored with 202. While a fleet has the secret, the fast step's PR read
becomes a catch-up poll every `[limits] webhook_poll_minutes` (default 10), for what a sleeping Mac or a down Funnel
missed.

## Serve health (#898)

Each minute serve writes its own numbers to `serve-health.json` in the state dir, and `GET /api/v1/health` returns
them as `serve`: `rss_mb`, `groups` (the last minute's `/live` first bytes, `/api/v1 reads` and `writes`, each with
`count`, `p50`, `p95` and `max` ms), `busy` (reads answered 503 in that minute), `clients` (live streams), `queued`
(taps waiting for a worker), `started` and `slow` (set while `/api/v1` p95 has been over 200 ms for 5 min). Each start
appends `<ts> cold|handover` to `serve-starts` (the last day kept).

`drover check` alerts once per state, then `<kind>-ok` when it clears:

| Alert | When |
|---|---|
| `serve-restarts` | more than 3 starts in 24 h that were not handovers |
| `serve-mem` | RSS over 300 MB |
| `serve-slow` | `/api/v1` p95 over 200 ms for 5 min |
| `serve-503` | any read answered 503 in the last minute |
| `drive-slow` | a fleet's last pass over `pass_max_secs` (120 s); names the fleet and its slowest step |

CI's gates are `crates/cli/tests/serve_health.rs`: the request-path guard (every read answers with the db write-locked,
and none calls git, gh or ssh) and the bench (each route group's p95 within 50 ms on the fixture state).

## JSON API v1 (#265)

`drover serve` answers JSON under `/api/v1/`, for Drover's screens and cooee core (below). Every route checks Host/Origin like `/live`; the routes kept for a paired device (a
transcript, a setup screen, and the paired ops: answer, drive, override, extend, park, unpair, orphans, reap, host, bqueue, cond, and
#555's send, menu, act, setup, chat-about, chat, close) refuse an unpaired one.

#725: `/api/v1/lifeboat` is a plain HTML page, not JSON: the fallback for when cooee is down ([lifeboat.md](lifeboat.md)).

| route | answer |
|---|---|
| `GET state` | the board in one read: `seq` (newest event), `now` and `tz` (the server's clock and UTC offset, s), `age` (the step's snapshot, null when none), `mtime`, `theme`, `fleets` (with `repo`), `queue`, `lanes`, `asks`, `hosts`, `services`, `alerts` |
| `GET health` | #709: `ok`, `config` (`ok`, or `invalid` with its `error`, `config invalid: <error> (fleet.toml:<line>)`); no answer: the process is dead. An invalid fleet.toml leaves the board reading the last good one (kept as `fleet.good.toml` beside the log), with that line as a red banner above every tab (`needs`' `config`); `check` alerts `config-invalid` once per distinct error, `config-ok` once it loads again |
| `GET host` | #267: the hosts view: `hosts` (each card's dot, words, rows with their gauge, value, sparkline series and, in an export, chart), `disk`, `orphans`, `leftovers`, `spawns`, `services`, `theme`, `deepseek`, `fleets`, `push`, `vendors` (#830: `setups`, each provider login's `{name, vendor}`, from the providers; #923: `accounts` stays empty), and on a live board `key`, `subs`, `devices`; `rw` and `live` say which forms show and whether they post |
| `GET queue` | #268: the Queue tab: a card per fleet (needing you first) with its `k` colour, serial groups, `need`, `paused` line or `drive` line and red `notes`, its `rows` (state, age and `t`, stage chips, lane, PR line, notes, a defaulted decision and its `ask`, ▲/▼ targets, the `lanes` list), #576 their `marks` by issue (`design`, `verify` `on`/`off`/null, `repro`, `serial`, `needs`, the verify `note`), `done` today, the `backlog` and the `config` sheet's data; `rw` and `live` say which forms show and whether they post |
| `GET issue/<fleet>/<n>` | #576: the issue's recorded verdicts, newest first: `role` (the stage: `rev`, `verify`), `round`, `sha`, `verdict`, `findings` (verdict.md after its first line), the newest one's screenshot `shots` (`/shot/<fleet>/<n>/<i>` serves the `i`th) and a `--repro` PASS's `repro` block (`kind` `run` or `verify`, its `text`) |
| `GET lessons` | #576: per fleet its `lessons` (`weekly`, `off`), the `last` lesson lane (`issue`, its spawn `ts`, the issue's `state`) and `next`, when tick runs the next (null: `off`) |
| `GET lessons/<fleet>/digest?days=N` | #576: the CLI's `lesson digest` (default 7 days) as `text` |
| `GET needs` | #269: the needs-you cards for this device: `snap` (#450: the step's look's time, 0 for none; the strip under the header shows its age once over 2 min), the open `asks` (`who`, `kind`, imported `from`), each `blocked` lane's card (unpaired: its `activity`, shed's note; paired: the last lines of shed's text as `tail`; #1053: no `menu`), the `needs-human` `issues` (one more round `pending`, the last verdict, its screenshot `shots` and #969 `shots_at`, the verdict's sha that `/shot/<fleet>/<n>/<i>/<sha>` names, the PR line, `merge`), `choosing` (`[chat·N]`); `rw`, `live`, `paired` and `pair` say which forms and buttons show |
| `GET lanes` | #269: the Lanes tab for this device: state `sections` with their repo groups and each lane's row (avatar `state`, `act`, `atone`, `coat`; `state` in `tone` for `ago`, `why`, the `activity` or `now` line) and hidden card (`head`, `shown`, `facts`, the PR line, `dead`, the `blocked` card); `error` (no fleet.toml) or `failed` (shed unread); #611: for a paired device, `shed`, the folded read-only `Other in shed (N)` section's rows (`GET shed`'s) |
| `GET shed` | #611 (paired only, else 401): every enabled host's shed sessions no lane or conductor in the log names and no `fleet` label marks, each `{name, host, runtime (shed or acp), labels, state (shed's), last (an ACP session's last reply, a terminal's last non-empty screen line), age (since the board first saw that state)}`; a host whose shed fails is one `{host, error}` entry, and stays up. One `ls` per host, cached 10 s like the lane list |
| `GET chat` | #269: the threads: per repo (`all` first) its thread `rows` and `archived` ones, then each thread's view (`ds`, `state`, its last 30 messages by `kind`, `older` or the `started` line); `rw`: a paired device on the live board writes; #620: serve keeps the folded tab by the log's last seq, fleet.toml's mtime, the agents and the minute, and a request in between reuses it without reading the log |
| `GET thread/<t>?before=N&n=30` | #408, #269: a thread's messages before seq `N`, as `chat`'s pages. #681: a handoff's message carries one `handoff` item (cooee's agent→agent message): `kind: "handoff"`, `id` (its seq; a repo handoff's is its card's), `from` (`{agent: "orchestrator", thread, repo}`), `to` (the thread id once delivered, else `{repo, pending: true}`), `title`, `summary` (the first line), `text`, `state` (`pending`, `delivered`, `failed` with its `error`, `forwarded`), `folded` (the sender's thread; the receiver's is the message form) and, while pending, `choices`: the card's rows (`n`, `text`, the `thread` it goes to, `new` for a new thread), each picked by `POST <op>` (`answer`) with its `fleet`, `id`, `choice` = `n` and `State.mtime`; the outcome shows in both threads' items. A repo handoff's own `kind: "handoff"` row after the pick has no item (its card's is the one) |
| `GET repo/<fleet>/threads` | #681: the repo's orchestrator threads, newest-active first: `id`, `title`, `last` (`line`, `ts`), `unread`, `awake` (its orchestrator not asleep), `archived`, and `newest: true` on the ongoing one (the first not archived), so cooee shows it as the agent's chat and the rest as side threads; `all` for the all-repos threads; 404 `NOT_FOUND` for no such fleet |
| `GET events?since=N` | #259's contract: the events after `N` (#266: `&until=M`, up to `M`) in `list`; over 500, `refresh: true` and none (read `state` again). Each as the Events tab says it: `glyph`, `tone`, `text`, `imp`, the row sprite's `act` and `coat`, `plain`, and its issue's `pr` or checks `run` |
| `GET lane/<agent>` | the lane's transcript page, the sheet's `/lane` JSON (`?before=N` pages back) plus #749's `blocks`: an ACP lane's transcript from shed's events (`prompt`, `reply`, `tool`, `plan`, `approval`; `?before=<seq>&n=N` pages back), a PTY lane's one `screen` block (its `lines`) |
| `POST <op>` | a form op (`pause`, `resume`, `mv`, `set`, `cond`, `bqueue`, `answer`, `drive`, `override`, `extend`, `park`, `unpark`, `host`, `reap`, …) with the HTML form's fields and `State.mtime`, as the form or a JSON object (#555: a list joins with `,`, `true` is `1`); answers `{ok, code, message}`. #977: a tap (`answer`, `drive`, `override`, `extend`, `park`, `unpark`, `card` `{id, act, sha}`, `config` `{id, act}`) is a job: 202 `ACCEPTED` with its `job` id at once, run after the jobs before it on its issue (#896), never refused for another job or a running drive; the same button tapped again while its job runs or within 15 min of the first tap returns that job (a card or config by its id alone) (a refused one may be tapped again); each logs `job` events |
| `GET job/<id>` | #977 (paired): a tap's `Job` (`id`, `op`, `fleet`, `issue`, `ts`): `state` `running`, `waiting` (`HOST_WAIT`: its issue's start waits for host; re-checked every `serve_host_secs`), `done`, `refused` (`code`) or `error`, and `message` (its toast) |
| `GET herd` | #555: the Herd scene's data |
| `GET roles?fleet=F` | #555: the fleet's [config] sheet (`config`: its roles with the model/effort pickers, the orchestrator) and `chat`, `[chat]`'s new-thread defaults with the harnesses it may name |
| `GET setup/<pane>` | #555: a setup terminal's screen (`lines`, `urls`, `closed`), the old board's `/lane?setup=` |
| `POST send` | #555: `{agent, text}` to a lane, or `{setup, text}` into a setup terminal: the old board's `/send` (10 a minute per device: `RATE_LIMIT`; `PAUSED`, `REV_READONLY`, `BAD_VALUE`) |
| `POST menu` | #555: `{agent, choice, h}`; #1053: refused `BAD_VALUE` (shed reports no menu; answer with `send`) |
| `POST act` | #555: `{agent, op, confirm}`, stop, kill, clean or close: `/act` (kill and clean dry-run without `confirm`) |
| `POST setup` | #555: `{host, name}`, start a configured setup terminal: `/setup` (`NO_SUCH_SETUP`) |
| `POST chat-about` | #555: `{fleet, issue}`, a backlog row's [chat about]: a new thread in the fleet's repo whose first message names the issue |
| `POST chat` | #555: `{harness, model, effort, mtime}`, `[chat]`'s defaults written into fleet.toml (`BAD_VALUE`, `STALE`, `DOCTOR` as `cond`) |
| `POST close` | #555: `{fleet, issue}`, a backlog issue's [close]: `gh issue close`, logged `closed`; `NOT_BACKLOG` for a queued issue or one with a lane |
| `POST qset` | #576 (paired): `fleet`, `issue`, `key` (`design`, `verify`, `repro`), `value` (`on`, `off`): the CLI's `queue <fleet> set <n> <key> <value>` |
| `POST lesson` | #576 (paired): `fleet`: the CLI's `lesson <fleet>`, its queue entry logged `by` the device |
| `POST orchestrator-attach` | #633 (paired): `{fleet, thread}`, a cooee thread as the repo's orchestrator thread (`orchestrator-attached`): `ask --thread` takes it, lane asks route to it, its asks and decisions are `events`; answers `OrchestratorAttach`: `brief`, `settings` (the synced orchestrator Claude settings), `env` (`DROVER_LANE`, `DROVER_THREAD`, `DROVER_PORT`), `guarded` (#565's ACP guard holds) and `why` not |
| `POST orchestrator-detach` | #633 (paired): `{fleet, thread}`, the link ends (`orchestrator-detached`; archiving the thread ends it too); `NO_SUCH_THREAD` unless attached |

Every answer is JSON, a refusal too: `{ok: false, code, message}` with 401 `UNPAIRED`, 503 `BUSY` (#501), 403 `FORBIDDEN`, 405
`READ_ONLY`, 409 and the `REFUSED` code (`STALE`, …), 404 `NOT_FOUND`. The theme is the phone's own
choice (localStorage): `state.theme` is the board's default, and no route writes it. The service worker
never caches `api/`. A breaking change is `/api/v2/`.

The types are defined once, in `crates/server/src/api.rs` beside the board's (#375): `just types` writes them to
`assets/types/api.d.ts`, which `web/` imports, and CI fails when that file is stale. The handlers are in
`crates/server/src/serve/v1.rs`. Each endpoint's JSON is golden-tested in `crates/cli/tests/api_v1.rs`. #555: the contract with
cooee core and Drover's screens is `crates/cli/tests/api_v1_contract.rs`: a fixture per route in
`crates/cli/tests/golden/api_v1/` (the route, its Rust type, the type's schema generated from its default, and a recorded
response). A response whose shape (keys, each value's JSON type) or a type whose schema
changes fails CI until the fixtures are updated in the same PR (`BLESS=1 cargo test --test api_v1_contract`).

`web/` is the page (#927), TypeScript with no framework: `npm run check` (`tsc --strict --noEmit`) and `npm run
build` (esbuild: `dist/board.js` and `dist/board.css`; the themes stay in `assets/swag/` and `assets/terminal/`). The
only dev dependencies are `typescript` and `esbuild`, at pinned versions. `web/dist` is committed and embedded in the
binary (`/a/<hash>/board.js`), so `cargo install` needs no Node. CI's `web` job rebuilds it and fails on a diff, so
run `npm run build` after a UI edit.

### Who consumes this

- Drover's screens (`/screens/…`, #814), which cooee and the phone page draw: built from the same data;
- cooee core, where it reads it (the lanes it shows as Workflow chats, cooee#33).

#609: every lane in `state`, `lanes` and `herd`, and each `queue` row (its active lane's, empty with
none), carries `runtime` (`shed` or `shed-acp`, from the lane's spawn: a config change later never moves it),
`host` (its `[hosts]` name, the main host's for a local lane, `local` with no `[hosts]`; `herd`'s stays null without
one) and `where`, the one label every surface shows: the host while 2 or more hosts are enabled (`air`, or
empty: no chip). The Lanes screen's rows show it; `status`'s HOST column, a `board` row's
`on …` line and `waiting`'s age show the same string. Each shape is in `crates/cli/tests/golden/api_v1/where.json`.

The rule: a screen that needs data drover doesn't send gets the `/api/v1` field here first, with its fixture in
`crates/cli/tests/api_v1_contract.rs`, then the consumer reads it. Consumers never derive engine state themselves.

## cooee's native screens (#814)

drover is cooee's `drover` plugin: `cooee-plugin.toml` at the repo root names its places (the Conductor's Queue and
Events chips, its chat cards, the drover menu row) and `[permissions]`. cooee runs `drover serve --addr
127.0.0.1:$PORT` and calls it with `Authorization: Bearer $COOEE_PLUGIN_TOKEN`; the page's own Host check and a paired
device work too (the phone page draws the same screens, #927). Beside `/api/v1` it answers JSON made of cooee's blocks
(cooee#125's screens-as-data design):

- `GET /screens/queue` (`?repo=<fleet>`), `/screens/events` (`?kind=merge,verdict,lane,ask`, `?before=<unix time>`),
  `/screens/fleets` (the Fleets sheet) and `/screens/cards` (the open asks, design reviews, needs-human issues and run
  cards as chat cards); #977: each full screen (not a sheet) shows the step's snapshot age, `as of 3m ago` (`as of: not yet known` before
  one), `warn` past 10 min, as a `snapshot` item of its `stat` block, else an `age` section's row; q1235: each ask card carries `fold`, its folded line (`#N · kind · question cut short · age`)
  and its `main` button (`{block, action, value, label}`, the recommended answer or one more round), which the app
  shows until a tap opens the full card; a second tap folds it again;
- #926: `GET /screens/lanes`, the Lanes chip: each live lane grouped waiting (blocked), working, asleep (idle), done, parked, dead, with its figure
  (`coat`, `state`), fleet and issue, role, host, now-line and age, and for a paired device the read-only `other in
  shed` section; a lane row (and a Queue row's live lane) opens `GET /screens/lanes/<agent>` (a paired device or the
  plugin token only), the lane sheet: its facts (issue, role, state, host, branch, port, PR), Stop/Kill/Clean (Close pane
  for an Others agent), a Send line and the transcript (`/api/v1/lane/<agent>`'s blocks as text and row blocks). Each
  action runs the board's `act` (the CLI verb, `confirm`ed) or `send`, logged `act`/`prompt` with `device: cooee`;
- `GET /screens/<x>/stream`: server-sent `{seq, ops}` events, a `screen` op first, then each change as `merge`,
  `replace`, `remove` and `insert` ops by block id, and the Queue chip's `badge` count;
- `POST /action` `{screen, block, action, values}`: an action not in the manifest's `[permissions]` actions is 403;
  each other runs the board's own op (a refusal is 409 with its line).

Tests: `crates/cli/tests/cooee_screens.rs` against cooee's fixtures in `crates/cli/tests/golden/cooee/`.

## Static export and the public demo

`drover serve --export <dir>` writes the page as static files instead of serving it: `index.html`, `sw.js`,
`manifest.webmanifest`, the icons, the `a/<hash8>/` assets and themes, and each screen as JSON at
`screens/<x>/index.html` (each place's screen, the cards and every sheet they open, such as a lane's), rendered by the
same code as the live page. Every URL in them is relative and the service worker keys its paths and cache on its
registration scope, so the files work under a sub-path (the live server at `/` behaves as before). A plain export is
the read-only view, without transcripts.

`--demo` (#39) is the public demo at <https://jirathip-dev.github.io/herdsman/>, built from the synthetic
`demo/` (3 made-up repos, `fleet.toml`, `events.jsonl`, `cache/` for issue/PR titles and CI, and
`runner.json`: the canned agent list, pane text and host values by command-line prefix). Nothing
runs: no shed, no gh, no network, and `demo/` is never written (#344: the export imports its files into a scratch database). "Now" is a minute after the log's last event, so ages never go stale.
The page carries a `demo` badge and a one-line note; an action only says "Demo with fake data", no POST is ever sent,
there is no stream (the dot is a static blue), no pairing and no "enable alerts". Lane sheets are the exported
`screens/lanes/<agent>`. `.github/workflows/pages.yml` publishes it on every push to `staging`; CI exports it too and
runs the leak gate over `docs/`, `demo/` and the export.
