# cooee-threads: who owns each current function (#1571)

Read on staging `fd81ef5c3`, D1 included. **keep**: stays in drover, in the module named. **delete**: nobody needs it
once its slice lands. Nothing moves to cooee: drover keeps its threads, and cooee's Orchestrator chats are a second run
path that calls drover's API.

## `crates/core/src/orchestrator.rs` (1,369 lines) and its two files

| Functions | Verdict | Where, or why |
|---|---|---|
| `is_role`, `by`, `acting`, `actor` | keep | `core::orchestrator`: who runs a command |
| `DEFAULT`, `name`, `label`, `id_of`, `name_of` | keep | `core::orchestrator`: the seat's name on labels |
| `role`, `models`, `set`, `save` | keep | `core::orchestrator`: the role and its `[config]` sheet |
| `brief::chat`, `brief::standing`, `brief::thread` | keep | `orchestrator/brief.rs`: the briefs, for both run paths |
| `chat::Seat`, `ids`, `seat`, `ChatProfile`, `hidden`, `profiles`, `profile`, `chat::started` | keep | `orchestrator/chat.rs`: the profiles cooee's Orchestrator loads (#1057) |
| `SCANS`, `WALKS`, `of`, `threads`, `find`, `fleet_of`, `title`, `archived` | keep | the drover thread registry; `core::thread::Registry` (D1) reads the same events for routing |
| `items`, `render`, `link`, `handoff_text`, `forwarded`, `text_ok`, `log` | keep | a thread's conversation and `thread show`'s text |
| `unread`, `mark_read`, `show`, `ls` | keep | drover's read mark and the CLI's reads |
| `say`, `post` | keep | the owner's message; D2 adds `said`, cooee's copy of a chat's owner line |
| `route`, `lane_ask`, `route_row`, `routed_to`, `to_owner`, `flow`, `tell` (in `thread/route.rs` since D1), `lane_of` | keep | the emitters; each logs a row, and delivers its line only to a drover thread (D2) |
| `handoff`, `send`, `forwards` | keep | a handoff row to a thread id, or to the desk from #1610 |
| `land` | delete | #1610: the handoff pick goes; the desk takes a handoff and [move] re-targets it |
| `new`, `new_with`, `resent`, `make`, `assign`, `title_ok`, `retitle`, `archive` | keep | opening, titling and archiving drover threads; D3's desk uses `make` |
| `files_ok`, `upload`, `dir`, `inside`, `file`, `copy_in` | keep | uploads to drover threads |
| `reply` | keep | an agent's reply in a drover thread |
| `deliver`, `once`, `START_SECS`, `acp`, `start`, `started`, `uuid`, `close`, `end`, `stop`, `hung` | keep | the engine for drover threads; D2's guard in `deliver` skips a cooee chat (#1598) |
| `sleep_idle`, `signed_out`, `silence`, `held`, `pending`, `human`, `since` | keep | idle sleep, the sign-in line, the rate-limit hold, the no-reply line, for drover threads |
| `defaults`, `on`, `thread_role`, `harness_models`, `pick`, `picked`, `chat` | keep | model, effort and harness pick and the `[chat]` defaults, for drover threads |
| `LATER`, `deferred` | keep | a delivery after the request, for drover threads |
| `link_of`, `attached`, `hold`, `attach`, `detach` | keep | cooee's attach: how a cooee chat gets its thread id; D2 lets it take `all` and `none` |
| `is_id` | keep | a drover thread id |

## `crates/server/src/serve/chat.rs` (1,326 lines)

| Functions | Verdict | Where, or why |
|---|---|---|
| `owner_card`, `owner_cards`, `msg_card`, `msg_body`, `ask_body`, `config_body`, `secret_body`, `run_body`, `run_card`, `options`, `note`, `card`, `act_button`, `ask`, `change`, `link` | keep | the builders of a row's block, drawn by cooee and by drover's page; D3 and #1610 add the [chat] and [move] buttons here |
| `open_ask`, `choosing`, `waiting`, `or`, `files_of`, `ext`, `state` | keep | drover's thread screens |
| `page`, `older`, `thread`, `row`, `tab`, `chat_tab`, `figure`, `row_block`, `list_screen`, `thread_screen`, `composer`, `threads_screen`, `mark_read` | keep | drover's own screens and `chat` tab |
| `post`, `qid`, `qfiles`, `queued`, `unqueue`, `flush`, `chip`, `chips`, `composer_chips`, `first`, `new_screen`, `file`, `stop`, `deliver` | keep | drover's composer, chips, new thread, files and stop, for drover threads |

## `crates/chat/src/threads.rs` (363 lines) and `threads/view.rs`

| Items | Verdict | Where, or why |
|---|---|---|
| everything | keep | `drover chat`'s Threads tab stays as today, for drover threads |

## Outside the three files

| Code | Verdict |
|---|---|
| `crates/core/src/webpush.rs`, `/push/*`, the VAPID key, the alert devices list, `drover push test`, `[[notify]] type = "webpush"` | keep: drover's push for its own page |
| `crates/server/src/serve/device.rs` | keep: it serves the scene to a companion device (#1497) |
| `crates/server/src/cooee.rs` | keep: `GET /cooee/profiles`, `POST /cooee/thread-started` |
| `POST /api/v1/orchestrator-attach`, `-detach`, `chat`, `chat-about` | keep; `chat-about` becomes the [chat] action in D3 |
| `drover thread new|title|reply|archive|restore|say|show|ls|handoff` | keep |
| `cooee-plugin.toml`'s `chat-cards` place | keep: cooee draws rows that name no attached chat there |

Drover loses only `land` (#1610). D2 adds a guard in `deliver`, `said` and its route. D3 adds the desk maker and the
[chat] action, and #1610 the `handoff-move` action. No line of the issue's 2.3k moves out of drover.
