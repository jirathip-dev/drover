# serve-split: the API surface, the gaps, and where each file goes (#1645)

Read on staging `7c0a28c1c`. "Frozen" means a shape file in #1571's contract folder
(`crates/cli/tests/golden/contract/v1/*.keys`), which `arch-check`'s `append_only` lets grow and never change.

## What the page's server needs from the API

| The edge needs | API path | Today | Gap |
|---|---|---|---|
| a screen | `GET /screens/<x>` (with `ETag`) | served; shapes checked by blocks' check and `cooee_screens.rs` | freeze the route and its envelope (S4) |
| a tap | `POST /action` | served; frozen by #1571's D1 | none |
| the device stream | `GET /stream`, `POST /stream/<sid>` | designed (#1318), not built | **build (S3)**; freeze (S4) |
| a screen's stream, until S7 | `GET /screens/<x>/stream` | served | none; never frozen, it is deleted |
| images a screen links | `GET /design/<fleet>/<n>/<i>`, `GET /shot/<fleet>/<n>/<i>[/<sha>]` | served, outside `/api/v1`, Host check only | the token rule; freeze (S4) |
| a chat file a screen links | `GET /thread/<t>/<file>` | served | leaving (#1571's D4); not frozen |
| the phone's timings | `POST /api/v1/timings` | served, no fixture, unpaired | freeze (S4) |
| the lifeboat | `GET` and `POST /api/v1/lifeboat` | served, no fixture | freeze the route (S4); its HTML is not a shape |
| who is asking | `GET /api/v1/device` | none: the server reads the `devices` table itself | **new (S2)** |
| pairing | `POST /api/v1/pair/request`, `pair/claim` | none: the server calls `devices::request` and `check` | **new (S2)** |
| its settings and the API's build | `GET /api/v1/hello` | none: the server reads fleet.toml and its own `SHA` | **new (S2)** |
| the places | `GET /api/v1/layout` | none: the shell reads `cooee-plugin.toml` and adds two places in code | **new (S2)** |
| the push key and subscribe | `GET /api/v1/push/key`, `POST /api/v1/push/subscribe` | none: the server calls `webpush` | **new (S2)**, leaving (#1571's D4); not frozen |
| a caller's identity | `Authorization`, `Drover-Device`, `Drover-Ip` on every path | the plugin token on screens and `/action` only; none on `/api/v1` | **the token rule on every path (S2 in process, S4 on the wire)** |

That is the whole list. The page calls no other `/api/v1` route, so this issue freezes no other. The old reads
(`state`, `host`, `queue`, `needs`, `lanes`, `chat`, `herd`, `roles`, `events`, …) move with `v1.rs` and keep their
re-recordable fixtures; pruning them is not this issue's.

What #1571 adds is the API's as well and needs nothing from the edge: `rows`, `rows/stream`, `thread/<id>/rows`,
`PUT thread/<id>`, `said`. cooee calls them on the API's address.

## Deleted in S0 (no caller in `web/src`, in cooee's manifest, or in the terminal)

| Route | Why it is safe | What goes with it |
|---|---|---|
| `GET /live` | the page does not call it; `board_style.rs` asserts the page never names it | the registration in `one`, `tick`'s fan-out, `Watch`, `parts`, the `clients` list, the `toast` on a job's end, `sw.js`'s two mentions |
| `GET /lane` | replaced by `/screens/lanes/<agent>` and `GET /api/v1/lane/<agent>` | its arm; `transcript` stays for the API route |
| the form `POST /` | every tap is `POST /action` | its arm and the 303 answers; `op` stays for `/action` and `POST /api/v1/<op>` |
| `POST /answer`, `/setup`, `/thread` | reached only as `POST /api/v1/menu`, `setup`, `chat-about` | their arms; `send` stays |
| `POST /report` | no caller: the page posts no report, and no `/api/v1` op reaches it | its arm and its branch of `send`; `drover report` (the CLI) stays |
| `POST /pair/cancel` | a new request already cancels the old one | its arm |
| `POST /push/unsubscribe` | the page never calls it | its arm |

Before the PR, S0's lane greps cooee's pinned commit for each path and the four tests that open `/live`
(`drive.rs`, `chat_tab.rs`, `config_card.rs`, `handover.rs`) move to a screen's stream. A hit in cooee keeps that
route and says so in the PR body.

## Where each file goes (S1)

| File (lines) | To | Note |
|---|---|---|
| `api.rs` (1,146) | `api/src/api.rs` | the `ts` feature and `just types` follow it |
| `screens.rs` (1,985), `screens_kit.rs`, `screens_lanes.rs`, `screens_fleets.rs`, `screens_hosts.rs`, `screens_pages.rs`, `herd_tests.rs` | `api/src/` | whole; `plugin` and the `/action` entry are the API's token rule |
| `screens_local.rs` (194) | `api/src/` | `Local` and `perform`; `Who` becomes `Viewer` in S2 |
| `serve/v1.rs` (1,265), `serve/chat.rs` (1,291), `serve/hub.rs` (472), `serve/tails.rs` (345), `serve/jobs.rs` (219), `serve/fresh.rs` (185), `serve/slo.rs` (113) | `api/src/serve/` | whole |
| `later.rs` (234), `cooee.rs` (49), `lifeboat.rs` (178) | `api/src/` | whole; `just html-lint` names the lifeboat's new path |
| `serve.rs` (3,478) | carved, below | the one file that is not a move |
| `hooks.rs` (220) | the webhook listener (`listen`, `reply`, `head`) to `api/src/hooks.rs`; `serve`, `held`, `take_over`, `board` to `cli/src/serve.rs` (the launcher) | the launcher keeps core's `timer`, `update` and `runner` calls |
| `handover.rs` (168) | stays in `crates/server` | S2: the socket path and the successor come from `Edge`; the memory watch moves to `api::host` |
| `serve/conn.rs` (118), `serve/wire.rs` (136) | stay in `crates/server`; `drover-wire` in S4 | both listeners use them from S4 |
| `serve/device.rs` (63) | stays in `crates/server` | relays the scene; `device_hosts` from `hello` |
| `lib.rs` (9) | stays | S0: the glob becomes `core_uses.rs` |

`serve.rs`, by the groups it has today:

| Lines today | What | To |
|---|---|---|
| 1–157 | the embedded files, the asset hash, constants | edge: `assets.rs` |
| 159–295 | `serve_on`, `serve_with`: the listener loop and the background threads | the loop to edge `lib.rs` (`run`); the threads (sampler, readers, hub, tails, jobs) to `api::host` |
| 297–430, 774–822 | `Memo`, `Server`, `new`, `refresh` | api |
| 433–490, 576–773 | HTTP helpers; small view helpers | the header and form helpers to both (in `wire` from S4); the view helpers to api |
| 823–945 | `one`: `/live`, the stream entry, the write cap | `/live` deleted (S0); the stream entry and the cap to api |
| 947–1082 | `deliver`, `tell`, `tick`, the caches | api (`tick` shrinks with `/live`) |
| 1085–1308 | `respond`, `reply`, `route`, `allowed`, `auth` | split: the edge's own arms (shell, files, themes, pair, push) and `allowed` to edge `router.rs` and `pair.rs`; the rest stays `Server::route` in api |
| 1310–1620 | `send`, `act`, `transcript`, `write`, `parts` | api |
| 1621–1733 | `page`: the shell | edge: `shell.rs` |
| 1734–2786, 3194–3478 | the tab and sheet builders | api |
| 2787–2833, 3125–3193 | `Canned`, `export` | the screens' half to api, the files' half to edge; the launcher calls both |
| 2834–3124 | themes | edge: `theme.rs` (`stage` to api) |

The edge after S1 is `lib.rs`, `router.rs`, `shell.rs`, `assets.rs`, `theme.rs`, `pair.rs`, `relay.rs`, `engine.rs`,
`handover.rs`, `device.rs`, `conn.rs`, `wire.rs` and, until S2, `core_uses.rs`: about 2,000 lines.

## How each core module leaves the edge (S2)

These are the lines S1 leaves in `core_uses.rs`. S2 deletes each and then the file.

| Core module | The edge uses it for | Replaced by |
|---|---|---|
| `devices` | the cookie check, a pairing request and claim, the cooee token, the auth log | `Drover-Device`; `device`, `pair/*`; the API makes the tokens and writes the auth log |
| `webpush` | the push key in the shell, subscribe | `push/key`, `push/subscribe` (leaving) |
| `config` | allowed hosts, device hosts, the theme, the hint, the limits | `hello`; the launcher's `Edge` |
| `events` | `pair-request`, `serve-mem-high` | logged by the API |
| `runner` | the inherited listeners, the successor, the pid, the request-path mark | `Edge.listeners`, `Edge.successor`; `std::process::id`; the mark is the API's |
| `clock` | now, a monotonic instant, sleep | the edge's `clock.rs` |
| `update`, `timer` | the control socket's path, asking a holder to hand over, the job's pid | the launcher (`cli/src/serve.rs`), which may link core |
| `Ctx`, `SHA`, `stderr!` | paths, the build, the log line | `Edge.state`, `Edge.build`, `Edge.log`: the launcher passes core's values and its one writer |

## Tests

| Test | Proves | Slice |
|---|---|---|
| the existing 104 files on `Server::respond`, `api_v1_contract.rs`, `cooee_screens.rs`, `serve_health.rs` | the API behaves as serve did; they name `drover::serve`, which re-exports `drover_api`'s | S1 (no test edited) |
| `cli/tests/edge.rs` (new) | the edge on a door that replays the contract folder's answers: the shell, a relayed screen with `ETag`, a tap, pairing end to end, a stream piped and closed from both sides, read-only, a browser's `Drover-Device` header dropped, `BUSY` with the API away; and every `RELAYED` line with no `until` has a contract file | S2, extended in S4 |
| `contract_v1.rs` (#1571's) | each new route's shape | S2 records, S4 covers the table |
| `handover.rs`, `page.rs`, `scene_device.rs` (existing, they start the binary) | two processes: the API host and `drover serve` started by the harness, killed on drop | S4 |
| the serve gates (150 ms p95, the request-path rule, memory) | unchanged numbers, measured through the edge | S4 |
