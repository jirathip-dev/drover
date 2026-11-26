# JSON API v1 (#265)

`drover serve` answers JSON under `/api/v1/`, for Drover's screens and cooee core (below). Every route checks Host/Origin like `/live`; the routes kept for a paired device (a
transcript, a setup screen, and the paired ops: answer, drive, override, extend, park, unpair, orphans, reap, host, bqueue, cond, and
#555's send, menu, act, setup, chat-about, chat, close) refuse an unpaired one.

#725: `/api/v1/lifeboat` is a plain HTML page, not JSON: the fallback for when cooee is down ([lifeboat.md](../lifeboat.md)).

| route | answer |
|---|---|
| `GET state` | the board in one read: `seq` (newest event), `now` and `tz` (the server's clock and UTC offset, s), `age` (the step's snapshot, null when none), `mtime`, `theme`, `fleets` (with `repo`), `queue`, `lanes`, `asks`, `hosts`, `services`, `alerts` |
| `GET health` | #709: `ok`, `config` (`ok`, or `invalid` with its `error`, `config invalid: <error> (fleet.toml:<line>)`); no answer: the process is dead. An invalid fleet.toml leaves the board reading the last good one (kept as `fleet.good.toml` beside the log), with that line as a red banner above every tab (`needs`' `config`); `check` alerts `config-invalid` once per distinct error, `config-ok` once it loads again |
| `GET host` | #267: the hosts view: `hosts` (each card's dot, words, rows with their gauge, value, sparkline series and, in an export, chart), `disk`, `orphans`, `leftovers`, `spawns`, `services`, `theme`, `deepseek`, `fleets`, `push`, `vendors` (#830: `setups`, each provider login's `{name, vendor}`, from the providers; #923: `accounts` stays empty), and on a live board `key`, `subs`, `devices`; `rw` and `live` say which forms show and whether they post |
| `GET queue` | #268: the Queue tab: a card per fleet (needing you first) with its `k` colour, serial groups, `need`, `paused` line or `drive` line and red `notes`, its `rows` (state, age and `t`, stage chips, lane, PR line, notes, a defaulted decision and its `ask`, ▲/▼ targets, the `lanes` list), #576 their `marks` by issue (`design`, `verify` `on`/`off`/null, `repro`, `serial`, `needs`, the verify `note`), #1172 `done`: today's `merged` events, newest first (`issue`, `title`, `note`, `age`, merge time `t`, `pr`, merge `sha`), the `backlog` and the `config` sheet's data; `rw` and `live` say which forms show and whether they post |
| `GET issue/<fleet>/<n>` | #576: the issue's recorded verdicts, newest first: `role` (the stage: `rev`, `verify`), `round`, `sha`, `verdict`, `findings` (verdict.md after its first line), the newest one's screenshot `shots` (`/shot/<fleet>/<n>/<i>` serves the `i`th) and a `--repro` PASS's `repro` block (`kind` `run` or `verify`, its `text`) |
| `GET lessons` | #576: per fleet its `lessons` (`weekly`, `off`), the `last` lesson lane (`issue`, its spawn `ts`, the issue's `state`) and `next`, when tick runs the next (null: `off`) |
| `GET lessons/<fleet>/digest?days=N` | #576: the CLI's `lesson digest` (default 7 days) as `text` |
| `GET needs` | #269: the needs-you cards for this device: `snap` (#450: the step's look's time, 0 for none; the strip under the header shows its age once over 2 min), the open `asks` (`who`, `kind`, imported `from`), each `blocked` lane's card (unpaired: its `activity`, shed's note; paired: the last lines of shed's text as `tail`; #1053: no `menu`), the `needs-human` `issues` (one more round `pending`, the last verdict, its screenshot `shots` and #969 `shots_at`, the verdict's sha that `/shot/<fleet>/<n>/<i>/<sha>` names, the PR line, `merge`), `choosing` (`[chat·N]`); `rw`, `live`, `paired` and `pair` say which forms and buttons show |
| `GET lanes` | #269: the Lanes tab for this device: state `sections` with their repo groups and each lane's row (avatar `state`, `act`, `atone`, `coat`; `state` in `tone` for `ago`, `why`, the `activity` or `now` line) and hidden card (`head`, `shown`, `facts`, the PR line, `dead`, the `blocked` card); `error` (no fleet.toml) or `failed` (shed unread); #611: for a paired device, `shed`, the folded read-only `Other in shed (N)` section's rows (`GET shed`'s) |
| `GET shed` | #611 (paired only, else 401): every enabled host's shed sessions no lane or conductor in the log names and no `fleet` label marks, each `{name, host, runtime (shed or acp), labels, state (shed's), last (an ACP session's last reply, a terminal's last non-empty screen line), age (since the board first saw that state)}`; a host whose shed fails is one `{host, error}` entry, and stays up. One `ls` per host, cached 10 s like the lane list |
| `GET chat` | #269: the threads: per repo (`all` first) its open threads' `rows` (#1622: `archived` stays empty but with `?archived=1`), and in `threads` only `?thread=<t>`'s view (`ds`, `state`, its last 30 messages by `kind`, `older` or the `started` line); `rw`: a paired device on the live board writes; #620: serve keeps the folded tab by the log's last seq, fleet.toml's mtime, the agents and the minute, and a request in between reuses it without reading the log |
| `GET thread/<t>?before=N&n=30` | #408, #269: a thread's messages before seq `N`, as `chat`'s pages; #1622: `?after=N`, every message after seq `N` (a poll's cursor); #1066: a linked cooee chat's too (its cards and their settle lines). #681: a handoff's message carries one `handoff` item (cooee's agent→agent message): `kind: "handoff"`, `id` (its seq; a repo handoff's is its card's), `from` (`{agent: "orchestrator", thread, repo}`), `to` (the thread id once delivered, else `{repo, pending: true}`), `title`, `summary` (the first line), `text`, `state` (`pending`, `delivered`, `failed` with its `error`, `forwarded`), `folded` (the sender's thread; the receiver's is the message form) and, while pending, `choices`: the card's rows (`n`, `text`, the `thread` it goes to, `new` for a new thread), each picked by `POST <op>` (`answer`) with its `fleet`, `id`, `choice` = `n` and `State.mtime`; the outcome shows in both threads' items. A repo handoff's own `kind: "handoff"` row after the pick has no item (its card's is the one) |
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
| `POST timings` | #1312 (paired): the page's batch `[[kind, ms, network, parse, render], …]` (500 at most; unknown kinds skipped) into health's `phone`; `BAD_TIMINGS` |
| `POST lesson` | #576 (paired): `fleet`: the CLI's `lesson <fleet>`, its queue entry logged `by` the device |
| `POST orchestrator-attach` | #633 (paired): `{fleet, thread}` (`fleet` a repo, `all` or `none`; `drover attach <repo> [<thread>]` logs the same and prints the brief path, the settings path and the env lines), a cooee thread as the repo's orchestrator thread (`orchestrator-attached`): `ask --thread` takes it, lane asks route to it, its asks and decisions are `events`; answers `OrchestratorAttach`: `brief`, `settings` (the synced orchestrator Claude settings), `env` (`DROVER_LANE`, `DROVER_THREAD`, `DROVER_PORT`), `guarded` (#565's ACP guard holds) and `why` not |
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
