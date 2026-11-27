# The phone page

`drover serve --addr <host:port>` is one phone page (Home Screen app, works offline with a `last update hh:mm`
line). `--read-only` serves GET only.

#927: the page is a renderer of Drover's screens, the same JSON blocks cooee draws (below, "cooee's native
screens"): one definition per screen in `crates/server/src/screens.rs`, two renderers, cooee's native one and this
page (`web/src`, built to `web/dist/board.js` and `board.css`). The page shell (`GET /`) carries `cooee-plugin.toml`'s
`[[places]]`: each `header-strip` place is a tab (Queue, Events, Lanes, in the manifest's order), each `menu-row` place
opens from `[⚙]` (the Fleets sheet), and the `chat-cards` place is the cards above every tab (open asks, design
reviews, needs-human issues, run cards and alerts, each folded to its one line and main button until tapped). A new
screen in the manifest is a new tab with no change to the page. Asks and run cards show as cards (#1773 cut the
Threads tab; a chat lives in a client, [orchestrator.md](../orchestrator.md)). `drover doctor` and `GET /api/v1/host` give the
hosts view. The plain
lifeboat ([lifeboat.md](../lifeboat.md)) stays the last resort when this page is broken too.

The page fetches a screen (`GET /screens/<x>`), draws it block by block, one renderer per block type (`screen`,
`section`, `list`, `row`, `card`, `text`, `stat`, `chips`, `choice`, `button`, `run`, `images`, `form` with its
`toggle`, `text`, `picker` and `secret` fields, `sheet`), follows its `stream` (`GET /screens/<x>/stream`: the open
tab's and the cards', and an open sheet's once an action ran in it) and applies each `{seq, ops}` patch in
place, flashing the rows that changed. A block's `action` POSTs `/action` `{screen, block, action, values}` (a
`confirm` asks first in the page's own dialog); the reply's `toast` shows under the header, its `ops` patch the
screen, `close` closes the sheet. #1315: nothing is read again after an action: the stream brings the result (a stream whose client left frees its
slot of serve's 4 within a second). A sent
thread message or lane line shows at once, grey until serve's reply, then until the stream brings it (once); a failed
one keeps its text with [retry]. An answered ask leaves at once and stays gone unless the answer fails. A
row's `tap` opens a sheet (`sheet`), another screen in the tab with `[‹ back]` (`open`) or a link (`url`). A sheet's
`done` posts its fields' values. A row's `figure` (`state`, `act`, a Queue row's `grow` and `issue`, a lane's `coat`, an
Events row's `moment`, the main host's `load`) is drawn by the phone's style (#1306): `terminal` the state glyph, `ascii`
the theme's figures, `pixel` its animated sprites (a Queue row's plant at its stage, a lane's avatar in its look, a
Hosts row's load miniature); a row arriving with a moment plays it (merged hops, FAIL shakes, needs you pulses).

The header has the sync dot (green: the open tab's stream is live, pulsing on a patch; amber: connecting; red:
offline; blue in the demo), `[↻]` (read the tab and the cards again) and `[⚙]` (the menu-row screens, the theme, the style
(terminal, ascii or pixel) and colours, and in the Home Screen app "enable alerts"). Each screen's last copy is kept on the phone (localStorage) and
painted first, offline too, with `last update hh:mm · Nm ago` on the strip until a fresh read or a live stream
replaces it. A read over 300 ms shows the style's loading art (the theme's sprite, its ascii frames, else `↻`) on the strip. `crates/cli/tests/page.rs` taps through it in Chrome.

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
watcher pushes on each new snapshot instead of polling shed. `tests/perf.rs` is the gate (budgets: [performance.md](../performance.md)): 30 lanes on 2 hosts, +300 ms
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

The page's look is data, no code: see [themes.md](../themes.md) for the contract (per-role actions, looks, load scene,
rail, moments, row sprites, icons, ASCII words, Lanes modes), the per-phone pick under `[⚙]`, `[serve] theme` (its
default) and the built-in `farm`, `kitchen`, `garden`, `bush` (pinned from the shared theme repo, #928) and `terminal`. `serve` serves `/themes/<name>/<file>`; `--export`
writes them to `themes/`. Colours are a separate per-phone pick in the same sheet: 8 built-in schemes plus your own
`<config dir>/schemes/<name>.json`, each contrast-gated ([themes.md](../themes.md#colour-schemes-244)).
