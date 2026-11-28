# cooee's native screens (#814)

drover is cooee's `drover` plugin: `cooee-plugin.toml` at the repo root names its places (the Conductor's Queue and
Events chips, its chat cards, the drover menu row) and `[permissions]`. cooee runs `drover serve --addr
127.0.0.1:$PORT` and calls it with `Authorization: Bearer $COOEE_PLUGIN_TOKEN`; the page's own Host check and a paired
device work too (the phone page draws the same screens, #927). Beside `/api/v1` it answers JSON made of cooee's blocks
(cooee#125's screens-as-data design):

- `GET /screens/queue` (`?repo=<fleet>`), `/screens/events` (`?kind=merge,verdict,lane,ask`, `imp` for the
  [important] chip: only events the feed marks `imp`; `?before=<unix time>`; #1378: rows in sections by time, `last
  hour`, `today`, `yesterday`, `earlier`, each `ev-<group>` with its list `ev-<group>-list`),
  `/screens/fleets` (the Fleets sheet: per fleet, a model and an effort picker for every role it configures, efforts
  from the role's harness; Save writes the role's model and effort, logged `set` via cooee; and its Hosts and
  Notifications rows, `GET /screens/notify`: each `[[notify]]` and per-fleet notifier, its kind, target and events,
  and the removed Telegram bridge as off) and `/screens/cards` (the open asks, design reviews, needs-human issues, run
  cards and #1066 config proposals and secret requests as chat cards); #977: each full screen (not a sheet) shows the step's snapshot age, `as of 3m ago` (`as of: not yet known` before
  one; #1310: sent as `{since, label}`, the label's `{}` the age the client ticks, as are a thread's message ages and the threads list's), `warn` past 10 min, as a `snapshot` item of its `stat` block, else an `age` section's row; q1235: each ask card carries `fold`, its folded line (`#N · kind · question cut short · age`)
  and its `main` button (`{block, action, value, label}`, the recommended answer or one more round), which the app
  shows until a tap opens the full card; a second tap folds it again;
- #1172: `GET /screens/queue/done?repo=<fleet>`, opened from the Queue's `done today` row (its value: the list's length): the fleet's merges of the last 24 h, newest first, each `#N title`, `merged HH:MM UTC · <sha>`, its `title`, `t` and `sha` from `done`; a row opens `/screens/issue` for it.
- #1316: Events, `/screens/queue/done` and `/screens/backlog` send their first 100 rows; the list (the backlog's form) then carries
  `more`, the next page's path with its filters and `after=<cursor>` (Events: on its last group's list, with `in=<group>`: the next page's rows all go in that list) (the last row's key: event seq, merge time and issue,
  issue number), no duplicate and no gap. The page fetches it when scrolled within a screen of the end and appends the rows
  in place. An Events stream keeps the window it opened with (`to=<cursor>`): a delta adds rows on top or changes loaded
  ones, never drops one.
- #926: `GET /screens/lanes`, the Lanes chip: each live lane grouped waiting (blocked), working, asleep (idle), done, parked, dead, with its figure
  (`coat`, `state`), fleet and issue, role, host, now-line and age, and for a paired device the read-only `other in
  shed` section; a lane row (and a Queue row's live lane) opens `GET /screens/lanes/<agent>` (a paired device or the
  plugin token only), the lane sheet: its facts (issue, role, state, host, branch, port, PR), Stop/Kill/Clean (Close pane
  for an Others agent), a Send line and the transcript. #1373: the transcript comes from serve's lane tails
  (`serve/tails.rs`), never from shed on the request: one background feeder reads every lane's last 200 lines each 30 s,
  local and remote, and an open sheet's lane up to 5,000 lines once, then each second (a remote lane each
  `serve_remote_secs`) for 10 min. The sheet shows its last 200 lines (at most 32 KB) as blocks of 50 lines, each id
  `t<gen>-<its first line's number>`, kept when a line arrives; `GET /screens/lanes/<agent>/older?before=t<gen>-<n>`
  pages back 200 kept lines (`blocks`, `more`; 410 once the tail was read anew). A watched tail keeps 5,000 lines or
  512 KB, another 200 lines, all of them 16 MB, unwatched lanes shrinking first. Each
  action runs the board's `act` (the CLI verb, `confirm`ed) or `send`, logged `act`/`prompt` with `device: cooee`;
  #1378: Kill and Clean run `act`'s dry run first and answer a sheet whose button's `confirm.message` is its line (what
  goes to `wip/`, or Clean's reasons); that button sends `confirm: "1"`; a refused dry run (UNSAFE_CLEAN) shows its line, no button;
  #1315: `send` replies once the `prompt` is logged and hands the line to shed after the reply; a failed delivery is
  logged `prompt-failed` (`agent`, `error`) and the sheet's transcript ends in a `✗ not delivered` row until the next prompt;
- #1091: `GET /screens/hosts`, the page's Hosts sheet (a [⚙] row, not cooee's): one row per host, main first, then
  A→Z, with its kind, on/off and live lane count; a remote host's [Off]/[On] runs `host` (`host-off`/`host-on`) and
  the main host's [Reap N], shown while leftovers exist, runs `reap`, each confirmed; #1378: a fleet disk section shows
  the worktree total (drive's last walk) against `worktree_budget_gb` and the orphan folders, [Remove N] running `orphans`. No gauges or charts (Beacon's).
  #1313: the leftover count is read off the request (the host sampler, `refresh`; never by a host action);
- #1305: `GET /screens/scene` (and its `/stream`), the page's Scene (page-only; the header's `[scene]` swaps the whole
  page between the screens and Scene, kept per phone; cooee draws its own scene): one `scene` block with `weather`
  (`sky` clear, windy, fog or storm from host load, memory pressure and the spawn gate; `wind`, `fog` 0–2, `storm`,
  `line`), #1415 `load` (calm, busy or held: a `sign` theme's needle) and `repos`, each a fleet (then `others`) with its
  queued issues at the gate (`blocked_by`) and its lanes in bands (`working`, `idle`, `done`, `blocked/dead`) as rows
  with `figure` (`state`, `act`, `coat`), `tone`, `value` (age), `host` and `needs`. The page draws the old Herd
  (#37, #71, #292): the ranch strip (the theme's `scene.svg`, the pinned swag pack, a `<config
  dir>/themes/<name>/scene.svg` overriding it; farm, with none, gets the drawn ranch), the farm sprites from the pinned
  swag pack, one page per repo ([‹] [›], the dots, ←/→, a wheel or a drag), the repo's needs-you cards from
  `/screens/cards`, and `[A]`/`[☀]`/`[☾]` in the header (Auto: night before 07:00 and from 19:00, phone clock);
- #1783: the last card of `/screens/cards` (paired only, so the page, the board and cooee's cards place all draw it) is
  `new-issue`: a repo chip, a line and [+] for a screenshot. Its `new-issue` action logs a `scope-request` (once per
  `req`) and starts that repo's one `scope` lane (`drover spawn <fleet> 0 scope`, on the main host) once the reply is
  sent; a second request waits. Each open request is a row, `scoping` or `waits`, whose [Drop] (`scope-drop`, behind
  its confirm) files nothing and closes a running lane. The lane is read-only (no edit, commit, spawn or merge); it asks
  through `drover ask <fleet> -` and posts its draft with `drover draft`, an ask whose `approve` makes drover file
  exactly that title and body and queue it with the draft's `--mark`s, `drop` files nothing, and a typed note goes to
  the lane as a revise; approve and drop close the lane and the next waiting request starts;
- `GET /screens/<x>` answers from the hub (#1372, `serve/hub.rs`): each screen is built once per change for every
  reader, and only when a signal it depends on moved (the log, the snapshot, a background read, the config, the gh
  cache folder, a 30 s clock); a read of a stale screen nobody is building builds it, others wait for that build. It
  carries `ETag: "<epoch>.<rev>"` and answers a matching `If-None-Match` with 304;
- `GET /screens/<x>/stream`: server-sent `{seq, ops}` events, a `screen` op first, then each change as `merge`,
  `replace`, `remove` and `insert` ops by block id, and the Queue chip's `badge` count;
  one publisher thread rebuilds the screens open streams hold (each second, at once after a write but at most once a second a screen, asleep while none
  is held) and hands each stream the diff; no server-wide stream cap, 16 a device;
- `POST /action` `{screen, block, action, values}`: an action not in the manifest's `[permissions]` actions is 403;
  each other runs the board's own op (a refusal is 409 with its line);
- `GET /api/v1/manifest` (#1774): the plugin's name, version, `api` URL, `actions` and `destructive`, the `[threads]`
  paths, and its `screens`, one place each: `type` (header-strip, chat-cards or menu-row, `kind` in Rust), `id`,
  `persona`, `label`, `subtitle` and `screen`; `cooee-plugin.toml` is `just gen`'s rendering of it.

Tests: `crates/cli/tests/cooee_screens.rs` against cooee's fixtures in `crates/cli/tests/golden/cooee/`.
