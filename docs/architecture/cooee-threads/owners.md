# cooee-threads: who owns each current function (#1571)

Read on staging `fd81ef5c3`, D1 included. **keep**: stays where it is. **move**: stays in drover, in the module named.
**delete**: goes in the slice named. Nothing moves into cooee: a client brings its own chat; it takes no drover code.

## `crates/core/src/orchestrator.rs` (1,369 lines) and its two files

| Functions | Verdict | Where, or why |
|---|---|---|
| `is_role`, `by`, `acting`, `actor` | keep | who runs a command; a judge lane is `is_role` too (#1609) |
| `DEFAULT`, `name`, `label`, `id_of`, `name_of` | keep | the seat's name on labels and `drover why` |
| `role`, `models`, `set`, `save` | keep | the orchestrator role and its `[config]` sheet; the judge reads it (#1609) |
| `brief::chat`, `brief::standing` | keep | the profile's brief, for a client chat and the judge |
| `brief::thread` | delete, D4 | the brief of a drover thread |
| `chat::Seat`, `ids`, `seat`, `ChatProfile`, `hidden`, `profiles`, `profile`, `chat::started` | keep | the profiles a client loads (#1057) |
| `link_of`, `attached`, `hold`, `attach`, `detach` | keep | a client chat's attach; D2 takes `all`, `none` and adds the CLI; D3 adopts an archived id |
| `SCANS`, `WALKS`, `of`, `find`, `fleet_of` | move, D4 | `core::thread`: the registry over attached chats and old threads, for `secret`, `propose`, `lib`'s ask |
| `threads`, `title`, `archived`, `is_id` | move, D4 | `core::thread`: history reads (old ids, titles) |
| `route`, `lane_ask`, `route_row` (in `thread/route.rs`) | delete, D3 | a lane ask goes to the judge, not to a thread |
| `routed_to`, `to_owner`, `flow`, `tell` (in `thread/route.rs`) | keep | rows; `to_owner` takes no thread (#1609), `tell` only logs (D2) |
| `lane_of` | keep | a row's lane |
| `handoff`, `send`, `forwards`, `forwarded`, `handoff_text` | keep | a handoff row to a chat or to `desk` (#1610) |
| `land` | delete, #1610 | the handoff pick goes; [move] re-targets |
| `new`, `new_with`, `resent`, `make`, `assign`, `title_ok`, `retitle`, `archive` | delete, D4 | opening, titling and archiving drover threads; D3 refuses `new` first |
| `files_ok`, `upload`, `dir`, `inside`, `file`, `copy_in` | delete, D4 | thread uploads; a client keeps its own (`hygiene`'s scan of `dir` goes too) |
| `reply` | delete, D4 | an agent's reply in a drover thread; `triage`'s summary becomes a `system` row to `desk` |
| `say`, `post` | delete, D4 | the owner's message into a drover thread; `answer`'s `DECISION` line becomes the decision row's `prompt` |
| `items`, `render`, `link`, `text_ok`, `log` | delete, D4 | a drover thread's conversation; `lesson`'s use of `link` reads the row instead; `tell`'s `log` becomes `emit` |
| `unread`, `mark_read`, `show`, `ls` | delete, D4 | `thread show|ls` and the read mark |
| `deliver`, `once`, `START_SECS`, `acp`, `start`, `started`, `uuid`, `close`, `end`, `stop`, `hung` | delete, D4 | the engine; D2 guards `deliver` first (#1598) |
| `sleep_idle`, `signed_out`, `silence`, `held`, `pending`, `human`, `since` | delete, D4 | idle sleep, sign-in line, rate-limit hold, no-reply line; `check` drops its calls |
| `defaults`, `on`, `thread_role`, `harness_models`, `pick`, `picked`, `chat` | delete, D4 | a thread's model, effort and harness pick and `[chat]`; a client picks its own |
| `LATER`, `deferred` | delete, D4 | deliveries after a request; `v1` and `jobs` drop the wrapper |

About 900 of the file's lines go in D4.

## `crates/server/src/serve/chat.rs` (1,326 lines)

| Functions | Verdict | Where, or why |
|---|---|---|
| `owner_card`, the two `pub` card fns, `msg_card`, `ask_body`, `config_body`, `secret_body`, `run_body`, `run_card`, `options`, `note`, `card`, `act_button`, `ask`, `change`, `link`, `open_ask`, `choosing` | move, D4 | `serve/cards.rs`: a row's block, drawn by the board, the page and a client; `options` puts the recommended one first and `note` is the note field |
| `msg_body`, `or`, `files_of`, `ext`, `state`, `waiting` | delete, D4 | a thread message's body and its files |
| `page`, `older`, `thread`, `row`, `tab`, `figure`, `row_block`, `list_screen`, `thread_screen`, `composer`, `composer_chips` | delete, D4 | the page's thread list, thread screen and `chat` tab |
| `post`, `qid`, `qfiles`, `queued`, `unqueue`, `flush`, `chip`, `chips`, `first`, `new_screen`, `file`, `stop`, `deliver` | delete, D4 | the composer, its queue, chips, [New], files and stop |

About 350 lines move and 950 go.

## `crates/chat/src/threads.rs` (363 lines) and `threads/view.rs` (500)

| Items | Verdict | Where, or why |
|---|---|---|
| everything | delete, D4 | the Threads tab; the Board tab already answers asks and cards (`board/cards.rs`, `board/forms.rs`) |
| `lib.rs`'s `Open::Threads`, `Tab`, the tab switch | delete, D4 | one tab is left; `drover board` runs it, and `drover chat` goes |

## Outside the three files

| Code | Verdict |
|---|---|
| `drover thread new|title|reply|archive|restore|say|show|ls` | delete, D4 (refused `THREADS_CUT` from D3) |
| `drover thread handoff` | keep: a handoff row (#1610) |
| `drover chat` | delete, D4: `drover board` is the TUI |
| `POST /api/v1/orchestrator-attach`, `-detach` | keep |
| `GET` and `POST /api/v1/chat`, the `chat-about` op | delete, D4 |
| `crates/core/src/webpush.rs`, `/push/*`, the VAPID key, the subscriptions, `drover push test`, `[[notify]] type = "webpush"` | move to drover-server, D6: the page's own push, sent by serve from the events it follows; its thread-title lookup (`find`, `title`) reads the row's text instead (D4) |
| `crates/core/src/devices.rs`: pairing, the devices list, cookie `check` and `find`, `denied`, `cooee_token` | move to drover-server, D6; `drover pair` dispatches there from the cli crate |
| `crates/core/src/devices.rs`: `sha256`, `random`, `write0600`, `tmp_name`, `save` | keep in core, D6 moves them to a core file helper module: core's callers (events, config, card, propose, proxy, drive, usage, transcript) are not push |
| `crates/core/src/events.rs` `[[notify]]` | keep: core sends the generic command hook only; from D6 it skips a `webpush` notifier, which serve sends |
| `crates/core/src/check.rs` `alerts-off`, `doctor.rs` and `timer.rs` subscription lines | move to drover-server, D6, with the subscriptions they read |
| `crates/server/src/serve/device.rs` | keep in drover-server: it serves the scene to a companion device (#1497), and D6 puts it beside the pairing it checks |
| `crates/server/src/cooee.rs` | keep: `GET /cooee/profiles`, `POST /cooee/thread-started`, listed in the manifest (D5) |
| `cooee-plugin.toml` | keep, generated from the manifest (D5) |
| `roles/orchestrator/role.md`, `chat.md` | keep: the rulebook; #1609 adds `judge.md` |
| `roles/orchestrator/thread.md` | delete, D4 |
| `[limits]` keys of the engine (idle sleep, hold, silence) | delete, D4; #1609 adds `judge_minutes`, `judge_max` |
