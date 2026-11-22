# cooee-threads: who owns each current function (#1571)

Read on staging `d989c5beb`. **keep**: stays in drover, in the module named. **cooee**: the job moves to cooee and
drover's code is deleted in D4. **delete**: nobody needs it after the cut. Until D4 every function runs as today for
drover-hosted threads.

## `crates/core/src/orchestrator.rs` (1,520 lines) and its two files

| Functions | Verdict | Where, or why |
|---|---|---|
| `is_role`, `by`, `acting`, `actor` | keep | `core::orchestrator`: who runs a command |
| `DEFAULT`, `name`, `label`, `id_of`, `name_of` | keep | `core::orchestrator`: the seat's name on labels |
| `role`, `models`, `set`, `save` | keep | `core::orchestrator`: the role and its `[config]` sheet (the harness decides the profile's agent) |
| `brief::chat`, `brief::standing` | keep | `orchestrator/brief.rs`: the one brief |
| `chat::Seat`, `ids`, `seat`, `ChatProfile`, `hidden`, `profiles`, `profile` | keep | `orchestrator/chat.rs`: one profile per repo (`all`, `none`, each fleet) |
| `SCANS`, `WALKS`, `of`, `threads`, `find`, `fleet_of`, `title`, `archived` | keep | `core::thread`: the registry, read from `thread-facts` (old kinds still read) |
| `items`, `render`, `link`, `handoff_text`, `forwarded`, `text_ok`, `log` | keep | `core::thread`: `items` becomes the typed `rows`; `render` is `thread show`'s text |
| `unread`, `mark_read`, `show`, `ls` | keep | `core::thread`: drover's own read mark and the CLI's two reads |
| `say`, `post` | keep | `core::thread::say`, one function: logs the `said` row with its `prompt`; no delivery |
| `route` | keep | `thread::route::home`: by thread id, else the repo's desk (#1609) |
| `lane_ask`, `route_row`, `routed_to`, `to_owner`, `lane_of`, `flow`, `tell` | keep | `thread::route`: each emits a row; `lane_ask` and `tell` set `prompt` where they delivered |
| `handoff`, `send`, `forwards` | keep | `thread::route`: a handoff row to a thread id or a desk; no new thread, no pick ask (#1610) |
| `land` | delete | the handoff pick goes with #1610; `handoff-move` re-targets |
| `new`, `new_with`, `resent`, `make`, `assign`, `title_ok`, `retitle`, `archive` | cooee | creating, titling, archiving a thread; drover hears through `PUT thread` |
| `files_ok`, `upload`, `dir`, `inside`, `file`, `copy_in` | cooee | uploads and the agent's folder |
| `reply` | cooee | the agent's reply is the chat message; triage's summary becomes a `system` row to the desk |
| `deliver`, `once`, `START_SECS`, `acp`, `start`, `started`, `uuid`, `close`, `end`, `stop`, `hung` | cooee | running the agent through shed: start, resume, type, cancel, the hung check |
| `sleep_idle`, `signed_out`, `silence`, `held`, `pending`, `human`, `since` | cooee | idle sleep, the sign-in line, the rate-limit hold and its `continue`, the no-reply line |
| `defaults`, `on`, `thread_role`, `harness_models`, `pick`, `picked` | cooee | model, effort and harness pick |
| `chat` (the `[chat]` defaults writer) | delete | new-thread defaults are cooee's; the `[chat]` table goes |
| `LATER`, `deferred` | delete | nothing is delivered after a request |
| `link_of`, `attached`, `hold`, `attach`, `detach`, `chat::started` | delete | replaced by `PUT thread`; the old events stay readable through `find` |
| `is_id` | delete | a thread id is opaque; a row's `from` is typed |
| `brief::thread` | delete | the thread brief told the agent to `thread reply`; one brief is left |

## `crates/server/src/serve/chat.rs` (1,291 lines)

| Functions | Verdict | Where, or why |
|---|---|---|
| `owner_card`, `owner_cards`, `msg_card`, `msg_body`, `ask_body`, `config_body`, `secret_body`, `run_body`, `run_card`, `options`, `note`, `card`, `act_button`, `ask`, `change`, `link` | keep | move to `serve/rows.rs`: the one builder of a row's block, drawn by cooee and by drover's page |
| `open_ask`, `choosing`, `waiting`, `or`, `files_of`, `ext` | keep | `serve/chat.rs`, reading rows |
| `page`, `older`, `thread`, `row`, `tab`, `chat_tab`, `figure`, `row_block`, `list_screen`, `thread_screen`, `composer`, `threads_screen`, `mark_read` | keep | `serve/chat.rs`: drover's own screens and `chat` tab, from `thread::rows` and `thread::list` |
| `state` | keep | `choose` from the rows, else the `state` cooee put |
| `post` | keep | ops `say`, `read` and `secret` only |
| `qid`, `qfiles`, `queued`, `unqueue`, `flush` | cooee | the composer's queue |
| `chip`, `chips`, `composer_chips` | cooee | model, effort and harness chips |
| `first`, `new_screen` | cooee | a new thread and its first-line title |
| `file` | cooee | a chat file's bytes |
| `stop`, `deliver` | cooee | stopping and typing into the agent |

## `crates/chat/src/threads.rs` (363 lines) and `threads/view.rs`

| Items | Verdict | Where, or why |
|---|---|---|
| `Threads::new`, `now`, `tick`, `load`, `apply`, `open_ask`, `open_card`, `select`, `confirm`, `key`, `chat_key`, `s`, `Act`, `Pick`, `Note::Done`, all of `view.rs` | keep | the Threads tab, from `thread::list` and `thread::rows` |
| `send` | keep | calls `thread::say`; the row shows at once, the reply when cooee posts it |
| `check`, `deliver`, `Wait`, `Shed`, `Note::Delivered`, `Note::Health`, `CHECK_SECS`, `TRIES`, `NO_JOB` | delete | they wait on a delivery and on shed; drover delivers nothing |

## Outside the three files

| Code | Verdict |
|---|---|
| `crates/core/src/webpush.rs`, `/push/subscribe` and `/push/unsubscribe`, the VAPID key, the alert devices list of `GET host`, `drover push test`, the page's push handler, `[[notify]] type = "webpush"` | delete in D4 (cooee sends) |
| `events::notify`'s push rules (which kinds, routine and routed asks, a watched thread) | keep, as `thread::route`'s `push` rule; "watched" is cooee's |
| `crates/server/src/serve/device.rs` | keep (open question 2) |
| `crates/server/src/cooee.rs` | keep `GET /cooee/profiles`; `POST /cooee/thread-started` deleted in D4 |
| `POST /api/v1/orchestrator-attach`, `orchestrator-detach`, `conductor-*`, `chat`, `chat-about` | delete in D4; `chat-about` becomes a `said` row to the repo's desk |
| `drover thread new|title|reply|archive|restore` | delete in D4; `thread say|show|ls|handoff` stay |
| `roles/orchestrator/thread.md` | delete in D4; `chat.md` gains the handoff and `--typed` lines |
| `cooee-plugin.toml`'s `chat-cards` place | delete in D4: every card is a row in a thread |

By a count of today's lines, about 1,450 leave drover in D4: some 800 of `orchestrator.rs`, 230 of `serve/chat.rs`,
100 of the terminal and 315 of web push with its routes. About 350 are new (`serve/rows.rs`, the registry, the
routes). The rest of the issue's 2.3k lines is the rendering drover's own surfaces keep, because they stay.
